//! Per-voice internal LFO and envelope modulation.
//!
//! LFO layout and field names come from `BParLFO::doRead` /
//! `BParLFO::PrsSetValues`; envelope payloads come from `BParEnv_AHDSR`.
//! Targets use the same pitch/pan/volume laws as instrument-defined external
//! MIDI routes. The serialized intensity is treated as the route depth; native
//! unit conversions (notably dB volume intensity) and tempo-synced LFO
//! frequencies are not yet decoded. Random and multi LFOs remain metadata only
//! and are not played. Volume envelopes are applied by the main amplitude path,
//! not here.

use nk_format::{
    model::Group,
    modulator::{Envelope, Lfo, LfoKind},
};

#[derive(Clone, Copy, PartialEq)]
enum Target {
    Pitch,
    Pan,
    Volume,
    Filter,
}

#[derive(Clone, Copy)]
struct Route {
    kind: LfoKind,
    delay_s: f32,
    frequency: f32,
    pulse_width: f32,
    start_phase: f32,
    target: Target,
    depth: f32,
}

struct EnvelopeRoute {
    envelope: Envelope,
    target: Target,
    depth: f32,
}

#[derive(Default)]
struct GroupRoutes {
    lfos: Vec<Route>,
    envelopes: Vec<EnvelopeRoute>,
}

pub(crate) struct Output {
    pub octaves: f32,
    pub gain: f32,
    pub pan: f32,
    pub filter_octaves: f32,
}

pub(crate) struct InternalMod {
    groups: Vec<GroupRoutes>,
}

fn playable(lfo: &Lfo) -> bool {
    matches!(
        lfo.kind,
        LfoKind::Sine | LfoKind::Rectangle | LfoKind::Triangle | LfoKind::Sawtooth
    )
}

fn waveform(kind: LfoKind, phase: f32, pulse_width: f32) -> f32 {
    match kind {
        LfoKind::Sine => (phase * std::f32::consts::TAU).sin(),
        LfoKind::Triangle => 1.0 - 4.0 * (phase - 0.5).abs(),
        LfoKind::Rectangle => {
            if phase < pulse_width.clamp(0.0, 1.0) {
                1.0
            } else {
                -1.0
            }
        }
        LfoKind::Sawtooth => 2.0 * phase - 1.0,
        LfoKind::Random | LfoKind::Multi => 0.0,
    }
}

impl InternalMod {
    pub(crate) fn new(groups: &[Group]) -> Self {
        let groups = groups
            .iter()
            .map(|group| {
                let mut result = GroupRoutes::default();
                for modulation in &group.internal_modulators {
                    if let Some(lfo) = &modulation.lfo {
                        if playable(lfo) {
                            for parameter in &modulation.parameters {
                                let target = match parameter.name.as_str() {
                                    "pitch" => Target::Pitch,
                                    "pan" => Target::Pan,
                                    "volume" => Target::Volume,
                                    "filterCutoff" => Target::Filter,
                                    _ => continue,
                                };
                                if !parameter.intensity.is_finite()
                                    || parameter.intensity.abs() > 16.0
                                {
                                    continue;
                                }
                                result.lfos.push(Route {
                                    kind: lfo.kind,
                                    delay_s: lfo.delay.max(0.0) / 1000.0,
                                    frequency: lfo.frequency.max(0.0),
                                    pulse_width: lfo.pulse_width,
                                    start_phase: lfo.start_phase,
                                    target,
                                    depth: parameter.intensity,
                                });
                            }
                        }
                    }
                    if modulation.is_envelope() {
                        let Some(envelope) = &modulation.envelope else {
                            continue;
                        };
                        for parameter in &modulation.parameters {
                            let target = match parameter.name.as_str() {
                                // Volume is applied by the amplitude path.
                                "pitch" => Target::Pitch,
                                "pan" => Target::Pan,
                                "filterCutoff" => Target::Filter,
                                _ => continue,
                            };
                            if !parameter.intensity.is_finite()
                                || parameter.intensity.abs() > 16.0
                                || result.envelopes.len() >= 1024
                            {
                                continue;
                            }
                            result.envelopes.push(EnvelopeRoute {
                                envelope: envelope.clone(),
                                target,
                                depth: parameter.intensity,
                            });
                        }
                    }
                }
                result
            })
            .collect();
        Self { groups }
    }

    pub(crate) fn evaluate(
        &self,
        group: usize,
        age: u64,
        release_age: Option<u64>,
        sample_rate: f32,
    ) -> Output {
        let mut output = Output {
            octaves: 0.0,
            gain: 1.0,
            pan: 0.0,
            filter_octaves: 0.0,
        };
        let Some(routes) = self.groups.get(group) else {
            return output;
        };
        let seconds = age as f64 / sample_rate.max(1.0) as f64;
        for route in &routes.lfos {
            let local = seconds as f32 - route.delay_s;
            if local < 0.0 || route.frequency <= 0.0 {
                continue;
            }
            let phase = (route.start_phase + local * route.frequency).rem_euclid(1.0);
            let value = waveform(route.kind, phase, route.pulse_width);
            match route.target {
                Target::Pitch => output.octaves += value * route.depth,
                Target::Pan => output.pan += value * route.depth,
                Target::Filter => output.filter_octaves += value * route.depth,
                Target::Volume => {
                    let unipolar = (value + 1.0) * 0.5;
                    output.gain *= 1.0 - (1.0 - unipolar) * route.depth.clamp(0.0, 1.0);
                }
            }
        }
        for route in &routes.envelopes {
            let level = super::envelope_level(&route.envelope, age, release_age, sample_rate)
                .clamp(0.0, 1.0);
            match route.target {
                Target::Pitch => output.octaves += level * route.depth,
                Target::Pan => output.pan += (level * 2.0 - 1.0) * route.depth,
                Target::Filter => output.filter_octaves += level * route.depth,
                Target::Volume => {}
            }
        }
        output.octaves = output.octaves.clamp(-16.0, 16.0);
        output.gain = output.gain.clamp(-64.0, 64.0);
        output.pan = output.pan.clamp(-2.0, 2.0);
        output.filter_octaves = output.filter_octaves.clamp(-16.0, 16.0);
        output
    }
}
