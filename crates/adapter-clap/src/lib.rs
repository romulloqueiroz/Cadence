//! CLAP hosting adapter: loads a `.clap` plugin and exposes it as a
//! [`plugin_api::Plugin`].
//!
//! Scope: audio processing for effects/instruments with audio ports,
//! descriptors, parameters, MIDI/note events, transport, bounded state, latency
//! and tail, per-instance host requests and creator-thread lifecycle dispatch.

mod main_thread;
use main_thread::HostState;
use plugin_api::MainThreadTasks;

use clap_sys::audio_buffer::clap_audio_buffer;
use clap_sys::entry::clap_plugin_entry;
use clap_sys::events::*;
use clap_sys::ext::audio_ports::{
    clap_audio_port_info, clap_host_audio_ports, clap_plugin_audio_ports, CLAP_AUDIO_PORT_IS_MAIN,
    CLAP_EXT_AUDIO_PORTS,
};
use clap_sys::ext::latency::{clap_host_latency, clap_plugin_latency, CLAP_EXT_LATENCY};
use clap_sys::ext::log::{
    clap_host_log, clap_log_severity, CLAP_EXT_LOG, CLAP_LOG_DEBUG, CLAP_LOG_ERROR, CLAP_LOG_FATAL,
    CLAP_LOG_HOST_MISBEHAVING, CLAP_LOG_INFO, CLAP_LOG_PLUGIN_MISBEHAVING, CLAP_LOG_WARNING,
};
use clap_sys::ext::note_ports::*;
use clap_sys::ext::params::{
    clap_host_params, clap_param_info, clap_plugin_params, CLAP_EXT_PARAMS, CLAP_PARAM_IS_BYPASS,
    CLAP_PARAM_IS_READONLY, CLAP_PARAM_IS_STEPPED, CLAP_PARAM_RESCAN_ALL, CLAP_PARAM_RESCAN_INFO,
};
use clap_sys::ext::state::{clap_host_state, clap_plugin_state, CLAP_EXT_STATE};
use clap_sys::ext::tail::{clap_host_tail, clap_plugin_tail, CLAP_EXT_TAIL};
use clap_sys::ext::thread_check::{clap_host_thread_check, CLAP_EXT_THREAD_CHECK};
use clap_sys::factory::plugin_factory::{clap_plugin_factory, CLAP_PLUGIN_FACTORY_ID};
use clap_sys::host::clap_host;
use clap_sys::plugin::{clap_plugin, clap_plugin_descriptor};
use clap_sys::plugin_features::{
    CLAP_PLUGIN_FEATURE_AUDIO_EFFECT, CLAP_PLUGIN_FEATURE_INSTRUMENT,
    CLAP_PLUGIN_FEATURE_NOTE_EFFECT,
};
use clap_sys::process::{clap_process, CLAP_PROCESS_ERROR};
use clap_sys::stream::{clap_istream, clap_ostream};
use clap_sys::version::{clap_version_is_compatible, CLAP_VERSION};
use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, EventKind, ParamFlags, ParameterInfo, Plugin,
    PluginKind, ProcessContext, State, StateError, UiNode, Unit, MAX_BLOCK_SIZE,
};
use std::ffi::{c_char, c_void, CStr, CString};
use std::fmt;
use std::mem::MaybeUninit;
use std::path::{Path, PathBuf};
use std::ptr;
use std::sync::atomic::Ordering;
use std::sync::Arc;

#[derive(Debug, Clone)]
pub enum ClapError {
    Load(String),
    MissingSymbol,
    IncompatibleVersion,
    Init,
    NoFactory,
    NoDescriptor,
    Create,
    NoAudioPorts,
    Api(String),
}

impl fmt::Display for ClapError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ClapError::Load(message) => write!(f, "CLAP load failed: {message}"),
            ClapError::MissingSymbol => write!(f, "missing clap_entry symbol"),
            ClapError::IncompatibleVersion => write!(f, "incompatible CLAP version"),
            ClapError::Init => write!(f, "CLAP entry init failed"),
            ClapError::NoFactory => write!(f, "CLAP plugin factory missing"),
            ClapError::NoDescriptor => write!(f, "CLAP descriptor missing"),
            ClapError::Create => write!(f, "CLAP create_plugin failed"),
            ClapError::NoAudioPorts => write!(f, "plugin has no audio-ports extension"),
            ClapError::Api(message) => write!(f, "CLAP API error: {message}"),
        }
    }
}

impl std::error::Error for ClapError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DescriptorInfo {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: PluginKind,
}

pub struct ClapModule {
    handle: *mut c_void,
    entry: *const clap_plugin_entry,
    factory: *const clap_plugin_factory,
}

// Only used to transfer teardown to the creator thread. The public module
// remains !Send, so clients cannot move entry calls onto an arbitrary thread.
struct ModuleTeardown(Option<ClapModule>);
unsafe impl Send for ModuleTeardown {}
impl ModuleTeardown {
    fn finish(self) {
        drop(self.0);
    }
}

enum Activation {
    Active,
    Reload,
    Failed,
}

impl ClapModule {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, ClapError> {
        let path = path.as_ref();
        let entry_path = resolve_entry(path)?;
        let c_entry = CString::new(entry_path.to_string_lossy().as_bytes())
            .map_err(|_| ClapError::Load("invalid entry path".into()))?;

        let handle = unsafe { libc::dlopen(c_entry.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(ClapError::Load(last_error()));
        }

        let symbol = unsafe { libc::dlsym(handle, b"clap_entry\0".as_ptr() as *const c_char) };
        if symbol.is_null() {
            let message = last_error();
            unsafe { libc::dlclose(handle) };
            return Err(ClapError::Load(message));
        }

        let entry = symbol as *const clap_plugin_entry;
        let version = unsafe { (*entry).clap_version };
        if !clap_version_is_compatible(version) {
            unsafe { libc::dlclose(handle) };
            return Err(ClapError::IncompatibleVersion);
        }

        let Some(init) = (unsafe { (*entry).init }) else {
            unsafe { libc::dlclose(handle) };
            return Err(ClapError::Init);
        };
        let c_original = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| ClapError::Load("invalid plugin path".into()))?;
        if !unsafe { init(c_original.as_ptr()) } {
            unsafe { libc::dlclose(handle) };
            return Err(ClapError::Init);
        }

        let Some(get_factory) = (unsafe { (*entry).get_factory }) else {
            deinit(entry);
            unsafe { libc::dlclose(handle) };
            return Err(ClapError::NoFactory);
        };
        let factory = unsafe { get_factory(CLAP_PLUGIN_FACTORY_ID.as_ptr()) };
        if factory.is_null() {
            deinit(entry);
            unsafe { libc::dlclose(handle) };
            return Err(ClapError::NoFactory);
        }

        Ok(ClapModule {
            handle,
            entry,
            factory: factory as *const clap_plugin_factory,
        })
    }

