//! Per-instance CLAP host requests and creator-thread dispatch. Realtime calls
//! only touch atomics. Queued operations are exclusively non-realtime lifecycle
//! and state operations, not audio processing or parameter automation.

use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, AtomicPtr, AtomicU32, AtomicUsize};
use std::sync::{mpsc, Arc, Mutex, OnceLock};
use std::thread::{self, ThreadId};

struct Task {
    run: Box<dyn FnOnce() + Send>,
    teardown: bool,
}

pub(crate) struct HostState {
    owner: ThreadId,
    plugin: AtomicPtr<clap_plugin>,
    tasks: Mutex<VecDeque<Task>>,
    pub callback: AtomicBool,
    pub restart: AtomicBool,
    pub process: AtomicBool,
    pub flush: AtomicBool,
    pub reload: AtomicBool,
    pub refresh: AtomicBool,
    pub values_ready: AtomicBool,
    pub shutting_down: AtomicBool,
    shutdown_ready: AtomicBool,
    pub timing_dirty: AtomicBool,
    worker_notification: AtomicBool,
    timing_changed: AtomicBool,
    pub latency: AtomicUsize,
    pub tail: AtomicUsize,
    parameters: OnceLock<Vec<(u32, f32, f32)>>,
    values: Box<[AtomicU32]>,
    generation: AtomicU32,
}

