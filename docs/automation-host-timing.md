# Host automation timing investigation

Source investigation on 2026-11-06 before the assignable automation
implementation, followed by the scoped wrapper edits described below. This is the checked-in JUCE 8.0.12 / Cadence source, not a runtime
observation or a claim about current upstream JUCE. Source locations below name
functions as well as baseline line numbers, because the implementation pass may
move them. No live Logic session was changed.

## Established source behavior

| Path | Source observation | Consequence |
| --- | --- | --- |
| AU plugin wrapper, `SetParameter`, baseline1167–1189 | Converts the native value to normalized form and updates the `AudioProcessorParameter`; the incoming frame offset is only passed to the fallback. `CanScheduleParameters` returns false at1194. | A normal JUCE parameter callback does not preserve the host's sample position. Setting a proxy value alone supplies block-rate state. |
| AU wrapper `Render` / local `processBlock`, baseline1534 /2132–2148 | Calls the processor once for the full render buffer. | Preserve this callback/pipeline size when adding explicit timed automation. |
| AU SDK `AUBase::ScheduleParameter`,1032–1049 | Immediate events call virtual `SetParameter`; when scheduling is enabled, events are also appended to an ordinary vector. Render completion clears the vector at1293–1294. | Merely returning true from `CanScheduleParameters` neither forwards ramp data nor makes a bounded callback path. An explicit supported-client scheduling override should consume events itself. |
| AU SDK `ProcessForScheduledParams`,1074 onward / header590–610 | Sorts events and invokes `ProcessScheduledSlice` for buffer subdivisions; ramp start offsets are signed. | The stock slicer is a poor fit for Cadence's fixed host pipeline callback semantics. Preserve signed ramp starts, including ramps beginning before the current buffer. |
| VST3 wrapper `processParameterChanges`,3616–3658 | MIDI-emulation parameter queues forward every point with its offset. Ordinary plugin parameters read only the final point and update the parameter value, discarding its offset. | Capture ordinary parameter queues before this last-point reduction. |
| VST3 wrapper `process`,3710–3711 /3727 onward | Processes parameter changes before audio and also supports parameter flush calls with no audio samples. | A custom hook must distinguish a control-only flush from points belonging to a real upcoming audio block. Do not replay stale flush points in the next block. |
| Vendored VST3 SDK `ivstparameterchanges.h`,38–78 | Queue points define a linear approximation. A block may omit a point at zero; the previous block's last value is conceptually at position-1. An abrupt jump is represented by adjacent old/new points. | Forwarding all points as steps preserves point locations but fails the defined curve semantics. Represent or evaluate linear sections and retain the proper previous value. |
| Cadence `RustPluginProcessor::processBlock`, baseline390–414 | Parameter dirty flags are converted to frame-zero `CadenceEvent`s, followed by MIDI events. | Add explicit timed automation events and avoid simultaneously reapplying their final values as dirty frame-zero values. |
| CLAP adapter `InputEvents::push`,878 onward | Copies `Event.frame` to the CLAP parameter event header and translates the stable parameter ID. | The existing backend event path can preserve explicitly supplied sample positions. |

The existing AU preroll correction is `std::llround(timeInSamples)` in the wrapper
at baseline1288. Preserve it when editing this file: replacing it with the old
negative rounding would regress transport continuity.

## Minimal scoped wrapper design

An independent design option is a project-owned host-automation interface,
compiled into the wrappers only under a Cadence-specific define. The processor
implements it, and each wrapper caches an interface pointer at construction.
That avoids adding a general JUCE `AudioProcessor` virtual method or changing
unrelated JUCE projects. Pass the actual `AudioProcessorParameter` or its stable
parameter index, normalized value, and frame/ramp data.

For AU, intercept direct `SetParameter` offsets and explicitly consume scheduled
immediate/ramp records. Advertise scheduling only for this client. Do not call
`AUBase::ScheduleParameter` for already handled records, because its vector path
would duplicate immediate events and introduce growth/allocation. Keep the
legacy behavior for processors without the interface. Direct `SetParameter` can
also represent host/UI control calls; its producer thread cannot be assumed to
be the audio thread. Capture must have an explicit bounded concurrency policy.

