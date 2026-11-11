//! Ordered program inserts, eight parallel send returns, then master inserts.
//! IR assets and all DSP storage are loaded/prepared off the audio callback.
use super::acbox_dsp::AcBoxAmp;
use super::amp_dsp::JumpAmp;
use super::bassinvader_dsp::BassInvaderAmp;
use super::basspro_amp::BassProAmp;
use super::cabinet_dsp::CabinetState;
use super::cat_amp::CatAmp;
use super::convolution::{Convolution, MAX_IR_SAMPLES};
use super::epiano_amp::EPianoAmp;
use super::hotsolo_dsp::HotSoloAmp;
use super::van51_amp::Van51Amp;
use nk_format::{
    audio::Sample,
    fx::{FxChain, FxSlot, SslBandKind},
    loader::LoadedInstrument,
    stream::SampleReader,
};
use plugin_api::provider::LoadError;

pub(crate) type AuxFrame = [[f32; 2]; 8];
pub(crate) const AUX_CHUNK: usize = 256;

struct Peaking {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

#[derive(Clone, Copy)]
enum BandKind {
    Peaking,
    LowShelf,
    HighShelf,
}

#[derive(Clone, Copy)]
enum ModKind {
    Chorus,
    Flanger,
}

/// Initial `[cos, sin]` quadrature states for both channels. Channel 0 uses
/// the serialized phase; channel 1 is offset by a quarter cycle.
fn oscillator_start(phase: f32) -> [[f32; 2]; 2] {
    let base = std::f32::consts::TAU * phase;
    let quarter = base + std::f32::consts::FRAC_PI_2;
    [[base.cos(), base.sin()], [quarter.cos(), quarter.sin()]]
}

/// Four-point cubic interpolation used by the native flanger/chorus delay
/// read (`f = frac + 1`, points `i-2 .. i+1`).
fn cubic_read(buffer: &[[f32; 2]], write: usize, delay: f32, ch: usize) -> f32 {
    let len = buffer.len();
    let delay = delay.clamp(2.0, (len - 3) as f32);
    let whole = delay.floor() as usize;
    let frac = delay - whole as f32;
    let i = (write + len - whole) % len;
    let x_m2 = buffer[(i + len - 2) % len][ch];
    let x_m1 = buffer[(i + len - 1) % len][ch];
    let x_0 = buffer[i][ch];
    let x_p1 = buffer[(i + 1) % len][ch];
    let f = frac + 1.0;
    let t = frac;
    (x_p1 * f * t * (t - 1.0) + (t - 1.0) * -x_m2 * t * (t - 1.0)) * (1.0 / 6.0)
        + (t - 1.0) * ((x_m1 * f * (t - 1.0) - x_0 * f * t) * 0.5)
}

#[derive(Clone, Copy)]
struct Band {
    kind: BandKind,
    frequency: f32,
    gain_db: f32,
    /// Direct Q for SSL bands; zero when `bandwidth` is used instead.
    q: f32,
    /// Octave bandwidth for the three-band peaking filter.
    bandwidth: f32,
}

impl Band {
    fn peaking(frequency: f32, bandwidth: f32, gain_db: f32) -> Self {
        Self {
            kind: BandKind::Peaking,
            frequency,
            gain_db,
            q: 0.0,
            bandwidth,
        }
    }

