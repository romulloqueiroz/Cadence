use host::{render_note, Host, NoteSpec};
use plugin_api::Plugin;
use sample_adapter::SamplerPlugin;

const PIANO: &str = "/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki";

// Missing optional content may skip; present-but-unloadable content must fail.
fn piano_if_present() -> Option<SamplerPlugin> {
    match std::fs::metadata(PIANO) {
        Ok(_) => Some(SamplerPlugin::load(PIANO).expect("installed Piano Uno must load")),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("SKIP Piano Uno: fixture is not installed");
            None
        }
        Err(error) => panic!("cannot inspect Piano Uno fixture: {error}"),
    }
}

#[test]
fn renders_kontakt_piano_if_present() {
    let Some(mut plugin) = piano_if_present() else {
        return;
    };
    let preloaded = plugin.preload_note(60, 100);
    assert!(preloaded > 0, "no samples preloaded");
    assert_eq!(plugin.capabilities().outputs, 2);

    let mut host = Host::new(Box::new(plugin), 48_000.0, 256);
    host.prepare();
    let audio = render_note(&mut host, NoteSpec::new(60, 100, 0.0, 0.5), 2.0);

    assert!(audio.peak() > 0.01, "peak {}", audio.peak());
    assert!(audio
        .left
        .iter()
        .chain(&audio.right)
        .all(|value| value.is_finite()));
}

#[test]
fn sampler_passes_conformance_if_present() {
    let Some(mut plugin) = piano_if_present() else {
        return;
    };
    plugin.preload_note(60, 100);

    let report = plugin_api::conformance::run(
        &mut plugin,
        &plugin_api::conformance::ConformanceConfig {
            expect_sound: true,
            ..Default::default()
        },
    );
    assert!(report.passed(), "{:?}", report.failures);
}

#[test]
fn real_piano_streaming_matches_ram_and_uses_bounded_memory_if_present() {
    use plugin_api::{AudioBuffer, Event, ProcessContext};
    use sample_adapter::StreamingConfig;
    let Some(mut streamed) = piano_if_present() else {
        return;
    };
    let initial = streamed.stream_all(StreamingConfig::default()).unwrap();
    let mut resident = SamplerPlugin::load(PIANO).unwrap();
    resident.preload_all();
    let full_bytes = resident.streaming_stats().resident_bytes;
    assert!(initial.resident_bytes < full_bytes);
    streamed.set_offline_rendering(true);
    let mut context = ProcessContext {
        block_size: 257,
        ..Default::default()
    };
    resident.prepare(&context);
    streamed.prepare(&context);
    let mut a = vec![0.0; 514];
    let mut b = a.clone();
    let mut max_error = 0.0f32;
    for i in 0..900 {
        let events = match i {
            0 => &[
                Event::note_on(13, 48, 100),
                Event::note_on(13, 60, 100),
                Event::note_on(13, 67, 100),
            ][..],
            650 => &[
                Event::note_off(31, 48),
                Event::note_off(31, 60),
                Event::note_off(31, 67),
            ][..],
            _ => &[],
        };
        a.fill(0.0);
        b.fill(0.0);
        resident.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut a, 2, &mut [], 0, 257),
            events,
        );
        streamed.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut b, 2, &mut [], 0, 257),
            events,
        );
        max_error = a
            .iter()
            .zip(&b)
            .fold(max_error, |e, (a, b)| e.max((a - b).abs()));
        assert_eq!(a, b, "piano block {i}");
        context.sample_pos += 257;
    }
    let stats = streamed.streaming_stats();
    assert!(stats.pages_read > 0);
    assert_eq!(stats.underrun_frames, 0);
    assert_eq!(stats.read_errors, 0);
    assert_eq!(stats.resident_bytes, initial.resident_bytes);
    println!("PIANO_STREAMING {{\"samples\":{},\"full_ram_bytes\":{},\"resident_bytes\":{},\"page_cache_bytes\":{},\"pages_read\":{},\"underrun_frames\":{},\"max_sample_error\":{}}}", streamed.cached_samples(), full_bytes, stats.resident_bytes, stats.page_cache_bytes, stats.pages_read, stats.underrun_frames, max_error);
}

#[test]
fn real_piano_realtime_pages_keep_up_if_present() {
    use plugin_api::{AudioBuffer, Event, ProcessContext};
    use sample_adapter::StreamingConfig;
    use std::{
        thread,
        time::{Duration, Instant},
    };
    let Some(mut plugin) = piano_if_present() else {
        return;
    };
    plugin.stream_all(StreamingConfig::default()).unwrap();
    let mut context = ProcessContext {
        block_size: 256,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; 512];
    let start = Instant::now();
    let mut maximum = Duration::ZERO;
    let mut peak = 0.0f32;
    for i in 0..600 {
        let events = if i == 0 {
            &[
                Event::note_on(0, 60, 100),
                Event::note_on(0, 64, 100),
                Event::note_on(0, 67, 100),
            ][..]
        } else {
            &[]
        };
        output.fill(0.0);
        let before = Instant::now();
        plugin.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256),
            events,
        );
        maximum = maximum.max(before.elapsed());
        peak = output.iter().fold(peak, |p, x| p.max(x.abs()));
        context.sample_pos += 256;
        let next = start + Duration::from_secs_f64((i + 1) as f64 * 256.0 / 48000.0);
        if let Some(wait) = next.checked_duration_since(Instant::now()) {
            thread::sleep(wait);
        }
    }
    let stats = plugin.streaming_stats();
    assert!(peak > 0.01);
    assert!(stats.pages_read > 0);
    assert_eq!(stats.underrun_frames, 0, "{stats:?}");
    assert_eq!(stats.read_errors, 0);
    println!("PIANO_REALTIME {{\"callbacks\":600,\"block_frames\":256,\"max_callback_us\":{},\"pages_read\":{},\"underrun_frames\":{},\"peak\":{}}}", maximum.as_micros(), stats.pages_read, stats.underrun_frames, peak);
}
