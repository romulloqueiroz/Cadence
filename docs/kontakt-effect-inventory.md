# Kontakt FX inventory

Inventory of the 68 serialized `BParFX` effect types recovered from the Kontakt 8.14.1 ARM64 image. The serialized type is the return value of each `BParFX<Class>::getSerType`; the payload reader is the matching `BParFX<Class>::doRead`. This is the roadmap for the wider effect matrix.

Status means Cadence has a playback path for that type. "open" means parser/DSP work remains. This inventory is structural evidence; it does not assert native audio parity for any effect.

| Type | BParFX class | BEffect DSP class | doRead | Status |
| ---: | --- | --- | --- | --- |
| 16 | `BParFXDelay` | `BEffectDelay` | `0x100783574` | implemented |
| 17 | `BParFXChorus` | `BEffectChorus` | `0x100783750` | implemented |
| 18 | `BParFXFlanger` | `BEffectFlanger` | `0x100783908` | implemented |
| 19 | `BParFXGainer` | `BEffectGainer` | `0x100780908` | implemented |
| 20 | `BParFXPhaser` | `BEffectPhaser` | `0x100783b08` | implemented |
| 21 | `BParFXReverb` | `BEffectReverb` | `0x100784abc` | implemented |
| 22 | `BParFXIRC` | `BEffectIRC` | `0x100785acc` | implemented |
| 23 | `BParFXSendLevels` | `BEffectSends` | `0x1007854a4` | implemented |
| 24 | `BParFXFilter` | `BEffectFilter` | `0x1007829cc` | implemented |
| 25 | `BParFXCompressor` | `BEffectCompressor` | `0x10078013c` | implemented |
| 26 | `BParFXInverter` | `BEffectInverter` | `0x1007806c8` | implemented |
| 27 | `BParFXDYX` | `BEffectDYX` | `0x100780778` | implemented |
| 28 | `BParFXLimiter` | `BEffectLimiter` | `0x100780858` | implemented |
| 29 | `BParFXShaper` | `BEffectShaper` | `0x100780ec8` | implemented |
| 29 | `BParFXSurroundPanner` | `BEffectSurroundPanner` | `0x100780b80` | implemented |
| 30 | `BParFXDistortion` | `BEffectDistortion` | `0x100781040` | implemented |
| 31 | `BParFXStereoSpread` | `BEffectStereo` | `0x100780f78` | implemented |
| 32 | `BParFXLoFi` | `BEffectLoFi` | `0x100780dd0` | implemented |
| 33 | `BParFXSkreamer` | `BEffectSkreamer` | `0x100781190` | implemented |
| 34 | `BParFXRotator` | `BEffectRotator` | `0x10078179c` | implemented |
| 35 | `BParFXTwang` | `BEffectTwang` | `0x100781b00` | implemented |
| 36 | `BParFXCabinet` | `BEffectCabinet` | `0x1007828b8` | implemented |
| 66 | `BParFXTape` | `BEffectTape` | `0x1007802d4` | implemented |
| 67 | `BParFXTrans` | `BEffectTrans` | `0x1007803b4` | implemented |
| 68 | `BParFXSSLGEQ` | `BEffectSSLGEQ` | `0x100781288` | implemented |
| 70 | `BParFXSSLGBusComp` | `BEffectSSLGBusComp` | `0x10078152c` | implemented |
| 76 | `BParFXFBComp` | `BEffectFBComp` | `0x10078051c` | implemented |
| 77 | `BParFXJump` | `BEffectJump` | `0x100781c70` | implemented |
| 82 | `BParFXVan51` | `BEffectVan51` | `0x100781db0` | implemented |
| 83 | `BParFXACBox` | `BEffectACBox` | `0x100781f68` | implemented |
| 84 | `BParFXBassInvader` | `BEffectBassInvader` | `0x100783fac` | implemented |
| 84 | `BParFXHotSolo` | `BEffectHotSolo` | `0x1007820a8` | implemented |
| 85 | `BParFXCat` | `BEffectCat` | `0x1007826c8` | implemented |
| 86 | `BParFXDStortion` | `BEffectDStortion` | `0x1007824a8` | implemented |
| 87 | `BParFXPlateReverb` | `BEffectPlateReverb` | `0x1007825d0` | implemented |
| 88 | `BParFXCryWah` | `BEffectCryWah` | `0x100782808` | implemented |
| 89 | `BParFXGaloisReverb` | `BEffectGaloisReverb` | `0x100784bb4` | implemented |
| 90 | `BParFXReplikaDelay` | `BEffectReplikaDelay` | `0x100784dac` | implemented |
| 91 | `BParFXPhasis` | `BEffectPhasis` | `0x100783ce4` | implemented |
| 92 | `BParFXFlair` | `BEffectFlair` | `0x100784288` | implemented |
| 93 | `BParFXChoral` | `BEffectChoral` | `0x100784638` | implemented |
| 94 | `BParFXCoreCell` | `BEffectCoreCell` | `0x100784830` | implemented |
| 95 | `BParFXHilbertLimiter` | `BEffectHilbertLimiter` | `0x1007849a0` | implemented |
| 96 | `BParFXSupercharger` | `BEffectSupercharger` | `0x100785710` | implemented |
| 97 | `BParFXBassPro` | `BEffectBassPro` | `0x100782218` | implemented |
| 99 | `BParFXPsycheDelay` | `BEffectPsycheDelay` | `0x1005f60bc` | implemented |
| 100 | `BParFXRingModulator` | `BEffectRingModulator` | `0x1007818ac` | implemented |
| 101 | `BParFXRaum` | `BEffectRaum` | `0x1005f7f04` | implemented |
| 102 | `BParFXBite` | `BEffectBite` | `0x1005f0b60` | implemented |
| 103 | `BParFXDirt` | `BEffectDirt` | `0x1005f1820` | implemented |
| 104 | `BParFXFreak` | `BEffectFreak` | `0x1005f36f4` | implemented |
| 105 | `BParFXVibratoChorus` | `BEffectVibratoChorus` | `0x1005fe9d8` | implemented |
| 106 | `BParFXWowFlutter` | `BEffectWowFlutter` | `0x1005ff330` | implemented |
| 107 | `BParFXTwinDelay` | `BEffectTwinDelay` | `0x1005fcce0` | implemented |
| 108 | `BParFXEPPreamps` | `BEffectEPPreamps` | `0x1005f2438` | implemented |
| 110 | `BParFXFuzz` | `BEffectFuzz` | `0x1005f4b3c` | implemented |
| 111 | `BParFXBigFuzz` | `BEffectBigFuzz` | `0x1005efd64` | implemented |
| 112 | `BParFXStereoTune` | `BEffectStereoTune` | `0x1005fac4c` | implemented |
| 113 | `BParFXBeatMasher` | `BEffectBeatMasher` | `0x1005edb34` | implemented |
| 114 | `BParFXGater` | `BEffectGater` | `0x1005f4f00` | implemented |
| 115 | `BParFXBeatSlicer` | `BEffectBeatSlicer` | `0x1005ee324` | implemented |
| 116 | `BParFXReverseGrain` | `BEffectReverseGrain` | `0x1005f9ca4` | implemented |
| 117 | `BParFXTransposeStretch` | `BEffectTransposeStretch` | `0x1005fc650` | implemented |
| 118 | `BParFXSuperFast100` | `BEffectSuperFast100` | `0x1005fa87c` | implemented |
| 119 | `BParFXChainsaw` | `BEffectChainsaw` | `0x1005f0f54` | implemented |
| 120 | `BParFXSkreamerDlx` | `BEffectSkreamerDlx` | `0x1005fa304` | implemented |
| 121 | `BParFXKolor` | `BEffectKolor` | `0x1005f58d0` | implemented |
| 122 | `BParFXReverbDelight` | `BEffectReverbDelight` | `0x1005f96d8` | implemented |

