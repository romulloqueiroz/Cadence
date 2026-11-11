#!/usr/bin/env python3
"""Build paired dry/JUMP native Kontakt component-state candidates.

Starts from an identity-verified saved native Kontakt state, preserving its NI
container, group/sample graph and ancillary bank state. Program FX are replaced
with a dry chain or a single JUMP insert. Scripts are emptied, and program and
container volume/pan are set to unity/center.
Native acceptance must subsequently be checked against captured component state;
this generator does not prove native execution or audio parity.
"""
from __future__ import annotations
import argparse,copy,json,struct,sys
from pathlib import Path

ROOT=Path(__file__).resolve().parents[1]
HELPERS=ROOT/'app/build/verification/native-kontakt-parity-2026-11-08'
sys.path.insert(0,str(HELPERS))
import build_native_bank as b
import validate_native_state as v


def structured(kind,version,private=b'',public=b'',children=b''):
    return b.Ser(kind,b'\1'+struct.pack('<H',version)+b''.join(b.p32(len(s))+s for s in (private,public,children)))


def empty_array(count):return b'\0'+struct.pack('<HI',0x13,count)+b'\0'*count


def fx_chain(jump=False):
    if not jump:return empty_array(8)
    # fix-id31 -> runtime type18 (BParFXJump), serialized child type77 v0x10.
    raw=struct.pack('<6f2B',0.5,0.5,0.5,0.5,0.5,0.5,0,0)
    child=b.Ser(77,b'\0'+struct.pack('<H',0x10)+raw)
    slot=structured(0x25,0x50,struct.pack('<iBiBffi',31,0,0,0,1.0,0.0,-1),children=child.encode())
    return b'\0'+struct.pack('<HI',0x13,8)+b'\1'+slot.encode()+b'\0'*7


def replace_program(top,program):
    out=[]
    for node in top:
        if node.kind==3:
            bc=b.ser_all(node.sections()[3]);slot_index=next(i for i,s in enumerate(bc) if s.kind==0x37)
            slots=bc[slot_index];containers=b.ser_all(slots.body[8:]);assert len(containers)==1
            cc=b.ser_all(containers[0].sections()[3]);index=next(i for i,s in enumerate(cc) if s.kind==0x36)
            cc[index]=b.Ser(0x36,struct.pack('<Hh',1,0)+program.body)
            container=containers[0].replace_sections(children=b''.join(s.encode() for s in cc))
            bc[slot_index]=b.Ser(0x37,b'\1'+b'\0'*7+container.encode())
            node=node.replace_sections(children=b''.join(s.encode() for s in bc))
        out.append(node)
    return out


def build_preserved(source,jump,chain_builder=fx_chain):
    """Preserve the valid native sampler graph; normalize downstream controls.

    All program insert/send/master slots are cleared, then JUMP is installed
    in insert slot0 for the wet candidate. Group effects stay upstream and
    identical between candidates. Scripts become copies of a native empty slot
    so no init handler can alter the measured controls after restore.
    """
    item,used=b.parse_item(source);assert used==len(source)
    node,preset=b.preset_item(item);top=b.ser_all(preset)
    program=v.extract_programs(top)[0]['program'];pc=b.ser_all(program.sections()[3])
    empty_script=next(s for s in pc if s.kind==6 and len(s.body)==27)
    pp=bytearray(program.sections()[2]);at=4+b.u32(pp)*2
    struct.pack_into('<3f',pp,at+9,1.0,0.0,1.0)
    role=0
    for index,child in enumerate(pc):
        if child.kind==0x3a:
            pc[index]=b.Ser(child.kind,chain_builder(jump and role==0));role+=1
        elif child.kind==6:pc[index]=copy.deepcopy(empty_script)
    program=program.replace_sections(public=bytes(pp),children=b''.join(s.encode() for s in pc))
    top=replace_program(top,program)
    # Native container public is an empty UTF16 name + two f32 volume/pan.
    for index,bank in enumerate(top):
        if bank.kind!=3:continue
        bc=b.ser_all(bank.sections()[3]);si=next(i for i,n in enumerate(bc) if n.kind==0x37)
        slot=bc[si];containers=b.ser_all(slot.body[8:]);assert len(containers)==1
        pub=bytearray(containers[0].sections()[2]);at=4+b.u32(pub)*2
        struct.pack_into('<2f',pub,at,1.0,0.0)
        containers[0]=containers[0].replace_sections(public=bytes(pub))
        bc[si]=b.Ser(slot.kind,slot.body[:8]+containers[0].encode())
        top[index]=bank.replace_sections(children=b''.join(s.encode() for s in bc))
    output=b''.join(s.encode() for s in top);old=node.payload;oldlen=b.u32(old,12)
    node.payload=old[:12]+b.p32(len(output))+old[16:20]+output+old[20+oldlen:]
    return item.encode()


