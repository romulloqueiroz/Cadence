# Plugin protocol v1

The universal contract for anything that runs inside the player. A plugin is
**not** a Kontakt instrument, a VST3, or a WASM module — those are *adapters*.
A plugin is a node that implements this protocol.

Reference implementation: `plugin_api::Plugin`.
Wire layout: `plugin_api::wire`.
Conformance gate: `plugin_api::conformance::gate`.

## 1. Lifecycle

```
create / load  →  descriptor + capabilities
              →  set_assets(provider)          // read-only package assets
              →  prepare(context)
              →  reset()
              →  (process(context, audio, events))*
              →  save_state() / load_state()
              →  drop
```

- `descriptor` and `capabilities` are stable for the instance's lifetime.
- `prepare` and `reset` run **off** the audio thread.
- `process` runs on the audio thread and is real-time safe.
- `save_state`/`load_state` run off the audio thread. A state saved by the
  plugin must load back into the same plugin version and reproduce the same
  output.

## 2. Real-time rules for `process`

- No allocation, no deallocation, no locks, no I/O, no blocking, no `panic!`.
- Touch only `audio.frames` and the declared channel counts.
- Write finite `f32` values. NaN/Inf is a conformance failure.
- Ignore unknown events and unknown parameter ids; never panic on host input.

## 3. Audio layout

Planar, one block:

```
outputs[channel * frames + frame]   // contiguous, channel-major
inputs[channel * frames + frame]
sidechain[channel * frames + frame]
```

- `Capabilities` declares `inputs`, `outputs`, `sidechain_inputs`.
- Instruments normally declare `inputs = 0`.
- Effects read `inputs` and write `outputs`; either may be equal or different.
- `frames` is between 1 and `MAX_BLOCK_SIZE` (4096). The host may call with
  smaller blocks than the prepared maximum.
- Outputs are cleared by the host before each call. Plugins add into them.
- The host owns the buffers and reuses them; do not retain references.

## 4. Events

Events are block-relative (`frame` in `0..frames`), delivered in ascending
frame order. Kinds:

| Kind | data1 | data2 | value |
|------|-------|-------|-------|
| NoteOn | key | velocity | velocity/127 |
| NoteOff | key | 0 | 0 |
| PolyPressure | key | pressure | pressure/127 |
| ControlChange | controller | value | value/127 |
| ProgramChange | program | 0 | — |
| PitchBend | — | — | -1..1 |
| ChannelPressure | — | — | 0..1 |
| Parameter | parameter id | — | normalized 0..1 |

- `NoteOn` with velocity 0 is a note-off.
- Process events at their exact frame (split the block); do not apply all
  events at frame 0.
- MIDI channel is in `channel`; MPE uses per-channel note expression.
- Transport (tempo, position, playing) is in `ProcessContext`, not events.

## 5. Parameters

`ParameterInfo` declares id, name, range, default, unit and flags. Ids are
stable and unique. Automation arrives as `Parameter` events with a normalized
value; use `denormalize` to map into the declared range.

- Ids must never be reused for a different meaning.
- `stepped` parameters should round to their step.
- Parameters not declared must be ignored.

## 6. UI

Plugins ship **data**, not executable UI. `UiNode` describes groups, knobs,
toggles, selectors, labels and meters. The host renders them and maps
interaction back to parameter ids. Native plugins are an adapter concern: their
own editor is bridged by the native adapter, not represented here.

## 7. State

`State { schema_version, bytes }` is opaque to the host.

- Bump `schema_version` when the layout changes; keep old versions loadable or
  return `SchemaTooNew`.
- State must round-trip: `load_state(save_state())` then `save_state()` yields
  the same bytes.
- Do not embed host-specific data.

## 8. Assets

Plugins may ship read-only assets (IRs, neural model weights, images). Before
`prepare`, the host calls `set_assets(Arc<dyn AssetProvider>)`.

- `AssetProvider::read(name)` returns bytes; `list()` enumerates them.
- Assets are content-addressed (SHA-256) and declared in the package manifest.
- Plugins never touch the filesystem or network directly; all access goes
  through the provider.
- Assets are immutable for the instance's lifetime.

## 9. Capabilities, latency and tail

`Capabilities` declares channel counts, `latency_samples` (for delay
compensation) and `tail_samples` (for freezing/offline renders). The host
queries this once per instance and allocates accordingly.

## 10. Providers (adapters)

`plugin_api::provider` defines the boundary:

| Format | Mechanism |
|--------|-----------|
| `Builtin` | compiled into the player |
| `Vst3` / `AudioUnit` / `Clap` | host the native binary (out-of-process) |
| `Wasm` | community plugins in the open format |
| `Rehosted` | clean-room reimplementation compiled to this protocol |
| `SampleLibrary` | Kontakt/EastWest/SFZ content driving the sampler engine |

Adding a format means adding a provider, not changing the host. Kontakt support
is the first `SampleLibrary` provider (`sample-adapter`).

## 11. Wire ABI

`plugin_api::wire` mirrors the contract as `#[repr(C)]` data for a future
out-of-process/WASM boundary:

- `WireProcessContext`, `WireCapabilities`, `WireEvent`.
- Audio crosses as planar pointers + `frames`; the sandbox copies into shared
  memory.
- The wire version and the API version advance together until they diverge.

## 12. Native Rust plugins (C ABI)

A native plugin is a `cdylib` exporting one symbol:

