# JUMP scalar graph recovered from saved pseudocode

This refines `kontakt-jump-processint-trace.md`. It is an ordered translation
of saved Ghidra output, not a native audio observation. A simple six-stage
immediate cascade does **not** describe `ProcessInt`.

## Evidence and limits

Sources are under `../../Kontakt-8-REA/pseudocode/functions/`:

| Function | SHA-256 |
| --- | --- |
| `0x1005cc6ac.c` (`ProcessInt`) | `a2f82dc52cd7a1fbf3abe4e35b2bf777313476c681ffa4a61038cf8ba283c6be` |
| `0x1005ca0a0.c` (`Initialize`) | `27bfdcc4ec3b5dcbc13bd13f62b5b765371e5fc65d6866c536bcad9b94ab3674` |
| `0x1005cc4cc.c` (`Process`) | `6a05bed5a51cf48ce1698d2a44891ca370a0301b53c69a1554497e4faf6bbb09` |
| `0x1005cbf1c.c` (`SetJump`) | `a270a1280478b2081c4bd32d8295f5ba5b2131c51a35a9a08ddc005b6e3bf173` |
| `0x1005cbde0.c` (`Reset`) | `3c7bc9c4dc1c952bf7d6e0283d6f4c7789d6b8e12698384303ea4ec5d4c10b78` |
| `0x1005c2b24.c` (`SetOutLevel`) | `2e12de530ff07d7f0a5e0216ab50085485da8ffd756a5648bc68449705610593` |
| `0x1005c2b2c.c` (`AnticipateVolumeSmoothing`) | `ee3aa3719bace951f3a88fff412f2962812b4edda6b1c128344f58fba3e392fb` |
| `0x1005bce24.c` (`SmoothVolumeAndOut`) | `609c274ae3595faaa28b1944ec6750ec322df828764fd5ef5e898e699d3ec7a6` |

These files identify Ghidra 12.1.4, profile
`040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`.
The original export Evidence ID documented by earlier passes is
`ev_8369a586de610d706294e52323779b18fdfb6ea6e127c1d77be18f0e0511748a`.
This pass read existing exports without reopening the native analysis engine.
The file hashes identify exactly what was read; the inherited export ID is
provenance, not a fresh tool observation. Original analysis completeness,
annotations, exact instruction rounding and native output parity remain unknown.

In the equations below, `C[hex]` is a global object float at that byte offset,
`S[hex]` is a float in the current channel (`this + channel*0x120`), and
`M[k]` is the smoothed current at `+0x44f0+4*k`. References to a four-float
vector mean consecutive offsets. Snapshot old inputs before writing outputs.
The equations retain native state dependencies, but do not promise bitwise
equivalence to the original NEON operations.

## Corrections to the earlier trace

1. The inner loop has four oversamples per host frame, not six stage
   iterations. `lVar22` takes `0,-1,-2,-3` (ProcessInt lines 327–331,
   1178–1179). It selects four FIR phases and writes four reversed-address
   output slots. SIMD lanes describe circuit branches computed at one
   internal time step.
2. There are eight nonlinear state lanes using six distinct curves: first
   bank `[Boost, Boost, Stage1, Stage2]` at `+0x250..+0x25c`, second bank
   `[PowerTube, PowerTube, SplitterP, SplitterN]` at `+0x280..+0x28c`.
   The first four lanes feed delayed state/mixing connections. The second
   bank computes power-tube branches before producing the splitter outputs
   used in the next internal time step.
3. The table-pointer order was misread in the earlier trace. `Initialize`
   uses `long*` indices: `0x917*8=0x48b8`, `0x918*8=0x48c0`,
   `0x919*8=0x48c8`. The correct mapping is:

| Pointer byte offset | Entries | Curve |
| --- | --- | --- |
| `0x48a0` | `0x13ec` | Boost |
| `0x48a8` | `0x1c0c` | Stage1 |
| `0x48b0` | `0x242c` | Stage2 |
| `0x48b8` | `0x3c8c` | PowerTube |
| `0x48c0` | `0x2c4c` | SplitterP |
| `0x48c8` | `0x346c` | SplitterN |

See Initialize lines 91,119,124,129,134,139 and ProcessInt lines
755–769,1027–1043. The six `0x20`-spaced blocks in the earlier trace are
not six homogeneous stage objects. For example `+0x2e0` is a supply
oscillator/envelope block, while `+0x2c0` contains reciprocal supply values.

## FIR oversampling

Initialize lines 246–261 install these exact, binary-representable constants.
Input history is `[x[n],x[n-1],x[n-2],x[n-3]]`. For internal phase `p=0..3`,
dot that history with the following row (`+0x46e0-16*p`):

