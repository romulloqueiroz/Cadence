use super::*;
use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    fs::{self, File},
    io::Write,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct TrackedAllocator;
thread_local! {
    static TRACK: Cell<bool> = const { Cell::new(false) };
    static ALLOCS: Cell<usize> = const { Cell::new(0) };
}
unsafe impl GlobalAlloc for TrackedAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        TRACK.with(|track| {
            if track.get() {
                ALLOCS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: TrackedAllocator = TrackedAllocator;
static SERIAL: AtomicUsize = AtomicUsize::new(0);

#[path = "plain_tests.rs"]
mod plain_tests;

struct Fixture {
    directory: PathBuf,
    sample: PathBuf,
}
impl Fixture {
    fn new(frames: usize) -> Self {
        let directory = std::env::temp_dir().join(format!(
            "cadence-stream-{}-{}",
            std::process::id(),
            SERIAL.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&directory).unwrap();
        let sample = directory.join("sample.wav");
        let mut file = File::create(&sample).unwrap();
        file.write_all(b"RIFF").unwrap();
        file.write_all(&(36 + frames as u32 * 4).to_le_bytes())
            .unwrap();
        file.write_all(b"WAVEfmt ").unwrap();
        file.write_all(&16u32.to_le_bytes()).unwrap();
        file.write_all(&1u16.to_le_bytes()).unwrap();
        file.write_all(&2u16.to_le_bytes()).unwrap();
        file.write_all(&48_000u32.to_le_bytes()).unwrap();
        file.write_all(&192_000u32.to_le_bytes()).unwrap();
        file.write_all(&4u16.to_le_bytes()).unwrap();
        file.write_all(&16u16.to_le_bytes()).unwrap();
        file.write_all(b"data").unwrap();
        file.write_all(&(frames as u32 * 4).to_le_bytes()).unwrap();
        let mut data = Vec::with_capacity(frames * 4);
        for frame in 0..frames {
            let left = ((frame as f64 * 0.023).sin() * 16000.0) as i16;
            let right = ((frame as f64 * 0.031).cos() * 12000.0) as i16;
            data.extend_from_slice(&left.to_le_bytes());
            data.extend_from_slice(&right.to_le_bytes());
        }
        file.write_all(&data).unwrap();
        Self { directory, sample }
    }

    fn plugin(&self, reverse: bool, loop_info: Option<(u32, u32, bool)>) -> SamplerPlugin {
        let template = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../nk-format/tests/fixtures/Kontakt680Template.nki");
        let mut plugin = SamplerPlugin::load(template).unwrap();
        plugin.loaded.path = self.directory.join("instrument.nki");
        plugin.sample_files = vec!["sample.wav".into()];
        plugin.program = model::ProgramInfo {
            name: "Streaming fixture".into(),
            midi_transpose: 0,
            volume: 1.0,
            pan: 0.0,
            tune: 1.0,
            key_range: (0, 127),
            velocity_range: (0, 127),
            default_key_switch: None,
            cc64_mode: model::Cc64Mode::default(),
            voice_groups: model::VoiceGroups::default(),
        };
        plugin.groups = vec![model::Group {
            name: "Normal".into(),
            volume: 1.0,
            pan: 0.0,
            tune: 1.0,
            key_tracking: true,
            reverse,
            midi_channel: 0,
            voice_group_index: -1,
            interp_quality: 0,
            release_trigger: false,
            release_trigger_monophonic: false,
            release_trigger_counter: 0,
            muted: false,
            soloed: false,
            one_shot: false,
            name_trigger_heuristics: true,
            filter: None,
            fx_chain: None,
            fx_amp_split_point: 6,
            envelope: None,
            envelope_intensity: 1.0,
            internal_modulators: Vec::new(),
            start_criteria: Vec::new(),
            external_modulators: Vec::new(),
        }];
        plugin.zones = vec![Zone {
            group_index: 0,
            version: 0x9c,
            sample_start: 137,
            sample_end: 19,
            absolute_end: false,
            one_shot: false,
            low_velocity: 0,
            high_velocity: 127,
            low_key: 0,
            high_key: 127,
            fade_low_velocity: 0,
            fade_high_velocity: 0,
            fade_low_key: 0,
            fade_high_key: 0,
            root_key: 60,
            volume: 1.0,
            pan: 0.0,
            tune: 1.0,
            filename_id: Some(0),
            sample_resolution: 16,
            sample_rate: 48000,
            channels: 2,
            num_frames: 0,
            root_note: 60,
            tuning: 1.0,
            sample_size: 0,
            loops: loop_info
                .into_iter()
                .map(|(start, length, alternating)| nk_format::zone::Loop {
                    mode: 1,
                    start,
                    length,
                    count: 0,
                    alternating,
                    tuning: 1.0,
                    crossfade_length: 0,
                })
                .collect(),
        }];
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        plugin.internal_mod = InternalMod::new(&plugin.groups);
        plugin.rebuild_voice_policies();
        plugin.rebuild_group_fx().unwrap();
        plugin.zone_samples = vec![None];
        plugin
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.directory);
    }
}

fn block(
    plugin: &mut SamplerPlugin,
    context: &ProcessContext,
    output: &mut [f32],
    events: &[Event],
) {
    output.fill(0.0);
    let mut audio = AudioBuffer::new(&mut [], 0, output, 2, &mut [], 0, context.block_size);
    plugin.process(context, &mut audio, events);
}

#[test]
fn note_lifetimes_saturation_pedal_release_and_reset_do_not_allocate() {
    let fixture = Fixture::new(10_000);
    let mut plugin = fixture.plugin(false, None);
    plugin.preload_all();
    let context = ProcessContext {
        block_size: 64,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut events = vec![Event::note_on(0, 60, 100); NOTE_POOL + 8];
    events.push(Event::control_change(0, 64, 127));
    events.push(Event::note_off(1, 60));
    let mut output = [0.0; 128];

    ALLOCS.with(|v| v.set(0));
    TRACK.with(|v| v.set(true));
    block(&mut plugin, &context, &mut output, &events);
    TRACK.with(|v| v.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert_eq!(plugin.notes.iter().flatten().count(), NOTE_POOL);
    assert!(plugin.notes.iter().flatten().all(|note| note.sustained));
    assert_eq!(
        plugin.notes.iter().flatten().map(|note| note.id).min(),
        Some(8)
    );
    assert!(output.iter().all(|value| value.is_finite()));

    ALLOCS.with(|v| v.set(0));
    TRACK.with(|v| v.set(true));
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::control_change(0, 64, 0)],
    );
    plugin.reset();
    TRACK.with(|v| v.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    assert!(plugin.notes.iter().all(Option::is_none));
    assert!(plugin.voices.iter().all(Option::is_none));
    assert!(plugin.sustain_down.iter().all(|down| !down));
    assert_eq!(plugin.next_note_id, 0);
}

#[test]
fn expression_routes_tables_smoothing_and_voice_reuse_do_not_allocate() {
    use nk_format::external_mod::{ExternalModulator, Target, Transformation};
    let fixture = Fixture::new(40_000);
    for streamed in [false, true] {
        let mut plugin = fixture.plugin(false, Some((200, 10_000, false)));
        plugin.groups[0].external_modulators = vec![
            ExternalModulator {
                source: 1,
                targets: vec![Target {
                    name: "pitch".into(),
                    intensity: 0.5,
                    flags: 20,
                    smoothing: 7,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ExternalModulator {
                source: 4,
                cc_number: 11,
                initial_value: -1,
                targets: vec![Target {
                    name: "volume".into(),
                    intensity: 1.0,
                    flags: 20,
                    smoothing: -10,
                    transform: Transformation::Table {
                        active: true,
                        values: Box::new(std::array::from_fn(|i| i as f32 / 127.0)),
                    },
                    ..Default::default()
                }],
                ..Default::default()
            },
            ExternalModulator {
                source: 3,
                targets: vec![Target {
                    name: "pan".into(),
                    intensity: 0.5,
                    flags: 20,
                    ..Default::default()
                }],
                ..Default::default()
            },
            ExternalModulator {
                source: 2,
                targets: vec![Target {
                    name: "volume".into(),
                    intensity: 0.25,
                    flags: 20,
                    ..Default::default()
                }],
                ..Default::default()
            },
        ];
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        if streamed {
            plugin.stream_all(StreamingConfig::default()).unwrap();
        } else {
            plugin.preload_all();
        }
        let context = ProcessContext {
            block_size: 128,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut attacks = vec![Event::note_on(0, 60, 100); VOICE_POOL];
        for (index, event) in attacks.iter_mut().enumerate() {
            event.channel = (index % 16) as u8;
        }
        let changes = [
            Event::pitch_bend(5, 0.5),
            Event::control_change(17, 11, 64),
            Event {
                frame: 19,
                kind: EventKind::ChannelPressure,
                channel: 0,
                data1: 64,
                data2: 0,
                value: 0.5,
            },
            Event {
                frame: 25,
                kind: EventKind::PolyPressure,
                channel: 0,
                data1: 60,
                data2: 64,
                value: 0.5,
            },
            Event::pitch_bend(50, -0.5),
        ];
        let mut output = [0.0; 256];
        ALLOCS.with(|v| v.set(0));
        TRACK.with(|v| v.set(true));
        block(&mut plugin, &context, &mut output, &attacks);
        for _ in 0..16 {
            block(&mut plugin, &context, &mut output, &changes);
        }
        plugin.reset();
        block(&mut plugin, &context, &mut output, &attacks);
        TRACK.with(|v| v.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0, "streamed={streamed}");
        assert!(output.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn pressure_broadcast_flag_selects_per_key_or_group_audio() {
    use nk_format::external_mod::{ExternalModulator, Target};
    let fixture = Fixture::new(10_000);
    for broadcast in [false, true] {
        let mut plugin = fixture.plugin(false, None);
        plugin.groups[0].key_tracking = false;
        plugin.groups[0].external_modulators = vec![ExternalModulator {
            source: 2,
            flag_v103: broadcast,
            targets: vec![Target {
                name: "volume".into(),
                intensity: 1.0,
                flags: 20,
                ..Default::default()
            }],
            ..Default::default()
        }];
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        plugin.preload_all();
        let context = ProcessContext {
            block_size: 64,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut second = Event::note_on(0, 61, 100);
        second.channel = 1;
        let events = [
            Event::note_on(0, 60, 100),
            second,
            Event {
                frame: 10,
                kind: EventKind::PolyPressure,
                channel: 0,
                data1: 60,
                data2: 127,
                value: 1.0,
            },
        ];
        let mut output = [0.0; 128];
        block(&mut plugin, &context, &mut output, &events);
        assert!(output[..10].iter().all(|value| *value == 0.0));
        let reference = plugin.samples[0].resident_frame(137 + 50).unwrap();
        let gain = if broadcast { 2.0 } else { 1.0 };
        assert!(
            (output[50] - reference.0 * gain).abs() < 1e-6,
            "broadcast={broadcast}"
        );
        assert!(
            (output[64 + 50] - reference.1 * gain).abs() < 1e-6,
            "broadcast={broadcast}"
        );
    }
}

#[test]
fn offline_pages_match_full_ram_with_pitch_reverse_and_loops() {
    let fixture = Fixture::new(140_000);
    for (reverse, loop_info) in [
        (false, None),
        (true, None),
        (false, Some((31_337, 73_123, false))),
        (false, Some((31_337, 73_123, true))),
    ] {
        let mut resident = fixture.plugin(reverse, loop_info);
        assert_eq!(resident.preload_all(), 1);
        let mut streamed = fixture.plugin(reverse, loop_info);
        let initial = streamed
            .stream_all(StreamingConfig {
                preload_frames: 8192,
                ..Default::default()
            })
            .unwrap();
        assert_eq!(initial.page_cache_bytes, 16 * 1024 * 1024);
        streamed.set_offline_rendering(true);
        let mut context = ProcessContext {
            sample_rate: 44_100.0,
            block_size: 257,
            ..Default::default()
        };
        resident.prepare(&context);
        streamed.prepare(&context);
        let mut a = vec![0.0; context.block_size * 2];
        let mut b = a.clone();
        for index in 0..750 {
            let events = if index == 0 {
                &[Event::note_on(17, 67, 100)][..]
            } else if index == 700 {
                &[Event::note_off(31, 67)][..]
            } else {
                &[]
            };
            block(&mut resident, &context, &mut a, events);
            block(&mut streamed, &context, &mut b, events);
            assert_eq!(a, b, "reverse={reverse}, loop={loop_info:?}, block={index}");
            context.sample_pos += context.block_size as i64;
        }
        let stats = streamed.streaming_stats();
        assert!(stats.pages_read > 0, "{stats:?}");
        assert_eq!(stats.read_errors, 0, "{stats:?}");
        assert_eq!(stats.underrun_frames, 0, "{stats:?}");
        assert_eq!(stats.resident_bytes, initial.resident_bytes);
        assert_eq!(stats.page_cache_bytes, initial.page_cache_bytes);
    }
}

#[test]
fn callback_has_no_allocations_with_stalled_disk_or_voice_stealing() {
    let fixture = Fixture::new(200_000);
    let mut plugin = fixture.plugin(false, None);
    let initial = plugin.stream_all(StreamingConfig::default()).unwrap();
    plugin
        .streaming
        .as_ref()
        .unwrap()
        .set_read_delay(Duration::from_millis(300));
    let context = ProcessContext {
        block_size: 128,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; 256];
    let events = [Event::note_on(0, 60, 100); 80];
    ALLOCS.with(|v| v.set(0));
    TRACK.with(|v| v.set(true));
    block(&mut plugin, &context, &mut output, &events);
    TRACK.with(|v| v.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0, "note on/stealing allocated");
    // Jump a live voice to a nonresident source region while disk is stalled.
    plugin.voices.iter_mut().flatten().for_each(|v| {
        v.position = 100_000.0;
        v.stream_page = usize::MAX;
    });
    let before = Instant::now();
    TRACK.with(|v| v.set(true));
    block(&mut plugin, &context, &mut output, &[]);
    let telemetry = plugin.streaming_telemetry().unwrap();
    plugin.reset();
    TRACK.with(|v| v.set(false));
    assert!(
        before.elapsed() < Duration::from_millis(150),
        "callback waited for disk"
    );
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "streaming/reset allocated or freed storage"
    );
    assert!(output.iter().all(|x| x.is_finite()));
    let stats = plugin.streaming_stats();
    assert!(stats.underrun_frames > 0);
    assert_eq!(telemetry.underrun_blocks, 1);
    assert_eq!(telemetry.underrun_frames, 64 * context.block_size as u64);
    assert_eq!(telemetry.active_voices, 64);
    assert_eq!(plugin.streaming_telemetry().unwrap().active_voices, 0);
    assert_eq!(stats.page_cache_bytes, initial.page_cache_bytes);
    assert_eq!(stats.resident_bytes, initial.resident_bytes);
    assert!(plugin.processing_error().is_none());
    let before = Instant::now();
    drop(plugin);
    assert!(
        before.elapsed() < Duration::from_millis(150),
        "shutdown failed to cancel artificial disk stall"
    );
}

#[test]
fn offline_voice_reuse_and_reset_cannot_publish_stale_samples() {
    let fixture = Fixture::new(100_000);
    let mut a = fixture.plugin(false, None);
    let mut b = fixture.plugin(false, None);
    let mut other = fs::read(&fixture.sample).unwrap();
    for bytes in other[44..].chunks_exact_mut(2) {
        let value = i16::from_le_bytes(bytes.try_into().unwrap());
        bytes.copy_from_slice(&(-value).to_le_bytes());
    }
    fs::write(fixture.directory.join("other.wav"), other).unwrap();
    for plugin in [&mut a, &mut b] {
        plugin.zones[0].low_key = 60;
        plugin.zones[0].high_key = 60;
        let mut other_zone = plugin.zones[0].clone();
        other_zone.low_key = 72;
        other_zone.high_key = 72;
        other_zone.filename_id = Some(1);
        plugin.zones.push(other_zone);
        plugin.sample_files.push("other.wav".into());
        plugin.zone_samples = vec![None; 2];
    }
    a.preload_all();
    b.stream_all(StreamingConfig::default()).unwrap();
    b.set_offline_rendering(true);
    let context = ProcessContext {
        block_size: 1024,
        ..Default::default()
    };
    a.prepare(&context);
    b.prepare(&context);
    let mut resident = vec![0.0; 2048];
    let mut streamed = resident.clone();
    let chord: Vec<_> = (0..80)
        .map(|i| Event::note_on(i % 32, if i % 2 == 0 { 60 } else { 72 }, 100))
        .collect();
    for index in 0..95 {
        if index == 60 {
            a.reset();
            b.reset();
        }
        let events = if index % 40 == 0 { &chord[..] } else { &[] };
        block(&mut a, &context, &mut resident, events);
        block(&mut b, &context, &mut streamed, events);
        assert_eq!(resident, streamed, "block {index}");
    }
    assert_eq!(b.streaming_stats().underrun_frames, 0);
    assert!(b.streaming_stats().pages_read > 0);
}

#[test]
fn preload_budget_and_read_failures_are_explicit() {
    let fixture = Fixture::new(140_000);
    let mut plugin = fixture.plugin(false, None);
    assert!(plugin
        .stream_all(StreamingConfig {
            max_preload_bytes: 1,
            ..Default::default()
        })
        .is_err());
    assert_eq!(plugin.streaming_stats().resident_bytes, 0);
    plugin.stream_all(StreamingConfig::default()).unwrap();
    File::options()
        .write(true)
        .open(&fixture.sample)
        .unwrap()
        .set_len(44 + 25_000 * 4)
        .unwrap();
    plugin.set_offline_rendering(true);
    let context = ProcessContext {
        block_size: 1024,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; 2048];
    for i in 0..35 {
        let events = if i == 0 {
            &[Event::note_on(0, 60, 100)][..]
        } else {
            &[]
        };
        block(&mut plugin, &context, &mut output, events);
    }
    assert!(plugin.streaming_stats().read_errors > 0);
    assert!(plugin.processing_error().is_some());
    assert!(output.iter().all(|x| x.is_finite()));
}

#[test]
fn pingpong_loop_including_last_sample_never_leaves_valid_frames() {
    let fixture = Fixture::new(64);
    let mut plugin = fixture.plugin(false, Some((60, 4, true)));
    plugin.zones[0].sample_start = 60;
    plugin.zones[0].sample_end = 0;
    plugin.preload_all();
    let context = ProcessContext {
        block_size: 16,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; 32];
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 100)],
    );
    let expected = [
        60, 61, 62, 63, 62, 61, 60, 61, 62, 63, 62, 61, 60, 61, 62, 63,
    ];
    for (i, frame) in expected.into_iter().enumerate() {
        assert_eq!(
            output[i],
            plugin.samples[0].resident_frame(frame).unwrap().0
        );
    }
    assert!(plugin.voices[0].is_some());
    // Pitch jumps larger than the complete loop remain within its sample range.
    plugin.voices[0].as_mut().unwrap().speed = 100.25;
    block(&mut plugin, &context, &mut output, &[]);
    let voice = plugin.voices[0].as_ref().unwrap();
    assert!((60.0..=63.0).contains(&voice.position));
}

#[test]
fn telemetry_reports_page_service_cache_voices_and_lifetime_counters() {
    let fixture = Fixture::new(200_000);
    let mut plugin = fixture.plugin(false, None);
    assert!(plugin.streaming_telemetry().is_none());
    let config = StreamingConfig::default();
    plugin.stream_all(config).unwrap();
    let before = plugin.streaming_telemetry().unwrap();
    assert_eq!(before.flags, 1);
    assert_eq!(before.active_voices, 0);
    assert_eq!(before.voice_capacity, 64);
    assert_eq!(before.pages_read, 0);
    assert_eq!(before.cache_ready_bytes, 0);
    assert!(before.resident_bytes > 0);
    assert_eq!(before.cache_capacity_bytes, 16 * 1024 * 1024);
    plugin
        .streaming
        .as_ref()
        .unwrap()
        .set_read_delay(Duration::from_millis(20));
    plugin.set_offline_rendering(true);
    let context = ProcessContext {
        block_size: 128,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; 256];
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 100)],
    );
    plugin.voices[0].as_mut().unwrap().position = 100_000.0;
    plugin.voices[0].as_mut().unwrap().stream_page = usize::MAX;
    block(&mut plugin, &context, &mut output, &[]);
    let deadline = Instant::now() + Duration::from_secs(1);
    while plugin.streaming_stats().pending_reads != 0 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    let after = plugin.streaming_telemetry().unwrap();
    assert_eq!(after.pending_reads, 0);
    assert_eq!(after.active_voices, 1);
    assert!(after.pages_read > 0);
    assert_eq!(after.read_count, after.pages_read);
    assert_eq!(after.read_errors, 0);
    assert_eq!(after.underrun_frames, 0);
    assert!(after.last_read_ns >= 20_000_000);
    assert!(after.max_read_ns >= after.last_read_ns);
    assert!(after.read_total_ns >= after.max_read_ns);
    assert!(after.cache_ready_bytes > 0);
    assert!(after.peak_cache_ready_bytes >= after.cache_ready_bytes);
    assert!(after.peak_cache_ready_bytes <= after.cache_capacity_bytes);
    plugin.reset();
    let reset = plugin.streaming_telemetry().unwrap();
    assert_eq!(reset.active_voices, 0);
    assert_eq!(reset.pages_read, after.pages_read);
    assert_eq!(reset.max_read_ns, after.max_read_ns);
    assert_eq!(reset.peak_cache_ready_bytes, after.peak_cache_ready_bytes);
}

fn render_seconds(plugin: &mut SamplerPlugin, seconds: f32, events: &[Event]) -> Vec<f32> {
    let sample_rate = 48_000.0f64;
    let total = (sample_rate * seconds as f64) as usize;
    let mut context = ProcessContext {
        sample_rate,
        block_size: 128,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; total * 2];
    for position in (0..total).step_by(128) {
        let count = 128.min(total - position);
        context.sample_pos = position as i64;
        context.block_size = count;
        let mut block_events = Vec::new();
        if position == 0 {
            block_events.extend_from_slice(events);
        }
        block(
            plugin,
            &context,
            &mut output[position * 2..(position + count) * 2],
            &block_events,
        );
    }
    output
}

fn rms(samples: &[f32]) -> f64 {
    (samples
        .iter()
        .map(|value| (value * value) as f64)
        .sum::<f64>()
        / samples.len() as f64)
        .sqrt()
}

#[test]
fn internal_volume_lfo_modulates_gain_over_time() {
    let fixture = Fixture::new(48_000);
    let loop_info = Some((137, 1000, false));
    let mut reference = fixture.plugin(false, loop_info);
    reference.preload_all();
    let mut modulated = fixture.plugin(false, loop_info);
    modulated.groups[0].internal_modulators = vec![nk_format::modulator::InternalModulator {
        parameters: vec![nk_format::modulator::TargetParameter {
            name: "volume".into(),
            intensity: 1.0,
        }],
        source_name: "LFO_SINE".into(),
        source_index: 1,
        envelope: None,
        lfo: Some(nk_format::modulator::Lfo {
            kind: nk_format::modulator::LfoKind::Sine,
            delay: 0.0,
            frequency: 1.0,
            pulse_width: 0.5,
            start_phase: 0.0,
            normalize: false,
        }),
    }];
    modulated.internal_mod = InternalMod::new(&modulated.groups);
    modulated.preload_all();

    let events = [Event::note_on(0, 60, 100)];
    let reference = render_seconds(&mut reference, 1.0, &events);
    let modulated = render_seconds(&mut modulated, 1.0, &events);
    let left = |audio: &[f32]| audio.iter().step_by(2).copied().collect::<Vec<_>>();
    let reference = left(&reference);
    let modulated = left(&modulated);
    for (center_s, expected) in [(0.25, 1.0), (0.5, 0.5), (0.75, 0.0)] {
        let start = (center_s * 48_000.0) as usize - 600;
        let window = start..start + 1200;
        let ratio = rms(&modulated[window.clone()]) / rms(&reference[window]);
        assert!(
            (ratio - expected).abs() < 0.08,
            "LFO phase at {center_s}s: ratio {ratio}, expected {expected}"
        );
    }
}

fn voice_limit_group(max_voices: u32, kill_mode: model::KillMode) -> model::VoiceGroup {
    model::VoiceGroup {
        name: "limit".into(),
        kill_mode,
        prefer_released: false,
        max_voices,
        fade_time_ms: 0,
        exclusion_group: -1,
    }
}

fn voice_notes(plugin: &SamplerPlugin) -> Vec<u8> {
    let mut notes: Vec<_> = plugin
        .voices
        .iter()
        .flatten()
        .map(|voice| voice.note)
        .collect();
    notes.sort();
    notes
}

#[test]
fn voice_group_limit_kills_the_oldest_voice() {
    let fixture = Fixture::new(48_000);
    let mut plugin = fixture.plugin(false, Some((137, 1000, false)));
    plugin.program.voice_groups.default = voice_limit_group(2, model::KillMode::Oldest);
    plugin.rebuild_voice_policies();
    plugin.preload_all();
    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    let events: Vec<_> = [(0, 60), (100, 62), (200, 64)]
        .into_iter()
        .map(|(frame, note)| {
            let mut event = Event::note_on(frame, note, 100);
            event.frame = frame;
            event
        })
        .collect();
    let mut output = vec![0.0; 512];
    block(&mut plugin, &context, &mut output, &events);
    assert_eq!(voice_notes(&plugin), vec![62, 64]);
}

#[test]
fn voice_group_limit_kills_per_mode() {
    for (mode, expected) in [
        (model::KillMode::Highest, vec![60, 62]),
        (model::KillMode::Lowest, vec![62, 64]),
        (model::KillMode::Newest, vec![60, 62]),
    ] {
        let fixture = Fixture::new(48_000);
        let mut plugin = fixture.plugin(false, Some((137, 1000, false)));
        plugin.program.voice_groups.default = voice_limit_group(2, mode);
        plugin.rebuild_voice_policies();
        plugin.preload_all();
        let context = ProcessContext {
            sample_rate: 48_000.0,
            block_size: 256,
            ..Default::default()
        };
        let events: Vec<_> = [(0, 60), (100, 64), (200, 62)]
            .into_iter()
            .map(|(frame, note)| Event::note_on(frame, note, 100))
            .collect();
        let mut output = vec![0.0; 512];
        block(&mut plugin, &context, &mut output, &events);
        assert_eq!(voice_notes(&plugin), expected, "{mode:?}");
    }
}

#[test]
fn exclusion_group_chokes_a_different_voice_group() {
    let fixture = Fixture::new(48_000);
    let mut plugin = fixture.plugin(false, Some((137, 1000, false)));
    let mut second_group = plugin.groups[0].clone();
    second_group.name = "Second".into();
    second_group.voice_group_index = 1;
    plugin.groups.push(second_group);
    let mut second_zone = plugin.zones[0].clone();
    second_zone.group_index = 1;
    second_zone.low_key = 62;
    second_zone.high_key = 62;
    plugin.zones.push(second_zone);
    plugin.zone_samples = vec![None; plugin.zones.len()];
    let exclusion = |index: i32| model::VoiceGroup {
        name: format!("group {index}"),
        kill_mode: model::KillMode::Oldest,
        prefer_released: false,
        max_voices: 0,
        fade_time_ms: 0,
        exclusion_group: 0,
    };
    plugin.program.voice_groups.overrides = vec![(0, exclusion(0)), (1, exclusion(1))];
    plugin.groups[0].voice_group_index = 0;
    plugin.rebuild_voice_policies();
    plugin.preload_all();

    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    let events = [Event::note_on(0, 60, 100), Event::note_on(100, 62, 100)];
    let mut output = vec![0.0; 512];
    block(&mut plugin, &context, &mut output, &events);
    assert_eq!(voice_notes(&plugin), vec![62]);
}

#[test]
fn voice_group_choke_fades_for_the_configured_time() {
    let fixture = Fixture::new(48_000);
    let mut plugin = fixture.plugin(false, Some((137, 1000, false)));
    plugin.program.voice_groups.default = model::VoiceGroup {
        name: "fade".into(),
        kill_mode: model::KillMode::Oldest,
        prefer_released: false,
        max_voices: 1,
        fade_time_ms: 10,
        exclusion_group: -1,
    };
    plugin.rebuild_voice_policies();
    plugin.preload_all();
    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    let events = [Event::note_on(0, 60, 100), Event::note_on(100, 62, 100)];
    let mut output = vec![0.0; 512];
    // 10 ms at 48 kHz is 480 frames: both voices remain through 412 frames
    // of fade, then the choked voice is retired.
    block(&mut plugin, &context, &mut output, &events);
    assert_eq!(voice_notes(&plugin), vec![60, 62]);
    block(&mut plugin, &context, &mut output, &[]);
    assert_eq!(voice_notes(&plugin), vec![60, 62]);
    block(&mut plugin, &context, &mut output, &[]);
    assert_eq!(voice_notes(&plugin), vec![62]);
}

fn advance(
    plugin: &mut SamplerPlugin,
    sample_rate: f64,
    frames: usize,
    events: &[Event],
) -> Vec<f32> {
    let block_size = 256;
    let mut output = Vec::with_capacity(frames);
    plugin.prepare(&ProcessContext {
        sample_rate,
        block_size,
        ..Default::default()
    });
    let mut done = 0;
    while done < frames {
        let count = block_size.min(frames - done);
        let context = ProcessContext {
            sample_rate,
            block_size: count,
            sample_pos: done as i64,
            ..Default::default()
        };
        let mut buffer = vec![0.0; count * 2];
        let block_events: Vec<_> = if done == 0 {
            events
                .iter()
                .map(|event| {
                    let mut event = *event;
                    event.frame = event.frame.min(count - 1);
                    event
                })
                .collect()
        } else {
            Vec::new()
        };
        block(plugin, &context, &mut buffer, &block_events);
        // AudioBuffer is channel-major; keep the left channel only.
        output.extend_from_slice(&buffer[..count]);
        done += count;
    }
    output
}

#[test]
fn finite_loop_count_stops_after_configured_wraps() {
    let fixture = Fixture::new(48_000);
    for (count, expected_loop) in [(0u32, true), (1, false)] {
        let mut plugin = fixture.plugin(false, Some((137, 1000, false)));
        plugin.zones[0].loops[0].count = count;
        plugin.preload_all();
        advance(&mut plugin, 48_000.0, 2_400, &[Event::note_on(0, 60, 100)]);
        let looping = plugin
            .voices
            .iter()
            .flatten()
            .all(|voice| voice.loop_state.is_some());
        assert_eq!(looping, expected_loop, "count={count}");
    }
}

#[test]
fn loop_tuning_changes_the_wrap_rate() {
    let fixture = Fixture::new(48_000);
    for (tuning, expected_loop) in [(1.0f32, true), (2.0, false)] {
        let mut plugin = fixture.plugin(false, Some((137, 1000, false)));
        plugin.zones[0].loops[0].count = 1;
        plugin.zones[0].loops[0].tuning = tuning;
        plugin.preload_all();
        advance(&mut plugin, 48_000.0, 1_300, &[Event::note_on(0, 60, 100)]);
        let looping = plugin
            .voices
            .iter()
            .flatten()
            .all(|voice| voice.loop_state.is_some());
        assert_eq!(looping, expected_loop, "tuning={tuning}");
    }
}

#[test]
fn loop_crossfade_smooths_the_seam() {
    let fixture = Fixture::new(48_000);
    let mut deltas = Vec::new();
    for crossfade in [0u32, 400] {
        let mut plugin = fixture.plugin(false, Some((500, 1000, false)));
        plugin.zones[0].sample_start = 0;
        plugin.zones[0].loops[0].crossfade_length = crossfade;
        plugin.preload_all();
        let left = advance(&mut plugin, 48_000.0, 1_700, &[Event::note_on(0, 60, 100)]);
        let seam = 1_500usize;
        deltas.push((left[seam] - left[seam - 1]).abs());
    }
    assert!(
        deltas[1] < deltas[0] * 0.5,
        "crossfaded seam delta {} not below {}",
        deltas[1],
        deltas[0]
    );
}

#[test]
fn interpolation_modes_bound_fractional_playback_error() {
    let fixture = Fixture::new(48_000);
    let speed = 2f64.powf(1.0 / 12.0);
    let ideal = |frame: usize| {
        let position = 137.0 + frame as f64 * speed;
        (position * 0.023).sin() * 16_000.0 / 32_768.0
    };
    let mut errors = Vec::new();
    for quality in [0i32, 1, 2] {
        let mut plugin = fixture.plugin(false, None);
        plugin.groups[0].interp_quality = quality;
        plugin.preload_all();
        let left = advance(&mut plugin, 48_000.0, 2_000, &[Event::note_on(0, 61, 100)]);
        let error = left[200..]
            .iter()
            .enumerate()
            .map(|(index, value)| {
                let expected = ideal(index + 200) as f32;
                (value - expected) * (value - expected)
            })
            .sum::<f32>();
        errors.push(error);
    }
    // The sinusoid is quantized to PCM16 by the fixture. All three kernels
    // should remain close to the analytic source without imposing a quality
    // ranking on the two still-approximate high/ultra modes.
    for (quality, error) in errors.iter().enumerate() {
        assert!(*error < 2e-6, "quality={quality} squared error={error}");
    }
}

#[test]
fn release_loop_replaces_the_sustain_loop_on_note_off() {
    let fixture = Fixture::new(48_000);
    let mut plugin = fixture.plugin(false, None);
    plugin.zones[0].loops = vec![
        nk_format::zone::Loop {
            mode: 1,
            start: 137,
            length: 1_000,
            count: 0,
            alternating: false,
            tuning: 1.0,
            crossfade_length: 0,
        },
        nk_format::zone::Loop {
            mode: 2,
            start: 500,
            length: 200,
            count: 0,
            alternating: false,
            tuning: 1.0,
            crossfade_length: 0,
        },
    ];
    plugin.preload_all();
    advance(&mut plugin, 48_000.0, 200, &[Event::note_on(0, 60, 100)]);
    let before = plugin
        .voices
        .iter()
        .flatten()
        .next()
        .and_then(|voice| voice.loop_state);
    assert_eq!(
        before.map(|state| (state.start, state.end)),
        Some((137, 1_137))
    );

    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 16,
        ..Default::default()
    };
    let mut output = vec![0.0; 32];
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_off(0, 60)],
    );
    let after = plugin
        .voices
        .iter()
        .flatten()
        .next()
        .and_then(|voice| voice.loop_state);
    assert_eq!(
        after.map(|state| (state.start, state.end)),
        Some((500, 700))
    );
}

#[test]
fn looped_and_cubic_playback_match_between_resident_and_streamed() {
    let fixture = Fixture::new(48_000);
    for (note, alternating, quality) in [(60u8, false, 0i32), (84, true, 2)] {
        let mut resident = fixture.plugin(false, Some((500, 1_000, alternating)));
        resident.zones[0].sample_start = 0;
        resident.zones[0].loops[0].crossfade_length = if alternating { 0 } else { 400 };
        resident.groups[0].interp_quality = quality;
        resident.preload_all();
        let mut streamed = fixture.plugin(false, Some((500, 1_000, alternating)));
        streamed.zones[0].sample_start = 0;
        streamed.zones[0].loops[0].crossfade_length = if alternating { 0 } else { 400 };
        streamed.groups[0].interp_quality = quality;
        streamed.stream_all(StreamingConfig::default()).unwrap();
        streamed.set_offline_rendering(true);

        let events = [Event::note_on(0, note, 100)];
        let a = advance(&mut resident, 48_000.0, 2_000, &events);
        let b = advance(&mut streamed, 48_000.0, 2_000, &events);
        assert_eq!(
            a, b,
            "note={note} alternating={alternating} quality={quality}"
        );
    }
}

struct FxSpec {
    ser_type: u16,
    version: u16,
    raw: Vec<u8>,
    bypass: bool,
}

fn fx_chain(specs: &[FxSpec]) -> Vec<u8> {
    let mut array = Vec::new();
    array.extend_from_slice(&0x13u16.to_le_bytes());
    array.extend_from_slice(&(specs.len() as u32).to_le_bytes());
    for spec in specs {
        array.push(1);
        let mut child = Vec::new();
        child.extend_from_slice(&spec.ser_type.to_le_bytes());
        child.extend_from_slice(&((3 + spec.raw.len()) as u32).to_le_bytes());
        child.push(0);
        child.extend_from_slice(&spec.version.to_le_bytes());
        child.extend_from_slice(&spec.raw);

        let mut private = Vec::new();
        private.extend_from_slice(&7i32.to_le_bytes());
        private.push(0);
        private.extend_from_slice(&0i32.to_le_bytes());
        private.push(spec.bypass as u8);
        private.extend_from_slice(&1.0f32.to_le_bytes());
        private.extend_from_slice(&0.0f32.to_le_bytes());
        private.extend_from_slice(&(-1i32).to_le_bytes());

        let mut payload = Vec::new();
        payload.push(1);
        payload.extend_from_slice(&0x50u16.to_le_bytes());
        payload.extend_from_slice(&(private.len() as u32).to_le_bytes());
        payload.extend_from_slice(&private);
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(&(child.len() as u32).to_le_bytes());
        payload.extend_from_slice(&child);

        array.extend_from_slice(&0x25u16.to_le_bytes());
        array.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        array.extend_from_slice(&payload);
    }
    array
}

fn filter_spec(bypass: bool, gain: f32) -> FxSpec {
    let mut raw = Vec::new();
    raw.extend_from_slice(&24i32.to_le_bytes());
    raw.extend_from_slice(&24i32.to_le_bytes());
    for _ in 0..3 {
        raw.extend_from_slice(&176.0f32.to_le_bytes());
        raw.extend_from_slice(&2.0f32.to_le_bytes());
        raw.extend_from_slice(&gain.to_le_bytes());
    }
    FxSpec {
        ser_type: 0x18,
        version: 0x92,
        raw,
        bypass,
    }
}

#[test]
fn binary_filter_fx_three_band_eq_boosts_the_band() {
    let fixture = Fixture::new(48_000);
    let chain = nk_format::fx::FxChain::parse(&fx_chain(&[filter_spec(false, 12.0)])).unwrap();
    assert!(chain.filter_bands().is_some());
    let mut boosted = fixture.plugin(false, None);
    boosted.fx = crate::fx_dsp::PostFx::from_chains(&[chain]);
    boosted.preload_all();
    let mut reference = fixture.plugin(false, None);
    reference.fx = None;
    reference.preload_all();
    let events = [Event::note_on(0, 60, 100)];
    let boosted = advance(&mut boosted, 48_000.0, 12_000, &events);
    let reference = advance(&mut reference, 48_000.0, 12_000, &events);
    let boosted = rms(&boosted[2_000..10_000]);
    let reference = rms(&reference[2_000..10_000]);
    assert!(
        boosted > reference * 2.5,
        "filter FX boost {boosted} not above reference {reference}"
    );

    let bypassed = nk_format::fx::FxChain::parse(&fx_chain(&[filter_spec(true, 12.0)])).unwrap();
    assert!(bypassed.filter_bands().is_none());
}

#[test]
fn binary_stereo_spread_narrows_the_mix() {
    let fixture = Fixture::new(48_000);
    let mut raw = Vec::new();
    raw.extend_from_slice(&(-1.0f32).to_le_bytes());
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    raw.push(0);
    let chain = nk_format::fx::FxChain::parse(&fx_chain(&[FxSpec {
        ser_type: 31,
        version: 0x70,
        raw,
        bypass: false,
    }]))
    .unwrap();
    let mut plugin = fixture.plugin(false, None);
    plugin.fx = crate::fx_dsp::PostFx::from_chains(&[chain]);
    plugin.preload_all();
    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    let mut output = vec![0.0; 512];
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 100)],
    );
    let difference = (0..256)
        .map(|frame| (output[frame] - output[256 + frame]).abs())
        .fold(0.0f32, f32::max);
    assert!(
        difference < 1e-5,
        "spread did not collapse to mono: {difference}"
    );
}