```
platform_plugin_v1() -> *const PluginVTable
```

The table is `#[repr(C)]` and contains: `abi_version`, `create`, `destroy`,
`descriptor`, `capabilities`, `prepare`, `reset`, `process`, `state_size`,
`state_schema`, `save_state`, `load_state`. Audio crosses as
`WireAudioBlock` (planar pointers + frame/channel counts); events as
`WireEvent`; context as `WireProcessContext`.

- No Rust ABI: only `#[repr(C)]` data and `extern "C"` functions.
- The loader checks `abi_version` and rejects mismatches.
- Native code has **no WASM sandbox**. For untrusted packages the production
  host must run the same vtable in a child process with shared-memory audio;
  the in-process loader is a development path.
- `plugin-sdk` provides `export_plugin!` to generate the table from a
  `plugin_api::Plugin` implementation.

New native plugins may also export the optional `platform_parameters_v1`
metadata function. It returns a `WireParameter` array (ID, name, range, default,
flags); older v1 binaries without the symbol remain loadable with opaque state
and no generic controls. The main v1 process vtable is unchanged.

## 13. Out-of-process transport

`plugin-sandbox` uses shared-memory protocol version 3. The worker opens the
mapping before loading the source, then unlinks its name so abrupt process
termination does not leave named shared memory behind.

- Eight audio slots hold sequence/epoch, stream position, planar audio, bounded
  events and process context. Ownership advances EMPTY → READY → BUSY → DONE
  with release/acquire atomics; only the current owner reads/writes payloads.
- Realtime `process_pipelined` copies inputs/events into a free slot and collects
  completed packets with bounded scans. It never waits, allocates, locks, reads
  clocks, or invokes process syscalls. A local preallocated sample timeline
  applies one prepared maximum buffer of delay, independent of callback sizes.
- Missing due output is zeroed and counted; expired samples are discarded.
  Queue overflow latches silence instead of dropping input events. Playing
  transport discontinuities invalidate queued output through an epoch change.
- Startup/control and synchronous offline processing use a separate `SharedBlock`
  and atomic commands. Lifecycle/state commands run outside the audio callback,
  drain pending work, and preserve opaque state schemas. Saves do not discard
  scheduled output; prepare/reset/load restart the pipeline.
- Defaults bound startup to 15 seconds and control/offline operations to 3
  seconds. A non-audio watchdog kills a realtime worker after 20 ms without
  outstanding work progressing, even without further callbacks.

Destruction stops/joins the watchdog and reaps the worker off the audio thread.
A worker also watches for parent death. States over 64 KiB fail explicitly.
The synchronous `Plugin::process` path remains for CLI/offline consumers;
`process_offline_delayed` preserves the realtime bridge's sample delay while
allowing bounded waits. JUCE uses C ABI version 2, reports intrinsic plus pipeline
latency and compensates parallel graph paths. Atomic statistics expose latency,
pending packets, underruns and missed frames.

A nonwaiting exchange is not a hard realtime scheduling guarantee. Resource
limits remain future work. Process isolation does not restrict the plugin's
filesystem/network privileges.

The worker loads native protocol libraries, signed package manifests (verified
with their assets), Kontakt instruments with preloaded samples, CLAP, and the
existing Rust AU adapter. CLAP forwards timed notes/controllers, parameters,
transport and state. Stable parameter keys retain original CLAP IDs while
small dense wire IDs are used for events.

`cadence-bridge` exposes a versioned C ABI to JUCE. The app's `RustPluginProcessor`
wraps the worker as a normal graph node and supplies generic writable parameter
controls. Direct JUCE AU/VST3 hosting remains in-process; a Rust VST3 adapter and
broader native-editor integration are separate work. CLAP already has worker-owned floating editors and creator-thread host-request dispatch; see `docs/native-clap-editor.md`. See `app/README.md` for current limits.

## 14. Conformance

`conformance::run` checks:

1. valid descriptor and capabilities;
2. parameter validity and unique ids;
3. UI references only declared parameters;
4. finite output (silence, note-on, note-off);
5. instruments are silent without a note and audible after one;
6. effects return finite output for finite input;
7. state round-trips.

`conformance::gate` returns a `Certificate` only on success. Publishing binds
that certificate to a signed package manifest.

## 15. Packages

A package is the manifest plus immutable assets:

- assets are addressed by SHA-256;
- the canonical manifest (signature removed) is signed with the publisher's
  Ed25519 key;
- install verifies the signature and every asset before copying into the store;
- the store path is keyed by the publisher public key, so a different key can
  never overwrite another publisher's plugin.

This trust layer is independent of OS code signing. macOS notarization and
Gatekeeper enforcement are a separate distribution concern; development builds
can run unsigned.

## 16. Discovery

Creators publish a signed **feed** (static JSON) listing releases. A release
points to a signed manifest and an asset base URL. Subscribers pin the feed's
publisher key on first fetch; a later key change is rejected. Assets are fetched
individually and verified by SHA-256, so no central server or registry is
required — the feed and packages can live on any static host.

## 17. Author checklist

- Implement `Plugin` (or a provider returning one).
- Declare honest capabilities and latency/tail.
- Never allocate or block in `process`.
- Keep events sample-accurate.
- Version and round-trip state.
- Pass conformance at the target sample rates and block sizes.
- Package, sign with your publisher key, and install.
- Publish the package and a signed feed.
