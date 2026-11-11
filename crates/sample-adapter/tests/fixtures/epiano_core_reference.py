#!/usr/bin/env python3
"""Native EP core rate/parameter/suspend/reset/control/audio goldens."""
import ctypes
import hashlib
import json
import math
import struct
from pathlib import Path
from epiano_native_reference import NativeImage, NativeEP, DIGEST, canonical_ep_snapshot


def parameters(case):
    values = [0., .5, 0., .5, 1., .5, .5, 1., 1., 1., 1., 0., 1., .5, 0., 0., 0., 0., 0., 0., 0., .5, 0.]
    values[0], values[2], values[11], values[18] = (case % 5) / 4, (case % 6) / 5, (case % 7) / 6, (case % 5) / 4
    values[1], values[3], values[5], values[6], values[12], values[13], values[20] = [.23, .71, .38, .61, .67, .29, .19]
    values[7:11] = [float((case >> i) & 1) for i in range(4)]
    values[22] = float(case % 2)
    return values


def main():
    folder = Path(__file__).resolve().parent
    cases, frames = 21, 512
    output = bytearray(b"EPCORE01" + struct.pack("<II", cases, frames))
    for case in range(cases):
        native = NativeImage()
        core = NativeEP(native)
        pointer, state = core.pointer, core.state
        rate = [44100., 48000., 88200., 96000., 192000.][case % 5]
        ratio, block = 1., 32
        struct.pack_into("<ffi", state, 0x7b0, rate, ratio, block)
        period = max(1, int(rate * .02 / block + .5))
        struct.pack_into("<I", state, 0x51c, period)
        struct.pack_into("<4f", state, 0x520, *([1. / (period * block)] * 4))
        native.function(0x1019df718, args=(ctypes.c_float, ctypes.c_float, ctypes.c_void_p, ctypes.c_int))(rate, ratio, pointer, block)
        values = parameters(case)
        setter = native.function(0x1019d751c, args=(ctypes.c_float, ctypes.c_void_p, ctypes.c_uint))
        for index, value in enumerate(values):
            setter(value, pointer, index)
        native.function(0x1019e0f98)(pointer)
        native.function(0x1019e10a8)(pointer)
        initial = canonical_ep_snapshot(native, state, pointer)
        output.extend(struct.pack("<3fI23fI", rate, ratio, float(block), case, *values, len(initial)))
        output.extend(initial)
        channels = [(ctypes.c_float * 1)() for _ in range(4)]
        inputs = (ctypes.c_void_p * 2)(*[ctypes.addressof(c) for c in channels[:2]])
        outputs = (ctypes.c_void_p * 2)(*[ctypes.addressof(c) for c in channels[2:]])
        process = native.function(0x1019d918c, args=(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint))
        for frame in range(frames):
            if frame == 128:
                changed = parameters(case + 1)
                for index, value in enumerate(changed):
                    setter(value, pointer, index)
            if frame % block == 0:
                state[0x3c0] = b"\0"
                native.function(0x1019d8630)(pointer)
            channels[0][0] = .3 if frame == 0 else math.sin(frame * .071) * .17
            channels[1][0] = -.2 if frame == 0 else math.cos(frame * .037) * .13
            process(pointer, ctypes.addressof(inputs), ctypes.addressof(outputs), 1)
            values4 = channels[0][0], channels[1][0], channels[2][0], channels[3][0]
            assert all(math.isfinite(v) for v in values4), (case, frame, values4)
            output.extend(struct.pack("<4f", *values4))
        print("EP case", case, "rate", rate, flush=True)
    path = folder / "epiano-core-native.bin"
    path.write_bytes(output)
    path.with_suffix(".json").write_text(json.dumps({"binary_sha256": DIGEST, "cases": cases,
        "frames_per_case": frames, "sha256": hashlib.sha256(output).hexdigest(),
        "method": __doc__, "limitations": ["Internal core rate, without the separate input/output rate converters or Kontakt host"]}, indent=2) + "\n")


if __name__ == "__main__":
    main()