For VST3, iterate all ordinary-parameter queue points. Interpret sections as
linear according to the vendored SDK, including the previous block's value at
position-1. Skip the legacy final-value application for a successfully handled
timed parameter, so it does not create an extra frame-zero change. A no-audio
flush should update control state without retaining timed events for later
unrelated audio. Reject/handle malformed offsets and nonfinite values explicitly.

For either format, do not discard a point merely because its value equals the
current UI value: its position may still delimit a curve. Preserve deterministic
same-frame ordering and keep listener/host notification separate from incoming
host automation to avoid feedback. Ramps spanning callbacks require carry state;
reset/seek/project replacement must clear obsolete timing state while preserving
valid saved control values.

## Capacity and backend limits

The C bridge/shared audio protocol currently caps a packet at256 events. Eight
per-sample automation lanes over4096 frames would need32768 parameter events,
before MIDI, so naive wrapper expansion cannot claim complete ramp delivery.
A compact linear-section representation or an explicit bounded approximation
policy is required. Overflow must not silently retarget or overwrite MIDI.

One backend-general option is compact ramp input followed by expansion inside
the sandbox worker's rendering path, using fixed temporary buffers and smaller
local DSP calls only while a ramp is active. This preserves the host graph and
pipeline packet sizes. It needs tests for context/timeline/MIDI offsets,
interpolation, format minimum block sizes, performance and event bounds. It is a
design option, not an already established behavior of this source.

Direct JUCE-hosted target plugins have a second timing boundary:

- AU target parameter `setValue` in `juce_AudioUnitPluginFormatImpl.h`,625–632,
  calls `AudioUnitSetParameter` with offset zero under its plugin lock.
- VST3 target parameter `setValue` in `juce_VST3PluginFormatImpl.h`,2019, updates
  cached state; process audio at2493–2497 forwards the cached latest value at
  offset zero. The MIDI-mapped route separately supports timed points.

Consequently, assigning a host macro to a normal direct AU/VST3 parameter setter
is not evidence of sample-accurate target delivery. Those mappings need their
own native-format extension or an explicit block-rate limitation. The built-in
Protocol/CLAP/Kontakt worker path already has timestamped parameter events and
can be extended without subdividing the host graph callback.

## Verification implications

Verify native AU `AudioUnitSetParameter` offsets and `AudioUnitScheduleParameters`
for multiple immediates, explicit ramps, negative ramp starts, cross-block carry,
flush/no-audio behavior and invalid values. Verify VST3 queues containing
multiple points, only an end-of-block endpoint, adjacent-point jumps, and control
flushes. Assert measured sample values and unchanged reported pipeline latency;
calling a new C++ queue API directly is insufficient to establish wrapper wiring.

Also verify dense automation plus MIDI capacity, no callback allocations,
stable target IDs after reorder/recall, safe remove/unmap, deterministic same-frame
ordering, and bounded concurrent host/UI submissions. These findings are source
observations and implementation guidance. Runtime host scheduling and third-party
plugin behavior still require their own tests.

## Scoped wrapper implementation

The implementation pass adds `HostAutomation.h` only under
`CADENCE_HOST_AUTOMATION` in AU and VST3 client translation units. The interface
is owned by Cadence; the wrappers leave ordinary JUCE processing as the fallback
for processors without it. The parent application build supplies the include
path/definition and owns queue storage, concurrency, compact-ramp delivery,
capacity policy and target routing.

AU now forwards native `SetParameter` offsets to `queueHostAutomation()` before
the approximate-value suppression. Its explicit `ScheduleParameter()` consumes
immediate and ramp records without AUBase's event vector or scheduled slicer.
It validates native IDs/scopes, finite values and duration conversion, preserves
signed ramp start offsets, and advertises scheduling only for a client.
Unhandled immediate parameters use the existing setter fallback; unhandled
ramp-only records retain the prior JUCE behavior of no ramp processing. No
sample-position rounding code was altered: the signed-preroll `std::llround`
correction remains intact.

