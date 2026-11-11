# JUMP runtime port

Type 77 now uses the recovered JUMP circuit in `jump_dsp.rs`, replacing the
previous approximate cascade. `amp_dsp.rs` retains the curves, table setup,
setters and rate coefficients. `jump_controls.rs` assembles the native control
targets, table metadata and operating point.

The circuit processes four internal phases per host frame with the native
four-phase input FIR and 16-tap output FIR. Both four-lane filter banks,
eight nonlinear states over six curves, delayed tone/feedback connections,
power-supply oscillator/integrators and voltage smoothing are translated in
native order. Index clamping preserves endpoint-segment extrapolation.

The saved wire controls map to SetJump arguments `[0,1,5,4,3,2]`: wire field 2
is level, fields 5/4/3 are the tone controls. Flag 0 selects boost and flag 1
selects mono downmix. The two flags now reach those circuit paths. Preparing
or resetting restores the native nonzero operating point instead of zeroing
all nonlinear states.

Control changes through `JumpAmp::set_settings` retain the circuit state,
prepare targets off the audio thread, and fade at 32-host-frame boundaries
with four current updates per frame. The drive-dependent coefficients are
rebuilt from the interpolated current on every internal phase while fading.
Existing rack/sample-host live FX event dispatch is separate; this does not
add a new DAW automation interface for individual amp controls. The common
FX slot wet/dry/output routing stays in `FxNode`.

## Evidence and numerical coverage

The native source addresses, hashes, ordered recurrences and resolved base
virtual dispatch are in `kontakt-jump-scalar-graph.md`. An independent Python
replay produces `tests/fixtures/jump-process.bin`: three explicit snapshots
at 44.1/48/96 kHz with 2,048 input/output frames each and 72 terminal states.
All 6,144 output frames match exactly in the Rust scalar implementation.
These snapshots deliberately include asymmetric bias values to exercise both
power-table paths; they are not claims about a native preset's defaults.

Additional tests cover table extrapolation, prepared-state repeatability,
finite output over 8–192 kHz and control endpoints, mono cancellation,
stereo state independence, control fading and zero callback allocations,
including coefficient rebuilds. The bulk rate fixture still covers 4,396
values across 28 cases. Provenance and generator scripts accompany fixtures.

The scalar implementation uses ordinary float division in place of NEON
reciprocal estimates/refinements, and does not force native instruction FMA
contraction. Static scalar replay therefore cannot establish native bit parity.

## Native audio validation

Native capture work and identity-checked saved states are retained under
`app/build/verification/native-jump-2026-11-08/`. The capture host accepts an
explicit `--allow-preroll` option because JUMP can emit a small circuit/reset
transient before the test note. It records that exception and the measured
preroll peak; its normal silence gate remains enabled by default.

The ignored `jump_render_native_dry_capture` test is an offline analysis
utility, run only with absolute `CADENCE_JUMP_NATIVE_INPUT` and
`CADENCE_JUMP_PROBE_OUTPUT` paths. It feeds the actual native dry recording
through the Rust amp, avoiding the sampler reimplementation as a confounder.
Optional input/output scales are recorded and must not be mistaken for raw
unmodified comparisons. `scripts/compare-kontakt-audio.py` reports raw,
delay-aligned and gain-fitted comparisons separately.

The accepted pair is `native-unity-dry/` plus `native-unity-jump/`: 48 kHz,
256-frame host blocks, note 60/velocity 100 at frame 4800 and note-off at
292800. Feeding the native dry take through `JumpAmp` isolates the amplifier
and matches the native wet take at zero delay.

- Frames 0..293376 (the region the native host actually rendered): RMS error
  7.71e-6, peak error 1.20e-5, SNR 81.66 dB, correlation 0.9999999966.
- Frames 293279..293376 (97 silent-input frames where both engines continue
  from state): RMS error 1.38e-8, peak error 7.96e-8.

Kontakt stops rendering the insert chain at the first fully silent host block.
The dry take's last nonzero frame is 293278 (block 1145), and the wet take's
last nonzero frame is 293375, the end of that same block. From frame 293376
(block 1146) the native wet output is exactly zero. The Rust render instead
carries the circuit's genuine ~340 ms decay tail to frame 309654, so a
full-length raw comparison reports only 59.67 dB. That gap is the host's
silent-block skip, not an amplifier difference, and it means tail parity is
not observable from this capture. The ignored probe renders every frame and
does not emulate the skip.

`scripts/compare-kontakt-audio.py` now accepts `--start-frame`/`--end-frame`
and `--allow-preroll` so the processed window, including the permitted reset
preroll, can be recorded explicitly. The result is
`native-unity-idle-windowed-comparison.json` beside the Rust render. Native
wet-vs-wet repeatability is SNR 93.66 dB, so the remaining windowed error is
consistent with scalar-vs-NEON rounding rather than a topology, coefficient or
control-target error. This is a strong numerical agreement, not a bit-exact
parity claim.
