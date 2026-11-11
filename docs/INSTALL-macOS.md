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

When installing a source build, use the repository's installer after quitting
your DAW:

```sh
bash scripts/install-built-plugins.sh Release /absolute/path/to/platform/app/build-release
```

It backs up existing bundles, stages and verifies their replacements, refreshes
your user's macOS Audio Unit registration, and checks that the registered AU
version matches the installed bundle. Version 0.1.1 updates the host metadata
after the sidechain addition. An older cached zero-input layout can leave Logic
silent even though Cadence's rack loads. Reopen Logic after updating; if its
plug-in manager still shows old information, reset and rescan **Cadence** there.
Also check that the Cadence insert is enabled in the channel strip.

To check the registered version independently:

```sh
python3 scripts/verify-au-registration.py
```

Development archives are ad-hoc signed, not Developer ID signed or notarized.
macOS may block downloaded software. Only for a download you trust, follow
macOS Privacy & Security's approval flow. If your Mac's policy still blocks it,
use a notarized release or build from source. No security settings need to be
disabled globally.

Third-party plugins and sample libraries are not included. Install your own
content separately. Saved projects referring to local files require those files
on the receiving machine. This is an early build; test with a copy of a project.
Kontakt support is a [documented subset](SUPPORTED-CONTENT.md). For a failed
Kontakt rack slot, select **Locate...** to choose missing samples or a moved
instrument, then save the project. Original instrument files are not modified.

To uninstall, quit your DAW and remove the installed Cadence bundles. Settings
are stored separately in `~/Library/Application Support/Cadence/`.
