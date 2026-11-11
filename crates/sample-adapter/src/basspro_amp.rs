//! BASSPRO recovered numerical state and controls. Wire order is fixed by
//! BParFX::SetObj, and equations follow 0x1005e09bc..0x1005e4a30.
use crate::amp_dsp::TableBase;
use nk_format::fx::AmpSettings;
use std::sync::OnceLock;
const INITIAL: &[u8; 0x6b0] = include_bytes!("basspro_initial.bin");
fn tables() -> &'static [TableBase; 3] {
    static T: OnceLock<[TableBase; 3]> = OnceLock::new();
    T.get_or_init(|| {
        let setups = [
            (0x3f4dd2f2, 0x420f8f5c, 0x3c75c28f),
            (0x3f13f7cf, 0x42091eb8, 0x3c21c658),
            (0x3f6d0e56, 0x4218147b, 0x3c2a5d56),
        ];
        std::array::from_fn(|i| {
            let (a, b, c) = setups[i];
            TableBase::setup_with(
                include_bytes!("basspro_curves.bin"),
                i,
                f32::from_bits(a),
                f32::from_bits(b),
                f32::from_bits(c),
                true,
                true,
            )
        })
    })
}
fn exp_index(x: f32) -> f32 {
    let i = x as i32;
    if i < 0 {
        return 0.;
    }
    if i >= 2400 {
        return 1_048_576.;
    }
    let low = ((f64::from(i) - 1200.) / 60.).exp2() as f32;
    let high = ((f64::from(i) - 1199.) / 60.).exp2() as f32;
    (high - low).mul_add(x - i as f32, low)
}
fn db(x: f32) -> f32 {
    exp_index(x.mul_add(9.965784, 1200.0))
}
fn hz(x: f32) -> f32 {
    exp_index(x.mul_add(5.0, 1381.8816))
}
fn fixed(at: usize, fraction: f32) -> f32 {
    let index = (at - 0x9dc2200) / 8;
    exp_index(index as f32 + fraction)
}
fn band_width(width: f32, gain: f32) -> f32 {
    let e = f64::from(exp_index((width * 0.5).mul_add(5.0, 1200.0)));
    ((e - 1.0 / e) / f64::from(gain.sqrt())) as f32
}
// ARM FRECPE followed by FRECPS and the explicit second refinement.
fn reciprocal(value: f32) -> f32 {
    let bits = value.to_bits();
    let exponent = (bits >> 23) & 255;
    if !(1..=252).contains(&exponent) {
        return 1. / value;
    }
    let mantissa = 256 + ((bits & 0x7fffff) >> 15);
    let estimate = (((1u32 << 19) / (mantissa * 2 + 1)) + 1) >> 1;
    let seed =
        f32::from_bits((bits & 0x80000000) | ((253 - exponent) << 23) | ((estimate - 256) << 15));
    let x = seed * (-value).mul_add(seed, 2.);
    (x + x) - value * (x * x)
}
pub(crate) struct BassProAmp {
    settings: AmpSettings,
    rate: f32,
    s: Box<[f32; 0x3400 / 4]>,
    mono: bool,
    graphic: bool,
    smooth_dirty: [bool; 2],
    smooth_count: [i32; 2],
    slice_left: usize,
    derived_dirty: bool,
    eq_count: usize,
    eq_target: usize,
}
impl BassProAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            rate: self.rate,
            s: Box::new(*self.s),
            mono: self.mono,
            graphic: self.graphic,
            smooth_dirty: self.smooth_dirty,
            smooth_count: self.smooth_count,
            slice_left: self.slice_left,
            derived_dirty: self.derived_dirty,
            eq_count: self.eq_count,
            eq_target: self.eq_target,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.rate = template.rate;
        self.s[..].copy_from_slice(&template.s[..]);
        self.mono = template.mono;
        self.graphic = template.graphic;
        self.smooth_dirty = template.smooth_dirty;
        self.smooth_count = template.smooth_count;
        self.slice_left = template.slice_left;
        self.derived_dirty = template.derived_dirty;
        self.eq_count = template.eq_count;
        self.eq_target = template.eq_target;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        let mut a = Self {
            settings,
            rate: 48000.,
            s: Box::new([0.; 0x3400 / 4]),
            mono: false,
            graphic: false,
            smooth_dirty: [true; 2],
            smooth_count: [0; 2],
            slice_left: 0,
            derived_dirty: true,
            eq_count: 3,
            eq_target: 1,
        };
        a.prepare(48000.).expect("valid initial BASSPRO rate");
        a
    }
    fn get(&self, at: usize) -> f32 {
        self.s[at / 4]
    }
    fn put(&mut self, at: usize, x: f32) {
        self.s[at / 4] = x;
    }
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0. {
            return Err("invalid amp sample rate");
        }
        self.rate = rate;
        let _ = tables();
        self.s.fill(0.);
        for (i, b) in INITIAL.chunks_exact(4).enumerate() {
            self.s[0x2d30 / 4 + i] = f32::from_le_bytes(b.try_into().unwrap());
        }
        let os = rate * 4.;
        let scale = (1. / os) * f32::from_bits(0x40c9_0e56);
        self.put(0x2d30, os);
        self.put(0x2d34, 1. / os);
        self.put(0x2d38, scale);
        let pi = std::f32::consts::PI / rate;
        for (i, at) in [
            0x9dc5188, 0x9dc53b8, 0x9dc5570, 0x9dc5700, 0x9dc5840, 0x9dc5a48, 0x9dc5c50, 0x9dc5e30,
            0x9dc6100,
        ]
        .into_iter()
        .enumerate()
        {
            self.put(0x319c + i * 4, pi * fixed(at, 0.8815918));
        }
        let rate_filters = [
            (0x30d0, 0x9dc4a08),
            (0x30d4, 0x9dc4a08),
            (0x30d8, 0x9dc4a08),
            (0x3148, 0x9dc5fe8),
            (0x3144, 0x9dc6060),
            (0x3150, 0x9dc4bc0),
            (0x3158, 0x9dc4bc0),
            (0x317c, 0x9dc5778),
            (0x3168, 0x9dc5688),
            (0x316c, 0x9dc4d28),
            (0x3178, 0x9dc5d68),
            (0x3174, 0x9dc5c50),
            (0x3170, 0x9dc5868),
            (0x30dc, 0x9dc4a58),
            (0x3164, 0x9dc4878),
            (0x3160, 0x9dc5638),
        ];
        for (dest, source) in rate_filters {
            self.put(dest, (scale * fixed(source, 0.8815918)).min(1.));
        }
        self.put(0x3154, 1.);
        let n = (os * 0.02 * 0.0078125 + 0.5) as i32;
        let m = (rate * 0.02 * 0.03125 + 0.5) as i32;
        self.put(0x2ebc, 1. / (n.max(1) * 128) as f32);
        self.put(0x306c, 1. / (m.max(1) * 32) as f32);
        self.set_settings(self.settings);
        self.reset();
        Ok(())
    }
    pub(crate) fn reset(&mut self) {
        self.s[0x1d0 / 4..0x14d0 / 4].fill(0.);
        for i in 0..12 {
            self.put(0x2e20 + i * 4, self.get(0x2e80 + i * 4));
            self.put(0x2e50 + i * 4, 0.);
        }
        for i in 0..28 {
            self.put(0x2f10 + i * 4, self.get(0x2ff0 + i * 4));
            self.put(0x2f80 + i * 4, 0.);
        }
        self.slice_left = 0;
        self.smooth_dirty = [true; 2];
        self.smooth_count = [0; 2];
        self.derived_dirty = true;
        self.eq_count = if self.graphic { 3 } else { 1 };
        self.eq_target = self.eq_count;
    }
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        self.settings = settings;
        let vals = settings.values();
        let v = |i: usize| vals.get(i).copied().unwrap_or(0.5).clamp(0., 1.);
        let flags = settings.flags();
        let flag = |i: usize| flags.get(i).copied().unwrap_or(false);
        let graphic = flag(3);
        if graphic && !self.graphic {
            self.eq_count = 3;
            for c in 0..2 {
                self.s[(0x2ac + c * 0x130) / 4..(0x2c4 + c * 0x130) / 4].fill(0.);
                self.s[(0x2dc + c * 0x130) / 4..(0x2f4 + c * 0x130) / 4].fill(0.);
            }
        }
        self.graphic = graphic;
        self.mono = flag(4);
        self.eq_target = if graphic { 3 } else { 1 };
        self.put(
            0x2e94,
            if flag(0) {
                f32::from_bits(0x40074367)
            } else {
                0.
            },
        );
        self.put(0x2ea0, if flag(0) { 1. } else { 0. });
        self.put(0x2d48, if flag(0) { -1.2589 } else { -1.13501 });
        self.put(
            0x2e98,
            if flag(1) {
                f32::from_bits(0x3fc63f68)
            } else {
                0.
            },
        );
        self.put(
            0x2e9c,
            if flag(2) {
                f32::from_bits(0x3f4b573f)
            } else {
                0.
            },
        );
        self.put(0x2ea4, if flag(2) { 1. } else { 0. });
        let gain = (db(v(0).mul_add(28.04, -28.04)) - 0.0396278) * 0.5855022;
        self.put(0x2d40, gain);
        self.put(0x2e80, if flag(2) { gain * 0.7943 } else { gain });
        let drive = v(5);
        self.put(0x2d74, drive);
        self.put(
            0x3140,
            (hz(drive.mul_add(-26., 135.)) * self.get(0x2d38)).min(1.),
        );
        let dg = db(drive.mul_add(2.5, -19.5));
        self.put(0x2d44, dg);
        self.put(0x2e84, dg * self.get(0x2d48));
        self.put(0x2e90, db(drive.mul_add(18.45, -3.0999985)));
        self.put(
            0x2d4c,
            ((drive * -0.469) * drive).mul_add(drive, 1.) * 0.0354813,
        );
        let bass = db(v(1).mul_add(30., -15.));
        self.put(0x2d60, bass);
        self.put(0x3054, bass.sqrt());
        let treble = db(v(4).mul_add(38., -19.));
        self.put(0x2d64, treble);
        self.put(0x3050, treble.sqrt());
        let mid = v(2).mul_add(2., -1.);
        let mid_db = mid.mul_add(13.35, (mid * -2.65) * mid);
        let width = ((mid.abs() - 1.) * -20.).mul_add(1.313253, 30.);
        self.put(0x2d68, mid_db);
        self.put(0x2d70, width);
        let mg = db(mid_db);
        self.put(0x3028, mg);
        self.put(0x2ff8, band_width(width, mg));
        self.put(0x2d6c, v(3));
        self.put(
            0x2ff4,
            hz(v(3).mul_add(48., 55.35)) * (std::f32::consts::PI / self.get(0x2d30)),
        );
        let level = (db(v(6).mul_add(31.35, -31.35)) - 0.027070733) * 0.31040284;
        self.put(0x2d50, level);
        let constants = [
            (11.3, -9.599998, 1.5479451, 28.400002),
            (10.6, -11.599998, 1.3417721, 9.),
            (10.7, -30.399998, 1.3375, 28.),
            (10.7, -23.000004, 1.3896104, 17.199997),
            (10.6, -25.900002, 1.3589743, 19.5),
            (10.3, -25.399994, 1.3205128, 16.100006),
            (10.8, -17., 1.3670886, 15.399994),
            (11.4, -12.800003, 1.443038, 14.199997),
            (12.4, -9.599998, 1.5308642, 26.799995),
        ];
        for (i, (range, a, b, c)) in constants.into_iter().enumerate() {
            let x = v(i + 7).mul_add(2., -1.);
            let bend = x * (x * 0.8).mul_add(x, 0.2);
            let band_db = bend * range;
            let width = ((bend.abs() - 1.) * a).mul_add(b, c);
            self.put(0x2d80 + i * 4, band_db);
            self.put(0x2dc8 + i * 4, width);
            let gain = if graphic { db(band_db) } else { 1. };
            self.put(0x302c + i * 4, gain);
            self.put(0x2ffc + i * 4, band_width(width, gain));
        }
        let gl = db(v(16).mul_add(-4., 22.).mul_add(v(16), -10.));
        self.put(0x2d5c, gl);
        let lev = fixed(0x9dc4bd8, 0.5209961)
            * ((self.get(0x2d7c) * level) * self.get(0x2d4c))
            * (if graphic { gl } else { 1. });
        self.put(0x2e8c, lev);
        self.smooth_dirty = [true; 2];
        self.derived_dirty = true;
    }
    fn control(&mut self) {
        for group in 0..2 {
            let (cur, delta, target, len, recip) = if group == 0 {
                (0x2e20, 0x2e50, 0x2e80, 12, 0x2ebc)
            } else {
                (0x2f10, 0x2f80, 0x2ff0, 28, 0x306c)
            };
            if self.smooth_dirty[group] {
                self.smooth_dirty[group] = false;
                let mut total = 0.;
                for i in 0..len {
                    let step = (self.get(target + i * 4) - self.get(cur + i * 4)) * self.get(recip);
                    if step * step < 1e-15 {
                        self.put(delta + i * 4, 0.);
                        self.put(cur + i * 4, self.get(target + i * 4));
                    } else {
                        self.put(delta + i * 4, step);
                        total += step * step;
                    }
                }
                self.smooth_count[group] = if total > 0. {
                    ((self.rate * 0.02 * 0.03125 + 0.5) as i32).max(1)
                } else {
                    0
                };
            } else if self.smooth_count[group] > 0 {
                if group == 1 {
                    self.derived_dirty = true;
                }
                self.smooth_count[group] -= 1;
                if self.smooth_count[group] == 0 {
                    for i in 0..len {
                        self.put(cur + i * 4, self.get(target + i * 4));
                        self.put(delta + i * 4, 0.);
                    }
                }
            }
        }
        if self.smooth_count[1] == 0 {
            self.eq_count = self.eq_target;
        }
    }
    fn derive_eq(&mut self) {
        self.put(0x3198, self.get(0x2f14));
        for i in 0..4 {
            self.put(0x33c0 + i * 4, 1.);
        }
        for group in 0..self.eq_count {
            for lane in 0..4 {
                let i = group * 4 + lane;
                let freq = self.get(0x3190 + i * 4);
                let square = freq * freq;
                let damp = self.get(0x2f10 + i * 4) * freq;
                let gd = self.get(0x2f40 + i * 4) * damp;
                let sum = square + 1.;
                let den = sum + damp;
                let den2 = sum + gd;
                // Preserve both native reciprocal refinements and their f32 order.
                let inv = reciprocal(den);
                let inv2 = reciprocal(den2);
                let common = (square - 1.) * 2.;
                self.put(0x3290 + i * 4, inv * common);
                self.put(0x32c0 + i * 4, inv2 * common);
                self.put(0x32f0 + i * 4, inv * (sum - damp));
                self.put(0x3320 + i * 4, inv2 * (sum - gd));
                self.put(0x33c0 + lane * 4, den2 * inv * self.get(0x33c0 + lane * 4));
            }
        }
        let t = self.get(0x2f70);
        let b = self.get(0x2f74);
        let t2 = t * t;
        let k = (t2 + 1.) / t2;
        let w = fixed(0x9dc5fe8, 0.8815918) * -std::f32::consts::TAU;
        let f = self.get(0x2d30) * 2.;
        let r0 = (f - std::f32::consts::TAU) / (f + std::f32::consts::TAU);
        let r1 = (f + w / k) / (f - w / k);
        let r2 = (f + w) / (f - w);
        self.put(0x2ee8, -(r0 + r1));
        self.put(0x2eec, r0 * r1);
        self.put(0x2ee0, -(r0 + r2));
        self.put(0x2ee4, r0 * r2);
        self.put(
            0x2d58,
            (k * ((f + std::f32::consts::TAU) * (f - w / k)))
                / ((f + std::f32::consts::TAU) * (f - w)),
        );
        let wb = (b * -156.) * std::f32::consts::TAU;
        let wbi = (-156. / b) * std::f32::consts::TAU;
        let wt = (t * -554.) * std::f32::consts::TAU;
        let wti = (-554. / t) * std::f32::consts::TAU;
        let a = (wb + f) / (f - wb);
        let b = (wbi + f) / (f - wbi);
        let c = (wti + f) / (f - wti);
        let d = (wt + f) / (f - wt);
        self.put(0x2ed8, -(a + c));
        self.put(0x2edc, a * c);
        self.put(0x2ed0, -(b + d));
        self.put(0x2ed4, b * d);
        self.put(0x2d54, ((f - wb) * (f - wti) * t2) / ((f - wbi) * (f - wt)));
        self.put(0x3290, self.get(0x2ed0));
        self.put(0x3294, self.get(0x2ee0));
        self.put(0x32c0, self.get(0x2ed8));
        self.put(0x32c4, self.get(0x2ee8));
        self.put(0x32f0, self.get(0x2ed4));
        self.put(0x32f4, self.get(0x2ee4));
        self.put(0x3320, self.get(0x2edc));
        self.put(0x3324, self.get(0x2eec));
    }
    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        if self.slice_left == 0 {
            self.control();
            self.slice_left = 32;
        }
        for i in 0..28 {
            self.put(
                0x2f10 + i * 4,
                self.get(0x2f10 + i * 4) + self.get(0x2f80 + i * 4),
            );
        }
        if self.derived_dirty || self.smooth_count[1] > 0 {
            self.derive_eq();
            self.derived_dirty = false;
        }
        let mut smooth = [[0.; 12]; 4];
        for step in &mut smooth {
            for (i, smooth_value) in step.iter_mut().enumerate() {
                self.put(
                    0x2e20 + i * 4,
                    self.get(0x2e20 + i * 4) + self.get(0x2e50 + i * 4),
                );
                *smooth_value = self.get(0x2e20 + i * 4);
            }
        }
        let output = if self.mono {
            let x = left.mul_add(0.5, right * 0.5);
            let y = self.channel(0, x, &smooth);
            [y, y]
        } else {
            [
                self.channel(0, left, &smooth),
                self.channel(1, right, &smooth),
            ]
        };
        self.slice_left -= 1;
        output
    }
    fn channel(&mut self, ch: usize, input: f32, smooth: &[[f32; 12]; 4]) -> f32 {
        let base = 0x1d0 / 4 + ch * 0x130 / 4;
        let mut h = [0.; 76];
        h.copy_from_slice(&self.s[base..base + 76]);
        h[0] = input;
        for (step, gains) in smooth.iter().enumerate() {
            let at = 0x30c0 - step * 16;
            let mut fir = [0.; 4];
            for i in 0..4 {
                fir[i] = h[i] * self.get(at + i * 4);
            }
            let up = (fir[0] + fir[2]) + (fir[1] + fir[3]);
            let x = [
                h[(0x28c - 0x1d0) / 4],
                h[(0x280 - 0x1d0) / 4],
                up,
                h[(0x234 - 0x1d0) / 4],
            ];
            let mut d = [0.; 4];
            for i in 0..4 {
                let n = (0x220 - 0x1d0) / 4 + i;
                d[i] = x[i] - h[n];
                h[n] += self.get(0x30d0 + i * 4) * d[i];
            }
            let level = (self.get(0x2d54) * self.get(0x2d58)) * gains[3];
            let driven = [
                gains[0] * d[0],
                gains[1] * d[1],
                gains[2] * d[2],
                level * d[3],
            ];
            let mut y = [0.; 4];
            for i in 0..4 {
                let raw = (h[(0x270 - 0x1d0) / 4 + i] * self.get(0x3180 + i * 4))
                    + (self.get(0x30f0 + i * 4) + self.get(0x30e0 + i * 4) * driven[i]);
                let lower = self.get(0x3110 + i * 4);
                let upper = self.get(0x3100 + i * 4);
                let idx = (raw.max(lower).min(upper) - lower) as usize;
                let t = &tables()[[1, 2, 0, 0][i]];
                let ent = t.entries[idx.min(255)];
                y[i] = ent[0] + ent[1] * (raw - (lower + idx as f32));
            }
            let mut lp = [0.; 4];
            let mut slow = [0.; 4];
            for i in 0..4 {
                let a = (0x230 - 0x1d0) / 4 + i;
                lp[i] = h[a] + self.get(0x3140 + i * 4) * (self.get(0x3240 + i * 4) * y[i] - h[a]);
                h[a] = lp[i];
                let a = (0x240 - 0x1d0) / 4 + i;
                slow[i] = h[a] + self.get(0x3150 + i * 4) * (y[i] - h[a]);
                h[a] = slow[i];
            }
            let mut feedback = slow;
            feedback[0] = y[0] + gains[9] * (slow[0] - y[0]);
            h[(0x270 - 0x1d0) / 4..(0x280 - 0x1d0) / 4].copy_from_slice(&feedback);
            let old = h[(0x288 - 0x1d0) / 4];
            let fifth = [old, driven[3], lp[0], lp[0]];
            let mut diff = [0.; 4];
            for i in 0..4 {
                let n = (0x250 - 0x1d0) / 4 + i;
                diff[i] = fifth[i] - h[n];
                h[n] += diff[i] * self.get(0x3160 + i * 4);
            }
            let bass = old + gains[8] * (h[(0x250 - 0x1d0) / 4] - old);
            let sum = diff[3] + diff[2] * self.get(0x3088);
            let x = [h[(0x284 - 0x1d0) / 4], old, sum, lp[2]];
            let mut high = [0.; 4];
            for i in 0..4 {
                let n = (0x260 - 0x1d0) / 4 + i;
                high[i] = x[i] - h[n];
                h[n] += high[i] * self.get(0x3170 + i * 4);
            }
            let update = [
                x[0] + gains[4] * high[0],
                bass + gains[5] * high[1],
                x[2] + gains[6] * high[2],
                x[3] + gains[7] * high[3],
            ];
            h[(0x280 - 0x1d0) / 4..(0x290 - 0x1d0) / 4].copy_from_slice(&update);
            h[(0x1ec - 0x1d0) / 4 - step] = diff[1];
        }
        let mut fir = [0.; 4];
        for i in 0..4 {
            fir[i] = ((h[4 + i] * self.get(0x31c0 + i * 4) + h[8 + i] * self.get(0x31d0 + i * 4))
                + h[12 + i] * self.get(0x31e0 + i * 4))
                + h[16 + i] * self.get(0x31f0 + i * 4);
        }
        let out = (fir[0] + fir[2]) + (fir[1] + fir[3]);
        for i in (8..20).rev() {
            h[i] = h[i - 4];
        }
        h[3] = h[2];
        h[2] = h[1];
        h[1] = h[0];
        let n = (0x290 - 0x1d0) / 4;
        let mut eq = [out, h[n], h[n + 1], h[n + 2]];
        for group in 0..self.eq_count {
            for (lane, sample) in eq.iter_mut().enumerate() {
                let i = group * 4 + lane;
                let a = (0x2a0 - 0x1d0) / 4 + i;
                let b = (0x2d0 - 0x1d0) / 4 + i;
                let x = *sample;
                let y = x + h[a];
                h[a] = h[b] + (x * self.get(0x32c0 + i * 4) - y * self.get(0x3290 + i * 4));
                h[b] = x * self.get(0x3320 + i * 4) - y * self.get(0x32f0 + i * 4);
                *sample = y;
            }
        }
        for (i, value) in eq.iter_mut().enumerate() {
            *value *= self.get(0x33c0 + i * 4);
        }
        h[n..n + 4].copy_from_slice(&eq);
        self.s[base..base + 76].copy_from_slice(&h);
        eq[3]
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;
    fn settings(values: [f32; 17], flags: [bool; 5]) -> AmpSettings {
        let mut s = AmpSettings {
            kind: AmpKind::BassPro,
            values,
            value_count: 17,
            flags: [false; 8],
            flag_count: 5,
            modes: [0; 4],
        };
        s.flags[..5].copy_from_slice(&flags);
        s
    }
    #[test]
    fn basspro_native_control_snapshots() {
        let fixture = include_bytes!("../tests/fixtures/basspro-controls.bin");
        let cases = [
            ([0.; 17], [false; 5]),
            ([0.5; 17], [false; 5]),
            ([1.; 17], [true; 5]),
            (
                [
                    0.13, 0.78, 0.23, 0.91, 0.66, 0.37, 0.49, 0.22, 0.88, 0.17, 0.81, 0.31, 0.72,
                    0.36, 0.68, 0.44, 0.57,
                ],
                [true, false, true, true, false],
            ),
        ];
        let mut cursor = 0;
        for rate in [32000., 44100., 48000., 96000., 192000.] {
            for (values, flags) in cases {
                let mut amp = BassProAmp::new(settings(values, flags));
                amp.prepare(rate).unwrap();
                for at in (0x2d30..0x33e0).step_by(4) {
                    let want = f32::from_le_bytes(fixture[cursor..cursor + 4].try_into().unwrap());
                    cursor += 4;
                    if at == 0x2d3c
                        || (0x2df0..0x2e20).contains(&at)
                        || (0x2eb0..0x2ff0).contains(&at)
                        || (0x3060..0x3080).contains(&at)
                        || (0x3120..0x3140).contains(&at)
                        || at >= 0x3290
                    {
                        continue;
                    }
                    let actual = amp.get(at);
                    assert!((actual-want).abs()<=2e-6*want.abs().max(1.),"rate {rate} values {values:?} flags {flags:?} offset {at:x}: {actual} vs {want}");
                }
            }
        }
    }
    #[test]
    fn basspro_native_process_frames() {
        let input = include_bytes!("../tests/fixtures/basspro-input.bin");
        let fixture = include_bytes!("../tests/fixtures/basspro-process.bin");
        let mut cursor = 0;
        let mut worst = 0.0f32;
        for rate in [44100., 48000., 96000.] {
            for flags in [[false; 5], [true; 5], [true, false, true, true, false]] {
                for values in [
                    [0.5; 17],
                    [0.; 17],
                    [1.; 17],
                    [
                        0.13, 0.78, 0.23, 0.91, 0.66, 0.37, 0.49, 0.22, 0.88, 0.17, 0.81, 0.31,
                        0.72, 0.36, 0.68, 0.44, 0.57,
                    ],
                ] {
                    let mut amp = BassProAmp::new(settings(values, flags));
                    amp.prepare(rate).unwrap();
                    for frame in 0..1024 {
                        let l =
                            f32::from_le_bytes(input[frame * 8..frame * 8 + 4].try_into().unwrap());
                        let r = f32::from_le_bytes(
                            input[frame * 8 + 4..frame * 8 + 8].try_into().unwrap(),
                        );
                        let output = amp.process(l, r);

                        for (c, actual) in output.into_iter().enumerate() {
                            let want =
                                f32::from_le_bytes(fixture[cursor..cursor + 4].try_into().unwrap());
                            cursor += 4;
                            worst = worst.max((actual - want).abs());
                            assert!((actual-want).abs()<3e-5+2e-6*want.abs(),"rate {rate} values {values:?} flags {flags:?} frame {frame} channel {c}: {} vs {want} (worst {worst})",actual);
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn basspro_native_live_control_transitions() {
        let input = include_bytes!("../tests/fixtures/basspro-input.bin");
        let fixture = include_bytes!("../tests/fixtures/basspro-transition.bin");
        let mut amp = BassProAmp::new(settings([0.5; 17], [false; 5]));
        let stages = [
            ([0.5; 17], [false; 5]),
            ([1.; 17], [true; 5]),
            (
                [
                    0.13, 0.78, 0.23, 0.91, 0.66, 0.37, 0.49, 0.22, 0.88, 0.17, 0.81, 0.31, 0.72,
                    0.36, 0.68, 0.44, 0.57,
                ],
                [true, false, true, true, false],
            ),
            ([0.; 17], [false; 5]),
        ];
        let mut cursor = 0;
        for (stage, (values, flags)) in stages.into_iter().enumerate() {
            if stage > 0 {
                amp.set_settings(settings(values, flags));
            }
            for frame in 0..1024 {
                let l = f32::from_le_bytes(input[frame * 8..frame * 8 + 4].try_into().unwrap());
                let r = f32::from_le_bytes(input[frame * 8 + 4..frame * 8 + 8].try_into().unwrap());
                let out = amp.process(l, r);
                for (c, actual) in out.into_iter().enumerate() {
                    let want = f32::from_le_bytes(fixture[cursor..cursor + 4].try_into().unwrap());
                    cursor += 4;
                    assert!(
                        (actual - want).abs() < 3e-5 + 2e-6 * want.abs(),
                        "stage {stage} frame {frame} channel {c}: {} vs {want}",
                        actual
                    );
                }
            }
        }
    }
}
