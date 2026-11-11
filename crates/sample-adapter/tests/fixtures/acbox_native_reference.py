#!/usr/bin/env python3
"""Independent isolated original ARM64 ACBox numerical oracle; no app entry point.
A private digest checked image is RW populated then RX. Curve getter and empty
base reset callbacks replace host lifecycle; libc memory imports are bound.
"""
import ctypes,hashlib,json,math,mmap,platform,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[5]
BINARY=ROOT/'Kontakt-8-REA/Kontakt-8.arm64'
DIGEST='42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3'
FOLDER=Path(__file__).resolve().parent
CURVES=[0x4d44f00,0x4d45f00,0x4d43f00,0x4d44700,0x4d45700]
class Oracle:
 def __init__(self):
  assert platform.system()=='Darwin' and platform.machine()=='arm64'
  assert hashlib.sha256(BINARY.read_bytes()).hexdigest()==DIGEST
  self.file=BINARY.open('rb'); self.image=mmap.mmap(self.file.fileno(),0,flags=mmap.MAP_PRIVATE,prot=mmap.PROT_READ|mmap.PROT_WRITE)
  self.base=ctypes.addressof(ctypes.c_ubyte.from_buffer(self.image));libc=ctypes.CDLL(None,use_errno=True)
  for i in range(2401):
   low=struct.unpack('<f',struct.pack('<f',math.exp2((i-1200)/60)))[0]; high=struct.unpack('<f',struct.pack('<f',math.exp2((i+1-1200)/60)))[0]
   struct.pack_into('<ff',self.image,0x9dc2200+i*8,low,high-low)
  for offset,symbol in [(0x9636d90,'bzero'),(0x9637988,'memcpy')]:struct.pack_into('<Q',self.image,offset,ctypes.cast(getattr(libc,symbol),ctypes.c_void_p).value)
  assert not libc.mprotect(ctypes.c_void_p(self.base),ctypes.c_size_t(len(self.image)),mmap.PROT_READ|mmap.PROT_EXEC)
  self.keep=[]
 def fn(self,offset,*args):return ctypes.CFUNCTYPE(None,*args)(self.base+offset)
 def ptr(self,state):return ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
 def initialize(self,rate=48000.,values=[.5]*7,mono=False):
  s=bytearray(0x4200);p=self.ptr(s);noop=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(lambda p:None);vt=(ctypes.c_void_p*40)();vt[6]=self.base+0x5d8494;vt[27]=ctypes.cast(noop,ctypes.c_void_p).value;self.keep += [noop,vt];struct.pack_into('<Q',s,0,ctypes.addressof(vt))
  for i,curve in enumerate(CURVES):
   v=(ctypes.c_void_p*3)();fn=ctypes.CFUNCTYPE(ctypes.c_void_p)(lambda curve=curve:self.base+curve);v[2]=ctypes.cast(fn,ctypes.c_void_p).value;self.keep += [v,fn];struct.pack_into('<Q',s,0x15d0+i*0x820,ctypes.addressof(v))
  for off,bits in [(0x3e98,0x3f800000),(0x3e9c,0x39df1eea),(0x3e90,0x40000000)]:struct.pack_into('<I',s,off,bits)
  self.fn(0x5d77c4,ctypes.c_float,ctypes.c_void_p)(rate,p)
  self.set(s,values,mono);self.fn(0x5d8494,ctypes.c_void_p)(p)
  struct.pack_into('<ii',s,0x124,2,2)
  return s
 def set(self,s,values,mono=False):self.fn(0x5d85d8,*([ctypes.c_float]*7),ctypes.c_void_p,ctypes.c_bool)(*values,self.ptr(s),mono)
 def sanitize(self,s):
  s=bytearray(s)
  for off in [0]+[0x15d0+i*0x820 for i in range(5)]+list(range(0x3fe0,0x4008,8)):struct.pack_into('<Q',s,off,0)
  s[0x20:0x30]=bytes(16)
  return s
 def render(self,s,data):
  frames=len(data)//8;inputs=[(ctypes.c_float*32)() for _ in range(2)]
  for c in range(2):struct.pack_into('<Q',s,0x20+c*8,ctypes.addressof(inputs[c]))
  pd=ctypes.create_string_buffer(0xb0000);out=bytearray();ptr=self.ptr(s);process=self.fn(0x5d8b0c,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_int,ctypes.c_ulong)
  # Process wrapper's 32-host-frame smooth clock, expressed independently.
  for frame in range(0,frames,32):
   if s[0x3f70]:
    s[0x3f70]=0;struct.pack_into('<i',s,0x3f80,2)
    self.fn(0x6164a8,ctypes.c_void_p)(ptr+0x3ea0)
   else:
    n=struct.unpack_from('<i',s,0x3f74)[0]-1;struct.pack_into('<i',s,0x3f74,n)
    if n==0:s[0x3eb0:0x3ee8]=s[0x3f30:0x3f68];s[0x3ef0:0x3f28]=bytes(56);s[0x3f71]=0;struct.pack_into('<i',s,0x3f80,4)
    elif struct.unpack_from('<i',s,0x3f80)[0]==2:struct.pack_into('<i',s,0x3f80,3)
    elif struct.unpack_from('<i',s,0x3f80)[0]==4:struct.pack_into('<i',s,0x3f80,0)
   count=min(32,frames-frame)
   for i in range(count):
    for c in range(2):inputs[c][i]=struct.unpack_from('<f',data,(frame+i)*8+c*4)[0]
   process(ptr,ctypes.addressof(pd),0,count)
   for i in range(count):
    for c in range(2):out.extend(ctypes.string_at(ctypes.addressof(pd)+0x9c010+c*0x1000+i*4,4))
  return out

