# Kontakt external MIDI modulation runtime evidence

Static investigation, 2026-11-07. Reuses the saved Kontakt 8.14.1 ARM64 main
executable export. No Kontakt process was launched. These observations establish
implementation reference behavior; they do not establish runtime parity.

## Provenance

Catalog: `../../Kontakt-8-REA/combined-functions.tsv`. Function paths below are
relative to `../../Kontakt-8-REA/pseudocode/functions/`. Original REA export
Evidence ID: `ev_8369a586de610d706294e52323779b18fdfb6ea6e127c1d77be18f0e0511748a`.
Analysis profile: `040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`.
Saved output is Ghidra pseudocode, with its own warnings and uncertain types.
`BGroup::UpdateTemplates` explicitly retains a type-propagation warning and
unrecovered dispatch cases; the focused paths cited here remain visible.

## Targets, source polarity and intensity

`BModTargetBase::staticInit`, `0x100419c48.c:25-41`, maps `volume` to target 2,
`pan` to 3 and `pitch` to 6. `BGroup::GenSysModScript`,
`0x1004c6e14.c:1911-1953`, classifies pan and pitch as additive targets;
`:2194` emits `$out := $in + shaped_signal * $intensity`.

Pitch modulation values are octave offsets. `BDsp::ProcessVoice`,
`0x10046772c.c:598-644`, selects target buffer `0x60` (target 6 times 16) and
converts its value with the polynomial/float-exponent approximation to `2^x`.
`BDsp::GetBlockSampler`, `0x100471750.c:97-101`, multiplies playback speed by
this modulation factor. Consequently intensity `1/6` with a full positive bend
means two semitones; there is no evidence for imposing one global bend range.

Source bipolarity is chosen from source enum, not target flag 4.
`UpdateTemplates`, `0x1003c4f1c.c:6134-6141`, tests source mask `0x352082`, whose
members are `[1,7,13,16,18,20,21]`. Of pitch bend (1), poly pressure (2), channel
pressure (3) and CC (4), only pitch bend is bipolar. Key position (7) has the
raw value `(note-60)/12` (`DispatchVoice`, `0x1003e6d08.c:101,144`).

Target flag 2 controls inversion:

- `ExpFuncFCInvNeg`, `0x10024d338.c:13-24`, negates when flag 2 is set.
- `ExpFuncFCInvFlip`, `0x10024d2f4.c:13-24`, maps `s` to `1-s` when flag 2 is set.
- The generator uses negation for pitch/pan and other additive/bipolar targets.
  A plain unipolar volume source is flipped. A plain bipolar volume source is
  optionally negated and then mapped to `(s+1)/2`. See
  `0x1004c6e14.c:2065-2085` and the range functions `0x10024d370.c` /
  `0x10024d3a0.c`.

Flag 4 is inspected for the special modulator-intensity target `0x19` only in
these generator branches (`:1986,:2152`). Treating it as a global polarity flag
would incorrectly center pressure/CC sources. Flag 16 records initialized
target depth; missing legacy depths are initialized in
`UpdateTemplates:6188-6201` (pitch defaults to approximately `1/6`).

## Volume combination and legacy simulation flag

For target 2 the generated formula is
`output = input * (1-(1-shaped_signal)*intensity)`.
Static sources in enum range 7 through 13 (including key position and attack
velocity) cube the **individual route's attenuation factor**:
`output = input * (1-(1-shaped_signal)*intensity)^3`.
Do not cube the aggregate gain after every unrelated route.

This follows `0x1004c6e14.c:2275-2304`: `bVar12` is true for source 7..13
(`:750-754`) and chooses `fcubic` around the route factor. `ExpFuncFCubic`,
`0x10024d2c8.c:13-21`, returns exactly `x*x*x`.

`simulateK2EnvTimeModulation` does not alter the supported pitch/pan/volume
formula. Pan/pitch leave via the additive branch before that flag is consulted.
For volume, the flag's branch at `:2253-2256` selects the empty output wrapper
and immediately jumps to `LAB_1004c8f04`; the unsigned comparison against target
`0xbf` succeeds for target 2. Thus it skips the envelope-time `fcubic` wrapper
at `:2268`. The default wrapper strings were checked in the original ARM64
binary: VA `0x109104157` is `$in`, VA `0x10910d03c` is the empty string.

## Initial values and channel state

An untouched CC with `initialValue=-1` must **omit its modulation route**.
It must not apply the volume law with a zero source and silence the instrument.

`UpdateTemplates`, `0x1003c4f1c.c:4282-4289`, sets runtime byte `+0xb0` for a
negative MIDI CC initial value; nonnegative initial values call `ResetToValue`
with `initialValue/127`. `BDsp::ProcessVoice`, `0x10046772c.c:525-530`, skips a
modulator with byte `+0xb0` set, independently of explicit bypass at `+0xc`.
`BExtMod::SetNextVal`, `0x1007705d0.c:25`, and `ResetToValue`,
`0x1007706a8.c:17-21`, clear this byte when a real value becomes available.

`MIDIControllerState::storeMIDIValue`, `0x100774120.c`, stores pitch bend,
channel pressure and CC values and timestamps by MIDI channel.
`getInitialVoiceValue`, `0x100774294.c:25-92`, selects channel-specific state,
or aggregate state for an unspecified channel, and rejects absent/stale
timestamps. `DispatchVoice`, `0x1003e6d08.c:103-112,177-178`, resets new voice
sources to a valid previously received value, clearing the untouched bypass.

