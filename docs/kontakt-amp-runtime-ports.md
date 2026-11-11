# Recovered amp playback, 2026-11-10

All eight amp variants use separate recovered processors. The version-specific
`AmpSettings` payload is retained through decoding and dispatch; none of these
variants uses MultiDrive. Each processor has prepare, reset, live-target and
stereo-frame entry points. Audio callbacks and sampler reset allocate nothing.

| Amp | Playback owner | Independent native coverage |
| --- | --- | --- |
| JUMP | `jump_dsp.rs` | Circuit/setup/rate fixtures, live controls, captured dry-input comparisons |
| VAN51 | `van51_amp.rs`, `van51_dsp.rs`, `van51_process.rs` | Polynomial/rate/initial-state and ProcessInt goldens; 448 observed host frames; actual dry-input host-state replay |
| ACBox | `acbox_dsp.rs`, `acbox_process.rs` | 20 initial states, five rates, static/live audio;90,112 scalar output samples match exactly on ARM64 |
| HotSolo | `hotsolo_dsp.rs`, `hotsolo_numeric.rs` | 16 cases across four rates and 6144 stereo frames, switches and live controls; bit-exact on ARM64 |
| CAT | `cat_amp.rs` | 40 control snapshots and 24×1024 native stereo frames across three rates, mono and four control sets |
| BassPro | `basspro_amp.rs` | 20 snapshots, 36×1024 settled frames and 4096 live-control frames, including graphic-EQ switches |
| BassInvader | `bassinvader_dsp.rs` | 26,624 static/live core frames, 8192 rate-wrapper frames and 2048 mode-transition frames; exact ARM64 output in tested cases |
| EPPreamps | `epiano_dsp.rs`, `epiano_amp.rs` | 21 cases and 10,752 core frames across all preamp/EQ/tremolo modes and live controls; exact ARM64 output; original serialized setter and complete rate-wrapper goldens |

The generators and SHA-bound provenance are under
`crates/sample-adapter/tests/fixtures/`. The source image is the recorded ARM64
Kontakt artifact with SHA-256
`42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.
Original numerical helpers supply the oracle; their outputs are independent of
the Rust arithmetic. The product contains reconstructed source and numeric data.
It never loads or executes the original image.

## Modern models and rate conversion

BassInvader and EPPreamps target an internal rate near 88.2 kHz. The exact policy is
`2^round(log2(88200/host_rate))`, with native ties-away rounding. The shared
`AmpResampler` retains original power-of-two interpolation/decimation kernels,
phase, histories and status. Nine factors from 1/16 through 16 pass both native
32-frame block and minimum-group streaming fixtures. Configurations contain
numeric data and canonical offsets; no original instructions are embedded.

At192kHz these models downsample the input by two. A per-sample callback must
collect that complete group, adding one causal buffering frame compared with
the original 32-frame host wrapper. Comparison tests state that exact offset;
they use no fitted delay or gain. Callback partitioning does not change it.
Common 44.1/48/96 kHz paths need no such additional frame. Ratios outside 1/16..16
are rejected during preparation rather than approximated.

EPPreamps retains its five neural preamp choices, six EQ choices and seven
tremolo choices, plus nonlinear shape, phase, mono and switch controls. Model
weights in zero-fill storage require four selected native data initializers;
the oracle recreates that BSS and runs only those numerical initializers before
capturing canonical model/state data. This avoids treating LINKEDIT file bytes
as initialized weights. Its Eigen decomposition/solve helpers have separate
original-native fixtures, including a dense pivoted inverse.

Native EPPreamps Suspend+Reset intentionally retains some operational state.
The native-reset method remains for numerical replay. Sampler reset uses an
allocation-free restart from the prepared numerical configuration so rewinding
is deterministic. Live setters retain the circuit history and native fades.
Tempo-synced EPPreamps speed follows the sampler's 120 BPM context; a new host
transport/individual-amp DAW automation API is outside this change.

## Verification scope

The shared checks cover serialized insert/master/send dispatch, wet gain and
bypass, stereo/mono, invalid-rate recovery, 44.1/48/96/192 kHz preparation, irregular
callback partitions, deterministic reset and allocation-free processing/live
ramps. Endpoint controls and additional 32 kHz/192 kHz cases are covered in the
individual amp suites. Native fixtures test specific cases rather than an
exhaustive parameter space.

Full Kontakt instrument/host lifecycle equivalence is separate from isolated
circuit parity. VAN51's observed lifecycle difference is explained in
`kontakt-van51-runtime.md`; the ordinary Cadence lifecycle remains explicit.
Build/test reports are retained in
`app/build/verification/kontakt-amp-ports-2026-11-10/`.

## Group/per-voice integration (2026-11-11)

The same eight recovered processors now run in group insert slots as well as
program insert/master/send chains. State is independent for every voice and
physical amp slot. The decoded amplifier boundary places slots before or after
voice gain/envelope/pan/fades and determines each send tap's signal. See
`group-fx-runtime.md`; this replaces the former group-amp omission.
