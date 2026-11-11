#!/usr/bin/env python3
"""Compile a circuit oracle from the saved Ghidra VAN51 ProcessInt body.

The oracle mechanically preserves the recovered operation order and reads an
explicit 0xa000-byte state snapshot. Coefficient correctness is outside this
circuit-only check. Table entries come from the independently replayed fixture.
The --native-process mode executes only the isolated original numerical routine.
By default, compiled pseudocode uses actual NEON reciprocal
estimate/refinement instructions; --scalar-reciprocal selects portable division.
"""
from __future__ import annotations
import argparse
import hashlib
import json
import math
import struct
from pathlib import Path
import re
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[5]
SOURCE = ROOT / 'Kontakt-8-REA/pseudocode/functions/0x1005d55bc.c'

# Ghidra displays vector bit reinterpretations using C casts. Restore their
# intended bit behavior; true integer->float and double->float remain numeric.
HEADER = r'''
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
#include <string.h>
#include <math.h>
#include <type_traits>
#if defined(__aarch64__) && !defined(VAN51_SCALAR_RECIPROCAL)
#include <arm_neon.h>
#endif
typedef unsigned char undefined1;
typedef unsigned int undefined4;
typedef unsigned long long undefined8;
typedef unsigned char byte;
typedef unsigned long ulong;
typedef unsigned int uint;
typedef unsigned __int128 uint128;
union Blob {
 uint128 all;
 unsigned char bytes[16];
 uint64_t q[2];
 uint32_t u[4];
 int32_t i[4];
 float f[4];
 unsigned char& operator[](size_t n) {return bytes[n];}
 operator void*() {return bytes;}
};
inline Blob operator^(Blob a,Blob b) {a.all^=b.all;return a;}
inline Blob operator&(Blob a,Blob b) {a.all&=b.all;return a;}
inline Blob operator~(Blob a) {a.all=~a.all;return a;}
inline float gh_float(uint32_t x) {float f;memcpy(&f,&x,4);return f;}
template<class T> inline float gh_float(T x) {
 if constexpr (std::is_unsigned<T>::value) return gh_float((uint32_t)x);
 else return (float)x;
}
inline float gh_float(int x) {return (float)x;}
// ProcessInt's four FCVTXN boundaries use round-to-odd, not a C cast.
inline float gh_float(double x) {
 float y=(float)x;
 if ((double)y==x || !isfinite(x)) return y;
 uint32_t bits;memcpy(&bits,&y,4);
 if (!(bits&1)) {if(fabs((double)y)>fabs(x))--bits;bits|=1;}
 memcpy(&y,&bits,4);return y;
}
inline float gh_float(float x) {return x;}

inline uint32_t gh_uint(float x) {uint32_t u;memcpy(&u,&x,4);return u;}
template<class T> inline uint32_t gh_uint(T x) {return (uint32_t)x;}
inline uint64_t bits(float x) {return gh_uint(x);}
inline uint64_t bits(double x) {return gh_uint((float)x);}
template<class T> inline uint64_t bits(T x) {return (uint64_t)x;}
template<class H,class L> uint64_t concat(H h,L l,int n) {
 return (bits(h)<<(n*8)) | (bits(l) & ((uint64_t(1)<<(n*8))-1));
}
#define CONCAT11(h,l) concat(h,l,1)
#define CONCAT12(h,l) concat(h,l,2)
#define CONCAT13(h,l) ((uint32_t)concat(h,l,3))
#define CONCAT14(h,l) concat(h,l,4)
#define CONCAT15(h,l) concat(h,l,5)
#define CONCAT16(h,l) concat(h,l,6)
#define CONCAT17(h,l) concat(h,l,7)
#define CONCAT44(h,l) concat(h,l,4)
#define SUB41(x,n) ((byte)(bits(x)>>((n)*8)))
#define SUB128(x,n) ((uint64_t)((x).all>>((n)*8)))
#define SUB164(x,n) ((uint32_t)((x).all>>((n)*8)))
inline Blob NEON_ext(Blob a,Blob b,int n,int) {
 Blob r={};for(int i=0;i<16;i++)r.bytes[i]=i+n<16?a.bytes[i+n]:b.bytes[i+n-16];return r;
}
inline Blob NEON_scvtf(Blob a,int) {
 Blob r={};for(int i=0;i<4;i++)r.f[i]=(float)a.i[i];return r;
}
inline Blob NEON_frecpe(Blob a,int) {
 Blob r={};
#if defined(__aarch64__) && !defined(VAN51_SCALAR_RECIPROCAL)
 float32x4_t v;memcpy(&v,&a,16);v=vrecpeq_f32(v);memcpy(&r,&v,16);
#else
 for(int i=0;i<4;i++)r.f[i]=1.0f/a.f[i];
#endif
 return r;
}
inline Blob NEON_frecps(Blob a,Blob b,int) {
 Blob r={};
#if defined(__aarch64__) && !defined(VAN51_SCALAR_RECIPROCAL)
 float32x4_t v,w;memcpy(&v,&a,16);memcpy(&w,&b,16);v=vrecpsq_f32(v,w);memcpy(&r,&v,16);
#else
 for(int i=0;i<4;i++)r.f[i]=2.0f-a.f[i]*b.f[i];
#endif
 return r;
}
#define _bzero(p,n) memset(p,0,n)
#define _memcpy memcpy
'''

