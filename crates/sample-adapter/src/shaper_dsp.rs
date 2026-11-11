//! BEffectShaper scalar paths. Tables are exact little-endian floats recovered
//! from Kontakt 8 ARM64; provenance and limits: docs/kontakt-shaper-types.md.
use nk_format::fx::{ShaperKind, ShaperParams};

const TABLES: &[u8; 28_160] = include_bytes!("shaper_tables.bin");
const COMPRESS_GAIN: usize = 51 * 128;
const DRIVE_AMOUNT: usize = COMPRESS_GAIN + 128;
const DRIVE_MIX: usize = DRIVE_AMOUNT + 128;
const DRIVE_GAIN: usize = DRIVE_MIX + 128;

fn table(index: usize) -> f32 {
    let start = index * 4;
    f32::from_le_bytes(TABLES[start..start + 4].try_into().unwrap())
}
fn lerp(a: f32, b: f32, fraction: f32) -> f32 {
    a + (b - a) * fraction
}
fn lookup(base: usize, position: f32) -> f32 {
    let index = position as usize;
    // Native reads one float past the table at position 127 with weight zero.
    // Clamp the upper neighbor so the reconstructed processor stays in bounds.
    lerp(
        table(base + index),
        table(base + (index + 1).min(127)),
        position - index as f32,
    )
}
fn coefficient(rate: f32, time_ms: f32) -> f32 {
    1.0 - (-6.643856 / (rate * 0.001 * time_ms - 1.0).max(1.0)).exp2()
}

#[derive(Clone, Copy)]
pub(crate) struct ShaperState {
    kind: Option<ShaperKind>,
    shape: f32,
    rate: f32,
    position: f32,
    dirty: bool,
    shape_coeff: f32,
    release_coeff: f32,
    gain_coeff: f32,
    envelope: f32,
    gain: f32,
    amount: f32,
    mix: f32,
    correction: f32,
}
impl Default for ShaperState {
    fn default() -> Self {
        Self {
            kind: None,
            shape: 0.0,
            rate: 0.0,
            position: 0.0,
            dirty: true,
            shape_coeff: 0.0,
            release_coeff: 0.0,
            gain_coeff: 0.0,
            // BEffectShaper::Reset seeds both histories at unity, not silence.
            envelope: 1.0,
            gain: 1.0,
            amount: 1.0,
            mix: 0.0,
            correction: 1.0,
        }
    }
}
impl ShaperState {
    pub(crate) fn step(&mut self, params: ShaperParams, rate: f32, input: [f32; 2]) -> [f32; 2] {
        if !rate.is_finite() || rate <= 0.0 || !params.shape.is_finite() {
            *self = Self::default();
            return [0.0; 2];
        }
        let shape = params.shape.clamp(-1.0, 1.0);
        let target = (shape + 1.0)
            * 0.5
            * match params.kind {
                ShaperKind::Drums => 50.0,
                ShaperKind::Enhanced => 127.0,
                ShaperKind::Classic => 0.0,
            };
        if self.kind != Some(params.kind) || self.rate != rate {
            *self = Self {
                kind: Some(params.kind),
                shape,
                rate,
                position: target,
                shape_coeff: coefficient(rate, 50.0),
                release_coeff: coefficient(rate, f32::from_bits(0x41cbcfab)),
                gain_coeff: coefficient(rate, f32::from_bits(0x402b707a)),
                ..Self::default()
            };
        } else if self.shape != shape {
            self.shape = shape;
            self.dirty = true;
        }
        if params.kind == ShaperKind::Classic {
            return input.map(|x| classic(shape, x));
        }
        if shape <= -0.9999 {
            // Internal copy bypass freezes the envelope and coefficient slew.
            return input;
        }
        if self.dirty {
            let delta = (target - self.position) * self.shape_coeff;
            if delta.abs() >= 0.0001 {
                self.position += delta;
            } else {
                self.position = target;
                self.dirty = false;
            }
            match params.kind {
                ShaperKind::Enhanced => {
                    self.amount = lookup(DRIVE_AMOUNT, self.position);
                    self.mix = lookup(DRIVE_MIX, self.position);
                    self.correction = lookup(DRIVE_GAIN, self.position);
                }
                ShaperKind::Drums => self.correction = lookup(COMPRESS_GAIN, self.position),
                ShaperKind::Classic => unreachable!(),
            }
        }
        match params.kind {
            ShaperKind::Enhanced => input.map(|x| {
                let driven = (x * self.amount).clamp(-8.0, 8.0);
                x + (self.correction * driven * (driven.abs() * -0.0625 + 1.0) - x) * self.mix
            }),
            ShaperKind::Drums => {
                let peak = input[0].abs().max(input[1].abs());
                self.envelope = if peak >= self.envelope {
                    peak
                } else {
                    self.envelope + self.release_coeff * (peak - self.envelope) + 1e-12 - 1e-12
                };
                let level = (self.envelope + 1e-5).min(1.0);
                let position = level * 127.0;
                let column = position as usize;
                let fraction = position - column as f32;
                let row = self.position as usize;
                let curve = |row: usize| {
                    let base = row.min(50) * 128;
                    lerp(
                        table(base + column),
                        table(base + (column + 1).min(127)),
                        fraction,
                    )
                };
                let desired = lerp(curve(row), curve(row + 1), self.position - row as f32) / level;
                self.gain = self.gain + self.gain_coeff * (desired - self.gain) + 1e-12 - 1e-12;
                let gain = self.gain * self.correction;
                input.map(|x| gain * x)
            }
            ShaperKind::Classic => unreachable!(),
        }
    }
}

fn classic(shape: f32, input: f32) -> f32 {
    // Keep the stored half-shape arithmetic: rounding matters at the gates.
    let control = (shape * 0.5 + 0.5) * 8.0;
    if shape.abs() <= 0.0001 || (control > 3.999 && control < 4.0001) {
        return input;
    }
    if control < 4.0 {
        let amount = control * 0.975 + 0.1;
        let reciprocal = 1.0 / (amount + 1.0);
        reciprocal * (amount * input + input * input * input)
    } else {
        let mix = control - 4.0;
        let magnitude = if control >= 5.0 {
            (input * mix).abs()
        } else {
            input.abs()
        }
        .min(1.0);
        let shaped = (2.0 * magnitude - magnitude * magnitude).copysign(input);
        if control >= 5.0 {
            shaped
        } else {
            mix * shaped + (1.0 - mix) * input
        }
    }
}
