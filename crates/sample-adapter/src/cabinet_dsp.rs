//! BEffectCabinet scalar paths recovered from Kontakt 8 ARM64.
//!
//! `SetCabinet` builds 23 fixed second-order sections from a measured cabinet
//! table plus one smoothed tone section. `ProcessInt` runs a two-lane (even /
//! odd) 24-section cascade with a one-sample pipeline delay, then a two-line
//! reflection block. Coefficients and the process structure are recovered from
//! `0x1005c35e8` / `0x1005c3b58`; the delay-smoothing poles are provisional
//! because their native initializer lives in non-file-backed `__common` BSS.
//! See `tests/fixtures/cabinet_reference.py` and its provenance JSON.
use nk_format::fx::CabinetParams;

const TABLES: &[u8; 8_140] = include_bytes!("cabinet_tables.bin");
const SECTIONS: usize = 24;
const ROW_FLOATS: usize = 185;
const SECTION_FLOATS: usize = 8;

/// Provisional delay-smoothing constants (native value is BSS-initialized).
const DELAY_KA: f32 = 600.6;
const DELAY_KB: f32 = 53.8;

const EVEN_LANE: [usize; 12] = [22, 20, 18, 16, 14, 12, 10, 8, 6, 4, 2, 0];
const ODD_LANE: [usize; 12] = [23, 21, 19, 17, 15, 13, 11, 9, 7, 5, 3, 1];

fn row_float(row: usize, index: usize) -> f32 {
    let start = (row * ROW_FLOATS + index) * 4;
    f32::from_le_bytes(TABLES[start..start + 4].try_into().unwrap())
}

fn exp_map(db: f32) -> f32 {
    let x = db * 9.965_784 + 1200.0;
    let index = x as i32;
    if index >= 2400 {
        return 20.0f64.exp2() as f32;
    }
    if index < 0 {
        return 0.0;
    }
    let value = ((index as f64 - 1200.0) / 60.0).exp2() as f32;
    let next = ((index as f64 + 1.0 - 1200.0) / 60.0).exp2() as f32;
    value + (next - value) * (x - index as f32)
}

fn complex_div(ar: f32, ai: f32, br: f32, bi: f32) -> (f32, f32) {
    let den = br * br + bi * bi;
    ((ar * br + ai * bi) / den, (ai * br - ar * bi) / den)
}

fn complex_mul(ar: f32, ai: f32, br: f32, bi: f32) -> (f32, f32) {
    (ar * br - ai * bi, ar * bi + ai * br)
}

/// Recovered `BEffectCabinet::SetCabinet` coefficient build.
/// Returns `sections[0]` = smoothed tone section, `sections[1..]` = table.
pub(crate) fn set_cabinet(
    size: f32,
    air: f32,
    treble: f32,
    bass: f32,
    cabinet: i32,
    rate: f32,
) -> ([[f32; 4]; SECTIONS], [f32; 4]) {
    let cabinet = cabinet.clamp(0, 10) as usize;
    let e_size = exp_map(size * 6.196_078_3 - 3.098_039_2);
    let e_bass = exp_map(15.0 * bass);
    let e_treble = exp_map(15.0 * treble);
    let w = e_size * rate * 0.318_309_87;

    let sqrt_bass = e_bass.sqrt();
    let sqrt_treble = e_treble.sqrt();
    let c1 = (w + sqrt_bass * -349.23) / (w - sqrt_bass * -349.23);
    let c3 = (w + -349.23 / sqrt_bass) / (w - -349.23 / sqrt_bass);
    let c2 = (w + -1975.5 / sqrt_treble) / (w - -1975.5 / sqrt_treble);
    let c4 = (w + sqrt_treble * -1975.5) / (w - sqrt_treble * -1975.5);

    let mut sections = [[0.0f32; 4]; SECTIONS];
    sections[0] = [-(c1 + c3), c1 * c3, c2 + c4, -(c2 * c4)];

    let (cab_id, _, _) = SETUPS[cabinet];
    let level = row_float(cabinet, SECTION_FLOATS * 23);
    let level = if cab_id == 8 || cab_id > 14 || cabinet > 9 {
        level
    } else {
        -level
    };
    let mut gain = if cab_id == 9 || (0x15..=0x18).contains(&cab_id) {
        -level
    } else {
        level
    };

    let threshold = w * -0.0003;
    for section in 0..23 {
        let base = section * SECTION_FLOATS;
        let s0 = row_float(cabinet, base);
        let s1 = row_float(cabinet, base + 1);
        let s2 = row_float(cabinet, base + 2);
        let s3 = row_float(cabinet, base + 3);
        let s4 = row_float(cabinet, base + 4);
        let s5 = row_float(cabinet, base + 5);
        let s6 = row_float(cabinet, base + 6);
        let s7 = row_float(cabinet, base + 7);

        let first = if s4 > 0.0 { -s4 } else { s4 };
        let mut limited = first;
        if first != 0.0 || s5 != 0.0 {
            limited = threshold;
        }
        if first <= threshold {
            limited = first;
        }

        let second = if s6 > 0.0 { -s6 } else { s6 };
        let mut limited2 = second;
        if limited != 0.0 || s5 != 0.0 {
            limited2 = threshold;
        }
        if second <= threshold {
            limited2 = second;
        }

        let d1 = complex_div(w + s0, s1, w - s0, -s1);
        let d2 = complex_div(w + limited, s5, w - limited, -s5);
        let d3 = complex_div(w + s2, s3, w - s2, -s3);
        let d4 = complex_div(w + limited2, s7, w - limited2, -s7);

        sections[section + 1] = [
            -(d1.0 + d3.0),
            complex_mul(d1.0, d1.1, d3.0, d3.1).0,
            d2.0 + d4.0,
            -complex_mul(d2.0, d2.1, d4.0, d4.1).0,
        ];

        let b1 = (w - s0, -s1);
        let b2 = (w - limited, -s5);
        let b3 = (w - s2, -s3);
        let b4 = (w - limited2, -s7);
        let m1 = complex_mul(b1.0, b1.1, b3.0, b3.1);
        let m2 = complex_mul(b2.0, b2.1, b4.0, b4.1);
        gain *= complex_div(m1.0, m1.1, m2.0, m2.1).0;
    }

    let norm = (e_treble * (w + sqrt_bass * 349.23) * (w + 1975.5 / sqrt_treble))
        / ((w + 349.23 / sqrt_bass) * (w + sqrt_treble * 1975.5));
    gain *= norm;
    let gains = [gain, gain, gain * air * 0.7, gain * air * 0.3];
    (sections, gains)
}

