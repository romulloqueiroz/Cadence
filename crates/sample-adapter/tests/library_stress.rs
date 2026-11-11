//! Dense offline performance against a decodable Kontakt library, intended for
//! external-library validation. Skips when the library is absent.
//!
//! Override the instrument with `CADENCE_LARGE_LIBRARY=/path/to/file.nki`.

use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext};
use sample_adapter::{SamplerPlugin, StreamingConfig};
use std::path::Path;

const DEFAULT: &str = "/Volumes/T7/Shelter Booms/FMG_Shelter_Booms/FMG Shelter Booms.nki";
const BLOCKS: [usize; 4] = [64, 128, 256, 512];

fn library() -> Option<String> {
    let path = std::env::var("CADENCE_LARGE_LIBRARY").unwrap_or_else(|_| DEFAULT.into());
    Path::new(&path).exists().then_some(path)
}

fn schedule(total: usize) -> Vec<(usize, Event)> {
    let mut events = Vec::new();
    // A new note every 50 ms, cycling across the library's mapped range, with
    // overlapping releases, velocity variation and two sustain-pedal changes.
    for (index, start) in (0..total).step_by(2_400).enumerate() {
        let key = 48 + (index % 25) as u8;
        let velocity = 40 + ((index * 13) % 80) as u8;
        events.push((start, Event::note_on(0, key, velocity)));
        if index % 3 == 0 {
            let harmony = (key + 7).min(127);
            events.push((
                start,
                Event::note_on(0, harmony, velocity.saturating_sub(10).max(1)),
            ));
            events.push((start + 9_600, Event::note_off(0, harmony)));
        }
        events.push((start + 19_200, Event::note_off(0, key)));
    }
    events.push((48_000, Event::control_change(0, 64, 127)));
    events.push((240_000, Event::control_change(0, 64, 0)));
    events.sort_by_key(|(frame, _)| *frame);
    events
}

#[test]
fn decodable_large_library_streams_a_dense_performance() {
    let Some(path) = library() else {
        eprintln!("skip: no decodable large library at {DEFAULT}");
        return;
    };
    let mut plugin = SamplerPlugin::load(&path).unwrap();
    let initial = plugin.stream_all(StreamingConfig::default()).unwrap();
    plugin.set_offline_rendering(true);
    let mut context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    plugin.prepare(&context);

    let total = 48_000 * 10;
    let events = schedule(total);
    let mut output = vec![0.0f32; BLOCKS[3] * 2];
    let mut peak = 0.0f32;
    let mut finite = true;
    let mut frame = 0usize;
    let mut index = 0usize;
    let mut callbacks = 0usize;
    let mut max_frames = 0usize;
    while frame < total {
        let block = BLOCKS[callbacks % BLOCKS.len()];
        let count = block.min(total - frame);
        let end = frame + count;
        let mut active = Vec::new();
        while index < events.len() && events[index].0 < end {
            let (at, mut event) = events[index];
            if at >= frame {
                event.frame = at - frame;
                active.push(event);
            }
            index += 1;
        }
        context.block_size = count;
        context.sample_pos = frame as i64;
        output[..count * 2].fill(0.0);
        let start = std::time::Instant::now();
        plugin.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut output[..count * 2], 2, &mut [], 0, count),
            &active,
        );
        max_frames = max_frames.max(start.elapsed().as_micros() as usize);
        for value in &output[..count * 2] {
            finite &= value.is_finite();
            peak = peak.max(value.abs());
        }
        frame = end;
        callbacks += 1;
    }
    let stats = plugin.streaming_stats();
    assert!(finite, "non-finite audio");
    assert!(peak > 0.01, "silent render, peak {peak}");
    assert_eq!(stats.read_errors, 0, "{stats:?}");
    assert_eq!(stats.underrun_frames, 0, "{stats:?}");
    assert!(
        stats.pages_read > 0,
        "library stayed fully resident: {stats:?}"
    );
    assert_eq!(stats.resident_bytes, initial.resident_bytes);
    println!(
        "LIBRARY_STRESS {{\"path\":\"{path}\",\"seconds\":10,\"callbacks\":{callbacks},\"peak\":{peak},\"samples\":{},\"page_cache_bytes\":{},\"resident_bytes\":{},\"pages_read\":{},\"underrun_frames\":{},\"read_errors\":{},\"max_callback_us\":{max_frames}}}",
        plugin.cached_samples(),
        stats.page_cache_bytes,
        stats.resident_bytes,
        stats.pages_read,
        stats.underrun_frames,
        stats.read_errors,
    );
}

#[test]
fn encrypted_libraries_load_with_keys_or_fail_explicitly() {
    let encrypted = [
        "/Volumes/T7/GroveBass/Instruments/GroveBass.nki",
        "/Volumes/T7/Jacob Collier Audience Choir Library/Instruments/Jacob Collier Audience Choir.nki",
        "/Volumes/T7/Users/Shared/Krimh Drums Free/Instruments/Krimh Drums Free.nki",
    ];
    let mut checked = 0;
    for path in encrypted {
        if !Path::new(path).exists() {
            continue;
        }
        checked += 1;
        match SamplerPlugin::load(path) {
            Ok(plugin) => {
                assert!(!plugin.program_name().is_empty());
                println!(
                    "RETAIL_DECODED {{\"path\":\"{path}\",\"program\":\"{}\"}}",
                    plugin.program_name()
                );
            }
            Err(error) => {
                let message = error.to_string();
                assert!(
                    message.contains("encrypted")
                        || message.contains("Kontakt")
                        || message.contains("retail"),
                    "{path}: unexpected error {message}"
                );
                println!("RETAIL_UNAVAILABLE {{\"path\":\"{path}\",\"error\":\"{message}\"}}");
            }
        }
    }
    if checked == 0 {
        eprintln!("skip: no retail libraries found");
    }
}
