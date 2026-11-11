//! Cross-process macOS Audio Workgroup membership.
//!
//! The host process owns the DAW's `os_workgroup_t`. A Mach send right for the
//! workgroup is published under a per-worker bootstrap service name. The worker
//! reconstructs the workgroup with `os_workgroup_create_with_port` and joins it
//! from its rendering thread. Service names are generation-suffixed because the
//! deprecated bootstrap registry cannot replace an existing name in place.
//!
//! All of this is macOS-only. Other platforms compile the no-op paths.
#![allow(deprecated)]

#[cfg(target_os = "macos")]
mod imp {
    use crate::shared::PipelineMonitor;
    use std::ffi::{c_char, c_void, CString};
    use std::sync::{
        atomic::{AtomicBool, AtomicPtr, Ordering},
        Arc,
    };
    use std::time::Duration;

    const BOOTSTRAP_SUCCESS: i32 = 0;

    #[repr(C)]
    #[derive(Clone, Copy)]
    pub struct JoinToken {
        sig: u32,
        opaque: [u8; 36],
    }
    impl Default for JoinToken {
        fn default() -> Self {
            Self {
                sig: 0,
                opaque: [0; 36],
            }
        }
    }

    extern "C" {
        static bootstrap_port: libc::mach_port_t;
        fn bootstrap_register(
            bootstrap_port: libc::mach_port_t,
            service_name: *const c_char,
            service_port: libc::mach_port_t,
        ) -> i32;
        fn bootstrap_look_up(
            bootstrap_port: libc::mach_port_t,
            service_name: *const c_char,
            service_port: *mut libc::mach_port_t,
        ) -> i32;
        fn mach_port_deallocate(task: libc::mach_port_t, name: libc::mach_port_t) -> i32;

        fn os_workgroup_copy_port(workgroup: *mut c_void, port: *mut u32) -> i32;
        fn os_workgroup_create_with_port(name: *const c_char, port: u32) -> *mut c_void;
        fn os_workgroup_join(workgroup: *mut c_void, token: *mut JoinToken) -> i32;
        fn os_workgroup_leave(workgroup: *mut c_void, token: *mut JoinToken);
    }

    fn service_name(base: &str, generation: u32) -> Option<CString> {
        CString::new(format!("{base}.{generation}")).ok()
    }

    /// Host side. Publishes a Mach send right for `workgroup` under `base.generation`.
    /// A null workgroup publishes an unavailable generation so workers leave.
    /// The caller must pass a valid retained `os_workgroup_t` or null.
    #[allow(clippy::not_unsafe_ptr_arg_deref)]
    pub fn publish(
        mapping: &crate::shared::PipelineMonitor,
        base: &str,
        workgroup: *mut c_void,
        generation: u32,
    ) -> bool {
        if workgroup.is_null() {
            mapping.set_workgroup_available(generation, false);
            return true;
        }
        let Some(name) = service_name(base, generation) else {
            return false;
        };
        let mut port = 0u32;
        if unsafe { os_workgroup_copy_port(workgroup, &mut port) } != 0 || port == 0 {
            mapping.set_workgroup_available(generation, false);
            return false;
        }
        let result = unsafe { bootstrap_register(bootstrap_port, name.as_ptr(), port) };
        unsafe {
            mach_port_deallocate(libc::mach_task_self(), port);
        }
        let registered = result == BOOTSTRAP_SUCCESS;
        mapping.set_workgroup_available(generation, registered);
        registered
    }

