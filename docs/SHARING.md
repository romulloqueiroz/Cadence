# Source and binary sharing

Use `platform/` as the repository root, not the surrounding audio workspace.
Build products, JUCE checkout, local reports, signing credentials and distribution
archives are ignored. Small `.bin` files in crates are source tables/test inputs,
not disposable build products; they must remain available for builds/tests.

## Build a release archive

On macOS, install Xcode command-line tools, CMake, Git, Python 3 and stable Rust.

```sh
python3 scripts/setup-juce.py
cmake -S app -B app/build-release -DCMAKE_BUILD_TYPE=Release
cmake --build app/build-release --target Cadence_Standalone Cadence_AU Cadence_VST3 CadenceScanTests -j 4
cargo build --workspace --locked
cargo test --workspace --locked
CADENCE_TEST_TMPDIR=/tmp ctest --test-dir app/build-release --output-on-failure
python3 scripts/package-macos.py --build-dir app/build-release --configuration Release
```

For subsequent source changes, rebuild and check the same release directory with
`python3 scripts/verify-packaged.py --build-dir app/build-release`. To install its
AU/VST3 bundles locally, run
`bash scripts/install-built-plugins.sh Release app/build-release`.

The packager verifies signatures and embedded helpers, then writes a zip and
SHA-256 checksum under `dist/`. Upload these as GitHub Release assets. CI also
builds and saves downloadable artifacts; it never publishes a release itself.
The archive architecture comes from the actual executable. Build on each target
architecture with the corresponding Rust target installed; universal merging is
not implemented. Do not describe Debug packages as release builds.

The [v1 release checklist](RELEASE-CHECKLIST.md) separates a development beta
from a public release. `python3 scripts/release-audit.py` records dependency
declarations and materials requiring review; `--public` fails while the existing
licensing prerequisites are unresolved. `python3 scripts/sign-macos.py --check`
reports installed Developer ID identities without signing or uploading anything.
The signing script stages copies, supports an existing notarytool Keychain
profile and records Apple's actual result in an adjacent JSON report.

## Before public distribution

The repository currently contains recovered Kontakt numeric translations,
extracted DSP tables, native reference captures and a Kontakt template fixture.
Their provenance files describe technical origins, not redistribution grants.
This preparation does not establish permission to publish those materials or
binaries containing them. Resolve their redistribution status or replace them
with independently distributable implementations/fixtures before public release.
They have not been silently removed: some are compiled into the engine.

Cargo manifests declare `Apache-2.0 OR MIT`; top-level license texts and a verified
ownership/license inventory are still needed before advertising an open-source
license. No blanket license has been added over material of uncertain origin.
JUCE is pinned with local modifications; review its actual `LICENSE.md` and the
licenses of dependencies before choosing your distribution terms.

For ordinary public downloads, Developer ID signing and Apple notarization are
separate release steps requiring the publisher's Apple account/credentials. The
current packager preserves and verifies the build's ad-hoc signatures and labels
archives accordingly. It does not claim notarization.

## Put the prepared source on GitHub

```sh
cd /path/to/audio/platform
git init -b main
git add .
git status --short
# Review the staged file list and the provenance/licensing items above first.
git commit -m "Initial Cadence source"
git remote add origin YOUR_REPOSITORY_URL
git push -u origin main
```

Use a private repository while redistribution questions are unresolved. Do not
commit `dist/` or build trees; attach distributable archives to releases instead.
