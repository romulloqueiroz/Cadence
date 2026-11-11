//! Worker-owned native CLAP editor host (macOS).
//!
//! The audio loop runs on a worker thread, leaving the process main thread for
//! Cocoa. Creator-thread tasks handle callbacks, activation, state and teardown;
//! the audio thread owns process/start/stop/reset and active parameter flushes.

#![cfg(target_os = "macos")]

use crate::shared::PipelineMonitor;
use clap_sys::ext::gui::*;
use clap_sys::plugin::clap_plugin;
use objc2::rc::{autoreleasepool, Retained};
use objc2::runtime::ProtocolObject;
use objc2::{define_class, msg_send, DefinedClass, MainThreadMarker, MainThreadOnly};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSEventMask, NSWindow,
    NSWindowDelegate, NSWindowStyleMask,
};
use objc2_foundation::{
    NSDate, NSDefaultRunLoopMode, NSObject, NSObjectProtocol, NSPoint, NSRect, NSSize, NSString,
};
use std::cell::Cell;
use std::ffi::{c_void, CString};
use std::ptr;
use std::sync::Arc;

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "CadenceClapWindowDelegate"]
    #[ivars = Cell<bool>]
    struct WindowDelegate;

    unsafe impl NSObjectProtocol for WindowDelegate {}
    unsafe impl NSWindowDelegate for WindowDelegate {
        #[unsafe(method(windowShouldClose:))]
        fn window_should_close(&self, _: &NSWindow) -> bool {
            // Defer plugin hide/destroy until Cocoa finishes dispatching the
            // click. The host owns the single close/destruction path.
            self.ivars().set(true);
            false
        }
    }
);

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    static kCFRunLoopDefaultMode: *const c_void;
    fn CFRunLoopRunInMode(mode: *const c_void, seconds: f64, return_after_source: bool) -> i32;
}

pub struct GuiContext {
    _app: Retained<NSApplication>,
    plugin: *const clap_plugin,
    gui: *const clap_plugin_gui,
    title: String,
    window: Option<Retained<NSWindow>>,
    delegate: Retained<WindowDelegate>,
    open: bool,
}

fn cocoa_api() -> CString {
    CString::new("cocoa").unwrap()
}

impl GuiContext {
    /// Must run on the process main thread after the plugin was created there.
    pub fn new(plugin: *mut c_void, title: String) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        if plugin.is_null() {
            return None;
        }
        let plugin = plugin as *const clap_plugin;
        let extension = unsafe {
            (*plugin)
                .get_extension
                .map(|get| get(plugin, CLAP_EXT_GUI.as_ptr()))
        }?;
        if extension.is_null() {
            return None;
        }
        let gui = extension as *const clap_plugin_gui;
        let supported = unsafe {
            (*gui)
                .is_api_supported
                .map(|check| check(plugin, cocoa_api().as_ptr(), false))
                .unwrap_or(false)
        };
        if !supported {
            return None;
        }
        let app = NSApplication::sharedApplication(mtm);
        let _ = app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        let delegate = WindowDelegate::alloc(mtm).set_ivars(Cell::new(false));
        let delegate = unsafe { msg_send![super(delegate), init] };
        Some(Self {
            _app: app,
            plugin,
            gui,
            title,
            window: None,
            delegate,
            open: false,
        })
    }

    pub fn open(&mut self) -> bool {
        if self.open {
            return true;
        }
        let mtm = match MainThreadMarker::new() {
            Some(mtm) => mtm,
            None => return false,
        };
        let created = unsafe {
            (*self.gui)
                .create
                .map(|create| create(self.plugin, cocoa_api().as_ptr(), false))
                .unwrap_or(false)
        };
        if !created {
            return false;
        }
        let mut width = 640u32;
        let mut height = 480u32;
        unsafe {
            if let Some(get_size) = (*self.gui).get_size {
                let _ = get_size(self.plugin, &mut width, &mut height);
            }
        }
        let style = NSWindowStyleMask::Titled
            | NSWindowStyleMask::Closable
            | NSWindowStyleMask::Miniaturizable;
        let rect = NSRect::new(
            NSPoint::new(200.0, 200.0),
            NSSize::new(f64::from(width.max(80)), f64::from(height.max(80))),
        );
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                style,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // Retained owns the window; Cocoa must not release it a second time.
        unsafe {
            window.setReleasedWhenClosed(false);
        }
        self.delegate.ivars().set(false);
        window.setDelegate(Some(ProtocolObject::from_ref(&*self.delegate)));
        window.setTitle(&NSString::from_str(&self.title));
        window.center();
        let Some(view) = window.contentView() else {
            let _ = unsafe { (*self.gui).destroy.map(|destroy| destroy(self.plugin)) };
            return false;
        };
        let cocoa = cocoa_api();
        let clap_window = clap_window {
            api: cocoa.as_ptr(),
            specific: clap_window_handle {
                cocoa: Retained::as_ptr(&view) as *mut c_void,
            },
        };
        let attached = unsafe {
            (*self.gui)
                .set_parent
                .map(|set_parent| set_parent(self.plugin, &clap_window))
                .unwrap_or(false)
        };
        if !attached {
            let _ = unsafe { (*self.gui).destroy.map(|destroy| destroy(self.plugin)) };
            return false;
        }
        let shown = unsafe {
            (*self.gui)
                .show
                .map(|show| show(self.plugin))
                .unwrap_or(false)
        };
        if !shown {
            let _ = unsafe { (*self.gui).destroy.map(|destroy| destroy(self.plugin)) };
            return false;
        }
        window.makeKeyAndOrderFront(None);
        self.window = Some(window);
        self.open = true;
        true
    }

    pub fn close(&mut self) {
        if !self.open {
            return;
        }
        unsafe {
            let _ = (*self.gui).hide.map(|hide| hide(self.plugin));
            let _ = (*self.gui).destroy.map(|destroy| destroy(self.plugin));
        }
        if let Some(window) = self.window.take() {
            window.setDelegate(None);
            window.orderOut(None);
            window.close();
        }
        self.open = false;
    }

    fn dispatch_events(&self) {
        // A bare CFRunLoop does not dispatch NSApplication mouse/key events.
        // Drain a bounded batch so input cannot starve host requests.
        for _ in 0..64 {
            let event = self._app.nextEventMatchingMask_untilDate_inMode_dequeue(
                NSEventMask::Any,
                Some(&NSDate::distantPast()),
                unsafe { NSDefaultRunLoopMode },
                true,
            );
            let Some(event) = event else {
                break;
            };
            self._app.sendEvent(&event);
        }
        self._app.updateWindows();
    }

    /// Service Cocoa input and finish a user-initiated title-bar close. Must be
    /// called on the creator thread; returns whether the native editor is open.
    pub fn poll(&mut self) -> bool {
        self.dispatch_events();
        if self.delegate.ivars().replace(false) {
            self.close();
        }
        self.open
    }
}

