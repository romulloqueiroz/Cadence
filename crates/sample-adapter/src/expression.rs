//! Instrument-defined external MIDI modulation. See the saved evidence in
//! docs/kontakt-external-mod-runtime-evidence.md and midi-expression.md.

use super::curve::shaped_with_k;
use super::group_fx::GROUP_FX_SLOTS;
use super::Voice;
use nk_format::{
    external_mod::{CurvePoint, Transformation},
    model::Group,
};
use plugin_api::provider::LoadError;

#[derive(Clone, Copy)]
struct Value {
    normalized: f32,
    legacy: f32,
    stamp: u64,
}

#[derive(Clone, Copy, Default)]
struct Smooth {
    initialized: bool,
    value: f32,
    remaining: u32,
    step: f32,
    born: u64,
    source_stamp: Option<u64>,
    source_value: f32,
    poly_pressure: f32,
}

#[derive(Clone, Copy)]
enum Target {
    Pitch,
    Volume,
    Pan,
    Filter(Option<usize>),
}

struct Route {
    source: u32,
    cc: usize,
    normalized: bool,
    broadcast_pressure: bool,
    initial: i16,
    target: Target,
    depth: f32,
    invert: bool,
    table: Option<Box<[f32; 128]>>,
    smoothing: i16,
    alpha: f32,
    ramp_frames: u32,
}

fn curve_value(points: &[CurvePoint], x: f32) -> f32 {
    let pair = points.windows(2).find(|pair| x <= pair[1].x).unwrap();
    let delta = pair[1].y - pair[0].y;
    let t = (x - pair[0].x) / (pair[1].x - pair[0].x);
    let curvature = pair[0].curvature * 16.0 * if delta < 0.0 { -1.0 } else { 1.0 };
    pair[0].y + delta * shaped_with_k(t, curvature)
}

impl Route {
    fn shape(&self, value: f32) -> f32 {
        let bipolar = matches!(self.source, 1 | 7);
        if let Some(table) = &self.table {
            let input = match self.source {
                // DispatchVoice supplies octaves from middle C. The table
                // is addressed by the original note's position over 0..127.
                7 => (value * 12.0 + 60.0) / 127.0,
                1 => (value + 1.0) * 0.5,
                _ => value,
            };
            let position = input.clamp(0.0, 1.0) * 127.0;
            let index = position as usize;
            let fraction = position - index as f32;
            let value = table[index] + (table[(index + 1).min(127)] - table[index]) * fraction;
            // GenSysModScript always wraps key-position table output in
            // frange_bi, including a key-position route to volume.
            let value = if self.source == 7 {
                value * 2.0 - 1.0
            } else {
                value
            };
            return match self.target {
                Target::Volume => {
                    if self.invert {
                        1.0 - value
                    } else {
                        value
                    }
                }
                _ => {
                    let value = if self.source == 1 {
                        value * 2.0 - 1.0
                    } else {
                        value
                    };
                    if self.invert {
                        -value
                    } else {
                        value
                    }
                }
            };
        }
        match self.target {
            Target::Volume if bipolar => ((if self.invert { -value } else { value }) + 1.0) * 0.5,
            Target::Volume if self.invert => 1.0 - value,
            _ if self.invert => -value,
            _ => value,
        }
    }
}

impl Smooth {
    fn source(&mut self, value: Value, normalized: bool) -> f32 {
        if self.source_stamp != Some(value.stamp) {
            // Native initial-voice lookup uses the high-precision store even
            // for legacy routes. Later changes honor the route's precision flag.
            self.source_value = if normalized || value.stamp <= self.born {
                value.normalized
            } else {
                value.legacy
            };
            self.source_stamp = Some(value.stamp);
        }
        self.source_value
    }

    fn next(&mut self, signal: f32, route: &Route) -> f32 {
        if !self.initialized || route.smoothing == 0 {
            self.initialized = true;
            self.value = signal;
        } else if route.smoothing > 0 {
            if (signal - self.value).abs() >= 1e-5 {
                self.value += (signal - self.value) * route.alpha;
            }
        } else {
            // Keep an in-flight ramp's slope when the input changes again,
            // as the recovered control-rate generator does.
            if self.remaining == 0 && (signal - self.value).abs() >= 1e-5 {
                self.remaining = route.ramp_frames;
                self.step = (signal - self.value) / self.remaining as f32;
            }
            if self.remaining != 0 {
                self.remaining -= 1;
                self.value += self.step;
            }
        }
        self.value
    }
}

