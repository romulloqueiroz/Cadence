use host::chain::{render_note as render_chain_note, Chain};
use host::demo::{GainEffect, SinePlugin};
use host::{render_note as render_host_note, Host, NoteSpec};
use plugin_api::{AudioBuffer, Capabilities, Descriptor, Plugin, PluginKind, ProcessContext};

fn note() -> NoteSpec {
    NoteSpec::new(69, 100, 0.0, 0.5)
}

#[test]
fn unity_gain_effect_matches_direct_instrument() {
    let mut host = Host::new(Box::new(SinePlugin::default()), 48_000.0, 256);
    host.prepare();
    let direct = render_host_note(&mut host, note(), 1.0);

    let mut chain = Chain::new(48_000.0, 256);
    chain.add(Box::new(SinePlugin::default()), false);
    chain.add(Box::new(GainEffect::default()), false);
    chain.prepare();
    let processed = render_chain_note(&mut chain, note(), 1.0);

    assert_eq!(direct.frames(), processed.frames());
    let max_diff = direct
        .left
        .iter()
        .zip(&processed.left)
        .map(|(a, b)| (a - b).abs())
        .fold(0.0f32, f32::max);
    assert!(max_diff < 1.0e-5, "max diff {max_diff}");
}

#[test]
fn attenuation_and_bypass_behave() {
    let mut bypassed = Chain::new(48_000.0, 256);
    bypassed.add(Box::new(SinePlugin::default()), false);
    bypassed.add(Box::new(GainEffect::default()), true);
    bypassed.prepare();
    let full = render_chain_note(&mut bypassed, note(), 1.0);
    assert!(full.peak() > 0.1);

    let mut attenuated_plugin = GainEffect::default();
    attenuated_plugin
        .load_state(&plugin_api::State {
            schema_version: 1,
            bytes: (-24.0f32).to_le_bytes().to_vec(),
        })
        .unwrap();

    let mut attenuated = Chain::new(48_000.0, 256);
    attenuated.add(Box::new(SinePlugin::default()), false);
    attenuated.add(Box::new(attenuated_plugin), false);
    attenuated.prepare();
    let quiet = render_chain_note(&mut attenuated, note(), 1.0);

    assert!(
        quiet.peak() < full.peak() * 0.2,
        "{} vs {}",
        quiet.peak(),
        full.peak()
    );
}

struct LatencyEffect;

impl Plugin for LatencyEffect {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "com.example.latency",
            name: "Latency",
            vendor: "Example",
            version: "1.0.0",
            kind: PluginKind::Effect,
        }
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            inputs: 2,
            outputs: 2,
            sidechain_inputs: 0,
            latency_samples: 64,
            tail_samples: 0,
        }
    }

    fn prepare(&mut self, _context: &ProcessContext) {}
    fn reset(&mut self) {}

    fn process(
        &mut self,
        _context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        _events: &[plugin_api::Event],
    ) {
        for channel in 0..audio.output_channels.min(audio.input_channels) {
            let input = audio.input(channel).to_vec();
            audio.output_mut(channel).copy_from_slice(&input);
        }
    }
}

#[test]
fn latency_is_summed() {
    let mut chain = Chain::new(48_000.0, 256);
    chain.add(Box::new(LatencyEffect), false);
    chain.add(Box::new(LatencyEffect), false);
    assert_eq!(chain.latency_samples(), 128);
}

#[test]
fn gain_effect_passes_conformance() {
    let mut effect = GainEffect::default();
    let report = plugin_api::conformance::run(
        &mut effect,
        &plugin_api::conformance::ConformanceConfig::default(),
    );
    assert!(report.passed(), "{:?}", report.failures);
}
