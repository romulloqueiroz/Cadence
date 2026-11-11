# Filter control timing and SV1 smoothing

Implemented 2026-11-08 for the six models in
[the filter fidelity increment](kontakt-filter-fidelity.md): legacy one/two-pole
LP/HP and SV1 LP/HP. Modulation now reaches these processors on persistent
32-frame control boundaries. SV1 interpolates its integrator coefficient;
legacy filters slew normalized parameters before generating coefficients.
Later [higher-order/band/legacy-ladder support](kontakt-higher-band-ladder-filters.md)
adds seven models using the legacy timing path below.

## Recovered evidence

Evidence is the saved Kontakt 8.14.1 ARM64 Ghidra export under
`../../Kontakt-8-REA/pseudocode/functions/`. This is static analysis with the
provider limitations preserved in those files, not execution or a Kontakt
render capture. The legacy Process export has removed-block/provider warnings;
its supported mainline rules are distinguished from unresolved paths below.

| Behavior | Saved function |
| --- | --- |
| SV1 wrapper samples bound modulation, clamps to 0–1 and refreshes every 32 frames | `0x10053012c.c` |
| Initial control period is 32 | `0x10052fe00.c` |
| Cutoff setter computes target coefficient and marks it dirty even for repeated values | `0x1006429fc.c` |
| Ramp ticks are `max(1, floor(rate * 0.002 / 32 + 0.5))`; increment multiplier is `1 / (ticks * 32)` | `0x100642970.c` |
| New targets restart the ramp; squared increments below 1e-15 snap; completed ramps copy the exact target | `0x1006471d8.c` |
| Coefficient increments happen before each audio frame and are shared across channels | `0x100643108.c` |
| SV1 sample-rate update resets integrators; resume restores target coefficients | `0x10052febc.c`, `0x1006429c8.c`, `0x1006429e0.c` |
| Legacy 32-frame updates, cutoff slew and resonance lag | `0x100620000.c` |
| Legacy buffer presence/constant flags and anticipation | `0x1004f47b8.c`, `0x1004f4798.c`, `0x1004ea774.c` |
| Reset zeroes the native control counter | `0x1005bca2c.c` |

## Implemented behavior

Each voice/filter slot owns its control countdown. Program insert/send/master
filters own separate state too. Callback boundaries, aux scratch chunks and
unrelated MIDI events do not restart clocks or coefficient ramps. Group route
presence is retained at compilation so a bound modulation buffer is distinct
from a scalar parameter, including when the route currently evaluates to zero.

SV1 samples moving cutoff on its 32-frame grid, computes the recovered target
coefficient, and linearly increments that coefficient before processing the
next frame's two channels. This avoids channel-dependent ramp advancement.
The nominal 2 ms duration is quantized by control periods:

| Sample rate | Ramp periods | Ramp samples |
| --- | ---: | ---: |
| 8 kHz | 1 | 32 |
| 44.1 kHz | 3 | 96 |
| 48 kHz | 3 | 96 |
| 96 kHz | 6 | 192 |
| 192 kHz | 12 | 384 |

A scalar target change starts one finite ramp. A bound modulation buffer
refreshes at every tick and restarts from the current coefficient, even if the
sampled target is unchanged. It consequently approaches a held target through
repeated shorter advances until the native small-increment threshold snaps it.
A change that starts and ends between control samples is not observed. Audio
integrator charge persists while the coefficient moves.

For the supported legacy models, the normalized cutoff step limit is
`clamp(1 / (current_resonance * 31 + 4), 1/35, 1/4)`. The recovered buffer
mainline caps upward motion and accepts downward changes at the control
boundary. Scalar changes are limited in both directions. Resonance moves by
1.5% of its remaining difference per active control update, with a 0.0001
deadband. Static idle filters preserve their remaining control interval;
bound dynamic modulation keeps the clock advancing. Legacy coefficients
retain their 1e-6 parameter-change guard and change at control boundaries,
rather than receiving SV1's linear coefficient interpolation.

Cold starts/reset retain Cadence's existing policy: start from the first
effective control with clear audio histories, without an artificial startup
sweep. Sample-rate changes re-evaluate the effective coefficient and restart
control timing. SV1 clears integrators on rate changes; legacy audio histories
are retained. Rate-invalid DSP calls clear timing/history state and return zero;
later valid calls recover without allocation. All storage is fixed-size, and
lookup initialization still occurs during loading outside the callback.

## Verification

Five new regressions cover the independent time-domain reference, exact finite
ramp endpoints, repeated-target restarts, tiny-step snaps, upward/downward
legacy changes, resonance lag, idle clocks, reset/rate/model changes, and a
routed CC change between event/aux/host boundaries. The previous integrator
regression now checks the last held frame and first ramp frame explicitly.

The independent Python generator selects controls on a timeline and evaluates
ramps in closed form; it does not import the Rust state machine. There are
2,660 checked coefficient/audio points over five rates, scalar/buffer modes,
reversals, off-grid changes and an unsampled short pulse. Audio tolerance is
1e-5; coefficient tolerance is `2e-6 * (1 + abs(reference coefficient))` because
float step accumulation differs from the f64 closed form. Scalar ramp counters
are checked exactly. Near the small-increment threshold, repeated-target float
rounding can shift the last snap by one control period relative to the f64
reference; those last counters are not asserted equal. Separate tests check
the exact float threshold and endpoint rules.

The 640-point steady transfer checks are retained. Existing stereo streamed/
resident renders are exactly equal at 1/64/257/1024-frame callbacks, including
smoothed indexed cutoff changes and zero callback/reset allocations and
deallocations. The full sampler/format suites pass 193 tests. Piano playback,
conformance and exact streamed/RAM equality pass with zero streaming underruns.
The optimized 600-callback run's slowest callback is 3,258 us against a 5,333 us
budget (256 frames / 48 kHz); this finite local measurement includes scheduling
and is not a hard realtime guarantee. Clippy, formatting, standalone/AU/VST3
builds and built-bundle signatures pass. The reference's maximum audio error
is 0.000008798 and maximum coefficient error is 0.000003250.

Artifacts, source/evidence hashes, logs, generator/reference and Piano render:
`../app/build/verification/filter-control-timing-2026-11-08/`.

## Limits and remaining work

- Cloned template/onset coefficients and native scheduling across voice starts
  need runtime corroboration. The cold-start/resume policy above is explicit;
  this increment does not prove native lifecycle or host-block parity.
- Native constant-buffer optimization flags, legacy 0.3-second base-cutoff bias
  and arbitrary internal-object modulation paths are not reproduced. Current
  preset base parameters are static; the new timing supports their existing
  routed cutoff paths. Editable FX automation is still not exposed.
- Unindexed XML/internal octave routes retain Cadence's bounded compatibility
  mapping and are sampled on these clocks; arbitrary native object routing and
  resonance/drive modulation remain open.
- Exact native float/SIMD ordering, full DSP state serialization, bypass
  automation, higher-order/band/ladder filters and independent Kontakt/Logic
  comparisons remain open.
- CTest requires writable test temp storage and remains blocked at shared-memory
  creation in this managed sandbox. Installed bundles have not been replaced.
