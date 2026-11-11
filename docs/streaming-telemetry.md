# Streaming diagnostics

The Kontakt instrument inspector refreshes at 5 Hz with a bounded atomic
snapshot from the worker. No polling RPC, file access, wait, lock, or allocation
is added to audio processing. Other plugin types keep their existing inspector
without sample-streaming figures.

| Display | Meaning |
| --- | --- |
| Voices | Currently active sampler voices / fixed voice capacity, including release voices until they finish. |
| Read queue | Requested or filling pages at the last sampled block; it can change while sampled. |
| Resident | Decoded zone attacks and loop-boundary pages retained until reload. |
| Cache | Allocated fixed page storage, whether ready or empty; this is additional to resident bytes. |
| Ready / peak ready | Decoded bytes in reusable ready slots and lifetime peak. These pages occupy the allocated cache, not additional memory. |
| Pages read | Successful background page deliveries; resident preload is excluded. |
| Read errors | Failed page reads plus offline page timeouts. |
| Page time mean / last / peak | Milliseconds from the disk worker beginning an attempt through file open/read/decode/copy completion (or error). Excludes time waiting in the request queue; includes injected stall time in tests. This is not physical disk latency or disk utilization percent. |
| Streaming misses | Blocks containing missing data and output voice frames with either interpolation tap missing. Frames sum across voices; resampling does not make this a count of source-file frames. |
| Host dropouts | Worker pipeline delivery failures; separate from the sampler's streaming misses. The rack footer also reports this pipeline-only count. |

Sample/cache bytes exclude instrument metadata, decoder scratch, thread/runtime
memory and OS filesystem cache. They are not process RSS. Counters and peaks
cover the loaded worker's lifetime and include offline processing; note resets
clear voices but do not clear these counters. Reload the instrument to begin a
new measurement. Host pipeline counters retain their existing prepare/reset
scope, independently of sampler counters.

The worker publishes after startup, completed audio blocks and successful
control operations. A completely idle worker retains its last publication;
background read completion alone does not trigger a new host snapshot. This
keeps one publisher and avoids a telemetry command competing with audio.

The disk thread brackets timing fields with an atomic generation. Readers make
at most three attempts and retain the last coherent completed timing group if
publication is busy. Read errors are sampled independently so terminal failures
remain visible even when a timing snapshot is skipped; the timing attempt count may briefly lag a
new error. Ready/pending page occupancy is sampled separately; peak ready bytes
are at least the sampled ready bytes and never decrease in this loaded sampler.
The worker then publishes all scalars in a second coherent shared-memory snapshot. UI readers also try at most
three times; the inspector retains its last good snapshot if publication is busy
or interrupted. Reads do not clear counters or peaks.

`Plugin::streaming_telemetry()` is a Rust owner-thread hook returning optional
scalars. Shared worker protocol version 5 added the atomic payload; version 6
added Audio Workgroup fields, version 8 added dynamic plugin timing plus the
seqlock parameter snapshot, version 9 added MIDI/SysEx output, version 10 added
the structural reload flag, and version 11 adds GUI state. Version 11 is the
current layout; older helpers are rejected. The public C bridge ABI stays 2 with an additive
`cadence_bridge_streaming_stats()` and a fixed 112-byte `CadenceStreamingStats`.
The C call returns zero for a valid query, with flags bit 0 when data is available;
unavailable/busy queries zero their output. Invalid pointers return -1. Existing
C structures are unchanged.

**Restart Logic and the standalone after installing this build** so the loaded
host and helper use the same private worker protocol. Saved projects and native
plugin state formats are unchanged.

## Kontakt reference

Saved static pseudocode documents mapped-memory current/peak counters, a
read-and-clear peak API, separate disk-overload latch/count, request-attempt
counts, voice counters and UI polling. These informed the separation and labels.
Kontakt's recovered disk-load getter represents a decaying deadline-pressure
metric; it does not establish measured milliseconds or physical-disk busy time.
Cadence's page timing and non-destructive lifetime counters are independent
design choices. Exact addresses, hashes, observations and unknowns are retained
in [the evidence ledger](kontakt-streaming-evidence.md#diagnostics-follow-up-2026-11-06).

## Verification

Tests cover real page-read timing with an injected delay, active voice/reset
semantics, stable allocated memory, ready/peak bounds, missing-data block/frame
counts, no callback allocation or deallocation, coherent concurrent snapshots,
maximum-width counters, generation wrapping, interrupted publishers, distinct
pipeline counters, worker-version rejection and the C layout/null contract.
Real Piano Uno exercises the shipping worker, C bridge, inspector text/bounds
and a rendered inspector snapshot. Large-library Logic stress remains deferred
until the user has an appropriate library.