    pub fn descriptors(&self) -> Vec<DescriptorInfo> {
        let factory = unsafe { &*self.factory };
        let Some(count_fn) = factory.get_plugin_count else {
            return Vec::new();
        };
        let Some(descriptor_fn) = factory.get_plugin_descriptor else {
            return Vec::new();
        };
        let count = unsafe { count_fn(self.factory) };
        let mut descriptors = Vec::new();
        for index in 0..count {
            let descriptor = unsafe { descriptor_fn(self.factory, index) };
            if descriptor.is_null() {
                continue;
            }
            let descriptor = unsafe { &*descriptor };
            descriptors.push(DescriptorInfo {
                id: cstr(descriptor.id),
                name: cstr(descriptor.name),
                vendor: cstr(descriptor.vendor),
                version: cstr(descriptor.version),
                kind: kind_from_features(descriptor.features),
            });
        }
        descriptors
    }

    pub fn create(self, plugin_id: Option<&str>) -> Result<ClapPlugin, ClapError> {
        let factory = unsafe { &*self.factory };
        let Some(count_fn) = factory.get_plugin_count else {
            return Err(ClapError::NoFactory);
        };
        let Some(descriptor_fn) = factory.get_plugin_descriptor else {
            return Err(ClapError::NoDescriptor);
        };
        let Some(create_fn) = factory.create_plugin else {
            return Err(ClapError::Create);
        };

        let count = unsafe { count_fn(self.factory) };
        let mut chosen: Option<(*const clap_plugin_descriptor, CString)> = None;
        for index in 0..count {
            let descriptor = unsafe { descriptor_fn(self.factory, index) };
            if descriptor.is_null() {
                continue;
            }
            let id = cstr(unsafe { (*descriptor).id });
            let matches = plugin_id.map_or(index == 0, |wanted| wanted == id);
            if matches {
                let c_id = CString::new(id).map_err(|_| ClapError::NoDescriptor)?;
                chosen = Some((descriptor, c_id));
                break;
            }
        }
        let Some((descriptor, c_id)) = chosen else {
            return Err(ClapError::NoDescriptor);
        };

        let requests = HostState::new();
        let host = Box::new(clap_host {
            clap_version: CLAP_VERSION,
            host_data: Arc::as_ptr(&requests) as *mut c_void,
            name: b"Platform Host\0".as_ptr() as *const c_char,
            vendor: b"Platform\0".as_ptr() as *const c_char,
            url: b"https://example.invalid\0".as_ptr() as *const c_char,
            version: b"0.1.0\0".as_ptr() as *const c_char,
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        });

        let plugin = unsafe {
            create_fn(
                self.factory,
                host.as_ref() as *const clap_host,
                c_id.as_ptr(),
            )
        };
        if plugin.is_null() {
            return Err(ClapError::Create);
        }

        let Some(init) = (unsafe { (*plugin).init }) else {
            unsafe { destroy_plugin(plugin) };
            return Err(ClapError::Init);
        };
        if !unsafe { init(plugin) } {
            unsafe { destroy_plugin(plugin) };
            return Err(ClapError::Init);
        }

        let mut instance = ClapPlugin {
            module: Some(self),
            host,
            requests: requests.clone(),
            plugin,
            descriptor: Descriptor {
                id: "clap",
                name: "CLAP",
                vendor: "",
                version: "0",
                kind: PluginKind::Effect,
            },
            capabilities: Capabilities::stereo_effect(),
            parameters: Vec::new(),
            variants: Vec::new(),
            parameter_ids: Vec::new(),
            native_notes: false,
            input_ports: Vec::new(),
            output_ports: Vec::new(),
            input_ptrs: Vec::new(),
            output_ptrs: Vec::new(),
            input_buffers: Vec::new(),
            output_buffers: Vec::new(),
            out_events: empty_output_events(),
            output: Box::new(OutputCollector::new(requests)),
            silence: vec![0.0; MAX_BLOCK_SIZE],
            discard: vec![0.0; MAX_BLOCK_SIZE],
            active: false,
            processing: false,
            context: None,
            failed: false,
        };

        instance.discover(descriptor)?;
        instance.requests.attach(
            instance.plugin,
            &instance.parameter_ids,
            &instance.parameters,
        );
        instance.out_events = clap_output_events {
            ctx: instance.output.as_mut() as *mut OutputCollector as *mut c_void,
            try_push: Some(output_event_push),
        };
        Ok(instance)
    }
}

impl Drop for ClapModule {
    fn drop(&mut self) {
        deinit(self.entry);
        unsafe {
            libc::dlclose(self.handle);
        }
    }
}

const MAX_OUTPUT_EVENTS: usize = 256;
const MAX_OUTPUT_SYSEX: usize = 16384;

struct OutputCollector {
    requests: Arc<HostState>,
    events: [Event; MAX_OUTPUT_EVENTS],
    count: usize,
    sysex: [u8; MAX_OUTPUT_SYSEX],
    sysex_len: usize,
}
impl OutputCollector {
    fn new(requests: Arc<HostState>) -> Self {
        Self {
            requests,
            events: [Event::note_on(0, 0, 0); MAX_OUTPUT_EVENTS],
            count: 0,
            sysex: [0; MAX_OUTPUT_SYSEX],
            sysex_len: 0,
        }
    }
    fn reset(&mut self) {
        self.count = 0;
        self.sysex_len = 0;
    }
    fn push(&mut self, event: Event) -> bool {
        if self.count == self.events.len() {
            return false;
        }
        self.events[self.count] = event;
        self.count += 1;
        true
    }
}

#[derive(Clone, Copy)]
struct InputPort {
    channels: usize,
    sidechain: bool,
}

pub struct ClapPlugin {
    module: Option<ClapModule>,
    host: Box<clap_host>,
    requests: Arc<HostState>,
    plugin: *const clap_plugin,
    descriptor: Descriptor,
    capabilities: Capabilities,
    parameters: Vec<ParameterInfo>,
    variants: Vec<Descriptor>,
    parameter_ids: Vec<u32>,
    native_notes: bool,
    input_ports: Vec<InputPort>,
    output_ports: Vec<usize>,
    input_ptrs: Vec<Vec<*mut f32>>,
    output_ptrs: Vec<Vec<*mut f32>>,
    input_buffers: Vec<clap_audio_buffer>,
    output_buffers: Vec<clap_audio_buffer>,
    out_events: clap_output_events,
    output: Box<OutputCollector>,
    silence: Vec<f32>,
    discard: Vec<f32>,
    active: bool,
    processing: bool,
    context: Option<ProcessContext>,
    failed: bool,
}

unsafe impl Send for ClapPlugin {}

impl ClapPlugin {
    fn stop_processing(&mut self) {
        if self.processing {
            let _audio_thread = AudioThreadScope::enter();
            if let Some(stop) = unsafe { (*self.plugin).stop_processing } {
                unsafe { stop(self.plugin) };
            }
            self.processing = false;
        }
    }

