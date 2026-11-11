//! SDK for native Rust plugins.
//!
//! A plugin crate is a `cdylib` that implements [`plugin_api::Plugin`] and uses
//! [`export_plugin!`]:
//!
//! ```ignore
//! fn descriptor() -> plugin_api::Descriptor { /* ... */ }
//! fn capabilities() -> plugin_api::Capabilities { plugin_api::Capabilities::stereo_effect() }
//! fn create() -> MyPlugin { MyPlugin::default() }
//!
//! plugin_sdk::export_plugin!(MyPlugin, descriptor, capabilities, create);
//! ```
//!
//! The macro exports `platform_plugin_v1()` returning the C ABI vtable.

pub use plugin_api;
pub mod host_abi;

#[macro_export]
macro_rules! export_plugin {
    ($ty:ty, $descriptor:path, $capabilities:path, $create:path) => {
        #[no_mangle]
        pub unsafe extern "C" fn platform_parameters_v1(
            count: *mut usize,
        ) -> *const $crate::plugin_api::abi::WireParameter {
            use ::core::sync::atomic::{AtomicUsize, Ordering};
            static PTR: AtomicUsize = AtomicUsize::new(0);
            static COUNT: AtomicUsize = AtomicUsize::new(0);
            static INIT: ::std::sync::Once = ::std::sync::Once::new();
            INIT.call_once(|| {
                use $crate::plugin_api::Plugin;
                let parameters = $create().parameters();
                let result: Vec<_> = parameters
                    .iter()
                    .map(|p| $crate::plugin_api::abi::WireParameter {
                        id: p.id,
                        name: ::std::ffi::CString::new(p.name)
                            .unwrap_or_default()
                            .into_raw(),
                        min: p.min,
                        max: p.max,
                        default: p.default,
                        flags: u32::from(p.flags.stepped) | (u32::from(p.flags.read_only) << 1),
                    })
                    .collect();
                COUNT.store(result.len(), Ordering::Relaxed);
                PTR.store(
                    ::std::boxed::Box::leak(result.into_boxed_slice()).as_ptr() as usize,
                    Ordering::Release,
                );
            });
            if !count.is_null() {
                unsafe {
                    *count = COUNT.load(Ordering::Relaxed);
                }
            }
            PTR.load(Ordering::Acquire) as *const $crate::plugin_api::abi::WireParameter
        }

        #[no_mangle]
        pub extern "C" fn platform_plugin_v1() -> *const $crate::plugin_api::abi::PluginVTable {
            static TABLE: $crate::plugin_api::abi::PluginVTable =
                $crate::plugin_api::abi::PluginVTable {
                    abi_version: $crate::plugin_api::abi::ABI_VERSION,
                    create: create_instance,
                    destroy: $crate::host_abi::destroy_box::<$ty>,
                    descriptor: descriptor_instance,
                    capabilities: capabilities_instance,
                    prepare: $crate::host_abi::prepare_box::<$ty>,
                    reset: $crate::host_abi::reset_box::<$ty>,
                    process: $crate::host_abi::process_box::<$ty>,
                    state_size: $crate::host_abi::state_size_box::<$ty>,
                    state_schema: $crate::host_abi::state_schema_box::<$ty>,
                    save_state: $crate::host_abi::save_state_box::<$ty>,
                    load_state: $crate::host_abi::load_state_box::<$ty>,
                };
            &TABLE
        }

        unsafe extern "C" fn create_instance() -> *mut ::core::ffi::c_void {
            $crate::host_abi::create_box($create())
        }

        unsafe extern "C" fn descriptor_instance() -> *const $crate::plugin_api::abi::WireDescriptor
        {
            use ::core::sync::atomic::{AtomicUsize, Ordering};
            static PTR: AtomicUsize = AtomicUsize::new(0);
            let existing = PTR.load(Ordering::Acquire);
            if existing != 0 {
                return existing as *const $crate::plugin_api::abi::WireDescriptor;
            }
            let raw = ::std::boxed::Box::into_raw(::std::boxed::Box::new(
                $crate::plugin_api::abi::wire_descriptor(&$descriptor()),
            ));
            match PTR.compare_exchange(0, raw as usize, Ordering::AcqRel, Ordering::Acquire) {
                Ok(_) => raw,
                Err(winner) => winner as *const $crate::plugin_api::abi::WireDescriptor,
            }
        }

        unsafe extern "C" fn capabilities_instance() -> $crate::plugin_api::wire::WireCapabilities {
            $capabilities().into()
        }
    };
}
