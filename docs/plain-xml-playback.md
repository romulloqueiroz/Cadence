# Plain K2 XML instrument playback

Cadence can play the supported subset of unencrypted plain `.nki` containers:
`0x7FA89012` plus zlib-compressed `K2_Container` / `K2_Bank` XML. A single program
is converted into the same instrument model and streamed sampler as binary NI
presets. The loader also recognizes a plain instrument inside a file container.
`sample-host`, the production worker source loader and `nki-render` share the
high-level model/path loader. No new public C or shared-memory ABI is introduced.

## Supported mapping

| XML | Playback meaning |
| --- | --- |
| Program `volume`, `pan`, `tune`, `transpose` | Linear gain, balance, pitch ratio and MIDI-note transposition before zone selection/pitch tracking |
| Program/zone key and velocity bounds | Inclusive ranges; program bounds restrict zone ranges |
| Group `index`, zone `groupIdx` | Explicit IDs mapped to dense indices, including sparse/out-of-order IDs; duplicates/missing references fail |
| Group `volume`, `pan`, `tune` | Compose with program and zone values |
| Group `keyTracking`, `reverse`, `muted`, `soloed` | Pitch tracking, reverse range playback and group gating |
| Group `releaseTrigger` | Explicit note-off trigger; display names do not change XML routing |
| Group `filterCutoff`, `filterQ`, `filterType`, `filterBypass` | Per-voice biquad low-pass/high-pass with cutoff modulation; see [filter basics](kontakt-filter-basics.md) |
| Zone `rootKey`, `zoneVolume`, `zonePan`, `zoneTune` | Root note, linear gain, balance and pitch ratio |
| Zone `sampleStart`, `sampleEnd` | Absolute first/last frame, **inclusive**; a missing end means EOF |
| `Sample/V file_ex2` or `file` | Exact decoded sample reference; no heuristic search through unrelated metadata |
| `Loops/Loop` | One forward/ping-pong `until_end` or `until_release` loop with count, tuning and (forward) crossfade; `oneshot` ignores note-off |
| `IntModulators/K2_IntMod/Envelope` | AHDSR/AHD fields in milliseconds, sustain level, note-off-less mode, partial intensity and volume/pitch/pan/filter targets; bypassed modulators are skipped |
| `IntModulators/K2_IntMod/LFO` | Sine/rectangle/triangle/sawtooth `type`, `delay`, `frequency`, `pulseWidth`, `startPhase`, `normalizeMultiLFO`; targets as above |
| `ExtModulators/K2_ExtMod` | Instrument-defined bend, pressure, MIDI CC and attack-velocity routes to pitch/pan/volume, including target depths, inversion, tables/curves and smoothing; see [MIDI expression](midi-expression.md) |

Sample-end interpretation is carried explicitly in the model. Binary presets
continue to use their end-trim count, while XML uses an inclusive absolute frame.
This avoids guessing from sample length, and supports a zone containing just
frame zero. Fractional interpolation cannot read past a zone's endpoint.

Positive K2 tune fields are ratios (1=unity). The observed legacy neutral
program/group `tune=0` template value is normalized to unity. Zone and loop
ratios must be positive. Bounds checked by this implementation include MIDI
values 0..127, transpose -127..127, gains 0..64, ratios 1/256..256, pan -1..1,
envelope times up to one hour, and finite numeric values.

A matched note-off ends `until_release` looping and triggers explicit release
groups using the held note's velocity. Unmatched/repeated note-offs do not create
new release voices. Declared envelope release times, including values below
50 ms, are honored; the un-enveloped fallback retains its existing behavior.
One-shot voices ignore note-off. Binary group-name trigger heuristics are kept
for existing compatibility, while XML follows explicit trigger flags.

## Sample paths

Ordinary paths normalize backslashes. Extended NI paths decode `@v` volume,
`b` parent, `d` length-prefixed folder and `f`/`F` filename components. The `f`
metadata prefix is six UTF-16 units; `F` uses eleven. `%hhhh` escapes are decoded
as UTF-16, including surrogate pairs. Invalid lengths, truncated names, embedded
NULs, malformed Unicode and NKS-library (`m`) references are explicit errors.
Relative paths resolve beside the instrument; existing content-root/NKR and
embedded-sample resolution remain available. Missing-file relocation and broad
basename searches are not added.