    fn flush_parameters(&mut self) {
        let extension = self.extension(CLAP_EXT_PARAMS.as_ptr());
        if extension.is_null() {
            return;
        }
        let Some(flush) = (unsafe { (*(extension as *const clap_plugin_params)).flush }) else {
            return;
        };
        let input = clap_input_events {
            ctx: ptr::null_mut(),
            size: Some(empty_input_size),
            get: Some(empty_input_get),
        };
        self.output.reset();
        if self.active {
            let _audio_thread = AudioThreadScope::enter();
            unsafe { flush(self.plugin, &input, &self.out_events) };
        } else {
            let plugin = self.plugin as usize;
            let requests = self.requests.clone();
            self.requests.call(move || {
                let input = clap_input_events {
                    ctx: ptr::null_mut(),
                    size: Some(empty_input_size),
                    get: Some(empty_input_get),
                };
                let mut output = OutputCollector::new(requests);
                let events = clap_output_events {
                    ctx: (&mut output as *mut OutputCollector).cast(),
                    try_push: Some(output_event_push),
                };
                unsafe { flush(plugin as *const clap_plugin, &input, &events) };
            });
        }
        self.requests.refresh.store(true, Ordering::Release);
    }

    pub fn set_variants(&mut self, variants: Vec<DescriptorInfo>) {
        self.variants = variants
            .into_iter()
            .map(|variant| Descriptor {
                id: Box::leak(variant.id.into_boxed_str()),
                name: Box::leak(variant.name.into_boxed_str()),
                vendor: Box::leak(variant.vendor.into_boxed_str()),
                version: Box::leak(variant.version.into_boxed_str()),
                kind: variant.kind,
            })
            .collect();
    }

    fn discover(&mut self, descriptor: *const clap_plugin_descriptor) -> Result<(), ClapError> {
        let descriptor = unsafe { &*descriptor };
        let id = intern(descriptor.id);
        let name = intern(descriptor.name);
        let vendor = intern(descriptor.vendor);
        let version = intern(descriptor.version);
        self.descriptor = Descriptor {
            id,
            name,
            vendor,
            version,
            kind: kind_from_features(descriptor.features),
        };

        self.discover_ports()?;
        self.discover_parameters();
        let extension = self.extension(CLAP_EXT_NOTE_PORTS.as_ptr());
        if !extension.is_null() {
            let ports = unsafe { &*(extension as *const clap_plugin_note_ports) };
            if let Some(get) = ports.get {
                let mut info = MaybeUninit::<clap_note_port_info>::zeroed();
                if unsafe { get(self.plugin, 0, true, info.as_mut_ptr()) } {
                    self.native_notes = unsafe { info.assume_init() }.supported_dialects
                        & CLAP_NOTE_DIALECT_CLAP
                        != 0;
                }
            }
        }

        self.capabilities = Capabilities {
            inputs: self
                .input_ports
                .iter()
                .filter(|port| !port.sidechain)
                .map(|port| port.channels)
                .sum(),
            outputs: self.output_ports.iter().sum(),
            sidechain_inputs: self
                .input_ports
                .iter()
                .filter(|port| port.sidechain)
                .map(|port| port.channels)
                .sum(),
            latency_samples: self.latency(),
            tail_samples: self.tail(),
        };
        if self.capabilities.outputs == 0 {
            return Err(ClapError::NoAudioPorts);
        }

        self.input_ptrs = self
            .input_ports
            .iter()
            .map(|port| vec![ptr::null_mut(); port.channels])
            .collect();
        self.output_ptrs = self
            .output_ports
            .iter()
            .map(|channels| vec![ptr::null_mut(); *channels])
            .collect();
        self.input_buffers = self
            .input_ptrs
            .iter_mut()
            .map(|channels| audio_buffer(channels))
            .collect();
        self.output_buffers = self
            .output_ptrs
            .iter_mut()
            .map(|channels| audio_buffer(channels))
            .collect();

        Ok(())
    }

    fn discover_ports(&mut self) -> Result<(), ClapError> {
        let extension = self.extension(CLAP_EXT_AUDIO_PORTS.as_ptr());
        if extension.is_null() {
            return Err(ClapError::NoAudioPorts);
        }
        let ports = unsafe { &*(extension as *const clap_plugin_audio_ports) };
        let (Some(count), Some(get)) = (ports.count, ports.get) else {
            return Err(ClapError::NoAudioPorts);
        };

        let mut inputs = Vec::new();
        let mut outputs = Vec::new();
        for is_input in [true, false] {
            let count = unsafe { count(self.plugin, is_input) };
            for index in 0..count {
                let mut info = MaybeUninit::<clap_audio_port_info>::zeroed();
                if unsafe { get(self.plugin, index, is_input, info.as_mut_ptr()) } {
                    let info = unsafe { info.assume_init() };
                    if is_input {
                        inputs.push(InputPort {
                            channels: info.channel_count as usize,
                            sidechain: info.flags & CLAP_AUDIO_PORT_IS_MAIN == 0,
                        });
                    } else {
                        outputs.push(info.channel_count as usize);
                    }
                }
            }
        }
        // Some plugins omit the main-port flag; keep the first port as the main
        // input and treat additional unflagged ports as aux input.
        if self.descriptor.kind != PluginKind::Instrument
            && !inputs.is_empty()
            && inputs.iter().all(|port| port.sidechain)
        {
            inputs[0].sidechain = false;
        }
        self.input_ports = inputs;
        self.output_ports = outputs;
        Ok(())
    }

    fn discover_parameters(&mut self) {
        let extension = self.extension(CLAP_EXT_PARAMS.as_ptr());
        if extension.is_null() {
            return;
        }
        let params = unsafe { &*(extension as *const clap_plugin_params) };
        let (Some(count), Some(get_info)) = (params.count, params.get_info) else {
            return;
        };
        let count = unsafe { count(self.plugin) };
        for index in 0..count {
            let mut info = MaybeUninit::<clap_param_info>::zeroed();
            if !unsafe { get_info(self.plugin, index, info.as_mut_ptr()) } {
                continue;
            }
            let info = unsafe { info.assume_init() };
            let name = unsafe { CStr::from_ptr(info.name.as_ptr()) }
                .to_string_lossy()
                .into_owned();
            if name.is_empty() {
                continue;
            }
            if self.parameters.len() >= plugin_api::MAX_PARAMETERS
                || !(info.min_value.is_finite()
                    && info.max_value.is_finite()
                    && info.default_value.is_finite())
                || info.min_value >= info.max_value
            {
                continue;
            }
            self.parameter_ids.push(info.id);
            self.parameters.push(ParameterInfo {
                id: self.parameters.len() as u32,
                name: Box::leak(name.into_boxed_str()),
                min: info.min_value as f32,
                max: info.max_value as f32,
                default: info.default_value as f32,
                unit: Unit::None,
                flags: ParamFlags {
                    stepped: info.flags & CLAP_PARAM_IS_STEPPED != 0,
                    read_only: info.flags & CLAP_PARAM_IS_READONLY != 0,
                    bypass: info.flags & CLAP_PARAM_IS_BYPASS != 0,
                },
            });
        }
    }

