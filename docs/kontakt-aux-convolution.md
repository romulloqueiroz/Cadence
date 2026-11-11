# Aux sends and convolution

Implemented 2026-11-07 for the stereo sampler: ordered group/program send taps,
eight parallel returns, and mono/stereo IRC in program insert/master/send slots.
This covers the installed Piano Uno IRC settings and independent fixtures;
custom IR shaping and full Kontakt DSP parity remain separate work.

## Recovered topology and wire format

Evidence is the saved Kontakt 8.14.1 ARM64 Ghidra export under
`../../Kontakt-8-REA/pseudocode/functions/`, not original source or a fresh
runtime comparison. Original provider limitations remain applicable.

- `BParFXSendLevels::doRead`, `0x1007854a4.c`: type 23, versions 0x50/0x51,
  counted eight-send array; 0x51 also has 17 sidechain-send fields.
- `BParFX::SetObj`, `0x10040cefc.c:435-454`: send levels are copied directly
  into the runtime gain fields. They are linear gains, not dB or cubed UI values.
- `BEffectSends::Process`, `0x10050e100.c`, is a no-op on dry audio.
  `BDsp::PutSignalToSendFX`, `0x1003ac1f8.c`, and
  `BInsertBus::PutSignalToSendFX`, `0x100409434.c`, accumulate into sends.
- `BProgram::ApplySendFX`, `0x10042b204.c`, processes separate return inputs;
  `mixInSendFX`, `0x10042b70c.c`, mixes the returns into the program.
- `BParFXIRC::doRead`, `0x100785acc.c`: type 22, versions 0x60/0x70;
  rate factor, block size, predelay, ER/LR length and EQ, crossover, five flags,
  two counted eight-point envelopes and a signed filename reference.
- `SER::ReadBFNTrnsSpecialType`, `0x10077c874.c`, resolves that filename-table
  index. `-1` denotes no IR. The normal sample resolver already handles the
  factory `.nkrResources/...` reference.
- `BEffectIRC::Process`, `0x1005c9614.c`, applies separate dry and wet output
  levels. `RebuildIRC`, `0x1005c7e7c.c:452-529`, computes auto-gain from the
  maximum channel energy: unity below 0.001, otherwise
  `min(2, sqrt(1 / (2 * max_energy)))`.

The real Concert preset has an empty send-FX array. Its send-level slot is in
program inserts, while IRC is master slot 6, before the limiter. Thus its hall
is a serial wet/dry insert, not a send-return chain. Earlier handoff descriptions
conflated these paths. Synthetic fixtures independently validate parallel sends.

## Implemented processing

- Every supported group send taps the signal at its physical slot. The native
  `fxIdxAmpSplitPoint` (default6) separates pre- and post-amplifier effects.
  Pre-amplifier taps precede voice gain/envelope/pan/fade; post-amplifier taps
  already contain those operations. They are not multiplied a second time.
  See `group-fx-runtime.md` and the native placement evidence.
- Program inserts run in order and send from their slot positions. Each of eight
  return slots processes only its own accumulated input. Returns sum with dry
  audio before master inserts. Empty and bypassed slots retain their addresses.
- Send slots leave dry audio unchanged, including their common output-level
  field, matching the recovered no-op processor. IRC uses its own decoded
  linear wet/dry gains. An unassigned IR contributes dry only; a bypassed IR
  neither loads its asset nor contributes a return/tail.
- A fixed 256-frame aux scratch buffer is allocated during load. Event-delimited
  rendering is divided into bounded chunks; program FX run immediately after
  each chunk. No buffer growth depends on callback size or note count.
- IRs are indexed, checked, decoded and prepared through the normal WAV/NCW and
  resource/retail sample paths off the audio thread. Missing/malformed IRs fail
  load explicitly. Non-finite data, unsupported channel counts and oversized
  assets fail before unbounded decoding/preparation.
- The convolution engine uses a direct 256-tap FIR head and 512-point FFT
  partitions for the rest. Input histories and overlap state persist across
  host and event boundaries. This adds zero algorithmic latency; convolution
  tail length is included in capabilities/current-tail reporting.
