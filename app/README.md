# Cadence app

Cadence builds as a macOS standalone app, AU, and VST3. Its graph combines
sandboxed Rust protocol/CLAP/Kontakt nodes with direct JUCE AU/VST3 hosting.
The charcoal/lime Library, Chain, and Inspector layout is preserved.

CLAP core host requests now run through per-instance creator-thread dispatch.
Native editor title-bar closes and Inspector Open/Close state are synchronized.
See [current callback support](../docs/native-clap-adapter.md) and
[editor lifecycle/verification limits](../docs/native-clap-editor.md). Historical
verification sections below describe the builds available at those times.

Plain K2 XML `.nki` playback is implemented for the supported single-program
subset, using the normal background loading and sample streaming path.
[Format support and verification](../docs/plain-xml-playback.md) lists the exact
boundaries; conditional XML group starts are reported as unsupported.

## Use

Relaunch the rebuilt app. **Sine Demo** and **Gain** are available immediately
without scanning installed third-party plugins. Select Sine Demo and press
**Add**, send MIDI from a controller/DAW, then add Gain and edit its control in
the Inspector. The standalone currently needs a MIDI input device; an onscreen
keyboard and transport controls are not implemented.

**Open...** selects a `.nki`/`.nkm`, `.clap`, native protocol `.dylib`, or signed
`manifest.json`. Kontakt files load directly into a recoverable rack slot and
are added to the Library. Other sources are probed in the scanner; select a
successful Library result and press **Add**. Kontakt prepares resident sample attacks in a background loading worker and
streams later pages during playback. **Loading...**
appears in its rack slot; **Cancel loading** cancels pending loads while the current
rack keeps playing. Closing the editor leaves loading running. Wait for the new
slot to finish loading before playing it or starting a bounce.
Missing/unsupported samples report an
error instead of presenting an empty playable instrument.

Select a Kontakt slot and use **Locate...** in the Inspector header to choose
sample folders or a moved instrument. Recovery retains the slot and saved state;
chosen paths persist in the Library and project. Compatibility notices remain
visible in the Inspector, and unsupported active binary effects/multi-program
playback fail explicitly. See [v1 supported content](../docs/SUPPORTED-CONTENT.md)
and the [release checklist](../docs/RELEASE-CHECKLIST.md).

**Scan** discovers system/user AU, VST3 and CLAP bundles, the bundled protocol
sources, and installed manifests under `PLATFORM_HOME/plugins` or
`~/.local/share/platform/plugins`. Kontakt libraries are opened explicitly,
without sweeping all installed content. Signed package signatures and asset
hashes are verified before their native library executes.

Bridged sources run in separate Rust workers and receive host MIDI, transport,
audio and parameter updates. The Inspector supplies generic writable controls;
read-only parameters are omitted. CLAP's original parameter IDs are preserved
for state/controls while dense IDs carry its parameter events. Older protocol
binaries without the optional parameter-metadata symbol still play and preserve
opaque state, but have no generic controls.

Chain state saves descriptions, source/CLAP selector, bypass, opaque state/schema,
and normalized control values. Restore, reorder, bypass, and editor removal work
with bridged nodes. Project recall queues background loads; saving during loading
retains pending descriptions, bypass and saved state. Failed slots retain their
state for a later retry on reopen. A failed worker is silenced and its Inspector asks for removal
and reload; other nodes and the host remain alive. A failed state capture is
reported in the Inspector and retains the last successfully captured state.

Worker failures are recorded off the audio thread in
`~/Library/Logs/Cadence/worker-failures-<hostpid>.jsonl` on macOS (rotating after 1 MiB).
Records include plugin/worker identity, the first failure reason, queue/dropout
counters and terminal exit status. Set `CADENCE_WORKER_LOG_DIR` to override the
directory, or to an empty string to disable persistent logging. User cancellations
are excluded from failure records. The Inspector distinguishes timeouts, queue
overflow, protocol errors and process exits.

## Build and check

From `platform/`:

```sh
python3 scripts/setup-juce.py
cmake -S app -B app/build -DCMAKE_BUILD_TYPE=Debug
cmake --build app/build --target Cadence_Standalone Cadence_AU Cadence_VST3 CadenceScanTests -j 4
ctest --test-dir app/build --output-on-failure

cargo build -p plugin-sandbox -p gain
cargo test -p plugin-api -p plugin-sdk -p plugin-loader -p plugin-sandbox -p cadence-bridge -p adapter-clap -p plugin-store -p sample-adapter -p host
cargo clippy -p cadence-bridge -p plugin-sandbox -p plugin-sdk -p plugin-loader -p adapter-clap -p gain --all-targets -- -D warnings
```

CMake builds the Rust bridge, worker and gain example in release mode under
`app/build/rust/`, then links the bridge into the JUCE targets. It also builds a
small deterministic CLAP test fixture, which is not distributed in the app.
The configured Rust target standard library and sibling `vendor/JUCE` must exist;
`JUCE_PATH` can select another JUCE checkout. A build targets one macOS
architecture at a time; ARM64 was verified here. Universal packaging is pending.

Outputs: `app/build/Cadence_artefacts/Debug/{Standalone,AU,VST3}`. Each bundle has:

- `Contents/Helpers/Cadence Scanner.app` (with its own Rust worker).
- `Contents/Helpers/plugin-worker`.
- `Contents/Resources/ProtocolPlugins/gain.dylib`.

All helpers are resolved relative to Cadence's module, including inside a DAW,
and the complete bundles are ad-hoc signed. No developer-workspace path is needed
at runtime. Bundled examples use portable `builtin:sine`/`builtin:gain` sources
in saved chains. Installed content and explicitly opened files still use their own
absolute paths. Signing/notarization for distribution remains separate work.

## Scanning

Scanning starts only through **Scan**, **Open...**, or a selected skipped retry.
Each candidate has a fresh helper and a 15-second deadline. One settings directory
can have only one active scan, including multiple instances in one DAW.

- **Cancel** stops the helper; completed results remain in the browser/cache.
- **Skip plugin** stops the current probe, remembers the skip, and continues.
- Crashes, timeouts and load errors are saved in **Skipped (N)**. Choose one
  entry to retry only that source; success removes the skip.
- Probes are journalled before launch. Interrupted/cancelled probes remain skipped
  on the next scan. An unreadable skip file stops scanning safely.
- Results update live and merge with the previous catalogue. Previously cached
  descriptions are retained when a source fails to rescan.

Settings: `~/Library/Application Support/Cadence/{plugins.xml,skipped-plugins.xml,scan.lock}`.
An older `~/Library/Cadence/plugins.xml` is copied when no new cache exists.
Activation requests can still appear during a probe; Cancel/Skip closes requests
owned by that helper. A Rust child also exits if its scanning parent disappears.

## Boundary and limits

`crates/cadence-bridge/include/cadence_bridge.h` defines C ABI version 2.
Callers serialize prepare/reset/state/destruction against processing; immutable
metadata, failure status and pipeline statistics can be queried independently.
`plugin-sandbox` shared-memory version 4 has an eight-slot audio packet ring and
a separate control/state exchange. Shared-memory and semaphore names are unlinked
when the worker opens them; existing handles remain usable until destruction.

Realtime callbacks publish audio/events and collect completed output without
waiting, allocating, locking, reading clocks, or managing processes. A nonblocking
semaphore post wakes the worker directly, replacing its polling sleep. Each
bridged node adds one prepared maximum buffer of sample delay, in addition to
its intrinsic latency. The graph compensates parallel paths and reports summed
rack latency to the DAW. Variable callbacks retain the same sample delay;
realtime frames must not exceed the prepared maximum. The footer and Inspector
show latency and worker underruns.