    fn from_ssl(band: nk_format::fx::SslBand) -> Self {
        Self {
            kind: match band.kind {
                SslBandKind::LowShelf => BandKind::LowShelf,
                SslBandKind::Peaking => BandKind::Peaking,
                SslBandKind::HighShelf => BandKind::HighShelf,
            },
            frequency: band.frequency,
            gain_db: band.gain_db,
            q: band.q.max(0.1),
            bandwidth: 0.0,
        }
    }
}
enum Processor {
    Shaper {
        params: nk_format::fx::ShaperParams,
        state: super::shaper_dsp::ShaperState,
        rate: f32,
    },
    NativeFilter {
        filter: nk_format::fx::NativeFilter,
        state: Box<super::filter_dsp::NativeFilterState>,
        rate: f32,
    },
    Gainer(f32),
    Inverter([bool; 2]),
    Delay {
        time_ms: f32,
        feedback: f32,
        pan: f32,
        damping: f32,
        rate: f32,
        buffer: Vec<[f32; 2]>,
        index: usize,
        damp: [f32; 2],
    },
    ModulatedDelay {
        kind: ModKind,
        params: nk_format::fx::ModulationParams,
        rate: f32,
        buffer: Vec<[f32; 2]>,
        index: usize,
        /// Quadrature sine state `[cos, sin]` per channel.
        osc: [[f32; 2]; 2],
    },
    Phaser {
        params: nk_format::fx::ModulationParams,
        rate: f32,
        /// Quadrature sine state `[cos, sin]` per channel.
        osc: [[f32; 2]; 2],
        /// Four first-order all-pass outputs and previous inputs, per stage.
        ap_y: [[f32; 2]; 4],
        ap_x: [[f32; 2]; 4],
    },
    Compressor {
        model: i32,
        threshold_db: f32,
        ratio: f32,
        attack_ms: f32,
        release_ms: f32,
        stereo_link: bool,
        rate: f32,
        /// Normalized gain reduction in `[0, 1]`; classic/Pro paths.
        reduction: f32,
        /// Enhanced model's two-pole envelope and release hold, per channel.
        env1: [f32; 2],
        env2: [f32; 2],
        env_hold: [f32; 2],
    },
    Distortion {
        transistor: bool,
        drive: f32,
        damping: f32,
        state: [f32; 2],
    },
    Skreamer {
        tone: f32,
        drive: f32,
        bass: f32,
        bright: f32,
        mix: f32,
        tone_state: [f32; 2],
    },
    MultiDrive {
        drive: f32,
        bass: f32,
        mid: f32,
        treble: f32,
        level: f32,
        low_a: f32,
        mid_a: f32,
        low_state: [f32; 2],
        mid_state: [f32; 2],
    },
    Gater {
        rate_hz: f32,
        depth: f32,
        phase: f32,
        sample_rate: f32,
    },
    Transient {
        attack: f32,
        sustain: f32,
        gain: f32,
        rate: f32,
        fast: f32,
        slow: f32,
    },
    BeatRepeat {
        buffer: Vec<[f32; 2]>,
        index: usize,
        len: usize,
        mix: f32,
        captured: bool,
        rate: f32,
    },
    RingMod {
        rate_hz: f32,
        depth: f32,
        phase: f32,
        sample_rate: f32,
    },
    LoFi {
        downsample: f32,
        bits: f32,
        mix: f32,
        tone: f32,
        hold: [f32; 2],
        counter: u32,
        tone_state: [f32; 2],
    },
    Rotator {
        rate_hz: f32,
        depth: f32,
        phase: f32,
        sample_rate: f32,
    },
    Wah {
        cutoff: f32,
        q: f32,
        coeffs: Peaking,
        states: [[f32; 2]; 2],
    },
    Pan(f32),
    Reverb {
        lines: Vec<Vec<[f32; 2]>>,
        positions: [usize; 4],
        damps: [[f32; 2]; 4],
        decay: f32,
        damping: f32,
        mix: f32,
        rate: f32,
        /// `BParFXReverb` pre-delay (ms) and line.
        pre_delay_ms: f32,
        pre_buffer: Vec<[f32; 2]>,
        pre_index: usize,
        room_size: f32,
        color: f32,
        stereo: f32,
        filter: f32,
        tone_lp: [f32; 2],
        /// True when the recovered `BParFXReverb` fields drive the network.
        recovered: bool,
    },
    ReverseGrain {
        buffer: Vec<[f32; 2]>,
        index: usize,
        len: usize,
        mix: f32,
        captured: bool,
        rate: f32,
    },
    Bands {
        bands: Vec<Band>,
        coeffs: Vec<Peaking>,
        states: Vec<[[f32; 2]; 2]>,
    },
    Spread {
        width: f32,
        pan: f32,
    },
    Limiter {
        gain: f32,
        release_ms: f32,
        release_coeff: f32,
        envelope: f32,
    },
    Send([f32; 8]),
    Convolution(Box<Convolution>),
    Cabinet(Box<CabinetState>),
    AmpJump(Box<JumpAmp>),
    AmpVan51(Box<Van51Amp>),
    AmpCat(Box<CatAmp>),
    AmpEPiano(Box<EPianoAmp>),
    AmpBassInvader(Box<BassInvaderAmp>),
    AmpHotSolo(Box<HotSoloAmp>),
    AmpBassPro(Box<BassProAmp>),
    AmpAcBox(Box<AcBoxAmp>),
    Gain,
}
struct Stage {
    processor: Processor,
    wet: f32,
    dry: f32,
}

fn compressor(params: nk_format::fx::CompressorParams) -> Processor {
    Processor::Compressor {
        model: params.model,
        threshold_db: params.threshold_db,
        ratio: params.ratio,
        attack_ms: params.attack_ms,
        release_ms: params.release_ms,
        stereo_link: params.stereo_link,
        rate: 48_000.0,
        reduction: 0.0,
        env1: [0.0; 2],
        env2: [0.0; 2],
        env_hold: [0.0; 2],
    }
}

fn multi_drive(params: nk_format::fx::DriveParams) -> Processor {
    Processor::MultiDrive {
        drive: params.drive,
        bass: params.bass,
        mid: params.mid,
        treble: params.treble,
        level: params.level,
        low_a: 0.0,
        mid_a: 0.0,
        low_state: [0.0; 2],
        mid_state: [0.0; 2],
    }
}
impl Stage {
    fn bands(bands: Vec<Band>) -> Processor {
        let coeffs = bands
            .iter()
            .map(|_| Peaking {
                b0: 1.0,
                b1: 0.0,
                b2: 0.0,
                a1: 0.0,
                a2: 0.0,
            })
            .collect();
        let states = vec![[[0.0; 2]; 2]; bands.len()];
        Processor::Bands {
            bands,
            coeffs,
            states,
        }
    }
    fn compile(
        slot: &FxSlot,
        load: &mut impl FnMut(usize) -> Result<Sample, LoadError>,
    ) -> Result<Option<Self>, LoadError> {
        if slot.bypass {
            return Ok(None);
        }
        let Some(child) = &slot.child else {
            return Ok(None);
        };
        if !slot.out_level.is_finite()
            || !slot.out_level_dry.is_finite()
            || slot.out_level.abs() > 64.0
            || slot.out_level_dry.abs() > 64.0
        {
            return Err(LoadError::Failed("FX wet/dry gain is out of range".into()));
        }
        let fail = |e: nk_format::Error| LoadError::Failed(e.to_string());
        let mut dry = 0.0;
        let processor = if let Some(filter) = child.native_filter().map_err(fail)? {
            super::filter_dsp::initialize();
            dry = slot.out_level_dry;
            Processor::NativeFilter {
                filter,
                state: Default::default(),
                rate: 48_000.0,
            }
        } else if let Some(pan) = child.surround_panner_params() {
            dry = slot.out_level_dry;
            Processor::Pan(pan)
        } else if let Some(params) = child.shaper_params().map_err(fail)? {
            // Shaper's SetObj supplies only out_level. Its Process never
            // prepares the base dry cache (unlike IRC), even at neutral shape.
            Processor::Shaper {
                params,
                state: Default::default(),
                rate: 48_000.0,
            }
        } else if let Some(gain) = child.gainer() {
            Processor::Gainer(gain)
        } else if let Some(invert) = child.inverter() {
            Processor::Inverter(invert)
        } else if let Some(params) = child.delay_params() {
            dry = slot.out_level_dry;
            Processor::Delay {
                time_ms: params.time_ms,
                feedback: params.feedback,
                pan: params.pan,
                damping: params.damping,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                damp: [0.0; 2],
            }
        } else if let Some(params) = child.chorus_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.flanger_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Flanger,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.phaser_params() {
            dry = slot.out_level_dry;
            Processor::Phaser {
                params,
                rate: 48_000.0,
                osc: [[0.0; 2]; 2],
                ap_y: [[0.0; 2]; 4],
                ap_x: [[0.0; 2]; 4],
            }
        } else if let Some(params) = child.compressor_params() {
            dry = slot.out_level_dry;
            Processor::Compressor {
                model: params.model,
                threshold_db: params.threshold_db,
                ratio: params.ratio,
                attack_ms: params.attack_ms,
                release_ms: params.release_ms,
                stereo_link: params.stereo_link,
                rate: 48_000.0,
                reduction: 0.0,
                env1: [0.0; 2],
                env2: [0.0; 2],
                env_hold: [0.0; 2],
            }
        } else if let Some(params) = child.distortion_params() {
            dry = slot.out_level_dry;
            Processor::Distortion {
                transistor: params.transistor,
                drive: params.drive,
                damping: params.damping,
                state: [0.0; 2],
            }
        } else if let Some(params) = child.skreamer_params() {
            Processor::Skreamer {
                tone: params.tone,
                drive: params.drive,
                bass: params.bass,
                bright: params.bright,
                mix: params.mix,
                tone_state: [0.0; 2],
            }
        } else if let Some(params) = child.dstortion_params() {
            multi_drive(params)
        } else if let Some(params) = child.fuzz_params() {
            multi_drive(params)
        } else if let Some(params) = child.big_fuzz_params() {
            multi_drive(params)
        } else if let Some(params) = child.chainsaw_params() {
            multi_drive(params)
        } else if let Some(params) = child.skreamer_dlx_params() {
            Processor::Skreamer {
                tone: params.tone,
                drive: params.drive,
                bass: params.bass,
                bright: params.bright,
                mix: params.mix,
                tone_state: [0.0; 2],
            }
        } else if let Some(params) = child.kolor_params() {
            multi_drive(params)
        } else if let Some(params) = child.replika_delay_params() {
            dry = slot.out_level_dry;
            Processor::Delay {
                time_ms: params.time_ms,
                feedback: params.feedback,
                pan: params.pan,
                damping: params.damping,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                damp: [0.0; 2],
            }
        } else if let Some((rate_hz, depth)) = child.gater_params() {
            dry = slot.out_level_dry;
            Processor::Gater {
                rate_hz,
                depth,
                phase: 0.0,
                sample_rate: 48_000.0,
            }
        } else if let Some(params) = child.vibrato_chorus_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.wow_flutter_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.tape_params() {
            multi_drive(params)
        } else if let Some(params) = child.trans_params() {
            Processor::Transient {
                attack: params.attack,
                sustain: params.sustain,
                gain: params.gain,
                rate: 48_000.0,
                fast: 0.0,
                slow: 0.0,
            }
        } else if let Some(params) = child.psyche_delay_params() {
            dry = slot.out_level_dry;
            Processor::Delay {
                time_ms: params.time_ms,
                feedback: params.feedback,
                pan: params.pan,
                damping: params.damping,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                damp: [0.0; 2],
            }
        } else if let Some(params) = child.twin_delay_params() {
            dry = slot.out_level_dry;
            Processor::Delay {
                time_ms: params.time_ms,
                feedback: params.feedback,
                pan: params.pan,
                damping: params.damping,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                damp: [0.0; 2],
            }
        } else if let Some(params) = child.super_fast_params() {
            multi_drive(params)
        } else if let Some(params) = child.stereo_tune_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some((length, mix)) = child.beat_masher_params() {
            Processor::BeatRepeat {
                buffer: Vec::new(),
                index: 0,
                len: ((length * 0.5 * 48_000.0) as usize).max(1),
                mix,
                captured: false,
                rate: 48_000.0,
            }
        } else if let Some((rate, depth)) = child.beat_slicer_params() {
            dry = slot.out_level_dry;
            Processor::Gater {
                rate_hz: rate,
                depth,
                phase: 0.0,
                sample_rate: 48_000.0,
            }
        } else if let Some((length, mix)) = child.reverse_grain_params() {
            Processor::ReverseGrain {
                buffer: Vec::new(),
                index: 0,
                len: ((length * 0.5 * 48_000.0) as usize).max(1),
                mix,
                captured: false,
                rate: 48_000.0,
            }
        } else if let Some(params) = child.transpose_stretch_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.phasis_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.flair_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.choral_params() {
            dry = slot.out_level_dry;
            Processor::ModulatedDelay {
                kind: ModKind::Chorus,
                params,
                rate: 48_000.0,
                buffer: Vec::new(),
                index: 0,
                osc: [[0.0; 2]; 2],
            }
        } else if let Some((rate_hz, depth)) = child.ring_mod_params() {
            dry = slot.out_level_dry;
            Processor::RingMod {
                rate_hz,
                depth,
                phase: 0.0,
                sample_rate: 48_000.0,
            }
        } else if let Some((downsample, bits, mix, tone)) = child.lo_fi_params() {
            dry = slot.out_level_dry;
            Processor::LoFi {
                downsample,
                bits,
                mix,
                tone,
                hold: [0.0; 2],
                counter: 0,
                tone_state: [0.0; 2],
            }
        } else if let Some((rate_hz, depth, phase)) = child.rotator_params() {
            dry = slot.out_level_dry;
            Processor::Rotator {
                rate_hz,
                depth,
                phase,
                sample_rate: 48_000.0,
            }
        } else if let Some(params) = child.dyx_params() {
            dry = slot.out_level_dry;
            compressor(params)
        } else if let Some(params) = child.ssl_bus_comp_params() {
            dry = slot.out_level_dry;
            compressor(params)
        } else if let Some(params) = child.fb_comp_params() {
            dry = slot.out_level_dry;
            compressor(params)
        } else if let Some(params) = child.hilbert_limiter_params() {
            dry = slot.out_level_dry;
            compressor(params)
        } else if let Some(params) = child.supercharger_params() {
            dry = slot.out_level_dry;
            compressor(params)
        } else if let Some(params) = child.twang_params() {
            multi_drive(params)
        } else if let Some((position, _enable)) = child.cry_wah_params() {
            dry = slot.out_level_dry;
            Processor::Wah {
                cutoff: 200.0 * 2f32.powf(position * 5.0),
                q: 3.0,
                coeffs: Peaking {
                    b0: 1.0,
                    b1: 0.0,
                    b2: 0.0,
                    a1: 0.0,
                    a2: 0.0,
                },
                states: [[0.0; 2]; 2],
            }
        } else if let Some(params) = child.bite_params() {
            multi_drive(params)
        } else if let Some(params) = child.dirt_params() {
            multi_drive(params)
        } else if let Some(params) = child.freak_params() {
            multi_drive(params)
        } else if let Some(params) = child.cabinet_params() {
            Processor::Cabinet(Box::new(CabinetState::new(params)))
        } else if let Some(settings) = child.amp_settings() {
            match settings.kind {
                nk_format::fx::AmpKind::Jump => {
                    Processor::AmpJump(Box::new(JumpAmp::new(settings)))
                }
                nk_format::fx::AmpKind::Van51 => {
                    Processor::AmpVan51(Box::new(Van51Amp::new(settings)))
                }
                nk_format::fx::AmpKind::Cat => Processor::AmpCat(Box::new(CatAmp::new(settings))),
                nk_format::fx::AmpKind::EpPreamps => {
                    Processor::AmpEPiano(Box::new(EPianoAmp::new(settings)))
                }
                nk_format::fx::AmpKind::BassInvader => {
                    Processor::AmpBassInvader(Box::new(BassInvaderAmp::new(settings)))
                }
                nk_format::fx::AmpKind::HotSolo => {
                    Processor::AmpHotSolo(Box::new(HotSoloAmp::new(settings)))
                }
                nk_format::fx::AmpKind::BassPro => {
                    Processor::AmpBassPro(Box::new(BassProAmp::new(settings)))
                }
                nk_format::fx::AmpKind::AcBox => {
                    Processor::AmpAcBox(Box::new(AcBoxAmp::new(settings)))
                }
            }
        } else if let Some(params) = child.core_cell_params() {
            multi_drive(params)
        } else if let Some(params) = child.base_reverb_params() {
            dry = slot.out_level_dry;
            Processor::Reverb {
                lines: Vec::new(),
                positions: [0; 4],
                damps: [[0.0; 2]; 4],
                decay: 0.0,
                damping: 0.0,
                mix: 1.0,
                rate: 48_000.0,
                pre_delay_ms: params.pre_delay_ms,
                pre_buffer: Vec::new(),
                pre_index: 0,
                room_size: params.room_size,
                color: params.color,
                stereo: params.stereo,
                filter: params.filter,
                tone_lp: [0.0; 2],
                recovered: true,
            }
        } else if let Some((decay, damping, mix)) = child.reverb_params() {
            dry = slot.out_level_dry;
            Processor::Reverb {
                lines: Vec::new(),
                positions: [0; 4],
                damps: [[0.0; 2]; 4],
                decay,
                damping,
                mix,
                rate: 48_000.0,
                pre_delay_ms: 0.0,
                pre_buffer: Vec::new(),
                pre_index: 0,
                room_size: 0.0,
                color: 0.0,
                stereo: 0.0,
                filter: 0.0,
                tone_lp: [0.0; 2],
                recovered: false,
            }
        } else if let Some(bands) = child.filter_bands() {
            Self::bands(
                bands
                    .into_iter()
                    .map(|(frequency, bandwidth, gain_db)| {
                        Band::peaking(frequency, bandwidth, gain_db)
                    })
                    .collect(),
            )
        } else if let Some((spread, pan)) = child.stereo_spread() {
            Processor::Spread {
                width: (1.0 + spread).clamp(0.0, 2.0),
                pan: pan.clamp(-1.0, 1.0),
            }
        } else if let Some((gain, release_ms)) = child.limiter() {
            Processor::Limiter {
                gain,
                release_ms,
                release_coeff: 1.0,
                envelope: 1.0,
            }
        } else if let Some(bands) = child.ssl_bands() {
            Self::bands(bands.into_iter().map(Band::from_ssl).collect())
        } else if let Some(levels) = child.send_levels().map_err(fail)? {
            Processor::Send(levels.levels)
        } else if let Some(params) = child.convolution().map_err(fail)? {
            dry = slot.out_level_dry;
            if params.filename_id < -1 {
                return Err(LoadError::Failed("invalid IR filename index".into()));
            }
            if params.filename_id == -1 {
                return Ok(Some(Self {
                    processor: Processor::Gain,
                    wet: dry,
                    dry: 0.0,
                }));
            }
            let source = load(params.filename_id as usize)?;
            Processor::Convolution(Box::new(
                Convolution::new(source, params).map_err(|e| LoadError::Failed(e.into()))?,
            ))
        } else {
            return Ok(None);
        };
        Ok(Some(Self {
            processor,
            wet: slot.out_level,
            dry,
        }))
    }
    fn prepare(&mut self, rate: f32, budget: &mut usize) -> Result<(), &'static str> {
        match &mut self.processor {
            Processor::NativeFilter { rate: current, .. } => *current = rate,
            Processor::Shaper { rate: current, .. } => *current = rate,
            Processor::Cabinet(state) => state.prepare(rate)?,
            Processor::AmpJump(state) => state.prepare(rate)?,
            Processor::AmpVan51(state) => state.prepare(rate)?,
            Processor::AmpCat(state) => state.prepare(rate)?,
            Processor::AmpEPiano(state) => state.prepare(rate)?,
            Processor::AmpBassInvader(state) => state.prepare(rate)?,
            Processor::AmpHotSolo(state) => state.prepare(rate)?,
            Processor::AmpBassPro(state) => state.prepare(rate)?,
            Processor::AmpAcBox(state) => state.prepare(rate)?,
            Processor::Bands { bands, coeffs, .. } => {
                for (band, c) in bands.iter().zip(coeffs) {
                    band_coeffs(*band, rate, c);
                }
            }
            Processor::Delay {
                time_ms,
                rate: stored_rate,
                buffer,
                index,
                damp,
                ..
            } => {
                *stored_rate = rate;
                let frames = ((*time_ms / 1000.0) * rate).ceil() as usize;
                let wanted = frames.max(1) + 1;
                if buffer.len() < wanted {
                    buffer.resize(wanted, [0.0; 2]);
                }
                buffer.fill([0.0; 2]);
                *index = 0;
                *damp = [0.0; 2];
            }
            Processor::ModulatedDelay {
                kind,
                params,
                rate: stored_rate,
                buffer,
                index,
                osc,
            } => {
                *stored_rate = rate;
                // Chorus base delay is a fixed 20 ms; flanger's base delay
                // is its fifth serialized field, in milliseconds.
                let base_ms = match kind {
                    ModKind::Chorus => 20.0,
                    ModKind::Flanger => params.color.clamp(0.0, 100.0),
                };
                // Delay can swing from `base - depth` to `base + depth`.
                let base_samples = ((base_ms / 1000.0) * rate).min(4096.0) as usize;
                let frames = base_samples * 2 + 160;
                if buffer.len() < frames {
                    buffer.resize(frames.max(8), [0.0; 2]);
                }
                buffer.fill([0.0; 2]);
                *index = 0;
                *osc = oscillator_start(params.phase);
            }
            Processor::Phaser {
                params,
                rate: stored_rate,
                osc,
                ap_y,
                ap_x,
            } => {
                *stored_rate = rate;
                *osc = oscillator_start(params.phase);
                *ap_y = [[0.0; 2]; 4];
                *ap_x = [[0.0; 2]; 4];
            }
            Processor::Reverb {
                lines,
                positions,
                damps,
                rate: stored_rate,
                pre_buffer,
                pre_index,
                tone_lp,
                recovered,
                ..
            } => {
                *stored_rate = rate;
                let lengths = [1557usize, 1617, 1491, 1422];
                if lines.len() != 4 {
                    lines.clear();
                    lines.extend(lengths.iter().map(|_| Vec::new()));
                }
                for (line, length) in lines.iter_mut().zip(lengths) {
                    let frames = ((length as f32) * rate / 48_000.0).round() as usize;
                    if line.len() < frames.max(1) {
                        line.resize(frames.max(1), [0.0; 2]);
                    }
                    line.fill([0.0; 2]);
                }
                *positions = [0; 4];
                *damps = [[0.0; 2]; 4];
                *tone_lp = [0.0; 2];
                if *recovered {
                    let frames = ((0.2 * rate) as usize).max(2) + 2;
                    if pre_buffer.len() < frames {
                        pre_buffer.resize(frames, [0.0; 2]);
                    }
                    pre_buffer.fill([0.0; 2]);
                    *pre_index = 0;
                }
            }
            Processor::Wah {
                cutoff,
                q,
                coeffs,
                states,
            } => {
                bandpass_coeffs(*cutoff, *q, rate, coeffs);
                *states = [[0.0; 2]; 2];
            }
            Processor::RingMod {
                phase, sample_rate, ..
            } => {
                *phase = 0.0;
                *sample_rate = rate;
            }
            Processor::LoFi {
                counter,
                hold,
                tone_state,
                ..
            } => {
                *counter = 0;
                *hold = [0.0; 2];
                *tone_state = [0.0; 2];
            }
            Processor::Rotator {
                phase, sample_rate, ..
            } => {
                *phase = 0.0;
                *sample_rate = rate;
            }
            Processor::BeatRepeat {
                buffer,
                index,
                captured,
                rate: stored_rate,
                ..
            } => {
                *stored_rate = rate;
                *index = 0;
                *captured = false;
                buffer.clear();
            }
            Processor::ReverseGrain {
                buffer,
                index,
                captured,
                rate: stored_rate,
                ..
            } => {
                *stored_rate = rate;
                *index = 0;
                *captured = false;
                buffer.clear();
            }
            Processor::Transient {
                rate: stored_rate,
                fast,
                slow,
                ..
            } => {
                *stored_rate = rate;
                *fast = 0.0;
                *slow = 0.0;
            }
            Processor::Gater {
                phase, sample_rate, ..
            } => {
                *phase = 0.0;
                *sample_rate = rate;
            }
            Processor::MultiDrive {
                low_a,
                mid_a,
                low_state,
                mid_state,
                ..
            } => {
                *low_a = 1.0 - (-std::f32::consts::TAU * 300.0 / rate).exp();
                *mid_a = 1.0 - (-std::f32::consts::TAU * 3000.0 / rate).exp();
                *low_state = [0.0; 2];
                *mid_state = [0.0; 2];
            }
            Processor::Compressor {
                rate: stored_rate,
                reduction,
                env1,
                env2,
                env_hold,
                ..
            } => {
                *stored_rate = rate;
                *reduction = 0.0;
                *env1 = [0.0; 2];
                *env2 = [0.0; 2];
                *env_hold = [0.0; 2];
            }
            Processor::Limiter {
                release_ms,
                release_coeff,
                ..
            } => {
                // BEffectLimiter::CopyCoefficients: linear `ln2/N` ramp,
                // not an exponential one-pole.
                *release_coeff = if *release_ms <= 0.0 {
                    1.0
                } else {
                    (std::f32::consts::LN_2 / (*release_ms * rate / 1000.0)).min(1.0)
                };
            }
            Processor::Convolution(c) => c.prepare(rate, budget)?,
            _ => {}
        }
        Ok(())
    }
    fn reset(&mut self) {
        match &mut self.processor {
            Processor::NativeFilter { state, .. } => **state = Default::default(),
            Processor::Shaper { state, .. } => *state = Default::default(),
            Processor::Bands { states, .. } => states.fill([[0.0; 2]; 2]),
            Processor::Delay {
                buffer,
                index,
                damp,
                ..
            } => {
                buffer.fill([0.0; 2]);
                *index = 0;
                *damp = [0.0; 2];
            }
            Processor::ModulatedDelay {
                buffer,
                index,
                osc,
                params,
                ..
            } => {
                buffer.fill([0.0; 2]);
                *index = 0;
                *osc = oscillator_start(params.phase);
            }
            Processor::Phaser {
                osc,
                ap_y,
                ap_x,
                params,
                ..
            } => {
                *osc = oscillator_start(params.phase);
                *ap_y = [[0.0; 2]; 4];
                *ap_x = [[0.0; 2]; 4];
            }
            Processor::Compressor {
                reduction,
                env1,
                env2,
                env_hold,
                ..
            } => {
                *reduction = 0.0;
                *env1 = [0.0; 2];
                *env2 = [0.0; 2];
                *env_hold = [0.0; 2];
            }
            Processor::Distortion { state, .. } => *state = [0.0; 2],
            Processor::Gater { phase, .. } => *phase = 0.0,
            Processor::Transient { fast, slow, .. } => {
                *fast = 0.0;
                *slow = 0.0;
            }
            Processor::RingMod { phase, .. } => *phase = 0.0,
            Processor::Wah { states, .. } => *states = [[0.0; 2]; 2],
            Processor::Pan(_) => {}
            Processor::Reverb {
                lines,
                positions,
                damps,
                pre_buffer,
                pre_index,
                tone_lp,
                ..
            } => {
                for line in lines.iter_mut() {
                    line.fill([0.0; 2]);
                }
                *positions = [0; 4];
                *damps = [[0.0; 2]; 4];
                pre_buffer.fill([0.0; 2]);
                *pre_index = 0;
                *tone_lp = [0.0; 2];
            }
            Processor::LoFi {
                counter,
                hold,
                tone_state,
                ..
            } => {
                *counter = 0;
                *hold = [0.0; 2];
                *tone_state = [0.0; 2];
            }
            Processor::Rotator { phase, .. } => *phase = 0.0,
            Processor::BeatRepeat {
                buffer,
                index,
                captured,
                ..
            } => {
                buffer.clear();
                *index = 0;
                *captured = false;
            }
            Processor::ReverseGrain {
                buffer,
                index,
                captured,
                ..
            } => {
                buffer.clear();
                *index = 0;
                *captured = false;
            }
            Processor::MultiDrive {
                low_state,
                mid_state,
                ..
            } => {
                *low_state = [0.0; 2];
                *mid_state = [0.0; 2];
            }
            Processor::Skreamer { tone_state, .. } => *tone_state = [0.0; 2],
            Processor::Limiter { envelope, .. } => *envelope = 1.0,
            Processor::Convolution(c) => c.reset(),
            Processor::AmpJump(state) => state.reset(),
            Processor::AmpVan51(state) => state.reset(),
            Processor::AmpCat(state) => state.reset(),
            Processor::AmpEPiano(state) => state.reset(),
            Processor::AmpBassInvader(state) => state.reset(),
            Processor::AmpHotSolo(state) => state.reset(),
            Processor::AmpBassPro(state) => state.reset(),
            Processor::AmpAcBox(state) => state.reset(),
            _ => {}
        }
    }
    fn tail(&self) -> usize {
        if let Processor::Convolution(c) = &self.processor {
            c.tail()
        } else {
            0
        }
    }
    fn step(&mut self, input: [f32; 2], aux: &mut AuxFrame) -> [f32; 2] {
        let mut output = input;
        match &mut self.processor {
            Processor::Shaper {
                params,
                state,
                rate,
            } => output = state.step(*params, *rate, input),
            Processor::Cabinet(state) => output = state.process(input[0], input[1]),
            Processor::AmpJump(state) => output = state.process(input[0], input[1]),
            Processor::AmpVan51(state) => output = state.process(input[0], input[1]),
            Processor::AmpCat(state) => output = state.process(input[0], input[1]),
            Processor::AmpEPiano(state) => output = state.process(input[0], input[1]),
            Processor::AmpBassInvader(state) => output = state.process(input[0], input[1]),
            Processor::AmpHotSolo(state) => output = state.process(input[0], input[1]),
            Processor::AmpBassPro(state) => output = state.process(input[0], input[1]),
            Processor::AmpAcBox(state) => output = state.process(input[0], input[1]),
            Processor::NativeFilter {
                filter,
                state,
                rate,
            } => {
                output = state.step(*filter, filter.cutoff, 0.0, *rate, input);
            }
            Processor::Gainer(gain) => {
                output[0] *= *gain;
                output[1] *= *gain;
            }
            Processor::Inverter(invert) => {
                if invert[0] {
                    output[0] = -output[0];
                }
                if invert[1] {
                    output[1] = -output[1];
                }
            }
            Processor::Delay {
                feedback,
                pan,
                damping,
                rate,
                buffer,
                index,
                damp,
                time_ms,
            } => {
                let frames = ((*time_ms / 1000.0) * *rate).round() as usize;
                if frames == 0 || buffer.len() <= frames {
                    output = [0.0; 2];
                } else {
                    let read = (*index + buffer.len() - frames) % buffer.len();
                    let wet = buffer[read];
                    let coeff = 1.0 - damping.clamp(0.0, 0.999);
                    for ch in 0..2 {
                        damp[ch] += (wet[ch] - damp[ch]) * coeff;
                        buffer[*index][ch] = input[ch] + damp[ch] * *feedback;
                    }
                    *index = (*index + 1) % buffer.len();
                    let left = wet[0] * (1.0 - *pan).max(0.0);
                    let right = wet[1] * (1.0 + *pan).max(0.0);
                    output = [left, right];
                }
            }
            Processor::ModulatedDelay {
                kind,
                params,
                rate,
                buffer,
                index,
                osc,
            } => {
                let frames = buffer.len();
                if frames < 8 {
                    output = [0.0; 2];
                } else {
                    let k = 2.0 * (std::f32::consts::PI * params.speed_hz / *rate).sin();
                    match kind {
                        ModKind::Flanger => {
                            // `BEffectFlanger::SetFlanger`: fifth field is the
                            // base delay in ms, depth sweeps up to 50 samples.
                            let base =
                                (params.color.clamp(0.0, 100.0) * 0.001 * *rate).clamp(2.0, 4096.0);
                            let depth = (params.depth * 50.0).clamp(0.0, (base - 1.0).max(0.0));
                            buffer[*index] = input;
                            for ch in 0..2 {
                                let c = osc[ch][0];
                                let s = osc[ch][1];
                                let cn = c - s * k;
                                let sn = s + cn * k;
                                osc[ch] = [cn, sn];
                                let y = cubic_read(buffer, *index, base + depth * sn, ch);
                                // Native writes `input - 0.95*feedback*output`.
                                buffer[*index][ch] = input[ch] - 0.95 * params.feedback * y;
                                output[ch] = y;
                            }
                        }
                        ModKind::Chorus => {
                            // Fixed 20 ms base delay, depth sweeps up to 75
                            // samples, two taps 0.015 cycles apart.
                            let base = 0.020 * *rate;
                            let depth = params.depth * 75.0;
                            let delta = std::f32::consts::TAU * 0.015;
                            let (delta_s, delta_c) = (delta.sin(), delta.cos());
                            for ch in 0..2 {
                                let c = osc[ch][0];
                                let s = osc[ch][1];
                                let cn = c - s * k;
                                let sn = s + cn * k;
                                osc[ch] = [cn, sn];
                                buffer[*index][ch] = input[ch];
                                let y1 = cubic_read(buffer, *index, base + depth * sn, ch);
                                let s2 = sn * delta_c + cn * delta_s;
                                let y2 = cubic_read(buffer, *index, base + depth * s2, ch);
                                output[ch] = y1 + y2;
                            }
                        }
                    }
                    *index = (*index + 1) % frames;
                }
            }
            Processor::Phaser {
                params,
                rate,
                osc,
                ap_y,
                ap_x,
            } => {
                // `BEffectPhaser::Process`: four first-order all-pass stages,
                // quadrature LFO, exponential coefficient curve.
                let k = 2.0 * (std::f32::consts::PI * params.speed_hz / *rate).sin();
                let feedback = params.feedback * 0.85;
                for ch in 0..2 {
                    let c = osc[ch][0];
                    let s = osc[ch][1];
                    let cn = c - s * k;
                    let sn = s + cn * k;
                    osc[ch] = [cn, sn];
                    let x = cn * params.depth * 3.0;
                    let t = (x.exp() - 1.0) * 0.054_310_93 * 0.314_166_6 + 0.019_166_66;
                    let a = (1.0 - t) / (1.0 + t);
                    let mut v = input[ch] + ap_y[3][ch] * feedback;
                    for stage in 0..4 {
                        let y = a * (ap_y[stage][ch] + v) - ap_x[stage][ch];
                        ap_x[stage][ch] = v;
                        ap_y[stage][ch] = y;
                        v = y;
                    }
                    output[ch] = v;
                }
            }
            Processor::Bands { coeffs, states, .. } => {
                for (c, state) in coeffs.iter().zip(states) {
                    output[0] = biquad_step(output[0], &mut state[0], c);
                    output[1] = biquad_step(output[1], &mut state[1], c);
                }
            }
            Processor::Spread { width, pan } => {
                let mid = (input[0] + input[1]) * 0.5;
                let side = (input[0] - input[1]) * 0.5 * *width;
                output = [
                    (mid + side) * (1.0 - *pan).max(0.0),
                    (mid - side) * (1.0 + *pan).max(0.0),
                ];
            }
            Processor::Limiter {
                gain,
                release_coeff,
                envelope,
                ..
            } => {
                // `inGain` is natural dB; `CopyCoefficients` uses `2^(dB/6)`.
                let linear_gain = (*gain * (1.0 / 6.0)).exp2();
                output[0] *= linear_gain;
                output[1] *= linear_gain;
                let peak = output[0].abs().max(output[1].abs());
                let target = if peak > 1.0 { 1.0 / peak } else { 1.0 };
                if target < *envelope {
                    *envelope = target;
                } else {
                    *envelope += (target - *envelope) * *release_coeff;
                }
                output[0] *= *envelope;
                output[1] *= *envelope;
            }
            Processor::Compressor {
                model,
                threshold_db,
                ratio,
                attack_ms,
                release_ms,
                stereo_link,
                rate,
                reduction,
                env1,
                env2,
                env_hold,
            } => {
                // Recovered natural units: linear threshold is `2^(dB/6)`
                // and `ratio` is the display ratio R with slope `1/R`.
                let threshold = (*threshold_db * (1.0 / 6.0)).exp2();
                let ratio = ratio.max(1.0);
                let slope = 1.0 / ratio;
                let linked = if *stereo_link {
                    (input[0].abs() + input[1].abs()) * 0.5
                } else {
                    input[0].abs().max(input[1].abs())
                };
                let mut gains = [1.0f32; 2];
                if *model == 1 {
                    // BEffectCompressor::ProcessIntEnhanced: recovered
                    // piecewise static curve and two-pole envelope.
                    let attack = attack_ms.max(1.0);
                    let release = release_ms.max(50.0);
                    let ratio_shape = if slope < 0.04 {
                        5.0
                    } else {
                        1.0 / (slope + 0.13) - 0.884
                    };
                    let curve_scale = (1.0 / threshold.max(0.0004))
                        * ratio_shape
                        * (1.0 + (2.0 * attack.ln()) / 6.907_76);
                    let a = 2513.2 / (*rate * attack);
                    let r = 1.0 - 1778.3 / (*rate * release);
                    for ch in 0..2 {
                        let level = if *stereo_link {
                            linked
                        } else {
                            input[ch].abs()
                        };
                        let e = (level - threshold).max(0.0) * curve_scale;
                        let curve = if e <= 0.5 {
                            0.46 * e
                        } else if e <= 1.207 {
                            0.039_052_4 + 0.381_895 * e
                        } else if e <= 2.207 {
                            0.089_62 + 0.34 * e
                        } else if e <= 3.621 {
                            0.184_456 + 0.297_03 * e
                        } else if e <= 5.621 {
                            0.318_54 + 0.26 * e
                        } else if e <= 8.449 {
                            0.468_168 + 0.233_38 * e
                        } else if e <= 12.449 {
                            0.623_465 + 0.215 * e
                        } else {
                            0.832_671 + 0.198_195 * e
                        };
                        env1[ch] += a * (curve - (env2[ch] + env1[ch] * 1.8));
                        env2[ch] += a * env1[ch];
                        env_hold[ch] = (env_hold[ch] * r).max(env2[ch]);
                        gains[ch] = 1.0 / (env_hold[ch] + 1.0);
                    }
                } else {
                    // Classic/Pro static curve: `(T/x)^(1-1/R)`, normalized
                    // so full scale lands on the exported ceiling
                    // `T^(1-1/R)` (`BEffectCompressor::SetCompressor`).
                    let ceiling = threshold.powf(1.0 - slope);
                    let target = if linked > threshold {
                        (threshold / linked).powf(1.0 - slope)
                    } else {
                        1.0
                    };
                    let target_reduction = if ceiling < 1.0 {
                        (1.0 - target) / (1.0 - ceiling)
                    } else {
                        0.0
                    };
                    let attack = target_reduction > *reduction;
                    let n = |ms: f32| (ms * 0.001 * *rate).max(4.0);
                    let coeff: f32 = if *model == 0 {
                        let (base, ms) = if attack {
                            (0.625f32, *attack_ms)
                        } else {
                            (1.0f32 / 3.0, *release_ms)
                        };
                        1.0 - base.powf(1.0 / n(ms))
                    } else {
                        // Pro release is the exported `ln2/N` linear ramp.
                        let ms = if attack { *attack_ms } else { *release_ms };
                        std::f32::consts::LN_2 / n(ms)
                    };
                    *reduction += (target_reduction - *reduction) * coeff.clamp(0.0, 1.0);
                    gains = [1.0 - *reduction * (1.0 - ceiling); 2];
                }
                output[0] *= gains[0];
                output[1] *= gains[1];
            }
            Processor::Distortion {
                transistor,
                drive,
                damping,
                state,
            } => {
                let gain = 1.0 + *drive * 24.0;
                let coeff = 1.0 - damping.clamp(0.0, 0.99);
                for ch in 0..2 {
                    let x = input[ch] * gain;
                    let shaped = if *transistor {
                        x / (1.0 + x.abs())
                    } else {
                        x.tanh()
                    };
                    state[ch] += (shaped - state[ch]) * coeff;
                }
                output = *state;
            }
            Processor::Skreamer {
                tone,
                drive,
                bass,
                bright,
                mix,
                tone_state,
            } => {
                let gain = 1.0 + *drive * 20.0;
                let tone_coeff = 1.0 - tone.clamp(0.0, 0.99);
                let bass_gain = 1.0 + (*bass - 0.5) * 0.6;
                let bright_gain = 1.0 + (*bright - 0.5) * 0.6;
                for ch in 0..2 {
                    let shaped = (input[ch] * gain + 0.3 * (input[ch] * gain).powi(2)).tanh();
                    tone_state[ch] += (shaped - tone_state[ch]) * tone_coeff;
                    let low = tone_state[ch];
                    let high = shaped - low;
                    let voiced = low * bass_gain + high * bright_gain;
                    output[ch] = voiced * *mix + input[ch] * (1.0 - *mix);
                }
            }
            Processor::MultiDrive {
                drive,
                bass,
                mid,
                treble,
                level,
                low_a,
                mid_a,
                low_state,
                mid_state,
            } => {
                let gain = 1.0 + *drive * 30.0;
                let bass_gain = 1.0 + (*bass - 0.5) * 0.8;
                let mid_gain = 1.0 + (*mid - 0.5) * 0.8;
                let treble_gain = 1.0 + (*treble - 0.5) * 0.8;
                for ch in 0..2 {
                    let shaped = (input[ch] * gain).tanh();
                    low_state[ch] += (shaped - low_state[ch]) * *low_a;
                    mid_state[ch] += (shaped - mid_state[ch]) * *mid_a;
                    let low = low_state[ch];
                    let mid_band = mid_state[ch] - low_state[ch];
                    let high = shaped - mid_state[ch];
                    output[ch] =
                        *level * (low * bass_gain + mid_band * mid_gain + high * treble_gain);
                }
            }
            Processor::Gater {
                rate_hz,
                depth,
                phase,
                sample_rate,
            } => {
                *phase += *rate_hz / *sample_rate;
                if *phase >= 1.0 {
                    *phase -= 1.0;
                }
                let gate = if *phase < 0.5 { 1.0 } else { 1.0 - *depth };
                output[0] *= gate;
                output[1] *= gate;
            }
            Processor::Transient {
                attack,
                sustain,
                gain,
                rate,
                fast,
                slow,
            } => {
                let level = input[0].abs().max(input[1].abs());
                let fast_coeff = 1.0 - (-1.0 / (0.001 * *rate)).exp();
                let slow_coeff = 1.0 - (-1.0 / (0.05 * *rate)).exp();
                *fast += (level - *fast) * fast_coeff;
                *slow += (level - *slow) * slow_coeff;
                let ratio = if *slow > 1e-6 { *fast / *slow } else { 1.0 };
                let boost =
                    (1.0 + *attack * (ratio - 1.0) + *sustain * (1.0 - 1.0 / ratio.max(0.1)))
                        .clamp(0.0, 4.0);
                output[0] *= boost * *gain;
                output[1] *= boost * *gain;
            }
            Processor::BeatRepeat {
                buffer,
                index,
                len,
                mix,
                captured,
                ..
            } => {
                let length = (*len).max(1);
                if buffer.len() != length {
                    buffer.resize(length, [0.0; 2]);
                    *index = 0;
                    *captured = false;
                }
                if !*captured {
                    buffer[*index] = input;
                }
                output[0] = input[0] * (1.0 - *mix) + buffer[*index][0] * *mix;
                output[1] = input[1] * (1.0 - *mix) + buffer[*index][1] * *mix;
                *index += 1;
                if *index >= length {
                    *index = 0;
                    *captured = true;
                }
            }
            Processor::ReverseGrain {
                buffer,
                index,
                len,
                mix,
                captured,
                ..
            } => {
                let length = (*len).max(1);
                if buffer.len() != length {
                    buffer.resize(length, [0.0; 2]);
                    *index = 0;
                    *captured = false;
                }
                let grain = if *captured {
                    let value = buffer[*index];
                    if *index == 0 {
                        *captured = false;
                    } else {
                        *index -= 1;
                    }
                    value
                } else {
                    buffer[*index] = input;
                    *index += 1;
                    if *index >= length {
                        *index = length - 1;
                        *captured = true;
                    }
                    input
                };
                output[0] = input[0] * (1.0 - *mix) + grain[0] * *mix;
                output[1] = input[1] * (1.0 - *mix) + grain[1] * *mix;
            }
            Processor::RingMod {
                rate_hz,
                depth,
                phase,
                sample_rate,
            } => {
                let carrier = (std::f32::consts::TAU * *phase).sin();
                let gain = 1.0 - *depth + *depth * carrier;
                output[0] *= gain;
                output[1] *= gain;
                *phase += *rate_hz / *sample_rate;
                if *phase >= 1.0 {
                    *phase -= 1.0;
                }
            }
            Processor::LoFi {
                downsample,
                bits,
                mix,
                tone,
                hold,
                counter,
                tone_state,
            } => {
                *counter = counter.wrapping_add(1);
                let factor = 1 + (*downsample * 31.0) as u32;
                if *counter >= factor {
                    *counter = 0;
                    *hold = input;
                }
                let levels = 2f32.powi(16 - (*bits * 14.0) as i32);
                let coeff = 1.0 - tone.clamp(0.0, 0.99);
                for ch in 0..2 {
                    let quantized = (hold[ch] * levels).round() / levels;
                    tone_state[ch] += (quantized - tone_state[ch]) * coeff;
                    output[ch] = input[ch] * (1.0 - *mix) + tone_state[ch] * *mix;
                }
            }
            Processor::Rotator {
                rate_hz,
                depth,
                phase,
                sample_rate,
            } => {
                let angle = *depth * (std::f32::consts::TAU * *phase).sin();
                let c = angle.cos();
                let s = angle.sin();
                output = [input[0] * c - input[1] * s, input[0] * s + input[1] * c];
                *phase += *rate_hz / *sample_rate;
                if *phase >= 1.0 {
                    *phase -= 1.0;
                }
            }
            Processor::Wah { coeffs, states, .. } => {
                output[0] = biquad_step(input[0], &mut states[0], coeffs);
                output[1] = biquad_step(input[1], &mut states[1], coeffs);
            }
            Processor::Pan(pan) => {
                output = [
                    input[0] * (1.0 - *pan).max(0.0),
                    input[1] * (1.0 + *pan).max(0.0),
                ];
            }
            Processor::Reverb {
                lines,
                positions,
                damps,
                decay,
                damping,
                mix,
                rate,
                pre_delay_ms,
                pre_buffer,
                pre_index,
                room_size,
                color,
                stereo,
                filter,
                tone_lp,
                recovered,
            } => {
                let mut network_input = input;
                if *recovered && pre_buffer.len() > 2 {
                    let frames = ((*pre_delay_ms * 0.001 * *rate).round() as usize)
                        .min(pre_buffer.len() - 1);
                    if frames >= 1 {
                        let read = (*pre_index + pre_buffer.len() - frames) % pre_buffer.len();
                        network_input = pre_buffer[read];
                        pre_buffer[*pre_index] = input;
                        *pre_index = (*pre_index + 1) % pre_buffer.len();
                    }
                }
                let feedback = if *recovered {
                    0.70 + 0.29 * room_size.clamp(0.0, 1.0)
                } else {
                    *decay
                };
                let damp_amount = if *recovered {
                    color.clamp(0.0, 0.999)
                } else {
                    damping.clamp(0.0, 0.99)
                };
                let damp_coeff = 1.0 - damp_amount;
                let mut wet = [0.0f32; 2];
                for index in 0..4 {
                    let length = lines[index].len();
                    if length == 0 {
                        continue;
                    }
                    let at = positions[index];
                    let out = lines[index][at];
                    for ch in 0..2 {
                        damps[index][ch] += (out[ch] - damps[index][ch]) * damp_coeff;
                        lines[index][at][ch] = network_input[ch] + damps[index][ch] * feedback;
                        wet[ch] += out[ch];
                    }
                    positions[index] = (at + 1) % length;
                }
                for sample in &mut wet {
                    *sample *= 0.25;
                }
                if *recovered {
                    let tone = 1.0 - filter.clamp(0.0, 0.995);
                    for ch in 0..2 {
                        tone_lp[ch] += (wet[ch] - tone_lp[ch]) * tone;
                    }
                    let mid = (tone_lp[0] + tone_lp[1]) * 0.5;
                    let side = (tone_lp[0] - tone_lp[1]) * 0.5 * (0.5 + stereo.clamp(0.0, 1.0));
                    output = [mid + side, mid - side];
                } else {
                    for ch in 0..2 {
                        output[ch] = network_input[ch] * (1.0 - *mix) + wet[ch] * *mix;
                    }
                }
            }
            Processor::Send(levels) => {
                for (bus, level) in aux.iter_mut().zip(levels) {
                    bus[0] += input[0] * *level;
                    bus[1] += input[1] * *level;
                }
                // BEffectSends::Process is a no-op on the dry signal.
                return input;
            }
            Processor::Convolution(c) => output = c.step(input),
            Processor::Gain => {}
        }
        [
            output[0] * self.wet + input[0] * self.dry,
            output[1] * self.wet + input[1] * self.dry,
        ]
    }
}