Duplicate type numbers (29 Shaper/SurroundPanner, 84 BassInvader/HotSolo) come from `getSerType` functions whose decompiled bodies contain more than one return; those two pairs still need an independent `getSerType` check before their serializers are used.

## Implemented in this batch

- `BParFXInverter` (26, v0x60): two channel-invert flags; `Processor::Inverter` flips the selected channels.

- `BParFXGainer` (19, v0x50): one clamped linear gain; `Processor::Gainer` scales both channels.

Both are parsed by `FxChild::inverter`/`FxChild::gainer` and covered by `gainer_and_inverter_apply_to_their_serialized_parameters`.

Modulation batch (approximate, synthetic tests only):
- `BParFXDelay` (16, v0x50/0x51): time ms, feedback, pan, damping. `Processor::Delay` is a stereo delay line with damping and feedback; the dry/wet uses the common slot levels.
- `BParFXChorus` (17): depth/speed/phase. `Processor::ModulatedDelay` (15 ms base, 10 ms sweep).
- `BParFXFlanger` (18): depth/feedback/speed/phase/color. `Processor::ModulatedDelay` (2 ms base, 2 ms sweep).
- `BParFXPhaser` (20): depth/feedback/speed/phase. Eight-stage first-order all-pass cascade.

The parameter names and wire order come from `BParFX*::PrsSetValues` and `doRead`. The exact depth/speed unit laws and the native LFO/all-pass topology are not yet verified, so these are documented approximations. Tests: `delay_echo_and_feedback_follow_the_serialized_fields`, `modulated_effects_run_and_stay_finite`, `flanger_and_phaser_run_and_stay_finite`.

