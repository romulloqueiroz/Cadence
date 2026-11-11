//! A small reference plugin used by the host demo and tests.

use plugin_api::{
    AudioBuffer, Descriptor, Event, EventKind, ParamFlags, ParameterInfo, Plugin, PluginKind,
    ProcessContext, State, StateError, UiNode, Unit,
};
use std::f32::consts::TAU;

#[derive(Debug, Clone)]
pub struct SinePlugin {
    sample_rate: f32,
    phase: f32,
    frequency: f32,
    gain_db: f32,
    envelope: f32,
    attack_coef: f32,
    release_coef: f32,
    releasing: bool,
    active: bool,
}

impl Default for SinePlugin {
    fn default() -> Self {
        let mut plugin = SinePlugin {
            sample_rate: 48_000.0,
            phase: 0.0,
            frequency: 440.0,
            gain_db: 0.0,
            envelope: 0.0,
            attack_coef: 0.0,
            release_coef: 0.0,
            releasing: false,
            active: false,
        };
        plugin.prepare(&ProcessContext::default());
        plugin
    }
}

impl SinePlugin {
    fn update_coefficients(&mut self) {
        let rate = self.sample_rate.max(1.0);
        self.attack_coef = (-1.0 / (0.005 * rate)).exp();
        self.release_coef = (-1.0 / (0.050 * rate)).exp();
    }

    fn linear_gain(&self) -> f32 {
        10f32.powf(self.gain_db / 20.0)
    }
}

impl Plugin for SinePlugin {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "com.example.sine",
            name: "Sine Demo",
            vendor: "Example",
            version: "1.0.0",
            kind: PluginKind::Instrument,
        }
    }

    fn capabilities(&self) -> plugin_api::Capabilities {
        plugin_api::Capabilities::stereo_instrument()
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![ParameterInfo {
            id: 1,
            name: "Gain",
            min: -60.0,
            max: 12.0,
            default: 0.0,
            unit: Unit::Decibels,
            flags: ParamFlags::default(),
        }]
    }

    fn ui(&self) -> Vec<UiNode> {
        vec![UiNode::Group {
            label: "Output".into(),
            children: vec![UiNode::Knob {
                param: 1,
                label: "Gain".into(),
            }],
        }]
    }

    fn prepare(&mut self, context: &ProcessContext) {
        self.sample_rate = context.sample_rate as f32;
        self.update_coefficients();
    }

    fn reset(&mut self) {
        self.phase = 0.0;
        self.envelope = 0.0;
        self.releasing = false;
        self.active = false;
    }

    fn process(
        &mut self,
        _context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        let mut increment = TAU * self.frequency / self.sample_rate.max(1.0);
        let mut gain = self.linear_gain();
        for frame in 0..audio.frames {
            for event in events.iter().filter(|event| event.frame == frame) {
                match event.kind {
                    EventKind::NoteOn if event.data2 > 0 => {
                        self.frequency = 440.0 * 2f32.powf((event.data1 as f32 - 69.0) / 12.0);
                        increment = TAU * self.frequency / self.sample_rate.max(1.0);
                        self.releasing = false;
                        self.active = true;
                    }
                    EventKind::NoteOff | EventKind::NoteOn => self.releasing = true,
                    EventKind::Parameter if event.data1 == 1 => {
                        self.gain_db = -60.0 + event.value.clamp(0.0, 1.0) * 72.0;
                        gain = self.linear_gain();
                    }
                    _ => {}
                }
            }
            if !self.active {
                continue;
            }
            let target = if self.releasing { 0.0 } else { 1.0 };
            let coefficient = if self.releasing {
                self.release_coef
            } else {
                self.attack_coef
            };
            self.envelope = target + (self.envelope - target) * coefficient;

            let value = self.phase.sin() * self.envelope * gain * 0.2;
            for channel in 0..audio.output_channels {
                audio.output_mut(channel)[frame] += value;
            }
            self.phase = (self.phase + increment) % TAU;

            if self.releasing && self.envelope < 1.0e-4 {
                self.active = false;
            }
        }
    }

    fn save_state(&self) -> State {
        State {
            schema_version: 1,
            bytes: self.gain_db.to_le_bytes().to_vec(),
        }
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        if state.schema_version > 1 {
            return Err(StateError::SchemaTooNew {
                found: state.schema_version,
                supported: 1,
            });
        }
        if state.bytes.len() != 4 {
            return Err(StateError::Malformed("expected 4 bytes".into()));
        }
        self.gain_db = f32::from_le_bytes(state.bytes[..4].try_into().unwrap());
        Ok(())
    }
}

const GAIN_EFFECT: ParameterInfo = ParameterInfo {
    id: 1,
    name: "Gain",
    min: -60.0,
    max: 12.0,
    default: 0.0,
    unit: Unit::Decibels,
    flags: ParamFlags {
        stepped: false,
        read_only: false,
        bypass: false,
    },
};

/// A stereo utility gain. Used to exercise the effect graph.
#[derive(Debug, Clone)]
pub struct GainEffect {
    gain_db: f32,
}

impl Default for GainEffect {
    fn default() -> Self {
        GainEffect { gain_db: 0.0 }
    }
}

impl Plugin for GainEffect {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "com.example.gain",
            name: "Gain",
            vendor: "Example",
            version: "1.0.0",
            kind: PluginKind::Effect,
        }
    }

    fn capabilities(&self) -> plugin_api::Capabilities {
        plugin_api::Capabilities::stereo_effect()
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![GAIN_EFFECT]
    }

    fn ui(&self) -> Vec<UiNode> {
        vec![UiNode::Knob {
            param: 1,
            label: "Gain".into(),
        }]
    }

    fn prepare(&mut self, _context: &ProcessContext) {}

    fn reset(&mut self) {}

    fn process(
        &mut self,
        _context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        for event in events {
            if event.kind == EventKind::Parameter && event.data1 as u32 == GAIN_EFFECT.id {
                self.gain_db = GAIN_EFFECT.denormalize(event.value);
            }
        }

        let frames = audio.frames;
        let inputs = audio.input_channels.min(2);
        let outputs = audio.output_channels.min(2);
        if frames == 0 || inputs == 0 || outputs == 0 {
            return;
        }
        let gain = 10f32.powf(self.gain_db / 20.0);

        for frame in 0..frames {
            if inputs == 1 {
                let value = audio.input(0)[frame] * gain;
                for channel in 0..outputs {
                    audio.output_mut(channel)[frame] = value;
                }
            } else {
                for channel in 0..outputs.min(inputs) {
                    let value = audio.input(channel)[frame] * gain;
                    audio.output_mut(channel)[frame] = value;
                }
            }
        }
    }

    fn save_state(&self) -> State {
        State {
            schema_version: 1,
            bytes: self.gain_db.to_le_bytes().to_vec(),
        }
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        if state.schema_version > 1 {
            return Err(StateError::SchemaTooNew {
                found: state.schema_version,
                supported: 1,
            });
        }
        if state.bytes.len() != 4 {
            return Err(StateError::Malformed("expected 4 bytes".into()));
        }
        self.gain_db = f32::from_le_bytes(state.bytes[..4].try_into().unwrap());
        Ok(())
    }
}
