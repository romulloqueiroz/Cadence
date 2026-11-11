use nk_format::loader::LoadedInstrument;
use nk_format::model::{Group, Instrument};
use nk_format::{audio, zone};
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

struct Options {
    nki: PathBuf,
    out: PathBuf,
    note: i32,
    velocity: u16,
    group: Option<u32>,
    seconds: f32,
    release: f32,
    gain: f32,
    rate: u32,
}

struct Voice<'a> {
    zone: &'a zone::Zone,
    trigger_frame: usize,
    is_release: bool,
}

fn main() -> ExitCode {
    let options = match parse_args() {
        Ok(options) => options,
        Err(message) => {
            eprintln!("{message}");
            usage();
            return ExitCode::from(2);
        }
    };

    match render(&options) {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn render(options: &Options) -> Result<(), String> {
    let mut loaded_instrument = LoadedInstrument::load(&options.nki).map_err(|e| e.to_string())?;
    let instrument = loaded_instrument.instrument.clone();
    let mapped_note = options.note + instrument.program.midi_transpose;
    let has_solo = instrument.groups.iter().any(|group| group.soloed);

    let matching: Vec<&zone::Zone> = instrument
        .zones
        .iter()
        .filter(|z| {
            z.low_key as i32 <= mapped_note
                && mapped_note <= z.high_key as i32
                && z.low_velocity <= options.velocity
                && options.velocity <= z.high_velocity
                && z.filename_id.is_some()
                && instrument
                    .group(z.group_index)
                    .map_or(true, |group| !group.muted && (!has_solo || group.soloed))
        })
        .collect();

    if matching.is_empty() {
        return Err(format!(
            "no zone matches note {} velocity {}",
            options.note, options.velocity
        ));
    }

    let out_frames = (options.seconds * options.rate as f32) as usize;
    let note_off_frame =
        ((options.seconds - options.release).max(0.0) * options.rate as f32) as usize;
    let mut left = vec![0.0f32; out_frames];
    let mut right = vec![0.0f32; out_frames];

    let voices = select_voices(&instrument, matching.clone(), options, note_off_frame);
    if voices.is_empty() {
        return Err("no playable voices".to_string());
    }

    let mut loaded = 0usize;
    for voice in &voices {
        let sample_ref = instrument
            .sample_files
            .get(voice.zone.filename_id.unwrap() as usize)
            .ok_or_else(|| "sample id outside file list".to_string())?;
        let sample = loaded_instrument
            .decoded_sample(sample_ref)
            .map_err(|e| format!("{sample_ref}: {e}"))?;
        mix_voice(
            &sample,
            voice,
            &instrument,
            options,
            note_off_frame,
            &mut left,
            &mut right,
        );
        loaded += 1;
    }

    let release_voices = voices.iter().filter(|v| v.is_release).count();
    eprintln!(
        "program={:?} note={} velocity={} voices={} (release={}) groups={} -> {} frames @ {} Hz",
        instrument.program.name,
        options.note,
        options.velocity,
        loaded,
        release_voices,
        voices
            .iter()
            .map(|v| v.zone.group_index.to_string())
            .collect::<Vec<_>>()
            .join(","),
        out_frames,
        options.rate
    );

    write_wav(&options.out, options.rate, &[left, right])
        .map_err(|e| format!("{}: {e}", options.out.display()))?;
    Ok(())
}

fn select_voices<'a>(
    instrument: &'a Instrument,
    matching: Vec<&'a zone::Zone>,
    options: &Options,
    note_off_frame: usize,
) -> Vec<Voice<'a>> {
    if let Some(group) = options.group {
        // Audition a single group: trigger everything at note-on.
        return matching
            .into_iter()
            .filter(|z| z.group_index == group)
            .map(|zone| Voice {
                zone,
                trigger_frame: 0,
                is_release: false,
            })
            .collect();
    }

    let mut main = Vec::new();
    let mut release = Vec::new();
    for zone in matching {
        let group = instrument.group(zone.group_index);
        if group.is_some_and(is_release_group) {
            release.push(Voice {
                zone,
                trigger_frame: note_off_frame,
                is_release: true,
            });
        } else if group.is_some_and(is_pedal_group) {
            // Pedal layers only make sense with the sustain pedal held.
        } else {
            main.push(Voice {
                zone,
                trigger_frame: 0,
                is_release: false,
            });
        }
    }

    main.extend(release);
    main
}

