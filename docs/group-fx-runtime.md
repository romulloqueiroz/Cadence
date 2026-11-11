# Group amps and amplifier placement

Implemented 2026-11-11. The sampler now renders JUMP, VAN51, ACBox, HotSolo, CAT,
BassPro, BassInvader and EPPreamps in group insert slots, independently per voice.
Program insert, master and parallel-return behavior is retained.

## Physical chain and amplifier boundary

`Group::fx_amp_split_point` decodes the binary public field and XML
`fxIdxAmpSplitPoint`. Its native default is 6; valid values are 0..8. Malformed
values are rejected. Empty and bypassed slots still count toward the boundary.

The order is:

1. Source sample and physical group slots below the boundary.
2. Voice/zone/selection gain, amplitude envelope and volume modulation, pan,
   voice-kill and all-sound-off fades.
3. Remaining physical group slots.
4. Voice summation into program buses; program inserts, returns and master FX.

A group send taps the current signal at its exact slot. Pre-amplifier sends
bypass the amplifier's gain/envelope/pan/fade; post-amplifier sends already
contain them. Applying those controls to every tap again would be incorrect.
The user-facing sampler master gain remains outside the group nonlinear drive.
Legacy XML filter parameters retain their existing pre-chain position.

Original source/instruction evidence, including the default and serialization
layout, is recorded in `group-fx-native-placement-notes.md`. Full native
modulation interpolation, arbitrary bus/output matrices and sidechains are
separate from this boundary implementation.

## Voice state and realtime behavior

`group_amp.rs` owns prepared images and persistent circuit banks. Templates are
deduplicated by saved amp settings; banks are keyed by amp kind and that kind's
occurrence within a chain. Each bank reserves only the configured voice capacity.
Consequently many groups can reuse the same bank instead of allocating a full
voice pool for every group.

Voice start/steal copies a prepared numerical image into an existing bank slot.
Audio rendering, voice activation, voice retirement and reset neither allocate
nor destroy heap owners. Multiple amps of the same kind in one chain have
separate banks; simultaneous voices never share circuit histories. Preparation
and sample-rate changes rebuild storage off callback. Invalid rates silence amp
routes until a valid preparation and activation.

EPPreamps' temporary numerical arena is bounded to 64 KiB, reducing a 64-voice
EP bank by 60 MiB compared with the previous 1 MiB-per-instance reservation. Native
core/wrapper, endpoint and dense-matrix fixtures continue to pass. Representative
64-voice bank storage is approximately 0.9 MiB JUMP, 2.5 MiB VAN51, 1.0 MiB ACBox,
7.5 MiB HotSolo, 17.9 MiB BassInvader, 0.08 MiB CAT, 0.8 MiB BassPro and 15.4 MiB EP.
Prepared templates and chain metadata are additional and depend on the preset.

## Verification

Focused regressions cover all eight models, physical splits 0/6/8, sparse slots,
bypass, mixed filter/shaper/amp ordering, pre/post send taps, independent nonlinear
polyphony, shared-bank template changes, stealing/reset, callback partitioning,
sample-rate recovery and zero callback allocations/deallocations. Binary/XML
parser tests also check boundaries and following-field framing.

Original Kontakt captures and Cadence render comparisons are retained under
`app/build/verification/group-amp-native-2026-11-10/`. They use controlled factory
sample references, JUMP in physical slot 3, nonunity group gain/pan, and one/two
simultaneous duplicate zones. Identity readbacks and declared settings are
recorded separately from audio measurements; full host parity is not implied by
state identity alone. Consult the final comparison report for measured scope,
processed windows and remaining sample-playback/tail differences.

Build, test and installation results are recorded in
`app/build/verification/group-amp-port-2026-11-10/report.json`.

## Remaining scope

This adds saved group amp settings and existing supported modulation targets.
New amp-specific external modulation/DAW controls, unsupported non-amp group FX,
additional output routing and complete Kontakt voice-tail lifecycle equivalence
remain separate work. Existing modern-amp 192 kHz buffering and synchronized-speed
limits remain documented in `kontakt-amp-runtime-ports.md`.

## Measured native comparison and completed installation

The corrected shared fixtures are in
`group-amp-native-2026-11-10/piano-controlled/relative/`. Only the donor's orphaned
filter-cutoff route was removed after replacing its filter chain; retained
pitch/velocity-volume routes, internal envelopes and filename table are verified
byte-identical. A local `Content` symlink resolves the same original sample files
for Cadence. Rejected exploratory fixtures are preserved separately.

All six native state readbacks are confirmed and all native repeat pairs are
bit-exact. Both engines' two-zone renders equal exactly twice their single-zone
renders, across all frames, in dry/pre/post cases. The declared doubling is a
polyphony prediction, not a fitted gain.

Raw native-versus-Cadence SNR is 54.03 dB dry, 42.60 dB with JUMP before the amplifier,
and 42.63 dB afterward, for either voice count. These residual waveform differences
mean full host parity is not established. No gain/delay fit is applied, and a
held-note window is reported separately. Native and Cadence pre/post outputs
are distinct. `scripts/compare-group-amp-captures.py` reproduces the report.

Final checks: 317 tests pass, 4 explicit diagnostic probes ignored; workspace
all-target Clippy and rustfmt checks pass. Packaged build and scan/bridge CTest
pass. Updated AU/VST3 bundles are installed under the user's Audio/Plug-Ins
directories; each installed file matches its built bundle and code signatures
verify. The prior installation is backed up in
`app/build/verification/plugin-install-20261111T030322Z`.
