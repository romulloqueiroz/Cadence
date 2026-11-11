//! JUMP control targets and operating point, recovered from the saved native
//! constructor, Initialize, Reset, SetJump and BParFX::SetObj bodies.
//!
//! This helper builds a settled control state. It does not replace Process's
//! live 32-frame smoothing schedule or the base effect's volume/bypass path.

use crate::amp_dsp::{exp_index, jump_eq_filters, jump_tables, JumpRateCoefficients, TableBase};
use nk_format::fx::AmpSettings;

pub(crate) struct JumpControlState {
    /// Native +0x45b0..+0x460c targets, snapped to currents by Reset.
    pub targets: [f32; 24],
    pub rate_coefficients: JumpRateCoefficients,
    /// SetJump's final boolean (+0x1c0): average inputs and process one channel.
    pub mono: bool,
}

fn drive_curve(drive: f32) -> f32 {
    if drive <= 0.1 {
        0.001 + 0.09 * drive
    } else if drive <= 0.4 {
        -0.014333333 + 0.24333332 * drive
    } else if drive <= 0.6 {
        -0.191 + 0.685 * drive
    } else if drive <= 0.9 {
        -1.26 + 2.4666667 * drive
    } else {
        0.96
    }
}

impl JumpControlState {
    /// Recover the parameter permutation from BParFX::SetObj (0x10040cefc),
    /// rather than assigning GUI meanings from serialized position. The six
    /// wire floats map to SetJump arguments [0,1,5,4,3,2].
    pub(crate) fn build(settings: AmpSettings, rate: f32) -> Self {
        let values = settings.values();
        let at = |index: usize| values.get(index).copied().unwrap_or(0.5);
        let drive = at(0);
        let presence = at(1).min(0.95);
        let p3 = at(5);
        let p4 = at(4);
        let p5 = at(3);
        let level = at(2);
        let boost = settings.flags().first().copied().unwrap_or(false);
        let mono = settings.flags().get(1).copied().unwrap_or(false);
        let input_scale = 1.0;
        let power_scale = f32::from_bits(0x3d0186e3);
        // Initialize ends with setParamResponse(0), not its earlier temporary
        // 0.5 call. That setter maps 0 to -24 dB and leaves a -0 offset.
        let response_db = -24.0;
        let response_offset = -0.0;
        let channel = false;
        let drive_target = drive_curve(drive);
        let rate_coefficients = JumpRateCoefficients::new(
            rate,
            drive_target,
            input_scale,
            power_scale,
            response_db,
            response_offset,
            channel,
        );
        let eq = jump_eq_filters(p3, p4, p5, presence, rate_coefficients.base.rate_scale);
        let mut targets = [0.0; 24];
        targets[0] = eq.eq_c0;
        targets[1] = eq.eq_c1;
        targets[3] = f32::from_bits(0x3e3cb242);
        targets[4] = eq.eq_b0;
        targets[5] = eq.eq_b1;
        targets[7] = f32::from_bits(0x3ee4b3b6);
        targets[8] = if boost { 0.0 } else { input_scale };
        targets[10] = eq.f_d8;
        targets[11] = eq.f_dc;
        targets[14] = eq.f_e8;
        targets[15] = eq.f_ec;
        targets[16] = eq.f_f0;
        targets[17] = eq.f_f4;
        targets[18] = eq.f_f8;
        targets[19] = drive_target;
        let output_scale = f32::from_bits(0x3a7b892f);
        targets[20] = if boost {
            output_scale
        } else {
            output_scale * 1.5848932
        };
        if boost {
            let x = drive * 0.9 + 0.1;
            let db = x * (x * (x * (x * (x * 179.49 - 548.95) + 633.8) - 352.38) + 105.46) - 17.633;
            let amount = exp_index(db * 9.965784 + 1200.0);
            targets[2] = (1.0 - amount) * 0.94406086;
            targets[6] = amount * 0.94406086;
        }
        let level_db = if level > 0.8 {
            level * 2.5 - 1.1
        } else if level > 0.35 {
            level * 2.0 - 0.7
        } else {
            0.0
        };
        targets[12] = exp_index(level_db * 9.965784 + 1200.0) - 1.0;
        let x = level * 0.8 + 0.1;
        targets[13] =
            x * (x * (x * (x * (x * -14.011 + 28.594) - 22.506) + 7.8023) - 1.3704) + 0.87383 - 1.0;
        let x = x.max(0.2);
        targets[9] = ((x * (x * (x * (x * (x * 15734.0 - 37019.0) + 33888.0) - 14634.0) + 3046.2)
            - 179.33)
            * rate_coefficients.base.rate_scale)
            .min(1.0);
        Self {
            targets,
            rate_coefficients,
            mono,
        }
    }