fn is_release_group(group: &Group) -> bool {
    group.release_trigger
        || (group.name_trigger_heuristics && group.name.to_ascii_lowercase().contains("release"))
}

fn is_pedal_group(group: &Group) -> bool {
    group.name_trigger_heuristics && group.name.to_ascii_lowercase().contains("pedal")
}

#[allow(clippy::too_many_arguments)]
fn mix_voice(
    source: &audio::Sample,
    voice: &Voice<'_>,
    instrument: &Instrument,
    options: &Options,
    note_off_frame: usize,
    left: &mut [f32],
    right: &mut [f32],
) {
    let zone = voice.zone;
    let out_frames = left.len();
    if voice.trigger_frame >= out_frames {
        return;
    }

    let start = (zone.sample_start as usize).min(source.num_frames);
    let end = zone.end_frame(source.num_frames);
    if end <= start {
        return;
    }

    let gain = instrument.zone_gain(zone) * options.gain;
    let pan = instrument.zone_pan(zone);
    let (left_gain, right_gain) = linear_balance(pan);
    let tune = instrument.zone_tune(zone);
    let group = instrument.group(zone.group_index);
    let semitones = if group.map_or(true, |group| group.key_tracking) {
        options.note as f32 + instrument.program.midi_transpose as f32 - zone.root_key as f32
    } else {
        0.0
    };
    let speed =
        2.0f32.powf(semitones / 12.0) * tune * source.sample_rate as f32 / options.rate as f32;

    let loop_info = zone
        .loops
        .iter()
        .find(|l| l.length > 0 && (l.mode == 1 || l.mode == 2))
        .map(|l| {
            let loop_start = (l.start as usize).clamp(start, end);
            let loop_end = (l.start as usize)
                .saturating_add(l.length as usize)
                .clamp(loop_start, end);
            (loop_start, loop_end, l.alternating)
        });

    let envelope = instrument
        .group(zone.group_index)
        .and_then(|g| g.envelope.as_ref());

    let local_note_off =
        if voice.is_release || zone.one_shot || group.is_some_and(|group| group.one_shot) {
            f32::INFINITY
        } else if note_off_frame > voice.trigger_frame {
            (note_off_frame - voice.trigger_frame) as f32 / options.rate as f32
        } else {
            0.0
        };

    let fade_in = (0.003 * options.rate as f32).max(1.0) as usize;
    let fade_out = (0.03 * options.rate as f32).max(1.0) as usize;

    let reverse = group.is_some_and(|group| group.reverse);
    let mut position = if reverse {
        (end - 1) as f32
    } else {
        start as f32
    };
    let mut direction = if reverse { -1.0f32 } else { 1.0f32 };
    let mut looping = loop_info.is_some();

    for frame in voice.trigger_frame..out_frames {
        let local_frame = frame - voice.trigger_frame;
        let local_time = local_frame as f32 / options.rate as f32;

        if looping && local_time >= local_note_off && zone.loops.iter().any(|l| l.mode == 2) {
            looping = false;
        }
        if position >= end as f32 || position < start as f32 {
            break;
        }

        let index = position.floor() as usize;
        let frac = position.fract();
        let next = (index + 1).min(end - 1);
        let l = source.samples[0][index] * (1.0 - frac) + source.samples[0][next] * frac;
        let r = if source.channels > 1 {
            source.samples[1][index] * (1.0 - frac) + source.samples[1][next] * frac
        } else {
            l
        };

        let mut amplitude = gain;
        if let Some(env) = envelope {
            amplitude *= envelope_level(env, local_time, local_note_off);
        }
        if local_frame < fade_in {
            amplitude *= local_frame as f32 / fade_in as f32;
        }
        if out_frames - frame < fade_out {
            amplitude *= (out_frames - frame) as f32 / fade_out as f32;
        }

        left[frame] += l * amplitude * left_gain;
        right[frame] += r * amplitude * right_gain;

        position += speed * direction;

        if let Some((loop_start, loop_end, alternating)) = loop_info {
            if looping && loop_end > loop_start {
                if direction > 0.0 && position >= loop_end as f32 {
                    if alternating {
                        direction = -1.0;
                        position = 2.0 * loop_end as f32 - position;
                    } else {
                        position = loop_start as f32 + (position - loop_end as f32);
                    }
                } else if direction < 0.0 && position < loop_start as f32 {
                    if alternating {
                        direction = 1.0;
                        position = 2.0 * loop_start as f32 - position;
                    } else {
                        position = loop_end as f32 - (loop_start as f32 - position);
                    }
                }
            }
        }
    }
}

