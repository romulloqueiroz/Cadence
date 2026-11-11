//! NI wave-shaping table foundation for the amp family.
//!
//! `TableBase::setupTable` (VAN51 image alias `0x1005caad0`, generic image
//! `0x101b8c140`) turns a 256-point `double` stage curve into the relative
//! value/slope table used by the amp `ProcessInt` bodies. The six JCM800 JUMP
//! curves are embedded verbatim. `jump_dsp` applies the coupled, oversampled
//! circuit; see `docs/kontakt-amp-engine.md`.
#![allow(dead_code)] // foundation for the upcoming ProcessInt port
use std::sync::OnceLock;

const CURVES: &[u8; 12_288] = include_bytes!("jump_curves.bin");
const VAN51_CURVES: &[u8; 24_576] = include_bytes!("van51_curves.bin");
const ENTRIES: usize = 256;

pub(crate) const JUMP_BOOST: usize = 0;
pub(crate) const JUMP_STAGE1: usize = 1;
pub(crate) const JUMP_STAGE2: usize = 2;
pub(crate) const JUMP_SPLITTER_P: usize = 3;
pub(crate) const JUMP_SPLITTER_N: usize = 4;
pub(crate) const JUMP_POWER_TUBE: usize = 5;

pub(crate) const VAN51_STAGE1: usize = 0;
pub(crate) const VAN51_STAGE2_RHYTHM: usize = 1;
pub(crate) const VAN51_STAGE2_LEAD: usize = 2;
pub(crate) const VAN51_STAGE3_RHYTHM: usize = 3;
pub(crate) const VAN51_STAGE3_LEAD: usize = 4;
pub(crate) const VAN51_STAGE4: usize = 5;
pub(crate) const VAN51_STAGE5: usize = 6;
pub(crate) const VAN51_STAGE6: usize = 7;
pub(crate) const VAN51_STAGE8: usize = 8;
pub(crate) const VAN51_SPLITTER1: usize = 9;
pub(crate) const VAN51_SPLITTER2: usize = 10;
pub(crate) const VAN51_POWER_TUBE: usize = 11;

fn curve_at(curves: &[u8], index: usize, position: usize) -> f64 {
    let start = (index * ENTRIES + position) * 8;
    f64::from_le_bytes(curves[start..start + 8].try_into().unwrap())
}

fn curve(index: usize, position: usize) -> f64 {
    curve_at(CURVES, index, position)
}

#[derive(Clone)]
pub(crate) struct TableBase {
    pub anchor: i32,
    pub fraction: f32,
    pub scale: f32,
    pub offset: f32,
    pub baseline: f32,
    /// `(value - baseline, next - value)` per curve point.
    pub entries: [[f32; 2]; ENTRIES],
}

impl TableBase {
    /// Recovered `TableBase::setupTable(bool /*curve*/, float, float, float,
    /// unsigned flags, int has_offset)`.
    pub(crate) fn setup(
        curve_index: usize,
        param1: f32,
        param2: f32,
        param3: f32,
        flag: bool,
        has_offset: bool,
    ) -> Self {
        Self::setup_with(
            CURVES,
            curve_index,
            param1,
            param2,
            param3,
            flag,
            has_offset,
        )
    }

