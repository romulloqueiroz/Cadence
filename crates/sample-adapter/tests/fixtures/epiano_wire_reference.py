#!/usr/bin/env python3
"""Observe original BEffectEPPreamps::SetEffectParameters parameter dispatch."""
import ctypes
import hashlib
import json
import struct
from pathlib import Path
from epiano_native_reference import NativeImage, DIGEST


def main():
    native = NativeImage()
    engine = ctypes.create_string_buffer(0x11c18)
    tempo = ctypes.create_string_buffer(16)
    struct.pack_into("<f", tempo, 8, 120.)
    struct.pack_into("<Q", engine, 0x11c10, ctypes.addressof(tempo))
    received = {}
    callback = ctypes.CFUNCTYPE(None, ctypes.c_float, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint)(
        lambda value, owner, index, unused: received.__setitem__(index, value))
    vtable = (ctypes.c_void_p * 34)()
    vtable[33] = ctypes.cast(callback, ctypes.c_void_p).value
    owner = (ctypes.c_void_p * 1)(ctypes.addressof(vtable))
    setter = native.function(0x1005f1b50, args=(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p))
    result = bytearray(b"EPWIRE01" + struct.pack("<I", 20))
    for case in range(20):
        values = [.1, .2, .3, .4, .5, .6, .7, .8, [0., .25, .5, 1., 4.][case % 5], .731, .913]
        flags = [bool((case >> i) & 1) for i in range(6)]
        modes = [case % 5, case % 6, case % 7, case % 5]
        payload = ctypes.create_string_buffer(0x78)
        for offset, value in zip([8, 12, 16, 20, 28, 32, 36, 40, 92, 100, 104], values):
            struct.pack_into("<f", payload, offset, value)
        struct.pack_into("<f", payload, 44, .5)
        for offset, value in zip([81, 48, 24, 25, 26, 27], flags):
            payload[offset] = bytes([value])
        payload[80] = bytes([values[8] > 1e-9])
        payload[96] = b"\1"  # BNoteValue's clamp-to-one setting.
        struct.pack_into("<4i", payload, 52, *modes)
        struct.pack_into("<2f", payload, 108, .01, 20.)
        setter(ctypes.addressof(owner), ctypes.addressof(payload), ctypes.addressof(engine))
        expected = [received.get(i, 0.) for i in range(23)]
        # Internal defaults not sent by BEffectEPPreamps.
        for index, value in [(12, 1.), (13, .5), (21, .5)]:
            if index not in received:
                expected[index] = value
        result.extend(struct.pack("<11f6B4i23f", *values, *flags, *modes, *expected))
    folder = Path(__file__).resolve().parent
    (folder / "epiano-wire-native.bin").write_bytes(result)
    (folder / "epiano-wire-native.json").write_text(json.dumps({"binary_sha256": DIGEST,
        "function": "0x1005f1b50", "tempo": 120, "cases": 20,
        "sha256": hashlib.sha256(result).hexdigest(), "method": __doc__,
        "limitations": ["Parameter dispatch at120BPM, with explicit payload defaults and numerical callback; not native instrument loading"]}, indent=2) + "\n")


if __name__ == "__main__":
    main()
