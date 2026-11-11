use nk_format::ncw;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() != 2 {
        eprintln!("usage: ncw-decode <input.ncw> <output.wav>");
        return ExitCode::from(2);
    }

    let bytes = match std::fs::read(&args[0]) {
        Ok(bytes) => bytes,
        Err(e) => {
            eprintln!("{}: {e}", args[0]);
            return ExitCode::FAILURE;
        }
    };
    let sample = match ncw::parse(&bytes) {
        Ok(sample) => sample,
        Err(e) => {
            eprintln!("{}: {e}", args[0]);
            return ExitCode::FAILURE;
        }
    };

    if let Err(e) = write_wav_pcm16(&args[1], &sample) {
        eprintln!("{}: {e}", args[1]);
        return ExitCode::FAILURE;
    }

    println!(
        "{}: {} ch, {} Hz, {} bits, {} frames -> {}",
        args[0],
        sample.channels,
        sample.sample_rate,
        sample.bits_per_sample,
        sample.num_frames,
        args[1]
    );
    ExitCode::SUCCESS
}

fn write_wav_pcm16(path: &str, sample: &ncw::Ncw) -> io::Result<()> {
    let channels = sample.channels as u16;
    let frames = sample.num_frames;
    let data_size = (frames * sample.channels * 2) as u32;
    let mut file = BufWriter::new(File::create(path)?);

    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&sample.sample_rate.to_le_bytes())?;
    file.write_all(&(sample.sample_rate * channels as u32 * 2).to_le_bytes())?;
    file.write_all(&(channels * 2).to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;

    for frame in 0..frames {
        for channel in 0..sample.channels {
            let value = sample.samples[channel][frame].clamp(-1.0, 1.0);
            let pcm = (value * i16::MAX as f32).round() as i16;
            file.write_all(&pcm.to_le_bytes())?;
        }
    }
    file.flush()
}
