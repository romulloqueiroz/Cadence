//! Scalar JUMP circuit recovered from BEffectJump::ProcessInt (0x1005cc6ac).
//! Four internal phases per host frame, two coupled four-lane nonlinear banks,
//! delayed supplies, and the native interpolation/decimation FIRs.
use crate::amp_dsp::{jump_tables, Smoothing24, TableBase};
use crate::jump_controls::JumpControlState;
use nk_format::fx::AmpSettings;
use std::ops::{Index, IndexMut};

const COEFFICIENT_BASE: usize = 0x4630;
const STATE_BASE: usize = 0x1d0;
const INPUT_FIR: [[f32; 4]; 4] = [
    [-0.0234375, 0.2265625, 0.8671875, -0.0703125],
    [-0.0625, 0.5625, 0.5625, -0.0625],
    [-0.0703125, 0.8671875, 0.2265625, -0.0234375],
    [0.0, 1.0, 0.0, 0.0],
];
#[allow(clippy::excessive_precision)] // Exact dyadic fractions from the native FIR.
const OUTPUT_FIR: [f32; 16] = [
    -0.005859375,
    -0.015625,
    -0.017578125,
    0.0,
    0.056640625,
    0.140625,
    0.216796875,
    0.25,
    0.216796875,
    0.140625,
    0.056640625,
    -0.0,
    -0.017578125,
    -0.015625,
    -0.005859375,
    0.0,
];

#[derive(Clone)]
struct Coefficients([f32; 256]);
impl Index<usize> for Coefficients {
    type Output = f32;
    fn index(&self, offset: usize) -> &Self::Output {
        &self.0[(offset - COEFFICIENT_BASE) / 4]
    }
}
impl IndexMut<usize> for Coefficients {
    fn index_mut(&mut self, offset: usize) -> &mut Self::Output {
        &mut self.0[(offset - COEFFICIENT_BASE) / 4]
    }
}

#[derive(Clone)]
struct Channel([f32; 72]);
impl Index<usize> for Channel {
    type Output = f32;
    fn index(&self, offset: usize) -> &Self::Output {
        &self.0[(offset - STATE_BASE) / 4]
    }
}
impl IndexMut<usize> for Channel {
    fn index_mut(&mut self, offset: usize) -> &mut Self::Output {
        &mut self.0[(offset - STATE_BASE) / 4]
    }
}

pub(crate) struct JumpAmp {
    settings: AmpSettings,
    rate: f32,
    tables: [TableBase; 6],
    coefficients: Coefficients,
    channels: [Channel; 2],
    smooth: Smoothing24,
    mono: bool,
    targets_pending: bool,
    refresh: bool,
    slice_remaining: usize,
}

