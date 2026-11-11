//! Worker entry point shared by the command-line binary and tests.
use crate::{automation::render, metadata::PluginMetadata, shared::*};
use plugin_api::wire::WireProcessContext;
use plugin_api::{AudioBuffer, Event, Plugin, State};
use std::{
    path::Path,
    sync::{atomic::Ordering, Arc},
    time::Duration,
};

pub fn load_plugin(source: &str, selector: Option<&str>) -> Result<Box<dyn Plugin>, String> {
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct KontaktSource {
        path: String,
        #[serde(default)]
        sample_roots: Vec<std::path::PathBuf>,
    }
    let relocated = source
        .strip_prefix("cadence-kontakt:")
        .map(|json| {
            if json.len() > 32_768 {
                return Err("Instrument recovery settings exceed 32 KiB".to_owned());
            }
            serde_json::from_str::<KontaktSource>(json)
                .map_err(|e| format!("Invalid instrument recovery settings: {e}"))
        })
        .transpose()?;
    let path = Path::new(relocated.as_ref().map_or(source, |s| s.path.as_str()));
    if relocated.is_some()
        && !path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("nki") || e.eq_ignore_ascii_case("nkm"))
    {
        return Err("Instrument recovery requires an .nki or .nkm file".into());
    }
    if source == "builtin:sine" {
        return Ok(Box::new(host::demo::SinePlugin::default()));
    }
    if source == "builtin:gain" {
        let executable = std::env::current_exe().map_err(|e| e.to_string())?;
        let contents = executable
            .parent()
            .and_then(Path::parent)
            .ok_or("Worker has no bundle directory")?;
        let library = contents.join("Resources/ProtocolPlugins/gain.dylib");
        return plugin_loader::LoadedPlugin::load(library)
            .map(|plugin| Box::new(plugin) as Box<dyn Plugin>)
            .map_err(|e| e.to_string());
    }
    if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("nki") || e.eq_ignore_ascii_case("nkm"))
    {
        let roots = relocated
            .as_ref()
            .map_or(&[][..], |s| s.sample_roots.as_slice());
        let mut sampler = sample_adapter::SamplerPlugin::load_with_sample_roots(path, roots)
            .map_err(|e| e.to_string())?;
        sampler
            .stream_all(sample_adapter::StreamingConfig::default())
            .map_err(|e| e.to_string())?;
        if sampler.cached_samples() == 0 {
            return Err("No playable samples were found; the instrument may be unsupported or its samples missing".into());
        }
        return Ok(Box::new(sampler));
    }
    if path.file_name().is_some_and(|v| v == "manifest.json") {
        let directory = path.parent().ok_or("Package has no directory")?;
        let package = plugin_package::Package::load_from(directory).map_err(|e| e.to_string())?;
        package.validate().map_err(|e| e.to_string())?;
        package.verify().map_err(|e| e.to_string())?;
        let entry = directory.join("assets").join(&package.manifest().entry);
        let mut plugin = plugin_loader::LoadedPlugin::load(entry).map_err(|e| e.to_string())?;
        plugin.set_assets(Arc::new(package));
        return Ok(Box::new(plugin));
    }
    if adapter_clap::is_clap_path(path) {
        let module = adapter_clap::ClapModule::load(path).map_err(|e| e.to_string())?;
        let variants = module.descriptors();
        let mut plugin = module.create(selector).map_err(|e| e.to_string())?;
        plugin.set_variants(variants);
        return Ok(Box::new(plugin));
    }
    #[cfg(target_os = "macos")]
    if source.starts_with("au:") {
        return adapter_au::AuPlugin::load(source)
            .map(|p| Box::new(p) as Box<dyn Plugin>)
            .map_err(|e| e.to_string());
    }
    plugin_loader::LoadedPlugin::load(path)
        .map(|p| Box::new(p) as Box<dyn Plugin>)
        .map_err(|e| e.to_string())
}

fn bytes_to_shared(mapping: &mut SharedMapping, data: &[u8], schema: u32) -> bool {
    if data.len() > MAX_STATE {
        return false;
    }
    let b = mapping.block_mut();
    b.state[..data.len()].copy_from_slice(data);
    b.state_len = data.len() as u32;
    b.state_schema = schema;
    true
}

