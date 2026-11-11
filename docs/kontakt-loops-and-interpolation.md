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

- `0 normal`: linear interpolation.
- `1 high`: 4-point Catmull-Rom cubic interpolation.
- `2 ultra`: 6-point fifth-order Lagrange interpolation (Cadence-defined; the
  native ultra kernel is not decoded).

Cubic reads `index-1..index+2`. Streaming plans the previous page as well, and
falls back to linear when either outer tap is unavailable. The analysed
libraries use `normal` for every group.

## Limits

- Only the first sustain loop and the first release loop are used; further
  entries are ignored, and loops are not played simultaneously.
- Ping-pong loops do not apply a crossfade; their turnaround is inherently
  value-continuous.
- `ultra` uses a 6-point Lagrange kernel rather than the native kernel, so it
  is not sample-exact against Kontakt.
- Finite counts and tuned loops are accepted from XML and binary presets, but
  the analysed retail libraries only exercise infinite, untuned loops.

## Evidence

- `crates/nk-format/SPEC.md` loop-array and group field layouts.
- `../../Kontakt-8-REA/pseudocode/functions/0x1003f166c.c`
  (`BEngine::IO_SetLoopCount`)
- `../../Kontakt-8-REA/pseudocode/functions/0x100137480.c`
  (`SampleEditorModule::SetLoopCount`)
- Kontakt 8 string table: `interpQuality` followed by `normal`, `high`, `ultra`.
