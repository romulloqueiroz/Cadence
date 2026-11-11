use host::{render_note, Host, NoteSpec};
use plugin_api::{Plugin, ProcessContext};
use sample_adapter::SamplerPlugin;
use std::io::{self, Write};
use std::process::ExitCode;

fn write_float_wav(audio: &host::RenderedAudio, path: &str) -> io::Result<()> {
    let data_bytes = u32::try_from(
        audio
            .frames()
            .checked_mul(8)
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "render too large"))?,
    )
    .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "render too large"))?;
    let mut file = io::BufWriter::new(std::fs::File::create(path)?);
    file.write_all(b"RIFF")?;
    file.write_all(&(data_bytes + 36).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&3u16.to_le_bytes())?; // IEEE float, no PCM clipping/quantization.
    file.write_all(&2u16.to_le_bytes())?;
    file.write_all(&audio.sample_rate.to_le_bytes())?;
    file.write_all(&(audio.sample_rate * 8).to_le_bytes())?;
    file.write_all(&8u16.to_le_bytes())?;
    file.write_all(&32u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_bytes.to_le_bytes())?;
    for frame in 0..audio.frames() {
        file.write_all(&audio.left[frame].to_le_bytes())?;
        file.write_all(&audio.right[frame].to_le_bytes())?;
    }
    file.flush()
}

fn main() -> ExitCode {
    let mut input = None;
    let mut output = None;
    let mut note = 60u8;
    let mut velocity = 100u8;
    let mut seconds = 3.0f64;
    let mut rate = 48_000f64;
    let mut block = 256usize;
    let mut resident = false;
    let mut float_wav = false;

    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut i = 0;
    while i < args.len() {
        let value = args.get(i + 1).cloned();
        match args[i].as_str() {
            "--float-wav" => {
                float_wav = true;
                i += 1;
            }
            "--resident" => {
                resident = true;
                i += 1;
            }
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
                if input.is_none() {
                    input = Some(other.to_string());
                } else if output.is_none() {
                    output = Some(other.to_string());
                }
                i += 1;
            }
        }
    }

    let (Some(input), Some(output)) = (input, output) else {
        eprintln!(
            "usage: sample-host <file.nki> <out.wav> [--note 60] [--velocity 100] [--seconds 3] [--rate 48000] [--block 256] [--resident] [--float-wav]"
        );
        return ExitCode::from(2);
    };

    let mut plugin = match SamplerPlugin::load(&input) {
        Ok(plugin) => plugin,
        Err(e) => {
            eprintln!("{input}: {e}");
            return ExitCode::FAILURE;
        }
    };

    for notice in plugin.load_notices() {
        eprintln!("Compatibility: {notice}");
    }
    let preloaded = if resident {
        plugin.preload_note(note as i32, velocity as u16)
    } else {
        match plugin.stream_all(sample_adapter::StreamingConfig::default()) {
            Ok(stats) => eprintln!("streaming={stats:?}"),
            Err(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
        plugin.cached_samples()
    };
    eprintln!(
        "program={:?} preloaded={preloaded} samples",
        plugin.program_name()
    );
    if preloaded == 0 {
        eprintln!("no playable samples were cached for this instrument/note");
        return ExitCode::FAILURE;
    }

    plugin.prepare(&ProcessContext {
        sample_rate: rate,
        block_size: block,
        ..Default::default()
    });
    eprintln!(
        "convolution_memory_bytes={} tail_samples={}",
        plugin.convolution_memory_bytes(),
        plugin.current_tail_samples()
    );
    let mut host = Host::new(Box::new(plugin), rate, block);
    host.prepare();

    let audio = render_note(
        &mut host,
        NoteSpec::new(note, velocity, 0.1, seconds * 0.6),
        seconds,
    );

    if let Some(error) = host.plugin().processing_error() {
        eprintln!("{error}");
        return ExitCode::FAILURE;
    }

    let written = if float_wav {
        write_float_wav(&audio, &output)
    } else {
        audio.write_wav(&output)
    };
    if let Err(e) = written {
        eprintln!("{output}: {e}");
        return ExitCode::FAILURE;
    }

    println!(
        "{}: {} frames, peak {:.3}, rms {:.4} -> {output}",
        host.descriptor().name,
        audio.frames(),
        audio.peak(),
        audio.rms()
    );
    ExitCode::SUCCESS
}