fn publish_timing(plugin: &dyn Plugin, mapping: &SharedMapping, last: &mut (u32, u32)) {
    let latency = plugin.current_latency_samples().min(u32::MAX as usize) as u32;
    let tail = plugin.current_tail_samples().min(u32::MAX as usize) as u32;
    if (latency, tail) != *last {
        mapping.set_plugin_timing(latency, tail);
        *last = (latency, tail);
    }
}

fn publish_parameters(plugin: &dyn Plugin, mapping: &SharedMapping) {
    let mut values = [(0u32, 0.0f32); MAX_PARAM_SNAPSHOT];
    let count = plugin.fill_parameter_values(&mut values);
    if count > 0 {
        mapping.publish_parameter_values(&values[..count]);
    }
}

fn error(mapping: &mut SharedMapping, message: &str) {
    bytes_to_shared(
        mapping,
        &message.as_bytes()[..message.len().min(MAX_STATE)],
        0,
    );
    mapping.block().block_state.store(ERROR, Ordering::Release);
}

#[cfg(target_os = "macos")]
fn audio_worker_priority() {
    // Apple pthread/qos.h and sys/qos.h: this is best-effort user-interactive
    // QoS, not a realtime scheduling or Audio Workgroup guarantee.
    extern "C" {
        fn pthread_set_qos_class_self_np(class: u32, relative: i32) -> i32;
    }
    unsafe {
        pthread_set_qos_class_self_np(0x21, 0);
    }
}
#[cfg(not(target_os = "macos"))]
fn audio_worker_priority() {}

// Set policy on the rendering thread, never on the host audio callback.
// Control/state/offline operations return to ordinary scheduling.
#[derive(Default)]
struct AudioScheduling {
    current: Option<(u64, usize)>,
    #[cfg(target_os = "macos")]
    workgroup: Option<std::sync::Arc<crate::workgroup::WorkerWorkgroup>>,
    #[cfg(target_os = "macos")]
    membership: crate::workgroup::Membership,
    #[cfg(target_os = "macos")]
    policy_active: bool,
    #[cfg(target_os = "macos")]
    workgroup_state: bool,
}
impl AudioScheduling {
    #[cfg(target_os = "macos")]
    fn attach_workgroup(
        &mut self,
        workgroup: Option<std::sync::Arc<crate::workgroup::WorkerWorkgroup>>,
    ) {
        self.workgroup = workgroup;
    }
    #[cfg(target_os = "macos")]
    // libc recommends a separate Mach crate for these timebase bindings;
    // the underlying macOS API remains supported.
    #[allow(deprecated)]
    fn set(&mut self, context: Option<&plugin_api::ProcessContext>, mapping: &SharedMapping) {
        #[cfg(target_os = "macos")]
        {
            let desired = self
                .workgroup
                .as_ref()
                .map_or(std::ptr::null_mut(), |workgroup| workgroup.current());
            let joined = self.membership.sync(desired);
            if joined != self.workgroup_state {
                self.workgroup_state = joined;
                mapping.set_workgroup_joined(joined);
            }
            if joined {
                // The host workgroup owns scheduling while it exists. Remove a
                // previously requested time-constraint policy once, then leave
                // policy decisions to the workgroup.
                if self.policy_active {
                    let thread = unsafe { libc::pthread_mach_thread_np(libc::pthread_self()) };
                    unsafe {
                        libc::thread_policy_set(
                            thread,
                            libc::THREAD_STANDARD_POLICY as libc::thread_policy_flavor_t,
                            std::ptr::null_mut(),
                            libc::THREAD_STANDARD_POLICY_COUNT as u32,
                        );
                    }
                    self.policy_active = false;
                }
                mapping.set_realtime_scheduling(false);
                self.current = None;
                return;
            }
        }
        let requested = context.map(|c| (c.sample_rate.to_bits(), c.block_size));
        if requested == self.current {
            return;
        }
        unsafe {
            let thread = libc::pthread_mach_thread_np(libc::pthread_self());
            let result = if let Some(context) = context {
                let mut timebase = libc::mach_timebase_info { numer: 0, denom: 0 };
                if libc::mach_timebase_info(&mut timebase) != 0 || timebase.numer == 0 {
                    return;
                }
                let ticks = |seconds: f64| -> u32 {
                    (seconds * 1e9 * timebase.denom as f64 / timebase.numer as f64)
                        .clamp(1.0, u32::MAX as f64) as u32
                };
                let period = (context.block_size as f64 / context.sample_rate).clamp(0.001, 0.1);
                let mut policy = libc::thread_time_constraint_policy {
                    period: ticks(period),
                    computation: ticks((period * 0.25).min(0.001)),
                    constraint: ticks(period.min(0.01)),
                    preemptible: 1,
                };
                libc::thread_policy_set(
                    thread,
                    libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
                    (&mut policy as *mut libc::thread_time_constraint_policy).cast(),
                    libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
                )
            } else {
                libc::thread_policy_set(
                    thread,
                    libc::THREAD_STANDARD_POLICY as libc::thread_policy_flavor_t,
                    std::ptr::null_mut(),
                    libc::THREAD_STANDARD_POLICY_COUNT as u32,
                )
            };
            let policy = result == 0 && context.is_some();
            mapping.set_realtime_scheduling(policy);
            #[cfg(target_os = "macos")]
            {
                self.policy_active = policy;
            }
            if result != 0 {
                eprintln!("plugin worker: scheduling policy request failed ({result}); retaining QoS fallback");
            }
        }
        self.current = requested;
    }
    #[cfg(not(target_os = "macos"))]
    fn set(&mut self, _: Option<&plugin_api::ProcessContext>, _: &SharedMapping) {}
}

