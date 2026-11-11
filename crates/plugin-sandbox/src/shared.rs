//! Shared-memory block exchanged between the host and a worker process.

use crate::SandboxError;
use plugin_api::wire::WireEvent;
use std::ffi::CString;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

pub const MAGIC: u32 = 0x4B53_424C; // "KSBL"
pub const VERSION: u32 = 11;
pub const MAX_CHANNELS: usize = 8;
pub const MAX_BLOCK: usize = 4096;
pub const MAX_EVENTS: usize = 256;
pub const MAX_STATE: usize = 65_536;
pub const MAX_PARAM_SNAPSHOT: usize = 512;
pub const MAX_OUTPUT_SYSEX: usize = 16384;

pub const BLOCK_IDLE: u32 = 0;
pub const BLOCK_READY: u32 = 1;
pub const BLOCK_DONE: u32 = 2;
pub const PREPARE: u32 = 3;
pub const RESET: u32 = 4;
pub const SAVE: u32 = 5;
pub const LOAD: u32 = 6;
pub const ERROR: u32 = 7;

#[repr(C)]
pub struct SharedBlock {
    pub magic: u32,
    pub version: u32,
    pub block_state: AtomicU32,
    pub frames: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub sidechain_channels: u32,
    pub event_count: u32,
    pub state_len: u32,
    pub state_schema: u32,
    pub sample_rate: f64,
    pub block_size: u32,
    pub tempo: f64,
    pub flags: u32,
    pub sample_pos: i64,
    pub ppq_position: f64,
    pub inputs: [f32; MAX_CHANNELS * MAX_BLOCK],
    pub outputs: [f32; MAX_CHANNELS * MAX_BLOCK],
    pub sidechain: [f32; MAX_CHANNELS * MAX_BLOCK],
    pub events: [WireEvent; MAX_EVENTS],
    pub output_event_count: u32,
    pub output_sysex_len: u32,
    pub output_events: [WireEvent; MAX_EVENTS],
    pub output_sysex: [u8; MAX_OUTPUT_SYSEX],
    pub state: [u8; MAX_STATE],
}

pub const PIPELINE_SLOTS: usize = 8;
pub const SLOT_EMPTY: u32 = 0;
pub const SLOT_READY: u32 = 1;
pub const SLOT_BUSY: u32 = 2;
pub const SLOT_DONE: u32 = 3;
pub const SLOT_WRITING: u32 = 4;

