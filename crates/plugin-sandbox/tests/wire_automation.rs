//! The wire path must retain transport-only ramps without extending SDK events.
use host::demo::SinePlugin;
use plugin_api::{wire::WireEvent, AudioBuffer, Event, Plugin, ProcessContext};
use plugin_sandbox::SandboxPlugin;

#[test]
fn compact_ramp_survives_shipping_worker_transport_with_exact_delay() {
    const FRAMES: usize = 64;
    let mut worker =
        SandboxPlugin::spawn(env!("CARGO_BIN_EXE_plugin-worker"), "builtin:sine").unwrap();
    let mut context = ProcessContext {
        block_size: FRAMES,
        playing: true,
        ..Default::default()
    };
    worker.prepare(&context);
    assert_eq!(worker.pipeline_latency(), FRAMES);

    let mut reference = SinePlugin::default();
    reference.prepare(&context);
    let mut reference_events = vec![Event::note_on(0, 69, 100)];
    for frame in 7..=39 {
        let value = (0.25f64 + 0.5 * (frame - 7) as f64 / 32.0) as f32;
        reference_events.push(Event::parameter(frame, 1, value));
    }
    let mut expected = [0.0; FRAMES * 2];
    reference.process(
        &context,
        &mut AudioBuffer::new(&mut [], 0, &mut expected, 2, &mut [], 0, FRAMES),
        &reference_events,
    );

    let start: WireEvent = Event::parameter(7, 1, 0.25).into();
    let mut ramp = start;
    ramp.kind = 8;
    ramp.data2 = 32;
    ramp.value = 0.75;
    let wires = [Event::note_on(0, 69, 100).into(), start, ramp];
    assert!(
        ramp.to_event().is_none(),
        "the ordinary SDK enum remains unchanged"
    );
    let mut output = [1.0; FRAMES * 2];
    worker.process_offline_delayed_wires(
        &context,
        &mut AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, FRAMES),
        &wires,
    );
    assert!(!worker.has_failed());
    assert!(
        output.iter().all(|value| *value == 0.0),
        "reported delay warms with silence"
    );
    context.sample_pos += FRAMES as i64;
    worker.process_offline_delayed_wires(
        &context,
        &mut AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, FRAMES),
        &[],
    );
    assert!(!worker.has_failed());
    assert_eq!(
        output, expected,
        "all ramp samples and stereo layout survive transport"
    );
    assert_eq!(worker.pipeline_monitor().underruns(), 0);
}