def one_zone(source):
    """Retain the original 60-root WAV/NCW attachment and matching group only."""
    item,used=b.parse_item(source);assert used==len(source)
    node,preset=b.preset_item(item);top=b.ser_all(preset)
    program=v.extract_programs(top)[0]['program'];pc=b.ser_all(program.sections()[3])
    groups=v.array_objects(next(n for n in pc if n.kind==0x33))
    zones=v.array_objects(next(n for n in pc if n.kind==0x34),True)
    selected=[]
    for index,(group,zone) in enumerate(zones):
        pub=zone.sections()[2];vl,vh,kl,kh=struct.unpack_from('<4H',pub,12)
        if kl<=60<=kh and vl<=100<=vh and b.u16(pub,28)==60:
            selected.append((index,group,zone))
    assert len(selected)==1
    index,group,zone=selected[0]
    for position,child in enumerate(pc):
        if child.kind==0x33:pc[position]=b.Ser(child.kind,b.p32(1)+groups[group][1].body)
        elif child.kind==0x34:pc[position]=b.Ser(child.kind,b.p32(1)+b.p32(0)+zone.body)
    program=program.replace_sections(children=b''.join(s.encode() for s in pc));top=replace_program(top,program)
    output=b''.join(s.encode() for s in top);old=node.payload;oldlen=b.u32(old,12)
    node.payload=old[:12]+b.p32(len(output))+old[16:20]+output+old[20+oldlen:]
    return item.encode(),{'original_zone_index':index,'original_group':group,'original_group_name':v.text_at(groups[group][1].sections()[2]),'root':60,'original_attachment_retained':True}


def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--donor',type=Path,default=ROOT/'app/build/verification/kontakt-comparison-20261108T012259Z-47329/native-runner-clean/component-state.bin')
    parser.add_argument('--output',type=Path,default=ROOT/'app/build/verification/native-jump-2026-11-08')
    parser.add_argument('--one-zone',action='store_true')
    args=parser.parse_args();out=args.output.resolve();out.mkdir(parents=True,exist_ok=True)
    source=args.donor.read_bytes();files={}
    for name,jump in [('dry',False),('jump',True)]:
        candidate=build_preserved(source,jump)
        detail=None
        if args.one_zone:candidate,detail=one_zone(candidate)
        prefix='onezone' if args.one_zone else 'unity'
        path=out/f'{prefix}-{name}-candidate.nkm';path.write_bytes(candidate);files[name]={'path':str(path),'sha256':b.sha(candidate),'zone':detail}
    manifest={'source_state':str(args.donor),'source_sha256':b.sha(source),'candidates':files,'jump':{'fix_type':31,'serialized_child':77,'version':16,'values':[0.5]*6,'flags':[False,False]},'expected':{'programs':1,'groups':1 if args.one_zone else 6,'zones':1 if args.one_zone else 123,'program_volume':1.0,'program_pan':0.0,'container_volume':1.0,'container_pan':0.0,'scripts':'empty','program_fx':'only JUMP in wet insert0, all other program slots empty','group_processing':'retained unchanged upstream of JUMP'},'status':'candidate_generated_native_acceptance_unverified'}
    (out/('onezone-candidate-manifest.json' if args.one_zone else 'unity-candidate-manifest.json')).write_text(json.dumps(manifest,indent=2)+'\n');print(json.dumps(manifest,indent=2))

if __name__=='__main__':main()