// s_Setups: (cabinetId, column, row), first three ints of each 0x28 entry.
const SETUPS: [(i32, i32, i32); 11] = [
    (3, 4, 0),
    (5, 4, 0),
    (6, 4, 0),
    (6, 4, 4),
    (9, 4, 0),
    (9, 4, 3),
    (13, 4, 0),
    (13, 4, 3),
    (18, 4, 0),
    (18, 4, 4),
    (8, 1, 0),
];

pub(crate) struct CabinetState {
    params: CabinetParams,
    rate: f32,
    period: usize,
    increment: f32,
    length: usize,
    mask: usize,
    head: usize,
    taps: [usize; 8],
    ca: f32,
    cb: f32,
    sections: [[f32; 4]; SECTIONS],
    target: [[f32; 4]; SECTIONS],
    increments: [f32; 4],
    gains: [f32; 4],
    target_gains: [f32; 4],
    gain_increments: [f32; 4],
    smooth_left: usize,
    s1: [[f32; 2]; SECTIONS],
    s2: [[f32; 2]; SECTIONS],
    even_prev: [f32; 2],
    l1: Vec<f32>,
    l2: Vec<f32>,
    da: [f32; 4],
    db: [f32; 4],
}

impl CabinetState {
    pub(crate) fn new(params: CabinetParams) -> Self {
        let mut state = Self {
            params,
            rate: 0.0,
            period: 0,
            increment: 0.0,
            length: 0,
            mask: 0,
            head: 0,
            taps: [0; 8],
            ca: 0.0,
            cb: 0.0,
            sections: [[0.0; 4]; SECTIONS],
            target: [[0.0; 4]; SECTIONS],
            increments: [0.0; 4],
            gains: [0.0; 4],
            target_gains: [0.0; 4],
            gain_increments: [0.0; 4],
            smooth_left: 0,
            s1: [[0.0; 2]; SECTIONS],
            s2: [[0.0; 2]; SECTIONS],
            even_prev: [0.0; 2],
            l1: Vec::new(),
            l2: Vec::new(),
            da: [0.0; 4],
            db: [0.0; 4],
        };
        state.configure(48_000.0);
        state
    }

    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid cabinet sample rate");
        }
        if self.rate != rate || self.l1.is_empty() {
            self.configure(rate);
        }
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn set_params(&mut self, params: CabinetParams) {
        self.params = params;
        if self.rate > 0.0 {
            self.apply_parameters();
        }
    }

    fn taps_for(rate: f32) -> [usize; 8] {
        let mut taps = [0usize; 8];
        taps[0] = (rate * 0.015) as usize;
        let mut value = rate * 0.015 * 1.17;
        taps[1] = value as usize;
        value *= 1.17;
        let third = value * 1.17;
        taps[2] = value as usize;
        taps[3] = third as usize;
        value = third * 1.17;
        taps[4] = value as usize;
        value *= 1.17;
        let sixth = value * 1.17;
        taps[5] = value as usize;
        taps[6] = sixth as usize;
        taps[7] = (sixth * 1.17) as usize;
        taps
    }

    fn configure(&mut self, rate: f32) {
        self.rate = rate;
        self.period = (rate * 0.05 * 0.03125 + 0.5) as usize;
        self.increment = 1.0 / (self.period as f32 * 32.0);
        let exponent = ((rate * 0.045_018_62).ln() / std::f32::consts::LN_2) as i32 + 1;
        self.length = 1usize << (exponent as u32 & 0x1f);
        self.mask = self.length - 1;
        self.taps = Self::taps_for(rate);
        let ratio = std::f32::consts::TAU / rate;
        self.ca = (DELAY_KA * ratio).min(1.0);
        self.cb = (DELAY_KB * ratio).min(1.0);
        self.l1 = vec![0.0; self.length];
        self.l2 = vec![0.0; self.length];
        self.reset();
        self.apply_parameters();
    }

    fn reset(&mut self) {
        self.head = 0;
        self.sections = [[0.0; 4]; SECTIONS];
        self.gains = [0.0; 4];
        self.increments = [0.0; 4];
        self.gain_increments = [0.0; 4];
        self.smooth_left = 0;
        self.s1 = [[0.0; 2]; SECTIONS];
        self.s2 = [[0.0; 2]; SECTIONS];
        self.even_prev = [0.0; 2];
        self.da = [0.0; 4];
        self.db = [0.0; 4];
        self.l1.fill(0.0);
        self.l2.fill(0.0);
    }

    fn apply_parameters(&mut self) {
        let p = self.params;
        let (target, gains) =
            set_cabinet(p.size, p.air, p.treble, p.bass, p.cabinet_type, self.rate);
        self.target = target;
        self.target_gains = gains;
        let mut increments = [0.0f32; 4];
        for (index, (current, target)) in self.sections[23]
            .iter()
            .zip(self.target[0].iter())
            .enumerate()
        {
            let step = (*target - *current) * self.increment;
            increments[index] = if step * step < 1e-25 { 0.0 } else { step };
        }
        let mut gain_increments = [0.0f32; 4];
        for (index, (current, target)) in
            self.gains.iter().zip(self.target_gains.iter()).enumerate()
        {
            let step = (*target - *current) * self.increment;
            gain_increments[index] = if step * step < 1e-25 { 0.0 } else { step };
        }
        self.increments = increments;
        self.gain_increments = gain_increments;
        let moving = increments.iter().any(|value| *value != 0.0)
            || gain_increments.iter().any(|value| *value != 0.0);
        self.smooth_left = if moving { self.period * 32 } else { 0 };
        if !moving {
            self.sections[23] = self.target[0];
            self.gains = self.target_gains;
        }
        for index in 0..23 {
            self.sections[index] = self.target[index + 1];
        }
    }

    fn advance(&mut self) {
        if self.smooth_left == 0 {
            return;
        }
        for index in 0..4 {
            self.sections[23][index] += self.increments[index];
            self.gains[index] += self.gain_increments[index];
        }
        self.smooth_left -= 1;
        if self.smooth_left == 0 {
            self.sections[23] = self.target[0];
            self.gains = self.target_gains;
            self.increments = [0.0; 4];
            self.gain_increments = [0.0; 4];
        }
    }

    fn section(&mut self, index: usize, input: [f32; 2]) -> [f32; 2] {
        let [p0, p1, p2, p3] = self.sections[index];
        let mut output = [0.0; 2];
        for channel in 0..2 {
            let value = input[channel];
            let y = value + self.s1[index][channel];
            self.s1[index][channel] = p0 * value + p2 * y + self.s2[index][channel];
            self.s2[index][channel] = p1 * value + p3 * y;
            output[channel] = y;
        }
        output
    }

    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        self.advance();
        let mut even = [left, right];
        for index in EVEN_LANE {
            even = self.section(index, even);
        }
        let mut odd = self.even_prev;
        for index in ODD_LANE {
            odd = self.section(index, odd);
        }
        self.even_prev = even;

        let head = self.head;
        let p = self.l1[(self.taps[0] + head) & self.mask]
            + self.l1[(self.taps[4] + head) & self.mask] * 0.533_650_1;
        let r = self.l2[(self.taps[1] + head) & self.mask]
            + self.l2[(self.taps[5] + head) & self.mask] * 0.533_650_1;
        let q = self.l1[(self.taps[2] + head) & self.mask] * -0.730_513_6
            + self.l1[(self.taps[6] + head) & self.mask] * -0.389_838_64;
        let s = self.l2[(self.taps[3] + head) & self.mask] * -0.730_513_6
            + self.l2[(self.taps[7] + head) & self.mask] * -0.389_838_64;
        let reflection = [p + q, r + s];
        let total = p + q + r + s;

        let direct = [self.gains[0] * odd[0], self.gains[1] * odd[1]];
        let output = [direct[0] + reflection[0], direct[1] + reflection[1]];

        let delta = [
            direct[0] - self.da[0],
            direct[1] - self.da[1],
            self.gains[2] * odd[0] + 0.16 * total - self.da[2],
            self.gains[3] * odd[1] + 0.16 * total - self.da[3],
        ];
        for (index, value) in delta.iter().enumerate() {
            self.da[index] += self.ca * *value;
            self.db[index] += self.cb * (*value - self.db[index]);
        }

        self.head = head.wrapping_sub(1) & self.mask;
        self.l1[self.head] = self.db[2];
        self.l2[self.head] = self.db[3];
        output
    }
}
