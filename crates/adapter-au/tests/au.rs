#![cfg(target_os = "macos")]

use plugin_api::{AudioBuffer, Plugin, ProcessContext};

#[test]
fn renders_apple_eq_if_present() {
    let Ok(mut plugin) = adapter_au::AuPlugin::load("AUNBandEQ") else {
        return;
    };
    let context = ProcessContext::default();
    plugin.prepare(&context);

    let frames = 256;
    let mut inputs = vec![0.0f32; 2 * frames];
    let mut outputs = vec![0.0f32; 2 * frames];
    let mut sidechain = Vec::new();
    let mut audible = false;

    for block in 0..8 {
        for frame in 0..frames {
            let phase = ((block * frames) + frame) as f32 * 0.05;
            let value = phase.sin() * 0.25;
            inputs[frame] = value;
            inputs[frames + frame] = value;
        }
        let mut block_context = context;
        block_context.sample_pos = (block * frames) as i64;

        let mut audio =
            AudioBuffer::new(&mut inputs, 2, &mut outputs, 2, &mut sidechain, 0, frames);
        plugin.process(&block_context, &mut audio, &[]);

        assert!(outputs.iter().all(|value| value.is_finite()));
        audible |= outputs.iter().any(|value| value.abs() > 0.001);
    }

    assert!(audible, "AU produced silence");
}
