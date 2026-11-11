# Higher-order, band and legacy ladder filters

Implemented 2026-11-08. Seven previously approximate group models now use
recovered processors, and the same models play in program insert/send/master
slots. Together with the earlier six, the explicit inventory is thirteen
models. This is not the complete Kontakt filter/effect matrix.

## Added model inventory

| Serialized ID | Model | Recovered processing |
| --- | --- | --- |
| 4 | Legacy two-pole band-pass | One-pole low-pass then high-pass; resonance does not change its steady transfer |
| 8 | Legacy four-pole low-pass | Distinct two-biquad prototype; feedback compression after stage one |
| 9 | Legacy four-pole high-pass | Distinct two-biquad prototype; input compression |
| 10 | Legacy four-pole band-pass | Separate low/high warps and normalization; feedback compression after stage one |
| 11 | Legacy four-pole band reject | Parallel low/high branches, double coefficients and float histories |
| 12 | Legacy six-pole low-pass | Three distinct biquads; feedback compression after stage two |
| 17 | Original legacy ladder | Two nonlinear four-stage passes per audio frame |

Already supported: IDs 2/3/6/7 (legacy LP/HP one/two-pole) and 50/51 (SV1).
The new decoders retain normalized controls in version-0x92 records, use the
native effective second subtype field and reject non-finite/truncated/trailing
records. The supported record version has not been broadened. Higher-order
models no longer become repeated identical RBJ sections, and ID 17 no longer
becomes a generic four-pole cascade. Newer ladder/SV families remain separate.

## Static evidence

Evidence is the saved Kontakt 8.14.1 ARM64 Ghidra export under
`../../Kontakt-8-REA/pseudocode/functions/`. Provider limitations/warnings remain
applicable. No Kontakt runtime or audio capture was performed in this increment.

| Fact | Saved function(s) |
| --- | --- |
| Effective type and normalized controls | `0x1007829cc.c`, `0x1003e1ba0.c` |
| Group factory selection, including ID 17 legacy ladder | `0x1003c4f1c.c` |
| Four-pole float prototypes | `0x100445d68.c` |
| LP4/HP4 coefficients | `0x10050b8fc.c`, `0x1004f410c.c` |
| BP2 coefficients and two-stage recurrence | `0x1005353f0.c`, `0x1005351e0.c` |
| BP4 coefficients and processing | `0x100535a14.c`, `0x100535a04.c` |
| BR4 double prototypes, coefficients and parallel recurrence | `0x10053623c.c`, `0x1005365bc.c`, `0x100536d0c.c` |
| LP4/BP4 inline feedback compression, HP4 input compression | `0x10050bfe0.c`, `0x1004f4a98.c` |
| LP6 prototypes, coefficients and compression | `0x1006297e4.c`, `0x1006299e4.c`, `0x1006213a8.c` |
| Legacy ladder rate mapping and coefficients | `0x100628488.c`, `0x100628754.c` |
| Legacy ladder two-pass scalar recurrence and reset | `0x100626c7c.c`, `0x100628500.c` |

The higher-order models share the recovered polynomial cutoff control warp,
but use their own prototype poles, resonance factors and output compensation.
LP/HP4 and LP6 normalize their sections jointly. BP4 normalizes its low/high
sections individually. BR4 sums two independently normalized branches instead
of cascading notches. HP4 coefficient generation retains the native mixed
float/double transforms; BR4 uses the saved double prototype constants and
rounds each branch output/history to float.

Native compression is `limited + slope * (input - limited)`, where `limited`
is clamped to ±1. The slope is 0.6 for four-pole processing and 0.7 for LP6.
For inline compression, the compressed section output is retained in that
section's feedback history and passed to the next section. Moving clipping to
the end of a cascade would produce different audio. BR4 has no such compressor.

The ID-17 ladder is the older `BFilterLadder_legacy`, not the newer KONTAKTFX
ladder family. Its frequency lookup covers `20 + 100 * cutoff` semitones,
unlike SV1's 115-semitone span. Its coefficient uses the recovered `6.283f`
constant and 1.57 cap; feedback is linear in resonance up to 4. Each channel
retains previous input, four integrator states and previous feedback. Two
four-stage passes run per audio frame, with the native piecewise polynomial
saturator. Output is the updated fourth state, rather than the last raw stage
or its saturation. The recovered 1e-12 denormal offsets are retained; a silent
channel therefore has a tiny own output floor.

## Routing, timing and storage

