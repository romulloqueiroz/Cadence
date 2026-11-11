//! Host-side handle. All methods except process must run off the audio thread;
//! callers serialize lifecycle/state calls against process.
use crate::diagnostics::{self, FailureReason};
use crate::metadata::PluginMetadata;
use crate::shared::*;
use crate::SandboxError;
use plugin_api::wire::WireEvent;
use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, ParameterInfo, Plugin, ProcessContext, State,
    StateError,
};
use std::os::unix::process::CommandExt;
use std::{
    cell::Cell,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{
        atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Debug, Clone)]
pub struct SandboxConfig {
    /// Minimum stall timeout; a prepared audio buffer may require longer.
    pub block_timeout: Duration,
    pub startup_timeout: Duration,
    pub command_timeout: Duration,
    /// Set None to disable persistent diagnostics.
    pub diagnostic_directory: Option<PathBuf>,
}
impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            block_timeout: Duration::from_millis(20),
            startup_timeout: Duration::from_secs(15),
            command_timeout: Duration::from_secs(3),
            diagnostic_directory: diagnostics::default_directory(),
        }
    }
}

struct Watchdog {
    failed: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
    timeout_ns: Arc<AtomicU64>,
    reason: Arc<AtomicU32>,
    thread: Option<std::thread::JoinHandle<()>>,
}
impl Watchdog {
    fn start(
        pid: u32,
        monitor: PipelineMonitor,
        timeout: Duration,
        plugin_id: String,
        diagnostic_directory: Option<PathBuf>,
    ) -> Result<Self, std::io::Error> {
        let failed = Arc::new(AtomicBool::new(false));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_failed = failed.clone();
        let thread_stop = stop.clone();
        let timeout_ns = Arc::new(AtomicU64::new(
            timeout.as_nanos().min(u64::MAX as u128) as u64
        ));
        let thread_timeout = timeout_ns.clone();
        let reason = Arc::new(AtomicU32::new(0));
        let thread_reason = reason.clone();
        let thread = std::thread::Builder::new()
            .name("plugin-watchdog".into())
            .spawn(move || {
                let mut previous = monitor.completed();
                let mut pending_since = None;
                let mut recorded = false;
                while !thread_stop.load(Ordering::Acquire) {
                    let completed = monitor.completed();
                    let exit = diagnostics::exited(pid);
                    if exit.is_some() {
                        let _ = thread_reason.compare_exchange(
                            0,
                            FailureReason::WorkerExited as u32,
                            Ordering::AcqRel,
                            Ordering::Acquire,
                        );
                        thread_failed.store(true, Ordering::Release);
                    }
                    if monitor.published() > completed {
                        if previous != completed || pending_since.is_none() {
                            pending_since = Some(Instant::now());
                        }
                        let timeout = Duration::from_nanos(thread_timeout.load(Ordering::Acquire));
                        if pending_since.is_some_and(|time| time.elapsed() >= timeout)
                            || monitor.error()
                        {
                            // Progress may have arrived after the first snapshot.
                            if monitor.completed() == completed {
                                let cause = if monitor.error() {
                                    FailureReason::Protocol
                                } else {
                                    FailureReason::PipelineTimeout
                                };
                                let _ = thread_reason.compare_exchange(
                                    0,
                                    cause as u32,
                                    Ordering::AcqRel,
                                    Ordering::Acquire,
                                );
                                thread_failed.store(true, Ordering::Release);
                            }
                        }
                    } else {
                        pending_since = None;
                    }
                    previous = completed;
                    if thread_failed.load(Ordering::Acquire) {
                        diagnostics::record(
                            diagnostic_directory.as_deref(),
                            &plugin_id,
                            pid,
                            FailureReason::from_code(thread_reason.load(Ordering::Acquire)),
                            &monitor,
                            exit,
                        );
                        recorded = true;
                        // The PID has not been reaped by its owner yet, so it cannot
                        // have been recycled. This group was created just for it.
                        unsafe {
                            libc::kill(-(pid as i32), libc::SIGKILL);
                            libc::kill(pid as i32, libc::SIGKILL);
                        }
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(1));
                }
                if !recorded && thread_failed.load(Ordering::Acquire) {
                    diagnostics::record(
                        diagnostic_directory.as_deref(),
                        &plugin_id,
                        pid,
                        FailureReason::from_code(thread_reason.load(Ordering::Acquire)),
                        &monitor,
                        diagnostics::exited(pid),
                    );
                }
            })?;
        Ok(Self {
            failed,
            stop,
            timeout_ns,
            reason,
            thread: Some(thread),
        })
    }
    fn set_timeout(&self, timeout: Duration) {
        self.timeout_ns.store(
            timeout.as_nanos().min(u64::MAX as u128) as u64,
            Ordering::Release,
        );
    }
    fn request_stop(&self, reason: FailureReason) {
        let _ = self
            .reason
            .compare_exchange(0, reason as u32, Ordering::AcqRel, Ordering::Acquire);
        self.failed.store(true, Ordering::Release);
    }
    fn stop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
impl Drop for Watchdog {
    fn drop(&mut self) {
        self.stop();
    }
}

static WORKGROUP_COUNTER: AtomicU32 = AtomicU32::new(0);

#[derive(Clone)]
struct OutputGroup {
    events: [WireEvent; MAX_EVENTS],
    event_count: usize,
    sysex: [u8; MAX_OUTPUT_SYSEX],
    sysex_len: usize,
}
impl Default for OutputGroup {
    fn default() -> Self {
        Self {
            events: [WireEvent::default(); MAX_EVENTS],
            event_count: 0,
            sysex: [0; MAX_OUTPUT_SYSEX],
            sysex_len: 0,
        }
    }
}

pub struct SandboxPlugin {
    child: Child,
    watchdog: Watchdog,
    mapping: SharedMapping,
    timeline: crate::pipeline::Timeline,
    read_sequence: u64,
    metadata: PluginMetadata,
    descriptor: Descriptor,
    parameters: Vec<ParameterInfo>,
    failed: Cell<bool>,
    config: SandboxConfig,
    late_blocks: Cell<u64>,
    workgroup_base: String,
    output_groups: Vec<OutputGroup>,
    output_read: usize,
    output_write: usize,
    output_count: usize,
}

impl SandboxPlugin {
    pub fn spawn(worker: impl AsRef<Path>, plugin: impl AsRef<Path>) -> Result<Self, SandboxError> {
        Self::spawn_with_config(worker, plugin, SandboxConfig::default())
    }
    pub fn spawn_with_config(
        worker: impl AsRef<Path>,
        plugin: impl AsRef<Path>,
        config: SandboxConfig,
    ) -> Result<Self, SandboxError> {
        Self::spawn_selected(worker, plugin, None, config)
    }
    pub fn spawn_selected(
        worker: impl AsRef<Path>,
        plugin: impl AsRef<Path>,
        selector: Option<&str>,
        config: SandboxConfig,
    ) -> Result<Self, SandboxError> {
        Self::spawn_selected_cancellable(worker, plugin, selector, config, &|| false)
    }