    pub(crate) fn setup_with(
        curves: &[u8],
        curve_index: usize,
        param1: f32,
        param2: f32,
        param3: f32,
        flag: bool,
        has_offset: bool,
    ) -> Self {
        let scaled = param2 * 256.0 * param3;
        let offset = if has_offset { scaled } else { 0.0 };
        let scale = param2 * 256.0;
        let position = param1 * 256.0 - offset;

        let mut baseline = 0.0f64;
        if flag {
            let target = position as f64;
            let increment = offset as f64;
            loop {
                let raw = target + baseline * increment;
                let index = if raw < 0.0 {
                    0u32
                } else if raw > u32::MAX as f64 {
                    u32::MAX
                } else {
                    raw as u32
                };
                let limit = index.min(255) as usize;
                baseline = curve_at(curves, curve_index, limit);
                let (low, high) = if index < 255 {
                    (
                        curve_at(curves, curve_index, limit),
                        curve_at(curves, curve_index, limit + 1),
                    )
                } else {
                    (
                        curve_at(curves, curve_index, 254),
                        curve_at(curves, curve_index, 255),
                    )
                };
                let fraction = ((target + increment * baseline) - limit as f64)
                    / (1.0 - increment * (high - low));
                baseline += (high - low) * fraction;
                if !((fraction < 0.0 && limit != 0) || (fraction >= 1.0 && index < 255)) {
                    break;
                }
            }
        }

        let final_position = position + (baseline * offset as f64) as f32;
        let anchor = (final_position + 0.5) as i32;
        let fraction = final_position - anchor as f32;
        let baseline_f = baseline as f32;

        let mut entries = [[0.0f32; 2]; ENTRIES];
        let mut current = curve_at(curves, curve_index, 0);
        for (index, entry) in entries.iter_mut().enumerate().take(255) {
            let next = curve_at(curves, curve_index, index + 1);
            entry[0] = (current - baseline) as f32;
            entry[1] = (next - current) as f32;
            current = next;
        }
        entries[255][0] = (current - baseline) as f32;
        entries[255][1] =
            (curve_at(curves, curve_index, 255) - curve_at(curves, curve_index, 254)) as f32;

        Self {
            anchor,
            fraction,
            scale,
            offset: scaled,
            baseline: baseline_f,
            entries,
        }
    }

    /// The power-tube curve is installed manually by each amp's
    /// `Initialize` instead of calling `setupTable`.
    pub(crate) fn power_tube() -> Self {
        Self::manual(CURVES, JUMP_POWER_TUBE, 0x101, 0xBE6D_9000, 0x4525_9F6C)
    }

    /// Manual power-tube build: `rel = value`, `slope = next - value`.
    pub(crate) fn manual(
        curves: &[u8],
        curve_index: usize,
        anchor: i32,
        fraction_bits: u32,
        scale_bits: u32,
    ) -> Self {
        let mut entries = [[0.0f32; 2]; ENTRIES];
        let mut current = curve_at(curves, curve_index, 0);
        for (index, entry) in entries.iter_mut().enumerate().take(255) {
            let next = curve_at(curves, curve_index, index + 1);
            entry[0] = current as f32;
            entry[1] = (next - current) as f32;
            current = next;
        }
        entries[255][0] = current as f32;
        entries[255][1] =
            (curve_at(curves, curve_index, 255) - curve_at(curves, curve_index, 254)) as f32;
        Self {
            anchor,
            fraction: f32::from_bits(fraction_bits),
            scale: f32::from_bits(scale_bits),
            offset: 0.0,
            baseline: 0.0,
            entries,
        }
    }
}

