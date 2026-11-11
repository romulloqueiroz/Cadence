# Kontakt external modulation serialization evidence

Static investigation, 2026-11-07. Target: the saved Kontakt 8.14.1 ARM64 export,
`../../Kontakt-8-REA/`. No Kontakt process was launched and no analysis database
or executable was modified. Saved Ghidra pseudocode is provider output, not
original source. Original export Evidence ID:
`ev_8369a586de610d706294e52323779b18fdfb6ea6e127c1d77be18f0e0511748a`.
Analysis profile:
`040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`.

## Exact format observations

All pseudocode paths below are relative to
`../../Kontakt-8-REA/pseudocode/functions/`.

| Fact | Saved source |
| --- | --- |
| External modulator type is `0x0c`, current private version `0x105` | `0x1007c9070.c:16`, `0x1007c9078.c:16` |
| External array type is `0x3c`, current array version `0x13` | `0x1004e6a28.c:16`, `0x1004e6a30.c:16` |
| Current array writes count 64, presence byte per slot and a type-`0x0c` chunk for every present entry | `0x1004e6c4c.c:24-78` |
| Older arrays before version `0x13` use implicit count 32; newer arrays read an explicit signed count | `0x1004e6a38.c:24-36` |
| Group `0x4a` is a separate optional serializable object at group `+0x668`, not the external modulation array | `0x1007b16b0.c:80-108`, `0x1007b0500.c:159-173` |

For external array version `0x11`, when the native ReadContext flag at `+0x44`
is true, a further `s32` is read after each presence byte. This context-specific
mode is separate from ordinary file reading and must not be guessed from bytes.

### External private data

`BParExternalMod::doReadPrivPars`, `0x10078700c.c:30-262`, accepts versions
`0x100..0x105`. Current writing is `0x100787798.c:21-44`.

1. The base target-list payload described below.
2. A source display-name string.
3. `s32 kind`: 0 none, 1 external source, 2 zone-envelope source.
4. Only when `kind == 1`: serialized source `s32`; for runtime source 4 add
   `u8 ccNumber`; for runtime source 5 and version `>=0x102` add `s32` source
   index; then `s16 initialValue`.
5. `u8 bypass`, `u8 simulateK2EnvTimeModulation`, `s32 classID`.
6. Version `>=0x103`: one boolean at native `+0x4c8`.
7. Version `>=0x104`: one boolean at native `+0x4c9`.

The two final booleans are retained as `flag_v103` / `flag_v104`; their runtime
meaning is not established here. `0x1004e6a38.c:78-86` establishes defaults:
initial value 0, class ID -1, both final flags true. Target reset
`0x1003ffe08.c:18-23` establishes intensity 1, flags/smoothing 0 and slot index
-1. These defaults are useful for importing omitted XML fields, but are not
evidence of initial MIDI controller values.

Version `0x100` chooses legacy `BString::Retrieve` versus `Stream::readString`
according to patch app version. Both use the same nonnegative `u32` byte length
and following bytes. The legacy form additionally accepts negative lengths as
a null sentinel. Cadence's parser accepts their common nonnegative encoding and
explicitly rejects negative/absurd strings rather than guessing the app version.
References: `0x10078700c.c:39-98`, `0x100268984.c:25-53`,
`0x102b51074.c:22-49`, `0x102b52098.c:21-31`.

### Source enum conversion

The serialized source is not generally identical to Kontakt's runtime enum.
`ReadEnumAsInt<BExternalModSource>` at `0x100787724.c:20-22` indexes a table at
VA `0x104d5c584`. Direct reads from `Kontakt-8.arm64` corroborate these 19 signed
32-bit entries at file offset `0x4d5c584`:

```
serialized: 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18
runtime:    0 1 2 3 4 7 8 9 10 11 12 13 5 16 17 18 19 20 21
```

The reverse writer table is at VA/file `0x104d5c52c`/`0x4d5c52c`;
`0x100786f90.c:18-19` performs that conversion. XML source names in
`0x1004d2ad0.c:199-249` corroborate runtime 1 pitch bend, 2 poly aftertouch,
3 mono/channel aftertouch, 4 MIDI CC, 7 key position, 8 attack velocity,
9 release velocity, 10 release-trigger counter, 11 constant, 12 random
unipolar and 13 random bipolar.

### Target-list payload

The external reader invokes the base private reader using base version 2
(`0x10078700c.c:96-98` and subsequent cases). Its version number is implicit;
there is no extra serialized base-version field.

Base writer `0x100786de4.c:27-60`, corroborated by reader
`0x100786498.c:65-120`, writes:

1. `s32 targetCount` (native storage has 16 target slots).
2. **For every target**, before any transformation: string target XML name,
   `f32 intensity`, `s16 slotIdx`, `u8 flags`, `s16 smoothingCoef`, string
   target display name, conditional `s8 targetObjIdx`, `u8 tableOpen`.
3. **For every target again**, its transformation payload.

The optional object-index byte is present for native effect/modulator targets.
Classification is numerical: effect IDs `[0x1e,0x8b)`, modulator IDs
`[0x19,0x1c)`, `[0x8d,0xad)`, ID `0xb4`, `[0xb7,0xe1)`.
See `0x1003a7cd0.c:18-22`, `0x1003a7d10.c:16`. Target-name mappings are explicitly
registered by `0x100419c48.c`; this provides the names used by the parser's
classification. Volume/pan/pitch (IDs 2/3/6) do not have this byte;
filterCutoff/filterQ (IDs `0x1f`/`0x20`) do. Unknown names map to native target
zero, and have no object-index byte (`0x10041c3fc.c:24-46`). Special aliases in
that function retain the same effect/modulator classification.

Transformation kind byte (`0x100786cbc.c:22-63`):

- 0: no further bytes.
- 1: boolean active, then 128 `f32` table values
  (`0x1007862e4.c:18-23`, `0x100786338.c:21-31`).
- 2: boolean active, signed byte point count, then that many triples of `f32`
  x/y/curvature (`0x1007860cc.c:19-30`, `0x100786154.c:22-38`). Native curve storage
  holds 32 points. Negative/excess counts fail explicitly in Cadence.

## Implementation and verification boundary

`crates/nk-format/src/external_mod.rs` decodes the opaque existing `PresetChunk`
object wrapper and its private data. It preserves inactive tables/curves and
effect/modulator metadata, independently of the playable sampler subset.
Unsupported versions, corrupt framing, non-finite values, oversized counts,
invalid strings and unknown transformation kinds are explicit errors.

Five parser unit tests cover every supported private version, source conversion,
source-specific fields, two-target metadata ordering, effect object indices,
inactive tables, curve triples, zone-envelope field omission, all truncated
prefixes and malformed values. Synthetic serialized fixtures are independently
authored from the observed field ordering, not vendor preset redistribution.
This is static format evidence and parser verification, not runtime parity.
