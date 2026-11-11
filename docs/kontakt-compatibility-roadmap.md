# Cadence Kontakt compatibility backlog

Created 2026-11-07 from the saved Kontakt 8 ARM64 decompilation and current
`nk-format` / `sample-adapter` source. This is the implementation order. Each
stage has explicit supported behavior, independent audio regressions, realtime
allocation checks where relevant, and documented limits before completion.
Saved pseudocode is evidence for behavior, not original source or a guarantee
of runtime parity. Smaller stages may complete independently within an area.

## Baseline already implemented

- [x] Plain K2 XML and binary NI program/group/zone loading; WAV/NCW samples.
- [x] Background sample streaming, bounded cache and inspector telemetry.
- [x] Gain/pan/tune, reverse, basic forward/ping-pong loops, amplitude AHDSR.
- [x] Key/velocity crossfades; round robin, random, controller and latched
  keyswitch group starts; sustain-pedal deferral and release layers.
- [x] Retail preset/sample decoding for the validated `0x100` libraries.
- [x] Cadence Rust worker integration, JUCE app/AU/VST3 shell and native hosting.

## Ordered work

### 1. MIDI note identity and expression

- [x] Channel-aware, bounded note lifetimes; repeated attacks;
  deferred release identity; panic cleanup and malformed-event handling.
  See [implementation and validation](midi-note-identity.md).
- [x] Decode external modulation routes from binary and XML presets, retaining
  source, depth, polarity, shaping, smoothing and initial-value semantics.
  See [MIDI expression and bounded playback support](midi-expression.md).
- [x] Pitch bend, channel/polyphonic pressure and MIDI CC routes affect voices
  where routed to pitch/pan/volume, with initial/live channel state and native
  per-key/group pressure flags, and matching-voice pressure inheritance at
  attack. Native MIDI/UMP normalization and exact 32-frame smoothing remain
  fidelity follow-ups.
- [x] Release velocity is tracked per original onset and fed to the
  release-velocity source (runtime ID 9) only at termination; note-on velocity
  zero synthesizes the 0x8000 internal value (~0.5). Volume routes are linear in
  release velocity, unlike the cubed attack-velocity source. CC121 has no
  generated-script handling and is delivered as an ordinary controller (no
  reset policy). CC120 now performs the recovered 10 ms fade before discarding
  events. See [MIDI expression](midi-expression.md) and
  [note lifetimes](midi-note-identity.md).
- [ ] Fidelity follow-ups: native MIDI/UMP normalization, exact 32-frame
  smoothing, and independent runtime corroboration of CC120/CC123 pedal-layer
  interaction.
- [x] Tests: equal pitches on different channels, sustained/retriggered notes,
  routed expression before/during notes, reset, block-size independence,
  streamed/resident equality, and no callback allocations/deallocations.

### 2. Complete envelope and modulation primitives

- [x] Curved AHDSR attack using the recovered segment-curvature law, plus
  retained/applied amplitude-envelope intensity. See
  [envelope curves and LFOs](kontakt-envelope-curves.md).
- [x] Per-voice free-running internal sine/rectangle/triangle/sawtooth LFOs to
  volume, pan and pitch from `BParLFO` binary payloads.
- [x] Pitch/pan envelope targets, multiple targets per modulator and partial
  amplitude-envelope intensity (XML and binary AHDSR).
- [ ] Filter-cutoff envelope targets, multiple amplitude envelopes,
  `ENV_DBD`/`ENV_FM7`, and per-target envelope curvature.
- [x] XML internal-LFO parsing (`K2_IntMod/LFO`) sharing the binary playback
  path.
- [ ] LFO tempo sync, note-value rates, random/multi playback and native dB
  intensity units.
- [ ] Validate analytic cases and recovered behavior independently of the
  implementation, including real-library intensity defaults.

### 3. Voice groups and group routing

- [x] Decode/play group MIDI channel: binary `All` is `-1`, XML `All` is `0`,
  and `1..=16` selects a one-based MIDI channel; non-matching groups are
  filtered before zone selection or release-layer spawning.