Model conversion and filesystem work happen during loading/preparation. Resident
attacks and the bounded streaming cache are prepared before playback. No XML
parsing, path lookup or sample I/O occurs on the audio callback. Stream preparation
rejects XML playback/loop ranges outside the actual sample.

XML is capped at 64 MiB after inflation, 128 levels, 262144 nodes and 4096 direct
parameters per element. Compressed instrument reads retain the existing 128 MiB
metadata cap. Duplicated structural mapping lists and parameters fail explicitly.

## Boundaries

This is a supported playback subset, not complete Kontakt engine emulation.
Key/velocity fades now follow the recovered discrete edge law, and conditional
`GroupStart` rules are evaluated in the sampler: round-robin and random
selection, latched key switches, controller criteria and sustain-pedal release
deferral. Finite loop counts, loop retuning, forward crossfades and
sustain/release loop switching are now played; additional loop entries beyond
the first sustain/release pair, multiple amplitude envelopes and multiple XML
programs still fail explicitly rather than being approximated. Group `interpQuality` selects linear (`normal`) or cubic
(`high`/`ultra`) interpolation.

Scripts, effects beyond the XML group filter, binary FX slots and advanced
playback modes are not emulated by the sampler. External MIDI routes have the
supported subset documented in [MIDI expression](midi-expression.md). The
amplitude envelope's `atkCurving` value now shapes the attack segment using the
recovered curvature law; other envelope segments remain linear. Pitch/pan/filter
envelope targets, partial amplitude intensity and internal LFOs are applied
from XML and binary modulators. No Kontakt runtime parity or arbitrary-library compatibility is
claimed. Remaining group-start polish is
tracked in [kontakt-group-start-evidence.md](kontakt-group-start-evidence.md).

## Provenance and verification

Format facts were checked against the existing independent ConvertWithMoss
reader snapshot: [K2 tags/ratios](https://github.com/git-moss/ConvertWithMoss/blob/main/src/main/java/de/mossgrabers/convertwithmoss/format/ni/kontakt/type/kontakt2/K2Tag.java),
[XML mapping/path grammar](https://github.com/git-moss/ConvertWithMoss/blob/main/src/main/java/de/mossgrabers/convertwithmoss/format/ni/kontakt/AbstractNKIMetadataFileHandler.java),
and its inclusive endpoint contract in
[ISampleZone](https://github.com/git-moss/ConvertWithMoss/blob/main/src/main/java/de/mossgrabers/convertwithmoss/core/model/ISampleZone.java).
The existing local `kontakt_def.xml` supplies neutral legacy template evidence;
it contains no zones and is not proof of zone-bearing Kontakt runtime behavior.
The fixture XML and demo sine sample are independently authored; no vendor audio
or preset content is redistributed.

Eight format/CLI tests, nine sampler tests and one production-loader test cover
analytic gain/pitch/pan, sparse IDs, encoded/Unicode paths, inclusive endpoints,
range failures, reverse/key tracking, exact envelope release timing, explicit
release groups, one-shot/loop behavior, mute/solo, streamed/full-RAM equality and
allocation/deallocation guards. Existing Piano Uno tests also ran successfully.

Evidence and a playable demo:
`app/build/verification/plain-xml-playback-2026-11-07/report.json`.
The demo's 48 kHz stereo render is identical between streamed and resident CLI
paths, with measured sustain pitch approximately 440 Hz.

```sh
cargo test -p nk-format -p sample-adapter --offline
cargo test -p plugin-sandbox --test plain --offline
cargo run -p sample-adapter --bin sample-host -- /path/instrument.nki /tmp/xml.wav
```

Standalone/AU/VST3/scanner/preview/test targets are rebuilt locally. Full worker
IPC/CTest/Logic validation is still blocked by the shell's shared-memory
restriction, as recorded in the preceding CLAP report. No installed AU or Logic
project was changed. This pass does not resume deferred large-library testing.
