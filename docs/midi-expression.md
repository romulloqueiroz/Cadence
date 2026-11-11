# Instrument-defined MIDI expression

Cadence's Rust Kontakt sampler now decodes binary/XML external modulation routes
and applies pitch bend, channel/polyphonic pressure, MIDI CC, key position and
attack/release velocity to pitch, volume, pan and supported filter cutoff targets. An instrument without those routes keeps its existing
sound: there is no global bend range, pressure assignment or CC volume override.

## Playback behavior

- Pitch depths are octave offsets: depth 1/6 produces two semitones at full
  positive bend. Multiple routes add; the resulting octave value multiplies
  the voice's base tuning/sample rate. Pan offsets add to the existing balance.
- Volume routes multiply by `1-(1-source)*depth`. Key-position and attack-velocity
  routes cube their own attenuation factor; it does not cube other routes or the master gain.
- Each target retains its own depth, inversion, smoothing and transformation.
  Tables have 128 entries, clamped/interpolated at source position times 127.
  Curves are sampled onto that grid at load time, using recovered curvature
  and exponential approximation, including descending segments. No curve
  construction or parsing happens in processing.
- Release velocity is a per-onset source (runtime ID 9), not a live channel
  value. Its routes stay bypassed while the key is held and become active at
  termination, where the external note-off's data byte is normalized by 127.
  A MIDI1 note-on with velocity zero synthesizes release velocity 0x8000 in the
  internal 16-bit domain, treated as 0.5. Note-off velocity is carried on
  `Event.data2`; the sample adapter reads it directly. Release-triggered voices
  inherit the originating onset's release velocity.
- Volume routes are linear in release velocity; the cubed attenuation law
  applies to key position (source 7) and attack velocity (source 8).
- Pitch bend, channel pressure and CC events update the proper MIDI channel.
  Valid prior channel state initializes newly started voices. An untouched CC
  with negative `initialValue` omits its route; an explicit 0..127 initial value
  supplies the fallback until that controller is touched.
- Poly pressure requires a live matching note/channel. The serialized v1.03
  flag selects pressure for that key or legacy broadcasting to the matched
  group's voices, including other keys/channels. Another group without a
  matching seed is unaffected. Pre-attack pressure is not globally cached.
- First smoothing values initialize immediately. Subsequent positive values
  use a sample-rate-equivalent exponential with 99% settling in the specified
  milliseconds. Negative values use the recovered quarter-duration ramp rule
  and preserve an in-flight slope when the target changes.
- Reset clears channel expression and per-voice route state. Reused/stolen
  voices initialize their own state. Storage and curve tables are allocated
  during loading/preparation; expression events and rendering allocate or free
  no storage.

## File decoding

The external object is type `0x0c`, private versions `0x100..0x105`. Target
metadata precedes all target transformations. The external array is type `0x3c`:
before version `0x13` it has 32 implicit slots; version `0x13` carries a count up
to 64. Truncated arrays fail explicitly rather than dropping routes. Serialized
source IDs are converted through Kontakt's mapping instead of assumed to match
runtime IDs. Binary object and XML fields are retained in the playable model.

XML uses `ExtModulators/K2_ExtMod/Targets/Target` with direct `V` fields for the
source and targets. Both version attributes and `V name="version"` are read;
conflicting versions fail. XML before 1.0 requires migrations and is rejected.
Sparse `t0..t127` tables are interpolated; `xN/yN/cN` curve points are bounded
and ordered. Inactive transformations remain available in metadata.

The existing `Kontakt680Template.nki` fixture decodes its actual `VEL_VOLUME`
(source 8, depth 0.393701) and `PB_PITCH` (source 1, depth 0.166667, smoothing
250 ms) routes. These match the saved Kontakt default XML metadata.

## Explicit boundaries

This stage supports MIDI source types 1..4, key position type 7, attack velocity
8 and release velocity 9 on pitch/pan/volume and supported cutoff targets.
Indexed external cutoff destinations 0..7 retain binary group FX slot identity;
invalid/unsupported destinations fail loading. Key-position values, shaping,
cutoff units and validation are described in
[key-position filter routing](kontakt-key-position-filter-routing.md).
Nonzero unsupported external sources on supported destinations fail with a
group/target diagnostic. Zero-depth routes are inactive and omitted. Other DSP
destinations and zone-envelope source kind 2 remain in metadata for later
stages. Script-slot routing and the full KSP runtime remain unsupported.

CC and attack velocity use the existing protocol's seven-bit values divided by
127; pressure follows normalized `Event.value`, with the retained precision flag
selecting the legacy raw channel-pressure path for running voices. Native
expanded MIDI/UMP midpoint and endpoint normalization is not fully reproduced.
A new attack inherits poly pressure from a matching live key/channel voice in
the same group, as in the recovered initial-voice lookup; without a match, new
poly-pressure sources initialize to zero.

The smoother reproduces the recovered time law continuously, rather than
Kontakt's 32-frame control raster and interpolation. Original/native numeric
and runtime parity have not been established. Format counts, curve domains,
aggregate gain/pitch/pan and event storage have explicit Cadence bounds.
The basic `nk-format` `nki-render` utility retains its older renderer; use
`sample-host` or Cadence's production worker for expression playback.

No C ABI, shared-memory layout or saved-state schema change is introduced.
Workspace app/plugin bundles are rebuilt for this stage; installed versions
and Logic sessions are separate validation targets.

## Evidence and verification

The saved static observations, original REA Evidence ID and source references
are recorded in [wire evidence](kontakt-external-mod-wire-evidence.md) and
[runtime evidence](kontakt-external-mod-runtime-evidence.md).

Independent public audio fixtures verify frequency changes, initial/live state,
channel/key/group isolation, bypass/inversion, individual velocity attenuation,
tables/curves and smoothing. They compare exact streamed/resident output and
multiple block sizes. Allocator-guarded tests cover routed events, tables,
smoothing, voice reuse and reset in both resident and realtime-streamed paths.
Format tests verify the real binary template, 64-slot arrays, version gates,
source mappings and malformed/truncated input. Existing retail-library checks
continue to cover GroveBass, Audience Choir and Krimh Drums.

Current results: `app/build/verification/midi-expression-2026-11-07/report.json`.
