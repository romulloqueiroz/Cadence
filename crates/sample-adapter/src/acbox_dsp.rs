//! AC30 circuit port from the original ACBox Initialize/SetSampleRate/SetACBox.
//! Offsets retain their native identity for independent snapshot validation.
#![allow(clippy::excessive_precision, clippy::approx_constant)] // Exact recovered literals.
use crate::amp_dsp::TableBase;
use nk_format::fx::AmpSettings;
use std::sync::OnceLock;
#[path = "acbox_constants.rs"]
mod constants;
#[path = "acbox_process.rs"]
mod process;
const CURVES: &[u8; 10240] = include_bytes!("acbox_curves.bin");
pub(super) fn tables() -> &'static [TableBase; 5] {
    static TABLES: OnceLock<[TableBase; 5]> = OnceLock::new();
    TABLES.get_or_init(|| {
        let args = [
            (0x3f4f5c29, 0x42813333, 0x3c38b13f),
            (0x3f0a3d71, 0x42091eb8, 0x3c16bb99),
            (0x3efae148, 0x42593333, 0),
            (0x3f000000, 0x42546666, 0),
        ];
        std::array::from_fn(|i| {
            if i == 4 {
                TableBase::manual(CURVES, 4, 0x76, 0x3c831000, 0x45d4cccd)
            } else {
                let (a, b, c) = args[i];
                TableBase::setup_with(
                    CURVES,
                    i,
                    f32::from_bits(a),
                    f32::from_bits(b),
                    f32::from_bits(c),
                    true,
                    true,
                )
            }
        })
    })
}
/// The original setters use FMADD for ExpTable interpolation.
fn exp_index(x: f32) -> f32 {
    let index = x as i32;
    if index >= 2400 {
        return 1048576.0;
    }
    if index < 0 {
        return 0.0;
    }
    let base = ((index as f64 - 1200.0) / 60.0).exp2() as f32;
    let next = ((index as f64 + 1.0 - 1200.0) / 60.0).exp2() as f32;
    (next - base).mul_add(x - index as f32, base)
}
pub(super) struct State {
    bytes: Box<[u8]>,
}
impl State {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(super) fn duplicate_prepared(&self) -> Self {
        Self {
            bytes: self.bytes.to_vec().into_boxed_slice(),
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(super) fn copy_prepared_from(&mut self, template: &Self) {
        self.bytes[..].copy_from_slice(&template.bytes[..]);
    }

    fn new() -> Self {
        Self {
            bytes: vec![0; 0x4200].into_boxed_slice(),
        }
    }
    pub(super) fn f(&self, a: usize) -> f32 {
        f32::from_bits(self.u(a))
    }
    pub(super) fn u(&self, a: usize) -> u32 {
        u32::from_le_bytes(self.bytes[a..a + 4].try_into().unwrap())
    }
    pub(super) fn put(&mut self, a: usize, v: f32) {
        self.putu(a, v.to_bits())
    }
    pub(super) fn putu(&mut self, a: usize, v: u32) {
        self.bytes[a..a + 4].copy_from_slice(&v.to_le_bytes())
    }
    pub(super) fn vec(&self, a: usize) -> [f32; 4] {
        std::array::from_fn(|i| self.f(a + 4 * i))
    }
    pub(super) fn setvec(&mut self, a: usize, v: [f32; 4]) {
        for (i, v) in v.into_iter().enumerate() {
            self.put(a + i * 4, v)
        }
    }
    fn initialize(&mut self, rate: f32) {
        self.bytes.fill(0);
        for &(a, v) in constants::INITIAL {
            self.putu(a, v)
        }
        for (i, t) in tables().iter().take(4).enumerate() {
            self.put(0x3fa0 + i * 4, t.scale);
            self.put(0x3fb0 + i * 4, t.fraction);
            self.put(0x3fc0 + i * 4, 255.0 - t.anchor as f32);
            self.put(0x3fd0 + i * 4, -(t.anchor as f32));
            self.put(0x4020 + i * 4, t.offset)
        }
        self.set_rate(rate);
        // Initialize overwrites the supply response after the zero-initialized rate call.
        let k = self.f(0x3e78);
        let inv = self.f(0x3e74);
        for (a, index, frac) in [
            (0x40e0, 1348, 0.6816406),
            (0x40e8, 1128, 0.6816406),
            (0x40ec, 1058, 0.6816406),
        ] {
            self.put(a, (exp_index(index as f32 + frac) * k).min(1.0));
        }
        // -38.32 dB, the original constructor/Initialize supply operating point.
        let e = exp_index(818.0 + 0.11114502);
        self.put(0x40f8, e * 1000.0 * inv);
        self.put(0x40fc, e * -0.4149 * 1000.0 * inv);
    }
    fn set_rate(&mut self, rate: f32) {
        let rate = rate * 4.0;
        self.put(0x3e70, rate);
        let n = ((rate * 0.02 * 0.0078125 + 0.5) as i32).max(1);
        self.putu(0x3f78, n as u32);
        self.put(0x3f7c, 1.0 / (n << 7) as f32);
        let inv = 1.0 / rate;
        let k = inv * 6.283;
        self.put(0x3e74, inv);
        self.put(0x3e78, k);
        self.put(0x4080, (exp_index(1516.0 + 0.8815918) * k).min(1.0));
        self.put(0x4084, (exp_index(1516.0 + 0.8815918) * k).min(1.0));
        self.put(0x4050, (exp_index(1741.0 + 0.8815918) * k).min(1.0));
        self.put(0x4060, (exp_index(1991.0 + 0.8815918) * k).min(1.0));
        self.put(0x4058, (exp_index(1381.0 + 0.8815918) * k).min(1.0));
        self.put(0x3f90, (exp_index(1281.0 + 0.8815918) * k).min(1.0));
        self.put(0x3f94, (exp_index(1281.0 + 0.8815918) * k).min(1.0));
        self.put(0x3f98, (exp_index(1306.0 + 0.8815918) * k).min(1.0));
        self.put(0x3f9c, (exp_index(1306.0 + 0.8815918) * k).min(1.0));
        self.put(0x4010, (exp_index(1956.0 + 0.8815918) * k).min(1.0));
        self.put(0x4014, (exp_index(2031.0 + 0.8815918) * k).min(1.0));
        self.put(0x4018, (exp_index(2036.0 + 0.8815918) * k).min(1.0));
        self.put(0x401c, (exp_index(2031.0 + 0.8815918) * k).min(1.0));
        self.put(0x4040, (exp_index(1286.0 + 0.8815918) * k).min(1.0));
        self.put(0x4044, (exp_index(1336.0 + 0.8815918) * k).min(1.0));
        self.put(0x4048, (exp_index(1751.0 + 0.8815918) * k).min(1.0));
        self.put(0x404c, (exp_index(1751.0 + 0.8815918) * k).min(1.0));
        self.put(0x4078, (exp_index(2001.0 + 0.8815918) * k).min(1.0));
        self.put(0x4070, (exp_index(1421.0 + 0.8815918) * k).min(1.0));
        self.put(0x40e4, (exp_index(1181.0 + 0.8815918) * k).min(1.0));

        let tone = self.f(0x3e84);
        for (a, pitch) in [
            (0x40e0, tone + 2.0),
            (0x40e8, tone + -42.0),
            (0x40ec, tone + -56.0),
        ] {
            self.put(a, (k * exp_index(pitch.mul_add(5.0, 1381.8816))).min(1.0));
        }
        let treble = self.f(0x3e80);
        let bass = self.f(0x3e7c);
        self.put(0x3f58, treble * self.f(0x3f44) * 4.4884);
        self.put(
            0x4054,
            (k * exp_index(treble.mul_add(-17.0, 25.0).mul_add(5.0, 1381.8816))).min(1.0),
        );
        self.put(
            0x4068,
            (k * exp_index(treble.mul_add(-24.0, 73.0).mul_add(5.0, 1381.8816))).min(1.0),
        );
        let drive = bass.mul_add(-0.02, 0.0292) * 0.562;
        self.put(0x3f44, drive);
        self.put(0x3f58, treble * drive * 4.4884);
        let cubic = bass * bass * bass;
        self.put(0x3f54, cubic * 0.08390659);
        self.bytes[0x3f70] = 1;
        self.putu(0x3f80, 1);
        self.put(
            0x4064,
            (k * exp_index(cubic.mul_add(27.7, 86.3).mul_add(5.0, 1381.8816))).min(1.0),
        );
        let db = tone.mul_add(0.5, -34.0);
        self.put(
            0x40fc,
            (exp_index(db.mul_add(9.965784, 1200.0)) * -0.4149 * 1000.0) * inv,
        );
        self.put(
            0x40f8,
            (exp_index((db + self.f(0x3e88)).mul_add(9.965784, 1200.0)) * 1000.0) * inv,
        );
        self.put(0x406c, 1.0);
        self.put(
            0x40f0,
            inv * if self.bytes[0x3e8c] != 0 {
                120.0
            } else {
                100.0
            },
        );
        self.put(0x3e94, inv * (self.f(0x3e90) * 4.0));
    }
    fn set(&mut self, settings: AmpSettings) {
        let mut v = [0.5; 7];
        for (a, b) in v.iter_mut().zip(settings.values()) {
            *a = b.clamp(0.0, 1.0)
        }
        self.bytes[0x1a2] = 1;
        self.bytes[0x1c0] = settings.flags().first().copied().unwrap_or(false) as u8;
        let gain =
            |u: f32| exp_index(u.mul_add(30.0, -30.0).mul_add(9.965784, 1200.0)) - 0.031622775;
        self.put(0x3f48, gain(v[0]) * 1.0326555);
        self.put(0x3f40, gain(v[1]) * 1.8360614);
        self.put(0x3e7c, v[3]);
        let drive = v[3].mul_add(-0.02, 0.0292) * 0.562;
        self.put(0x3f44, drive);
        self.put(0x3e80, v[2]);
        self.put(0x3f58, drive * v[2] * 4.4884);
        let cubic = v[3] * v[3] * v[3];
        self.put(
            0x3f50,
            (v[1] + -0.5).mul_add(v[1] + -0.5, -0.25) * -2.123821,
        );
        self.put(0x3f54, cubic * 0.08390659);
        let k = self.f(0x3e78);
        self.put(
            0x4064,
            (exp_index(cubic.mul_add(27.7, 86.3).mul_add(5.0, 1381.8816)) * k).min(1.0),
        );
        self.put(
            0x4054,
            (exp_index(v[2].mul_add(-17.0, 25.0).mul_add(5.0, 1381.8816)) * k).min(1.0),
        );
        self.put(
            0x4068,
            (exp_index(v[2].mul_add(-24.0, 73.0).mul_add(5.0, 1381.8816)) * k).min(1.0),
        );
        self.put(0x3f30, (1.0 - v[4]) * 0.631);
        self.put(0x3f34, (1.0 - v[4]) * 0.631);
        self.put(0x3f38, v[4] * 0.631);
        self.put(0x3f3c, v[4] * 0.631);
        self.put(0x3e90, v[5].mul_add(5.0, 2.0));
        self.put(0x3e94, self.f(0x3e90) * 4.0 * self.f(0x3e74));
        self.put(0x3f60, v[6] * 50.0);
        self.bytes[0x3f70] = 1;
        self.putu(0x3f80, 1);
    }
    fn reset(&mut self) {
        for i in 0..16 {
            self.putu(0x3eb0 + i * 4, self.u(0x3f30 + i * 4));
        }
        self.bytes[0x3f70] = 1;
        self.putu(0x3f80, 1);
        self.putu(0x1bc, 0);
        for channel in 0..16 {
            let b = channel * 0x140;
            self.bytes[b + 0x1d0..b + 0x310].fill(0);
            let voltage = self.f(0x40f4);
            let supply = voltage * 0.6461;
            self.put(b + 0x304, supply);
            self.setvec(b + 0x260, [1.0 / supply; 4]);
            self.setvec(b + 0x270, [supply; 4]);
            self.put(b + 0x2e0, 1.0);
            self.put(b + 0x2f0, voltage * 0.6319);
            let stages = [
                voltage * 0.6052,
                voltage * 0.5457,
                voltage * 0.6319,
                voltage * 0.6319,
            ];
            self.setvec(b + 0x2e8, [stages[0], stages[1], stages[2], 0.0]);
            self.setvec(b + 0x250, stages);
            self.setvec(b + 0x240, stages.map(|x| 1.0 / x));
            self.put(b + 0x2c0, voltage * 0.3554);
            self.put(b + 0x30c, 1.0);
        }
    }
    fn control(&mut self) {
        if self.bytes[0x3f70] == 0 {
            let n = self.u(0x3f74).wrapping_sub(1);
            self.putu(0x3f74, n);
            if n == 0 {
                for i in 0..16 {
                    self.putu(0x3eb0 + 4 * i, self.u(0x3f30 + 4 * i));
                    self.putu(0x3ef0 + 4 * i, 0)
                }
                self.bytes[0x3f71] = 0;
                self.putu(0x3f80, 4)
            } else if self.u(0x3f80) == 2 {
                self.putu(0x3f80, 3)
            } else if self.u(0x3f80) == 4 {
                self.putu(0x3f80, 0)
            }
            return;
        }
        self.bytes[0x3f70] = 0;
        self.putu(0x3f80, 2);
        let mut sum = 0.0;
        for i in 0..16 {
            let a = 0x3eb0 + i * 4;
            let target = self.f(0x3f30 + i * 4);
            let delta = (target - self.f(a)) * self.f(0x3f7c);
            let square = delta * delta;
            if square < 1e-15 {
                self.put(a, target);
                self.put(0x3ef0 + i * 4, 0.0)
            } else {
                self.put(0x3ef0 + i * 4, delta);
                sum += square
            }
        }
        if sum <= 0.0 {
            self.putu(0x3f80, 4)
        } else {
            self.putu(0x3f74, self.u(0x3f78));
            self.bytes[0x3f71] = 1
        }
    }
}
pub(crate) struct AcBoxAmp {
    settings: AmpSettings,
    state: State,
    process: process::Process,
    remaining: usize,
}
impl AcBoxAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            state: self.state.duplicate_prepared(),
            process: self.process.duplicate_prepared(),
            remaining: self.remaining,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.state.copy_prepared_from(&template.state);
        self.process.copy_prepared_from(&template.process);
        self.remaining = template.remaining;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        let mut a = Self {
            settings,
            state: State::new(),
            process: process::Process::default(),
            remaining: 0,
        };
        a.prepare(48000.0).unwrap();
        a
    }
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        };
        self.state.initialize(rate);
        self.set_settings(self.settings);
        self.reset();
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        self.state.reset();
        self.remaining = 0
    }
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        self.settings = settings;
        self.state.set(settings)
    }
    pub(crate) fn process(&mut self, l: f32, r: f32) -> [f32; 2] {
        if self.remaining == 0 {
            self.state.control();
            self.process.begin(&self.state);
            self.remaining = 32
        }
        let out = self.process.frame(&mut self.state, l, r);
        self.remaining -= 1;
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;
    fn settings(values: [f32; 7], mono: bool) -> AmpSettings {
        let mut s = AmpSettings {
            kind: AmpKind::AcBox,
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
    fn acbox_native_live_targets_and_rate_changes() {
        let data = include_bytes!("../tests/fixtures/acbox-live-native.bin");
        let mut at = 0;
        let f = |blob: &[u8], a: usize| f32::from_le_bytes(blob[a..a + 4].try_into().unwrap());
        for rate in [44100.0, 48000.0, 192000.0] {
            for mono in [false, true] {
                at += 0x4200;
                let input = &data[at..at + 32768];
                at += 32768;
                let expected = &data[at..at + 32768];
                at += 32768;
                let final_state = &data[at..at + 0x4200];
                at += 0x4200;
                let mut amp = AcBoxAmp::new(settings([0.2; 7], mono));
                amp.prepare(rate).unwrap();
                let mut max = 0.0f32;
                for frame in 0..4096 {
                    let values = match frame {
                        64 => Some([0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2]),
                        128 => Some([0.3, 0.2, 0.1, 0.4, 0.5, 0.6, 0.9]),
                        _ => None,
                    };
                    if let Some(values) = values {
                        let before = amp.state.bytes[0x1d0..0x15d0].to_vec();
                        amp.set_settings(settings(values, mono));
                        assert_eq!(&amp.state.bytes[0x1d0..0x15d0], before);
                    }
                    let out = amp.process(f(input, frame * 8), f(input, frame * 8 + 4));
                    for (c, &value) in out.iter().enumerate() {
                        assert!(value.is_finite());
                        max = max.max((value - f(expected, frame * 8 + c * 4)).abs())
                    }
                }
                assert!(max <= 0.0, "rate {rate} mono {mono} live max {max}");
                for a in (0x3eb0..0x3f70).step_by(4) {
                    assert_eq!(amp.state.f(a), f(final_state, a), "control {a:x} at {rate}");
                }
            }
        }
        let rates = include_bytes!("../tests/fixtures/acbox-rate-native.bin");
        for (i, rate) in [32000.0, 44100.0, 48000.0, 96000.0, 192000.0]
            .into_iter()
            .enumerate()
        {
            let mut amp = AcBoxAmp::new(settings([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7], false));
            amp.state.set_rate(rate);
            for a in (0x3e70..0x4180).step_by(4) {
                if (0x3fe0..0x4008).contains(&a) {
                    continue;
                }
                assert_eq!(
                    amp.state.u(a),
                    u32::from_le_bytes(
                        rates[i * 0x4200 + a..i * 0x4200 + a + 4]
                            .try_into()
                            .unwrap()
                    ),
                    "rate {rate} offset {a:x}"
                );
            }
        }
    }
    #[test]
    fn acbox_reset_and_invalid_rates() {
        let mut a = AcBoxAmp::new(settings([0.5; 7], false));
        for rate in [0.0, -1.0, f32::NAN, f32::INFINITY] {
            assert!(a.prepare(rate).is_err())
        }
        let first: Vec<_> = (0..2048)
            .map(|i| a.process((i as f32 * 0.09).sin() * 0.2, 0.0))
            .collect();
        a.reset();
        for (i, expected) in first.into_iter().enumerate() {
            assert_eq!(a.process((i as f32 * 0.09).sin() * 0.2, 0.0), expected)
        }
        let mut mono = AcBoxAmp::new(settings([0.5; 7], true));
        let mut silent = AcBoxAmp::new(settings([0.5; 7], true));
        for i in 0..1024 {
            let x = (i as f32 * 0.1).sin() * 0.2;
            let output = mono.process(x, -x);
            assert_eq!(output, silent.process(0.0, 0.0));
            assert_eq!(output[0], output[1]);
        }
    }
    #[test]
    fn acbox_native_initialization_and_process() {
        let data = include_bytes!("../tests/fixtures/acbox-native.bin");
        let mut at = 0;
        for rate in [32000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            for (values, mono) in [
                ([0.5; 7], false),
                ([0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7], false),
                ([1.0; 7], true),
                ([0.0; 7], false),
            ] {
                let initial = &data[at..at + 0x4200];
                at += 0x4200;
                let input = &data[at..at + 8192];
                at += 8192;
                let expected = &data[at..at + 8192];
                at += 8192;
                let final_state = &data[at..at + 0x4200];
                at += 0x4200;
                let mut amp = AcBoxAmp::new(settings(values, mono));
                amp.prepare(rate).unwrap();
                let f =
                    |blob: &[u8], a: usize| f32::from_le_bytes(blob[a..a + 4].try_into().unwrap());
                assert_eq!(
                    &amp.state.bytes[0x1d0..0x15d0],
                    &initial[0x1d0..0x15d0],
                    "native reset histories"
                );
                let mut initial_errors = vec![];
                for a in (0x3e70..0x4180).step_by(4) {
                    if (0x3fe0..0x4008).contains(&a) || a == 0x3f71 {
                        continue;
                    }
                    if amp.state.u(a) != u32::from_le_bytes(initial[a..a + 4].try_into().unwrap()) {
                        initial_errors.push((a, amp.state.f(a), f(initial, a)))
                    }
                }
                assert!(
                    initial_errors.is_empty(),
                    "rate {rate} values {values:?}: initialization {initial_errors:x?}"
                );
                let mut max = 0.0f32;
                let mut first = None;
                for frame in 0..1024 {
                    let out = amp.process(f(input, frame * 8), f(input, frame * 8 + 4));
                    for (c, &value) in out.iter().enumerate() {
                        assert!(value.is_finite());
                        let e = f(expected, frame * 8 + c * 4);
                        let err = (value - e).abs();
                        max = max.max(err);
                        if err > 1e-6 && first.is_none() {
                            first = Some((frame, c, value, e))
                        }
                    }
                }
                if max > 0.0 {
                    let differences: Vec<_> = (0x220..0x310)
                        .step_by(4)
                        .filter_map(|a| {
                            let e = f(final_state, a);
                            let v = amp.state.f(a);
                            if (e - v).abs() > 1e-5 {
                                Some((a, v, e))
                            } else {
                                None
                            }
                        })
                        .collect();
                    panic!("rate {rate} values {values:?} mono {mono} max {max} first {first:?} states {differences:x?}")
                }
            }
        }
    }
}
