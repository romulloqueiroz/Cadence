//! Scalar translation of `BEffectVan51::ProcessInt` (0x1005d55bc).
//!
//! The native routine processes channels outside its frame loop and advances
//! shared smoothing only on the last channel. A slice snapshot preserves that
//! ordering while the sampler consumes one stereo frame at a time.
use crate::amp_dsp::TableBase;
use crate::van51_dsp::Van51State;

#[derive(Default)]
pub(crate) struct Van51Process {
    left_controls: [f32; 28],
}

impl Van51Process {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            left_controls: self.left_controls,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.left_controls = template.left_controls;
    }

    pub(crate) fn begin_slice(&mut self, state: &Van51State) {
        self.left_controls = std::array::from_fn(|i| state.f32(0x94f0 + i * 4));
    }

    pub(crate) fn process_frame(
        &mut self,
        state: &mut Van51State,
        tables: &[TableBase; 12],
        left: f32,
        right: f32,
    ) -> [f32; 2] {
        let mono = state.u8(0x1c0) != 0;
        let inputs = if mono {
            [left * 0.5 + right * 0.5, 0.0]
        } else {
            [left, right]
        };
        let channels = if mono { 1 } else { 2 };
        let mut outputs = [0.0; 2];
        for channel in 0..channels {
            let base = channel * 0x310;
            state.set_f32(base + 0x1d0, inputs[channel]);
            for phase in 0..4 {
                let controls = if channel + 1 == channels {
                    let currents = std::array::from_fn(|i| {
                        state.f32(0x94f0 + i * 4) + state.f32(0x9560 + i * 4)
                    });
                    for (i, value) in currents.iter().enumerate() {
                        state.set_f32(0x94f0 + i * 4, *value);
                    }
                    currents
                } else {
                    self.left_controls
                };
                let fir = 0x9690 - phase * 0x10;
                let input = state.f32(base + 0x1d0) * state.f32(fir)
                    + state.f32(base + 0x1d8) * state.f32(fir + 8)
                    + state.f32(base + 0x1d4) * state.f32(fir + 4)
                    + state.f32(base + 0x1dc) * state.f32(fir + 12);
                let value = internal_step(state, tables, base, &controls, input);
                state.set_f32(base + 0x1ec - phase * 4, value);
            }
            // Native reduces the four SIMD columns in this order.
            const ORDER: [usize; 16] = [0, 4, 8, 12, 2, 6, 10, 14, 1, 5, 9, 13, 3, 7, 11, 15];
            let mut sum = state.f32(base + 0x1e0) * state.f32(0x96a0);
            for i in &ORDER[1..] {
                sum += state.f32(base + 0x1e0 + i * 4) * state.f32(0x96a0 + i * 4);
            }
            outputs[channel] = sum;
            for i in (0..12).rev() {
                state.set_f32(base + 0x1f0 + i * 4, state.f32(base + 0x1e0 + i * 4));
            }
            state.set_f32(base + 0x1dc, state.f32(base + 0x1d8));
            state.set_f32(base + 0x1d8, state.f32(base + 0x1d4));
            state.set_f32(base + 0x1d4, state.f32(base + 0x1d0));
        }
        if mono {
            outputs[1] = outputs[0];
        }
        outputs
    }
}

/// Four input and four output histories. The two-lane reduction order follows
/// the native `fmul/fadd/faddp`, including the precomputed zero fourth tap.
fn filter4(state: &mut Van51State, history: usize, coefficients: usize, input: f64) -> f64 {
    let h: [f64; 8] = std::array::from_fn(|i| state.f64(history + i * 8));
    let c: [f64; 10] = std::array::from_fn(|i| state.f64(coefficients + i * 8));
    let p0 = (h[0] * c[2] + h[2] * c[4] + c[0] * input) + (h[4] * c[6] + h[6] * c[8]);
    let p1 = (h[1] * c[3] + h[3] * c[5]) + (h[5] * c[7] + h[7] * c[9]);
    let output = p0 + p1;
    // Native histories are ordered [x0, x2, x1, x3, y0, y2, y1, y3].
    let next = [input, h[2], h[0], h[1], output, h[6], h[4], h[5]];
    for (i, value) in next.iter().enumerate() {
        state.set_f64(history + i * 8, *value);
    }
    output
}

