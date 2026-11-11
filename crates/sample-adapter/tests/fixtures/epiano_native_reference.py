#!/usr/bin/env python3
"""Isolated EPianoPreamps native oracle, bound to the recorded ARM64 image.

Rebases Mach-O chained data pointers and resolves host C/C++ numerical/memory
imports. No plugin loader, application entry point, network or native UI runs.
The private mapping is never written back to the artifact.
"""
import ctypes
import hashlib
import json
import math
import mmap
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
BINARY = ROOT.parent / "Kontakt-8-REA/Kontakt-8.arm64"
DIGEST = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"


class NativeImage:
    def __init__(self):
        content = BINARY.read_bytes()
        assert hashlib.sha256(content).hexdigest() == DIGEST
        self.file = BINARY.open("rb")
        self.memory = mmap.mmap(self.file.fileno(), 0, flags=mmap.MAP_PRIVATE,
                                prot=mmap.PROT_READ | mmap.PROT_WRITE)
        self.base = ctypes.addressof(ctypes.c_ubyte.from_buffer(self.memory))
        self.libc = ctypes.CDLL(None, use_errno=True)
        self.cpp = ctypes.CDLL("/usr/lib/libc++.1.dylib", mode=ctypes.RTLD_GLOBAL)
        self.allocations = {}
        self.keep = []
        self.libc.malloc.argtypes = [ctypes.c_size_t]
        self.libc.malloc.restype = ctypes.c_void_p
        self.libc.free.argtypes = [ctypes.c_void_p]
        self.libc.posix_memalign.argtypes = [ctypes.POINTER(ctypes.c_void_p), ctypes.c_size_t, ctypes.c_size_t]
        def allocate(size):
            pointer = self.libc.malloc(size)
            self.allocations[pointer] = size
            return pointer
        def release(pointer):
            self.allocations.pop(pointer, None)
            self.libc.free(pointer)
        def aligned(output, alignment, size):
            result = self.libc.posix_memalign(output, alignment, size)
            if not result:
                self.allocations[output[0]] = size
            return result
        allocate_callback = ctypes.CFUNCTYPE(ctypes.c_void_p, ctypes.c_size_t)(allocate)
        release_callback = ctypes.CFUNCTYPE(None, ctypes.c_void_p)(release)
        aligned_callback = ctypes.CFUNCTYPE(ctypes.c_int, ctypes.POINTER(ctypes.c_void_p), ctypes.c_size_t, ctypes.c_size_t)(aligned)
        self.keep.extend([allocate_callback, release_callback, aligned_callback])
        replacements = {name: ctypes.cast(callback, ctypes.c_void_p).value for names, callback in [
            (["__Znwm", "__Znam", "_malloc"], allocate_callback),
            (["__ZdlPv", "__ZdaPv", "__ZdlPvm", "__ZdaPvm", "_free"], release_callback),
            (["_posix_memalign"], aligned_callback)] for name in names}
        self.libc.dlsym.argtypes = [ctypes.c_void_p, ctypes.c_char_p]
        self.libc.dlsym.restype = ctypes.c_void_p
        count = struct.unpack_from("<I", content, 16)[0]
        position = 32
        segments = []
        for _ in range(count):
            cmd, size = struct.unpack_from("<II", content, position)
            if cmd == 0x80000034:
                fixups, fixups_size = struct.unpack_from("<II", content, position + 8)
            if cmd == 0x19:
                name, va, virtual_size, offset, file_size = struct.unpack_from("<16sQQQQ", content, position + 8)
                segments.append((name.rstrip(b"\0"), va, virtual_size, offset, file_size))
            position += size
        # The data segment's zero-fill tail overlaps LINKEDIT in the flat file.
        # Numerical helpers never access LINKEDIT; recreate that BSS as dyld does.
        for name, va, virtual_size, offset, file_size in segments:
            if virtual_size > file_size and name == b"__DATA":
                start_bss = va - 0x100000000 + file_size
                self.memory[start_bss:va - 0x100000000 + virtual_size] = bytes(virtual_size - file_size)
        version, starts, imports, symbols, import_count, import_format, symbol_format = struct.unpack_from("<7I", content, fixups)
        assert (version, import_format, symbol_format) == (0, 3, 0)
        bindings = []
        self.unresolved = []
        for index in range(import_count):
            descriptor, addend = struct.unpack_from("<Qq", content, fixups + imports + index * 16)
            name_offset = descriptor >> 32
            end = content.index(0, fixups + symbols + name_offset)
            name = content[fixups + symbols + name_offset:end].decode()
            address = replacements.get(name) or self.libc.dlsym(ctypes.c_void_p(-2), name.removeprefix("_").encode())
            if not address:
                self.unresolved.append(name)
            bindings.append((address or 0) + addend)
        segment_count = struct.unpack_from("<I", content, fixups + starts)[0]
        relocated = 0
        self.canonical_fixups = {}
        for segment_index in range(segment_count):
            relative = struct.unpack_from("<I", content, fixups + starts + 4 + segment_index * 4)[0]
            if not relative:
                continue
            start = fixups + starts + relative
            size, page_size, pointer_format, segment_offset, maximum, page_count = struct.unpack_from("<IHHQIH", content, start)
            assert pointer_format == 6
            for page in range(page_count):
                first = struct.unpack_from("<H", content, start + 22 + page * 2)[0]
                if first == 0xffff:
                    continue
                assert not first & 0x8000, "multi-start page needs explicit support"
                cursor = segment_offset + page * page_size + first
                while True:
                    raw = struct.unpack_from("<Q", content, cursor)[0]
                    stride = (raw >> 51) & 0xfff
                    if raw >> 63:
                        ordinal = raw & 0xffffff
                        addend = (raw >> 24) & 255
                        if addend & 128:
                            addend -= 256
                        value = bindings[ordinal] + addend
                        canonical = 0x50000000000 if cursor == 0x9636c10 else 0
                    else:
                        value = self.base + (raw & ((1 << 36) - 1))
                        value |= ((raw >> 36) & 255) << 56
                        canonical = 0x100000000 + (raw & ((1 << 36) - 1))
                    self.canonical_fixups[cursor] = canonical
                    struct.pack_into("<Q", self.memory, cursor, value & 0xffffffffffffffff)
                    relocated += 1
                    if not stride:
                        break
                    cursor += stride * 4
        for index in range(2401):
            low = struct.unpack("<f", struct.pack("<f", math.exp2((index - 1200) / 60)))[0]
            high = struct.unpack("<f", struct.pack("<f", math.exp2((index + 1 - 1200) / 60)))[0]
            struct.pack_into("<ff", self.memory, 0x9dc2200 + index * 8, low, high - low)
        for name, va, virtual_size, offset, file_size in segments:
            if name == b"__TEXT":
                result = self.libc.mprotect(ctypes.c_void_p(self.base + offset), ctypes.c_size_t(file_size), mmap.PROT_READ | mmap.PROT_EXEC)
                if result:
                    raise OSError(ctypes.get_errno(), "mprotect executable text")
        self.relocated = relocated

    def function(self, address, result=None, args=(ctypes.c_void_p,)):
        return ctypes.CFUNCTYPE(result, *args)(self.base + address - 0x100000000)


