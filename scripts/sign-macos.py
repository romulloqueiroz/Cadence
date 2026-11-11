#!/usr/bin/env python3
"""Stage Developer ID signed bundles and optionally notarize a distributable DMG.

The source build is never modified. Authentication uses an existing Keychain
profile; passwords and private keys are never command-line arguments here.
"""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import plistlib
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parents[1]
BUNDLES = [('AU', 'Cadence.component'), ('VST3', 'Cadence.vst3'), ('Standalone', 'Cadence.app')]
MACHO = {b'\xcf\xfa\xed\xfe', b'\xce\xfa\xed\xfe', b'\xca\xfe\xba\xbe', b'\xca\xfe\xba\xbf'}

def run(*args):
    return subprocess.run(args, check=True, capture_output=True, text=True).stdout.strip()

def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build-dir', type=Path, default=ROOT/'app/build-release')
    p.add_argument('--configuration', default='Release', choices=['Release', 'RelWithDebInfo'])
    p.add_argument('--output', type=Path, default=ROOT/'dist/Cadence-DeveloperID.dmg')
    p.add_argument('--identity', help='Installed Developer ID Application certificate name or SHA-1')
    p.add_argument('--notary-profile', help='Existing notarytool Keychain profile; enables upload to Apple')
    p.add_argument('--check', action='store_true', help='Report preflight only; never sign or upload')
    args = p.parse_args()
    if platform.system() != 'Darwin': p.error('macOS signing tools are required')
    identities = run('/usr/bin/security', 'find-identity', '-v', '-p', 'codesigning')
    valid = [s.strip() for s in identities.splitlines() if 'Developer ID Application:' in s]
    source = args.build_dir.resolve()/'Cadence_artefacts'/args.configuration
    absent = [str(source/f/n) for f,n in BUNDLES if not (source/f/n).is_dir()]
    selected = [s for s in valid if args.identity and args.identity in s]
    preflight = {'developer_id_identities': valid, 'identity_selected': len(selected) == 1,
                 'missing_bundles': absent, 'notarization_requested': bool(args.notary_profile)}
    if args.check:
        print(json.dumps(preflight, indent=2)); return 0
    if len(selected) != 1: p.error('Select exactly one installed Developer ID Application identity; run --check')
    if absent: p.error('Build bundles are missing: '+', '.join(absent))
    output = args.output.resolve()
    if output.suffix.lower() != '.dmg': p.error('--output must end in .dmg')
    if output.exists() or output.with_suffix('.report.json').exists(): p.error('Output already exists; choose another path')
    output.parent.mkdir(parents=True, exist_ok=True)
    report = {'status': 'staging', 'identity': args.identity, 'configuration': args.configuration,
              'notarization_requested': bool(args.notary_profile)}
    report_path = output.with_suffix('.report.json')
    with tempfile.TemporaryDirectory(prefix='.cadence-sign-', dir=output.parent) as tmp:
        temp = Path(tmp); stage = temp/'Cadence'; stage.mkdir()
        entitlements = temp/'host.plist'
        # Workers host user-selected native plugins, including ad-hoc plugins.
        entitlements.write_bytes(plistlib.dumps({'com.apple.security.cs.disable-library-validation': True}))
        def sign(path, runtime=True):
            options = ['--options', 'runtime', '--entitlements', str(entitlements)] if runtime else []
            run('/usr/bin/codesign', '--force', '--timestamp', '--sign', args.identity, *options, str(path))
        try:
            for fmt,name in BUNDLES:
                destination = stage/fmt/name; destination.parent.mkdir()
                run('/usr/bin/ditto', str(source/fmt/name), str(destination))
                for binary in destination.rglob('*'):
                    if not binary.is_file() or binary.is_symlink(): continue
                    with binary.open('rb') as stream: magic = stream.read(4)
                    if magic in MACHO: sign(binary)
                nested = sorted([f for f in destination.rglob('*.app') if f.is_dir()], key=lambda f:len(f.parts), reverse=True)
                for app in nested: sign(app)
                sign(destination)
                run('/usr/bin/codesign', '--verify', '--deep', '--strict', str(destination))
            for doc in ['INSTALL-macOS.md', 'SUPPORTED-CONTENT.md']:
                shutil.copy2(ROOT/'docs'/doc, stage/doc)
            shutil.copy2(ROOT/'vendor/JUCE/LICENSE.md', stage/'JUCE-LICENSE.md')
            (stage/'BUILD.txt').write_text('Developer ID signed\nNotarization is established by the adjacent release report.\n')
            image = temp/'Cadence.dmg'
            run('/usr/bin/hdiutil', 'create', '-srcfolder', str(stage), '-volname', 'Cadence', '-format', 'UDZO', str(image))
            sign(image, runtime=False)
            report['status'] = 'signed-not-notarized'
            if args.notary_profile:
                submission = json.loads(run('/usr/bin/xcrun', 'notarytool', 'submit', str(image), '--keychain-profile', args.notary_profile, '--wait', '--timeout', '30m', '--output-format', 'json'))
                report['notarization'] = submission
                if submission.get('status') != 'Accepted': raise RuntimeError('Apple did not accept the submission; inspect the report and notarytool log')
                run('/usr/bin/xcrun', 'stapler', 'staple', str(image))
                run('/usr/bin/xcrun', 'stapler', 'validate', str(image))
                report['status'] = 'notarized-and-stapled'
            shutil.move(image, output)
            with output.open('rb') as stream: digest = hashlib.file_digest(stream, 'sha256').hexdigest()
            output.with_suffix('.dmg.sha256').write_text(f'{digest}  {output.name}\n')
            report.update(output=str(output), sha256=digest)
        except Exception as error:
            report.update(status='failed', error=str(error)); raise
        finally:
            report_path.write_text(json.dumps(report, indent=2)+'\n')
    print(json.dumps(report, indent=2)); return 0

if __name__ == '__main__': raise SystemExit(main())