fn run_audio(
    mut plugin: Box<dyn Plugin>,
    mut mapping: SharedMapping,
    metadata: PluginMetadata,
    parent: i32,
) -> Result<(), String> {
    let result = (|| -> Result<(), String> {
        let mut timing = (
            plugin.current_latency_samples().min(u32::MAX as usize) as u32,
            plugin.current_tail_samples().min(u32::MAX as usize) as u32,
        );
        let mut events = [Event::note_on(0, 0, 0); MAX_EVENTS];
        let mut output_events = [Event::note_on(0, 0, 0); MAX_EVENTS];
        let mut output_sysex = [0u8; MAX_OUTPUT_SYSEX];
        let mut audio_epoch = mapping.epoch();
        audio_worker_priority();
        let mut scheduling = AudioScheduling::default();
        #[cfg(target_os = "macos")]
        if let Ok(base) = std::env::var("CADENCE_WORKGROUP_SERVICE") {
            scheduling.attach_workgroup(crate::workgroup::watch(mapping.monitor(), base));
        }
        loop {
            if plugin.host_service_pending() {
                scheduling.set(None, &mapping);
                plugin.service_host_requests();
            }
            publish_timing(plugin.as_ref(), &mapping, &mut timing);
            if plugin.take_parameter_refresh_request() {
                publish_parameters(plugin.as_ref(), &mapping);
            }
            if plugin.take_reload_request() {
                mapping.request_reload();
            }
            if let Some(message) = plugin.processing_error() {
                error(&mut mapping, message);
                mapping.pipeline_error();
                return Err(message.into());
            }
            let completed = mapping.completed();
            if mapping.published() > completed {
                let slot = mapping.slot(completed as usize);
                if slot
                    .state
                    .compare_exchange(SLOT_READY, SLOT_BUSY, Ordering::Acquire, Ordering::Relaxed)
                    .is_ok()
                {
                    // READY acquisition transfers exclusive payload ownership.
                    let packet = unsafe { &mut *slot.packet_ptr() };
                    let frames = packet.frames as usize;
                    if packet.sequence != completed + 1
                        || frames == 0
                        || frames > MAX_BLOCK
                        || packet.input_channels as usize > MAX_CHANNELS
                        || packet.output_channels as usize > MAX_CHANNELS
                        || packet.sidechain_channels as usize > MAX_CHANNELS
                        || packet.event_count as usize > MAX_EVENTS
                    {
                        mapping.pipeline_error();
                        return Err("Invalid pipelined audio packet".into());
                    }
                    if packet.epoch != audio_epoch {
                        plugin.reset();
                        audio_epoch = packet.epoch;
                    }
                    let context = packet.context.to_context();
                    plugin.set_offline_rendering(false);
                    scheduling.set(Some(&context), &mapping);
                    let mut audio = AudioBuffer::new(
                        &mut packet.inputs,
                        packet.input_channels as usize,
                        &mut packet.outputs,
                        packet.output_channels as usize,
                        &mut packet.sidechain,
                        packet.sidechain_channels as usize,
                        frames,
                    );
                    audio.outputs.fill(0.0);
                    if !render(
                        plugin.as_mut(),
                        &context,
                        &mut audio,
                        &packet.events[..packet.event_count as usize],
                        &mut events,
                        &metadata.parameters,
                    ) {
                        mapping.pipeline_error();
                        return Err("Invalid pipeline context".into());
                    }
                    let (output_count, output_sysex_len) =
                        plugin.fill_output_events(&mut output_events, &mut output_sysex);
                    let output_count = output_count.min(MAX_EVENTS);
                    let output_sysex_len = output_sysex_len.min(MAX_OUTPUT_SYSEX);
                    packet.output_event_count = output_count as u32;
                    packet.output_sysex_len = output_sysex_len as u32;
                    for (index, event) in output_events[..output_count].iter().enumerate() {
                        packet.output_events[index] = (*event).into();
                    }
                    packet.output_sysex[..output_sysex_len]
                        .copy_from_slice(&output_sysex[..output_sysex_len]);
                    mapping.publish_streaming_telemetry(plugin.streaming_telemetry());
                    publish_timing(plugin.as_ref(), &mapping, &mut timing);
                    if plugin.take_parameter_refresh_request() {
                        publish_parameters(plugin.as_ref(), &mapping);
                    }
                    if plugin.take_reload_request() {
                        mapping.request_reload();
                    }
                    if let Some(message) = plugin.processing_error() {
                        mapping.pipeline_error();
                        return Err(message.into());
                    }

                    slot.state.store(SLOT_DONE, Ordering::Release);
                    mapping.complete(completed + 1);
                    continue;
                }
            }
            let command = mapping.block().block_state.load(Ordering::Acquire);
            if command == BLOCK_IDLE || command == BLOCK_DONE || command == ERROR {
                if unsafe { libc::getppid() } != parent {
                    return Ok(());
                }
                mapping.wait_for_work().map_err(|e| e.to_string())?;
                continue;
            }
            scheduling.set(None, &mapping);
            match command {
                BLOCK_READY => {
                    plugin.set_offline_rendering(true);
                    let b = mapping.block_mut();
                    let frames = b.frames as usize;
                    if frames == 0
                        || frames > MAX_BLOCK
                        || b.input_channels as usize > MAX_CHANNELS
                        || b.output_channels as usize > MAX_CHANNELS
                        || b.sidechain_channels as usize > MAX_CHANNELS
                    {
                        error(&mut mapping, "Invalid audio block");
                        continue;
                    }
                    let context = WireProcessContext {
                        sample_rate: b.sample_rate,
                        block_size: b.block_size,
                        tempo: b.tempo,
                        flags: b.flags,
                        sample_pos: b.sample_pos,
                        ppq_position: b.ppq_position,
                    }
                    .to_context();
                    if !context.is_valid() {
                        error(&mut mapping, "Invalid process context");
                        continue;
                    }
                    let wires = &b.events[..(b.event_count as usize).min(MAX_EVENTS)];
                    let mut audio = AudioBuffer::new(
                        &mut b.inputs,
                        b.input_channels as usize,
                        &mut b.outputs,
                        b.output_channels as usize,
                        &mut b.sidechain,
                        b.sidechain_channels as usize,
                        frames,
                    );
                    audio.outputs.fill(0.0);
                    if !render(
                        plugin.as_mut(),
                        &context,
                        &mut audio,
                        wires,
                        &mut events,
                        &metadata.parameters,
                    ) {
                        error(&mut mapping, "Invalid process context");
                        continue;
                    }
                    let (output_count, output_sysex_len) =
                        plugin.fill_output_events(&mut output_events, &mut output_sysex);
                    let output_count = output_count.min(MAX_EVENTS);
                    let output_sysex_len = output_sysex_len.min(MAX_OUTPUT_SYSEX);
                    b.output_event_count = output_count as u32;
                    b.output_sysex_len = output_sysex_len as u32;
                    for (index, event) in output_events[..output_count].iter().enumerate() {
                        b.output_events[index] = (*event).into();
                    }
                    b.output_sysex[..output_sysex_len]
                        .copy_from_slice(&output_sysex[..output_sysex_len]);
                    mapping.publish_streaming_telemetry(plugin.streaming_telemetry());
                    publish_timing(plugin.as_ref(), &mapping, &mut timing);
                    if plugin.take_parameter_refresh_request() {
                        publish_parameters(plugin.as_ref(), &mapping);
                    }
                    if plugin.take_reload_request() {
                        mapping.request_reload();
                    }
                    if let Some(message) = plugin.processing_error() {
                        error(&mut mapping, message);
                        continue;
                    }
                }
                PREPARE => {
                    let b = mapping.block();
                    let context = plugin_api::ProcessContext {
                        sample_rate: b.sample_rate,
                        block_size: b.block_size as usize,
                        ..Default::default()
                    };
                    if !context.is_valid() {
                        error(&mut mapping, "Invalid prepare context");
                        continue;
                    }
                    plugin.prepare(&context);
                }
                RESET => plugin.reset(),
                SAVE => {
                    let state = match plugin.try_save_state() {
                        Ok(state) => state,
                        Err(e) => {
                            error(&mut mapping, &e.to_string());
                            continue;
                        }
                    };
                    if !bytes_to_shared(&mut mapping, &state.bytes, state.schema_version) {
                        error(&mut mapping, "Plugin state exceeds 64 KiB");
                        continue;
                    }
                }
                LOAD => {
                    let b = mapping.block();
                    if b.state_len as usize > MAX_STATE {
                        error(&mut mapping, "Invalid state length");
                        continue;
                    }
                    let state = State {
                        schema_version: b.state_schema,
                        bytes: b.state[..b.state_len as usize].to_vec(),
                    };
                    if let Err(e) = plugin.load_state(&state) {
                        error(&mut mapping, &e.to_string());
                        continue;
                    }
                }
                _ => {
                    error(&mut mapping, "Unknown command");
                    continue;
                }
            }
            if command == PREPARE || command == RESET || command == LOAD {
                audio_epoch = mapping.epoch();
            }
            if command == PREPARE || command == LOAD {
                publish_timing(plugin.as_ref(), &mapping, &mut timing);
            }
            if command == LOAD {
                publish_parameters(plugin.as_ref(), &mapping);
            }
            mapping.publish_streaming_telemetry(plugin.streaming_telemetry());
            mapping
                .block()
                .block_state
                .store(BLOCK_DONE, Ordering::Release);
        }
    })();
    // CLAP destruction is dispatched to the main loop. Keep that loop alive
    // until its native window and plugin have both been destroyed.
    drop(plugin);
    mapping.set_gui_stop(true);
    result
}