    fn latency(&self) -> usize {
        let extension = self.extension(CLAP_EXT_LATENCY.as_ptr());
        if extension.is_null() {
            return 0;
        }
        let latency = unsafe { &*(extension as *const clap_plugin_latency) };
        latency
            .get
            .map_or(0, |get| unsafe { get(self.plugin) } as usize)
    }

    fn tail(&self) -> usize {
        let extension = self.extension(CLAP_EXT_TAIL.as_ptr());
        if extension.is_null() {
            return 0;
        }
        let tail = unsafe { &*(extension as *const clap_plugin_tail) };
        tail.get
            .map_or(0, |get| unsafe { get(self.plugin) } as usize)
    }

    fn extension(&self, id: *const c_char) -> *const c_void {
        let Some(get_extension) = (unsafe { (*self.plugin).get_extension }) else {
            return ptr::null();
        };
        unsafe { get_extension(self.plugin, id) }
    }
}

impl Plugin for ClapPlugin {
    fn descriptor(&self) -> Descriptor {
        self.descriptor
    }

    fn variants(&self) -> Vec<Descriptor> {
        self.variants.clone()
    }

    fn capabilities(&self) -> Capabilities {
        self.capabilities
    }

    fn current_latency_samples(&self) -> usize {
        self.requests.latency.load(Ordering::Acquire)
    }

    fn current_tail_samples(&self) -> usize {
        self.requests.tail.load(Ordering::Acquire)
    }

    fn main_thread_tasks(&self) -> Option<Arc<dyn MainThreadTasks>> {
        Some(self.requests.clone())
    }

    fn service_host_requests(&mut self) {
        if self.requests.is_owner() {
            self.requests.run_pending();
        }
        if self.failed {
            return;
        }
        if self.requests.restart.swap(false, Ordering::AcqRel) {
            if let Some(context) = self.context {
                self.prepare(&context);
            }
        }
        // Cadence processes every supplied host block, including silence; it
        // deliberately does not suspend plugins on CLAP_PROCESS_SLEEP. A wake
        // request needs no synthetic audio or invented transport position.
        self.requests.process.store(false, Ordering::Release);
        if self.requests.flush.swap(false, Ordering::AcqRel) {
            self.flush_parameters();
        }
    }

    fn host_service_pending(&self) -> bool {
        self.requests.restart.load(Ordering::Acquire)
            || self.requests.flush.load(Ordering::Acquire)
            || self.requests.process.load(Ordering::Acquire)
            || (self.requests.is_owner() && self.requests.callback.load(Ordering::Acquire))
    }

    fn processing_error(&self) -> Option<&'static str> {
        self.failed
            .then_some("CLAP processing or activation failed")
    }

    fn fill_output_events(&mut self, events: &mut [Event], sysex: &mut [u8]) -> (usize, usize) {
        let count = self.output.count.min(events.len());
        events[..count].copy_from_slice(&self.output.events[..count]);
        let sysex_len = self.output.sysex_len.min(sysex.len());
        sysex[..sysex_len].copy_from_slice(&self.output.sysex[..sysex_len]);
        (count, sysex_len)
    }

    fn take_parameter_refresh_request(&self) -> bool {
        self.requests.values_ready.swap(false, Ordering::AcqRel)
    }

    fn take_reload_request(&self) -> bool {
        self.requests.reload.swap(false, Ordering::AcqRel)
    }

    fn native_gui_handle(&self) -> *mut c_void {
        self.plugin as *mut c_void
    }

    fn fill_parameter_values(&self, out: &mut [(u32, f32)]) -> usize {
        self.requests.snapshot(out)
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        self.parameters.clone()
    }

    fn parameter_key(&self, id: u32) -> String {
        self.parameter_ids
            .get(id as usize)
            .copied()
            .unwrap_or(id)
            .to_string()
    }

    fn ui(&self) -> Vec<UiNode> {
        Vec::new()
    }

    fn prepare(&mut self, context: &ProcessContext) {
        if self.failed {
            return;
        }
        self.stop_processing();
        let plugin = self.plugin as usize;
        let was_active = self.active;
        let sample_rate = context.sample_rate;
        let requests = self.requests.clone();
        let activation = self.requests.call(move || unsafe {
            let plugin = plugin as *const clap_plugin;
            if was_active {
                if let Some(deactivate) = (*plugin).deactivate {
                    deactivate(plugin);
                }
            }
            // A restart can apply structural changes during deactivate(). Do
            // not reactivate against stale buffers/cookies before slot reload.
            if was_active && requests.reload.load(Ordering::Acquire) {
                return Activation::Reload;
            }
            if (*plugin)
                .activate
                .is_some_and(|activate| activate(plugin, sample_rate, 1, MAX_BLOCK_SIZE as u32))
            {
                Activation::Active
            } else {
                Activation::Failed
            }
        });
        self.active = matches!(activation, Activation::Active);
        self.context = Some(*context);
        if !self.active {
            self.failed = matches!(activation, Activation::Failed);
            return;
        }
        {
            let _audio_thread = AudioThreadScope::enter();
            self.processing = unsafe { (*self.plugin).start_processing }
                .is_some_and(|start| unsafe { start(self.plugin) });
        }
        self.failed = !self.processing;
        self.requests.timing_dirty.store(true, Ordering::Release);
        self.requests.refresh.store(true, Ordering::Release);
        if self.requests.is_owner() {
            self.requests.run_pending();
        }
    }

    fn reset(&mut self) {
        if self.active {
            let _audio_thread = AudioThreadScope::enter();
            if let Some(reset) = unsafe { (*self.plugin).reset } {
                unsafe { reset(self.plugin) };
            }
        }
    }

    fn process(&mut self, context: &ProcessContext, audio: &mut AudioBuffer<'_>, events: &[Event]) {
        if self.failed || !self.processing {
            return;
        }
        let _audio_thread = AudioThreadScope::enter();
        self.output.reset();
        let frames = audio.frames.min(MAX_BLOCK_SIZE);
        if frames == 0 {
            return;
        }

        let silence = self.silence.as_mut_ptr();
        let discard = self.discard.as_mut_ptr();

        let mut main_channel = 0;
        let mut side_channel = 0;
        for (port, pointers) in self.input_ports.iter().zip(self.input_ptrs.iter_mut()) {
            for pointer in pointers.iter_mut() {
                if port.sidechain {
                    *pointer = if side_channel < audio.sidechain_channels {
                        audio.sidechain_mut(side_channel).as_mut_ptr()
                    } else {
                        silence
                    };
                    side_channel += 1;
                } else {
                    *pointer = if main_channel < audio.input_channels {
                        audio.input_mut(main_channel).as_mut_ptr()
                    } else {
                        silence
                    };
                    main_channel += 1;
                }
            }
        }

        let mut channel = 0;
        for pointers in &mut self.output_ptrs {
            for pointer in pointers.iter_mut() {
                *pointer = if channel < audio.output_channels {
                    audio.output_mut(channel).as_mut_ptr()
                } else {
                    discard
                };
                channel += 1;
            }
        }

        let mut event_buffer = EventBuffer::new();
        for event in events.iter().take(256) {
            event_buffer.push(event, self);
        }
        for index in 1..event_buffer.length {
            let event = event_buffer.events[index];
            let mut position = index;
            while position > 0
                && unsafe { event_buffer.events[position - 1].header.time > event.header.time }
            {
                event_buffer.events[position] = event_buffer.events[position - 1];
                position -= 1;
            }
            event_buffer.events[position] = event;
        }
        let input_events = clap_input_events {
            ctx: (&mut event_buffer as *mut EventBuffer).cast(),
            size: Some(event_size),
            get: Some(event_get),
        };
        let transport = clap_event_transport {
            header: event_header::<clap_event_transport>(0, CLAP_EVENT_TRANSPORT),
            flags: CLAP_TRANSPORT_HAS_TEMPO
                | CLAP_TRANSPORT_HAS_BEATS_TIMELINE
                | CLAP_TRANSPORT_HAS_SECONDS_TIMELINE
                | if context.playing {
                    CLAP_TRANSPORT_IS_PLAYING
                } else {
                    0
                },
            song_pos_beats: (context.ppq_position
                * clap_sys::fixedpoint::CLAP_BEATTIME_FACTOR as f64)
                as i64,
            song_pos_seconds: (context.sample_pos as f64 / context.sample_rate
                * clap_sys::fixedpoint::CLAP_SECTIME_FACTOR as f64)
                as i64,
            tempo: context.tempo,
            tempo_inc: 0.0,
            loop_start_beats: 0,
            loop_end_beats: 0,
            loop_start_seconds: 0,
            loop_end_seconds: 0,
            bar_start: 0,
            bar_number: 0,
            tsig_num: 4,
            tsig_denom: 4,
        };
        let process = clap_process {
            // Timeline seeks/preroll are not a monotonic CLAP steady clock.
            steady_time: -1,
            frames_count: frames as u32,
            transport: &transport,
            audio_inputs: self.input_buffers.as_ptr(),
            audio_outputs: self.output_buffers.as_mut_ptr(),
            audio_inputs_count: self.input_buffers.len() as u32,
            audio_outputs_count: self.output_buffers.len() as u32,
            in_events: &input_events,
            out_events: &self.out_events,
        };

        let Some(process_fn) = (unsafe { (*self.plugin).process }) else {
            self.failed = true;
            return;
        };
        let status = unsafe { process_fn(self.plugin, &process) };
        if status == CLAP_PROCESS_ERROR {
            self.failed = true;
        }
    }

    fn save_state(&self) -> State {
        self.try_save_state().unwrap_or_default()
    }

    fn try_save_state(&self) -> Result<State, StateError> {
        let plugin = self.plugin as usize;
        self.requests
            .call(move || save_clap_state(plugin as *const clap_plugin))
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        let plugin = self.plugin as usize;
        let state = state.clone();
        let result = self
            .requests
            .call(move || load_clap_state(plugin as *const clap_plugin, &state));
        self.requests.refresh.store(true, Ordering::Release);
        self.requests.timing_dirty.store(true, Ordering::Release);
        if self.requests.is_owner() {
            self.requests.run_pending();
        }
        result
    }
}