/// ARM64 `FCVTXN` jams discarded bits into an odd low mantissa bit. The
/// native circuit uses it at all four double-filter to float boundaries.
fn narrow_odd(value: f64) -> f32 {
    let rounded = value as f32;
    if f64::from(rounded) == value || !value.is_finite() {
        return rounded;
    }
    let mut bits = rounded.to_bits();
    if bits & 1 == 0 {
        if f64::from(rounded).abs() > value.abs() {
            bits -= 1;
        }
        bits |= 1;
    }
    f32::from_bits(bits)
}

/// Table endpoint clamps select a segment; the original, unclamped value is
/// retained for its linear extrapolation residual.
fn lookup(table: &TableBase, value: f32) -> f32 {
    let low = -(table.anchor as f32);
    let high = 255.0 - table.anchor as f32;
    let index = (value.clamp(low, high) - low) as usize;
    let entry = table.entries[index.min(255)];
    entry[0] + entry[1] * (value - (low + index as f32))
}

fn nonlinear_bank(
    state: &mut Van51State,
    history: usize,
    coefficients: usize,
    inputs: [f32; 4],
    two_pole: bool,
) -> [f32; 4] {
    let mut result = [0.0; 4];
    let mut first_h = 0.0;
    for (lane, (output, input)) in result.iter_mut().zip(inputs).enumerate() {
        let lane = lane * 4;
        let u = state.f32(history + lane) + state.f32(coefficients + lane) * input;
        let h;
        if u <= state.f32(coefficients + 0x50 + lane) {
            *output = u * state.f32(coefficients + 0x20 + lane);
            h = 0.0;
        } else {
            *output =
                state.f32(coefficients + 0x80 + lane) + state.f32(coefficients + 0x70 + lane) * u;
            h = u * state.f32(coefficients + 0x60 + lane) + state.f32(coefficients + 0x90 + lane);
        }
        if lane == 0 {
            first_h = h;
        }
        let value = if coefficients == 0x9ac0 {
            h * state.f32(coefficients + 0x40 + lane)
                + input * state.f32(coefficients + 0x10 + lane)
                + state.f32(coefficients + 0x30 + lane) * *output
        } else {
            input * state.f32(coefficients + 0x10 + lane)
                + state.f32(coefficients + 0x30 + lane) * *output
                + h * state.f32(coefficients + 0x40 + lane)
        };
        state.set_f32(
            history + lane,
            value
                + if two_pole && lane == 0 {
                    state.f32(history + 0x10)
                } else {
                    0.0
                },
        );
    }
    if two_pole {
        state.set_f32(
            history + 0x10,
            (inputs[0] * state.f32(history + 0x18) - result[0] * state.f32(history + 0x14))
                + first_h * state.f32(history + 0x1c),
        );
    }
    result
}

#[cfg(target_arch = "aarch64")]
pub(super) fn reciprocal(value: f32) -> f32 {
    use std::arch::aarch64::{vdup_n_f32, vget_lane_f32, vmul_f32, vrecpe_f32, vrecps_f32};
    // The supply feedback uses one native reciprocal refinement, rather than
    // a correctly rounded division. Preserve its operating point on ARM64.
    unsafe {
        let value = vdup_n_f32(value);
        let estimate = vrecpe_f32(value);
        vget_lane_f32::<0>(vmul_f32(estimate, vrecps_f32(estimate, value)))
    }
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) fn reciprocal(value: f32) -> f32 {
    1.0 / value
}