#[test]
fn binary_limiter_caps_the_mix() {
    let fixture = Fixture::new(48_000);
    let mut raw = Vec::new();
    raw.extend_from_slice(&20.0f32.to_le_bytes());
    raw.extend_from_slice(&50.0f32.to_le_bytes());
    let chain = nk_format::fx::FxChain::parse(&fx_chain(&[FxSpec {
        ser_type: 28,
        version: 0x60,
        raw,
        bypass: false,
    }]))
    .unwrap();
    let mut plugin = fixture.plugin(false, None);
    plugin.fx = crate::fx_dsp::PostFx::from_chains(&[chain]);
    plugin.preload_all();
    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    let mut output = vec![0.0; 512];
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 100)],
    );
    let peak = output
        .iter()
        .fold(0.0f32, |peak, value| peak.max(value.abs()));
    assert!(peak <= 1.001, "limiter peak {peak} exceeded full scale");
    assert!(peak > 0.5, "limiter attenuated the signal to {peak}");
}

#[test]
fn binary_group_filter_decodes_and_applies() {
    let fixture = Fixture::new(48_000);
    let mut raw = Vec::new();
    raw.extend_from_slice(&6i32.to_le_bytes());
    raw.extend_from_slice(&6i32.to_le_bytes());
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    let chain = nk_format::fx::FxChain::parse(&fx_chain(&[FxSpec {
        ser_type: 0x18,
        version: 0x92,
        raw,
        bypass: false,
    }]))
    .unwrap();
    let filters = chain.simple_filters();
    assert_eq!(filters.len(), 1);
    assert_eq!(filters[0].shape, nk_format::fx::FilterShape::Lowpass);
    assert_eq!(filters[0].poles, 2);

    let mut filtered = fixture.plugin(false, None);
    filtered.groups[0].fx_chain = Some(chain);
    filtered.rebuild_group_fx().unwrap();
    filtered.preload_all();
    let mut reference = fixture.plugin(false, None);
    reference.group_fx = vec![Vec::new()];
    reference.preload_all();
    let events = [Event::note_on(0, 60, 100)];
    let filtered = advance(&mut filtered, 48_000.0, 6_000, &events);
    let reference = advance(&mut reference, 48_000.0, 6_000, &events);
    let filtered = rms(&filtered[2_000..5_000]);
    let reference = rms(&reference[2_000..5_000]);
    assert!(
        filtered < reference * 0.2,
        "group low-pass {filtered} did not attenuate {reference}"
    );
}

