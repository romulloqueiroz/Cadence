use plugin_api::{
    wire::WireEvent, AudioBuffer, Capabilities, Descriptor, Event, EventKind, ParamFlags,
    ParameterInfo, Plugin, PluginKind, ProcessContext, Unit,
};
use plugin_sandbox::{
    automation::{render, RAMP_KIND},
    metadata::PluginMetadata,
    shared::{MAX_BLOCK, MAX_EVENTS},
};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
};

struct Allocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for Allocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        TRACK.with(|track| {
            if track.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.dealloc(pointer, layout) }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCATIONS.with(|n| n.set(n.get() + 1));
            }
        });
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: Allocator = Allocator;

struct Probe {
    values: [f32; 17],
    calls: usize,
    call_frames: Vec<usize>,
    positions: Vec<i64>,
    ppq: Vec<f64>,
    context_blocks: Vec<usize>,
    notes: [f32; MAX_EVENTS],
    note_count: usize,
    event_count: usize,
    output_nan: bool,
}
impl Probe {
    fn new() -> Self {
        Self {
            values: [0.0; 17],
            calls: 0,
            call_frames: vec![0; MAX_BLOCK],
            positions: vec![0; MAX_BLOCK],
            ppq: vec![0.0; MAX_BLOCK],
            context_blocks: vec![0; MAX_BLOCK],
            notes: [0.0; MAX_EVENTS],
            note_count: 0,
            event_count: 0,
            output_nan: false,
        }
    }
}
impl Plugin for Probe {
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "test.ramp",
            name: "Ramp probe",
            vendor: "Cadence",
            version: "1",
            kind: PluginKind::Effect,
        }
    }
    fn capabilities(&self) -> Capabilities {
        Capabilities {
            inputs: 8,
            outputs: 8,
            sidechain_inputs: 8,
            latency_samples: 0,
            tail_samples: 0,
        }
    }
    fn parameters(&self) -> Vec<ParameterInfo> {
        (1..=16)
            .map(|id| ParameterInfo {
                id,
                name: "Value",
                min: 0.0,
                max: 1.0,
                default: 0.0,
                unit: Unit::None,
                flags: ParamFlags {
                    stepped: false,
                    read_only: false,
                    bypass: false,
                },
            })
            .collect()
    }
    fn prepare(&mut self, _: &ProcessContext) {}
    fn reset(&mut self) {}
    fn process(&mut self, context: &ProcessContext, audio: &mut AudioBuffer<'_>, events: &[Event]) {
        self.call_frames[self.calls] = audio.frames;
        self.positions[self.calls] = context.sample_pos;
        self.ppq[self.calls] = context.ppq_position;
        self.context_blocks[self.calls] = context.block_size;
        self.calls += 1;
        for event in events {
            self.event_count += 1;
            match event.kind {
                EventKind::Parameter => self.values[event.data1 as usize] = event.value,
                EventKind::NoteOn => {
                    self.notes[self.note_count] = self.values[1];
                    self.note_count += 1;
                }
                _ => {}
            }
        }
        for channel in 0..audio.output_channels {
            for frame in 0..audio.frames {
                let input = if channel < audio.input_channels {
                    audio.input(channel)[frame]
                } else {
                    1.0
                };
                let side = if audio.sidechain_channels > 0 {
                    audio.sidechain(channel % audio.sidechain_channels)[frame]
                } else {
                    0.0
                };
                let gain = if channel == 1 {
                    self.values[2]
                } else {
                    self.values[1]
                };
                audio.output_mut(channel)[frame] = if self.output_nan {
                    f32::NAN
                } else {
                    input * gain + side * self.values[3]
                };
            }
        }
    }
}
fn parameter(frame: usize, id: u32, value: f32) -> WireEvent {
    Event::parameter(frame, id, value).into()
}
fn ramp(frame: usize, id: u32, from: f32, to: f32, duration: u16) -> [WireEvent; 2] {
    let start = parameter(frame, id, from);
    let mut end = parameter(frame, id, to);
    end.kind = RAMP_KIND;
    end.data2 = duration;
    [start, end]
}
fn apply(
    probe: &mut Probe,
    context: &ProcessContext,
    frames: usize,
    wires: &[WireEvent],
) -> (bool, Vec<f32>, usize) {
    let metadata = PluginMetadata::from_plugin(probe);
    let mut output = vec![0.0; frames];
    let mut events = [Event::note_on(0, 0, 0); MAX_EVENTS];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 1, &mut [], 0, frames);
    ALLOCATIONS.with(|n| n.set(0));
    TRACK.with(|track| track.set(true));
    let ok = render(
        probe,
        context,
        &mut audio,
        wires,
        &mut events,
        &metadata.parameters,
    );
    TRACK.with(|track| track.set(false));
    (ok, output, ALLOCATIONS.with(Cell::get))
}
fn assert_values(actual: &[f32], expected: &[f32]) {
    assert_eq!(actual.len(), expected.len());
    for (frame, (a, b)) in actual.iter().zip(expected).enumerate() {
        assert!((a - b).abs() < 1e-6, "frame{frame}: {a} != {b}");
    }
}

