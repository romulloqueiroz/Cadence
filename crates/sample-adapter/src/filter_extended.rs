//! Recovered higher-order legacy filters and the original ladder (ID 17).
//! Coefficients use the initialized Kontakt prototypes, not repeated RBJ bands.
use super::filter_dsp::{legacy_control_value, pitch_frequency, warp};
use nk_format::fx::{NativeFilter, NativeFilterKind};

#[derive(Clone, Copy, Default)]
pub(super) struct Coefficients {
    pub sections: [[f64; 5]; 3],
    pub output: f32,
}

pub(super) fn supports(kind: NativeFilterKind) -> bool {
    use NativeFilterKind::*;
    matches!(
        kind,
        LegacyBandpass2
            | LegacyLowpass4
            | LegacyHighpass4
            | LegacyBandpass4
            | LegacyBandreject4
            | LegacyLowpass6
            | LegacyLadder
    )
}

// Native prototype layout is [b1,b2,a1,a2,b0]. Its denominator is
// 1-a1*z^-1-a2*z^-2 before the frequency substitution.
const FOUR: [[u32; 5]; 2] = [
    [0x3e846b75, 0x3e046b75, 0x3f1ba37a, 0xbe003bba, 0x3e046b75],
    [0x3eb208f2, 0x3e3208f2, 0x3f514087, 0xbf034979, 0x3e3208f2],
];
const SIX: [[u32; 5]; 3] = [
    [0x3e81d5d5, 0x3e01d5d5, 0x3f1899d8, 0xbdd37d69, 0x3e01d5d5],
    [0x3e939041, 0x3e139041, 0x3f2d700b, 0xbe820097, 0x3e139041],
    [0x3ec145cb, 0x3e4145cb, 0x3f63295c, 0xbf246f26, 0x3e4145cb],
];
const FOUR_FACTORS: [[f32; 2]; 2] = [[0.647, 6.294], [0.3, 0.796]];
const SIX_FACTORS: [[f32; 2]; 3] = [[0.771, 7.786], [0.572, 2.388], [0.149, 0.473]];
const REJECT: [[u64; 5]; 2] = [
    [
        0x3fd08d6e92da2f77,
        0x3fc08d6e92da2f77,
        0x3fe3746f3973960a,
        0xbfc0077731371601,
        0x3fc08d6e92da2f77,
    ],
    [
        0x3fd6411e3c874553,
        0x3fc6411e3c874553,
        0x3fea2810de924e77,
        0xbfe0692f1b1993c9,
        0x3fc6411e3c874553,
    ],
];

#[derive(Clone, Copy)]
struct Section {
    numerator: [f32; 3],
    feedback: [f32; 2],
}
impl Section {
    fn coefficients(self, high: bool, gain: f32) -> [f64; 5] {
        [
            (self.numerator[0] * gain) as f64,
            (self.numerator[1] * gain * if high { -1.0 } else { 1.0 }) as f64,
            (self.numerator[2] * gain) as f64,
            (self.feedback[0] * if high { 1.0 } else { -1.0 }) as f64,
            -self.feedback[1] as f64,
        ]
    }
    fn dc_numerator(self) -> f32 {
        self.numerator[0] + self.numerator[1] + self.numerator[2]
    }
}

fn section(prototype: [u32; 5], factors: [f32; 2], r: f32, w: f32) -> Section {
    let [b1, b2, a1, a2, b0] = prototype.map(f32::from_bits);
    let a1 = (r * factors[0] + 1.0) * a1;
    let a2 = (r * factors[1] + 1.0) * a2;
    let w2 = w * w;
    let norm = 1.0 / ((w * a1 + 1.0) - w2 * a2);
    Section {
        numerator: [
            norm * ((b0 - w * b1) + w2 * b2),
            (b1 + w * b0 * -2.0 + w2 * b1 + w * b2 * -2.0) * norm,
            (-(b1 * w) + w2 * b0 + b2) * norm,
        ],
        feedback: [
            (a1 + w * 2.0 + w2 * a1 + w * a2 * -2.0) * norm,
            ((-(a1 * w) - w2) + a2) * norm,
        ],
    }
}

// HP4 evaluates its transforms in double, rounds each expanded term to float,
// multiplies by the double normalizer, then stores float coefficients.
fn section_mixed(prototype: [u32; 5], factors: [f32; 2], r: f32, w: f32) -> Section {
    let [b1, b2, p1, p2, b0] = prototype.map(|x| f32::from_bits(x) as f64);
    let a1 = ((r * factors[0]) as f64 + 1.0) * p1;
    let a2 = ((r * factors[1]) as f64 + 1.0) * p2;
    let w = w as f64;
    let w2 = w * w;
    let norm = 1.0 / ((1.0 + a1 * w) - a2 * w2);
    let store = |x: f64| (norm * (x as f32) as f64) as f32;
    Section {
        numerator: [
            store((b0 - b1 * w) + b2 * w2),
            store(b1 + b0 * -2.0 * w + b1 * w2 + b2 * -2.0 * w),
            store(-b1 * w + b0 * w2 + b2),
        ],
        feedback: [
            store(a1 + 2.0 * w + a1 * w2 + a2 * -2.0 * w),
            store((-w2 - a1 * w) + a2),
        ],
    }
}

