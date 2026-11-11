# Kontakt amp family

The NI guitar/bass amp effects are a separate DSP family from the cabinet.
Their serialized payloads are parsed exactly. JUMP and VAN51 now use recovered
circuits; the remaining kinds use the shared multi-drive approximation.
This document records the recovered layout,
the control-law entry points and the wave-shaping engine that the full port
needs.

Source image: `Kontakt-8-REA/Kontakt-8.arm64`, SHA-256 `42a3216b…`.

## Serialized layouts

All readers are version-checked; `FxChild::amp_settings` rejects unsupported
versions, truncated payloads, non-finite floats and out-of-range mode ints.

| Type | Class | Version | Floats | Bools | Modes |
| ---: | --- | ---: | ---: | ---: | --- |
| 77 | `BParFXJump` | 0x10 | 6 | 2 | — |
| 82 | `BParFXVan51` | 0x10 | 8 | 5 | — |
| 83 | `BParFXACBox` | 0x10 | 7 | 1 | — |
| 84 | `BParFXHotSolo` | 0x10 | 8 | 2 | — |
| 84 | `BParFXBassInvader` | 0x51 | 7 | 3 (+implicit 4th) | — |
| 84 | `BParFXBassInvader` | 0x52 | 7 | 4 | — |
| 85 | `BParFXCat` | 0x10 | 7 | 1 | — |
| 97 | `BParFXBassPro` | 0x10 | 17 | 5 | — |
| 108 | `BParFXEPPreamps` | 0x11 | 11 | 6 | 4 s32 |

`BParFXEPPreamps::doRead` is ordered unusually: floats at payload offsets
`8, c, 10, 14, 1c, 20, 24, 28, 5c, 64, 68`, then two bools, four s32 modes
(limits 5, 6, 7, 5), then four more bools. The `+0x50` bool is derived from
`0x5c > 1e-9` and is not serialized.

Type 84 is shared by `HotSolo` (0x10) and `BassInvader` (0x51/0x52);
`AmpSettings` distinguishes them by version and, for 0x51, sets the fourth
flag to true because it is not serialized. Type 97's inventory entry is
`BassPro` with the same 17-float payload.

## Setters and ProcessInt entry points

| Type | Class | Set | ProcessInt |
| ---: | --- | --- | --- |
| 77 | `BEffectJump` | `0x1005cbf1c` `SetJump` | `0x1005cc6ac` |
| 82 | `BEffectVan51` | `0x1005d4fe0` `SetVan51` | `0x1005d55bc` |
| 83 | `BEffectACBox` | `0x1005d85d8` `SetACBox` | `0x1005d8b0c` |
| 84 | `BEffectHotSolo` | `0x1005de8e8` `SetHotSolo` | `0x1005decbc` |
| 85 | `BEffectCat` | `0x1005e66bc` `SetCat` | `0x1005e6f70` |
| 97 | `BEffectBassPro` | `0x1005e2fb8` `SetBassPro` | `0x1005e3950` |
| 108 | `BEffectEPPreamps` | `0x1005f1b50` `SetEffectParameters` | wrapper `0x10051c7c0` |

The setters are relatively small (74–186 lines) and mostly map normalized
controls to smoothing targets and filter coefficients:

- `SetJump`: drive curve `f(x) = x*(a*x+b)` with piecewise breakpoints at
  0.1/0.4/0.6/0.9, presence clamp at 0.95, four EQ calls, and a level curve
  with breakpoints at 0.35/0.8.
- `SetVan51`: ten-step log-pot tables (`s_cfLogPot522Table`,
  `s_cfLogPot516Table`) for gain/bass/mid/treble/presence, plus stage filter
  updates.
- `SetACBox`: direct scalar coefficients. Input gains use
  `(exp_map(30*u-30) - 0.031622775) * k`; `u4` gives a cubic drive term and
  an asymmetry term `((u2-0.5)^2 - 0.25) * -2.123821`; three one-pole
  coefficients use `min(1, k*exp_map(...))`; mix is `0.631`; drive is
  `u6*5+2`; level is `u7*50`.

## Wave-shaping engine

The `ProcessInt` bodies (998–1420 lines) are built on
`NI::DSP::*::WaveShapingTables`, not the cabinet's measured-biquad rows:

| Namespace | Stage tables |
| --- | --- |
| `NI::DSP::JUMP` | `JCM800BoostStage`, `JCM800Stage1`, `JCM800Stage2`, `JCM800SplitterP/N`, `JCM800PowerTube` |
| `NI::DSP::VAN51` | `Van51Stage1`, `Stage2Rhythm/Lead`, `Stage3Rhythm/Lead`, `Stage4/5/6/8`, `Splitter1/2`, `PowerTube` |
| `NI::DSP::ACBOX` | `AC30IntoA1a`, `AC30A1atoK2a`, `AC30MixtoA5B/A`, `AC30G89toI89` |
| `NI::DSP::HOTSOLO` | `HotSoloStage1/2/3/4/5/7`, `Splitter1/2`, `PowerTube` |
| `NI::DSP::BASSPRO` | `BassProIntoV16`, `BassProV16toV11`, `BassProR35toV33` |

