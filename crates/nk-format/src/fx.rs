//! Program-level FX slot chains (`PARAMETER_ARRAY_8` -> `BParameterArraySer<BParFX,8>`).
//!
//! Layout follows the saved `BParameterArraySer<BParFX,8>::doRead`,
//! `BParFX::doReadPrivPars` (version 0x50) and
//! `BSerializableStructured::doRead`. The type-specific child payload is
//! retained verbatim; only the common BParFX fields are decoded here.

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct FxChild {
    pub ser_type: u16,
    pub version: u16,
    /// Private section of the type-specific object.
    pub private: Vec<u8>,
    /// Public section of the type-specific object.
    pub public: Vec<u8>,
    /// Nested children of the type-specific object.
    pub children: Vec<u8>,
    /// Raw payload for objects whose reader is not sectioned (e.g. the filter).
    pub raw: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FxSlot {
    pub class_id: i32,
    pub bypass: bool,
    pub out_level: f32,
    pub out_level_dry: f32,
    pub send_partition: i32,
    pub routers_open: bool,
    pub fix_type: i32,
    pub object_version: u16,
    pub child: Option<FxChild>,
}

pub type FilterBands = (f32, Vec<(f32, f32, f32)>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslBandKind {
    LowShelf,
    Peaking,
    HighShelf,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DelayParams {
    pub time_ms: f32,
    pub feedback: f32,
    pub pan: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DistortionParams {
    pub transistor: bool,
    pub drive: f32,
    pub damping: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CabinetParams {
    pub size: f32,
    pub air: f32,
    pub treble: f32,
    pub bass: f32,
    pub cabinet_type: i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmpKind {
    Jump,
    Van51,
    AcBox,
    HotSolo,
    BassInvader,
    Cat,
    BassPro,
    EpPreamps,
}

/// Recovered `BParFX*` amp payload order, consumed by the separate recovered
/// circuits in sample-adapter.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmpSettings {
    pub kind: AmpKind,
    pub values: [f32; 17],
    pub value_count: u8,
    pub flags: [bool; 8],
    pub flag_count: u8,
    pub modes: [i32; 4],
}

impl AmpSettings {
    pub fn values(&self) -> &[f32] {
        &self.values[..self.value_count as usize]
    }
    pub fn flags(&self) -> &[bool] {
        &self.flags[..self.flag_count as usize]
    }
    pub fn drive_params(&self) -> DriveParams {
        let v = self.values();
        DriveParams {
            drive: v[0].clamp(0.0, 1.0),
            tone: v[1].clamp(0.0, 1.0),
            bass: v[2].clamp(0.0, 1.0),
            mid: v[3].clamp(0.0, 1.0),
            treble: v[4].clamp(0.0, 1.0),
            level: v[5].clamp(0.0, 2.0),
            mode: self.flags.first().copied().unwrap_or(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TransientParams {
    pub attack: f32,
    pub sustain: f32,
    pub gain: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DriveParams {
    pub drive: f32,
    pub tone: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub level: f32,
    pub mode: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkreamerParams {
    pub tone: f32,
    pub drive: f32,
    pub bass: f32,
    pub bright: f32,
    pub mix: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CompressorParams {
    pub model: i32,
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack_ms: f32,
    pub release_ms: f32,
    pub stereo_link: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ModulationParams {
    pub depth: f32,
    pub speed_hz: f32,
    pub phase: f32,
    pub feedback: f32,
    pub color: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ReverbParams {
    pub pre_delay_ms: f32,
    pub room_size: f32,
    pub stereo: f32,
    pub color: f32,
    pub filter: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SslBand {
    pub kind: SslBandKind,
    pub frequency: f32,
    pub q: f32,
    pub gain_db: f32,
}

/// Recovered `NI::DSP::CSL::G_EQ` curves (natural values at unity 0.0, 0.1, …,
/// 1.0). Extracted from `HF_*_Curve_GE`, `HMF_*_Curve_GE`, `LMF_*_Curve_GE` and
/// `LF_*_Curve_GE` in the Kontakt 8.14.1 ARM64 image.
const LF_GAIN_GE: [f32; 11] = [
    -19.5, -16.0, -10.0, -5.3, -2.0, 0.0, 2.0, 5.3, 10.0, 16.0, 19.5,
];
const LF_FREQ_GE: [f32; 11] = [
    40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34,
];
const LMF_GAIN_GE: [f32; 11] = [
    -21.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 21.5,
];
const LMF_FREQ_GE: [f32; 11] = [
    223.0, 239.0, 323.0, 494.0, 668.0, 809.0, 1050.0, 1479.0, 1841.0, 2083.0, 2200.0,
];
const LMF_Q_GE: [f32; 11] = [0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2];
const HMF_GAIN_GE: [f32; 11] = [
    -20.0, -17.5, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 17.5, 20.0,
];
const HMF_FREQ_GE: [f32; 11] = [
    692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74, 6892.65,
];
const HMF_Q_GE: [f32; 11] = [
    0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22,
];
const HF_GAIN_GE: [f32; 11] = [
    -20.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 20.5,
];
const HF_FREQ_GE: [f32; 11] = [
    1539.0, 1710.0, 2400.0, 3980.0, 5460.0, 6820.0, 9160.0, 13100.0, 17813.0, 21000.0, 21800.0,
];

fn compressor_from_values(
    threshold: f32,
    ratio: f32,
    attack_ms: f32,
    release_ms: f32,
) -> Option<CompressorParams> {
    if ![threshold, ratio, attack_ms, release_ms]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    Some(CompressorParams {
        model: 0,
        threshold_db: (20.0 * threshold.max(1e-6).log10()).clamp(-60.0, 0.0),
        ratio: ratio.clamp(1.0, 100.0),
        attack_ms: attack_ms.clamp(0.0, 2000.0),
        release_ms: release_ms.clamp(0.0, 5000.0),
        stereo_link: true,
    })
}

fn modulation4(r: &mut Reader<'_>) -> Option<ModulationParams> {
    let depth = r.f32().ok()?;
    let speed = r.f32().ok()?;
    let phase = r.f32().ok()?;
    let feedback = r.f32().ok()?;
    Some(checked_modulation(depth, speed, phase, feedback, 0.0))
}

fn delay_from_reader(r: &mut Reader<'_>) -> Option<DelayParams> {
    let time_ms = r.f32().ok()?;
    let feedback = r.f32().ok()?;
    let pan = r.f32().ok()?;
    let damping = r.f32().ok()?;
    if ![time_ms, feedback, pan, damping]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    Some(DelayParams {
        time_ms: time_ms.clamp(0.0, 5000.0),
        feedback: feedback.clamp(0.0, 0.98),
        pan: pan.clamp(-1.0, 1.0),
        damping: damping.clamp(0.0, 1.0),
    })
}

fn drive_params(
    drive: f32,
    tone: f32,
    bass: f32,
    mid: f32,
    treble: f32,
    level: f32,
    mode: bool,
) -> Option<DriveParams> {
    if ![drive, tone, bass, mid, treble, level]
        .iter()
        .all(|v| v.is_finite())
    {
        return None;
    }
    Some(DriveParams {
        drive: drive.clamp(0.0, 1.0),
        tone: tone.clamp(0.0, 1.0),
        bass: bass.clamp(0.0, 1.0),
        mid: mid.clamp(0.0, 1.0),
        treble: treble.clamp(0.0, 1.0),
        level: level.clamp(0.0, 2.0),
        mode,
    })
}

fn checked_modulation(
    depth: f32,
    speed_hz: f32,
    phase: f32,
    feedback: f32,
    color: f32,
) -> ModulationParams {
    let values = [depth, speed_hz, phase, feedback, color];
    if values.iter().any(|v| !v.is_finite()) {
        return ModulationParams {
            depth: 0.0,
            speed_hz: 0.0,
            phase: 0.0,
            feedback: 0.0,
            color: 0.0,
        };
    }
    ModulationParams {
        depth: depth.clamp(0.0, 1.0),
        speed_hz: speed_hz.clamp(0.0, 40.0),
        phase: phase.rem_euclid(1.0),
        feedback: feedback.clamp(-0.98, 0.98),
        color: color.clamp(0.0, 1.0),
    }
}

fn modulation(r: &mut Reader<'_>, feedback: f32, color: f32) -> Option<ModulationParams> {
    let depth = r.f32().ok()?;
    let speed_hz = r.f32().ok()?;
    let phase = r.f32().ok()?;
    Some(checked_modulation(depth, speed_hz, phase, feedback, color))
}

fn curve(points: &[f32; 11], unity: f32) -> f32 {
    let position = unity.clamp(0.0, 1.0) * 10.0;
    let index = (position as usize).min(9);
    let fraction = position - index as f32;
    points[index] + (points[index + 1] - points[index]) * fraction
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterShape {
    Lowpass,
    Highpass,
    Bandpass,
    Bandreject,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SimpleFilter {
    pub shape: FilterShape,
    /// 1, 2, 4 or 6 poles.
    pub poles: u8,
    pub cutoff_hz: f32,
    pub q: f32,
}

/// Recovered filter families whose serialized controls must stay normalized.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NativeFilterKind {
    LegacyLowpass1,
    LegacyHighpass1,
    LegacyLowpass2,
    LegacyHighpass2,
    LegacyBandpass2,
    LegacyLowpass4,
    LegacyHighpass4,
    LegacyBandpass4,
    LegacyBandreject4,
    LegacyLowpass6,
    LegacyLadder,
    SvLowpass1,
    SvHighpass1,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NativeFilter {
    pub kind: NativeFilterKind,
    pub cutoff: f32,
    pub resonance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShaperKind {
    Classic,
    Drums,
    Enhanced,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ShaperParams {
    /// Native bipolar control, clamped by BParFX::SetObj.
    pub shape: f32,
    pub kind: ShaperKind,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FxChain {
    pub version: u16,
    pub slots: Vec<Option<FxSlot>>,
}

/// Eight parallel sends plus the native sidechain-send metadata.
#[derive(Debug, Clone, PartialEq)]
pub struct SendLevels {
    pub levels: [f32; 8],
    pub sidechains: [f32; 17],
}

/// Natural (not UI-normalized) IRC parameters, versions 0x60/0x70.
#[derive(Debug, Clone, PartialEq)]
pub struct ConvolutionParams {
    pub rate_factor: f32,
    pub block_size: i32,
    pub predelay_ms: f32,
    pub early_length: f32,
    pub early_highpass: f32,
    pub early_lowpass: f32,
    pub late_length: f32,
    pub late_highpass: f32,
    pub late_lowpass: f32,
    pub crossover: f32,
    pub reverse: bool,
    pub auto_gain: bool,
    pub preserve_length: bool,
    pub bypass_latency_compensation: bool,
    pub envelope_active: bool,
    pub envelope_times: Vec<f32>,
    pub envelope_levels: Vec<f32>,
    pub filename_id: i32,
}

impl FxChild {
    /// Version 0x92 repeats the subtype before its normalized controls.
    /// Other filter families retain their existing, approximate playback path.
    pub fn native_filter(&self) -> Result<Option<NativeFilter>, Error> {
        if self.ser_type != 24 || self.version != 0x92 {
            return Ok(None);
        }
        let mut r = Reader::new(&self.raw);
        r.i32()?;
        // The native reader overwrites the subtype with this second value.
        // Do not require historical/versioned subtype fields to agree.
        let fix = r.i32()?;
        let kind = match fix {
            2 => NativeFilterKind::LegacyLowpass1,
            3 => NativeFilterKind::LegacyHighpass1,
            6 => NativeFilterKind::LegacyLowpass2,
            7 => NativeFilterKind::LegacyHighpass2,
            4 => NativeFilterKind::LegacyBandpass2,
            8 => NativeFilterKind::LegacyLowpass4,
            9 => NativeFilterKind::LegacyHighpass4,
            10 => NativeFilterKind::LegacyBandpass4,
            11 => NativeFilterKind::LegacyBandreject4,
            12 => NativeFilterKind::LegacyLowpass6,
            17 => NativeFilterKind::LegacyLadder,
            50 => NativeFilterKind::SvLowpass1,
            51 => NativeFilterKind::SvHighpass1,
            _ => return Ok(None),
        };
        let result = NativeFilter {
            kind,
            cutoff: r.finite()?.clamp(0.0, 1.0),
            resonance: r.finite()?.clamp(0.0, 1.0),
        };
        r.finish()?;
        Ok(Some(result))
    }

    pub fn send_levels(&self) -> Result<Option<SendLevels>, Error> {
        if self.ser_type != 23 {
            return Ok(None);
        }
        if !matches!(self.version, 0x50 | 0x51) {
            return Err(Error::Malformed("unsupported send-level version".into()));
        }
        let mut r = Reader::new(&self.raw);
        let mut levels = [0.0; 8];
        let mut sidechains = [1.0; 17];
        read_fx_levels(&mut r, &mut levels)?;
        if self.version == 0x51 {
            read_fx_levels(&mut r, &mut sidechains)?;
        }
        r.finish()?;
        Ok(Some(SendLevels { levels, sidechains }))
    }

    pub fn convolution(&self) -> Result<Option<ConvolutionParams>, Error> {
        if self.ser_type != 22 {
            return Ok(None);
        }
        if !matches!(self.version, 0x60 | 0x70) {
            return Err(Error::Malformed("unsupported IRC version".into()));
        }
        let mut r = Reader::new(&self.raw);
        let result = ConvolutionParams {
            rate_factor: r.finite()?,
            block_size: r.i32()?,
            predelay_ms: r.finite()?,
            early_length: r.finite()?,
            early_highpass: r.finite()?,
            early_lowpass: r.finite()?,
            late_length: r.finite()?,
            late_highpass: r.finite()?,
            late_lowpass: r.finite()?,
            crossover: r.finite()?,
            reverse: r.boolean()?,
            auto_gain: r.boolean()?,
            preserve_length: r.boolean()?,
            bypass_latency_compensation: r.boolean()?,
            envelope_active: r.boolean()?,
            envelope_times: read_ir_envelope(&mut r)?,
            envelope_levels: read_ir_envelope(&mut r)?,
            filename_id: r.i32()?,
        };
        r.finish()?;
        Ok(Some(result))
    }

    /// Version-0x92 filter bands: two fix IDs then (frequency, bandwidth, gain).
    pub fn filter_bands(&self) -> Option<Vec<(f32, f32, f32)>> {
        if self.ser_type != 0x18 || self.version != 0x92 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        r.i32().ok()?;
        r.i32().ok()?;
        let mut bands = Vec::new();
        while r.data.len().saturating_sub(r.pos) >= 12 {
            let frequency = r.f32().ok()?;
            let bandwidth = r.f32().ok()?;
            let gain = r.f32().ok()?;
            if !frequency.is_finite()
                || !bandwidth.is_finite()
                || !gain.is_finite()
                || !(10.0..=24_000.0).contains(&frequency)
                || !(0.01..=16.0).contains(&bandwidth)
                || gain.abs() > 48.0
            {
                return None;
            }
            bands.push((frequency, bandwidth, gain));
        }
        if bands.is_empty() {
            None
        } else {
            Some(bands)
        }
    }

    pub fn simple_filter(&self) -> Option<SimpleFilter> {
        if self.ser_type != 0x18 || self.version != 0x92 || self.raw.len() != 16 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        r.i32().ok()?;
        let fix = r.i32().ok()?;
        let normalized = r.f32().ok()?;
        let resonance = r.f32().ok()?;
        if !normalized.is_finite() || !resonance.is_finite() {
            return None;
        }
        let (shape, poles) = match fix {
            2 => (FilterShape::Lowpass, 1),
            3 => (FilterShape::Highpass, 1),
            4 => (FilterShape::Bandpass, 2),
            6 => (FilterShape::Lowpass, 2),
            7 => (FilterShape::Highpass, 2),
            8 => (FilterShape::Lowpass, 4),
            9 => (FilterShape::Highpass, 4),
            10 => (FilterShape::Bandpass, 4),
            11 => (FilterShape::Bandreject, 4),
            12 => (FilterShape::Lowpass, 6),
            17 => (FilterShape::Lowpass, 4),
            // fixtypeToType maps 50/51 to runtime 0x2a/0x2b; the
            // BFilterSV1 wrapper selects low-pass/high-pass respectively.
            50 => (FilterShape::Lowpass, 1),
            51 => (FilterShape::Highpass, 1),
            _ => return None,
        };
        let cutoff_hz = (20.0f32 * 1000f32.powf(normalized.clamp(0.0, 1.0))).clamp(10.0, 24_000.0);
        Some(SimpleFilter {
            shape,
            poles,
            cutoff_hz,
            q: 0.707 + resonance.clamp(0.0, 1.0) * 9.293,
        })
    }

    /// BParFXShaper v0x80: f32 shape followed by signed mode ID.
    pub fn shaper_params(&self) -> Result<Option<ShaperParams>, Error> {
        if self.ser_type != 29 {
            return Ok(None);
        }
        if self.version != 0x80 {
            return Err(Error::Malformed("unsupported shaper version".into()));
        }
        let mut r = Reader::new(&self.raw);
        let shape = r.finite()?.clamp(-1.0, 1.0);
        let kind = match r.i32()? {
            1 => ShaperKind::Drums,
            2 => ShaperKind::Enhanced,
            // The native dispatcher treats every other signed ID as Classic.
            _ => ShaperKind::Classic,
        };
        r.finish()?;
        Ok(Some(ShaperParams { shape, kind }))
    }

    pub fn shaper(&self) -> Option<(f32, i32)> {
        if self.ser_type != 29 || self.version != 0x80 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let shape = r.f32().ok()?;
        let kind = r.i32().ok()?;
        if !shape.is_finite() {
            return None;
        }
        Some((shape, kind))
    }

    /// SSL G EQ bands with the recovered G_EQ control curves.
    ///
    /// Wire order and runtime fields come from `BParFXSSLGEQ::doRead`
    /// (`0x100781288`) and `BEffectSSLGEQ::setParameters` (`0x1004243ac`).
    /// Gains/frequencies/Q are normalized unity values mapped through the
    /// `G_EQ` eleven-point curves. The low and high bands are shelves; only the
    /// two mid bands are peaking. Linear interpolation between the recovered
    /// points is an explicit approximation of the native spline.
    pub fn ssl_bands(&self) -> Option<Vec<SslBand>> {
        if self.ser_type != 68 || self.version != 0x11 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let lf_gain = r.f32().ok()?;
        let lf_freq = r.f32().ok()?;
        r.u8().ok()?;
        let lmf_gain = r.f32().ok()?;
        let lmf_freq = r.f32().ok()?;
        let lmf_q = r.f32().ok()?;
        let hmf_gain = r.f32().ok()?;
        let hmf_freq = r.f32().ok()?;
        let hmf_q = r.f32().ok()?;
        let hf_gain = r.f32().ok()?;
        let hf_freq = r.f32().ok()?;
        r.u8().ok()?;
        let _output = r.f32().ok()?;
        let _input = r.f32().ok()?;
        r.u8().ok()?;
        r.u8().ok()?;
        let bands = [
            SslBand {
                kind: SslBandKind::LowShelf,
                frequency: curve(&LF_FREQ_GE, lf_freq),
                q: 0.707,
                gain_db: curve(&LF_GAIN_GE, lf_gain),
            },
            SslBand {
                kind: SslBandKind::Peaking,
                frequency: curve(&LMF_FREQ_GE, lmf_freq),
                q: curve(&LMF_Q_GE, lmf_q),
                gain_db: curve(&LMF_GAIN_GE, lmf_gain),
            },
            SslBand {
                kind: SslBandKind::Peaking,
                frequency: curve(&HMF_FREQ_GE, hmf_freq),
                q: curve(&HMF_Q_GE, hmf_q),
                gain_db: curve(&HMF_GAIN_GE, hmf_gain),
            },
            SslBand {
                kind: SslBandKind::HighShelf,
                frequency: curve(&HF_FREQ_GE, hf_freq),
                q: 0.707,
                gain_db: curve(&HF_GAIN_GE, hf_gain),
            },
        ];
        if bands.iter().any(|band| {
            !band.frequency.is_finite() || !band.gain_db.is_finite() || !band.q.is_finite()
        }) {
            return None;
        }
        Some(bands.into())
    }

    /// `BParFXGainer` v0x50: one clamped linear gain.
    pub fn gainer(&self) -> Option<f32> {
        if self.ser_type != 19 || self.version != 0x50 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let gain = r.f32().ok()?;
        r.finish().ok()?;
        gain.is_finite().then_some(gain.clamp(0.0, 1.0))
    }

    /// `BParFXInverter` v0x60: two channel-invert flags.
    pub fn inverter(&self) -> Option<[bool; 2]> {
        if self.ser_type != 26 || self.version != 0x60 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let left = r.u8().ok()? != 0;
        let right = r.u8().ok()? != 0;
        r.finish().ok()?;
        Some([left, right])
    }

    /// `BParFXDelay`: time/feedback/panning/damping (v0x50/v0x51).
    pub fn delay_params(&self) -> Option<DelayParams> {
        if self.ser_type != 16 || !matches!(self.version, 0x50 | 0x51) {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let time_ms = r.f32().ok()?;
        let feedback = r.f32().ok()?;
        let pan = r.f32().ok()?;
        let damping = r.f32().ok()?;
        let values = [time_ms, feedback, pan, damping];
        if values.iter().any(|v| !v.is_finite()) {
            return None;
        }
        Some(DelayParams {
            time_ms: time_ms.clamp(0.0, 5000.0),
            // BParFXDelay::SetObj scales the serialized feedback by 0.98.
            feedback: (feedback * 0.98).clamp(0.0, 0.98),
            pan: pan.clamp(-1.0, 1.0),
            damping: damping.clamp(0.0, 1.0),
        })
    }

    /// `BParFXSurroundPanner` v0x70: count + per-source x/y/enable; mapped to
    /// a simple stereo pan using the first source.
    pub fn surround_panner_params(&self) -> Option<f32> {
        if self.ser_type != 29 || self.version != 0x70 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let count = r.u32().ok()?;
        if count == 0 {
            return Some(0.0);
        }
        let x = r.f32().ok()?;
        let _y = r.f32().ok()?;
        let _enabled = r.u8().ok()?;
        x.is_finite().then_some(x.clamp(-1.0, 1.0))
    }

    /// `BParFXCoreCell` v0x51: ten floats; first six drive multi-drive.
    pub fn core_cell_params(&self) -> Option<DriveParams> {
        if self.ser_type != 94 || self.version != 0x51 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mut values = [0.0f32; 6];
        for value in &mut values {
            *value = r.f32().ok()?;
        }
        drive_params(
            values[0], values[1], values[2], values[3], values[4], values[5], false,
        )
    }

    /// Reverb family: three floats mapped to decay/damping/mix.
    /// `BParFXReverb` v0x50: the serialized order is pre-delay, room size,
    /// stereo, colour, filter (`doRead` + `PrsSetValues` tag names). The
    /// previous shared `(decay, damping, mix)` interpretation was wrong.
    pub fn base_reverb_params(&self) -> Option<ReverbParams> {
        if self.ser_type != 21 || self.version != 0x50 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let pre_delay_ms = r.f32().ok()?;
        let room_size = r.f32().ok()?;
        let stereo = r.f32().ok()?;
        let color = r.f32().ok()?;
        let filter = r.f32().ok()?;
        if [pre_delay_ms, room_size, stereo, color, filter]
            .iter()
            .any(|v| !v.is_finite())
        {
            return None;
        }
        Some(ReverbParams {
            pre_delay_ms: pre_delay_ms.clamp(0.0, 200.0),
            room_size: room_size.clamp(0.0, 1.0),
            stereo: stereo.clamp(0.0, 1.0),
            color: color.clamp(0.0, 1.0),
            filter: filter.clamp(0.0, 1.0),
        })
    }

    pub fn reverb_params(&self) -> Option<(f32, f32, f32)> {
        let has_mode = match (self.ser_type, self.version) {
            (21, 0x50) | (87, 0x10) | (122, 0x10) => false,
            (89, 0x10) | (101, 0x10) => true,
            _ => return None,
        };
        let mut r = Reader::new(&self.raw);
        if has_mode {
            r.u32().ok()?;
        }
        let decay = r.f32().ok()?;
        let damping = r.f32().ok()?;
        let mix = r.f32().ok()?;
        if ![decay, damping, mix].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some((
            decay.clamp(0.0, 0.99),
            damping.clamp(0.0, 1.0),
            mix.clamp(0.0, 1.0),
        ))
    }

    /// `BParFXCabinet` v0x50: size, air, treble, bass, cabinetType (S32).
    pub fn cabinet_params(&self) -> Option<CabinetParams> {
        if self.ser_type != 36 || self.version != 0x50 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let size = r.f32().ok()?;
        let air = r.f32().ok()?;
        let treble = r.f32().ok()?;
        let bass = r.f32().ok()?;
        let cabinet_type = r.i32().ok()?;
        if ![size, air, treble, bass].into_iter().all(f32::is_finite) {
            return None;
        }
        Some(CabinetParams {
            size,
            air,
            treble,
            bass,
            cabinet_type,
        })
    }

    /// Amp family payloads, exactly as read by `BParFX*::doRead`. JUMP and
    /// VAN51 have recovered `Set*`/`ProcessInt` playback implementations.
    pub fn amp_settings(&self) -> Option<AmpSettings> {
        let (kind, floats, flags, modes): (AmpKind, usize, usize, bool) =
            match (self.ser_type, self.version) {
                (77, 0x10) => (AmpKind::Jump, 6, 2, false),
                (82, 0x10) => (AmpKind::Van51, 8, 5, false),
                (83, 0x10) => (AmpKind::AcBox, 7, 1, false),
                (84, 0x10) => (AmpKind::HotSolo, 8, 2, false),
                (84, 0x51) => (AmpKind::BassInvader, 7, 4, false),
                (84, 0x52) => (AmpKind::BassInvader, 7, 4, false),
                (85, 0x10) => (AmpKind::Cat, 7, 1, false),
                (97, 0x10) => (AmpKind::BassPro, 17, 5, false),
                (108, 0x11) => (AmpKind::EpPreamps, 11, 6, true),
                _ => return None,
            };
        let mut r = Reader::new(&self.raw);
        let mut values = [0.0f32; 17];
        for value in values.iter_mut().take(floats) {
            *value = r.f32().ok()?;
            if !value.is_finite() {
                return None;
            }
        }
        let mut settings = AmpSettings {
            kind,
            values,
            value_count: floats as u8,
            flags: [false; 8],
            flag_count: flags as u8,
            modes: [0; 4],
        };
        if modes {
            settings.flags[0] = r.u8().ok()? != 0;
            settings.flags[1] = r.u8().ok()? != 0;
            let limits = [5, 6, 7, 5];
            for (index, limit) in limits.into_iter().enumerate() {
                let value = r.i32().ok()?;
                if value < 0 || value >= limit {
                    return None;
                }
                settings.modes[index] = value;
            }
            for flag in settings.flags.iter_mut().skip(2).take(4) {
                *flag = r.u8().ok()? != 0;
            }
        } else {
            let serialized = if (self.ser_type, self.version) == (84, 0x51) {
                flags - 1
            } else {
                flags
            };
            for flag in settings.flags.iter_mut().take(serialized) {
                *flag = r.u8().ok()? != 0;
            }
            if flags > serialized {
                settings.flags[serialized] = true;
            }
        }
        Some(settings)
    }

    /// `BParFXTwang` v0x50/0x51: four floats + two flags; mapped to multi-drive.
    pub fn twang_params(&self) -> Option<DriveParams> {
        if self.ser_type != 35 || !matches!(self.version, 0x50 | 0x51) {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let _extra = r.f32().ok()?;
        let level = r.f32().ok()?;
        drive_params(drive, tone, 0.5, 0.5, tone, level, false)
    }

    /// `BParFXCryWah` v0x10: one float + enable; cutoff position.
    pub fn cry_wah_params(&self) -> Option<(f32, bool)> {
        if self.ser_type != 88 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let position = r.f32().ok()?;
        let enable = r.u8().ok()? != 0;
        position
            .is_finite()
            .then_some((position.clamp(0.0, 1.0), enable))
    }

    /// `BParFXBite` v0x10: mode + eleven floats; first six drive multi-drive.
    pub fn bite_params(&self) -> Option<DriveParams> {
        if self.ser_type != 102 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mode = r.u32().ok()?;
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let bass = r.f32().ok()?;
        let mid = r.f32().ok()?;
        let treble = r.f32().ok()?;
        let level = r.f32().ok()?;
        if !(0..3).contains(&mode) {
            return None;
        }
        drive_params(drive, tone, bass, mid, treble, level, false)
    }

    /// `BParFXDirt` v0x10: ten floats + flags; first six drive multi-drive.
    pub fn dirt_params(&self) -> Option<DriveParams> {
        if self.ser_type != 103 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mut values = [0.0f32; 6];
        for value in &mut values {
            *value = r.f32().ok()?;
        }
        drive_params(
            values[0], values[1], values[2], values[3], values[4], values[5], false,
        )
    }

    /// `BParFXFreak` v0x10: twelve floats + flags; first six drive multi-drive.
    pub fn freak_params(&self) -> Option<DriveParams> {
        if self.ser_type != 104 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mut values = [0.0f32; 6];
        for value in &mut values {
            *value = r.f32().ok()?;
        }
        drive_params(
            values[0], values[1], values[2], values[3], values[4], values[5], false,
        )
    }

    /// `BParFXDYX` v0x60: two floats, mode and enable; mapped to the compressor.
    pub fn dyx_params(&self) -> Option<CompressorParams> {
        if self.ser_type != 27 || self.version != 0x60 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let threshold = r.f32().ok()?;
        let amount = r.f32().ok()?;
        let _mode = r.u32().ok()?;
        let _enable = r.u8().ok()?;
        compressor_from_values(threshold, 1.0 + amount.clamp(0.0, 1.0) * 19.0, 5.0, 100.0)
    }

    /// `BParFXSSLGBusComp` v0x10/0x11/0x12: threshold/ratio/attack/release.
    pub fn ssl_bus_comp_params(&self) -> Option<CompressorParams> {
        if self.ser_type != 70 || !(0x10..=0x12).contains(&self.version) {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let threshold = r.f32().ok()?;
        let ratio = r.f32().ok()?;
        let attack = r.f32().ok()?;
        let release = r.f32().ok()?;
        compressor_from_values(
            threshold,
            1.0 + ratio.clamp(0.0, 1.0) * 19.0,
            attack,
            release,
        )
    }

    /// `BParFXFBComp` v0x10: seven floats; first four drive the compressor.
    pub fn fb_comp_params(&self) -> Option<CompressorParams> {
        if self.ser_type != 76 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let threshold = r.f32().ok()?;
        let ratio = r.f32().ok()?;
        let attack = r.f32().ok()?;
        let release = r.f32().ok()?;
        compressor_from_values(
            threshold,
            1.0 + ratio.clamp(0.0, 1.0) * 19.0,
            attack,
            release,
        )
    }

    /// `BParFXHilbertLimiter` v0x51: three floats + enable; mapped to a limiter.
    pub fn hilbert_limiter_params(&self) -> Option<CompressorParams> {
        if self.ser_type != 95 || self.version != 0x51 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let threshold = r.f32().ok()?;
        let release = r.f32().ok()?;
        let _output = r.f32().ok()?;
        compressor_from_values(threshold, 100.0, 1.0, release)
    }

    /// `BParFXSupercharger` v0x51: seven floats + mode fields.
    pub fn supercharger_params(&self) -> Option<CompressorParams> {
        if self.ser_type != 96 || self.version != 0x51 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let threshold = r.f32().ok()?;
        let ratio = r.f32().ok()?;
        let attack = r.f32().ok()?;
        let release = r.f32().ok()?;
        compressor_from_values(
            threshold,
            1.0 + ratio.clamp(0.0, 1.0) * 19.0,
            attack,
            release,
        )
    }

    /// `BParFXRingModulator` v0x50: carrier rate and depth from the first two
    /// floats after the enable flag.
    pub fn ring_mod_params(&self) -> Option<(f32, f32)> {
        if self.ser_type != 100 || self.version != 0x50 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let _enable = r.u8().ok()?;
        let rate = r.f32().ok()?;
        let depth = r.f32().ok()?;
        if !rate.is_finite() || !depth.is_finite() {
            return None;
        }
        Some((rate.clamp(0.0, 2000.0), depth.clamp(0.0, 1.0)))
    }

    /// `BParFXLoFi` v0x70: four floats + enable.
    pub fn lo_fi_params(&self) -> Option<(f32, f32, f32, f32)> {
        if self.ser_type != 32 || self.version != 0x70 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let downsample = r.f32().ok()?;
        let bits = r.f32().ok()?;
        let _enable = r.u8().ok()?;
        let mix = r.f32().ok()?;
        let tone = r.f32().ok()?;
        if ![downsample, bits, mix, tone].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some((
            downsample.clamp(0.0, 1.0),
            bits.clamp(0.0, 1.0),
            mix.clamp(0.0, 1.0),
            tone.clamp(0.0, 1.0),
        ))
    }

    /// `BParFXRotator` v0x50: six floats; first three map to rate/depth/phase.
    pub fn rotator_params(&self) -> Option<(f32, f32, f32)> {
        if self.ser_type != 34 || self.version != 0x50 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let rate = r.f32().ok()?;
        let depth = r.f32().ok()?;
        let phase = r.f32().ok()?;
        if ![rate, depth, phase].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some((
            rate.clamp(0.0, 40.0),
            depth.clamp(0.0, 1.0),
            phase.rem_euclid(1.0),
        ))
    }

    /// `BParFXPhasis` v0x51: nine floats; first four map to depth/speed/phase/feedback.
    pub fn phasis_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 91 || self.version != 0x51 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        modulation4(&mut r)
    }

    /// `BParFXFlair` v0x51: nine floats; first four map to depth/speed/phase/feedback.
    pub fn flair_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 92 || self.version != 0x51 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        modulation4(&mut r)
    }

    /// `BParFXChoral` v0x51: seven floats; first four map to depth/speed/phase/feedback.
    pub fn choral_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 93 || self.version != 0x51 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        modulation4(&mut r)
    }

    /// `BParFXStereoTune` v0x10: four floats, mapped to a stereo detune
    /// (approximate; not a true pitch shifter).
    pub fn stereo_tune_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 112 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let depth = r.f32().ok()?;
        let speed = r.f32().ok()?;
        let _mix = r.f32().ok()?;
        let _level = r.f32().ok()?;
        Some(checked_modulation(depth, speed.max(0.1), 0.0, 0.0, 0.0))
    }

    /// `BParFXBeatMasher` v1: three floats + flags + mode.
    pub fn beat_masher_params(&self) -> Option<(f32, f32)> {
        if self.ser_type != 113 || self.version != 1 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let length = r.f32().ok()?;
        let mix = r.f32().ok()?;
        if !length.is_finite() || !mix.is_finite() {
            return None;
        }
        Some((length.clamp(0.0, 1.0), mix.clamp(0.0, 1.0)))
    }

    /// `BParFXBeatSlicer` v1: two floats + flags + modes.
    pub fn beat_slicer_params(&self) -> Option<(f32, f32)> {
        if self.ser_type != 115 || self.version != 1 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let rate = r.f32().ok()?;
        let depth = r.f32().ok()?;
        if !rate.is_finite() || !depth.is_finite() {
            return None;
        }
        Some((rate.clamp(0.0, 40.0), depth.clamp(0.0, 1.0)))
    }

    /// `BParFXReverseGrain` v1: four floats + flags.
    pub fn reverse_grain_params(&self) -> Option<(f32, f32)> {
        if self.ser_type != 116 || self.version != 1 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let length = r.f32().ok()?;
        let mix = r.f32().ok()?;
        if !length.is_finite() || !mix.is_finite() {
            return None;
        }
        Some((length.clamp(0.0, 1.0), mix.clamp(0.0, 1.0)))
    }

    /// `BParFXTransposeStretch` v1: four floats + flags; mapped to a
    /// modulated-delay approximation.
    pub fn transpose_stretch_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 117 || self.version != 1 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let amount = r.f32().ok()?;
        let mix = r.f32().ok()?;
        let _extra = r.f32().ok()?;
        let _extra2 = r.f32().ok()?;
        Some(checked_modulation(amount, 5.0, 0.0, 0.0, mix))
    }

    /// `BParFXTape` v0x10: drive/tone/mix via the multi-band drive path.
    pub fn tape_params(&self) -> Option<DriveParams> {
        if self.ser_type != 66 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let mix = r.f32().ok()?;
        let mode = r.u8().ok()? != 0;
        drive_params(drive, tone, 0.5, 0.5, tone, mix, mode)
    }

    /// `BParFXTrans` v0x10: attack/sustain/gain + two-way mode.
    pub fn trans_params(&self) -> Option<TransientParams> {
        if self.ser_type != 67 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let attack = r.f32().ok()?;
        let sustain = r.f32().ok()?;
        let gain = r.f32().ok()?;
        let mode = r.u32().ok()?;
        if mode > 1 || ![attack, sustain, gain].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(TransientParams {
            attack: attack.clamp(0.0, 1.0),
            sustain: sustain.clamp(0.0, 1.0),
            gain: gain.clamp(0.0, 4.0),
        })
    }

    /// `BParFXPsycheDelay` v0x10: first four floats reuse the Delay layout.
    pub fn psyche_delay_params(&self) -> Option<DelayParams> {
        if self.ser_type != 99 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        delay_from_reader(&mut r)
    }

    /// `BParFXTwinDelay` v0x10: first delay group.
    pub fn twin_delay_params(&self) -> Option<DelayParams> {
        if self.ser_type != 107 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let time_ms = r.f32().ok()?;
        let feedback = r.f32().ok()?;
        let damping = r.f32().ok()?;
        let _enable = r.u8().ok()?;
        if ![time_ms, feedback, damping].iter().all(|v| v.is_finite()) {
            return None;
        }
        Some(DelayParams {
            time_ms: time_ms.clamp(0.0, 5000.0),
            feedback: feedback.clamp(0.0, 0.98),
            pan: 0.0,
            damping: damping.clamp(0.0, 1.0),
        })
    }

    /// `BParFXSuperFast100` v0x10: eight floats + flags; first six drive a
    /// multi-band waveshaper.
    pub fn super_fast_params(&self) -> Option<DriveParams> {
        if self.ser_type != 118 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mut values = [0.0f32; 8];
        for value in &mut values {
            *value = r.f32().ok()?;
        }
        if !values.iter().all(|v| v.is_finite()) {
            return None;
        }
        drive_params(
            values[0], values[1], values[2], values[3], values[4], values[5], false,
        )
    }

    /// `BParFXReplikaDelay` v0x12: model + ten floats + flags. The first four
    /// floats reuse the Delay layout (time/feedback/panning/damping).
    pub fn replika_delay_params(&self) -> Option<DelayParams> {
        if self.ser_type != 90 || self.version != 0x12 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let model = r.u32().ok()?;
        let time_ms = r.f32().ok()?;
        let feedback = r.f32().ok()?;
        let pan = r.f32().ok()?;
        let damping = r.f32().ok()?;
        if !(0..5).contains(&model)
            || ![time_ms, feedback, pan, damping]
                .iter()
                .all(|v| v.is_finite())
        {
            return None;
        }
        Some(DelayParams {
            time_ms: time_ms.clamp(0.0, 5000.0),
            // BParFXDelay::SetObj scales the serialized feedback by 0.98.
            feedback: (feedback * 0.98).clamp(0.0, 0.98),
            pan: pan.clamp(-1.0, 1.0),
            damping: damping.clamp(0.0, 1.0),
        })
    }

    /// `BParFXGater` v1: rate, depth, phase, shape + four mode flags.
    pub fn gater_params(&self) -> Option<(f32, f32)> {
        if self.ser_type != 114 || self.version != 1 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let rate = r.f32().ok()?;
        let depth = r.f32().ok()?;
        if !rate.is_finite() || !depth.is_finite() {
            return None;
        }
        Some((rate.clamp(0.0, 40.0), depth.clamp(0.0, 1.0)))
    }

    /// `BParFXVibratoChorus` v0x10: mode/submode + rate/depth/mix/phase.
    pub fn vibrato_chorus_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 105 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let _mode = r.u32().ok()?;
        let _sub = r.u32().ok()?;
        let rate = r.f32().ok()?;
        let depth = r.f32().ok()?;
        let _mix = r.f32().ok()?;
        let phase = r.f32().ok()?;
        Some(checked_modulation(depth, rate, phase, 0.0, 0.0))
    }

    /// `BParFXWowFlutter` v0x10: wow/flutter rates and depths + tone/mix.
    pub fn wow_flutter_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 106 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let wow_rate = r.f32().ok()?;
        let wow_depth = r.f32().ok()?;
        let flutter_rate = r.f32().ok()?;
        let flutter_depth = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let _mix = r.f32().ok()?;
        let _extra = r.f32().ok()?;
        Some(checked_modulation(
            (wow_depth + flutter_depth) * 0.5,
            wow_rate.max(flutter_rate),
            0.0,
            0.0,
            tone,
        ))
    }

    /// `BParFXChainsaw` v0x10: four floats + enable.
    pub fn chainsaw_params(&self) -> Option<DriveParams> {
        if self.ser_type != 119 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let treble = r.f32().ok()?;
        let level = r.f32().ok()?;
        let mode = r.u8().ok()? != 0;
        drive_params(drive, tone, 0.5, 0.5, treble, level, mode)
    }

    /// `BParFXSkreamerDlx` v0x10: mode + five floats + enable.
    pub fn skreamer_dlx_params(&self) -> Option<SkreamerParams> {
        if self.ser_type != 120 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mode = r.u32().ok()?;
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let bass = r.f32().ok()?;
        let bright = r.f32().ok()?;
        let mix = r.f32().ok()?;
        let _enable = r.u8().ok()?;
        if !(0..=2).contains(&mode)
            || ![drive, tone, bass, bright, mix]
                .iter()
                .all(|v| v.is_finite())
        {
            return None;
        }
        Some(SkreamerParams {
            drive: drive.clamp(0.0, 1.0),
            tone: tone.clamp(0.0, 1.0),
            bass: bass.clamp(0.0, 1.0),
            bright: bright.clamp(0.0, 1.0),
            mix: mix.clamp(0.0, 1.0),
        })
    }

    /// `BParFXKolor` v0x10: mode + eleven floats + enable. The first six are
    /// mapped to drive/tone/bass/mid/treble/level; the remaining fields are
    /// parsed but not yet assigned to distinct DSP stages.
    pub fn kolor_params(&self) -> Option<DriveParams> {
        if self.ser_type != 121 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mode = r.u32().ok()?;
        let mut values = [0.0f32; 11];
        for value in &mut values {
            *value = r.f32().ok()?;
        }
        let enable = r.u8().ok()? != 0;
        if !(0..10).contains(&mode) || !values.iter().all(|v| v.is_finite()) {
            return None;
        }
        drive_params(
            values[0], values[1], values[2], values[3], values[4], values[5], enable,
        )
    }

    /// `BParFXDStortion` v0x10: six floats + mode. Field names are inferred
    /// from the setter's filter/gain structure.
    pub fn dstortion_params(&self) -> Option<DriveParams> {
        if self.ser_type != 86 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let bass = r.f32().ok()?;
        let mid = r.f32().ok()?;
        let treble = r.f32().ok()?;
        let level = r.f32().ok()?;
        let mode = r.u8().ok()? != 0;
        drive_params(drive, tone, bass, mid, treble, level, mode)
    }

    /// `BParFXFuzz` v0x10: three floats + mode.
    pub fn fuzz_params(&self) -> Option<DriveParams> {
        if self.ser_type != 110 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let drive = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let level = r.f32().ok()?;
        let mode = r.u8().ok()? != 0;
        drive_params(drive, tone, 0.5, 0.5, tone, level, mode)
    }

    /// `BParFXBigFuzz` v0x10: four floats + mode.
    pub fn big_fuzz_params(&self) -> Option<DriveParams> {
        if self.ser_type != 111 || self.version != 0x10 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let drive = r.f32().ok()?;
        let bass = r.f32().ok()?;
        let tone = r.f32().ok()?;
        let level = r.f32().ok()?;
        let mode = r.u8().ok()? != 0;
        drive_params(drive, tone, bass, 0.5, tone, level, mode)
    }

    /// `BParFXDistortion` v0x60: type (tube/transistor), drive, damping.
    pub fn distortion_params(&self) -> Option<DistortionParams> {
        if self.ser_type != 30 || self.version != 0x60 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let mode = r.u32().ok()? as i32;
        let drive = r.f32().ok()?;
        let damping = r.f32().ok()?;
        if !(0..=1).contains(&mode) || !drive.is_finite() || !damping.is_finite() {
            return None;
        }
        Some(DistortionParams {
            transistor: mode == 1,
            drive: drive.clamp(0.0, 1.0),
            damping: damping.clamp(0.0, 1.0),
        })
    }

    /// `BParFXSkreamer` v0x50: tone, drive, bass, bright, mix.
    pub fn skreamer_params(&self) -> Option<SkreamerParams> {
        if self.ser_type != 33 || self.version != 0x50 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let tone = r.f32().ok()?;
        let drive = r.f32().ok()?;
        let bass = r.f32().ok()?;
        let bright = r.f32().ok()?;
        let mix = r.f32().ok()?;
        if ![tone, drive, bass, bright, mix]
            .iter()
            .all(|v| v.is_finite())
        {
            return None;
        }
        Some(SkreamerParams {
            tone: tone.clamp(0.0, 1.0),
            drive: drive.clamp(0.0, 1.0),
            bass: bass.clamp(0.0, 1.0),
            bright: bright.clamp(0.0, 1.0),
            mix: mix.clamp(0.0, 1.0),
        })
    }

    /// `BParFXCompressor` v0x70: model, threshold, ratio, attack, release,
    /// stereo link. The stored threshold is a linear amplitude; the runtime
    /// uses `20*log10(threshold)`.
    pub fn compressor_params(&self) -> Option<CompressorParams> {
        if self.ser_type != 25 || self.version != 0x70 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let model = r.u32().ok()? as i32;
        // v0x70 stores the recovered natural units: threshold in the
        // `6*log2`-scaled dB domain, ratio as the compression slope `1/R`.
        // `BEffectCompressor::NatToUnity_*` / `UnityToNat_*` invert these.
        let threshold_db = r.f32().ok()?;
        let ratio_slope = r.f32().ok()?;
        let attack_ms = r.f32().ok()?;
        let release_ms = r.f32().ok()?;
        let stereo_link = r.u8().ok()? != 0;
        if !(0..=2).contains(&model)
            || ![threshold_db, ratio_slope, attack_ms, release_ms]
                .iter()
                .all(|v| v.is_finite())
            || ratio_slope <= 0.0
        {
            return None;
        }
        Some(CompressorParams {
            model,
            threshold_db: threshold_db.clamp(-68.0, 0.0),
            ratio: (1.0 / ratio_slope).clamp(1.0, 25.0),
            attack_ms: attack_ms.clamp(0.0, 2000.0),
            release_ms: release_ms.clamp(0.0, 5000.0),
            stereo_link,
        })
    }

    /// `BParFXChorus`: depth/speed/phase (+ note-value speed).
    pub fn chorus_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 17 || !matches!(self.version, 0x50 | 0x51) {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        modulation(&mut r, 0.0, 0.0)
    }

    /// `BParFXFlanger` v0x50: depth/feedback/speed/phase/base-delay-ms.
    /// The fifth field feeds `BEffectFlanger::SetFlanger` as the base delay
    /// in milliseconds, not a tone colour.
    pub fn flanger_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 18 || !matches!(self.version, 0x50 | 0x51) {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let depth = r.f32().ok()?;
        let feedback = r.f32().ok()?;
        let speed_hz = r.f32().ok()?;
        let phase = r.f32().ok()?;
        let delay_ms = r.f32().ok()?;
        if [depth, feedback, speed_hz, phase, delay_ms]
            .iter()
            .any(|v| !v.is_finite())
        {
            return None;
        }
        Some(ModulationParams {
            depth: depth.clamp(0.0, 1.0),
            speed_hz: speed_hz.clamp(0.0, 40.0),
            phase: phase.rem_euclid(1.0),
            feedback: feedback.clamp(-0.98, 0.98),
            color: delay_ms.clamp(0.0, 100.0),
        })
    }

    /// `BParFXPhaser`: depth/feedback/speed/phase.
    pub fn phaser_params(&self) -> Option<ModulationParams> {
        if self.ser_type != 20 || !matches!(self.version, 0x50 | 0x51) {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let depth = r.f32().ok()?;
        let feedback = r.f32().ok()?;
        let speed_hz = r.f32().ok()?;
        let phase = r.f32().ok()?;
        Some(checked_modulation(depth, speed_hz, phase, feedback, 0.0))
    }

    pub fn stereo_spread(&self) -> Option<(f32, f32)> {
        if self.ser_type != 31 || self.version != 0x70 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let spread = r.f32().ok()?;
        let pan = r.f32().ok()?;
        if !spread.is_finite() || !pan.is_finite() {
            return None;
        }
        Some((spread, pan))
    }

    pub fn limiter(&self) -> Option<(f32, f32)> {
        if self.ser_type != 28 || self.version != 0x60 {
            return None;
        }
        let mut r = Reader::new(&self.raw);
        let in_gain = r.f32().ok()?;
        let release_ms = r.f32().ok()?;
        if !in_gain.is_finite() || !release_ms.is_finite() || !(0.0..=2000.0).contains(&release_ms)
        {
            return None;
        }
        Some((in_gain, release_ms))
    }
}

impl FxChain {
    pub fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let version = r.u16()?;
        let count = if version < 0x13 {
            8
        } else {
            r.u32()?.min(256) as usize
        };
        let mut slots = Vec::with_capacity(count);
        for _ in 0..count {
            let present = r.u8()? != 0;
            if !present {
                slots.push(None);
                continue;
            }
            let object = r.chunked()?;
            slots.push(Some(parse_slot(&object)?));
        }
        Ok(Self { version, slots })
    }

    pub fn stereo_spread(&self) -> Option<(f32, f32)> {
        self.slots
            .iter()
            .flatten()
            .filter(|slot| !slot.bypass)
            .find_map(|slot| slot.child.as_ref().and_then(FxChild::stereo_spread))
    }

    pub fn shaper(&self) -> Option<(f32, i32)> {
        self.slots
            .iter()
            .flatten()
            .filter(|slot| !slot.bypass)
            .find_map(|slot| slot.child.as_ref().and_then(FxChild::shaper))
    }

    pub fn ssl_bands(&self) -> Option<(f32, Vec<SslBand>)> {
        for slot in self.slots.iter().flatten() {
            if slot.bypass {
                continue;
            }
            if let Some(bands) = slot.child.as_ref().and_then(FxChild::ssl_bands) {
                return Some((slot.out_level, bands));
            }
        }
        None
    }

    pub fn simple_filters(&self) -> Vec<SimpleFilter> {
        let mut filters = Vec::new();
        for slot in self.slots.iter().flatten() {
            if slot.bypass {
                continue;
            }
            if let Some(filter) = slot.child.as_ref().and_then(FxChild::simple_filter) {
                filters.push(filter);
            }
        }
        filters
    }

    /// Decode the first non-bypassed filter slot's three-band parameters.
    ///
    /// Version 0x92 filter children store two subtype fix IDs followed by one
    /// (freq, bandwidth, gain) triplet per active band; factory content uses
    /// three triplets for subtype 0x61.
    pub fn filter_bands(&self) -> Option<FilterBands> {
        for slot in self.slots.iter().flatten() {
            if slot.bypass {
                continue;
            }
            let child = slot.child.as_ref()?;
            if child.ser_type != 0x18 || child.version != 0x92 {
                continue;
            }
            let mut r = Reader::new(&child.raw);
            r.i32().ok()?;
            r.i32().ok()?;
            let mut bands = Vec::new();
            while r.data.len().saturating_sub(r.pos) >= 12 {
                let frequency = r.f32().ok()?;
                let bandwidth = r.f32().ok()?;
                let gain = r.f32().ok()?;
                if !frequency.is_finite()
                    || !bandwidth.is_finite()
                    || !gain.is_finite()
                    || !(10.0..=24_000.0).contains(&frequency)
                    || !(0.01..=16.0).contains(&bandwidth)
                    || gain.abs() > 48.0
                {
                    return None;
                }
                bands.push((frequency, bandwidth, gain));
            }
            if !bands.is_empty() {
                return Some((slot.out_level, bands));
            }
        }
        None
    }
}

fn parse_slot(object: &Chunk) -> Result<FxSlot, Error> {
    if object.version != 0x50 {
        return Err(Error::Malformed(format!(
            "unsupported BParFX version {:#x}",
            object.version
        )));
    }
    let mut r = Reader::new(&object.private);
    let fix_type = r.i32()?;
    let mut slot = FxSlot {
        class_id: 0,
        bypass: false,
        out_level: 1.0,
        out_level_dry: 0.0,
        send_partition: -1,
        routers_open: false,
        fix_type,
        object_version: object.version,
        child: None,
    };
    if fix_type != 0 {
        slot.routers_open = r.u8()? != 0;
        slot.class_id = r.i32()?;
        slot.bypass = r.u8()? != 0;
        slot.out_level = r.f32()?;
        slot.out_level_dry = r.f32()?;
        slot.send_partition = r.i32()?;
    }
    if !object.children.is_empty() {
        let mut c = Reader::new(&object.children);
        slot.child = Some(c.chunked()?.into_child()?);
    }
    Ok(slot)
}

fn read_fx_levels(r: &mut Reader<'_>, values: &mut [f32]) -> Result<(), Error> {
    let count = r.i32()?;
    if count < 0 || count as usize > values.len() {
        return Err(Error::Malformed("FX level array exceeds capacity".into()));
    }
    for value in &mut values[..count as usize] {
        *value = r.finite()?;
        if !(0.0..=64.0).contains(value) {
            return Err(Error::Malformed("FX send level out of range".into()));
        }
    }
    Ok(())
}

fn read_ir_envelope(r: &mut Reader<'_>) -> Result<Vec<f32>, Error> {
    let count = r.i32()?;
    if !(0..=8).contains(&count) {
        return Err(Error::Malformed("IRC envelope exceeds eight points".into()));
    }
    (0..count).map(|_| r.finite()).collect()
}

struct Chunk {
    ser_type: u16,
    version: u16,
    private: Vec<u8>,
    public: Vec<u8>,
    children: Vec<u8>,
    raw: Vec<u8>,
}

impl Chunk {
    fn into_child(self) -> Result<FxChild, Error> {
        Ok(FxChild {
            ser_type: self.ser_type,
            version: self.version,
            private: self.private,
            public: self.public,
            children: self.children,
            raw: self.raw,
        })
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated {
            needed: usize::MAX,
            len: self.data.len(),
        })?;
        if end > self.data.len() {
            return Err(Error::Truncated {
                needed: end,
                len: self.data.len(),
            });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        self.u32().map(|v| v as i32)
    }

    fn f32(&mut self) -> Result<f32, Error> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn finite(&mut self) -> Result<f32, Error> {
        let value = self.f32()?;
        if value.is_finite() {
            Ok(value)
        } else {
            Err(Error::Malformed("non-finite FX parameter".into()))
        }
    }

    fn boolean(&mut self) -> Result<bool, Error> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::Malformed("invalid FX boolean".into())),
        }
    }

    fn finish(&self) -> Result<(), Error> {
        if self.pos == self.data.len() {
            Ok(())
        } else {
            Err(Error::Malformed("trailing FX parameter bytes".into()))
        }
    }

    /// `SER::ReadContext::enterChunk` + `SER::Read` + `BSerializableStructured`.
    fn chunked(&mut self) -> Result<Chunk, Error> {
        let ser_type = self.u16()?;
        let size = self.u32()? as usize;
        let start = self.pos;
        let structured = self.u8()? != 0;
        let version = self.u16()?;
        if !structured {
            let remaining = size.saturating_sub(3);
            let raw = self.take(remaining)?.to_vec();
            return Ok(Chunk {
                ser_type,
                version,
                private: Vec::new(),
                public: Vec::new(),
                children: Vec::new(),
                raw,
            });
        }
        if ser_type != 0x25 {
            let raw_len = size.saturating_sub(3);
            let raw = self.take(raw_len)?.to_vec();
            return Ok(Chunk {
                ser_type,
                version,
                private: Vec::new(),
                public: Vec::new(),
                children: Vec::new(),
                raw,
            });
        }
        let private_size = self.u32()? as usize;
        let private = self.take(private_size)?.to_vec();
        let public_size = self.u32()? as usize;
        let public = self.take(public_size)?.to_vec();
        let children_size = self.u32()? as usize;
        let children = self.take(children_size)?.to_vec();
        let consumed = self.pos - start;
        if consumed != size {
            return Err(Error::Malformed(format!(
                "FX chunk size {size} but consumed {consumed}"
            )));
        }
        Ok(Chunk {
            ser_type,
            version,
            private,
            public,
            children,
            raw: Vec::new(),
        })
    }
}