fn reject_section(
    prototype: [u64; 5],
    factors: [f32; 2],
    r: f32,
    w: f32,
    high: bool,
    level: f64,
) -> [f64; 5] {
    let [b1, b2, p1, p2, b0] = prototype.map(f64::from_bits);
    let a1 = ((r * factors[0]) as f64 + 1.0) * p1;
    let a2 = ((r * factors[1]) as f64 + 1.0) * p2;
    let w = w as f64;
    let w2 = w * w;
    let norm = 1.0 / ((w * a1 + 1.0) - w2 * a2);
    let fb1 = (a1 + w * 2.0 + w2 * a1 + w * a2 * -2.0) * norm;
    let fb2 = ((-(a1 * w) - w2) + a2) * norm;
    let n0 = ((b0 - w * b1) + w2 * b2) * norm;
    let n1 = (b1 + w * b0 * -2.0 + w2 * b1 + w * b2 * -2.0) * norm;
    let n2 = (b2 + -(b1 * w) + w2 * b0) * norm;
    let gain = ((1.0 - (fb1 + fb2)) / (n2 + n0 + n1)) * level;
    [
        n0 * gain,
        n1 * gain * if high { -1.0 } else { 1.0 },
        n2 * gain,
        fb1 * if high { 1.0 } else { -1.0 },
        -fb2,
    ]
}

pub(super) fn prepare(filter: NativeFilter, cutoff: f32, octaves: f32, rate: f32) -> Coefficients {
    use NativeFilterKind::*;
    let mut result = Coefficients {
        output: 1.0,
        ..Default::default()
    };
    if filter.kind == LegacyLadder {
        let midi = (cutoff * 100.0 + 20.0 + octaves.clamp(-16.0, 16.0) * 12.0).clamp(20.0, 120.0);
        let hz = pitch_frequency(midi);
        // The recovered constant is deliberately 6.283f, not full-precision TAU.
        let native_tau = f32::from_bits(0x40c90e56);
        result.sections[0][0] = ((hz * (native_tau / rate)).clamp(0.0, 1.57) * 0.5) as f64;
        result.sections[0][1] = (filter.resonance * 4.0) as f64;
        return result;
    }
    let control = legacy_control_value(cutoff, octaves, rate);
    if filter.kind == LegacyBandpass2 {
        let p = f32::from_bits(0x3e963993) as f64;
        let low = warp(control, 0.82, false) as f64;
        let high = warp(control, 0.82, true) as f64 * 0.99;
        for (i, w) in [low, high].into_iter().enumerate() {
            let feedback = ((w + p) / (1.0 + w * p)) as f32;
            let b = (1.0 - feedback) * 0.5;
            result.sections[i] = [
                b as f64,
                (b * if i == 1 { -1.0 } else { 1.0 }) as f64,
                0.0,
                (feedback * if i == 1 { 1.0 } else { -1.0 }) as f64,
                0.0,
            ];
        }
        return result;
    }
    let r = 1.0 - (1.0 - filter.resonance) * (1.0 - filter.resonance);
    if filter.kind == LegacyBandreject4 {
        let level = ((1.0 - r) * 0.19999999 + 0.8) as f64;
        result.sections[0] = reject_section(
            REJECT[0],
            FOUR_FACTORS[0],
            r,
            warp(control, 0.82, false),
            false,
            level,
        );
        result.sections[1] = reject_section(
            REJECT[1],
            FOUR_FACTORS[1],
            r,
            warp(control, 0.82, true),
            true,
            level,
        );
        return result;
    }
    result.output = (1.0 - r * r) * 0.25 + 0.75;
    if filter.kind == LegacyBandpass4 {
        let level = ((1.0 - r * r) * 0.35000002 + 0.65) as f64;
        for i in 0..2 {
            let s = section(FOUR[i], FOUR_FACTORS[i], r, warp(control, 0.82, i == 1));
            // The native band-pass normalizes each stage separately in double.
            let gain =
                ((1.0 - (s.feedback[0] + s.feedback[1]) as f64) / s.dc_numerator() as f64) * level;
            let mut c = s.coefficients(i == 1, 1.0);
            for n in &mut c[..3] {
                *n = (*n * gain) as f32 as f64;
            }
            result.sections[i] = c;
        }
        return result;
    }
    let high = filter.kind == LegacyHighpass4;
    let w = warp(control, if high { 0.82 } else { 0.99 }, high);
    let count = if filter.kind == LegacyLowpass6 { 3 } else { 2 };
    let prototype = if count == 3 {
        SIX
    } else {
        [FOUR[0], FOUR[1], FOUR[1]]
    };
    let factors = if count == 3 {
        SIX_FACTORS
    } else {
        [FOUR_FACTORS[0], FOUR_FACTORS[1], FOUR_FACTORS[1]]
    };
    let mut stages = [section(prototype[0], factors[0], r, w); 3];
    for i in 0..count {
        stages[i] = if high {
            section_mixed(prototype[i], factors[i], r, w)
        } else {
            section(prototype[i], factors[i], r, w)
        };
    }
    let feedback_sum = if count == 3 {
        stages[0].feedback[1]
            + stages[0].feedback[0]
            + stages[1].feedback[0]
            + stages[1].feedback[1]
            + stages[2].feedback[1]
            + stages[2].feedback[0]
    } else {
        stages[0].feedback[0]
            + stages[0].feedback[1]
            + stages[1].feedback[1]
            + stages[1].feedback[0]
    };
    let first = if high { 0 } else { 1 };
    let second = 1 - first;
    let mut numerator_sum = stages[0].numerator[first]
        + stages[0].numerator[second]
        + stages[0].numerator[2]
        + stages[1].numerator[first]
        + stages[1].numerator[second]
        + stages[1].numerator[2];
    if count == 3 {
        numerator_sum = numerator_sum
            + stages[2].numerator[1]
            + stages[2].numerator[0]
            + stages[2].numerator[2];
    }
    let level = (1.0 - r * r) * if count == 3 { 0.55 } else { 0.35000002 }
        + if count == 3 { 0.45 } else { 0.65 };
    let gain = ((count as f32 - feedback_sum) / numerator_sum) * level;
    for (i, s) in stages.iter().take(count).enumerate() {
        result.sections[i] = s.coefficients(high, gain);
    }
    result
}