| Phase | Four input coefficients |
| --- | --- |
| 0 | `[-0.0234375, 0.2265625, 0.8671875, -0.0703125]` |
| 1 | `[-0.0625, 0.5625, 0.5625, -0.0625]` |
| 2 | `[-0.0703125, 0.8671875, 0.2265625, -0.0234375]` |
| 3 | `[0, 1, 0, 0]` |

The output phases are saved at `S[0x1ec-4*p]`. At frame end the 16 floats
`S[0x1e0..0x21c]` are dotted with `C[0x46f0..0x472c]` and the output
history shifts by four floats. Coefficients in ascending address order:

```
[-0.005859375, -0.015625, -0.017578125, 0,
  0.056640625, 0.140625, 0.216796875, 0.25,
  0.216796875, 0.140625, 0.056640625, -0.0,
 -0.017578125, -0.015625, -0.005859375, 0]
```

ProcessInt lines 545–553 and 1175–1231 establish the dot products and history
movement. There is no six-stage loop to substitute for this phase loop.
`SetSampleRate` first establishes `rate4=host_rate*4`, then uses
`2*rate4=host_rate*8` for the bilinear coefficient polynomials.

## Nonlinear filter primitive

The first filter bank (ProcessInt lines 554–683) consumes
`x=[upsampled_input, upsampled_input, old S[2d0], old S[2d4]]`.
For lane `i=0..3`:

```
u = S[220+4*i] + C[4730+4*i]*x[i]
if u <= C[4750+4*i]:
    a[i] = C[47b0+4*i]*u
    h = 0
else:
    a[i] = C[4790+4*i] + C[4770+4*i]*u
    h = C[47f0+4*i] + C[47d0+4*i]*u

S[220+4*i] = old S[230+4*i] + C[4850+4*i]*h
             + (x[i]*C[4830+4*i] - C[4810+4*i]*a[i])
S[230+4*i] = (x[i]*C[4880+4*i] - a[i]*C[4870+4*i])
             + h*C[4890+4*i]
```

The second bank (lines 812–915) consumes
`x=[old S[288],old S[28c],tone_mix,tone_mix]` and has one state per lane:

```
u = S[240+4*i] + C[4740+4*i]*x[i]
if u <= C[4760+4*i]:
    b[i] = C[47c0+4*i]*u
    h = 0
else:
    b[i] = C[47a0+4*i] + C[4780+4*i]*u
    h = C[4800+4*i] + C[47e0+4*i]*u
S[240+4*i] = C[4860+4*i]*h
             + (C[4840+4*i]*x[i] - C[4820+4*i]*b[i])
```

`S[24c]` is subsequently overwritten with the internal output sample each
phase (line 1076). Preserve that write: it is part of the feedback wiring.

## Exact table-indexing structure

For a table with integer `anchor`, the coordinate `q` already includes the
table's fraction and circuit gain/offset. Define:

```
lo = -float(anchor)
hi = 255.0 - float(anchor)
index = trunc(min(max(q, lo), hi) - lo)
shaped = entries[index].value
         + entries[index].slope * (q - (lo + float(index)))
```

The clamp controls **only the index**. The residual uses original `q`, so
the endpoint segments extrapolate beyond the table domain. Clamping `q`
before evaluating the residual would change native behavior. The slope is
from `entries[index]`, not `entries[index+1]`; `value` is already relative
to the table baseline. The code does not subtract the center entry again.
See ProcessInt lines 687–785 and 947–1068.

Finite inputs within the stated range reduce the decompiler's elaborate
saturating integer-conversion logic to the expression above. NaN/overflow
conversion behavior needs instruction-level verification before claiming
equivalence for invalid inputs.

The first bank coordinates and target gains (lines 680–785) are:

```
q0 = old S[250] + C[4950] + C[4930]*a[0]*old S[2c8]
q1 = q0
q2 = C[4958] + C[4938]*a[2]*old S[2c8]
q3 = C[495c] + C[493c]*a[3]*old S[2c4]

target = [old S[2bc]*Boost(q0), old S[2b8]*Boost(q1),
          old S[2b8]*Stage1(q2), old S[2b4]*Stage2(q3)]
S[250+4*i] += C[48d0+4*i]*(target[i]-S[250+4*i])
S[2d4] = S[258]
```

The two Boost lanes share input coordinate and fetched entry, but have
different target gains and smoothing coefficients. They are not interchangeable.
`a[1]` is computed by the first filter bank but does not enter these table
coordinates in the saved output.

