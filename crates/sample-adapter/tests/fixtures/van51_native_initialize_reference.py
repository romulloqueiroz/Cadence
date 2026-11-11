"""Replay the original Initialize, SetVan51 and Reset numerical routines.

Only the stage curve getters and initially empty base-history reset are stubbed.
The application entry point, base constructor and plugin loader are not run.
The resulting snapshot validates default coefficients and controls, not native
host lifecycle behavior. Requires the digest-checked macOS ARM64 image.
"""
import ctypes,struct,pathlib,json,hashlib
from van51_native_coefficients_reference import NativeHelpers
native=NativeHelpers();state=bytearray(0xa000);p=ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
keep=[]
noop=ctypes.CFUNCTYPE(None,ctypes.c_void_p)(lambda p:None);keep.append(noop)
vt=(ctypes.c_void_p*40)();vt[6]=native.base+0x5cf318;vt[27]=ctypes.cast(noop,ctypes.c_void_p).value
struct.pack_into('<Q',state,0,ctypes.addressof(vt))
for i in range(12):
 v=(ctypes.c_void_p*3)();fn=ctypes.CFUNCTYPE(ctypes.c_void_p)(lambda i=i:native.base+0x4d3df00+i*2048);v[2]=ctypes.cast(fn,ctypes.c_void_p).value;keep.extend([v,fn]);struct.pack_into('<Q',state,0x32d0+i*0x820,ctypes.addressof(v))
for off,bits in [(0x9450,0x3f800000),(0x9454,0x3f800000),(0x94a8,0x3f800000),(0x94ac,0x3d9511e5),(0x94b0,0x3c4dc613)]:struct.pack_into('<I',state,off,bits)
ctypes.CFUNCTYPE(None,ctypes.c_float,ctypes.c_void_p)(native.base+0x5cdc34)(48000.0,p)
ctypes.CFUNCTYPE(None,ctypes.c_void_p,*([ctypes.c_bool]*4),*([ctypes.c_float]*8),ctypes.c_bool)(native.base+0x5d4fe0)(p,False,False,False,False,*([.5]*8),False)
ctypes.CFUNCTYPE(None,ctypes.c_void_p)(native.base+0x5cf318)(p)
struct.pack_into('<ii',state,0x124,2,2)
for at in [0]+[0x32d0+i*0x820 for i in range(12)]:struct.pack_into('<Q',state,at,0)
state[0x9c80:0x9d00]=bytes(128)

folder=pathlib.Path(__file__).resolve().parent
output=folder/'van51-initialize.bin'
output.write_bytes(state)
output.with_suffix('.json').write_text(json.dumps({
 'binary_sha256':'42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3',
 'functions':['0x1005cdc34','0x1005d4fe0','0x1005cf318'],
 'rate':48000,'values':[0.5]*8,'flags':[False]*5,
 'method':'Original isolated ARM64 Initialize, SetVan51, Reset; explicit curve getters and empty base reset; recovered constructor scalar constants',
 'limitations':['Not application/plugin initialization; base history reset is a no-op on the initially zeroed state'],
 'snapshot_sha256':hashlib.sha256(state).hexdigest(),
 'generator_sha256':hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()
},indent=2)+'\n')
print(output)
