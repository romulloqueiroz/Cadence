#![cfg(target_os = "macos")]

use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext};
use plugin_sandbox::{host::SandboxConfig, SandboxPlugin};
use std::{
    path::Path,
    time::{Duration, Instant},
};

fn wait_for(message: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while !condition() {
        assert!(Instant::now() < deadline, "{message}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn log(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_default()
}

fn note(worker: &mut SandboxPlugin, context: &mut ProcessContext, key: u8) -> Vec<f32> {
    let mut output = vec![0.0; 512];
    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256);
    worker.process(context, &mut audio, &[Event::note_on(0, key, 100)]);
    assert!(!worker.has_failed(), "CLAP worker failed for note {key}");
    context.sample_pos += 256;
    output
}

#[test]
fn clap_requests_work_on_real_main_thread_with_and_without_editor() {
    let executable = std::env::current_exe().unwrap();
    let target = executable.parent().unwrap().parent().unwrap();
    let fixture = target.join("libcadence_test_plugin.dylib");
    assert!(
        fixture.exists(),
        "Build the CLAP fixture first: cargo build -p cadence-test-plugin"
    );
    let directory =
        std::env::temp_dir().join(format!("cadence-host-requests-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let source = directory.join("requests.clap");
    std::fs::copy(fixture, &source).unwrap();

    for (selector, gui) in [
        ("test.cadence.tone", true),
        ("test.cadence.tone.second", false),
    ] {
        let path = directory.join(if gui { "editor.log" } else { "headless.log" });
        let _ = std::fs::remove_file(&path);
        let previous = std::env::var_os("CADENCE_CLAP_REQUEST_LOG");
        std::env::set_var("CADENCE_CLAP_REQUEST_LOG", &path);
        let spawn = SandboxPlugin::spawn_selected(
            env!("CARGO_BIN_EXE_plugin-worker"),
            &source,
            Some(selector),
            SandboxConfig::default(),
        );
        if let Some(previous) = previous {
            std::env::set_var("CADENCE_CLAP_REQUEST_LOG", previous);
        } else {
            std::env::remove_var("CADENCE_CLAP_REQUEST_LOG");
        }
        let mut worker = spawn.unwrap();
        let monitor = worker.pipeline_monitor();

        // Init requests a callback, which requests an inactive parameter flush.
        // This must work while the host sends no audio or control messages.
        wait_for("inactive flush did not run", || {
            log(&path)
                .contains("callback main=true active=false activations=0 flushes=1 gain=0.375")
        });
        let mut context = ProcessContext {
            sample_rate: 48000.0,
            block_size: 256,
            ..Default::default()
        };
        worker.prepare(&context);
        assert!(!worker.has_failed());
        if gui {
            monitor.set_gui_request(1);
            wait_for("native editor did not open", || monitor.gui_open());
        } else {
            assert!(!monitor.gui_ready());
        }

        let before = log(&path).matches("callback ").count();
        note(&mut worker, &mut context, 124);
        wait_for("main callback was not serviced without more audio", || {
            log(&path).matches("callback ").count() > before
        });

        note(&mut worker, &mut context, 122);
        wait_for("active flush did not run while host was idle", || {
            log(&path).contains("flushes=2 gain=0.375")
        });
        let mut ids = [0; 8];
        let mut values = [0.0; 8];
        wait_for("flush output did not reach host parameter snapshot", || {
            monitor
                .parameter_snapshot(&mut ids, &mut values)
                .is_some_and(|(count, _)| count > 0 && (values[0] - 0.375).abs() < 1e-6)
        });

        let saved = worker.save_state_checked().unwrap();
        note(&mut worker, &mut context, 123);
        wait_for("restart did not reactivate same instance", || {
            log(&path).contains("activate main=true active=true activations=2")
        });
        assert_eq!(
            worker.save_state_checked().unwrap(),
            saved,
            "restart changed plugin state"
        );
        assert!(
            !monitor.reload_requested(),
            "ordinary restart should retain the same slot/instance"
        );
        let output = note(&mut worker, &mut context, 121);
        assert!(output.iter().all(|sample| (*sample - 0.375).abs() < 1e-6));
        let output = note(&mut worker, &mut context, 69);
        assert!(output.iter().all(|sample| (*sample - 0.375).abs() < 1e-6));

        if gui {
            // Fixture invokes NSWindow.performClose, the same delegate route
            // as the title-bar close button, through a real main-thread callback.
            note(&mut worker, &mut context, 120);
            wait_for("native title-bar close did not update host state", || {
                !monitor.gui_open()
            });
            monitor.set_gui_request(1);
            wait_for("native editor did not reopen after user close", || {
                monitor.gui_open()
            });
            note(&mut worker, &mut context, 69);
        }
        assert!(
            !log(&path).contains("main=false"),
            "CLAP lifecycle or callback ran off the actual OS main thread: {}",
            log(&path)
        );
        drop(worker);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
