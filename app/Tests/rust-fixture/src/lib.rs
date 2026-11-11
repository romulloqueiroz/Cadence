//! Deterministic CLAP instrument used only by bridge integration tests.
use clap_sys::{
    entry::*,
    events::*,
    ext::{
        audio_ports::*, gui::*, latency::*, log::CLAP_EXT_LOG, note_ports::*, params::*, state::*,
        thread_check::*,
    },
    factory::plugin_factory::*,
    host::*,
    plugin::*,
    process::*,
    stream::*,
    version::*,
};
use std::{
    ffi::{c_char, c_void, CStr},
    ptr,
    sync::atomic::{AtomicBool, AtomicU32, AtomicUsize, Ordering},
};
const RAW_GAIN_ID: u32 = 0x10001; // Deliberately outside the protocol's u16 event namespace.
static FEATURES: FeatureList = FeatureList([b"instrument\0".as_ptr().cast(), ptr::null()]);
struct FeatureList([*const c_char; 2]);
unsafe impl Sync for FeatureList {}
static DESCRIPTOR: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: b"test.cadence.tone\0".as_ptr().cast(),
    name: b"Bridge Test Tone\0".as_ptr().cast(),
    vendor: b"Cadence Tests\0".as_ptr().cast(),
    url: b"\0".as_ptr().cast(),
    manual_url: b"\0".as_ptr().cast(),
    support_url: b"\0".as_ptr().cast(),
    version: b"1.0\0".as_ptr().cast(),
    description: b"Deterministic test fixture\0".as_ptr().cast(),
    features: FEATURES.0.as_ptr(),
};
static DESCRIPTOR3: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: b"test.cadence.tone.sidechain\0".as_ptr().cast(),
    name: b"Bridge Test Sidechain\0".as_ptr().cast(),
    vendor: b"Cadence Tests\0".as_ptr().cast(),
    url: b"\0".as_ptr().cast(),
    manual_url: b"\0".as_ptr().cast(),
    support_url: b"\0".as_ptr().cast(),
    version: b"1.2\0".as_ptr().cast(),
    description: b"Sidechain test fixture\0".as_ptr().cast(),
    features: FEATURES.0.as_ptr(),
};
static DESCRIPTOR2: clap_plugin_descriptor = clap_plugin_descriptor {
    clap_version: CLAP_VERSION,
    id: b"test.cadence.tone.second\0".as_ptr().cast(),
    name: b"Bridge Test Tone 2\0".as_ptr().cast(),
    vendor: b"Cadence Tests\0".as_ptr().cast(),
    url: b"\0".as_ptr().cast(),
    manual_url: b"\0".as_ptr().cast(),
    support_url: b"\0".as_ptr().cast(),
    version: b"1.1\0".as_ptr().cast(),
    description: b"Second selectable fixture variant\0".as_ptr().cast(),
    features: FEATURES.0.as_ptr(),
};
struct Instance {
    api: clap_plugin,
    host: *const clap_host,
    sidechain: bool,
    gain: AtomicU32,
    sounding: AtomicBool,
    active: AtomicBool,
    pending: AtomicU32,
    flush_count: AtomicU32,
    activation_count: AtomicU32,
    structural_restart: AtomicBool,
    gui_window: AtomicUsize,
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
unsafe fn instance<'a>(p: *const clap_plugin) -> &'a Instance {
    unsafe { &*((*p).plugin_data as *const Instance) }
}
fn gain(state: &Instance) -> f32 {
    f32::from_bits(state.gain.load(Ordering::Acquire))
}
fn set_gain(state: &Instance, value: f32) {
    state.gain.store(value.to_bits(), Ordering::Release);
}
fn thread_is(state: &Instance, audio: bool) -> bool {
    let extension =
        unsafe { (*state.host).get_extension.unwrap()(state.host, CLAP_EXT_THREAD_CHECK.as_ptr()) };
    let check = unsafe { &*(extension as *const clap_host_thread_check) };
    unsafe {
        check.is_audio_thread.unwrap()(state.host) == audio
            && check.is_main_thread.unwrap()(state.host) != audio
    }
}
fn log_main(state: &Instance, name: &str) {
    if let Ok(path) = std::env::var("CADENCE_CLAP_REQUEST_LOG") {
        use std::io::Write;
        #[cfg(target_os = "macos")]
        let real_main = {
            unsafe extern "C" {
                fn pthread_main_np() -> i32;
            }
            unsafe { pthread_main_np() != 0 }
        };
        #[cfg(not(target_os = "macos"))]
        let real_main = true;
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .unwrap();
        writeln!(
            file,
            "{name} main={} active={} activations={} flushes={} gain={}",
            real_main && thread_is(state, false),
            state.active.load(Ordering::Acquire),
            state.activation_count.load(Ordering::Acquire),
            state.flush_count.load(Ordering::Acquire),
            gain(state)
        )
        .unwrap();
    }
}
fn request_test(state: &Instance, key: i16) {
    if (119..=124).contains(&key) {
        state.pending.store(key as u32, Ordering::Release);
        unsafe {
            (*state.host).request_callback.unwrap()(state.host);
        }
    }
}

fn request_structural_rescan(state: &Instance) {
    if state.host.is_null() {
        return;
    }
    let Some(get) = (unsafe { (*state.host).get_extension }) else {
        return;
    };
    let extension = unsafe { get(state.host, CLAP_EXT_PARAMS.as_ptr()) };
    if extension.is_null() {
        return;
    }
    let params = unsafe { &*(extension as *const clap_host_params) };
    if let Some(rescan) = params.rescan {
        unsafe { rescan(state.host, CLAP_PARAM_RESCAN_ALL) };
    }
}
fn request_parameter_rescan(state: &Instance) {
    if state.host.is_null() {
        return;
    }
    let Some(get) = (unsafe { (*state.host).get_extension }) else {
        return;
    };
    let extension = unsafe { get(state.host, CLAP_EXT_PARAMS.as_ptr()) };
    if extension.is_null() {
        return;
    }
    let params = unsafe { &*(extension as *const clap_host_params) };
    if let Some(rescan) = params.rescan {
        unsafe { rescan(state.host, CLAP_PARAM_RESCAN_VALUES) };
    }
}
unsafe extern "C" fn init(p: *const clap_plugin) -> bool {
    let host = unsafe { instance(p) }.host;
    if host.is_null() {
        return false;
    }
    let get = match unsafe { (*host).get_extension } {
        Some(get) => get,
        None => return false,
    };
    let thread = unsafe { get(host, CLAP_EXT_THREAD_CHECK.as_ptr()) };
    let log = unsafe { get(host, CLAP_EXT_LOG.as_ptr()) };
    if thread.is_null() || log.is_null() {
        return false;
    }
    let thread = unsafe { &*(thread as *const clap_host_thread_check) };
    let is_main = match thread.is_main_thread {
        Some(check) => unsafe { check(host) },
        None => false,
    };
    let is_audio = match thread.is_audio_thread {
        Some(check) => unsafe { check(host) },
        None => true,
    };
    if is_main && !is_audio && std::env::var_os("CADENCE_CLAP_REQUEST_LOG").is_some() {
        unsafe { instance(p) }.pending.store(122, Ordering::Release);
        unsafe {
            (*host).request_callback.unwrap()(host);
        }
    }
    is_main && !is_audio
}
unsafe extern "C" fn destroy(p: *const clap_plugin) {
    drop(unsafe { Box::from_raw((*p).plugin_data as *mut Instance) });
}
unsafe extern "C" fn activate(p: *const clap_plugin, _: f64, _: u32, _: u32) -> bool {
    let state = unsafe { instance(p) };
    if !thread_is(state, false) {
        return false;
    }
    state.active.store(true, Ordering::Release);
    state.activation_count.fetch_add(1, Ordering::AcqRel);
    log_main(state, "activate");
    true
}
unsafe extern "C" fn deactivate(p: *const clap_plugin) {
    let state = unsafe { instance(p) };
    state.active.store(false, Ordering::Release);
    log_main(state, "deactivate");
    if state.structural_restart.swap(false, Ordering::AcqRel) {
        request_structural_rescan(state);
    }
}
unsafe extern "C" fn start(p: *const clap_plugin) -> bool {
    let state = unsafe { instance(p) };
    thread_is(state, true) && state.active.load(Ordering::Acquire)
}
unsafe extern "C" fn stop(_: *const clap_plugin) {}
unsafe extern "C" fn reset(p: *const clap_plugin) {
    unsafe { instance(p) }
        .sounding
        .store(false, Ordering::Release);
}
unsafe extern "C" fn process(p: *const clap_plugin, process: *const clap_process) -> i32 {
    let state = unsafe { instance(p) };
    let process = unsafe { &*process };
    if !state.active.load(Ordering::Acquire)
        || !thread_is(state, true)
        || process.transport.is_null()
    {
        return CLAP_PROCESS_ERROR;
    }
    let events = unsafe { &*process.in_events };
    let mut index = 0;
    let count = unsafe { events.size.unwrap()(events) };
    for frame in 0..process.frames_count {
        while index < count {
            let header = unsafe { events.get.unwrap()(events, index) };
            if unsafe { (*header).time } > frame {
                break;
            }
            match unsafe { (*header).type_ } {
                CLAP_EVENT_NOTE_ON => {
                    let note = unsafe { &*(header as *const clap_event_note) };
                    state.sounding.store(true, Ordering::Release);
                    request_test(state, note.key);
                    if note.key == 127 {
                        set_gain(state, 0.25);
                        request_parameter_rescan(state);
                    }
                    if note.key == 125 {
                        request_structural_rescan(state);
                    }
                    if note.key == 126 && !process.out_events.is_null() {
                        if let (Some(push), Some(list)) = (
                            unsafe { (*process.out_events).try_push },
                            Some(process.out_events),
                        ) {
                            let midi = clap_event_midi {
                                header: event_header::<clap_event_midi>(frame, CLAP_EVENT_MIDI),
                                port_index: 0,
                                data: [0x90, 64, 100],
                            };
                            unsafe { push(list, &midi.header) };
                            let bytes = [0xf0, 0x7d, 0x01, 0xf7];
                            let sysex = clap_event_midi_sysex {
                                header: event_header::<clap_event_midi_sysex>(
                                    frame,
                                    CLAP_EVENT_MIDI_SYSEX,
                                ),
                                port_index: 0,
                                buffer: bytes.as_ptr(),
                                size: bytes.len() as u32,
                            };
                            unsafe { push(list, &sysex.header) };
                        }
                    }
                }
                CLAP_EVENT_NOTE_OFF => state.sounding.store(false, Ordering::Release),
                CLAP_EVENT_MIDI => {
                    let e = unsafe { &*(header as *const clap_event_midi) };
                    if e.data[0] & 0xf0 == 0x90 {
                        state.sounding.store(e.data[2] > 0, Ordering::Release);
                        if e.data[1] == 127 {
                            set_gain(state, 0.25);
                            request_parameter_rescan(state);
                        }
                    }
                    if e.data[0] & 0xf0 == 0x80 {
                        state.sounding.store(false, Ordering::Release);
                    }
                }
                CLAP_EVENT_PARAM_VALUE => {
                    let e = unsafe { &*(header as *const clap_event_param_value) };
                    if e.param_id == RAW_GAIN_ID {
                        let previous = gain(state);
                        set_gain(state, e.value as f32);
                        if (previous > 0.8) != (gain(state) > 0.8) {
                            let ext = unsafe {
                                (*state.host).get_extension.unwrap()(
                                    state.host,
                                    CLAP_EXT_LATENCY.as_ptr(),
                                )
                            };
                            let latency = unsafe { &*(ext as *const clap_host_latency) };
                            unsafe {
                                latency.changed.unwrap()(state.host);
                            }
                        }
                        if gain(state) > 0.99 {
                            loop {
                                std::hint::spin_loop();
                            }
                        }
                    }
                }
                _ => {}
            }
            index += 1;
        }
        let side = if state.sidechain && process.audio_inputs_count > 0 {
            let input = unsafe { &*process.audio_inputs };
            if input.channel_count > 0 && !(*input.data32).is_null() {
                unsafe { *(*input.data32).add(frame as usize) }
            } else {
                0.0
            }
        } else {
            0.0
        };
        let output = unsafe { &*process.audio_outputs };
        for channel in 0..output.channel_count {
            unsafe {
                *(*output.data32.add(channel as usize)).add(frame as usize) =
                    if state.sounding.load(Ordering::Acquire) {
                        gain(state) * (1.0 + side)
                    } else {
                        0.0
                    };
            }
        }
    }
    CLAP_PROCESS_CONTINUE
}
unsafe extern "C" fn port_count(p: *const clap_plugin, input: bool) -> u32 {
    if input && !unsafe { instance(p) }.sidechain {
        0
    } else {
        1
    }
}
unsafe extern "C" fn port_get(
    plugin: *const clap_plugin,
    index: u32,
    input: bool,
    info: *mut clap_audio_port_info,
) -> bool {
    let plugin = unsafe { instance(plugin) };
    if index != 0 || (input && !plugin.sidechain) {
        return false;
    }
    let mut p: clap_audio_port_info = unsafe { std::mem::zeroed() };
    p.id = if input { 1 } else { 0 };
    p.channel_count = 2;
    p.flags = if input { 0 } else { CLAP_AUDIO_PORT_IS_MAIN };
    p.port_type = CLAP_PORT_STEREO.as_ptr();
    p.in_place_pair = u32::MAX;
    unsafe { *info = p };
    true
}
static AUDIO: clap_plugin_audio_ports = clap_plugin_audio_ports {
    count: Some(port_count),
    get: Some(port_get),
};
unsafe extern "C" fn note_count(_: *const clap_plugin, input: bool) -> u32 {
    u32::from(input)
}
unsafe extern "C" fn note_get(
    _: *const clap_plugin,
    index: u32,
    input: bool,
    info: *mut clap_note_port_info,
) -> bool {
    if !input || index != 0 {
        return false;
    }
    let mut p: clap_note_port_info = unsafe { std::mem::zeroed() };
    p.id = 0;
    p.supported_dialects = CLAP_NOTE_DIALECT_CLAP | CLAP_NOTE_DIALECT_MIDI;
    p.preferred_dialect = CLAP_NOTE_DIALECT_CLAP;
    unsafe { *info = p };
    true
}
static NOTES: clap_plugin_note_ports = clap_plugin_note_ports {
    count: Some(note_count),
    get: Some(note_get),
};
unsafe extern "C" fn param_count(_: *const clap_plugin) -> u32 {
    1
}
unsafe extern "C" fn param_info(
    _: *const clap_plugin,
    index: u32,
    info: *mut clap_param_info,
) -> bool {
    if index != 0 {
        return false;
    }
    let mut p: clap_param_info = unsafe { std::mem::zeroed() };
    p.id = RAW_GAIN_ID;
    p.flags = CLAP_PARAM_IS_AUTOMATABLE;
    p.min_value = 0.0;
    p.max_value = 1.0;
    p.default_value = 0.5;
    for (d, s) in p.name.iter_mut().zip(b"Gain\0") {
        *d = *s as c_char;
    }
    unsafe { *info = p };
    true
}
unsafe extern "C" fn param_value(p: *const clap_plugin, id: u32, value: *mut f64) -> bool {
    if id != RAW_GAIN_ID {
        return false;
    }
    unsafe { *value = gain(instance(p)) as f64 };
    true
}
unsafe extern "C" fn latency_get(p: *const clap_plugin) -> u32 {
    if gain(unsafe { instance(p) }) > 0.8 {
        128
    } else {
        0
    }
}
static LATENCY: clap_plugin_latency = clap_plugin_latency {
    get: Some(latency_get),
};
unsafe extern "C" fn gui_supported(
    _: *const clap_plugin,
    api: *const c_char,
    floating: bool,
) -> bool {
    !api.is_null() && !floating && unsafe { CStr::from_ptr(api) }.to_str().ok() == Some("cocoa")
}
unsafe extern "C" fn gui_create(_: *const clap_plugin, _: *const c_char, _: bool) -> bool {
    true
}
unsafe extern "C" fn gui_destroy(p: *const clap_plugin) {
    unsafe { instance(p) }
        .gui_window
        .store(0, Ordering::Release);
}
unsafe extern "C" fn gui_get_size(_: *const clap_plugin, w: *mut u32, h: *mut u32) -> bool {
    unsafe {
        *w = 320;
        *h = 240;
    }
    true
}
unsafe extern "C" fn gui_set_parent(p: *const clap_plugin, parent: *const clap_window) -> bool {
    #[cfg(target_os = "macos")]
    {
        let view = unsafe { &*((*parent).specific.cocoa as *const objc2_app_kit::NSView) };
        let window = view.window().unwrap();
        unsafe { instance(p) }.gui_window.store(
            objc2::rc::Retained::as_ptr(&window) as usize,
            Ordering::Release,
        );
    }
    true
}
unsafe extern "C" fn gui_show(_: *const clap_plugin) -> bool {
    true
}
unsafe extern "C" fn gui_hide(_: *const clap_plugin) -> bool {
    true
}
static GUI: clap_plugin_gui = clap_plugin_gui {
    is_api_supported: Some(gui_supported),
    get_preferred_api: None,
    create: Some(gui_create),
    destroy: Some(gui_destroy),
    set_scale: None,
    get_size: Some(gui_get_size),
    can_resize: None,
    get_resize_hints: None,
    adjust_size: None,
    set_size: None,
    set_parent: Some(gui_set_parent),
    set_transient: None,
    suggest_title: None,
    show: Some(gui_show),
    hide: Some(gui_hide),
};
static PARAMS: clap_plugin_params = clap_plugin_params {
    count: Some(param_count),
    get_info: Some(param_info),
    get_value: Some(param_value),
    value_to_text: None,
    text_to_value: None,
    flush: Some(flush),
};
unsafe extern "C" fn save(p: *const clap_plugin, stream: *const clap_ostream) -> bool {
    let bytes = gain(unsafe { instance(p) }).to_le_bytes();
    unsafe { (*stream).write.unwrap()(stream, bytes.as_ptr().cast(), 4) == 4 }
}
unsafe extern "C" fn load(p: *const clap_plugin, stream: *const clap_istream) -> bool {
    let mut bytes = [0; 4];
    if unsafe { (*stream).read.unwrap()(stream, bytes.as_mut_ptr().cast(), 4) } != 4 {
        return false;
    }
    set_gain(unsafe { instance(p) }, f32::from_le_bytes(bytes));
    true
}
static STATE: clap_plugin_state = clap_plugin_state {
    save: Some(save),
    load: Some(load),
};
unsafe extern "C" fn extension(p: *const clap_plugin, id: *const c_char) -> *const c_void {
    let id = unsafe { CStr::from_ptr(id) };
    if id == CLAP_EXT_AUDIO_PORTS {
        (&AUDIO as *const clap_plugin_audio_ports).cast()
    } else if id == CLAP_EXT_NOTE_PORTS {
        (&NOTES as *const clap_plugin_note_ports).cast()
    } else if id == CLAP_EXT_PARAMS {
        (&PARAMS as *const clap_plugin_params).cast()
    } else if id == CLAP_EXT_STATE {
        (&STATE as *const clap_plugin_state).cast()
    } else if id == CLAP_EXT_LATENCY {
        (&LATENCY as *const clap_plugin_latency).cast()
    } else if id == CLAP_EXT_GUI && !ptr::eq(unsafe { (*p).desc }, &DESCRIPTOR2) {
        (&GUI as *const clap_plugin_gui).cast()
    } else {
        ptr::null()
    }
}
unsafe extern "C" fn callback(p: *const clap_plugin) {
    let state = unsafe { instance(p) };
    let request = state.pending.swap(0, Ordering::AcqRel);
    log_main(state, "callback");
    unsafe {
        match request {
            119 => {
                state.structural_restart.store(true, Ordering::Release);
                (*state.host).request_restart.unwrap()(state.host);
            }
            120 => {
                #[cfg(target_os = "macos")]
                {
                    let window =
                        state.gui_window.load(Ordering::Acquire) as *const objc2_app_kit::NSWindow;
                    if !window.is_null() {
                        (*window).performClose(None);
                    }
                }
            }
            123 => (*state.host).request_restart.unwrap()(state.host),
            122 => {
                let ext =
                    (*state.host).get_extension.unwrap()(state.host, CLAP_EXT_PARAMS.as_ptr());
                (*(ext as *const clap_host_params)).request_flush.unwrap()(state.host);
            }
            121 => (*state.host).request_process.unwrap()(state.host),
            _ => {}
        }
    }
}
unsafe extern "C" fn flush(
    p: *const clap_plugin,
    _: *const clap_input_events,
    output: *const clap_output_events,
) {
    let state = unsafe { instance(p) };
    if !thread_is(state, state.active.load(Ordering::Acquire)) {
        return;
    }
    set_gain(state, 0.375);
    state.flush_count.fetch_add(1, Ordering::AcqRel);
    let event = clap_event_param_value {
        header: event_header::<clap_event_param_value>(0, CLAP_EVENT_PARAM_VALUE),
        param_id: RAW_GAIN_ID,
        cookie: ptr::null_mut(),
        note_id: -1,
        port_index: -1,
        channel: -1,
        key: -1,
        value: 0.375,
    };
    unsafe {
        (*output).try_push.unwrap()(output, &event.header);
        (*state.host).request_callback.unwrap()(state.host);
    }
}
unsafe extern "C" fn count(_: *const clap_plugin_factory) -> u32 {
    3
}
unsafe extern "C" fn descriptor(
    _: *const clap_plugin_factory,
    index: u32,
) -> *const clap_plugin_descriptor {
    match index {
        0 => &DESCRIPTOR,
        1 => &DESCRIPTOR2,
        2 => &DESCRIPTOR3,
        _ => ptr::null(),
    }
}
unsafe extern "C" fn create(
    _: *const clap_plugin_factory,
    host: *const clap_host,
    id: *const c_char,
) -> *const clap_plugin {
    let id = unsafe { CStr::from_ptr(id) };
    let descriptor = if id == unsafe { CStr::from_ptr(DESCRIPTOR.id) } {
        &DESCRIPTOR
    } else if id == unsafe { CStr::from_ptr(DESCRIPTOR2.id) } {
        &DESCRIPTOR2
    } else if id == unsafe { CStr::from_ptr(DESCRIPTOR3.id) } {
        &DESCRIPTOR3
    } else {
        return ptr::null();
    };
    let sidechain = descriptor.id == DESCRIPTOR3.id;
    let mut p = Box::new(Instance {
        api: clap_plugin {
            desc: descriptor,
            plugin_data: ptr::null_mut(),
            init: Some(init),
            destroy: Some(destroy),
            activate: Some(activate),
            deactivate: Some(deactivate),
            start_processing: Some(start),
            stop_processing: Some(stop),
            reset: Some(reset),
            process: Some(process),
            get_extension: Some(extension),
            on_main_thread: Some(callback),
        },
        host,
        sidechain,
        gain: AtomicU32::new(0.5f32.to_bits()),
        sounding: AtomicBool::new(false),
        active: AtomicBool::new(false),
        pending: AtomicU32::new(0),
        flush_count: AtomicU32::new(0),
        activation_count: AtomicU32::new(0),
        structural_restart: AtomicBool::new(false),
        gui_window: AtomicUsize::new(0),
    });
    p.api.plugin_data = (&mut *p as *mut Instance).cast();
    let p = Box::into_raw(p);
    unsafe { &(*p).api }
}
static FACTORY: clap_plugin_factory = clap_plugin_factory {
    get_plugin_count: Some(count),
    get_plugin_descriptor: Some(descriptor),
    create_plugin: Some(create),
};
unsafe extern "C" fn entry_init(path: *const c_char) -> bool {
    let path = unsafe { CStr::from_ptr(path) }.to_string_lossy();
    if path.contains("slow-load") || path.contains("hang-load") {
        let _ = std::fs::write(format!("{path}.loading"), std::process::id().to_string());
        std::thread::sleep(std::time::Duration::from_millis(
            if path.contains("hang-load") {
                60000
            } else {
                600
            },
        ));
    }
    true
}
unsafe extern "C" fn deinit() {}
unsafe extern "C" fn factory(id: *const c_char) -> *const c_void {
    if unsafe { CStr::from_ptr(id) } == CLAP_PLUGIN_FACTORY_ID {
        (&FACTORY as *const clap_plugin_factory).cast()
    } else {
        ptr::null()
    }
}
#[no_mangle]
pub static clap_entry: clap_plugin_entry = clap_plugin_entry {
    clap_version: CLAP_VERSION,
    init: Some(entry_init),
    deinit: Some(deinit),
    get_factory: Some(factory),
};
