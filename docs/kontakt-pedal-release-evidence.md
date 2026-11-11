# Kontakt MIDI state, sustain and release evidence

Static investigation, 2026-11-07. No Kontakt process was launched, and no original analysis or executable was changed. These findings inform Cadence's sampler; they do not establish complete Kontakt runtime equivalence.

## Provenance

Target: Kontakt 8.14.1 ARM64 main executable, saved at `../../Kontakt-8-REA/Kontakt-8.arm64` relative to this directory. The saved analysis README records universal-executable SHA-256 `8300e2408c247e280fdebb60b76ff36cbca830c16449002dfa470b0ae07ec31c` and ARM64 SHA-256 `42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3`.

Source catalog: `Kontakt-8-REA/combined-functions.tsv`. Pseudocode paths below are relative to `Kontakt-8-REA/pseudocode/functions/`. The saved Ghidra 12.1.4 analysis profile is `040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd`; original REA export Evidence ID is `ev_8369a586de610d706294e52323779b18fdfb6ea6e127c1d77be18f0e0511748a`. Saved pseudocode is provider output, not original source; prior database annotations and original analysis completeness remain unknown.

The decisive behavior is in **BEngine::GenerateSysScript**, rather than a CC64 branch inside the plain voice renderer. This function constructs the system KSP scripts subsequently run by note, release and controller callbacks. This explains why tracing only NoteOn/NoteOff misses group start and pedal behavior.

## Observed behavior

| Behavior | Source and location | Observation |
| --- | --- | --- |
| Pedal threshold and delayed note release | `0x1003d418c.c`, lines 435–455 | Generated release callback sets the internal event mark, then ignores the release and requests CC64 delivery when `%CC[64] >= 64`. Generated controller callback calls `note_off(by_marks($NI_INTERNAL_MARK))` when CC64 is touched and falls below 64. Only release-marked events are replayed, so still-held keys are not released by pedal up. |
| Release layers wait for pedal up | `0x1003d418c.c`, lines 582–645 | Generated release-layer callback exits if the damping pedal is down. Otherwise it searches affected groups for release groups, starts one synthetic note and sets `$NI_SYS_EVENT_PAR_3` to 1. Its note callback enables release groups and suppresses ordinary groups. |
| Release-layer velocity | `0x1003d418c.c`, lines 605–609; `0x1003ea164.c`, lines 91–110 | Synthetic note is `play_note($EVENT_NOTE, $EVENT_VELOCITY, 0, 0)`. NoteOff passes the original BNoteEvent to callback 2; it does not replace attack velocity with release velocity. Release groups therefore use the original event's attack velocity. |
| Release velocity remains independent | `0x1003d256c.c`, lines 55–88; `0x1003ea164.c`, lines 126–135; `0x1004c3a64.c` | External note-off records release velocity separately on the original note event and its BReleaseEvent. Effective voice termination receives the release velocity. `DoTerminateVoice` feeds it to the release-velocity modulator source ID `0x20109`. |
| MIDI note-on velocity zero | `0x1003d1d3c.c`; `0x1003d256c.c`, lines 67–88 | MIDI1 note-on with velocity zero routes to ExternalNoteOff. Its synthesized release velocity is `0x8000` in the internal 16-bit velocity domain. Ordinary note-off uses its actual velocity. |
| Note lifetime identity | `0x1003d256c.c`, lines 27–186; `0x1003ea164.c`, lines 126–135 | ExternalNoteOff matches input address/channel and original note. It skips events with an existing release pointer at `+0x4e8`. The resulting BReleaseEvent points to its original note event, and voice termination matches that event's identifier. |
| Repeated same-pitch attacks | `0x1003d256c.c`, entire loop | The scan continues through `+0x518` after creating each release. A single external note-off releases **all** eligible matching note events, rather than just the oldest matching attack. Events already pending release are skipped. |
| Controller state is channel-addressed | `0x100774120.c` | CC values are stored at `state + channel*0x200 + cc*4`; channel is extracted from the MIDI packet. `0x1003d344c.c` also records per-channel engine controller changes. |
| Initial voice controller lookup | `0x100774294.c` | `getInitialVoiceValue` supports a requested channel's controller state and a separate aggregate fallback for an unspecified channel. Timestamp/sentinel checks may reject an unavailable initial value. This does not establish default CC values. |
| Latched key switches | `0x1003d418c.c`, lines 679–717 | Non-polyphonic `$keySelectRange_N` variables are declared during init and updated on note callbacks. Their state is not cleared by a release callback. This is a shared instrument latch, not a held-key condition or per-channel latch. |
| Key-switch update bounds | `0x1003d418c.c`, lines 683–717 | It computes the bounding interval `min(all key lows)..max(all key highs)`. A note inside it clears all selection flags, then sets every configured interval containing that note. A gap inside the bounding interval clears every selection; a note outside it preserves the previous selection. Overlapping intervals may both be selected. |
| Initial key switch | `0x1003f7b04.c`; original instructions `0x1003d4f2c..0x1003d4f60` | If the stored default is negative, `AdaptDefaultKeyKeySwitch` derives the lowest configured key criterion's low bound. Each key-range flag is initialized by inclusive membership of that default key in its interval. |
| Raw note versus transpose | `0x1003d418c.c`, lines 679–717, 1184–1194, 1208–1299; `0x1003e9418.c`, lines 275–310 | Key selection is generated into the earlier system note script. Instrument transpose is generated into a subsequent system script. NoteOn runs scripts before instrument/zone key range selection. Consequently key selection sees the input note at that stage and can update even when the note has no playable zone. User scripts may alter later behavior. |
| Round robin | `0x1003d418c.c`, lines 421–428, 723–731 | One shared counter starts at -1 and increments modulo `max(configured cycle class)+1` on ordinary note callbacks. It does not advance for synthetic release-trigger notes. Thus a release uses the current instrument counter, not necessarily its own attack's old counter. |
| Random | `0x1003d418c.c`, lines 428–429, 734–738 | One shared random index is chosen from `0..numRandom-1` for each note callback. `numRandom` counts random criteria entries. This draw has no release-trigger guard, unlike round robin. |
| Criterion connectors | `0x1003d418c.c`, lines 751–894; original data below | It emits parenthesized individual predicates in one flat expression, using `and`, `and not`, or `or`. No additional grouping is introduced around successive criteria. KSP expression precedence is a separate parser concern. |
| CC120/123 | `0x1003d418c.c`, lines 1107–1111 | When enabled, CC120 calls `fade_out($ALL_EVENTS,10000,1)` and then `note_off($ALL_EVENTS)`; CC123 calls `note_off($ALL_EVENTS)`. The setting defaults enabled at program `+0x1f7e6` in `0x10039a694.c`, line 119. |