impl HostState {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            owner: thread::current().id(),
            plugin: AtomicPtr::new(ptr::null_mut()),
            tasks: Mutex::new(VecDeque::new()),
            callback: AtomicBool::new(false),
            restart: AtomicBool::new(false),
            process: AtomicBool::new(false),
            flush: AtomicBool::new(false),
            reload: AtomicBool::new(false),
            refresh: AtomicBool::new(false),
            values_ready: AtomicBool::new(false),
            shutting_down: AtomicBool::new(false),
            shutdown_ready: AtomicBool::new(false),
            timing_dirty: AtomicBool::new(true),
            worker_notification: AtomicBool::new(false),
            timing_changed: AtomicBool::new(false),
            latency: AtomicUsize::new(0),
            tail: AtomicUsize::new(0),
            parameters: OnceLock::new(),
            values: (0..plugin_api::MAX_PARAMETERS)
                .map(|_| AtomicU32::new(f32::NAN.to_bits()))
                .collect(),
            generation: AtomicU32::new(0),
        })
    }

    pub fn attach(&self, plugin: *const clap_plugin, ids: &[u32], info: &[ParameterInfo]) {
        self.plugin.store(plugin.cast_mut(), Ordering::Release);
        let _ = self.parameters.set(
            ids.iter()
                .zip(info)
                .map(|(id, p)| (*id, p.min, p.max))
                .collect(),
        );
        self.refresh.store(true, Ordering::Release);
        self.run_pending();
    }

    pub fn is_owner(&self) -> bool {
        self.owner == thread::current().id()
    }

    /// Caller must be outside process(). The worker supervisor bounds a hung
    /// third-party operation and terminates the process; never time out and let
    /// a queued operation use a plugin that the caller has already destroyed.
    pub fn call<R: Send + 'static>(&self, task: impl FnOnce() -> R + Send + 'static) -> R {
        self.call_mode(task, false)
    }

    pub fn teardown(&self, task: impl FnOnce() + Send + 'static) {
        self.call_mode(task, true);
    }

    fn call_mode<R: Send + 'static>(
        &self,
        task: impl FnOnce() -> R + Send + 'static,
        teardown: bool,
    ) -> R {
        if self.is_owner() {
            return task();
        }
        let (send, recv) = mpsc::sync_channel(1);
        self.tasks.lock().unwrap().push_back(Task {
            run: Box::new(move || {
                let _ = send.send(task());
            }),
            teardown,
        });
        recv.recv().expect("CLAP main-thread dispatcher stopped")
    }

    pub fn detach(&self) {
        self.plugin.store(ptr::null_mut(), Ordering::Release);
    }

    pub fn snapshot(&self, out: &mut [(u32, f32)]) -> usize {
        let count = self.parameters.get().map_or(0, Vec::len).min(out.len());
        for _ in 0..3 {
            let before = self.generation.load(Ordering::Acquire);
            if before & 1 != 0 {
                continue;
            }
            let mut written = 0;
            for index in 0..count {
                let value = f32::from_bits(self.values[index].load(Ordering::Relaxed));
                if value.is_finite() {
                    out[written] = (index as u32, value);
                    written += 1;
                }
            }
            std::sync::atomic::fence(Ordering::Acquire);
            if self.generation.load(Ordering::Relaxed) == before {
                return written;
            }
        }
        self.values_ready.store(true, Ordering::Release);
        0
    }

    fn update(&self, plugin: *const clap_plugin) {
        if self.refresh.swap(false, Ordering::AcqRel) {
            self.worker_notification.store(true, Ordering::Release);
            let params =
                unsafe { (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_PARAMS.as_ptr()) };
            if !params.is_null() {
                if let Some(get) = unsafe { (*(params as *const clap_plugin_params)).get_value } {
                    self.generation.fetch_add(1, Ordering::AcqRel);
                    if let Some(parameters) = self.parameters.get() {
                        for (index, (id, min, max)) in parameters.iter().enumerate() {
                            let mut value = 0.0;
                            let valid =
                                unsafe { get(plugin, *id, &mut value) } && value.is_finite();
                            let normalized = if valid {
                                ((value as f32 - min) / (max - min)).clamp(0.0, 1.0)
                            } else {
                                f32::NAN
                            };
                            self.values[index].store(normalized.to_bits(), Ordering::Relaxed);
                        }
                    }
                    self.generation.fetch_add(1, Ordering::Release);
                    self.values_ready.store(true, Ordering::Release);
                }
            }
        }
        if self.timing_dirty.swap(false, Ordering::AcqRel) {
            self.worker_notification.store(true, Ordering::Release);
            unsafe {
                let get_extension = (*plugin).get_extension.unwrap();
                let latency = get_extension(plugin, CLAP_EXT_LATENCY.as_ptr());
                if !latency.is_null() {
                    if let Some(get) = (*(latency as *const clap_plugin_latency)).get {
                        self.latency.store(get(plugin) as usize, Ordering::Release);
                    }
                }
                let tail = get_extension(plugin, CLAP_EXT_TAIL.as_ptr());
                if !tail.is_null() {
                    if let Some(get) = (*(tail as *const clap_plugin_tail)).get {
                        self.tail.store(get(plugin) as usize, Ordering::Release);
                    }
                }
            }
            // The audio thread may already be blocked waiting for work, so the
            // creator thread must publish the new timing itself.
            self.timing_changed.store(true, Ordering::Release);
        }
    }
}

impl plugin_api::MainThreadTasks for HostState {
    fn run_pending(&self) {
        assert!(
            self.is_owner(),
            "CLAP main-thread tasks called on the wrong thread"
        );
        // Drop the queue lock before entering plugin code. A callback can enqueue
        // further work; cap each turn so a plugin cannot starve the Cocoa loop.
        for _ in 0..16 {
            let task = self.tasks.lock().unwrap().pop_front();
            match task {
                Some(task) => {
                    if task.teardown && !self.shutdown_ready.load(Ordering::Acquire) {
                        self.tasks.lock().unwrap().push_front(task);
                        break;
                    }
                    (task.run)();
                }
                None => break,
            }
        }
        let plugin = self.plugin.load(Ordering::Acquire);
        if plugin.is_null() || self.shutting_down.load(Ordering::Acquire) {
            return;
        }
        if self.callback.swap(false, Ordering::AcqRel) {
            if let Some(callback) = unsafe { (*plugin).on_main_thread } {
                unsafe { callback(plugin) };
            }
        }
        self.update(plugin);
    }

