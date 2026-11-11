#!/usr/bin/env python3
"""Isolated original BassInvader numerical constructor/rate/DRU circuit oracle.
Private SHA-bound image; RX text and RW private data. A branch bypasses C++
std::function registration, while its10 numeric callbacks are called explicitly.
Only libc numerical/memory imports are bound; no app/plugin entry point runs.
"""
from pathlib import Path
import ctypes,hashlib,json,math,mmap,platform,struct
ROOT=Path(__file__).resolve().parents[5];BINARY=ROOT/'Kontakt-8-REA/Kontakt-8.arm64';FOLDER=Path(__file__).parent;OUT=FOLDER.parents[1]/'src'
DIGEST='42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3'
CALLBACKS=[0x19c2228,0x19c2420,0x19c25a4,0x19c2728,0x19c2a84,0x19c2b64,0x19c2998,0x19c28ac,0x19c2ca4,0x19c2d84]
class Oracle:
 def __init__(self):
  assert platform.system()=='Darwin' and platform.machine()=='arm64';assert hashlib.sha256(BINARY.read_bytes()).hexdigest()==DIGEST
  self.file=BINARY.open('rb');self.image=mmap.mmap(self.file.fileno(),0,flags=mmap.MAP_PRIVATE,prot=mmap.PROT_READ|mmap.PROT_WRITE);self.base=ctypes.addressof(ctypes.c_ubyte.from_buffer(self.image));self.libc=ctypes.CDLL(None,use_errno=True)
  guard=ctypes.c_uint64(0x123456789abcd);self.keep=[guard];struct.pack_into('<Q',self.image,0x9636c10,ctypes.addressof(guard))
  for at,symbol in [(0x9637bb0,'posix_memalign'),(0x9637bc0,'powf'),(0x9636b98,'__exp10f')]:struct.pack_into('<Q',self.image,at,ctypes.cast(getattr(self.libc,symbol),ctypes.c_void_p).value)
  struct.pack_into('<I',self.image,0x19bec94,0x14000000|((0x19bef68-0x19bec94)//4))
  assert not self.libc.mprotect(ctypes.c_void_p(self.base),ctypes.c_size_t(0x434c000),mmap.PROT_READ|mmap.PROT_EXEC)
  self.image[0x9db5000:0x9db6000]=bytes(4096)
  self.fn(0x19c6568)();self.fn(0x19c6754)();(OUT/'bassinvader_globals.bin').write_bytes(self.image[0x9db5000:0x9db6000])
 def fn(self,a,*args):return ctypes.CFUNCTYPE(None,*args)(self.base+a)
 def ptr(self,s):return ctypes.addressof(ctypes.c_ubyte.from_buffer(s))
 def callback(self,s,i):
  obj=(ctypes.c_uint64*2)(0,self.ptr(s));self.fn(CALLBACKS[i],ctypes.c_void_p)(ctypes.addressof(obj))
 def construct(self):
  s=bytearray(0x2000);self.fn(0x19bdc84,ctypes.c_void_p)(self.ptr(s));return s
 def prepare(self,rate,values):
  s=self.construct();p=self.ptr(s);struct.pack_into('<f',s,0x530,rate);self.fn(0x19c04a4,ctypes.c_float,ctypes.c_void_p)(rate,p)
  for i,v in enumerate(values):struct.pack_into('<f',s,0x4c+i*4,v);self.callback(s,i)
  self.fn(0x19c0a50,ctypes.c_void_p)(p);return s
 def smoothing(self,s,values,rate):
  p=self.ptr(s)
  for i,v in enumerate(values):
   for array in range(6):
    pointer=struct.unpack_from('<Q',s,0x200+array*24)[0];ctypes.c_float.from_address(pointer+i*4).value=0. if array in (1,5) else v
   vt=(ctypes.c_void_p*7)();vt[6]=self.base+CALLBACKS[i];obj=(ctypes.c_void_p*2)(ctypes.addressof(vt),p);self.keep.extend([vt,obj]);struct.pack_into('<Q',s,0x90+i*32,ctypes.addressof(obj))
  count=max(1,int((rate*struct.unpack('<f',struct.pack('<f',.075))[0])/32+.5));struct.pack_into('<I',s,0x29c,count)
  for i in range(4):struct.pack_into('<f',s,0x2a0+i*4,1./(count*32))
 def targets(self,s,values):
  pointer=struct.unpack_from('<Q',s,0x248)[0]
  for i,v in enumerate(values):ctypes.c_float.from_address(pointer+i*4).value=v
  s[0x294]=1;struct.pack_into('<I',s,0x290,1)
 def control(self,s):
  start=False
  if s[0x294]:s[0x294]=0;start=True
  elif struct.unpack_from('<I',s,0x290)[0]:
   remaining=(struct.unpack_from('<I',s,0x298)[0]-1)&0xffffffff;struct.pack_into('<I',s,0x298,remaining)
   if remaining==0:
    pointers=[struct.unpack_from('<Q',s,0x200+i*24)[0] for i in range(6)]
    ctypes.memmove(pointers[0],pointers[2],48);ctypes.memmove(pointers[4],pointers[2],48);ctypes.memset(pointers[1],0,48);ctypes.memset(pointers[5],0,48);struct.pack_into('<I',s,0x290,4);s[0x295]=0;start=True
   elif struct.unpack_from('<I',s,0x290)[0]==2:struct.pack_into('<I',s,0x290,3)
   elif struct.unpack_from('<I',s,0x290)[0]==4:struct.pack_into('<I',s,0x290,0)
  if start:self.fn(0x4ffc1c,ctypes.c_void_p)(self.ptr(s)+0x200)
 def render(self,s,frames,right=None):
  a=(ctypes.c_float*len(frames))(*frames);b=(ctypes.c_float*len(frames))(*(frames if right is None else right));o=(ctypes.c_float*len(frames))();p=(ctypes.c_void_p*2)(ctypes.addressof(a),ctypes.addressof(b));q=(ctypes.c_void_p*2)(ctypes.addressof(o),ctypes.addressof(o));self.fn(0x19bf044,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint)(self.ptr(s),ctypes.addressof(p),ctypes.addressof(q),len(frames));return bytes(o)
 def sanitize(self,s):
  s=bytearray(s);s[:16]=bytes(16)
  for a in range(0x200,0x290,8):struct.pack_into('<Q',s,a,0)
  for i in range(10):struct.pack_into('<Q',s,0x90+i*32,0)
  return s

def main():
 n=Oracle();init=n.construct();(FOLDER/'bassinvader-construct-native.bin').write_bytes(n.sanitize(init));records=[];blob=bytearray()
 for rate in [44100.,48000.,64000.,88200.,96000.]:
  for values in [[.5,0.,0.,0.,.5,.5,.5,.5,.5,.5],[.23,1.,1.,1.,.17,.31,.79,.91,.67,.83],[0.]*10,[1.]*10]:
   s=n.prepare(rate,values);initial=n.sanitize(s);frames=[math.sin(i*.071)*.2+math.sin(i*.013)*.09 for i in range(512)];data=struct.pack('<512f',*frames);out=n.render(s,frames)
   for chunk in [initial,data,out,n.sanitize(s)]:blob.extend(chunk)
   records.append({'rate':rate,'values':values,'frames':len(frames)})
 (FOLDER/'bassinvader-native.bin').write_bytes(blob);(FOLDER/'bassinvader-native.json').write_text(json.dumps({'binary_sha256':DIGEST,'cases':records,'sha256':hashlib.sha256(blob).hexdigest(),'method':__doc__,'limitations':['Core numerical behavior only; constructor C++ callback registration bypassed by branch and explicit numeric callbacks','No wrapper SRC or host rendering in this fixture']},indent=2)+'\n')
 live=bytearray();live_records=[]
 for rate in [48000.,96000.]:
  values=[.5,0.,0.,0.,.5,.5,.5,.5,.5,.5];s=n.prepare(rate,values);n.smoothing(s,values,rate);initial=n.sanitize(s);frames=[math.sin(i*.071)*.2+math.sin(i*.013)*.09 for i in range(8192)];out=bytearray()
  for frame in range(0,8192,32):
   if frame==64:n.targets(s,[.23,1.,1.,1.,.17,.31,.79,.91,.67,.83])
   if frame==128:n.targets(s,[.91,0.,0.,0.,.79,.67,.37,.17,.31,.23])
   n.control(s);out.extend(n.render(s,frames[frame:frame+32]))
  for chunk in [initial,struct.pack('<8192f',*frames),out,n.sanitize(s)]:live.extend(chunk)
  live_records.append({'rate':rate,'frames':8192,'events':[64,128]})
 (FOLDER/'bassinvader-live-native.bin').write_bytes(live);(FOLDER/'bassinvader-live-native.json').write_text(json.dumps({'binary_sha256':DIGEST,'cases':live_records,'sha256':hashlib.sha256(live).hexdigest(),'method':'Original core ProcessInt, parameter callbacks and startSmooth; wrapper32-frame countdown reproduced; no host/SRC validation'},indent=2)+'\n')
 provenance={'binary_sha256':DIGEST,'method':__doc__,'global_initializers':['0x1019c6568','0x1019c6754'],'bss_zeroed':['0x109db5000..0x109db6000'],'constructor_branch':{'at':'0x1019bec94','target':'0x1019bef68','purpose':'Bypass C++ std::function registration; original constructor numeric prelude and epilogue execute'},'functions':['0x1019bdc84','0x1019bf044','0x1019c04a4','0x1019c0a50','0x1004ffc1c']+[hex(0x100000000+a)for a in CALLBACKS],'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'global_data_sha256':hashlib.sha256((OUT/'bassinvader_globals.bin').read_bytes()).hexdigest(),'outputs':{name:hashlib.sha256((FOLDER/name).read_bytes()).hexdigest()for name in ['bassinvader-construct-native.bin','bassinvader-native.bin','bassinvader-live-native.bin']},'limitations':['Pure isolated core and numerical model initialization; no Kontakt/plugin host execution','Constructor callback object registration replaced with explicit equivalent numerical callback records','Wrapper host control countdown reconstructed around original native startSmooth and process calls']}
 (FOLDER/'bassinvader-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
 print('Generated',len(records),'native BassInvader cases +',len(live_records),'live cases')
if __name__=='__main__':main()