fn envelope_level(env: &nk_format::modulator::Envelope, time: f32, note_off: f32) -> f32 {
    let attack = (env.attack / 1000.0).max(0.0);
    let hold = (env.hold / 1000.0).max(0.0);
    let decay = (env.decay / 1000.0).max(0.0);
    let sustain = if env.ahd_only {
        0.0
    } else {
        env.sustain.clamp(0.0, 1.0)
    };
    let release = ((if env.ahd_only { env.decay } else { env.release }) / 1000.0).max(0.0);

    let sustain_level = |t: f32| {
        if t < attack {
            if attack <= 0.0 {
                1.0
            } else {
                t / attack
            }
        } else if t < attack + hold {
            1.0
        } else if t < attack + hold + decay {
            if decay <= 0.0 {
                sustain
            } else {
                1.0 + (sustain - 1.0) * ((t - attack - hold) / decay)
            }
        } else {
            sustain
        }
    };

    if time < note_off {
        sustain_level(time)
    } else {
        let level = sustain_level(note_off);
        let dt = time - note_off;
        if dt >= release {
            0.0
        } else {
            level * (1.0 - dt / release)
        }
    }
}

fn linear_balance(pan: f32) -> (f32, f32) {
    if pan >= 0.0 {
        (1.0 - pan, 1.0)
    } else {
        (1.0, 1.0 + pan)
    }
}

fn parse_args() -> Result<Options, String> {
    let mut args = std::env::args().skip(1);
    let nki = args.next().ok_or("missing input .nki")?.into();
    let out = args.next().ok_or("missing output .wav")?.into();

    let mut options = Options {
        nki,
        out,
        note: 60,
        velocity: 100,
        group: None,
        seconds: 4.0,
        release: 0.8,
        gain: 1.0,
        rate: 48_000,
    };

    let rest: Vec<String> = args.collect();
    let mut i = 0;
    while i < rest.len() {
        let value = |i: usize| {
            rest.get(i)
                .cloned()
                .ok_or_else(|| format!("missing value for {}", rest[i - 1]))
        };
        match rest[i].as_str() {
            "--note" => options.note = value(i + 1)?.parse().map_err(|_| "bad --note")?,
            "--velocity" => {
                options.velocity = value(i + 1)?.parse().map_err(|_| "bad --velocity")?
            }
            "--group" => options.group = Some(value(i + 1)?.parse().map_err(|_| "bad --group")?),
            "--seconds" => options.seconds = value(i + 1)?.parse().map_err(|_| "bad --seconds")?,
            "--release" => options.release = value(i + 1)?.parse().map_err(|_| "bad --release")?,
            "--gain" => options.gain = value(i + 1)?.parse().map_err(|_| "bad --gain")?,
            "--rate" => options.rate = value(i + 1)?.parse().map_err(|_| "bad --rate")?,
            other => return Err(format!("unknown option {other}")),
        }
        i += 2;
    }

    Ok(options)
}

fn write_wav(path: &Path, rate: u32, audio: &[Vec<f32>]) -> io::Result<()> {
    let frames = audio[0].len();
    let channels = audio.len() as u16;
    let data_size = (frames * audio.len() * 2) as u32;
    let mut file = BufWriter::new(File::create(path)?);

    file.write_all(b"RIFF")?;
    file.write_all(&(36 + data_size).to_le_bytes())?;
    file.write_all(b"WAVEfmt ")?;
    file.write_all(&16u32.to_le_bytes())?;
    file.write_all(&1u16.to_le_bytes())?;
    file.write_all(&channels.to_le_bytes())?;
    file.write_all(&rate.to_le_bytes())?;
    file.write_all(&(rate * channels as u32 * 2).to_le_bytes())?;
    file.write_all(&(channels * 2).to_le_bytes())?;
    file.write_all(&16u16.to_le_bytes())?;
    file.write_all(b"data")?;
    file.write_all(&data_size.to_le_bytes())?;

    for frame in 0..frames {
        for channel in audio.iter() {
            let value = channel[frame].clamp(-1.0, 1.0);
            let pcm = (value * i16::MAX as f32).round() as i16;
            file.write_all(&pcm.to_le_bytes())?;
        }
    }
    file.flush()
}

fn usage() {
    eprintln!(
        "usage: nki-render <file.nki> <out.wav> [--note 60] [--velocity 100] \
[--group N] [--seconds 4] [--release 0.8] [--gain 1] [--rate 48000]"
    );
}