- Mono IRs filter L/R independently; stereo IRs are diagonal L-to-L/R-to-R.
  There is no implicit crossfeed or four-channel true-stereo interpretation.
- Predelay is a prepared delay ring. Reverse and auto-gain are applied during
  preparation. Sample-rate changes rebuild kernels off the callback using
  linear resampling with zero extension at the final tap.
- Reset clears histories, live-spectrum flags, pending overlap and predelay
  without allocating or freeing. Invalid preparation silences FX, reports a
  processing error, and clears histories before a later valid recovery.

## Bounds and explicit limits

- Source IRs: mono/stereo, at most 1,048,576 frames per asset, aggregate source
  stereo-equivalent budget 2,097,152 samples. Prepared kernels share the same
  2,097,152-sample budget, counting the two independent processing channels.
- Prepared rate: 8–192 kHz subject to that budget; predelay: 0–2000 ms. Long IRs
  at high rates may fail preparation. Owned IR/FFT/history bytes are exposed by
  `SamplerPlugin::convolution_memory_bytes`, separate from sample-page telemetry.
- This increment supports natural ER/LR lengths 1, default 20/20000 Hz IR EQ,
  inactive envelopes, and auto/1 rate-factor settings. Other settings fail
  explicitly. The default IR EQ is treated as unshaped; native default-filter
  coloration is not claimed. Crossover/preserve-length metadata has no effect
  when ER/LR stretching and shaping are inactive.
- Native rate reduction, native convolution block sizes, coefficient/kernel
  interpolation and arbitrary true-stereo routing are not reproduced. Cadence
  uses its own full-rate, zero-latency partition schedule and linear resampler.
- Sidechain-send fields remain metadata. Send-to-send/master-to-send feedback,
  nondefault return output partitions and per-voice group convolution fail
  explicitly. Dynamic send modulation, FX editing/automation and exact group
  pre/post-amplifier placement remain open.
- Static preset recall reloads the same IR and gains; reset removes tails.
  The sampler's opaque state still stores its output gain only; editable FX
  parameter state and captured DSP histories are not implemented.

## Validation and artifacts

Nine new regressions cover malformed/truncated send/IRC records; independent
naive FIR comparison across FFT partitions; sparse taps/returns and ordering;
parallel summing before master FX; separate group sends; stereo isolation;
wet/dry, predelay, reverse, auto-gain and resampling; reset/preparation recovery;
missing/bypassed IRs, bounds and feedback rejection; exact output across
1/64/127/257/1024-frame blocks; zero callback/reset allocations/deallocations;
and real IR tails after all voices are killed.

The sampler/format suites pass 182 tests. Real Piano Concert:

- Resolves `Concert Hall A.WAV` from `Piano Uno 1.0.nkr`: stereo, 44.1 kHz,
  158,202 frames. All six installed Piano Uno presets use compatible IRC
  settings; their parameter inventory is retained separately from audio tests.
- At 48 kHz: convolution tail 172,192 samples (~3.59 seconds), owned convolution
  storage 12,302,208 bytes. Sample resident/page-cache telemetry excludes this.
- A distinct wet tail remains after CC120 finishes fading/killing voices.
- Streamed/RAM chord-release output is exactly equal over 900 blocks; no sample
  streaming underruns or read errors. Playback and conformance pass.
- The optimized paced run completed 600 callbacks of 256 frames with zero
  streaming underruns; the slowest callback was 1,187 us against a 5,333 us
  budget. This is a finite local measurement, not a hard realtime guarantee.
  Debug timing can exceed that budget; both logs are retained.

Workspace Clippy warnings-denied, formatting and standalone/AU/VST3/test builds
pass. CTest is blocked at fixture creation by the managed sandbox's shared-memory
permission (`Operation not permitted`). No installed plugin was replaced and no
live Logic/Kontakt comparison is claimed.

Evidence, hashes, timing logs and the wet Piano render:
`../app/build/verification/aux-convolution-2026-11-07/`.
