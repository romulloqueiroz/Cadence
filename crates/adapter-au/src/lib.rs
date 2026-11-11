//! macOS Audio Unit (AUv2) hosting adapter.
//!
//! Scope: enumerate installed components, instantiate an effect or instrument,
//! process stereo audio, expose parameters, and send MIDI to instruments.
//! State, native UI bridging and AUv3 app extensions are not handled yet.
//!
//! Components are addressed by name, e.g. `au:Archetype Petrucci`.

#![cfg(target_os = "macos")]

use coreaudio_sys as ca;
use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, EventKind, ParamFlags, ParameterInfo, Plugin,
    PluginKind, ProcessContext, State, StateError, UiNode, Unit, MAX_BLOCK_SIZE,
};
use std::collections::HashSet;
use std::ffi::{c_char, c_void, CStr};
use std::fmt;
use std::mem::{size_of, MaybeUninit};
use std::ptr;

#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFStringGetCString(
        string: ca::CFStringRef,
        buffer: *mut c_char,
        size: isize,
        encoding: u32,
    ) -> u8;
    fn CFRelease(value: *const c_void);
}

const CF_STRING_ENCODING_UTF8: u32 = 0x0800_0100;

#[derive(Debug, Clone)]
pub enum AuError {
    NotFound(String),
    Instantiate(i32),
    Property(i32),
    Render(i32),
}

impl fmt::Display for AuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AuError::NotFound(name) => write!(f, "Audio Unit not found: {name}"),
            AuError::Instantiate(status) => write!(f, "Audio Unit instantiate failed ({status})"),
            AuError::Property(status) => write!(f, "Audio Unit property failed ({status})"),
            AuError::Render(status) => write!(f, "Audio Unit render failed ({status})"),
        }
    }
}

impl std::error::Error for AuError {}

#[derive(Debug, Clone)]
pub struct AuComponentInfo {
    pub name: String,
    pub component_type: u32,
    pub sub_type: u32,
    pub manufacturer: u32,
}

pub fn is_au_query(query: &str) -> bool {
    query.starts_with("au:")
}

pub fn list_components() -> Vec<AuComponentInfo> {
    let query = wildcard();
    let mut component = unsafe { ca::AudioComponentFindNext(ptr::null_mut(), &query) };
    let mut out = Vec::new();
    while !component.is_null() {
        let mut description = wildcard();
        if unsafe { ca::AudioComponentGetDescription(component, &mut description) } == 0 {
            out.push(AuComponentInfo {
                name: component_name(component),
                component_type: description.componentType,
                sub_type: description.componentSubType,
                manufacturer: description.componentManufacturer,
            });
        }
        component = unsafe { ca::AudioComponentFindNext(component, &query) };
    }
    out
}

struct RenderState {
    inputs: [*const f32; 2],
    channels: usize,
    frames: usize,
}

impl Default for RenderState {
    fn default() -> Self {
        RenderState {
            inputs: [ptr::null(); 2],
            channels: 0,
            frames: 0,
        }
    }
}

#[repr(C)]
struct AudioBufferList2 {
    number_buffers: u32,
    buffers: [ca::AudioBuffer; 2],
}

pub struct AuPlugin {
    unit: ca::AudioUnit,
    descriptor: Descriptor,
    capabilities: Capabilities,
    parameters: Vec<ParameterInfo>,
    render: Box<RenderState>,
    silence: Vec<f32>,
    active: bool,
    failed: bool,
}

unsafe impl Send for AuPlugin {}

impl AuPlugin {
    pub fn load(query: &str) -> Result<Self, AuError> {
        let name = query.strip_prefix("au:").unwrap_or(query);
        let (component, description) =
            find_component(name).ok_or_else(|| AuError::NotFound(name.to_string()))?;

        let mut unit: ca::AudioUnit = ptr::null_mut();
        let status = unsafe { ca::AudioComponentInstanceNew(component, &mut unit) };
        if status != 0 || unit.is_null() {
            return Err(AuError::Instantiate(status));
        }

        let kind = match description.componentType {
            t if t == ca::kAudioUnitType_MusicDevice || t == ca::kAudioUnitType_Generator => {
                PluginKind::Instrument
            }
            _ => PluginKind::Effect,
        };
        let component_name = component_name(component);
        let descriptor = Descriptor {
            id: Box::leak(
                format!(
                    "au.{:08X}.{:08X}",
                    description.componentSubType, description.componentManufacturer
                )
                .into_boxed_str(),
            ),
            name: Box::leak(component_name.into_boxed_str()),
            vendor: "AudioUnit",
            version: "1",
            kind,
        };
        let capabilities = Capabilities {
            inputs: if kind == PluginKind::Instrument { 0 } else { 2 },
            outputs: 2,
            sidechain_inputs: 0,
            latency_samples: 0,
            tail_samples: 0,
        };

        let mut plugin = AuPlugin {
            unit,
            descriptor,
            capabilities,
            parameters: Vec::new(),
            render: Box::new(RenderState::default()),
            silence: vec![0.0; MAX_BLOCK_SIZE],
            active: false,
            failed: false,
        };
        plugin.discover_parameters();
        Ok(plugin)
    }

