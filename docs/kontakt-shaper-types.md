# Kontakt shaper modes — 2026-11-08

Cadence replaces the group-only tanh approximation with Classic (ID 0), Drums
(ID 1) and Enhanced (ID 2). All three run in ordered per-voice group slots,
program inserts, parallel aux returns and master slots. This is a scalar
reconstruction from saved static evidence; native Kontakt/Logic audio parity
has not been measured.

## Evidence and parameter handling

Source artifact: `Kontakt-8-REA/Kontakt-8.arm64`, SHA-256
`42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.
Saved Ghidra 12.1.4 exports under `Kontakt-8-REA/pseudocode/functions/` retain
their provider limitations. No native session or runtime capture was opened.

| Evidence address | Recovered behavior |
| --- | --- |
| `0x100780ec8`, `0x100780f44` | Read/write v0x80: float shape, signed mode ID |
| `0x100189544` | UI names Classic=0, Enhanced=2, Drums=1 |
| `0x10040cefc` case 7 | Clamp shape to [-1,1], map table position, bypass gates, reset on mode change |
| `0x1005c0940` | Classic branches, mode dispatch, common output path |
| `0x1005c1120` | Enhanced interpolated drive/mix/correction and polynomial |
| `0x1005c13ec` | Drums linked peak/envelope, bilinear curves, smoothed gain |
| `0x10060114c`, `0x100601338`, `0x10060126c` | Coefficients, unity-seeded reset, coefficient copy |
| `0x1005bc630`, `0x1005bce24`, `0x1005bd784`, `0x1005c2b24` | Base wet/output gain and separately prepared dry cache |

The parser requires exactly eight payload bytes, finite shape and version
0x80. It clamps finite shape controls like SetObj. IDs other than 1/2 use the
native Classic fallback; they are not inferred to be additional modes.
Malformed active records and unsupported shaper versions fail preparation.
Bypassed slots retain the existing skip policy. Older XML/pre-v2 readers are
not added here.

The normal loaded shaper path supplies only `out_level` and never prepares the
base dry cache. Even neutral shape receives output gain, and serialized
`out_level_dry` is ignored. This differs from IRC. The conclusion does not cover
arbitrary external mutations of native runtime objects.

## DSP behavior

Classic preserves the native half-shape float arithmetic and piecewise cubic,
partially blended soft clipping and driven soft clipping branches. Negative
shape is active and does not clip its input. The two static neutral gates are
preserved: `abs(shape) <= .0001`, plus `3.999 < (shape*.5+.5)*8 < 4.0001`.
The negative cubic branch has a discontinuity at that neutral region in the
saved scalar source; no extra continuity correction is introduced.

Enhanced interpolates drive amount, drive mix and gain correction, clamps the
driven sample to [-8,8], then blends the corrected polynomial with the input.
Drums uses the greater stereo magnitude as a linked detector with instantaneous
attack and recovered release. Its bounded envelope indexes a 51-by-128 curve
surface, interpolated in shape and envelope. A separately smoothed gain and
table correction multiply both channels. Histories start at unity on Reset.

Both table modes bypass at `shape <= -.9999` and freeze audio/control state
while internally bypassed. They smooth table position once per stereo frame,
using the recovered 50 ms coefficient and .0001 increment snap threshold.
Gain/release times are the recovered float values 2.67874/25.4764 ms, using
`1 - exp2(-6.643856 / max(1, rate*.001*timeMs - 1))`. Native denormal offsets
in envelope/gain updates are retained. Cadence cold initialization starts at the loaded
shape target; same-mode shape changes slew from the current position.

All five exact tables are embedded as a 28,160-byte little-endian asset,
`crates/sample-adapter/src/shaper_tables.bin`, with SHA-256
`b506b4e021c2605aff586337f2f94b9ee826cc01759b9ceaf8dba642b3d15cd0`.
They were extracted from file offsets `[0x9933300,0x993a100)`, corresponding
to virtual addresses `[0x109933300,0x10993a100)`. Table offsets/dimensions and
individual hashes are retained in verification provenance. Native upper
interpolation reads can cross a table/row at zero weight; upper neighbors are
clamped safely, preserving these finite endpoint results.

Each voice has independent fixed-size state at each original group slot;
program slots have independent state. Audio callbacks and reset require no
heap allocation or deallocation. Histories survive host, event and aux splits.
Cadence rebuilds coefficients and resets histories on rate changes; exact native
host lifecycle ordering remains unverified.

## Verification and limits

Seven new regressions cover strict parsing, native fallback IDs, bipolar
branch/neutral boundaries, linked stereo detection, reset/rate/model/bypass
behavior, independent slot/voice state, program insert/send/master routing,
gain semantics and streamed/resident callback partitioning. Existing nonlinear
routing expectations now evaluate Classic's recovered transfer.

An independently written Python oracle imports no Cadence DSP. Its exact table
extraction, explicit scalar float arithmetic and saved code produce **20,352
stereo reference frames**: 8,832 Classic, 5,760 Drums and 5,760 Enhanced, at
44.1/48/96 kHz. Cases include high amplitudes, silence, gate/crossover boundaries,
shape changes and bypass periods. Measured maximum error is zero for each mode
in the debug verification run. The enforced bound is
`0.00002 * (1 + abs(reference))`. This validates the scalar reconstruction,
not compiler FMA/SIMD equivalence or a native-render comparison.

All modes have exact stereo playback across 1/64/257/1024-frame callbacks,
resident/streamed storage and reset, with zero streaming underruns and zero
callback/reset allocations or deallocations. Piano Uno remains a real-content
regression; synthetic references establish the added mode coverage.
Full format/sampler suites pass **207 tests**. Workspace Clippy with warnings
denied, formatting, standalone/AU/VST3/test builds, bundle signatures and
normalized bundled-worker payload checks pass. The real Piano streamed/RAM
comparison is exact, with zero underruns. A finite optimized 600-callback run
measured a maximum of 848 us against the 5,333 us block budget; this is not a
hard realtime or long-duration guarantee. A ten-second regression render is
retained. CTest initially hit managed-sandbox shared-memory restrictions, then
passed in the user's normal Terminal: one integration suite, zero failed/skipped,
20.26 seconds. The tested release worker hash matches this increment. The suite
includes sandboxed Piano playback and nested AU/VST3 rendering; this is separate
from a native Kontakt DSP comparison. Full output and review are retained in
`app/build/verification/packaged-20261108T005357Z-41327/`.

Build/check/render results and source/artifact hashes are retained in
`app/build/verification/shaper-types-2026-11-08/`. Installed plugins are unchanged.

Remaining shaper work: shape modulation buffers and their 32-frame native
control mapping, live FX editing/automation and coefficient-copy lifecycle,
older serialized/XML versions, exact common-volume smoothing and independent
Kontakt audio comparisons. Group insert placement around the amplitude stage
remains the existing compatibility limit. Next current-effect fidelity work
is SSLGEQ controls/units, followed by spread modes and limiter behavior.
