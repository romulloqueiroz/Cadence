//! Recovered legacy filter and SV1 control laws; shared fixed-size voice state.
use nk_format::fx::{NativeFilter, NativeFilterKind};
use std::sync::OnceLock;

static EXP_TABLE: OnceLock<[f32; 2401]> = OnceLock::new();

/// Called only during load/prepare, never lazily by the sample processor.
pub(crate) fn initialize() {
    EXP_TABLE.get_or_init(|| std::array::from_fn(|i| (i as f64 / 60.0 - 20.0).exp2() as f32));
}

const CONTROL_FRAMES: u8 = 32;

#[derive(Clone, Copy, Default)]
pub(crate) struct NativeFilterState {
    left: [f32; 12],
    right: [f32; 12],
    coeffs: [f32; 5],
    extended: super::filter_extended::Coefficients,
    cutoff: f32,
    resonance: f32,
    octaves: f32,
    mapped_cutoff: f32,
    mapped_resonance: f32,
    rate: f32,
    kind: Option<NativeFilterKind>,
    until_control: u8,
    target_g: f32,
    delta_g: f32,
    ramp_ticks: u32,
}
fn is_sv(kind: NativeFilterKind) -> bool {
    matches!(
        kind,
        NativeFilterKind::SvLowpass1 | NativeFilterKind::SvHighpass1
    )
}
impl NativeFilterState {
    pub(crate) fn step(
        &mut self,
        filter: NativeFilter,
        cutoff: f32,
        octaves: f32,
        rate: f32,
        input: [f32; 2],
    ) -> [f32; 2] {
        self.step_inner(filter, cutoff, octaves, rate, input, false)
    }
    /// A bound modulation buffer refreshes the SV1 target every control tick,
    /// even when its sampled value is unchanged (native wrapper 0x10053012c).
    pub(crate) fn step_modulated(
        &mut self,
        filter: NativeFilter,
        cutoff: f32,
        octaves: f32,
        rate: f32,
        input: [f32; 2],
    ) -> [f32; 2] {
        self.step_inner(filter, cutoff, octaves, rate, input, true)
    }
    fn step_inner(
        &mut self,
        filter: NativeFilter,
        cutoff: f32,
        octaves: f32,
        rate: f32,
        input: [f32; 2],
        modulated: bool,
    ) -> [f32; 2] {
        if !rate.is_finite() || rate <= 0.0 {
            *self = Self::default();
            return [0.0; 2];
        }
        let cutoff = cutoff.clamp(0.0, 1.0);
        let sv = is_sv(filter.kind);
        if self.kind != Some(filter.kind) || self.rate != rate {
            // Resume from the first effective control, preserving the established
            // Cadence onset policy. Native SV1 rate updates reset integrators.
            if sv || self.kind != Some(filter.kind) {
                self.left = [0.0; 12];
                self.right = [0.0; 12];
            }
            self.map_coefficients(filter, cutoff, octaves, rate);
            self.cutoff = cutoff;
            self.resonance = filter.resonance;
            self.mapped_cutoff = cutoff;
            self.mapped_resonance = filter.resonance;
            self.octaves = octaves;
            self.rate = rate;
            self.kind = Some(filter.kind);
            self.until_control = if sv || modulated { CONTROL_FRAMES } else { 0 };
            self.target_g = self.coeffs[0];
            self.delta_g = 0.0;
            self.ramp_ticks = 0;
        } else if sv {
            if self.until_control == 0 {
                self.until_control = CONTROL_FRAMES;
                self.sv_control(filter, cutoff, octaves, modulated);
            }
        } else {
            let active = modulated
                || (cutoff - self.cutoff).abs() > 0.0001
                || (filter.resonance - self.resonance).abs() > 0.0001
                || octaves != self.octaves;
            if active && self.until_control == 0 {
                self.until_control = CONTROL_FRAMES;
                self.legacy_control(filter, cutoff, octaves, modulated);
            }
        }
        if sv {
            self.until_control -= 1;
            // Increment once per frame, before processing either channel.
            self.coeffs[0] += self.delta_g;
        } else if self.until_control > 0
            && (modulated
                || (cutoff - self.cutoff).abs() > 0.0001
                || (filter.resonance - self.resonance).abs() > 0.0001
                || octaves != self.octaves)
        {
            // Legacy idle filters retain their remaining control interval.
            self.until_control -= 1;
        }
        let run = |x: f32, s: &mut [f32; 12]| {
            if sv {
                let g = self.coeffs[0];
                let low = (s[0] + s[0] + x * g) / (g + 1.0);
                let high = x - low;
                s[0] += high * g;
                if filter.kind == NativeFilterKind::SvHighpass1 {
                    high
                } else {
                    low
                }
            } else if super::filter_extended::supports(filter.kind) {
                super::filter_extended::step(filter.kind, x, s, &self.extended)
            } else {
                let c = self.coeffs;
                let y = c[1] * s[0] + x * c[0] + s[1] * c[2] - s[2] * c[3] - s[3] * c[4];
                s[1] = s[0];
                s[0] = x;
                s[3] = s[2];
                s[2] = y;
                y
            }
        };
        [
            run(input[0], &mut self.left),
            run(input[1], &mut self.right),
        ]
    }
    fn map_coefficients(&mut self, filter: NativeFilter, cutoff: f32, octaves: f32, rate: f32) {
        if super::filter_extended::supports(filter.kind) {
            self.extended = super::filter_extended::prepare(filter, cutoff, octaves, rate);
        } else {
            self.coeffs = coefficients(filter, cutoff, octaves, rate);
        }
    }
    fn sv_control(&mut self, filter: NativeFilter, cutoff: f32, octaves: f32, modulated: bool) {
        if modulated || cutoff != self.cutoff || octaves != self.octaves {
            self.target_g = coefficients(filter, cutoff, octaves, self.rate)[0];
            self.cutoff = cutoff;
            self.octaves = octaves;
            let ticks = ((self.rate * 0.002 / CONTROL_FRAMES as f32 + 0.5) as u32).max(1);
            let reciprocal = 1.0 / (ticks * CONTROL_FRAMES as u32) as f32;
            self.delta_g = (self.target_g - self.coeffs[0]) * reciprocal;
            if self.delta_g * self.delta_g < 1e-15 {
                self.coeffs[0] = self.target_g;
                self.delta_g = 0.0;
                self.ramp_ticks = 0;
            } else {
                self.ramp_ticks = ticks;
            }
        } else if self.ramp_ticks > 0 {
            self.ramp_ticks -= 1;
            if self.ramp_ticks == 0 {
                self.coeffs[0] = self.target_g;
                self.delta_g = 0.0;
            }
        }
    }
    fn legacy_control(&mut self, filter: NativeFilter, cutoff: f32, octaves: f32, modulated: bool) {
        // BFilterBase_MF::Process 0x100620000: the buffer route caps upward
        // cutoff motion; downward buffer changes pass through immediately.
        let limit = (1.0 / (self.resonance * 31.0 + 4.0)).clamp(0.028571429, 0.25);
        let difference = cutoff - self.cutoff;
        self.cutoff += if modulated {
            difference.min(limit)
        } else {
            difference.clamp(-limit, limit)
        };
        let q_difference = filter.resonance - self.resonance;
        if q_difference.abs() > 0.0001 {
            self.resonance += q_difference * 0.015;
        }
        if (self.cutoff - self.mapped_cutoff).abs() >= 1e-6
            || (self.resonance - self.mapped_resonance).abs() >= 1e-6
            || octaves != self.octaves
        {
            let smoothed = NativeFilter {
                resonance: self.resonance,
                ..filter
            };
            self.map_coefficients(smoothed, self.cutoff, octaves, self.rate);
            self.mapped_cutoff = self.cutoff;
            self.mapped_resonance = self.resonance;
            self.octaves = octaves;
        }
    }
}