    fn is_shutting_down(&self) -> bool {
        self.shutting_down.load(Ordering::Acquire)
    }

    fn take_worker_notification(&self) -> bool {
        self.worker_notification.swap(false, Ordering::AcqRel)
            || self.restart.load(Ordering::Acquire)
            || self.process.load(Ordering::Acquire)
            || self.flush.load(Ordering::Acquire)
            || self.reload.load(Ordering::Acquire)
    }

    fn take_timing(&self) -> Option<(u32, u32)> {
        self.timing_changed.swap(false, Ordering::AcqRel).then(|| {
            (
                self.latency.load(Ordering::Acquire).min(u32::MAX as usize) as u32,
                self.tail.load(Ordering::Acquire).min(u32::MAX as usize) as u32,
            )
        })
    }

    fn allow_shutdown(&self) {
        assert!(self.is_owner());
        self.shutdown_ready.store(true, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn host(state: &Arc<HostState>) -> clap_host {
        clap_host {
            clap_version: CLAP_VERSION,
            host_data: Arc::as_ptr(state) as *mut c_void,
            name: ptr::null(),
            vendor: ptr::null(),
            url: ptr::null(),
            version: ptr::null(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        }
    }

    #[test]
    fn requests_are_per_instance_and_thread_safe() {
        let first = HostState::new();
        let second = HostState::new();
        let first_host = host(&first);
        let second_host = host(&second);
        let address = &first_host as *const clap_host as usize;
        std::thread::spawn(move || unsafe {
            let host = address as *const clap_host;
            for _ in 0..10000 {
                host_request_callback(host);
                host_request_restart(host);
                host_request_process(host);
                host_params_request_flush(host);
            }
        })
        .join()
        .unwrap();
        assert!(first.callback.swap(false, Ordering::AcqRel));
        assert!(!first.callback.load(Ordering::Acquire));
        assert!(first.restart.load(Ordering::Acquire));
        assert!(first.process.load(Ordering::Acquire));
        assert!(first.flush.load(Ordering::Acquire));
        assert!(!second.callback.load(Ordering::Acquire));
        assert!(!second.take_worker_notification());
        unsafe {
            host_params_rescan(&second_host, CLAP_PARAM_RESCAN_ALL);
        }
        assert!(second.reload.load(Ordering::Acquire));
        assert!(!first.reload.load(Ordering::Acquire));
    }

    #[test]
    fn lifecycle_queue_runs_on_owner_without_holding_queue_lock() {
        let state = HostState::new();
        let owner = std::thread::current().id();
        let other = state.clone();
        let join = std::thread::spawn(move || {
            let nested = other.clone();
            other.call(move || {
                assert_eq!(owner, std::thread::current().id());
                nested.call(|| 42)
            })
        });
        while !join.is_finished() {
            state.run_pending();
            std::thread::yield_now();
        }
        assert_eq!(join.join().unwrap(), 42);
    }

    #[test]
    fn teardown_waits_for_native_view_close_acknowledgement() {
        let state = HostState::new();
        let other = state.clone();
        let ran = Arc::new(AtomicBool::new(false));
        let ran_other = ran.clone();
        let thread = std::thread::spawn(move || {
            other.shutting_down.store(true, Ordering::Release);
            other.teardown(move || ran_other.store(true, Ordering::Release));
        });
        while state.tasks.lock().unwrap().is_empty() {
            std::thread::yield_now();
        }
        state.run_pending();
        assert!(!ran.load(Ordering::Acquire));
        assert!(!thread.is_finished());
        state.allow_shutdown();
        state.run_pending();
        thread.join().unwrap();
        assert!(ran.load(Ordering::Acquire));
    }

    #[test]
    fn callbacks_coalesce_and_reentrant_requests_wait_until_next_turn() {
        struct CallbackData {
            host: *const clap_host,
            count: AtomicUsize,
        }
        unsafe extern "C" fn callback(plugin: *const clap_plugin) {
            let data = unsafe { &*((*plugin).plugin_data as *const CallbackData) };
            assert!(unsafe { host_is_main_thread(data.host) });
            if data.count.fetch_add(1, Ordering::AcqRel) == 0 {
                unsafe {
                    host_request_callback(data.host);
                }
            }
        }
        unsafe extern "C" fn extension(_: *const clap_plugin, _: *const c_char) -> *const c_void {
            ptr::null()
        }
        let state = HostState::new();
        let host = host(&state);
        let data = CallbackData {
            host: &host,
            count: AtomicUsize::new(0),
        };
        let plugin = clap_plugin {
            desc: ptr::null(),
            plugin_data: &data as *const _ as *mut c_void,
            init: None,
            destroy: None,
            activate: None,
            deactivate: None,
            start_processing: None,
            stop_processing: None,
            reset: None,
            process: None,
            get_extension: Some(extension),
            on_main_thread: Some(callback),
        };
        state.attach(&plugin, &[], &[]);
        for _ in 0..10000 {
            unsafe {
                host_request_callback(&host);
            }
        }
        state.run_pending();
        assert_eq!(data.count.load(Ordering::Acquire), 1);
        state.run_pending();
        assert_eq!(data.count.load(Ordering::Acquire), 2);
        state.run_pending();
        assert_eq!(data.count.load(Ordering::Acquire), 2);
        state.detach();
    }

    #[test]
    fn busy_parameter_snapshot_is_bounded_and_requests_retry() {
        let state = HostState::new();
        state.parameters.set(vec![(7, 0.0, 1.0)]).unwrap();
        state.generation.store(1, Ordering::Release);
        let mut out = [(0, 0.0)];
        assert_eq!(state.snapshot(&mut out), 0);
        assert!(state.values_ready.load(Ordering::Acquire));
        state.values[0].store(0.75f32.to_bits(), Ordering::Relaxed);
        state.generation.store(2, Ordering::Release);
        assert_eq!(state.snapshot(&mut out), 1);
        assert_eq!(out[0], (0, 0.75));
    }

    struct TimingData {
        latency: AtomicUsize,
    }

    unsafe extern "C" fn timing_latency_get(plugin: *const clap_plugin) -> u32 {
        let data = unsafe { &*((*plugin).plugin_data as *const TimingData) };
        data.latency.load(Ordering::Acquire) as u32
    }

    unsafe extern "C" fn timing_extension(
        _: *const clap_plugin,
        id: *const c_char,
    ) -> *const c_void {
        if id.is_null() {
            return ptr::null();
        }
        if unsafe { CStr::from_ptr(id) } == CLAP_EXT_LATENCY {
            (&TIMING_LATENCY as *const clap_plugin_latency).cast()
        } else {
            ptr::null()
        }
    }

    static TIMING_LATENCY: clap_plugin_latency = clap_plugin_latency {
        get: Some(timing_latency_get),
    };

    #[test]
    fn creator_thread_exposes_timing_for_an_idle_audio_thread() {
        let state = HostState::new();
        let host = host(&state);
        let data = TimingData {
            latency: AtomicUsize::new(0),
        };
        let plugin = clap_plugin {
            desc: ptr::null(),
            plugin_data: &data as *const _ as *mut c_void,
            init: None,
            destroy: None,
            activate: None,
            deactivate: None,
            start_processing: None,
            stop_processing: None,
            reset: None,
            process: None,
            get_extension: Some(timing_extension),
            on_main_thread: None,
        };
        state.attach(&plugin, &[], &[]);
        // The initial attach reads timing; drain it before changing latency.
        let _ = state.take_timing();

        data.latency.store(128, Ordering::Release);
        unsafe { host_changed(&host) };
        state.run_pending();
        assert_eq!(state.take_timing(), Some((128, 0)));
        state.run_pending();
        assert_eq!(state.take_timing(), None);
        state.detach();
    }
}
