//! The universal plugin protocol.
//!
//! This crate is the contract every plugin author targets — whether they are
//! writing a new instrument, hosting a native VST3/AU/CLAP, or publishing a
//! clean-room reimplementation of an existing library. It defines:
//!
//! - planar audio with arbitrary input/output channels and a sidechain bus;
//! - block-relative, sample-accurate MIDI and parameter events;
//! - validated parameters and a declarative UI schema;
//! - versioned opaque state;
//! - declared capabilities (channel counts, latency, tail);
//! - a provider abstraction for format adapters.
//!
//! Real-time rules for [`Plugin::process`]:
//! - no allocation, no I/O, no locks, no panics on malformed host input;
//! - touch only `frames` and the declared channel counts;
//! - write finite `f32` values (no NaN/Inf).

#![forbid(unsafe_code)]

pub mod abi;
pub mod conformance;
pub mod provider;
pub mod wire;

/// Protocol version. Bumping this is a breaking change.
pub const API_VERSION: u32 = 1;
/// Upper bound the host may pass as `ProcessContext::block_size`.
pub const MAX_BLOCK_SIZE: usize = 4096;
/// Maximum channels on any single bus.
pub const MAX_CHANNELS: usize = 256;
/// Upper bound for a single parameter id namespace.
pub const MAX_PARAMETERS: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginKind {
    Instrument,
    Effect,
    MidiEffect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Descriptor {
    /// Stable, reverse-DNS style id, e.g. `com.example.rebit`.
    pub id: &'static str,
    pub name: &'static str,
    pub vendor: &'static str,
    pub version: &'static str,
    pub kind: PluginKind,
}

impl Descriptor {
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
            && !self.name.is_empty()
            && !self.vendor.is_empty()
            && !self.version.is_empty()
            && self.id.contains('.')
    }
}

/// What the plugin can do. The host allocates buffers from this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Main input channels (0 for instruments).
    pub inputs: usize,
    /// Main output channels.
    pub outputs: usize,
    /// Optional sidechain input channels (0 = none).
    pub sidechain_inputs: usize,
    /// Reported latency in samples, for host delay compensation.
    pub latency_samples: usize,
    /// Tail length in samples after input stops.
    pub tail_samples: usize,
}

impl Capabilities {
    pub fn stereo_instrument() -> Self {
        Capabilities {
            inputs: 0,
            outputs: 2,
            sidechain_inputs: 0,
            latency_samples: 0,
            tail_samples: 0,
        }
    }

    pub fn stereo_effect() -> Self {
        Capabilities {
            inputs: 2,
            outputs: 2,
            sidechain_inputs: 0,
            latency_samples: 0,
            tail_samples: 0,
        }
    }

    pub fn is_valid(&self) -> bool {
        let channels = self.inputs + self.outputs + self.sidechain_inputs;
        channels > 0
            && self.inputs <= MAX_CHANNELS
            && self.outputs <= MAX_CHANNELS
            && self.sidechain_inputs <= MAX_CHANNELS
            && self.latency_samples < 10_000_000
            && self.tail_samples < 100_000_000
    }
}

#[derive(Debug, Clone, Copy)]
pub struct ProcessContext {
    pub sample_rate: f64,
    pub block_size: usize,
    pub tempo: f64,
    pub playing: bool,
    /// Signed host timeline position; negative values represent DAW preroll.
    pub sample_pos: i64,
    /// Musical position in quarter notes.
    pub ppq_position: f64,
}

impl Default for ProcessContext {
    fn default() -> Self {
        ProcessContext {
            sample_rate: 48_000.0,
            block_size: 256,
            tempo: 120.0,
            playing: false,
            sample_pos: 0,
            ppq_position: 0.0,
        }
    }
}

impl ProcessContext {
    pub fn is_valid(&self) -> bool {
        self.sample_rate.is_finite()
            && self.sample_rate > 0.0
            && self.block_size > 0
            && self.block_size <= MAX_BLOCK_SIZE
            && self.tempo.is_finite()
            && self.tempo > 0.0
            && self.ppq_position.is_finite()
    }
}

