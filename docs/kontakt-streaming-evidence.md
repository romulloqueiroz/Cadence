# Kontakt streaming reference

Read on 2026-11-06 to guide Cadence's independent Rust sample-streaming implementation.

## Identity and scope

The reference is the saved REA/Ghidra pseudocode export of **Kontakt 8.14.1's ARM64 main executable**, under `../../Kontakt-8-REA/`. The ARM64 artifact SHA-256 was rechecked during this reading: `42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`. See the archive's [README](../../Kontakt-8-REA/README.md) and [combined catalog](../../Kontakt-8-REA/combined-functions.tsv).

The files report Ghidra 12.1.4, analysis profile `040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`. This pass read existing artifacts; it did not run Kontakt, open a new REA session, reimport a binary, or establish new runtime observations. The addresses below identify saved static observations, not newly issued REA Evidence IDs. No retained Evidence ID was reconstructed or invented.

## Static findings

| Saved function | Observed pseudocode | Implication for Cadence |
| --- | --- | --- |
| [`BStreamingEngine::ScheduleDFDSegment`, `0x1003f9c1c`](../../Kontakt-8-REA/pseudocode/functions/0x1003f9c1c.c) | A segment's scheduling count increments; the previously zero case links it into a global scheduled list and can signal a dispatch semaphore. An explicit flag calls `CompleteDFDPreload` for the owning sample. | Keep repeated page requests bounded and coalesced. Separate asynchronous playback requests from an explicit preload-completion path. |
| [`BStreamingEngine::CompleteDFDPreload`, `0x1003f9d24`](../../Kontakt-8-REA/pseudocode/functions/0x1003f9d24.c) | Acquires a recursive request mutex, transfers scheduled segments into a request list, sorts it, and processes all requests or requests associated with a selected sample. | Loading must establish the initial playable region before publishing an instrument. A blocking completion operation belongs outside realtime playback. |
| [`BStreamingEngine::ProcessDFDRequest`, `0x1003fa5f0`](../../Kontakt-8-REA/pseudocode/functions/0x1003fa5f0.c) | A segment with no positive use count clears block pointers and unmaps its storage. A positively referenced unmapped segment obtains a block byte range through `GetSizeBlocks` and `MapToMemory`, publishes block addresses, and tracks current/peak mapped bytes. Mapping failure sets `s_bGetSampleFrameError`. | Stream ranges rather than entire samples; keep ownership explicit while audio can read a page; expose memory and failure counters. Cadence can use decoded fixed-size buffers rather than copying Kontakt's mapped-file layout. |
| [`DFDRequestCompare`, `0x1003fa8dc`](../../Kontakt-8-REA/pseudocode/functions/0x1003fa8dc.c) | Compares the sum of two double fields per request (`+0x28` and `+0x30`). `ProcessDFDRequest` also uses these fields with a clock reading. | The fields are consistent with deadline-oriented scheduling. Their exact units and population sites were not established here; do not claim a recovered scheduling specification. |
| [`BStreamingEngine::run`, `0x1003fab4c`](../../Kontakt-8-REA/pseudocode/functions/0x1003fab4c.c) | A dedicated loop waits on a dispatch semaphore, checks a finish flag, transfers scheduled segments, sorts requests, and invokes `ProcessDFDRequest`. It uses a timed wait while a usage metric remains active. | Perform disk work on an owned background thread with explicit shutdown. Do not perform file access in the audio callback. |
| [`BSample::CalculateDFDSegments`, `0x10043e39c`](../../Kontakt-8-REA/pseudocode/functions/0x10043e39c.c) | Starts from a preload interval, extends segments forward and backward with `GetNumBlocksFitting`, uses shifts by nine in frame/block calculations, allocates segment descriptors, and sorts them by position. | Include nonzero sample starts and backward/looped access in tests. The observed block calculations use 512-frame units, but Cadence's cache page size is its own design choice. |
| [`BSample::MapPreloadAreaBlocks`, `0x1004410c8`](../../Kontakt-8-REA/pseudocode/functions/0x1004410c8.c) | Fills the sample's block-pointer table from an already allocated preload area, with format-dependent offsets and a 512-frame block progression. | Keep an initial decoded region resident so note attacks do not depend on disk scheduling. |
| [`BSampleSettings::CalculateDFDInfo`, `0x10045dca8`](../../Kontakt-8-REA/pseudocode/functions/0x10045dca8.c) | References an instrument preload setting or global override, zone/sample offsets, and a reverse flag; rounds a positive calculated byte size to a multiple of `0x1800`. | Preload policy must account for zone start and playback direction. The exact original setting units and all field meanings remain unverified. |