fn sin7(x: f32) -> f32 {
    if x.abs() < 0.0001 {
        return 0.0;
    }
    let x3 = x * x * x;
    let x5 = x * x * x3;
    x + x3 * -0.16666667 + x5 * 0.008333334 + x * x * x5 * -0.0001984127
}
fn cos6(x: f32) -> f32 {
    if x.abs() < 0.0001 {
        return 1.0;
    }
    let x4 = x * x * x * x;
    x * x * -0.5 + 1.0 + x4 * 0.041666668 + x * x * x4 * -0.0013888889
}

pub(super) fn legacy_control_value(cutoff: f32, octaves: f32, rate: f32) -> f32 {
    let x = cutoff * 6.15;
    let exp_minus_one = if x < 0.0001 {
        0.0
    } else {
        let x3 = x * x * x;
        let x4 = x3 * x;
        let x5 = x4 * x;
        let x6 = x5 * x;
        let x7 = x6 * x;
        (x + 1.0
            + x * x * 0.5
            + x3 * 0.16666667
            + x4 * 0.041666668
            + x5 * 0.008333334
            + x6 * 0.0013888889
            + x7 * 0.0001984127
            + x7 * x * 2.4801588e-5) as f64
            - 1.0
    };
    let control = ((exp_minus_one / 388.67681884765625) as f32).clamp(0.0, 1.0) + 0.002;
    let scale = if octaves == 0.0 {
        1.0
    } else {
        octaves.clamp(-16.0, 16.0).exp2()
    };
    // The octave compatibility route must not drive the recovered prototype
    // below its native minimum, where float normalization becomes 0/0.
    let control = (control * scale).clamp(0.002, 1.002);
    ((44100.0 / rate) * 0.998004 * control).clamp(0.0, 1.0)
}