Poly pressure is not stored by `MIDIControllerState::storeMIDIValue`.
`DispatchVoice:113-137` can inherit matching existing voice state; otherwise its
default for source 2 is zero. A broad pre-attack channel/key pressure cache is
not established by these functions. Live pressure dispatch matches the note
and group/instrument input channel (`DoExtModulationRunningVoices`,
`0x1003d3034.c:117-166`). Source flag at `+0x1d4` (serialized flag v1.03)
additionally broadcasts matched poly pressure through the group's voices;
implement this only after preserving the observed channel/note gating or
report it as unsupported.

## MIDI value normalization

Pitch bend is normalized asymmetrically around the exact center: negative
values divide their distance from center by `2^31`, positive values by
`2^31-1` in the expanded MIDI2 domain. MIDI1 conversion preserves center and
both endpoints. See `0x1003d3034.c:65-93` and `0x100774294.c:60-65`.

Running CC/channel-pressure sources use the 7-bit value divided by 127 when
runtime flag `+0x1d5` (serialized v1.04 flag) is false. When true, they use the
full expanded 32-bit normalized value (`0x1003d3034.c:144-160`). Poly pressure
always follows the expanded value in this path (`:130-139`). For MIDI1 value
`v`, the exact expansion in `0x100774120.c` / `0x1003d3034.c` is:

```
raw = v << 25
if v > 64:
    raw |= (v & 63) * 0x82082 | ((v >> 5) & 1)
normalized = raw / 2^32 if raw <= 0x80000000 else raw / (2^32-1)
```

This gives `64 -> 0.5`, whereas the legacy 7-bit path gives `64/127`.
Valid prior CC/channel-pressure state read for a new voice uses the expanded
value even when that source's running flag is false. Attack velocity likewise
selects between legacy 7-bit and expanded 16-bit normalization through that
flag (`DispatchVoice:79-90,146-151`). Cadence's public `Event.value` is already
normalized and cannot preserve arbitrary original 32-bit UMP resolution;
preserve that abstraction and document any normalization approximation.

## Tables and curves

The generator transforms source coordinates as follows
(`0x1004c6e14.c:2014-2033`):

| Source | Table input | Table output before inversion |
|---|---|---|
| Unipolar pressure/CC/velocity | raw source | raw table value |
| Pitch bend to pan/pitch | `(source+1)/2` | `2*table_value-1` |
| Pitch bend to volume | `(source+1)/2` | raw table value |
| Key position | `(source*12+60)/127` | `2*table_value-1` |

`ExpFuncTableVal`, `0x10024cf24.c:27-45`, clamps input to 0..1 and linearly
interpolates the 128-entry table at `input*127`. Handle the last entry explicitly
instead of reproducing the native provider's apparent one-past-end expression.
Inversion follows shaping. For volume, a shaped table with output 0..1 uses
`1-value` when inverted; additive targets negate.

Active curves are sampled into a 128-entry table at `i/127` by
`GenSysModScript:1335-1341`. `BModCurve::GetCurveValue`,
`0x10041c7dc.c:47-88`, selects consecutive breakpoints, uses the outgoing
curvature on the lower point, changes its sign for a descending segment, and
uses straight interpolation when `abs(curvature*16) <= 0.01`. For a segment
fraction `t`, let `k=curvature*16`, reversing its sign when the segment descends.
The interpolated fraction is `(exp(k*t)-1)/(exp(abs(k))-1)` for `k>0.01`, and
`1-(exp(abs(k)*(1-t))-1)/(exp(abs(k))-1)` for `k<-0.01`.

The saved native code approximates each exponential by constructing an IEEE754
float from integer bits: with `x=input*1.442695`, `f=x-trunc(x)`, the bit pattern
is `trunc((x+127+(f-f*f)*-0.33971)*8388608)`. The sampler compiles bounded
ascending/descending curved and linear segments into the native `i/127`
128-entry table during loading. Independent audio tests compare both curvature
signs and both directions against mathematical exponentials with a 0.003 gain
tolerance, accounting for the recovered fast approximation. Repeated or
non-increasing breakpoint x coordinates remain explicit format errors.

## Smoothing

Signed smoothing values are read from target offset `+6`.
`GenSysModScript:1871-1894` computes:

```
D = abs(smoothing_ms) / 1000 * sample_rate / 32
positive: alpha = 1 - exp(-log(100)/D)
negative: N = trunc(D*0.25 + 0.5)
zero: no smoothing
```

The positive generated callback initializes its first value immediately,
then updates `b += (target-b)*alpha` when `abs(target-b) >= 1e-5`
(`:2129`). It does not explicitly snap to the target below that threshold.
The negative callback initializes immediately, starts a linear ramp of N
control ticks when needed, and preserves the current step until the ramp
counter expires even if a new target arrives (`:2114`). Guard a zero derived
ramp duration instead of reproducing a possible divide-by-zero.

Native modulator processing has a 32-frame raster and separate interpolation
(`BDsp::ProcessVoice:610-611,695-697`). A continuous sample-rate-equivalent
exponential smoother (99% settling within the specified milliseconds) or
continuous linear ramp is an explicit approximation. It can be block-size
independent and musically useful while native raster/interpolation parity
remains unverified.

## Supported initial implementation boundary

The most directly supported subset is pitch bend, channel/poly pressure, CC
and attack velocity routed to pitch/pan/volume, with instrument-defined depths,
inversion, untouched CC omission, explicit CC fallback, bypass, legacy
poly-pressure group broadcast, and active tables/linear or exponential curves.
Other destinations/source types, arbitrary UMP resolution, legacy
negative-intensity/depth migration and exact native smoothing raster still need
explicit handling or diagnostics rather than silent approximation.
