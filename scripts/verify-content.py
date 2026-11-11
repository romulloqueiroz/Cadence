#!/usr/bin/env python3
"""Record real-library loading/render smoke checks, separately from parity/DAW qualification."""
import argparse
from array import array
import hashlib
import json
import math
from pathlib import Path
import struct
import subprocess
import sys

ROOT=Path(__file__).resolve().parents[1]
DEFAULT=Path('/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments')

def audio_stats(path):
    b=path.read_bytes();at=12;payload=None;fmt=None
    if b[:4]!=b'RIFF' or b[8:12]!=b'WAVE': raise ValueError('Renderer did not produce RIFF/WAVE')
    while at+8<=len(b):
        tag=b[at:at+4];n=struct.unpack_from('<I',b,at+4)[0];block=b[at+8:at+8+n]
        if len(block)!=n:raise ValueError('Truncated WAV')
        if tag==b'fmt ':fmt=struct.unpack_from('<HHIIHH',block)
        if tag==b'data':payload=block
        at+=8+n+(n&1)
    if payload is None or fmt is None or fmt[0]!=3 or fmt[1]!=2 or fmt[5]!=32:raise ValueError('Expected stereo float32 WAV')
    values=array('f');values.frombytes(payload)
    if sys.byteorder!='little':values.byteswap()
    if not values or not all(map(math.isfinite,values)):raise ValueError('Missing/nonfinite audio')
    peak=max(map(abs,values))
    if peak<=1e-6:raise ValueError('Silent render')
    return {'sample_rate':fmt[2],'frames':len(values)//2,'peak':peak,'sha256':hashlib.sha256(b).hexdigest()}

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--renderer',type=Path,required=True)
    p.add_argument('--instrument',type=Path,action='append')
    p.add_argument('--required',action='append',help='Required instrument basename; defaults to all explicit inputs, or Concert in the default installed sweep')
    p.add_argument('--output',type=Path,required=True)
    args=p.parse_args();args.output.mkdir(parents=True,exist_ok=True)
    instruments=args.instrument if args.instrument is not None else sorted(DEFAULT.glob('*.nki'))
    if not instruments:p.error('No instruments found; supply --instrument paths')
    required=args.required if args.required is not None else ([p.name for p in instruments] if args.instrument is not None else ['Piano Uno - Concert.nki'])
    renderer=args.renderer.resolve()
    report={'renderer_sha256':hashlib.sha256(renderer.read_bytes()).hexdigest(),'cases':[],
        'method':'Native Cadence worker engine CLI, resident attacks plus streaming, note60/velocity100,48kHz/256frames,3second stereo float render',
        'limits':['Load/render smoke only; musical completeness, native parity, polyphonic stress and real-DAW recall are separate checks.','Script notices mean saved mappings may play while scripted behavior is absent.','Library/sample files are not copied into this report.']}
    for index,instrument in enumerate(instruments):
        instrument=instrument.resolve();case={'instrument':str(instrument),'name':instrument.name,'required':instrument.name in required}
        audio=args.output/f'{index:02}-render.wav'
        if not instrument.is_file():case.update(status='missing',error='Instrument is not installed')
        else:
            case['instrument_sha256']=hashlib.sha256(instrument.read_bytes()).hexdigest()
            try:
                result=subprocess.run([str(renderer),str(instrument),str(audio),'--float-wav','--seconds','3'],cwd=ROOT,text=True,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=120)
                (args.output/f'{index:02}-render.log').write_text(result.stdout)
                if result.returncode:case.update(status='load-or-render-rejected',error=result.stdout[-4000:])
                else:case.update(status='audible-render',audio=audio_stats(audio),notices=[line for line in result.stdout.splitlines() if line.startswith('Compatibility:')])
            except (subprocess.TimeoutExpired,ValueError,struct.error,OSError) as error:case.update(status='failed',error=str(error))
        report['cases'].append(case);print(f"{instrument.name}: {case['status']}",flush=True)
    missing=set(required)-{c['name'] for c in report['cases']}
    failures=[c['name'] for c in report['cases'] if c['required'] and c['status']!='audible-render']+sorted(missing)
    report.update(status='required-smoke-passed' if not failures else 'required-smoke-failed',required_failures=failures)
    (args.output/'report.json').write_text(json.dumps(report,indent=2)+'\n');return 1 if failures else 0

if __name__=='__main__':raise SystemExit(main())