pub(super) fn warp(control: f32, factor: f32, high: bool) -> f32 {
    if high {
        let x = control * std::f32::consts::FRAC_PI_2 * factor * 0.9971332;
        -cos6(x + 0.5) / cos6(x - 0.5)
    } else {
        let x = control * std::f32::consts::FRAC_PI_2 * factor * 0.9987809;
        sin7(0.5 - x) / sin7(x + 0.5)
    }
}

fn legacy_warp(cutoff: f32, octaves: f32, rate: f32, high: bool, two: bool) -> f32 {
    let factor = if high {
        if two {
            0.92
        } else {
            0.82
        }
    } else {
        0.99
    };
    warp(legacy_control_value(cutoff, octaves, rate), factor, high)
}

pub(super) fn pitch_frequency(semitones: f32) -> f32 {
    let table = EXP_TABLE
        .get()
        .expect("filter table initialized during load");
    let index = (semitones * 5.0 + 1381.8816).clamp(0.0, 2399.0);
    let i = index as usize;
    table[i] + (table[i + 1] - table[i]) * (index - i as f32)
}

pub(crate) fn coefficients(filter: NativeFilter, cutoff: f32, octaves: f32, rate: f32) -> [f32; 5] {
    use NativeFilterKind::*;
    if matches!(filter.kind, SvLowpass1 | SvHighpass1) {
        // ExpTable initializer 0x101c02f98: exp2(i/60 - 20), with
        // float base values and adjacent float differences. No RT initialization.
        let table = EXP_TABLE
            .get()
            .expect("filter table initialized during load");
        // Legacy unindexed routes are expressed in octaves by Cadence. Apply
        // that offset in SV1's pitch coordinate and keep its native bounds.
        let cutoff = (cutoff + octaves.clamp(-16.0, 16.0) * (12.0 / 115.0)).clamp(0.0, 1.0);
        let index = ((cutoff * 115.0 + 20.0) * 5.0 + 1381.8816).clamp(0.0, 2399.0);
        let i = index as usize;
        let hz = table[i] + (table[i + 1] - table[i]) * (index - i as f32);
        let x = hz * std::f32::consts::PI / rate;
        let g = x * ((x * x * 0.4 + 1.0) * x * x * 0.33333334 + 1.0);
        return [g, 0.0, 0.0, 0.0, 0.0];
    }
    let high = matches!(filter.kind, LegacyHighpass1 | LegacyHighpass2);
    let two = matches!(filter.kind, LegacyLowpass2 | LegacyHighpass2);
    let w = legacy_warp(cutoff, octaves, rate, high, two);
    if !two {
        let prototype = f32::from_bits(0x3e963993);
        let feedback = ((w + prototype) as f64 / (1.0 + w as f64 * prototype as f64)) as f32;
        let b = (1.0 - feedback) * 0.5;
        return if high {
            [b, -b, 0.0, feedback, 0.0]
        } else {
            [b, b, 0.0, -feedback, 0.0]
        };
    }
    let resonance = 1.0 - (1.0 - filter.resonance) * (1.0 - filter.resonance);
    let a1 = f32::from_bits(0x3f2d700b);
    let a2 = (resonance * 2.687 + 1.0) * f32::from_bits(0xbe820097);
    let b0 = f32::from_bits(0x3e139041);
    let b1 = f32::from_bits(0x3e939041);
    let b2 = b0;
    let w2 = w * w;
    let norm = 1.0 / ((w * a1 + 1.0) - w2 * a2);
    let fb1 = (a1 + w * 2.0 + w2 * a1 + w * a2 * -2.0) * norm;
    let fb2 = ((-(a1 * w) - w2) + a2) * norm;
    let n0 = norm * ((b0 - w * b1) + w2 * b2);
    let n1 = (b1 + w * b0 * -2.0 + w2 * b1 + w * b2 * -2.0) * norm;
    let n2 = (-(b1 * w) + w2 * b0 + b2) * norm;
    let gain = ((1.0 - (fb1 + fb2)) / (n1 + n0 + n2)) * ((1.0 - resonance) * 0.55 + 0.45);
    if high {
        [n0 * gain, -n1 * gain, n2 * gain, fb1, -fb2]
    } else {
        [n0 * gain, n1 * gain, n2 * gain, -fb1, -fb2]
    }
}

#[cfg(test)]
#[path = "filter_timing_tests.rs"]
mod timing_tests;

#[cfg(test)]
#[path = "filter_extended_tests.rs"]
mod extended_tests;