VST3 passes the audio-frame count to parameter-queue handling. With a client and
a real audio block, every ordinary parameter point forms a compact linear
section from the previous point (initially current value at offset-1). Duplicate
positions become point changes. Malformed values/offsets are presented to the
client's validation path so recognized parameters cannot fall back to an unsafe
last-value setter. Recognized timed queues skip the old final-value setter.
A no-audio flush uses the existing normal parameter setter, updating immediate
control state without leaving timestamped events for unrelated future audio.
The existing MIDI-emulation branch is unchanged.

Native-format test access is already available: a nested AU's
`ExtensionsVisitor::AudioUnitClient::getAudioUnitHandle()` permits native set and
schedule calls. VST3's visitor exposes `getIComponentPtr()`, from which a test can
query `IAudioProcessor` and supply native input queues. These exercise the wrapper
hooks rather than only calling Cadence's queue functions directly.

Implementation status: the AU and VST3 wrappers compile with
`CADENCE_HOST_AUTOMATION` and the parent rack path is integrated. In-process
tests establish sample-offset delivery through the Cadence rack, exact compact
ramp expansion, recall, editor assignment and safe removal.

Direct **native AU** targets receive `AudioUnitParameterEvent` records with
buffer offsets/ramps through `AudioUnitScheduleParameters`, before the graph
renders the target. The native AU's parameter ID is taken from JUCE's hosted
parameter ID and its min/max from `kAudioUnitProperty_ParameterInfo`.

Direct **native VST3** targets now implement the project-owned
`cadence::NativeAutomationTarget` hook in JUCE's hosted VST3 instance. Timed
points/ramps are appended to the preallocated VST3 host parameter queue during
`processAudio`, before `IAudioProcessor::process`. The host cache is updated
without adding a competing frame-zero event, and pending events are discarded at
the start of each automation block so a skipped target cannot replay stale
events.

Nested Cadence AU and VST3 tests verify baseline and frame-100 offset delivery.
Exact audio timing in a real DAW session remains part of the deferred Logic pass.
See `app/build/verification/native-host-automation-2026-11-06/report.json`.

## Source identity at investigation

SHA-256 values captured for the inspected source files (baseline may subsequently
change during implementation):

```text
0315724b7cc59b90e3118759e5a49a37135e22b6f1c05759be3d40f211648839  vendor/JUCE/modules/juce_audio_plugin_client/juce_audio_plugin_client_AU_1.mm
16031325b31be2e32b23689f07338bd6c631941475b9bbf87d176d1f2565c202  vendor/JUCE/modules/juce_audio_plugin_client/juce_audio_plugin_client_VST3.cpp
ccb225ffbfb6bcce9aa90d5af83389af7a7d55d2def27374bc61311cc2c6553f  vendor/JUCE/modules/juce_audio_plugin_client/AU/AudioUnitSDK/AUBase.cpp
fd71d7001f07cbd6b874163e878714aee0632b93da8261916327cac37f1b560f  vendor/JUCE/modules/juce_audio_plugin_client/AU/AudioUnitSDK/AUBase.h
4f177d8f9e207fc4f247fde5aebfbdf863d02bced08a6c2d21d759f043eea3dd  vendor/JUCE/modules/juce_audio_processors_headless/format_types/VST3_SDK/pluginterfaces/vst/ivstparameterchanges.h
dc03c6bd92e1373c3ee0b183847b867fd1a5a131df6a7c46fcea0dfe2bfdad64  vendor/JUCE/modules/juce_audio_processors_headless/format_types/juce_VST3PluginFormatImpl.h
56a38a3bc8e037324c17d90e3b00ad959a203736235e4c58f1d9f1abdd01038e  vendor/JUCE/modules/juce_audio_processors_headless/format_types/juce_AudioUnitPluginFormatImpl.h
c5bfe47b5cc3093b455ac318bbb9efc46e048f62782d3e1bc1ddc4abaf64c5d2  app/Source/RustPluginProcessor.cpp
b56ee6bd03fabc53f8e638be979670d6a7cbf295da28f5b193a280e0ce254dca  crates/adapter-clap/src/lib.rs
```
