#!/usr/bin/env python3
"""Run original stereo SRC in minimal integer-ratio input/output batches.

Downsampling gets one ratio-sized batch per output frame; upsampling gets one
input frame per call. This avoids the original DOWN kernel's unsupported path
with a zero-sized output block. Native converter only, no application lifecycle.
"""
import ctypes,hashlib,json,math,struct
from pathlib import Path
from epiano_native_reference import NativeImage,DIGEST

def main():
 folder=Path(__file__).resolve().parent;out=bytearray(b'SRCSTR01'+struct.pack('<I',9));report=[]
 for exponent in range(-4,5):
  native=NativeImage();state=ctypes.create_string_buffer(256);pointer=ctypes.addressof(state)
  native.function(0x101ea7aa0,args=(ctypes.c_float,ctypes.c_float,ctypes.c_void_p,ctypes.c_uint,ctypes.c_uint))(48000.,48000.*2**exponent,pointer,2,2)
  channels=[(ctypes.c_float*32)() for _ in range(4)];inputs=(ctypes.c_void_p*2)(*[ctypes.addressof(c) for c in channels[:2]]);outputs=(ctypes.c_void_p*2)(*[ctypes.addressof(c) for c in channels[2:]])
  convert=native.function(0x101ea9770,args=(ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint,ctypes.c_void_p,ctypes.c_uint,ctypes.c_uint))
  n=max(1,2**(-exponent));blocks=1024//n;out.extend(struct.pack('<iII',exponent,n,blocks));produced=0
  for block in range(blocks):
   for i in range(n):
    frame=block*n+i;channels[0][i]=.75 if frame==0 else math.sin(frame*.071)*.17;channels[1][i]=-.3 if frame==0 else math.cos(frame*.037)*.13;out.extend(struct.pack('<ff',channels[0][i],channels[1][i]))
   quotient=struct.unpack_from('<I',state,0x18)[0];remainder=struct.unpack_from('<f',state,0x1c)[0];divisor=struct.unpack_from('<f',state,0x20)[0];remaining=struct.unpack_from('<i',state,0xb4)[0];phase=struct.unpack_from('<f',state,0xb8)[0]
   count=n if exponent==0 else int(((n-remaining+1)*divisor-phase)/(remainder+divisor*quotient));assert count==max(1,2**exponent),(exponent,block,count)
   convert(pointer,ctypes.addressof(inputs),n,ctypes.addressof(outputs),count,2)
   out.extend(struct.pack('<I',count))
   for i in range(count):out.extend(struct.pack('<ff',channels[2][i],channels[3][i]))
   produced+=count
  report.append({'exponent':exponent,'input_batch_frames':n,'blocks':blocks,'output_frames':produced});print(report[-1],flush=True)
 path=folder/'amp-resampler-stream.bin';path.write_bytes(out);path.with_suffix('.json').write_text(json.dumps({'binary_sha256':DIGEST,'method':__doc__,'cases':report,'sha256':hashlib.sha256(out).hexdigest()},indent=2)+'\n')
if __name__=='__main__':main()