Dynamics batch: `BParFXCompressor` (25, v0x70) now parses model/threshold/ratio/attack/release/stereoLink and runs a feed-forward peak compressor. Covered by `compressor_reduces_loud_material_and_passes_quiet_material`. Models (classic/enhanced/pro) are not yet differentiated.

Drives batch: `BParFXDistortion` (30, v0x60: tube/transistor, drive, damping) and `BParFXSkreamer` (33, v0x50: tone/drive/bass/bright/mix) now run waveshaping paths. Covered by `distortion_and_skreamer_shape_and_stay_finite`. Curves/filters are recovered parameter names plus standard approximations.

Remaining drives: `BParFXDStortion` (86, six floats + mode), `BParFXFuzz` (110, three floats + mode) and `BParFXBigFuzz` (111, four floats + mode) now run a shared multi-band waveshaper. Their field names are inferred from the `SetDStortion`/`SetFuzz`/`SetBigFuzz` filter/gain structure; exact control laws remain approximate. Covered by `dstortion_fuzz_and_big_fuzz_run_and_stay_finite`.

`BParFXChainsaw` (119), `BParFXSkreamerDlx` (120) and `BParFXKolor` (121) now parse their `doRead` layouts and run through the shared drive paths (SkreamerDlx uses the Skreamer path, Chainsaw/Kolor the multi-band path). Kolor's remaining five fields are parsed but not yet assigned to distinct DSP stages. Covered by `chainsaw_skreamer_dlx_and_kolor_run_and_stay_finite`.

Time/pitch batch: `BParFXReplikaDelay` (90) uses the Delay path (first four floats), `BParFXGater` (114) is a periodic amplitude gate, and `BParFXVibratoChorus` (105)/`BParFXWowFlutter` (106) use the chorus modulated-delay path. Mix/tone fields are parsed but not yet mapped for the vibrato/wow effects. Covered by `replika_gater_vibrato_and_wow_flutter_run_and_stay_finite`.

Second time/performance batch: `Tape` (66), `Trans` (67), `PsycheDelay` (99), `TwinDelay` (107), `SuperFast100` (118), `StereoTune` (112), `BeatMasher` (113), `BeatSlicer` (115), `ReverseGrain` (116), `TransposeStretch` (117) now run. These are functional approximations (detune/beat-repeat/gate/reverse-grain), not native-verified time-stretch or pitch-shift models. Covered by `tape_trans_psyche_twin_and_superfast_run_and_stay_finite` and `performance_effects_run_and_stay_finite`.

Modulation/utility batch: `Phasis` (91), `Flair` (92), `Choral` (93) use the chorus modulated-delay path; `RingModulator` (100), `LoFi` (32) and `Rotator` (34) have dedicated carrier, bitcrush/downsample and stereo-rotation paths. Covered by `modulation_and_lofi_effects_run_and_stay_finite`.