#[test]
fn linear_ramp_has_exact_start_interior_and_endpoint_samples_without_allocating() {
    let mut probe = Probe::new();
    let context = ProcessContext {
        block_size: 9,
        ..Default::default()
    };
    let (ok, out, allocations) = apply(&mut probe, &context, 9, &ramp(2, 1, 0.25, 1.0, 4));
    assert!(ok);
    assert_eq!(allocations, 0);
    assert_values(
        &out,
        &[0.0, 0.0, 0.25, 0.4375, 0.625, 0.8125, 1.0, 1.0, 1.0],
    );
    assert_eq!(probe.calls, 9);
    assert!(probe.call_frames[..9].iter().all(|&n| n == 1));
    assert!(probe.context_blocks[..9].iter().all(|&n| n == 1));
}
#[test]
fn simultaneous_ramps_copy_all_planar_channels_and_sidechain_at_original_stride() {
    let mut probe = Probe::new();
    let frames = 7;
    let mut inputs: Vec<_> = (0..8)
        .flat_map(|c| (0..frames).map(move |f| (100 * c + f + 1) as f32))
        .collect();
    let mut sidechain: Vec<_> = (0..8)
        .flat_map(|c| (0..frames).map(move |f| (10 * c + f + 1) as f32))
        .collect();
    let original_inputs = inputs.clone();
    let original_sidechain = sidechain.clone();
    let mut output = vec![0.0; 8 * frames];
    let mut wires = Vec::new();
    wires.extend(ramp(0, 1, 0.0, 1.0, 4));
    wires.extend(ramp(1, 2, 1.0, 0.0, 2));
    wires.push(parameter(0, 3, 0.5));
    let metadata = PluginMetadata::from_plugin(&probe);
    let context = ProcessContext {
        block_size: frames,
        ..Default::default()
    };
    let mut audio = AudioBuffer::new(&mut inputs, 8, &mut output, 8, &mut sidechain, 8, frames);
    let mut events = [Event::note_on(0, 0, 0); MAX_EVENTS];
    assert!(render(
        &mut probe,
        &context,
        &mut audio,
        &wires,
        &mut events,
        &metadata.parameters
    ));
    for channel in 0..8 {
        for frame in 0..frames {
            let gain = if channel == 1 {
                match frame {
                    0 => 0.0,
                    1 => 1.0,
                    2 => 0.5,
                    _ => 0.0,
                }
            } else {
                (frame as f32 / 4.0).min(1.0)
            };
            let expected = original_inputs[channel * frames + frame] * gain
                + original_sidechain[channel * frames + frame] * 0.5;
            assert!(
                (output[channel * frames + frame] - expected).abs() < 1e-5,
                "channel{channel} frame{frame}"
            );
        }
    }
    assert_eq!(inputs, original_inputs);
    assert_eq!(sidechain, original_sidechain);
}
#[test]
fn parameter_updates_precede_same_sample_notes_and_explicit_override_stops_ramp() {
    let mut probe = Probe::new();
    // Deliberately append automation after independently ordered MIDI.
    let mut wires = vec![
        Event::note_on(2, 60, 100).into(),
        Event::note_on(4, 60, 100).into(),
    ];
    wires.extend(ramp(0, 1, 0.0, 1.0, 8));
    wires.push(parameter(4, 1, 0.9));
    let (ok, out, _) = apply(&mut probe, &ProcessContext::default(), 7, &wires);
    assert!(ok);
    assert_values(&probe.notes[..2], &[0.25, 0.9]);
    assert_values(&out, &[0.0, 0.125, 0.25, 0.375, 0.9, 0.9, 0.9]);
}
#[test]
fn signed_preroll_and_ppq_advance_per_sample() {
    let mut probe = Probe::new();
    let context = ProcessContext {
        sample_pos: -3,
        ppq_position: -0.000125,
        sample_rate: 48000.0,
        tempo: 120.0,
        block_size: 7,
        playing: true,
    };
    let (ok, _, _) = apply(&mut probe, &context, 7, &ramp(0, 1, 0.0, 1.0, 6));
    assert!(ok);
    assert_eq!(&probe.positions[..7], &[-3, -2, -1, 0, 1, 2, 3]);
    for frame in 0..7 {
        assert!((probe.ppq[frame] - (context.ppq_position + frame as f64 / 24000.0)).abs() < 1e-15);
    }
}
#[test]
fn boundary_endpoint_and_clipped_continuations_are_supplied_by_next_packet() {
    let mut probe = Probe::new();
    let context = ProcessContext::default();
    let (ok, a, _) = apply(&mut probe, &context, 4, &ramp(0, 1, 0.0, 1.0, 8));
    assert!(ok);
    assert_values(&a, &[0.0, 0.125, 0.25, 0.375]);
    let (ok, b, _) = apply(&mut probe, &context, 4, &ramp(0, 1, 0.5, 1.0, 4));
    assert!(ok);
    assert_values(&b, &[0.5, 0.625, 0.75, 0.875]);
    let (ok, c, _) = apply(&mut probe, &context, 2, &[parameter(0, 1, 1.0)]);
    assert!(ok);
    assert_values(&c, &[1.0, 1.0]);
}
#[test]
fn normal_packet_stays_one_call_with_original_context_filtering_and_event_limit() {
    let mut probe = Probe::new();
    let context = ProcessContext {
        sample_pos: -128,
        block_size: 64,
        ..Default::default()
    };
    let mut wires = vec![parameter(0, 1, 0.25); MAX_EVENTS + 3];
    wires[0].channel = 16;
    wires[1].kind = 99;
    wires[2].value = f32::NAN;
    wires[3].frame = 8;
    let (ok, out, allocations) = apply(&mut probe, &context, 8, &wires);
    assert!(ok);
    assert_eq!(allocations, 0);
    assert_eq!(probe.calls, 1);
    assert_eq!(probe.call_frames[0], 8);
    assert_eq!(probe.context_blocks[0], 64);
    assert_eq!(probe.positions[0], -128);
    assert_eq!(probe.event_count, MAX_EVENTS - 4);
    assert_values(&out, &[0.25; 8]);
    probe.output_nan = true;
    let (_, out, _) = apply(&mut probe, &context, 8, &[]);
    assert_eq!(out, vec![0.0; 8]);
}
#[test]
fn max_events_and_eight_active_ramps_fit_fixed_storage() {
    let mut probe = Probe::new();
    let mut wires = Vec::new();
    for id in 1..=8 {
        wires.extend(ramp(0, id, 0.0, 1.0, 1));
    }
    for _ in wires.len()..MAX_EVENTS {
        wires.push(Event::control_change(1, 1, 64).into());
    }
    let (ok, _, allocations) = apply(&mut probe, &ProcessContext::default(), 2, &wires);
    assert!(ok);
    assert_eq!(allocations, 0);
    assert_eq!(probe.event_count, 8 + 8 + MAX_EVENTS - 16);
    let mut sequential = Vec::new();
    for id in 1..=9 {
        sequential.extend(ramp((id as usize - 1) * 2, id, 0.0, 1.0, 1));
    }
    let mut probe = Probe::new();
    assert!(apply(&mut probe, &ProcessContext::default(), 20, &sequential).0);
}
#[test]
fn malformed_ramps_fail_before_dsp_or_output_mutation() {
    let valid = ramp(0, 1, 0.0, 1.0, 4).to_vec();
    let mut cases = Vec::new();
    cases.push(vec![valid[1]]);
    for (which, field, value) in [
        (0, 0, 1),
        (1, 0, 1),
        (1, 1, 2),
        (1, 2, 1),
        (1, 3, 0),
        (1, 3, 4097),
        (0, 2, 8),
        (1, 2, 8),
        (1, 1, 200),
    ] {
        let mut wires = valid.clone();
        match field {
            0 => wires[which].channel = value as u16,
            1 => wires[which].data1 = value as u16,
            2 => wires[which].frame = value,
            3 => wires[which].data2 = value as u16,
            _ => unreachable!(),
        };
        cases.push(wires);
    }
    for value in [f32::NAN, f32::INFINITY, -0.1, 1.1] {
        for index in 0..2 {
            let mut wires = valid.clone();
            wires[index].value = value;
            cases.push(wires);
        }
    }
    let mut wrong_kind = valid.clone();
    wrong_kind[0].kind = 0;
    cases.push(wrong_kind);
    let mut unknown = valid.clone();
    let mut extra = parameter(1, 2, 0.0);
    extra.kind = 99;
    unknown.push(extra);
    cases.push(unknown);
    let mut nine = Vec::new();
    for id in 1..=9 {
        nine.extend(ramp(0, id, 0.0, 1.0, 4));
    }
    cases.push(nine);
    let mut too_many = valid.clone();
    too_many.extend(vec![parameter(0, 1, 0.0); MAX_EVENTS - 1]);
    cases.push(too_many);
    for wires in cases {
        let mut probe = Probe::new();
        let metadata = PluginMetadata::from_plugin(&probe);
        let mut output = vec![0.75; 8];
        let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 1, &mut [], 0, 8);
        let mut events = [Event::note_on(0, 0, 0); MAX_EVENTS];
        assert!(!render(
            &mut probe,
            &ProcessContext::default(),
            &mut audio,
            &wires,
            &mut events,
            &metadata.parameters
        ));
        assert_eq!(probe.calls, 0);
        assert_eq!(output, vec![0.75; 8]);
    }
    let mut probe = Probe::new();
    let mut metadata = PluginMetadata::from_plugin(&probe);
    metadata.parameters[0].read_only = true;
    let mut output = [0.0; 8];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 1, &mut [], 0, 8);
    let mut events = [Event::note_on(0, 0, 0); MAX_EVENTS];
    assert!(!render(
        &mut probe,
        &ProcessContext::default(),
        &mut audio,
        &valid,
        &mut events,
        &metadata.parameters
    ));
    let context = ProcessContext {
        sample_pos: i64::MAX,
        ..Default::default()
    };
    metadata.parameters[0].read_only = false;
    assert!(!render(
        &mut probe,
        &context,
        &mut audio,
        &valid,
        &mut events,
        &metadata.parameters
    ));
    assert_eq!(probe.calls, 0);
}

