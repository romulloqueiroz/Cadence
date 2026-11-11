# Group filter basics

Implemented 2026-11-07 as the first stage-5 increment of the
[Kontakt compatibility backlog](kontakt-compatibility-roadmap.md).
Later [filter fidelity work](kontakt-filter-fidelity.md) replaces the generic
binary cutoff/Q mapping for legacy LP/HP one/two-pole and SV1 (six models),
including every group filter in the installed Piano Uno Concert preset.
Other binary families retain the approximations described below.

## Playback behavior

Cadence now has a per-voice biquad path:

- XML group parameters `filterCutoff` (Hz, 10–24000), `filterQ` (0.1–20),
  `filterType` (`lowpass`/`highpass`) and `filterBypass`.
- RBJ low-pass/high-pass coefficients, run as two independent transposed
  direct-form-II states for the left and right channels.
- The cutoff is `base * 2^modulation_octaves`, clamped to 10 Hz and
  `0.45 * sample_rate`. Coefficients are recomputed when the modulation moves
  by more than 0.01 octave and then cached across samples in the segment.
- Cutoff modulation sources:
  - external MIDI routes (`filterCutoff` targets from bend/pressure/CC/velocity),
  - internal AHDSR envelopes and LFOs that target `filterCutoff`.
  Both use the existing route depth as octaves.

Binary group/instrument slots also reach playback as described below. See
[the routing correction](kontakt-fx-routing.md) for current ordering and limits.

## Binary FX graph evidence

- `BGroup::ResetParameters` (`0x1003fd448`) clears
  `BParameterArray<BParFX, 8>` at `group + 0x8b0`: every group has eight FX
  slots.
- `BParameterArraySer<BParFX, 8>::doRead` (`0x1004e5e68`) reads the slot array:
  version `< 0x13` implies eight slots; newer versions read an explicit count,
  then for each slot a presence flag and a serialized `BParFX` object.
- `BParFX::PrsSetValues` (`0x1004d18f0`) maps XML FX types; type `0x19` is
  `BParFXFilter`, and `BParFXFilter::doRead` (`0x1007829cc`) carries the filter
  subtype/cutoff/resonance fields.
- The factory Piano Uno program's `PARAMETER_ARRAY_8` starts with version
  `0x13`, count `8`, slot 0 present, and type `0x25` (Cabinet), confirming the
  array framing.
- The common `BParFX` wire header is decoded (`SER::Read` type/size,
  structured flag, version `0x50`, private/common fields, children), and Cadence
  parses program-level `PARAMETER_ARRAY_8` chains into `Instrument.fx_chains`.
- Version-0x92 filter children store two subtype fix IDs followed by one
  `(frequency, bandwidth_octaves, gain_db)` triplet per active band. Factory
  Piano Uno slot 4 is subtype `0x61` with three identical flat bands
  (632.4 Hz, 1.666 octaves, 0 dB), so it is decoded and applied as a flat
  3-band peaking EQ.
- The post-mix chain also applies:
  - `BParFXStereoSpread` (ser 31, v0x70): `f32 spread`, `f32 pan`,
    `bool pseudo`; width = `1 + spread`, then pan.
  - `BParFXLimiter` (ser 28, v0x60): `f32 inGain` (dB), `f32 release` (ms);
    a peak limiter at 0 dBFS with instant attack and the configured release.
  - Each slot's common `outLevel` linear gain.
- The previous post-mix implementation flattened factory Piano Uno arrays
  into filter → stereo spread → EQ → IRC/sends → limiter. That flattening is
  now removed; insert and master arrays surround the parallel returns.
  The limiter's `-15 dB` input gain and `3.1748`
  (`+10 dB`) output level are applied. [Send levels and IRC convolution](kontakt-aux-convolution.md)
  now reach playback for the supported settings.
- The group-level eight-slot FX array is found in the group private data at the
  `13 00 08 00 00 00` array header (offset 1657 in factory Piano groups). It is
  parsed into `Group.fx_chain`.
- Group filters use a 16-byte version-0x92 payload: two fix IDs, a normalized
  cutoff and resonance. Fix IDs map to `lp1pole`/`hp1pole` (2/3),
  `bp2pole` (4), `lp2pole`/`hp2pole` (6/7), `lp4pole`/`hp4pole`/`bp4pole`/
  `br4pole` (8–11), `lp6pole` (12) and `ladder` (17). Cutoff is mapped
  exponentially to 20 Hz–20 kHz and applied per voice with the same
  `filterCutoff` modulation as the XML filter.
- Decoded factory Piano group filters include Dry Tones low-pass/high-pass,
  Key Noise high-pass+low-pass, Release/Sustain high-pass and Pedal low-pass.
  Key-position/indexed cutoff routes and the previously skipped SV1 low-pass
  (subtype 50) now play; Piano Uno loads and passes its real-library tests. See
  [key-position routing](kontakt-key-position-filter-routing.md).
- Group stereo spread is applied per voice in slot order (M/S width and pan).
  Shaper slots (`ser 29`, v0x80) now use recovered Classic/Drums/Enhanced paths;
  see [shaper types](kontakt-shaper-types.md).
- Program `BParFXSSLGEQ` (ser 68, v0x11) now uses the recovered `G_EQ` control
  curves: low/high bands are shelves, the two mid bands are peaking, and gains,
  frequencies and Q are mapped through the native eleven-point curves. See
  [SSL G EQ control curves](kontakt-sslgeq-control-curves.md). Send taps,
  parallel returns and IRC now have playback paths; see
  [aux/convolution support](kontakt-aux-convolution.md).

## Limits

- Supported effects have playback paths, but full native parity is not
  established. Thirteen filter models now use recovered control laws/topologies;
  see the [current filter inventory](kontakt-higher-band-ladder-filters.md).
  SSL EQ controls/mappings and other filter families remain incomplete.
- Supported group slots retain order, repeats, bypass and output gain; group
  pre/post-amplifier placement and internal/other effect-parameter object
  modulation remain open. External indexed cutoff targeting is implemented.
- Program insert/master roles are preserved; send slots process independent
  aux inputs and mix wet returns before master FX.
- Stereo/linked filter modes, drive, per-sample control raster, broader FX,
  dynamic controls, tails, sample-rate switching and FX state recall need
  separate validation. See [routing details](kontakt-fx-routing.md).

## Evidence

- `../../Kontakt-8-REA/pseudocode/functions/0x1003fd448.c`
  (`BGroup::ResetParameters`, eight-slot FX array)
- `../../Kontakt-8-REA/pseudocode/functions/0x1004e5e68.c`
  (`BParameterArraySer<BParFX, 8>::doRead`)
- `../../Kontakt-8-REA/pseudocode/functions/0x1004d18f0.c`
  (`BParFX::PrsSetValues`, type mapping)
- `../../Kontakt-8-REA/pseudocode/functions/0x1007829cc.c`
  (`BParFXFilter::doRead`)