fn save_clap_state(plugin: *const clap_plugin) -> Result<State, StateError> {
    let extension = unsafe { (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_STATE.as_ptr()) };
    if extension.is_null() {
        return Ok(State::empty());
    }
    let state = unsafe { &*(extension as *const clap_plugin_state) };
    let Some(save) = state.save else {
        return Err(StateError::Malformed(
            "CLAP state save is unavailable".into(),
        ));
    };
    let mut writer = StateWriter {
        bytes: Vec::new(),
        failed: false,
    };
    let stream = clap_ostream {
        ctx: (&mut writer as *mut StateWriter).cast(),
        write: Some(state_write),
    };
    if !unsafe { save(plugin, &stream) } || writer.failed {
        return Err(StateError::Malformed(
            "CLAP state save failed or exceeds 64 KiB".into(),
        ));
    }
    Ok(State {
        schema_version: 1,
        bytes: writer.bytes,
    })
}

fn load_clap_state(plugin: *const clap_plugin, state: &State) -> Result<(), StateError> {
    if state.schema_version > 1 {
        return Err(StateError::SchemaTooNew {
            found: state.schema_version,
            supported: 1,
        });
    }
    let extension = unsafe { (*plugin).get_extension.unwrap()(plugin, CLAP_EXT_STATE.as_ptr()) };
    if extension.is_null() {
        return if state.bytes.is_empty() {
            Ok(())
        } else {
            Err(StateError::Malformed("CLAP has no state extension".into()))
        };
    }
    let extension = unsafe { &*(extension as *const clap_plugin_state) };
    let Some(load) = extension.load else {
        return Err(StateError::Malformed(
            "CLAP state load is unavailable".into(),
        ));
    };
    let mut reader = StateReader {
        bytes: &state.bytes,
        position: 0,
    };
    let stream = clap_istream {
        ctx: (&mut reader as *mut StateReader).cast(),
        read: Some(state_read),
    };
    if unsafe { load(plugin, &stream) } {
        Ok(())
    } else {
        Err(StateError::Malformed("CLAP rejected state".into()))
    }
}

impl Drop for ClapPlugin {
    fn drop(&mut self) {
        self.stop_processing();
        self.requests.shutting_down.store(true, Ordering::Release);
        let plugin = self.plugin as usize;
        let was_active = self.active;
        let module = ModuleTeardown(self.module.take());
        let requests = self.requests.clone();
        self.requests.teardown(move || unsafe {
            let plugin = plugin as *const clap_plugin;
            if was_active {
                if let Some(deactivate) = (*plugin).deactivate {
                    deactivate(plugin);
                }
            }
            requests.detach();
            destroy_plugin(plugin);
            module.finish();
        });
        let _ = &self.host;
    }
}

pub fn is_clap_path(path: &Path) -> bool {
    let text = path.to_string_lossy();
    text.ends_with(".clap") || text.contains(".clap/") || text.contains(".clap\\")
}

fn resolve_entry(path: &Path) -> Result<PathBuf, ClapError> {
    if path.is_dir() {
        let macos = path.join("Contents/MacOS");
        if let Ok(entries) = std::fs::read_dir(&macos) {
            for entry in entries.flatten() {
                let candidate = entry.path();
                if candidate.is_file() {
                    return Ok(candidate);
                }
            }
        }
        return Err(ClapError::Load(format!(
            "no plugin binary in {}",
            macos.display()
        )));
    }
    if path.is_file() {
        Ok(path.to_path_buf())
    } else {
        Err(ClapError::Load(format!("{} not found", path.display())))
    }
}

