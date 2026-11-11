#!/usr/bin/env python3
"""Run the isolated original ARM64 ProcessInt on explicit circuit inputs.

Only bzero/memcpy import slots are bound to the host C library. No Kontakt
application or plugin entry point is loaded. The image remains private and
unchanged on disk. This oracle requires macOS ARM64 and the recorded image.
"""
import argparse
import ctypes
import hashlib
import json
import mmap
import platform
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
BINARY = ROOT / "Kontakt-8-REA/Kontakt-8.arm64"
DIGEST = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    for name in ["state", "input", "output", "state-output"]:
        parser.add_argument("--" + name, type=Path, required=True)
    parser.add_argument("--setup", type=Path, default=Path(__file__).with_name("van51-setup.bin"))
    parser.add_argument("--block-size", type=int, default=1024)
    parser.add_argument("--start-frame", type=int, default=0)
    args = parser.parse_args()
    if platform.system() != "Darwin" or platform.machine() != "arm64":
        parser.error("the native oracle requires macOS ARM64")
    if hashlib.sha256(BINARY.read_bytes()).hexdigest() != DIGEST:
        parser.error("unexpected native image digest")
    state = bytearray(args.state.read_bytes())
    data = args.input.read_bytes()
    if len(state) != 0xa000 or len(data) % 8 or not 1 <= len(data) // 8 <= 1048576:
        parser.error("expected 0xa000-byte state and 1..1048576 stereo f32 frames")
    frames = len(data) // 8
    samples = struct.unpack("<" + "f" * frames * 2, data)
    if not 1 <= args.block_size <= 1024 or not 0 <= args.start_frame < frames:
        parser.error("invalid block size or start frame")
    channels = [(ctypes.c_float * args.block_size)() for c in range(2)]
    for c, channel in enumerate(channels):
        struct.pack_into("<Q", state, 0x20 + c * 8, ctypes.addressof(channel))
    struct.pack_into("<ii", state, 0x124, 2, 2)
    tables = []
    setup = args.setup.read_bytes()
    for index in range(12):
        at = index * 2068 + 20
        values = struct.unpack_from("<256f", setup, at)
        slopes = struct.unpack_from("<256f", setup, at + 1024)
        tables.append((ctypes.c_float * 512)(*[v for pair in zip(values, slopes) for v in pair]))
    for slot, index in enumerate([0, 8, 4, 3, 5, 0, 0, 0, 2, 6, 9, 10, 1, 7, 11, 11]):
        struct.pack_into("<Q", state, 0x9c80 + slot * 8, ctypes.addressof(tables[index]))
    libc = ctypes.CDLL(None, use_errno=True)
    with BINARY.open("rb") as source:
        image = mmap.mmap(source.fileno(), 0, flags=mmap.MAP_PRIVATE, prot=mmap.PROT_READ | mmap.PROT_WRITE)
        base = ctypes.addressof(ctypes.c_ubyte.from_buffer(image))
        for offset, symbol in [(0x9636d90, "bzero"), (0x9637988, "memcpy")]:
            struct.pack_into("<Q", image, offset, ctypes.cast(getattr(libc, symbol), ctypes.c_void_p).value)
        if libc.mprotect(ctypes.c_void_p(base), ctypes.c_size_t(len(image)), mmap.PROT_READ | mmap.PROT_EXEC):
            raise OSError(ctypes.get_errno(), "mprotect")
        process_data = ctypes.create_string_buffer(0xb0000)
        ptr = ctypes.addressof(ctypes.c_ubyte.from_buffer(state))
        function = ctypes.CFUNCTYPE(None, ctypes.c_void_p, ctypes.c_void_p, ctypes.c_int, ctypes.c_ulong)(base + 0x5d55bc)
        control = ctypes.CFUNCTYPE(None, ctypes.c_void_p)(base + 0x5d530c)
        output_values = (ctypes.c_float * (frames * 2))()
        native_outputs = [(ctypes.c_float * args.block_size).from_address(ctypes.addressof(process_data) + 0x9c010 + c * 0x1000) for c in range(2)]
        for start in range(args.start_frame, frames, args.block_size):
            count = min(args.block_size, frames - start)
            for i in range(count):
                for c in range(2):
                    channels[c][i] = samples[(start + i) * 2 + c]
            if args.block_size == 32:
                control(ptr)
            function(ptr, ctypes.addressof(process_data), 0, count)
            for i in range(count):
                for c in range(2):
                    output_values[(start + i) * 2 + c] = native_outputs[c][i]
        output = bytes(output_values)
        args.output.write_bytes(output)
        state[0x20:0x30] = bytes(16)
        state[0x9c80:0x9d00] = bytes(128)
        args.state_output.write_bytes(state)
        image.close()
    report = {"binary_sha256": DIGEST, "function": "0x1005d55bc", "frames": frames,
              "initial_state_sha256": hashlib.sha256(args.state.read_bytes()).hexdigest(),
              "input_sha256": hashlib.sha256(data).hexdigest(), "output_sha256": hashlib.sha256(output).hexdigest(),
              "method": "Original isolated ARM64 ProcessInt instructions, explicit coefficient state and independent setup tables; only libc memory imports bound",
              "limitations": ["Does not validate initialization or the full Kontakt host lifecycle"]}
    args.output.with_suffix(".json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