    pub fn unit(&self) -> ca::AudioUnit {
        self.unit
    }

    fn discover_parameters(&mut self) {
        let mut seen = HashSet::new();
        for scope in [ca::kAudioUnitScope_Global, ca::kAudioUnitScope_Input] {
            let mut size = 4096u32;
            let mut ids = vec![0u32; 1024];
            let status = unsafe {
                ca::AudioUnitGetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_ParameterList,
                    scope,
                    0,
                    ids.as_mut_ptr() as *mut c_void,
                    &mut size,
                )
            };
            if status != 0 || size == 0 {
                continue;
            }
            let count = (size as usize / size_of::<u32>()).min(ids.len());
            ids.truncate(count);
            for id in ids {
                if !seen.insert(id) {
                    continue;
                }
                let mut info = MaybeUninit::<ca::AudioUnitParameterInfo>::zeroed();
                let mut info_size = size_of::<ca::AudioUnitParameterInfo>() as u32;
                let status = unsafe {
                    ca::AudioUnitGetProperty(
                        self.unit,
                        ca::kAudioUnitProperty_ParameterInfo,
                        scope,
                        id,
                        info.as_mut_ptr() as *mut c_void,
                        &mut info_size,
                    )
                };
                if status != 0 {
                    continue;
                }
                let info = unsafe { info.assume_init() };
                let name = unsafe { CStr::from_ptr(info.name.as_ptr()) }
                    .to_string_lossy()
                    .into_owned();
                if name.is_empty() {
                    continue;
                }
                self.parameters.push(ParameterInfo {
                    id,
                    name: Box::leak(name.into_boxed_str()),
                    min: info.minValue,
                    max: info.maxValue,
                    default: info.defaultValue,
                    unit: parameter_unit(info.unit),
                    flags: ParamFlags {
                        stepped: info.unit == ca::kAudioUnitParameterUnit_Indexed
                            || info.unit == ca::kAudioUnitParameterUnit_Boolean,
                        read_only: info.flags != 0
                            && (info.flags & ca::kAudioUnitParameterFlag_IsWritable) == 0,
                        bypass: false,
                    },
                });
            }
        }
    }

    fn update_latency(&mut self, sample_rate: f64) {
        let mut seconds: f64 = 0.0;
        let mut size = size_of::<f64>() as u32;
        let status = unsafe {
            ca::AudioUnitGetProperty(
                self.unit,
                ca::kAudioUnitProperty_Latency,
                ca::kAudioUnitScope_Global,
                0,
                &mut seconds as *mut f64 as *mut c_void,
                &mut size,
            )
        };
        if status == 0 && seconds > 0.0 {
            self.capabilities.latency_samples = (seconds * sample_rate) as usize;
        }
    }

    fn set_parameter(&self, id: u32, value: f32, offset: u32) {
        let status = unsafe {
            ca::AudioUnitSetParameter(self.unit, id, ca::kAudioUnitScope_Global, 0, value, offset)
        };
        if status != 0 {
            unsafe {
                ca::AudioUnitSetParameter(
                    self.unit,
                    id,
                    ca::kAudioUnitScope_Input,
                    0,
                    value,
                    offset,
                );
            }
        }
    }

    fn handle_event(&self, event: &Event) {
        match event.kind {
            EventKind::Parameter => {
                if let Some(parameter) = self
                    .parameters
                    .iter()
                    .find(|parameter| parameter.id == event.data1 as u32)
                {
                    let value = parameter.denormalize(event.value);
                    self.set_parameter(parameter.id, value, event.frame as u32);
                }
            }
            _ if self.descriptor.kind == PluginKind::Instrument => {
                let (status, data1, data2): (u32, u32, u32) = match event.kind {
                    EventKind::NoteOn => (
                        (0x90 | event.channel) as u32,
                        event.data1 as u32,
                        event.data2 as u32,
                    ),
                    EventKind::NoteOff => (
                        (0x80 | event.channel) as u32,
                        event.data1 as u32,
                        event.data2 as u32,
                    ),
                    EventKind::ControlChange => (
                        (0xB0 | event.channel) as u32,
                        event.data1 as u32,
                        event.data2 as u32,
                    ),
                    EventKind::PitchBend => {
                        let value =
                            (((event.value.clamp(-1.0, 1.0) + 1.0) * 8191.5) as u32).min(16383);
                        (
                            (0xE0 | event.channel) as u32,
                            value & 0x7F,
                            (value >> 7) & 0x7F,
                        )
                    }
                    EventKind::ChannelPressure => (
                        (0xD0 | event.channel) as u32,
                        (event.value.clamp(0.0, 1.0) * 127.0) as u32,
                        0,
                    ),
                    _ => return,
                };
                unsafe {
                    ca::MusicDeviceMIDIEvent(self.unit, status, data1, data2, event.frame as u32);
                }
            }
            _ => {}
        }
    }
}

