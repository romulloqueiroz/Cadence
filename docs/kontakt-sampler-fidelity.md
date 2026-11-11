# Controlled sampler fidelity, 2026-11-11

The normal-quality sampler now uses the recovered four-point Lagrange kernel
instead of linear interpolation. High/ultra retain their existing approximations.
Streaming requests the previous page for normal playback's extra left tap.
Unavailable outer taps retain the bounded linear fallback.

The GitHub source baseline is `aa3c6d4` (`main`, Cadence). Its pinned JUCE checkout,
seven-file patch, CI and release packager are retained. Verification supports
`--build-dir app/build-release`; local installation accepts
`Release app/build-release`. An older Debug build cache still references the
former cross-project JUCE checkout and must be regenerated before reuse.

## Fixed-input measurements

Six unchanged, identity-checked Piano Uno candidates exercise dry playback and
JUMP slot 3 before/after the amplifier boundary, each with one/two identical
voices. Rate is 48 kHz, host block 256, note 60/velocity 100 at frame 4800 and
note-off at 292800. No delay or gain fitting is applied to these full-length
480000-frame comparisons. Both voice counts give the same SNR:

| Case | Previous SNR | Updated SNR |
| --- | ---: | ---: |
| Dry | 54.03 dB | 56.31 dB |
| JUMP before amplifier | 42.60 dB | 50.45 dB |
| JUMP after amplifier | 42.63 dB | 50.37 dB |

In both engines, two identical voices still equal exactly twice one voice.
The new analytic regression reproduces asymmetric cubic data in stereo and
checks bounded edge handling. Existing streamed/resident, reverse/loop,
callback-partition and allocation checks exercise the changed playback path.

## Separating the amp from its input

A bounded original-Kontakt `BEffectJump::ProcessInt` trace observes input through
the object's channel pointers and output in `BProcessData`. Its input equals the
native dry recording bit-for-bit through frame 293023. Its first complete output
pass equals the native wet recording bit-for-bit over frames 4800..293376,
including 352 zero-input circuit frames. The instrumented and uninstrumented
native recordings also match exactly.

Feeding that same native dry input through Rust JUMP gives **81.56 dB raw SNR**
over the complete observed interval, with zero delay. The much larger
whole-instrument mismatch therefore originates upstream of the amp in this case.
This is one control setting/note/rate, not general instrument or tail parity.

## Remaining phase difference

Native normal playback reaches `BSample::ResampleChannelQuad_L_SSE2`. The entry
speed is observed as `0x3f8b2027` (1.0869187116622925). Cache-local single-precision
positions advance in four lanes, with scalar alignment/tail handling. Independent
emulation matches all **6916** captured before/after positions and produced-frame
counts exactly. Rounding accumulates relative to Cadence's double-precision clock.
A fitted replacement speed would conceal this behavior and is not implemented.

Matching that clock requires a separate change covering cache boundaries, the
unobserved initial/scalar fill path, pitch changes, reverse/loops, streaming and
callback partitions. Native guard samples, high/ultra sinc kernels and voice/tail
lifetime differences also remain open.

## Reusable evidence

Local artifacts are deliberately outside the source repository:

- `app/build/verification/group-amp-fidelity-2026-11-11/`: guarded capture scripts,
  executable/input hashes, identity checks, traces, exported REA evidence,
  isolated amp comparison and six-case `after-normal/cadence-comparison.json`.
- `app/build/verification/dry-sampler-investigation-2026-11-11/`: independent
  waveform replay, `check-native-phase.py` and `native-phase-check.json`.
- Installed Kontakt arm64 offsets: JUMP `0x5d3630`, normal resampler `0x4df780`.
  VST3 executable SHA-256:
  `1c6034abfd234f074c5ec84254ea30950a24a3cb7c16ef4b2dd810e2dd79471a`.
- Static dispatch/kernel sources are listed in
  [loops and interpolation](kontakt-loops-and-interpolation.md).

The first amp trace pass is complete; its second pass is partially buffered and
excluded from trace parity claims. Two initial resampler instrumentation attempts
captured no numerical records; only the validated third attempt is used.
