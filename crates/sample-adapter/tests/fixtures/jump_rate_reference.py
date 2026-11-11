#!/usr/bin/env python3
"""Independent scalar replay of BEffectJump::SetSampleRate (0x1005caf30).

Source is saved Ghidra 12.1.4 output, not original source. This script never
reads the Rust implementation. It verifies the source binary identity and
extracts the four-lane constant through Mach-O segment mappings.

The SIMD reciprocal estimate plus refinements is represented by rounded f32
division, so reciprocal-derived coefficients require a small relative tolerance.
Arithmetic otherwise rounds operands and results at each f32 operation, without
FMA contraction. These are static coefficient references, NOT Kontakt audio or
a native instruction-execution oracle.

Only actual writes are emitted. +0x46b0..+0x472c are NOT written by this setter;
the table pointers at +0x48a0..+0x48c8 are outside this fixture. The +0x4930..
table metadata is initialized elsewhere. EQ/presence/control targets set by
separate functions are covered by jump_controls_reference.py.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path

BINARY_SHA256 = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"
FUNCTION = "Kontakt-8-REA/pseudocode/functions/0x1005caf30.c"
VECTOR_ADDRESS = 0x104CC8330


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def bits(value):
    return struct.unpack("<I", struct.pack("<f", value))[0]


def add(a, b):
    return f32(f32(a) + f32(b))


def sub(a, b):
    return f32(f32(a) - f32(b))


def mul(a, b):
    return f32(f32(a) * f32(b))


def div(a, b):
    return f32(f32(a) / f32(b))


def exp_index(x):
    x = f32(x)
    index = int(x)
    if index >= 2400:
        return f32(2.0 ** 20.0)
    if index < 0:
        return 0.0
    base = f32(math.exp2((index - 1200.0) / 60.0))
    nxt = f32(math.exp2((index + 1 - 1200.0) / 60.0))
    return add(base, mul(sub(nxt, base), sub(x, index)))


def address_bytes(binary, address, length):
    """Resolve a file-backed LC_SEGMENT_64 range; do not assume VA == offset."""
    assert struct.unpack_from("<I", binary)[0] == 0xFEEDFACF
    ncmds = struct.unpack_from("<I", binary, 16)[0]
    position = 32
    for _ in range(ncmds):
        command, size = struct.unpack_from("<II", binary, position)
        if command == 0x19:
            vmaddr, _, fileoff, filesize = struct.unpack_from("<4Q", binary, position + 24)
            if vmaddr <= address and address + length <= vmaddr + filesize:
                offset = fileoff + address - vmaddr
                return binary[offset:offset + length], offset
        position += size
    raise ValueError(f"address {address:#x} is not file-backed")


def replay(rate, drive, input_scale, power_scale, response_db, response_offset,
           channel, vector):
    """Replay writes in native address order; four-float vectors are low to high."""
    out = {}

    def put(offset, *values):
        for lane, value in enumerate(values):
            out[offset + lane * 4] = f32(value)

    rate4 = mul(rate, 4.0)
    inverse = div(1.0, rate4)
    scale = mul(inverse, 6.283)
    period = max(1, int(add(mul(mul(rate4, 0.02), 0.0078125), 0.5)))
    put(0x4490, rate4, inverse, scale)
    put(0x4618, period, div(1.0, period * 128))
    put(0x453C, drive)
    put(0x2E4, mul(inverse, 120.0 if channel else 100.0))

    # ExpTable globals are value/slope pairs indexed relative to 0x109dc2200.
    tuning = {i: exp_index(add(i, 0.8815918)) for i in
              (1585, 1586, 1746, 1927, 1996, 2021, 2026, 2031, 2106)}
    clamp = lambda value: min(value, 1.0)
    put(0x48D0, *(clamp(mul(tuning[i], scale)) for i in (1586, 2106, 1996, 2031)))
    put(0x48E0, clamp(mul(tuning[1746], scale)), 0.0,
        clamp(mul(tuning[1927], scale)), clamp(mul(tuning[1585], scale)))
    low = clamp(mul(tuning[2021], scale))
    high = clamp(mul(tuning[2026], scale))
    put(0x48F0, low, low, high, high)
    put(0x4900, 0.0, 0.0, struct.unpack("<f", struct.pack("<I", 0x3F2E147A))[0], 0.0)
    first = clamp(mul(scale, exp_index(add(mul(response_db, 5.0), 1381.8816))))
    remaining = clamp(mul(scale, exp_index(add(mul(sub(response_db, 56.0), 5.0), 1381.8816))))
    put(0x4910, 0.0, 0.0, first, 0.0)
    put(0x4920, remaining, remaining, remaining, 0.0)

    # From this point fVar13 is TWICE rate4, i.e. eight times the host rate.
    r = add(rate4, rate4)
    p0 = [add(mul(r, 690.9), 1.47e6), f32(2.94e6),
          add(mul(r, -690.9), 1.47e6), 0.0]
    p1 = [mul(add(mul(r, 0.2209), 530.0), r), mul(mul(r, -0.4418), r),
          mul(add(mul(r, 0.2209), -530.0), r), 0.0]
    p2 = [add(mul(add(mul(r, 0.2209), 1220.9), r), 1e6),
          add(mul(mul(r, -0.4418), r), 2e6),
          add(mul(add(mul(r, 0.2209), -1220.9), r), 1e6), 0.0]
    p3 = [add(mul(r, -6.909e8), -1.47e12), f32(-2.94e12),
          add(mul(r, 6.909e8), -1.47e12), 0.0]
    p4 = [mul(add(mul(r, -0.2209), -1000.0), r), mul(mul(r, 0.4418), r),
          mul(add(mul(r, -0.2209), 1000.0), r), 0.0]
    p6 = [add(mul(r, 6.9090003e8), 1e12), f32(2e12),
          add(mul(r, -6.9090003e8), 1e12), 0.0]
    for offset, values in ((0x4630, p0), (0x4640, p1), (0x4650, p2),
                           (0x4660, p3), (0x4670, p4), (0x4680, p4), (0x4690, p6)):
        put(offset, *values)
    put(0x46A0, *(struct.unpack("<4f", struct.pack("<4I", 0x471B82B3, 0x418B3333,
                                                 0xC18B3333, 0xC18B3333))))

    # Native polynomial groups. Keeping the tone-dependent lane separate makes
    # this replay independent of the Rust structure/group representation.
    den_a = [add(1.0, mul(r, 6.82176e-6)), add(1.0, mul(r, 6.82176e-6)),
             add(p0[0], mul(drive, add(mul(drive, p4[0]), p1[0]))),
             add(1.0, mul(r, add(mul(r, 2.284106e-6), 0.0209009)))]
    num_a = [1.0, 1.0, mul(drive, add(mul(drive, p4[0]), p2[0])),
             mul(r, add(mul(r, 2.284106e-6), 0.01034))]
    source_a = [f32(-68000.0), f32(-68000.0),
                mul(drive, add(mul(drive, p6[0]), p3[0])),
                add(mul(r, -4963.623), -470000.0)]
    middle_den_a = [add(mul(r, -6.82176e-6), 1.0), add(mul(r, -6.82176e-6), 1.0),
                    add(p0[1], mul(drive, add(mul(drive, p4[1]), p1[1]))),
                    add(mul(mul(r, -4.568212e-6), r), 2.0)]
    middle_num_a = [1.0, 1.0, mul(drive, add(mul(drive, p4[1]), p2[1])),
                    mul(mul(r, -4.568212e-6), r)]
    middle_source_a = [f32(-68000.0), f32(-68000.0),
                       mul(drive, add(mul(drive, p6[1]), p3[1])), f32(-940000.0)]
    last_den_a = [0.0, 0.0, add(p0[2], mul(drive, add(mul(drive, p4[2]), p1[2]))),
                  add(mul(r, add(mul(r, 2.284106e-6), -0.0209009)), 1.0)]
    last_num_a = [0.0, 0.0, mul(drive, add(mul(drive, p4[2]), p2[2])),
                  mul(r, add(mul(r, 2.284106e-6), -0.01034))]
    last_source_a = [0.0, 0.0, mul(drive, add(mul(drive, p6[2]), p3[2])),
                     add(mul(r, 4963.623), -470000.0)]
    put(0x4730, *num_a)
    put(0x4810, *middle_den_a)
    put(0x4830, *middle_num_a)
    put(0x4850, *middle_source_a)
    put(0x4870, *last_den_a)
    put(0x4880, *last_num_a)
    put(0x4890, *last_source_a)

    resistance_a = [f32(v) for v in (2818.38, 2818.38, 39810.7, 1584.89)]
    factor_a = [f32(v) for v in (2.5, 2.5, 17.4, 1.0)]
    rec_a = [div(1.0, x) for x in den_a]
    impedance_a = [div(1.0, sub(mul(d, resist), source))
                   for d, resist, source in zip(den_a, resistance_a, source_a)]
    put(0x47B0, *rec_a)
    put(0x47D0, *impedance_a)
    put(0x4770, *(mul(inv, resist) for inv, resist in zip(impedance_a, resistance_a)))
    put(0x4790, *(mul(sub(0.0, mul(source, factor)), inv)
                  for source, factor, inv in zip(source_a, factor_a, impedance_a)))
    put(0x47F0, *(mul(sub(0.0, mul(d, factor)), inv)
                  for d, factor, inv in zip(den_a, factor_a, impedance_a)))
    put(0x4750, *(mul(d, factor) for d, factor in zip(den_a, factor_a)))
    out[0x4730] = mul(input_scale, out[0x4730])
    out[0x4830] = mul(input_scale, out[0x4830])
    out[0x4880] = mul(input_scale, out[0x4880])

    den_b = [add(mul(r, 0.022), 1.0)] * 3 + [1.0]
    num_b = [mul(r, 0.022)] * 3 + [0.0]
    source_b = [add(mul(r, -22000.0), -2e6)] * 2 + [f32(-500000.0), 0.0]
    put(0x4740, *num_b)
    put(0x4820, *([add(mul(r, -0.022), 1.0)] * 3 + [0.0]))
    put(0x4840, *([mul(r, -0.022)] * 3 + [0.0]))
    put(0x4860, *([add(mul(r, 22000.0), -2e6)] * 2 + [f32(-500000.0), 0.0]))
    rec_b = [div(1.0, x) for x in den_b]
    resistance_b = [f32(v) for v in (37583.7, 37583.7, 112202.0, 1.0)]
    impedance_b = [div(1.0, sub(mul(d, resist), source))
                   for d, resist, source in zip(den_b, resistance_b, source_b)]
    put(0x47C0, *rec_b)
    put(0x47E0, *impedance_b)
    put(0x4780, *(mul(inv, resist) for inv, resist in zip(impedance_b, resistance_b)))
    put(0x47A0, *(mul(sub(0.0, mul(source, factor)), inv)
                  for source, factor, inv in zip(source_b, vector, impedance_b)))
    scaled_den_b = [mul(d, factor) for d, factor in zip(den_b, vector)]
    put(0x4800, *(mul(sub(0.0, d), inv) for d, inv in zip(scaled_den_b, impedance_b)))
    put(0x4760, *scaled_den_b)
    put(0x4740, mul(num_b[0], 0.7943282), mul(num_b[1], 0.7943282))
    put(0x4840, mul(mul(r, -0.022), 0.7943282), mul(mul(r, -0.022), 0.7943282))
    put(0x478C, div(-0.024208618, power_scale))
    put(0x47CC, div(-0.024208618, power_scale))

    response1 = exp_index(add(mul(add(mul(response_db, 0.5), -40.0), 9.965784), 1200.0))
    response2 = exp_index(add(mul(add(add(mul(response_db, 0.5), -13.0), response_offset),
                                 9.965784), 1200.0))
    put(0x49C4, mul(mul(response1, -1000.0), inverse))
    put(0x49C0, mul(mul(inverse, response2), 1000.0))
    return out


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    root = Path(__file__).resolve().parents[5]
    parser.add_argument("--binary", type=Path, default=root / "Kontakt-8-REA/Kontakt-8.arm64")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    binary = args.binary.read_bytes()
    assert hashlib.sha256(binary).hexdigest() == BINARY_SHA256, "source binary identity changed"
    vector_bytes, vector_offset = address_bytes(binary, VECTOR_ADDRESS, 16)
    vector = struct.unpack("<4f", vector_bytes)
    assert vector == (24.0, 24.0, 0.0, 0.0)
    cases = []
    for rate in (8000.0, 44100.0, 48000.0, 96000.0, 192000.0):
        for index, drive in enumerate((0.001, 0.01, 0.1515, 0.71, 0.96)):
            cases.append((rate, drive, (1.0, 0.25, 2.5, 1.5, 0.0)[index],
                          (1.0, 2.0, 0.5, 0.25, 4.0)[index],
                          (-24.0, -12.0, 0.0, 12.0, 24.0)[index],
                          (0.0, -6.0, 0.0, 6.0, 0.0)[index], index % 2))
    # Explicit ExpTable clamps and non-audio test rates exercise bounded branches.
    cases.extend(((48000.0, 0.1515, 1.0, 1.0, -1000.0, 0.0, 0),
                  (48000.0, 0.1515, 1.0, 1.0, 1000.0, 0.0, 1),
                  (100.0, 0.1515, 1.0, 1.0, 0.0, 0.0, 0)))
    lines = ["case\trate\tdrive_target\tinput_scale\tpower_scale\tresponse_db\tresponse_offset\tchannel\toffset\tvalue"]
    count = 0
    for case, args_case in enumerate(cases):
        inputs = [f32(v) for v in args_case]
        values = replay(*inputs, vector)
        for offset, value in sorted(values.items()):
            assert math.isfinite(value), (case, hex(offset), value)
            lines.append("\t".join([str(case), *(f"{v:.9g}" for v in inputs), hex(offset), f"{value:.9g}"]))
            count += 1
    fixture = ("\n".join(lines) + "\n").encode()
    output = args.output / "jump-rate.tsv"
    output.write_bytes(fixture)
    provenance = {
        "origin": "Independent Python f32 replay of saved Ghidra SetSampleRate; static coefficient reference",
        "binary_sha256": BINARY_SHA256,
        "source_function": FUNCTION,
        "source_sha256": hashlib.sha256((root / FUNCTION).read_bytes()).hexdigest(),
        "vector_address": hex(VECTOR_ADDRESS),
        "vector_file_offset": hex(vector_offset),
        "vector_values": list(vector),
        "vector_bits": [hex(bits(v)) for v in vector],
        "cases": len(cases), "values": count,
        "fixture_sha256": hashlib.sha256(fixture).hexdigest(),
        "limitations": [
            "No Kontakt execution or native audio capture",
            "NEON reciprocal estimate/refinement replaced by rounded f32 division",
            "No FMA contraction; native instruction rounding can differ",
            "Only writes in SetSampleRate are covered; EQ targets, table metadata/pointers and untouched gaps are omitted",
            "SetSampleRate consumes the pre-existing drive target before updating it at function exit; fixtures supply that input explicitly",
        ],
    }
    (args.output / "jump-rate-provenance.json").write_text(json.dumps(provenance, indent=2, sort_keys=True) + "\n")
    print(f"Wrote {len(cases)} cases / {count} coefficients to {output}")


if __name__ == "__main__":
    main()