    /// Write native float coefficient offsets into a block based at +0x4630.
    /// The runtime's fixed FIR vectors in +0x46b0..+0x472c are left intact.
    pub(crate) fn write_coefficients(&self, coefficients: &mut [f32; 256]) {
        let mut put = |offset: usize, values: &[f32]| {
            let index = (offset - 0x4630) / 4;
            coefficients[index..index + values.len()].copy_from_slice(values);
        };
        for (index, row) in self.rate_coefficients.prototypes.iter().enumerate() {
            put(0x4630 + index * 16, row);
        }
        for (index, row) in self.rate_coefficients.circuit.iter().enumerate() {
            put(0x4730 + index * 16, row);
        }
        for (index, row) in self.rate_coefficients.base.vectors().iter().enumerate() {
            put(0x48d0 + index * 16, row);
        }
        // ProcessInt's first refresh sets the paired-integrator rate/damping
        // from the smoothed EQ targets, even when they have already snapped.
        put(0x4910, &[self.targets[17]]);
        put(0x4900, &[self.targets[18]]);
        put(0x49c0, &self.rate_coefficients.response);
        let tables = jump_tables();
        let power = TableBase::power_tube();
        let bank_a = [&tables[0], &tables[0], &tables[1], &tables[2]];
        let bank_b = [&power, &power, &tables[3], &tables[4]];
        put(
            0x4930,
            &[
                tables[0].scale * 1.2589254,
                tables[0].scale * 1.2589254,
                tables[1].scale,
                tables[2].scale * 1.584894,
            ],
        );
        put(
            0x4940,
            &[
                power.scale * 0.89,
                power.scale * 0.89,
                tables[3].scale,
                tables[4].scale,
            ],
        );
        for (offset, bank) in [(0x4950, bank_a), (0x4960, bank_b)] {
            put(offset, &bank.map(|table| table.fraction));
            put(
                offset + 0x20,
                &bank.map(|table| 255.0 - table.anchor as f32),
            );
            put(offset + 0x40, &bank.map(|table| -(table.anchor as f32)));
        }
        // Initialize's final supply-bias assignment after response(0) and
        // voltage modulation(0). Keep the source multiply/divide grouping.
        let bias = ((power.scale * 0.89) * -37.8) / 0.89;
        put(0x49b0, &[bias, bias, 0.0, 0.0]);
        put(0x49c8, &[f32::from_bits(0x3b0767ab), 484.0]);
        put(0x49d0, &[f32::from_bits(0x3a7b892f)]);
    }

