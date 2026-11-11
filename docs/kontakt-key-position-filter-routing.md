# Key position and indexed filter-cutoff modulation

Implemented 2026-11-07. Piano Uno Concert now loads, renders and passes the
real-library sampler checks; the previous modulation-source rejection is fixed.
The later [six-model filter increment](kontakt-filter-fidelity.md) supersedes
the approximate cutoff mapping and biquad-state limits below for legacy LP/HP
one/two-pole and SV1 filters. The note/source routing evidence still applies.
[Control timing and smoothing](kontakt-filter-control-timing.md) now run for
those six models as well.

## Recovered behavior

The implementation reuses saved Kontakt 8.14.1 ARM64 Ghidra pseudocode under
`../../Kontakt-8-REA/pseudocode/functions/`. These are static observations with
the original provider limitations, not fresh Kontakt runtime comparisons.

| Behavior | Saved function |
| --- | --- |
| Source 7 starts at `(note - 60) / 12` | `BDsp::DispatchVoice`, `0x1003e6d08.c:101,144` |
| Key-position table input is `(source * 12 + 60) / 127`; table output is bipolar | `BGroup::GenSysModScript`, `0x1004c6e14.c:2014-2033` |
| Key-position volume uses the individual route's cubed attenuation | `GenSysModScript`, source classification at `:743-754`, volume at `:2275-2304` |
| FX modulation has a distinct slot address | `BGroup::InsertModIdxFX`, `0x100407104.c`; `BDsp::HandleVoiceEfx`, `0x1003aa3a8.c:286-311` (cutoff buffer `target 0x1f * 16 + slot`) |
| Cutoff modulation operates on the normalized filter parameter | `BFilterBase_MF::Process`, `0x100620000.c`; SV wrapper `0x1006280a0.c` |
| Serialized filter IDs 50/51 map to runtime 0x2a/0x2b | `BParFXFilter::fixtypeToType`, `0x1003e1ba0.c` |
| Those types instantiate SV1 and select low/high output | `BGroup::UpdateTemplates`, `0x1003c4f1c.c:4375-4382`; `BFilterSV1::setTypeFromKontaktEnum`, `0x10061f5ec.c`; `FilterSV1::setFilterType`, `0x100642948.c` |
| SV1 steady-coefficient recurrence is a first-order trapezoidal filter | `FilterSV1::processAudio`, `0x100643108.c` |

Existing transformation/curve interpolation evidence is in
[kontakt-external-mod-runtime-evidence.md](kontakt-external-mod-runtime-evidence.md).

## Playback and bounds

- Key position is evaluated from each voice's note independently of sample
  key tracking. Pitch, pan, volume and cutoff routes share the existing depth,
  inversion, smoothing and transformation machinery. Plain key position is
  bipolar; table/curve lookup uses note coordinates and bipolar output.
- Indexed external cutoff targets 0..7 address the original binary group FX
  slot. Sparse, bypassed and repeated effects do not renumber later targets.
  Bypassed filters remain valid destinations without affecting another slot.
- A target must refer to a decoded supported simple group filter. Invalid,
  missing or unsupported nonzero targets fail during load with a diagnostic.
  Modulation script-slot routing (`slotIdx != -1`) remains unsupported.
- Indexed cutoff offsets add in normalized parameter space, are bounded to
  0..1 with the base value, then pass through Cadence's existing approximate
  20 Hz–20 kHz cutoff mapping. Existing unindexed XML/internal cutoff routes
  retain their octave-offset behavior. Exact native frequency response and
  control-rate interpolation remain separate fidelity work.
- SV1 low/high subtypes 50/51 are no longer skipped. First-order slots now use
  a bilinear first-order transfer instead of the old two-pole biquad. This
  preserves the first-order steady transfer; native normalized coefficient
  mapping and coefficient interpolation are not claimed exact.
- Filter coefficient caches persist with each voice/filter, rather than
  restarting at every block or event segment. This keeps the existing 0.01
  octave update threshold independent of host segmentation. Sample-rate
  changes invalidate cached coefficients; reset/reuse starts new voice state.
- Eight fixed cutoff accumulators and preallocated per-voice filter/route
  storage keep rendering and reset free of allocations and deallocations.

## Validation

The stricter installed-Piano test reproduced the old source-7 error before the
fix. All four real Piano tests now fail on load errors when the fixture exists;
only an absent file skips, with an explicit message.

Seven new tests cover raw key-position values, tables/curves/inversion, the
individual volume law, sparse/indexed/bypassed target isolation, invalid target
rejection, first-order frequency response, XML loader-to-audio pitch ratios,
resident/streamed and 64/257-frame equality, and callback/reset allocation
checks. The complete sampler/format suites report **173 passed, 0 failed**.

Real Piano Uno Concert:

- Offline playback and plugin conformance pass.
- 111 samples: 327,657,600 bytes full RAM; 21,954,560 bytes resident attacks
  plus 16,777,216-byte page cache in streaming mode.
- 900 blocks of a chord/release performance: streamed/full-RAM sample error
  is exactly zero; no streaming underruns or read errors.
- 600 paced 256-frame callbacks: zero streaming underruns. Maximum measured
  callback 1,298 microseconds; finite local measurement, not a scheduling guarantee.
- An explicit three-second streamed CLI render produces 144,000 stereo frames,
  peak about 0.089, RMS about 0.0101.

Logs, hashes, report and the rendered WAV:
`../app/build/verification/key-position-filter-routing-2026-11-07/`.
Workspace Clippy with warnings denied, formatting, and standalone/AU/VST3/test
builds pass. CTest cannot instantiate its loading fixture because this managed
sandbox denies shared-memory creation (`Operation not permitted`). No installed
plugin was replaced and no live Logic or Kontakt comparison is claimed.

## Next

[Aux/send routing, wet returns and convolution](kontakt-aux-convolution.md) are
now implemented for the supported settings. Internal modulator
object targeting, other effect parameters, exact DSP responses, pre/post-amp
placement and the wider compatibility backlog remain open.