## Tone/mix wiring and the second nonlinear bank

ProcessInt lines 786–811 first updates four poles:

```
S[260] += (old S[2d8]-S[260])*M[14]
S[264] += (old S[2dc]-S[264])*M[15]
S[268] += (S[254]-S[268])*C[48e8]
S[26c] += (S[25c]-S[26c])*C[48ec]

stage2_mix = M[7]*S[25c] + M[3]*S[26c]
tone_mix = M[4]*old S[2d8] + M[0]*S[260]
           + M[5]*old S[2dc] + M[1]*S[264]
S[2d0] = M[8]*upsampled_input + M[6]*S[254] + M[2]*S[268]
```

Run the second filter bank with `tone_mix`, then lines 916–946 give:

```
e0 = b[2] - S[270]
e1 = b[3] - S[274]
e2 = stage2_mix - S[278]
e3 = stage2_mix - S[27c]
S[2d8] = e2
combined = b[2] + M[12]*e0 + b[3] + M[13]*e1
S[278] += M[10]*e2
S[27c] += M[11]*e3
S[270] += C[48e0]*e0
S[274] += M[9]*e1

q0 = C[4960] + C[49c8]*(C[49b0] + C[4940]*(old S[298]*0.111235954 + b[0]))
q1 = C[4964] + C[49c8]*(C[49b4] + C[4944]*(old S[298]*0.111235954 + b[1]))
q2 = C[4968] + old S[2c0]*(C[49b8] + C[4948]*combined)
q3 = C[496c] + old S[2c0]*(C[49bc] + C[494c]*combined)

target = [C[49cc]*PowerTube(q0), C[49cc]*PowerTube(q1),
          old S[2b0]*SplitterP(q2), old S[2b0]*SplitterN(q3)]
S[280+4*i] += C[48f0+4*i]*(target[i]-S[280+4*i])
internal_output = (S[280]-S[284])*M[20]
S[1ec-4*phase] = internal_output
S[24c] = internal_output
```

`C[49c8]` and `C[49cc]` are constructor values, not sample-rate results.
They are `0.00206611561588943` and `484.0` respectively (constructor
`puVar1[0x939]=0x43f200003b0767ab`). The first lookup bank's coordinate
offsets are `C[4950..495c]`, lower bounds `C[4990..499c]`, upper bounds
`C[4970..497c]`. Second bank corresponding offsets are `C[4960..496c]`,
`C[49a0..49ac]`, `C[4980..498c]`. Initialize's table-field copies define
these independently of SetSampleRate.

## Supply and delayed response states

ProcessInt lines 1081–1100:

```
S[2e0] += S[2e4]
if S[2e0] >= 1: S[2e0] -= 2
r = S[2e8] - (S[2ec] + S[2e8]*S[2e0]*S[2e0])
S[2ec] += C[49c4]*(S[280]+S[284]) + C[49c0]*max(r,0)
```

Lines 1101–1172 run four paired integrators. Snapshot `p=S[290..29c]`,
`v=S[2a0..2ac]`; inputs are `[e3,e3,S[2ec],S[2ec]]`, using the
highpassed `e3` from the tone/mix block:

```
e[i] = input[i] - (C[4900+4*i]*v[i] + p[i])
v[i] += C[4910+4*i]*e[i]
p[i] += C[4910+4*i]*v[i]
S[2a0..2ac] = v
S[290..29c] = p
t = e[0] + p[0]
S[2dc] = t + M[16]*(e3-t)

old_supply = S[2b0..2bc]
targets = [p[2]*0.8712, old_supply[0]*0.85,
           old_supply[1]*0.96, old_supply[2]*0]
S[2b0+4*i] += C[4920+4*i]*(targets[i]-S[2b0+4*i])
S[2c0+4*i] = reciprocal_refinement(S[2b0+4*i])
```

The `NEON_ext` shuffles in this block establish the delayed supply chain;
using new supply lane 0 immediately for lane 1 would change it. `C[492c]`
is zero, so `S[2bc]` preserves its initialized Boost offset.

The supply reciprocals use NEON reciprocal estimate, reciprocal step, and
another Newton step. Ordinary `1.0/x` is an explicit numerical substitution
unless its differences are measured against native instructions.

## Initialization and coefficient automation requirements

