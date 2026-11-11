#!/usr/bin/env python3
"""Package existing Cadence macOS bundles, preserving their signatures."""
import argparse
import hashlib
from pathlib import Path
import platform
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]

def run(*args):
    return subprocess.run(args, check=True, stdout=subprocess.PIPE).stdout.decode().strip()

def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--build-dir', type=Path, default=ROOT / 'app/build-release')
    parser.add_argument('--configuration', choices=['Release', 'Debug', 'RelWithDebInfo', 'MinSizeRel'], default='Release')
    parser.add_argument('--version', default='0.1.0')
    args = parser.parse_args()
    if platform.system() != 'Darwin':
        raise SystemExit('Packaging requires macOS (ditto, lipo and codesign).')
    if not args.version or any(c not in 'abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789._-' for c in args.version):
        raise SystemExit('Version must contain only letters, digits, dots, underscores or hyphens.')
    artifacts = args.build_dir.resolve() / 'Cadence_artefacts' / args.configuration
    bundles = [('AU', 'Cadence.component'), ('VST3', 'Cadence.vst3'), ('Standalone', 'Cadence.app')]
    architectures = set()
    for fmt, name in bundles:
        bundle = artifacts / fmt / name
        for relative in ['Contents/MacOS/Cadence', 'Contents/Helpers/plugin-worker',
                         'Contents/Helpers/Cadence Scanner.app/Contents/MacOS/Cadence Scanner',
                         'Contents/Helpers/Cadence Scanner.app/Contents/Helpers/plugin-worker',
                         'Contents/Resources/ProtocolPlugins/gain.dylib',
                         'Contents/Helpers/Cadence Scanner.app/Contents/Resources/ProtocolPlugins/gain.dylib']:
            binary = bundle / relative
            if not binary.is_file():
                raise SystemExit(f'Missing bundled executable: {binary}')
            architectures.add(run('/usr/bin/lipo', '-archs', str(binary)))
            # Reject accidental links to the development tree in distributed binaries.
            dependencies = run('/usr/bin/otool', '-L', str(binary)).splitlines()[1:]
            # A dylib's own LC_ID_DYLIB also appears in otool -L, but is not a dependency.
            install_ids = run('/usr/bin/otool', '-D', str(binary)).splitlines()[1:]
            for dependency in dependencies:
                path = dependency.strip().split(' (', 1)[0]
                if path in install_ids:
                    continue
                if path.startswith('/') and not path.startswith(('/usr/lib/', '/System/Library/')):
                    raise SystemExit(f'Nonportable dynamic dependency in {binary}: {path}')
        run('/usr/bin/codesign', '--verify', '--deep', '--strict', str(bundle))
    if len(architectures) != 1 or next(iter(architectures)) not in ('arm64', 'x86_64'):
        raise SystemExit(f'Expected matching single-architecture binaries: {architectures}')
    arch = architectures.pop()
    output = ROOT / 'dist'
    output.mkdir(exist_ok=True)
    stem = f'Cadence-{args.version}-macOS-{arch}-{args.configuration}-adhoc'
    archive = output / f'{stem}.zip'
    if archive.exists():
        raise SystemExit(f'Archive already exists; move it aside first: {archive}')
    # ditto preserves macOS bundle metadata, permissions and symlinks.
    with tempfile.TemporaryDirectory(prefix='.package-', dir=output) as temporary:
        stage = Path(temporary) / stem
        stage.mkdir()
        for fmt, name in bundles:
            destination = stage / fmt / name
            destination.parent.mkdir()
            run('/usr/bin/ditto', str(artifacts / fmt / name), str(destination))
            run('/usr/bin/codesign', '--verify', '--deep', '--strict', str(destination))
        shutil.copy2(ROOT / 'docs/INSTALL-macOS.md', stage / 'INSTALL.md')
        shutil.copy2(ROOT / 'docs/SHARING.md', stage / 'SHARING.md')
        shutil.copy2(ROOT / 'vendor/JUCE/LICENSE.md', stage / 'JUCE-LICENSE.md')
        (stage / 'BUILD.txt').write_text(f'Cadence {args.version}\nArchitecture: {arch}\nConfiguration: {args.configuration}\nSigning: ad-hoc; not notarized\n')
        staged_zip = Path(temporary) / 'archive.zip'
        run('/usr/bin/ditto', '-c', '-k', '--sequesterRsrc', '--keepParent', str(stage), str(staged_zip))
        shutil.move(staged_zip, archive)
    with archive.open('rb') as stream:
        digest = hashlib.file_digest(stream, 'sha256').hexdigest()
    archive.with_suffix('.zip.sha256').write_text(f'{digest}  {archive.name}\n')
    print(f'Created {archive} ({archive.stat().st_size / 1024**2:.1f} MiB)')

if __name__ == '__main__':
    main()