def canonical_ep_snapshot(native, state, pointer):
    """Canonicalize EP pointers while preserving typed packed scalar fields.

    handleTremoloSpeedParameter (0x1019e14f8) writes f32 speed + dirty byte
    at each listed slot. Those 64-bit patterns can overlap ASLR image ranges;
    interpreting them as pointers corrupts the native numerical snapshot.
    """
    from amp_resampler_native_reference import canonical_snapshot
    result = canonical_snapshot(native, state, pointer, 0x6000)
    for offset in [0x58b8, 0x5920, 0x59c8, 0x5a30, 0x5ac0, 0x5b28,
                   0x5bb0, 0x5c18, 0x5ca0, 0x5d08, 0x5dc0, 0x5e28]:
        result[offset:offset+8] = state.raw[offset:offset+8]
    return result


class NativeEP:
    def __init__(self, image):
        self.image = image
        self.state = ctypes.create_string_buffer(0x6000)
        self.pointer = ctypes.addressof(self.state)
        for initializer in [0x1019eb704, 0x1019eb950, 0x1019ebb9c, 0x1019ebff8]:
            image.function(initializer, args=())()
        print("EP constructor", flush=True)
        image.function(0x1019d4708)(self.pointer)
        print("EP onInit", flush=True)
        image.function(0x1019d5e18)(self.pointer)
        print("EP initialized", flush=True)


if __name__ == "__main__":
    native = NativeImage()
    print(json.dumps({"relocated": native.relocated, "unresolved_imports": len(native.unresolved)}), flush=True)
    core = NativeEP(native)
    Path("/tmp/epiano-native-constructed.bin").write_bytes(core.state.raw)
    Path("/tmp/epiano-native-pointers.json").write_text(json.dumps({"image": native.base, "core": core.pointer}))