fn compress(x: f32, slope: f32) -> f32 {
    let limited = x.clamp(-1.0, 1.0);
    limited + slope * (x - limited)
}
fn stage(x: f32, s: &mut [f32], c: &[f64; 5], slope: Option<f32>, double: bool) -> f32 {
    let y = if double {
        (c[1] * s[0] as f64 + x as f64 * c[0] + s[1] as f64 * c[2]
            - s[2] as f64 * c[3]
            - s[3] as f64 * c[4]) as f32
    } else {
        c[1] as f32 * s[0] + x * c[0] as f32 + s[1] * c[2] as f32
            - s[2] * c[3] as f32
            - s[3] * c[4] as f32
    };
    let y = if let Some(slope) = slope {
        compress(y, slope)
    } else {
        y
    };
    s[1] = s[0];
    s[0] = x;
    s[3] = s[2];
    s[2] = y;
    y
}
fn saturate(x: f32) -> f32 {
    if x.abs() <= 50.0 {
        x * (x.abs() * -0.01 + 1.0)
    } else {
        25.0f32.copysign(x)
    }
}
fn ladder_pass(input: f32, s: &mut [f32], p: f32) -> f32 {
    let a = p * 0.7692308;
    let mut d = a * input;
    let mut raw = 0.0;
    for state in &mut s[1..5] {
        raw = *state + d;
        *state = -(raw * (p - 1.0)) + d * 0.3 + 1e-12;
        d = a * saturate(raw);
    }
    raw
}
pub(super) fn step(kind: NativeFilterKind, mut x: f32, s: &mut [f32; 12], c: &Coefficients) -> f32 {
    use NativeFilterKind::*;
    if kind == LegacyLadder {
        let p = c.sections[0][0] as f32;
        let k = c.sections[0][1] as f32;
        let first = ladder_pass((x + s[0]) * 0.5 - s[5], s, p);
        let second = ladder_pass(x - (k * saturate(first) + 1e-12), s, p);
        s[5] = k * saturate(second) + 1e-12;
        s[0] = x;
        return s[4];
    }
    if kind == LegacyBandreject4 {
        let low = stage(x, &mut s[..4], &c.sections[0], None, true);
        let high = stage(x, &mut s[4..8], &c.sections[1], None, true);
        return low + high;
    }
    if kind == LegacyHighpass4 {
        x = compress(x, 0.6);
    }
    let count = if kind == LegacyLowpass6 { 3 } else { 2 };
    for i in 0..count {
        let slope = match (kind, i) {
            (LegacyLowpass4 | LegacyBandpass4, 0) => Some(0.6),
            (LegacyLowpass6, 1) => Some(0.7),
            _ => None,
        };
        x = stage(x, &mut s[i * 4..i * 4 + 4], &c.sections[i], slope, false);
    }
    x * c.output
}