impl Plugin for AuPlugin {
    fn descriptor(&self) -> Descriptor {
        self.descriptor
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        self.parameters.clone()
    }

    fn ui(&self) -> Vec<UiNode> {
        Vec::new()
    }

    fn prepare(&mut self, context: &ProcessContext) {
        if self.active || self.failed {
            return;
        }

        let asbd = ca::AudioStreamBasicDescription {
            mSampleRate: context.sample_rate,
            mFormatID: ca::kAudioFormatLinearPCM,
            mFormatFlags: ca::kAudioFormatFlagIsFloat
                | ca::kAudioFormatFlagIsPacked
                | ca::kAudioFormatFlagIsNonInterleaved,
            mBytesPerPacket: 4,
            mFramesPerPacket: 1,
            mBytesPerFrame: 4,
            mChannelsPerFrame: 2,
            mBitsPerChannel: 32,
            mReserved: 0,
        };
        let format_size = size_of::<ca::AudioStreamBasicDescription>() as u32;
        let max_frames = MAX_BLOCK_SIZE as u32;
        unsafe {
            ca::AudioUnitSetProperty(
                self.unit,
                ca::kAudioUnitProperty_MaximumFramesPerSlice,
                ca::kAudioUnitScope_Global,
                0,
                &max_frames as *const u32 as *const c_void,
                size_of::<u32>() as u32,
            );
        }

        if self.descriptor.kind == PluginKind::Effect {
            let status = unsafe {
                ca::AudioUnitSetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_StreamFormat,
                    ca::kAudioUnitScope_Input,
                    0,
                    &asbd as *const _ as *const c_void,
                    format_size,
                )
            };
            if status != 0 {
                self.failed = true;
                eprintln!("AU input stream format failed: {status}");
                return;
            }
        }

        let status = unsafe {
            ca::AudioUnitSetProperty(
                self.unit,
                ca::kAudioUnitProperty_StreamFormat,
                ca::kAudioUnitScope_Output,
                0,
                &asbd as *const _ as *const c_void,
                format_size,
            )
        };
        if status != 0 {
            self.failed = true;
            eprintln!("AU output stream format failed: {status}");
            return;
        }

