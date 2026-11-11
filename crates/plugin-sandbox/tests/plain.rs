//! Production source-dispatch/loading path without shared-memory IPC.
use plugin_api::{AudioBuffer, Event, ProcessContext};
use std::io::Write;

#[test]
fn worker_source_loader_streams_plain_xml_and_reports_missing_samples() {
    let directory =
        std::env::temp_dir().join(format!("cadence-worker-plain-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("Samples")).unwrap();
    let mut wav = Vec::new();
    let frames = 128u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + frames * 4).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&192_000u32.to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(frames * 4).to_le_bytes());
    for _ in 0..frames {
        wav.extend_from_slice(&10_000i16.to_le_bytes());
        wav.extend_from_slice(&(-5_000i16).to_le_bytes());
    }
    let sample = directory.join("Samples/tone Ω.wav");
    std::fs::write(&sample, wav).unwrap();
    let xml = include_str!("../../nk-format/tests/fixtures/plain-playback.xml");
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml.as_bytes()).unwrap();
    let mut data = vec![0; 0xaa];
    data[..4].copy_from_slice(&nk_format_magic().to_le_bytes());
    data.extend(encoder.finish().unwrap());
    let path = directory.join("plain.nki");
    std::fs::write(&path, data).unwrap();
    let mut plugin = plugin_sandbox::worker::load_plugin(path.to_str().unwrap(), None).unwrap();
    let context = ProcessContext {
        block_size: 32,
        ..Default::default()
    };
    plugin.prepare(&context);
    plugin.set_offline_rendering(true);
    let mut output = [0.0; 64];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 32);
    plugin.process(&context, &mut audio, &[Event::note_on(2, 48, 100)]);
    assert!(plugin.processing_error().is_none());
    for channel in 0..2 {
        for frame in 0..32 {
            let expected = if (2..9).contains(&frame) {
                if channel == 0 {
                    10_000.0 / 32768.0 * 0.1
                } else {
                    -5_000.0 / 32768.0 * 0.1
                }
            } else {
                0.0
            };
            assert!((output[channel * 32 + frame] - expected).abs() < 1e-6);
        }
    }
    assert!(plugin.streaming_telemetry().unwrap().resident_bytes > 0);
    drop(plugin);
    std::fs::remove_file(sample).unwrap();
    let error = match plugin_sandbox::worker::load_plugin(path.to_str().unwrap(), None) {
        Ok(_) => panic!("missing sample accepted"),
        Err(error) => error,
    };
    assert!(error.contains("not found"), "{error}");
    std::fs::remove_dir_all(directory).unwrap();
}

fn nk_format_magic() -> u32 {
    0x7fa8_9012
}