`NI::DSP::WaveShapingTables::TableBase::setupTable` (`0x101b8c140`, VAN51
alias `0x1005caad0`) takes a 256-entry `double` curve from the stage's
virtual `+0x10` method and builds a 256-entry `(value - baseline, next -
value)` table plus an interpolation anchor. `BEffectJump::Initialize`
(`0x1005ca0a0`) calls it for Boost/Stage1/Stage2/SplitterP/SplitterN and
builds the power-tube table manually.

### JUMP curves extracted (2026-11-08)

| Stage | `s_dUnprocessedData` | Entries at |
| --- | ---: | ---: |
| Boost | `0x104d3af00` | `0x13ec` |
| Stage1 | `0x104d3b700` | `0x1c0c` |
| Stage2 | `0x104d3bf00` | `0x242c` |
| SplitterP | `0x104d3c700` | `0x2c4c` |
| SplitterN | `0x104d3cf00` | `0x346c` |
| PowerTube | `0x104d3d700` | `0x3c8c` |

`TableBase` layout: `+8` int anchor, `+0xc` float fraction, `+0x10` float
scale, `+0x14` float offset, `+0x18` float baseline, `+0x1c` 256 × `(rel,
slope)` pairs. The five `setupTable` calls use:

```
Boost      0.64899999, 48.977882, 0.027000001, flag=1, has_offset=0
Stage1     0.27700001, 11.091748, 0,           flag=1, has_offset=1
Stage2     0.86400002, 63.095734, 0,           flag=1, has_offset=1
SplitterP  0.65499997, 32.359367, 0,           flag=1, has_offset=1
SplitterN  0.42300001, 26.607250, 0,           flag=1, has_offset=1
```

The power-tube table is built with `rel = value`, `slope = next - value`,
anchor `0x101`, fraction `-0.2319`, scale `2646.0`.

### Vendor-wide curve addresses (resolved 2026-11-08)

`llvm-nm -n Kontakt-8.arm64` resolves every `WaveShapingTables::*::s_dUnprocessedData`
symbol, so the curves no longer need to be inferred from pseudocode. Each is a
file-backed 256 × f64 array; stage `getUnprocessedTable` returns it.

| Namespace | `s_dUnprocessedData` addresses |
| --- | --- |
| `JUMP` | Boost `0x104d3af00`, Stage1 `0x104d3b700`, Stage2 `0x104d3bf00`, SplitterP `0x104d3c700`, SplitterN `0x104d3cf00`, PowerTube `0x104d3d700` |
| `VAN51` | Stage1 `0x104d3df00`, Stage2Rhythm `0x104d3e700`, Stage2Lead `0x104d3ef00`, Stage3Rhythm `0x104d3f700`, Stage3Lead `0x104d3ff00`, Stage4 `0x104d40700`, Stage5 `0x104d40f00`, Stage6 `0x104d41700`, Stage8 `0x104d41f00`, Splitter1 `0x104d42700`, Splitter2 `0x104d42f00`, PowerTube `0x104d43700` |
| `ACBOX` | MixtoA5B `0x104d43f00`, MixtoA5A `0x104d44700`, IntoA1a `0x104d44f00`, G89toI89 `0x104d45700`, A1atoK2a `0x104d45f00` |
| `HOTSOLO` | Stage1 `0x104d46700`, Stage2 `0x104d46f00`, Stage3 `0x104d47700`, Stage4 `0x104d47f00`, Stage5 `0x104d48700`, Stage7 `0x104d48f00`, Splitter1 `0x104d49700`, Splitter2 `0x104d49f00`, PowerTube `0x104d4a700` |
| `BASSPRO` | IntoV16 `0x104d4af00`, V16toV11 `0x104d4b700`, R35toV33 `0x104d4bf00` |
| `CAT` | CatWaveTable `0x104d4c700` |

`EPPreamps` has no `s_dUnprocessedData` symbol and needs a different path.

### VAN51 curves and tables ported (2026-11-08)

`tests/fixtures/van51_reference.py` extracts the twelve curves and replays the
eleven `setupTable` calls from `BEffectVan51::Initialize` plus the manual
power-tube build (`anchor 0x178`, fraction `0xbe449800`, scale `0x44042f30`).
Stage-to-offset order comes from `BEffectVan51::BEffectVan51` (`0x1005cd984`):
Stage1, Stage2Rhythm, Stage2Lead, Stage3Rhythm, Stage3Lead, Stage4, Stage5,
Stage6, Stage8, Splitter1, Splitter2. Outputs are `src/van51_curves.bin`,
`tests/fixtures/van51-setup.bin` and `van51_provenance.json`.

