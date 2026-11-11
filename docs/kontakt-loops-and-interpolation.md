# Loop counts, tuning, crossfades and interpolation quality

Implemented 2026-11-07 as the stage-4 increment of the
[Kontakt compatibility backlog](kontakt-compatibility-roadmap.md).

## Loop wire fields

`ZONE` child `LOOP_ARRAY (0x39)` stores `u16 enablement`, then for each set bit:

```
u16 0x60, i32 mode, u32 start, u32 length, u32 count,
u8 alternating, f32 tuning, u32 crossfade_length
```

Modes are `1` until end, `2` until release and `3` one-shot. The XML fields are
`loopStart`, `loopLength`, `loopCount`, `loopTuning`, `alternatingLoop`,
`xfadeLength` and `mode`.

Validated content:

| Library | Loop shape |
| --- | --- |
| Audience Choir | 126 loops, mode 1, count 0, tuning 1.0; forward and alternating; crossfades 9,600–40,300 frames |
| FMG / Krimh / Piano Uno / GroveBass | no loop array in the analysed presets |

## Playback behavior

- `count == 0` is infinite. `count == N` allows `N` wraps after the initial
  pass; the next boundary stops looping and playback continues past the loop
  end. This interpretation follows the conventional count-after-first-pass
  meaning; native's exact counting is not independently corroborated.
- `loopTuning` multiplies the playback-rate increment while the loop is active.
- A forward loop with `crossfade_length > 0` blends the loop tail with the
  pre-loop material at `position - loop_length`. At the wrap the output is
  therefore continuous with the loop start. If the pre-loop range falls before
  the zone start, no crossfade is applied.
- Alternating/ping-pong loops are value-continuous at the turnaround because
  the direction reverses in place; no extra seam crossfade is applied.
- A zone may carry both a sustain loop (`mode 1`) and a release loop
  (`mode 2`). The sustain loop plays while the note is held; note-off switches
  the same voice to the release loop. If only a `mode 2` loop exists it plays
  until note-off, then stops. Additional loop entries beyond the first of each
  mode are ignored; they are not played simultaneously.

## Interpolation quality

The group public-data field `interp_quality` (XML `interpQuality`) maps to the
native `normal`, `high` and `ultra` modes found next to the option string:

- `0 normal`: recovered 4-point third-order Lagrange interpolation.
- `1 high`: Cadence currently uses 4-point Catmull-Rom interpolation.
- `2 ultra`: Cadence currently uses 6-point fifth-order Lagrange interpolation.

Native `BEngine::SetVoiceParameters` selects windowed-sinc tables for `high`
and `ultra`; Cadence's two kernels are approximations. Native normal dispatches
through `BDsp::DoGetBlockSampler` / `BSample::FillBlock_ResL` to
`BSample::ResampleChannelQuad_L_SSE2`, whose four taps use the recovered cubic
polynomial with factors `1/2` and `1/6`.

Normal reads `index-1..index+2`; streaming includes the previous page. Cadence
retains a bounded linear fallback when outer taps are unavailable, including
sample/zone edges. Native guard samples and loop-edge tap remapping remain
unverified. The analysed libraries use `normal` for every group.

A controlled Piano Uno dry capture at 48 kHz improved from 54.03 dB to 56.31 dB
raw SNR after replacing normal's previous linear kernel, with zero fitted delay
and no gain compensation. A separate playback-position drift and release-tail
difference remain; this does not establish sample-exact native playback.
See [controlled sampler fidelity](kontakt-sampler-fidelity.md) for the six-case
comparison and independent native phase trace.

## Limits

- Only the first sustain loop and the first release loop are used; further
  entries are ignored, and loops are not played simultaneously.
- Ping-pong loops do not apply a crossfade; their turnaround is inherently
  value-continuous.
- `high` and `ultra` use approximate kernels rather than the native
  windowed-sinc kernels. Normal retains different boundary handling and
  playback-position arithmetic, so no mode is claimed sample-exact.
- Finite counts and tuned loops are accepted from XML and binary presets, but
  the analysed retail libraries only exercise infinite, untuned loops.

## Evidence

- `crates/nk-format/SPEC.md` loop-array and group field layouts.
- `../../Kontakt-8-REA/pseudocode/functions/0x1003f166c.c`
  (`BEngine::IO_SetLoopCount`)
- `../../Kontakt-8-REA/pseudocode/functions/0x100137480.c`
  (`SampleEditorModule::SetLoopCount`)
- Kontakt 8 string table: `interpQuality` followed by `normal`, `high`, `ultra`.

- Saved original Kontakt arm64 pseudocode, analysis profile
  `040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`:
  `0x1003e803c` (`BEngine::SetVoiceParameters`, quality/table selection),
  `0x10046e510` (`BDsp::DoGetBlockSampler`, resampling dispatch),
  `0x10046ec00` (`BSample::FillBlock_ResL`, normal path),
  `0x1004dc2a4` (`BSample::ResampleChannelQuad_L_SSE2`, four-point polynomial).
  These are static evidence; saved native audio supplies the bounded runtime
  comparison above.