pub(crate) struct Modulation {
    groups: Vec<Vec<Route>>,
    states: Vec<Smooth>,
    stride: usize,
    clock: u64,
    bend: [Option<Value>; 16],
    pressure: [Option<Value>; 16],
    controllers: Box<[[Option<Value>; 128]]>,
}

pub(crate) struct Output {
    pub octaves: f32,
    pub gain: f32,
    pub pan: f32,
    /// Existing unindexed XML cutoff routes, in octaves.
    pub filter_octaves: f32,
    /// Indexed Kontakt FX targets, additive in the filter's normalized domain.
    pub filter_slots: [f32; GROUP_FX_SLOTS],
}

impl Modulation {
    pub(crate) fn new(groups: &[Group], voices: usize) -> Result<Self, LoadError> {
        let mut compiled = Vec::with_capacity(groups.len());
        for group in groups {
            let mut routes = Vec::new();
            for modulation in &group.external_modulators {
                if modulation.bypass
                    // Zone-envelope sources are retained in the format model
                    // for the next envelope stage; this engine handles MIDI.
                    || modulation.kind != 1
                    || modulation.source == 0
                {
                    continue;
                }
                for target in &modulation.targets {
                    let mut kind = match target.name.as_str() {
                        "pitch" => Target::Pitch,
                        "volume" => Target::Volume,
                        "pan" => Target::Pan,
                        "filterCutoff" => Target::Filter(None),
                        // Other targets belong to the later DSP/modulation stages.
                        _ => continue,
                    };
                    let unsupported = |message: &str| {
                        LoadError::Failed(format!(
                            "external modulation in group {:?}, target {:?}: {message}",
                            group.name, target.name
                        ))
                    };
                    let depth = if target.flags & 16 == 0 && matches!(kind, Target::Pitch) {
                        1.0 / 6.0
                    } else {
                        target.intensity
                    };
                    // Static zero-depth routes have no audible effect, even
                    // when they reserve a script/modulator source not yet run.
                    if depth == 0.0 {
                        continue;
                    }
                    if !matches!(modulation.source, 1..=4 | 7..=9) {
                        return Err(unsupported("unsupported modulation source"));
                    }
                    if modulation.source == 4 && modulation.initial_value > 127 {
                        return Err(unsupported("CC initial value exceeds 127"));
                    }
                    if target.slot_idx != -1 {
                        return Err(unsupported("modulation slot routing is not supported"));
                    }
                    if matches!(kind, Target::Filter(_)) {
                        let index = target.target_obj_idx.unwrap_or(-1);
                        if index < -1 || index as i16 >= GROUP_FX_SLOTS as i16 {
                            return Err(unsupported("filter target index must be -1 or 0..7"));
                        }
                        if index >= 0 {
                            let index = index as usize;
                            // Bypass is allowed: the route stays addressed to
                            // its original slot even when that stage is absent
                            // from the compiled chain. Never compact slot IDs.
                            let filter = group
                                .fx_chain
                                .as_ref()
                                .and_then(|chain| chain.slots.get(index))
                                .and_then(Option::as_ref)
                                .and_then(|slot| slot.child.as_ref())
                                .and_then(nk_format::fx::FxChild::simple_filter);
                            if filter.is_none() {
                                return Err(unsupported(
                                    "indexed cutoff target is not a supported group filter",
                                ));
                            }
                            kind = Target::Filter(Some(index));
                        }
                    } else if target.target_obj_idx.is_some_and(|index| index >= 0) {
                        return Err(unsupported("object-target routing is not supported"));
                    }
                    let limit = if matches!(kind, Target::Filter(_)) {
                        16.0
                    } else {
                        1.0
                    };
                    if !target.intensity.is_finite() || target.intensity.abs() > limit {
                        return Err(unsupported("target intensity out of range"));
                    }
                    let table = match &target.transform {
                        Transformation::None
                        | Transformation::Table { active: false, .. }
                        | Transformation::Curve { active: false, .. } => None,
                        Transformation::Table { values, .. } => {
                            if values
                                .iter()
                                .any(|value| !value.is_finite() || value.abs() > 16.0)
                            {
                                return Err(unsupported("invalid modulation table"));
                            }
                            Some(values.clone())
                        }
                        Transformation::Curve { points, .. } => {
                            if points.len() < 2
                                || points.iter().any(|point| {
                                    !point.x.is_finite()
                                        || !point.y.is_finite()
                                        || point.y.abs() > 16.0
                                        || !point.curvature.is_finite()
                                        || point.curvature.abs() > 1.0
                                })
                                || points.windows(2).any(|pair| pair[0].x >= pair[1].x)
                                || points.first().unwrap().x != 0.0
                                || points.last().unwrap().x != 1.0
                            {
                                return Err(unsupported("invalid modulation curve spanning 0..1"));
                            }
                            Some(Box::new(std::array::from_fn(|index| {
                                let x = index as f32 / 127.0;
                                curve_value(points, x)
                            })))
                        }
                    };
                    routes.push(Route {
                        source: modulation.source,
                        cc: modulation.cc_number as usize,
                        normalized: modulation.flag_v104,
                        broadcast_pressure: modulation.flag_v103,
                        initial: modulation.initial_value,
                        target: kind,
                        // Native UpdateTemplates initializes legacy pitch depths.
                        depth,
                        invert: target.flags & 2 != 0,
                        table,
                        smoothing: target.smoothing,
                        alpha: 1.0,
                        ramp_frames: 1,
                    });
                }
            }
            if routes.len() > 1024 {
                return Err(LoadError::Failed(
                    "external route count exceeds 1024 per group".into(),
                ));
            }
            compiled.push(routes);
        }
        let stride = compiled.iter().map(Vec::len).max().unwrap_or(0);
        let mut result = Self {
            groups: compiled,
            states: vec![Smooth::default(); voices * stride],
            stride,
            clock: 0,
            bend: [None; 16],
            pressure: [None; 16],
            controllers: vec![[None; 128]; 16].into_boxed_slice(),
        };
        result.prepare(48_000.0);
        Ok(result)
    }

