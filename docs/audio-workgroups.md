# Cross-process Audio Workgroups

Cadence's AU runs in the DAW process, while each bridged instrument runs in a
sandboxed worker process. macOS Audio Workgroups are process-local objects, so
the worker cannot join the DAW's workgroup directly. This implementation
transfers a Mach send right for the host workgroup and reconstructs it in the
worker.

## Flow

1. JUCE's AU wrapper calls `AudioProcessor::audioWorkgroupContextChanged` with
   the host workgroup. `vendor/JUCE/.../juce_AudioWorkgroup_mac.h` now exposes
   `getRealAudioWorkgroupHandle()` for this purpose.
2. `CadenceAudioProcessor` retains the handle with `os_retain` and publishes it
   through one atomic pointer. A clear update uses a non-null sentinel so "no
   update" and "remove the workgroup" remain distinguishable. The host audio
   callback does not lock, allocate or perform Mach IPC.
3. `handleAsyncUpdate` (message thread) releases the replaced handle, increments
   a generation, and calls `RustPluginProcessor::setAudioWorkgroup` for every
   bridged slot. New, restored and unbypassed workers also receive the current
   handle.
4. The Rust bridge calls `os_workgroup_copy_port` and registers that send right
   under a per-worker bootstrap service name suffixed with the generation.
   `bootstrap_register` cannot replace a name in place, so a new generation uses
   a new name.
5. The worker's `plugin-workgroup` thread polls the shared generation, calls
   `bootstrap_look_up`, reconstructs the object with
   `os_workgroup_create_with_port`, and publishes the handle atomically.
6. The rendering thread calls `os_workgroup_join` when the handle changes and
   `os_workgroup_leave` when it is cleared or replaced. Both calls are documented
   real-time safe. Joining a host workgroup supersedes the worker's Mach
   time-constraint request; the time-constraint policy is removed once.
7. `CadencePipelineStats::scheduling_flags` now reports bit 0 as the
   time-constraint policy and bit 1 as host workgroup membership. The inspector
   footer shows `Workgroup`, `Realtime` or `QoS`.

## Shared state

Private worker protocol version 11 adds the three workgroup atomics
(`workgroup_generation`, `workgroup_present`, `workgroup_joined`) plus dynamic
plugin timing, parameter snapshot and MIDI output fields. The host publishes presence before
generation; the worker observes generation with acquire/release ordering. A
cleared generation tells the rendering thread to leave.

## Limits

- AU and the standalone app are wired. The standalone passes
  `AudioIODevice::getWorkgroup()` in `audioDeviceAboutToStart` and clears it on
  stop. JUCE's VST3 wrapper exposes no host workgroup and the VST3 format has no
  standard handshake, so VST3 remains unsupported.
- Registered service names cannot be removed through the deprecated bootstrap
  API. Each workgroup generation leaves a name and send right in the host's
  bootstrap namespace until that process exits. Workgroup changes are rare, but
  repeated device changes over a very long host session can accumulate entries.
- The worker may keep the previous workgroup object alive after a change until
  the worker process exits; this avoids releasing an object while the rendering
  thread could still be using it.
- `os_workgroup_join` failures do not fail the audio callback; membership is
  retried on later blocks.
- The implementation was verified in-process with the default output device's
  workgroup. A live Logic session check remains part of the deferred DAW pass.