#[repr(C)]
pub struct AudioPacket {
    pub sequence: u64,
    pub stream_start: u64,
    pub epoch: u64,
    pub context: plugin_api::wire::WireProcessContext,
    pub frames: u32,
    pub input_channels: u32,
    pub output_channels: u32,
    pub sidechain_channels: u32,
    pub event_count: u32,
    pub events: [WireEvent; MAX_EVENTS],
    pub output_event_count: u32,
    pub output_sysex_len: u32,
    pub output_events: [WireEvent; MAX_EVENTS],
    pub output_sysex: [u8; MAX_OUTPUT_SYSEX],
    pub inputs: [f32; MAX_CHANNELS * MAX_BLOCK],
    pub outputs: [f32; MAX_CHANNELS * MAX_BLOCK],
    pub sidechain: [f32; MAX_CHANNELS * MAX_BLOCK],
}
#[repr(C)]
pub struct AudioSlot {
    pub state: AtomicU32,
    packet: std::cell::UnsafeCell<AudioPacket>,
}
impl AudioSlot {
    /// Caller must own EMPTY/WRITING (producer), BUSY (worker), or DONE (consumer).
    pub fn packet_ptr(&self) -> *mut AudioPacket {
        self.packet.get()
    }
}
#[repr(C)]
struct SharedRegion {
    published: std::sync::atomic::AtomicU64,
    completed: std::sync::atomic::AtomicU64,
    epoch: std::sync::atomic::AtomicU64,
    error: AtomicU32,
    latency: AtomicU32,
    realtime_scheduling: AtomicU32,
    underruns: std::sync::atomic::AtomicU64,
    late_frames: std::sync::atomic::AtomicU64,
    control: std::cell::UnsafeCell<SharedBlock>,
    slots: [AudioSlot; PIPELINE_SLOTS],
    // Appended so the control header remains at its previous offset; version
    // checks still reject older helpers explicitly before any audio exchange.
    telemetry: crate::telemetry::AtomicStreamingTelemetry,
    workgroup_generation: AtomicU32,
    workgroup_present: AtomicU32,
    workgroup_joined: AtomicU32,
    plugin_latency: AtomicU32,
    plugin_tail: AtomicU32,
    parameter_generation: AtomicU64,
    parameter_count: AtomicU32,
    parameter_ids: [AtomicU32; MAX_PARAM_SNAPSHOT],
    parameter_values: [AtomicU32; MAX_PARAM_SNAPSHOT],
    reload_requested: AtomicU32,
    gui_request: AtomicU32,
    gui_open: AtomicU32,
    gui_ready: AtomicU32,
    gui_stop: AtomicU32,
}
struct MappingStorage {
    pointer: *mut SharedRegion,
    size: usize,
    name: CString,
    owner: bool,
    wake: *mut libc::sem_t,
}
// Only PipelineMonitor crosses Rust threads; it exposes atomics, never payloads.
// All payload access is exclusive and synchronized by the slot/control protocol.
unsafe impl Send for MappingStorage {}
unsafe impl Sync for MappingStorage {}
impl Drop for MappingStorage {
    fn drop(&mut self) {
        unsafe {
            libc::munmap(self.pointer.cast(), self.size);
            libc::sem_close(self.wake);
            if self.owner {
                libc::shm_unlink(self.name.as_ptr());
                libc::sem_unlink(self.name.as_ptr());
            }
        }
    }
}
#[derive(Clone)]
pub struct PipelineMonitor {
    storage: std::sync::Arc<MappingStorage>,
}
impl PipelineMonitor {
    /// Main/control thread only: wake idle worker housekeeping after plugin
    /// requests or main-thread snapshot updates. Realtime host callbacks use
    /// the existing packet notification path.
    pub fn notify_worker(&self) {
        unsafe {
            libc::sem_post(self.storage.wake);
        }
    }
    fn region(&self) -> &SharedRegion {
        unsafe { &*self.storage.pointer }
    }
    /// At most three atomic snapshot attempts. `None` means telemetry is
    /// unavailable or publication is in progress; polling never waits/resets.
    pub fn streaming_telemetry(&self) -> Option<plugin_api::StreamingTelemetry> {
        self.region().telemetry.snapshot()
    }
    pub fn published(&self) -> u64 {
        self.region().published.load(Ordering::Acquire)
    }
    pub fn completed(&self) -> u64 {
        self.region().completed.load(Ordering::Acquire)
    }
    pub fn error(&self) -> bool {
        self.region().error.load(Ordering::Acquire) != 0
    }
    pub fn latency(&self) -> u32 {
        self.region().latency.load(Ordering::Acquire)
    }
    pub fn scheduling_flags(&self) -> u32 {
        self.region().realtime_scheduling.load(Ordering::Acquire)
    }
    pub fn realtime_scheduling(&self) -> bool {
        self.scheduling_flags() & 1 != 0
    }
    pub fn workgroup_joined(&self) -> bool {
        self.scheduling_flags() & 2 != 0
    }
    pub fn workgroup_generation(&self) -> u32 {
        self.region().workgroup_generation.load(Ordering::Acquire)
    }
    pub fn workgroup_present(&self) -> bool {
        self.region().workgroup_present.load(Ordering::Acquire) != 0
    }
    pub fn set_workgroup_available(&self, generation: u32, present: bool) {
        self.region()
            .workgroup_present
            .store(u32::from(present), Ordering::Release);
        self.region()
            .workgroup_generation
            .store(generation, Ordering::Release);
    }
    pub fn reload_requested(&self) -> bool {
        self.region().reload_requested.load(Ordering::Acquire) != 0
    }
    pub fn request_reload(&self) {
        self.region().reload_requested.store(1, Ordering::Release);
    }
    pub fn set_gui_ready(&self, ready: bool) {
        self.region()
            .gui_ready
            .store(u32::from(ready), Ordering::Release);
    }
    pub fn gui_ready(&self) -> bool {
        self.region().gui_ready.load(Ordering::Acquire) != 0
    }
    pub fn set_gui_open(&self, open: bool) {
        self.region()
            .gui_open
            .store(u32::from(open), Ordering::Release);
    }
    pub fn gui_open(&self) -> bool {
        self.region().gui_open.load(Ordering::Acquire) != 0
    }
    pub fn gui_request(&self) -> u32 {
        self.region().gui_request.load(Ordering::Acquire)
    }
    pub fn set_gui_request(&self, request: u32) {
        self.region().gui_request.store(request, Ordering::Release);
    }
    pub fn set_gui_stop(&self, stop: bool) {
        self.region()
            .gui_stop
            .store(u32::from(stop), Ordering::Release);
    }
    pub fn gui_stop(&self) -> bool {
        self.region().gui_stop.load(Ordering::Acquire) != 0
    }
    pub fn plugin_latency(&self) -> u32 {
        self.region().plugin_latency.load(Ordering::Acquire)
    }
    pub fn plugin_tail(&self) -> u32 {
        self.region().plugin_tail.load(Ordering::Acquire)
    }
    pub fn set_plugin_timing(&self, latency: u32, tail: u32) {
        self.region()
            .plugin_latency
            .store(latency, Ordering::Release);
        self.region().plugin_tail.store(tail, Ordering::Release);
    }
    pub fn parameter_generation(&self) -> u64 {
        self.region().parameter_generation.load(Ordering::Acquire)
    }
    /// Seqlock publication from the worker; readers never block.
    pub fn publish_parameter_values(&self, parameters: &[(u32, f32)]) {
        let region = self.region();
        let before = region.parameter_generation.load(Ordering::Relaxed);
        let odd = if before & 1 == 0 {
            before + 1
        } else {
            before + 2
        };
        region.parameter_generation.store(odd, Ordering::Release);
        let count = parameters.len().min(MAX_PARAM_SNAPSHOT);
        region
            .parameter_count
            .store(count as u32, Ordering::Relaxed);
        for (index, (id, value)) in parameters.iter().take(count).enumerate() {
            region.parameter_ids[index].store(*id, Ordering::Relaxed);
            region.parameter_values[index].store(value.to_bits(), Ordering::Relaxed);
        }
        region
            .parameter_generation
            .store(odd + 1, Ordering::Release);
    }
    /// Bounded coherent read. Returns the count and generation, or None while a
    /// publication is in progress or after three unstable attempts.
    pub fn parameter_snapshot(&self, ids: &mut [u32], values: &mut [f32]) -> Option<(usize, u64)> {
        let region = self.region();
        for _ in 0..3 {
            let before = region.parameter_generation.load(Ordering::Acquire);
            if before & 1 != 0 {
                continue;
            }
            let count = region.parameter_count.load(Ordering::Acquire) as usize;
            if count > ids.len().min(values.len()).min(MAX_PARAM_SNAPSHOT) {
                return None;
            }
            for index in 0..count {
                ids[index] = region.parameter_ids[index].load(Ordering::Relaxed);
                values[index] =
                    f32::from_bits(region.parameter_values[index].load(Ordering::Relaxed));
            }
            if region.parameter_generation.load(Ordering::Acquire) == before {
                return Some((count, before));
            }
        }
        None
    }
    pub fn underruns(&self) -> u64 {
        self.region().underruns.load(Ordering::Relaxed)
    }
    pub fn late_frames(&self) -> u64 {
        self.region().late_frames.load(Ordering::Relaxed)
    }
}
pub struct SharedMapping {
    storage: std::sync::Arc<MappingStorage>,
    // The control/payload owner must not be shared between Rust threads.
    not_sync: std::marker::PhantomData<std::cell::Cell<()>>,
}
impl SharedMapping {
    /// Sole worker writer only; independent of audio/control payload ownership.
    pub fn publish_streaming_telemetry(&self, value: Option<plugin_api::StreamingTelemetry>) {
        self.region().telemetry.publish(value);
    }
    pub fn set_realtime_scheduling(&self, enabled: bool) {
        if enabled {
            self.region()
                .realtime_scheduling
                .fetch_or(1, Ordering::AcqRel);
        } else {
            self.region()
                .realtime_scheduling
                .fetch_and(!1, Ordering::AcqRel);
        }
    }
    pub fn set_workgroup_joined(&self, enabled: bool) {
        if enabled {
            self.region()
                .realtime_scheduling
                .fetch_or(2, Ordering::AcqRel);
        } else {
            self.region()
                .realtime_scheduling
                .fetch_and(!2, Ordering::AcqRel);
        }
    }
    /// Publish availability before the generation so a watcher that observes
    /// the new generation also observes the matching presence flag.
    pub fn set_workgroup_available(&self, generation: u32, present: bool) {
        self.monitor().set_workgroup_available(generation, present);
    }
    pub fn set_plugin_timing(&self, latency: u32, tail: u32) {
        self.monitor().set_plugin_timing(latency, tail);
    }
    pub fn set_gui_request(&self, request: u32) {
        self.monitor().set_gui_request(request);
    }
    pub fn set_gui_stop(&self, stop: bool) {
        self.monitor().set_gui_stop(stop);
    }
    pub fn request_reload(&self) {
        self.monitor().request_reload();
    }
    pub fn publish_parameter_values(&self, parameters: &[(u32, f32)]) {
        self.monitor().publish_parameter_values(parameters);
    }
    /// Nonblocking notification: no allocation, mutex or wait on the producer.
    pub fn notify(&self) -> bool {
        unsafe { libc::sem_post(self.storage.wake) == 0 }
    }
    /// Worker thread only. Counting retains notifications arriving before sleep.
    pub fn wait_for_work(&self) -> Result<(), SandboxError> {
        loop {
            if unsafe { libc::sem_wait(self.storage.wake) } == 0 {
                return Ok(());
            }
            if std::io::Error::last_os_error().kind() != std::io::ErrorKind::Interrupted {
                return Err(SandboxError::SharedMemory(last_error()));
            }
        }
    }
    fn region(&self) -> &SharedRegion {
        unsafe { &*self.storage.pointer }
    }
    pub fn monitor(&self) -> PipelineMonitor {
        PipelineMonitor {
            storage: self.storage.clone(),
        }
    }
    pub fn slot(&self, index: usize) -> &AudioSlot {
        &self.region().slots[index % PIPELINE_SLOTS]
    }
    pub fn publish(&self, sequence: u64) {
        self.region().published.store(sequence, Ordering::Release);
    }
    pub fn complete(&self, sequence: u64) {
        self.region().completed.store(sequence, Ordering::Release);
    }
    pub fn epoch(&self) -> u64 {
        self.region().epoch.load(Ordering::Acquire)
    }
    pub fn set_epoch(&self, epoch: u64) {
        self.region().epoch.store(epoch, Ordering::Release);
    }
    pub fn pipeline_error(&self) {
        self.region().error.store(1, Ordering::Release);
    }
    pub fn pipeline_has_error(&self) -> bool {
        self.region().error.load(Ordering::Acquire) != 0
    }
    pub fn published(&self) -> u64 {
        self.region().published.load(Ordering::Acquire)
    }
    pub fn completed(&self) -> u64 {
        self.region().completed.load(Ordering::Acquire)
    }
    pub fn set_latency(&self, latency: u32) {
        self.region().latency.store(latency, Ordering::Release);
    }
    pub fn reset_stats(&self) {
        self.region().underruns.store(0, Ordering::Relaxed);
        self.region().late_frames.store(0, Ordering::Relaxed);
    }
    pub fn underrun(&self, frames: u64) {
        if frames > 0 {
            self.region().underruns.fetch_add(1, Ordering::Relaxed);
            self.region()
                .late_frames
                .fetch_add(frames, Ordering::Relaxed);
        }
    }
    pub fn create() -> Result<Self, SandboxError> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let name = CString::new(format!(
            "/ks-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ))
        .map_err(|_| SandboxError::SharedMemory("invalid shared-memory name".into()))?;
        let size = std::mem::size_of::<SharedRegion>();
        let fd = unsafe {
            libc::shm_open(
                name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        if fd < 0 {
            return Err(SandboxError::SharedMemory(last_error()));
        }
        if unsafe { libc::ftruncate(fd, size as libc::off_t) } != 0 {
            let error = last_error();
            unsafe {
                libc::close(fd);
                libc::shm_unlink(name.as_ptr());
            }
            return Err(SandboxError::SharedMemory(error));
        }
        Self::map(fd, name, true, true)
    }
    pub fn open(name: &str) -> Result<Self, SandboxError> {
        let name = CString::new(name)
            .map_err(|_| SandboxError::SharedMemory("invalid shared-memory name".into()))?;
        let fd = unsafe { libc::shm_open(name.as_ptr(), libc::O_RDWR, 0) };
        if fd < 0 {
            return Err(SandboxError::SharedMemory(last_error()));
        }
        let mut stat = std::mem::MaybeUninit::<libc::stat>::uninit();
        if unsafe { libc::fstat(fd, stat.as_mut_ptr()) } != 0
            || unsafe { stat.assume_init() }.st_size
                < std::mem::size_of::<SharedRegion>() as libc::off_t
        {
            unsafe {
                libc::close(fd);
            }
            return Err(SandboxError::SharedMemory(
                "shared region is too small or unavailable".into(),
            ));
        }
        Self::map(fd, name, false, false)
    }
    fn map(fd: i32, name: CString, owner: bool, initialize: bool) -> Result<Self, SandboxError> {
        let size = std::mem::size_of::<SharedRegion>();
        let pointer = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                size,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        unsafe {
            libc::close(fd);
        }
        if pointer == libc::MAP_FAILED {
            let message = last_error();
            if owner {
                unsafe {
                    libc::shm_unlink(name.as_ptr());
                }
            }
            return Err(SandboxError::SharedMemory(message));
        }
        let wake = if owner {
            unsafe {
                libc::sem_open(
                    name.as_ptr(),
                    libc::O_CREAT | libc::O_EXCL,
                    0o600 as libc::c_uint,
                    0u32,
                )
            }
        } else {
            unsafe { libc::sem_open(name.as_ptr(), 0) }
        };
        if wake == libc::SEM_FAILED {
            let message = last_error();
            unsafe {
                libc::munmap(pointer, size);
                if owner {
                    libc::shm_unlink(name.as_ptr());
                }
            }
            return Err(SandboxError::SharedMemory(message));
        }
        let mut mapping = Self {
            storage: std::sync::Arc::new(MappingStorage {
                pointer: pointer.cast(),
                size,
                name,
                owner,
                wake,
            }),
            not_sync: std::marker::PhantomData,
        };
        if initialize {
            unsafe {
                std::ptr::write_bytes(pointer.cast::<u8>(), 0, size);
            }
            let control = mapping.block_mut();
            control.magic = MAGIC;
            control.version = VERSION;
        }
        if mapping.block().magic != MAGIC || mapping.block().version != VERSION {
            return Err(SandboxError::SharedMemory(
                "shared block magic/version mismatch".into(),
            ));
        }
        Ok(mapping)
    }
    pub fn name(&self) -> &str {
        self.storage.name.to_str().unwrap_or("")
    }
    pub fn unlink_name(&self) {
        unsafe {
            libc::shm_unlink(self.storage.name.as_ptr());
            libc::sem_unlink(self.storage.name.as_ptr());
        }
    }
    pub fn block(&self) -> &SharedBlock {
        unsafe { &*self.region().control.get() }
    }
    pub fn block_mut(&mut self) -> &mut SharedBlock {
        unsafe { &mut *self.region().control.get() }
    }
}
fn last_error() -> String {
    std::io::Error::last_os_error().to_string()
}

#[cfg(test)]
mod telemetry_tests {
    use super::*;
    use plugin_api::StreamingTelemetry;
    #[test]
    fn independent_mappings_publish_telemetry_without_touching_pipeline_counters() {
        let owner = SharedMapping::create().unwrap();
        let worker = SharedMapping::open(owner.name()).unwrap();
        let monitor = owner.monitor();
        assert_eq!(monitor.streaming_telemetry(), None);
        owner.underrun(64);
        owner.publish(8);
        owner.complete(6);
        owner.set_latency(2048);
        let value = StreamingTelemetry {
            resident_bytes: u64::MAX,
            cache_capacity_bytes: 16 * 1024 * 1024,
            active_voices: 42,
            voice_capacity: 64,
            pending_reads: 256,
            flags: 0x81,
            ..Default::default()
        };
        worker.publish_streaming_telemetry(Some(value));
        assert_eq!(monitor.streaming_telemetry(), Some(value));
        assert_eq!(monitor.underruns(), 1);
        assert_eq!(monitor.late_frames(), 64);
        assert_eq!(monitor.published(), 8);
        assert_eq!(monitor.completed(), 6);
        assert_eq!(monitor.latency(), 2048);
        owner.reset_stats();
        assert_eq!(monitor.streaming_telemetry(), Some(value));
        worker.publish_streaming_telemetry(None);
        assert_eq!(monitor.streaming_telemetry(), None);
    }
    #[test]
    fn helpers_reject_previous_shared_layout_version() {
        let mut owner = SharedMapping::create().unwrap();
        assert_eq!(owner.block().version, 11);
        owner.block_mut().version = 10;
        assert!(SharedMapping::open(owner.name()).is_err());
    }
}