#[test]
fn binary_group_spread_and_shaper_apply() {
    let fixture = Fixture::new(48_000);
    let context = ProcessContext {
        sample_rate: 48_000.0,
        block_size: 256,
        ..Default::default()
    };
    let mut spread = fixture.plugin(false, None);
    spread.group_fx = vec![vec![GroupFxStage::Spread {
        slot: 0,
        spread: -1.0,
        pan: 0.0,
        out_level: 1.0,
    }]];
    spread.preload_all();
    let mut output = vec![0.0; 512];
    block(
        &mut spread,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 100)],
    );
    let difference = (0..256)
        .map(|frame| (output[frame] - output[256 + frame]).abs())
        .fold(0.0f32, f32::max);
    assert!(difference < 1e-5, "group spread did not collapse to mono");

    let mut shaper = fixture.plugin(false, None);
    shaper.group_fx = vec![vec![GroupFxStage::Shaper {
        params: nk_format::fx::ShaperParams {
            shape: 1.0,
            kind: nk_format::fx::ShaperKind::Classic,
        },
        slot: 0,
        out_level: 1.0,
    }]];
    shaper.preload_all();
    let mut shaped = vec![0.0; 512];
    block(
        &mut shaper,
        &context,
        &mut shaped,
        &[Event::note_on(0, 60, 100)],
    );
    let peak = shaped
        .iter()
        .fold(0.0f32, |peak, value| peak.max(value.abs()));
    assert!(peak <= 1.0 && peak > 0.2, "Classic shaper output: {peak}");
}

