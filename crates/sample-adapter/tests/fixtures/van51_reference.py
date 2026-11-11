#!/usr/bin/env python3
"""Extract VAN51 wave-shaping curves and replay TableBase::setupTable.

Static evidence:
  NI::DSP::VAN51::WaveShapingTables::*::s_dUnprocessedData (twelve 256x f64 curves)
  NI::DSP::VAN51::WaveShapingTables::TableBase::setupTable  0x1005caad0
  BEffectVan51::Initialize                                  0x1005cdc34
  BEffectVan51::BEffectVan51                                0x1005cd984 (stage/vtable order)

Curve addresses are the returns of each stage's `getUnprocessedTable`
(0x1005ed66c..0x1005ed6fc), resolved from the ARM64 image. The eleven
`setupTable` calls use the arguments in SETUPS; `Van51PowerTube` is built
manually by Initialize at stage offset +0x8c30. This script is independent of
the Rust port and writes the curve blob plus the expected table objects.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import struct
from pathlib import Path

BINARY_SHA256 = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"
BINARY_BASE = 0x100000000

CURVES = [
    ("stage1", 0x104D3DF00),
    ("stage2_rhythm", 0x104D3E700),
    ("stage2_lead", 0x104D3EF00),
    ("stage3_rhythm", 0x104D3F700),
    ("stage3_lead", 0x104D3FF00),
    ("stage4", 0x104D40700),
    ("stage5", 0x104D40F00),
    ("stage6", 0x104D41700),
    ("stage8", 0x104D41F00),
    ("splitter1", 0x104D42700),
    ("splitter2", 0x104D42F00),
    ("power_tube", 0x104D43700),
]

# (name, curve index, param_1, param_2, param_3, flag, param_6)
SETUPS = [
    ("stage1", 0, 0x3F045A1D, 0x4234BE0C, 0x3C6F4AEA, 1, 0),
    ("stage2_rhythm", 1, 0x3ED6872B, 0x415F6B40, 0x3CD55338, 1, 0),
    ("stage2_lead", 2, 0x3EDB22D1, 0x4145FAB9, 0, 1, 0),
    ("stage3_rhythm", 3, 0x3F45A1CB, 0x3F800000, 0, 1, 0),
    ("stage3_lead", 4, 0x3F2B020C, 0x3FA4E55C, 0, 1, 0),
    ("stage4", 5, 0x3F19999A, 0x41D4DBA6, 0, 1, 0),
    ("stage5", 6, 0x3ECFDF3B, 0x41F73940, 0, 1, 0),
    ("stage6", 7, 0x3E9A1CAC, 0x4005B707, 0, 1, 0),
    ("stage8", 8, 0x3EB645A2, 0x41F73940, 0x3CEE7E2E, 1, 0),
    ("splitter1", 9, 0x3ED6872B, 0x41BFE819, 0, 1, 0),
    ("splitter2", 10, 0x3EFC6A7F, 0x41BFE819, 0, 1, 0),
]

# Fixed control tables referenced by the Van51 setters (11 f32 each).
CONTROL_TABLES = [
    ("lp2f_lead_on", 0x104D1C7D0),
    ("hp3f_lead_on", 0x104D1C7FC),
    ("hp3gain_lead_on", 0x104D1C828),
    ("lp2f_lead_off", 0x104D1C854),
    ("hp3f_lead_off", 0x104D1C880),
    ("hp3gain_lead_off", 0x104D1C8AC),
    ("lp2f_rhythm_on", 0x104D1C8D8),
    ("hp3f_rhythm_on", 0x104D1C904),
    ("hp3gain_rhythm_on", 0x104D1C930),
    ("lp2f_rhythm_off", 0x104D1C95C),
    ("hp3f_rhythm_off", 0x104D1C988),
    ("hp3gain_rhythm_off", 0x104D1C9B4),
]
CRUNCH_TABLES = [
    ("hp4f_crunch_on_bright_on", 0x104D1C9E0),
    ("lp5f_crunch_on_bright_on", 0x104D1CA0C),
    ("lp5gain_crunch_on_bright_on", 0x104D1CA38),
    ("lp6f_crunch_on_bright_on", 0x104D1CA64),
    ("lp6gain_crunch_on_bright_on", 0x104D1CA90),
    ("lp7f_crunch_on_bright_on", 0x104D1CABC),
    ("s2rgain_crunch_on_bright_on", 0x104D1CAE8),
    ("hp4f_crunch_on_bright_off", 0x104D1CB14),
    ("lp5f_crunch_on_bright_off", 0x104D1CB40),
    ("lp5gain_crunch_on_bright_off", 0x104D1CB6C),
    ("lp6f_crunch_on_bright_off", 0x104D1CB98),
    ("lp6gain_crunch_on_bright_off", 0x104D1CBC4),
    ("lp7f_crunch_on_bright_off", 0x104D1CBF0),
    ("s2rgain_crunch_on_bright_off", 0x104D1CC1C),
    ("hp4f_crunch_off_bright_on", 0x104D1CC48),
    ("lp5f_crunch_off_bright_on", 0x104D1CC74),
    ("lp5gain_crunch_off_bright_on", 0x104D1CCA0),
    ("lp6f_crunch_off_bright_on", 0x104D1CCCC),
    ("lp6gain_crunch_off_bright_on", 0x104D1CCF8),
    ("lp7f_crunch_off_bright_on", 0x104D1CD24),
    ("s2rgain_crunch_off_bright_on", 0x104D1CD50),
    ("hp4f_crunch_off_bright_off", 0x104D1CD7C),
    ("lp5f_crunch_off_bright_off", 0x104D1CDA8),
    ("lp5gain_crunch_off_bright_off", 0x104D1CDD4),
    ("lp6f_crunch_off_bright_off", 0x104D1CE00),
    ("lp6gain_crunch_off_bright_off", 0x104D1CE2C),
    ("lp7f_crunch_off_bright_off", 0x104D1CE58),
    ("s2rgain_crunch_off_bright_off", 0x104D1CE84),
]

LOG_POT_TABLES = [
    ("log_pot516", 0x104D1C74C),
    ("log_pot522", 0x104D1C778),
    ("log_pot528", 0x104D1C7A4),
]

# Manual table installed by Initialize for Van51PowerTube.
POWER_TUBE_ANCHOR = 0x178
POWER_TUBE_FRACTION = 0xBE449800
POWER_TUBE_SCALE = 0x44042F30


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
    return (POWER_TUBE_ANCHOR, from_bits(POWER_TUBE_FRACTION),
            from_bits(POWER_TUBE_SCALE), 0.0, 0.0, rel, slope)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path,
                        default=Path(__file__).resolve().parents[5] / "Kontakt-8-REA/Kontakt-8.arm64")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--curves", type=Path,
                        default=Path(__file__).resolve().parents[2] / "src/van51_curves.bin")
    parser.add_argument("--setup", type=Path,
                        default=Path(__file__).resolve().parent / "van51-setup.bin")
    parser.add_argument("--tables", type=Path,
                        default=Path(__file__).resolve().parents[2] / "src/van51_tables.bin")
    args = parser.parse_args()

    binary = args.binary.read_bytes()
    actual = hashlib.sha256(binary).hexdigest()
    if actual != BINARY_SHA256:
        raise SystemExit(f"Unexpected source binary SHA-256: {actual}")

    tables = []
    curve_blob = bytearray()
    for _, address in CURVES:
        start = address - BINARY_BASE
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
    anchor, fraction, scale, offset, baseline, rel, slope = power_tube(tables[11])
    setup_blob.extend(struct.pack("<i4f", anchor, fraction, scale, offset, baseline))
    setup_blob.extend(struct.pack("<256f", *rel))
    setup_blob.extend(struct.pack("<256f", *slope))
    args.setup.write_bytes(setup_blob)

    table_blob = bytearray()
    for _, address in CONTROL_TABLES + LOG_POT_TABLES + CRUNCH_TABLES:
        start = address - BINARY_BASE
        values = struct.unpack_from("<11f", binary, start)
        table_blob.extend(struct.pack("<11f", *values))
    args.tables.write_bytes(table_blob)

    provenance = {
        "origin": "Independent Python replay of TableBase::setupTable; static evidence only",
        "binary_sha256": actual,
        "curves": {name: hex(address) for name, address in CURVES},
        "curve_blob_sha256": hashlib.sha256(curve_blob).hexdigest(),
        "setup_blob_sha256": hashlib.sha256(setup_blob).hexdigest(),
        "setup_function": "Kontakt-8-REA/pseudocode/functions/0x1005caad0.c",
        "initialize_function": "Kontakt-8-REA/pseudocode/functions/0x1005cdc34.c",
        "constructor_function": "Kontakt-8-REA/pseudocode/functions/0x1005cd984.c",
        "curve_getters": "0x1005ed66c..0x1005ed6fc",
        "control_tables": {name: hex(address) for name, address in CONTROL_TABLES},
        "log_pot_tables": {name: hex(address) for name, address in LOG_POT_TABLES},
        "crunch_tables": {name: hex(address) for name, address in CRUNCH_TABLES},
        "table_blob_sha256": hashlib.sha256(args.tables.read_bytes()).hexdigest(),
    }
    (args.output / "van51_provenance.json").write_text(
        json.dumps(provenance, indent=2, sort_keys=True) + "\n"
    )


if __name__ == "__main__":
    main()