pub fn run(source: &str, mapping_name: &str, selector: Option<&str>) -> Result<(), String> {
    let parent = std::env::var("CADENCE_SANDBOX_PARENT_PID")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or_else(|| unsafe { libc::getppid() });
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_millis(250));
        if unsafe { libc::getppid() } != parent {
            unsafe {
                if libc::getpgrp() == libc::getpid() {
                    libc::kill(-libc::getpgrp(), libc::SIGKILL);
                }
                libc::_exit(0);
            }
        }
    });
    let mut mapping = SharedMapping::open(mapping_name).map_err(|e| e.to_string())?;
    mapping.unlink_name();
    let plugin = match load_plugin(source, selector) {
        Ok(plugin) => plugin,
        Err(message) => {
            error(&mut mapping, &message);
            return Err(message);
        }
    };
    let metadata = PluginMetadata::from_plugin(plugin.as_ref());
    if !metadata.is_valid() {
        error(
            &mut mapping,
            "Unsupported plugin capabilities or parameters",
        );
        return Err("Invalid metadata".into());
    }
    let json = serde_json::to_vec(&metadata).map_err(|e| e.to_string())?;
    if !bytes_to_shared(&mut mapping, &json, 0) {
        error(&mut mapping, "Plugin metadata is too large");
        return Err("Metadata too large".into());
    }
    mapping.publish_streaming_telemetry(plugin.streaming_telemetry());
    let timing = (
        plugin.current_latency_samples().min(u32::MAX as usize) as u32,
        plugin.current_tail_samples().min(u32::MAX as usize) as u32,
    );
    mapping.set_plugin_timing(timing.0, timing.1);
    publish_parameters(plugin.as_ref(), &mapping);
    mapping
        .block()
        .block_state
        .store(BLOCK_DONE, Ordering::Release);
    #[cfg(target_os = "macos")]
    {
        let tasks = plugin.main_thread_tasks();
        let gui = crate::gui::prepare(plugin.native_gui_handle(), metadata.name.clone());
        if tasks.is_some() || gui.is_some() {
            let monitor = mapping.monitor();
            let audio = std::thread::Builder::new()
                .name("plugin-audio".into())
                .spawn(move || run_audio(plugin, mapping, metadata, parent))
                .map_err(|e| e.to_string())?;
            crate::gui::run(gui, tasks, monitor);
            return audio
                .join()
                .map_err(|_| "audio thread panicked".to_string())?;
        }
    }
    run_audio(plugin, mapping, metadata, parent)
}