## Design boundary

These observations support the broad architecture of resident attacks, requested segments, a disk worker, and explicit memory/error accounting. Cadence's Rust implementation, cache limits, synchronization, lookahead, underrun behavior, and offline-rendering policy are independent engineering choices. They must be established by Cadence's own tests. The pseudocode does not demonstrate that simply calling a recovered routine would make our adapter stream.

## Limitations and open questions

- This is static decompiler output, not original source or a buildable Kontakt engine. Provider warnings include removed unreachable blocks, uncertain parameter recovery, and a terminal-flow warning in `ScheduleDFDSegment`.
- The exported acquire/release and tagged-pointer operations do not establish a complete concurrency proof. We did not recover Kontakt's complete ownership protocol or copy its data structures.
- The exact preload defaults, request-field units, eviction budget, offline-render behavior, and recovery after a disk stall remain unknown from this focused reading.
- Mapping ranges alone does not prove that all eventual sample-memory accesses avoid page faults. Cadence must explicitly read/decode pages on its disk thread and test its own callback behavior.
- The reviewed boundary is eight named functions. Native Intel code, other bundled components, broader call-site coverage, and runtime parity are outside this pass.

## Artifact hashes

SHA-256 of each reviewed saved pseudocode file (path prefix `../../Kontakt-8-REA/pseudocode/functions/`):

```text
0x1003f9c1c.c  91e9a6448154c7d94bafe9c5d29f8a09f0d8f9e6aedd95855bf15503730c2b5d
0x1003f9d24.c  9e7e202d320e90ff10854e25b284dcf220330751e9783c60c321dd224c07e4ce
0x1003fa5f0.c  0a38bcda0e57f14ce5ddbf187dfcdb0d9ebf7149d7f5a42fb9f2f222c90fde96
0x1003fa8dc.c  0078f1dc2722f0dd7b640f576c61b3304a9cb7036d3edc5025b772844fb525cb
0x1003fab4c.c  67f7c13723cff084b5fdf485079e0980cd553f26f8be74769d660e61e418deb7
0x10043e39c.c  6fbbb3c13842df59cb4e93ac727f725a09c5de9519f197b14c2676413aa2fd56
0x1004410c8.c  be5510c24d1d20ac59d52279b596505f80974c29a56cfe1d1be8aaa99206f21c
0x10045dca8.c  f5367396f6b84ec0677535e5fb667009b28ed35a049d56e89f29bf035f7fafae
```

## Diagnostics follow-up (2026-11-06)

This focused follow-up uses the same saved artifact and profile. It adds static references for Cadence's streaming inspector. It does not establish Kontakt runtime behavior, the original UI's refresh interval, or equivalence between Kontakt's metrics and Cadence's telemetry.

