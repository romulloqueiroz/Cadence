//! Stable C ABI for native Rust plugins (`cdylib`).
//!
//! A native plugin exports one symbol:
//!
//! ```text
//! platform_plugin_v1() -> *const PluginVTable
//! ```
//!
//! The table points at process-wide function pointers. Instances are opaque
//! pointers created/destroyed through the table. Audio and events use the
//! layout-stable wire types; the host owns all buffers.

use crate::{Capabilities, Descriptor, PluginKind};
use std::ffi::{c_char, c_void, CString};

/// Native ABI version. Checked by the loader.
pub const ABI_VERSION: u32 = 1;

/// Exported entry point name.
pub const ENTRY_SYMBOL: &[u8] = b"platform_plugin_v1\0";

/// Optional metadata extension; the original v1 process vtable stays unchanged.
pub const PARAMETERS_SYMBOL: &[u8] = b"platform_parameters_v1\0";
#[repr(C)]
#[derive(Clone, Copy)]
pub struct WireParameter {
    pub id: u32,
    pub name: *const c_char,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub flags: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WireDescriptor {
    pub id: *const c_char,
    pub name: *const c_char,
    pub vendor: *const c_char,
    pub version: *const c_char,
    pub kind: u32,
}

#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct WireAudioBlock {
    pub inputs: *mut f32,
    pub input_channels: u32,
    pub outputs: *mut f32,
    pub output_channels: u32,
    pub sidechain: *mut f32,
    pub sidechain_channels: u32,
    pub frames: u32,
}

pub type CreateFn = unsafe extern "C" fn() -> *mut c_void;
pub type DestroyFn = unsafe extern "C" fn(*mut c_void);
pub type DescriptorFn = unsafe extern "C" fn() -> *const WireDescriptor;
pub type CapabilitiesFn = unsafe extern "C" fn() -> crate::wire::WireCapabilities;
pub type PrepareFn = unsafe extern "C" fn(*mut c_void, *const crate::wire::WireProcessContext);
pub type ResetFn = unsafe extern "C" fn(*mut c_void);
pub type ProcessFn = unsafe extern "C" fn(
    *mut c_void,
    *const crate::wire::WireProcessContext,
    *mut WireAudioBlock,
    *const crate::wire::WireEvent,
    usize,
);
pub type StateSizeFn = unsafe extern "C" fn(*mut c_void) -> usize;
pub type StateSchemaFn = unsafe extern "C" fn(*mut c_void) -> u32;
pub type SaveStateFn = unsafe extern "C" fn(*mut c_void, *mut u8, usize) -> usize;
pub type LoadStateFn = unsafe extern "C" fn(*mut c_void, *const u8, usize, u32) -> i32;

/// The function table a native plugin exports.
#[repr(C)]
pub struct PluginVTable {
    pub abi_version: u32,
    pub create: CreateFn,
    pub destroy: DestroyFn,
    pub descriptor: DescriptorFn,
    pub capabilities: CapabilitiesFn,
    pub prepare: PrepareFn,
    pub reset: ResetFn,
    pub process: ProcessFn,
    pub state_size: StateSizeFn,
    pub state_schema: StateSchemaFn,
    pub save_state: SaveStateFn,
    pub load_state: LoadStateFn,
}

/// Convert a descriptor to the C layout. Strings are leaked once per plugin;
/// call this from a `OnceLock` in the exported descriptor function.
pub fn wire_descriptor(descriptor: &Descriptor) -> WireDescriptor {
    fn leak(text: &'static str) -> *const c_char {
        CString::new(text).unwrap_or_default().into_raw() as *const c_char
    }

    WireDescriptor {
        id: leak(descriptor.id),
        name: leak(descriptor.name),
        vendor: leak(descriptor.vendor),
        version: leak(descriptor.version),
        kind: kind_code(descriptor.kind),
    }
}

pub fn kind_code(kind: PluginKind) -> u32 {
    match kind {
        PluginKind::Instrument => 0,
        PluginKind::Effect => 1,
        PluginKind::MidiEffect => 2,
    }
}

pub fn kind_from_code(code: u32) -> Option<PluginKind> {
    match code {
        0 => Some(PluginKind::Instrument),
        1 => Some(PluginKind::Effect),
        2 => Some(PluginKind::MidiEffect),
        _ => None,
    }
}

/// Convenience for the loader: convert capabilities without depending on the
/// `wire` module directly.
pub fn capabilities_to_wire(capabilities: Capabilities) -> crate::wire::WireCapabilities {
    capabilities.into()
}
