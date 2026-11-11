#!/usr/bin/env python3
"""Retain bounded, read-only ProcessInt entry/exit observations from Kontakt.

The capture script/provenance stay under app/build/verification. This extractor
copies the initial numerical state and 14 consecutive 32-frame input/output
slices, removing all process-specific pointers. No target code is shipped.
"""
import hashlib
import json
import struct
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
SOURCE = ROOT / "app/build/verification/native-van51-host-trace-2026-11-09"
FOLDER = Path(__file__).resolve().parent
state = bytearray((SOURCE / "slice-000-state.bin").read_bytes())
for at in range(0, len(state), 8):
    value = struct.unpack_from("<Q", state, at)[0]
    if 0x100000000 <= value < 0x1000000000000:
        struct.pack_into("<Q", state, at, 0)
output = bytearray(b"VANHOST1" + struct.pack("<II", 14, 32) + state)
for index in range(14):
    before = (SOURCE / f"slice-{index:03}-input.bin").read_bytes()
    after = (SOURCE / f"slice-{index:03}-output.bin").read_bytes()
    assert len(before) == len(after) == 32 * 8
    for at in range(0, len(before), 8):
        output.extend(before[at:at+8] + after[at:at+8])
path = FOLDER / "van51-host-trace.bin"
path.write_bytes(output)
path.with_suffix(".json").write_text(json.dumps({
    "method": __doc__, "source": str(SOURCE), "function": "BEffectVan51::ProcessInt",
    "installed_vst3_offset": "0x5dc6dc", "frames": 448,
    "blob_sha256": hashlib.sha256(output).hexdigest(),
    "observations": ["Presence target retains its 44.1 kHz value after host preparation at 48 kHz",
                     "Host activation fades three unity currents from zero",
                     "Full native dry-input replay from captured state is bit-identical through the last non-silent host block"],
    "limitations": ["One default-control host activation; does not prescribe application lifecycle policy"]
}, indent=2) + "\n")