        if self.descriptor.kind == PluginKind::Effect {
            let state = self.render.as_mut() as *mut RenderState as *mut c_void;
            let callback = ca::AURenderCallbackStruct {
                inputProc: Some(render_input),
                inputProcRefCon: state,
            };
            let status = unsafe {
                ca::AudioUnitSetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_SetRenderCallback,
                    ca::kAudioUnitScope_Input,
                    0,
                    &callback as *const _ as *const c_void,
                    size_of::<ca::AURenderCallbackStruct>() as u32,
                )
            };
            if status != 0 {
                self.failed = true;
                eprintln!("AU render callback failed: {status}");
                return;
            }
        }

        if std::env::var_os("AU_DEBUG").is_some() {
            let mut elements = 0u32;
            let mut size = size_of::<u32>() as u32;
            let st = unsafe {
                ca::AudioUnitGetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_ElementCount,
                    ca::kAudioUnitScope_Input,
                    0,
                    &mut elements as *mut u32 as *mut c_void,
                    &mut size,
                )
            };
            eprintln!("AU_DEBUG input elements status={st} count={elements}");
            let mut elements = 0u32;
            let mut size = size_of::<u32>() as u32;
            let st = unsafe {
                ca::AudioUnitGetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_ElementCount,
                    ca::kAudioUnitScope_Output,
                    0,
                    &mut elements as *mut u32 as *mut c_void,
                    &mut size,
                )
            };
            eprintln!("AU_DEBUG output elements status={st} count={elements}");

            let mut actual: ca::AudioStreamBasicDescription = Default::default();
            let mut size = size_of::<ca::AudioStreamBasicDescription>() as u32;
            let st = unsafe {
                ca::AudioUnitGetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_StreamFormat,
                    ca::kAudioUnitScope_Input,
                    0,
                    &mut actual as *mut _ as *mut c_void,
                    &mut size,
                )
            };
            eprintln!(
                "AU_DEBUG input format status={st} ch={} flags={:#x} bpf={} bpp={}",
                actual.mChannelsPerFrame,
                actual.mFormatFlags,
                actual.mBytesPerFrame,
                actual.mBytesPerPacket
            );
            let mut actual: ca::AudioStreamBasicDescription = Default::default();
            let mut size = size_of::<ca::AudioStreamBasicDescription>() as u32;
            let st = unsafe {
                ca::AudioUnitGetProperty(
                    self.unit,
                    ca::kAudioUnitProperty_StreamFormat,
                    ca::kAudioUnitScope_Output,
                    0,
                    &mut actual as *mut _ as *mut c_void,
                    &mut size,
                )
            };
            eprintln!(
                "AU_DEBUG output format status={st} ch={} flags={:#x} bpf={} bpp={}",
                actual.mChannelsPerFrame,
                actual.mFormatFlags,
                actual.mBytesPerFrame,
                actual.mBytesPerPacket
            );
        }

        let status = unsafe { ca::AudioUnitInitialize(self.unit) };
        if status != 0 {
            self.failed = true;
            eprintln!("AU initialize failed: {status}");
            return;
        }

        self.update_latency(context.sample_rate);
        self.active = true;
    }

    fn reset(&mut self) {
        if self.active {
            unsafe {
                ca::AudioUnitReset(self.unit, ca::kAudioUnitScope_Global, 0);
            }
        }
    }

    fn process(&mut self, context: &ProcessContext, audio: &mut AudioBuffer<'_>, events: &[Event]) {
        if self.failed || !self.active {
            return;
        }
        let frames = audio.frames.min(MAX_BLOCK_SIZE);
        if frames == 0 || audio.output_channels < 2 {
            return;
        }

        for event in events {
            self.handle_event(event);
        }

        self.render.frames = frames;
        self.render.channels = audio.input_channels.min(2);
        for channel in 0..2 {
            self.render.inputs[channel] = if channel < self.render.channels {
                audio.input_mut(channel).as_mut_ptr() as *const f32
            } else {
                self.silence.as_ptr()
            };
        }

        let left = audio.output_mut(0).as_mut_ptr() as *mut c_void;
        let right = audio.output_mut(1).as_mut_ptr() as *mut c_void;
        let mut buffers = AudioBufferList2 {
            number_buffers: 2,
            buffers: [
                ca::AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: (frames * 4) as u32,
                    mData: left,
                },
                ca::AudioBuffer {
                    mNumberChannels: 1,
                    mDataByteSize: (frames * 4) as u32,
                    mData: right,
                },
            ],
        };

        let mut flags = 0u32;
        let timestamp = ca::AudioTimeStamp {
            mSampleTime: context.sample_pos as f64,
            mFlags: ca::kAudioTimeStampSampleTimeValid,
            ..Default::default()
        };

        let status = unsafe {
            ca::AudioUnitRender(
                self.unit,
                &mut flags,
                &timestamp,
                0,
                frames as u32,
                &mut buffers as *mut AudioBufferList2 as *mut ca::AudioBufferList,
            )
        };
        if status != 0 {
            self.failed = true;
            eprintln!("AU render failed: {status}");
        }
    }

    fn save_state(&self) -> State {
        State::empty()
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        if state.bytes.is_empty() {
            Ok(())
        } else {
            Err(StateError::Malformed(
                "Audio Unit state is not wired yet".into(),
            ))
        }
    }
}

