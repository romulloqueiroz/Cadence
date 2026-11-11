//! Example native Rust effect: a stereo gain.
//!
//! Build with `cargo build -p gain`; the host loads it with `native-host`.

use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, EventKind, ParamFlags, ParameterInfo, Plugin,
    PluginKind, ProcessContext, State, StateError, UiNode, Unit,
};
use plugin_sdk::export_plugin;

const GAIN: ParameterInfo = ParameterInfo {
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

    fn capabilities(&self) -> Capabilities {
        Capabilities::stereo_effect()
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![GAIN]
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
        let frames = audio.frames;
        let inputs = audio.input_channels.min(2);
        let outputs = audio.output_channels.min(2);
        if frames == 0 || inputs == 0 || outputs == 0 {
            return;
        }
        let mut gain = 10f32.powf(self.gain_db / 20.0);

        for frame in 0..frames {
            for event in events.iter().filter(|event| event.frame == frame) {
                if event.kind == EventKind::Parameter && event.data1 as u32 == GAIN.id {
                    self.gain_db = GAIN.denormalize(event.value);
                    gain = 10f32.powf(self.gain_db / 20.0);
                }
            }
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

fn descriptor() -> Descriptor {
    GainEffect::default().descriptor()
}

fn capabilities() -> Capabilities {
    GainEffect::default().capabilities()
}

fn create() -> GainEffect {
    GainEffect::default()
}

export_plugin!(GainEffect, descriptor, capabilities, create);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parameter_changes_start_at_their_sample_offsets() {
        let mut plugin = GainEffect::default();
        let mut input = [0.5; 32];
        let mut output = [0.0; 32];
        let mut audio = AudioBuffer::new(&mut input, 2, &mut output, 2, &mut [], 0, 16);
        // Event order is deliberately different from sample order.
        plugin.process(
            &ProcessContext::default(),
            &mut audio,
            &[
                Event::parameter(12, 1, 60.0 / 72.0),
                Event::parameter(7, 1, 48.0 / 72.0),
            ],
        );
        for channel in 0..2 {
            for frame in 0..16 {
                let expected = if (7..12).contains(&frame) {
                    0.5 * 10f32.powf(-12.0 / 20.0)
                } else {
                    0.5
                };
                assert!((audio.outputs[channel * 16 + frame] - expected).abs() < 1e-6);
            }
        }
    }
}
