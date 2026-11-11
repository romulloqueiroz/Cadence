//! VAN51 coefficient build recovered from `BEffectVan51::SetSampleRate`
//! (`0x1005ce690`) and its helper setters. The native object is a flat byte
//! image; this module mirrors the absolute offsets so each helper is a literal
//! translation. Only the coefficient/state image is modelled here; the
//! ProcessInt circuit is implemented by the companion process module.
#![allow(clippy::excessive_precision)] // Recovered native coefficient literals and oracle values.

#[path = "van51_feedback_filter.rs"]
mod feedback_filter;
#[path = "van51_lead_filter.rs"]
mod lead_filter;
#[path = "van51_tone_stack.rs"]
mod tone_stack;
use feedback_filter::set_feedback_filter;
use lead_filter::set_stage2_lead_filter;
use tone_stack::set_tone_stack;

pub(crate) const VAN51_STATE_SIZE: usize = 0xA000;

/// Twelve 11-entry stage-1 filter tables (`0x104d1c7d0..`), three log-pot
/// tables (`0x104d1c74c`, `0x104d1c778`, `0x104d1c7a4`), then the 28
/// crunch/rhythm tables (`0x104d1c9e0..`).
const VAN51_TABLES: &[u8; 1892] = include_bytes!("van51_tables.bin");
const LOG_TABLE_BASE: usize = 12;
const CRUNCH_TABLE_BASE: usize = 15;

fn table(index: usize, position: usize) -> f32 {
    let at = (index * 11 + position) * 4;
    f32::from_le_bytes(VAN51_TABLES[at..at + 4].try_into().unwrap())
}

/// Linear interpolation through an 11-entry table with the native 0..9 index
/// and one-extra-entry clamp.
fn interpolate(index: usize, control: f32) -> f32 {
    let scaled = control * 10.0;
    let mut slot = scaled as i32;
    if slot > 8 {
        slot = 9;
    }
    if slot < 0 {
        slot = 0;
    }
    let slot = slot as usize;
    let fraction = scaled - slot as f32;
    let low = table(index, slot);
    low + (table(index, slot + 1) - low) * fraction
}

/// Serialized SetVan51 floats in their native argument order. The booleans
/// are the four channel switches followed by the native mono-input switch.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Van51Controls {
    pub values: [f32; 8],
    pub lead: bool,
    pub hi_gain: bool,
    pub bright: bool,
    pub crunch: bool,
    pub mono: bool,
}

impl Default for Van51Controls {
    fn default() -> Self {
        Self {
            values: [0.5; 8],
            lead: false,
            hi_gain: false,
            bright: false,
            crunch: false,
            mono: false,
        }
    }
}

pub(crate) struct Van51State {
    bytes: Box<[u8]>,
}

impl Van51State {
    /// Allocate a duplicate on the loading/preparation thread only.
    pub(crate) fn duplicate_prepared(&self) -> Self {
        Self {
            bytes: self.bytes.to_vec().into_boxed_slice(),
        }
    }
    /// Restore a same-rate prepared image without allocating or dropping owners.
    pub(crate) fn copy_prepared_from(&mut self, template: &Self) {
        self.bytes[..].copy_from_slice(&template.bytes[..]);
    }

    pub(crate) fn new() -> Self {
        Self {
            bytes: vec![0u8; VAN51_STATE_SIZE].into_boxed_slice(),
        }
    }

    #[cfg(test)]
    pub(crate) fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Native constructor/Initialize constants, with the table pointers kept
    /// in the Rust owner rather than embedded as process-specific addresses.
    pub(crate) fn initialize(&mut self, sample_rate: f32) {
        self.bytes.fill(0);
        self.set_u32(0x94bc, 0x3f800000);
        self.set_u32(0x94a8, 0x3f800000);
        self.set_u32(0x94ac, 0x3d9511e5);
        self.set_u32(0x94b0, 0x3c4dc613);
        self.set_u32(0x96fc, 0x3f5765ac);
        self.set_u32(0x96f8, 0x3f7d11d1);
        self.set_u32(0x9d18, 0x3b5c6ff1);
        self.set_u32(0x9d1c, 0x43d198b3);
        // 4x interpolation/decimation FIR vectors from Initialize. Their
        // scaling source is the constructor's +94a8 unity input scale.
        for (at, values) in [
            (0x9660, [0.0, 1.0, 0.0, 0.0]),
            (0x966c, [0.0, -0.0703125, 0.8671875, 0.2265625]),
            (0x967c, [-0.0234375, -0.0625, 0.5625, 0.5625]),
            (0x968c, [-0.0625, -0.0234375, 0.2265625, 0.8671875]),
        ] {
            for (lane, value) in values.into_iter().enumerate() {
                self.set_f32(at + lane * 4, value);
            }
        }
        self.set_f32(0x969c, -0.0703125);
        for (offset, bits) in [
            (0x96a0, 0xbc800000bbc00000u64),
            (0x96a8, 0x00000000bc900000),
            (0x96b0, 0x3e1000003d680000),
            (0x96b8, 0x3e8000003e5e0000),
            (0x96c0, 0x3e1000003e5e0000),
            (0x96c8, 0x800000003d680000),
            (0x96d0, 0xbc800000bc900000),
            (0x96d8, 0x00000000bbc00000),
        ] {
            self.set_u32(offset, bits as u32);
            self.set_u32(offset + 4, (bits >> 32) as u32);
        }
        let tables = crate::amp_dsp::van51_tables();
        for (lane, index) in [
            Some(0),
            Some(8),
            Some(4),
            Some(3),
            Some(5),
            None,
            None,
            None,
            Some(2),
            Some(6),
            Some(9),
            Some(10),
            Some(1),
            Some(7),
            Some(11),
            Some(11),
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(index) = index {
                let table = &tables[index];
                self.set_f32(0x9b80 + lane * 4, table.scale);
                self.set_f32(0x9bc0 + lane * 4, table.fraction);
                self.set_f32(0x9c00 + lane * 4, 255.0 - table.anchor as f32);
                self.set_f32(0x9c40 + lane * 4, -(table.anchor as f32));
            }
        }
        self.set_f32(0x9b60, tables[0].offset);
        self.set_f32(0x9b64, tables[8].offset);
        self.set_f32(0x9b70, tables[1].offset);
        // The power-tube's native extrapolation clamps are supplied manually.
        for lane in [14, 15] {
            self.set_u32(0x9bc0 + lane * 4, 0xbe449800);
            self.set_u32(0x9c00 + lane * 4, 0xc2f20000);
            self.set_u32(0x9c40 + lane * 4, 0xc3bc0000);
        }
        for offset in [0x95d0, 0x9610, 0x9620] {
            self.set_f32(offset, 1.0);
        }
        set_sample_rate(self, sample_rate);
        self.set_controls(&Van51Controls::default());
        set_param_response(self, 0.5);
        self.set_f32(0x948c, 0.5);
        self.set_f32(0x9480, 0.5);
        self.set_f32(0x9484, 0.5);
        self.set_u32(0x94b8, 0x80000000);
        let bias = tables[11].scale * (520.0 * -0.113076925 + -70.3);
        self.set_f32(0x9d0c, bias);
        self.set_f32(0x9d08, bias);
        self.clear();
    }

