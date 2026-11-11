#!/bin/bash
# Run in a normal macOS Terminal to install the already-built user plugins.
set -euo pipefail

project_root="$(cd "$(dirname "$0")/.." && pwd)"
configuration="${1:-Debug}"
build_dir="${2:-$project_root/app/build}"
artifacts="$build_dir/Cadence_artefacts/$configuration"
backup="$build_dir/verification/plugin-install-$(date -u +%Y%m%dT%H%M%SZ)"

for source in "$artifacts/AU/Cadence.component" "$artifacts/VST3/Cadence.vst3"; do
    if [[ ! -d "$source" ]]; then
        printf 'Missing built plugin: %s\n' "$source" >&2
        exit 1
    fi
    /usr/bin/codesign --verify --deep --strict "$source"
done

install_bundle() {
    local format="$1" bundle="$2" destination="$3"
    mkdir -p "$destination"
    if [[ -d "$destination/$bundle" ]]; then
        mkdir -p "$backup/$format"
        /usr/bin/ditto "$destination/$bundle" "$backup/$format/$bundle"
    fi
    /usr/bin/ditto "$artifacts/$format/$bundle" "$destination/$bundle"
    /usr/bin/codesign --verify --deep --strict "$destination/$bundle"
    printf 'Installed %s\n' "$destination/$bundle"
}

install_bundle AU Cadence.component "$HOME/Library/Audio/Plug-Ins/Components"
install_bundle VST3 Cadence.vst3 "$HOME/Library/Audio/Plug-Ins/VST3"
printf 'Existing bundles, if any, were backed up under %s\n' "$backup"
