//! C ABI between JUCE and the Rust sandbox. Plugin code stays in plugin-worker.
#![allow(clippy::missing_safety_doc)]
use plugin_api::wire::{WireEvent, WireProcessContext};
use plugin_api::{AudioBuffer, Plugin, ProcessContext, State};
use plugin_sandbox::SandboxPlugin;
use std::{
    cell::UnsafeCell,
    ffi::{c_char, c_void, CStr, CString},
    panic::{catch_unwind, AssertUnwindSafe},
    ptr, slice,
    sync::{
        atomic::{AtomicBool, AtomicU32, Ordering},
        Arc,
    },
};

struct Inner {
    sandbox: SandboxPlugin,
    inputs: Vec<f32>,
    sidechain: Vec<f32>,
}
pub struct CadenceBridge {
    inner: UnsafeCell<Inner>,
    metadata: CString,
    failed: AtomicBool,
    worker_id: u32,
    monitor: plugin_sandbox::shared::PipelineMonitor,
    worker_failed: Arc<AtomicBool>,
    failure_reason: Arc<AtomicU32>,
    workgroup_base: String,
}

fn guarded<T>(fallback: T, function: impl FnOnce() -> T) -> T {
    catch_unwind(AssertUnwindSafe(function)).unwrap_or(fallback)
}
unsafe fn text<'a>(pointer: *const c_char) -> Result<&'a str, String> {
    if pointer.is_null() {
        return Err("Missing string argument".into());
    }
    unsafe { CStr::from_ptr(pointer) }
        .to_str()
        .map_err(|e| e.to_string())
}
unsafe fn write_error(destination: *mut c_char, capacity: usize, message: &str) {
    if destination.is_null() || capacity == 0 {
        return;
    }
    let count = message.len().min(capacity - 1);
    unsafe {
        ptr::copy_nonoverlapping(message.as_ptr(), destination.cast(), count);
        *destination.add(count) = 0;
    }
}
#[no_mangle]
pub extern "C" fn cadence_bridge_abi() -> u32 {
    2
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_open(
    worker: *const c_char,
    source: *const c_char,
    selector: *const c_char,
    error: *mut c_char,
    capacity: usize,
) -> *mut CadenceBridge {
    unsafe {
        cadence_bridge_open_cancellable(
            worker,
            source,
            selector,
            error,
            capacity,
            None,
            ptr::null_mut(),
        )
    }
}

pub type CancelCheck = Option<unsafe extern "C" fn(*mut c_void) -> i32>;
fn cancelled(check: CancelCheck, context: *mut c_void) -> bool {
    check.is_some_and(|callback| unsafe { callback(context) != 0 })
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_open_cancellable(
    worker: *const c_char,
    source: *const c_char,
    selector: *const c_char,
    error: *mut c_char,
    capacity: usize,
    check: CancelCheck,
    context: *mut c_void,
) -> *mut CadenceBridge {
    guarded(ptr::null_mut(), || {
        let result = (|| {
            let worker = unsafe { text(worker) }?;
            let source = unsafe { text(source) }?;
            let selector = if selector.is_null() {
                None
            } else {
                Some(unsafe { text(selector) }?)
            };
            let sandbox = SandboxPlugin::spawn_selected_cancellable(
                worker,
                source,
                selector,
                Default::default(),
                &|| cancelled(check, context),
            )
            .map_err(|e| e.to_string())?;
            let metadata =
                CString::new(serde_json::to_string(sandbox.metadata()).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let worker_id = sandbox.worker_id();
            let monitor = sandbox.pipeline_monitor();
            let workgroup_base = sandbox.workgroup_base().to_owned();
            let worker_failed = sandbox.failure_flag();
            let failure_reason = sandbox.failure_reason_flag();
            Ok::<_, String>(Box::new(CadenceBridge {
                inner: UnsafeCell::new(Inner {
                    sandbox,
                    inputs: vec![
                        0.0;
                        plugin_sandbox::shared::MAX_CHANNELS
                            * plugin_sandbox::shared::MAX_BLOCK
                    ],
                    sidechain: vec![
                        0.0;
                        plugin_sandbox::shared::MAX_CHANNELS
                            * plugin_sandbox::shared::MAX_BLOCK
                    ],
                }),
                metadata,
                failed: AtomicBool::new(false),
                worker_id,
                monitor,
                worker_failed,
                failure_reason,
                workgroup_base,
            }))
        })();
        match result {
            Ok(handle) => Box::into_raw(handle),
            Err(message) => {
                unsafe {
                    write_error(error, capacity, &message);
                }
                ptr::null_mut()
            }
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_prepare_cancellable(
    handle: *mut CadenceBridge,
    sample_rate: f64,
    max_block: u32,
    check: CancelCheck,
    context: *mut c_void,
) -> i32 {
    guarded(-1, || {
        if handle.is_null() {
            return -1;
        }
        let inner = unsafe { &mut *(*handle).inner.get() };
        let audio_context = ProcessContext {
            sample_rate,
            block_size: max_block as usize,
            ..Default::default()
        };
        if inner
            .sandbox
            .prepare_cancellable(&audio_context, &|| cancelled(check, context))
            .is_ok()
        {
            0
        } else {
            -1
        }
    })
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_load_cancellable(
    handle: *mut CadenceBridge,
    input: *const u8,
    length: usize,
    schema: u32,
    check: CancelCheck,
    context: *mut c_void,
) -> i32 {
    guarded(-1, || {
        if handle.is_null() || length > 65536 || (length > 0 && input.is_null()) {
            return -1;
        }
        let bytes = if length == 0 {
            Vec::new()
        } else {
            unsafe { slice::from_raw_parts(input, length) }.to_vec()
        };
        let inner = unsafe { &mut *(*handle).inner.get() };
        if inner
            .sandbox
            .load_state_cancellable(
                &State {
                    schema_version: schema,
                    bytes,
                },
                &|| cancelled(check, context),
            )
            .is_ok()
        {
            0
        } else {
            -1
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_close(handle: *mut CadenceBridge) {
    if !handle.is_null() {
        guarded((), || drop(unsafe { Box::from_raw(handle) }));
    }
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_metadata(
    handle: *const CadenceBridge,
    output: *mut c_char,
    capacity: usize,
) -> usize {
    if handle.is_null() {
        return 0;
    }
    let bytes = unsafe { &*handle }.metadata.as_bytes_with_nul();
    if !output.is_null() && capacity >= bytes.len() {
        unsafe {
            ptr::copy_nonoverlapping(bytes.as_ptr(), output.cast(), bytes.len());
        }
    }
    bytes.len()
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_prepare(
    handle: *mut CadenceBridge,
    sample_rate: f64,
    max_block: u32,
) -> i32 {
    guarded(-1, || {
        if handle.is_null() {
            return -1;
        }
        let shared = unsafe { &*handle };
        let h = unsafe { &mut *shared.inner.get() };
        let context = ProcessContext {
            sample_rate,
            block_size: max_block as usize,
            ..Default::default()
        };
        if !context.is_valid() || shared.failed.load(Ordering::Relaxed) {
            return -1;
        }
        h.sandbox.prepare(&context);
        shared
            .failed
            .store(h.sandbox.has_failed(), Ordering::Release);
        if h.sandbox.has_failed() {
            -1
        } else {
            0
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_reset(handle: *mut CadenceBridge) -> i32 {
    guarded(-1, || {
        if handle.is_null() {
            return -1;
        }
        let shared = unsafe { &*handle };
        let h = unsafe { &mut *shared.inner.get() };
        h.sandbox.reset();
        shared
            .failed
            .store(h.sandbox.has_failed(), Ordering::Release);
        if h.sandbox.has_failed() {
            -1
        } else {
            0
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_process(
    handle: *mut CadenceBridge,
    context: *const WireProcessContext,
    inputs: *const f32,
    input_channels: u32,
    outputs: *mut f32,
    output_channels: u32,
    frames: u32,
    events: *const WireEvent,
    event_count: u32,
) -> i32 {
    unsafe {
        process_inner(
            handle,
            context,
            inputs,
            input_channels,
            ptr::null(),
            0,
            outputs,
            output_channels,
            frames,
            events,
            event_count,
        )
    }
}

/// Additive sidechain-capable process entry point. Keeps the original ABI
/// function valid for hosts that only use main audio.
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_process_sidechain(
    handle: *mut CadenceBridge,
    context: *const WireProcessContext,
    inputs: *const f32,
    input_channels: u32,
    sidechain: *const f32,
    sidechain_channels: u32,
    outputs: *mut f32,
    output_channels: u32,
    frames: u32,
    events: *const WireEvent,
    event_count: u32,
) -> i32 {
    unsafe {
        process_inner(
            handle,
            context,
            inputs,
            input_channels,
            sidechain,
            sidechain_channels,
            outputs,
            output_channels,
            frames,
            events,
            event_count,
        )
    }
}

#[allow(clippy::too_many_arguments)]
unsafe fn process_inner(
    handle: *mut CadenceBridge,
    context: *const WireProcessContext,
    inputs: *const f32,
    input_channels: u32,
    sidechain: *const f32,
    sidechain_channels: u32,
    outputs: *mut f32,
    output_channels: u32,
    frames: u32,
    events: *const WireEvent,
    event_count: u32,
) -> i32 {
    if outputs.is_null() || frames > 4096 || output_channels > 8 {
        return -1;
    }
    let output =
        unsafe { slice::from_raw_parts_mut(outputs, frames as usize * output_channels as usize) };
    output.fill(0.0);
    guarded(-1, || {
        if handle.is_null()
            || context.is_null()
            || frames == 0
            || input_channels > 8
            || sidechain_channels > 8
            || event_count > 256
            || (input_channels > 0 && inputs.is_null())
            || (sidechain_channels > 0 && sidechain.is_null())
            || (event_count > 0 && events.is_null())
        {
            return -1;
        }
        let shared = unsafe { &*handle };
        let h = unsafe { &mut *shared.inner.get() };
        if shared.failed.load(Ordering::Relaxed) {
            return -1;
        }
        let offline = unsafe { (*context).flags } & 2 != 0;
        let context = unsafe { *context }.to_context();
        if !context.is_valid() {
            return -1;
        }
        let count = frames as usize * input_channels as usize;
        if count > 0 {
            h.inputs[..count].copy_from_slice(unsafe { slice::from_raw_parts(inputs, count) });
        }
        let side_count = frames as usize * sidechain_channels as usize;
        if side_count > 0 {
            h.sidechain[..side_count]
                .copy_from_slice(unsafe { slice::from_raw_parts(sidechain, side_count) });
        }
        let mut audio = AudioBuffer::new(
            &mut h.inputs[..count],
            input_channels as usize,
            output,
            output_channels as usize,
            &mut h.sidechain[..side_count],
            sidechain_channels as usize,
            frames as usize,
        );
        let incoming = if event_count == 0 {
            &[][..]
        } else {
            unsafe { slice::from_raw_parts(events, event_count as usize) }
        };
        let mut storage = [WireEvent::default(); 256];
        let Ok(length) = validated_wires(incoming, frames, &mut storage) else {
            return -1;
        };
        if offline {
            h.sandbox
                .process_offline_delayed_wires(&context, &mut audio, &storage[..length]);
        } else {
            h.sandbox
                .process_pipelined_wires(&context, &mut audio, &storage[..length]);
        }
        shared
            .failed
            .store(h.sandbox.has_failed(), Ordering::Release);
        if h.sandbox.has_failed() {
            -1
        } else {
            0
        }
    })
}
/// Preserve transport-only compact ramps without extending the Rust plugin SDK
/// enum. Ordinary events retain the previous conversion/filtering semantics.
fn validated_wires(
    incoming: &[WireEvent],
    frames: u32,
    output: &mut [WireEvent; 256],
) -> Result<usize, ()> {
    if incoming.len() > output.len() {
        return Err(());
    }
    let mut length = 0;
    for wire in incoming.iter().copied() {
        if wire.kind == 8 {
            if wire.frame >= frames
                || wire.channel != 0
                || !(1..=4096).contains(&wire.data2)
                || !wire.value.is_finite()
                || !(0.0..=1.0).contains(&wire.value)
            {
                return Err(());
            }
            output[length] = wire;
            length += 1;
        } else if let Some(event) = wire
            .to_event()
            .filter(|e| e.frame < frames as usize && e.value.is_finite() && e.channel < 16)
        {
            output[length] = event.into();
            length += 1;
        }
    }
    Ok(length)
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_save(
    handle: *mut CadenceBridge,
    output: *mut u8,
    capacity: usize,
    schema: *mut u32,
) -> i32 {
    guarded(-1, || {
        if handle.is_null() || output.is_null() || schema.is_null() {
            return -1;
        }
        let shared = unsafe { &*handle };
        let h = unsafe { &mut *shared.inner.get() };
        let Ok(state) = h.sandbox.save_state_checked() else {
            return -1;
        };
        if state.bytes.len() > capacity {
            return -1;
        }
        unsafe {
            ptr::copy_nonoverlapping(state.bytes.as_ptr(), output, state.bytes.len());
            *schema = state.schema_version;
        }
        state.bytes.len() as i32
    })
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_load(
    handle: *mut CadenceBridge,
    input: *const u8,
    length: usize,
    schema: u32,
) -> i32 {
    guarded(-1, || {
        if handle.is_null() || length > 65536 || (length > 0 && input.is_null()) {
            return -1;
        }
        let bytes = if length == 0 {
            Vec::new()
        } else {
            unsafe { slice::from_raw_parts(input, length) }.to_vec()
        };
        if unsafe { &mut *(*handle).inner.get() }
            .sandbox
            .load_state(&State {
                schema_version: schema,
                bytes,
            })
            .is_ok()
        {
            0
        } else {
            -1
        }
    })
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_failed(handle: *const CadenceBridge) -> u32 {
    if handle.is_null() {
        return 1;
    }
    let h = unsafe { &*handle };
    u32::from(
        h.failed.load(Ordering::Acquire)
            || h.worker_failed.load(Ordering::Acquire)
            || h.monitor.error(),
    )
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_worker_id(handle: *const CadenceBridge) -> u32 {
    if handle.is_null() {
        return 0;
    }
    unsafe { &*handle }.worker_id
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_failure_reason(handle: *const CadenceBridge) -> u32 {
    if handle.is_null() {
        return 0;
    }
    unsafe { &*handle }.failure_reason.load(Ordering::Acquire)
}

#[repr(C)]
#[derive(Default)]
pub struct CadencePipelineStats {
    pub underruns: u64,
    pub missed_frames: u64,
    pub pending_packets: u64,
    pub latency_frames: u32,
    pub scheduling_flags: u32,
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_pipeline_stats(
    handle: *const CadenceBridge,
    output: *mut CadencePipelineStats,
) -> i32 {
    if handle.is_null() || output.is_null() {
        return -1;
    }
    let h = unsafe { &*handle };
    unsafe {
        *output = CadencePipelineStats {
            underruns: h.monitor.underruns(),
            missed_frames: h.monitor.late_frames(),
            pending_packets: h.monitor.published().saturating_sub(h.monitor.completed()),
            latency_frames: h.monitor.latency(),
            scheduling_flags: h.monitor.scheduling_flags(),
        };
    }
    0
}

#[repr(C)]
#[derive(Default, Debug, Clone, Copy)]
pub struct CadenceDynamicStats {
    pub plugin_latency_frames: u32,
    pub plugin_tail_frames: u32,
}
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_dynamic_stats(
    handle: *const CadenceBridge,
    output: *mut CadenceDynamicStats,
) -> i32 {
    if handle.is_null() || output.is_null() {
        return -1;
    }
    let h = unsafe { &*handle };
    unsafe {
        *output = CadenceDynamicStats {
            plugin_latency_frames: h.monitor.plugin_latency(),
            plugin_tail_frames: h.monitor.plugin_tail(),
        };
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_take_output_midi(
    handle: *mut CadenceBridge,
    events: *mut plugin_api::wire::WireEvent,
    event_capacity: u32,
    sysex: *mut u8,
    sysex_capacity: u32,
    event_count: *mut u32,
    sysex_length: *mut u32,
) -> i32 {
    if handle.is_null() || (event_capacity > 0 && events.is_null()) {
        return -1;
    }
    let shared = unsafe { &*handle };
    let h = unsafe { &mut *shared.inner.get() };
    let mut wires = [plugin_api::wire::WireEvent::default(); 256];
    let mut sysex_storage = [0u8; plugin_sandbox::shared::MAX_OUTPUT_SYSEX];
    let (count, length) = h.sandbox.take_output_group(&mut wires, &mut sysex_storage);
    let count = count.min(event_capacity as usize);
    for (index, wire) in wires[..count].iter().enumerate() {
        unsafe {
            events.add(index).write(*wire);
        }
    }
    let length = length.min(sysex_capacity as usize);
    if length > 0 {
        if sysex.is_null() {
            return -1;
        }
        unsafe {
            std::ptr::copy_nonoverlapping(sysex_storage.as_ptr(), sysex, length);
        }
    }
    if !event_count.is_null() {
        unsafe {
            *event_count = count as u32;
        }
    }
    if !sysex_length.is_null() {
        unsafe {
            *sysex_length = length as u32;
        }
    }
    0
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_set_gui_request(
    handle: *const CadenceBridge,
    request: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    unsafe { &*handle }.monitor.set_gui_request(request);
    0
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_gui_state(handle: *const CadenceBridge) -> u32 {
    if handle.is_null() {
        return 0;
    }
    let monitor = unsafe { &*handle }.monitor.clone();
    u32::from(monitor.gui_ready()) | (u32::from(monitor.gui_open()) << 1)
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_reload_requested(handle: *const CadenceBridge) -> u32 {
    if handle.is_null() {
        return 0;
    }
    u32::from(unsafe { &*handle }.monitor.reload_requested())
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_parameter_generation(handle: *const CadenceBridge) -> u64 {
    if handle.is_null() {
        return 0;
    }
    unsafe { &*handle }.monitor.parameter_generation()
}

#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_parameter_snapshot(
    handle: *const CadenceBridge,
    ids: *mut u32,
    values: *mut f32,
    capacity: u32,
    generation: *mut u64,
) -> i32 {
    if handle.is_null() || (capacity > 0 && (ids.is_null() || values.is_null())) {
        return -1;
    }
    let capacity = capacity as usize;
    let ids = unsafe { slice::from_raw_parts_mut(ids, capacity) };
    let values = unsafe { slice::from_raw_parts_mut(values, capacity) };
    let h = unsafe { &*handle };
    match h.monitor.parameter_snapshot(ids, values) {
        Some((count, current)) => {
            if !generation.is_null() {
                unsafe { *generation = current };
            }
            count as i32
        }
        None => -1,
    }
}

/// Additive C API; pipeline stats stay pipeline-only, streaming misses separate.
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_set_workgroup(
    handle: *const CadenceBridge,
    workgroup: *mut c_void,
    generation: u32,
) -> i32 {
    if handle.is_null() {
        return -1;
    }
    let h = unsafe { &*handle };
    if plugin_sandbox::workgroup::publish(&h.monitor, &h.workgroup_base, workgroup, generation) {
        0
    } else {
        -1
    }
}

#[repr(C)]
#[derive(Debug, Default, Clone, Copy)]
pub struct CadenceStreamingStats {
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
#[no_mangle]
pub unsafe extern "C" fn cadence_bridge_streaming_stats(
    handle: *const CadenceBridge,
    output: *mut CadenceStreamingStats,
) -> i32 {
    if handle.is_null() || output.is_null() {
        return -1;
    }
    let telemetry = unsafe { &*handle }.monitor.streaming_telemetry();
    let stats = telemetry.map_or_else(CadenceStreamingStats::default, |value| {
        CadenceStreamingStats {
            resident_bytes: value.resident_bytes,
            cache_capacity_bytes: value.cache_capacity_bytes,
            cache_ready_bytes: value.cache_ready_bytes,
            peak_cache_ready_bytes: value.peak_cache_ready_bytes,
            pages_read: value.pages_read,
            read_errors: value.read_errors,
            underrun_frames: value.underrun_frames,
            underrun_blocks: value.underrun_blocks,
            read_count: value.read_count,
            read_total_ns: value.read_total_ns,
            last_read_ns: value.last_read_ns,
            max_read_ns: value.max_read_ns,
            active_voices: value.active_voices,
            voice_capacity: value.voice_capacity,
            pending_reads: value.pending_reads,
            flags: value.flags,
        }
    });
    unsafe {
        *output = stats;
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;
    fn compact_ramp() -> WireEvent {
        let mut wire = WireEvent::default();
        wire.frame = 31;
        wire.kind = 8;
        wire.channel = 0;
        wire.data1 = 42;
        wire.data2 = 4096;
        wire.value = 1.0;
        wire
    }

    #[test]
    fn compact_wire_ramps_are_preserved_including_cross_packet_duration() {
        let ramp = compact_ramp();
        assert!(ramp.to_event().is_none(), "SDK event kinds are unchanged");
        let mut output = [WireEvent::default(); 256];
        assert_eq!(validated_wires(&[ramp], 64, &mut output), Ok(1));
        assert_eq!(output[0], ramp);
    }

    #[test]
    fn malformed_compact_ramps_reject_the_packet_instead_of_disappearing() {
        let ramp = compact_ramp();
        let mut bad = Vec::new();
        for value in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            let mut wire = ramp;
            wire.value = value;
            bad.push(wire);
        }
        for duration in [0, 4097, u16::MAX] {
            let mut wire = ramp;
            wire.data2 = duration;
            bad.push(wire);
        }
        let mut wire = ramp;
        wire.channel = 1;
        bad.push(wire);
        wire = ramp;
        wire.frame = 64;
        bad.push(wire);
        let valid: WireEvent = plugin_api::Event::note_on(0, 60, 100).into();
        let mut output = [WireEvent::default(); 256];
        for wire in bad {
            assert_eq!(
                validated_wires(&[valid, wire], 64, &mut output),
                Err(()),
                "{wire:?}"
            );
        }
        assert_eq!(validated_wires(&[ramp], 0, &mut output), Err(()));
        assert_eq!(validated_wires(&[ramp; 257], 64, &mut output), Err(()));
    }

    #[test]
    fn ordinary_wire_events_retain_existing_filter_and_conversion_semantics() {
        let valid: WireEvent = plugin_api::Event::parameter(1, 1, 1.25).into();
        let mut invalid = valid;
        invalid.channel = 16;
        let mut late = valid;
        late.frame = 64;
        let mut nonfinite = valid;
        nonfinite.value = f32::NAN;
        let mut wrapped_channel = valid;
        wrapped_channel.channel = 256; // Existing SDK conversion narrows to u8.
        let mut output = [WireEvent::default(); 256];
        assert_eq!(
            validated_wires(
                &[valid, invalid, late, nonfinite, wrapped_channel],
                64,
                &mut output
            ),
            Ok(2)
        );
        assert_eq!(output[0], valid);
        assert_eq!(output[1].channel, 0);
    }

    #[test]
    fn c_layout_and_null_arguments() {
        assert_eq!(std::mem::size_of::<WireProcessContext>(), 48);
        assert_eq!(std::mem::size_of::<WireEvent>(), 20);
        assert_eq!(cadence_bridge_abi(), 2);
        assert_eq!(std::mem::size_of::<CadenceStreamingStats>(), 112);
        unsafe {
            assert_eq!(cadence_bridge_prepare(ptr::null_mut(), 48000.0, 256), -1);
            assert_eq!(cadence_bridge_metadata(ptr::null(), ptr::null_mut(), 0), 0);
            assert_eq!(cadence_bridge_failed(ptr::null()), 1);
            assert_eq!(cadence_bridge_failure_reason(ptr::null()), 0);
            let mut stats = CadenceStreamingStats::default();
            assert_eq!(cadence_bridge_streaming_stats(ptr::null(), &mut stats), -1);
            assert_eq!(
                cadence_bridge_streaming_stats(ptr::null(), ptr::null_mut()),
                -1
            );
            let mut output = [1.0; 32];
            assert_eq!(
                cadence_bridge_process(
                    ptr::null_mut(),
                    ptr::null(),
                    ptr::null(),
                    0,
                    output.as_mut_ptr(),
                    2,
                    16,
                    ptr::null(),
                    0
                ),
                -1
            );
            assert!(output.iter().all(|v| *v == 0.0));
        }
    }
}