- [x] Voice-group membership, per-group voice limits, kill modes, released-voice
  preference, fade-out and exclusion groups from `VOICE_GROUPS`/`BVoiceLimit`.
  See [voice groups](kontakt-voice-groups.md).
- [ ] KSP/Script-driven voice-group changes (`IO_SetVoiceGroup`), exact
  `kill_any` choice and pool-exhaustion fade behavior.
- [ ] Replace the remaining binary pedal display-name routing heuristic.
  Release groups now use the serialized release-trigger flag.
  with recovered metadata. Factory Piano groups named `Release`/`Pedal` have
  `releaseTrigger=false`, and the monophonic byte is true for every analysed
  group, so name routing is retained pending better evidence. See
  [voice groups](kontakt-voice-groups.md).
- [ ] Resolve group-start expression precedence against the KSP parser.

### 4. Advanced loops and sample interpolation

- [x] Finite loop counts, loop retuning and forward-loop crossfades, with XML
  and binary fields. See
  [loops and interpolation](kontakt-loops-and-interpolation.md).
- [x] Interpolation quality: recovered `normal` 4-point Lagrange; `high`
  Catmull-Rom and `ultra` 6-point Lagrange remain approximations of native
  windowed-sinc modes. Streaming falls back to linear when outer taps are
  unavailable. Normal playback-position arithmetic and native edge handling
  remain to be matched.
- [x] Sustain/release loop switching and a 6-point `ultra` kernel; ping-pong
  turnarounds are value-continuous and need no crossfade.
- [ ] Additional loop entries beyond the first sustain/release pair, exact
  native high/ultra kernels, and wider boundary/high-transposition parity.

### 5. Kontakt filters and effects

- [x] Decode program-level FX chains: `PARAMETER_ARRAY_8` -> eight `BParFX`
  slots, common fields, and type-specific child payloads. Preserve insert,
  parallel-send and master array roles; malformed arrays are load errors.
- [x] Program insert/master playback for binary three-band EQ, stereo spread,
  peak limiter, four-band SSLGEQ approximation and supported slot output gains.
- [x] Decode group FX slots; apply supported LP/HP/BP/notch filters, spread and
  shaper per voice, preserving slot order, repeated instances, bypass and
  output gains. See [routing correction and evidence](kontakt-fx-routing.md).
- [x] XML group low-pass/high-pass with external/internal cutoff modulation.
  See [filter basics](kontakt-filter-basics.md).
- [x] Ordered group/program send taps, eight parallel returns and program
  insert/master/send IRC with bounded resource loading, wet/dry gains,
  predelay/reverse/auto-gain, tail reporting and allocation-free callbacks.
  See [supported settings and validation](kontakt-aux-convolution.md).
- [ ] Custom IRC ER/LR stretching/EQ, active envelopes, native rate-reduction
  and control/kernel interpolation; sidechain sends, feedback, other output
  partitions, per-voice IRC and dynamic send modulation remain open.
- [ ] Exact filter subtype/response matrix, remaining SSLGEQ input/output/mode
  fields, spread modes and limiter behavior. The four SSLGEQ bands now use the
  recovered control curves and shelf topology; see
  [SSL G EQ control curves](kontakt-sslgeq-control-curves.md).
- [x] Recovered legacy LP/HP one/two-pole and SV1 normalized control laws,
  prototype responses, resonance compensation and persistent family state;
  group and program insert/send/master playback. These cover Concert's group
  filter inventory. See [six-model filter increment](kontakt-filter-fidelity.md).
- [x] Persistent 32-frame control timing for the six recovered models, SV1's
  quantized 2 ms coefficient ramps/repeated-target handling and supported legacy
  cutoff slew/resonance lag. See [timing and bounds](kontakt-filter-control-timing.md).
- [x] Legacy BP2, LP/HP4, BP/BR4, LP6 and original ladder (IDs 4/8/9/10/11/12/17):
  initialized prototypes, distinct cascade/parallel topology, native compression
  placement, mixed/double precision where recovered and two-pass nonlinear
  ladder processing. Thirteen explicit models now play in group and program
  insert/send/master slots. See [inventory and validation](kontakt-higher-band-ladder-filters.md).