| Metric / saved reference | Static observation | Cadence interpretation |
| --- | --- | --- |
| [Mapped-byte peak, `BStreamingEngine::GetDFDPeakNumBytesMapped`, `0x1003fa8c8`](../../Kontakt-8-REA/pseudocode/functions/0x1003fa8c8.c) | Returns the saved `dfdCurNumBytesMappedPeak`; a true argument clears that peak to zero after capturing the return value. The previously reviewed `ProcessDFDRequest` adds the mapped range's returned byte size on a successful map, raises the peak, and subtracts/clamps current bytes to zero on unmap. | Current bytes, allocated capacity, and peak bytes are different measurements. A peak-reset operation should have explicit semantics; a display poll should not silently destroy a counter shared with another reader. The saved getter itself performs a destructive reset when requested. |
| [Rack sample memory, `BEngine::getRackMemoryUsage`, `0x1004a03f4`](../../Kontakt-8-REA/pseudocode/functions/0x1004a03f4.c), and [`recalculateMemoryUsage`, `0x10049ff08`](../../Kontakt-8-REA/pseudocode/functions/0x10049ff08.c) | The getter recalculates when needed and reads a rack field. Recalculation traverses slot/program sample-pool references, uses per-item flags and a unique-item hash table, and sums sample-pool-item size fields. [`Engine::getMemoryUsageBytes`, `0x100fae2f8`](../../Kontakt-8-REA/pseudocode/functions/0x100fae2f8.c) delegates to it. | This is application sample-memory accounting, not a process-RSS query. Cadence should label its resident decoded sample bytes and fixed page-cache allocation separately. Metadata, decoder scratch, other plugins, OS cache, and process overhead are outside those two counters. |
| [Disk load getter, `BEngine::IO_GetDiskLoad`, `0x1003fb75c`](../../Kontakt-8-REA/pseudocode/functions/0x1003fb75c.c), and the previously reviewed `ProcessDFDRequest` / `run` | The getter returns `curDFDUsage` as a float. Following a successful mapping, request processing uses a clock value and request double fields at `+0x28` / `+0x30` to compute an elapsed/allowance ratio when the allowance is positive and the deadline has not passed; otherwise the candidate is `1.0`. It raises the global value if the candidate is greater. The worker loop multiplies that value by `0.98` per iteration/wake. | This is consistent with a decaying deadline-pressure indicator, not raw physical-disk busy percentage or a directly measured read duration. Cadence's page-read timing should be independently defined and labeled, e.g. end-to-end page read/decode time, including opening/seeking/decoding. Do not present that duration as recovered Kontakt disk-load semantics. |
| [Disk error event, `BEngine::DFDError`, `0x1003f9bf4`](../../Kontakt-8-REA/pseudocode/functions/0x1003f9bf4.c), and [overload getter, `IO_GetDiskOverload`, `0x10049fc60`](../../Kontakt-8-REA/pseudocode/functions/0x10049fc60.c) | The error routine sets an engine flag at `+0x108a8`, increments a stored integer at `+0x11ed8`, and writes `0x101` to a voice field. The overload getter returns the flag and clears it. The request-processing path separately sets `s_bGetSampleFrameError` when mapping fails. | Disk pressure, a latched overload event, cumulative error counts, and unavailable sample data have distinct meanings. Keep Cadence's missing-page voice-frame counter separate from host/worker pipeline callback dropouts and from terminal file/decode failures. The focused reference does not establish an exact one-to-one mapping between the original error increment and missed audio frames. |
| [Voice count, `BEngine::IO_GetNumVoicesPlaying`, `0x1003ed7b8`](../../Kontakt-8-REA/pseudocode/functions/0x1003ed7b8.c) | A negative first selector reads a global stored count at `+0x28390`; other branches read counts indexed by slot (0–63) or by a wider selector (0–8191), with range checks. The named API delegates through [`ApplicationPerformanceMetrics::getNumVoicesPlaying`, `0x100d8b098`](../../Kontakt-8-REA/pseudocode/functions/0x100d8b098.c) and [`Engine::getNumVoicesPlaying`, `0x100fae2e8`](../../Kontakt-8-REA/pseudocode/functions/0x100fae2e8.c) using the negative global selector. | Current active voices is distinct from note-on count, held keys, or total requests. Cadence should define this as occupied live voice slots, including release voices until completion, and report the pool limit alongside it. The original count's writers and release-tail semantics were not recovered in this follow-up. |
| [UI polling, `PerformanceMetricsModel::poll`, `0x100d06a78`](../../Kontakt-8-REA/pseudocode/functions/0x100d06a78.c), and [`fetchMemoryUsage`, `0x100d06b98`](../../Kontakt-8-REA/pseudocode/functions/0x100d06b98.c) | Poll reads the voice metric on each invocation, then uses a countdown to sample other metrics less often. It multiplies the returned disk-load float by 100 before storing an integer, queries disk-overload separately, and fetches memory for file-size formatting. It emits change notifications only when stored values differ. | Format memory/timing and update UI outside the audio callback. Snapshot counters without waiting on disk work. A displayed percent is only meaningful when its underlying denominator is defined; Cadence's millisecond timing should remain a duration. No exact UI refresh rate is established by these functions. |