`Reset`, not zero-fill, establishes the operating point. With
`voltage_scale = S[2e8]/413`, it seeds `S[2ec]=voltage_scale*395.695`,
`S[2e0]=0.7160450220108032`, `S[2b0..2b8]` from float bit patterns
`[0x43abad91,0x4391ed0e,0x438c16c9]*voltage_scale`, `S[2bc]` from
Boost's table offset, and `S[2c0..2cc]` from
`[0.00291243,0.00342639,0.00356916,0.0029539]*voltage_scale`.
It also seeds `S[280],S[284]` with bit patterns
`[0x41e44c30,0x41e56f9e]`, `S[298]=f32::from_bits(0x43c514fe)`;
the remaining shown signal states are cleared. `Initialize` seeds
`S[2e8]=413` for every channel. Its final virtual `+0x30` call resolves to
`BEffectJump::Reset`. Reset's initial virtual `+0xd8` call resolves to
`BEffectBase::AnticipateVolumeSmoothing`, which copies the output-level
target at `+0x1a4` to its smoothed current at `+0x1ac`. This adds no circuit
state or hidden amp gain; see the file-backed dispatch evidence below.

During parameter smoothing, the 24 currents advance on **each internal
phase**, once globally across channels (the last channel publishes shared
currents; earlier channels keep local copies). `Process` handles 32-host-frame
slices, and the smoother's rate is based on `4*host_rate`.

When `+44c5` requests coefficient refresh (ProcessInt lines 467–542):

* `C[4910]=M[17]` and `C[4900]=M[18]`.
* `M[19]` is the drive curve target used to recompute the first filter's
  lane-2 polynomial coefficients, threshold and reciprocal/gain/offset terms
  from `C[4630..46ac]`. This happens each internal step while smoothing.
* The refresh flag remains set while smoothing is active; coefficient
  recomputation is not simply a SetSampleRate-only operation.

A faithful replacement therefore also needs the complete Initialize table
metadata, Reset operating point, four-phase FIR path, coupled state wiring,
per-internal-step smoothing, and coefficient refresh path. A bulk coefficient
port alone does not establish faithful playback. Before replacing the runtime,
validate a scalar recurrence replay against an independent trace and obtain
native audio captures. No coefficient or curve fixture currently constitutes
such an audio parity check.

Residual verification: the saved decompiler emits float/bit reinterpretation
casts and a dead-looking `NEON_ext(in_q23,...)` near the first lookup. These
must not be copied as C/Rust numeric casts. The useful lane reads/writes above
are visible, but instruction-level rounding, reciprocal-estimate behavior,
and invalid-input semantics remain unverified by this pass. The virtual
Reset/base output-gain dispatch is now resolved by the binary metadata below.

## Resolved virtual dispatch and external output level

The saved pseudocode's unresolved virtual calls can be resolved statically
from `Kontakt-8-REA/Kontakt-8.arm64`, SHA-256
`42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.
`LC_DYLD_CHAINED_FIXUPS` reports pointer format 6
(`DYLD_CHAINED_PTR_64_OFFSET`) for the `__DATA_CONST` segment, which contains
the JUMP vtable at virtual address `0x1096c3368` / file offset `0x96c3368`.
These are file-backed rebases, not unresolved imports: each decoded target
is the low 36 target bits plus the preferred image base `0x100000000`.
The remaining bits include the chained-fixup linkage and must not be
interpreted as an ordinary absolute function address.

| Vtable slot | Pointer VA / file offset | Raw chained word | Decoded function |
| --- | --- | --- | --- |
| `+0x30` | `0x1096c3398` / `0x96c3398` | `0x100000005cbde0` | `0x1005cbde0`, `BEffectJump::Reset` |
| `+0xd0` | `0x1096c3438` / `0x96c3438` | `0x100000005c2b24` | `0x1005c2b24`, `BEffectBase::SetOutLevel` |
| `+0xd8` | `0x1096c3440` / `0x96c3440` | `0x100000005c2b2c` | `0x1005c2b2c`, `BEffectBase::AnticipateVolumeSmoothing` |

`BParFX::SetObj` (`0x10040cefc`, lines 552–559) calls slot `+0xd0` with
its external slot output-level field before `SetJump`. `SetOutLevel`
assigns `+0x1a4`; `SmoothVolumeAndOut` multiplies the processed signal by
that level, smoothing changes through `+0x1ac`. For a settled slot,
Cadence's `FxNode` already applies the same `slot.out_level` as its wet gain
after `JumpAmp::process`; adding it inside the circuit would apply it twice.
The JUMP-specific level polynomial and internal normalization `M[20]` remain
inside the circuit. Native base output-level automation smoothing is a
separate integration concern from this settled-gain equivalence.

This resolves dispatch and gain ownership from static binary/source evidence.
It does not itself observe native execution or establish native audio parity.
