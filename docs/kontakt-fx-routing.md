# Kontakt FX routing: first correction

Implemented 2026-11-07. This covers ordering and program-chain identity for
currently supported effects, not full Kontakt routing or DSP parity.

## Evidence and placement

The saved Kontakt 8.14.1 ARM64 pseudocode establishes these static facts:

- `BDsp::HandleVoiceEfx` (`0x1003aa3a8`) walks eight effect pointers owned by
  the voice, in slot order. `BDsp::ProcessVoice` (`0x10046772c`) calls it during
  voice rendering. Group inserts are per voice; moving the shaper to a summed
  group bus would change nonlinear polyphonic behavior.
- `BProgram::doReadChildren` (`0x1007b95ac`) reads the first program effect
  array at `program + 0x1f2b8`, then the send array at `+0x1f430`. Versions
  `0xae..=0xb5` also read the master array at `+0x1f660`.
- `BProgram::UpdateTemplatesIFX` (`0x1003cd404`) uses the insert bus at
  `+0x1f1e0`; `UpdateTemplatesMFX` (`0x1003d0228`) uses `+0x1f588`.
  `BInsertBus::doReadChildren` (`0x1007bef9c`) reads the bus's array at
  `bus + 0xd8`, confirming the insert/master offsets above.
- `BProgram::ApplySendFX` (`0x10042b204`) processes each send effect using its
  own input buffer. These are parallel returns, not a serial chain on dry audio.

These are saved decompiler outputs under `../../Kontakt-8-REA/pseudocode/functions/`,
not original source or fresh runtime observations. Provider limitations remain;
this change does not establish audible equality with Kontakt.

## Implemented behavior

- Supported group filter, spread and shaper slots compile into one ordered
  list. Repeated effect types, bypass and each supported slot's output gain
  are respected. Previously, all filters ran first, followed by only the first
  spread and first shaper, and group slot gains were lost.
- Filter state remains independent per voice and filter stage. Storage covers
  eight six-pole slots plus the legacy XML filter (25 stereo biquad states).
  More than eight group slots fails during preparation of the loaded model,
  before callbacks can index beyond that storage.
- Program inserts process the dry mix, followed by the master inserts.
  Send-array slots never process dry audio. [Parallel returns and IRC](kontakt-aux-convolution.md)
  are now implemented for the supported settings.
- Empty program arrays retain their positions. Malformed program arrays now
  fail loading instead of disappearing and shifting the later arrays' roles.
- Construction allocates off the callback. Rendering and reset use bounded
  storage and retain sample-offset event handling.

## Validation

Seven new regressions cover program chain roles, malformed/empty arrays, group
slot order/repetition/bypass/gains, filter state/maximum legal chain, per-voice
polyphonic shaping, block-size invariance, reset and zero callback allocations
or deallocations. The sampler and format suites pass; workspace Clippy with
warnings denied passes; standalone, AU, VST3 and integration-test builds pass.

Validation logs and a machine-readable report are in
`../app/build/verification/fx-routing-2026-11-07/`.

The full workspace tests encounter `shm_open` permission errors in this managed
sandbox. Host integration validation is reported separately in the evidence
report, not counted as a pass.

The initial routing correction exposed a real Piano Uno Concert load error
in `Modulation::new`: Key Noise / filterCutoff / unsupported source 7.
This is now resolved by [key position and indexed cutoff routing](kontakt-key-position-filter-routing.md).
All four installed-Piano tests now fail on load errors and execute successfully,
including streaming/resident equality and paced realtime callbacks.

## Remaining work

- [Send taps, parallel returns and IR loading/convolution](kontakt-aux-convolution.md)
  are implemented; custom IR shaping, sidechain/feedback and wider routing remain.
- Group insert placement around the amplifier is now recovered and implemented,
  including all eight amp models and phase-aware send taps. See
  `group-fx-runtime.md`. Wider bus selection and output partitions remain open.
- Thirteen filter models and all three shaper modes now use recovered paths:
  [filters](kontakt-higher-band-ladder-filters.md), [shapers](kontakt-shaper-types.md).
  Other filter families, spread/SSL EQ models and ignored controls remain open.
- Internal/other effect-parameter object modulation, unknown-effect diagnostics, dynamic FX
  controls, dry/wet controls, bypass tails, FX parameter state recall and
  sample-rate/control-raster parity remain separate tasks.
- No installed plugin was replaced by this increment. Rebuilt artifacts are
  under `app/build/Cadence_artefacts/Debug/`; real DAW validation remains open.

## Group amplifier integration update (2026-11-11)

`Group::fx_amp_split_point` now retains the binary/XML amplifier boundary with
native default6. Physical slots0..split precede volume/envelope/pan/fades;
remaining slots follow them. All eight recovered amps have independent
per-voice states, prepared off callback and reused without allocation. Pre/post
send taps retain their exact location. See `group-fx-runtime.md` for validation
and `group-fx-native-placement-notes.md` for original-artifact evidence.
