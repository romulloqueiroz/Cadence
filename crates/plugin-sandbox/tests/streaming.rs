//! Real adapter through the shipping worker; optional installed-library fixture.
use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext};
use plugin_sandbox::{host::SandboxConfig, SandboxPlugin};
use sample_adapter::SamplerPlugin;
use std::{path::Path, time::Duration};

#[test]
fn kontakt_worker_streams_and_matches_resident_reference_if_present() {
    let piano = Path::new("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki");
    if !piano.exists() {
        return;
    }
    let mut reference = SamplerPlugin::load(piano).unwrap();
    reference.preload_note(60, 100);
    let mut worker = SandboxPlugin::spawn_with_config(
        env!("CARGO_BIN_EXE_plugin-worker"),
        piano,
        SandboxConfig {
            block_timeout: Duration::from_secs(3),
            ..Default::default()
        },
    )
    .unwrap();
    let mut context = ProcessContext {
        block_size: 256,
        ..Default::default()
    };
    reference.prepare(&context);
    worker.prepare(&context);
    let initial = worker.pipeline_monitor().streaming_telemetry().unwrap();
    assert_eq!(initial.active_voices, 0);
    assert_eq!(initial.voice_capacity, 64);
    assert_eq!(initial.cache_capacity_bytes, 16 * 1024 * 1024);
    let mut a = vec![0.0; 512];
    let mut b = a.clone();
    for index in 0..500 {
        let events = if index == 0 {
            &[Event::note_on(19, 60, 100)][..]
        } else if index == 390 {
            &[Event::note_off(29, 60)][..]
        } else {
            &[]
        };
        a.fill(0.0);
        b.fill(0.0);
        reference.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut a, 2, &mut [], 0, 256),
            events,
        );
        worker.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut b, 2, &mut [], 0, 256),
            events,
        );
        assert!(!worker.has_failed(), "worker failed on block {index}");
        assert_eq!(a, b, "worker sample mismatch on block {index}");
        context.sample_pos += 256;
    }
    let snapshot = worker.pipeline_monitor().streaming_telemetry().unwrap();
    assert!(snapshot.pages_read > 0);
    assert!(snapshot.max_read_ns > 0);
    assert!(snapshot.read_total_ns >= snapshot.max_read_ns);
    assert_eq!(snapshot.underrun_frames, 0);
    assert_eq!(snapshot.underrun_blocks, 0);
    assert_eq!(worker.pipeline_monitor().underruns(), 0);
    worker.reset();
    let reset = worker.pipeline_monitor().streaming_telemetry().unwrap();
    assert_eq!(reset.active_voices, 0);
    assert!(reset.pages_read >= snapshot.pages_read);
    assert_eq!(reset.resident_bytes, snapshot.resident_bytes);
}
