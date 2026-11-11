# Sample streaming

Cadence's Kontakt compatibility sampler streams WAV and NCW samples in the
production worker and the `sample-host` CLI. This is independent Rust code guided
by [saved Kontakt static evidence](kontakt-streaming-evidence.md), not execution
of Kontakt's recovered pseudocode.

## Loading and memory

`SamplerPlugin::stream_all(StreamingConfig)` resolves each referenced sample to a
`SampleLocation` (file path, offset, length), opens its metadata, and decodes only
resident pages. Standard NI instrument metadata is capped at 64 MiB; NI file
containers and NKR resources are indexed with seeks so sample payloads remain on
disk. Existing `sample()`/`decoded_sample()` and `preload_all()` remain explicit
whole-sample reference APIs. No unsupported format silently falls back to full
preloading in production.

Pages contain 8192 stereo frames (64 KiB). Mono sources are duplicated; playback
uses the first two source channels, as the existing stereo sampler did.
`StreamingConfig` defaults to 16384 attack frames and 128 MiB of resident decoded
pages. Attack windows include the partial start page plus the requested window;
nonzero zone starts, reverse starts, and both loop boundaries are covered. Pages
are deduplicated by sample/frame range. Exceeding the resident budget fails
loading with an explicit message; the implementation does not silently shorten
attacks or allocate beyond that budget.

Each of 64 voices owns four reusable stream buffers: **16 MiB total**. Storage is
allocated and touched before publication. Cache capacity cannot grow during
playback. Decoder scratch/output are transient disk-thread allocations and are
bounded separately: normal cache reads request at most 8192 frames, and the public
range API rejects requests above 1048576 frames. Metadata, filenames, thread
stacks, and OS filesystem cache are outside the decoded-audio budget. This is a
bound on sample/cache storage, not a process-RSS promise.

`SampleReader` handles PCM 8/16/24/32 and float 32/64 WAV (including extensible
subformats) and NCW's 512-frame blocks. NCW offset pairs are read on demand;
there is no full decoded sample or full block table per reader. Readers retain
metadata between reads but no open file handle. Each page opens/closes its file,
so large sample counts cannot exhaust a per-sample handle pool.

## Playback ownership

Realtime processing owns voices, immutable resident pages and the request side
of fixed per-voice slots. Requests carry a sample ID and aligned page start.
The disk thread acquires REQUESTED, sets FILLING, reads/decodes, then releases
READY or FAILED. Audio acquires READY and copies frame values. The worker never
modifies READY storage; audio never retires FILLING storage. Requests can be
coalesced/cancelled before filling. Voice stealing/reset cannot publish a stale
page as another sample. Completed stale pages can be reclaimed mid-page.

Forward and reverse lookahead request up to four source pages. Loop boundaries
are resident; loop continuation uses the same lookahead. Ping-pong endpoint
folding keeps positions within valid sample frames, including large pitch steps.
Positions use f64 so long files retain fractional source positions. Note events
remain sample-accurate within each host block.

The disk worker polls idle requests at 1 ms intervals, with round-robin service
across voices. Audio request publication uses atomics only: no file operations,
allocation/deallocation, mutex, wake syscall or wait. This is an engineering
choice, not a recovered Kontakt timing guarantee. High pitch ratios, many
simultaneous voices, or a slow/saturated disk can exceed lookahead.

A late realtime frame decays the last valid frame and fades back in over 64
frames when data resumes. It advances the voice timeline and increments
`StreamingStats::underrun_frames` (output voice frames with either interpolation
tap missing). The inspector reports streaming misses separately from host pipeline
dropouts. The rack footer reports host dropouts only. Sampler counters remain
cumulative until reload, including offline renders. Late pages alone do not kill
it. See [streaming diagnostics](streaming-telemetry.md) for units and sampling.

## Offline rendering and failures

`Plugin::set_offline_rendering` is a Rust host hook with no native/wire ABI layout
change. The shipping worker uses false for pipelined realtime packets and true
for synchronous offline blocks. Both reference `render_note` helpers select
true during their render and restore false afterward. Direct sampler callers
must select offline mode themselves if they render faster than wall-clock time.
Native protocol plugins retain their existing vtable behavior.

Offline reads may wait for the owned disk thread, with a 2 s page deadline.
Timeout is latched for that offline run; a failed disk read or timeout is exposed
through `processing_error()`, propagated by the shipping worker, and checked by
`sample-host` before writing output. The sandbox's own command/process deadline
also applies and can end a render first. The reference helpers return audio by
their existing signature; direct users must inspect their plugin's error.

Shutdown sets stop, wakes an idle worker and joins it off the audio thread.
Artificial delay waits check stop every 1 ms. An OS file operation can still block
until the kernel returns; cancellation of a sandboxed load kills/reaps its worker
process under the existing loading protocol. In-process destruction cannot
interrupt arbitrary kernel filesystem stalls.

## Verification

- Synthetic WAV streamed/full-RAM sample identity with pitch, nonzero starts,
  reverse, forward/ping-pong loops, release, reset and 80-note voice stealing.
- Analytical ping-pong sequence at the last sample and pitch steps larger than
  the loop, independently of resident/streamed parity.
- Realtime callback allocation/deallocation guard during note-on, voice stealing,
  nonresident playback with a 300 ms disk delay, and reset. The callback finishes
  while disk is stalled; finite output, underrun counters and bounded storage
  are checked. Artificially stalled worker shutdown cancels promptly.
- Resident budget rejection and truncated-file read errors are explicit.
- Range/full decode parity for WAV/NCW, malformed offsets, real Piano Uno NCW/NKR,
  nested/synthetic resources, 1024 readers without retained handles, and a sparse
  512 MiB monolith with seek-only sample indexing.
- Installed Piano Uno: 111 samples, 327657600 full decoded bytes versus 21954560
  resident + 16777216 cache bytes (88.18% lower decoded sample/cache storage).
  A pitched chord/release render matches full RAM exactly. 600 paced 256-frame
  realtime callbacks have zero streaming underruns. A separate shipping-worker
  render also matches its full-RAM reference exactly.

Run from `platform/`:

```sh
cargo test -p nk-format -p sample-adapter -p plugin-sandbox --offline
cargo run -p sample-adapter --bin sample-host -- /path/instrument.nki /tmp/stream.wav
cargo run -p sample-adapter --bin sample-host -- /path/instrument.nki /tmp/ram.wav --resident
```

Real-library tests are optional when Piano Uno is absent. Local verification
logs/report record whether they actually ran. Crossfades, advanced Kontakt
scripting/group semantics, encrypted content and multi-program expansion remain
outside this sampler's established compatibility. Finite tests do not establish
arbitrary-load, multi-hour DAW or Kontakt runtime equivalence.