    pub fn spawn_selected_cancellable(
        worker: impl AsRef<Path>,
        plugin: impl AsRef<Path>,
        selector: Option<&str>,
        config: SandboxConfig,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Self, SandboxError> {
        if cancelled() {
            return Err(SandboxError::Cancelled);
        }
        let mapping = SharedMapping::create()?;
        let workgroup_base = format!(
            "audio.cadence.wg.{}.{}",
            std::process::id(),
            WORKGROUP_COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let mut command = Command::new(worker.as_ref());
        command
            .arg(plugin.as_ref())
            .arg(mapping.name())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit())
            .env("CADENCE_SANDBOX_PARENT_PID", std::process::id().to_string())
            .env("CADENCE_WORKGROUP_SERVICE", &workgroup_base)
            .process_group(0);
        if let Some(selector) = selector {
            command.arg(selector);
        }
        let mut child = command
            .spawn()
            .map_err(|e| SandboxError::Spawn(e.to_string()))?;
        let deadline = Instant::now() + config.startup_timeout;
        let ready = loop {
            if cancelled() {
                break Err(SandboxError::Cancelled);
            }
            match mapping.block().block_state.load(Ordering::Acquire) {
                BLOCK_DONE => {
                    let block = mapping.block();
                    let length = block.state_len as usize;
                    if length > MAX_STATE {
                        break Err(SandboxError::Protocol("Metadata too large".into()));
                    }
                    break serde_json::from_slice::<PluginMetadata>(&block.state[..length])
                        .map_err(|e| SandboxError::Protocol(e.to_string()))
                        .and_then(|m| {
                            if m.is_valid() {
                                Ok(m)
                            } else {
                                Err(SandboxError::Protocol("Invalid plugin metadata".into()))
                            }
                        });
                }
                ERROR => break Err(SandboxError::Worker(shared_error(&mapping))),
                _ => {}
            }
            if Instant::now() >= deadline {
                break Err(SandboxError::Worker("Plugin startup timed out".into()));
            }
            match child.try_wait() {
                Ok(Some(_)) => {
                    break Err(SandboxError::Worker("Worker exited before ready".into()))
                }
                Err(e) => break Err(SandboxError::Io(e)),
                _ => {}
            }
            std::thread::sleep(Duration::from_millis(1));
        };
        match ready {
            Ok(metadata) => {
                mapping
                    .block()
                    .block_state
                    .store(BLOCK_IDLE, Ordering::Release);
                let descriptor = metadata.descriptor();
                let parameters = metadata.parameters();
                let watchdog = match Watchdog::start(
                    child.id(),
                    mapping.monitor(),
                    config.block_timeout,
                    metadata.id.clone(),
                    config.diagnostic_directory.clone(),
                ) {
                    Ok(watchdog) => watchdog,
                    Err(error) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err(SandboxError::Io(error));
                    }
                };
                Ok(Self {
                    child,
                    watchdog,
                    mapping,
                    timeline: crate::pipeline::Timeline::new(),
                    read_sequence: 0,
                    metadata,
                    descriptor,
                    parameters,
                    failed: Cell::new(false),
                    config,
                    late_blocks: Cell::new(0),
                    workgroup_base,
                    output_groups: vec![OutputGroup::default(); PIPELINE_SLOTS * 2],
                    output_read: 0,
                    output_write: 0,
                    output_count: 0,
                })
            }
            Err(error) => {
                if !matches!(error, SandboxError::Cancelled) {
                    diagnostics::record(
                        config.diagnostic_directory.as_deref(),
                        "startup",
                        child.id(),
                        FailureReason::Startup,
                        &mapping.monitor(),
                        diagnostics::exited(child.id()),
                    );
                }
                unsafe {
                    libc::kill(-(child.id() as i32), libc::SIGKILL);
                }
                let _ = child.kill();
                let _ = child.wait();
                Err(error)
            }
        }
    }
    fn fail(&self, reason: FailureReason) {
        self.failed.set(true);
        self.watchdog.request_stop(reason);
    }
    pub fn metadata(&self) -> &PluginMetadata {
        &self.metadata
    }
    pub fn has_failed(&self) -> bool {
        self.failed.get()
            || self.watchdog.failed.load(Ordering::Acquire)
            || self.mapping.pipeline_has_error()
    }
    pub fn late_blocks(&self) -> u64 {
        self.late_blocks.get()
    }
    pub fn worker_id(&self) -> u32 {
        self.child.id()
    }
    /// Non-audio control path. `workgroup` is an `os_workgroup_t` borrowed for
    /// the duration of this call; a null value removes availability.
    pub fn set_audio_workgroup(&self, workgroup: *mut std::ffi::c_void, generation: u32) -> bool {
        let monitor = self.mapping.monitor();
        crate::workgroup::publish(&monitor, &self.workgroup_base, workgroup, generation)
    }
    pub fn workgroup_base(&self) -> &str {
        &self.workgroup_base
    }

