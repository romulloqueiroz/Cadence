#!/usr/bin/env python3
"""Compose independent original NI SRC functions and BassInvader neural core.
No app or plugin host entry point runs. Covers wrapper signal path, not a full
Kontakt-host render; frame API downsampling queue latency is recorded separately.
"""
from bassinvader_native_reference import Oracle,DIGEST,FOLDER
from epiano_native_reference import NativeImage
import ctypes,math,struct,json,hashlib
from pathlib import Path
class SRC:
 def __init__(self,ratio):
  self.n=NativeImage();self.s=ctypes.create_string_buffer(256);self.p=ctypes.addressof(self.s);self.ratio=ratio
  self.n.function(0x101ea7aa0,args=(ctypes.c_float,ctypes.c_float,ctypes.c_void_p,ctypes.c_uint,ctypes.c_uint))(48000.,48000.*ratio,self.p,2,2)
  self.convert=self.n.function(0x101ea9770,args=(ctypes.c_void_p,ctypes.c_void_p,ctypes.c_uint,ctypes.c_void_p,ctypes.c_uint,ctypes.c_uint))
 def process(self,inputs):
  if self.ratio==1:return inputs
  count=len(inputs[0]);channels=[(ctypes.c_float*512)()for _ in range(4)]
  for c in range(2):
   for i,v in enumerate(inputs[c]):channels[c][i]=v
  p=(ctypes.c_void_p*2)(*[ctypes.addressof(c)for c in channels[:2]]);q=(ctypes.c_void_p*2)(*[ctypes.addressof(c)for c in channels[2:]])
  whole=struct.unpack_from('<I',self.s,0x18)[0];num,den=struct.unpack_from('<ff',self.s,0x1c);remaining,phase=struct.unpack_from('<if',self.s,0xb4);produced=int(((count-remaining+1)*den-phase)/(num+den*whole))
  self.convert(self.p,ctypes.addressof(p),count,ctypes.addressof(q),produced,2)
  return [[channels[c+2][i]for i in range(produced)]for c in range(2)]
 def close(self):self.n.memory.close();self.n.file.close()
def main():
 n=Oracle();blob=bytearray();cases=[]
 for rate in [44100.,48000.,96000.,192000.]:
  ratio=2**round(math.log2(88200./rate));internal=round(rate*ratio)
  for mono in [False,True]:
   up=SRC(ratio);down=SRC(1/ratio);values=[.5,0.,0.,0.,.5,.5,.5,.5,.5,.5];cores=[n.prepare(internal,values)for _ in range(2)];inputs=[[math.sin(i*.071)*.2 for i in range(1024)],[math.cos(i*.037)*.13 for i in range(1024)]];output=[[],[]]
   for frame in range(0,1024,32):
    data=up.process([channel[frame:frame+32]for channel in inputs]);outputs=[]
    if mono:
     result=list(struct.unpack('<'+str(len(data[0]))+'f',n.render(cores[0],data[0],data[1])));outputs=[result]*2
    else:
     for c in range(2):outputs.append(list(struct.unpack('<'+str(len(data[c]))+'f',n.render(cores[c],data[c]))))
    data=down.process(outputs)
    for c in range(2):output[c].extend(data[c])
   assert all(len(c)==1024 for c in output)
   for data in [inputs,output]:
    for frame in zip(*data):blob.extend(struct.pack('<2f',*frame))
   up.close();down.close();cases.append({'rate':rate,'ratio':ratio,'mono':mono,'frames':1024})
 (FOLDER/'bassinvader-wrapper-native.bin').write_bytes(blob);(FOLDER/'bassinvader-wrapper-native.json').write_text(json.dumps({'binary_sha256':DIGEST,'cases':cases,'sha256':hashlib.sha256(blob).hexdigest(),'method':__doc__},indent=2)+'\n');print('Generated',len(cases),'native SRC/core compositions')
 mode_blob=bytearray();mode_cases=[]
 for rate in [48000.,192000.]:
  ratio=2**round(math.log2(88200./rate));internal=round(rate*ratio);ups=[SRC(ratio)for _ in range(2)];downs=[SRC(1/ratio)for _ in range(2)];values=[.5,0.,0.,0.,.5,.5,.5,.5,.5,.5];cores=[n.prepare(internal,values)for _ in range(2)];inputs=[[math.sin(i*.071)*.2 for i in range(1024)],[math.cos(i*.037)*.13 for i in range(1024)]];output=[[],[]]
  for frame in range(0,1024,32):
   mono=256<=frame<512;produced=[]
   for core in range(1 if mono else 2):
    data=[c[frame:frame+32]for c in inputs] if mono else [inputs[core][frame:frame+32]]*2;data=ups[core].process(data)
    processed=list(struct.unpack('<'+str(len(data[0]))+'f',n.render(cores[core],data[0],data[1])));produced.append(downs[core].process([processed]*2)[0])
   output[0].extend(produced[0]);output[1].extend(produced[0 if mono else 1])
  for data in [inputs,output]:
   for frame in zip(*data):mode_blob.extend(struct.pack('<2f',*frame))
  for src in ups+downs:src.close()
  mode_cases.append({'rate':rate,'frames':1024,'mono_on':256,'mono_off':512,'delay_frames':max(0,int(1/ratio)-1)})
 (FOLDER/'bassinvader-mode-native.bin').write_bytes(mode_blob);(FOLDER/'bassinvader-mode-native.json').write_text(json.dumps({'binary_sha256':DIGEST,'cases':mode_cases,'sha256':hashlib.sha256(mode_blob).hexdigest(),'method':'Original BassInvader core and independent SRC pairs; explicit mode routing preserves inactive right channel history; no host lifecycle or actual SetMonoMode call'},indent=2)+'\n')

 provenance={'binary_sha256':DIGEST,'generator_sha256':hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),'method':__doc__,'core_generator':'bassinvader_native_reference.py','src_generator':'epiano_native_reference.NativeImage','functions':['0x1019bf044','0x101ea7aa0','0x101ea9770'],'src_block_frames':32,'outputs':{name:hashlib.sha256((FOLDER/name).read_bytes()).hexdigest()for name in ['bassinvader-wrapper-native.bin','bassinvader-mode-native.bin']},'inverse_queue_delay':'The sample callback requires q-1 frames for ratio1/q;192k configuration ratio0.5 adds1 frame. Native compositions are32-frame batches; tests compare with this explicit queue latency.','limitations':['Isolated original neural core plus original NI sample-rate-conversion functions; no full Kontakt host capture','Mode routing is explicit using original function calls and independent converter histories; host silence optimization not executed']}
 (FOLDER/'bassinvader-wrapper-provenance.json').write_text(json.dumps(provenance,indent=2)+'\n')

if __name__=='__main__':main()
