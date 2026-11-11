#!/usr/bin/env python3
"""Scalar replay of the saved Kontakt JUMP ProcessInt body.

This reads saved native curve bytes and replays recovered float operations. It
never reads the Rust implementation and never executes Kontakt. Coefficient
snapshots and current values are explicit test inputs, not a claim that the
complete native control/base-effect initialization has been replicated.

All arithmetic rounds to binary32 after each operation; scalar division stands
in for NEON reciprocal estimate/refinement and no FMA contraction is modeled.
The original phase order, delayed state dependencies, and FIR summation order
are retained. See jump-process-provenance.json for exact sources and schema.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path

from jump_reference import BINARY_SHA256, CURVES, SETUPS, setup_table, power_tube, from_bits
from jump_rate_reference import f32, add, sub, mul, div, replay, address_bytes, VECTOR_ADDRESS, exp_index
from jump_controls_reference import set_eq_filters

C_BASE, C_COUNT = 0x4630, (0x49E0 - 0x4630) // 4
S_BASE, S_COUNT = 0x1D0, 0x120 // 4
UP = [
    [-0.0234375, 0.2265625, 0.8671875, -0.0703125],
    [-0.0625, 0.5625, 0.5625, -0.0625],
    [-0.0703125, 0.8671875, 0.2265625, -0.0234375],
    [0.0, 1.0, 0.0, 0.0],
]
DOWN = [-0.005859375, -0.015625, -0.017578125, 0.0,
        0.056640625, 0.140625, 0.216796875, 0.25,
        0.216796875, 0.140625, 0.056640625, -0.0,
        -0.017578125, -0.015625, -0.005859375, 0.0]
FIR_ORDER = [0, 4, 8, 12, 2, 6, 10, 14, 1, 5, 9, 13, 3, 7, 11, 15]
SOURCES = ["0x1005cc6ac.c", "0x1005ca0a0.c", "0x1005cbde0.c", "0x1005c9e78.c",
           "0x1005caf30.c", "0x1005cbf1c.c", "0x1005cd29c.c", "0x1005cac6c.c"]


def table_lookup(table, q):
    anchor, _, _, _, _, values, slopes = table
    lo = f32(-anchor)
    hi = sub(255.0, anchor)
    index = int(sub(min(max(q, lo), hi), lo))
    # Only the selected index is clamped. The residual extrapolates the original q.
    return add(values[index], mul(slopes[index], sub(q, add(lo, index))))


def initial_state(rate, boost_offset, channel=False):
    s = [0.0] * S_COUNT
    def put(offset, *values):
        for lane, value in enumerate(values):
            s[(offset - S_BASE) // 4 + lane] = f32(value)
    put(0x2E0, from_bits(0x3F374EBA), mul(div(1.0, mul(rate, 4.0)), 120.0 if channel else 100.0),
        413.0, 395.695)
    put(0x2B0, from_bits(0x43ABAD91), from_bits(0x4391ED0E), from_bits(0x438C16C9), boost_offset)
    put(0x2C0, 0.00291243, 0.00342639, 0.00356916, 0.0029539)
    put(0x280, from_bits(0x41E44C30), from_bits(0x41E56F9E), 0.0, 0.0)
    put(0x298, from_bits(0x43C514FE), 0.0)
    return s


def snapshot(rate, drive, eq, tables, channel=False):
    """Explicit meaningful coefficient/current snapshot, independent of Rust SetJump."""
    response = 0.0
    power_scale = from_bits(0x3D0186E3)
    values = replay(rate, drive, 1.0, power_scale, response, -0.0, channel, (24.0, 24.0, 0.0, 0.0))
    c = [0.0] * C_COUNT
    def put(offset, *values):
        for lane, value in enumerate(values):
            c[(offset - C_BASE) // 4 + lane] = f32(value)
    for offset, value in values.items():
        if C_BASE <= offset < C_BASE + C_COUNT * 4:
            put(offset, value)
    for phase, row in enumerate(UP):
        put(0x46E0 - phase * 16, *row)
    put(0x46F0, *DOWN)
    # Initialize metadata (bank A then bank B); duplicate curves retain own lanes.
    for bank, indices in enumerate(([0, 0, 1, 2], [5, 5, 3, 4])):
        for lane, index in enumerate(indices):
            table = tables[index]
            gain = table[2]
            if bank == 0 and lane < 2:
                gain = mul(gain, 1.2589254)
            elif bank == 0 and lane == 3:
                gain = mul(gain, 1.584894)
            elif bank == 1 and lane < 2:
                gain = mul(gain, 0.89)
            put(0x4930 + bank * 16 + lane * 4, gain)
            put(0x4950 + bank * 16 + lane * 4, table[1])
            put(0x4970 + bank * 16 + lane * 4, sub(255.0, table[0]))
            put(0x4990 + bank * 16 + lane * 4, -table[0])
    put(0x49C8, from_bits(0x3B0767AB), 484.0)
    # Deliberately asymmetric tube-bias inputs exercise both power-table paths.
    # These are explicit snapshot inputs, not native default-control claims.
    pgain = c[(0x4940 - C_BASE) // 4]
    put(0x49B0, div(mul(pgain, -37.8), 0.89), div(mul(pgain, -67.8), 0.89), 0.0, 0.0)
    q = set_eq_filters(*eq, f32(mul(div(1.0, mul(rate, 4.0)), 6.283)))
    m = [0.0] * 24
    m[0], m[1], m[4], m[5] = q[2], q[3], q[0], q[1]
    m[3], m[7], m[8] = from_bits(0x3E3CB242), from_bits(0x3EE4B3B6), 1.0
    m[9] = f32(0.003)
    m[10], m[11] = q[4], q[5]
    m[12], m[13] = f32(0.0), from_bits(0xBEA6476E)
    m[14], m[15], m[16], m[17], m[18] = q[6], q[7], q[8], q[9], q[10]
    m[19] = f32(drive)
    m[20] = mul(from_bits(0x3A7B892F), 1.5848932)
    # Native ProcessInt refresh writes these two current-dependent values.
    put(0x4910, m[17])
    put(0x4900, m[18])
    return c, m, initial_state(rate, tables[0][3], channel)


class Replay:
    def __init__(self, c, m, state, tables):
        self.c, self.m, self.s, self.tables = c, m, state.copy(), tables

    def process(self, input_value):
        c, m, s = self.c, self.m, self.s
        def C(offset): return c[(offset - C_BASE) // 4]
        def S(offset): return s[(offset - S_BASE) // 4]
        def put(offset, *values):
            for lane, value in enumerate(values):
                s[(offset - S_BASE) // 4 + lane] = f32(value)
        put(0x1D0, input_value)
        for phase in range(4):
            row = UP[phase]
            xup = add(add(add(mul(S(0x1D0), row[0]), mul(S(0x1D8), row[2])),
                             mul(S(0x1D4), row[1])), mul(S(0x1DC), row[3]))
            x = [xup, xup, S(0x2D0), S(0x2D4)]
            a = [0.0] * 4
            for i in range(4):
                o = i * 4
                u = add(S(0x220 + o), mul(C(0x4730 + o), x[i]))
                if u <= C(0x4750 + o):
                    a[i], h = mul(C(0x47B0 + o), u), 0.0
                else:
                    a[i] = add(C(0x4790 + o), mul(C(0x4770 + o), u))
                    h = add(C(0x47F0 + o), mul(C(0x47D0 + o), u))
                put(0x220 + o, add(add(S(0x230 + o), mul(C(0x4850 + o), h)),
                                   sub(mul(x[i], C(0x4830 + o)), mul(C(0x4810 + o), a[i]))))
                put(0x230 + o, add(sub(mul(x[i], C(0x4880 + o)), mul(a[i], C(0x4870 + o))),
                                   mul(h, C(0x4890 + o))))
            q0 = add(add(S(0x250), C(0x4950)), mul(mul(C(0x4930), a[0]), S(0x2C8)))
            qs = [q0, q0,
                  add(C(0x4958), mul(mul(C(0x4938), a[2]), S(0x2C8))),
                  add(C(0x495C), mul(mul(C(0x493C), a[3]), S(0x2C4)))]
            gains = [S(0x2BC), S(0x2B8), S(0x2B8), S(0x2B4)]
            for i, table_id in enumerate((0, 0, 1, 2)):
                at = 0x250 + i * 4
                target = mul(gains[i], table_lookup(self.tables[table_id], qs[i]))
                put(at, add(S(at), mul(C(0x48D0 + i * 4), sub(target, S(at)))))
            put(0x2D4, S(0x258))
            for at, source, coeff in ((0x260, S(0x2D8), m[14]), (0x264, S(0x2DC), m[15]),
                                      (0x268, S(0x254), C(0x48E8)), (0x26C, S(0x25C), C(0x48EC))):
                put(at, add(S(at), mul(sub(source, S(at)), coeff)))
            stage2 = add(mul(m[7], S(0x25C)), mul(m[3], S(0x26C)))
            tone = add(add(add(mul(m[4], S(0x2D8)), mul(m[0], S(0x260))),
                           mul(m[5], S(0x2DC))), mul(m[1], S(0x264)))
            put(0x2D0, add(add(mul(m[8], xup), mul(m[6], S(0x254))), mul(m[2], S(0x268))))
            x = [S(0x288), S(0x28C), tone, tone]
            b = [0.0] * 4
            for i in range(4):
                o = i * 4
                u = add(S(0x240 + o), mul(C(0x4740 + o), x[i]))
                if u <= C(0x4760 + o):
                    b[i], h = mul(C(0x47C0 + o), u), 0.0
                else:
                    b[i] = add(C(0x47A0 + o), mul(C(0x4780 + o), u))
                    h = add(C(0x4800 + o), mul(C(0x47E0 + o), u))
                put(0x240 + o, add(mul(C(0x4860 + o), h),
                                   sub(mul(C(0x4840 + o), x[i]), mul(C(0x4820 + o), b[i]))))
            e0, e1 = sub(b[2], S(0x270)), sub(b[3], S(0x274))
            e2, e3 = sub(stage2, S(0x278)), sub(stage2, S(0x27C))
            put(0x2D8, e2)
            combined = add(add(add(b[2], mul(m[12], e0)), b[3]), mul(m[13], e1))
            put(0x278, add(S(0x278), mul(m[10], e2)))
            put(0x27C, add(S(0x27C), mul(m[11], e3)))
            put(0x270, add(S(0x270), mul(C(0x48E0), e0)))
            put(0x274, add(S(0x274), mul(m[9], e1)))
            qs = [add(C(0x4960 + i * 4), mul(C(0x49C8),
                      add(C(0x49B0 + i * 4), mul(C(0x4940 + i * 4), add(mul(S(0x298), 0.111235954), b[i])))))
                  for i in range(2)]
            qs.extend(add(C(0x4968 + i * 4), mul(S(0x2C0), add(C(0x49B8 + i * 4),
                          mul(C(0x4948 + i * 4), combined)))) for i in range(2))
            gains = [C(0x49CC), C(0x49CC), S(0x2B0), S(0x2B0)]
            for i, table_id in enumerate((5, 5, 3, 4)):
                at = 0x280 + i * 4
                target = mul(gains[i], table_lookup(self.tables[table_id], qs[i]))
                put(at, add(S(at), mul(C(0x48F0 + i * 4), sub(target, S(at)))))
            out = mul(sub(S(0x280), S(0x284)), m[20])
            put(0x1EC - phase * 4, out)
            put(0x24C, out)
            wave = add(S(0x2E0), S(0x2E4))
            if wave >= 1.0:
                wave = sub(wave, 2.0)
            put(0x2E0, wave)
            delta = sub(S(0x2E8), add(S(0x2EC), mul(mul(S(0x2E8), wave), wave)))
            put(0x2EC, add(add(S(0x2EC), mul(C(0x49C4), add(S(0x280), S(0x284)))),
                           mul(C(0x49C0), max(delta, 0.0))))
            p, v = [S(0x290 + i * 4) for i in range(4)], [S(0x2A0 + i * 4) for i in range(4)]
            inputs = [e3, e3, S(0x2EC), S(0x2EC)]
            errors = [sub(inputs[i], add(mul(C(0x4900 + i * 4), v[i]), p[i])) for i in range(4)]
            for i in range(4):
                v[i] = add(v[i], mul(C(0x4910 + i * 4), errors[i]))
                p[i] = add(p[i], mul(C(0x4910 + i * 4), v[i]))
            put(0x2A0, *v)
            put(0x290, *p)
            combined = add(errors[0], p[0])
            put(0x2DC, add(combined, mul(m[16], sub(e3, combined))))
            supply = [S(0x2B0 + i * 4) for i in range(4)]
            targets = [mul(p[2], 0.8712), mul(supply[0], 0.85), mul(supply[1], 0.96), 0.0]
            for i in range(4):
                supply[i] = add(supply[i], mul(C(0x4920 + i * 4), sub(targets[i], supply[i])))
            put(0x2B0, *supply)
            put(0x2C0, *(div(1.0, value) for value in supply))
        # The decompiler's scalarized NEON dot is column-major, not address order.
        old = [S(0x1E0 + i * 4) for i in range(16)]
        products = [mul(old[i], DOWN[i]) for i in FIR_ORDER]
        result = products[0]
        for product in products[1:]:
            result = add(result, product)
        for i in range(12):
            put(0x1F0 + i * 4, old[i])
        # Native EXT keeps x[n] duplicated in lanes0/1; next frame overwrites lane0.
        x0, x1, x2 = S(0x1D0), S(0x1D4), S(0x1D8)
        put(0x1D0, x0, x0, x1, x2)
        assert math.isfinite(result) and all(map(math.isfinite, s)), "unstable replay"
        return result


def main():
    root = Path(__file__).resolve().parents[5]
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, default=root / "Kontakt-8-REA/Kontakt-8.arm64")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    binary = args.binary.read_bytes()
    assert hashlib.sha256(binary).hexdigest() == BINARY_SHA256
    raw_curves = [struct.unpack("<256d", address_bytes(binary, address, 256 * 8)[0]) for _, address in CURVES]
    tables = [setup_table(raw_curves[index], *(from_bits(v) for v in (a, b, c)), flag, offset)
              for _, index, a, b, c, flag, offset in SETUPS]
    tables.append(power_tube(raw_curves[5]))
    cases = [(44100.0, 0.001, (0.3, 0.4, 0.5, 0.5), False, 2048),
             (48000.0, 0.1515, (0.5, 0.5, 0.5, 0.5), False, 2048),
             (96000.0, 0.96, (0.8, 0.2, 0.9, 0.1), True, 2048)]
    blob = bytearray(b"JMPPROC1")
    blob.extend(struct.pack("<4I", len(cases), C_COUNT, 24, S_COUNT))
    meta_cases = []
    for index, (rate, drive, eq, channel, frames) in enumerate(cases):
        c, m, initial = snapshot(rate, f32(drive), tuple(map(f32, eq)), tables, channel)
        replay_case = Replay(c, m, initial, tables)
        audio = []
        for frame in range(frames):
            # Impulses, transients, multitone and silent tail exercise all histories.
            if frame < 256:
                value = (0.0, 0.25, -0.4, 0.8, -0.8)[frame % 5] if frame < 20 else 0.0
            elif frame < 1536:
                value = 0.11 * math.sin(math.tau * 173 * frame / rate) + 0.07 * math.sin(math.tau * 2401 * frame / rate)
            else:
                value = 0.0
            value = f32(value)
            audio.extend((value, replay_case.process(value)))
        blob.extend(struct.pack("<fI", rate, frames))
        for values in (c, m, initial, audio, replay_case.s):
            blob.extend(struct.pack(f"<{len(values)}f", *values))
        meta_cases.append({"rate": rate, "drive_target": drive, "eq": eq, "channel": channel,
                           "frames": frames, "peak": max(abs(v) for v in audio[1::2]),
                           "output_rms": math.sqrt(sum(v * v for v in audio[1::2]) / frames)})
    path = args.output / "jump-process.bin"
    path.write_bytes(blob)
    provenance = {
        "origin": "Independent scalar binary32 replay of saved Ghidra ProcessInt, not native execution",
        "binary_sha256": BINARY_SHA256, "fixture_sha256": hashlib.sha256(blob).hexdigest(),
        "schema": "8-byte JMPPROC1; u32 case_count,c_count=236,m_count=24,s_count=72; each case f32 host_rate,u32 frames; f32 c[236] at +4630..49dc,m[24],initial_state[72] at +1d0..2ec; frames pairs(input,expected_output); final_state[72]. Little endian.",
        "sources": {name: hashlib.sha256((root / "Kontakt-8-REA/pseudocode/functions" / name).read_bytes()).hexdigest() for name in SOURCES},
        "cases": meta_cases,
        "limitations": ["No Kontakt execution or native audio parity", "Explicit static current/coefficient snapshots; setter and automation scheduling not covered", "Scalar reciprocal division substitutes NEON estimates/refinements", "No instruction FMA contraction", "Virtual base-effect initialization/gain interactions not established"],
    }
    (args.output / "jump-process-provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(json.dumps({"path": str(path), "bytes": len(blob), "cases": meta_cases}, indent=2))


if __name__ == "__main__":
    main()
