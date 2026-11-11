# Kontakt Audio Workgroup reference

Read on 2026-11-06 from the saved REA/Ghidra export of Kontakt 8.14.1 (ARM64) in
`../../Kontakt-8-REA/`. This is saved static pseudocode, not original source and
not a runtime trace.

## Observed functions

| Function | Address | Observation |
| --- | --- | --- |
| `NI::AB::IInstrument::setAudioWorkgroup(const NI::AB::AudioWorkgroup&)` | `0x1007ee9c8` | Empty body in this export; the concrete implementation is elsewhere. Shows the host-facing interface accepts a workgroup by const reference. |
| `NI::AB::InterfaceBase::setAudioWorkgroup(const NI::AB::AudioWorkgroup&)` | `0x101ea6a5c` | Forwards through a virtual dispatch at `+0x178` on an object at `+0x8b8`; pseudocode could not recover the jump table. |
| `NI::NSA::LockedAudioDevice::getOSWorkgroup()` | `0x101057938` | Takes a shared lock, resolves an `AggregatedDevice`/`CoreAudioDevice`, and returns the device's stored workgroup or a static `noWorkgroup` object. A failed `try_lock` returns `noWorkgroup`. |
| `NI::NSA::CoreAudioUtility::getPropertyList<os_workgroup_s>` | `0x101058f58` | Generic CoreAudio property-list reader instantiated for `os_workgroup_t`, consistent with reading `kAudioDevicePropertyIOThreadOSWorkgroup`. |
| Call sites `0x10104a1e0`, `0x1010483b0` | — | Query the locked device workgroup and call `InterfaceBase::setAudioWorkgroup`. |
| `_mach_timebase_info` / `_thread_policy_set` | external imports | Kontakt also uses Mach timebase conversion and thread policy APIs; these are separate from workgroup membership. |

## Design implications used by Cadence

- Obtain a workgroup from the audio device/format host rather than inventing one.
- Keep an explicit "no workgroup" value; a failed lock or unavailable device must
  fall back cleanly.
- Separate the saved/visible host interface from the live runtime object.
- Joining must happen on the rendering thread; device/workgroup discovery happens
  on a control thread.

## Cadence implementation

Cadence uses JUCE's `AudioProcessor::audioWorkgroupContextChanged` (AU) and a
cross-process Mach send right. It does not execute Kontakt code and makes no
runtime-parity claim. Implementation and limits: `docs/audio-workgroups.md`.
Verification: `app/build/verification/workgroup-native-adapter-2026-11-06/report.json`.

## Saved artifact hashes

```text
b33d0fe640e616b2b2f0395ee7ae77ca75cdf9d3ca388542d8411485049f9430  0x1007ee9c8.c
fefbd952c83d32126e07cba92e1791941879a1f177805afcb703f8584bc2b290  0x101ea6a5c.c
638c4d88225f966938e713e46167d29ef23426aedc73283814a9fb752257bfb7  0x101057938.c
28263e87e275061cfb9b84465c27e3e9f0dd42848e73514f995beb999efc780e  0x101058f58.c
14c91e8399c701a84f99d44d6ca548fb3b5a4c075475d0ec6f94437b688b3556  0x10104a1e0.c
b02864246c79e1838264a5099ea97138ea1a9bdeae3f47578e1f15825ea8b4d1  0x1010483b0.c
```