Dynamics batch: `DYX` (27), `SSLGBusComp` (70), `FBComp` (76), `Supercharger` (96) and `HilbertLimiter` (95) map their recovered threshold/ratio/attack/release fields onto the feed-forward compressor/limiter path. Covered by `dynamics_effects_run_and_stay_finite`.

Filter/drive batch: `Twang` (35), `Bite` (102), `Dirt` (103), `Freak` (104) use the multi-band drive path; `CryWah` (88) uses a resonant band-pass wah. Covered by `filter_and_drive_effects_run_and_stay_finite`.

Historical initial approximation (superseded by the recovered amp/cabinet ports): the amp/cabinet family (`Cabinet` 36, `Jump` 77, `Van51` 82, `ACBox` 83, `BassInvader`/`HotSolo` 84, `Cat` 85, `BassPro` 97, `EPPreamps` 108) maps its first six floats onto multi-drive; `SurroundPanner` (29) maps the first source to stereo pan; `CoreCell` (94) uses multi-drive; `Reverb` (21), `PlateReverb` (87), `GaloisReverb` (89), `Raum` (101) and `ReverbDelight` (122) use a parallel-comb reverb approximation. Covered by `amp_family_runs_and_stay_finite` and `panner_core_cell_and_reverbs_run_and_stay_finite`.

Type 29 is shared by Shaper (v0x80) and SurroundPanner (v0x70); the panner is matched first and the shaper only accepts v0x80. Type 84 is shared by BassInvader (v0x52) and HotSolo (v0x10); each now selects its own recovered amp circuit.

## Fidelity pass status

Started 2026-11-08. This tracks which playback paths use recovered laws rather
than a standard approximation. "Exact" means the audible control law was read
from the decompiled `Set*`/`Process` code; it is not a native-parity claim.

- Exact/recovered: `Filter` (prototype/resonance laws), `SSLGEQ` (G_EQ curves
  and shelf topology), `Shaper` (embedded tables), `Limiter` (input gain `2^(dB/6)`,
  output level and the linear `ln2/N` release), `Compressor` (natural
  threshold/slope units, classic/enhanced static curves, native attack/release
  coefficients, Pro `ln2/N` release), `Chorus` (20 ms base delay, 75-sample
  depth, quadrature LFO), `Flanger` (base delay in field 5, 50-sample depth,
  `-0.95*feedback`), `Phaser` (four first-order all-passes, exponential
  coefficient curve, quadrature LFO), `SendLevels`, `IRC` (wet/dry/auto-gain),
  `StereoSpread` (width = 1 + spread, then pan), `Delay` (feedback scaled by
  0.98), `Cabinet` (11 measured rows, exact `SetCabinet` coefficient build,
  24-section even/odd pipeline cascade, two-line reflection block; see
  `kontakt-cabinet.md`).
- Partly recovered: `Compressor` Enhanced piecewise curve and Pro detector are
  ported structurally but not yet native-parity verified; `Phasis`/`Flair`/
  `Choral` still use the chorus path; the amp family's serialized layouts are
  now exact (`AmpSettings`); type 77 uses a table-based JUMP engine and the
  other amp variants now also have recovered processors. See `kontakt-amp-engine.md`.
- Approximate: reverb family, tape/transient, time-stretch/granular and the
  drive nonlinearities (distortion/skreamer/fuzz family). Amp circuits now have
  native numerical fixtures; full host lifecycle parity has narrower coverage. Cabinet is
  recovered except for its BSS-initialized delay-smoothing poles.

Next fidelity targets, in order: `FXFDelayLight` (delay engine), reverb
topology, then the amp/EQ curves.

Notes from this pass:
- The "ambiguous compressor argument order" was an ABI/decompiler artifact. The
  field order is fixed by `BParFXCompressor::doRead`/`PrsSetValues` and
  `BParFX::SetObj`; `SetCompressor` consumes threshold in `6*log2` units, ratio
  as the slope `1/R`, attack/release in ms. Classic uses `1-0.625^(1/N)` for
  attack and `1-(1/3)^(1/N)` for release; Pro uses `ln2/N`.
