# Native CLAP adapter status

The CLAP adapter remains sandboxed: the module is loaded in `plugin-worker`, not
in the Cadence/DAW process.

## Multi-plugin enumeration

`ClapModule` already read every descriptor from a module. Those descriptors now
travel in `PluginMetadata::variants` (protocol metadata, not the audio wire ABI).
Each variant carries id, name, vendor, version and kind.

`RustPluginProcessor::describeAll` opens the module once and returns one
`PluginDescription` per variant. `fileOrIdentifier` contains the source path plus
the stable CLAP plugin id, so `open()` passes that id as the selector. The
packaged scanner uses `describeAll`, so a multi-plugin `.clap` file produces one
library entry per plugin. The test fixture exposes two deliberately selectable
plugins to cover this path end to end.

Limit: enumeration currently creates the first plugin in the module to obtain
metadata. A module whose first plugin cannot be created but whose later plugins
can would still fail enumeration. A list-only worker mode would remove that
dependency.

## Host extensions

The worker now answers these `clap_host` extensions:

| Extension | Behaviour |
| --- | --- |
| `clap.log` | Logs outside the audio thread; audio-thread logs are dropped to keep the callback real-time safe |
| `clap.thread-check` | Reports scoped audio operations and the actual creator thread; unrelated threads are neither |
| `clap.latency` | Current value is published to the host; `changed()` triggers a live latency update |
| `clap.tail` | Current value is published to the host; `changed()` triggers a live tail update |
| `clap.params` | `rescan`/`clear` trigger a live value snapshot; `request_flush` schedules an idle/active parameter flush on the correct thread |
| `clap.state` | `mark_dirty` requests a parameter-value refresh |
| `clap.audio-ports` | Reports rescan flags unsupported; `rescan` reports reload-required |
| `clap.note-ports` | Reports CLAP and MIDI dialects; `rescan` requests a structural reload |

## Live latency, tail and parameter values

The worker publishes current latency/tail atomics after processing. A creator
thread that services a `clap.latency`/`clap.tail` change also publishes new
timing directly through `MainThreadTasks::take_timing`, because the audio loop is
blocked while idle. `RustPluginProcessor` polls the shared dynamic stats and
parameter generation on a 16 ms message-thread timer, then applies
`setLatencySamples`/tail updates and adopts refreshed parameter values, so JUCE
can notify the DAW of a PDC change without a new block, reload, or open editor.

When the plugin requests a parameter rescan, the worker snapshots all current
normalized parameter values into a seqlock-protected shared array. The host reads
the snapshot on the message thread, adopts values without marking them as
worker-bound dirty edits, and notifies parameter listeners. This covers
plugin-side value changes (state load, internal modulation, future editors) and
is visible while the DAW is idle.

Parameter **info/count** and audio-port layout changes cannot be mutated safely
inside a loaded JUCE processor. Instead, `CLAP_PARAM_RESCAN_INFO/ALL` and
audio-port rescans now mark the worker for an automatic, state-preserving slot
reload. The rack saves the plugin state, removes the old graph node, loads a
fresh worker with the new metadata/bus layout through the existing asynchronous
loader, restores state, and re-resolves automation by slot UUID. Expect a brief
gap; the DAW bus layout of Cadence itself is unchanged.

Native AU/VST3 structural changes still require a manual reload because the
async loader currently owns the Rust/CLAP path only.

## Core host requests

Host callbacks record per-instance atomic flags; they do not allocate, log, lock
or call plugin code. `request_callback` is coalesced and delivered on the creator
thread, including while no host audio blocks arrive. A callback requesting
another callback is deferred to a later main-loop turn.

`request_restart` is serviced between blocks: stop on the audio thread,
deactivate/reactivate on the creator thread, then start on the audio thread.
Ordinary restarts retain the same plugin instance and state. If deactivation
requests a structural reload, the plugin stays quiescent until the rack replaces
it; stale port buffers are not processed. Parameter input events use stable IDs
and null cookies because a full rescan can invalidate cached cookies before the
asynchronous replacement completes. A restart may cause an audible gap.

Cadence continuously processes all supplied host blocks, including silence; it
does not suspend on CLAP_PROCESS_SLEEP. `request_process` is acknowledged within
that policy, without generating audio blocks or inventing a transport position.

`request_flush` is serviced without concurrent process calls: active plugins
flush on the audio worker, inactive plugins on the creator thread. Parameter
output events request a main-thread value snapshot, and feedback can be published
while the DAW is idle. State/value/timing getters no longer run on the audio
worker. Worker lifecycle/flush housekeeping leaves elevated realtime scheduling
before it can dispatch non-realtime work. The host audio callback never waits.

The macOS main loop services both editor and headless CLAP workers. Other direct
users of the Rust adapter must service the optional MainThreadTasks handle on
the creator thread and call the worker housekeeping hook between process calls.
Before queued destruction, the creator must close any native views and call
`allow_shutdown()`; the worker loop does this automatically.

Authoritative contracts: [CLAP host requests](https://github.com/free-audio/clap/blob/main/include/clap/host.h),
[plugin lifecycle](https://github.com/free-audio/clap/blob/main/include/clap/plugin.h),
and [parameter flushing](https://github.com/free-audio/clap/blob/main/include/clap/ext/params.h).

## Remaining adapter work

- Native CLAP editors are implemented as worker-owned floating windows; see
  [editor lifecycle and limits](native-clap-editor.md). Embedding and the GUI
  resize/show/hide-request host extension remain separate work.
- CLAP structural changes use an automatic asynchronous slot reload and can
  produce a brief gap. Native AU/VST3 structural changes still need manual reload.
- First-plugin instantiation during module enumeration remains a dependency;
  a descriptor-only discovery mode would remove it.
- Broader third-party/DAW and multi-hour stress validation is still required.
  Worker IPC and CTest now run in this environment; a Logic playback/recall
  session remains a manual check.
