#!/usr/bin/env python3
"""Digest-bound isolated CAT Initialize/SetCat/Reset/ProcessInt oracle.
No application, plugin loader or constructor runs. Only libc memory imports and
explicit curve getter/base empty-history reset are bound in a private RW->RX map.
"""
import ctypes,struct,pathlib,json,hashlib,math,mmap,platform
from van51_native_coefficients_reference import BINARY,EXPECTED
FOLDER=pathlib.Path(__file__).resolve().parent
class CatNative:
 def __init__(self):
  assert platform.system()=='Darwin' and platform.machine()=='arm64'
  data=BINARY.read_bytes();assert hashlib.sha256(data).hexdigest()==EXPECTED
  self.image=mmap.mmap(-1,len(data),prot=mmap.PROT_READ|mmap.PROT_WRITE);self.image[:]=data
  self.base=ctypes.addressof(ctypes.c_ubyte.from_buffer(self.image));libc=ctypes.CDLL(None)
  for i in range(2401):
   lo=ctypes.c_float(math.exp2((i-1200)/60)).value;hi=ctypes.c_float(math.exp2((i+1-1200)/60)).value
   struct.pack_into('<ff',self.image,0x9dc2200+i*8,lo,hi-lo)
  for at,name in [(0x9636d90,'bzero'),(0x9637988,'memcpy'),(0x96379a0,'memset_pattern16')]:struct.pack_into('<Q',self.image,at,ctypes.cast(getattr(libc,name),ctypes.c_void_p).value)
  self.guard=ctypes.c_uint64(0x12345678);struct.pack_into('<Q',self.image,0x9636c10,ctypes.addressof(self.guard))
  assert libc.mprotect(ctypes.c_void_p(self.base),ctypes.c_size_t(len(data)),mmap.PROT_READ|mmap.PROT_EXEC)==0
  self.keep=[]
 def fn(self,at,*args):return ctypes.CFUNCTYPE(None,*args)(self.base+at)
 def state(self,rate,values,mono=False):
  state=bytearray(0x1500);p=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
  noop=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(lambda p:None);vt=(ctypes.c_void_p*40)();vt[6]=self.base+0x5e666c;vt[27]=ctypes.cast(noop,ctypes.c_void_p).value
  getter=ctypes.CFUNCTYPE(ctypes.c_void_p)(lambda:self.base+0x4d4c700);cv=(ctypes.c_void_p*3)();cv[2]=ctypes.cast(getter,ctypes.c_void_p).value
  struct.pack_into('<Q',state,0,ctypes.addressof(vt));struct.pack_into('<Q',state,0xc60,ctypes.addressof(cv))
  self.fn(0x5e6140,ctypes.c_float,ctypes.c_void_p)(rate,p)
  self.fn(0x5e66bc,ctypes.c_void_p,*([ctypes.c_float]*7),ctypes.c_bool)(p,*values,mono)
  self.fn(0x5e666c,ctypes.c_void_p)(p)
  struct.pack_into('<ii',state,0x124,2,2)
  struct.pack_into('<Q',state,0,0);struct.pack_into('<Q',state,0xc60,0)
  return state
 def process(self,state,frames,mono=False):
  p=ctypes.addressof(ctypes.c_ubyte.from_buffer(state));pd=ctypes.create_string_buffer(0xb0000);outs=[]
  l=(ctypes.c_float*len(frames))(*[f[0] for f in frames]);r=(ctypes.c_float*len(frames))(*[f[1] for f in frames])
  for c,ch in enumerate([l,r]):struct.pack_into('<Q',state,0x20+8*c,ctypes.addressof(ch))
  self.fn(0x5e6f70,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_int,ctypes.c_int)(p,ctypes.addressof(pd),0,len(frames))
  for i in range(len(frames)):outs.extend([ctypes.c_float.from_address(ctypes.addressof(pd)+0x9c010+c*0x1000+i*4).value for c in range(2)])
  return struct.pack('<'+'f'*len(outs),*outs)
def main():
 native=CatNative();binary=BINARY.read_bytes()
 (FOLDER.parents[1]/'src/cat_curves.bin').write_bytes(binary[0x4d4c700:0x4d4cf00])
 constants=b''.join(binary[off:off+16] for off in [0x4cc8e80,0x4cc8e90,0x4cc8e20,0x4cc8ea0,0x4cc8e10,0x4cc8e00,0x4cc83a0,0x4cc83b0,0x4cc83c0,0x4cc83e0])
 (FOLDER.parents[1]/'src/cat_constants.bin').write_bytes(constants)
 blob=bytearray();cases=[]
 for rate in [32000.,44100.,48000.,96000.,192000.]:
  for values in [[0.]*7,[.5]*7,[1.]*7,[.13,.78,.23,.91,.66,.37,.49]]:
   for mono in [False,True]:
    state=native.state(rate,values,mono);state[0x260:0x268]=bytes(8)
    blob.extend(state[0x1c4:0x240]);blob.extend(state[0x248:0x250]);blob.extend(state[0x270:0x360])
    cases.append({'rate':rate,'values':values,'mono':mono})
 (FOLDER/'cat-controls.bin').write_bytes(blob)
 # Fresh state retains its own table pointer. Settled controls produce a pure ProcessInt golden.
 frames=[(math.sin(i*.071)*.31,math.cos(i*.043)*.23) for i in range(1024)]
 inp=b''.join(struct.pack('<ff',*f) for f in frames);(FOLDER/'cat-input.bin').write_bytes(inp)
 outputs=bytearray()
 for rate in [44100.,48000.,96000.]:
  for mono in [False,True]:
   for values in [[.5]*7,[0.]*7,[1.]*7,[.13,.78,.23,.91,.66,.37,.49]]:outputs.extend(native.process(native.state(rate,values,mono),frames,mono))
 (FOLDER/'cat-process.bin').write_bytes(outputs)
 (FOLDER/'cat-native.json').write_text(json.dumps({'binary_sha256':EXPECTED,'functions':['0x1005e6140','0x1005e66bc','0x1005e666c','0x1005e6f70'],'cases':cases,'generator_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),'curve_address':'0x104d4c700','constants_addresses':['0x104cc8e80','0x104cc8e90','0x104cc8e20','0x104cc8ea0','0x104cc8e10','0x104cc8e00','0x104cc83a0','0x104cc83b0','0x104cc83c0','0x104cc83e0'],'frames_per_process_case':1024,'process_rates':[44100,48000,96000],'process_mono':[False,True],'process_values':[[.5]*7,[0.]*7,[1.]*7,[.13,.78,.23,.91,.66,.37,.49]],'process_cases':24,'method':__doc__,'limitations':['Isolated numerical circuit only; no full host or base volume/bypass parity.'],'outputs_sha256':hashlib.sha256(outputs).hexdigest()},indent=2)+'\n')
if __name__=='__main__':main()