    /// Worker side. Watches the shared generation and keeps the current workgroup
    /// object alive; the rendering thread joins it.
    pub struct WorkerWorkgroup {
        current: AtomicPtr<c_void>,
        stop: AtomicBool,
    }
    impl WorkerWorkgroup {
        fn store(&self, workgroup: *mut c_void) {
            self.current.store(workgroup, Ordering::Release);
        }
        pub fn current(&self) -> *mut c_void {
            self.current.load(Ordering::Acquire)
        }
    }
    impl Drop for WorkerWorkgroup {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Release);
        }
    }

    pub fn watch(mapping: PipelineMonitor, base: String) -> Option<Arc<WorkerWorkgroup>> {
        let state = Arc::new(WorkerWorkgroup {
            current: AtomicPtr::new(std::ptr::null_mut()),
            stop: AtomicBool::new(false),
        });
        let thread_state = state.clone();
        std::thread::Builder::new()
            .name("plugin-workgroup".into())
            .spawn(move || {
                let mut last_generation = u32::MAX;
                let mut retained: Vec<*mut c_void> = Vec::new();
                while !thread_state.stop.load(Ordering::Acquire) {
                    let generation = mapping.workgroup_generation();
                    if generation != last_generation {
                        last_generation = generation;
                        let next = if mapping.workgroup_present() {
                            service_name(&base, generation).and_then(|name| {
                                let mut port = 0u32;
                                let result = unsafe {
                                    bootstrap_look_up(bootstrap_port, name.as_ptr(), &mut port)
                                };
                                if result != BOOTSTRAP_SUCCESS || port == 0 {
                                    return None;
                                }
                                let handle =
                                    unsafe { os_workgroup_create_with_port(name.as_ptr(), port) };
                                unsafe {
                                    mach_port_deallocate(libc::mach_task_self(), port);
                                }
                                (!handle.is_null()).then_some(handle)
                            })
                        } else {
                            None
                        };
                        let previous = thread_state.current();
                        if !previous.is_null() {
                            // Worker shutdown is process-local; retain every
                            // replaced object for this process's lifetime rather
                            // than releasing one a rendering thread may still use.
                            retained.push(previous);
                        }
                        thread_state.store(next.unwrap_or(std::ptr::null_mut()));
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                drop(retained);
            })
            .ok()?;
        Some(state)
    }

    /// Rendering-thread membership. Join and leave are real-time safe.
    pub struct Membership {
        handle: *mut c_void,
        token: JoinToken,
        joined: bool,
    }
    impl Default for Membership {
        fn default() -> Self {
            Self::new()
        }
    }
    impl Membership {
        pub fn new() -> Self {
            Self {
                handle: std::ptr::null_mut(),
                token: JoinToken::default(),
                joined: false,
            }
        }
        /// Returns true when the calling thread is currently a member.
        pub fn sync(&mut self, desired: *mut c_void) -> bool {
            if desired != self.handle {
                if self.joined {
                    unsafe { os_workgroup_leave(self.handle, &mut self.token) };
                    self.joined = false;
                }
                self.handle = desired;
            }
            if !self.handle.is_null() && !self.joined {
                let mut token = JoinToken::default();
                // Failed joins are retried on the next block; a cancelled
                // workgroup should not fail the audio callback.
                if unsafe { os_workgroup_join(self.handle, &mut token) } == 0 {
                    self.token = token;
                    self.joined = true;
                }
            }
            self.joined
        }
    }
    impl Drop for Membership {
        fn drop(&mut self) {
            if self.joined {
                unsafe { os_workgroup_leave(self.handle, &mut self.token) };
            }
        }
    }
}

#[cfg(target_os = "macos")]
pub use imp::{publish, watch, Membership, WorkerWorkgroup};

#[cfg(not(target_os = "macos"))]
mod imp {
    use crate::shared::PipelineMonitor;
    use std::sync::Arc;

    pub struct WorkerWorkgroup;
    impl WorkerWorkgroup {
        pub fn current(&self) -> *mut std::ffi::c_void {
            std::ptr::null_mut()
        }
    }
    pub fn watch(_: PipelineMonitor, _: String) -> Option<Arc<WorkerWorkgroup>> {
        None
    }
    pub struct Membership;
    impl Default for Membership {
        fn default() -> Self {
            Self::new()
        }
    }
    impl Membership {
        pub fn new() -> Self {
            Self
        }
        pub fn sync(&mut self, _: *mut std::ffi::c_void) -> bool {
            false
        }
    }
}

#[cfg(not(target_os = "macos"))]
pub use imp::{watch, Membership, WorkerWorkgroup};

#[cfg(not(target_os = "macos"))]
pub fn publish(
    _: &crate::shared::PipelineMonitor,
    _: &str,
    _: *mut std::ffi::c_void,
    _: u32,
) -> bool {
    false
}