The previously reviewed `ProcessDFDRequest` increments `dfdNumRequestsProcessed` before attempting a mapping. Consequently, that stored counter is an attempted-request count, not proof of successful sample delivery. Cadence may instead count successful decoded pages and file/decode errors separately, provided their labels define those choices.

### Diagnostics limits and definitions

- The disk-pressure formula's request-field population sites and clock-unit contract remain untraced. The decompiler displays a clock conversion, but a precise original timing API must not be inferred solely from it.
- Mapped bytes refer to Kontakt's mapping representation. They do not establish physical residency or equal Cadence's decoded stereo buffers.
- The exposed rack-memory getter may allocate during recalculation (the saved pseudocode constructs a unique-item hash table). That is not a pattern to copy into a realtime telemetry read.
- A distinct [`BDsp::DFDError` export at `0x1004e1dd8`](../../Kontakt-8-REA/pseudocode/functions/0x1004e1dd8.c) is an empty body in this artifact. The concrete `BEngine::DFDError` above is not empty. This pass does not establish all dispatch paths or infer that Kontakt ignores disk errors.
- Cadence streaming underruns should be described as output frames per voice with unavailable sample data, including a missing interpolation tap; resampling means this is not a count of distinct source frames. Keep audio-clock callback dropouts as a separate metric.
- Counter widths, saturation/reset policy, cross-thread snapshot consistency, and UI sampling cadence are Cadence implementation decisions to validate independently. No runtime reads or mutations of Kontakt counters were performed.

### Additional artifact hashes

SHA-256 of the additionally reviewed saved pseudocode files, with the same path prefix as above:

```text
0x1003fa8c8.c  2354865f538e2d360f313f855c5e0d64cac2845057026814ae120e020c66393b
0x1003f9bf4.c  a23ff062c366beaf4f5be4415724e7b0c4d4c3b9e72d2173a0c7bdfdbc745f9a
0x1003ed7b8.c  242d20f1c625a46b3277f9bc14f054a599cb696c3eb4335c9d307abbabbca00f
0x1003fb75c.c  7d9a44c7bed0e28904ee2c796d10c36558869a358688e727b90b0db969d3fc23
0x10049fc60.c  38156cf4196f108f7fe2fa357fa6c1803e57e4ac2b3108f4ff76f8dfeb409b5f
0x10049ff08.c  6cbd5d9e2607560197e68e59a6b521b90f3087691a50b7146b42c4de44eacdbe
0x1004a03f4.c  20e4f918b24288edbd8b093fec869cb067eda1654d93ba57d3ef9a489c9d841a
0x100d06a78.c  04a6de4c992dd0e24e9dd64ea7acc2fbc88ff5483caa374e51a29b1c94bf3b21
0x100d06b98.c  7a65ca304df487c0b1fb727aa66c81dc0efa34bac864b4e00ce9bbce50fc2a31
0x100fae2f8.c  646944738f3a68b3e268095a2f10232f2af8ed60a93a3a0675eb9fd8d44feae0
0x100d8b098.c  d6c29cb48033a1005255bbe80c3f53e73cfa5d6e41dbc99c2c14581384a6b091
0x100fae2e8.c  cdb1b3cdff3420ff433e85d3cdc63519a851d76130ffe3cce1e0669d29720056
0x1004e1dd8.c  b631fdf33a43a85d78df4417d47cc3f9a6c647f1def91c88c77cd134cf447ee1
```