    /// Recovered SetVan51 call, preserving the parameter-to-target mapping.
    pub(crate) fn set_controls(&mut self, controls: &Van51Controls) {
        self.set_u8(0x1a2, 1);
        self.set_u8(0x1c0, u8::from(controls.mono));
        set_param_lead_channel(self, controls.lead);
        self.set_u8(0x9491, u8::from(controls.hi_gain));
        self.set_u32(
            0x95d4,
            if controls.hi_gain {
                0x3f800000
            } else {
                0x3f004dce
            },
        );
        self.set_u8(0x9640, 1);
        self.set_u32(0x9650, 1);
        self.set_u8(0x9492, u8::from(controls.bright));
        set_stage1_filters(self);
        set_stage2_rhythm_filters(self);
        self.set_u8(0x9493, u8::from(controls.crunch));
        set_stage2_rhythm_filters(self);
        self.set_f32(0x945c, controls.values[0]);
        set_stage2_rhythm_filters(self);
        for (index, offset, table_index, target, dirty) in [
            (1, 0x9460, Some(LOG_TABLE_BASE + 1), 0x9494, 0x94c1),
            (2, 0x9464, Some(LOG_TABLE_BASE + 1), 0x9498, 0x94c0),
            (3, 0x9468, None, 0, 0x94c0),
            (4, 0x946c, None, 0, 0x94c0),
            (5, 0x9470, Some(LOG_TABLE_BASE), 0x949c, 0x94c0),
            (6, 0x9474, Some(LOG_TABLE_BASE + 1), 0x94a4, 0x94c2),
        ] {
            let value = controls.values[index];
            if self.f32(offset) != value {
                self.set_f32(offset, value);
                if let Some(table_index) = table_index {
                    self.set_f32(target, interpolate(table_index, value));
                }
                self.set_u8(dirty, 1);
                if index == 1 {
                    set_stage1_filters(self);
                }
            }
        }
        set_param_presence(self, controls.values[7]);
    }

    /// Snap all 28 smoothed targets as Reset does, and rebuild stateful
    /// polynomial filters before clearing the sixteen native channel banks.
    pub(crate) fn clear(&mut self) {
        set_tone_stack(self);
        set_feedback_filter(self);
        set_stage2_lead_filter(self);
        for channel in 0..16 {
            let base = channel * 0x310;
            let supply = 520.0f32;
            self.set_f32(base + 0x448, supply);
            let supply = supply / 520.0;
            self.set_f32(base + 0x44c, supply * 505.68307);
            self.set_u32(base + 0x440, 0xbf27027f);
            for (lane, voltage) in [504.62445, 469.05557, 334.3473, 318.2227]
                .into_iter()
                .enumerate()
            {
                let voltage = supply * voltage;
                self.set_f32(base + 0x270 + lane * 4, voltage);
                self.set_f32(
                    base + 0x450 + lane * 4,
                    crate::van51_process::reciprocal(voltage),
                );
            }
            for (start, end) in [
                (0x1d0, 0x270),
                (0x280, 0x440),
                (0x460, 0x4b4),
                (0x4c0, 0x4e0),
            ] {
                self.bytes[base + start..base + end].fill(0);
            }
        }
        self.set_u32(0x1bc, 0);
        self.set_u8(0x1a1, 1);
        self.set_u8(0x1b7, 0);
        self.snap_controls();
    }

    pub(crate) fn snap_controls(&mut self) {
        for index in 0..28 {
            self.set_u32(0x94f0 + index * 4, self.u32(0x95d0 + index * 4));
            self.set_u32(0x9560 + index * 4, 0);
        }
        self.set_u8(0x9640, 1);
        self.set_u32(0x9650, 1);
    }