## CC64 encoding

`BProgram::PrsSetValues`, `0x10042dff0.c`, lines 223–241, parses these exact XML strings into program offset `+0x21450`:

| XML value | Runtime value | System pedal behavior |
| --- | --- | --- |
| `cc64_mode_sustain_plus_controller` | 0 | Generated pedal handler enabled |
| `cc64_mode_sustain_without_controller` | 1 | Generated pedal handler enabled |
| `cc64_mode_controller_only` | 3 | Generated pedal handler disabled |

`BProgram::ResetParameters`, `0x10039a694.c`, line 148, sets runtime 0. `GenerateSysScript` disables the system pedal script when this field is greater than 2. The separate suppression of controller modulation for mode 1 has not been traced here; its name alone does not prove the implementation path.

Binary private-parameter serialization differs from runtime enum values. `BProgram::doWritePrivPars`, `0x1007b2b14.c`, lines 26–42, writes this as the **first S32**: runtime 0→stored 0, runtime 1→stored 1, runtime 3→stored 2. Other runtime values throw BWrongValue. `BProgram::doReadPrivPars`, `0x1007b5680.c`, reads an S32, rejects values greater than 2, then maps through the original table at `0x104d5c5d0`, containing the little-endian values **0, 1, 3**.

## Original-byte corroboration

Read-only `llvm-objdump` from the installed Xcode toolchain decoded the already-saved ARM64 file; the target was never executed. `__TEXT` VA `0x100000000` maps to file offset 0 according to the saved Mach-O load commands.

* At `0x1003d46a4..0x1003d46b0`, the function forms `program+0x21444`, loads its `+0xc` field and compares it with 3 (CC64 runtime mode).
* At `0x1003d4f2c..0x1003d4f60`, it loads the default key via `ldrsh w8,[x25,#8]`, compares the interval's low and high against it, and writes the interval ID and resulting boolean to the Sprintf argument slots. This fills a gap in saved pseudocode, which omits the variadic Sprintf arguments.
* Original table bytes at VA `0x104d5c5d0`: `000000000100000003000000`.
* Original strings at VAs `0x1090ff1f0`, `0x1090ff1fa`, `0x1090ff208`: `" and ...\n"`, `" and not ...\n"`, `" or ...\n"`. The generated connector table is at `0x1096c2268`; its chained-fixup pointer encodings must not be mistaken for plain virtual addresses.

Reproduce the focused default-key disassembly:

```sh
/Applications/Xcode.app/Contents/Developer/Toolchains/XcodeDefault.xctoolchain/usr/bin/llvm-objdump \
  --disassemble --start-address=0x1003d4f2c --stop-address=0x1003d4f64 \
  /Users/rom/Desktop/audio/Kontakt-8-REA/Kontakt-8.arm64
```