pub(crate) struct PostFx {
    inserts: Vec<Stage>,
    sends: Vec<Vec<Stage>>,
    master: Vec<Stage>,
    failed: bool,
}
impl PostFx {
    pub(crate) fn load(loaded: &mut LoadedInstrument) -> Result<Option<Self>, LoadError> {
        let chains = loaded.instrument.fx_chains.clone();
        let mut samples = 0usize;
        Self::build(&chains, &mut |index| {
            let reference = loaded
                .instrument
                .sample_files
                .get(index)
                .cloned()
                .ok_or_else(|| {
                    LoadError::Failed(format!("IR filename index {index} is missing"))
                })?;
            let pad = loaded
                .retail_key()
                .map(|key| std::sync::Arc::<[u8]>::from(key.pad()));
            let location = loaded
                .sample_location(&reference)
                .map_err(|e| LoadError::Failed(format!("IR {reference:?}: {e}")))?;
            let mut reader = SampleReader::open_with_pad(location, pad)
                .map_err(|e| LoadError::Failed(format!("IR {reference:?}: {e}")))?;
            let info = reader.metadata();
            samples = samples
                .checked_add(info.num_frames.saturating_mul(2))
                .ok_or_else(|| LoadError::Failed("IR budget overflow".into()))?;
            if !(1..=2).contains(&info.channels)
                || info.num_frames > 1_048_576
                || samples > MAX_IR_SAMPLES
            {
                return Err(LoadError::Failed(
                    "IR assets exceed mono/stereo resident budget".into(),
                ));
            }
            reader
                .read_frames(0, info.num_frames)
                .map_err(|e| LoadError::Failed(format!("IR {reference:?}: {e}")))
        })
    }
    #[cfg(test)]
    pub(crate) fn from_chains(chains: &[FxChain]) -> Option<Self> {
        Self::build(chains, &mut |_| {
            Err(LoadError::Failed("IR resolver required".into()))
        })
        .expect("valid FX fixture")
    }
    pub(crate) fn build(
        chains: &[FxChain],
        load: &mut impl FnMut(usize) -> Result<Sample, LoadError>,
    ) -> Result<Option<Self>, LoadError> {
        if chains.len() > 3 || chains.iter().any(|c| c.slots.len() > 8) {
            return Err(LoadError::Failed(
                "program FX exceeds three arrays/eight slots".into(),
            ));
        }
        let mut compiled = Vec::new();
        for (role, chain) in chains.iter().enumerate() {
            let mut stages = Vec::new();
            for slot in chain.slots.iter() {
                if role == 1
                    && slot
                        .as_ref()
                        .is_some_and(|slot| !slot.bypass && slot.send_partition != -1)
                {
                    return Err(LoadError::Failed(
                        "nondefault send output partitions are unsupported by the stereo sampler"
                            .into(),
                    ));
                }
                let stage = if let Some(slot) = slot {
                    Stage::compile(slot, load)?
                } else {
                    None
                };
                if role != 0
                    && stage
                        .as_ref()
                        .is_some_and(|s| matches!(s.processor, Processor::Send(_)))
                {
                    return Err(LoadError::Failed(
                        "send-to-send/master-to-send feedback routing is unsupported".into(),
                    ));
                }
                stages.push(stage);
            }
            compiled.push(stages);
        }
        while compiled.len() < 3 {
            compiled.push(Vec::new());
        }
        let master = compiled
            .pop()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut sends = compiled
            .pop()
            .unwrap()
            .into_iter()
            .map(|s| s.into_iter().collect())
            .collect::<Vec<Vec<Stage>>>();
        sends.resize_with(8, Vec::new);
        let inserts = compiled
            .pop()
            .unwrap()
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if inserts.is_empty() && master.is_empty() && sends.iter().all(Vec::is_empty) {
            return Ok(None);
        }
        let mut fx = Self {
            inserts,
            sends,
            master,
            failed: false,
        };
        fx.try_prepare(48_000.0)
            .map_err(|e| LoadError::Failed(e.into()))?;
        Ok(Some(fx))
    }
    fn try_prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        let mut budget = 0;
        for stage in self
            .inserts
            .iter_mut()
            .chain(self.sends.iter_mut().flatten())
            .chain(self.master.iter_mut())
        {
            stage.prepare(rate, &mut budget)?;
        }
        Ok(())
    }
    pub(crate) fn prepare(&mut self, rate: f32) {
        let failed = self.try_prepare(rate).is_err();
        if failed || self.failed {
            self.reset();
        }
        self.failed = failed;
    }
    pub(crate) fn failed(&self) -> bool {
        self.failed
    }
    pub(crate) fn reset(&mut self) {
        for stage in self
            .inserts
            .iter_mut()
            .chain(self.sends.iter_mut().flatten())
            .chain(self.master.iter_mut())
        {
            stage.reset();
        }
    }
    pub(crate) fn convolution_memory_bytes(&self) -> usize {
        self.inserts
            .iter()
            .chain(self.sends.iter().flatten())
            .chain(&self.master)
            .map(|s| {
                if let Processor::Convolution(c) = &s.processor {
                    c.memory_bytes()
                } else {
                    0
                }
            })
            .sum()
    }
    pub(crate) fn tail(&self) -> usize {
        self.inserts.iter().map(Stage::tail).sum::<usize>()
            + self
                .sends
                .iter()
                .map(|s| s.iter().map(Stage::tail).sum::<usize>())
                .max()
                .unwrap_or(0)
            + self.master.iter().map(Stage::tail).sum::<usize>()
    }
    fn step(&mut self, mut input: [f32; 2], aux: &mut AuxFrame) -> [f32; 2] {
        if self.failed {
            return [0.0; 2];
        }
        for stage in &mut self.inserts {
            input = stage.step(input, aux);
        }
        for (bus, stages) in self.sends.iter_mut().enumerate() {
            if stages.is_empty() {
                continue;
            }
            let mut wet = aux[bus];
            for stage in stages {
                wet = stage.step(wet, &mut [[0.0; 2]; 8]);
            }
            input[0] += wet[0];
            input[1] += wet[1];
        }
        for stage in &mut self.master {
            input = stage.step(input, &mut [[0.0; 2]; 8]);
        }
        input
    }
    pub(crate) fn process_range(
        &mut self,
        outputs: &mut [f32],
        channels: usize,
        frames: usize,
        start: usize,
        aux: &mut [AuxFrame],
    ) {
        if channels == 0 {
            return;
        }
        for (i, buses) in aux.iter_mut().enumerate() {
            let at = start + i;
            let input = [
                outputs[at],
                if channels > 1 {
                    outputs[frames + at]
                } else {
                    outputs[at]
                },
            ];
            let output = self.step(input, buses);
            outputs[at] = if channels > 1 {
                output[0]
            } else {
                (output[0] + output[1]) * 0.5
            };
            if channels > 1 {
                outputs[frames + at] = output[1];
            }
        }
    }
    #[cfg(test)]
    pub(crate) fn process(&mut self, outputs: &mut [f32], channels: usize, frames: usize) {
        for at in 0..frames {
            self.process_range(outputs, channels, frames, at, &mut [[[0.0; 2]; 8]]);
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn bandpass_coeffs(frequency: f32, q: f32, sample_rate: f32, coeffs: &mut Peaking) {
    let frequency = frequency.clamp(10.0, sample_rate * 0.45);
    let w0 = std::f32::consts::TAU * frequency / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = sin_w0 / (2.0 * q.max(0.1));
    let a0 = 1.0 + alpha;
    coeffs.b0 = alpha / a0;
    coeffs.b1 = 0.0;
    coeffs.b2 = -alpha / a0;
    coeffs.a1 = -2.0 * cos_w0 / a0;
    coeffs.a2 = (1.0 - alpha) / a0;
}

fn band_coeffs(band: Band, sample_rate: f32, coeffs: &mut Peaking) {
    let frequency = band.frequency.clamp(10.0, sample_rate * 0.45);
    let a = 10f32.powf(band.gain_db / 40.0);
    let w0 = std::f32::consts::TAU * frequency / sample_rate;
    let sin_w0 = w0.sin();
    let cos_w0 = w0.cos();
    let alpha = if sin_w0.abs() < 1e-9 {
        0.0
    } else if band.bandwidth > 0.0 {
        sin_w0 * ((std::f32::consts::LN_2 * 0.5 * band.bandwidth * w0 / sin_w0).sinh())
    } else {
        sin_w0 / (2.0 * band.q.max(0.1))
    };
    match band.kind {
        BandKind::Peaking => {
            let a0 = 1.0 + alpha / a;
            coeffs.b0 = (1.0 + alpha * a) / a0;
            coeffs.b1 = -2.0 * cos_w0 / a0;
            coeffs.b2 = (1.0 - alpha * a) / a0;
            coeffs.a1 = -2.0 * cos_w0 / a0;
            coeffs.a2 = (1.0 - alpha / a) / a0;
        }
        BandKind::LowShelf => {
            let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
            let a0 = (a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
            coeffs.b0 = a * ((a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha) / a0;
            coeffs.b1 = 2.0 * a * ((a - 1.0) - (a + 1.0) * cos_w0) / a0;
            coeffs.b2 = a * ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
            coeffs.a1 = -2.0 * ((a - 1.0) + (a + 1.0) * cos_w0) / a0;
            coeffs.a2 = ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
        }
        BandKind::HighShelf => {
            let two_sqrt_a_alpha = 2.0 * a.sqrt() * alpha;
            let a0 = (a + 1.0) - (a - 1.0) * cos_w0 + two_sqrt_a_alpha;
            coeffs.b0 = a * ((a + 1.0) + (a - 1.0) * cos_w0 + two_sqrt_a_alpha) / a0;
            coeffs.b1 = -2.0 * a * ((a - 1.0) + (a + 1.0) * cos_w0) / a0;
            coeffs.b2 = a * ((a + 1.0) + (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
            coeffs.a1 = 2.0 * ((a - 1.0) - (a + 1.0) * cos_w0) / a0;
            coeffs.a2 = ((a + 1.0) - (a - 1.0) * cos_w0 - two_sqrt_a_alpha) / a0;
        }
    }
}

fn biquad_step(input: f32, state: &mut [f32; 2], coeffs: &Peaking) -> f32 {
    let output = coeffs.b0 * input + state[0];
    state[0] = coeffs.b1 * input - coeffs.a1 * output + state[1];
    state[1] = coeffs.b2 * input - coeffs.a2 * output;
    output
}