/// The eleven `setupTable` calls plus the manual power-tube build made by
/// `BEffectVan51::Initialize`, in curve-blob order.
pub(crate) fn van51_tables() -> &'static [TableBase; 12] {
    static TABLES: OnceLock<[TableBase; 12]> = OnceLock::new();
    TABLES.get_or_init(|| {
        const SETUPS: [(usize, u32, u32, u32, bool); 11] = [
            (VAN51_STAGE1, 0x3F04_5A1D, 0x4234_BE0C, 0x3C6F_4AEA, true),
            (
                VAN51_STAGE2_RHYTHM,
                0x3ED6_872B,
                0x415F_6B40,
                0x3CD5_5338,
                true,
            ),
            (VAN51_STAGE2_LEAD, 0x3EDB_22D1, 0x4145_FAB9, 0, true),
            (VAN51_STAGE3_RHYTHM, 0x3F45_A1CB, 0x3F80_0000, 0, true),
            (VAN51_STAGE3_LEAD, 0x3F2B_020C, 0x3FA4_E55C, 0, true),
            (VAN51_STAGE4, 0x3F19_999A, 0x41D4_DBA6, 0, true),
            (VAN51_STAGE5, 0x3ECF_DF3B, 0x41F7_3940, 0, true),
            (VAN51_STAGE6, 0x3E9A_1CAC, 0x4005_B707, 0, true),
            (VAN51_STAGE8, 0x3EB6_45A2, 0x41F7_3940, 0x3CEE_7E2E, true),
            (VAN51_SPLITTER1, 0x3ED6_872B, 0x41BF_E819, 0, true),
            (VAN51_SPLITTER2, 0x3EFC_6A7F, 0x41BF_E819, 0, true),
        ];
        let mut tables: [TableBase; 12] = std::array::from_fn(|_| TableBase::power_tube());
        for (index, (curve, p1, p2, p3, flag)) in SETUPS.into_iter().enumerate() {
            tables[index] = TableBase::setup_with(
                VAN51_CURVES,
                curve,
                f32::from_bits(p1),
                f32::from_bits(p2),
                f32::from_bits(p3),
                flag,
                false,
            );
        }
        tables[VAN51_POWER_TUBE] = TableBase::manual(
            VAN51_CURVES,
            VAN51_POWER_TUBE,
            0x178,
            0xBE44_9800,
            0x4404_2F30,
        );
        tables
    })
}

/// The five `setupTable` calls made by `BEffectJump::Initialize`.
pub(crate) fn jump_tables() -> &'static [TableBase; 5] {
    static TABLES: OnceLock<[TableBase; 5]> = OnceLock::new();
    TABLES.get_or_init(|| {
        [
            TableBase::setup(
                JUMP_BOOST,
                f32::from_bits(0x3F26_24DD),
                f32::from_bits(0x4243_E95A),
                f32::from_bits(0x3CDD_2F1B),
                true,
                false,
            ),
            TableBase::setup(
                JUMP_STAGE1,
                f32::from_bits(0x3E8D_D2F2),
                f32::from_bits(0x4131_77CD),
                0.0,
                true,
                true,
            ),
            TableBase::setup(
                JUMP_STAGE2,
                f32::from_bits(0x3F5D_2F1B),
                f32::from_bits(0x427C_6208),
                0.0,
                true,
                true,
            ),
            TableBase::setup(
                JUMP_SPLITTER_P,
                f32::from_bits(0x3F27_AE14),
                f32::from_bits(0x4201_6FFE),
                0.0,
                true,
                true,
            ),
            TableBase::setup(
                JUMP_SPLITTER_N,
                f32::from_bits(0x3ED8_9375),
                f32::from_bits(0x41D4_DBA6),
                0.0,
                true,
                true,
            ),
        ]
    })
}