    pub fn pipeline_monitor(&self) -> PipelineMonitor {
        self.mapping.monitor()
    }
    pub fn failure_reason(&self) -> FailureReason {
        FailureReason::from_code(self.watchdog.reason.load(Ordering::Acquire))
    }
    pub fn failure_reason_flag(&self) -> Arc<AtomicU32> {
        self.watchdog.reason.clone()
    }

    pub fn failure_flag(&self) -> Arc<AtomicBool> {
        self.watchdog.failed.clone()
    }
    pub fn pipeline_latency(&self) -> usize {
        self.timeline.latency
    }

    /// Lifecycle/state only: the audio callback never calls this wait.
    fn wait_pipeline(&self) -> Result<(), SandboxError> {
        let deadline = Instant::now() + self.config.command_timeout;
        loop {
            if self.has_failed() {
                return Err(SandboxError::Worker("Worker has failed".into()));
            }
            if self.mapping.completed() == self.mapping.published() {
                return Ok(());
            }
            if Instant::now() >= deadline {
                self.fail(FailureReason::DrainTimeout);
                return Err(SandboxError::Worker("Pipeline drain timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    fn push_output_group(&mut self, group: OutputGroup) {
        if self.output_count == self.output_groups.len() {
            return;
        }
        let index = self.output_write % self.output_groups.len();
        self.output_groups[index] = group;
        self.output_write += 1;
        self.output_count += 1;
    }

    /// Returns one completed packet's output MIDI group, if any.
    pub fn take_output_group(
        &mut self,
        events: &mut [WireEvent],
        sysex: &mut [u8],
    ) -> (usize, usize) {
        if self.output_count == 0 {
            return (0, 0);
        }
        let index = self.output_read % self.output_groups.len();
        let group = &self.output_groups[index];
        let count = group.event_count.min(events.len());
        events[..count].copy_from_slice(&group.events[..count]);
        let sysex_len = group.sysex_len.min(sysex.len());
        sysex[..sysex_len].copy_from_slice(&group.sysex[..sysex_len]);
        self.output_read += 1;
        self.output_count -= 1;
        (count, sysex_len)
    }

    fn drain_pipeline(&mut self) -> bool {
        for _ in 0..PIPELINE_SLOTS {
            let slot = self.mapping.slot(self.read_sequence as usize);
            if slot.state.load(Ordering::Acquire) != SLOT_DONE {
                break;
            }
            // Acquiring DONE grants the host read ownership of this packet.
            let packet = unsafe { &*slot.packet_ptr() };
            if packet.sequence != self.read_sequence + 1 || !self.timeline.stage_packet(packet) {
                self.fail(FailureReason::Protocol);
                return false;
            }
            let event_count = (packet.output_event_count as usize).min(MAX_EVENTS);
            let sysex_len = (packet.output_sysex_len as usize).min(MAX_OUTPUT_SYSEX);
            let mut events = [WireEvent::default(); MAX_EVENTS];
            events[..event_count].copy_from_slice(&packet.output_events[..event_count]);
            let mut sysex = [0u8; MAX_OUTPUT_SYSEX];
            sysex[..sysex_len].copy_from_slice(&packet.output_sysex[..sysex_len]);
            slot.state.store(SLOT_EMPTY, Ordering::Release);
            self.push_output_group(OutputGroup {
                events,
                event_count,
                sysex,
                sysex_len,
            });
            self.read_sequence += 1;
        }
        true
    }

    fn reset_pipeline(&mut self, latency: usize) -> Result<(), SandboxError> {
        self.wait_pipeline()?;
        if !self.drain_pipeline() {
            return Err(SandboxError::Protocol("Invalid queued audio".into()));
        }
        self.timeline.reset(latency);
        self.output_read = 0;
        self.output_write = 0;
        self.output_count = 0;
        self.mapping.set_epoch(self.timeline.epoch);
        self.mapping.set_latency(latency as u32);
        self.mapping.reset_stats();
        Ok(())
    }

    fn valid_audio(
        &self,
        context: &ProcessContext,
        audio: &AudioBuffer<'_>,
        events: &[WireEvent],
    ) -> bool {
        audio.frames > 0
            && audio.frames <= MAX_BLOCK
            && audio.input_channels <= MAX_CHANNELS
            && audio.output_channels <= MAX_CHANNELS
            && audio.sidechain_channels <= MAX_CHANNELS
            && events.len() <= MAX_EVENTS
            && context.is_valid()
            && self.timeline.position < u64::MAX - (MAX_BLOCK * 8) as u64
            && self.mapping.published() < u64::MAX - PIPELINE_SLOTS as u64
    }

    /// When small callbacks fill the ring, combine only an unclaimed final
    /// packet. CAS ownership prevents the worker from reading while we append.
    /// This keeps the fixed allocation and preserves every event/sample time.
    fn append_pending(
        &self,
        context: &ProcessContext,
        audio: &AudioBuffer<'_>,
        events: &[WireEvent],
    ) -> bool {
        let published = self.mapping.published();
        if published == 0 {
            return false;
        }
        let slot = self.mapping.slot((published - 1) as usize);
        if slot
            .state
            .compare_exchange(
                SLOT_READY,
                SLOT_WRITING,
                Ordering::Acquire,
                Ordering::Relaxed,
            )
            .is_err()
        {
            return false;
        }
        // The worker must also claim READY with CAS before reading payloads.
        let packet = unsafe { &mut *slot.packet_ptr() };
        let previous = packet.frames as usize;
        let frames = previous + audio.frames;
        let next = plugin_api::wire::WireProcessContext::from(*context);
        let contiguous_host_time = packet.context.sample_pos.saturating_add(previous as i64)
            == context.sample_pos
            || (!context.playing && packet.context.sample_pos == context.sample_pos);
        let compatible = packet.sequence == published
            && packet.epoch == self.timeline.epoch
            && packet.stream_start.saturating_add(previous as u64) == self.timeline.position
            && frames <= self.timeline.latency.min(MAX_BLOCK)
            && packet.event_count as usize + events.len() <= MAX_EVENTS
            && packet.input_channels as usize == audio.input_channels
            && packet.output_channels as usize == audio.output_channels
            && packet.sidechain_channels as usize == audio.sidechain_channels
            && packet.context.sample_rate == context.sample_rate
            && packet.context.tempo == context.tempo
            && packet.context.flags == next.flags
            && contiguous_host_time;
        if compatible {
            append_planar(
                &mut packet.inputs,
                previous,
                audio.frames,
                audio.input_channels,
                audio.inputs,
            );
            append_planar(
                &mut packet.sidechain,
                previous,
                audio.frames,
                audio.sidechain_channels,
                audio.sidechain,
            );
            let start = packet.event_count as usize;
            for (destination, event) in packet.events[start..].iter_mut().zip(events) {
                *destination = *event;
                destination.frame += previous as u32;
            }
            packet.frames = frames as u32;
            packet.event_count += events.len() as u32;
            packet.context.block_size = packet.context.block_size.max(frames as u32);
        }
        slot.state.store(SLOT_READY, Ordering::Release);
        if !self.mapping.notify() {
            self.fail(FailureReason::Notification);
            return false;
        }
        compatible
    }

    /// Real-time path: finite queue scans/copies only. No waits, clocks,
    /// allocation, Arc cloning, process syscalls or locks.
    pub fn process_pipelined(
        &mut self,
        context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        let mut storage = [WireEvent::default(); MAX_EVENTS];
        let Some(wires) = events_to_wires(events, &mut storage) else {
            audio.outputs.fill(0.0);
            if !self.has_failed() && audio.frames > 0 {
                self.fail(FailureReason::InvalidAudio);
            }
            return;
        };
        self.process_pipelined_wires(context, audio, wires);
    }

    /// Wire-preserving transport, including compact automation sections. The
    /// worker validates event semantics; the host retains packet/event bounds.
    pub fn process_pipelined_wires(
        &mut self,
        context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[WireEvent],
    ) {
        audio.outputs.fill(0.0);
        if self.has_failed() || audio.frames == 0 {
            return;
        }
        if !self.valid_audio(context, audio, events) || audio.frames > self.timeline.latency {
            self.fail(FailureReason::InvalidAudio);
            return;
        }
        if self.timeline.context(context, audio.frames) {
            self.mapping.set_epoch(self.timeline.epoch);
        }
        if !self.drain_pipeline() {
            return;
        }
        let published = self.mapping.published();
        if self
            .mapping
            .slot(published as usize)
            .state
            .load(Ordering::Acquire)
            != SLOT_EMPTY
        {
            if self.append_pending(context, audio, events) {
                if !self.drain_pipeline() {
                    return;
                }
                self.mapping.underrun(self.timeline.render(audio));
                return;
            }
            // A worker may have claimed the tail while completing earlier slots.
            if !self.drain_pipeline() {
                return;
            }
        }
        let slot = self.mapping.slot(published as usize);
        if slot.state.load(Ordering::Acquire) != SLOT_EMPTY {
            // Dropping an input packet could drop note-off events. Fail closed
            // instead; the watchdog stops this worker independently.
            self.fail(FailureReason::QueueFull);
            return;
        }
        // EMPTY belongs exclusively to the producer until READY is released.
        let packet = unsafe { &mut *slot.packet_ptr() };
        packet.sequence = published + 1;
        packet.stream_start = self.timeline.position;
        packet.epoch = self.timeline.epoch;
        packet.context = (*context).into();
        packet.frames = audio.frames as u32;
        packet.input_channels = audio.input_channels as u32;
        packet.output_channels = audio.output_channels as u32;
        packet.sidechain_channels = audio.sidechain_channels as u32;
        packet.inputs[..audio.frames * audio.input_channels]
            .copy_from_slice(&audio.inputs[..audio.frames * audio.input_channels]);
        packet.sidechain[..audio.frames * audio.sidechain_channels]
            .copy_from_slice(&audio.sidechain[..audio.frames * audio.sidechain_channels]);
        for (destination, event) in packet.events.iter_mut().zip(events) {
            *destination = *event;
        }
        packet.event_count = events.len() as u32;
        slot.state.store(SLOT_READY, Ordering::Release);
        self.mapping.publish(published + 1);
        if !self.mapping.notify() {
            self.fail(FailureReason::Notification);
            return;
        }
        // A second bounded scan can pick up results that finished while the
        // input was copied; it never polls for unfinished work.
        if !self.drain_pipeline() {
            return;
        }
        self.mapping.underrun(self.timeline.render(audio));
    }

    /// Offline rendering may wait for workers, but uses the identical sample
    /// delay so DAW compensation and bounce timing match real-time playback.
    pub fn process_offline_delayed(
        &mut self,
        context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        let mut storage = [WireEvent::default(); MAX_EVENTS];
        let Some(wires) = events_to_wires(events, &mut storage) else {
            audio.outputs.fill(0.0);
            if !self.has_failed() && audio.frames > 0 {
                self.fail(FailureReason::Protocol);
            }
            return;
        };
        self.process_offline_delayed_wires(context, audio, wires);
    }

    /// Wire-preserving transport, including compact automation sections. The
    /// worker validates event semantics; the host retains packet/event bounds.
    pub fn process_offline_delayed_wires(
        &mut self,
        context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[WireEvent],
    ) {
        audio.outputs.fill(0.0);
        if self.has_failed() || audio.frames == 0 {
            return;
        }
        if !self.valid_audio(context, audio, events) || self.timeline.latency == 0 {
            self.fail(FailureReason::Protocol);
            return;
        }
        if self.wait_pipeline().is_err() || !self.drain_pipeline() {
            self.fail(FailureReason::Protocol);
            return;
        }
        if self.timeline.context(context, audio.frames) {
            self.mapping.set_epoch(self.timeline.epoch);
            if self.command(RESET).is_err() {
                self.fail(FailureReason::Protocol);
                return;
            }
        }
        self.process_sync_timeout(context, audio, events, self.config.command_timeout);
        if self.has_failed() {
            audio.outputs.fill(0.0);
            return;
        }
        {
            let block = self.mapping.block();
            let event_count = (block.output_event_count as usize).min(MAX_EVENTS);
            let sysex_len = (block.output_sysex_len as usize).min(MAX_OUTPUT_SYSEX);
            let mut events = [WireEvent::default(); MAX_EVENTS];
            events[..event_count].copy_from_slice(&block.output_events[..event_count]);
            let mut sysex = [0u8; MAX_OUTPUT_SYSEX];
            sysex[..sysex_len].copy_from_slice(&block.output_sysex[..sysex_len]);
            self.push_output_group(OutputGroup {
                events,
                event_count,
                sysex,
                sysex_len,
            });
        }
        self.timeline.stage(
            self.timeline.position,
            audio.frames,
            audio.output_channels,
            audio.outputs,
        );
        audio.outputs.fill(0.0);
        self.mapping.underrun(self.timeline.render(audio));
    }

    fn process_sync(
        &mut self,
        context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        let mut storage = [WireEvent::default(); MAX_EVENTS];
        let Some(wires) = events_to_wires(events, &mut storage) else {
            audio.outputs.fill(0.0);
            if !self.has_failed() && audio.frames > 0 {
                self.fail(FailureReason::Protocol);
            }
            return;
        };
        self.process_sync_timeout(context, audio, wires, self.audio_timeout(context));
    }
    fn audio_timeout(&self, context: &ProcessContext) -> Duration {
        if context.is_valid() {
            self.config.block_timeout.max(
                Duration::try_from_secs_f64(context.block_size as f64 / context.sample_rate)
                    .unwrap_or(self.config.command_timeout),
            )
        } else {
            self.config.block_timeout
        }
    }
    fn process_sync_timeout(
        &mut self,
        context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[WireEvent],
        timeout: Duration,
    ) {
        audio.outputs.fill(0.0);
        if self.has_failed() || audio.frames == 0 {
            return;
        }
        let frames = audio.frames;
        if frames > MAX_BLOCK
            || audio.input_channels > MAX_CHANNELS
            || audio.output_channels > MAX_CHANNELS
            || audio.sidechain_channels > MAX_CHANNELS
            || events.len() > MAX_EVENTS
            || !context.is_valid()
        {
            self.fail(FailureReason::Protocol);
            return;
        }
        if self.wait_pipeline().is_err() {
            self.fail(FailureReason::Protocol);
            return;
        }
        let b = self.mapping.block_mut();
        b.frames = frames as u32;
        b.input_channels = audio.input_channels as u32;
        b.output_channels = audio.output_channels as u32;
        b.sidechain_channels = audio.sidechain_channels as u32;
        b.sample_rate = context.sample_rate;
        b.block_size = context.block_size as u32;
        b.tempo = context.tempo;
        b.flags = if context.playing {
            plugin_api::wire::FLAG_PLAYING
        } else {
            0
        };
        b.sample_pos = context.sample_pos;
        b.ppq_position = context.ppq_position;
        b.inputs[..frames * audio.input_channels]
            .copy_from_slice(&audio.inputs[..frames * audio.input_channels]);
        b.sidechain[..frames * audio.sidechain_channels]
            .copy_from_slice(&audio.sidechain[..frames * audio.sidechain_channels]);
        b.outputs[..frames * audio.output_channels].fill(0.0);
        for (destination, event) in b.events.iter_mut().zip(events) {
            *destination = *event;
        }
        b.event_count = events.len() as u32;
        b.block_state.store(BLOCK_READY, Ordering::Release);
        if !self.mapping.notify() {
            self.fail(FailureReason::Notification);
            return;
        }
        let deadline = Instant::now() + timeout;
        loop {
            match self.mapping.block().block_state.load(Ordering::Acquire) {
                BLOCK_DONE => break,
                ERROR => {
                    self.fail(FailureReason::Protocol);
                    return;
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                self.late_blocks.set(self.late_blocks.get() + 1);
                self.fail(FailureReason::ProcessTimeout);
                // The watchdog kills it off-thread. No process syscalls or
                // further shared-memory writes happen in this callback.
                return;
            }
            std::hint::spin_loop();
        }
        let b = self.mapping.block();
        audio.outputs[..frames * audio.output_channels]
            .copy_from_slice(&b.outputs[..frames * audio.output_channels]);
        for sample in audio.outputs.iter_mut() {
            if !sample.is_finite() {
                *sample = 0.0;
            }
        }
    }
    fn command(&self, command: u32) -> Result<(), SandboxError> {
        self.command_cancellable(command, &|| false)
    }
    fn command_cancellable(
        &self,
        command: u32,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), SandboxError> {
        self.wait_pipeline()?;
        if cancelled() {
            return Err(SandboxError::Cancelled);
        }
        self.mapping
            .block()
            .block_state
            .store(command, Ordering::Release);
        if !self.mapping.notify() {
            self.fail(FailureReason::Notification);
            return Err(SandboxError::Worker("Worker notification failed".into()));
        }
        let deadline = Instant::now() + self.config.command_timeout;
        loop {
            if cancelled() {
                self.fail(FailureReason::Cancelled);
                return Err(SandboxError::Cancelled);
            }
            match self.mapping.block().block_state.load(Ordering::Acquire) {
                BLOCK_DONE => return Ok(()),
                ERROR => {
                    self.fail(FailureReason::WorkerReported);
                    return Err(SandboxError::Worker(shared_error(&self.mapping)));
                }
                _ => {}
            }
            if Instant::now() >= deadline {
                self.fail(FailureReason::CommandTimeout);
                return Err(SandboxError::Worker("Worker command timed out".into()));
            }
            std::thread::sleep(Duration::from_millis(1));
        }
    }

    pub fn save_state_checked(&self) -> Result<State, SandboxError> {
        self.command(SAVE)?;
        let block = self.mapping.block();
        let length = block.state_len as usize;
        if length > MAX_STATE {
            return Err(SandboxError::Protocol("Invalid state size".into()));
        }
        Ok(State {
            schema_version: block.state_schema,
            bytes: block.state[..length].to_vec(),
        })
    }

    /// Used only for an unattached instrument on its loading thread.
    pub fn prepare_cancellable(
        &mut self,
        context: &ProcessContext,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), SandboxError> {
        if self.has_failed() || !context.is_valid() {
            return Err(SandboxError::Worker(
                "Invalid or failed worker preparation".into(),
            ));
        }
        self.reset_pipeline(context.block_size)?;
        self.watchdog.set_timeout(self.audio_timeout(context));
        let b = self.mapping.block_mut();
        b.sample_rate = context.sample_rate;
        b.block_size = context.block_size as u32;
        self.command_cancellable(PREPARE, cancelled)
    }

    pub fn load_state_cancellable(
        &mut self,
        state: &State,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(), SandboxError> {
        if self.has_failed() || state.bytes.len() > MAX_STATE {
            return Err(SandboxError::Worker(
                "Invalid state or failed worker".into(),
            ));
        }
        self.reset_pipeline(self.timeline.latency)?;
        let b = self.mapping.block_mut();
        b.state[..state.bytes.len()].copy_from_slice(&state.bytes);
        b.state_len = state.bytes.len() as u32;
        b.state_schema = state.schema_version;
        self.command_cancellable(LOAD, cancelled)
    }
}

/// The Rust SDK event enum intentionally does not acquire transport-only ramp
/// variants. Existing callers keep it; conversion uses fixed stack storage.
fn events_to_wires<'a>(
    events: &[Event],
    storage: &'a mut [WireEvent; MAX_EVENTS],
) -> Option<&'a [WireEvent]> {
    if events.len() > MAX_EVENTS {
        return None;
    }
    for (destination, event) in storage.iter_mut().zip(events) {
        *destination = (*event).into();
    }
    Some(&storage[..events.len()])
}

impl Plugin for SandboxPlugin {
    fn load_notices(&self) -> Vec<String> {
        self.metadata.notices.clone()
    }
    fn descriptor(&self) -> Descriptor {
        self.descriptor
    }
    fn capabilities(&self) -> Capabilities {
        self.metadata.capabilities()
    }
    fn parameters(&self) -> Vec<ParameterInfo> {
        self.parameters.clone()
    }
    fn prepare(&mut self, context: &ProcessContext) {
        if self.has_failed() {
            return;
        }
        if !context.is_valid() {
            self.fail(FailureReason::Protocol);
            return;
        }
        if self.reset_pipeline(context.block_size).is_err() {
            self.fail(FailureReason::Protocol);
            return;
        }
        self.watchdog.set_timeout(self.audio_timeout(context));
        let b = self.mapping.block_mut();
        b.sample_rate = context.sample_rate;
        b.block_size = context.block_size as u32;
        if self.command(PREPARE).is_err() {
            self.fail(FailureReason::Protocol);
        }
    }
    fn reset(&mut self) {
        if self.reset_pipeline(self.timeline.latency).is_err() {
            self.fail(FailureReason::Protocol);
            return;
        }
        if self.command(RESET).is_err() {
            self.fail(FailureReason::Protocol);
        }
    }
    fn process(&mut self, context: &ProcessContext, audio: &mut AudioBuffer<'_>, events: &[Event]) {
        self.process_sync(context, audio, events);
    }
    fn save_state(&self) -> State {
        self.save_state_checked().unwrap_or_default()
    }
    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        if self.has_failed() {
            return Err(StateError::Malformed("Worker has failed".into()));
        }
        if state.bytes.len() > MAX_STATE {
            return Err(StateError::Malformed("State exceeds 64 KiB".into()));
        }
        self.reset_pipeline(self.timeline.latency)
            .map_err(|e| StateError::Malformed(e.to_string()))?;
        let b = self.mapping.block_mut();
        b.state[..state.bytes.len()].copy_from_slice(&state.bytes);
        b.state_len = state.bytes.len() as u32;
        b.state_schema = state.schema_version;
        self.command(LOAD)
            .map_err(|e| StateError::Malformed(e.to_string()))
    }
}
fn append_planar(
    destination: &mut [f32],
    previous: usize,
    added: usize,
    channels: usize,
    source: &[f32],
) {
    let frames = previous + added;
    for channel in (0..channels).rev() {
        destination.copy_within(
            channel * previous..(channel + 1) * previous,
            channel * frames,
        );
        destination[channel * frames + previous..(channel + 1) * frames]
            .copy_from_slice(&source[channel * added..(channel + 1) * added]);
    }
}
impl Drop for SandboxPlugin {
    fn drop(&mut self) {
        if self.mapping.pipeline_has_error() && self.failure_reason() == FailureReason::None {
            self.watchdog.request_stop(FailureReason::Protocol);
        }
        self.watchdog.stop();
        unsafe {
            libc::kill(-(self.child.id() as i32), libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn shared_error(mapping: &SharedMapping) -> String {
    let b = mapping.block();
    String::from_utf8_lossy(&b.state[..(b.state_len as usize).min(MAX_STATE)]).into_owned()
}
