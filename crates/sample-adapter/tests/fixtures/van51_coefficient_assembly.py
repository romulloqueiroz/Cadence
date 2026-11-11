#!/usr/bin/env python3
"""Emit straight-line Rust SSA from the saved VAN51 ARM64 polynomial helpers.

Unlike the Ghidra scalar locals, instruction registers retain every overwritten
lane separately. Fused instructions use f64::mul_add. Only mnemonics encountered
in these two pure helpers are supported; other instructions fail explicitly.
The script does not execute the Kontakt image. Native numerical verification
is performed separately by van51_coefficients_reference.py's recorded oracle.

CLI: python3 van51_coefficient_assembly.py ASSEMBLY [OUTPUT_OFFSETS ...]
"""
import re,struct,pathlib,sys,math
ROOT=pathlib.Path(__file__).resolve().parents[4]
BINARY_PATH=ROOT.parent/'Kontakt-8-REA/Kontakt-8.arm64'
BINARY=BINARY_PATH.read_bytes()
class Replay:
 def __init__(self):
  self.nodes=[];self.cache={};self.x={'x0':0x10000000000,'sp':0x20000000000,'xzr':0,'wzr':0};self.v={f'v{i}':[self.const(0.0)]*2 for i in range(32)};self.mem={};self.outputs={}
 def node(self,op,*args):
  if op not in ('const','state32','state64') and all(self.nodes[n][0]=='const' for n in args):
   a=self.nodes[args[0]][1][0];b=self.nodes[args[1]][1][0] if len(args)>1 else None;c=self.nodes[args[2]][1][0] if len(args)>2 else None
   val={'fmul':lambda:a*b,'fadd':lambda:a+b,'fsub':lambda:a-b,'fdiv':lambda:a/b,'fneg':lambda:-a,'fnmul':lambda:-(a*b),'fmadd':lambda:math.fma(a,b,c),'fmla':lambda:math.fma(a,b,c),'fmls':lambda:math.fma(-a,b,c),'fmsub':lambda:math.fma(-a,b,c),'fnmsub':lambda:math.fma(a,b,-c),'fnmadd':lambda:math.fma(-a,b,-c)}[op]()
   op,args='const',(val,)
  key=(op,struct.pack('<d',args[0])) if op=='const' else (op,args)
  if key in self.cache:return self.cache[key]
  n=len(self.nodes);self.nodes.append((op,args));self.cache[key]=n;return n
 def const(self,f):return self.node('const',f)
 def get(self,r):
  m=re.fullmatch(r'v(\d+)\[(\d)\]',r)
  if m:return self.v['v'+m[1]][int(m[2])]
  if r[0] in 'ds':return self.v['v'+r[1:]][0]
  raise Exception(r)
 def set(self,r,n):self.v['v'+r[1:]]=[n,self.const(0.0)]
 def getx(self,r):return self.x.get('x'+r[1:] if r.startswith('w') else r,0)
 def setx(self,r,n):self.x['x'+r[1:] if r.startswith('w') else r]=n & (0xffffffff if r.startswith('w') else 0xffffffffffffffff)
 def address(self,part):
  m=re.fullmatch(r'\[(\w+)(?:, (#?-?0x[\da-f]+|#?-?\d+|[xw]\d+))?\](!)?',part);assert m,part
  reg,off,pre=m.groups();off=(self.getx(off) if off[0] in 'xw' else int(off.lstrip('#'),0)) if off else 0
  if pre:self.setx(reg,self.getx(reg)+off);off=0
  return self.getx(reg)+off
 def load(self,addr,width):
  if addr in self.mem:return self.mem[addr]
  if addr>=0x20000000000-0x1000:return self.const(0.0)
  if addr>=0x10000000000:return self.node('state32' if width==4 else 'state64',addr-0x10000000000)
  return self.const(struct.unpack_from('<f' if width==4 else '<d',BINARY,addr-0x100000000)[0])
 def store(self,addr,node):
  self.mem[addr]=node
  if 0x10000000000<=addr<0x10000010000:self.outputs[addr-0x10000000000]=node
 def vector(self,arg):
  if '[' in arg:return [self.get(arg)]*2
  return self.v[arg][:]
 def run(self,path,stop=None):
  for line in open(path):
   m=re.match(r'([0-9a-f]+):\s+([0-9a-f]+)\s+([\w.]+)\s*(.*)',line)
   if not m:continue
   pc=int(m[1],16)
   if pc==stop:break
   op=m[3];arg=m[4].split(';')[0].strip();parts=re.split(r', (?![^\[]*\])',arg)
   try:
    if op=='ret':break
    elif op in ('mov','movk','adrp','add','sub'):
     if op=='mov':
      if parts[0][0]=='d':self.set(parts[0],self.get(parts[1]))
      else:self.setx(parts[0],int(parts[1][1:],0) if parts[1].startswith('#') else self.getx(parts[1]))
     elif op=='adrp':self.setx(parts[0],int(parts[1].split()[0],0))
     elif op=='movk':
      sh=int(parts[2].split('#')[1]) if len(parts)>2 else 0;self.setx(parts[0],(self.getx(parts[0]) & ~(0xffff<<sh))|(int(parts[1][1:],0)<<sh))
     else:
      a=self.getx(parts[1]);b=int(parts[2][1:],0) if parts[2].startswith('#') else self.getx(parts[2]);self.setx(parts[0],a+b if op=='add' else a-b)
    elif op in ('ldr','str','strb','ldp','stp'):
     pair=op in ('ldp','stp');regs=parts[:2] if pair else parts[:1];addresspart=parts[2] if pair else parts[1];addr=self.address(addresspart)
     for r in regs:
      width={'q':16,'d':8,'s':4,'x':8,'w':4}[r[0]]
      if op in ('ldr','ldp'):
       if r.startswith('q'):self.v['v'+r[1:]]=[self.load(addr,8),self.load(addr+8,8)]
       elif r[0] in 'ds':self.set(r,self.load(addr,width))
       else:self.setx(r,0)
      else:
       if r.startswith('q'):
        for i,n in enumerate(self.v['v'+r[1:]]):self.store(addr+i*8,n)
       elif r[0] in 'ds':self.store(addr,self.get(r))
       elif r=='xzr':self.store(addr,self.const(0.0))
      addr+=width
     if len(parts)>(3 if pair else 2):self.setx('sp',self.getx('sp')+int(parts[-1][1:],0))
    elif op=='fcvt':self.set(parts[0],self.get(parts[1]))
    elif op=='fmov':
     val=parts[1]
     n=self.const(float(val[1:])) if val.startswith('#') else self.const(struct.unpack('<d',struct.pack('<Q',self.getx(val)))[0]) if val.startswith('x') else self.get(val)
     self.set(parts[0],n)
    elif op=='mov.16b':self.v[parts[0]]=self.v[parts[1]][:]
    elif op=='mov.d':
     target=re.fullmatch(r'(v\d+)\[(\d)\]',parts[0]);self.v[target[1]][int(target[2])]=self.get(parts[1])
    elif op=='dup.2d':
     val=parts[1];n=self.const(struct.unpack('<d',struct.pack('<Q',self.getx(val)))[0]) if val.startswith('x') else self.get(val);self.v[parts[0]]=[n,n]
    elif op=='fmov.2d':self.v[parts[0]]=[self.const(float(parts[1][1:]))]*2
    elif op=='movi.2d':self.v[parts[0]]=[self.const(0.0)]*2
    elif op in ('zip1.2d','zip2.2d'):
     lane=int(op[3])-1;self.v[parts[0]]=[self.v[parts[1]][lane],self.v[parts[2]][lane]]
    elif op=='ext.16b':
     lane=int(parts[3][1:],0)//8;self.v[parts[0]]=(self.v[parts[1]]+self.v[parts[2]])[lane:lane+2]
    elif op.endswith('.2d'):
     arith=op.split('.')[0];a=self.vector(parts[1]);b=self.vector(parts[2]) if len(parts)>2 else None;old=self.vector(parts[0]);ns=[]
     for i in range(2):ns.append(self.node(arith,a[i],b[i],old[i]) if arith in ('fmla','fmls') else self.node(arith,a[i],b[i]) if b else self.node(arith,a[i]))
     self.v[parts[0]]=ns
    elif op.endswith('.d'):
     arith=op.split('.')[0];a=self.get(parts[1]);b=self.get(parts[2]);self.set(parts[0],self.node(arith,a,b,self.get(parts[0])) if arith in ('fmla','fmls') else self.node(arith,a,b))
    elif op in ('fmadd','fmsub','fnmadd','fnmsub'):
     self.set(parts[0],self.node(op,*[self.get(p) for p in parts[1:]]))
    elif op in ('fmul','fadd','fsub','fdiv','fnmul','fneg'):
     self.set(parts[0],self.node(op,*[self.get(p) for p in parts[1:]]))
    else:raise Exception('unknown '+op)
   except Exception as e:raise Exception(hex(pc)+' '+op+' '+arg) from e
  return self
 def rust(self,targets):
  needed=set()
  def dep(n):
   if n in needed:return
   needed.add(n);op,args=self.nodes[n]
   if op not in ('const','state32','state64'):
    for arg in args:dep(arg)
  for target in targets:dep(self.outputs[target])
  lines=[]
  def v(n):return f'c{n}'
  for n in sorted(needed):
   op,args=self.nodes[n]
   if op=='const':value=repr(args[0])
   elif op=='state32':value=f'f64::from(s.f32({hex(args[0])}))'
   elif op=='state64':value=f's.f64({hex(args[0])})'
   else:
    a=v(args[0]);b=v(args[1]) if len(args)>1 else None;c=v(args[2]) if len(args)>2 else None
    value={'fmul':lambda:f'{a}*{b}','fadd':lambda:f'{a}+{b}','fsub':lambda:f'{a}-{b}','fdiv':lambda:f'{a}/{b}','fneg':lambda:f'-{a}','fnmul':lambda:f'-({a}*{b})','fmadd':lambda:f'{a}.mul_add({b},{c})','fmla':lambda:f'{a}.mul_add({b},{c})','fmls':lambda:f'(-{a}).mul_add({b},{c})','fmsub':lambda:f'(-{a}).mul_add({b},{c})','fnmsub':lambda:f'{a}.mul_add({b},-{c})','fnmadd':lambda:f'(-{a}).mul_add({b},-{c})'}[op]()
   lines.append(f'    let {v(n)}: f64 = {value};')
  for target in targets:lines.append(f'    s.set_f64({hex(target)}, {v(self.outputs[target])});')
  return '\n'.join(lines)