/// Planar audio for one block. Each channel is a contiguous run of `frames`
/// samples.
///
/// Channel counts are clamped to the supplied buffer lengths, so a plugin can
/// always trust `channel_count * frames <= slice.len()`.
pub struct AudioBuffer<'a> {
    pub inputs: &'a mut [f32],
    pub outputs: &'a mut [f32],
    pub sidechain: &'a mut [f32],
    pub input_channels: usize,
    pub output_channels: usize,
    pub sidechain_channels: usize,
    pub frames: usize,
}

impl<'a> AudioBuffer<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        inputs: &'a mut [f32],
        input_channels: usize,
        outputs: &'a mut [f32],
        output_channels: usize,
        sidechain: &'a mut [f32],
        sidechain_channels: usize,
        frames: usize,
    ) -> Self {
        let input_channels = clamp_channels(inputs.len(), frames, input_channels);
        let output_channels = clamp_channels(outputs.len(), frames, output_channels);
        let sidechain_channels = clamp_channels(sidechain.len(), frames, sidechain_channels);
        AudioBuffer {
            inputs,
            outputs,
            sidechain,
            input_channels,
            output_channels,
            sidechain_channels,
            frames,
        }
    }

    pub fn input(&self, channel: usize) -> &[f32] {
        channel_slice(self.inputs, self.input_channels, self.frames, channel)
    }

    pub fn input_mut(&mut self, channel: usize) -> &mut [f32] {
        channel_slice_mut(self.inputs, self.input_channels, self.frames, channel)
    }

    pub fn output(&self, channel: usize) -> &[f32] {
        channel_slice(self.outputs, self.output_channels, self.frames, channel)
    }

    pub fn output_mut(&mut self, channel: usize) -> &mut [f32] {
        channel_slice_mut(self.outputs, self.output_channels, self.frames, channel)
    }

    pub fn sidechain(&self, channel: usize) -> &[f32] {
        channel_slice(
            self.sidechain,
            self.sidechain_channels,
            self.frames,
            channel,
        )
    }

    pub fn sidechain_mut(&mut self, channel: usize) -> &mut [f32] {
        channel_slice_mut(
            self.sidechain,
            self.sidechain_channels,
            self.frames,
            channel,
        )
    }

    pub fn clear_outputs(&mut self) {
        self.outputs.fill(0.0);
    }
}

fn clamp_channels(len: usize, frames: usize, requested: usize) -> usize {
    let available = len.checked_div(frames).unwrap_or(0);
    requested.min(available).min(MAX_CHANNELS)
}

fn channel_slice(data: &[f32], channels: usize, frames: usize, channel: usize) -> &[f32] {
    if channel >= channels || frames == 0 {
        return &[];
    }
    let start = channel * frames;
    &data[start..start + frames]
}

fn channel_slice_mut(
    data: &mut [f32],
    channels: usize,
    frames: usize,
    channel: usize,
) -> &mut [f32] {
    if channel >= channels || frames == 0 {
        return &mut [];
    }
    let start = channel * frames;
    &mut data[start..start + frames]
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventKind {
    NoteOn,
    NoteOff,
    PolyPressure,
    ControlChange,
    ProgramChange,
    PitchBend,
    ChannelPressure,
    /// `data1` is the parameter id, `value` is the normalized value.
    Parameter,
    /// `data1` is the offset into the output SysEx buffer, `data2` is the length.
    SysEx,
}

/// A timed event. `frame` is a sample offset within the current block; events
/// are not required to be sorted, but plugins should process them in order.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Event {
    pub frame: usize,
    pub kind: EventKind,
    pub channel: u8,
    pub data1: u16,
    pub data2: u16,
    pub value: f32,
}

impl Event {
    pub fn note_on(frame: usize, key: u8, velocity: u8) -> Self {
        Event {
            frame,
            kind: EventKind::NoteOn,
            channel: 0,
            data1: key as u16,
            data2: velocity as u16,
            value: velocity as f32 / 127.0,
        }
    }

