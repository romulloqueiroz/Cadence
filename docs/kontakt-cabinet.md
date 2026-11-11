# Kontakt cabinet model

Recovered `BParFXCabinet` (serialized type 36, version 0x50) playback path.
Evidence is the ARM64 image `Kontakt-8-REA/Kontakt-8.arm64` (SHA-256
`42a3216b…`) and the saved pseudocode under `Kontakt-8-REA/pseudocode/`.
This is a static-evidence port; no native audio capture is claimed.

## Wire payload

`BParFXCabinet::doRead` (`0x1007828b8`) at version 0x50 reads:

```
f32 size, f32 air, f32 treble, f32 bass, s32 cabinetType
```

The parser is `nk_format::fx::FxChild::cabinet_params` and the DSP is
`sample_adapter::cabinet_dsp`.

## Tables

| Symbol | Address | Shape |
| --- | ---: | --- |
| `BEffectCabinet::s_Setups` | `0x1096c2f60` | 11 × 0x28; first three ints `(cabinetId, column, row)` |
| `BEffectCabinet::s_CabFilterMap` | `0x104cc986c` | 32 × 25 ints; `map[cabinetId][column*5+row]` |
| filter rows | `0x104cca294` | stride `0x2e4`; 23 × 8 floats + one level float |

The previous handoff listed `0x104cca574` as the table base. That address is
`row[0] + 0x2e0` (row 0's level); the row base is `0x104cca294`. The map has
32 cabinet groups, not 11. Only the eleven rows selected by `s_Setups` are
embedded (`cabinet_tables.bin`, 11 × 185 floats).

A filter row is 23 sections of eight floats:

```
[0] real part of zero/pole A       [4] real part of zero/pole B
[1] imag part of A                 [5] imag part of B
[2] real part of A'                [6] real part of B'
[3] imag part of A'                [7] imag part of B'
[8..] next section                 [0x2e0] level
```

`A'`/`B'` are the conjugates (`s2 == s0`, `s3 == -s1`, `s6 == s4`,
`s7 == -s5`), so each section is a real biquad.

## SetCabinet (`0x1005c35e8`)

The exponential table is runtime-initialized, not file-backed. Its
constructor (`0x101c02f98`) builds 2401 entries:

```
value[i] = 2^((i-1200)/60)
slope[i] = value[i+1] - value[i]
```

`exp_map(db)` clamps to `i < 2400`, returns 0 below 0 and 2^20 above 2399,
and interpolates `value[i] + slope[i]*(x-i)`.

```
e_size   = exp_map(size * 6.1960783 - 3.0980392)
e_bass   = exp_map(15 * bass)
e_treble = exp_map(15 * treble)
w        = e_size * rate / pi
```

The four complex factors use `w` and the section's eight floats. The real
parts of `s4`/`s6` are clamped by `w * -0.0003`; the second clamp tests the
first clamp result (a native quirk). Each section stores

```
P0 = -(Re d1 + Re d3)      P2 = Re d2 + Re d4
P1 = Re(d1*d3)             P3 = -Re(d2*d4)
```

and is played as

```
y  = x + s1
s1 = P0*x + P2*y + s2
s2 = P1*x + P3*y
```

The last section (index 23) is the tone section built from `e_bass`/`e_treble`
and is smoothed; the 23 table sections are fixed. The running gain is the
product of `Re(b1*b3 / b2*b4)` times the row level and a final
bass/treble normalization. Output targets are `(gain, gain, 0.7*air*gain,
0.3*air*gain)`.

## ProcessInt (`0x1005c3b58`)

24 sections are processed as two lanes with a one-sample pipeline delay:

* even lane 22, 20, …, 0 from the serialized input;
* odd lane 23, 21, …, 1 from the previous sample's even-lane output;
* final output is the odd lane.

The pipeline delay is an optimization in the native code, but it is part of
the sample-accurate response. Per-section state is two transposed
direct-form-II values per channel. Section 23 uses the smoothed tone
coefficients.

The reflection block is a two-line feedback network. Delay length is
`1 << (floor(log2(rate*0.04501862))+1)`, mask `length-1`, and eight taps at
`int(rate*0.015*1.17^k)`. Line 1 uses even taps, line 2 odd taps:

```
p = L1[t0] + 0.5336501*L1[t4]
r = L2[t1] + 0.5336501*L2[t5]
q = -0.7305136*L1[t2] - 0.38983864*L1[t6]
s = -0.7305136*L2[t3] - 0.38983864*L2[t7]
out = [gain0*odd0 + p + q, gain1*odd1 + r + s]
```

The delayed signals written back are one-pole smoothers of
`gain*odd + 0.16*(p+q+r+s)`.

## Verification

`crates/sample-adapter/tests/fixtures/cabinet_reference.py` is the extractor
and independent scalar reference. It writes `src/cabinet_tables.bin`,
`cabinet-coefficients.tsv`, `cabinet-frames.tsv` and
`cabinet_provenance.json`.

* 44 coefficient cases × 24 sections × 4 coefficients (4,224 values):
  maximum absolute error 1.16e-10 against the Rust port.
* 1,536 stereo frames over 44.1/48/96 kHz and two cabinets: zero sample
  error.
* `cargo test -p nk-format -p sample-adapter`: 110 sampler/format lib tests
  pass; all integration suites pass.
* Clippy `-D warnings` and `cargo fmt --all -- --check` pass.

## Limits

* The two delay-smoothing poles are initialized in non-file-backed `__common`
  BSS. The provenance records the provisional `DELAY_KA`/`DELAY_KB` values
  used by both the reference and the Rust port. This is the only invented
  numeric part.
* The native coefficient crossfade state machine (`Process`, 0x210/0x214)
  is not ported. Parameter changes currently smooth the tone section and the
  output/feedback gains; the 23 table sections change immediately.
* Cabinet automation/recall beyond the static serialized fields is not wired.
* The amp family now has recovered, separate processors; see
  `kontakt-amp-runtime-ports.md` for their native validation scope.
* No native Kontakt audio comparison has been made for this effect.