def main():
 n=Oracle();raw=BINARY.read_bytes();(FOLDER.parents[1]/'src/acbox_curves.bin').write_bytes(b''.join(raw[a:a+2048] for a in CURVES))
 init=n.initialize();(FOLDER/'acbox-initialize.bin').write_bytes(n.sanitize(init))
 # Supply/FIR/static constants generated from native initialization; no code bytes.
 constants=[]
 for off in range(0x3e70,0x4180,4):
  if off in range(0x3fe0,0x4008):continue
  bits=struct.unpack_from('<I',init,off)[0]
  if bits:constants.append((off,bits))
 (FOLDER.parents[1]/'src/acbox_constants.rs').write_text('// Original isolated Initialize numerical constants at 48 kHz; pointers excluded.\npub(super) const INITIAL: &[(usize,u32)] = &[\n'+''.join(f'    (0x{a:x}, 0x{v:08x}),\n' for a,v in constants)+'];\n')
 records=[];allbytes=bytearray()
 for rate in [32000.,44100.,48000.,96000.,192000.]:
  for values,mono in [([.5]*7,False),([.1,.2,.3,.4,.5,.6,.7],False),([1.]*7,True),([0.]*7,False)]:
   s=n.initialize(rate,values,mono);initial=n.sanitize(s);data=b''.join(struct.pack('<ff',math.sin(i*.071)*.2,math.sin(i*.037)*.13) for i in range(1024));output=n.render(s,data)
   for blob in [initial,data,output,n.sanitize(s)]:allbytes.extend(blob)
   records.append({'rate':rate,'values':values,'mono':mono,'frames':1024})
 (FOLDER/'acbox-native.bin').write_bytes(allbytes)
 (FOLDER/'acbox-native.json').write_text(json.dumps({'binary_sha256':DIGEST,'functions':['0x1005d77c4','0x1005d7dcc','0x1005d8494','0x1005d85d8','0x1005d8b0c'],'cases':records,'sha256':hashlib.sha256(allbytes).hexdigest(),'limitations':['Isolated numerical routines; curve getter and empty base-reset callbacks; wrapper control clock reproduced, not full Kontakt host parity']},indent=2)+'\n')
 rates=bytearray()
 for rate in [32000.,44100.,48000.,96000.,192000.]:
  s=n.initialize(48000.,[.1,.2,.3,.4,.5,.6,.7]);n.fn(0x5d7dcc,ctypes.c_float,ctypes.c_void_p)(rate,n.ptr(s));rates.extend(n.sanitize(s))
 (FOLDER/'acbox-rate-native.bin').write_bytes(rates)
 live=bytearray();live_records=[]
 for rate in [44100.,48000.,192000.]:
  for mono in [False,True]:
   s=n.initialize(rate,[.2]*7,mono);initial=n.sanitize(s);data=b''.join(struct.pack('<ff',math.sin(i*.071)*.2,math.sin(i*.037)*.13) for i in range(4096));out=bytearray()
   for start,end,values in [(0,64,None),(64,128,[.8,.7,.6,.5,.4,.3,.2]),(128,4096,[.3,.2,.1,.4,.5,.6,.9])]:
    if values is not None:n.set(s,values,mono)
    out.extend(n.render(s,data[start*8:end*8]))
   for blob in [initial,data,out,n.sanitize(s)]:live.extend(blob)
   live_records.append({'rate':rate,'mono':mono,'frames':4096,'events':{'64':[.8,.7,.6,.5,.4,.3,.2],'128':[.3,.2,.1,.4,.5,.6,.9]}})
 (FOLDER/'acbox-live-native.bin').write_bytes(live)
 (FOLDER/'acbox-live-native.json').write_text(json.dumps({'binary_sha256':DIGEST,'cases':live_records,'sha256':hashlib.sha256(live).hexdigest(),'smoothing_helper':'0x1006164a8','limitations':['Isolated ProcessInt and actual smoothing helper, wrapper countdown reproduced; does not establish full host parity']},indent=2)+'\n')
 outputs=['acbox-native.bin','acbox-live-native.bin','acbox-rate-native.bin','acbox-initialize.bin']
 metadata={'binary_sha256':DIGEST,'method':'Original isolated macOS ARM64 numerical functions; digest-checked private RW image changed to RX; ExpTable populated from rounded f32 bases/slopes; libc bzero/memcpy imports bound; curve getter and initially empty base reset callbacks only',
 'functions':{'initialize':'0x1005d77c4','rate':'0x1005d7dcc','reset':'0x1005d8494','setter':'0x1005d85d8','process_int':'0x1005d8b0c','start_smooth':'0x1006164a8'},
 'curve_addresses':[hex(0x100000000+a) for a in CURVES],
 'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
 'outputs':{name:hashlib.sha256((FOLDER/name).read_bytes()).hexdigest() for name in outputs},
 'limitations':['No application/plugin entry point or native host lifecycle executed','Control wrapper countdown reproduced; actual startSmooth and ProcessInt instructions executed','Curve getters return the selected immutable original curves; empty base reset callback applies only to initially zeroed histories']}
 (FOLDER/'acbox-provenance.json').write_text(json.dumps(metadata,indent=2)+'\n')
 print('Generated',len(records),'static and',len(live_records),'live ACBox native cases')

if __name__=='__main__':main()
