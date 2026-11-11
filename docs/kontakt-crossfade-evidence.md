# Kontakt zone key/velocity crossfade evidence

Read on 2026-11-07 for Cadence's independent zone-selection implementation.

## Artifact and observation boundary

The target is the saved REA/Ghidra export of Kontakt 8.14.1's ARM64 main
executable, under `../../Kontakt-8-REA/`. The ARM64 file was rehashed during this
pass and matches SHA-256
`42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.
The files report Ghidra 12.1.4 and analysis profile
`040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`.

This pass read existing pseudocode and the combined catalog. It did not launch
Kontakt, modify its artifacts, open another analysis session or issue new REA
Evidence records. The addresses below name saved static observations, not
invented Evidence IDs or runtime measurements. The archive's
[README](../../Kontakt-8-REA/README.md) describes the export/provenance limits.

## Findings

| Saved function | Static observation | Implementation consequence |
| --- | --- | --- |
| [`BZone::PrsSetValues`, `0x100462f2c`](../../Kontakt-8-REA/pseudocode/functions/0x100462f2c.c) | `lowKey`/`highKey` populate signed-short fields `+0x9ec`/`+0x9ee`; `lowVelocity`/`highVelocity` populate `+0x9e8`/`+0x9ea`. `fadeLowKey`/`fadeHighKey` map to `+0x9f4`/`+0x9f6`, and `fadeLowVelo`/`fadeHighVelo` to `+0x9f0`/`+0x9f2`. The numeric values are read through `atoi`. | XML names are tied to the exact fields consumed by voice gain calculation, rather than inferred from editor labels. Preserve these widths as integer MIDI steps. |
| [`BProgram::CalculateNoteOnLookupTable`, `0x1003db7ac`](../../Kontakt-8-REA/pseudocode/functions/0x1003db7ac.c) | The lookup-table builder iterates from the stored low key through the stored high key inclusively, adding valid keys below 128. Fade fields are not used to widen that key selection. | Fades occupy the inside of the inclusive range. They do not extend the set of selected notes. |
| [`BEngine::NoteOn`, `0x1003e9418`](../../Kontakt-8-REA/pseudocode/functions/0x1003e9418.c) | The visible zone-selection branch checks `lowVelocity <= velocity <= highVelocity`, using the upper seven bits of the note-event velocity (`>> 9 & 0x7f`). | Velocity range selection is inclusive. Cadence's existing 7-bit velocity event domain can feed the fade calculation directly. |
| [`BEngine::SetVoiceParameters`, `0x1003e803c`](../../Kontakt-8-REA/pseudocode/functions/0x1003e803c.c) | Before multiplying by zone volume at `+0x9fc`, the function starts a fade gain at 1. For each nonzero fade width, if distance from the relevant edge is less than that width, it multiplies by `(distance + 1) / (width + 1)`. The order is key-low, key-high, velocity-low, velocity-high. The visible section contains no sine, square root, power transform or cross-zone normalization. | Use discrete linear-amplitude edge ramps and multiply all four factors. The outermost included MIDI value has nonzero gain `1/(width+1)`; setting the endpoint gain to zero would change the recovered behavior. Overlapping low/high fades within one zone also multiply. |
| [`BProgram::ApplyCrossfade`, `0x1004326c4`](../../Kontakt-8-REA/pseudocode/functions/0x1004326c4.c) | The editor-side operation sizes fade widths from overlapping integer bounds with `+1` terms and can shorten paired widths to fit a zone. | Corroborates width-as-count rather than an absolute fade endpoint. This editing function alone is not the playback gain specification; the voice-setup function above supplies that evidence. |

For a lower boundary `L`, upper boundary `H`, input `x` and widths `a`/`b`, the
observed per-axis behavior inside `L <= x <= H` is:

```text
low_factor  = (x - L + 1)/(a + 1), if a > 0 and x - L < a; otherwise 1
high_factor = (H - x + 1)/(b + 1), if b > 0 and H - x < b; otherwise 1
axis_gain   = low_factor * high_factor
zone_fade   = key_axis_gain * velocity_axis_gain
```

An eight-value overlap, such as MIDI values 60 through 67, uses width 8 on both
participating zone edges to produce complementary amplitudes. Their included
edge gains run from `1/9` to `8/9`; the sum of the two zone gains is 1 throughout
that overlap. A width of 7 over that same eight-value overlap would not be the
same mapping.

Program and group ranges must be independent gates. Clipping a zone's low key
from 60 to a program's low key of 65 would move the fade origin and change its
gain. With an original fade width of 10, note 65 must retain gain `6/11`.

## Cadence implementation and checks

`Zone::selection_gain(mapped_note, velocity)` in `crates/nk-format/src/zone.rs`
implements the recovered range/fade calculation using stack storage and scalar
arithmetic. It returns zero outside the inclusive range, for invalid MIDI input
or malformed range/fade fields above 127. Those malformed-input choices are
Cadence's defensive behavior; the decompiled runtime assumes fields have already
been validated/normalized.

Eight focused unit tests cover inclusive bounds, both key edges, both velocity
edges, complementary overlaps, simultaneous four-edge multiplication, decoded
binary fade fields, preservation of the original fade origin, extreme widths
and malformed inputs. Run them with:

```sh
cargo test -p nk-format --lib selection_tests --offline
```

The sampler must apply the returned fade once when launching each voice and use
the mapped/transposed key. Choosing the velocity event for release triggers is a
separate note-routing policy: the inspected gain function reads its note event's
velocity but does not establish how that release event was constructed.

## Limits

- These observations concern zone key/velocity launch gain, not sample-loop
  crossfades, group morphing, script modulation or arbitrary later DSP gain.
- No equal-power curve appears in the inspected launch-gain section. This does
  not establish that no other Kontakt crossfade feature uses nonlinear curves.
- `SetVoiceParameters` and `NoteOn` retain Ghidra removed-unreachable-block
  warnings. The lookup-table function retains provider terminal-flow notes.
  Pseudocode is not original source or independently observed execution.
- This does not establish full Kontakt audio/runtime parity, behavior for
  script-directed special note events, or higher-resolution velocity handling.

## Saved-file hashes

SHA-256, with paths relative to
`../../Kontakt-8-REA/pseudocode/functions/`:

```text
0x1003e803c.c  51d71c2def962116f37b9b4ba93985844cdb66f07140088579c64ac8ce2bc620
0x100462f2c.c  60830878d28f85a304c913d4907a89b4c5d28f4e9d1285c8ba8aad9fe91f5c7e
0x1003db7ac.c  69509010429e343c6c08e7692bd61e62c1a6f340c7bcb7a72731c35fb20bf1de
0x1003e9418.c  fc160f26cdf9f48cf916765edaceafd6d9dbda1fea0972ddba99300c365fc645
0x1004326c4.c  2f083772947ae2164da89155826c026dabfb4f9c22d8c10f205e573ed0bdcaff
```
