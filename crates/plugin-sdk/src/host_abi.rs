//! Generic trampolines from the C ABI into a concrete [`Plugin`].
//!
//! These are used by the `export_plugin!` macro. All pointer arguments are
//! host-owned and validated for null; malformed input is ignored rather than
//! causing undefined behaviour.

#![allow(clippy::missing_safety_doc)]

use plugin_api::abi::WireAudioBlock;
use plugin_api::wire::{WireEvent, WireProcessContext};
use plugin_api::{AudioBuffer, Event, Plugin, State};
use std::ffi::c_void;
use std::mem::MaybeUninit;
use std::slice;

pub const MAX_EVENTS: usize = 256;

pub unsafe fn create_box<T: Plugin + 'static>(value: T) -> *mut c_void {
    Box::into_raw(Box::new(value)) as *mut c_void
}

pub unsafe extern "C" fn destroy_box<T: Plugin>(instance: *mut c_void) {
    if !instance.is_null() {
        drop(unsafe { Box::from_raw(instance as *mut T) });
    }
}

pub unsafe extern "C" fn prepare_box<T: Plugin>(
    instance: *mut c_void,
    context: *const WireProcessContext,
) {
    if instance.is_null() || context.is_null() {
        return;
    }
    let plugin = unsafe { &mut *(instance as *mut T) };
    let context = unsafe { (*context).to_context() };
    plugin.prepare(&context);
}

pub unsafe extern "C" fn reset_box<T: Plugin>(instance: *mut c_void) {
    if instance.is_null() {
        return;
    }
    let plugin = unsafe { &mut *(instance as *mut T) };
    plugin.reset();
}

pub unsafe extern "C" fn process_box<T: Plugin>(
    instance: *mut c_void,
    context: *const WireProcessContext,
    block: *mut WireAudioBlock,
    events: *const WireEvent,
    event_count: usize,
) {
    if instance.is_null() || context.is_null() || block.is_null() {
        return;
    }

    let plugin = unsafe { &mut *(instance as *mut T) };
    let context = unsafe { (*context).to_context() };
    let block = unsafe { &*block };
    let frames = block.frames as usize;

    let input_channels = block.input_channels as usize;
    let output_channels = block.output_channels as usize;
    let sidechain_channels = block.sidechain_channels as usize;

    let inputs = unsafe { mutable_slice(block.inputs, input_channels * frames) };
    let outputs = unsafe { mutable_slice(block.outputs, output_channels * frames) };
    let sidechain = unsafe { mutable_slice(block.sidechain, sidechain_channels * frames) };

    let mut audio = AudioBuffer::new(
        inputs,
        input_channels,
        outputs,
        output_channels,
        sidechain,
        sidechain_channels,
        frames,
    );

    let mut storage: [MaybeUninit<Event>; MAX_EVENTS] =
        std::array::from_fn(|_| MaybeUninit::uninit());
    let mut count = 0;
    if !events.is_null() {
        for index in 0..event_count.min(MAX_EVENTS) {
            let wire = unsafe { *events.add(index) };
            if let Some(event) = wire.to_event() {
                storage[count].write(event);
                count += 1;
            }
        }
    }
    let events = unsafe { slice::from_raw_parts(storage.as_ptr() as *const Event, count) };

    plugin.process(&context, &mut audio, events);
}

pub unsafe extern "C" fn state_size_box<T: Plugin>(instance: *mut c_void) -> usize {
    if instance.is_null() {
        return 0;
    }
    let plugin = unsafe { &*(instance as *const T) };
    plugin.save_state().bytes.len()
}

pub unsafe extern "C" fn state_schema_box<T: Plugin>(instance: *mut c_void) -> u32 {
    if instance.is_null() {
        return 0;
    }
    let plugin = unsafe { &*(instance as *const T) };
    plugin.save_state().schema_version
}

pub unsafe extern "C" fn save_state_box<T: Plugin>(
    instance: *mut c_void,
    output: *mut u8,
    capacity: usize,
) -> usize {
    if instance.is_null() {
        return 0;
    }
    let plugin = unsafe { &*(instance as *const T) };
    let state = plugin.save_state();
    let count = state.bytes.len().min(capacity);
    if !output.is_null() && count > 0 {
        unsafe {
            std::ptr::copy_nonoverlapping(state.bytes.as_ptr(), output, count);
        }
    }
    state.bytes.len()
}

pub unsafe extern "C" fn load_state_box<T: Plugin>(
    instance: *mut c_void,
    data: *const u8,
    length: usize,
    schema_version: u32,
) -> i32 {
    if instance.is_null() {
        return -1;
    }
    let plugin = unsafe { &mut *(instance as *mut T) };
    let bytes = if length == 0 {
        Vec::new()
    } else if data.is_null() {
        return -1;
    } else {
        unsafe { slice::from_raw_parts(data, length).to_vec() }
    };
    let state = State {
        schema_version,
        bytes,
    };
    match plugin.load_state(&state) {
        Ok(()) => 0,
        Err(_) => 1,
    }
}

unsafe fn mutable_slice<'a>(pointer: *mut f32, length: usize) -> &'a mut [f32] {
    if pointer.is_null() || length == 0 {
        return unsafe {
            slice::from_raw_parts_mut(std::ptr::NonNull::<f32>::dangling().as_ptr(), 0)
        };
    }
    unsafe { slice::from_raw_parts_mut(pointer, length) }
}