# Read one C unary operand, including casts, calls and member/index postfixes.
def operand_end(s, pos):
    n = len(s)
    while pos < n and s[pos].isspace(): pos += 1
    if s[pos] in '*&~!+-': return operand_end(s,pos+1)
    if s[pos] == '(':
        end = balanced_end(s,pos)
        inner = s[pos+1:end-1].strip()
        if re.fullmatch(r'(?:const\s+)?(?:float|double|u?int|ulong|long|byte|undefined[148]|Blob)(?:\s*\*)*', inner):
            return operand_end(s,end)
        pos = end
    else:
        m = re.match(r'(?:[A-Za-z_]\w*|(?:0x[\da-fA-F]+|\d+(?:\.\d*)?)(?:[eE][+-]?\d+)?[uUlLfF]*)', s[pos:])
        if not m: raise ValueError('operand '+repr(s[pos:pos+100]))
        pos += m.end()
    while pos<n:
        q=pos
        while q<n and s[q].isspace():q+=1
        if q<n and s[q] in '([': pos=balanced_end(s,q)
        elif q<n and s[q]=='.':
            m=re.match(r'\.\w+',s[q:]);pos=q+m.end()
        else:break
    return pos

def balanced_end(s,pos):
    stack=[s[pos]];pairs={')':'(',']':'['}
    for i in range(pos+1,len(s)):
        if s[i] in '([':stack.append(s[i])
        elif s[i] in ')]':
            assert stack.pop()==pairs[s[i]]
            if not stack:return i+1
    raise ValueError('unbalanced')

def casts(s):
    # Work from the right so nested original casts are converted independently.
    matches=list(re.finditer(r'\((float|uint)\)',s))
    for m in reversed(matches):
        end=operand_end(s,m.end())
        s=s[:m.start()]+'gh_'+m.group(1)+'('+s[m.end():end]+')'+s[end:]
    return s

def adapt(source):
    s=source[source.index('void __ZN12BEffectVan5110ProcessInt'):]
    # The two Ghidra SUB128 aliases refer to the OLD vector read, not the
    # subsequently written memory. Assembly confirms standard history shifts.
    for off,pau in [('0x388','pauVar3'),('0x408','pauVar5')]:
        write=f'*(double *)*{pau} = dVar31;'
        old=f'*(long *)(lVar24 + {off}) = SUB128(*{pau},0);'
        assert write in s and old in s
        s=s.replace(write,f'uint64_t old_{pau} = SUB128(*{pau},0);\n          '+write)
        s=s.replace(old,f'*(long *)(lVar24 + {off}) = old_{pau};')
    s=re.sub(r'undefined1 \(\*(pauVar\d+)\) \[(?:12|16)\];',r'Blob *\1;',s)
    s=re.sub(r'undefined1 (auVar\d+) \[(?:12|16)\];',r'Blob \1 = {};',s)
    s=re.sub(r'\(undefined1 \(\*\) \[(?:12|16)\]\)',r'(Blob *)',s)
    s=re.sub(r'\(\*(pauVar\d+)\)',r'(*\1)',s)
    # Casts through native array values mean the address of their first byte.
    s=re.sub(r'\*\((float|double|undefined8) \*\)\*(pauVar\d+)',r'*(\1 *)&(*\2)',s)
    s=casts(s)
    # Each subpiece assignment stores bits of RHS; 12-byte slices copy low96.
    def assign(m):
        var,at,size,rhs=m.groups();at=int(at);size=int(size)
        if size==12:
            return f'{var}.all = ({var}.all & (~uint128(0) << 96)) | ({rhs}.all & ((uint128(1)<<96)-1));'
        if size==8:return f'{var}.q[{at//8}] = {rhs};'
        return f'{var}.u[{at//4}] = (uint32_t)bits({rhs});'
    s=re.sub(r'(auVar\d+)\._(\d+)_(\d+)_\s*=\s*(.*?);',assign,s,flags=re.S)
    # Remaining slices are reads. The integer vectors are only used as integer
    # indices until auVar48 is reused for a floating ext/reciprocal near the end.
    float48=s.index('auVar48 = NEON_ext(auVar80')
    def read(m):
        var,at,size=m.groups();at=int(at);size=int(size)
        if size==8:return f'{var}.q[{at//8}]'
        if size==12:return var
        integer=var in {'auVar49','auVar95','auVar85','auVar116','auVar96'} or (var=='auVar48' and m.start()<float48)
        return f'{var}.{"u" if integer else "f"}[{at//4}]'
    s=re.sub(r'(auVar\d+)\._(\d+)_(\d+)_',read,s)
    # Recovered ProcessInt scalar constants are binary32, including limits.
    s=re.sub(r'(?<![\w.])(?:\d+\.\d*(?:e[+-]?\d+)?)(?![\w.])',lambda m:m.group(0)+'f',s)
    return HEADER+'\n'+s

