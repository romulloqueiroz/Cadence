# Cadence v1 supported content

The first release target is an Apple Silicon macOS standalone app, AU and VST3
with a stereo rack. Intel/universal builds and other operating systems are not
part of this qualification. The deployment target is macOS 11; only OS/DAW
versions recorded in the release report are actually tested.

## What can be used

- Built-in Sine Demo and Gain without additional content.
- Single-program Kontakt instruments using supported saved sample mappings,
  WAV/NCW samples, envelopes, loops, group starts, modulation and effects.
- Plain K2 XML and supported NI binary containers, including a one-program multi.
- User-installed AU/VST3, supported CLAP and native protocol plugins. Plugin
  and library installation/licensing remain separate from Cadence.

Piano Uno Concert is the installed real-library reference. Passing a parser or
producing audible output does not certify another library. Consult the release
qualification matrix for the exact instrument/build combinations tested.

## Compatibility notices and errors

Kontakt instruments show a persistent, scrollable compatibility notice in the
Inspector. Detected script data is reported separately. KSP scripts, scripted
interfaces, time-stretch/formant and other advanced source engines are not
implemented. Their presence can change behavior even when the saved sample
mapping plays. Cadence does not claim complete Kontakt compatibility.

Active binary group/program effects that cannot be compiled now fail loading
with an explicit effect type/slot error; they are not silently omitted. Bypassed
effects remain inactive. Some implemented effects and interpolation modes remain
approximations. XML effect arrays are reported as unplayed; supported XML group
filter parameters still apply. Multis with multiple programs are rejected rather
than playing an incomplete or mixed selection.

Unsupported encryption, malformed metadata, missing samples and resident-memory
budget overflow are load errors. The current sampler has 64 voices, a fixed
16 MiB streaming cache and a 128 MiB resident attack/loop budget. Large libraries
that exceed this budget are outside v1's supported subset.

## Recover missing or moved content

**Open...** loads a Kontakt file directly into a rack slot and adds it to the
Library. Loading runs in the background; a failed slot retains its description,
bypass, automation identity and any saved state.

Select that slot and use **Locate...** in the Inspector header:

- **Locate samples...** selects a library/sample folder and retries loading.
- **Locate instrument...** selects a moved or renamed `.nki`/`.nkm` and retries.
- **Clear sample locations** removes chosen recovery folders and retries the
  original references. It does not delete any samples or change the instrument.

The original instrument file is never rewritten. Chosen paths are stored in the
Library entry and DAW/rack state, so save the project after recovery. Recovery
restarts the loaded instrument; replay held notes afterward.

Existing valid references take precedence. For missing external WAV/NCW and NKX
monolith files, search is confined to at most eight explicitly chosen folders.
The unique best trailing-path match wins; equally good duplicate filenames
produce an error requiring a narrower folder. Directory symlinks are not followed;
select their target folder directly. A search is bounded to 100000 entries and
32 directory levels. NKR relocation and portable content collection are not
implemented. Copying a project does not bundle its external libraries.

## Other limits

- The standalone currently requires MIDI input; an onscreen keyboard and
  transport controls remain future usability work.
- Native CLAP windows have limited resize/show-hide extension support.
- Process separation protects against worker crashes, not filesystem/network
  access. Direct AU/VST3 nodes run in the host process.
- Long-session, large-library and wider DAW/OS qualification remains open unless
  explicitly recorded for the release build.
