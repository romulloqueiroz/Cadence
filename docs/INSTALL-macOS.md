# Install Cadence on macOS

Use the archive matching your Mac: arm64 is Apple Silicon; x86_64 is Intel.
These are separate builds, not universal binaries. Minimum deployment target is
macOS 11; testing on every supported OS and DAW is still pending.

Quit your DAW and Cadence before installing. Back up an existing Cadence bundle
before replacing it. Copy whole bundles, including Contents/Helpers:

- `AU/Cadence.component` → `~/Library/Audio/Plug-Ins/Components/`
- `VST3/Cadence.vst3` → `~/Library/Audio/Plug-Ins/VST3/`
- `Standalone/Cadence.app` → `~/Applications/`

Create missing folders in Finder (Go → Go to Folder), then restart your DAW
and rescan plugins if necessary. Sine Demo and Gain work without additional
content. The standalone currently needs a MIDI input device.

Development archives are ad-hoc signed, not Developer ID signed or notarized.
macOS may block downloaded software. Only for a download you trust, follow
macOS Privacy & Security's approval flow. If your Mac's policy still blocks it,
use a notarized release or build from source. No security settings need to be
disabled globally.

Third-party plugins and sample libraries are not included. Install your own
content separately. Saved projects referring to local files require those files
on the receiving machine. This is an early build; test with a copy of a project.

To uninstall, quit your DAW and remove the installed Cadence bundles. Settings
are stored separately in `~/Library/Application Support/Cadence/`.