    /// Native 32-host-frame control-rate call (128 internal samples).
    pub(crate) fn on_control_rate(&mut self) {
        if self.u8(0x94c0) != 0 && self.f32(0x9560) == 0.0 {
            self.set_u8(0x94c0, 0);
            set_tone_stack(self);
        }
        if self.u8(0x94c1) != 0 && self.f32(0x95a0) == 0.0 {
            self.set_u8(0x94c1, 0);
            set_stage2_lead_filter(self);
        }
        if self.u8(0x94c2) != 0 && self.f32(0x95b0) == 0.0 {
            self.set_u8(0x94c2, 0);
            set_feedback_filter(self);
        }
        if self.u8(0x9640) == 0 {
            let remaining = self.u32(0x9644).wrapping_sub(1);
            self.set_u32(0x9644, remaining);
            if remaining == 0 {
                for index in 0..28 {
                    self.set_u32(0x94f0 + index * 4, self.u32(0x95d0 + index * 4));
                    self.set_u32(0x9560 + index * 4, 0);
                }
                self.set_u8(0x9641, 0);
                self.set_u32(0x9650, 4);
            } else if self.u32(0x9650) == 2 {
                self.set_u32(0x9650, 3);
            } else if self.u32(0x9650) == 4 {
                self.set_u32(0x9650, 0);
            }
            return;
        }
        self.set_u8(0x9640, 0);
        self.set_u32(0x9650, 2);
        let mut sum = 0.0;
        for row in 0..7 {
            for lane in 0..4 {
                let index = row * 4 + lane;
                let target = self.f32(0x95d0 + index * 4);
                let delta = (target - self.f32(0x94f0 + index * 4)) * self.f32(0x964c);
                let square = delta * delta;
                if square < 1e-15 {
                    self.set_f32(0x9560 + index * 4, 0.0);
                    self.set_f32(0x94f0 + index * 4, target);
                } else {
                    self.set_f32(0x9560 + index * 4, delta);
                    sum += square;
                }
            }
        }
        if sum <= 0.0 {
            self.set_u32(0x9650, 4);
        } else {
            self.set_u32(0x9644, self.u32(0x9648));
            self.set_u8(0x9641, 1);
        }
    }

    #[inline]
    pub(crate) fn f32(&self, offset: usize) -> f32 {
        f32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }

    #[inline]
    pub(crate) fn set_f32(&mut self, offset: usize, value: f32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    #[inline]
    pub(crate) fn u8(&self, offset: usize) -> u8 {
        self.bytes[offset]
    }

    #[inline]
    pub(crate) fn set_u8(&mut self, offset: usize, value: u8) {
        self.bytes[offset] = value;
    }

    #[inline]
    pub(crate) fn u32(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.bytes[offset..offset + 4].try_into().unwrap())
    }

    #[inline]
    pub(crate) fn set_u32(&mut self, offset: usize, value: u32) {
        self.bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
    }

    /// `CONCAT44(hi, lo)` stored through a `ulong`: `lo` at the low address.
    #[inline]
    pub(crate) fn set_pair(&mut self, offset: usize, hi: f32, lo: f32) {
        self.set_f32(offset, lo);
        self.set_f32(offset + 4, hi);
    }

    #[inline]
    pub(crate) fn f64(&self, offset: usize) -> f64 {
        f64::from_le_bytes(self.bytes[offset..offset + 8].try_into().unwrap())
    }