Group processing remains per voice, ordered at original slot addresses with
bypass and output gain. Program inserts, parallel send returns and master
slots share the processors and retain wet/dry gains. The new models use the
existing [32-frame legacy control timing](kontakt-filter-control-timing.md),
including routed cutoff slew and resonance lag. The earlier lifecycle,
constant-buffer and unindexed-octave compatibility limits still apply.

Each voice/filter has fixed capacity for three biquad histories or the six
ladder state values per channel. No state allocation depends on callback size.
Program filter state is boxed during load to keep the processor enum compact;
reset clears the existing allocation in place. Callback/reset tests count both
allocations and deallocations. Histories survive host/event/aux segmentation;
reset/model changes clear state, and sample-rate changes rebuild coefficients.

## Verification

Seven new regressions cover prototype responses, actual impulse audio,
nonlinear placement, ladder state-space/unrolled references, extreme settings,
reset/rate changes and streamed/resident routed playback. Existing strict
record and program insert/send/master tests now cover all thirteen IDs,
including allocation-free reset of the boxed program state.

Independent references do not import Rust production code:

- **1,024 higher-order response points** evaluate prototype transfers directly
  under the frequency substitution, with gain derived in the prototype domain.
  Ordinary maximum complex error is 0.0005091. Minimum-cutoff float cascades
  have a separate cancellation-sensitive bound: `0.05 + 0.05 * abs(reference)`.
  The maximum observed error is 0.08325, at LP4 / 48 kHz / cutoff 0 / resonance
  0.7 / 48 Hz (reference magnitude about 4.74). Ordinary cases require
  `0.001 + 0.003 * abs(reference)`. These bounds are against f64 algebra,
  not measured native audio or a bitwise match.
- **6,912 higher-order stereo frames** compare nonlinear audio with an
  independent polynomial/direct-form simulation over three rates and levels
  up to 40. Maximum sample error is 0.0003942; maximum error divided by
  `1 + abs(reference)` is 0.0001773 (limit 0.001).
- **128 ladder small-signal response points** use an independent six-state
  complex matrix solve. Actual float impulse audio agrees within 0.000005903
  complex error. A matched silent-channel trace is subtracted for this check
  because the linearized matrix omits native denormal offsets.
- **9,216 ladder nonlinear stereo frames** use independent unrolled equations,
  including high drive beyond the saturation threshold. Maximum sample error
  is 0.000004398; bounds are `0.0003 + 0.00002 * abs(reference)`. Finite short
  trajectories avoid claiming long resonant float/phase identity.
- Every added model has exact stereo output across **1/64/257/1024-frame**
  callbacks, with resident and streamed samples, sparse indexed cutoff routes,
  smoothed CC changes and reset. Streaming underruns and callback/reset
  allocations/deallocations are zero.
- Previous 640 steady-response and 2,660 control-timeline points are retained.
  Full sampler/format suites pass **200 tests**. Installed Piano playback,
  conformance and exact streamed/RAM equality remain covered as regressions;
  Piano does not itself fixture the seven newly added models.

Clippy warnings-denied, formatting and standalone/AU/VST3/test builds pass.
Built-bundle signatures pass. All three bundled worker payloads match the built
release worker after removing signing metadata and normalizing its allocation
size in temporary copies; the verification script is retained. Optimized paced
Piano playback completes 600 callbacks with
zero streaming underruns; the slowest callback is 1,533 us / 5,333 us budget
(256 frames / 48 kHz, finite local measurement, not a hard realtime guarantee).
Debug timing can exceed that budget. Timing and the Piano regression render
are retained with the report. CTest remains blocked by managed-sandbox shared-memory
creation. Installed bundles have not been replaced.

Evidence, source/artifact hashes, logs, references and render:
`../app/build/verification/higher-band-ladder-filters-2026-11-08/`.

## Remaining work

- Independent Kontakt/Logic audio comparisons, native onset/template scheduling,
  exact SIMD/float ordering and minimum-cutoff/high-resonance rounding behavior.
- Native constant-buffer flags, base-cutoff bias, arbitrary internal-object and
  resonance/drive modulation, group pre/post-amplifier placement; editable FX
  automation and DSP state recall.
- Other serialized versions and modern ladder/SV/formant/phaser/comb/multi-filter
  families; unsupported-effect diagnostics remain a wider compatibility task.
- [Shaper types](kontakt-shaper-types.md) are now implemented. Remaining current
  FX approximations: SSLGEQ controls/units, spread modes and limiter behavior.
