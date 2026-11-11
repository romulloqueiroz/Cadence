#!/usr/bin/env python3
"""Regenerate EP model/initial-state data and canonical immutable data pages."""
import hashlib
import json
import re
import struct
from pathlib import Path
from epiano_native_reference import NativeImage, NativeEP, canonical_ep_snapshot, DIGEST


def write_changed(path, data):
    if not path.exists() or path.read_bytes() != data:
        path.write_bytes(data)


def main():
    folder = Path(__file__).resolve().parent
    source = folder.parents[1] / "src"
    native = NativeImage()
    core = NativeEP(native)
    initial = canonical_ep_snapshot(native, core.state, core.pointer)
    write_changed(source / "epiano_initial.bin", initial)
    text = (source / "epiano_numeric.rs").read_text().split("pub(super) const CONSTANT_PAGES:")[1]
    pages = [int(value, 16) for value in re.findall(r"0x[0-9a-f]+", text)]
    constants = bytearray((source / "epiano_constants.bin").read_bytes())
    for index, page in enumerate(pages):
        offset = page - 0x100000000
        for address, canonical in native.canonical_fixups.items():
            if offset <= address < offset + 4096:
                struct.pack_into("<Q", constants, index * 4096 + address - offset, canonical)
    write_changed(source / "epiano_constants_canonical.bin", constants)
    report = {"binary_sha256": DIGEST,
        "functions": ["0x1019eb704", "0x1019eb950", "0x1019ebb9c", "0x1019ebff8", "0x1019d4708", "0x1019d5e18"],
        "method": "Original selected model-data initializers, constructor and onInit; typed numerical state and heap data, self/image pointers canonicalized. No executable bytes retained.",
        "initial_bytes": len(initial), "initial_sha256": hashlib.sha256(initial).hexdigest(),
        "canonical_constants_sha256": hashlib.sha256(constants).hexdigest(),
        "limitations": ["Selected numerical initialization only; no plugin loader, application entry point or native UI"]}
    (folder / "epiano-initial.json").write_text(json.dumps(report, indent=2) + "\n")
    print(json.dumps(report))


if __name__ == "__main__":
    main()