MAIN = r'''
int main(int argc,char**argv) {
 if(argc!=6){fprintf(stderr,"state input setup output stateout\n");return 2;}
 alignas(16) unsigned char mem[0xa000]={};
 FILE*f=fopen(argv[1],"rb");if(!f || fread(mem,1,sizeof mem,f)!=sizeof mem)return 3;fclose(f);
 f=fopen(argv[2],"rb");if(!f)return 3;fseek(f,0,SEEK_END);long bytes=ftell(f);rewind(f);
 int frames=bytes/8;if(frames<1 || frames>1024)return 4;
 float*in=(float*)calloc(frames*2,sizeof(float));fread(in,4,frames*2,f);fclose(f);
 float*channels[2];for(int c=0;c<2;c++){channels[c]=(float*)calloc(frames,sizeof(float));for(int i=0;i<frames;i++)channels[c][i]=in[i*2+c];}
 for(int c=0;c<2;c++)memcpy(mem+0x20+c*8,&channels[c],8);
 *(int*)(mem+0x124)=2;*(int*)(mem+0x128)=2;
 float tables[12][512]={};
 f=fopen(argv[3],"rb");if(!f)return 3;
 for(int t=0;t<12;t++){
  unsigned char header[20];float values[256],slopes[256];
  if(fread(header,1,20,f)!=20 || fread(values,4,256,f)!=256 || fread(slopes,4,256,f)!=256)return 3;
  for(int i=0;i<256;i++){tables[t][i*2]=values[i];tables[t][i*2+1]=slopes[i];}
 }
 fclose(f);
 int map[16]={0,8,4,3,5,0,0,0,2,6,9,10,1,7,11,11};
 for(int i=0;i<16;i++){void*p=tables[map[i]];memcpy(mem+0x9c80+i*8,&p,8);}
 unsigned char*data=(unsigned char*)calloc(0xb0000,1);
 __ZN12BEffectVan5110ProcessIntER12BProcessDataiib((long)mem,(long)data,0,frames);
 f=fopen(argv[4],"wb");if(!f)return 3;
 for(int i=0;i<frames;i++)for(int c=0;c<2;c++)fwrite(data+0x9c010+c*0x1000+i*4,4,1,f);fclose(f);
 // Do not persist process-local input/table pointers.
 memset(mem+0x20,0,16);memset(mem+0x9c80,0,128);
 f=fopen(argv[5],"wb");if(!f)return 3;fwrite(mem,1,sizeof mem,f);fclose(f);
 return 0;
}
'''


