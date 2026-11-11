# SSL G EQ control curves

Implemented 2026-11-08 for the four program SSLGEQ bands. This replaces the
previous approximation, which guessed the gain range, frequency mapping and
filter topology. The four bands now use the recovered Kontakt `G_EQ` control
curves; the low and high bands are shelves, and only the two mid bands are
peaking.

Evidence is the saved Kontakt 8.14.1 ARM64 Ghidra export and the matching
symbols in the image, not original source. No Kontakt process was launched for
this increment; the native reference is the separately captured
`kontakt-comparison-20261108T012259Z-47329` render.

## Wire order and runtime fields

`BParFXSSLGEQ::doRead`, `0x100781288`, version `0x11`, reads:

| Wire | Runtime field | Band |
| --- | --- | --- |
| f32 | +0x0c | LF gain |
| f32 | +0x10 | LF frequency |
| bool | +0x14 | mode flag |
| f32 | +0x18 | LMF gain |
| f32 | +0x1c | LMF frequency |
| f32 | +0x20 | LMF Q |
| f32 | +0x24 | HMF gain |
| f32 | +0x28 | HMF frequency |
| f32 | +0x2c | HMF Q |
| f32 | +0x30 | HF gain |
| f32 | +0x34 | HF frequency |
| bool | +0x3c | mode flag |
| f32 | +0x08 | input gain |
| f32 | +0x38 | output gain |
| bool | +0x3e | enable flag |
| bool | +0x3d | enable flag |

`BEffectSSLGEQ::setParameters`, `0x1004243ac`, passes the eight band gains,
frequencies and Q values to `G_EQ::setGModeParameters`. `G_EQ::calcFilters` maps
them through the parameter tables. The low and high bands are shelves; the mid
bands are peaking.

## Recovered curves

The natural values at unity `0.0, 0.1, …, 1.0` are the eleven control points
below. They come from the `HF_*_Curve_GE`, `HMF_*_Curve_GE`, `LMF_*_Curve_GE`
and `LF_*_Curve_GE` symbols (for example `HF_Gain_Curve_GE` at
`0x1076e1a30`). Cadence interpolates linearly between adjacent points. The
native curve is a spline, so linear interpolation is an explicit approximation.

| Curve | Natural values |
| --- | --- |
| LF gain (dB) | -19.5, -16.0, -10.0, -5.3, -2.0, 0.0, 2.0, 5.3, 10.0, 16.0, 19.5 |
| LF frequency (Hz) | 40.43, 44.64, 63.44, 101.36, 140.21, 183.73, 245.15, 351.55, 513.3, 614.68, 697.34 |
| LMF gain (dB) | -21.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 21.5 |
| LMF frequency (Hz) | 223.0, 239.0, 323.0, 494.0, 668.0, 809.0, 1050.0, 1479.0, 1841.0, 2083.0, 2200.0 |
| LMF Q | 0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.3, 0.25, 0.21, 0.2 |
| HMF gain (dB) | -20.0, -17.5, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 17.5, 20.0 |
| HMF frequency (Hz) | 692.48, 730.72, 1022.48, 1614.67, 2112.54, 2619.29, 3291.53, 4544.29, 5634.35, 6444.74, 6892.65 |
| HMF Q | 0.65, 0.62, 0.58, 0.5, 0.45, 0.4, 0.37, 0.31, 0.26, 0.22, 0.22 |
| HF gain (dB) | -20.5, -18.0, -11.0, -6.0, -2.0, 0.0, 2.0, 6.0, 11.0, 18.0, 20.5 |
| HF frequency (Hz) | 1539.0, 1710.0, 2400.0, 3980.0, 5460.0, 6820.0, 9160.0, 13100.0, 17813.0, 21000.0, 21800.0 |

For Piano Uno - Concert, the active program-insert SSLGEQ decodes to a
-3.18 dB low shelf at about 316 Hz, +3.44 dB high shelf at about 6142 Hz, and
neutral mid bands. The slot's common output level (1.4069) is still applied.

## Implementation

- `nk_format::fx::FxChild::ssl_bands` reads the wire order above and returns
  four `SslBand` values with `LowShelf`, `Peaking` or `HighShelf`.
- `sample_adapter::fx_dsp` builds RBJ low/high shelf biquads for the edge bands
  and RBJ peaking biquads for the mid bands. The existing three-band
  `BParFXFilter` path keeps its octave-bandwidth peaking coefficients.
- Shelves use the recovered Q (0.707 for the edge bands). The mid bands use the
  recovered Q curves.
- All preparation remains off the audio callback; the band count is fixed at
  four, so no callback allocation changes.

## Measured effect

Native reference: `kontakt-comparison-20261108T012259Z-47329/native-runner-clean`.
Cadence reference after the change: `cadence-sslfix.wav` (byte-identical across
two renders).

| Metric | Before | After |
| --- | --- | --- |
| Delay-aligned correlation | 0.9528 | 0.9858 |
| Delay-aligned SNR | 0.44 dB | 12.09 dB |
| Delay+gain-fitted SNR | 10.35 dB | 15.51 dB |
| Fitted Cadence gain to match native | 0.5139 (-5.78 dB) | 0.8431 (-1.48 dB) |

After the SSLGEQ fix the sustain matches within about 1.4 dB. A separate
release-trigger fix then removed the binary display-name heuristic for release
groups: `GenerateSysScript` builds `%rlsTrigGroups` from the serialized
release-trigger flag (runtime +0x65b), and the Concert preset sets that flag on
no group. The group named "Release" therefore plays as a normal layer on
note-on instead of being spawned fresh at note-off. The post-note-off tail now
matches at 7-8 s (native/Cadence RMS 1.11/1.14) and is about 7 dB quiet at
6.2 s, instead of about 18 dB loud. This is an improvement, not a parity pass.

## Bounds and open items

- Linear interpolation between the recovered control points approximates the
  native spline.
- The edge-band stopband slope and the exact G_EQ topology/phase are not
  independently verified.
- The `+0x08`/`+0x38` input/output fields and the four mode/enable flags are
  parsed but not applied as separate gains; the common slot output level is
  applied.
- One preset, note and velocity. Other EQ settings and other SSLGEQ versions
  are not covered.
- The release-group tail, group amp placement and IRC wet/dry remain open.

## Evidence

- `../../Kontakt-8-REA/pseudocode/functions/0x100781288.c`
- `../../Kontakt-8-REA/pseudocode/functions/0x1004243ac.c`
- `../../Kontakt-8-REA/pseudocode/functions/0x101c026a4.c`
- `app/build/verification/kontakt-comparison-20261108T012259Z-47329/sslgeq-fix-result.json`
- `app/build/verification/kontakt-comparison-20261108T012259Z-47329/native-runner-clean/comparison-sslfix.json`