- The limiter release is the native linear `ln2/N` ramp (`CopyCoefficients`) and
  now passes the filter-fidelity regression.
- The flanger's fifth serialized field is the base delay in ms, not a colour
  control; the old implementation treated it as tone colour.
- Modulation/reverb/amp control laws are all readable from scalar `Process`
  bodies; the remaining work is porting, not native measurement.
- The recovered natural↔unity conversions now live in `nk-format/src/units.rs`
  (compressor atk/dcy/threshold/ratio, limiter gain/release, delay time, reverb
  pre-delay, cabinet tone, flanger colour), with endpoint/round-trip tests.
- Type 16 delay is `NI::DSP::FXFDelayLight`, a full engine (five styles,
  diffusion, saturation, sample-rate conversion, dozens of parameter ids). The
  v0x50 payload exposes only time/feedback/panning/damping; the style comes from
  the binary preset, so the current simple delay remains an approximation until
  the style controls are mapped. The other fields are not in the XML.
- `BParFXReverb` (type 21) fields are pre-delay, room size, stereo, colour and
  filter — not decay/damping/mix. `doRead`/`PrsSetValues` fix the order; the
  playback path now uses pre-delay, room-size feedback, colour damping, stereo
  width and the filter tone. The four-line topology is still an approximation.
  The other reverb types have different payloads (Galois 9 floats, Plate 5,
  Raum 13 + flags, Delight 11 + flags) and remain approximate until each is
  mapped from its `doRead`/`UnityToDisplay_*` pair. Raum and Delight are thin
  wrappers over separate DSP cores (`ni::effektrig::dsp::raum::Raum` and
  `fenderdeluxereverb::FenderDeluxeReverb`) whose `SetEffectParameters` map the
  serialized fields to parameter ids; Galois/Plate have no exported
  `SetEffectParameters` symbol, so their field→id map is still open.

## Suggested order

1. Simple fixed-DSP: `BParFXInverter`, `BParFXGainer` (done); `BParFXRingModulator`, `BParFXCryWah`, `BParFXLoFi` (bit/downsample) next.

2. Modulated delays: `BParFXDelay`, `BParFXChorus`, `BParFXFlanger`, `BParFXPhaser`, `BParFXPhasis`, `BParFXFlair`, `BParFXChoral`.

3. Dynamics: `BParFXCompressor`, `BParFXFBComp`, `BParFXSSLGBusComp`, `BParFXSupercharger`.

4. Drives/nonlinear: `BParFXDistortion`, `BParFXSkreamer`, `BParFXDStortion`, `BParFXFuzz`, `BParFXBigFuzz`, `BParFXChainsaw`, `BParFXKolor`, `BParFXSkreamerDlx`.

5. Time/pitch/performance: `BParFXTape`, `BParFXTrans`, `BParFXReplikaDelay`, `BParFXTransposeStretch`, `BParFXBeatMasher`, `BParFXBeatSlicer`, `BParFXGater`, `BParFXReverseGrain`, `BParFXStereoTune`, `BParFXSuperFast100`, `BParFXVibratoChorus`, `BParFXWowFlutter`.

6. Reverbs/content-dependent: `BParFXReverb`, `BParFXGaloisReverb`, `BParFXPlateReverb`, `BParFXRaum`, `BParFXReverbDelight`, `BParFXPsycheDelay`, `BParFXTwinDelay`, `BParFXBite`, `BParFXDirt`, `BParFXFreak`, `BParFXEPPreamps`, `BParFXCabinet`, `BParFXACBox`, `BParFXTwang`, `BParFXRotator`.

7. Amp/utility/special: `BParFXJump`, `BParFXDYX`, `BParFXSurroundPanner`, `BParFXCat`, `BParFXBassPro`, `BParFXBassInvader`, `BParFXHilbertLimiter`, `BParFXCoreCell`.

Already implemented: Filter, Limiter, SendLevels, Shaper, StereoSpread, IRC, SSLGEQ, Gainer, Inverter.

Each batch needs a synthetic impulse/sine fixture plus, where available, a native capture. Content-dependent reverbs/cabinets also need the referenced IR/sample assets.