def synthetic_snapshot(case, setup):
    """Explicit stable test coefficients, independent of Rust initialization.

    These are deliberately arbitrary circuit inputs, not recovered native knob
    presets. Original table metadata and FIR constants retain their real layout.
    The cases exercise both nonlinear branches, asymmetric channel histories,
    history shifts, current/delta updates and mono downmix/broadcast.
    """
    state=bytearray(0xa000)
    def put(offset,*values):struct.pack_into('<'+'f'*len(values),state,offset,*values)
    def doubles(offset,*values):struct.pack_into('<'+'d'*len(values),state,offset,*values)
    struct.pack_into('<ii',state,0x124,2,2)
    state[0x1c0]=int(case==3)
    put(0x94ac,0.5,0.7)
    put(0x94f0,*[0.25+0.01*(i%5) for i in range(28)])
    if case>=1:
        put(0x9560,*[(-1 if i%2 else 1)*(i+1)*2.0e-7 for i in range(28)])
    up=[[-0.0234375,0.2265625,0.8671875,-0.0703125],
        [-0.0625,0.5625,0.5625,-0.0625],
        [-0.0703125,0.8671875,0.2265625,-0.0234375],[0,1,0,0]]
    for phase,row in enumerate(up):put(0x9690-phase*16,*row)
    put(0x96a0,*[-0.005859375,-0.015625,-0.017578125,0,0.056640625,
                0.140625,0.216796875,0.25,0.216796875,0.140625,
                0.056640625,-0.0,-0.017578125,-0.015625,-0.005859375,0])
    for off in range(0x96e0,0x9750,4):put(off,0.01)
    for off in range(0x9750,0x9980,0x50):
        doubles(off,*[0.1,0.03,0.01,0.02,0.01,0.01,0.001,0.001,0.002,0])
    threshold=[2.3,-0.002,0.0,0.02][case]
    for base in [0x9980,0x9a20,0x9ac0]:
        values=[0.2,0.01,0.8,0.01,-0.02,threshold,0.05,0.1,0.0,0.0]
        for j,val in enumerate(values):put(base+j*16,*[val]*4)
    table_map=[0,8,4,3,5,None,None,None,2,6,9,10,1,7,11,11]
    for lane,table in enumerate(table_map):
        if table is None:continue
        anchor,fraction,scale,offset,baseline=struct.unpack_from('<i4f',setup,table*2068)
        put(0x9b80+lane*4,scale)
        put(0x9bc0+lane*4,offset)
        put(0x9c00+lane*4,255-anchor)
        put(0x9c40+lane*4,-anchor)
    put(0x9b60,*[1]*8)
    put(0x9d10,0.001,0.000001,0.1,0.1)
    for channel in range(2):
        base=channel*0x310
        if case>=1:
            for off in range(0x220,0x270,4):
                put(base+off,0.003*math.sin(off*0.1+channel))
            for off in range(0x280,0x440,8):
                doubles(base+off,0.0001*math.cos(off*0.1+channel))
            for off in range(0x460,0x4e0,4):
                put(base+off,0.002*math.sin(off*0.1+channel))
        put(base+0x270,1,1,1,1)
        put(base+0x440,0,0.001,1,0)
        put(base+0x450,1,1,1,1)
    return state


def generate_fixture(args, exe):
    cases=[]
    setup=args.setup.read_bytes()
    for case,frames in enumerate([512,113,137,64]):
        state=synthetic_snapshot(case,setup)
        amplitude=[0.1,0.7,4.0,0.3][case]
        values=[v for i in range(frames) for v in
                [amplitude*math.sin(i*0.17),-amplitude*math.cos(i*0.23)]]
        inputs=struct.pack('<'+'f'*len(values),*values)
        cases.append((f'synthetic-{case}',frames,bytes(state),inputs))
    if args.initial_state:
        state=args.initial_state.read_bytes()
        frames=512
        values=[v for i in range(frames) for v in [0.1*math.sin(i*0.17),-0.1*math.cos(i*0.23)]]
        cases.append(('initialized-explicit-snapshot',frames,state,struct.pack('<1024f',*values)))
    out=bytearray(b'VANPROC1'+struct.pack('<I',len(cases)))
    summary=[]
    for name,frames,state,inputs in cases:
        state_path=args.build_dir/(name+'-initial.bin')
        input_path=args.build_dir/(name+'-input.bin')
        output_path=args.build_dir/(name+'-output.bin')
        final_path=args.build_dir/(name+'-final.bin')
        state_path.write_bytes(state);input_path.write_bytes(inputs)
        if args.native_process:
            subprocess.run([sys.executable,str(Path(__file__).with_name('van51_native_process_reference.py')),
                '--state',str(state_path),'--input',str(input_path),'--setup',str(args.setup),
                '--output',str(output_path),'--state-output',str(final_path),'--block-size',str(frames)],check=True,stdout=subprocess.DEVNULL)
        else:
            subprocess.run([str(exe),str(state_path),str(input_path),str(args.setup),str(output_path),str(final_path)],check=True)
        expected=output_path.read_bytes();final=final_path.read_bytes()
        numbers=struct.unpack('<'+'f'*(frames*2),expected)
        if not all(math.isfinite(v) for v in numbers):
            raise ValueError('Non-finite oracle output in '+name)
        out.extend(struct.pack('<I',frames));out.extend(state)
        for i in range(frames):out.extend(inputs[i*8:i*8+8]+expected[i*8:i*8+8])
        out.extend(final)
        summary.append({'name':name,'frames':frames,'mono':state[0x1c0]!=0,
                        'initial_sha256':hashlib.sha256(state).hexdigest(),
                        'input_sha256':hashlib.sha256(inputs).hexdigest(),
                        'output_sha256':hashlib.sha256(expected).hexdigest(),
                        'final_state_sha256':hashlib.sha256(final).hexdigest()})
    args.fixture.write_bytes(out)
    return summary

