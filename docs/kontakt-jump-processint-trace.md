# BEffectJump::ProcessInt trace

Static evidence from saved Ghidra 12.1.4 output at
`../../Kontakt-8-REA/pseudocode/functions/0x1005cc6ac.c` (`ProcessInt`),
`0x1005ca0a0.c` (`Initialize`), and `0x1005caf30.c` (`SetSampleRate`).
These files are decompiler output, not original source or observed execution.
The ordered scalar recurrences are recorded in
[kontakt-jump-scalar-graph.md](kontakt-jump-scalar-graph.md).

## Corrections to the earlier handoff

The earlier six-stage-cascade interpretation was wrong. The inner counter
runs `0,-1,-2,-3`, processing four oversamples per host frame. The two SIMD
banks carry distinct circuit branches:

- Bank A: Boost, Boost, Stage1, Stage2.
- Bank B: PowerTube, PowerTube, SplitterP, SplitterN.

Their state feeds later oversamples; replacing them with six immediate
one-pole stages loses that timing and feedback. Six source curves do not
imply a six-step processing loop. The 16 initialized state records are
channel records with stride `0x120`, not 16 amp stages.

`Initialize`'s `param_2` is a `long*`. Multiplying its slots by eight gives:

| Slot | Byte offset | Table |
| --- | ---: | --- |
| `0x914` | `0x48a0` | Boost (`0x13ec`) |
| `0x915` | `0x48a8` | Stage1 (`0x1c0c`) |
| `0x916` | `0x48b0` | Stage2 (`0x242c`) |
| `0x917` | `0x48b8` | PowerTube (`0x3c8c`) |
| `0x918` | `0x48c0` | SplitterP (`0x2c4c`) |
| `0x919` | `0x48c8` | SplitterN (`0x346c`) |

The previous trace swapped the last three pointer labels. It also placed
`+0x48f4` / `+0x48f8` in the wrong lanes. The native rate block replicates
`48f0=48f4` and `48f8=48fc`; `JumpRateBase` now follows that packing.

## Rate-dependent coefficient port

`JumpRateCoefficients` in `crates/sample-adapter/src/amp_dsp.rs` now ports:

- All eight prototype vectors at `+0x4630..+0x46ac`.
- All 23 interleaved circuit vectors at `+0x4730..+0x489c`.
- The six rate/smoothing/history vectors at `+0x48d0..+0x492c` through
  `JumpRateBase::vectors()`.
- Response gains `+0x49c0/+0x49c4`, smoothing period/factor, initial drive
  current and channel state gain.

The entry rate is multiplied by four at the start of `SetSampleRate`.
Its later bilinear-transform scalar is twice that internal rate, hence
`8 * host_rate`, not `2 * host_rate` as the earlier trace stated. The
`+0x46b0..+0x472c` FIR vectors are initialized separately; the setter does
not write them. `+0x4930..+0x49bc` contains table metadata populated by
`Initialize`; it is not a missing bulk rate block. The setter does not
write `+0x49cc`.

The inputs to this coefficient build are native fields: existing drive
target `+0x45fc`, input scale `+0x44c8`, power scale `+0x44d0`, response dB
`+0x44b8`, response offset `+0x44bc`, channel flag `+0x44c4`. In particular,
`SetSampleRate` consumes the pre-existing drive target before recomputing
that target at its exit. Normalized controls need their setters first.

`DAT_104cc8330` is a file-backed vector `[24,24,0,0]`. The nine apparent
BSS tuning globals are ExpTable entries at indices
1927/1585/1586/2106/1996/2031/1746/2021/2026 plus `0.8815918`.
`JumpTuning::new` reconstructs them.

The independent Python replay `tests/fixtures/jump_rate_reference.py`
checks the source image SHA-256, resolves the literal vector through
Mach-O segments, and produces `jump-rate.tsv` plus provenance. All 4,396
values (157 fields × 28 cases) match the Rust scalar build exactly. Cases
span 8/44.1/48/96/192 kHz, drive targets, input/power scales, response and
channel branches, plus low-rate/ExpTable clamp cases. Both scalar versions
replace NEON reciprocal estimate/refinements with float division and omit
instruction-level FMA contraction; this does not establish bit parity with
the native instructions or audio parity.

## Lookup and smoothing

The native lookup clamps only the index selection. With `q` already
including its table fraction, `lo=-anchor` and `hi=255-anchor`:

```
i = trunc(clamp(q, lo, hi) - lo)
shaped = entries[i].rel + entries[i].slope * (q - (lo + i))
```

Interpolation uses the original `q`, including outside the index range.
The earlier `input - (state + anchor)` description was incorrect.

The shared `SmoothingToolbox<24>` currents/targets/increments start at
`+0x44f0/+0x45b0/+0x4550`. `ProcessInt` advances currents per oversample
and regenerates the drive-dependent lane when its update flag is set.
`Smoothing24` alone does not reproduce the full block/countdown behavior.

## Remaining implementation

1. Port and verify the ordered 4× oversampled circuit graph, fixed FIRs,
   lane metadata, initial states, table interpolation and feedback timing.
2. Port the full control-target and drive-rebuild scheduling before replacing
   `JumpAmp` playback. Type 77 already uses `AmpJump`, but its present
   runtime is still the documented approximation and does not consume the
   newly ported coefficient foundation.
3. Capture an identity-verified native Kontakt JUMP render and compare raw,
   aligned and gain-fitted audio, including a native repeat.

No native JUMP audio parity is claimed. Other amps and the wider FX matrix
remain subsequent work.