## Remaining unknowns and implementation boundaries

* No special CC121 handling was found in the generated system controller script. Cadence's reset-controllers behavior is an explicit host policy unless traced independently.
* Exact power-on/controller defaults are not established by the two MIDIControllerState methods. Initializing unspecified CC values to zero is a deterministic Cadence policy.
* The generated CC120 handler proves a 10 ms fade and note-off request, but not whether its kill flag suppresses every synthetic release layer downstream. Immediate panic with release-layer suppression is a deliberate Cadence policy if used.
* CC123 requests note-off; the system pedal handler could still defer those releases while CC64 is down. A final runtime test or deeper callback-path trace is needed for full interaction parity.
* Script `%CC` ownership across omnichannel input is not established solely by the channel-addressed native modulation store. Keep that distinction explicit when using per-channel controller banks in Cadence.
* For sustain-without-controller mode, the generated pedal handler necessarily reads raw `%CC[64]`, and group controller criteria reference the same `%CC` array. Allowing raw pedal values for internal group criteria is the supported inference. No CC64 suppression was found along `ExtModulation` → `BScriptParser::RunCallback` or in `MIDIControllerState`; the exact external-modulator/user-script suppression path remains unknown.
* Connector strings establish emitted syntax, not KSP parser precedence. A flat left-to-right fold is not proven by these strings.
* The default keyswitch latch is instrument-wide. Providing additional held-key or per-channel articulation modes would be Cadence functionality, not behavior recovered here.
* Note-off scans all matching external note lifetimes. Voice identity is still necessary because older sustained releases and newer attacks can coexist, and effective termination must target their original onset identities.

## File digests

The following SHA-256 values bind the saved inputs used for the observations above. They are computed from the current saved files, not assertions of original source authenticity.

| Input path | SHA-256 |
| --- | --- |
| `README.md` | `b55bcc937246e7b1d973927013acb7205d35c37688754c547f02c3fada6ccef8` |
| `decompile-manifest.json` | `3e7323156663e809e0a4e8cabacdf7a58ad414973305d4df3630a0900773e027` |
| `combined-functions.tsv` | `a39f6e28e6e2185ab3bda64c8102316d56949d19a271dd95d13d41d9c919a9d3` |
| `pseudocode/functions/0x1003d418c.c` | `e24fc07cc539793e4188e0852a08a38e972799ce667afbe010fbfb68425d4b50` |
| `pseudocode/functions/0x1003d256c.c` | `d725b0e2a467e75ab9006184ada8c8d2ad15fa056c0a5a361282d622e3ff34e2` |
| `pseudocode/functions/0x1003ea164.c` | `9a63f731611df8c94bf7dd9054b28ee1dc0f7d2ec52c50598e850d6edffeeb4d` |
| `pseudocode/functions/0x1003e9418.c` | `fc160f26cdf9f48cf916765edaceafd6d9dbda1fea0972ddba99300c365fc645` |
| `pseudocode/functions/0x100774120.c` | `c913955b196c8c6ee45d14d333f54f5519c7a3a8cf9adf7b692d1a5e183c2e46` |
| `pseudocode/functions/0x100774294.c` | `e2048876052a1e3ccc0d9b8900755407cb258487bf9fbda429b4d93ef5d3c3c7` |
| `pseudocode/functions/0x1003d344c.c` | `3387f4d01d04bd2b353022e8a84a34a5adad84bb8b48c521159adfb06a4ca2ba` |
| `pseudocode/functions/0x10042dff0.c` | `89d7a26fb1e081cd3d7e547ce409c033d414a7bb66418c8eb55e052b20c03d38` |
| `pseudocode/functions/0x10039a694.c` | `f7a2fad5779ffa7f042ae976af0d74971800aa7cf5108ccce4bf02b4129ccf5e` |
| `pseudocode/functions/0x1007b2b14.c` | `1bc0aac378d550e7f08c60e33625574cc10fdef556e7215960b8b796a7b340c0` |
| `pseudocode/functions/0x1007b5680.c` | `d48fb8570897a9d888f1c467a0324b8d435d0ff92d0612a59898352331e0bf9b` |
| `pseudocode/functions/0x1003f7b04.c` | `7b7a28ef8502ba4e8e97123e84fe36f854f730a07a982dafa9ef68a7a144fd58` |
| `pseudocode/functions/0x1004c3a64.c` | `07f6da8867825a29ae8d257ff12f49646d5b23ac542811babfad06752e5b60fc` |
| `pseudocode/functions/0x1002dbc64.c` | `f39cbbfdd1a477bdf3f35a7897b4fcd1789129787cc14fb04969ad5fe5fef410` |