fn internal_step(
    state: &mut Van51State,
    tables: &[TableBase; 12],
    base: usize,
    m: &[f32; 28],
    input: f32,
) -> f32 {
    let old_460: [f32; 4] = std::array::from_fn(|i| state.f32(base + 0x460 + i * 4));
    let old_480: [f32; 4] = std::array::from_fn(|i| state.f32(base + 0x480 + i * 4));
    let mut response = filter4(state, base + 0x300, 0x97f0, old_460[1] as f64);
    if state.f32(0x95a0) != 0.0 {
        let alternate = filter4(state, base + 0x340, 0x9840, old_460[1] as f64);
        response = alternate + (response - alternate) * m[16] as f64;
    }
    let mut tone = filter4(state, base + 0x280, 0x9750, old_480[0] as f64);
    if state.f32(0x9560) != 0.0 {
        let alternate = filter4(state, base + 0x2c0, 0x97a0, old_480[0] as f64);
        tone = alternate + (tone - alternate) * m[0] as f64;
    }
    let mut feedback = filter4(state, base + 0x380, 0x9890, old_480[2] as f64);
    if state.f32(0x95b0) != 0.0 {
        let alternate = filter4(state, base + 0x400, 0x98e0, old_480[2] as f64);
        feedback = alternate + (feedback - alternate) * m[20] as f64;
    }
    let stage8 = filter4(state, base + 0x3c0, 0x9930, state.f32(base + 0x47c) as f64);
    let old_478 = state.f32(base + 0x478);
    let low0 = state.f32(base + 0x220) + (old_460[0] - state.f32(base + 0x220)) * m[2];
    let high1 = old_460[1] - state.f32(base + 0x224);
    state.set_f32(base + 0x220, low0);
    state.set_f32(base + 0x224, state.f32(base + 0x224) + high1 * m[3]);
    let low2 = state.f32(base + 0x228) + (old_460[2] - state.f32(base + 0x228)) * state.f32(0x96e8);
    let low3 = state.f32(base + 0x22c) + (old_460[3] - state.f32(base + 0x22c)) * state.f32(0x96ec);
    state.set_f32(base + 0x228, low2);
    state.set_f32(base + 0x22c, low3);
    let pre_inputs = [old_478, old_478, high1, narrow_odd(tone)];
    let mut pre = [0.0; 4];
    for i in 0..4 {
        let at = base + 0x230 + i * 4;
        let value = state.f32(at) + m[4 + i] * (pre_inputs[i] - state.f32(at));
        state.set_f32(at, value);
        pre[i] = value;
    }
    let mut mix = [0.0; 4];
    for i in 0..4 {
        mix[i] = pre_inputs[i] + m[8 + i] * pre[i];
        state.set_f32(base + 0x470 + i * 4, mix[i]);
    }
    let nonlinear_inputs = [
        input * m[1],
        input * m[1],
        state.f32(base + 0x48c),
        state.f32(base + 0x498),
    ];
    let a = nonlinear_bank(state, base + 0x4a0, 0x9980, nonlinear_inputs, true);
    let supply2 = state.f32(base + 0x458);
    let supply3 = state.f32(base + 0x45c);
    let q = [
        low0 + state.f32(0x9bc0) + state.f32(0x9b80) * a[0] * 1.2679144 * supply3,
        old_480[1] + state.f32(0x9bc4) + state.f32(0x9b84) * narrow_odd(stage8) * supply2,
        state.f32(0x9bc8) + state.f32(0x9b88) * a[2] * 4.477442 * supply3 + 0.0,
        state.f32(0x9bcc) + state.f32(0x9b8c) * a[3] * 18.6 * supply3 + 0.0,
    ];
    let shaped = [
        lookup(&tables[0], q[0]),
        lookup(&tables[8], q[1]),
        lookup(&tables[4], q[2]),
        lookup(&tables[3], q[3]),
    ];
    let supply0 = state.f32(base + 0x270);
    let supply1 = state.f32(base + 0x274);
    let supply_target2 = state.f32(base + 0x278);
    let supply_target3 = state.f32(base + 0x27c);
    let targets = [
        supply_target3 * shaped[0] * state.f32(0x96f8),
        supply_target2 * shaped[1] * state.f32(0x96fc),
        supply_target3 * shaped[2] * m[14],
        supply_target3 * shaped[3] * m[15],
    ];
    let rates = [
        m[12],
        state.f32(0x9704),
        state.f32(0x9708),
        state.f32(0x970c),
    ];
    let mut first_low = [0.0; 4];
    for i in 0..4 {
        let at = base + 0x240 + i * 4;
        first_low[i] = state.f32(at) + rates[i] * (targets[i] - state.f32(at));
        state.set_f32(at, first_low[i]);
    }
    let old_high: [f32; 4] = std::array::from_fn(|i| state.f32(base + 0x250 + i * 4));
    let error: [f32; 4] = std::array::from_fn(|i| first_low[i] - old_high[i]);
    state.set_f32(base + 0x484, state.f32(0x9b64) * shaped[1]);
    state.set_f32(base + 0x460, state.f32(0x9b60) * shaped[0]);
    state.set_f32(base + 0x464, first_low[0] + error[0] * m[22]);
    let next_tone = narrow_odd(feedback) + first_low[1] + error[1] * m[23];
    let next_mix =
        first_low[2] + error[2] * state.f32(0x9718) + first_low[3] + error[3] * state.f32(0x971c);
    let high_rates = [m[18], m[19], state.f32(0x9728), state.f32(0x972c)];
    for i in 0..4 {
        state.set_f32(base + 0x250 + i * 4, old_high[i] + error[i] * high_rates[i]);
    }
    let b = nonlinear_bank(
        state,
        base + 0x4c0,
        0x9a20,
        [next_mix, low3, next_tone, next_tone],
        false,
    );
    let qb0 = state.f32(0x9bd0) + state.f32(0x9b90) * b[0] * 1.2589254 * supply2;
    state.set_f32(base + 0x46c, lookup(&tables[5], qb0) * supply_target2);
    let q2 = [
        state.f32(0x9be0) + state.f32(0x9ba0) * supply3 * narrow_odd(response) * 2.5907648,
        state.f32(0x9be4) + state.f32(0x9ba4) * supply2 * b[1] * 1.0,
        state.f32(0x9be8) + state.f32(0x9ba8) * state.f32(base + 0x454) * b[2] * 0.63095737,
        state.f32(0x9bec) + state.f32(0x9bac) * state.f32(base + 0x454) * b[3] * 1.0,
    ];
    let stage2 = lookup(&tables[2], q2[0]);
    state.set_f32(base + 0x48c, supply_target3 * stage2);
    let targets2 = [
        mix[0],
        supply_target2 * lookup(&tables[6], q2[1]),
        supply1 * lookup(&tables[9], q2[2]),
        supply1 * lookup(&tables[10], q2[3]),
    ];
    let rates2 = [
        m[9],
        state.f32(0x9734),
        state.f32(0x9738),
        state.f32(0x973c),
    ];
    let mut second_low = [0.0; 4];
    for i in 0..4 {
        let at = base + 0x260 + i * 4;
        second_low[i] = state.f32(at) + rates2[i] * (targets2[i] - state.f32(at));
        state.set_f32(at, second_low[i]);
    }
    let bank3_input = [
        m[24] * second_low[0],
        second_low[1],
        second_low[2],
        second_low[3],
    ];
    let d = nonlinear_bank(state, base + 0x4d0, 0x9ac0, bank3_input, false);
    let old_stage6 = old_480[0];
    let q3 = [
        low2 + state.f32(0x9bf0)
            + supply3
                * (state.f32(0x9d00)
                    + state.f32(0x9bb0) * (d[0] * 0.9567833 + old_stage6 * 0.0 + supply0 * 0.0)),
        state.f32(0x9bf4)
            + supply2
                * (state.f32(0x9d04)
                    + state.f32(0x9bb4)
                        * (d[1] * 1.4125376 + old_stage6 * 0.13462321 + supply0 * 0.0))
            + 0.0,
        state.f32(0x9bf8)
            + state.f32(0x9d18)
                * (state.f32(0x9d08)
                    + state.f32(0x9bb8) * (d[2] * 0.95 + old_stage6 * 0.0 + supply0 * 0.14))
            + 0.0,
        state.f32(0x9bfc)
            + state.f32(0x9d18)
                * (state.f32(0x9d0c)
                    + state.f32(0x9bbc) * (d[3] * 0.95 + old_stage6 * 0.0 + supply0 * 0.14))
            + 0.0,
    ];
    let rhythm = lookup(&tables[1], q3[0]);
    let power0 = state.f32(0x9d1c) * lookup(&tables[11], q3[2]);
    let power1 = state.f32(0x9d1c) * lookup(&tables[11], q3[3]);
    state.set_f32(base + 0x468, state.f32(0x9b70) * rhythm);
    state.set_f32(base + 0x498, supply_target3 * rhythm);
    state.set_f32(base + 0x480, supply_target2 * lookup(&tables[7], q3[1]));
    let differential = state.f32(0x94ac) * (power1 - power0);
    state.set_f32(base + 0x488, differential);
    let output = state.f32(0x94b0) * differential;
    let mut phase = state.f32(base + 0x440) + state.f32(base + 0x444);
    if phase >= 1.0 {
        phase -= 2.0;
    }
    state.set_f32(base + 0x440, phase);
    let ripple = state.f32(base + 0x448)
        - (state.f32(base + 0x44c) + state.f32(base + 0x448) * phase * phase);
    let sag = state.f32(base + 0x44c)
        + state.f32(0x9d14) * (power1 + power0)
        + state.f32(0x9d10) * if ripple > 0.0 { ripple } else { 0.0 };
    state.set_f32(base + 0x44c, sag);
    let supplies = [supply0, supply1, supply_target2, supply_target3];
    let supply_targets = [
        sag * 0.99,
        supply0 * 0.93,
        supply1 * 0.71,
        supply_target2 * 0.95,
    ];
    for i in 0..4 {
        let value = supplies[i] + state.f32(0x9740 + i * 4) * (supply_targets[i] - supplies[i]);
        state.set_f32(base + 0x270 + i * 4, value);
        state.set_f32(base + 0x450 + i * 4, reciprocal(value));
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::amp_dsp::van51_tables;

    #[test]
    fn van51_matches_observed_kontakt_host_activation() {
        let bytes = include_bytes!("../tests/fixtures/van51-host-trace.bin");
        assert_eq!(&bytes[..8], b"VANHOST1");
        let blocks = u32::from_le_bytes(bytes[8..12].try_into().unwrap()) as usize;
        let frames = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let mut state = Van51State::new();
        for (i, value) in bytes[16..16 + 0xa000].iter().copied().enumerate() {
            state.set_u8(i, value);
        }
        let mut at = 16 + 0xa000;
        let mut process = Van51Process::default();
        for block in 0..blocks {
            // Entry state was observed after the first control callback.
            if block != 0 {
                state.on_control_rate();
            }
            process.begin_slice(&state);
            for frame in 0..frames {
                let values: [f32; 4] = std::array::from_fn(|_| {
                    let value = f32::from_le_bytes(bytes[at..at + 4].try_into().unwrap());
                    at += 4;
                    value
                });
                let actual =
                    process.process_frame(&mut state, van51_tables(), values[0], values[1]);
                for (channel, actual) in actual.into_iter().enumerate() {
                    let expected = values[2 + channel];
                    assert!(actual.is_finite() && (actual - expected).abs() <= 2e-6 * (1.0 + expected.abs()),
                        "host block {block} frame {frame} channel {channel}: {actual} != {expected}");
                }
            }
        }
        assert_eq!(at, bytes.len());
    }

    #[test]
    fn van51_circuit_matches_independent_process_replay() {
        #[cfg(target_arch = "aarch64")]
        let bytes = include_bytes!("../tests/fixtures/van51-process-native.bin");
        #[cfg(not(target_arch = "aarch64"))]
        let bytes = include_bytes!("../tests/fixtures/van51-process.bin");
        assert_eq!(&bytes[..8], b"VANPROC1");
        let mut cursor = 8;
        let read_u32 = |cursor: &mut usize| {
            let value = u32::from_le_bytes(bytes[*cursor..*cursor + 4].try_into().unwrap());
            *cursor += 4;
            value
        };
        let count = read_u32(&mut cursor);
        assert_eq!(count, if cfg!(target_arch = "aarch64") { 5 } else { 4 });
        let mut frames_tested = 0;
        let mut maximum_error = 0.0f32;
        for case in 0..count {
            let frames = read_u32(&mut cursor) as usize;
            let mut state = Van51State::new();
            for (i, value) in bytes[cursor..cursor + 0xa000].iter().enumerate() {
                state.set_u8(i, *value);
            }
            cursor += 0xa000;
            let mut process = Van51Process::default();
            // Each fixture represents one native ProcessInt call, including
            // the left-channel frozen-current rule for the whole call.
            process.begin_slice(&state);
            for frame in 0..frames {
                let values: [f32; 4] =
                    std::array::from_fn(|_| f32::from_bits(read_u32(&mut cursor)));
                let actual =
                    process.process_frame(&mut state, van51_tables(), values[0], values[1]);
                for (channel, (actual, expected)) in actual
                    .into_iter()
                    .zip(values[2..].iter().copied())
                    .enumerate()
                {
                    let error = (actual - expected).abs();
                    maximum_error = maximum_error.max(error);
                    assert!(
                        actual.is_finite() && error <= 2e-6 * (1.0 + expected.abs()),
                        "case {case} frame {frame} channel {channel}: {actual} != {expected}"
                    );
                }
                frames_tested += 1;
            }
            let expected = &bytes[cursor..cursor + 0xa000];
            cursor += 0xa000;
            for channel in 0..2 {
                let base = channel * 0x310;
                // Native operating-point states include strongly cancelling
                // terms. Compare that case at the audio boundary; the four
                // synthetic cases verify every terminal history separately.
                for offset in (0x1d0..0x4e0).step_by(4) {
                    let offset = base + offset;
                    if (base + 0x280..base + 0x440).contains(&offset) {
                        if (offset - base) % 8 != 0 {
                            continue;
                        }
                        let expected =
                            f64::from_le_bytes(expected[offset..offset + 8].try_into().unwrap());
                        let actual = state.f64(offset);
                        assert!(
                            actual.is_finite()
                                && (case == 4
                                    || (actual - expected).abs() <= 2e-6 * (1.0 + expected.abs())),
                            "case {case} state {offset:x}: {actual} != {expected}"
                        );
                    } else {
                        let expected =
                            f32::from_le_bytes(expected[offset..offset + 4].try_into().unwrap());
                        let actual = state.f32(offset);
                        assert!(
                            actual.is_finite()
                                && (case == 4
                                    || (actual - expected).abs() <= 2e-6 * (1.0 + expected.abs())),
                            "case {case} state {offset:x}: {actual} != {expected}"
                        );
                    }
                }
            }
            for offset in (0x94f0..0x9560).step_by(4) {
                let expected = f32::from_le_bytes(expected[offset..offset + 4].try_into().unwrap());
                assert_eq!(
                    state.f32(offset),
                    expected,
                    "case {case} control {offset:x}"
                );
            }
        }
        assert_eq!(cursor, bytes.len());
        eprintln!("VAN51_PROCESS frames={frames_tested} max_error={maximum_error}");
    }

    /// Offline differential-analysis probe. Explicit paths keep it outside
    /// routine tests and permit comparing the identical starting memory image.
    #[test]
    #[ignore = "requires CADENCE_VAN51_PROCESS_PROBE directory"]
    fn van51_process_probe() {
        let prefix = std::env::var("CADENCE_VAN51_PROCESS_PROBE").unwrap();
        let path = std::path::Path::new(&prefix);
        let mut state = Van51State::new();
        if path.join("state.bin").exists() {
            let bytes = std::fs::read(path.join("state.bin")).unwrap();
            assert_eq!(bytes.len(), crate::van51_dsp::VAN51_STATE_SIZE);
            for (i, value) in bytes.into_iter().enumerate() {
                state.set_u8(i, value);
            }
        } else {
            state.initialize(48_000.0);
            state.on_control_rate();
            state.snap_controls();
        }
        let bytes: Vec<u8> = (0..crate::van51_dsp::VAN51_STATE_SIZE)
            .map(|i| state.u8(i))
            .collect();
        std::fs::write(path.join("initial.bin"), bytes).unwrap();
        let inputs = if path.join("input.bin").exists() {
            std::fs::read(path.join("input.bin")).unwrap()
        } else {
            (0..512)
                .flat_map(|i| {
                    let value = if i == 0 { 0.1f32 } else { 0.0f32 };
                    [value, 0.0].into_iter().flat_map(f32::to_le_bytes)
                })
                .collect()
        };
        std::fs::write(path.join("input.bin"), &inputs).unwrap();
        let mut process = Van51Process::default();
        let mut outputs = Vec::new();
        let mut first_nan = None;
        for (frame, bytes) in inputs.chunks_exact(8).enumerate() {
            if frame % 32 == 0 {
                process.begin_slice(&state);
            }
            let output = process.process_frame(
                &mut state,
                van51_tables(),
                f32::from_le_bytes(bytes[..4].try_into().unwrap()),
                f32::from_le_bytes(bytes[4..].try_into().unwrap()),
            );
            if !output.iter().all(|v| v.is_finite()) && first_nan.is_none() {
                first_nan = Some(frame);
            }
            for value in output {
                outputs.extend_from_slice(&value.to_le_bytes());
            }
        }
        std::fs::write(path.join("rust-output.bin"), outputs).unwrap();
        let bytes: Vec<u8> = (0..crate::van51_dsp::VAN51_STATE_SIZE)
            .map(|i| state.u8(i))
            .collect();
        std::fs::write(path.join("rust-final.bin"), bytes).unwrap();
        eprintln!("VAN51_PROCESS_PROBE first_nonfinite={first_nan:?}");
    }
}