    /// Initial channel operating point at +0x1d0..+0x2ec. These are Reset's
    /// circuit seeds with Initialize's 413 V supply and SetSampleRate's clock.
    pub(crate) fn reset_channel(&self) -> [f32; 72] {
        let mut state = [0.0; 72];
        let mut put = |offset: usize, value: f32| state[(offset - 0x1d0) / 4] = value;
        put(0x280, f32::from_bits(0x41e44c30));
        put(0x284, f32::from_bits(0x41e56f9e));
        put(0x298, f32::from_bits(0x43c514fe));
        put(0x2b0, f32::from_bits(0x43abad91));
        put(0x2b4, f32::from_bits(0x4391ed0e));
        put(0x2b8, f32::from_bits(0x438c16c9));
        put(0x2bc, jump_tables()[0].offset);
        put(0x2c0, 0.00291243);
        put(0x2c4, 0.00342639);
        put(0x2c8, 0.00356916);
        put(0x2cc, 0.0029539);
        put(0x2e0, f32::from_bits(0x3f374eba));
        put(0x2e4, self.rate_coefficients.base.state_gain);
        put(0x2e8, 413.0);
        put(0x2ec, 395.695);
        state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nk_format::fx::AmpKind;

    fn settings(values: [f32; 6], boost: bool, mono: bool) -> AmpSettings {
        let mut wire = AmpSettings {
            kind: AmpKind::Jump,
            values: [0.0; 17],
            value_count: 6,
            flags: [false; 8],
            flag_count: 2,
            modes: [0; 4],
        };
        wire.values[..6].copy_from_slice(&values);
        wire.flags[..2].copy_from_slice(&[boost, mono]);
        wire
    }

    #[test]
    fn jump_wire_level_is_third_float_and_eq_order_is_reversed() {
        let a = JumpControlState::build(
            settings([0.5, 0.5, 0.0, 0.2, 0.4, 0.8], false, false),
            48000.0,
        );
        // SetJump's sixth float controls level; BParFX::SetObj supplies wire
        // float 2 there. A level <=.35 has an exactly zero gain increment.
        assert_eq!(a.targets[12], 0.0);
        let b = JumpControlState::build(
            settings([0.5, 0.5, 0.9, 0.2, 0.4, 0.8], false, false),
            48000.0,
        );
        assert!(b.targets[12] > 0.0);
        // Changing only wire level must leave the tone-filter targets alone.
        for i in [0, 1, 4, 5, 10, 11, 14, 15, 16, 17, 18] {
            assert_eq!(a.targets[i], b.targets[i]);
        }
        let eq = jump_eq_filters(0.8, 0.4, 0.2, 0.5, a.rate_coefficients.base.rate_scale);
        assert_eq!(a.targets[4], eq.eq_b0);
        assert_eq!(a.targets[5], eq.eq_b1);
        assert_eq!(a.targets[0], eq.eq_c0);
        assert_eq!(a.targets[1], eq.eq_c1);
    }

    #[test]
    fn jump_boost_and_mono_wire_flags_reach_distinct_native_controls() {
        let normal = JumpControlState::build(settings([0.5; 6], false, false), 48000.0);
        let boost = JumpControlState::build(settings([0.5; 6], true, false), 48000.0);
        let mono = JumpControlState::build(settings([0.5; 6], false, true), 48000.0);
        assert_eq!(normal.targets[8], 1.0);
        assert_eq!(boost.targets[8], 0.0);
        assert_eq!(normal.targets[2], 0.0);
        assert_eq!(normal.targets[6], 0.0);
        assert!((boost.targets[2] + boost.targets[6] - 0.94406086).abs() < 1e-7);
        assert_eq!(normal.targets[20], boost.targets[20] * 1.5848932);
        assert!(!normal.mono && !boost.mono && mono.mono);
        assert_eq!(normal.targets, mono.targets);
    }

    #[test]
    fn jump_reset_preserves_nonzero_circuit_operating_point() {
        let control = JumpControlState::build(settings([0.5; 6], false, false), 48000.0);
        let state = control.reset_channel();
        let at = |offset: usize| state[(offset - 0x1d0) / 4];
        assert_eq!(at(0x2e8), 413.0);
        assert_eq!(at(0x2ec), 395.695);
        assert_eq!(at(0x2bc), jump_tables()[0].offset);
        assert_eq!(at(0x2e4).to_bits(), 0x3a088888);
        assert_eq!(at(0x280).to_bits(), 0x41e44c30);
        assert_eq!(at(0x284).to_bits(), 0x41e56f9e);
        assert_eq!(at(0x298).to_bits(), 0x43c514fe);
        for offset in [0x2b0, 0x2b4, 0x2b8] {
            assert!(at(offset) > 250.0);
        }
    }
}
