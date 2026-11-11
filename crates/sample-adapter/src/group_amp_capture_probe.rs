//! Explicit offline capture utility; not part of normal regression runs.
use super::*;

#[test]
#[ignore = "requires CADENCE_GROUP_CAPTURE_INPUT and CADENCE_GROUP_CAPTURE_OUTPUT"]
fn group_amp_render_native_candidate() {
    let input = std::env::var("CADENCE_GROUP_CAPTURE_INPUT").unwrap();
    let output = std::env::var("CADENCE_GROUP_CAPTURE_OUTPUT").unwrap();
    let mut plugin = SamplerPlugin::load(&input).unwrap();
    plugin.voices.truncate(4);
    plugin.rebuild_group_fx().unwrap();
    plugin.preload_all();
    assert!(plugin.zone_samples.iter().all(Option::is_some));
    let context = ProcessContext {
        sample_rate: 48_000.,
        block_size: 256,
        ..Default::default()
    };
    plugin.prepare(&context);
    plugin.reset();
    let frames = 480_000;
    let mut samples = Vec::with_capacity(frames * 2);
    let mut peak = 0f32;
    for start in (0..frames).step_by(context.block_size) {
        let count = (frames - start).min(context.block_size);
        let context = ProcessContext {
            block_size: count,
            sample_pos: start as i64,
            ..context
        };
        let mut buffer = vec![0.; count * 2];
        let mut events = Vec::new();
        for (at, mut event) in [
            (4800, Event::note_on(0, 60, 100)),
            (292800, Event::note_off(0, 60)),
        ] {
            if (start..start + count).contains(&at) {
                event.frame = at - start;
                events.push(event);
            }
        }
        block(&mut plugin, &context, &mut buffer, &events);
        for frame in 0..count {
            for value in [buffer[frame], buffer[count + frame]] {
                assert!(value.is_finite());
                peak = peak.max(value.abs());
                samples.push(value);
            }
        }
    }
    let data_bytes = (frames * 8) as u32;
    let mut file = std::io::BufWriter::new(File::create(output).unwrap());
    file.write_all(b"RIFF").unwrap();
    file.write_all(&(data_bytes + 36).to_le_bytes()).unwrap();
    file.write_all(b"WAVEfmt ").unwrap();
    file.write_all(&16u32.to_le_bytes()).unwrap();
    file.write_all(&3u16.to_le_bytes()).unwrap();
    file.write_all(&2u16.to_le_bytes()).unwrap();
    file.write_all(&48000u32.to_le_bytes()).unwrap();
    file.write_all(&384000u32.to_le_bytes()).unwrap();
    file.write_all(&8u16.to_le_bytes()).unwrap();
    file.write_all(&32u16.to_le_bytes()).unwrap();
    file.write_all(b"data").unwrap();
    file.write_all(&data_bytes.to_le_bytes()).unwrap();
    for value in samples {
        file.write_all(&value.to_le_bytes()).unwrap();
    }
    file.flush().unwrap();
    assert!(peak > 1e-6, "candidate did not produce audible output");
    eprintln!(
        "GROUP_CAPTURE groups={} zones={} frames={frames} peak={peak} input={input}",
        plugin.groups.len(),
        plugin.zones.len()
    );
}
