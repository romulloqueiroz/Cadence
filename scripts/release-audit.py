#!/usr/bin/env python3
"""Inventory source/dependency provenance and unresolved public-release gates.

This records declarations and evidence; it does not establish redistribution
rights. --public fails while the existing sharing requirements remain unresolved.
"""
import argparse
import hashlib
import json
from pathlib import Path
import subprocess

ROOT=Path(__file__).resolve().parents[1]

def run(*args): return subprocess.check_output(args,cwd=ROOT,text=True).strip()

def source_fingerprint():
    paths=run('git','ls-files','--cached','--others','--exclude-standard').splitlines()
    digest=hashlib.sha256()
    for name in sorted(set(paths)):
        if not (name.startswith(('app/Source/','app/Tests/','crates/','plugins/','patches/')) or name in ['Cargo.toml','Cargo.lock','app/CMakeLists.txt']):continue
        path=ROOT/name
        if path.is_file():digest.update(name.encode()+b'\0'+hashlib.sha256(path.read_bytes()).digest())
    return digest.hexdigest()

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--output',type=Path,default=ROOT/'dist/Cadence-release-audit.json')
    p.add_argument('--public',action='store_true')
    p.add_argument('--distribution-report',type=Path,help='Actual sign-macos.py notarization report')
    p.add_argument('--qualification-report',type=Path,help='Current build automated/DAW/session qualification report')
    args=p.parse_args()
    metadata=json.loads(run('cargo','metadata','--format-version','1','--locked','--offline'))
    members=set(metadata['workspace_members'])
    packages=[{'name':s['name'],'version':s['version'],'license_declared':s['license'],'source':s['source'],'workspace_member':s['id'] in members} for s in metadata['packages']]
    packages.sort(key=lambda s:(s['name'],s['version']))
    paths=run('git','ls-files','--cached','--others','--exclude-standard').splitlines()
    records=[]
    for relative in sorted(set(paths)):
        path=ROOT/relative
        if not path.is_file(): continue
        translated = relative.startswith('crates/sample-adapter/src/') and relative.endswith('.rs') and any(word in path.read_text(errors='replace')[:1200].lower() for word in ['recovered','pseudocode','translated','numeric translation'])
        if (translated or relative.endswith(('.bin','provenance.json')) or relative.endswith('_reference.py') or relative=='patches/juce-cadence.patch' or relative.endswith('Kontakt680Template.nki')):
            records.append({'path':relative,'sha256':hashlib.sha256(path.read_bytes()).hexdigest(),'redistribution_status':'requires review; provenance is not permission'})
    review_path=ROOT/'docs/REDISTRIBUTION-REVIEW.json'
    review=json.loads(review_path.read_text()) if review_path.exists() else {}
    blockers=[]
    if review.get('status')!='completed': blockers.append('Ownership/redistribution review is not completed')
    if not any((ROOT/n).is_file() for n in ['LICENSE','LICENSE.md','LICENSE-MIT','LICENSE-APACHE']): blockers.append('Top-level source license texts are absent')
    if any(not s['license_declared'] and not s['workspace_member'] for s in packages): blockers.append('Some external dependencies do not declare a license; inspect the inventory')
    distribution=json.loads(args.distribution_report.read_text()) if args.distribution_report else {}
    if distribution.get('status')!='notarized-and-stapled':blockers.append('An accepted notarization/stapling report is missing')
    elif not Path(distribution.get('output','')).is_file() or hashlib.sha256(Path(distribution['output']).read_bytes()).hexdigest()!=distribution.get('sha256'):
        blockers.append('The notarized artifact is missing or does not match its recorded checksum')
    qualification=json.loads(args.qualification_report.read_text()) if args.qualification_report else {}
    fingerprint=source_fingerprint()
    if qualification.get('source_fingerprint')!=fingerprint or qualification.get('automated_checks')!='passed':blockers.append('Current-source automated qualification evidence is missing')
    if qualification.get('daw_current_build')!='passed':blockers.append('Current release candidate DAW qualification is incomplete')
    if qualification.get('long_session')!='passed':blockers.append('Long-session/storage qualification is incomplete')
    report={'status':'public-release-blocked' if blockers else 'provenance-checks-recorded',
            'source_commit':run('git','rev-parse','HEAD'),'source_dirty':bool(run('git','status','--porcelain')),
            'source_fingerprint':fingerprint,
            'workspace_license_declared':'Apache-2.0 OR MIT','dependencies':packages,'material_inventory':records,
            'redistribution_review':review,'public_release_blockers':blockers,
            'distribution_report':str(args.distribution_report) if args.distribution_report else None,
            'qualification_report':str(args.qualification_report) if args.qualification_report else None,
            'limits':['Dependency license fields are package declarations, not verified grants or a legal conclusion.','A completed provenance review alone does not certify the software or approve publication.']}
    args.output.parent.mkdir(parents=True,exist_ok=True);args.output.write_text(json.dumps(report,indent=2)+'\n')
    print(json.dumps({'report':str(args.output),'dependency_packages':len(packages),'materials_for_review':len(records),'public_release_blockers':blockers},indent=2))
    return 2 if args.public and blockers else 0

if __name__=='__main__': raise SystemExit(main())