pub(crate) fn exp_index(x: f32) -> f32 {
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

fn clamp_unit(value: f32) -> f32 {
    if value > 1.0 {
        1.0
    } else {
        value
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct JumpEqControls {
    pub eq_b0: f32,
    pub eq_b1: f32,
    pub eq_c0: f32,
    pub eq_c1: f32,
    pub f_d8: f32,
    pub f_dc: f32,
    pub f_e8: f32,
    pub f_ec: f32,
    pub f_f0: f32,
    pub f_f4: f32,
    pub f_f8: f32,
}

/// Recovered `BEffectJump::setEQFilters` (`0x1005cd29c`).
pub(crate) fn jump_eq_filters(
    p3: f32,
    p4: f32,
    p5: f32,
    presence: f32,
    rate_scale: f32,
) -> JumpEqControls {
    let f3 = p3;
    let f5 = p4;
    let f9 = p5;
    let mut f6 = (f3 - 0.3) + f3 - 0.3;
    let mut f8 = if f6 < 0.0 { 0.0 } else { f6 };
    f6 = if f8 > 1.0 { 1.0 } else { f8 };
    f8 = f6 * -20.0 + 35.0;
    f6 = f8 + ((f6 * -24.0 + 39.0) - f8) * f5;
    f6 = f6 * 5.0 + 1381.8816;
    f8 = exp_index(f6);

    let mut f7 = (presence - 1.0) * 45.7245 * 9.965784 + 1200.0;
    f7 = exp_index(f7);

    let mut ratio = f5 / 0.8;
    ratio = if ratio > 1.0 { 1.0 } else { ratio };
    let f10 = if f3 > 0.5 { 0.5 } else { f3 };

    let mut f12 = f5 * 0.8 + 0.1;
    f12 = f12 * (f12 * (f12 * 0.42947 - 1.1055) + 1.249) - 0.10895;

    let f16 = f3 * (f3 * (f3 * -1511.7 + 3512.3) - 2636.5) + 946.3;
    let f17 = f3 * -61.350513 + 87.30706;
    f6 = (f7 - 0.0051733875) / 0.9948266;

    let eq_b0 = f12 * 1.1220185 * (1.0 - f9) * f6;
    let f14 = f3 * (f3 * (f3 * (f3 * 1.4336 - 3.0688) + 1.415) + 0.75803) + 0.26767;
    let f15 = f5 * (f5 * (f5 * (f5 * 1.2364 - 3.0531) + 2.3111) - 0.23654) + 0.16086;
    let eq_b1 = f9 * 0.84139514 * f6;

    let poly = f3 * (f3 * (f3 * (f3 * 1.1243 - 2.4493) + 1.1882) + 0.5478) + 0.40344;
    let eq_c0 = (((f14 + f5 * (poly - f14)) - f12) * 1.1220185) * (1.0 - f9) * f6;
    let eq_c1 = (((f15 + f3 * (0.63095737 - f15)) - 1.0) * 0.84139514) * f9 * f6;

    let value = (f17 + f5 * ((f3 * -43.339115 + 69.295654) - f17)) * rate_scale;
    let f_d8 = clamp_unit(value);
    let f_dc = clamp_unit(f8 * rate_scale);

    let poly = f3 * (f3 * (f3 * -998.45 + 2766.8) - 2538.9) + 947.65;
    let f_e8 = clamp_unit((f16 + f5 * (poly - f16)) * rate_scale);
    let f_ec =
        clamp_unit((f10 * (f10 * (f10 * 3140.39 - 1847.82) - 507.476) + 469.976) * rate_scale);

    let f9 = (ratio * -99.00284 + 622.25397) * rate_scale;
    let f_f4 = if f9 > 1.0 { 1.0 } else { f9 };
    let part1 = ratio * (ratio * -0.16689 + 0.42032) + 0.28409;
    let b = ratio * (ratio * (ratio * (ratio * 4.2561 - 9.8707) + 8.1957) - 2.8836) - 0.23847;
    let c = ratio
        * (ratio * (ratio * (ratio * (ratio * 55.338 - 156.56) + 161.2) - 73.88) + 14.981)
        - 0.47557;
    let d = ratio
        * (ratio * (ratio * (ratio * (ratio * -42.201 + 120.15) - 124.14) + 56.677) - 11.296)
        + 0.57709;
    let f_f0 = part1 + f3 * (b + f3 * (c + f3 * d));
    let f_f8 = 1.0 / (f5 * -0.04 + 0.22);

    JumpEqControls {
        eq_b0,
        eq_b1,
        eq_c0,
        eq_c1,
        f_d8,
        f_dc,
        f_e8,
        f_ec,
        f_f0,
        f_f4,
        f_f8,
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct JumpResponse {
    pub f18: f32,
    pub f20: f32,
    pub f24: f32,
    pub f28: f32,
    pub f_c4: f32,
    pub f_c0: f32,
}

/// Recovered `BEffectJump::setParamResponse` (`0x1005cac6c`).
pub(crate) fn jump_param_response(
    response: f32,
    rate_scale: f32,
    rate_inv: f32,
    offset: f32,
) -> JumpResponse {
    let f4 = response * 48.0 - 24.0;
    let f7 = exp_index(f4 * 5.0 + 1381.8816);
    let f6 = rate_scale;
    let f18 = (f7 * f6).min(1.0);

    let index = (f4 - 56.0) * 5.0 + 1381.8816;
    let base = if index >= 2400.0 {
        20.0f64.exp2() as f32
    } else if index < 0.0 {
        0.0
    } else {
        exp_index(index)
    };
    let f20 = if index < 0.0 {
        0.0
    } else {
        (f6 * base).min(1.0)
    };
    let f24 = f20;
    let f28 = (f6 * base).min(1.0);

    let f_c4 = exp_index((f4 * 0.5 - 40.0) * 9.965784 + 1200.0) * -1000.0 * rate_inv;
    let f_c0 = exp_index((f4 * 0.5 - 13.0 + offset) * 9.965784 + 1200.0) * rate_inv * 1000.0;
    JumpResponse {
        f18,
        f20,
        f24,
        f28,
        f_c4,
        f_c0,
    }
}

/// `NI::AB::SmoothingToolbox<24>` (`0x100615f08`) as used by
/// `BEffectJump::Process`. Currents sit at toolbox `+0x10`, increments at
/// `+0x70`, targets at `+0xd0`; `+0x131` is the pending flag, `+0x134` the
/// remaining block count, `+0x138` the period count, `+0x13c` the factor and
/// `+0x140` the state (2 smoothing, 4 done).
#[derive(Clone)]
pub(crate) struct Smoothing24 {
    pub currents: [f32; 24],
    pub increments: [f32; 24],
    pub targets: [f32; 24],
    pub factor: f32,
    pub periods: i32,
    pub remaining: i32,
    pub pending: bool,
    pub state: i32,
}

impl Default for Smoothing24 {
    fn default() -> Self {
        Self {
            currents: [0.0; 24],
            increments: [0.0; 24],
            targets: [0.0; 24],
            factor: 0.0,
            periods: 0,
            remaining: 0,
            pending: false,
            state: 4,
        }
    }
}

impl Smoothing24 {
    pub(crate) fn start_smooth(&mut self) {
        let mut total = 0.0f32;
        for index in 0..24 {
            let delta = (self.targets[index] - self.currents[index]) * self.factor;
            self.increments[index] = delta;
            let square = delta * delta;
            if square < 1e-15 {
                self.increments[index] = 0.0;
                self.currents[index] = self.targets[index];
            } else {
                total += square;
            }
        }
        if total <= 0.0 {
            self.state = 4;
        } else {
            self.remaining = self.periods;
            self.pending = true;
        }
    }

    /// One internal oversample: add the stored increment to each current.
    pub(crate) fn advance(&mut self) {
        for (current, increment) in self.currents.iter_mut().zip(self.increments.iter()) {
            *current += *increment;
        }
    }

    /// Called when the block countdown reaches zero: snap to the targets.
    pub(crate) fn snap(&mut self) {
        self.currents = self.targets;
        self.increments = [0.0; 24];
        self.pending = false;
        self.state = 4;
    }
}

/// Fixed ExpTable lookups used by `BEffectJump::SetSampleRate` (`0x1005caf30`).
/// The image reads `value + slope*0.8815918` at these table indices; the BSS
/// "tuning constants" are therefore recoverable from the reconstructed table.
#[derive(Clone, Copy, Default)]
pub(crate) struct JumpTuning {
    pub c5e58: f32,
    pub c5388: f32,
    pub c5390: f32,
    pub c63d0: f32,
    pub c6060: f32,
    pub c6178: f32,
    pub c5890: f32,
    pub c6128: f32,
    pub c6150: f32,
}

impl JumpTuning {
    pub(crate) fn new() -> Self {
        let at = |index: i32| exp_index(index as f32 + 0.8815918);
        Self {
            c5e58: at(1927),
            c5388: at(1585),
            c5390: at(1586),
            c63d0: at(2106),
            c6060: at(1996),
            c6178: at(2031),
            c5890: at(1746),
            c6128: at(2021),
            c6150: at(2026),
        }
    }
}

/// Rate-dependent part of `BEffectJump::SetSampleRate` (`0x1005caf30`):
/// the `+0x48d0..+0x4928` smoothing/clamp block. `JumpRateCoefficients`
/// contains the bulk circuit coefficients computed from this rate.
#[derive(Clone, Copy, Default)]
pub(crate) struct JumpRateBase {
    pub period: i32,
    pub smooth_factor: f32,
    pub rate_scale: f32,
    pub rate_inv: f32,
    pub rate_x4: f32,
    pub d0: f32,
    pub d4: f32,
    pub d8: f32,
    pub dc: f32,
    pub e0: f32,
    pub e8: f32,
    pub ec: f32,
    pub f0: f32,
    pub f4: f32,
    pub f8: f32,
    pub fc: f32,
    pub f918: f32,
    pub f920: f32,
    pub f924: f32,
    pub f928: f32,
    pub state_gain: f32,
}

impl JumpRateBase {
    /// Four-lane rows at +0x48d0, +0x48e0, ... +0x4920, including
    /// the zeroed response histories and fixed +0x4908 blend.
    pub(crate) fn vectors(&self) -> [[f32; 4]; 6] {
        [
            [self.d0, self.d4, self.d8, self.dc],
            [self.e0, 0.0, self.e8, self.ec],
            [self.f0, self.f4, self.f8, self.fc],
            [0.0, 0.0, f32::from_bits(0x3f2e147a), 0.0],
            [0.0, 0.0, self.f918, 0.0],
            [self.f920, self.f924, self.f928, 0.0],
        ]
    }

    #[allow(clippy::approx_constant)]
    pub(crate) fn new(rate: f32, response: f32, channel: bool) -> Self {
        let rate_x4 = rate * 4.0;
        let period = ((rate_x4 * 0.02 * 0.0078125 + 0.5) as i32).max(1);
        let smooth_factor = 1.0 / (period as f32 * 128.0);
        let rate_scale = (1.0 / rate_x4) * 6.283;
        let rate_inv = 1.0 / rate_x4;
        let t = JumpTuning::new();
        let unit = |value: f32| if value > 1.0 { 1.0 } else { value };

        let d0 = unit(t.c5390 * rate_scale);
        let d4 = unit(t.c63d0 * rate_scale);
        let d8 = unit(t.c6060 * rate_scale);
        let dc = unit(t.c6178 * rate_scale);
        let e8 = unit(t.c5e58 * rate_scale);
        let ec = unit(t.c5388 * rate_scale);
        let e0 = unit(t.c5890 * rate_scale);
        let f0 = unit(t.c6128 * rate_scale);
        let f4 = f0;
        let f8 = unit(t.c6150 * rate_scale);
        let fc = f8;

        let response1 = exp_index(response * 5.0 + 1381.8816);
        let f918 = unit(rate_scale * response1);
        let value2 = exp_index((response - 56.0) * 5.0 + 1381.8816);
        let f920 = unit(rate_scale * value2);
        let f924 = f920;
        let f928 = f920;

        let state_gain = rate_inv * if channel { 120.0 } else { 100.0 };
        Self {
            period,
            smooth_factor,
            rate_scale,
            rate_inv,
            rate_x4,
            d0,
            d4,
            d8,
            dc,
            e0,
            e8,
            ec,
            f0,
            f4,
            f8,
            fc,
            f918,
            f920,
            f924,
            f928,
            state_gain,
        }
    }
}

/// Circuit coefficient vectors written by `BEffectJump::SetSampleRate`.
///
/// The native engine runs four oversamples per host frame. Its bilinear
/// transform uses twice that internal rate (`8 * host_rate`). Rows preserve
/// native offsets and lane order: the two four-lane circuit groups are not a
/// six-stage cascade. The FIR vectors in the gap `+0x46b0..+0x472c` are fixed
/// by Initialize and are deliberately not part of this rate-dependent block.
#[derive(Clone)]
pub(crate) struct JumpRateCoefficients {
    pub base: JumpRateBase,
    /// Existing target snapped to the native +0x453c current before the rebuild.
    pub drive_current: f32,
    /// Eight four-float prototype vectors at +0x4630..+0x46ac.
    pub prototypes: [[f32; 4]; 8],
    /// Twenty-three four-float circuit vectors at +0x4730..+0x489c.
    pub circuit: [[f32; 4]; 23],
    /// +0x49c0 / +0x49c4 (positive / negative response gains).
    pub response: [f32; 2],
}

impl JumpRateCoefficients {
    /// Inputs are native setter fields, not normalized UI parameters:
    /// drive_target=+0x45fc, input_scale=+0x44c8, power_scale=+0x44d0,
    /// response_db=+0x44b8, response_offset=+0x44bc, channel=+0x44c4.
    pub(crate) fn new(
        rate: f32,
        drive_target: f32,
        input_scale: f32,
        power_scale: f32,
        response_db: f32,
        response_offset: f32,
        channel: bool,
    ) -> Self {
        let base = JumpRateBase::new(rate, response_db, channel);
        let k = base.rate_x4 + base.rate_x4;
        let d = drive_target;
        let prototypes = [
            [k * 690.9 + 1.47e6, 2.94e6, k * -690.9 + 1.47e6, 0.0],
            [
                (k * 0.2209 + 530.0) * k,
                k * -0.4418 * k,
                (k * 0.2209 - 530.0) * k,
                0.0,
            ],
            [
                (k * 0.2209 + 1220.9) * k + 1e6,
                k * -0.4418 * k + 2e6,
                (k * 0.2209 - 1220.9) * k + 1e6,
                0.0,
            ],
            [k * -6.909e8 - 1.47e12, -2.94e12, k * 6.909e8 - 1.47e12, 0.0],
            [
                (k * -0.2209 - 1000.0) * k,
                k * 0.4418 * k,
                (k * -0.2209 + 1000.0) * k,
                0.0,
            ],
            [
                (k * -0.2209 - 1000.0) * k,
                k * 0.4418 * k,
                (k * -0.2209 + 1000.0) * k,
                0.0,
            ],
            [k * 6.9090003e8 + 1e12, 2e12, k * -6.9090003e8 + 1e12, 0.0],
            [
                f32::from_bits(0x471b82b3),
                f32::from_bits(0x418b3333),
                f32::from_bits(0xc18b3333),
                f32::from_bits(0xc18b3333),
            ],
        ];
        let mut circuit = [[0.0; 4]; 23];
        let row = |offset: usize| (offset - 0x4730) / 16;
        let denom_a = [
            1.0 + k * 6.82176e-6,
            1.0 + k * 6.82176e-6,
            prototypes[0][0] + d * (d * prototypes[4][0] + prototypes[1][0]),
            1.0 + k * (k * 2.284106e-6 + 0.0209009),
        ];
        let feedback_a = [
            -68000.0,
            -68000.0,
            d * (d * prototypes[6][0] + prototypes[3][0]),
            k * -4963.623 - 470000.0,
        ];
        circuit[row(0x4730)] = [
            1.0,
            1.0,
            d * (d * prototypes[4][0] + prototypes[2][0]),
            k * (k * 2.284106e-6 + 0.01034),
        ];
        circuit[row(0x4810)] = [
            k * -6.82176e-6 + 1.0,
            k * -6.82176e-6 + 1.0,
            prototypes[0][1] + d * (d * prototypes[4][1] + prototypes[1][1]),
            k * (k * -4.568212e-6) + 2.0,
        ];
        circuit[row(0x4830)] = [
            1.0,
            1.0,
            d * (d * prototypes[4][1] + prototypes[2][1]),
            k * (k * -4.568212e-6),
        ];
        circuit[row(0x4850)] = [
            -68000.0,
            -68000.0,
            d * (d * prototypes[6][1] + prototypes[3][1]),
            -940000.0,
        ];
        circuit[row(0x4870)] = [
            0.0,
            0.0,
            prototypes[0][2] + d * (d * prototypes[4][2] + prototypes[1][2]),
            k * (k * 2.284106e-6 - 0.0209009) + 1.0,
        ];
        circuit[row(0x4880)] = [
            0.0,
            0.0,
            d * (d * prototypes[5][2] + prototypes[2][2]),
            k * (k * 2.284106e-6 - 0.01034),
        ];
        circuit[row(0x4890)] = [
            0.0,
            0.0,
            d * (d * prototypes[6][2] + prototypes[3][2]),
            k * 4963.623 - 470000.0,
        ];
        let load_a = [2818.38, 2818.38, 39810.7, 1584.89];
        let scale_a = [2.5, 2.5, 17.4, 1.0];
        // Native uses FRECPE, FRECPS and a second Newton refinement. Scalar
        // division preserves the formula, with a possible few-ULP difference.
        for lane in 0..4 {
            let inv = 1.0 / (denom_a[lane] * load_a[lane] - feedback_a[lane]);
            circuit[row(0x47b0)][lane] = 1.0 / denom_a[lane];
            circuit[row(0x47d0)][lane] = inv;
            circuit[row(0x4770)][lane] = inv * load_a[lane];
            circuit[row(0x4790)][lane] = (0.0 - feedback_a[lane] * scale_a[lane]) * inv;
            circuit[row(0x47f0)][lane] = (0.0 - denom_a[lane] * scale_a[lane]) * inv;
            circuit[row(0x4750)][lane] = denom_a[lane] * scale_a[lane];
        }
        circuit[row(0x4730)][0] *= input_scale;
        circuit[row(0x4830)][0] *= input_scale;
        circuit[row(0x4880)][0] *= input_scale;

        let denom_b = [k * 0.022 + 1.0, k * 0.022 + 1.0, k * 0.022 + 1.0, 1.0];
        let feedback_b = [k * -22000.0 - 2e6, k * -22000.0 - 2e6, -500000.0, 0.0];
        circuit[row(0x4740)] = [k * 0.022, k * 0.022, k * 0.022, 0.0];
        circuit[row(0x4820)] = [k * -0.022 + 1.0, k * -0.022 + 1.0, k * -0.022 + 1.0, 0.0];
        circuit[row(0x4840)] = [k * -0.022, k * -0.022, k * -0.022, 0.0];
        circuit[row(0x4860)] = [k * 22000.0 - 2e6, k * 22000.0 - 2e6, -500000.0, 0.0];
        let load_b = [37583.7, 37583.7, 112202.0, 1.0];
        // Literal vector at native VA 0x104cc8330, extracted from the image.
        let scale_b = [24.0, 24.0, 0.0, 0.0];
        for lane in 0..4 {
            let inv = 1.0 / (denom_b[lane] * load_b[lane] - feedback_b[lane]);
            circuit[row(0x47c0)][lane] = 1.0 / denom_b[lane];
            circuit[row(0x47e0)][lane] = inv;
            circuit[row(0x4780)][lane] = inv * load_b[lane];
            circuit[row(0x47a0)][lane] = (0.0 - feedback_b[lane] * scale_b[lane]) * inv;
            let threshold = denom_b[lane] * scale_b[lane];
            circuit[row(0x4800)][lane] = (0.0 - threshold) * inv;
            circuit[row(0x4760)][lane] = threshold;
        }
        for offset in [0x4740, 0x4840] {
            for value in &mut circuit[row(offset)][..2] {
                *value *= 0.7943282;
            }
        }
        let power_gain = -0.024208618 / power_scale;
        circuit[row(0x4780)][3] = power_gain;
        circuit[row(0x47c0)][3] = power_gain;
        let response_low = exp_index((response_db * 0.5 - 40.0) * 9.965784 + 1200.0);
        let response_high =
            exp_index((response_db * 0.5 - 13.0 + response_offset) * 9.965784 + 1200.0);
        Self {
            base,
            drive_current: drive_target,
            prototypes,
            circuit,
            response: [
                base.rate_inv * response_high * 1000.0,
                response_low * -1000.0 * base.rate_inv,
            ],
        }
    }
}

pub(crate) use crate::jump_dsp::JumpAmp;
