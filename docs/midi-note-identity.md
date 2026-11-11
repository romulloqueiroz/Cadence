# MIDI note lifetimes in the Kontakt sampler

Implemented 2026-11-07 as the first substage of the
[Kontakt compatibility backlog](kontakt-compatibility-roadmap.md).

## Behavior

Each attack has a unique onset ID, MIDI channel, input note, attack velocity and
held/deferred state. Voices retain their onset ID and channel. An external
note-off matches all unreleased onsets of the same note **and channel**.
An unmatched or repeated note-off cannot terminate another channel's voices or
create extra release layers.

Repeated same-pitch attacks retain their individual attack velocities. A single
note-off releases all eligible matching attacks, and each can trigger the
appropriate velocity-selected release layers. This follows the saved
`BEngine::ExternalNoteOff` scan, rather than assuming FIFO note matching.

Sustain defers each original onset independently. Pedal-up releases only that
channel's deferred onsets, including their release layers; it leaves still-held
retriggers sounding. CC120 clears note metadata and ramps all sounding voices
to silence over the recovered 10 ms before retiring them, so later
note-offs/pedal-up cannot resurrect release layers. CC123 requests note-off for
all held onsets through the same pedal/release-layer path, matching the generated
system script's instrument-wide `$ALL_EVENTS` request.

Out-of-range MIDI channel/note/controller/velocity fields are ignored before
truncation. Parameter events retain the existing parameter-ID convention.

## Realtime storage and policy

There are 2,048 preallocated note-event slots, independent of the 64-voice pool.
They also track attacks with only release zones or no playable attack zone.
No vector grows or frees storage during note-off, pedal-up or panic. Full
capacity retires the oldest tracked onset and its remaining voices without
generating release layers. At `u64` ID exhaustion, a reset clears voices and
controller/selection state before ID reuse. These bounds are explicit Cadence
policies; Kontakt's event capacity and overflow policy have not been established.

## Evidence and limits

Saved static observations are in
[kontakt-pedal-release-evidence.md](kontakt-pedal-release-evidence.md).
The original REA export Evidence ID is
`ev_8369a586de610d706294e52323779b18fdfb6ea6e127c1d77be18f0e0511748a`.
The decisive saved pseudocode is
`../../Kontakt-8-REA/pseudocode/functions/0x1003d256c.c` (external note-off
scan, original-event release pointer and continuation through repeated attacks),
`0x1003ea164.c` (effective termination by original event) and
`0x1003d418c.c` (generated system pedal/release/CC120/123 scripts).

This is a source-level implementation informed by static evidence. CC123's
interaction with the generated pedal handler is an inference; it has no new
Kontakt runtime comparison. CC120 now implements the recovered 10 ms fade and
clears note metadata first (release-layer suppression is a documented Cadence
policy). CC121 has no generated-script branch in the recovered evidence, so it
is delivered as an ordinary controller with no reset policy. Release velocity
is tracked per onset and fed to the release-velocity source at termination.
Group MIDI/voice-group settings remain pending. [External MIDI expression
routes](midi-expression.md) were implemented in the following substage. No C
ABI, shared memory layout or saved-state schema changes are introduced.

## Verification

Six new public-API audio regressions in
`crates/sample-adapter/tests/group_selection.rs` cover channel isolation,
pedal/retrigger identity, attack-velocity-specific release layers, panic cleanup,
CC123 and malformed MIDI. They exercise both resident and streamed playback;
channel isolation additionally compares exact output across block sizes.
An allocator-guarded unit test saturates all 2,048 note slots, defers/releases
them with the pedal and resets, checking zero allocations **and deallocations**
while processing. Existing sampler and format regressions remain applicable.

The current verification results and build/IPC limits are recorded in
`app/build/verification/midi-note-identity-2026-11-07/report.json`.
