#!/usr/bin/env python3
"""Validate returned native amp state against its prepared component candidate.

This checks program/zone/asset/control identity; native process/audio evidence is
recorded separately. No audio identity is inferred from state alone.
"""
from __future__ import annotations
import argparse,collections,json,struct,sys
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path.insert(0,str(ROOT/'app/build/verification/native-kontakt-parity-2026-11-08'))
import build_native_bank as b
import validate_native_state as v


def details(path):
    state=v.inspect_state(path,Path('/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki'))
    item,used=b.parse_item(path.read_bytes());assert used==path.stat().st_size
    _,data=b.preset_item(item);top=b.ser_all(data);program=v.extract_programs(top)[0]['program']
    public=program.sections()[2];at=4+b.u32(public)*2
    fields=struct.unpack_from('<dB3f4Bh',public,at)
    banks=[n for n in top if n.kind==3];assert len(banks)==1
    slots=next(n for n in b.ser_all(banks[0].sections()[3]) if n.kind==0x37)
    containers=b.ser_all(slots.body[8:]);assert len(containers)==1
    cp=containers[0].sections()[2];at=4+b.u32(cp)*2
    arrays=[n.body for n in b.ser_all(program.sections()[3]) if n.kind==0x3a]
    fx=[]
    for role,array in enumerate(arrays):
        assert array[0]==0 and b.u16(array,1)==0x13
        count=b.u32(array,3);at=7
        for slot in range(count):
            present=array[at];at+=1
            if not present:continue
            n=b.u32(array,at+2);obj=b.ser_all(array[at:at+6+n])[0];at+=6+n
            child=b.ser_all(obj.sections()[3]);assert len(child)==1
            fx.append({'role':role,'slot':slot,'private_hex':obj.sections()[1].hex(),'child_type':child[0].kind,'child_hex':child[0].body.hex()})
        assert at==len(array)
    return {'state':state,'program_volume':fields[2],'program_pan':fields[3],'program_tune':fields[4],'container_volume_pan':list(struct.unpack_from('<2f',cp,4+b.u32(cp)*2)),'fx':fx}


def main():
    parser=argparse.ArgumentParser(description=__doc__);parser.add_argument('candidate',type=Path);parser.add_argument('returned',type=Path);parser.add_argument('--output',type=Path,required=True);args=parser.parse_args()
    expected=details(args.candidate.resolve());actual=details(args.returned.resolve());errors=[]
    a=actual['state']['programs'];e=expected['state']['programs']
    if len(a)!=len(e):errors.append('program count differs')
    else:
        for wanted,observed in zip(e,a):
            for key in ('name','groups'):
                if wanted[key]!=observed[key]:errors.append(f'{key} differs')
            if collections.Counter(map(v.mapping,wanted['zones']))!=collections.Counter(map(v.mapping,observed['zones'])):errors.append('zone mapping/assets differ')
    for key in ('program_volume','program_pan','program_tune','container_volume_pan','fx'):
        if expected[key]!=actual[key]:errors.append(f'{key} differs')
    report={'status':'confirmed' if not errors else 'rejected','errors':errors,'candidate':str(args.candidate),'candidate_sha256':b.sha(args.candidate.read_bytes()),'returned':str(args.returned),'returned_sha256':b.sha(args.returned.read_bytes()),'identity':{k:v for k,v in actual.items() if k!='state'},'programs':[{'name':p['name'],'groups':len(p['groups']),'zones':len(p['zones']),'unique_zone_assets':len({z['asset_key'] for z in p['zones']})} for p in a],'limitations':['state identity alone does not prove native execution or audio parity','group processing and source samples remain the original factory upstream graph']}
    args.output.write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report,indent=2));return bool(errors)

if __name__=='__main__':sys.exit(main())
