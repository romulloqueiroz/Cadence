#!/usr/bin/env python3
"""Generate HOTSOLO oracles from isolated SHA-bound ARM64 numerical routines.
Private RW-to-RX image; only libc bzero/memcpy imports and explicit curve
getters are bound. No application, UI, plug-in entry point, or base constructor.
"""
import ctypes,hashlib,json,math,mmap,platform,struct
from pathlib import Path
ROOT=Path(__file__).resolve().parents[4];BINARY=ROOT.parent/'Kontakt-8-REA/Kontakt-8.arm64'
DIGEST='42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3'
CASES=[(r,[v]*8,[boost,mono]) for r in [44100.,48000.,96000.,192000.] for v,boost,mono in [(.0,False,False),(.5,False,False),(1.,True,False),(.27,True,True)]]
FRAMES=384
def main():
 assert platform.system()=='Darwin' and platform.machine()=='arm64'
 assert hashlib.sha256(BINARY.read_bytes()).hexdigest()==DIGEST
 libc=ctypes.CDLL(None,use_errno=True)
 source=BINARY.open('rb');image=mmap.mmap(source.fileno(),0,flags=mmap.MAP_PRIVATE,prot=mmap.PROT_READ|mmap.PROT_WRITE);base=ctypes.addressof(ctypes.c_ubyte.from_buffer(image))
 for i in range(2401):
  low=ctypes.c_float(math.exp2((i-1200)/60)).value;high=ctypes.c_float(math.exp2((i+1-1200)/60)).value
  struct.pack_into('<ff',image,0x9dc2200+i*8,low,high-low)
 for offset,name in [(0x9636d90,'bzero'),(0x9637988,'memcpy')]:struct.pack_into('<Q',image,offset,ctypes.cast(getattr(libc,name),ctypes.c_void_p).value)
 if libc.mprotect(ctypes.c_void_p(base),ctypes.c_size_t(len(image)),mmap.PROT_READ|mmap.PROT_EXEC):raise OSError(ctypes.get_errno(),'mprotect')
 keep=[];noop=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(lambda p:None);keep.append(noop)
 vt=(ctypes.c_void_p*40)();vt[6]=base+0x5dadf0;vt[27]=ctypes.cast(noop,ctypes.c_void_p).value
 table_vts=[]
 for i in range(9):
  v=(ctypes.c_void_p*3)();fn=ctypes.CFUNCTYPE(ctypes.c_void_p)(lambda i=i:base+0x4d46700+i*2048);v[2]=ctypes.cast(fn,ctypes.c_void_p).value;table_vts.append(v);keep.extend([v,fn])
 init=ctypes.CFUNCTYPE(None,ctypes.c_float,ctypes.c_void_p)(base+0x5d9838)
 setter=ctypes.CFUNCTYPE(None,ctypes.c_void_p,ctypes.c_bool,*([ctypes.c_float]*8),ctypes.c_bool)(base+0x5de8e8)
 reset=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(base+0x5dadf0)
 control=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(base+0x5deb88)
 process=ctypes.CFUNCTYPE(None,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_int,ctypes.c_ulong)(base+0x5decbc)
 result=bytearray(b'HSNATV01'+struct.pack('<II',len(CASES),FRAMES))
 def normalize(state):
  state=bytearray(state)
  for at in [0,0x20,0x28]+[0x27d0+i*0x820 for i in range(9)]+list(range(0x77d0,0x7828,8)):struct.pack_into('<Q',state,at,0)
  return state
 for rate,values,flags in CASES:
  state=bytearray(0x8000);ptr=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
  struct.pack_into('<Q',state,0,ctypes.addressof(vt))
  for i,v in enumerate(table_vts):struct.pack_into('<Q',state,0x27d0+i*0x820,ctypes.addressof(v))
  for off,bits in [(0x7144,0x3f800000),(0x7148,0x3d96cbc9),(0x714c,0x3ccdc613)]:struct.pack_into('<I',state,off,bits)
  init(rate,ptr);setter(ptr,flags[0],*[values[i] for i in [0,1,7,6,5,2,3,4]],flags[1]);reset(ptr)
  struct.pack_into('<I',state,0x124,2)
  inputs=[(ctypes.c_float*1)() for _ in range(2)]
  for c in range(2):struct.pack_into('<Q',state,0x20+c*8,ctypes.addressof(inputs[c]))
  data=ctypes.create_string_buffer(0xb0000);data_ptr=ctypes.addressof(data)
  outputs=[ctypes.c_float.from_address(data_ptr+0x9c010+c*0x1000) for c in range(2)]
  result.extend(struct.pack('<f8f2B2x',rate,*values,*flags));result.extend(normalize(state))
  for frame in range(FRAMES):
   if frame==128:
    values=[.78,.24,.63,.48,.9,.17,.37,.61];flags=[not flags[0],flags[1]]
    setter(ptr,flags[0],*[values[i] for i in [0,1,7,6,5,2,3,4]],flags[1])
   if frame%32==0:control(ptr)
   left=ctypes.c_float(math.sin(frame*.071)*.2).value;right=ctypes.c_float(math.cos(frame*.041)*.08).value
   inputs[0][0]=left;inputs[1][0]=right;process(ptr,data_ptr,0,1)
   result.extend(struct.pack('<4f',left,right,outputs[0].value,outputs[0].value if flags[1] else outputs[1].value))
  result.extend(normalize(state))
 folder=Path(__file__).parent;output=folder/'hotsolo-native.bin';output.write_bytes(result)
 report={'binary_sha256':DIGEST,'functions':['0x1005d9838','0x1005de8e8','0x1005dadf0','0x1005deb88','0x1005decbc'],'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'cases':len(CASES),'frames_per_case':FRAMES,'parameter_change_frame':128,'method':__doc__,'snapshot_sha256':hashlib.sha256(result).hexdigest(),'limitations':['Isolated native numerical parity does not establish full Kontakt-host parity.','Base volume anticipation is a no-op on initially zero base state.']}
 output.with_suffix('.json').write_text(json.dumps(report,indent=2)+'\n');print(json.dumps(report))
if __name__=='__main__':main()