impl JumpAmp {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            settings: self.settings,
            rate: self.rate,
            tables: self.tables.clone(),
            coefficients: self.coefficients.clone(),
            channels: self.channels.clone(),
            smooth: self.smooth.clone(),
            mono: self.mono,
            targets_pending: self.targets_pending,
            refresh: self.refresh,
            slice_remaining: self.slice_remaining,
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.settings = template.settings;
        self.rate = template.rate;
        self.tables = template.tables.clone();
        self.coefficients = template.coefficients.clone();
        self.channels = template.channels.clone();
        self.smooth = template.smooth.clone();
        self.mono = template.mono;
        self.targets_pending = template.targets_pending;
        self.refresh = template.refresh;
        self.slice_remaining = template.slice_remaining;
    }

    pub(crate) fn new(settings: AmpSettings) -> Self {
        let base = jump_tables();
        let mut amp = Self {
            settings,
            rate: 48_000.0,
            tables: [
                base[0].clone(),
                base[1].clone(),
                base[2].clone(),
                base[3].clone(),
                base[4].clone(),
                TableBase::power_tube(),
            ],
            coefficients: Coefficients([0.0; 256]),
            channels: [Channel([0.0; 72]), Channel([0.0; 72])],
            smooth: Smoothing24::default(),
            mono: false,
            targets_pending: false,
            refresh: true,
            slice_remaining: 0,
        };
        amp.prepare(48_000.0)
            .expect("valid initial JUMP sample rate");
        amp
    }

    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        self.rate = rate;
        self.set_settings(self.settings);
        self.reset();
        Ok(())
    }

    pub(crate) fn reset(&mut self) {
        let control = JumpControlState::build(self.settings, self.rate);
        control.write_coefficients(&mut self.coefficients.0);
        self.channels = [
            Channel(control.reset_channel()),
            Channel(control.reset_channel()),
        ];
        self.smooth.snap();
        self.targets_pending = false;
        self.refresh = true;
        self.slice_remaining = 0;
    }

    /// Prepare setter targets on the control thread. Processing interpolates
    /// them at the native 32-host-frame boundaries without resetting audio state.
    pub(crate) fn set_settings(&mut self, settings: AmpSettings) {
        let control = JumpControlState::build(settings, self.rate);
        control.write_coefficients(&mut self.coefficients.0);
        self.settings = settings;
        self.mono = control.mono;
        self.smooth.targets = control.targets;
        self.smooth.factor = control.rate_coefficients.base.smooth_factor;
        self.smooth.periods = control.rate_coefficients.base.period;
        self.smooth.state = 1;
        self.targets_pending = true;
        self.refresh = true;
    }

    fn begin_slice(&mut self) {
        if self.targets_pending {
            self.targets_pending = false;
            self.smooth.state = 2;
            self.smooth.start_smooth();
            self.refresh = true;
        } else {
            self.smooth.remaining -= 1;
            if self.smooth.remaining == 0 {
                self.smooth.snap();
            } else if self.smooth.state == 2 {
                self.smooth.state = 3;
            } else if self.smooth.state == 4 {
                self.smooth.state = 0;
            }
        }
        self.slice_remaining = 32;
    }

    /// Refresh the drive-dependent lane and resonance from current, smoothed
    /// controls. Native ProcessInt does this on each internal phase during a fade.
    fn refresh_coefficients(&mut self) {
        let c = &mut self.coefficients;
        let m = &self.smooth.currents;
        c[0x4910] = m[17];
        c[0x4900] = m[18];
        let d = m[19];
        let denom = c[0x4630] + (c[0x4670] * d + c[0x4640]) * d;
        let feedback = (c[0x4690] * d + c[0x4660]) * d;
        c[0x4818] = c[0x4634] + (c[0x4674] * d + c[0x4644]) * d;
        c[0x4878] = c[0x4638] + (c[0x4678] * d + c[0x4648]) * d;
        c[0x4838] = (c[0x4684] * d + c[0x4654]) * d;
        c[0x4888] = (c[0x4688] * d + c[0x4658]) * d;
        c[0x4858] = (c[0x4694] * d + c[0x4664]) * d;
        c[0x4738] = (c[0x4680] * d + c[0x4650]) * d;
        c[0x4898] = (c[0x4698] * d + c[0x4668]) * d;
        c[0x4758] = denom * c[0x46a4];
        let inverse = 1.0 / (denom * c[0x46a0] - feedback);
        c[0x47b8] = 1.0 / denom;
        c[0x47d8] = inverse;
        c[0x47f8] = denom * c[0x46a8] * inverse;
        c[0x4798] = feedback * c[0x46ac] * inverse;
        c[0x4778] = c[0x46a0] * inverse;
        self.refresh = self.smooth.pending;
    }

    pub(crate) fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
        if self.slice_remaining == 0 {
            self.begin_slice();
        }
        let inputs = if self.mono {
            let mixed = left * 0.5 + right * 0.5;
            [mixed, mixed]
        } else {
            [left, right]
        };
        for (state, input) in self.channels.iter_mut().zip(inputs) {
            state[0x1d0] = input;
        }
        for (phase, fir) in INPUT_FIR.iter().enumerate() {
            self.smooth.advance();
            if self.refresh {
                self.refresh_coefficients();
            }
            let count = if self.mono { 1 } else { 2 };
            for state in &mut self.channels[..count] {
                let input = state[0x1d0] * fir[0]
                    + state[0x1d8] * fir[2]
                    + state[0x1d4] * fir[1]
                    + state[0x1dc] * fir[3];
                let output = internal_step(
                    state,
                    &self.coefficients,
                    &self.tables,
                    &self.smooth.currents,
                    input,
                );
                state[0x1ec - phase * 4] = output;
            }
        }
        let mut output = [0.0; 2];
        let count = if self.mono { 1 } else { 2 };
        for (value, state) in output[..count].iter_mut().zip(&mut self.channels[..count]) {
            // Native NEON reduction visits column-major lanes in this order.
            const ORDER: [usize; 16] = [0, 4, 8, 12, 2, 6, 10, 14, 1, 5, 9, 13, 3, 7, 11, 15];
            let mut sum = state[0x1e0] * OUTPUT_FIR[0];
            for i in &ORDER[1..] {
                sum += state[0x1e0 + i * 4] * OUTPUT_FIR[*i];
            }
            *value = sum;
            state.0.copy_within(4..16, 8);
            state[0x1dc] = state[0x1d8];
            state[0x1d8] = state[0x1d4];
            state[0x1d4] = state[0x1d0];
        }
        if self.mono {
            output[1] = output[0];
        }
        self.slice_remaining -= 1;
        output
    }
}

