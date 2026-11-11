//! Loads native Rust plugin `cdylib`s through the stable C ABI.
//!
//! This is the in-process loader for development. The production host runs the
//! same vtable in an out-of-process sandbox.

use plugin_api::abi::{self, PluginVTable, WireAudioBlock};
use plugin_api::wire::{WireEvent, WireProcessContext};
use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, ParamFlags, ParameterInfo, Plugin, PluginKind,
    ProcessContext, State, StateError, Unit,
};
use std::ffi::{c_char, c_void, CStr, CString};
use std::fmt;
use std::mem::MaybeUninit;
use std::path::Path;

const MAX_EVENTS: usize = 256;

pub struct LoadedPlugin {
    handle: *mut c_void,
    table: &'static PluginVTable,
    instance: *mut c_void,
    descriptor: Descriptor,
    parameters: Vec<ParameterInfo>,
}

// The instance is exclusively owned by the host thread. The vtable lives in
// the opened library, which this value keeps alive.
unsafe impl Send for LoadedPlugin {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    Unsupported,
    BadPath(String),
    Open(String),
    Symbol(String),
    Abi { found: u32, supported: u32 },
    Create,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Unsupported => write!(f, "native loading is unsupported on this platform"),
            LoadError::BadPath(path) => write!(f, "invalid plugin path: {path}"),
            LoadError::Open(message) => write!(f, "dlopen failed: {message}"),
            LoadError::Symbol(message) => write!(f, "missing entry point: {message}"),
            LoadError::Abi { found, supported } => {
                write!(f, "plugin ABI {found}, supported {supported}")
            }
            LoadError::Create => write!(f, "plugin create returned null"),
        }
    }
}

impl std::error::Error for LoadError {}

impl LoadedPlugin {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        #[cfg(unix)]
        {
            Self::load_unix(path.as_ref())
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(LoadError::Unsupported)
        }
    }

    #[cfg(unix)]
    fn load_unix(path: &Path) -> Result<Self, LoadError> {
        let cpath = CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| LoadError::BadPath(path.display().to_string()))?;

        let handle = unsafe { libc::dlopen(cpath.as_ptr(), libc::RTLD_NOW | libc::RTLD_LOCAL) };
        if handle.is_null() {
            return Err(LoadError::Open(unsafe { last_error() }));
        }

        let symbol = unsafe { libc::dlsym(handle, abi::ENTRY_SYMBOL.as_ptr() as *const c_char) };
        if symbol.is_null() {
            let message = unsafe { last_error() };
            unsafe { libc::dlclose(handle) };
            return Err(LoadError::Symbol(message));
        }

        let entry: extern "C" fn() -> *const PluginVTable = unsafe { std::mem::transmute(symbol) };
        let table_pointer = entry();
        if table_pointer.is_null() {
            unsafe { libc::dlclose(handle) };
            return Err(LoadError::Symbol("entry returned null".into()));
        }
        let table = unsafe { &*table_pointer };

        if table.abi_version != abi::ABI_VERSION {
            let found = table.abi_version;
            unsafe { libc::dlclose(handle) };
            return Err(LoadError::Abi {
                found,
                supported: abi::ABI_VERSION,
            });
        }

        let instance = unsafe { (table.create)() };
        if instance.is_null() {
            unsafe { libc::dlclose(handle) };
            return Err(LoadError::Create);
        }

        let descriptor = unsafe { read_descriptor((table.descriptor)()) };
        let mut parameters = Vec::new();
        let symbol =
            unsafe { libc::dlsym(handle, abi::PARAMETERS_SYMBOL.as_ptr() as *const c_char) };
        if !symbol.is_null() {
            let function: unsafe extern "C" fn(*mut usize) -> *const abi::WireParameter =
                unsafe { std::mem::transmute(symbol) };
            let mut count = 0;
            let pointer = unsafe { function(&mut count) };
            if !pointer.is_null() && count <= plugin_api::MAX_PARAMETERS {
                for index in 0..count {
                    let p = unsafe { &*pointer.add(index) };
                    let parameter = ParameterInfo {
                        id: p.id,
                        name: unsafe { cstr(p.name) },
                        min: p.min,
                        max: p.max,
                        default: p.default,
                        unit: Unit::None,
                        flags: ParamFlags {
                            stepped: p.flags & 1 != 0,
                            read_only: p.flags & 2 != 0,
                            bypass: false,
                        },
                    };
                    if parameter.is_valid() {
                        parameters.push(parameter);
                    }
                }
            }
        }

        Ok(LoadedPlugin {
            handle,
            table,
            instance,
            descriptor,
            parameters,
        })
    }
}