#[test]
fn binary_ssl_eq_band_boost_applies() {
    let fixture = Fixture::new(48_000);
    let mut raw = Vec::new();
    // Recovered G_EQ curves: unity 0.9 is +16.0 dB, 0.5 is 183.73 Hz.
    raw.extend_from_slice(&0.9f32.to_le_bytes());
    raw.extend_from_slice(&0.5f32.to_le_bytes());
    raw.push(0);
    for value in [0.5f32; 8] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.push(0);
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    raw.push(0);
    raw.push(0);
    let chain = nk_format::fx::FxChain::parse(&fx_chain(&[FxSpec {
        ser_type: 68,
        version: 0x11,
        raw,
        bypass: false,
    }]))
    .unwrap();
    let mut boosted = fixture.plugin(false, None);
    boosted.fx = crate::fx_dsp::PostFx::from_chains(&[chain]);
    boosted.group_fx = vec![Vec::new()];
    boosted.preload_all();
    let mut reference = fixture.plugin(false, None);
    reference.fx = None;
    reference.group_fx = vec![Vec::new()];
    reference.preload_all();
    let events = [Event::note_on(0, 60, 100)];
    let boosted = advance(&mut boosted, 48_000.0, 12_000, &events);
    let reference = advance(&mut reference, 48_000.0, 12_000, &events);
    let boosted = rms(&boosted[2_000..10_000]);
    let reference = rms(&reference[2_000..10_000]);
    assert!(
        boosted > reference * 1.5,
        "SSL EQ boost {boosted} not above reference {reference}"
    );
}