def rust_helper(assembly_path, helper):
 """Generate a complete owner for one filter, including fade-history changes."""
 name, address, current, outgoing, history, old_history, control = {
  'tone': ('set_tone_stack', '0x1005cf4c8..0x1005d353c', 0x9750, 0x97a0, 0x280, 0x2c0, 0x94f0),
  'lead': ('set_stage2_lead_filter', '0x1005d3d2c..0x1005d4fdc', 0x97f0, 0x9840, 0x300, 0x340, 0x9530),
  'feedback': ('set_feedback_filter', '0x1005d3540..0x1005d3d28', 0x9890, 0x98e0, 0x380, 0x400, 0x9540),
 }[helper]
 replay=Replay().run(assembly_path)
 body=replay.rust(list(range(current,current+0x50,8)))
 header=f"""//! Generated from VAN51 `{name}` ARM64 instructions ({address}).
//! Register SSA and fused operations preserve overwritten native accumulators.
//! Regenerate with `van51_coefficient_assembly.py --helper {helper} ASSEMBLY`.
use crate::van51_dsp::Van51State;

#[allow(clippy::too_many_lines, clippy::excessive_precision)]
pub(super) fn {name}(s: &mut Van51State) {{
    // Preserve the outgoing filter while new coefficients/history fade in.
    for offset in ({hex(current)}..{hex(current+0x50)}).step_by(8) {{
        s.set_f64(offset + {hex(outgoing-current)}, s.f64(offset));
    }}
    for channel in 0..16 {{
        let base = channel * 0x310;
        for offset in ({hex(history)}..{hex(history+0x40)}).step_by(8) {{
            s.set_f64(base + offset + {hex(old_history-history)}, s.f64(base + offset));
            s.set_f64(base + offset, 0.0);
        }}
    }}
"""
 return header+body+f"""
    s.set_u32({hex(control)}, 0);
    s.set_u8(0x9640, 1);
    s.set_u32(0x9650, 1);
}}
"""

if __name__=='__main__':
 import argparse
 parser=argparse.ArgumentParser(description=__doc__)
 parser.add_argument('assembly',type=pathlib.Path)
 parser.add_argument('offsets',nargs='*',type=lambda s:int(s,0))
 parser.add_argument('--helper',choices=['tone','lead','feedback'])
 args=parser.parse_args()
 if args.helper:print(rust_helper(args.assembly,args.helper),end='')
 else:print(Replay().run(args.assembly).rust(args.offsets))