impl Plugin for LoadedPlugin {
    fn parameters(&self) -> Vec<ParameterInfo> {
        self.parameters.clone()
    }
    fn descriptor(&self) -> Descriptor {
        self.descriptor
    }

    fn capabilities(&self) -> Capabilities {
        unsafe { (self.table.capabilities)().to_capabilities() }
    }

    fn prepare(&mut self, context: &ProcessContext) {
        let wire = WireProcessContext::from(*context);
        unsafe { (self.table.prepare)(self.instance, &wire) };
    }

    fn reset(&mut self) {
        unsafe { (self.table.reset)(self.instance) };
    }

    fn process(&mut self, context: &ProcessContext, audio: &mut AudioBuffer<'_>, events: &[Event]) {
        let wire = WireProcessContext::from(*context);
        let mut block = WireAudioBlock {
            inputs: audio.inputs.as_mut_ptr(),
            input_channels: audio.input_channels as u32,
            outputs: audio.outputs.as_mut_ptr(),
            output_channels: audio.output_channels as u32,
            sidechain: audio.sidechain.as_mut_ptr(),
            sidechain_channels: audio.sidechain_channels as u32,
            frames: audio.frames as u32,
        };

        let mut storage: [MaybeUninit<WireEvent>; MAX_EVENTS] =
            std::array::from_fn(|_| MaybeUninit::uninit());
        let mut count = 0;
        for event in events.iter().take(MAX_EVENTS) {
            storage[count].write((*event).into());
            count += 1;
        }
        let pointer = if count == 0 {
            std::ptr::null()
        } else {
            storage.as_ptr() as *const WireEvent
        };

        unsafe { (self.table.process)(self.instance, &wire, &mut block, pointer, count) };
    }

    fn save_state(&self) -> State {
        let size = unsafe { (self.table.state_size)(self.instance) };
        let schema_version = unsafe { (self.table.state_schema)(self.instance) };
        let mut bytes = vec![0u8; size];
        let written =
            unsafe { (self.table.save_state)(self.instance, bytes.as_mut_ptr(), bytes.len()) };
        bytes.truncate(written.min(size));
        State {
            schema_version,
            bytes,
        }
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        let code = unsafe {
            (self.table.load_state)(
                self.instance,
                state.bytes.as_ptr(),
                state.bytes.len(),
                state.schema_version,
            )
        };
        if code == 0 {
            Ok(())
        } else {
            Err(StateError::Malformed(format!(
                "plugin rejected state (code {code})"
            )))
        }
    }
}

impl Drop for LoadedPlugin {
    fn drop(&mut self) {
        unsafe {
            (self.table.destroy)(self.instance);
            libc::dlclose(self.handle);
        }
    }
}

#[cfg(unix)]
unsafe fn last_error() -> String {
    let pointer = unsafe { libc::dlerror() };
    if pointer.is_null() {
        "unknown dynamic-loader error".into()
    } else {
        unsafe { CStr::from_ptr(pointer).to_string_lossy().into_owned() }
    }
}

unsafe fn read_descriptor(raw: *const abi::WireDescriptor) -> Descriptor {
    if raw.is_null() {
        return Descriptor {
            id: "invalid",
            name: "Invalid",
            vendor: "",
            version: "0",
            kind: PluginKind::Effect,
        };
    }
    let raw = unsafe { &*raw };
    Descriptor {
        id: unsafe { cstr(raw.id) },
        name: unsafe { cstr(raw.name) },
        vendor: unsafe { cstr(raw.vendor) },
        version: unsafe { cstr(raw.version) },
        kind: abi::kind_from_code(raw.kind).unwrap_or(PluginKind::Effect),
    }
}

unsafe fn cstr(pointer: *const c_char) -> &'static str {
    if pointer.is_null() {
        return "";
    }
    let text = unsafe { CStr::from_ptr(pointer) }.to_str().unwrap_or("");
    // The plugin keeps these strings for the lifetime of its library, which the
    // loader holds open until after the instance is destroyed.
    unsafe { std::mem::transmute::<&str, &'static str>(text) }
}