    pub(crate) fn prepare(&mut self, sample_rate: f32) {
        let rate = sample_rate.max(1.0);
        for route in self.groups.iter_mut().flatten() {
            let frames = route.smoothing.unsigned_abs() as f64 * rate as f64 / 1000.0;
            route.alpha = if frames == 0.0 {
                1.0
            } else {
                (-(-std::f64::consts::LN_10 * 2.0 / frames).exp_m1()) as f32
            };
            // Native negative smoothing rounds to quarter-time control ticks.
            route.ramp_frames = ((frames / 128.0).round() as u32).saturating_mul(32).max(1);
        }
    }

    pub(crate) fn reset(&mut self) {
        self.clock = 0;
        self.bend.fill(None);
        self.pressure.fill(None);
        self.controllers.fill([None; 128]);
        self.states.fill(Smooth::default());
    }

    pub(crate) fn start_voice(&mut self, slot: usize) {
        self.states[slot * self.stride..(slot + 1) * self.stride].fill(Smooth {
            born: self.clock,
            ..Default::default()
        });
    }

    /// Native initial-voice lookup inherits poly pressure from a matching live
    /// key/channel in the same group. New poly-pressure sources otherwise start
    /// at zero.
    pub(crate) fn inherit_poly_pressure(
        &mut self,
        slot: usize,
        group: usize,
        channel: u8,
        note: u8,
        voices: &[Option<Voice>],
    ) {
        let Some(routes) = self.groups.get(group) else {
            return;
        };
        for (route_index, _route) in routes
            .iter()
            .enumerate()
            .filter(|(_, route)| route.source == 2)
        {
            let Some(source_slot) = voices.iter().position(|voice| {
                voice.as_ref().is_some_and(|voice| {
                    voice.group == group && voice.channel == channel && voice.note == note
                })
            }) else {
                continue;
            };
            let value = self.states[source_slot * self.stride + route_index].poly_pressure;
            self.states[slot * self.stride + route_index].poly_pressure = value;
        }
    }

