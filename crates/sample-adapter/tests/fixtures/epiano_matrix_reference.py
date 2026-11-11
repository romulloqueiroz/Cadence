#!/usr/bin/env python3
"""Original isolated Eigen 6x6 LU, triangular-solve and inverse helper goldens.

These dense matrices cover the shifted-NEG byte-stride bug that a diagonal
matrix misses. The fixture records native LU state, inverse, lower solve and
upper solve for each matrix; no application or plug-in entry point runs.
"""
import ctypes
import hashlib
import json
import struct
from pathlib import Path
from epiano_native_reference import NativeImage, DIGEST

native = NativeImage()
output = bytearray()
for shifted in [False, True]:
    matrix = [float(6+i) if ((i+2) % 6 if shifted else i) == j else (i+j+1)*.03
              for i in range(6) for j in range(6)]
    state = ctypes.create_string_buffer(0x180)
    struct.pack_into('<36d', state, 0, *matrix)
    native.function(0x1019e357c)(ctypes.addressof(state))
    output.extend(bytes(state))
    inputs = (ctypes.c_double*36)(*matrix)
    result = (ctypes.c_double*36)()
    native.function(0x1019e3324, args=(ctypes.c_void_p, ctypes.c_void_p))(
        ctypes.addressof(inputs), ctypes.addressof(result))
    output.extend(bytes(result))
    result = (ctypes.c_double*36)(*[1.0 if i == j else 0.0 for i in range(6) for j in range(6)])
    scratch1, scratch2 = ctypes.create_string_buffer(288), ctypes.create_string_buffer(288)
    blocking = (ctypes.c_uint64*5)(ctypes.addressof(scratch1), ctypes.addressof(scratch2), 6, 6, 6)
    for address in [0x1019e40b8, 0x1019e72c4]:
        native.function(address, args=(ctypes.c_uint64,)*8)(
            6, 6, ctypes.addressof(state), 6, ctypes.addressof(result), 1, 6, ctypes.addressof(blocking))
        output.extend(bytes(result))
path = Path(__file__).with_name('epiano-matrix-native.bin')
path.write_bytes(output)
path.with_suffix('.json').write_text(json.dumps({
    'binary_sha256': DIGEST,
    'cases': 2,
    'functions': ['0x1019e357c', '0x1019e3324', '0x1019e40b8', '0x1019e72c4'],
    'method': __doc__,
    'sha256': hashlib.sha256(output).hexdigest(),
    'generator_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
    'limitations': ['Isolated numerical helpers only; not full Kontakt-host lifecycle parity.'],
}, indent=2)+'\n')