def main():
    p=argparse.ArgumentParser(description=__doc__)
    p.add_argument('--build-dir',type=Path,default=Path('/tmp/van51-process-oracle'))
    p.add_argument('--scalar-reciprocal',action='store_true')
    p.add_argument('--native-process',action='store_true',help='execute isolated original ARM64 ProcessInt for the goldens')
    p.add_argument('--fixture',type=Path,help='generate VANPROC1 synthetic circuit regression fixture')
    p.add_argument('--initial-state',type=Path,help='append caller-supplied initialized state case to fixture')
    p.add_argument('--state',type=Path)
    p.add_argument('--input',type=Path)
    p.add_argument('--output',type=Path)
    p.add_argument('--state-output',type=Path)
    p.add_argument('--setup',type=Path,default=Path(__file__).with_name('van51-setup.bin'))
    a=p.parse_args();a.build_dir.mkdir(parents=True,exist_ok=True)
    cpp=a.build_dir/'oracle.cpp';exe=a.build_dir/'oracle'
    cpp.write_text(adapt(SOURCE.read_text())+'\n'+MAIN)
    subprocess.run(['clang++','-std=c++17','-O0','-ffp-contract=off','-fno-strict-aliasing']+(['-DVAN51_SCALAR_RECIPROCAL'] if a.scalar_reciprocal else [])+[str(cpp),'-o',str(exe)],check=True)
    if a.state:
        if not all([a.input,a.output,a.state_output]):p.error('state needs input/output/state-output')
        subprocess.run([str(exe),str(a.state),str(a.input),str(a.setup),str(a.output),str(a.state_output)],check=True)
    cases=generate_fixture(a,exe) if a.fixture else []
    provenance={
        'origin':('Isolated original ARM64 ProcessInt instruction execution' if a.native_process else 'Compiled saved Ghidra ProcessInt with mechanical vector/cast restoration; static replay only, not Kontakt runtime'),
        'source':str(SOURCE.relative_to(ROOT)),
        'source_sha256':hashlib.sha256(SOURCE.read_bytes()).hexdigest(),
        'binary_sha256':'42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3',
        'analysis_profile':'040072c9c8ab8c896677a5e18badb276196ce107557bde41bc438752d586aebd',
        'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
        'native_generator_sha256':hashlib.sha256(Path(__file__).with_name('van51_native_process_reference.py').read_bytes()).hexdigest() if a.native_process else None,
        'generated_cpp_sha256':None if a.native_process else hashlib.sha256(cpp.read_bytes()).hexdigest(),
        'setup_sha256':hashlib.sha256(a.setup.read_bytes()).hexdigest(),
        'arithmetic':('Original ARM64 instructions; native reciprocal, narrowing and fused operations' if a.native_process else 'float constants restored to binary32; FCVTXN round-to-odd narrowing; ffp-contract=off; '+('scalar 1/x then frecps approximation' if a.scalar_reciprocal else 'AArch64 native frecpe/frecps (scalar approximation on non-AArch64)')),
        'corrections':['Ghidra alias at channel+388: retain old390 before write; similarly +408 retains old410; native assembly 1005d654c..6560 confirms ordinary history shift'],
        'fixture_cases':cases,
        'fixture_schema':'VANPROC1 +u32case_count; each case u32frames,initial_state[0xa000],perframe[inputL,inputR,expectedL,expectedR]f32LE,final_state[0xa000]',
        'schema':'input/output interleaved stereo f32 LE (1..1024 frames); state is 40960 raw bytes; table/input pointer slots rebound locally and zeroed on export',
        'limitations':(['Explicit snapshots are circuit inputs; original ProcessInt execution does not validate the full host lifecycle'] if a.native_process else
            ['Initial coefficient/current snapshots are explicit circuit inputs; synthetic cases use arbitrary stable coefficients and do not validate SetVan51/SetSampleRate',
             'Compiled pseudocode replay, scalar non-fused arithmetic with explicit FCVTXN narrowing; no original ProcessInt execution']),
    }
    if a.fixture:
        provenance['fixture_sha256']=hashlib.sha256(a.fixture.read_bytes()).hexdigest()
        a.fixture.with_suffix('.json').write_text(json.dumps(provenance,indent=2)+'\n')
    (a.build_dir/'provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
    print(exe)
if __name__=='__main__':main()
