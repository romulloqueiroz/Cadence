# Legacy one/two-pole filters and SV1

Implemented 2026-11-07. Six serialized filter models now use recovered family
control laws and state recurrences instead of the generic 20 Hz–20 kHz/RBJ
approximation. These cover every group filter in the installed Piano Uno
Concert preset. Later [higher-order/band/legacy-ladder work](kontakt-higher-band-ladder-filters.md)
replaces seven more approximations; the explicit inventory now contains thirteen models.
The later [control timing increment](kontakt-filter-control-timing.md) adds
32-frame updates, SV1 coefficient ramps and legacy parameter slew for these
six models; its timing/lifecycle notes supersede the original cache policy.

## Supported models and recovered evidence

All evidence is saved Kontakt 8.14.1 ARM64 Ghidra pseudocode in
`../../Kontakt-8-REA/pseudocode/functions/`. Provider limitations in those files
still apply. This work did not execute Kontakt or capture its rendered audio.

| Serialized ID | Processor | Coefficient evidence |
| --- | --- | --- |
| 2 | Legacy one-pole low-pass | `0x10050d9ac.c` |
| 3 | Legacy one-pole high-pass | `0x100534c34.c` |
| 6 | Legacy two-pole low-pass | `0x100504b98.c` |
| 7 | Legacy two-pole high-pass | `0x1004f6608.c` |
| 50 | SV1 low-pass | `0x1006429fc.c`, `0x100643108.c` |
| 51 | SV1 high-pass | same SV1 core, selected high-pass output |

`BParFXFilter::doRead`, `0x1007829cc.c`, establishes the version-0x92
normalized fields. Its second subtype value overwrites the first; historical
subtype fields need not agree. `fixtypeToType`, `0x1003e1ba0.c`, and
`BGroup::UpdateTemplates`, `0x1003c4f1c.c`, establish the selected processor.
The new decoder retains the normalized cutoff/resonance, rejects non-finite or
truncated/trailing records for these models, and clamps finite controls to 0–1.
Other models continue through the existing support/approximation paths.

## Processing changes

Legacy models use the recovered eighth-degree exponential approximation,
normalization by 388.67681884765625, offset 0.002, and sample-rate factor
`44100 / rate * 0.998004`. Their low/high-pass frequency warps use the recovered
polynomial sine/cosine ratios and model-specific factors (LP 0.99; HP 0.82/0.92),
rather than a common logarithmic Hz range. Prototype constants come from
`Initialize`, `0x10050d6d0.c`, `0x1004dc22c.c`, and
`Calc1PoleUnityFilter`, `0x10061f65c.c`.

Two-pole resonance uses `1 - (1 - resonance)^2`, changes the prototype feedback,
and applies the recovered resonance-dependent output compensation. It no
longer becomes a generic Q from 0.707 to 10. One-pole resonance is inert in
these recovered cores. Legacy processing retains input/output histories as in
`0x10050d820.c` and `0x100622448.c`.

SV1 uses pitch coordinate `115 * cutoff + 20`, the interpolated exponential
lookup, and the recovered fifth-degree tangent approximation. The lookup is
constructed from `exp2(i / 60 - 20)`; its initializer is `0x101c02f98.c`.
Initialization happens during loading, outside the callback. SV1 retains its
native integrator charge when cutoff changes, rather than reusing transposed
biquad delays with a different coefficient mapping.

Group slots keep independent per-voice state at their original slot addresses.
Indexed cutoff offsets add directly to normalized cutoff before family mapping.
Program insert, send-return and master slots share the same six processors,
including their common wet/dry gains. Bypass and ordered slot gains retain the
existing routing behavior. Fixed-size state and lookup storage keep callbacks
and reset free of allocations/deallocations. Control updates now follow
[the recovered timing rules](kontakt-filter-control-timing.md), including the
legacy 1e-6 coefficient guard and SV1 interpolation. Reset/new voices start
with cleared histories.

The existing unindexed XML/internal modulation convention still uses octave
offsets. Cadence applies these to the legacy frequency warp or SV1's pitch
coordinate, keeping both within native parameter bounds. This is a compatibility design choice;
full recovered internal-object routing remains open.

## Verification

Six new regressions cover record framing and effective subtype, independent
frequency responses, SV1 charge across cutoff changes, program insert/send/
master wet/dry routing, reset and sample-rate changes, extreme settings, invalid
rate recovery, and exact stereo output across 1/64/257/1024-frame callbacks with
resident and streamed samples. Callback/reset allocation and deallocation
checks pass, including a smoothed indexed SV1 cutoff sweep.

The independent Python reference substitutes a complex frequency into the
warped prototype transfer; it does not import the Rust coefficient expansion
or sample processor. SV1 is checked against its solved integrator transfer.
640 complex response points cover six models, four rates, four cutoff values,
and additional resonance settings for two-pole models. Impulse responses use
65,536 samples per case. Ordinary cases require complex error below
`0.0001 + 0.001 * reference magnitude`; the ill-conditioned zero-cutoff legacy
two-pole corner has an explicit absolute tolerance of 0.02 because float
feedback-sum subtraction loses precision. Maximum observed error is 0.01610 in
that corner. These are static numerical regressions, not native audio captures
or a bit-for-bit parity claim.

The full sampler/format suites pass 188 tests. Installed Piano Concert playback,
conformance and streamed/RAM equality pass; streaming sample error is exactly
zero with no underruns. The optimized paced run completes 600 callbacks at
256 frames / 48 kHz; the slowest observed callback is 823 us against a 5,333 us
budget. This finite local measurement is not a hard realtime guarantee. Timing
and the ten-second stereo render are retained in the report below. Debug
callback measurements may exceed their 5.33 ms budget.

Workspace Clippy with warnings denied, formatting and standalone/AU/VST3/test
builds pass. Built bundles pass signature verification. CTest cannot describe
its loading fixture because sandbox shared-memory creation is denied; its first
attempt also needed a writable test temporary directory. Installed bundles
were not replaced. No live Logic or Kontakt comparison was performed.

Artifacts, logs, hashes, inventory, reference generator and render:
`../app/build/verification/filter-fidelity-2026-11-07/`.

## Remaining filter work

- [Control timing and SV1 smoothing](kontakt-filter-control-timing.md) are now
  implemented for these six models. Native lifecycle/constant-buffer flags,
  base-cutoff bias and runtime timing corroboration remain open.
- [Legacy BP2, LP/HP4, BP/BR4, LP6 and original ladder](kontakt-higher-band-ladder-filters.md)
  now have recovered playback paths. Modern ladder/SV and other models remain open.
- Exact native floating-point instruction ordering, SIMD behavior and dynamic
  parameter/bypass/state automation. One-pole coefficient normalization is
  algebraically simplified; the numerical reference is not proof of identical
  rounding to the native processor.
- Group pre/post-amplifier placement, arbitrary internal object targets,
  resonance/drive modulation and wider supported-version coverage.
- Independent Kontakt renders and end-to-end DAW recall/automation comparisons.