    pub fn note_off(frame: usize, key: u8) -> Self {
        Event {
            frame,
            kind: EventKind::NoteOff,
            channel: 0,
            data1: key as u16,
            data2: 0,
            value: 0.0,
        }
    }

    pub fn control_change(frame: usize, controller: u8, value: u8) -> Self {
        Event {
            frame,
            kind: EventKind::ControlChange,
            channel: 0,
            data1: controller as u16,
            data2: value as u16,
            value: value as f32 / 127.0,
        }
    }

    pub fn pitch_bend(frame: usize, value: f32) -> Self {
        Event {
            frame,
            kind: EventKind::PitchBend,
            channel: 0,
            data1: 0,
            data2: 0,
            value: value.clamp(-1.0, 1.0),
        }
    }

    pub fn parameter(frame: usize, id: u32, normalized: f32) -> Self {
        Event {
            frame,
            kind: EventKind::Parameter,
            channel: 0,
            data1: id.min(u16::MAX as u32) as u16,
            data2: 0,
            value: normalized.clamp(0.0, 1.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unit {
    None,
    Decibels,
    Hertz,
    Percent,
    Semitones,
    Milliseconds,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ParamFlags {
    pub stepped: bool,
    pub read_only: bool,
    pub bypass: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParameterInfo {
    pub id: u32,
    pub name: &'static str,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub unit: Unit,
    pub flags: ParamFlags,
}

impl ParameterInfo {
    pub fn is_valid(&self) -> bool {
        self.id != u32::MAX
            && !self.name.is_empty()
            && self.min.is_finite()
            && self.max.is_finite()
            && self.default.is_finite()
            && self.min <= self.default
            && self.default <= self.max
            && self.min < self.max
    }

    pub fn normalize(&self, value: f32) -> f32 {
        if self.max <= self.min {
            0.0
        } else {
            ((value - self.min) / (self.max - self.min)).clamp(0.0, 1.0)
        }
    }

    pub fn denormalize(&self, normalized: f32) -> f32 {
        self.min + normalized.clamp(0.0, 1.0) * (self.max - self.min)
    }
}

/// Declarative UI. The host renders these; plugins never ship executable UI.
#[derive(Debug, Clone, PartialEq)]
pub enum UiNode {
    Group {
        label: String,
        children: Vec<UiNode>,
    },
    Knob {
        param: u32,
        label: String,
    },
    Toggle {
        param: u32,
        label: String,
    },
    Selector {
        param: u32,
        label: String,
        options: Vec<String>,
    },
    Label {
        text: String,
    },
    Meter {
        label: String,
    },
}

/// Opaque, versioned plugin state.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct State {
    pub schema_version: u32,
    pub bytes: Vec<u8>,
}

impl State {
    pub fn empty() -> Self {
        State::default()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StateError {
    SchemaTooNew { found: u32, supported: u32 },
    Malformed(String),
}

impl fmt::Display for StateError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StateError::SchemaTooNew { found, supported } => {
                write!(
                    f,
                    "state schema {found} is newer than supported {supported}"
                )
            }
            StateError::Malformed(message) => write!(f, "malformed state: {message}"),
        }
    }
}

impl std::error::Error for StateError {}

use std::fmt;
use std::sync::Arc;

/// Read-only access to a plugin's package assets (IRs, model weights, images).
/// The host provides this before `prepare`; plugins must not touch the
/// filesystem directly.
pub trait AssetProvider: Send + Sync {
    fn read(&self, name: &str) -> Option<Vec<u8>>;
    fn list(&self) -> Vec<String>;
}

/// Sample/cache diagnostics, distinct from host pipeline dropouts and process RSS.
/// Counters/peaks cover this loaded instrument's lifetime; flags bit 0 = streaming.
/// underrun_frames counts output voice frames with either interpolation tap missing.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct StreamingTelemetry {
    pub resident_bytes: u64,
    pub cache_capacity_bytes: u64,
    pub cache_ready_bytes: u64,
    pub peak_cache_ready_bytes: u64,
    pub pages_read: u64,
    pub read_errors: u64,
    pub underrun_frames: u64,
    pub underrun_blocks: u64,
    pub read_count: u64,
    pub read_total_ns: u64,
    pub last_read_ns: u64,
    pub max_read_ns: u64,
    pub active_voices: u32,
    pub voice_capacity: u32,
    pub pending_reads: u32,
    pub flags: u32,
}

/// Optional format-specific tasks serviced by the thread which created a plugin.
/// Never call these methods from a realtime callback. The host must keep pumping
/// tasks until plugin destruction has completed.
pub trait MainThreadTasks: Send + Sync {
    fn run_pending(&self);
    fn is_shutting_down(&self) -> bool;
    fn take_worker_notification(&self) -> bool;
    /// Timing read while servicing a main-thread callback, ready to publish to
    /// the shared monitor even when the audio thread is idle.
    fn take_timing(&self) -> Option<(u32, u32)> {
        None
    }
    /// The creator thread has closed native views and can destroy the plugin.
    fn allow_shutdown(&self);
}

/// The universal plugin contract. The host owns the audio thread and calls
/// these methods; all real-time rules apply to `process`.
pub trait Plugin: Send {
    fn descriptor(&self) -> Descriptor;

    /// Load-time compatibility notices for the host UI. Never queried by the
    /// audio callback; adapters without limitations inherit an empty list.
    fn load_notices(&self) -> Vec<String> {
        Vec::new()
    }

    /// All plugin ids/descriptions contained by this loaded module. Most
    /// adapters expose one; CLAP modules may expose several selectable plugins.
    fn variants(&self) -> Vec<Descriptor> {
        Vec::new()
    }

    /// Declared once; the host allocates buffers from this.
    fn capabilities(&self) -> Capabilities;

    /// Current latency, which may change after initial discovery. Adapters that
    /// cannot change latency dynamically inherit the declared capability.
    fn current_latency_samples(&self) -> usize {
        self.capabilities().latency_samples
    }

    /// Current tail, which may change after initial discovery.
    fn current_tail_samples(&self) -> usize {
        self.capabilities().tail_samples
    }

    /// Native GUI handle for out-of-process editors; null when unsupported.
    fn native_gui_handle(&self) -> *mut std::ffi::c_void {
        std::ptr::null_mut()
    }

    fn main_thread_tasks(&self) -> Option<Arc<dyn MainThreadTasks>> {
        None
    }

    /// Worker-only housekeeping between process calls, including while idle.
    /// May perform non-realtime lifecycle work; never called by the host audio
    /// callback. Format callbacks only record requests until this safe boundary.
    fn service_host_requests(&mut self) {}

    /// Lifecycle/flush work is pending; drop elevated worker scheduling before
    /// servicing it. This query must itself be nonblocking.
    fn host_service_pending(&self) -> bool {
        false
    }

    /// Returns true once after the plugin requested a structural metadata reload.
    fn take_reload_request(&self) -> bool {
        false
    }

    /// Collects events produced by the last `process` call. SysEx events use
    /// `EventKind::SysEx` with data1=offset and data2=length into `sysex`.
    fn fill_output_events(&mut self, _events: &mut [Event], _sysex: &mut [u8]) -> (usize, usize) {
        (0, 0)
    }

    /// Returns true once after the plugin requested a parameter-value refresh.
    fn take_parameter_refresh_request(&self) -> bool {
        false
    }

    /// Writes current normalized parameter values into `out` and returns the
    /// count. Called off the audio callback only.
    fn fill_parameter_values(&self, _out: &mut [(u32, f32)]) -> usize {
        0
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        Vec::new()
    }

    /// Stable serialization identity, independent of a format adapter's wire ID.
    fn parameter_key(&self, id: u32) -> String {
        id.to_string()
    }

    fn ui(&self) -> Vec<UiNode> {
        Vec::new()
    }

    /// Called outside the audio thread with the current stream format.
    /// Called before `prepare` with read-only package assets.
    fn set_assets(&mut self, _assets: Arc<dyn AssetProvider>) {}

    fn prepare(&mut self, context: &ProcessContext);

    /// Select an explicitly non-realtime render. Streaming implementations may
    /// wait for disk pages only in this mode. Hosts must restore false before
    /// realtime processing. This adds no field to the native/wire ABI.
    fn set_offline_rendering(&mut self, _offline: bool) {}

    /// Terminal processing failure, queried by an owning host after a block.
    /// A late realtime cache page alone is an underrun, not a terminal error.
    fn processing_error(&self) -> Option<&'static str> {
        None
    }

    /// Cumulative output voice frames with missing sample data. Kept separate
    /// from host pipeline dropouts in streaming telemetry.
    fn source_underrun_frames(&self) -> u64 {
        0
    }

    /// Optional sampler diagnostics. Returns owned scalars only; safe to query
    /// between blocks on the plugin's owner thread. Timing covers page delivery
    /// (file access plus decoding), not physical disk utilization.
    fn streaming_telemetry(&self) -> Option<StreamingTelemetry> {
        None
    }

    /// Clear voices, delay lines and meters. Called outside the audio thread.
    fn reset(&mut self);

    fn process(&mut self, context: &ProcessContext, audio: &mut AudioBuffer<'_>, events: &[Event]);

    fn save_state(&self) -> State {
        State::empty()
    }

    /// State capture with explicit failure reporting for hosted formats.
    fn try_save_state(&self) -> Result<State, StateError> {
        Ok(self.save_state())
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        if state.bytes.is_empty() {
            Ok(())
        } else {
            Err(StateError::Malformed("plugin has no state".into()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn descriptor_and_parameter_validation() {
        let descriptor = Descriptor {
            id: "com.example.test",
            name: "Test",
            vendor: "Example",
            version: "1.0.0",
            kind: PluginKind::Instrument,
        };
        assert!(descriptor.is_valid());

        let parameter = ParameterInfo {
            id: 0,
            name: "Gain",
            min: -60.0,
            max: 12.0,
            default: 0.0,
            unit: Unit::Decibels,
            flags: ParamFlags::default(),
        };
        assert!(parameter.is_valid());
        assert!((parameter.normalize(0.0) - 60.0 / 72.0).abs() < 1e-6);
        assert!((parameter.denormalize(parameter.normalize(-6.0)) + 6.0).abs() < 1e-4);

        let bad = ParameterInfo {
            default: 13.0,
            ..parameter
        };
        assert!(!bad.is_valid());
    }

    #[test]
    fn context_and_capabilities_validation() {
        assert!(ProcessContext::default().is_valid());
        let bad = ProcessContext {
            sample_rate: 0.0,
            ..ProcessContext::default()
        };
        assert!(!bad.is_valid());

        assert!(Capabilities::stereo_instrument().is_valid());
        assert!(Capabilities::stereo_effect().is_valid());
        let none = Capabilities {
            inputs: 0,
            outputs: 0,
            sidechain_inputs: 0,
            latency_samples: 0,
            tail_samples: 0,
        };
        assert!(!none.is_valid());
    }

    #[test]
    fn audio_buffer_clamps_channels() {
        let mut inputs = vec![0.0f32; 2 * 16];
        let mut outputs = vec![0.0f32; 2 * 16];
        let mut sidechain = vec![0.0f32; 16];
        let buffer = AudioBuffer::new(
            &mut inputs,
            4, // request more than available
            &mut outputs,
            2,
            &mut sidechain,
            1,
            16,
        );
        assert_eq!(buffer.input_channels, 2);
        assert_eq!(buffer.output_channels, 2);
        assert_eq!(buffer.sidechain_channels, 1);
        assert_eq!(buffer.output(0).len(), 16);
        assert!(buffer.output(2).is_empty());
    }
}
