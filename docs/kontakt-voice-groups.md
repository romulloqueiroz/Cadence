# Voice groups, limits and choking

Implemented 2026-11-07 as the second stage-3 increment of the
[Kontakt compatibility backlog](kontakt-compatibility-roadmap.md).

## Wire format

`BProgram::readVoiceGroups` (`0x1007b91f4`) reads:

```
u16 0x32, s32 (ignored), BVoiceLimit default,
u8 mask[16] (128 presence bits),
for each set bit: [2-byte alignment pad] BVoiceLimit override
```

`BVoiceLimit::doRead` (version `0x60`, serialization type `0x2b`) stores:

```
WString name, s16 kill mode, bool preferReleased,
s32 maxNumVoices, s32 msFadeTime, s32 exclusionGroup
```

`BVoiceLimit::PrsSetValues` corroborates the XML names and modes:
`kill_any`, `kill_oldest`, `kill_newest`, `kill_highest`, `kill_lowest`.

The override objects are two-byte aligned. In the factory Piano Uno preset the
16-byte mask is followed by a pad byte when the running offset is odd; the
following `BVoiceLimit` objects are likewise separated by a pad when their
odd-length predecessor leaves an odd offset. FMG Shelter Booms, Krimh Drums and
the Choir have an all-zero mask.

`BGroup::doReadPubPars`/`PrsSetValues` store the group slot as
`voice_group_index` (binary `i32`, XML `voiceGroup`); `-1` selects the program
default limit.

Validated real presets:

| Library | Groups | Default max / fade | Explicit overrides |
| --- | --- | --- | --- |
| FMG Shelter Booms | 1 | 32 / 10 ms | none |
| Krimh Drums Free | 151 | 1024 / 10 ms | none |
| Audience Choir | 9 | 128 / 10 ms | none |
| Piano Uno – Concert | 6 | 256 / 10 ms | slots 0,1,2,3 = 5/100 voices, slot 9 = 1 voice |

Piano Uno assigns `Key Noise` to slot 1 and `Pedal` to slot 10, so the parsed
limits reach real playback.

## Playback behavior

- A group with a negative slot uses the program-default voice limit.
- `maxNumVoices` counts every voice whose group resolves to the same voice-group
  slot, including release voices. `0` means unlimited.
- On overflow, the kill mode selects the victim: `Any`/`Oldest` (largest age),
  `Newest` (smallest age), `Highest` (highest key) or `Lowest` (lowest key).
- `preferReleased` restricts the victim pool to voices already in release when
  any exist.
- `msFadeTime` fades the victim linearly. If no spare pool slot exists, the
  victim is retired immediately so the incoming attack can start. The pool is
  64 voices, so limits above 64 only interact with pool stealing.
- A non-negative `exclusionGroup` chokes voices in *other* voice-group slots
  that share the same exclusion group.

These are cross-group, pre-spawn checks: a note that fails zone selection does
not choke anything.

## Limits

- KSP/Script-driven voice-group changes and `IO_SetVoiceGroup` are not
  emulated.
- `kill_any` is implemented as oldest-first; native's exact "any" choice is
  unspecified by the saved evidence.
- Fade time is applied per voice only when another pool slot is available.
- Voice-group defaults are parsed strictly; unknown wire versions fail the
  instrument load rather than silently ignoring limits.
- There is no per-group articulation/monophonic policy beyond the recovered
  voice limit fields.

## Evidence

- `../../Kontakt-8-REA/pseudocode/functions/0x1007b91f4.c`
  (`BProgram::readVoiceGroups`)
- `../../Kontakt-8-REA/pseudocode/functions/0x10077d6d8.c`
  (`BVoiceLimit::doRead`)
- `../../Kontakt-8-REA/pseudocode/functions/0x1004cb404.c`
  (`BVoiceLimit::PrsSetValues`)
- `../../Kontakt-8-REA/pseudocode/functions/0x1004ba7e8.c`
  (`BEngine::FindAndKillVoice`, kill modes)

## Release/pedal group naming

The factory Piano Uno preset has groups named `Release` and `Pedal` whose
serialized `releaseTrigger` byte is **false**, and every group in the analysed
libraries reports the monophonic byte as true with counter `0`. The explicit
public fields therefore cannot currently replace the display-name heuristic
that routes release layers and pedal groups. Cadence retains that heuristic
until a better-scoped flag or KSP routing is recovered; the fields are stored
as metadata but do not drive playback.