fn shipping_ramp_inputs(block: usize) -> (Vec<WireEvent>, Vec<Event>) {
    let mut wires = Vec::new();
    let mut reference = Vec::new();
    match block {
        0 | 1 => {
            let (from, to, duration) = if block == 0 {
                (0.25f32, 0.75f32, 256)
            } else {
                (0.75f32, 0.125f32, 128)
            };
            wires.extend(ramp(0, 1, from, to, duration));
            for frame in 0..=usize::from(duration).min(255) {
                let t = frame as f64 / f64::from(duration);
                let value = (f64::from(from) + (f64::from(to) - f64::from(from)) * t) as f32;
                reference.push(Event::parameter(frame, 1, value));
            }
            if block == 0 {
                wires.push(Event::note_on(17, 69, 100).into());
                reference.push(Event::note_on(17, 69, 100));
            }
        }
        2 => {
            wires.push(parameter(0, 1, 0.5));
            wires.push(Event::note_off(117, 69).into());
            reference.push(Event::parameter(0, 1, 0.5));
            reference.push(Event::note_off(117, 69));
        }
        _ => {}
    }
    (wires, reference)
}

fn shipping_worker_ramp_comparison(offline: bool) {
    use plugin_sandbox::{host::SandboxConfig, SandboxPlugin};
    use std::time::{Duration, Instant};
    let mut worker = SandboxPlugin::spawn_with_config(
        env!("CARGO_BIN_EXE_plugin-worker"),
        "builtin:sine",
        SandboxConfig {
            block_timeout: Duration::from_secs(2),
            ..Default::default()
        },
    )
    .unwrap();
    let mut context = ProcessContext {
        block_size: 256,
        sample_pos: -512,
        ppq_position: -512.0 / 24_000.0,
        playing: true,
        ..Default::default()
    };
    worker.prepare(&context);
    assert!(!worker.has_failed());
    assert_eq!(worker.pipeline_latency(), 256);
    let mut reference = host::demo::SinePlugin::default();
    reference.prepare(&context);
    reference.reset();
    let monitor = worker.pipeline_monitor();
    let mut previous = vec![0.0f32; 512];
    let mut actual = vec![0.0f32; 512];
    let mut expected = vec![0.0f32; 512];
    for block in 0..6 {
        let (wires, events) = shipping_ramp_inputs(block);
        actual.fill(0.0);
        let mut audio = AudioBuffer::new(&mut [], 0, &mut actual, 2, &mut [], 0, 256);
        ALLOCATIONS.with(|count| count.set(0));
        TRACK.with(|track| track.set(true));
        if offline {
            worker.process_offline_delayed_wires(&context, &mut audio, &wires);
        } else {
            worker.process_pipelined_wires(&context, &mut audio, &wires);
        }
        TRACK.with(|track| track.set(false));
        assert_eq!(ALLOCATIONS.with(Cell::get), 0, "host callback allocated");
        assert!(
            !worker.has_failed(),
            "worker failed on block{block}, offline={offline}"
        );
        assert_eq!(
            actual, previous,
            "compact ramp differs from samplewise reference: block{block}, offline={offline}"
        );
        expected.fill(0.0);
        reference.process(
            &context,
            &mut AudioBuffer::new(&mut [], 0, &mut expected, 2, &mut [], 0, 256),
            &events,
        );
        previous.copy_from_slice(&expected);
        if !offline {
            // Observe scheduling outside the callback; the callback itself must
            // remain a bounded nonblocking packet exchange.
            let deadline = Instant::now() + Duration::from_secs(2);
            while monitor.completed() != monitor.published() {
                assert!(!worker.has_failed());
                assert!(
                    Instant::now() < deadline,
                    "worker did not finish ramp packet"
                );
                std::thread::sleep(Duration::from_micros(50));
            }
        }
        context.sample_pos += 256;
        context.ppq_position += 256.0 / 24_000.0;
    }
    assert_eq!(monitor.underruns(), 0);
}

#[test]
fn shipping_worker_pipelined_ramps_match_samplewise_reference_with_one_buffer_delay() {
    shipping_worker_ramp_comparison(false);
}
#[test]
fn shipping_worker_offline_ramps_match_samplewise_reference_with_one_buffer_delay() {
    shipping_worker_ramp_comparison(true);
}
