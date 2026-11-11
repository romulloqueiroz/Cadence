use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext, State};
use plugin_sandbox::{host::SandboxConfig, SandboxPlugin};
use std::{
    alloc::{GlobalAlloc, Layout, System},
    cell::Cell,
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
    time::{Duration, Instant},
};

struct CountAlloc;
thread_local! { static COUNTING: Cell<bool> = const { Cell::new(false) }; static ALLOCATIONS: Cell<usize> = const { Cell::new(0) }; }
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|flag| {
            if flag.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        unsafe { System.dealloc(ptr, layout) }
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|flag| {
            if flag.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.realloc(ptr, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;
fn worker() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_plugin-worker"))
}

#[test]
fn audio_exchange_is_allocation_free_and_handles_variable_blocks() {
    let mut plugin = SandboxPlugin::spawn(worker(), "builtin:sine").unwrap();
    assert_eq!(plugin.parameters().len(), 1);
    assert_eq!(plugin.metadata().name, "Sine Demo");
    assert!(plugin.pipeline_monitor().streaming_telemetry().is_none());
    let mut context = ProcessContext {
        block_size: 4096,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = vec![0.0; 8192];
    for frames in [1, 17, 64, 256, 4096] {
        let events = [Event::note_on(0, 69, 100)];
        let mut audio =
            AudioBuffer::new(&mut [], 0, &mut output[..2 * frames], 2, &mut [], 0, frames);
        ALLOCATIONS.with(|count| count.set(0));
        COUNTING.with(|flag| flag.set(true));
        plugin.process(&context, &mut audio, &events);
        COUNTING.with(|flag| flag.set(false));
        assert_eq!(
            ALLOCATIONS.with(Cell::get),
            0,
            "host audio callback allocated"
        );
        assert!(!plugin.has_failed());
        assert!(audio.outputs.iter().all(|v| v.is_finite()));
        context.sample_pos += frames as i64;
    }
    let state = plugin.save_state_checked().unwrap();
    plugin.load_state(&state).unwrap();
    assert_eq!(plugin.save_state_checked().unwrap(), state);
    assert!(plugin
        .load_state(&State {
            schema_version: state.schema_version,
            bytes: vec![0; 65537]
        })
        .is_err());
}

#[test]
fn crash_is_bounded_silent_and_reaped() {
    let mut plugin = SandboxPlugin::spawn_with_config(
        worker(),
        "builtin:sine",
        SandboxConfig {
            block_timeout: Duration::from_millis(5),
            ..Default::default()
        },
    )
    .unwrap();
    let context = ProcessContext::default();
    plugin.prepare(&context);
    let pid = plugin.worker_id();
    unsafe { libc::kill(pid as i32, libc::SIGKILL) };
    let mut output = [1.0; 512];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256);
    let start = Instant::now();
    plugin.process(&context, &mut audio, &[]);
    assert!(start.elapsed() < Duration::from_millis(200));
    assert!(plugin.has_failed());
    assert!(plugin.late_blocks() <= 1); // The watchdog may observe the exit first.
    assert!(audio.outputs.iter().all(|v| *v == 0.0));
    audio.outputs.fill(1.0);
    plugin.process(&context, &mut audio, &[]);
    assert!(audio.outputs.iter().all(|v| *v == 0.0));
    drop(plugin);
    assert_ne!(unsafe { libc::kill(pid as i32, 0) }, 0);
}

#[test]
fn missing_plugin_returns_error_without_waiting_for_startup_deadline() {
    let start = Instant::now();
    assert!(SandboxPlugin::spawn(worker(), "/does/not/exist.dylib").is_err());
    assert!(start.elapsed() < Duration::from_secs(3));
}

#[test]
fn startup_timeout_kills_worker_and_releases_resources() {
    use std::os::unix::fs::PermissionsExt;
    static COUNTER: AtomicUsize = AtomicUsize::new(0);
    let directory = std::env::temp_dir().join(format!(
        "cadence-startup-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&directory).unwrap();
    let script = directory.join("hang worker");
    let pid = directory.join("worker.pid");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 60\n",
            pid.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let start = Instant::now();
    let result = SandboxPlugin::spawn_with_config(
        &script,
        "unused",
        SandboxConfig {
            startup_timeout: Duration::from_millis(750),
            ..Default::default()
        },
    );
    let error = result.err().expect("Startup must fail");
    assert!(
        error.to_string().contains("startup timed out"),
        "Unexpected startup failure: {error}"
    );
    assert!(start.elapsed() < Duration::from_secs(2));
    // macOS can spend the whole deadline before entering a fresh executable.
    // When the script has begun, also verify its recorded pid was reaped.
    if let Ok(recorded) = std::fs::read_to_string(pid) {
        let worker_pid = recorded.parse::<i32>().unwrap();
        assert_ne!(unsafe { libc::kill(worker_pid, 0) }, 0);
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn signed_package_checks_assets_before_loading_code() {
    use plugin_package::{Manifest, Package};
    let library =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/libgain.dylib");
    if !library.exists() {
        eprintln!("SKIP signed native package test: build gain first");
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("cadence-signed-package-{}", std::process::id()));
    let mut package = Package::new(Manifest::new(
        "com.example.gain",
        "Gain",
        "1.0",
        plugin_api::PluginKind::Effect,
        "gain.dylib",
    ));
    package
        .add_asset(
            "gain.dylib",
            "application/octet-stream",
            std::fs::read(library).unwrap(),
        )
        .unwrap();
    package
        .sign(&ed25519_dalek::SigningKey::from_bytes(&[7; 32]))
        .unwrap();
    package.save_to(&directory).unwrap();
    let manifest = directory.join("manifest.json");
    let mut plugin = SandboxPlugin::spawn(worker(), &manifest).unwrap();
    assert_eq!(plugin.metadata().id, "com.example.gain");
    assert_eq!(plugin.parameters().len(), 1);
    let context = ProcessContext::default();
    plugin.prepare(&context);
    let mut input = vec![0.5; 512];
    let mut output = vec![0.0; 512];
    let mut audio = AudioBuffer::new(&mut input, 2, &mut output, 2, &mut [], 0, 256);
    plugin.process(&context, &mut audio, &[]);
    assert!(audio.outputs.iter().all(|v| (*v - 0.5).abs() < 1e-6));
    drop(plugin);
    std::fs::write(directory.join("assets/gain.dylib"), b"tampered").unwrap();
    assert!(SandboxPlugin::spawn(worker(), &manifest).is_err());
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn unlinking_shared_memory_keeps_existing_mappings_usable() {
    use plugin_sandbox::shared::SharedMapping;
    let owner = SharedMapping::create().unwrap();
    let mapped = SharedMapping::open(owner.name()).unwrap();
    mapped.unlink_name();
    assert!(SharedMapping::open(owner.name()).is_err());
    // Both semaphore handles survive unlink, and a notification sent before
    // the worker begins waiting must be retained.
    assert!(owner.notify());
    mapped.wait_for_work().unwrap();
    owner.block().block_state.store(123, Ordering::Release);
    assert_eq!(mapped.block().block_state.load(Ordering::Acquire), 123);
}

fn await_audio(plugin: &SandboxPlugin) {
    let monitor = plugin.pipeline_monitor();
    let deadline = Instant::now() + Duration::from_secs(1);
    while monitor.completed() != monitor.published() {
        assert!(
            !plugin.has_failed(),
            "worker failed while waiting outside callback"
        );
        assert!(Instant::now() < deadline, "worker did not finish");
        std::thread::sleep(Duration::from_micros(50));
    }
}

#[test]
fn pipeline_is_allocation_free_and_exactly_one_buffer_late() {
    for frames in [64, 128, 256] {
        let mut realtime = SandboxPlugin::spawn(worker(), "builtin:sine").unwrap();
        let mut reference = SandboxPlugin::spawn(worker(), "builtin:sine").unwrap();
        let mut context = ProcessContext {
            block_size: frames,
            playing: true,
            ..Default::default()
        };
        realtime.prepare(&context);
        reference.prepare(&context);
        assert_eq!(realtime.pipeline_latency(), frames);
        let monitor = realtime.pipeline_monitor();
        let mut actual = vec![0.0; 2 * frames];
        let mut expected = vec![0.0; 2 * frames];
        let mut prior = vec![0.0; 2 * frames];
        for index in 0..10 {
            let event = if index == 0 {
                Event::note_on(17, 69, 100)
            } else {
                Event::note_off(frames / 2, 69)
            };
            let events = if index == 0 || index == 4 {
                std::slice::from_ref(&event)
            } else {
                &[]
            };
            let mut audio = AudioBuffer::new(&mut [], 0, &mut actual, 2, &mut [], 0, frames);
            ALLOCATIONS.with(|n| n.set(0));
            COUNTING.with(|flag| flag.set(true));
            realtime.process_pipelined(&context, &mut audio, events);
            COUNTING.with(|flag| flag.set(false));
            assert_eq!(
                ALLOCATIONS.with(Cell::get),
                0,
                "pipeline callback allocated"
            );
            assert_eq!(
                actual, prior,
                "pipeline offset at {frames} samples, block {index}"
            );
            let mut reference_audio =
                AudioBuffer::new(&mut [], 0, &mut expected, 2, &mut [], 0, frames);
            reference.process(&context, &mut reference_audio, events);
            prior.copy_from_slice(&expected);
            await_audio(&realtime); // Scheduling is external to the callback.
            context.sample_pos += frames as i64;
        }
        assert_eq!(monitor.underruns(), 0);
    }
}

#[test]
fn stopped_worker_never_holds_callback_and_stale_audio_is_discarded() {
    let library =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/libgain.dylib");
    assert!(
        library.exists(),
        "build gain before running transport integration tests"
    );
    let mut plugin = SandboxPlugin::spawn_with_config(
        worker(),
        library,
        SandboxConfig {
            block_timeout: Duration::from_secs(1),
            ..Default::default()
        },
    )
    .unwrap();
    let context = ProcessContext {
        block_size: 64,
        ..Default::default()
    };
    plugin.prepare(&context);
    let pid = plugin.worker_id() as i32;
    unsafe { libc::kill(pid, libc::SIGSTOP) };
    let mut input = [1.0; 128];
    let mut output = [0.0; 128];
    let mut audio = AudioBuffer::new(&mut input, 2, &mut output, 2, &mut [], 0, 64);
    let start = Instant::now();
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(
        start.elapsed() < Duration::from_millis(10),
        "pipeline waited for stopped worker"
    );
    assert!(audio.outputs.iter().all(|v| *v == 0.0));
    audio.inputs.fill(2.0);
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(audio.outputs.iter().all(|v| *v == 0.0));
    assert_eq!(plugin.pipeline_monitor().underruns(), 1);
    unsafe { libc::kill(pid, libc::SIGCONT) };
    await_audio(&plugin);
    audio.inputs.fill(3.0);
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(
        audio.outputs.iter().all(|v| *v == 2.0),
        "missed first packet must not replay late"
    );
}

#[test]
fn pipeline_watchdog_operates_without_more_audio_callbacks() {
    let mut plugin = SandboxPlugin::spawn_with_config(
        worker(),
        "builtin:sine",
        SandboxConfig {
            block_timeout: Duration::from_millis(15),
            ..Default::default()
        },
    )
    .unwrap();
    let context = ProcessContext {
        block_size: 64,
        ..Default::default()
    };
    plugin.prepare(&context);
    unsafe { libc::kill(plugin.worker_id() as i32, libc::SIGSTOP) };
    let mut output = [0.0; 128];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 64);
    plugin.process_pipelined(&context, &mut audio, &[]);
    let deadline = Instant::now() + Duration::from_millis(250);
    while !plugin.has_failed() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(
        plugin.has_failed(),
        "watchdog did not stop the blocked worker"
    );
    let pid = plugin.worker_id() as i32;
    drop(plugin);
    assert_ne!(unsafe { libc::kill(pid, 0) }, 0);
}

#[test]
fn watchdog_allows_the_prepared_buffer_time_to_finish() {
    let mut plugin = SandboxPlugin::spawn_with_config(
        worker(),
        "builtin:sine",
        SandboxConfig {
            block_timeout: Duration::from_millis(5),
            ..Default::default()
        },
    )
    .unwrap();
    // A 4096-frame buffer at 48 kHz gives the worker 85 ms. A scheduling
    // pause shorter than that must not permanently kill a healthy instrument.
    let context = ProcessContext {
        block_size: 4096,
        ..Default::default()
    };
    plugin.prepare(&context);
    let pid = plugin.worker_id() as i32;
    assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
    let mut output = vec![0.0; 8192];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 4096);
    plugin.process_pipelined(&context, &mut audio, &[Event::note_on(0, 69, 100)]);
    std::thread::sleep(Duration::from_millis(30));
    unsafe { libc::kill(pid, libc::SIGCONT) };
    assert!(
        !plugin.has_failed(),
        "watchdog killed worker before its audio deadline"
    );
    await_audio(&plugin);
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(audio.outputs.iter().any(|sample| sample.abs() > 0.01));
    assert_eq!(plugin.pipeline_monitor().underruns(), 0);
}

#[test]
fn small_callbacks_do_not_exhaust_queue_before_prepared_audio_deadline() {
    queued_small_callbacks(false);
}

#[test]
fn combined_packets_preserve_stereo_inputs_and_parameter_timing() {
    queued_small_callbacks(true);
}

fn queued_small_callbacks(effect: bool) {
    let source = if effect {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug/libgain.dylib")
    } else {
        PathBuf::from("builtin:sine")
    };
    let mut plugin = SandboxPlugin::spawn(worker(), &source).unwrap();
    let mut reference = SandboxPlugin::spawn(worker(), &source).unwrap();
    let channels = if effect { 2 } else { 0 };
    let input_for = |index: usize| -> [f32; 128] {
        std::array::from_fn(|sample| {
            (1 + index * 64 + sample % 64) as f32 * if sample < 64 { 0.0001 } else { -0.0002 }
        })
    };
    let mut context = ProcessContext {
        block_size: 4096,
        playing: true,
        ..Default::default()
    };
    plugin.prepare(&context);
    reference.prepare(&context);
    let mut expected = Vec::new();
    for index in 0..32 {
        let onset = Event::note_on(17, 69, 100);
        let offset = if effect {
            Event::parameter(31, 1, 48.0 / 72.0)
        } else {
            Event::note_off(31, 69)
        };
        let events = if index == 0 && !effect {
            std::slice::from_ref(&onset)
        } else if index == 10 {
            std::slice::from_ref(&offset)
        } else {
            &[]
        };
        let mut samples = [0.0; 128];
        let mut inputs = input_for(index);
        let mut audio = AudioBuffer::new(
            &mut inputs[..channels * 64],
            channels,
            &mut samples,
            2,
            &mut [],
            0,
            64,
        );
        reference.process(&context, &mut audio, events);
        expected.push(samples);
    }
    let pid = plugin.worker_id() as i32;
    assert_eq!(unsafe { libc::kill(pid, libc::SIGSTOP) }, 0);
    let mut actual = [0.0; 128];
    // Sixteen small host callbacks consume only 1024 of the 4096 samples of
    // prepared latency. No output is due yet, and no MIDI may be dropped.
    for index in 0..16 {
        let onset = Event::note_on(17, 69, 100);
        let offset = if effect {
            Event::parameter(31, 1, 48.0 / 72.0)
        } else {
            Event::note_off(31, 69)
        };
        let events = if index == 0 && !effect {
            std::slice::from_ref(&onset)
        } else if index == 10 {
            std::slice::from_ref(&offset)
        } else {
            &[]
        };
        let mut inputs = input_for(index);
        let mut audio = AudioBuffer::new(
            &mut inputs[..channels * 64],
            channels,
            &mut actual,
            2,
            &mut [],
            0,
            64,
        );
        ALLOCATIONS.with(|count| count.set(0));
        COUNTING.with(|flag| flag.set(true));
        plugin.process_pipelined(&context, &mut audio, events);
        COUNTING.with(|flag| flag.set(false));
        assert_eq!(ALLOCATIONS.with(Cell::get), 0, "queue pressure allocated");
        assert!(
            !plugin.has_failed(),
            "queue exhausted at callback {index} before output was due"
        );
        context.sample_pos += 64;
    }
    assert_eq!(unsafe { libc::kill(pid, libc::SIGCONT) }, 0);
    await_audio(&plugin);
    // Compare the complete delayed waveform, including the coalesced note-off.
    for index in 16..96 {
        let mut inputs = input_for(index);
        let mut audio = AudioBuffer::new(
            &mut inputs[..channels * 64],
            channels,
            &mut actual,
            2,
            &mut [],
            0,
            64,
        );
        plugin.process_offline_delayed(&context, &mut audio, &[]);
        let silence = [0.0; 128];
        let target = if index < 64 {
            &silence
        } else {
            &expected[index - 64]
        };
        assert_eq!(
            &actual, target,
            "delayed audio after queue pressure at callback {index}"
        );
        context.sample_pos += 64;
    }
    assert_eq!(plugin.pipeline_monitor().underruns(), 0);
}

#[test]
fn state_capture_preserves_queued_output_and_seek_flushes_voices() {
    let mut plugin = SandboxPlugin::spawn(worker(), "builtin:sine").unwrap();
    let mut context = ProcessContext {
        block_size: 64,
        playing: true,
        ..Default::default()
    };
    plugin.prepare(&context);
    let mut output = [0.0; 128];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 64);
    plugin.process_pipelined(&context, &mut audio, &[Event::note_on(0, 69, 100)]);
    let state = plugin.save_state_checked().unwrap(); // Drains worker work, not its scheduled output.
    context.sample_pos = 64;
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(
        audio.outputs.iter().any(|v| v.abs() > 0.001),
        "saving must preserve delayed audio"
    );
    await_audio(&plugin);
    context.sample_pos = 9000;
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(
        audio.outputs.iter().all(|v| *v == 0.0),
        "seek should clear old scheduled output"
    );
    await_audio(&plugin);
    context.sample_pos += 64;
    plugin.process_pipelined(&context, &mut audio, &[]);
    assert!(
        audio.outputs.iter().all(|v| *v == 0.0),
        "worker voices should reset at seek epoch"
    );
    await_audio(&plugin);
    plugin.load_state(&state).unwrap();
    assert_eq!(plugin.pipeline_monitor().underruns(), 0);
}

fn diagnostic_directory(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!("cadence-{name}-{}", std::process::id()));
    std::fs::create_dir_all(&path).unwrap();
    path
}

#[test]
fn idle_worker_exit_is_logged_without_another_audio_callback() {
    use plugin_sandbox::diagnostics::FailureReason;
    let directory = diagnostic_directory("idle-exit");
    let plugin = SandboxPlugin::spawn_with_config(
        worker(),
        "builtin:sine",
        SandboxConfig {
            diagnostic_directory: Some(directory.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let pid = plugin.worker_id();
    assert_eq!(unsafe { libc::kill(pid as i32, libc::SIGKILL) }, 0);
    let end = Instant::now() + Duration::from_secs(1);
    while !plugin.has_failed() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(plugin.failure_reason(), FailureReason::WorkerExited);
    drop(plugin); // Joins the logger and reaps the observed child.
    assert_ne!(unsafe { libc::kill(pid as i32, 0) }, 0);
    let log = directory.join(format!("worker-failures-{}.jsonl", std::process::id()));
    let rows = std::fs::read_to_string(log).unwrap();
    let record: serde_json::Value = serde_json::from_str(rows.lines().last().unwrap()).unwrap();
    assert_eq!(record["reason"], "worker_exited");
    assert_eq!(record["worker_pid"], pid);
    assert_eq!(record["exit_status"], libc::SIGKILL);
    assert_eq!(record["plugin_id"], "com.example.sine");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn queue_overflow_reason_is_logged_off_the_audio_thread() {
    use plugin_sandbox::diagnostics::FailureReason;
    let directory = diagnostic_directory("queue-log");
    let mut plugin = SandboxPlugin::spawn_with_config(
        worker(),
        "builtin:sine",
        SandboxConfig {
            block_timeout: Duration::from_secs(1),
            diagnostic_directory: Some(directory.clone()),
            ..Default::default()
        },
    )
    .unwrap();
    let context = ProcessContext {
        block_size: 64,
        ..Default::default()
    };
    plugin.prepare(&context);
    unsafe {
        libc::kill(plugin.worker_id() as i32, libc::SIGSTOP);
    }
    let mut output = [0.0; 128];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 64);
    ALLOCATIONS.with(|n| n.set(0));
    COUNTING.with(|flag| flag.set(true));
    for _ in 0..9 {
        plugin.process_pipelined(&context, &mut audio, &[]);
    }
    COUNTING.with(|flag| flag.set(false));
    assert_eq!(ALLOCATIONS.with(Cell::get), 0);
    assert_eq!(plugin.failure_reason(), FailureReason::QueueFull);
    drop(plugin);
    let log = directory.join(format!("worker-failures-{}.jsonl", std::process::id()));
    let rows = std::fs::read_to_string(log).unwrap();
    let record: serde_json::Value = serde_json::from_str(rows.lines().last().unwrap()).unwrap();
    assert_eq!(record["reason"], "queue_overflow");
    assert_eq!(record["pending_packets"], 8);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn cancelling_startup_does_not_wait_for_timeout_or_log_a_crash() {
    use std::os::unix::fs::PermissionsExt;
    let directory = diagnostic_directory("cancel-startup");
    let script = directory.join("slow worker");
    let pid_file = directory.join("worker.pid");
    std::fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s' \"$$\" > '{}'\nexec /bin/sleep 60\n",
            pid_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o700)).unwrap();
    let start = Instant::now();
    let result = SandboxPlugin::spawn_selected_cancellable(
        &script,
        "unused",
        None,
        SandboxConfig {
            diagnostic_directory: Some(directory.clone()),
            ..Default::default()
        },
        &|| start.elapsed() > Duration::from_millis(100),
    );
    assert!(matches!(
        result,
        Err(plugin_sandbox::SandboxError::Cancelled)
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
    if let Ok(pid) = std::fs::read_to_string(pid_file) {
        assert_ne!(unsafe { libc::kill(pid.parse::<i32>().unwrap(), 0) }, 0);
    }
    assert!(!directory
        .join(format!("worker-failures-{}.jsonl", std::process::id()))
        .exists());
    std::fs::remove_dir_all(directory).unwrap();
}
