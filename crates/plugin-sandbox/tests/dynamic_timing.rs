#![cfg(target_os = "macos")]

//! A CLAP latency change is serviced by the worker's creator thread, so it must
//! reach the shared monitor even while the audio thread is idle.

use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext};
use plugin_sandbox::SandboxPlugin;
use std::time::{Duration, Instant};

#[test]
fn latency_change_reaches_the_host_without_another_audio_block() {
    let executable = std::env::current_exe().unwrap();
    let target = executable.parent().unwrap().parent().unwrap();
    let fixture = target.join("libcadence_test_plugin.dylib");
    if !fixture.exists() {
        eprintln!("skip: build the CLAP fixture with cargo build -p cadence-test-plugin");
        return;
    }
    let directory =
        std::env::temp_dir().join(format!("cadence-dynamic-timing-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("timing.clap");
    std::fs::copy(&fixture, &source).unwrap();

    let mut worker = SandboxPlugin::spawn(env!("CARGO_BIN_EXE_plugin-worker"), &source).unwrap();
    let monitor = worker.pipeline_monitor();
    let context = ProcessContext {
        sample_rate: 48000.0,
        block_size: 256,
        ..Default::default()
    };
    worker.prepare(&context);
    assert_eq!(monitor.plugin_latency(), 0);

    let mut output = vec![0.0; 512];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256);
    // The fixture reports 128 frames once gain crosses 0.8 and calls
    // clap_host_latency.changed from inside the process callback.
    worker.process(&context, &mut audio, &[Event::parameter(0, 0, 0.9)]);

    let deadline = Instant::now() + Duration::from_secs(2);
    while monitor.plugin_latency() != 128 && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(5));
    }
    assert_eq!(monitor.plugin_latency(), 128);
    let _ = std::fs::remove_dir_all(&directory);
}
