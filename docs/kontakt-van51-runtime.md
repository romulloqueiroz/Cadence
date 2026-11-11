# VAN51 runtime port

Type 82 now uses the recovered VAN51 circuit through `Processor::AmpVan51`.
`van51_dsp.rs` owns initialization, rate coefficients, controls and the native
28-value smoothing image. `van51_process.rs` owns four internal phases per host
frame, the three nonlinear banks, twelve shaping tables, double filters, FIRs
and supply feedback. `van51_amp.rs` connects those parts to playback.

All eight wire floats and five switches retain `BParFX::SetObj` order. The last
switch selects mono downmix/broadcast. Controls fade on a 32-frame clock;
the left channel retains the slice's starting controls while the last channel
advances the shared currents, matching native channel traversal. Processing
and reset allocate nothing. No new DAW automation interface is added.

## Recovered evidence

The saved image is `Kontakt-8-REA/Kontakt-8.arm64`, SHA-256
`42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.
Relevant native entries are Initialize `0x1005cdc34`, SetSampleRate
`0x1005ce690`, Reset `0x1005cf318`, SetVan51 `0x1005d4fe0`, onControlRate
`0x1005d530c` and ProcessInt `0x1005d55bc`.

The saved Ghidra polynomial expressions contain four incorrect SIMD accumulator
aliases. Tone, lead and feedback coefficients therefore use generated ARM64
register/lane SSA with explicit fused operations. Isolated execution of the
original helper instructions supplies the independent goldens: twenty knob/rate
cases include all seven polynomial inputs, plus five bulk-rate cases. The
original Initialize/SetVan51/Reset snapshot independently checks default targets,
including the feedback crossfade target at `+0x9620`.

Reset preserves the per-channel coefficients at `+0x4b4..+0x4bc`, clears the FIR
histories and restores the nonzero supply operating point. The internal clock
uses the native double-precision multiplication by tau. ARM64 supply reciprocals
retain one estimate/refinement; double-to-float filter boundaries retain FCVTXN
round-to-odd behavior. Other architectures use scalar reciprocal division.

Generators, hashes and golden snapshots live under `tests/fixtures/van51*`.
`van51_coefficients_reference.py --rust` regenerates the formatted polynomial
owners and native coefficient goldens on macOS ARM64. The circuit fixture has
four synthetic cases covering both branches, stereo histories, shared smoothing
and mono, plus an initialized case using original ARM64 ProcessInt on ARM64.
Synthetic cases check every terminal history; the initialized case checks audio
and finite histories because its internal terms strongly cancel. The portable
fixture uses compiled pseudocode with explicit round-to-odd narrowing.

## Audio comparison and limits

Artifacts are in `app/build/verification/native-van51-2026-11-09/`.
Both captured native states were checked against their candidates. The default
case uses eight values of 0.5 with all switches off; the second uses varied
values with all switches on, including lead and mono.

Rust versus the **original isolated native circuit**, over frames 0..293376 of
the actual dry recording, has SNR **105.05 dB**, RMS error **1.45e-7**, peak error
**2.55e-6**, zero delay and unit gain. The replay uses original isolated
Initialize/SetVan51/Reset instructions and 32-frame processing slices. It binds
only numerical dependencies; it does not recreate the full plugin lifecycle.

The full Kontakt host capture differs from both implementations: default Rust
comparison **59.51 dB**, default isolated-original comparison **59.51 dB**;
lead/mono Rust comparison **65.41 dB**. The discrepancy is now explained by the observed host entry state, below.
These earlier renders used the isolated initialization lifecycle.

Kontakt activates the insert on the note's host block at frame 4608; the note
starts at 4800. The diagnostic render explicitly defers processing to 4608.
Processing zeros from frame zero instead yields 48.50 dB in the earlier report.
Kontakt also skips the first fully silent block at frame 293376. Comparisons
exclude that unrendered tail; normal Cadence playback retains its full tail.
Host callback splits and live parameter timing beyond these cases remain outside
the established native coverage.

`final-audio-comparisons.json` records raw metrics and hashes. The ignored
`van51_render_native_dry_capture` test reproduces the Rust render with explicit
input/output paths, eight values, five flags and optional start frame. Gain
fitting is diagnostic only and is never used as a parity pass.

## Observed host lifecycle (2026-11-10)

A bounded read-only trace of the installed Kontakt VST3 captures the numerical
object at the first ProcessInt entry and fourteen consecutive 32-frame stereo
input/output slices. The native host retains its 44.1 kHz presence coefficient
when preparation switches to 48 kHz. It also starts three unity currents at zero
and fades them on activation. Cadence's ordinary prepare path uses the requested
rate and its own continuous-processing lifecycle.

Replaying the complete actual dry recording from that observed entry state
through the original native ProcessInt is bit-identical to the Kontakt host
through frame 293376. The same state/input through Rust gives 105.086 dB SNR,
RMS error 1.446e-7 and peak error 2.742e-6, with zero fitted delay and unit gain.
This identifies the earlier 59.51 dB discrepancy as lifecycle state, while retaining
the measured numerical precision limit of the reconstructed circuit.

`van51_matches_observed_kontakt_host_activation` retains the bounded 448-frame
regression, with process-specific pointers removed. The ignored dry render now
accepts `CADENCE_VAN51_PROBE_STATE` for an observed numerical entry image.
Artifacts and raw metrics are in
`app/build/verification/native-van51-host-trace-2026-11-09/observed-host-state-comparison.json`.
The capture session is detached, its evidence is exported, and no native analysis
engine remains active. Lead/mono and other host lifecycle sequences have not
received the same entry-state trace.
