# Envelope curves and internal LFOs

Implemented 2026-11-07 as the first stage-2 increment of the
[Kontakt compatibility backlog](kontakt-compatibility-roadmap.md).

## Curved AHDSR attack

The binary `PAR_INTERNAL_MOD` payload decodes the same six floats already used
for the amplitude envelope. `BParEnv_AHDSR::doRead` shows the wire order:
curve, attack, decay, hold, release, sustain, then the AHD-only flag. The curve
value is the serialized `atkCurve`/`atkCurving` field and is bounded to -1..1.

Cadence applies that curve to the **attack segment only**, using the recovered
segment interpolation shared with modulation tables:

- `atkCurve * 16` is the signed curvature `k`.
- `|k| <= 0.01` is linear.
- `k > 0` uses `(e^(t*k) - 1) / (e^k - 1)`.
- `k < 0` uses the mirrored descending form.
- The interpolation uses the recovered bounded fast-exponential approximation;
  no exponential runs in the audio callback.

This law is the same one used by `BZoneLevelEnv::RenderValue` and
`BModCurve::GetCurveValue` (see the external-modulation evidence). The decay,
hold, sustain and release segments stay linear until their own curvature fields
are decoded. XML `atkCurving` and binary `curve` both reach this path.

The amplitude envelope's block-1 target intensity is now retained and applied
as `1 - (1 - level) * intensity`, matching the volume-target law used by the
external and LFO routes. A partial intensity (0..1) is therefore supported;
multiple amplitude envelopes are still rejected explicitly.

## Other envelope targets

Internal AHDSR modulators targeting `pitch` or `pan` are now applied per voice
alongside the amplitude envelope. One modulator may carry several targets, each
with its own intensity. Pitch uses `level * intensity` octaves and pan uses
`(level * 2 - 1) * intensity`, reusing the LFO target laws. These unit choices
are Cadence policies: native dB/octave/semitone display conversions have not
been corroborated against the runtime. `filterCutoff` targets remain metadata
for the effects stage.

XML `IntModulators` now populate the same internal-modulator list (AHDSR only),
so XML instruments get pitch/pan envelope targets and partial amplitude depth
as well as binary containers.

## Internal LFOs

`BParLFO::doRead` gives the wire layout after the 34-byte base header:

```
s32 kind, f32 delay, f32 frequency, f32 pulseWidth, f32 startPhase,
bool normalizeMultiLFO
```

`BParLFO::PrsSetValues` corroborates the XML names (`delay`, `frequency`,
`pulseWidth`, `startPhase`, `normalizeMultiLFO`) and the kinds: sine,
rectangle, triangle, sawtooth, random and multi. `BLfoBase::Advance` confirms a
normalized phase advanced by frequency/sample rate; `BLfoBase::Reset` restarts
from the configured start phase.

Cadence plays per-voice, free-running sine, rectangle, triangle and sawtooth
LFOs to volume, pan and pitch. The serialized intensity is the route depth, and
the existing external-route laws are reused:

- volume: `1 - (1 - (value + 1) / 2) * depth`, depth clamped to 0..1
- pan: `pan += value * depth`
- pitch: `octaves += value * depth`

Waveform phase conventions (where each waveform starts) are Cadence-standard,
not independently verified against Kontakt. `Random` and `Multi` LFOs are
decoded into the model but remain metadata only.

## Boundaries

- `ENV_DBD`, `ENV_FM7`, multiple amplitude envelopes, filter-cutoff envelope
  targets and per-target envelope curves are not decoded or played yet.
- LFO tempo sync, note-value/legacy rate fields, native dB volume intensity and
  random/multi playback are not implemented.
- XML internal modulators decode AHDSR envelopes and `<LFO>` children
  (`type`, `delay`, `frequency`, `pulseWidth`, `startPhase`,
  `normalizeMultiLFO`); binary and XML LFOs now share the same playback path.
- No C ABI, IPC layout or saved-state schema change is introduced.

## Evidence

- `../../Kontakt-8-REA/pseudocode/functions/0x100788458.c`
  (`BParEnv_AHDSR::doRead`)
- `../../Kontakt-8-REA/pseudocode/functions/0x1004674c8.c`
  (`BZoneLevelEnv::RenderValue`)
- `../../Kontakt-8-REA/pseudocode/functions/0x10041c7dc.c`
  (`BModCurve::GetCurveValue`)
- `../../Kontakt-8-REA/pseudocode/functions/0x100787e54.c`
  (`BParLFO::doRead`)
- `../../Kontakt-8-REA/pseudocode/functions/0x1004d40ec.c`
  (`BParLFO::PrsSetValues`)
- `../../Kontakt-8-REA/pseudocode/functions/0x10016792c.c`
  (`BLfoBase::Advance`)