    pub(crate) fn bend(&mut self, channel: u8, value: f32) {
        if !value.is_finite() {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        let value = value.clamp(-1.0, 1.0);
        self.bend[channel as usize] = Some(Value {
            normalized: value,
            legacy: value,
            stamp: self.clock,
        });
    }

    pub(crate) fn pressure(&mut self, channel: u8, value: f32, raw: u16) {
        if !value.is_finite() {
            return;
        }
        self.clock = self.clock.wrapping_add(1);
        self.pressure[channel as usize] = Some(Value {
            normalized: value.clamp(0.0, 1.0),
            legacy: raw.min(127) as f32 / 127.0,
            stamp: self.clock,
        });
    }

    pub(crate) fn controller(&mut self, channel: u8, cc: u8, value: u8) {
        self.clock = self.clock.wrapping_add(1);
        // The public protocol carries MIDI CC as seven-bit data. Full UMP
        // precision is outside this adapter's current event contract.
        let value = value as f32 / 127.0;
        self.controllers[channel as usize][cc as usize] = Some(Value {
            normalized: value,
            legacy: value,
            stamp: self.clock,
        });
    }

    pub(crate) fn poly_pressure(
        &mut self,
        voices: &[Option<Voice>],
        channel: u8,
        note: u8,
        value: f32,
    ) {
        if !value.is_finite() {
            return;
        }
        let value = value.clamp(0.0, 1.0);
        for (group, routes) in self.groups.iter().enumerate() {
            // A matching live key/channel must seed a native group broadcast.
            if !voices
                .iter()
                .flatten()
                .any(|voice| voice.group == group && voice.channel == channel && voice.note == note)
            {
                continue;
            }
            for (route_index, route) in routes
                .iter()
                .enumerate()
                .filter(|(_, route)| route.source == 2)
            {
                for (slot, voice) in voices.iter().enumerate() {
                    if voice.as_ref().is_some_and(|voice| {
                        voice.group == group
                            && (route.broadcast_pressure
                                || (voice.channel == channel && voice.note == note))
                    }) {
                        self.states[slot * self.stride + route_index].poly_pressure = value;
                    }
                }
            }
        }
    }

    pub(crate) fn evaluate(
        &mut self,
        slot: usize,
        group: usize,
        channel: u8,
        note: u8,
        velocity: u8,
        release_velocity: Option<f32>,
    ) -> Output {
        let mut output = Output {
            octaves: 0.0,
            gain: 1.0,
            pan: 0.0,
            filter_octaves: 0.0,
            filter_slots: [0.0; GROUP_FX_SLOTS],
        };
        let Some(routes) = self.groups.get(group) else {
            return output;
        };
        for (route, state) in routes
            .iter()
            .zip(&mut self.states[slot * self.stride..(slot + 1) * self.stride])
        {
            let channel = channel as usize;
            let raw = match route.source {
                1 => self.bend[channel]
                    .map(|value| state.source(value, true))
                    .unwrap_or(0.0),
                2 => state.poly_pressure,
                3 => self.pressure[channel]
                    .map(|value| state.source(value, route.normalized))
                    .unwrap_or(0.0),
                4 => match self.controllers[channel][route.cc] {
                    Some(value) => state.source(value, route.normalized),
                    None if route.initial >= 0 => route.initial as f32 / 127.0,
                    None => continue, // Native initialValue=-1: bypass until touched.
                },
                7 => (note as f32 - 60.0) / 12.0,
                8 => velocity as f32 / 127.0,
                // Release velocity is fed at termination; before that the
                // source is unavailable and the route stays bypassed.
                9 => match release_velocity {
                    Some(value) => value.clamp(0.0, 1.0),
                    None => continue,
                },
                _ => unreachable!(),
            };
            let signal = state.next(route.shape(raw), route);
            match route.target {
                Target::Pitch => output.octaves += signal * route.depth,
                Target::Pan => output.pan += signal * route.depth,
                Target::Filter(None) => output.filter_octaves += signal * route.depth,
                Target::Filter(Some(index)) => output.filter_slots[index] += signal * route.depth,
                Target::Volume => {
                    let gain = 1.0 - (1.0 - signal) * route.depth;
                    let factor = if matches!(route.source, 7 | 8) {
                        gain * gain * gain
                    } else {
                        gain
                    };
                    output.gain = (output.gain * factor).clamp(-64.0, 64.0);
                }
            }
        }
        // Bound aggregate pathological preset arithmetic before DSP use.
        output.octaves = output.octaves.clamp(-16.0, 16.0);
        output.gain = output.gain.clamp(-64.0, 64.0);
        output.pan = output.pan.clamp(-2.0, 2.0);
        output.filter_octaves = output.filter_octaves.clamp(-16.0, 16.0);
        for offset in &mut output.filter_slots {
            *offset = offset.clamp(-16.0, 16.0);
        }
        output
    }
}