On macOS the worker requests a preemptible Mach time-constraint policy during
pipelined rendering, with the period derived from the audio context and a bounded
CPU reservation. Control, state and synchronous offline exchanges return to normal
scheduling. The statistics expose whether the OS accepted the realtime request;
a denied request retains the existing QoS fallback. When the AU host supplies an
Audio Workgroup, the worker joins it cross-process and the workgroup supersedes
the time-constraint request. The inspector footer reports `Workgroup`, `Realtime`
or `QoS`. See [cross-process workgroups](../docs/audio-workgroups.md) and
[Apple's audio workgroup guidance](https://developer.apple.com/documentation/audiotoolbox/understanding-audio-workgroups).

Unavailable output becomes silence, with missed samples counted as underruns;
late samples are discarded rather than replayed at the wrong position. Playing
transport seeks clear queued audio and reset voices. When small callbacks fill
the queue, compatible unclaimed packets combine within the prepared buffer size,
preserving planar channels and event offsets under CAS ownership. If they cannot
combine, the queue still fails closed rather than dropping note-offs.
Offline rendering can wait for workers,
including larger callbacks, with identical sample delay for DAW compensation.
The Rust CLI's `Plugin::process` retains its synchronous offline exchange.

Default deadlines are 15 seconds for startup and 3 seconds for control/offline
operations. A non-audio watchdog kills workers when outstanding realtime work has
not progressed for the larger of 20 ms and the prepared buffer's audio duration,
even if callbacks stop; destruction reaps them. It rechecks progress before killing.
These measured improvements do not guarantee hard realtime scheduling under
arbitrary load. CPU/resource limits and wider DAW stress testing remain pending.

Further limits:

- Worker isolation is a crash boundary with the user's ordinary privileges, not
  an OS filesystem/network security sandbox. Direct JUCE AU/VST3 nodes still run
  in the Cadence/DAW process.
- The rack exposes stereo audio. The bridge supports up to 8 channels internally,
  splits offline blocks above 4096 frames, accepts 256 events per chunk and 64 KiB of
  opaque state. State overflow fails explicitly. Excess MIDI events latch silence
  rather than risking stuck notes. MIDI-only/outputless plugins are rejected.
- Rack sidechain routing (bridged and native AU/VST3), sample-accurate
  rack-to-DAW automation, scheduled native AU/VST3 automation, native and
  bridged CLAP MIDI output (including SysEx), live CLAP latency/tail and
  plugin-to-host parameter values are implemented. A long-session workgroup
  churn harness is included. Native CLAP editors and parameter/port structure
  changes remain pending.
- CLAP modules now enumerate every selectable plugin and the scanner creates one
  library entry per variant. Core host-extension callbacks (log, thread-check,
  latency, tail, params, state, audio-ports, note-ports) are implemented. Native
  CLAP editors, live metadata changes and dynamic latency/port updates remain
  pending. Generic parameter controls and opaque state are wired.
- Kontakt compatibility inherits the current Rust reader/engine limits: playable
  unencrypted container variants, preloaded samples, and no complete KSP/effects
  emulation. Plain K2 XML single-program playback and streaming are implemented; advanced XML group starts and large-library validation remain pending.
- Source creation, prepare, and state operations run off the audio callback but
  can occupy the message thread. Asynchronous loading is a separate next step.
- The native v1 ABI does not yet bridge package asset callbacks; package integrity
  verification and native code loading are implemented.

## Verification (2026-11-05)

Standalone, AU, VST3, scanner, preview and integration targets build. CTest covers
scanner cancellation/skips/crash/timeout/recovery, partial results, cache reload,
live browser updates and responsive controls. Bridge checks cover sample-accurate
notes, CLAP ID mapping, parameters and opaque state, sample-rate changes, native
protocol effect chains, restore/reorder/bypass, editor lifecycle and crash/hang
silencing. The hang check observes watchdog termination before removing the node.

The packaged scanner imports protocol, CLAP and Kontakt sources. Local Piano Uno
Concert produces audible output through the Rust worker (the content test reports
a skip when that instrument is unavailable). Nested Cadence VST3 and AU restore
and render the bridged chain through JUCE format hosts. AU registration is local
to the test process; no system installation is needed. This is not a Logic Pro
session test. The AU is also installed in the user Components directory and
passes Apple’s `auval -v aumu Cdn1 Cadn`; Logic playback still needs a session check.

Rust checks cover allocation-free host audio exchange, variable block sizes,
startup timeouts, worker reaping, shared-memory unlinking, state schema retention,
signed-package playback and tampered-asset rejection. Clippy passes with warnings
denied. The complete editor and generic Inspector were rendered and inspected.
Distribution bundles and nested helpers are checked with deep/strict code-signature verification.

Pipeline checks cover allocation-free realtime exchange, stopped-worker callbacks
that return without waiting, late-output discard, watchdog termination without
further callbacks, state/seek handling, exact delay with variable blocks and
ring wrap, parallel-path compensation, and latency propagation through nested
AU/VST3 wrappers. Offline large-block rendering preserves the same sample delay.

A paced two-node Sine Demo + Gain benchmark at 48 kHz ran 1,000 callbacks per
buffer size with zero underruns. For 64/128/256 frames, rack latency was
128/256/512 samples, callback p99 was 15/34/31 microseconds, and the largest
observed callback was 44/64/67 microseconds. These are local measurements,
not scheduling bounds or Logic Pro results. Raw data is in
`app/build/verification/pipeline-benchmark.json`.

The benchmark keeps at least one buffer interval between callback starts and
records timer wake lateness. An earlier fixed-deadline run, which allowed
back-to-back catch-up callbacks after a late wake, recorded one underrun at
128 frames on a repeat while both workers remained healthy. That observation
is retained in `pipeline-benchmark-repeat-underrun.json`; OS scheduling stalls
can still cause dropouts in actual playback. Logic Pro and load testing remain
necessary before making broader reliability claims.

## First-note fix and Logic verification (2026-11-06)

Host sample positions now remain signed through the C++ bridge, Rust context,
wire and shared-memory exchange. Existing 64-bit field layouts are unchanged.
Negative latency-compensation preroll must remain contiguous across zero; clamping
it caused false seeks that reset voices and queued audio. A local patch in
`vendor/JUCE/modules/juce_audio_plugin_client/juce_audio_plugin_client_AU_1.mm`
uses `std::llround` instead of adding 0.5 and truncating negative sample times.
Retain this patch when updating JUCE until the upstream wrapper handles negative
positions correctly. CLAP receives signed song positions and an unavailable
steady clock (-1), since a seeking host timeline is not a monotonic clock.

The timeline regression failed at sample zero before the change and passes after
it; the wire test round-trips negative positions. New C++ worker tests cover
delayed audio and a held note across negative preroll. They compile, but could
not execute in the restricted shell because shared-memory creation is denied.
The test executable accepts `--pipeline-only`, and `CADENCE_TEST_TMPDIR=/tmp`
overrides JUCE's normally used per-app cache directory for temporary fixtures.

Standalone, AU, VST3, scanner and test targets build; 11 API/timeline unit tests,
workspace/all-targets check, and targeted Clippy with warnings denied pass.
The rebuilt AU was installed, its binary hash matched the build, and its deep
strict signature verified. The saved Logic project was reopened for an explicit
offline bounce of bars 1–9, at 48 kHz stereo 32-bit float, normalization off.
The old bounce lacked the first note. The updated bounce contains all eight
notes with detected onsets exactly at 0, 96000, ..., 672000 samples.

Recheck the artifact with:

```sh
python3 app/Tests/check_logic_bounce.py app/build/verification/cadence-logic-preroll-fixed.wav
```

The original 256-sample I/O setting is restored. Earlier realtime checks recorded
zero dropouts at 64-I/O and one at 128-I/O, with a larger prepared rack latency
of 2048 samples at both settings. A long-lived pre-fix Sine worker had also failed.
Those scheduling/reliability observations remain unresolved; this bounce check
does not establish broader realtime reliability. Details and before/after audio
are retained under `app/build/verification/`.

## Reliability verification (2026-11-06)

The previously blocked worker integrations now run with the shell sandbox removed.
Reproduced and fixed premature watchdog kills and small-callback queue exhaustion;
regressions verify exact stereo audio, note-offs and parameter timing through packet
combining, with no callback allocations. Gain now honors parameter sample offsets.
All 70 Rust workspace tests, workspace/all-targets Clippy with warnings denied,
complete CTest (including nested AU/VST3), standalone/plugin builds, installed AU
validation and strict/deep signing checks pass.

The extended benchmark runs a real realtime callback thread and checks that macOS
accepted realtime worker scheduling. 6,000 callbacks at each of 64/128/256 frames
completed with zero underruns and healthy workers, retaining the original delay.
Run it after builds finish:

```sh
cd app/build
CADENCE_TEST_TMPDIR=/tmp CADENCE_BENCH_CALLBACKS=6000 ctest --output-on-failure
```

Logic's copied `Cadence-Logic-Reliability.logicx` saves/reopens a stopped -12 dB Gain
edit. A full MIDI-region run at 128-sample I/O reports zero dropouts on both workers
and the rack. Original 256 I/O is restored and verified. The new 16-second offline
bounce has all eight onsets at exact sample positions; comparison to the previous
corrected reference measures -12.00000022 dB with maximum scaled-waveform error
3.33e-9. Artifacts and the full report are in
`app/build/verification/reliability-2026-11-06/`.

An earlier ordinary-thread extended run recorded four underruns with up to 16 ms
host timer lateness and is preserved in the report. The historical worker exit's
exact cause remains untraced; finite passing tests cover the reproduced defects,
not multi-hour or arbitrary-load guarantees. Host Audio Workgroup integration was
implemented later; see the latest workgroup section below. Both original Logic
project/audio and the previously installed AU are backed up under
`app/build/verification/`.

## Asynchronous loading

`PluginLoader` belongs to the processor rather than its editor. Protocol, CLAP and
Kontakt creation, preparation and saved-state loading run on its background thread.
Only prepared results attach to the graph on the message thread, with pending slots
preserving rack order. Audio-setting changes cause stale results to reload against
the current configuration. Removing a slot, cancelling, replacing the project or
destroying the processor invalidates pending results; starting Rust workers are
killed and reaped promptly. Cancellable bridge callbacks are used only during their
synchronous calls and are never retained by Rust or called from an audio callback.

Native AU/VST3 use JUCE's asynchronous factory and retain its format manager until
completion. Third-party native initialization/preparation may still require the
message thread; cancelling rejects its result, rather than interrupting arbitrary
in-process plugin code. Kontakt now prepares resident attacks outside the UI
thread and streams later pages on an owned disk thread; see the section below.

Tests use real CLAP workers with deliberately slow/hung initialization. They check
UI dispatch and existing audio during loading, cancellation/reaping, configuration
changes, pending saves/recall without an editor, stale completion rejection, failed
slots, bounded shutdown and native VST3 callback lifetime. The current loading UI
snapshot and logs are under `app/build/verification/async-loading-2026-11-06/`.

```sh
CADENCE_TEST_TMPDIR=/tmp CADENCE_WORKER_LOG_DIR=/tmp/cadence-test-diagnostics \
  app/build/CadenceScanTests_artefacts/Debug/CadenceScanTests --loading-only
```

The installed asynchronous-loading AU was also verified in Logic 12.3.1:
its saved two-slot project recalled -12 dB Gain, playback reported zero dropouts
on both workers and the rack, and the final Offline bounce contains all eight
notes at exact expected onset frames. Playback is stopped and the copied test
project is saved. The raw waveform differs from the earlier reference by a stable
oscillator phase shift: Sine Demo retains phase on NoteOn. Phase analysis confirms
matching timing/gain, with sustained-note residual RMS around 1e-7; both the raw
comparison and reproducible analysis are retained in the evidence directory.


## Kontakt sample streaming

The worker streams WAV/NCW pages, retaining zone attacks and loop boundaries in
RAM. Its decoded page cache is fixed at 16 MiB for 64 voices, with a separate
128 MiB resident preload budget. Over-budget libraries fail loading explicitly.
Realtime callbacks perform no file access, wait, allocation or deallocation.
Late source pages increment the existing Dropouts counter; offline rendering
waits for pages and reports disk errors/timeouts as failed renders.

Piano Uno verification reduced decoded sample/cache memory from 312.5 MiB to
36.9 MiB (88.18%) with exact full-RAM sample parity and zero streaming underruns
in paced playback. All 93 Rust tests, Clippy, AU/VST3 integration and 18000 paced
64/128/256-frame rack callbacks pass; installed AU validation/signatures pass.
The streaming AU is installed with the previous bundle backed up. Existing Logic
instances were not reopened during this change; reload Cadence to use the build.

See [design, tests and limits](../docs/sample-streaming.md),
[Kontakt static evidence](../docs/kontakt-streaming-evidence.md), and
[verification report](build/verification/sample-streaming-2026-11-06/report.json).
Finite local tests do not establish arbitrary-library or multi-hour reliability.


## Streaming inspector diagnostics

The Kontakt inspector now shows active voices, resident memory, allocated/ready
cache bytes and peak readiness, pending reads, successful pages and errors, page
delivery mean/last/peak time, and streaming miss blocks/voice frames. Host
pipeline dropouts remain separate in the inspector and rack footer. Page timing
includes file open/read/decode/copy; it is not physical disk utilization.

The design uses recovered Kontakt memory/voice/request/overload counter patterns
as a reference; Cadence measurements and counter lifetime are defined and tested
independently. Counters/peaks last until reload, including offline renders; voice
reset does not clear them. UI refresh is 5 Hz using bounded coherent atomic
snapshots, retaining its last good value when publication is busy.

All 105 Rust tests, workspace Clippy, AU/VST3 integration, 18000 paced rack
callbacks and installed AU validation pass. The real Piano Uno C bridge and
inspector rendering are checked; the rendered preview was visually inspected.
See [metric definitions and transport](../docs/streaming-telemetry.md) and
[verification report](build/verification/streaming-telemetry-2026-11-06/report.json).

**Restart Logic and the standalone** after this install: the private host/helper
protocol was version 5 for this report (current protocol is 11 after workgroup,
timing/parameter, MIDI output, structural reload and GUI fields were added). The public C ABI remains 2 with an additive stats API.
The previous AU is backed up as `Cadence-before-streaming-telemetry.component`
under build/verification. Large-library Logic tests remain deferred as requested;
no Logic project was changed during this diagnostics implementation.

## Audio Workgroups and native CLAP adapter (2026-11-06)

The AU now forwards the host Audio Workgroup to sandboxed workers. A retained
`os_workgroup_t` is coalesced on the audio thread, copied to a Mach send right on
the message thread, published under a generation-suffixed bootstrap service, and
reconstructed and joined by the worker rendering thread. Clearing the host
workgroup makes workers leave. `CadencePipelineStats::scheduling_flags` bit 1
reports membership. Details and limits: [docs/audio-workgroups.md](../docs/audio-workgroups.md).

CLAP modules enumerate all selectable plugins; the packaged scanner emits one
entry per variant with a stable selector. The CLAP host implements log,
thread-check, latency, tail, params, state, audio-ports and note-ports extensions.
Dynamic changes report reload-required instead of being rebuilt live. Native CLAP
editors remain deferred because the plugin's macOS view belongs to the worker
process. Details: [docs/native-clap-adapter.md](../docs/native-clap-adapter.md).

Verified: 120 Rust tests, Clippy, all builds, CTest 64.12s, 18000 paced callbacks
with zero dropouts, cross-process workgroup join/leave using the default output
device, multi-variant CLAP enumeration/selection, installed AU signatures and
`auval`. Restart Logic and the standalone after installing any build; the current
private protocol is 8. Report:
`build/verification/workgroup-native-adapter-2026-11-06/report.json`.

## Live CLAP updates, parameter feedback and sidechain (2026-11-06)

CLAP latency and tail are published after processing and applied on the message
thread so the DAW receives live PDC/tail updates. Parameter rescans snapshot
normalized values through a seqlock; the host adopts changed values without
echoing them back. Parameter/port structure changes still require reload.

The rack now exposes a stereo sidechain input and routes it to bridged plugins
that declare sidechain ports. The CLAP adapter classifies non-main input ports;
`cadence_bridge_process_sidechain` carries the audio; the graph input node feeds
each active bridged slot. The sidechain bus is inactive by default until the host
routes it. See [sidechain routing](../docs/sidechain-routing.md).

The standalone wrapper now forwards the audio-device workgroup. VST3 still has no
host workgroup handshake. Verified with the CLAP fixture: live latency change,
parameter feedback, sidechain scaling/clearing, 120 Rust tests, full CTest and
the installed AU validation. Report:
`build/verification/live-clap-sidechain-2026-11-06/report.json`.
