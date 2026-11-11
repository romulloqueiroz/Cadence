#!/usr/bin/env python3
"""Original isolated BASSPRO numerical Initialize/Set/Reset/ProcessInt oracle.
Uses CAT oracle's digest-bound private RW->RX map and known libc memory imports.
No application/plugin entry or base constructor runs. Output is circuit evidence.
"""
from cat_native_reference import CatNative,BINARY,EXPECTED,FOLDER
import ctypes,struct,json,hashlib,math,pathlib
class BassNative(CatNative):
 def state(self,rate,values,flags):
  state=bytearray(0x3400);p=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
  noop=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(lambda p:None);vt=(ctypes.c_void_p*40)();vt[6]=self.base+0x5e2a9c;vt[27]=ctypes.cast(noop,ctypes.c_void_p).value
  cvs=[];keepers=[noop,vt]
  struct.pack_into('<Q',state,0,ctypes.addressof(vt))
  for at,addr in [(0x14d0,0x4d4af00),(0x1cf0,0x4d4b700),(0x2510,0x4d4bf00)]:
   getter=ctypes.CFUNCTYPE(ctypes.c_void_p)(lambda addr=addr:self.base+addr);cv=(ctypes.c_void_p*3)();cv[2]=ctypes.cast(getter,ctypes.c_void_p).value;keepers.extend([getter,cv]);struct.pack_into('<Q',state,at,ctypes.addressof(cv))
  for at,bits in [(0x2d30,0x3f800000),(0x2d34,0x3f800000),(0x2d48,0x3f800000),(0x2d58,0x3f800000),(0x2d50,0x3f800000),(0x2d78,0x3f8a4c30),(0x2d7c,0x3f9154e2)]:struct.pack_into('<I',state,at,bits)
  self.fn(0x5e09bc,ctypes.c_float,ctypes.c_void_p)(rate,p)
  ordered=[values[i] for i in [0,5,1,2,4,3,6,7,8,9,10,11,12,13,14,15,16]]
  self.fn(0x5e2fb8,ctypes.c_void_p,*([ctypes.c_bool]*4),*([ctypes.c_float]*17),ctypes.c_bool)(p,flags[3],*flags[:3],*ordered,flags[4])
  self.fn(0x5e2a9c,ctypes.c_void_p)(p)
  struct.pack_into('<ii',state,0x124,2,2)
  for at in [0,0x14d0,0x1cf0,0x2510]:struct.pack_into('<Q',state,at,0)
  return state
 def set(self,state,values,flags):
  p=ctypes.addressof(ctypes.c_ubyte.from_buffer(state));ordered=[values[i] for i in [0,5,1,2,4,3,6,7,8,9,10,11,12,13,14,15,16]]
  self.fn(0x5e2fb8,ctypes.c_void_p,*([ctypes.c_bool]*4),*([ctypes.c_float]*17),ctypes.c_bool)(p,flags[3],*flags[:3],*ordered,flags[4])
 def process(self,state,frames,mono=False):
  p=ctypes.addressof(ctypes.c_ubyte.from_buffer(state));pd=ctypes.create_string_buffer(0xb0000);outs=[]
  l=(ctypes.c_float*len(frames))(*[f[0] for f in frames]);r=(ctypes.c_float*len(frames))(*[f[1] for f in frames])
  for c,ch in enumerate([l,r]):struct.pack_into('<Q',state,0x20+8*c,ctypes.addressof(ch))
  # Settle dirty derived EQ at the native control boundary.
  self.fn(0x5e3620,ctypes.c_void_p)(p)
  self.fn(0x5e3950,ctypes.c_void_p,ctypes.c_void_p,ctypes.c_int,ctypes.c_int)(p,ctypes.addressof(pd),0,len(frames))
  for i in range(len(frames)):outs.extend([ctypes.c_float.from_address(ctypes.addressof(pd)+0x9c010+c*0x1000+i*4).value for c in range(2)])
  return struct.pack('<'+'f'*len(outs),*outs)
def main():
 native=BassNative();b=BINARY.read_bytes();(FOLDER.parents[1]/'src/basspro_curves.bin').write_bytes(b[0x4d4af00:0x4d4c700])
 state=native.state(48000.,[.5]*17,[False]*5)
 for at in [0x3120,0x3128,0x3130]:struct.pack_into('<Q',state,at,0)
 (FOLDER.parents[1]/'src/basspro_initial.bin').write_bytes(state[0x2d30:0x33e0])
 cases=[];controls=bytearray()
 for rate in [32000.,44100.,48000.,96000.,192000.]:
  for values,flags in [([0.]*17,[False]*5),([.5]*17,[False]*5),([1.]*17,[True]*5),([.13,.78,.23,.91,.66,.37,.49,.22,.88,.17,.81,.31,.72,.36,.68,.44,.57],[True,False,True,True,False])]:
   state=native.state(rate,values,flags)
   for at in [0x3120,0x3128,0x3130]:struct.pack_into('<Q',state,at,0)
   controls.extend(state[0x2d30:0x33e0]);cases.append({'rate':rate,'values':values,'flags':flags})
 (FOLDER/'basspro-controls.bin').write_bytes(controls)
 frames=[(math.sin(i*.071)*.31,math.cos(i*.043)*.23) for i in range(1024)];(FOLDER/'basspro-input.bin').write_bytes(b''.join(struct.pack('<ff',*f) for f in frames))
 outputs=bytearray()
 for rate in [44100.,48000.,96000.]:
  for flags in [[False]*5,[True]*5,[True,False,True,True,False]]:
   for values in [[.5]*17,[0.]*17,[1.]*17,[.13,.78,.23,.91,.66,.37,.49,.22,.88,.17,.81,.31,.72,.36,.68,.44,.57]]:
    outputs.extend(native.process(native.state(rate,values,flags),frames))
 (FOLDER/'basspro-process.bin').write_bytes(outputs)
 state=native.state(48000.,[.5]*17,[False]*5);transition=bytearray();stages=[([.5]*17,[False]*5),([1.]*17,[True]*5),([.13,.78,.23,.91,.66,.37,.49,.22,.88,.17,.81,.31,.72,.36,.68,.44,.57],[True,False,True,True,False]),([0.]*17,[False]*5)]
 for stage,(values,flags) in enumerate(stages):
  if stage:native.set(state,values,flags)
  for start in range(0,1024,32):transition.extend(native.process(state,frames[start:start+32]))
 (FOLDER/'basspro-transition.bin').write_bytes(transition)
 (FOLDER/'basspro-native.json').write_text(json.dumps({'binary_sha256':EXPECTED,'functions':['0x1005e09bc','0x1005e2fb8','0x1005e2a9c','0x1005e3620','0x1005e3950'],'cases':cases,'frames_per_process_case':1024,'process_rates':[44100,48000,96000],'process_flags':[[False]*5,[True]*5,[True,False,True,True,False]],'process_values':[[.5]*17,[0.]*17,[1.]*17,[.13,.78,.23,.91,.66,.37,.49,.22,.88,.17,.81,.31,.72,.36,.68,.44,.57]],'process_cases':36,'transition_frames':4096,'runtime_initial_data_range':'native state +0x2d30..+0x33e0, sanitized table pointers; recovered scalar/filter defaults, no machine code','generator_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest(),'method':__doc__,'limitations':['Isolated numerical circuit only; no full host or base volume/bypass parity.'],'outputs_sha256':hashlib.sha256(outputs).hexdigest()},indent=2)+'\n')
if __name__=='__main__':main()