/// Native clamps the index and extrapolates the selected endpoint segment.
fn lookup(table: &TableBase, q: f32) -> f32 {
    let low = -(table.anchor as f32);
    let high = 255.0 - table.anchor as f32;
    let index = (q.clamp(low, high) - low) as usize;
    let entry = table.entries[index.min(255)];
    entry[0] + entry[1] * (q - (low + index as f32))
}

fn internal_step(
    s: &mut Channel,
    c: &Coefficients,
    tables: &[TableBase; 6],
    m: &[f32; 24],
    input: f32,
) -> f32 {
    let a_input = [input, input, s[0x2d0], s[0x2d4]];
    let mut a = [0.0; 4];
    for (lane, (output, input)) in a.iter_mut().zip(a_input).enumerate() {
        let at = lane * 4;
        let u = s[0x220 + at] + c[0x4730 + at] * input;
        let h;
        if u <= c[0x4750 + at] {
            *output = c[0x47b0 + at] * u;
            h = 0.0;
        } else {
            *output = c[0x4790 + at] + c[0x4770 + at] * u;
            h = c[0x47f0 + at] + c[0x47d0 + at] * u;
        }
        s[0x220 + at] = s[0x230 + at]
            + c[0x4850 + at] * h
            + (input * c[0x4830 + at] - c[0x4810 + at] * *output);
        s[0x230 + at] = (input * c[0x4880 + at] - *output * c[0x4870 + at]) + h * c[0x4890 + at];
    }
    let q0 = s[0x250] + c[0x4950] + c[0x4930] * a[0] * s[0x2c8];
    let q2 = c[0x4958] + c[0x4938] * a[2] * s[0x2c8];
    let q3 = c[0x495c] + c[0x493c] * a[3] * s[0x2c4];
    let boost = lookup(&tables[0], q0);
    let targets = [
        s[0x2bc] * boost,
        s[0x2b8] * boost,
        s[0x2b8] * lookup(&tables[1], q2),
        s[0x2b4] * lookup(&tables[2], q3),
    ];
    for (lane, target) in targets.into_iter().enumerate() {
        let at = lane * 4;
        s[0x250 + at] += c[0x48d0 + at] * (target - s[0x250 + at]);
    }
    s[0x2d4] = s[0x258];
    s[0x260] += (s[0x2d8] - s[0x260]) * m[14];
    s[0x264] += (s[0x2dc] - s[0x264]) * m[15];
    s[0x268] += (s[0x254] - s[0x268]) * c[0x48e8];
    s[0x26c] += (s[0x25c] - s[0x26c]) * c[0x48ec];
    let stage2_mix = m[7] * s[0x25c] + m[3] * s[0x26c];
    let tone_mix = m[4] * s[0x2d8] + m[0] * s[0x260] + m[5] * s[0x2dc] + m[1] * s[0x264];
    s[0x2d0] = m[8] * input + m[6] * s[0x254] + m[2] * s[0x268];

    let b_input = [s[0x288], s[0x28c], tone_mix, tone_mix];
    let mut b = [0.0; 4];
    for (lane, (output, input)) in b.iter_mut().zip(b_input).enumerate() {
        let at = lane * 4;
        let u = s[0x240 + at] + c[0x4740 + at] * input;
        let h;
        if u <= c[0x4760 + at] {
            *output = c[0x47c0 + at] * u;
            h = 0.0;
        } else {
            *output = c[0x47a0 + at] + c[0x4780 + at] * u;
            h = c[0x4800 + at] + c[0x47e0 + at] * u;
        }
        s[0x240 + at] = c[0x4860 + at] * h + (c[0x4840 + at] * input - c[0x4820 + at] * *output);
    }
    let e0 = b[2] - s[0x270];
    let e1 = b[3] - s[0x274];
    let e2 = stage2_mix - s[0x278];
    let e3 = stage2_mix - s[0x27c];
    s[0x2d8] = e2;
    let combined = b[2] + m[12] * e0 + b[3] + m[13] * e1;
    s[0x278] += m[10] * e2;
    s[0x27c] += m[11] * e3;
    s[0x270] += c[0x48e0] * e0;
    s[0x274] += m[9] * e1;
    let power_feedback = s[0x298] * 0.111235954;
    let q = [
        c[0x4960] + c[0x49c8] * (c[0x49b0] + c[0x4940] * (power_feedback + b[0])),
        c[0x4964] + c[0x49c8] * (c[0x49b4] + c[0x4944] * (power_feedback + b[1])),
        c[0x4968] + s[0x2c0] * (c[0x49b8] + c[0x4948] * combined),
        c[0x496c] + s[0x2c0] * (c[0x49bc] + c[0x494c] * combined),
    ];
    let targets = [
        c[0x49cc] * lookup(&tables[5], q[0]),
        c[0x49cc] * lookup(&tables[5], q[1]),
        s[0x2b0] * lookup(&tables[3], q[2]),
        s[0x2b0] * lookup(&tables[4], q[3]),
    ];
    for (lane, target) in targets.into_iter().enumerate() {
        let at = lane * 4;
        s[0x280 + at] += c[0x48f0 + at] * (target - s[0x280 + at]);
    }
    let output = (s[0x280] - s[0x284]) * m[20];
    s[0x24c] = output;

    s[0x2e0] += s[0x2e4];
    if s[0x2e0] >= 1.0 {
        s[0x2e0] -= 2.0;
    }
    let ripple = s[0x2e8] - (s[0x2ec] + s[0x2e8] * s[0x2e0] * s[0x2e0]);
    s[0x2ec] = s[0x2ec] + c[0x49c4] * (s[0x280] + s[0x284]) + c[0x49c0] * ripple.max(0.0);
    let integrator_input = [e3, e3, s[0x2ec], s[0x2ec]];
    let mut e_first = 0.0;
    for (lane, input) in integrator_input.into_iter().enumerate() {
        let at = lane * 4;
        let e = input - (c[0x4900 + at] * s[0x2a0 + at] + s[0x290 + at]);
        if lane == 0 {
            e_first = e;
        }
        s[0x2a0 + at] += c[0x4910 + at] * e;
        s[0x290 + at] += c[0x4910 + at] * s[0x2a0 + at];
    }
    let t = e_first + s[0x290];
    s[0x2dc] = t + m[16] * (e3 - t);
    let supply_targets = [
        s[0x298] * 0.8712,
        s[0x2b0] * 0.85,
        s[0x2b4] * 0.96,
        s[0x2b8] * 0.0,
    ];
    for (lane, target) in supply_targets.into_iter().enumerate() {
        let at = lane * 4;
        s[0x2b0 + at] += c[0x4920 + at] * (target - s[0x2b0 + at]);
        s[0x2c0 + at] = 1.0 / s[0x2b0 + at];
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;

    fn settings(values: [f32; 6], boost: bool, mono: bool) -> AmpSettings {
        let mut settings = AmpSettings {
            kind: AmpKind::Jump,
            values: [0.0; 17],
            value_count: 6,
            flags: [false; 8],
            flag_count: 2,
            modes: [0; 4],
        };
        settings.values[..6].copy_from_slice(&values);
        settings.flags[..2].copy_from_slice(&[boost, mono]);
        settings
    }

    fn u32_at(bytes: &[u8], cursor: &mut usize) -> u32 {
        let value = u32::from_le_bytes(bytes[*cursor..*cursor + 4].try_into().unwrap());
        *cursor += 4;
        value
    }
    fn f32_at(bytes: &[u8], cursor: &mut usize) -> f32 {
        f32::from_bits(u32_at(bytes, cursor))
    }

    #[test]
    fn jump_circuit_matches_independent_process_replay() {
        let bytes = include_bytes!("../tests/fixtures/jump-process.bin");
        assert_eq!(&bytes[..8], b"JMPPROC1");
        let mut cursor = 8;
        let count = u32_at(bytes, &mut cursor);
        let c_count = u32_at(bytes, &mut cursor) as usize;
        let m_count = u32_at(bytes, &mut cursor) as usize;
        let s_count = u32_at(bytes, &mut cursor) as usize;
        assert_eq!((count, c_count, m_count, s_count), (3, 236, 24, 72));
        let mut samples = 0;
        let mut max_error = 0.0f32;
        for case in 0..count {
            let rate = f32_at(bytes, &mut cursor);
            let frames = u32_at(bytes, &mut cursor);
            let mut amp = JumpAmp::new(settings([0.5; 6], false, false));
            amp.prepare(rate).unwrap();
            for value in &mut amp.coefficients.0[..c_count] {
                *value = f32_at(bytes, &mut cursor);
            }
            for value in &mut amp.smooth.currents {
                *value = f32_at(bytes, &mut cursor);
            }
            amp.smooth.targets = amp.smooth.currents;
            for value in &mut amp.channels[0].0 {
                *value = f32_at(bytes, &mut cursor);
            }
            amp.refresh = false;
            for frame in 0..frames {
                let input = f32_at(bytes, &mut cursor);
                let expected = f32_at(bytes, &mut cursor);
                let actual = amp.process(input, 0.0)[0];
                let error = (actual - expected).abs();
                max_error = max_error.max(error);
                assert!(
                    actual.is_finite() && error <= 2e-6 * (1.0 + expected.abs()),
                    "case {case} frame {frame}: actual {actual}, expected {expected}"
                );
                samples += 1;
            }
            for (index, actual) in amp.channels[0].0.iter().enumerate() {
                let expected = f32_at(bytes, &mut cursor);
                assert!(
                    (actual - expected).abs() <= 2e-6 * (1.0 + expected.abs()),
                    "case {case} state {:x}: actual {actual}, expected {expected}",
                    STATE_BASE + index * 4
                );
            }
        }
        assert_eq!(cursor, bytes.len());
        eprintln!("JUMP_PROCESS {{\"frames\":{samples},\"max_error\":{max_error}}}");
    }

    /// Explicit offline analysis utility. Feed a native dry capture through the
    /// recovered circuit to separate amp errors from sampler/envelope errors.
    /// The comparison script, not this renderer, measures native audio parity.
    #[test]
    #[ignore = "requires CADENCE_JUMP_NATIVE_INPUT and CADENCE_JUMP_PROBE_OUTPUT"]
    fn jump_render_native_dry_capture() {
        use std::io::Write;
        let source = std::env::var("CADENCE_JUMP_NATIVE_INPUT").unwrap();
        let output = std::env::var("CADENCE_JUMP_PROBE_OUTPUT").unwrap();
        let scale: f32 = std::env::var("CADENCE_JUMP_NATIVE_INPUT_SCALE")
            .unwrap_or_else(|_| "1".into())
            .parse()
            .unwrap();
        let gain: f32 = std::env::var("CADENCE_JUMP_PROBE_OUTPUT_GAIN")
            .unwrap_or_else(|_| "1".into())
            .parse()
            .unwrap();
        let start_frame: usize = std::env::var("CADENCE_JUMP_PROBE_START_FRAME")
            .unwrap_or_else(|_| "0".into())
            .parse()
            .unwrap();
        let mut values = [0.5; 6];
        if let Ok(params) = std::env::var("CADENCE_JUMP_PROBE_VALUES") {
            let params: Vec<f32> = params.split(',').map(|v| v.parse().unwrap()).collect();
            values.copy_from_slice(&params);
        }
        let boost = std::env::var("CADENCE_JUMP_PROBE_BOOST").is_ok_and(|v| v == "1");
        let mono = std::env::var("CADENCE_JUMP_PROBE_MONO").is_ok_and(|v| v == "1");
        let decoded = nk_format::audio::load(&std::fs::read(&source).unwrap()).unwrap();
        assert_eq!(decoded.channels, 2);
        let mut amp = JumpAmp::new(settings(values, boost, mono));
        amp.prepare(decoded.sample_rate as f32).unwrap();
        let size = u32::try_from(decoded.num_frames * 8).unwrap();
        let mut file = std::io::BufWriter::new(std::fs::File::create(output).unwrap());
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(size + 36).to_le_bytes()).unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&3u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&decoded.sample_rate.to_le_bytes()).unwrap();
        file.write_all(&(decoded.sample_rate * 8).to_le_bytes())
            .unwrap();
        file.write_all(&8u16.to_le_bytes()).unwrap();
        file.write_all(&32u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&size.to_le_bytes()).unwrap();
        let mut peak = 0.0f32;
        for frame in 0..decoded.num_frames {
            let output = if frame < start_frame {
                [0.0; 2]
            } else {
                amp.process(
                    decoded.samples[0][frame] * scale,
                    decoded.samples[1][frame] * scale,
                )
            };
            for sample in output {
                let sample = sample * gain;
                assert!(sample.is_finite());
                peak = peak.max(sample.abs());
                file.write_all(&sample.to_le_bytes()).unwrap();
            }
        }
        file.flush().unwrap();
        eprintln!(
            "JUMP_NATIVE_DRY_PROBE frames={} rate={} peak={} input_scale={} output_gain={} start_frame={}",
            decoded.num_frames, decoded.sample_rate, peak, scale, gain, start_frame
        );
    }

    #[test]
    fn jump_lookup_extrapolates_both_endpoint_segments() {
        let table = &jump_tables()[1];
        let low = -(table.anchor as f32);
        let high = 255.0 - table.anchor as f32;
        for (q, index, residual) in [(low - 2.0, 0, -2.0), (high + 3.0, 255, 3.0)] {
            assert_eq!(
                lookup(table, q),
                table.entries[index][0] + table.entries[index][1] * residual
            );
        }
        let q = low + 47.25;
        assert_eq!(
            lookup(table, q),
            table.entries[47][0] + table.entries[47][1] * 0.25
        );
    }

    #[test]
    fn jump_native_controls_stay_finite_and_prepare_repeats() {
        for rate in [8000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            for value in [0.0, 0.5, 1.0] {
                for boost in [false, true] {
                    let mut amp = JumpAmp::new(settings([value; 6], boost, false));
                    amp.prepare(rate).unwrap();
                    let mut first = Vec::with_capacity(2048);
                    let mut peak = 0.0f32;
                    for frame in 0..2048 {
                        let input = (frame as f32 * 0.07).sin() * 0.2;
                        let output = amp.process(input, -input * 0.3);
                        for value in output {
                            assert!(
                                value.is_finite(),
                                "rate {rate}, control {value}, boost {boost}"
                            );
                            peak = peak.max(value.abs());
                        }
                        first.push(output);
                    }
                    assert!(peak < 16.0, "unbounded output: rate {rate}, control {value}, boost {boost}: peak {peak}");
                    amp.prepare(rate).unwrap();
                    for (frame, expected) in first.iter().enumerate() {
                        let input = (frame as f32 * 0.07).sin() * 0.2;
                        assert_eq!(amp.process(input, -input * 0.3), *expected);
                    }
                }
            }
        }
    }

    #[test]
    fn jump_mono_downmix_and_stereo_state_are_independent() {
        let mut mono = JumpAmp::new(settings([0.5; 6], false, true));
        let mut silent = JumpAmp::new(settings([0.5; 6], false, true));
        let mut stereo = JumpAmp::new(settings([0.5; 6], false, false));
        let mut stereo_silent = JumpAmp::new(settings([0.5; 6], false, false));
        let mut input_effect = 0.0f32;
        for frame in 0..4096 {
            let input = (frame as f32 * 0.031).sin() * 0.25;
            let mixed = mono.process(input, -input);
            assert_eq!(mixed, silent.process(0.0, 0.0));
            assert_eq!(mixed[0], mixed[1]);
            let dry_right = stereo_silent.process(0.0, 0.0)[1];
            let out = stereo.process(input, 0.0);
            assert_eq!(out[1], dry_right);
            input_effect = input_effect.max((out[0] - dry_right).abs());
        }
        assert!(input_effect > 1e-4, "input must reach the circuit output");
    }

    #[test]
    fn jump_control_change_ramps_and_settles_without_resetting_circuit() {
        let mut amp = JumpAmp::new(settings([0.2; 6], false, false));
        for _ in 0..64 {
            amp.process(0.05, -0.1);
        }
        let before = amp.channels[0].0;
        amp.set_settings(settings([0.8; 6], true, false));
        assert_eq!(amp.channels[0].0, before);
        let start_drive = amp.smooth.currents[19];
        let target_drive = amp.smooth.targets[19];
        assert!(start_drive < target_drive);
        amp.process(0.05, -0.1);
        assert!(amp.smooth.currents[19] > start_drive && amp.smooth.currents[19] < target_drive);
        for _ in 0..1056 {
            let output = amp.process(0.05, -0.1);
            assert!(output.iter().all(|v| v.is_finite()));
        }
        assert_eq!(amp.smooth.currents, amp.smooth.targets);
        assert!(!amp.refresh && !amp.smooth.pending && !amp.targets_pending);
    }
}
