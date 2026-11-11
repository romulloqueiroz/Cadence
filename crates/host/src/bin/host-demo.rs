use host::demo::SinePlugin;
use host::{render_note, Host, NoteSpec};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut out = None;
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
                if out.is_none() {
                    out = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    let Some(out) = out else {
        eprintln!(
            "usage: host-demo <out.wav> [--note 69] [--velocity 100] [--seconds 2] [--rate 48000] [--block 256]"
        );
        return ExitCode::from(2);
    };

    let mut host = Host::new(Box::new(SinePlugin::default()), rate, block);
    host.prepare();

    let audio = render_note(
        &mut host,
        NoteSpec::new(note, velocity, 0.1, seconds * 0.6),
        seconds,
    );

    if let Err(e) = audio.write_wav(&out) {
        eprintln!("{out}: {e}");
        return ExitCode::FAILURE;
    }

    let descriptor = host.descriptor();
    println!(
        "{} ({}) {}: {} frames, peak {:.3}, rms {:.4} -> {out}",
        descriptor.name,
        descriptor.id,
        descriptor.version,
        audio.frames(),
        audio.peak(),
        audio.rms()
    );
    ExitCode::SUCCESS
}
