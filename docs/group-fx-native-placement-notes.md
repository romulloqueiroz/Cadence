# Native group FX amplifier placement

Recovered 2026-11-10 from the saved Kontakt 8 ARM64 analysis. These are static
observations of a shipped artifact, not a native playback comparison of the new
Cadence routing.

Source image: `Kontakt-8-REA/Kontakt-8.arm64`, SHA-256
`42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.
The saved Ghidra profile is
`040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`.
Source links below point to that existing decompilation. The native default and
serialization loads/stores were also checked in the original ARM64 instructions
with `llvm-objdump`; no new provider or live Kontakt session was opened.

## Serialized boundary and default

The boundary is a signed 32-bit integer in `BGroup + 0x8f0`.

| Evidence | Observed behavior |
| --- | --- |
| [BGroup::ResetParameters, 0x1003fd448](../../Kontakt-8-REA/pseudocode/functions/0x1003fd448.c#L46) | Initializes the boundary to **6**. Native instructions `0x1003fd4d0: mov w8,#6` and `0x1003fd4d4: str w8,[x19,#0x8f0]` corroborate the value. |
| [BGroup::PrsSetValues, 0x10040127c](../../Kontakt-8-REA/pseudocode/functions/0x10040127c.c#L516) | XML parameter **`fxIdxAmpSplitPoint`** is parsed with `atoi` and stored directly at `+0x8f0`. |
| [BGroup::doReadPubPars, 0x10078ba10](../../Kontakt-8-REA/pseudocode/functions/0x10078ba10.c#L55) | Reads `s32` after voice-group index (`+0x120`) and before muted/soloed. The shown branches cover public versions `0x90..0x96`; each branch reads this same field. Native stores are at `0x10078bae8`, `0x10078bbfc`, and `0x10078bd24`. |
| [BGroup::doWritePubPars, 0x1007b0954](../../Kontakt-8-REA/pseudocode/functions/0x1007b0954.c#L31) | Writes this exact signed 32-bit field in the corresponding public-data position. Native load is `0x1007b09ec: ldr w1,[x20,#0x8f0]`. |

`nk-format` previously consumed this field into the discarded local
`_fx_idx_amp_split_point`. It now exposes `Group::fx_amp_split_point: i32`.
XML defaults to 6. Both decoders reject values outside `0..=8`; this range check
is Cadence's malformed-input policy, not a claim that the shown native reader
validates it. The native reader and XML setter shown above store raw integers.

## Meaning of the boundary

[BDsp::HandleVoiceEfx, 0x1003aa3a8](../../Kontakt-8-REA/pseudocode/functions/0x1003aa3a8.c#L72)
reads `group = *(voice + 0x8e0)`. Its boolean argument selects a half-open range:

- `true`: slots `[0, group.fxIdxAmpSplitPoint)`;
- `false`: slots `[group.fxIdxAmpSplitPoint, 8)`.

Each slot is the effect pointer at `voice + 0x868 + slot * 8`. The loop advances
through physical slots, including null or bypassed entries. The boundary must
therefore be applied to the original serialized slot index, not the index in a
filtered vector of supported effects. For the default 6, slots 0 through 5 run
before the amplifier and slots 6 and 7 run afterward. Boundary 0 puts every slot
afterward; boundary 8 puts every slot before it. A null group pointer uses all
eight slots in the first phase and none in the second.

The loop's per-voice pointers establish that these processors are separate for
each voice. Summing notes before an amp or other nonlinear group effect would
change the recovered topology.

## Amplitude, pan, fade, and output order

The ordinary group-backed path in
[BDsp::ProcessVoice, 0x10046772c](../../Kontakt-8-REA/pseudocode/functions/0x10046772c.c)
has this audio order:

1. Render the source into the current voice buffers. Internal modulators have
   already been evaluated, but evaluation is separate from multiplying audio.
2. Invoke `HandleVoiceEfx(..., true, ...)` at line 947.
3. Apply the volume modulation buffer at mod-manager `+0x2a10` (line 957 and
   multiplication at 1231 onward). Negative values are clipped before
   interpolation. [BModTargetBase::staticInit, 0x100419c48](../../Kontakt-8-REA/pseudocode/functions/0x100419c48.c#L25)
   maps target enum 2 to `volume`; the buffer's target indexing in
   [BContMod::RunForAllTargetsInt, 0x1003a71a4](../../Kontakt-8-REA/pseudocode/functions/0x1003a71a4.c#L113)
   places target 2 at this offset. Amplitude-envelope routes targeting volume
   therefore act after the pre-amplifier insert phase.
4. Apply group channel swap (`group+0x128`, line 1445) and polarity inversion
   (`group+0x127`, line 1518), when enabled.
5. Apply group volume (`group+0x48`, line 1596 onward), with the shown group
   smoothing state at `+0x178`; effective mute forces zero. The XML setter
   [BGroup::PrsSetValues](../../Kontakt-8-REA/pseudocode/functions/0x10040127c.c#L207)
   ties `volume` directly to `+0x48`.
6. Apply voice gain (`voice+0x564`, line 1758 onward), using the smoothed value
   at `voice+0x57c` when it changes.
7. Apply pan: pan modulation is read from mod-manager `+0x2b10`, and group
   pan `group+0x4c` is combined with `voice+0x568` (line 1884 onward; group
   combination starts at 1978). Target enum 3 is `pan`. The unmodulated branch
   visibly uses linear balance factors `1-pan` / `1+pan` with endpoint clamps.
8. Apply `FadeVoice` and its amplitude multiplier at line 3029 (or line 3612
   in the alternate channel/pan branch).
9. Invoke `HandleVoiceEfx(..., false, ...)` at line 3232 (or 3815).
10. Mix/adapt the resulting channels into the voice's output destinations.

The post-amplifier call is guarded by the same ordinary-path flag
`voice+0x524 == 0` as the pre-amplifier call. Special preview/machine paths and
multichannel output branches are present; this note does not reconstruct them
as a separate compatibility implementation.

Voice gain includes zone gain and selection crossfades. In
[BEngine::SetVoiceParameters, 0x1003e803c](../../Kontakt-8-REA/pseudocode/functions/0x1003e803c.c#L199),
key/velocity fade factors are accumulated and then multiplied by the zone
volume at `zone+0x9fc`; the result is stored in `voice+0x564` at line 237.
Zone pan `zone+0xa00` is added to `voice+0x568` at lines 238–247. Thus moving
only group volume while retaining zone gain/crossfades before the pre-FX phase
would still disagree with this native ordering. Additional note-event gain
updates also enter `voice+0x564`; their complete script semantics are outside
this placement change.

## Send taps are taken at their slot

[HandleVoiceEfx](../../Kontakt-8-REA/pseudocode/functions/0x1003aa3a8.c#L232)
recognizes runtime kind byte `effect+9 == 0x0c` and invokes
`BDsp::PutSignalToSendFX` with the current physical slot index at line 254.
The send object does not transform dry audio; the tap occurs while walking the
selected pre- or post-amplifier half of the chain.

[PutSignalToSendFX, 0x1003ac1f8](../../Kontakt-8-REA/pseudocode/functions/0x1003ac1f8.c#L1338)
obtains its audio inputs directly from the current voice scratch pointers at
`DSP + thread*0x80 + channel*8 + 0x30`. The ordinary return-send path then calls
`BBufferManager::CopyBufferWithScaleBuf` (line 1390) with the send-level field at
`sendObject+0x1c0 + sendIndex*4`, the send's own interpolated modulation buffer,
and accumulation state. It does not separately read group volume/pan,
`voice+0x564/+0x568`, or the amplifier's volume/pan modulation buffers.

Consequently a pre-amplifier send taps the signal before amplifier gain,
amplitude envelope, pan and voice fade; a post-amplifier send already contains
those operations. Applying amplifier gain/pan again to every send tap would be
incorrect. Sidechain sends have their own path in the same function and are not
newly implemented by this boundary exposure.

## Validation and limits

The focused `group_fx_placement` parser tests cover binary and XML boundaries
0/6/8, native XML default 6, preservation of fields after the boundary, and
malformed/out-of-range values. `cargo test -p nk-format --offline` passes.

The field, default, physical-slot ranges, and listed source-order relationships
are directly supported by the saved source/instructions. Full envelope laws,
all modulation interpolation and smoothing, special source paths, arbitrary
output matrices, and sidechain parity remain separate questions. No new native
render comparison was performed as part of this placement research.