impl Drop for AuPlugin {
    fn drop(&mut self) {
        unsafe {
            ca::AudioUnitUninitialize(self.unit);
            ca::AudioComponentInstanceDispose(self.unit);
        }
    }
}

unsafe extern "C" fn render_input(
    reference: *mut c_void,
    _flags: *mut ca::AudioUnitRenderActionFlags,
    _timestamp: *const ca::AudioTimeStamp,
    _bus: u32,
    frames: u32,
    io_data: *mut ca::AudioBufferList,
) -> ca::OSStatus {
    if reference.is_null() || io_data.is_null() {
        return 0;
    }
    let state = unsafe { &*(reference as *const RenderState) };
    let list = unsafe { &mut *io_data };
    let count = (frames as usize).min(state.frames);
    let buffers = list.mBuffers.as_mut_ptr();

    for index in 0..list.mNumberBuffers as usize {
        let buffer = unsafe { &mut *buffers.add(index) };
        let channel = if state.channels == 0 {
            0
        } else {
            index.min(state.channels - 1)
        };
        let source = state.inputs[channel];
        if source.is_null() {
            continue;
        }
        if buffer.mData.is_null() {
            buffer.mData = source as *mut c_void;
            buffer.mDataByteSize = (count * 4) as u32;
            continue;
        }
        let destination = buffer.mData as *mut f32;
        if buffer.mNumberChannels <= 1 {
            unsafe {
                ptr::copy_nonoverlapping(source, destination, count);
                if count < frames as usize {
                    ptr::write_bytes(destination.add(count), 0, frames as usize - count);
                }
            }
        } else {
            // Interleaved fallback: duplicate this channel across the slots.
            for frame in 0..frames as usize {
                let value = if frame < count {
                    unsafe { *source.add(frame) }
                } else {
                    0.0
                };
                for slot in 0..buffer.mNumberChannels as usize {
                    unsafe {
                        *destination.add(frame * buffer.mNumberChannels as usize + slot) = value;
                    }
                }
            }
        }
    }
    0
}

fn wildcard() -> ca::AudioComponentDescription {
    ca::AudioComponentDescription {
        componentType: 0,
        componentSubType: 0,
        componentManufacturer: 0,
        componentFlags: 0,
        componentFlagsMask: 0,
    }
}

fn find_component(name: &str) -> Option<(ca::AudioComponent, ca::AudioComponentDescription)> {
    let query = name.to_ascii_lowercase();
    let description = wildcard();
    let mut component = unsafe { ca::AudioComponentFindNext(ptr::null_mut(), &description) };
    while !component.is_null() {
        let candidate = component_name(component).to_ascii_lowercase();
        if candidate.contains(&query) {
            let mut found = wildcard();
            if unsafe { ca::AudioComponentGetDescription(component, &mut found) } == 0 {
                return Some((component, found));
            }
        }
        component = unsafe { ca::AudioComponentFindNext(component, &description) };
    }
    None
}

fn component_name(component: ca::AudioComponent) -> String {
    let mut name: ca::CFStringRef = ptr::null();
    let status = unsafe { ca::AudioComponentCopyName(component, &mut name) };
    if status != 0 || name.is_null() {
        return String::new();
    }
    let mut buffer = [0 as c_char; 512];
    let ok = unsafe {
        CFStringGetCString(
            name,
            buffer.as_mut_ptr(),
            buffer.len() as isize,
            CF_STRING_ENCODING_UTF8,
        )
    };
    unsafe { CFRelease(name as *const c_void) };
    if ok == 0 {
        return String::new();
    }
    unsafe { CStr::from_ptr(buffer.as_ptr()) }
        .to_string_lossy()
        .into_owned()
}

fn parameter_unit(unit: ca::AudioUnitParameterUnit) -> Unit {
    match unit {
        ca::kAudioUnitParameterUnit_Hertz => Unit::Hertz,
        ca::kAudioUnitParameterUnit_Percent => Unit::Percent,
        ca::kAudioUnitParameterUnit_Decibels => Unit::Decibels,
        ca::kAudioUnitParameterUnit_Milliseconds => Unit::Milliseconds,
        _ => Unit::None,
    }
}
