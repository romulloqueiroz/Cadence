//! Real CLAP lifecycle/editor regression on the OS main thread. This target
//! deliberately uses no shared-memory IPC, so it also runs in restricted shells.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
thread_local! {
    static COUNTING: Cell<bool> = const { Cell::new(false) };
    static ALLOCATIONS: Cell<usize> = const { Cell::new(0) };
}
struct CountAlloc;
unsafe impl GlobalAlloc for CountAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        COUNTING.with(|flag| {
            if flag.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.alloc(layout) }
    }
    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe {
            System.dealloc(pointer, layout);
        }
    }
    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        COUNTING.with(|flag| {
            if flag.get() {
                ALLOCATIONS.with(|count| count.set(count.get() + 1));
            }
        });
        unsafe { System.realloc(pointer, layout, size) }
    }
}
#[global_allocator]
static ALLOCATOR: CountAlloc = CountAlloc;

#[cfg(target_os = "macos")]
fn main() {
    use adapter_clap::ClapModule;
    use plugin_api::{AudioBuffer, Event, Plugin, ProcessContext, State};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    };
    use std::time::{Duration, Instant};

    let executable = std::env::current_exe().unwrap();
    let target = executable.parent().unwrap().parent().unwrap();
    let fixture = target.join("libcadence_test_plugin.dylib");
    assert!(
        fixture.exists(),
        "Build the fixture first: cargo build -p cadence-test-plugin"
    );
    let directory =
        std::env::temp_dir().join(format!("cadence-main-thread-{}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();

    for (selector, with_gui) in [
        ("test.cadence.tone", true),
        ("test.cadence.tone.second", false),
    ] {
        let log = directory.join(if with_gui {
            "editor.log"
        } else {
            "headless.log"
        });
        std::env::set_var("CADENCE_CLAP_REQUEST_LOG", &log);
        let plugin = ClapModule::load(&fixture)
            .unwrap()
            .create(Some(selector))
            .unwrap();
        std::env::remove_var("CADENCE_CLAP_REQUEST_LOG");
        // The plugin test logger reads this environment on each main-thread
        // callback, so leave it set throughout this sequential standalone test.
        std::env::set_var("CADENCE_CLAP_REQUEST_LOG", &log);
        let tasks = plugin.main_thread_tasks().unwrap();
        let mut gui = plugin_sandbox::gui::prepare(
            plugin.native_gui_handle(),
            "Cadence CLAP regression".into(),
        );
        assert_eq!(gui.is_some(), with_gui);
        if let Some(gui) = &mut gui {
            assert!(gui.open());
        }
        let close_requested = Arc::new(AtomicBool::new(false));
        let closed = Arc::new(AtomicBool::new(false));
        let reopened = Arc::new(AtomicBool::new(false));
        let close_request_audio = close_requested.clone();
        let closed_audio = closed.clone();
        let reopened_audio = reopened.clone();
        let thread = std::thread::spawn(move || {
            let mut plugin = plugin;
            let wait = |message: &str,
                        plugin: &mut adapter_clap::ClapPlugin,
                        condition: &dyn Fn() -> bool| {
                let deadline = Instant::now() + Duration::from_secs(5);
                while !condition() {
                    plugin.service_host_requests();
                    assert!(Instant::now() < deadline, "{message}");
                    std::thread::sleep(Duration::from_millis(1));
                }
            };
            let read = || std::fs::read_to_string(&log).unwrap_or_default();
            wait("inactive flush", &mut plugin, &|| {
                read()
                    .contains("callback main=true active=false activations=0 flushes=1 gain=0.375")
            });
            let mut context = ProcessContext {
                sample_rate: 48000.0,
                block_size: 256,
                ..Default::default()
            };
            plugin.prepare(&context);
            assert!(plugin.processing_error().is_none());
            let process =
                |plugin: &mut adapter_clap::ClapPlugin, context: &mut ProcessContext, key| {
                    let mut output = [0.0; 512];
                    let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256);
                    ALLOCATIONS.with(|count| count.set(0));
                    COUNTING.with(|flag| flag.set(true));
                    plugin.process(context, &mut audio, &[Event::note_on(0, key, 100)]);
                    COUNTING.with(|flag| flag.set(false));
                    assert_eq!(
                        ALLOCATIONS.with(Cell::get),
                        0,
                        "CLAP process/request callback allocated"
                    );
                    assert!(plugin.processing_error().is_none());
                    assert!(output.iter().all(|value| (*value - 0.375).abs() < 1e-6));
                    context.sample_pos += 256;
                };
            process(&mut plugin, &mut context, 122);
            wait("active flush", &mut plugin, &|| {
                read().contains("active=true activations=1 flushes=2")
            });
            let before = plugin.try_save_state().unwrap();
            process(&mut plugin, &mut context, 123);
            wait("same-instance restart", &mut plugin, &|| {
                read().contains("activate main=true active=true activations=2")
            });
            assert_eq!(plugin.try_save_state().unwrap(), before);
            assert!(!plugin.take_reload_request());
            process(&mut plugin, &mut context, 121);
            process(&mut plugin, &mut context, 69);
            plugin
                .load_state(&State {
                    schema_version: 1,
                    bytes: 0.375f32.to_le_bytes().to_vec(),
                })
                .unwrap();
            assert_eq!(plugin.try_save_state().unwrap(), before);
            if with_gui {
                close_request_audio.store(true, Ordering::Release);
                process(&mut plugin, &mut context, 120);
                wait("native title-bar close", &mut plugin, &|| {
                    closed_audio.load(Ordering::Acquire)
                });
                wait("editor reopen", &mut plugin, &|| {
                    reopened_audio.load(Ordering::Acquire)
                });
                process(&mut plugin, &mut context, 69);
            }
            assert!(!read().contains("main=false"), "{}", read());
            process(&mut plugin, &mut context, 119);
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                plugin.service_host_requests();
                if plugin.take_reload_request() {
                    break;
                }
                assert!(
                    Instant::now() < deadline,
                    "structural restart did not request slot reload"
                );
                std::thread::sleep(Duration::from_millis(1));
            }
            assert!(plugin.processing_error().is_none());
            let mut output = [0.0; 512];
            let mut audio = AudioBuffer::new(&mut [], 0, &mut output, 2, &mut [], 0, 256);
            plugin.process(&context, &mut audio, &[Event::note_on(0, 69, 100)]);
            assert!(
                output.iter().all(|value| *value == 0.0),
                "structurally stale plugin should remain quiescent until reload"
            );
            // Drop must close the editor before main-thread destroy/dlclose.
            drop(plugin);
        });
        let deadline = Instant::now() + Duration::from_secs(20);
        let mut did_reopen = false;
        while !thread.is_finished() {
            assert!(Instant::now() < deadline, "CLAP main-thread test timed out");
            if tasks.is_shutting_down() {
                if let Some(gui) = &mut gui {
                    gui.close();
                }
                tasks.allow_shutdown();
            }
            tasks.run_pending();
            if let Some(gui) = &mut gui {
                let open = gui.poll();
                if close_requested.load(Ordering::Acquire) && !open && !did_reopen {
                    closed.store(true, Ordering::Release);
                    assert!(gui.open(), "editor could not reopen after native close");
                    reopened.store(true, Ordering::Release);
                    did_reopen = true;
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        thread.join().unwrap();
        std::env::remove_var("CADENCE_CLAP_REQUEST_LOG");
        println!(
            "PASS CLAP main-thread callbacks, idle/active flush, restart, state and {}",
            if with_gui {
                "native editor close/reopen"
            } else {
                "headless hosting"
            }
        );
    }
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("SKIP Cocoa main-thread regression: macOS only");
}
