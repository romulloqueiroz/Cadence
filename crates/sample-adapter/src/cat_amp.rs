//! Recovered CAT numerical circuit, Initialize/SetCat/Reset and 32-frame
//! control clock (Kontakt ARM64 0x1005e6140..0x1005e752c).
use crate::amp_dsp::{exp_index, TableBase};
use nk_format::fx::AmpSettings;
use std::sync::OnceLock;
const CONSTANTS: &[u8; 160] = include_bytes!("cat_constants.bin");
fn table() -> &'static TableBase {
    static TABLE: OnceLock<TableBase> = OnceLock::new();
    TABLE.get_or_init(|| {
        TableBase::setup_with(
            include_bytes!("cat_curves.bin"),
            0,
            0.5,
            f32::from_bits(0x3e7da357),
            0.0,
            true,
            true,
        )
    })
}
fn e(x: f32) -> f32 {
    exp_index(x)
}
fn hz(x: f32) -> f32 {
    e(x * 5.0 + 1381.8816)
}
fn db(x: f32) -> f32 {
    e(x * 9.965784 + 1200.0)
}
fn constant(n: usize) -> f32 {
    f32::from_le_bytes(CONSTANTS[n * 4..n * 4 + 4].try_into().unwrap())
}
pub(crate) struct CatAmp {
    settings: AmpSettings,
    rate: f32,
    s: [f32; 0x360 / 4],
    history: [[f32; 36]; 2],
    current: [f32; 4],
    target: [f32; 4],
    delta: [f32; 4],
    dirty: bool,
    smooth_left: i32,
    slice_left: usize,
    mono: bool,
}
impl CatAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            rate: self.rate,
            s: self.s,
            history: self.history,
            current: self.current,
            target: self.target,
            delta: self.delta,
            dirty: self.dirty,
            smooth_left: self.smooth_left,
            slice_left: self.slice_left,
            mono: self.mono,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.rate = template.rate;
        self.s = template.s;
        self.history = template.history;
        self.current = template.current;
        self.target = template.target;
        self.delta = template.delta;
        self.dirty = template.dirty;
        self.smooth_left = template.smooth_left;
        self.slice_left = template.slice_left;
        self.mono = template.mono;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        let mut amp = Self {
            settings,
            rate: 48000.0,
            s: [0.0; 0x360 / 4],
            history: [[0.0; 36]; 2],
            current: [0.0; 4],
            target: [0.0; 4],
            delta: [0.0; 4],
            dirty: true,
            smooth_left: 0,
            slice_left: 0,
            mono: false,
        };
        amp.prepare(48000.0).expect("valid initial CAT rate");
        amp
    }
    fn get(&self, at: usize) -> f32 {
        self.s[at / 4]
    }
    fn put(&mut self, at: usize, value: f32) {
        self.s[at / 4] = value;
    }
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        self.rate = rate;
        self.s.fill(0.0);
        for (block, at) in [
            0x290, 0x2d0, 0x2e0, 0x2f0, 0x300, 0x310, 0x320, 0x330, 0x340, 0x350,
        ]
        .into_iter()
        .enumerate()
        {
            for lane in 0..4 {
                self.put(at + lane * 4, constant(block * 4 + lane));
            }
        }
        for lane in 0..4 {
            self.put(0x2a0 + lane * 4, table().fraction);
            self.put(0x270 + lane * 4, 255.0 - table().anchor as f32);
            self.put(0x280 + lane * 4, -(table().anchor as f32));
        }
        self.put(0x1c4, f32::from_bits(0x40c9_0e56) / rate);
        self.put(0x2b8, (self.get(0x1c4) * e(1371.8816)).min(1.0));
        self.put(0x2bc, (self.get(0x1c4) * e(1381.8816)).min(1.0));
        self.set_settings(self.settings);
        self.reset();
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        self.history = [[0.0; 36]; 2];
        self.current = self.target;
        self.delta = [0.0; 4];
        self.slice_left = 0;
        self.dirty = true;
        self.smooth_left = 0;
    }
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        self.settings = settings;
        let values = settings.values();
        let v = |i: usize| values.get(i).copied().unwrap_or(0.5).clamp(0.0, 1.0);
        self.mono = settings.flags().first().copied().unwrap_or(false);
        let scale = self.get(0x1c4);
        let drive = v(1).sqrt();
        let bend = (drive * 1.08 - 0.43).max(0.0);
        self.put(0x1c8, drive);
        self.put(0x1ec, v(1));
        self.put(0x2d4, bend * -2.0 + 2.0);
        let cap = (bend + 1.3075) * bend * 0.293 + 0.7352;
        self.put(0x1d0, cap);
        self.put(0x1d4, (db(v(0) * 40.0 - 40.0) - 0.01) * 1.010101 * 0.451849);
        self.put(0x1e4, v(2));
        self.put(0x2c8, (scale * hz(v(2) * -60.0 + 135.0)).min(1.0));
        self.put(0x1dc, v(3));
        let bass = (1.0 - v(3)) * 4.93;
        self.put(0x2b4, (scale * hz(bass * bass * bass - 5.0)).min(1.0));
        self.put(0x1e0, v(4));
        let tone = v(4) * 80.0 - 40.0;
        self.put(0x1d8, db(tone) - 1.0);
        self.put(0x2cc, (scale * hz(43.0 - tone.max(0.0))).min(1.0));
        self.put(0x1f0, v(5));
        self.put(0x2c0, (scale * hz(v(5) * 60.0 + 70.0)).min(1.2039047));
        self.put(0x1e8, v(6));
        self.put(0x1cc, v(6) * 24.0 + 128.0);
        self.put(0x2c4, (scale * hz(self.get(0x1cc) - drive * 67.0)).min(cap));
        self.target = [
            table().scale,
            table().scale * self.get(0x2d4) * (db(drive * 60.0) - 1.0),
            self.get(0x1d4),
            self.get(0x1d8) * self.get(0x1d4),
        ];
        self.dirty = true;
    }
    fn control(&mut self) {
        if self.dirty {
            self.dirty = false;
            let n = (self.rate * 4.0 * 0.02 * 0.0078125 + 0.5) as i32;
            self.smooth_left = n.max(1);
            let scale = 1.0 / (self.smooth_left * 128) as f32;
            for i in 0..4 {
                self.delta[i] = (self.target[i] - self.current[i]) * scale;
                if self.delta[i] * self.delta[i] < 1e-15 {
                    self.delta[i] = 0.0;
                    self.current[i] = self.target[i];
                }
            }
        } else {
            self.smooth_left -= 1;
            if self.smooth_left == 0 {
                self.current = self.target;
                self.delta = [0.0; 4];
            }
        }
    }
    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        if self.slice_left == 0 {
            self.control();
            self.slice_left = 32;
        }
        // The original advances the common smoother only on the final channel.
        for i in 0..4 {
            self.current[i] += self.delta[i];
        }
        let out = if self.mono {
            let y = self.channel(0, left.mul_add(0.5, right * 0.5));
            [y, y]
        } else {
            [self.channel(0, left), self.channel(1, right)]
        };
        self.slice_left -= 1;
        out
    }
    fn channel(&mut self, ch: usize, input: f32) -> f32 {
        let mut h = self.history[ch];
        let mut x = [input + input, h[5], h[6], h[7]];
        let mut d = [0.0; 4];
        for i in 0..4 {
            d[i] = x[i] - h[8 + i];
            h[8 + i] += d[i] * self.get(0x2b0 + i * 4);
        }
        let mut b = [0.0; 4];
        let mut f = [0.0; 4];
        for i in 0..4 {
            let feedback = if i < 2 { h[16 + i] } else { 0.0 };
            b[i] = h[12 + i]
                + self.get(0x2c0 + i * 4)
                    * ((if i == 3 { d[2] } else { d[i] })
                        - (h[12 + i] * self.get(0x2d0 + i * 4) + feedback));
            f[i] = h[16 + i] + self.get(0x2c0 + i * 4) * b[i];
        }
        let old = h[1];
        x[2] = old;
        let mut c = [0.0; 4];
        for i in 0..4 {
            c[i] = x[i] * (if i == 2 { old } else { self.get(0x290 + i * 4) });
        }
        let a = [d[1], b[1], b[2], b[3]];
        let mut smooth = [0.0; 4];
        for i in 0..4 {
            smooth[i] = self.current[i] * a[i];
        }
        let mixed = [
            c[0] + f[0],
            c[2] + f[2],
            smooth[0] + smooth[1],
            smooth[2] + smooth[3],
        ];
        h[0] = mixed[2];
        let mut shaped = [0.0; 4];
        for (row, shaped_value) in shaped.iter_mut().enumerate() {
            let at = 0x2e0 + row * 16;
            let a = h[0] * self.get(at) + h[2] * self.get(at + 8);
            let z = h[1] * self.get(at + 4) + h[3] * self.get(at + 12);
            let value = self.get(0x2a0 + row * 4) + (a + z);
            let idx = (value
                .max(self.get(0x280 + row * 4))
                .min(self.get(0x270 + row * 4))
                - self.get(0x280 + row * 4)) as usize;
            let ent = table().entries[idx.min(255)];
            *shaped_value = ent[0] + ent[1] * (value - (self.get(0x280 + row * 4) + idx as f32));
        }
        let mut fir = [0.0; 4];
        for i in 0..4 {
            fir[i] = ((shaped[i] * self.get(0x320 + i * 4) + h[24 + i] * self.get(0x330 + i * 4))
                + h[28 + i] * self.get(0x340 + i * 4))
                + h[32 + i] * self.get(0x350 + i * 4);
        }
        let decimated = (fir[0] + fir[2]) + (fir[1] + fir[3]);
        h[32..36].copy_from_slice(&self.history[ch][28..32]);
        h[28..32].copy_from_slice(&self.history[ch][24..28]);
        h[24..28].copy_from_slice(&shaped);
        h[20..24].copy_from_slice(&shaped);
        h[3] = h[2];
        h[2] = h[1];
        h[1] = h[0];
        h[4] = mixed[0];
        h[5] = mixed[0];
        h[6] = decimated;
        h[7] = old * old;
        h[12..16].copy_from_slice(&b);
        h[16..20].copy_from_slice(&f);
        self.history[ch] = h;
        mixed[3]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;
    fn settings(values: [f32; 7], mono: bool) -> AmpSettings {
        let mut s = AmpSettings {
            kind: AmpKind::Cat,
            values: [0.0; 17],
            value_count: 7,
            flags: [false; 8],
            flag_count: 1,
            modes: [0; 4],
        };
        s.values[..7].copy_from_slice(&values);
        s.flags[0] = mono;
        s
    }
    #[test]
    fn cat_native_control_snapshots() {
        let fixture = include_bytes!("../tests/fixtures/cat-controls.bin");
        let mut cursor = 0;
        for rate in [32000., 44100., 48000., 96000., 192000.] {
            for values in [
                [0.; 7],
                [0.5; 7],
                [1.; 7],
                [0.13, 0.78, 0.23, 0.91, 0.66, 0.37, 0.49],
            ] {
                for mono in [false, true] {
                    let mut amp = CatAmp::new(settings(values, mono));
                    amp.prepare(rate).unwrap();
                    let mut expected = |actual: f32, at: usize| {
                        let want =
                            f32::from_le_bytes(fixture[cursor..cursor + 4].try_into().unwrap());
                        cursor += 4;
                        assert!(
                            (actual - want).abs() <= 2e-6 * want.abs().max(1.0),
                            "rate {rate} values {values:?} offset {at:x}: {actual} vs {want}"
                        );
                    };
                    for at in (0x1c4..0x240).step_by(4) {
                        let actual = if (0x210..0x220).contains(&at) {
                            amp.current[(at - 0x210) / 4]
                        } else if (0x230..0x240).contains(&at) {
                            amp.target[(at - 0x230) / 4]
                        } else {
                            amp.get(at)
                        };
                        expected(actual, at);
                    }
                    // Count and reciprocal are integers/floats in the native smoothing record.
                    let n = ((rate * 4.0 * 0.02 * 0.0078125 + 0.5) as i32).max(1);
                    expected(f32::from_bits(n as u32), 0x248);
                    expected(1.0 / (n * 128) as f32, 0x24c);
                    for at in (0x270..0x360).step_by(4) {
                        expected(amp.get(at), at);
                    }
                }
            }
        }
    }
    #[test]
    fn cat_native_process_frames() {
        let input = include_bytes!("../tests/fixtures/cat-input.bin");
        let fixture = include_bytes!("../tests/fixtures/cat-process.bin");
        let mut cursor = 0;
        for rate in [44100., 48000., 96000.] {
            for mono in [false, true] {
                for values in [
                    [0.5; 7],
                    [0.; 7],
                    [1.; 7],
                    [0.13, 0.78, 0.23, 0.91, 0.66, 0.37, 0.49],
                ] {
                    let mut amp = CatAmp::new(settings(values, mono));
                    amp.prepare(rate).unwrap();
                    for frame in 0..1024 {
                        let l =
                            f32::from_le_bytes(input[frame * 8..frame * 8 + 4].try_into().unwrap());
                        let r = f32::from_le_bytes(
                            input[frame * 8 + 4..frame * 8 + 8].try_into().unwrap(),
                        );
                        let out = amp.process(l, r);
                        for (c, actual) in out.into_iter().enumerate() {
                            let want =
                                f32::from_le_bytes(fixture[cursor..cursor + 4].try_into().unwrap());
                            cursor += 4;
                            assert!(
                                (actual - want).abs() < 2e-6 + 2e-6 * want.abs(),
                                "rate {rate} mono {mono} frame {frame} channel {c}: {} vs {want}",
                                actual
                            );
                        }
                    }
                }
            }
        }
    }
}