/// Prepare the main-thread GUI host. Returns None when the plugin has no Cocoa
/// GUI or the main thread cannot host Cocoa.
pub fn prepare(plugin: *mut c_void, title: String) -> Option<GuiContext> {
    GuiContext::new(plugin, title)
}

/// Run the GUI event loop until the audio thread stops or the parent exits.
pub fn run(
    mut context: Option<GuiContext>,
    tasks: Option<Arc<dyn plugin_api::MainThreadTasks>>,
    monitor: PipelineMonitor,
) {
    monitor.set_gui_ready(context.is_some());
    if context.is_some() && std::env::var("CADENCE_GUI_AUTO_OPEN").is_ok() {
        monitor.set_gui_request(1);
    }
    let mut last_request = 0u32;
    loop {
        let stopped = autoreleasepool(|_| {
            let user_closed = context.as_mut().is_some_and(|context| {
                let was_open = context.open;
                let is_open = context.poll();
                was_open && !is_open
            });
            let shutting_down = tasks.as_ref().is_some_and(|tasks| tasks.is_shutting_down());
            if shutting_down || user_closed {
                if let Some(context) = &mut context {
                    context.close();
                }
                monitor.set_gui_open(false);
                monitor.set_gui_request(0);
                if shutting_down {
                    if let Some(tasks) = &tasks {
                        tasks.allow_shutdown();
                    }
                }
            }
            let request = monitor.gui_request();
            if request != last_request {
                last_request = request;
                if request == 1 && !shutting_down {
                    let opened = context.as_mut().is_some_and(GuiContext::open);
                    monitor.set_gui_open(opened);
                } else if request == 0 || request == 2 {
                    if let Some(context) = &mut context {
                        context.close();
                    }
                    monitor.set_gui_open(false);
                }
            }
            if let Some(tasks) = &tasks {
                tasks.run_pending();
                if let Some((latency, tail)) = tasks.take_timing() {
                    // The audio thread can be idle while a main-thread callback
                    // reports new latency, so publish it from the creator thread.
                    monitor.set_plugin_timing(latency, tail);
                }
                if tasks.take_worker_notification() {
                    monitor.notify_worker();
                }
            }
            if monitor.gui_stop() {
                return true;
            }
            let start = std::time::Instant::now();
            unsafe {
                CFRunLoopRunInMode(kCFRunLoopDefaultMode, 0.002, false);
            }
            // A headless worker may have no CF sources, in which case RunInMode
            // returns immediately. Avoid spinning while still servicing requests.
            if let Some(remaining) =
                std::time::Duration::from_millis(2).checked_sub(start.elapsed())
            {
                std::thread::sleep(remaining);
            }
            false
        });
        if stopped {
            break;
        }
    }
    if let Some(context) = &mut context {
        context.close();
    }
    monitor.set_gui_ready(false);
    monitor.set_gui_open(false);
}

pub fn request_close(monitor: &PipelineMonitor) {
    monitor.set_gui_request(0);
}

#[allow(dead_code)]
pub fn null_handle() -> *mut c_void {
    ptr::null_mut()
}