    #[inline]
    pub(crate) fn set_f64(&mut self, offset: usize, value: f64) {
        self.bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
}

/// `BEffectVan51::setStage8Filter` (`0x1005d72e0`).
pub(crate) fn set_stage8_filter(s: &mut Van51State) {
    let r = f64::from(s.f32(0x9450)) + f64::from(s.f32(0x9450));
    let r2 = r * r;
    let r3 = r * r2;
    let d2 = r2 * 1.7061000000000002e-14 * 100000.0;
    let d9 = r * 5.17e-08;
    let d10 = r2 * 1.0890000000000001e-13 * 100000.0;
    let d12 = r * 3.3e-07;
    let d4 = -(r * 3.3e-07);
    let d6 = -(r2 * 1.7061000000000002e-14 * 100000.0);
    let d13 = r3 * 1.689039e-20 * 100000.0 * 100000.0;
    let d14 = r3 * 5.6301300000000004e-21 * 100000.0 * 100000.0;
    let d7 = 1.0
        / (d2 * 1000000.0
            + d14 * 1000000.0
            + d2 * 1000000.0
            + d2 * 1000000.0
            + d9 * 1000000.0
            + d2 * 100000.0
            + d2 * 100000.0
            + d10 * 100000.0
            + d9 * 100000.0
            + d12 * 100000.0
            + d12 * 100000.0
            + d12 * 100000.0
            + 1.0);
    let d8 = d7 * 0.043401026364474474;
    let d11 = d14 * 1000000.0 * d8;
    s.set_f64(0x9938, d11);
    s.set_f64(0x9930, d11);
    s.set_f64(
        0x9948,
        r3 * -5.6301300000000004e-21 * 100000.0 * 100000.0 * 1000000.0 * d8,
    );
    s.set_f64(
        0x9940,
        r3 * -1.689039e-20 * 100000.0 * 100000.0 * 1000000.0 * d8,
    );
    s.set_f64(0x9950, d13 * 1000000.0 * d8);
    s.set_f64(0x9958, 0.0);
    s.set_f64(
        0x9968,
        (-(d2 * 1000000.0)
            + d14 * 1000000.0
            + d6 * 1000000.0
            + d6 * 1000000.0
            + d9 * 1000000.0
            + d6 * 100000.0
            + d6 * 100000.0
            + -(r2 * 1.0890000000000001e-13 * 100000.0) * 100000.0
            + d9 * 100000.0
            + d12 * 100000.0
            + d12 * 100000.0
            + d12 * 100000.0
            + -1.0)
            * d7,
    );
    s.set_f64(
        0x9960,
        (d2 * 1000000.0
            + d13 * 1000000.0
            + d2 * 1000000.0
            + d2 * 1000000.0
            + -(r * 5.17e-08) * 1000000.0
            + d2 * 100000.0
            + d2 * 100000.0
            + d10 * 100000.0
            + -(r * 5.17e-08) * 100000.0
            + d4 * 100000.0
            + d4 * 100000.0
            + d4 * 100000.0
            + -3.0)
            * d7,
    );
    s.set_f64(
        0x9970,
        -(d7 * ((((((((((((-(d2 * 1000000.0) + d13 * 1000000.0) - d2 * 1000000.0)
            - d2 * 1000000.0)
            - d9 * 1000000.0)
            - d2 * 100000.0)
            - d2 * 100000.0)
            - d10 * 100000.0)
            - d9 * 100000.0)
            - d12 * 100000.0)
            - d12 * 100000.0)
            - d12 * 100000.0)
            + 3.0)),
    );
    s.set_f64(0x9978, 0.0);
}

/// `BEffectVan51::setNlFlt1` (`0x1005d66e4`). Writes the sixteen per-stage
/// coefficient records (stride `0x310`) and the nonlinear stage-1 bank.
pub(crate) fn set_nl_flt1(s: &mut Van51State) {
    let rate = s.f32(0x9450);
    let r = rate + rate;
    s.set_pair(0x9988, (r * 0.022) * 0.5011872, (r * 0.0792) * 0.70794576);
    s.set_pair(0x9980, (r * 0.0) * 1.0, (r * 0.508) * 1.0);
    s.set_pair(0x9998, (r * -0.022) * 0.5011872, (r * -0.0792) * 0.70794576);
    s.set_pair(0x9990, 0.0, 0.0);
    s.set_pair(
        0x99b8,
        0.0 - ((r * 0.0 + -0.02717) * r + 1.0),
        0.0 - ((r * 0.0 + -0.086438) * r + 1.0),
    );
    s.set_pair(
        0x99b0,
        0.0 - ((r * 0.0 + 0.0) * r + 0.0),
        0.0 - ((r * -2.146513e-05 + 0.0) * r + 2.0),
    );
    s.set_pair(
        0x99c8,
        (r * 0.0 + 10340.0) * r + -2e6,
        (r * 0.0 + 26056.8) * r + -3.6e6,
    );
    s.set_pair(
        0x99c0,
        (r * 0.0 + 0.0) * r + 0.0,
        (r * 0.72981447 + 0.0) * r + -8.068e6,
    );

    let f3 = (r * 0.0 + 0.0) * r + 0.0;
    let f5 = (r * 1.0732565e-05 + -0.5088733) * r + 1.0;
    let f6 = (r * 0.0 + -0.508) * r + 0.0;
    let f7 = (r * -0.36490723 + 20794.88) * r + -4.034e6;
    for stage in 0..16 {
        let base = 0x4b0 + stage * 0x310;
        s.set_f32(base, f3);
        s.set_f32(base + 4, f5);
        s.set_f32(base + 8, f6);
        s.set_f32(base + 0xc, f7);
    }

    let f3 = 1.0;
    let f5 = 1.0;
    let f6 = 1.0;
    let f7 = 1.0;
    let f8 = (r * 1.0732565e-05 + 0.5088733) * r + f3;
    let f9 = (r * 0.0 + 0.0) * r + f5;
    let f10 = (r * 0.0 + 0.086438) * r + f6;
    let f11 = (r * 0.0 + 0.02717) * r + f7;
    let f12 = (r * -0.36490723 + -20794.88) * r + -4.034e6;
    let f13 = (r * 0.0 + 0.0) * r + 0.0;
    let f14 = (r * 0.0 + -26056.8) * r + -3.6e6;
    let rf = (r * 0.0 + -10340.0) * r + -2e6;

    s.set_pair(0x99a8, f7 / f11, f6 / f10);
    s.set_pair(0x99a0, f5 / f9, f3 / f8);
    s.set_pair(0x99d8, f11 * 0.55, f10 * 0.35);
    s.set_pair(0x99d0, f9 * 0.0, f8 * 2.3);

    let d3 = f3 / (f8 * 42169.6 - f12);
    let d5 = f5 / (f9 * 1.0 - f13);
    let d6 = f6 / (f10 * 2.81838e6 - f14);
    let d7 = f7 / (f11 * 794328.0 - rf);
    s.set_f32(0x99e8, d6);
    s.set_f32(0x99ec, d7);
    s.set_f32(0x99e0, d3);
    s.set_f32(0x99e4, d5);
    s.set_pair(0x99f8, d7 * 794328.0, d6 * 2.81838e6);
    s.set_pair(0x99f0, d5 * 1.0, d3 * 42169.6);
    s.set_pair(0x9a08, (0.0 - rf * 0.55) * d7, (0.0 - f14 * 0.35) * d6);
    s.set_pair(0x9a00, (0.0 - f13 * 0.0) * d5, (0.0 - f12 * 2.3) * d3);
    s.set_pair(0x9a18, (0.0 - f11 * 0.55) * d7, (0.0 - f10 * 0.35) * d6);
    s.set_pair(0x9a10, (0.0 - f9 * 0.0) * d5, (0.0 - f8 * 2.3) * d3);

    if rate > 384000.0 {
        let r = rate + rate;
        s.set_f32(0x9980, r * 0.508);
        s.set_f32(0x99b0, -(r * r * -2.146513e-05 + 2.0));
        s.set_f32(0x99c0, r * r * 0.72981447 + -8.068e6);
        let f5 = r * (r * 1.0732565e-05 + -0.5088733) + 1.0;
        let f2 = r * -0.508;
        let f3 = r * (r * -0.36490723 + 20794.88) + -4.034e6;
        for stage in 0..16 {
            let base = 0x4b0 + stage * 0x310;
            s.set_f32(base + 4, f5);
            s.set_f32(base + 8, f2);
            s.set_f32(base + 0xc, f3);
        }
        let f2 = r * (r * 1.0732565e-05 + 0.5088733) + 1.0;
        let f3 = r * (r * -0.36490723 + -20794.88) + -4.034e6;
        s.set_f32(0x99a0, 1.0 / f2);
        s.set_f32(0x99d0, f2 * 2.3);
        let denom = f2 * 42169.6 - f3;
        s.set_f32(0x99e0, 1.0 / denom);
        s.set_f32(0x99f0, 42169.6 / denom);
        s.set_f32(0x9a00, (f3 * -2.3) / denom);
        s.set_f32(0x9a10, (f2 * -2.3) / denom);
    }
}

fn exp_index(x: f32) -> f32 {
    crate::amp_dsp::exp_index(x)
}

fn clamped_product(value: f32, scale: f32) -> f32 {
    let product = value * scale;
    if product <= 1.0 {
        product
    } else {
        1.0
    }
}

/// `BEffectVan51::setStage1Filters` (`0x1005d6b38`).
pub(crate) fn set_stage1_filters(s: &mut Van51State) {
    let bright = s.u8(0x9492) != 0;
    let lead = s.u8(0x9490) != 0;
    let (lp2f, hp3f, gain) = if lead {
        if bright {
            (0, 1, 2)
        } else {
            (3, 4, 5)
        }
    } else if bright {
        (6, 7, 8)
    } else {
        (9, 10, 11)
    };
    let control = s.f32(0x9460);
    let lp = interpolate(lp2f, control);
    let hp = interpolate(hp3f, control);
    let gain = interpolate(gain, control);
    let scale = s.f32(0x9458);
    s.set_f32(
        0x9600,
        clamped_product(exp_index(lp * 5.0 + 1381.8816), scale),
    );
    s.set_f32(
        0x9618,
        clamped_product(exp_index(hp * 5.0 + 1381.8816), scale),
    );
    s.set_f32(0x9628, exp_index(gain * 9.965784 + 1200.0) - 1.0);
    s.set_u8(0x9640, 1);
    s.set_u32(0x9650, 1);
}

/// `BEffectVan51::setStage2RhythmFilters` (`0x1005d6da4`).
pub(crate) fn set_stage2_rhythm_filters(s: &mut Van51State) {
    let bright = s.u8(0x9492) != 0;
    let crunch = s.u8(0x9493) != 0;
    let block = (if crunch { 0 } else { 2 }) + usize::from(!bright);
    let base = CRUNCH_TABLE_BASE + block * 7;
    let control = s.f32(0x945c);
    let scale = s.f32(0x9458);

    let hp4 = interpolate(base, control);
    s.set_f32(
        0x95dc,
        clamped_product(exp_index(hp4 * 5.0 + 1381.8816), scale),
    );

    let lp5f = interpolate(base + 1, control);
    let lp5gain = interpolate(base + 2, control);
    s.set_f32(
        0x95e8,
        clamped_product(exp_index(lp5f * 5.0 + 1381.8816), scale),
    );
    s.set_f32(0x95f8, exp_index(lp5gain * 9.965784 + 1200.0) - 1.0);

    let lp6f = interpolate(base + 3, control);
    let lp6gain = interpolate(base + 4, control);
    s.set_f32(
        0x95e0,
        clamped_product(exp_index(lp6f * 5.0 + 1381.8816), scale),
    );
    s.set_f32(0x95f0, exp_index(lp6gain * 9.965784 + 1200.0) - 1.0);

    let lp7f = interpolate(base + 5, control);
    s.set_f32(
        0x95f4,
        clamped_product(exp_index((lp7f + 8.0) * 5.0 + 1381.8816), scale),
    );

    let s2r = interpolate(base + 6, control);
    let gain = if control != 0.0 {
        exp_index((s2r + -26.8) * 9.965784 + 1200.0)
    } else {
        0.0
    };
    s.set_f32(0x9630, gain);
    s.set_u8(0x9640, 1);
    s.set_u32(0x9650, 1);
}

/// `BEffectVan51::setParamLeadChannel` (`0x1005cec58`).
pub(crate) fn set_param_lead_channel(s: &mut Van51State, lead: bool) {
    s.set_u8(0x9490, u8::from(lead));
    set_stage1_filters(s);
    let rhythm = !lead;
    if rhythm {
        s.set_f32(0x9608, 0.0);
        s.set_f32(0x960c, f32::from_bits(0x3EB5_AA1A));
    } else {
        s.set_f32(0x9608, f32::from_bits(0x3F18_7D50));
        s.set_f32(0x960c, 0.0);
    }
    let scale = if rhythm { 5.2 } else { 8.7 };
    let shelf = if rhythm { 50.0 } else { 38.0 };
    let high = exp_index(shelf * 5.0 + 1381.8816);
    let low = exp_index(scale * 9.965784 + 1200.0);
    s.set_f32(0x95ec, clamped_product(high, s.f32(0x9458)));
    s.set_f32(0x95fc, low - 1.0);
    s.set_u8(0x9640, 1);
    s.set_u32(0x9650, 1);
}

/// `BEffectVan51::setParamPresence` (`0x1005cedcc`).
pub(crate) fn set_param_presence(s: &mut Van51State, presence: f32) {
    if s.f32(0x9478) == presence {
        return;
    }
    s.set_f32(0x9478, presence);
    let value = interpolate(LOG_TABLE_BASE + 2, presence);
    s.set_f32(0x94a0, value);
    let high = value.clamp(0.5, 1.0);
    let shelf = (high * -47.5 + high * high * 60.7143 + 54.3571) * 5.0 + 1381.8816;
    s.set_f32(0x961c, clamped_product(exp_index(shelf), s.f32(0x9458)));
    let low = value.clamp(0.3, 1.0);
    let corner = (low * low * -7.37446 + low * low * low * 5.73232 + low * 3.27536 + -0.110952)
        * 9.965784
        + 1200.0;
    s.set_f32(0x962c, exp_index(corner) - 1.0);
    s.set_u8(0x9640, 1);
    s.set_u32(0x9650, 1);
    s.set_u8(0x94c2, 1);
}

/// `BEffectVan51::setParamResponse` (`0x1005cefc4`).
pub(crate) fn set_param_response(s: &mut Van51State, response: f32) {
    s.set_f32(0x9488, response);
    let value = response * 48.0 - 24.0;
    s.set_f32(0x94b4, value);
    let scale = s.f32(0x9458);
    for (offset, corner) in [
        (0x9740, -1.0),
        (0x9744, -44.0),
        (0x9748, -56.0),
        (0x974c, -42.0),
    ] {
        let gain = exp_index((value + corner) * 5.0 + 1381.8816);
        s.set_f32(offset, clamped_product(gain, scale));
    }
    let inverse = s.f32(0x9454);
    let low = exp_index((value * 0.5 + -50.0) * 9.965784 + 1200.0);
    s.set_f32(0x9d14, low * -1000.0 * inverse);
    let high = exp_index((value * 0.5 + 3.0 + s.f32(0x94b8)) * 9.965784 + 1200.0);
    s.set_f32(0x9d10, inverse * high * 1000.0);
}

/// Fixed ExpTable reads use the saved global's exact base/slope pair.
fn fixed_exp(address: usize, fraction: f32) -> f32 {
    let index = ((address - 0x109dc2200) / 8) as f32;
    let low = exp_index(index);
    low + (exp_index(index + 1.0) - low) * fraction
}

/// `BEffectVan51::SetSampleRate` (0x1005ce690), 4x internal rate.
pub(crate) fn set_sample_rate(s: &mut Van51State, rate: f32) {
    let rate = rate * 4.0;
    let period = ((rate * 0.02 * 0.0078125 + 0.5) as u32).max(1);
    s.set_u32(0x9648, period);
    s.set_f32(0x964c, 1.0 / (period << 7) as f32);
    s.set_f32(0x9450, rate);
    s.set_f32(0x9454, 1.0 / rate);
    let scale = (f64::from(1.0 / rate) * std::f64::consts::TAU) as f32;
    s.set_f32(0x9458, scale);
    let pole = clamped_product(fixed_exp(0x109dc5538, 0.3815918), scale);
    s.set_u32(0x9568, 0);
    s.set_f32(0x95d8, pole);
    s.set_f32(0x94f8, pole);
    for (offset, value) in [
        (0x96e0, 0),
        (0x96e4, 0),
        (0x9700, 0),
        (0x9704, 0x3f800000),
        (0x9710, 0),
        (0x9714, 0),
        (0x9720, 0),
        (0x9724, 0),
    ] {
        s.set_u32(offset, value);
    }
    for (offset, address, fraction) in [
        (0x96e8, 0x109dc6240, 0.8815918),
        (0x96ec, 0x109dc60b0, 0.8815918),
        (0x9708, 0x109dc5ab0, 0.3815918),
        (0x970c, 0x109dc59f8, 0.8815918),
        (0x9728, 0x109dc5ba0, 0.8815918),
        (0x972c, 0x109dc5ed0, 0.3815918),
        (0x9734, 0x109dc60f0, 0.3815918),
        (0x9738, 0x109dc60a0, 0.3815918),
        (0x973c, 0x109dc5ff0, 0.8815918),
    ] {
        s.set_f32(offset, clamped_product(fixed_exp(address, fraction), scale));
    }
    s.set_f32(0x9718, fixed_exp(0x109dc4890, 0.08300781) - 1.0);
    s.set_f32(0x971c, fixed_exp(0x109dc4a48, 0.3930664) - 1.0);
    set_param_response(s, s.f32(0x9488));
    set_nl_flt1(s);
    let r = rate + rate;
    for (base, numerator, numerator_scale, denominator, conductance, resistance, gain) in [
        (
            0x9a20,
            [0.00726, 0.00044, 0.022, 0.022],
            [1.0; 4],
            [0.00759, 0.00308, 0.022, 0.022],
            [1669.8, 264.0, 2200.0, 2200.0],
            [1e8, 1e8, 3758373.0, 50119.0],
            [5.5, 10.0, 0.0, 11.0],
        ),
        (
            0x9ac0,
            [0.0, 0.22, 0.009480303, 0.009480303],
            [1.0, 1.0, 1.1885022, 1.0],
            [7.633e-7, 0.44, 0.009480303, 0.009480303],
            [0.0, 220000.0, 10.428333, 10.428333],
            [1e8, 1e8, 316.228, 316.228],
            [10.0, 3.35, 51.0, 51.0],
        ),
    ] {
        for lane in 0..4 {
            let constant = if base == 0x9ac0 && lane == 0 {
                1.0
            } else {
                0.0
            };
            let b0 = (r * numerator[lane] + constant) * numerator_scale[lane];
            let b1 = (r * -numerator[lane] + constant) * numerator_scale[lane];
            let a0 = r * denominator[lane] + 1.0;
            let a1 = 0.0 - (r * -denominator[lane] + 1.0);
            let load = if base == 0x9a20 {
                [550000.0, 100000.0, 1.1e6, 1.1e6][lane]
            } else {
                [68000.0, 1e6, 625900.0, 625900.0][lane]
            };
            let c0 = r * -conductance[lane] + -load;
            let c1 = r * conductance[lane] + -load;
            let inv = 1.0 / (a0 * resistance[lane] - c0);
            for (row, value) in [
                b0,
                b1,
                1.0 / a0,
                a1,
                c1,
                a0 * gain[lane],
                inv,
                inv * resistance[lane],
                (0.0 - c0 * gain[lane]) * inv,
                (0.0 - a0 * gain[lane]) * inv,
            ]
            .into_iter()
            .enumerate()
            {
                s.set_f32(base + row * 16 + lane * 4, value);
            }
        }
    }
    let supply_rate = s.f32(0x9454) * if s.u8(0x947c) != 0 { 120.0 } else { 100.0 };
    for channel in 0..16 {
        s.set_f32(channel * 0x310 + 0x444, supply_rate);
    }
    s.set_f32(
        0x9d14,
        s.f32(0x9454) * exp_index((s.f32(0x94b4) * 0.5 + -50.0) * 9.965784 + 1200.0) * -1000.0,
    );
    s.set_f32(
        0x9d10,
        s.f32(0x9454)
            * exp_index((s.f32(0x94b4) * 0.5 + 3.0 + s.f32(0x94b8)) * 9.965784 + 1200.0)
            * 1000.0,
    );
    set_stage1_filters(s);
    set_stage2_lead_filter(s);
    set_stage2_rhythm_filters(s);
    set_stage8_filter(s);
    set_feedback_filter(s);
    set_tone_stack(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(actual: f32, expected: f32) -> bool {
        (actual - expected).abs() <= 2e-5 * (1.0 + expected.abs())
    }

    fn close64(actual: f64, expected: f64) -> bool {
        (actual - expected).abs() <= 2e-9 * (1.0 + expected.abs())
    }

    #[test]
    fn van51_polynomial_helpers_match_isolated_native_instructions() {
        let fixture = include_bytes!("../tests/fixtures/van51-polynomial.bin");
        let mut cursor = 0;
        for rate in [32000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            for knobs in [
                [0.0; 7],
                [
                    0.0543478243,
                    0.0543478243,
                    0.5,
                    0.5,
                    0.212807879,
                    0.0543478243,
                    0.1,
                ],
                [1.0; 7],
                [0.231, 0.77, 0.27, 0.63, 0.38, 0.41, 0.23],
            ] {
                let mut state = Van51State::new();
                state.set_f32(0x9450, rate * 4.0);
                for (offset, value) in [0x9494, 0x9498, 0x9468, 0x946c, 0x94a0, 0x94a4, 0x949c]
                    .into_iter()
                    .zip(knobs)
                {
                    state.set_f32(offset, value);
                }
                set_tone_stack(&mut state);
                set_feedback_filter(&mut state);
                set_stage2_lead_filter(&mut state);
                for (start, end) in [(0x9750, 0x98d0), (0x9890, 0x98e0)] {
                    for offset in (start..end).step_by(8) {
                        let want =
                            f64::from_le_bytes(fixture[cursor..cursor + 8].try_into().unwrap());
                        cursor += 8;
                        let got = state.f64(offset);
                        assert!(
                            got == want || (got - want).abs() <= want.abs() * 2e-13 + 1e-20,
                            "rate {rate} knobs {knobs:?} offset {offset:x}: {got} != {want}"
                        );
                    }
                }
            }
        }
        assert_eq!(cursor, fixture.len());
    }

    #[test]
    fn van51_rate_coefficients_match_isolated_native_instructions() {
        let fixture = include_bytes!("../tests/fixtures/van51-rate.bin");
        let mut cursor = 0;
        for rate in [32000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            let mut state = Van51State::new();
            for (offset, bits) in [
                (0x94a8, 0x3f800000),
                (0x94ac, 0x3d9511e5),
                (0x94b0, 0x3c4dc613),
                (0x9488, 0x3f000000),
            ] {
                state.set_u32(offset, bits);
            }
            set_sample_rate(&mut state, rate);
            for (start, end) in [(0x96e0, 0x9750), (0x9a20, 0x9b60), (0x4b4, 0x4c0)] {
                for offset in (start..end).step_by(4) {
                    let want = f32::from_le_bytes(fixture[cursor..cursor + 4].try_into().unwrap());
                    cursor += 4;
                    let got = state.f32(offset);
                    assert!(
                        got == want || (got - want).abs() <= want.abs() * 5e-6 + 1e-20,
                        "rate {rate} offset {offset:x}: {got} != {want}"
                    );
                }
            }
        }
        assert_eq!(cursor, fixture.len());
    }

    #[test]
    fn van51_initialized_state_is_finite_and_settled() {
        for rate in [8000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
            let mut state = Van51State::new();
            state.initialize(rate);
            for offset in (0x9a20..0x9b60).step_by(4) {
                assert!(state.f32(offset).is_finite(), "{rate} {offset:x}");
            }
            for index in 0..28 {
                assert_eq!(state.u32(0x94f0 + index * 4), state.u32(0x95d0 + index * 4));
                assert_eq!(state.f32(0x9560 + index * 4), 0.0);
            }
            if rate == 48000.0 {
                if let Ok(path) = std::env::var("VAN51_DUMP_STATE") {
                    std::fs::write(path, state.as_bytes()).unwrap();
                }
            }
        }
    }

    #[test]
    fn van51_initial_controls_match_original_initialize_and_reset() {
        let native = include_bytes!("../tests/fixtures/van51-initialize.bin");
        let mut state = Van51State::new();
        state.initialize(48_000.0);
        for index in 0..28 {
            let at = 0x95d0 + index * 4;
            let expected = f32::from_le_bytes(native[at..at + 4].try_into().unwrap());
            assert!(
                (state.f32(at) - expected).abs() <= 2e-6 * expected.abs(),
                "target {at:x}: {} != {expected}",
                state.f32(at)
            );
        }
        // Three separate filter crossfades always converge to the new bank.
        for at in [0x95d0, 0x9610, 0x9620] {
            assert_eq!(state.f32(at), 1.0);
        }
        assert_eq!(
            state.f32(0x9458).to_bits(),
            u32::from_le_bytes(native[0x9458..0x945c].try_into().unwrap())
        );
    }

    #[test]
    fn van51_stage8_and_nl_flt1_match_pseudocode_oracle() {
        for (rate, stage8, nl) in [
            (
                48000.0f32,
                [
                    0.043348493004396499,
                    -0.13004547901318944,
                    -0.043348493004396499,
                    0.13004547901318944,
                    2.9975785649253535,
                    0.99757976515586944,
                    -2.9951583302416314,
                ],
                [48768.0f32, 0.0, 5382.65332, 1058.50732, 6.76754098e-06, 1.0],
            ),
            (
                96000.0f32,
                [
                    0.043374747033794785,
                    -0.13012424110138429,
                    -0.043374747033794785,
                    0.13012424110138429,
                    2.9987888494556283,
                    0.99878914968495758,
                    -2.9975779991606508,
                ],
                [97536.0f32, 0.0, 10765.3066, 2117.01465, 2.02695878e-06, 1.0],
            ),
        ] {
            let mut state = Van51State::new();
            state.set_f32(0x9450, rate);
            set_stage8_filter(&mut state);
            let got = [
                state.f64(0x9930),
                state.f64(0x9940),
                state.f64(0x9948),
                state.f64(0x9950),
                state.f64(0x9960),
                state.f64(0x9968),
                state.f64(0x9970),
            ];
            let want = [
                stage8[0], stage8[1], stage8[2], stage8[3], stage8[4], stage8[5], stage8[6],
            ];
            for (a, b) in got.iter().zip(want.iter()) {
                assert!(close64(*a, *b), "rate {rate} stage8 {a} != {b}");
            }

            state.set_f32(0x9450, rate);
            set_nl_flt1(&mut state);
            let got = [
                state.f32(0x9980),
                state.f32(0x9984),
                state.f32(0x9988),
                state.f32(0x998c),
                state.f32(0x99a0),
                state.f32(0x99a4),
            ];
            for (a, b) in got.iter().zip(nl.iter()) {
                assert!(close(*a, *b), "rate {rate} nl {a} != {b}");
            }
            assert!(close(
                state.f32(0x4b4),
                if rate == 48000.0 {
                    50060.4805
                } else {
                    297942.594
                }
            ));
        }
    }

    #[test]
    fn van51_stage2_rhythm_matches_pseudocode_oracle() {
        let cases: [(u8, f32, [f32; 7]); 6] = [
            (
                0,
                0.0,
                [
                    7.50798863e-05,
                    0.00202030106,
                    1.8184216,
                    0.298775136,
                    -0.949300587,
                    1.0,
                    0.0,
                ],
            ),
            (
                0,
                0.5,
                [
                    0.00060063909,
                    0.00169886381,
                    1.29089642,
                    0.280379504,
                    -0.657227159,
                    1.0,
                    0.013031845,
                ],
            ),
            (
                0,
                1.0,
                [
                    0.0015135156,
                    0.00120127818,
                    2.54819059,
                    0.325813711,
                    -0.649242938,
                    0.248353213,
                    0.211352423,
                ],
            ),
            (
                1,
                0.0,
                [
                    0.000801756978,
                    0.00285713701,
                    2.54819059,
                    1.0,
                    -0.988779664,
                    1.0,
                    0.0,
                ],
            ),
            (
                1,
                0.5,
                [
                    0.000801756978,
                    0.00190691021,
                    1.69157028,
                    1.0,
                    -0.960189164,
                    1.0,
                    0.0988567322,
                ],
            ),
            (
                1,
                1.0,
                [
                    0.000801756978,
                    0.00214043446,
                    1.66075969,
                    0.193729997,
                    -0.683767438,
                    1.0,
                    0.204177171,
                ],
            ),
        ];
        for (bright, control, want) in cases {
            let mut state = Van51State::new();
            state.set_f32(0x9458, 6.2831855 / 48000.0);
            state.set_u8(0x9492, bright);
            state.set_u8(0x9493, 0);
            state.set_f32(0x945c, control);
            set_stage2_rhythm_filters(&mut state);
            let got = [
                state.f32(0x95dc),
                state.f32(0x95e8),
                state.f32(0x95f8),
                state.f32(0x95e0),
                state.f32(0x95f0),
                state.f32(0x95f4),
                state.f32(0x9630),
            ];
            for (a, b) in got.iter().zip(want.iter()) {
                assert!(
                    close(*a, *b),
                    "bright {bright} control {control}: {a} != {b}"
                );
            }
        }
    }

    #[test]
    fn van51_param_setters_match_pseudocode_oracle() {
        let rate_scale = 6.2831855f32 / 48000.0;
        let mut state = Van51State::new();
        state.set_f32(0x9450, 48000.0);
        state.set_f32(0x9454, 1.0 / 48000.0);
        state.set_f32(0x9458, rate_scale);
        state.set_f32(0x9460, 0.5);
        set_stage1_filters(&mut state);
        assert!(close(state.f32(0x9600), 1.0));
        assert!(close(state.f32(0x9618), 0.365713537));
        assert!(close(state.f32(0x9628), -0.411151409));

        for (lead, high, low, raw_high, raw_low) in [
            (
                false,
                0.0192204509f32,
                0.819717407f32,
                0.0f32,
                0.354813397f32,
            ),
            (true, 0.00961022545, 1.72273755, 0.595662117, 0.0),
        ] {
            let mut state = Van51State::new();
            state.set_f32(0x9450, 48000.0);
            state.set_f32(0x9454, 1.0 / 48000.0);
            state.set_f32(0x9458, rate_scale);
            set_param_lead_channel(&mut state, lead);
            assert!(close(state.f32(0x95ec), high), "lead {lead}");
            assert!(close(state.f32(0x95fc), low), "lead {lead}");
            assert!(close(state.f32(0x9608), raw_high), "lead {lead}");
            assert!(close(state.f32(0x960c), raw_low), "lead {lead}");
        }

        let mut state = Van51State::new();
        state.set_f32(0x9458, rate_scale);
        set_param_presence(&mut state, 0.5);
        assert!(close(state.f32(0x94a0), 0.212807879));
        assert!(close(state.f32(0x961c), 0.0150676323));
        assert!(close(state.f32(0x962c), 0.0426613092));

        let mut state = Van51State::new();
        state.set_f32(0x9454, 1.0 / 48000.0);
        state.set_f32(0x9458, rate_scale);
        set_param_response(&mut state, 0.5);
        assert!(close(state.f32(0x9740), 0.00101015053));
        assert!(close(state.f32(0x9744), 8.42743248e-05));
        assert!(close(state.f32(0x9748), 4.21371624e-05));
        assert!(close(state.f32(0x974c), 9.45947249e-05));
        assert!(close(state.f32(0x9d14), -6.5881708e-05));
        assert!(close(state.f32(0x9d10), 0.0294280406));
    }
}
