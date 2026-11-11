#!/usr/bin/env python3
"""Extract canonical SRC numeric initialization data from its native fixture.

The source fixture records isolated original Initialize/convert runs. This
extractor retains only the nine configuration snapshots for shipping runtime;
no audio captures, code bytes, or executable image are embedded in the product.
"""
from pathlib import Path
import struct,json,hashlib

def main():
 folder=Path(__file__).resolve().parent;source=folder/'amp-resampler-native.bin';data=source.read_bytes();assert data[:8]==b'SRCNAT01';cases=struct.unpack_from('<I',data,8)[0];cursor=12;output=bytearray();index=[]
 for _ in range(cases):
  exponent,size,frames=struct.unpack_from('<iII',data,cursor);cursor+=12;index.append({'exponent':exponent,'offset':len(output),'bytes':size});output.extend(data[cursor:cursor+size]);cursor+=size
  blocks=struct.unpack_from('<I',data,cursor)[0];cursor+=4
  for _ in range(blocks):cursor+=frames*8;count=struct.unpack_from('<I',data,cursor)[0];cursor+=4+count*8
 assert cursor==len(data)
 destination=folder.parents[1]/'src/amp_resampler_initial.bin';destination.write_bytes(output)
 (folder/'amp-resampler-initial.json').write_text(json.dumps({'method':__doc__,'source_fixture_sha256':hashlib.sha256(data).hexdigest(),'snapshot_sha256':hashlib.sha256(output).hexdigest(),'index':index},indent=2)+'\n')
if __name__=='__main__':main()
