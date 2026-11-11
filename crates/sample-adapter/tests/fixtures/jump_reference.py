#!/usr/bin/env python3
"""Extract JUMP wave-shaping curves and replay TableBase::setupTable.

Static evidence:
  NI::DSP::JUMP::WaveShapingTables::*::s_dUnprocessedData (six 256x f64 curves)
  NI::DSP::VAN51::WaveShapingTables::TableBase::setupTable  0x1005caad0
  BEffectJump::Initialize                                   0x1005ca0a0

The five curve objects use setupTable with the arguments below; the power-tube
table is built manually by Initialize. This script is independent of the Rust
port and writes the curve blob plus the expected table objects.
"""
from __future__ import annotations

import argparse
import hashlib
import struct
from pathlib import Path

BINARY_SHA256 = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"

CURVES = [
    ("boost", 0x104D3AF00),
    ("stage1", 0x104D3B700),
    ("stage2", 0x104D3BF00),
    ("splitter_p", 0x104D3C700),
    ("splitter_n", 0x104D3CF00),
    ("power_tube", 0x104D3D700),
]

# (name, table index, param_1, param_2, param_3, flag, param_6)
SETUPS = [
    ("boost", 0, 0x3F2624DD, 0x4243E95A, 0x3CDD2F1B, 1, 0),
    ("stage1", 1, 0x3E8DD2F2, 0x413177CD, 0, 1, 1),
    ("stage2", 2, 0x3F5D2F1B, 0x427C6208, 0, 1, 1),
    ("splitter_p", 3, 0x3F27AE14, 0x42016FFE, 0, 1, 1),
    ("splitter_n", 4, 0x3ED89375, 0x41D4DBA6, 0, 1, 1),
]


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def from_bits(bits: int) -> float:
    return struct.unpack("<f", struct.pack("<I", bits))[0]


def setup_table(table: list[float], p1: float, p2: float, p3: float,
                flag: int, param6: int):
    scaled = f32(p2 * 256.0 * p3)
    offset = scaled if param6 != 0 else 0.0
    scale = f32(p2 * 256.0)
    position = f32(p1 * 256.0 - offset)

    baseline = 0.0
    if flag & 1:
        target = float(position)
        increment = float(offset)
        baseline = 0.0
        while True:
            raw = target + baseline * increment
            index = int(raw)
            index = 0 if index < 0 else index
            index = 0xFFFFFFFF if index > 0xFFFFFFFF else index
            limit = min(index, 255)
            baseline = table[limit]
            if index < 255:
                low = table[limit]
                high = table[limit + 1]
            else:
                low = table[254]
                high = table[255]
            fraction = ((target + increment * baseline) - limit) / (
                1.0 - increment * (high - low)
            )
            baseline += (high - low) * fraction
            if not ((fraction < 0.0 and limit != 0) or (fraction >= 1.0 and index < 255)):
                break

    final = f32(float(position) + f32(baseline * float(offset)))
    anchor = int(final + 0.5)
    fraction = f32(final - anchor)
    base = f32(baseline)

    rel: list[float] = []
    slope: list[float] = []
    current = table[0]
    for index in range(255):
        nxt = table[index + 1]
        rel.append(f32(current - baseline))
        slope.append(f32(nxt - current))
        current = nxt
    rel.append(f32(table[255] - baseline))
    slope.append(f32(table[255] - table[254]))
    return anchor, fraction, scale, scaled, base, rel, slope


def power_tube(table: list[float]):
    rel: list[float] = []
    slope: list[float] = []
    current = table[0]
    for index in range(255):
        nxt = table[index + 1]
        rel.append(f32(current))
        slope.append(f32(nxt - current))
        current = nxt
    rel.append(f32(table[255]))
    slope.append(f32(table[255] - table[254]))
    anchor = 0x101
    fraction = struct.unpack("<f", struct.pack("<I", 0xBE6D9000))[0]
    scale = struct.unpack("<f", struct.pack("<I", 0x45259F6C))[0]
    return anchor, fraction, scale, 0.0, 0.0, rel, slope


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path,
                        default=Path(__file__).resolve().parents[5] / "Kontakt-8-REA/Kontakt-8.arm64")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--curves", type=Path,
                        default=Path(__file__).resolve().parents[2] / "src/jump_curves.bin")
    parser.add_argument("--setup", type=Path,
                        default=Path(__file__).resolve().parent / "jump-setup.bin")
    args = parser.parse_args()

    binary = args.binary.read_bytes()
    actual = hashlib.sha256(binary).hexdigest()
    if actual != BINARY_SHA256:
        raise SystemExit(f"Unexpected source binary SHA-256: {actual}")

    tables = []
    curve_blob = bytearray()
    for _, address in CURVES:
        start = address - 0x100000000
        table = list(struct.unpack_from("<256d", binary, start))
        tables.append(table)
        curve_blob.extend(struct.pack("<256d", *table))
    args.curves.write_bytes(curve_blob)

    setup_blob = bytearray()
    for _, index, p1, p2, p3, flag, param6 in SETUPS:
        anchor, fraction, scale, scaled, baseline, rel, slope = setup_table(
            tables[index], from_bits(p1), from_bits(p2), from_bits(p3), flag, param6
        )
        setup_blob.extend(struct.pack("<i4f", anchor, fraction, scale, scaled, baseline))
        setup_blob.extend(struct.pack("<256f", *rel))
        setup_blob.extend(struct.pack("<256f", *slope))
    anchor, fraction, scale, offset, baseline, rel, slope = power_tube(tables[5])
    setup_blob.extend(struct.pack("<i4f", anchor, fraction, scale, offset, baseline))
    setup_blob.extend(struct.pack("<256f", *rel))
    setup_blob.extend(struct.pack("<256f", *slope))
    args.setup.write_bytes(setup_blob)

    provenance = {
        "origin": "Independent Python replay of TableBase::setupTable; static evidence only",
        "binary_sha256": actual,
        "curves": {name: hex(address) for name, address in CURVES},
        "curve_blob_sha256": hashlib.sha256(curve_blob).hexdigest(),
        "setup_blob_sha256": hashlib.sha256(setup_blob).hexdigest(),
        "setup_function": "Kontakt-8-REA/pseudocode/functions/0x1005caad0.c",
        "initialize_function": "Kontakt-8-REA/pseudocode/functions/0x1005ca0a0.c",
    }
    (args.output / "jump_provenance.json").write_text(
        __import__("json").dumps(provenance, indent=2, sort_keys=True) + "\n"
    )


if __name__ == "__main__":
    main()
