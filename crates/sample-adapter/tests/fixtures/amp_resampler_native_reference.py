#!/usr/bin/env python3
"""Capture scalar state/data and original NI::AB converter outputs.

Only power-of-two stereo conversion configurations are retained. Their canonical
addresses are data indices for reconstructed Rust source, never executable bytes.
"""
import ctypes
import hashlib
import json
import math
import struct
from pathlib import Path
from epiano_native_reference import NativeImage, DIGEST

STATE = 0x10000000000


def canonical_snapshot(native, state, pointer, heap_start=0x1000):
    ranges = [(pointer, len(state), STATE)]
    cursor = heap_start
    allocations = list(native.allocations.items())
    for address, size in allocations:
        ranges.append((address, size, STATE + cursor))
        cursor = (cursor + size + 127) & ~127
    result = bytearray(max(len(state), cursor))
    result[:len(state)] = state.raw
    for address, size, canonical in ranges[1:]:
        result[canonical - STATE:canonical - STATE + size] = ctypes.string_at(address, size)
    raw_result = bytes(result)
    for offset in range(0, len(result) - 7, 8):
        value = struct.unpack_from("<Q", raw_result, offset)[0]
        vector_end = None
        # std::vector's end/capacity must remain relative to its own begin,
        # even when another native allocation is adjacent in the host heap.
        for back in (8, 16):
            if offset >= back:
                begin = struct.unpack_from("<Q", raw_result, offset-back)[0]
                for address, size, canonical in ranges[1:]:
                    if begin == address and value == address + size:
                        vector_end = canonical + size
                        break
        if vector_end is not None:
            value = vector_end
        elif native.base <= value < native.base + len(native.memory):
            value = value - native.base + 0x100000000
        else:
            match = next(((a,z,c) for a,z,c in ranges if a <= value < a+z), None)
            if match is None:
                match = next(((a,z,c) for a,z,c in ranges if value == a+z), None)
            if match is not None:
                address, size, canonical = match
                value = value - address + canonical
        struct.pack_into("<Q", result, offset, value)
    return result


def main():
    folder = Path(__file__).resolve().parent
    output = bytearray(b"SRCNAT01" + struct.pack("<I", 9))
    summary = []
    for exponent in range(-4, 5):
        native = NativeImage()
        state = ctypes.create_string_buffer(256)
        pointer = ctypes.addressof(state)
        factor = 2.0 ** exponent
        native.function(0x101ea7aa0, args=(ctypes.c_float, ctypes.c_float, ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint))(
            48000.0, 48000.0 * factor, pointer, 2, 2)
        initial = canonical_snapshot(native, state, pointer)
        block_frames, blocks = 32, 32
        output.extend(struct.pack("<iII", exponent, len(initial), block_frames))
        output.extend(initial)
        output.extend(struct.pack("<I", blocks))
        channels = [(ctypes.c_float * 512)() for _ in range(4)]
        inputs = (ctypes.c_void_p * 2)(*[ctypes.addressof(c) for c in channels[:2]])
        outputs = (ctypes.c_void_p * 2)(*[ctypes.addressof(c) for c in channels[2:]])
        convert = native.function(0x101ea9770, args=(ctypes.c_void_p, ctypes.c_void_p, ctypes.c_uint,
                                                   ctypes.c_void_p, ctypes.c_uint, ctypes.c_uint))
        produced = 0
        for block in range(blocks):
            for index in range(block_frames):
                frame = block * block_frames + index
                channels[0][index] = 0.75 if frame == 0 else math.sin(frame * 0.071) * 0.17
                channels[1][index] = -0.3 if frame == 0 else math.cos(frame * 0.037) * 0.13
                output.extend(struct.pack("<2f", channels[0][index], channels[1][index]))
            quotient = struct.unpack_from("<I", state, 0x18)[0]
            remainder = struct.unpack_from("<f", state, 0x1c)[0]
            divisor = struct.unpack_from("<f", state, 0x20)[0]
            remaining = struct.unpack_from("<i", state, 0xb4)[0]
            phase = struct.unpack_from("<f", state, 0xb8)[0]
            count = block_frames if exponent == 0 else int(((block_frames - remaining + 1) * divisor - phase) / (remainder + divisor * quotient))
            assert 0 <= count <= 512, (exponent, block, count)
            convert(pointer, ctypes.addressof(inputs), block_frames, ctypes.addressof(outputs), count, 2)
            output.extend(struct.pack("<I", count))
            for index in range(count):
                values = channels[2][index], channels[3][index]
                assert all(math.isfinite(v) for v in values)
                output.extend(struct.pack("<2f", *values))
            produced += count
        frames = block_frames * blocks
        summary.append({"exponent": exponent, "state_bytes": len(initial), "input_frames": frames,
                        "output_frames": produced, "allocations": len(native.allocations)})
        print(summary[-1], flush=True)
    path = folder / "amp-resampler-native.bin"
    path.write_bytes(output)
    path.with_suffix(".json").write_text(json.dumps({"binary_sha256": DIGEST, "method": __doc__,
        "cases": summary, "snapshot_base": hex(STATE), "blob_sha256": hashlib.sha256(output).hexdigest(),
        "functions": ["0x101ea7aa0", "0x101ea9770"],
        "limitations": ["Isolated converter only; no full instrument or host validation"]}, indent=2) + "\n")



if __name__ == "__main__":
    main()