#[path = "fx_routing_tests.rs"]
mod fx_routing_tests;

#[path = "key_position_tests.rs"]
mod key_position_tests;

#[path = "aux_tests.rs"]
mod aux_tests;

#[path = "filter_fidelity_tests.rs"]
mod filter_fidelity_tests;

#[path = "shaper_tests.rs"]
mod shaper_tests;

#[path = "cabinet_tests.rs"]
mod cabinet_tests;

#[path = "amp_tests.rs"]
mod amp_tests;

#[path = "group_amp_tests.rs"]
mod group_amp_tests;

#[path = "group_amp_capture_probe.rs"]
mod group_amp_capture_probe;

#[test]
fn normal_interpolation_reproduces_cubic_and_bounds_outer_taps() {
    // A four-point Lagrange interpolator reproduces any cubic exactly;
    // linear and Catmull-Rom produce different values for this asymmetric data.
    let left = vec![0.0, 1.0, 8.0, 27.0, 64.0];
    let right = left.iter().map(|x| -2.0 * x + 3.0).collect();
    let sample = SampleAsset {
        info: SampleInfo {
            channels: 2,
            sample_rate: 48_000,
            num_frames: 5,
        },
        data: SampleData::Resident(audio::Sample {
            channels: 2,
            sample_rate: 48_000,
            num_frames: 5,
            samples: vec![left, right],
        }),
    };
    for position in [1.125f64, 1.5, 1.875, 2.25, 2.75] {
        let (left, right) = sample_at(&sample, &mut None, 0, position, 5, 0).unwrap();
        let expected = position.powi(3) as f32;
        assert!((left - expected).abs() < 8e-6, "position={position}");
        assert!((right - (-2.0 * expected + 3.0)).abs() < 16e-6);
    }
    // Preserve the bounded linear fallback at asset/zone edges. Native guard
    // samples and loop-edge tap remapping have not yet been established.
    assert_eq!(
        sample_at(&sample, &mut None, 0, 0.5, 5, 0),
        Some((0.5, 2.0))
    );
    assert_eq!(
        sample_at(&sample, &mut None, 0, 3.5, 5, 0),
        Some((45.5, -88.0))
    );
    assert_eq!(
        sample_at(&sample, &mut None, 0, 4.5, 5, 0),
        Some((64.0, -125.0))
    );
    assert_eq!(sample_at(&sample, &mut None, 0, 5.0, 5, 0), None);
    assert_eq!(sample_at(&sample, &mut None, 0, -0.5, 5, 0), None);
}

