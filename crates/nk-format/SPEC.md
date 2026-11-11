# Kontakt instrument containers — recovered spec (working notes)

Status: **partial, verified where marked**. This is a behavioral spec for
interoperability, written from analysis of a shipped binary and from the
independent LGPL implementation [ConvertWithMoss](https://github.com/git-moss/ConvertWithMoss),
then validated against files written by Kontakt. It contains no Native
Instruments code or assets.

## Provenance

| Fact | Source | Confidence |
|------|--------|------------|
| Plain magic `0x7FA89012`, zlib → K2 XML | `BPatchHeaderV42` @ `0x100262ff0`; real `.nki` | verified |
| App-version packing `a<<24 \| b<<16 \| c<<8 \| d` | `createAppVersionNumber` @ `0x100262fdc` | verified |
| Chunk record layout | `BFile::FindChunk` @ `0x10022b148` | inferred |
| NI container (`hsin`) layout | ConvertWithMoss `NIContainerItem`/`NIContainerDataChunk` | verified on real files |
| FastLZ-compressed subtrees | ConvertWithMoss `SubTreeItemChunkData`/`FastLZ` | verified on real files |
| Preset-chunk tree, program/group/zone | ConvertWithMoss `KontaktPresetChunk` | verified on real files |
| Factory content is **not** encrypted by default | `Piano Uno - Concert.nki`: 1 program, 8 groups, 123 zones, 126 file refs | verified |
| MD5 header checksum | `BPatchHeaderV42::doCalcHash` @ `0x10026358c` | **unresolved** (see §7) |

## 1. Plain container (`.nki`, user-authored)

Little-endian, all multi-byte fields LE.

| Offset | Size | Field | Observed |
|--------|------|-------|----------|
| `0x00` | u32 | magic | `0x7FA89012` |
| `0x04` | u32 | unknown | `0x00000988` |
| `0x08` | u16 | format version | `0x0100` |
| `0x0A` | u32 | unknown | `0x3E012A72` |
| `0x0E` | u16 | flags | `1` |
| `0x10` | u32 | app version | `0x030909FF` → `3.9.9.255` |
| `0x14` | u32 | app signature | `0x4B6F6E33` |
| `0x18` | i32 | saved-at (Unix) | `1248963460` |
| `0x1C` | u32 | unknown | `0x3A` |
| `0xAA` | — | **zlib payload** | inflates to `K2_Container` XML |

The `0x0110` header revision uses a different header length. The reader tries
`0xAA`, then scans `0x80..0x1000` for a zlib stream that inflates to K2 XML.

## 2. NI file container (Kontakt 5+; most factory `.nki`/`.nkm`)

A top-level 64-bit-length block wraps an item. All little-endian.

```
block64 {                       // u64 length INCLUDING the 8 length bytes
  u32 1
  "hsin"
  u32 1
  u32 0
  uuid[16]
  DataChunk chain
  u32 item_version
  u32 num_children
  child* { u32 index, ascii4 domain, u32 chunk_type, Item }
}
```

`DataChunk` is itself a block64:

```
block64 {
  ascii4 domain
  u32 chunk_type
  u32 version (1)
  DataChunk next          // nested, unless this is the terminator
  payload                 // remainder of this block
}
```

Known chunk types: `1` Terminator, `100` Bank, `101` Authoring Application,
`106` Authorization, `108` Soundinfo, `109` Preset Chunk Item, `115` Subtree,
`118` Container Root.

Payloads:

- **Authoring Application** `101`: `u32 1`, `u8 compressed`, `u32 app_id`
  (`2` = Kontakt), `u32 1`, `utf16len` application version.
- **Authorization** `106`: `u32 1`, `u32 pid_content` (`1` or `2`). If `1`:
  `u32 license_blocks`, `u32 num_pids`, `utf16len` PIDs, `u32`, `u32`,
  `u32 checksum`. **Non-empty PIDs mean an encrypted subtree.**
- **Subtree** `115`: `u32 1`, `u8 compressed`; if compressed:
  `u32 uncompressed_size`, `u32 compressed_size`, FastLZ data that itself
  contains a full `Item`.
- **Preset Chunk Item** `109`: `u32 1`, `u32 dictionary_type`, `u32 count` (1),
  `u32 size`, `u32 unknown`, `size` bytes preset data, `u32 padding`,
  `u32 magic` (`0x8565620D`).
- **Container Root** `118`: `u32 1`, `u32 ni_sound_version`
  (`major<<20 | minor<<12 | patch`), `u32 repository_magic`,
  `u32 repository_type`, rest.

## 3. Preset chunk tree (inside chunk 109)

```
chunk { u16 id, u32 size, [size]object }
object = 0x00 + bytes                         // unstructured
       | 0x01 + u16 version
              + u32 len + private
              + u32 len + public
              + u32 len + child*               // structured
```

Arrays:

- `GROUP_LIST (0x33)`: `u32 count`, then `count` structured entries; entries
  become `GROUP (0x04)`.
- `ZONE_LIST (0x34)`: `u32 count`, then per entry `u32 reference` +
  structured entry. Zones are the entries themselves (their id is the
  reference, not `0x2C`).
- `PARAMETER_ARRAY_16 (0x3B)` / `_32 (0x3C)`: `0x00`, `u16 version`,
  `u32 count`, then `count` entries of `{ u8 present, if present: u16 id,
  u32 size, bytes }`. Older files omit the count and use the fixed size.

IDs: `0x03` BANK, `0x04` GROUP, `0x06` PAR_SCRIPT, `0x17` PAR_FX_SEND_LEVELS,
`0x28` PROGRAM, `0x29` PROGRAM_CONTAINER, `0x2C` ZONE, `0x32` VOICE_GROUPS,
`0x33` GROUP_LIST, `0x34` ZONE_LIST, `0x3A` PARAMETER_ARRAY_8,
`0x3D` FILENAME_LIST, `0x45` INSERT_BUS, `0x47` SAVE_SETTINGS,
`0x4B` FILENAME_LIST_EX, `0x4E` QUICK_BROWSE_DATA.

### Multi slot list (`BANK 0x03` → `SLOT_LIST 0x37`)

`SLOT_LIST` public data starts with an 8-byte little-endian bitset of 64 slots.
For each set bit a full preset chunk (`u16 id, u32 size, object`) follows; its
id must be `PROGRAM_CONTAINER (0x29)`. Inside it, `PROGRAM_LIST (0x36)` public
data is an array: `u32 count`, then `count` structured chunks (no id/size
header) which are `PROGRAM` chunks with their groups and zones. Expanding these
makes `.nkm` multis inspectable and playable.

### File list (`0x3D` / `0x4B`)

`FILENAME_LIST`: `i32 count`, then paths; the list is read twice (special
files, sample files), followed by 8 bytes of metadata per sample.
`FILENAME_LIST_EX`: `u16 version` (2 or 3). Version 3 stores one path list with
metadata interleaved (`4 + 4 + 20` bytes per entry).

A path is `u32 segment_count` followed by segments: `0` ASCII drive,
`1` UTF-16 drive, `2` UTF-16 directory + `/`, `3` `../`, `4` UTF-16 filename,
`6` no-op, `8`/`9` UTF-16 (NKX/NKM filename).

### Zone parameters (children of `ZONE_LIST`)

The zone's chunk id is the **group index**; `public_data` is:

```
u32 sample_start
u32 sample_end
u32 sample_start_mod_range
u16 low_velocity, high_velocity, low_key, high_key
u16 fade_low_velocity, fade_high_velocity, fade_low_key, fade_high_key
u16 root_key
f32 zone_volume, zone_pan, zone_tune
if version >= 0x9A: u8, u8, u32
u32 filename_id            // index into the sample file list
u32 sample_resolution
u32 sample_rate
u8  channels
u32 num_frames
u32 unknown
if version <= 0x93: u32 unknown
u32 root_note
f32 tuning
u8  unknown
u32 sample_size
```

Versions seen: `0x93` (4.2), `0x98` (5.3–5.6), `0x99` (5.8), `0x9A..=0x9C`
(6.5+; Piano Uno uses `0x9C`). Playback range is `sample_start` to
`num_frames - sample_end`.

### `.ncw` samples

```
u32 magic 0xD69EA801, u32 version (0x130 K4 / 0x131 K5+)
u16 channels, u16 bits_per_sample (16/24/32)
u32 sample_rate, u32 num_frames
u32 offset_table_address, u32 block_data_address, u32 total_block_size
[88 bytes padding]
offset table: (block_data_address - offset_table_address)/4 x u32, [0] == 0
blocks at block_data_address; block i spans offsets[i]..offsets[i+1]
```

Each block holds `channels` chunks of 512 frames:

```
u32 magic 0x3E9A0C16, i32 base, i16 precision, u16 flags, [4 pad], data
```

- `precision > 0`: delta-coded packed signed values, accumulated from `base`.
- `precision < 0`: truncated packed signed values, `abs(precision)` bits.
- `precision == 0`: raw samples, `bits_per_sample/8` bytes each.
- `flags & 1`: mid/side stereo (`L=M+S`, `R=M-S`).
- `flags & 2`: sample bits are IEEE-754 floats.

### Program and group parameters

Program public data:

```
utf16 name, f64 total_sample_size, u8 midi_transpose (two's complement),
f32 volume, f32 pan, f32 tune, ...
```

Group public data (group index = its position in `GROUP_LIST`):

```
utf16 name, f32 volume, f32 pan, f32 tune,
u8 key_tracking, u8 reverse, u8 release_trigger, u8 release_trigger_mono,
i32 release_trigger_counter, i16 midi_channel, i32 voice_group_index,
i32 fx_amp_split_point, u8 muted, u8 soloed, i32 interp_quality
```

`midi_channel` is `-1` for binary "All" (XML uses `0` or `All`); `1..=16`
selects a one-based MIDI channel. `voice_group_index` selects a program voice
limit; `-1` means the default. `interp_quality` is `0` normal (linear), `1`
high (cubic) or `2` ultra (6-point Lagrange).

The three tune fields default to `1.0` (unity) and compose multiplicatively;
the three volumes compose multiplicatively as well.

### Group filters (XML subset)

XML groups accept `filterCutoff` (Hz), `filterQ`, `filterType`
(`lowpass`/`highpass`) and `filterBypass`; Cadence plays them as a per-voice
biquad. Native binary group filters live in the eight-slot `BParFX` array
(`BGroup::ResetParameters` at `+0x8b0`), whose common/type-specific payload is
not decoded yet. See [filter basics](../../docs/kontakt-filter-basics.md).

### Program FX chains

`PARAMETER_ARRAY_8` children carry `BParameterArraySer<BParFX,8>` payloads:
`u16 version`, count (eight for versions below `0x13`, otherwise `u32`), then
per slot a presence byte and a `SER::Read` chunk. A `BParFX` chunk contains a
common version-`0x50` private section (fix type, class ID, bypass, out levels,
send partition) and a type-specific child chunk. Filter children use
`ser_type 0x18`; version `0x92` stores two subtype fix IDs followed by
`(frequency, bandwidth_octaves, gain_db)` band triplets. Stereo spread
(`ser_type 31`, v0x70) stores `spread`, `pan`, `pseudo`; the limiter
(`ser_type 28`, v0x60) stores `inGain` dB and `release` ms. See
[filter basics](../../docs/kontakt-filter-basics.md).

### Group FX array

Group private data contains a `BParameterArraySer<BParFX,8>` payload starting
with `u16 version 0x13, u32 count 8` at the same framing as program arrays.
Simple filter children (`ser_type 0x18`, v0x92, 16-byte payload) store two fix
IDs, a normalized cutoff and resonance. Filter fix IDs are 2 lp1, 3 hp1,
4 bp2, 6 lp2, 7 hp2, 8 lp4, 9 hp4, 10 bp4, 11 br4, 12 lp6, 17 ladder. See
[filter basics](../../docs/kontakt-filter-basics.md).

### SSLGEQ (ser 68, v0x11)

Reads `LF_Gain, LF_Freq, bool, LMF_Gain, LMF_Freq, LMF_Q, HMF_Gain, HMF_Freq,
HMF_Q, HF_Gain, HF_Freq, bool, HP_Freq, LP_Freq, bool, bool`. Program playback
maps the four bands with normalized frequency (20 Hz–20 kHz exponential),
gain (`(v-0.5)*24` dB) and Q (`0.3 + v*9.7`), converting Q to octave bandwidth.

### Voice groups

A `PROGRAM` child `VOICE_GROUPS (0x32)` contains:

```
BVoiceLimit default, u8 presence_mask[16],
for each set bit: [pad byte when offset is odd] BVoiceLimit override
```

A `BVoiceLimit` (version `0x60`) is:

```
utf16 name, s16 kill_mode, u8 prefer_released,
i32 max_voices, i32 fade_ms, i32 exclusion_group
```

Kill modes are `0` any, `1` oldest, `2` newest, `3` highest, `4` lowest;
`max_voices == 0` is unlimited and `exclusion_group == -1` is inactive.
Override objects are two-byte aligned (see
[voice groups](../../docs/kontakt-voice-groups.md)).

### Zone loops

A `ZONE` child `LOOP_ARRAY` (`0x39`) contains `u16 enablement`, then for each
set bit: `u16 0x60`, `i32 mode`, `u32 start`, `u32 length`, `u32 count`,
`u8 alternating`, `f32 tuning`, `u32 crossfade_length`, and one optional pad
byte. Modes: `1` until end, `2` until release, `3` one-shot. `count == 0` is
infinite; otherwise it is the number of wraps after the first pass. `tuning`
scales the playback rate while looping. `crossfade_length` blends a forward-loop
tail with the pre-loop material; alternating-loop turnarounds are inherently
value-continuous. A zone's first mode-1 loop plays while held and its first
mode-2 loop replaces it on note-off; a mode-2-only zone stops at note-off.
Further loop entries are retained but not selected.

### Amplifier envelopes

Inside a group's `PARAMETER_ARRAY_16` children, `PAR_INTERNAL_MOD (0x0D)` /
`PAR_MOD_BASE (0x00)` carry modulators:

```
u8 main_version (1; a leading 0 is skipped), u16 version (0x80/0x81)
u32 block1_size + block1, u32 separator (0), u32 block2_size + block2
```

Block 1 lists target parameter names plus their `f32` intensities and the
source; a padding heuristic is required because a few target/source pairs have
0–2 unaccounted bytes.
Block 2 (source `ENV_AHDSR`) stores, in milliseconds, after 34 bytes:
`f32 curve, attack, decay, hold, release, sustain, u8 ahd_only`.
XML `IntModulators/K2_IntMod/LFO` carries `type`, `delay`, `frequency`,
`pulseWidth`, `startPhase` and `normalizeMultiLFO`; it maps to the same
`modulator::Lfo` model as this binary payload.
Block 2 for an `LFO_*` source stores after 34 bytes:
`s32 kind (0 sine, 1 rectangle, 2 triangle, 3 sawtooth, 4 random, 5/6 multi),
f32 delay_ms, f32 frequency_hz, f32 pulse_width, f32 start_phase,
u8 normalize`. `ENV_DBD`/`ENV_FM7` payloads are not decoded yet.
Layouts follow `BParEnv_AHDSR::doRead`, `BParEnv_DBD::doRead` and
`BParLFO::doRead` in the saved Kontakt 8 analysis.

### NI file container (monolith `.nkx`/`.nki`)

```
ascii16 "/\ NI FC MTD  /\", 248 pad, 8x 0xF0, u64 file_count, u64 total_size
ascii16 "/\ NI FC TOC  /\", 600 pad
per file: u64 index, 16 pad, 600-byte UTF-16LE name, u64 pad, u64 end_offset
8x 0xF1, 16 pad, ascii16 TOC, 592 pad
[concatenated file data; file i length = end_offset_i - end_offset_(i-1)]
```

### K2 monolith / resource container (`.nkr`)

Magic `0x5E70AC54`. A directory:

```
u32 magic, 10 bytes, u32 num_items, 4 pad
per item: u16 length (total incl. 8), u32 pointer, i16 type, content[length-8]
```

Types: `0` end, `1` directory, `2` sample, `3` NKI, `4` wallpaper. Content is a
UTF-16LE name; a `DIRECTORY` item's pointer is another directory.

Resource blocks follow the directories and wallpapers sequentially:

- `0x2AE905FA` wallpaper: 10-byte header for version `0x111+` (14 for older),
  then `u64 length` and data.
- `0x16CCF80A` sample: `u16 version`, 13 bytes, `u32 length`, 8 bytes, data.
- `0x0040179F` raw sample, `0x4916E63C` NKI.

Sample pointers are **not** reliable absolute offsets in newer files; sample
payloads are extracted by scanning the block stream and pairing them with the
`SAMPLE` directory entries in file order. Verified on the Piano Uno `.nkr`
(3 WAV IRs extracted).

## 4. Observed container families

| Extension | Detection | Note |
|-----------|-----------|------|
| `.nki` (user-saved) | `0x7FA89012` | plain zlib XML — supported |
| `.nki` (factory) | NI container `hsin` | supported structurally |
| `.nkm` (multi) | NI container | bank/slot tree |
| `.nkc` (collection) | `0x3FE1107A` | directory-style container |
| `.nkx` / `.nkr` | resource containers | sample/resource storage |

## 5. Verified end-to-end

`Piano Uno - Concert.nki` (Kontakt 8.0.0.0, authoring app intact, no
authorization PIDs):

- 225,793-byte preset payload, FastLZ-compressed subtree decoded successfully.
- 1 program, 8 groups, 123 zones, 126 file references.
- Sample references are `.ncw` files under `Samples/Piano Uno Samples/`, plus
  an `.nkr` resource and an IR `.WAV`.

This confirms factory libraries can be read without decryption when the
`Authorization` chunk carries no PIDs.

End-to-end render: note 60, velocity 100, group 3 selected the B2 sustain
sample (`932,864` frames), pitch-shifted from root 59 and rendered to 3 s at
48 kHz (peak 0.41).

## 6. Supported by this crate

- `container` — plain `0x7FA89012` + zlib + K2 XML.
- `ni_container` — NI `hsin` item/chunk tree, including FastLZ subtrees.
- `preset_chunk` — program/group/zone tree and sample file lists.
- `model` — program/group parameters and combined gain/pan/tune.
- `preset_chunk::expanded` — expands `.nkm` bank slot lists into programs.
- `modulator` — volume AHDSR envelopes.
- `external_mod` — binary external MIDI route metadata; XML maps the same model.
  The sampler applies bend/pressure/CC/velocity to pitch/pan/volume; see
  [MIDI expression](../../docs/midi-expression.md) for support and fidelity limits.
- `zone` — key/velocity ranges, root, tuning, sample id, loops.
- `ncw` / `wav` / `audio` — `.ncw` and `.wav` decode to de-interleaved `f32`.
- `file_container` — K5 monolith table-of-contents extraction.
- `k2_monolith` — `.nkr` directory tree and sample extraction.
- `nk-inspect --zones` / `--preset-tree` — deep inspection.
- `ncw-decode` — decode a `.ncw` to WAV.
- `nki-render` — play one note from an installed library or monolith to WAV,
  with group/zone gain, pan, tune, envelopes, loops and WAV/NCW samples.

## 7. Unresolved

- Pitch envelopes and non-AHDSR envelope types (`ENV_DBD`, LFOs).
- Filters. Key/velocity fades and round-robin/random/controller/key-switch group
  starts are decoded and played by the sampler; see
  [group-start evidence](../../docs/kontakt-group-start-evidence.md) and
  [crossfade evidence](../../docs/kontakt-crossfade-evidence.md).
- Multi-layer playback is heuristic: all main groups are summed, groups named
  "release" (or with the release flag) trigger at note-off, "pedal" groups are
  skipped. Script-driven routing is not emulated.
- Plain K2 XML single-program playback is implemented; see [mapping, units, supported subset and tests](../../docs/plain-xml-playback.md). Conditional group starts, XML key/velocity fades, controller/key-switch latches and sustain-pedal release deferral are supported by the sampler; multi-program XML expansion remains unsupported.
- `.nkc` collections.
- `.nkc` collection container (detected, not decoded).
- `.nkr` NKI blocks (samples are extracted; embedded NKI program extraction is
  not yet wired up).
- Libraries with non-empty `Authorization` PIDs use a per-product 64 KiB XOR pad
  (`AES-CTR(JDX, HU)` over Kontakt's static seed). Cadence decodes these when the
  library `.nicnt` or the NI machine-key registry supplies `HU`/`JDX` and a
  licensed Kontakt installation supplies the seed; see
  [retail-library-decryption.md](../../docs/retail-library-decryption.md).
  macOS uses CommonCrypto for AES-CTR. `.nkc` collection containers and
  non-`0x100` sample encryption modes remain undecoded.
- Meaning of plain-header fields `0x04`, `0x0A`, `0x1C`; the MD5 checksum in
  `BPatchHeaderV42` does not fit the `0x0100` plain layout (revision mismatch:
  `0x0100` vs `0x0110`).

## Legal note

Interoperability research only. The Rust implementation is independent; no NI
code, binaries, samples, or assets are redistributed. NI's EULA restricts
reverse engineering; applicability varies by jurisdiction. The LGPL
ConvertWithMoss project is used as a behavioral reference, not as linked code.

## 8. Plain XML playback model

`plain_model` maps the supported K2 XML subset into `model::Instrument`.
XML sampleEnd is absolute and inclusive; binary sampleEnd remains an end trim.
The shared loader handles compressed plain instruments and plain presets inside
file containers. See [playback profile and reference evidence](../../docs/plain-xml-playback.md).
