#[cfg(target_os = "macos")]
#[test]
fn native_clap_editor_opens_in_worker() {
    let executable = std::env::current_exe().unwrap();
    let target = executable.parent().unwrap().parent().unwrap();
    let fixture = target.join("libcadence_test_plugin.dylib");
    if !fixture.exists() {
        return; // cargo did not build the test cdylib
    }
    let path = std::env::temp_dir().join(format!("cadence-gui-{}.clap", std::process::id()));
    std::fs::copy(&fixture, &path).unwrap();
    let mut worker =
        plugin_sandbox::SandboxPlugin::spawn(env!("CARGO_BIN_EXE_plugin-worker"), &path).unwrap();
    let monitor = worker.pipeline_monitor();
    monitor.set_gui_request(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while !monitor.gui_open() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    assert!(monitor.gui_ready(), "worker GUI thread did not start");
    assert!(monitor.gui_open(), "native CLAP editor did not open");
    use plugin_api::{AudioBuffer, Plugin, ProcessContext};
    let context = ProcessContext {
        block_size: 256,
        sample_rate: 48_000.0,
        playing: true,
        ..Default::default()
    };
    worker.prepare(&context);
    let mut output = vec![0.0f32; 512];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256);
    worker.process_offline_delayed(&context, &mut audio, &[]);
    assert!(
        !worker.has_failed(),
        "audio thread failed while the GUI was open"
    );
    monitor.set_gui_request(0);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while monitor.gui_open() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!monitor.gui_open(), "native editor did not close");
    monitor.set_gui_request(1);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
    while !monitor.gui_open() && std::time::Instant::now() < deadline {
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(monitor.gui_open(), "native editor did not reopen");
    drop(worker);
    let _ = std::fs::remove_file(path);
}