#[test]
fn normal_interpolation_matches_observed_kontakt_kernel_frames() {
    // Original Kontakt 8 arm64 VST3 ResampleChannelQuad_L_SSE2, runtime
    // offset 0x4df780 (same recovered function as 0x1004dc2a4). Four
    // consecutive stereo frames observed with speed bits 0x3f8b2027 and
    // BFractionalPos [69, 0.5627822875976563]. Source taps and output are
    // actual native reads, independently captured on 2026-11-11; they are
    // not generated by this Rust implementation.
    // Store exact IEEE754 words from the capture without decimal rounding.
    let observed = [
        (
            0x3f101280,
            [
                (0xbda95820, 0x3d7c13c0),
                (0xbda60470, 0x3d917f70),
                (0xbda58590, 0x3da72a70),
                (0xbdaa57f0, 0x3dbd6990),
            ],
            (0xbda53b12, 0x3d9d865f),
        ),
        (
            0x3f265300,
            [
                (0xbda60470, 0x3d917f70),
                (0xbda58590, 0x3da72a70),
                (0xbdaa57f0, 0x3dbd6990),
                (0xbdb49c80, 0x3dd26660),
            ],
            (0xbda80a93, 0x3db5ab2a),
        ),
        (
            0x3f3c9380,
            [
                (0xbda58590, 0x3da72a70),
                (0xbdaa57f0, 0x3dbd6990),
                (0xbdb49c80, 0x3dd26660),
                (0xbdc10050, 0x3de409b0),
            ],
            (0xbdb190be, 0x3dcd1ca2),
        ),
        (
            0x3f52d400,
            [
                (0xbdaa57f0, 0x3dbd6990),
                (0xbdb49c80, 0x3dd26660),
                (0xbdc10050, 0x3de409b0),
                (0xbdcbd470, 0x3df05c10),
            ],
            (0xbdbed2d0, 0x3de1417c),
        ),
    ];
    let decode = |(left, right)| (f32::from_bits(left), f32::from_bits(right));
    for (fraction, taps, expected) in observed {
        let taps = taps.map(decode);
        let expected = decode(expected);
        let actual = lagrange4(taps[0], taps[1], taps[2], taps[3], f32::from_bits(fraction));
        assert!((actual.0 - expected.0).abs() < 3e-8);
        assert!((actual.1 - expected.1).abs() < 3e-8);
    }
}