- [x] Classic/Drums/Enhanced shaper v0x80: recovered bipolar transfers, exact
  tables, linked stereo envelope/gain and shape smoothing, original slot/voice
  state and program insert/send/master playback. See [evidence and limits](kontakt-shaper-types.md).
- [ ] Native lifecycle/constant-buffer flags, base-cutoff bias, remaining
  modern/higher-order filter families and independent Kontakt audio comparisons.
- [x] Key-position modulation and external cutoff routing to the original
  binary filter slot; SV1 low/high subtype playback and strict installed-Piano
  load/audio tests. See [implementation and evidence](kontakt-key-position-filter-routing.md).
- [ ] Group pre/post-amplifier placement, internal modulator object routing,
  other effect-parameter modulation, output partitions, dry/wet controls and
  per-sample/control-rate parity.
- [ ] Wider dynamics, distortion/tape, delay/modulation and reverb models as
  enumerated in [the FX inventory](kontakt-effect-inventory.md); Gainer and
  Inverter are implemented.
  separate verified increments with an explicit supported-model inventory.
- [ ] Match dynamic bypass, tails, sample-rate changes and FX parameter recall.
- [x] Restore real Piano Uno loading past its unsupported filter-cutoff source
  and fail installed-Piano tests on load errors; playback, conformance, streaming
  parity and paced callback tests now execute and pass.

### 6. KSP runtime and scripted routing

- [ ] Decode script slots/source or compiled representation and persistence.
- [ ] Parser/interpreter: values, arrays, expressions, statements and functions.
- [ ] Bounded callback scheduling, event IDs, waits, note/release/controller
  callbacks and group/event operations connected to the sampler.
- [ ] Implement required engine-parameter and modulation operations.
- [ ] Add compatibility diagnostics for unsupported commands; validate real
  library scripts as well as independently authored regression scripts.

### 7. Scripted instrument interfaces

- [ ] Decode resources and script controls, layout, images and persistence.
- [ ] Connect control callbacks and parameters through the existing Rust/UI
  bridge; exercise state recall and DAW automation.

### 8. Advanced playback engines

- [ ] Time Machine variants and time/pitch independence.
- [ ] Tone Machine/formant behavior and wavetable playback.
- [ ] Trace Beat Machine beyond tempo-sync entry points before specifying it.
- [ ] Validate each engine's routing, modulation, streaming and CPU behavior.

### 9. Multis and channel/output routing

- [ ] Full multi-program XML/binary playback, slot MIDI routing and outputs.
  Binary slot parsing currently expands metadata; the playable model selects
  its first program, which is not complete multi support.
- [ ] Multi state recall, per-program controllers, effects and resource limits.

### 10. Remaining containers and sample modes

- [ ] `.nkc` collections and embedded NKI program extraction from `.nkr`.
- [ ] Sample encryption modes other than `0x100` and their format fixtures.
- [ ] Portable AES backend; preserve explicit errors for unavailable keys.
- [ ] Supported path relocation/library references and format-version edges.

### 11. Huge-library streaming

- [ ] Demand-based/shared attack preload policy that fits the resident budget
  for GroveBass (5,742 zones), with deterministic preparation and no file I/O
  or waiting in realtime callbacks.
- [ ] Slow-storage/error recovery and multi-hour stress with representative
  polyphony, transport changes, bounce and recall.

### 12. End-to-end compatibility validation

- [ ] Validate musical output/behavior with representative real libraries,
  extending the compatibility matrix per completed stage.
- [ ] Complete real Logic AU playback, bounce, automation and recall checks.
- [ ] Rebuild and verify standalone/AU/VST3 artifacts after engine changes.
  Distinguish source/tests, built artifacts and installed versions in reports.

## Evidence entry points

- `../../Kontakt-8-REA/README.md` and `combined-functions.tsv`: saved export.
- `kontakt-pedal-release-evidence.md`: note identity and controller semantics.
- `plain-xml-playback.md` and `../crates/nk-format/SPEC.md`: current subset.
- `retail-library-decryption.md`: decoded retail libraries and preload limits.
- `../SESSION-HANDOFF.md`: dated implementation/validation history; newer
  sections supersede historical limits.