`amp_dsp::van51_tables()` builds the same twelve tables through the shared
`TableBase` and matches the independent replay bit-for-bit
(`van51_tables_match_independent_setup_reference`).

Van51's `SetSampleRate` (`0x1005ce690`) reads its tuning globals from the same
ExpTable at `0x109dc2200` (`index = (va - 0x109dc2200) / 8`, pair
`(value, next - value)`), so those constants are reconstructible with
`exp_index`, not missing data. `SetVan51` (`0x1005d4fe0`) uses two ten-step log
pot tables at `0x104d1c74c` / `0x104d1c778` with linear interpolation to
`table[i+1]`, and a `SmoothingToolbox<24>` at `+0x9560..+0x9638` like JUMP.
Those setters and the `ProcessInt` circuit are now ported. Numerical helper
goldens, circuit checks, native audio comparison and remaining host discrepancy
are documented in [VAN51 runtime port](kontakt-van51-runtime.md).

### Parameter smoothing

`BEffectJump::Process` (`0x1005cc4cc`) owns a `SmoothingToolbox<24>` at
object `+0x44e0`. On a parameter change it calls `startSmooth` and runs a
four-block crossfade; when the fade finishes it copies the targets at
`+0x45b0..+0x4608` into the current values at `+0x44f0..+0x4548` and zeroes
the increments at `+0x4550..+0x45a8`. `ProcessInt` only reads the current
smoothed values.

### Stage application

`Initialize` installs entry pointers at `+0x48a0..+0x48c8` in the order
Boost, Stage1, Stage2, PowerTube, SplitterP, SplitterN. `ProcessInt` executes
four oversamples per host frame. Its SIMD banks are [Boost, Boost, Stage1,
Stage2] and [PowerTube, PowerTube, SplitterP, SplitterN], with feedback through
state from preceding oversamples. Six curves do not imply a six-stage loop.
The previous pointer order and cascade interpretation were incorrect.

The lookup clamps its index, then interpolates/extrapolates using the original
coordinate and the selected entry's own slope. Detailed recurrences and
remaining gaps: `kontakt-jump-scalar-graph.md`; coefficient coverage and
corrections: `kontakt-jump-processint-trace.md`.

### Ported foundation

`crates/sample-adapter/src/amp_dsp.rs` now embeds the six curves
(`jump_curves.bin`, SHA-256 recorded in `tests/fixtures/jump_provenance.json`),
ports `TableBase::setupTable` and the manual power-tube build, and verifies
all six objects bit-for-bit against an independent Python replay
(`tests/fixtures/jump_reference.py`, `jump-setup.bin`).

The two compact JUMP setters are also ported:

* `setEQFilters` (`0x1005cd29c`) → `jump_eq_filters` (11 coefficients from
  the two EQ-normalized fields, mix and presence);
* `setParamResponse` (`0x1005cac6c`) → `jump_param_response` (six coefficients
  from the response control).

They match `tests/fixtures/jump_controls_reference.py` over 96 values
(`jump-controls.tsv`), maximum absolute error 5.2e-6.

`SmoothingToolbox<24>` (`0x100615f08`) is ported as `Smoothing24`: it turns
24 `(target-current)*factor` deltas into per-sample increments, snaps tiny
deltas, and exposes the pending/period fields used by `Process`. The JUMP runtime now implements the ordered oversampled graph, control-target
integration and feedback timing in `jump_dsp.rs` / `jump_controls.rs`. Native
audio comparison remains a separate measured check; see `kontakt-jump-runtime.md`.

`JumpRateCoefficients` now ports all prototype and circuit rows written by
`SetSampleRate` (`+0x4630..+0x46ac`, `+0x4730..+0x489c`), the rate block,
and response gains. All 4,396 values across 28 cases match an independent
Python scalar replay exactly. Scalar reciprocal/FMA differences from native
instructions remain unmeasured. The rate-block lane packing was corrected.
These coefficients now drive the recovered JUMP playback circuit.

## Current status (2026-11-10)

All eight amp variants now select their own recovered processor: JUMP, VAN51,
ACBox, HotSolo, CAT, BassPro, BassInvader and EPPreamps. None of these cases
falls back to MultiDrive. Serialized layouts remain version-specific, including
BassInvader 0x51's implicit fourth flag.

The legacy amps retain their nonlinear tables, FIR phases, filters, supply
states and native control clocks. BassInvader and EPPreamps use recovered neural
models and the shared power-of-two sample-rate converter. Native coefficient,
state, audio and live-control fixtures are checked independently of the Rust
implementation. The source generators emit Rust arithmetic and numeric data;
playback never loads or executes Kontakt instructions.

See `kontakt-amp-runtime-ports.md` for the verification matrix and remaining
host-lifecycle limits, and `kontakt-van51-runtime.md` for the observed VAN51 host
activation difference. Individual amp parameter APIs support native fades;
new DAW automation dispatch remains separate work.
