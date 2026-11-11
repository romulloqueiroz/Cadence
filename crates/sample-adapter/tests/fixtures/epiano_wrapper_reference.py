#!/usr/bin/env python3
"""Original EP core plus original generic rate-conversion wrapper audio oracle."""
import ctypes
import hashlib
import json
import math
import struct
from pathlib import Path
from epiano_native_reference import NativeImage, NativeEP, DIGEST


def main():
    cases, frames, block = 16, 1024, 32
    blob = bytearray(b"EPWRAP01" + struct.pack("<II", cases, frames))
    for case in range(cases):
        native = NativeImage()
        core = NativeEP(native)
        rate = [44100., 48000., 96000., 192000.][case % 4]
        native.function(0x10051d3f4, args=(ctypes.c_float, ctypes.c_void_p, ctypes.c_int))(rate, core.pointer, block)
        values = [.23, .71, .38, .61, .84, .29, .19, .67, 0., .731, .913]
        flags = [False, bool((case // 4) % 2), bool(case % 3), False, True, False]
        modes = [case % 5, case % 6, case % 7, case % 5]
        setter = native.function(0x1019d751c, args=(ctypes.c_float, ctypes.c_void_p, ctypes.c_uint))
        callback = ctypes.CFUNCTYPE(None, ctypes.c_float, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint)(
            lambda value, owner, index, unused: setter(value, core.pointer, index))
        table = (ctypes.c_void_p * 34)()
        table[33] = ctypes.cast(callback, ctypes.c_void_p).value
        owner = (ctypes.c_void_p * 1)(ctypes.addressof(table))
        payload = ctypes.create_string_buffer(0x78)
        for offset, value in zip([8, 12, 16, 20, 28, 32, 36, 40, 92, 100, 104], values):
            struct.pack_into("<f", payload, offset, value)
        struct.pack_into("<f", payload, 44, .5)
        for offset, value in zip([81, 48, 24, 25, 26, 27], flags):
            payload[offset] = bytes([value])
        payload[96] = b"\1"
        struct.pack_into("<4i", payload, 52, *modes)
        struct.pack_into("<2f", payload, 108, .01, 20.)
        engine = ctypes.create_string_buffer(0x11c18)
        tempo = ctypes.create_string_buffer(16)
        struct.pack_into("<f", tempo, 8, 120.)
        struct.pack_into("<Q", engine, 0x11c10, ctypes.addressof(tempo))
        native.function(0x1005f1b50, args=(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p))(
            ctypes.addressof(owner), ctypes.addressof(payload), ctypes.addressof(engine))
        native.function(0x1019e0f98)(core.pointer)
        native.function(0x1019e10a8)(core.pointer)
        blob.extend(struct.pack("<f11f6B4i", rate, *values, *flags, *modes))
        channels = [(ctypes.c_float * block)() for _ in range(4)]
        inputs = (ctypes.c_void_p * 2)(*[ctypes.addressof(c) for c in channels[:2]])
        outputs = (ctypes.c_void_p * 2)(*[ctypes.addressof(c) for c in channels[2:]])
        process = native.function(0x10051d788, args=(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p,
                                                   ctypes.c_uint, ctypes.c_void_p, ctypes.c_uint))
        for start in range(0, frames, block):
            core.state[0x3c0] = b"\0"
            native.function(0x1019d8630)(core.pointer)
            for i in range(block):
                channels[0][i] = .3 if start + i == 0 else math.sin((start + i) * .071) * .17
                channels[1][i] = -.2 if start + i == 0 else math.cos((start + i) * .037) * .13
            process(core.pointer, ctypes.addressof(inputs), ctypes.addressof(outputs), block, None, 0)
            for i in range(block):
                values4 = [channel[i] for channel in channels]
                assert all(math.isfinite(v) for v in values4)
                blob.extend(struct.pack("<4f", *values4))
        print("EP wrapper case", case, "rate", rate, flush=True)
    folder = Path(__file__).resolve().parent
    (folder / "epiano-wrapper-native.bin").write_bytes(blob)
    (folder / "epiano-wrapper-native.json").write_text(json.dumps({"binary_sha256": DIGEST,
        "cases": cases, "frames_per_case": frames, "sha256": hashlib.sha256(blob).hexdigest(),
        "functions": ["0x1005f1b50", "0x10051d3f4", "0x10051d788", "0x1019d918c"],
        "method": __doc__, "limitations": ["Isolated host wrapper at32-frame blocks; excludes Kontakt activation, outer dry/wet/volume and instrument playback"]}, indent=2) + "\n")


if __name__ == "__main__":
    main()
