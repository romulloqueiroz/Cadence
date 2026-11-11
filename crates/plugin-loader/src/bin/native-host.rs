use host::chain::{render_note, Chain};
use host::demo::SinePlugin;
use host::NoteSpec;
use plugin_api::Plugin;
use plugin_loader::LoadedPlugin;
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut plugin_path = None;
    let mut output = None;
    let mut note = 69u8;
    let mut velocity = 100u8;
    let mut seconds = 2.0f64;
    let mut rate = 48_000f64;
    let mut block = 256usize;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--note" => {
                note = value.and_then(|v| v.parse().ok()).unwrap_or(note);
                i += 2;
            }
            "--velocity" => {
                velocity = value.and_then(|v| v.parse().ok()).unwrap_or(velocity);
                i += 2;
            }
            "--seconds" => {
                seconds = value.and_then(|v| v.parse().ok()).unwrap_or(seconds);
                i += 2;
            }
            "--rate" => {
                rate = value.and_then(|v| v.parse().ok()).unwrap_or(rate);
                i += 2;
            }
            "--block" => {
                block = value.and_then(|v| v.parse().ok()).unwrap_or(block);
                i += 2;
            }
            other => {
                if plugin_path.is_none() {
                    plugin_path = Some(other.to_string());
                } else if output.is_none() {
                    output = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    let (Some(plugin_path), Some(output)) = (plugin_path, output) else {
        eprintln!(
            "usage: native-host <plugin.dylib> <out.wav> [--note 69] [--velocity 100] [--seconds 2] [--rate 48000] [--block 256]"
        );
        return ExitCode::from(2);
    };

    let plugin = match LoadedPlugin::load(&plugin_path) {
        Ok(plugin) => plugin,
        Err(e) => {
            eprintln!("{plugin_path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let descriptor = plugin.descriptor();
    let capabilities = plugin.capabilities();
    eprintln!(
        "loaded {} ({}) {} in={} out={} latency={}",
        descriptor.name,
        descriptor.id,
        descriptor.version,
        capabilities.inputs,
        capabilities.outputs,
        capabilities.latency_samples
    );

    let mut chain = Chain::new(rate, block);
    chain.add(Box::new(SinePlugin::default()), false);
    chain.add(Box::new(plugin), false);
    chain.prepare();

    let audio = render_note(
        &mut chain,
        NoteSpec::new(note, velocity, 0.1, seconds * 0.6),
        seconds,
    );

    if let Err(e) = audio.write_wav(&output) {
        eprintln!("{output}: {e}");
        return ExitCode::FAILURE;
    }

    println!(
        "{}: {} frames, peak {:.3}, rms {:.4} -> {output}",
        descriptor.name,
        audio.frames(),
        audio.peak(),
        audio.rms()
    );
    ExitCode::SUCCESS
}