fn kind_from_features(features: *const *const c_char) -> PluginKind {
    if features.is_null() {
        return PluginKind::Effect;
    }
    let mut kind = PluginKind::Effect;
    let mut index = 0isize;
    loop {
        let feature = unsafe { *features.offset(index) };
        if feature.is_null() {
            break;
        }
        let feature = unsafe { CStr::from_ptr(feature) };
        if feature == CLAP_PLUGIN_FEATURE_INSTRUMENT {
            kind = PluginKind::Instrument;
        } else if feature == CLAP_PLUGIN_FEATURE_NOTE_EFFECT {
            kind = PluginKind::MidiEffect;
        } else if feature == CLAP_PLUGIN_FEATURE_AUDIO_EFFECT {
            kind = PluginKind::Effect;
        }
        index += 1;
    }
    kind
}

fn audio_buffer(channels: &mut [*mut f32]) -> clap_audio_buffer {
    clap_audio_buffer {
        data32: channels.as_mut_ptr(),
        data64: ptr::null_mut(),
        channel_count: channels.len() as u32,
        latency: 0,
        constant_mask: 0,
    }
}

unsafe extern "C" fn empty_input_size(_: *const clap_input_events) -> u32 {
    0
}
unsafe extern "C" fn empty_input_get(
    _: *const clap_input_events,
    _: u32,
) -> *const clap_event_header {
    ptr::null()
}

fn empty_output_events() -> clap_output_events {
    clap_output_events {
        ctx: ptr::null_mut(),
        try_push: Some(empty_event_push),
    }
}

unsafe extern "C" fn empty_event_push(
    _: *const clap_output_events,
    _: *const clap_event_header,
) -> bool {
    false
}

unsafe extern "C" fn output_event_push(
    list: *const clap_output_events,
    header: *const clap_event_header,
) -> bool {
    if list.is_null() || header.is_null() {
        return false;
    }
    let collector = unsafe { &mut *((*list).ctx as *mut OutputCollector) };
    let header_ptr = header;
    let header = unsafe { &*header_ptr };
    let frame = header.time as usize;
    if header.space_id != CLAP_CORE_EVENT_SPACE_ID {
        return false;
    }
    match header.type_ {
        CLAP_EVENT_PARAM_VALUE => {
            if (header.size as usize) < std::mem::size_of::<clap_event_param_value>() {
                return false;
            }
            collector.requests.refresh.store(true, Ordering::Release);
            true
        }
        CLAP_EVENT_NOTE_ON | CLAP_EVENT_NOTE_OFF => {
            let note = unsafe { &*(header_ptr as *const clap_event_note) };
            collector.push(Event {
                frame,
                kind: if header.type_ == CLAP_EVENT_NOTE_ON {
                    EventKind::NoteOn
                } else {
                    EventKind::NoteOff
                },
                channel: note.channel.clamp(0, 15) as u8,
                data1: note.key.clamp(0, 127) as u16,
                data2: 0,
                value: note.velocity.clamp(0.0, 1.0) as f32,
            })
        }
        CLAP_EVENT_NOTE_EXPRESSION => {
            let expression = unsafe { &*(header_ptr as *const clap_event_note_expression) };
            if expression.expression_id != CLAP_NOTE_EXPRESSION_PRESSURE {
                return true;
            }
            collector.push(Event {
                frame,
                kind: EventKind::PolyPressure,
                channel: expression.channel.clamp(0, 15) as u8,
                data1: expression.key.clamp(0, 127) as u16,
                data2: 0,
                value: expression.value.clamp(0.0, 1.0) as f32,
            })
        }
        CLAP_EVENT_MIDI => {
            let midi = unsafe { &*(header_ptr as *const clap_event_midi) };
            let status = midi.data[0] & 0xf0;
            let channel = midi.data[0] & 0x0f;
            let (kind, data1, value) = match status {
                0x80 => (
                    EventKind::NoteOff,
                    midi.data[1],
                    midi.data[2] as f32 / 127.0,
                ),
                0x90 => (EventKind::NoteOn, midi.data[1], midi.data[2] as f32 / 127.0),
                0xa0 => (
                    EventKind::PolyPressure,
                    midi.data[1],
                    midi.data[2] as f32 / 127.0,
                ),
                0xb0 => (
                    EventKind::ControlChange,
                    midi.data[1],
                    midi.data[2] as f32 / 127.0,
                ),
                0xc0 => (
                    EventKind::ProgramChange,
                    midi.data[1],
                    midi.data[1] as f32 / 127.0,
                ),
                0xd0 => (EventKind::ChannelPressure, 0, midi.data[1] as f32 / 127.0),
                0xe0 => (
                    EventKind::PitchBend,
                    0,
                    (((midi.data[2] as i32) << 7 | midi.data[1] as i32) as f32 / 8192.0) - 1.0,
                ),
                _ => return true,
            };
            collector.push(Event {
                frame,
                kind,
                channel,
                data1: data1 as u16,
                data2: 0,
                value,
            })
        }
        CLAP_EVENT_MIDI_SYSEX => {
            let sysex = unsafe { &*(header_ptr as *const clap_event_midi_sysex) };
            if sysex.buffer.is_null() || sysex.size == 0 {
                return true;
            }
            let length = sysex.size as usize;
            if collector.sysex_len.saturating_add(length) > collector.sysex.len()
                || length > u16::MAX as usize
            {
                return false;
            }
            let offset = collector.sysex_len;
            unsafe {
                std::ptr::copy_nonoverlapping(
                    sysex.buffer,
                    collector.sysex.as_mut_ptr().add(offset),
                    length,
                );
            }
            collector.sysex_len += length;
            collector.push(Event {
                frame,
                kind: EventKind::SysEx,
                channel: 0,
                data1: offset.min(u16::MAX as usize) as u16,
                data2: length as u16,
                value: 0.0,
            })
        }
        _ => true,
    }
}

thread_local! {
    static AUDIO_THREAD: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}
