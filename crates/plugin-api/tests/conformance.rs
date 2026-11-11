use plugin_api::conformance::{gate, run, ConformanceConfig};
use plugin_api::*;
use std::f32::consts::TAU;

#[derive(Default)]
struct Sine {
    phase: f32,
    frequency: f32,
    gain: f32,
    sample_rate: f32,
}

impl Plugin for Sine {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "com.example.sine",
            name: "Sine",
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
        vec![UiNode::Knob {
            param: 1,
            label: "Gain".into(),
        }]
    }

    fn prepare(&mut self, context: &ProcessContext) {
        self.sample_rate = context.sample_rate as f32;
    }

    fn reset(&mut self) {
        self.phase = 0.0;
    }

    fn process(
        &mut self,
        _context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        for event in events {
            if event.kind == EventKind::NoteOn {
                self.frequency = 440.0 * 2f32.powf((event.data1 as f32 - 69.0) / 12.0);
            }
        }

        let increment = if self.sample_rate > 0.0 {
            TAU * self.frequency / self.sample_rate
        } else {
            0.0
        };
        for frame in 0..audio.frames {
            let value = self.phase.sin() * 0.2 * 10f32.powf(self.gain / 20.0);
            for channel in 0..audio.output_channels {
                audio.output_mut(channel)[frame] += value;
            }
            self.phase = (self.phase + increment) % TAU;
        }
    }

    fn save_state(&self) -> State {
        State {
            schema_version: 1,
            bytes: self.gain.to_le_bytes().to_vec(),
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
        self.gain = f32::from_le_bytes(state.bytes[..4].try_into().unwrap());
        Ok(())
    }
}

struct Broken;

impl Plugin for Broken {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "com.example.broken",
            name: "Broken",
            vendor: "Example",
            version: "1.0.0",
            kind: PluginKind::Effect,
        }
    }

    fn capabilities(&self) -> plugin_api::Capabilities {
        plugin_api::Capabilities::stereo_effect()
    }

    fn prepare(&mut self, _context: &ProcessContext) {}
    fn reset(&mut self) {}

    fn process(
        &mut self,
        _context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        _events: &[Event],
    ) {
        for channel in 0..audio.output_channels {
            audio.output_mut(channel).fill(f32::NAN);
        }
    }
}

#[test]
fn well_formed_plugin_passes() {
    let mut plugin = Sine::default();
    let report = run(
        &mut plugin,
        &ConformanceConfig {
            expect_sound: true,
            ..ConformanceConfig::default()
        },
    );
    assert!(report.passed(), "{:?}", report.failures);
}

#[test]
fn non_finite_output_fails() {
    let mut plugin = Broken;
    let report = run(&mut plugin, &ConformanceConfig::default());
    assert!(!report.passed());
}

#[test]
fn state_round_trips() {
    let mut plugin = Sine {
        gain: -6.0,
        ..Sine::default()
    };
    let state = plugin.save_state();
    plugin.reset();
    plugin.load_state(&state).expect("state loads");
    assert_eq!(plugin.save_state(), state);
}

#[test]
fn rejects_future_state_schema() {
    let mut plugin = Sine::default();
    let future = State {
        schema_version: 2,
        bytes: vec![0, 0, 0, 0],
    };
    assert!(matches!(
        plugin.load_state(&future),
        Err(StateError::SchemaTooNew { .. })
    ));
}

#[test]
fn gate_certifies_well_formed_plugins_only() {
    let mut good = Sine::default();
    let certificate = gate(
        &mut good,
        &ConformanceConfig {
            expect_sound: true,
            ..Default::default()
        },
    )
    .expect("conformance passes");
    assert_eq!(certificate.plugin_id, "com.example.sine");
    assert_eq!(certificate.api_version, plugin_api::API_VERSION);

    let mut bad = Broken;
    assert!(gate(&mut bad, &ConformanceConfig::default()).is_err());
}
