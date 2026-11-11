#!/usr/bin/env python3
"""Rebuild BassInvader's Rust numeric source from SHA-bound ARM64 disassembly.
Only decoded operations are emitted; no machine code is stored or executed.
"""
import re,struct,hashlib,subprocess
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4]; BINARY=ROOT.parent/'Kontakt-8-REA/Kontakt-8.arm64'; OUT=ROOT/'crates/sample-adapter/src'
EXPECTED='42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3'
B=BINARY.read_bytes();assert hashlib.sha256(B).hexdigest()==EXPECTED
FUNCS={4321959044: 'construct', 4321964100: 'process', 4321966728: 'smooth', 4321967448: 'boost', 4321968188: 'bass_treble', 4321969316: 'rate', 4321970768: 'stabilize', 4321979916: 'assign5', 4321980856: 'assign9', 4321981736: 'input_output', 4321982404: 'input_dru', 4321983056: 'input_mat', 4321984044: 'assign4', 4321984840: 'himid_output', 4321985496: 'matrix', 4321987156: 'himid_dru', 4321988136: 'himid_mat', 4321989016: 'assign_lomid', 4321989896: 'lomid_dru', 4321990636: 'lomid_mat', 4321991616: 'power_output', 4321992236: 'leak3', 4321993060: 'leak4', 4323835476: 'eq', 4323842364: 'eq_ctor', 4321976872: 'parameter0', 4321977376: 'parameter1', 4321977764: 'parameter2', 4321978152: 'parameter3', 4321978540: 'parameter4', 4321978776: 'parameter5', 4321979012: 'parameter6', 4321979236: 'parameter7', 4321979556: 'parameter8', 4321979780: 'parameter9'}
raw='\n'.join(subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-objdump','-d',f'--start-address={a}',f'--stop-address={b}',str(BINARY)],text=True) for a,b in [(0x1019bdc84,0x1019c67d0),(0x101b87e54,0x101b89960)])
FUNCS[0x1004ffc1c]='start_smooth'
raw+='\n'+subprocess.check_output(['/opt/homebrew/opt/llvm/bin/llvm-objdump','-d','--start-address=0x1004ffc1c','--stop-address=0x100500090',str(BINARY)],text=True)
functions={};current=None;pages=set()
for line in raw.splitlines():
 m=re.match(r'([0-9a-f]+) <',line)
 if m:current=int(m[1],16);continue
 m=re.match(r'([0-9a-f]+):\s+[0-9a-f]+\s+([\w.]+)\s*(.*)',line)
 if not m or current not in FUNCS:continue
 pc=int(m[1],16);op=m[2];args=m[3].split(';')[0].strip();args=re.sub(r' <.*','',args)
 if current==0x1019bdc84 and pc>=0x1019bec94:continue
 functions.setdefault(current,[]).append((pc,op,args))
 if op=='adrp':pages.add(int(args.split(', ')[1],0))
functions[0x1019bdc84].append((0x1019bec94,'ret',''))
# Only immutable data pages referenced by decoded ADRP instructions. ExpTable is
# reconstructed independently, so do not embed its zero-filled runtime storage.
pages=sorted(p for p in pages if not 0x109dc2000<=p<=0x109dc6000)
(OUT/'bassinvader_constants.bin').write_bytes(b''.join((OUT/'bassinvader_globals.bin').read_bytes() if p==0x109db5000 else B[p-0x100000000:p-0x100000000+4096] for p in pages))


def parts(s):return re.split(r',\s*(?![^\[]*\])(?![^{}]*\})',s)
def num(s):return int(s.lstrip('#'),0)
def ri(s):return 31 if s=='sp' else 32 if s in ('xzr','wzr') else int(s[1:])
def xr(s):
 if s.startswith('#'):return str(num(s))+'u64' if num(s)>=0 else f'({num(s)}i64 as u64)'
 if s in ('xzr','wzr'):return '0u64'
 return f'r.x[{ri(s)}]'+(' as u32 as u64' if s.startswith('w') else '')
def xset(s,expr):return '' if s in ('xzr','wzr') else f'r.x[{ri(s)}] = ({expr})'+(' as u32 as u64' if s.startswith('w') else '')+';'
def vi(s):return int(re.search(r'\d+',s)[0])
def vf(s,lane=0,kind=None):
 kind=kind or ('d' if s.startswith('d') else 's');lane=int(re.search(r'\[(\d+)\]',s)[1]) if '[' in s else lane
 return f'{"fd" if kind=="d" else "fs"}(r.v[{vi(s)}], {lane})'
def bits(s):
 if s[0] in 'xw':return f'({xr(s)}) as u128'
 return f'r.v[{vi(s)}]'+(' & 0xffffffff' if s[0]=='s' else ' & 0xffffffffffffffff' if s[0]=='d' else '')
def vset(s,expr):return f'r.v[{vi(s)}] = {expr};'
def fset(s,expr):return vset(s,f'({expr}).to_bits() as u128')
def shift_expr(val,opts):
 if not opts:return val
 sh=opts[0].split();op=sh[0];amount=num(sh[1]) if sh[1].startswith('#') else int(sh[1]);
 if op=='lsl':return f'({val}).wrapping_shl({amount})'
 if op=='lsr':return f'({val}).wrapping_shr({amount})'
 if op=='uxth':return f'(({val}) as u16 as u64).wrapping_shl({amount})'
 if op=='uxtw':return f'(({val}) as u32 as u64).wrapping_shl({amount})'
 if op=='sxtw':return f'(({val}) as u32 as i32 as i64 as u64).wrapping_shl({amount})'
 raise Exception(opts)
def address(p):
 m=re.fullmatch(r'\[(\w+)(?:,\s*(.*?))?\](!)?',p);assert m,p
 reg,extra,pre=m.groups();es=parts(extra) if extra else [];off=shift_expr(xr(es[0]),es[1:]) if es else '0u64';val=f'({xr(reg)}).wrapping_add({off})'
 return (xset(reg,val)+' let addr = '+xr(reg)+';' if pre else f'let addr = {val};')
def cond(c):return {'eq':'r.z','ne':'!r.z','lt':'r.n != r.o','ge':'r.n == r.o','le':'r.z || r.n != r.o','gt':'!r.z && r.n == r.o','lo':'!r.c','hs':'r.c','hi':'r.c && !r.z','ls':'!r.c || r.z','mi':'r.n','pl':'!r.n','vs':'r.o','vc':'!r.o'}[c]
def call(addr):
 if addr in FUNCS:return f'{FUNCS[addr]}(m, r);'
 if addr==0x1019c1db0:return 'm.smoothing_construct(r.x[0]);'
 if addr==0x1004ff8bc:return '' # callback host machinery handled by Rust owner
 if addr in (0x1043488d0,0x1043498b4):return 'panic!("native numeric assertion");'
 if addr==0x10434983c:return 'r.v[0] = exp10(fs(r.v[0],0)).to_bits() as u128;'
 if addr==0x10434a838:return 'r.v[0] = fs(r.v[0],0).powf(fs(r.v[1],0)).to_bits() as u128;'
 raise Exception(hex(addr))
def emit(pc,op,arg,current):
 if current==0x1019bdc84 and 0x1019bdca0<=pc<=0x1019bdcac:return ''
 p=parts(arg);dest=p[0] if p else '';base=op.split('.')[0];suffix=op.split('.')[1] if '.' in op else None
 if op=='ret':return 'm.finish_construct();return;' if current==0x1019bdc84 else 'return;'
 if op=='br':return f'm.parameter_callback(r, {xr(p[0])});return;' # Initialize's virtual Reset tail
 if op=='blr':return f'm.parameter_callback(r, {xr(p[0])});'
 if op=='bl':return call(int(p[0],0))
 if op=='b':
  target=int(p[0],0)
  return f'pc={target};continue;' if functions[current][0][0]<=target<=functions[current][-1][0] else call(target)+' return;'
 if op.startswith('b.'):return f'if {cond(suffix)} {{ pc={int(p[0],0)};continue; }}'
 if op in ('cbz','cbnz'):return f'if {xr(p[0])} {"==" if op=="cbz" else "!="} 0 {{ pc={int(p[1],0)};continue; }}'
 if op in ('tbz','tbnz'):return f'if ({xr(p[0])} & (1u64 << {num(p[1])})) {"==" if op=="tbz" else "!="} 0 {{ pc={int(p[2],0)};continue; }}'
 if base in ('ldr','ldur','ldrb','ldrh','ldrsw','str','stur','strb','strh','ldp','stp'):
  pair=base in ('ldp','stp');regs=p[:2] if pair else p[:1];ai=2 if pair else 1;out=address(p[ai]);offset=0
  for reg in regs:
   w={'q':16,'d':8,'s':4,'x':8,'w':4}[reg[0]]
   if base in ('ldrb','strb'):w=1
   if base in ('ldrh','strh'):w=2
   if base=='ldrsw':w=4
   addr=f'addr.wrapping_add({offset})'
   if base.startswith('ld'):
    val=f'm.read({addr}, {w})'
    out+=vset(reg,val) if reg[0] in 'qds' else xset(reg,f'{val} as u32 as i32 as i64 as u64' if base=='ldrsw' else f'{val} as u64')
   else:out+=f'm.write({addr}, {w}, {bits(reg)});'
   offset+=w
  if len(p)>ai+1:out+=xset(re.search(r'\[(\w+)',p[ai])[1],f'addr.wrapping_add({xr(p[ai+1])})')
  return '{'+out+'}'
 if op in ('ld1.s','st1.s','ld1r.2s','ld1r.4s'):
  reg=vi(p[0]);lane=int(re.search(r'\[(\d+)\]',p[0])[1]) if '[' in p[0] else 0;out=address(p[1])
  if op=='ld1.s':out+=f'r.v[{reg}] = lane(r.v[{reg}], {lane}, 32, m.read(addr,4));'
  elif op=='st1.s':out+=f'm.write(addr,4,(r.v[{reg}] >> {lane*32}) & 0xffffffff);'
  else:out+=f'let a=m.read(addr,4);r.v[{reg}]=a|(a<<32)'+('|(a<<64)|(a<<96)' if op=='ld1r.4s' else '')+';'
  if len(p)>2:out+=xset(re.search(r'\[(\w+)',p[1])[1],f'addr.wrapping_add({xr(p[2])})')
  return '{'+out+'}'
 if op=='negs':return f'r.compare(0,{xr(p[1])},{32 if dest[0]=="w" else 64},false);'+xset(dest,f'0u64.wrapping_sub({xr(p[1])})')
 if op=='neg':return xset(dest,f'0u64.wrapping_sub({shift_expr(xr(p[1]),p[2:])})')
 if op=='tst':return f'{{let v={xr(p[0])}&{xr(p[1])};r.z=v==0;r.n=v&(1u64<<{31 if p[0][0]=="w" else 63})!=0;r.c=false;r.o=false;}}'
 if op in ('cmp','cmn','ccmp'):
  expr=f'r.compare({xr(p[0])}, {xr(p[1])}, {32 if p[0].startswith("w") else 64}, {str(op=="cmn").lower()});'
  return f'if {cond(p[3])} {{ {expr} }} else {{ r.flags({num(p[2])}); }}' if op=='ccmp' else expr
 if op=='extr':return xset(dest,f'({xr(p[1])} << {64-num(p[3])}) | ({xr(p[2])} >> {num(p[3])})')
 if op=='cset':return xset(dest,f'({cond(p[1])})as u64')
 if op=='csneg':return xset(dest,f'if {cond(p[3])} {{ {xr(p[1])} }} else {{0u64.wrapping_sub({xr(p[2])})}}')
 if op in ('csel','csinc'):return xset(dest,f'if {cond(p[3])} {{ {xr(p[1])} }} else {{ '+(f'({xr(p[2])}).wrapping_add(1)' if op=='csinc' else xr(p[2]))+' }')
 if op=='adrp':return xset(dest,str(int(p[1],0))+'u64')
 if op=='movk':
  amount=int(p[2].split('#')[1]) if len(p)>2 else 0
  return xset(dest,f'({xr(dest)} & !(0xffffu64 << {amount})) | ({xr(p[1])} << {amount})')
 if op=='mov':return fset(dest,vf(p[1],kind=dest[0])) if dest[0] in 'ds' else xset(dest,xr(p[1]))
 if op in ('add','sub','adds','subs','and','lsl','lsr','asr','orr','eor','mul','msub','madd','smaddl','sbfiz','ubfiz','ubfx','sxtw'):
  a=xr(p[1]);b=shift_expr(xr(p[2]),p[3:]) if len(p)>2 and op not in ('madd','msub','smaddl','sbfiz','ubfiz','ubfx') else xr(p[2]) if len(p)>2 else ''
  if op in ('add','adds'):val=f'({a}).wrapping_add({b})'
  elif op in ('sub','subs'):val=f'({a}).wrapping_sub({b})'
  elif op=='and':val=f'{a} & {b}'
  elif op=='orr':val=f'{a} | {b}'
  elif op=='eor':val=f'{a} ^ {b}'
  elif op=='lsr':val=f'({a}).wrapping_shr(({b})as u32)'
  elif op=='asr':val=f'(({a})as i64).wrapping_shr(({b})as u32)as u64'
  elif op=='lsl':val=f'({a}).wrapping_shl(({b}) as u32)'
  elif op=='sbfiz':val=f'((((({a}) << {64-num(p[3])}) as i64) >> {64-num(p[3])}) as u64).wrapping_shl({num(p[2])})'
  elif op=='ubfx':val=f'(({a}) >> {num(p[2])}) & {(1<<num(p[3]))-1}u64'
  elif op=='ubfiz':val=f'(({a}) & {(1<<num(p[3]))-1}u64).wrapping_shl({num(p[2])})'
  elif op=='sxtw':val=f'({a}) as u32 as i32 as i64 as u64'
  elif op=='msub':val=f'({xr(p[3])}).wrapping_sub(({a}).wrapping_mul({b}))'
  elif op=='mul':val=f'({a}).wrapping_mul({b})'
  elif op=='madd':val=f'({a}).wrapping_mul({b}).wrapping_add({xr(p[3])})'
  else:val=f'(({a}) as u32 as i32 as i64).wrapping_mul(({b}) as u32 as i32 as i64).wrapping_add({xr(p[3])} as i64) as u64'
  pre=f'r.compare({a},{b},{32 if dest.startswith("w") else 64},{str(op=="adds").lower()});' if op in ('adds','subs') else ''
  return pre+xset(dest,val)
 if op in ('fmov','fcvt','fcvtzs','scvtf','fcvtxn'):
  if op=='fmov':
   if p[1].startswith('#'):return fset(dest,str(float(p[1][1:]))+('f64' if dest[0]=='d' else 'f32'))
   if dest[0] in 'xw':return xset(dest,f'({bits(p[1])}) as u64')
   return vset(dest,bits(p[1]))
  if op=='fcvt':return fset(dest,f'({vf(p[1])}) as {"f64" if dest[0]=="d" else "f32"}')
  if op=='fcvtxn':return fset(dest,f'round_odd({vf(p[1])})')
  if op=='fcvtzs':return xset(dest,f'({vf(p[1])}) as {"i32" if dest[0]=="w" else "i64"} as u64')
  return fset(dest,f'({bits(p[1]) if p[1][0] in "sd" else xr(p[1])}) as {"i32" if p[1][0] in "ws" else "i64"} as {"f64" if dest[0]=="d" else "f32"}')
 if op=='fcmp':return f'r.float_compare({vf(p[0])} as f64, '+('0.0' if p[1].startswith('#') else vf(p[1])+' as f64')+');'
 if op=='fcsel':return fset(dest,f'if {cond(p[3])} {{ {vf(p[1])} }} else {{ {vf(p[2])} }}')
 if op in ('fmul','fnmul','fadd','fsub','fdiv','fmadd','fmsub','fnmadd','fnmsub','fneg','fsqrt','fabs','fminnm','fmaxnm','fabd'):
  a=vf(p[1]);b=vf(p[2]) if len(p)>2 else None;c=vf(p[3]) if len(p)>3 else None
  val={'fmul':lambda:f'{a}*{b}','fnmul':lambda:f'-({a}*{b})','fadd':lambda:f'{a}+{b}','fsub':lambda:f'{a}-{b}','fdiv':lambda:f'{a}/{b}','fmadd':lambda:f'{a}.mul_add({b},{c})','fmsub':lambda:f'(-{a}).mul_add({b},{c})','fnmadd':lambda:f'(-{a}).mul_add({b},-{c})','fnmsub':lambda:f'{a}.mul_add({b},-{c})','fneg':lambda:f'-{a}','fabd':lambda:f'({a}-{b}).abs()','fsqrt':lambda:f'{a}.sqrt()','fabs':lambda:f'{a}.abs()','fminnm':lambda:f'{a}.min({b})','fmaxnm':lambda:f'{a}.max({b})'}[op]()
  return fset(dest,val)
 if op in ('mov.s','mov.d'):
  width=32 if suffix=='s' else 64;src=p[1];value=f'((r.v[{vi(src)}] >> {int(re.search(r"\[(\d+)\]",src)[1])*width}) & {hex((1<<width)-1)})'
  return xset(dest,f'{value} as u64') if dest[0] in 'xw' else f'r.v[{vi(dest)}]=lane(r.v[{vi(dest)}],{int(re.search(r"\[(\d+)\]",dest)[1])},{width},{value});'
 if op in ('mov.16b',):return vset(dest,bits(p[1]))
 if base in ('movi','fmov'):
  if op=='movi':return vset(dest,xr(p[1])+' as u128')
  count,width=(int(suffix[:-1]),{'s':32,'d':64}[suffix[-1]]);n=num(p[1]) if base=='movi' else float(p[1][1:]);val=str(n)+'u128' if base=='movi' else f'({n}{"f64" if width==64 else "f32"}).to_bits() as u128'
  if len(p)>2:val=f'({val}) << {num(p[2].split()[-1])}'
  return '{let a='+val+';'+vset(dest,'|'.join(f'(a << {i*width})' for i in range(count)))+'}'
 if base=='dup':
  count=int(suffix[:-1]);width={'s':32,'d':64}[suffix[-1]];src=p[1];val=xr(src)+' as u128' if src[0] in 'xw' else f'(r.v[{vi(src)}] >> {int(re.search(r"\[(\d+)\]",src)[1])*width}) & {hex((1<<width)-1)}'
  return '{let a='+val+';'+vset(dest,'|'.join(f'(a << {i*width})' for i in range(count)))+'}'
 if suffix=='16b' or suffix=='8b':
  a=bits(p[1]);b=bits(p[2]) if len(p)>2 else None;old=bits(dest)
  val={'and':lambda:f'{a}&{b}','orr':lambda:f'{a}|{b}','orn':lambda:f'{a}|!{b}','bic':lambda:f'{a}&!{b}','bsl':lambda:f'({old}&{a})|(!{old}&{b})','bit':lambda:f'({old}&!{b})|({a}&{b})','bif':lambda:f'({old}&{b})|({a}&!{b})','ext':lambda:f'({a} >> {num(p[3])*8}) | ({b} << {128-num(p[3])*8})'}[base]()
  return vset(dest,'('+val+')'+(' & 0xffffffffffffffff' if suffix=='8b' else ''))
 if base in ('fcvtl','fcvtn'):
  if base=='fcvtl':return vset(dest,f'(fs(r.v[{vi(p[1])}],0) as f64).to_bits() as u128 | (((fs(r.v[{vi(p[1])}],1) as f64).to_bits() as u128)<<64)')
  return vset(dest,f'(fd(r.v[{vi(p[1])}],0) as f32).to_bits() as u128 | (((fd(r.v[{vi(p[1])}],1) as f32).to_bits() as u128)<<32)')
 if op=='sshll2.2d':return vset(dest,f'((((r.v[{vi(p[1])}] >> 64) as u32 as i32 as i64).wrapping_shl({num(p[2])}) as u64) as u128) | (((((r.v[{vi(p[1])}] >> 96) as u32 as i32 as i64).wrapping_shl({num(p[2])}) as u64) as u128)<<64)')
 if base=='faddp' and dest[0] in 'sd':return fset(dest,f'{vf(p[1],0,dest[0])}+{vf(p[1],1,dest[0])}')
 if suffix in ('4s','2s','2d','s','d'):
  width=64 if suffix[-1]=='d' else 32;count=int(suffix[:-1] or '1');kind='d' if width==64 else 's';f='fd' if width==64 else 'fs';out=[]
  for i in range(count):
   def v(j,l=i):return vf(p[j],l,kind)
   a=v(1);b=v(2) if len(p)>2 else None;old=v(0)
   if base in ('zip1','zip2','uzp1','uzp2','trn1','trn2','rev64'):
    if base.startswith('zip'):source=1+i%2;idx=i//2+(count//2 if base=='zip2' else 0)
    elif base.startswith('uzp'):source=1+i//(count//2);idx=(i%(count//2))*2+(1 if base=='uzp2' else 0)
    elif base.startswith('trn'):source=1+i%2;idx=(i//2)*2+(1 if base=='trn2' else 0)
    else:source=1;idx=i^1
    val=f'(r.v[{vi(p[source])}] >> {idx*width}) & {hex((1<<width)-1)}'
   elif base=='faddp':
    source=1+i//(count//2);idx=(i%(count//2))*2;val=f'({vf(p[source],idx,kind)}+{vf(p[source],idx+1,kind)}).to_bits() as u128'
   elif base=='add':val=f'((r.v[{vi(p[1])}] >> {i*width}) as u32).wrapping_add((r.v[{vi(p[2])}] >> {i*width}) as u32) as u128'
   elif base=='ushr':val=f'((r.v[{vi(p[1])}] >> {i*width}) & {hex((1<<width)-1)}).wrapping_shr({num(p[2])})'
   elif base=='shl':val=f'(((r.v[{vi(p[1])}] >> {i*width}) as u32).wrapping_shl({num(p[2])})) as u128'
   elif base in ('cmlt',):val=f'if ((r.v[{vi(p[1])}] >> {i*width}) as u32 as i32)<0 {{0xffffffffu128}} else {{0u128}}'
   elif base=='fcvtzs':val=f'({a} as i32 as u32) as u128'
   elif base=='scvtf':val=f'(((r.v[{vi(p[1])}] >> {i*width}) as u32 as i32) as f32).to_bits() as u128'
   elif base in ('fcmgt','fcmge','fcmlt','fcmeq'):val=f'if {a} { {"fcmgt":">","fcmge":">=","fcmlt":"<","fcmeq":"=="}[base]} '+('0.0' if p[2].startswith('#') else b)+f' {{{hex((1<<width)-1)}u128}} else {{0u128}}'
   else:
    val={'fmul':lambda:f'{a}*{b}','fadd':lambda:f'{a}+{b}','fsub':lambda:f'{a}-{b}','fdiv':lambda:f'{a}/{b}','fmla':lambda:f'{a}.mul_add({b},{old})','fmls':lambda:f'(-{a}).mul_add({b},{old})','frecpe':lambda:f'reciprocal_estimate({a})','frecps':lambda:f'(-{a}).mul_add({b},2.0)','fmin':lambda:f'{a}.min({b})','fmax':lambda:f'{a}.max({b})','fminnm':lambda:f'{a}.min({b})','frsqrte':lambda:f'rsqrt_estimate({a})','frsqrts':lambda:f'(-{a}).mul_add({b},3.0)*0.5','fabs':lambda:f'{a}.abs()'}[base]()
    val=f'({val}).to_bits() as u128'
   out.append(f'let a{i}:u128={val};')
  return '{'+''.join(out)+vset(dest,'|'.join(f'(a{i} << {i*width})' for i in range(count)))+'}'
 raise Exception(op+' '+arg)

lines=['//! BassInvader numeric source translated from recovered ARM64 data flow.','//! Rebuild with tests/fixtures/bassinvader_translate.py. No target instructions execute.','#![allow(unused_parens, unused_variables, unused_assignments, unused_mut, dead_code, unused_mut, unused_imports, unreachable_code, clippy::all)]','use super::{Memory, Registers, fs, fd, lane, reciprocal_estimate, round_odd, rsqrt_estimate, exp10};']
for start,instructions in functions.items():
 targets={start}
 for pc,op,arg in instructions:
  p=parts(arg)
  if op=='b' or op.startswith('b.') or op in ('cbz','cbnz','tbz','tbnz'):
   target=int(p[-1],0)
   if start<=target<=instructions[-1][0]:targets.add(target)
   if op!='b':targets.add(pc+4)
 blocks={};block=None
 for ins in instructions:
  if ins[0] in targets:block=ins[0];blocks[block]=[]
  blocks[block].append(ins)
 lines.append(f'pub(super) fn {FUNCS[start]}(m:&mut Memory, r:&mut Registers) {{ let mut pc={start}u64; loop {{match pc {{')
 for at,block in blocks.items():
  lines.append(f'{at} => {{')
  for pc,op,arg in block:
   try:statement=emit(pc,op,arg,start)
   except Exception as e:raise Exception(f'{hex(pc)} {op} {arg}') from e
   lines.append(statement)
  pc,op,arg=block[-1]
  if op not in ('ret','br','b'):lines.append(f'pc={pc+4};')
  lines.append('}')
 lines.append('_=>panic!("invalid recovered control flow {}",pc),}}}')
lines.append('pub(super) const CONSTANT_PAGES:&[u64]=&['+','.join(hex(p) for p in pages)+'];')
(OUT/'bassinvader_numeric.rs').write_text('\n'.join(lines)+'\n')
print('functions',len(functions),'instructions',sum(map(len,functions.values())),'constant pages',len(pages))

subprocess.run(['rustfmt','--edition','2021',str(OUT/'bassinvader_numeric.rs')],check=True)

import json
provenance={'binary_sha256':EXPECTED,'method':'Decoded original ARM64 operations emitted as Rust scalar/SIMD arithmetic and control flow; no executable bytes shipped','functions':{hex(a):{'name':FUNCS[a],'instructions':len(ins)}for a,ins in functions.items()},'instructions':sum(map(len,functions.values())),'constant_pages':[hex(p)for p in pages],'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'numeric_source_sha256':hashlib.sha256((OUT/'bassinvader_numeric.rs').read_bytes()).hexdigest(),'constants_sha256':hashlib.sha256((OUT/'bassinvader_constants.bin').read_bytes()).hexdigest(),'global_data_sha256':hashlib.sha256((OUT/'bassinvader_globals.bin').read_bytes()).hexdigest(),'constructor_omissions':['C++ std::function registration is represented by owned Rust callback records','Smoothing vectors use preallocated contiguous Rust state instead of native allocations','Stack canary uses an inert owned slot']}
(Path(__file__).parent/'bassinvader-numeric-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