struct AudioThreadScope {
    previous: bool,
}
impl AudioThreadScope {
    fn enter() -> Self {
        let previous = AUDIO_THREAD.with(|value| value.replace(true));
        Self { previous }
    }
}
impl Drop for AudioThreadScope {
    fn drop(&mut self) {
        AUDIO_THREAD.with(|value| value.set(self.previous));
    }
}
fn on_audio_thread() -> bool {
    AUDIO_THREAD.with(std::cell::Cell::get)
}
unsafe extern "C" fn host_log(
    _: *const clap_host,
    severity: clap_log_severity,
    message: *const c_char,
) {
    if message.is_null() || on_audio_thread() {
        return;
    }
    let prefix = match severity {
        CLAP_LOG_DEBUG => "debug",
        CLAP_LOG_INFO => "info",
        CLAP_LOG_WARNING => "warning",
        CLAP_LOG_ERROR => "error",
        CLAP_LOG_FATAL => "fatal",
        CLAP_LOG_HOST_MISBEHAVING => "host-misbehaving",
        CLAP_LOG_PLUGIN_MISBEHAVING => "plugin-misbehaving",
        _ => "log",
    };
    let message = unsafe { CStr::from_ptr(message) }.to_string_lossy();
    eprintln!("CLAP plugin {prefix}: {message}");
}
unsafe fn host_state<'a>(host: *const clap_host) -> Option<&'a HostState> {
    if host.is_null() {
        return None;
    }
    unsafe { ((*host).host_data as *const HostState).as_ref() }
}
unsafe extern "C" fn host_request_restart(host: *const clap_host) {
    if let Some(state) = unsafe { host_state(host) } {
        state.restart.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_request_process(host: *const clap_host) {
    if let Some(state) = unsafe { host_state(host) } {
        state.process.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_request_callback(host: *const clap_host) {
    if let Some(state) = unsafe { host_state(host) } {
        state.callback.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_is_main_thread(host: *const clap_host) -> bool {
    !on_audio_thread() && unsafe { host_state(host) }.is_some_and(HostState::is_owner)
}
unsafe extern "C" fn host_is_audio_thread(_: *const clap_host) -> bool {
    on_audio_thread()
}
unsafe extern "C" fn host_changed(host: *const clap_host) {
    if let Some(state) = unsafe { host_state(host) } {
        state.timing_dirty.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_params_rescan(host: *const clap_host, flags: u32) {
    if let Some(state) = unsafe { host_state(host) } {
        state.refresh.store(true, Ordering::Release);
        if flags & (CLAP_PARAM_RESCAN_INFO | CLAP_PARAM_RESCAN_ALL) != 0 {
            state.reload.store(true, Ordering::Release);
        }
    }
}
unsafe extern "C" fn host_params_clear(host: *const clap_host, _: u32, _: u32) {
    if let Some(state) = unsafe { host_state(host) } {
        state.refresh.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_params_request_flush(host: *const clap_host) {
    if let Some(state) = unsafe { host_state(host) } {
        state.flush.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_state_mark_dirty(host: *const clap_host) {
    if let Some(state) = unsafe { host_state(host) } {
        state.refresh.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_audio_ports_rescan_supported(_: *const clap_host, _: u32) -> bool {
    false
}
unsafe extern "C" fn host_audio_ports_rescan(host: *const clap_host, _: u32) {
    if let Some(state) = unsafe { host_state(host) } {
        state.reload.store(true, Ordering::Release);
    }
}
unsafe extern "C" fn host_note_ports_supported(_: *const clap_host) -> clap_note_dialect {
    CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI
}
unsafe extern "C" fn host_note_ports_rescan(host: *const clap_host, _: u32) {
    if let Some(state) = unsafe { host_state(host) } {
        state.reload.store(true, Ordering::Release);
    }
}

static HOST_LOG: clap_host_log = clap_host_log {
    log: Some(host_log),
};
static HOST_THREAD_CHECK: clap_host_thread_check = clap_host_thread_check {
    is_main_thread: Some(host_is_main_thread),
    is_audio_thread: Some(host_is_audio_thread),
};
static HOST_LATENCY: clap_host_latency = clap_host_latency {
    changed: Some(host_changed),
};
static HOST_TAIL: clap_host_tail = clap_host_tail {
    changed: Some(host_changed),
};
static HOST_PARAMS: clap_host_params = clap_host_params {
    rescan: Some(host_params_rescan),
    clear: Some(host_params_clear),
    request_flush: Some(host_params_request_flush),
};
static HOST_STATE: clap_host_state = clap_host_state {
    mark_dirty: Some(host_state_mark_dirty),
};
static HOST_AUDIO_PORTS: clap_host_audio_ports = clap_host_audio_ports {
    is_rescan_flag_supported: Some(host_audio_ports_rescan_supported),
    rescan: Some(host_audio_ports_rescan),
};
static HOST_NOTE_PORTS: clap_host_note_ports = clap_host_note_ports {
    supported_dialects: Some(host_note_ports_supported),
    rescan: Some(host_note_ports_rescan),
};

unsafe extern "C" fn host_get_extension(_: *const clap_host, id: *const c_char) -> *const c_void {
    if id.is_null() {
        return ptr::null();
    }
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_LOG {
        (&HOST_LOG as *const clap_host_log).cast()
    } else if id == CLAP_EXT_THREAD_CHECK {
        (&HOST_THREAD_CHECK as *const clap_host_thread_check).cast()
    } else if id == CLAP_EXT_LATENCY {
        (&HOST_LATENCY as *const clap_host_latency).cast()
    } else if id == CLAP_EXT_TAIL {
        (&HOST_TAIL as *const clap_host_tail).cast()
    } else if id == CLAP_EXT_PARAMS {
        (&HOST_PARAMS as *const clap_host_params).cast()
    } else if id == CLAP_EXT_STATE {
        (&HOST_STATE as *const clap_host_state).cast()
    } else if id == CLAP_EXT_AUDIO_PORTS {
        (&HOST_AUDIO_PORTS as *const clap_host_audio_ports).cast()
    } else if id == CLAP_EXT_NOTE_PORTS {
        (&HOST_NOTE_PORTS as *const clap_host_note_ports).cast()
    } else {
        ptr::null()
    }
}

unsafe fn destroy_plugin(plugin: *const clap_plugin) {
    if let Some(destroy) = unsafe { (*plugin).destroy } {
        unsafe { destroy(plugin) };
    }
}

fn deinit(entry: *const clap_plugin_entry) {
    if let Some(deinit) = unsafe { (*entry).deinit } {
        unsafe { deinit() };
    }
}

fn cstr(pointer: *const c_char) -> String {
    if pointer.is_null() {
        return String::new();
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_string_lossy()
        .into_owned()
}

fn intern(pointer: *const c_char) -> &'static str {
    Box::leak(cstr(pointer).into_boxed_str())
}

fn last_error() -> String {
    let pointer = unsafe { libc::dlerror() };
    if pointer.is_null() {
        "unknown dynamic-loader error".into()
    } else {
        unsafe { CStr::from_ptr(pointer).to_string_lossy().into_owned() }
    }
}

#[derive(Clone, Copy)]
union InputEvent {
    header: clap_event_header,
    note: clap_event_note,
    parameter: clap_event_param_value,
    midi: clap_event_midi,
    expression: clap_event_note_expression,
}
struct EventBuffer {
    events: [InputEvent; 256],
    length: usize,
}
fn event_header<T>(time: u32, type_: u16) -> clap_event_header {
    clap_event_header {
        size: std::mem::size_of::<T>() as u32,
        time,
        space_id: CLAP_CORE_EVENT_SPACE_ID,
        type_,
        flags: 0,
    }
}
impl EventBuffer {
    fn new() -> Self {
        Self {
            events: [InputEvent {
                header: event_header::<clap_event_header>(0, 0),
            }; 256],
            length: 0,
        }
    }
    fn push(&mut self, event: &Event, plugin: &ClapPlugin) {
        if self.length >= self.events.len() {
            return;
        }
        let time = event.frame as u32;
        let input = if event.kind == EventKind::Parameter {
            let index = event.data1 as usize;
            let Some(parameter) = plugin.parameters.get(index) else {
                return;
            };
            if parameter.flags.read_only {
                return;
            }
            InputEvent {
                parameter: clap_event_param_value {
                    header: event_header::<clap_event_param_value>(time, CLAP_EVENT_PARAM_VALUE),
                    param_id: plugin.parameter_ids[index],
                    // Rescans can invalidate cookies before asynchronous slot
                    // replacement. CLAP permits hosts to address params by ID.
                    cookie: ptr::null_mut(),
                    note_id: -1,
                    port_index: -1,
                    channel: -1,
                    key: -1,
                    value: parameter.denormalize(event.value) as f64,
                },
            }
        } else if plugin.native_notes
            && matches!(event.kind, EventKind::NoteOn | EventKind::NoteOff)
        {
            InputEvent {
                note: clap_event_note {
                    header: event_header::<clap_event_note>(
                        time,
                        if event.kind == EventKind::NoteOn {
                            CLAP_EVENT_NOTE_ON
                        } else {
                            CLAP_EVENT_NOTE_OFF
                        },
                    ),
                    note_id: -1,
                    port_index: 0,
                    channel: event.channel as i16,
                    key: event.data1 as i16,
                    velocity: event.value as f64,
                },
            }
        } else if plugin.native_notes && event.kind == EventKind::PolyPressure {
            InputEvent {
                expression: clap_event_note_expression {
                    header: event_header::<clap_event_note_expression>(
                        time,
                        CLAP_EVENT_NOTE_EXPRESSION,
                    ),
                    expression_id: CLAP_NOTE_EXPRESSION_PRESSURE,
                    note_id: -1,
                    port_index: 0,
                    channel: event.channel as i16,
                    key: event.data1 as i16,
                    value: event.value as f64,
                },
            }
        } else {
            let channel = event.channel & 15;
            let key = event.data1.min(127) as u8;
            let value = (event.value.clamp(0.0, 1.0) * 127.0).round() as u8;
            let data = match event.kind {
                EventKind::NoteOn => [0x90 | channel, key, value],
                EventKind::NoteOff => [0x80 | channel, key, value],
                EventKind::PolyPressure => [0xa0 | channel, key, value],
                EventKind::ControlChange => [0xb0 | channel, key, value],
                EventKind::ProgramChange => [0xc0 | channel, key, 0],
                EventKind::ChannelPressure => [0xd0 | channel, value, 0],
                EventKind::PitchBend => {
                    let bend = ((event.value.clamp(-1.0, 1.0) + 1.0) * 8192.0)
                        .round()
                        .min(16383.0) as u16;
                    [0xe0 | channel, (bend & 127) as u8, (bend >> 7) as u8]
                }
                _ => return,
            };
            InputEvent {
                midi: clap_event_midi {
                    header: event_header::<clap_event_midi>(time, CLAP_EVENT_MIDI),
                    port_index: 0,
                    data,
                },
            }
        };
        self.events[self.length] = input;
        self.length += 1;
    }
}
unsafe extern "C" fn event_size(list: *const clap_input_events) -> u32 {
    unsafe { &*((*list).ctx as *const EventBuffer) }.length as u32
}
unsafe extern "C" fn event_get(
    list: *const clap_input_events,
    index: u32,
) -> *const clap_event_header {
    let buffer = unsafe { &*((*list).ctx as *const EventBuffer) };
    if index as usize >= buffer.length {
        ptr::null()
    } else {
        unsafe { &buffer.events[index as usize].header }
    }
}
struct StateWriter {
    bytes: Vec<u8>,
    failed: bool,
}
unsafe extern "C" fn state_write(
    stream: *const clap_ostream,
    buffer: *const c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || buffer.is_null() {
        return -1;
    }
    let writer = unsafe { &mut *((*stream).ctx as *mut StateWriter) };
    if size > 65536 || writer.bytes.len() + size as usize > 65536 {
        writer.failed = true;
        return -1;
    }
    writer.bytes.extend_from_slice(unsafe {
        std::slice::from_raw_parts(buffer.cast::<u8>(), size as usize)
    });
    size as i64
}
struct StateReader<'a> {
    bytes: &'a [u8],
    position: usize,
}
unsafe extern "C" fn state_read(
    stream: *const clap_istream,
    buffer: *mut c_void,
    size: u64,
) -> i64 {
    if stream.is_null() || buffer.is_null() {
        return -1;
    }
    let reader = unsafe { &mut *((*stream).ctx as *mut StateReader<'_>) };
    let count = size.min(reader.bytes.len().saturating_sub(reader.position) as u64) as usize;
    unsafe {
        ptr::copy_nonoverlapping(
            reader.bytes[reader.position..].as_ptr(),
            buffer.cast::<u8>(),
            count,
        );
    }
    reader.position += count;
    count as i64
}

#[cfg(test)]
mod host_extension_tests {
    use super::*;

    #[test]
    fn core_host_extensions_are_advertised_with_working_thread_check() {
        let state = HostState::new();
        let host = clap_host {
            clap_version: CLAP_VERSION,
            host_data: Arc::as_ptr(&state) as *mut c_void,
            name: ptr::null(),
            vendor: ptr::null(),
            url: ptr::null(),
            version: ptr::null(),
            get_extension: Some(host_get_extension),
            request_restart: Some(host_request_restart),
            request_process: Some(host_request_process),
            request_callback: Some(host_request_callback),
        };
        unsafe {
            for id in [
                CLAP_EXT_LOG,
                CLAP_EXT_THREAD_CHECK,
                CLAP_EXT_LATENCY,
                CLAP_EXT_TAIL,
                CLAP_EXT_PARAMS,
                CLAP_EXT_STATE,
                CLAP_EXT_AUDIO_PORTS,
                CLAP_EXT_NOTE_PORTS,
            ] {
                assert!(!host_get_extension(ptr::null(), id.as_ptr()).is_null());
            }
            let thread = &*(host_get_extension(ptr::null(), CLAP_EXT_THREAD_CHECK.as_ptr())
                as *const clap_host_thread_check);
            assert!(thread.is_main_thread.unwrap()(&host));
            assert!(!thread.is_audio_thread.unwrap()(&host));
            {
                let _scope = AudioThreadScope::enter();
                assert!(thread.is_audio_thread.unwrap()(&host));
                assert!(!thread.is_main_thread.unwrap()(&host));
            }
            let address = &host as *const clap_host as usize;
            std::thread::spawn(move || {
                assert!(!host_is_main_thread(address as *const clap_host));
                assert!(!host_is_audio_thread(address as *const clap_host));
            })
            .join()
            .unwrap();
            let notes = &*(host_get_extension(ptr::null(), CLAP_EXT_NOTE_PORTS.as_ptr())
                as *const clap_host_note_ports);
            assert_eq!(
                notes.supported_dialects.unwrap()(ptr::null()) & CLAP_NOTE_DIALECT_CLAP,
                CLAP_NOTE_DIALECT_CLAP
            );
        }
    }
}
