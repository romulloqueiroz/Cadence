#!/usr/bin/env python3
"""Generate VAN51 coefficient goldens by executing only isolated native helpers.

The selected SHA-256-checked ARM64 image is mapped privately RW, the immutable
ExpTable is populated, and protection is changed to RX before numerical helper
calls. No application entry point, plug-in loader, imports, or native UI run.
Requires macOS ARM64. This is original instruction execution, unlike the saved
Ghidra pseudocode whose SIMD aliases invalidate four feedback expressions.
"""
from pathlib import Path
import ctypes,hashlib,json,math,mmap,platform,struct
ROOT=Path(__file__).resolve().parents[4]
BINARY=ROOT.parent/'Kontakt-8-REA/Kontakt-8.arm64'
EXPECTED='42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3'
RATES=[32000.0,44100.0,48000.0,96000.0,192000.0]
KNOBS=[[0.0]*7,[0.0543478243,0.0543478243,0.5,0.5,0.212807879,0.0543478243,0.1],[1.0]*7,[0.231,0.77,0.27,0.63,0.38,0.41,0.23]]
class NativeHelpers:
 def __init__(self):
  if platform.system()!='Darwin' or platform.machine()!='arm64':raise RuntimeError('macOS ARM64 native coefficient oracle required')
  if hashlib.sha256(BINARY.read_bytes()).hexdigest()!=EXPECTED:raise RuntimeError('Unexpected Kontakt image digest')
  self.file=BINARY.open('rb');self.memory=mmap.mmap(self.file.fileno(),0,flags=mmap.MAP_PRIVATE,prot=mmap.PROT_READ|mmap.PROT_WRITE)
  self.base=ctypes.addressof(ctypes.c_ubyte.from_buffer(self.memory))
  for i in range(2401):
   low=struct.unpack('<f',struct.pack('<f',math.exp2((i-1200)/60)))[0]
   high=struct.unpack('<f',struct.pack('<f',math.exp2((i+1-1200)/60)))[0]
   struct.pack_into('<ff',self.memory,0x9dc2200+i*8,low,high-low)
  libc=ctypes.CDLL(None,use_errno=True)
  if libc.mprotect(ctypes.c_void_p(self.base),ctypes.c_size_t(len(self.memory)),mmap.PROT_READ|mmap.PROT_EXEC):raise OSError(ctypes.get_errno(),'mprotect')
 def polynomial(self,address,state):
  ptr=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
  ctypes.CFUNCTYPE(None,ctypes.c_void_p)(self.base+address-0x100000000)(ptr)
 def rate(self,rate,state):
  ptr=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
  ctypes.CFUNCTYPE(None,ctypes.c_float,ctypes.c_void_p)(self.base+0x5ce690)(rate,ptr)

CONTROL_CASES=[([0.0]*8,[False]*5),([0.5]*8,[False]*5),([1.0]*8,[True]*5),([0.1,0.2,0.3,0.4,0.5,0.6,0.7,0.8],[True,False,True,False,True]),([0.8,0.7,0.6,0.5,0.4,0.3,0.2,0.1],[False,True,False,True,False])]
CONTROL_OFFSETS=list(range(0x945c,0x947c,4))+list(range(0x9494,0x94a8,4))+list(range(0x95d0,0x9640,4))
CONTROL_FLAGS=[0x1c0,0x9490,0x9491,0x9492,0x9493,0x94c0,0x94c1,0x94c2]
def main():
 native=NativeHelpers();polynomial=bytearray();rates=bytearray();controls=bytearray()
 for rate in RATES:
  for knobs in KNOBS:
   state=bytearray(0xa000);struct.pack_into('<f',state,0x9450,rate*4)
   for offset,value in zip([0x9494,0x9498,0x9468,0x946c,0x94a0,0x94a4,0x949c],knobs):struct.pack_into('<f',state,offset,value)
   for address in [0x1005cf4c8,0x1005d3540,0x1005d3d2c]:native.polynomial(address,state)
   polynomial.extend(state[0x9750:0x98d0]);polynomial.extend(state[0x9890:0x98e0])
  state=bytearray(0xa000)
  for offset,bits in [(0x94a8,0x3f800000),(0x94ac,0x3d9511e5),(0x94b0,0x3c4dc613),(0x9488,0x3f000000)]:struct.pack_into('<I',state,offset,bits)
  native.rate(rate,state)
  rates.extend(state[0x96e0:0x9750]);rates.extend(state[0x9a20:0x9b60]);rates.extend(state[0x4b4:0x4c0])
 for values,flags in CONTROL_CASES:
  state=bytearray(0xa000)
  rate=struct.unpack('<f',struct.pack('<f',192000.0))[0]
  inverse=struct.unpack('<f',struct.pack('<f',1.0/rate))[0]
  scale=struct.unpack('<f',struct.pack('<f',inverse*struct.unpack('<f',struct.pack('<f',6.2831855))[0]))[0]
  for offset,value in [(0x9450,rate),(0x9454,inverse),(0x9458,scale)]:struct.pack_into('<f',state,offset,value)
  ptr=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
  signature=ctypes.CFUNCTYPE(None,ctypes.c_void_p,*([ctypes.c_bool]*4),*([ctypes.c_float]*8),ctypes.c_bool)
  signature(native.base+0x5d4fe0)(ptr,*flags[:4],*values,flags[4])
  for offset in CONTROL_OFFSETS:controls.extend(state[offset:offset+4])
  controls.extend(bytes(state[o] for o in CONTROL_FLAGS))
 folder=Path(__file__).parent
 (folder/'van51-polynomial.bin').write_bytes(polynomial)
 (folder/'van51-rate.bin').write_bytes(rates)
 (folder/'van51-controls.bin').write_bytes(controls)
 provenance={'binary_sha256':EXPECTED,'polynomial_helpers':['0x1005cf4c8','0x1005d3540','0x1005d3d2c'],'rate_helper':'0x1005ce690','method':'Isolated original ARM64 helper instruction execution, private mmap RW to RX, ctypes function ABI; no app/plugin initialization. ExpTable reconstructed with rounded f32 bases and rounded-f32 differences.','polynomial_cases':len(RATES)*len(KNOBS),'rate_cases':len(RATES),'control_cases':len(CONTROL_CASES),'controls_sha256':hashlib.sha256(controls).hexdigest(),'polynomial_sha256':hashlib.sha256(polynomial).hexdigest(),'rate_sha256':hashlib.sha256(rates).hexdigest(),'limitations':'Proves these helper coefficient outputs only; not full native audio or host behavior.'}
 (folder/'van51-polynomial-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')
if __name__=='__main__':main()
