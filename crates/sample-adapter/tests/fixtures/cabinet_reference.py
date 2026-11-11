#!/usr/bin/env python3
"""Extract the Kontakt 8 cabinet tables and build an independent reference.

Static evidence:
  s_Setups        0x1096c2f60  (11 x 0x28, first three ints used)
  s_CabFilterMap  0x104cc986c  (11 x 25 ints)
  filter rows     0x104cca294  (0x2e4-byte rows: 23 x 8 floats + level)
  BEffectCabinet::SetCabinet  0x1005c35e8
  BEffectCabinet::ProcessInt  0x1005c3b58
  ExpTable init               0x101c02f98

The Python process model is the linear 24-section cascade with the native
two-lane (even/odd) one-sample pipeline and the recovered reflection block.
It exists only to cross-check the Rust port; it is not a native audio capture.

The two delay-smoothing poles are initialized in writable BSS by a routine
that is not file-backed, so their exact values are not recoverable from this
image.  This reference uses the two provisional constants noted below and the
provenance records that limitation.
"""
from __future__ import annotations

import argparse
import hashlib
import json
import math
import struct
from pathlib import Path

BINARY_SHA256 = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"
SETUPS_ADDR = 0x1096C2F60
MAP_ADDR = 0x104CC986C
ROWS_ADDR = 0x104CCA294
ROW_STRIDE = 0x2E4
ROW_FLOATS = 185  # 23 sections x 8 coefficients + level
SECTION_COUNT = 23
FILTER_GROUPS = 11
MAP_GROUPS = 32

# Provisional: the native constants are written to __common at runtime and are
# not present in the file.  They are expressed through the native law
# clamp(k * tau / rate, 0, 1).
DELAY_KA = 600.6
DELAY_KB = 53.8


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def add(a: float, b: float) -> float:
    return f32(a + b)


def sub(a: float, b: float) -> float:
    return f32(a - b)


def mul(a: float, b: float) -> float:
    return f32(a * b)


def div(a: float, b: float) -> float:
    return f32(a / b)


def exp_map(db: float) -> float:
    """Reconstructed AB ExpTable lookup: exp2((i-1200)/60), forward slope."""
    x = add(mul(f32(db), f32(9.965784)), f32(1200.0))
    index = int(x)
    if index >= 2400:
        return f32(math.exp2(20.0))
    if index < 0:
        return 0.0
    value = f32(math.exp2((index - 1200.0) / 60.0))
    successor = f32(math.exp2((index + 1 - 1200.0) / 60.0))
    slope = f32(successor - value)
    return add(value, mul(slope, f32(x - index)))


def cdiv(ar: float, ai: float, br: float, bi: float) -> tuple[float, float]:
    den = add(mul(br, br), mul(bi, bi))
    return div(add(mul(ar, br), mul(ai, bi)), den), div(sub(mul(ai, br), mul(ar, bi)), den)


def cmul(ar: float, ai: float, br: float, bi: float) -> tuple[float, float]:
    return sub(mul(ar, br), mul(ai, bi)), add(mul(ar, bi), mul(ai, br))


class Tables:
    def __init__(self, binary: bytes):
        self.binary = binary
        self.setups = [
            struct.unpack_from("<3i", binary, self.off(SETUPS_ADDR) + i * 0x28)
            for i in range(FILTER_GROUPS)
        ]
        self.cab_map = [
            struct.unpack_from("<25i", binary, self.off(MAP_ADDR) + i * 100)
            for i in range(MAP_GROUPS)
        ]
        self.rows: list[list[float]] = []
        for cabinet, column, row in self.setups:
            index = self.cab_map[cabinet][column * 5 + row]
            row_off = self.off(ROWS_ADDR) + index * ROW_STRIDE
            self.rows.append(list(struct.unpack_from(f"<{ROW_FLOATS}f", binary, row_off)))

    @staticmethod
    def off(address: int) -> int:
        return address - 0x100000000

    def blob(self) -> bytes:
        return b"".join(struct.pack(f"<{ROW_FLOATS}f", *row) for row in self.rows)


def set_cabinet(tables: Tables, size: float, air: float, treble: float, bass: float,
                cabinet: int, rate: float):
    size, air, treble, bass = f32(size), f32(air), f32(treble), f32(bass)
    e_size = exp_map(f32(size * 6.1960783 - 3.0980392))
    e_bass = exp_map(f32(15.0 * bass))
    e_treble = exp_map(f32(15.0 * treble))
    w = mul(mul(e_size, f32(rate)), f32(0.31830987))

    sqrt_bass = f32(math.sqrt(e_bass))
    sqrt_treble = f32(math.sqrt(e_treble))
    neg349 = f32(-349.23)
    neg1975 = f32(-1975.5)
    c1 = div(add(w, mul(sqrt_bass, neg349)), sub(w, mul(sqrt_bass, neg349)))
    c3 = div(add(w, div(neg349, sqrt_bass)), sub(w, div(neg349, sqrt_bass)))
    c2 = div(add(w, div(neg1975, sqrt_treble)), sub(w, div(neg1975, sqrt_treble)))
    c4 = div(add(w, mul(sqrt_treble, neg1975)), sub(w, mul(sqrt_treble, neg1975)))
    dynamic = [f32(-add(c1, c3)), mul(c1, c3), add(c2, c4), f32(-mul(c2, c4))]

    cabinet_row = tables.rows[cabinet]
    table_sections = [cabinet_row[n * 8:(n + 1) * 8] for n in range(SECTION_COUNT)]
    level = f32(cabinet_row[SECTION_COUNT * 8])

    cab_id, _, _ = tables.setups[cabinet]
    if not ((cab_id == 8 or cab_id > 14) or cabinet > 9):
        level = f32(-level)
    if cab_id == 9 or 0x15 <= cab_id <= 0x18:
        gain_sign = f32(-level)
    else:
        gain_sign = level

    threshold = mul(w, f32(-0.0003))
    gain = gain_sign
    sections: list[list[float]] = [dynamic]
    for values in table_sections:
        (s0, s1, s2, s3, s4, s5, s6, s7) = values

        first = f32(-s4) if s4 > 0.0 else s4
        limited = first
        if first != 0.0 or s5 != 0.0:
            limited = threshold
        if first <= threshold:
            limited = first

        second = f32(-s6) if s6 > 0.0 else s6
        limited2 = second
        if limited != 0.0 or s5 != 0.0:
            limited2 = threshold
        if second <= threshold:
            limited2 = second

        d1 = cdiv(add(w, s0), s1, sub(w, s0), -s1)
        d2 = cdiv(add(w, limited), s5, sub(w, limited), -s5)
        d3 = cdiv(add(w, s2), s3, sub(w, s2), -s3)
        d4 = cdiv(add(w, limited2), s7, sub(w, limited2), -s7)

        p0 = f32(-add(d1[0], d3[0]))
        p1 = cmul(d1[0], d1[1], d3[0], d3[1])[0]
        p2 = add(d2[0], d4[0])
        p3 = f32(-cmul(d2[0], d2[1], d4[0], d4[1])[0])
        sections.append([p0, p1, p2, p3])

        b1 = (sub(w, s0), f32(-s1))
        b2 = (sub(w, limited), f32(-s5))
        b3 = (sub(w, s2), f32(-s3))
        b4 = (sub(w, limited2), f32(-s7))
        m1 = cmul(*b1, *b3)
        m2 = cmul(*b2, *b4)
        gain = mul(gain, cdiv(*m1, *m2)[0])

    norm = div(
        mul(mul(e_treble, add(w, mul(sqrt_bass, f32(349.23)))),
            add(w, div(f32(1975.5), sqrt_treble))),
        mul(add(w, div(f32(349.23), sqrt_bass)),
            add(w, mul(sqrt_treble, f32(1975.5)))),
    )
    gain = mul(gain, norm)
    gains = [gain, gain, mul(gain, mul(air, f32(0.7))), mul(gain, mul(air, f32(0.3)))]
    return sections, gains


class Cabinet:
    def __init__(self, tables: Tables, rate: float):
        self.tables = tables
        self.rate = f32(rate)
        self.period = int(self.rate * 0.05 * 0.03125 + 0.5)
        self.increment = div(f32(1.0), f32(self.period << 5))
        self.length = 1 << ((int(math.log(self.rate * 0.04501862) / math.log(2.0)) + 1) & 0x1F)
        self.mask = self.length - 1
        self.head = 0
        self.l1 = [0.0] * self.length
        self.l2 = [0.0] * self.length
        self.taps = self._taps()
        ratio = f32(6.2831855 / self.rate)
        self.ca = min(f32(DELAY_KA * ratio), 1.0)
        self.cb = min(f32(DELAY_KB * ratio), 1.0)
        self.sections = [[0.0] * 4 for _ in range(24)]
        self.gains = [0.0] * 4
        self.target_sections = [[0.0] * 4 for _ in range(24)]
        self.target_gains = [0.0] * 4
        self.increments = [0.0] * 4
        self.gain_increments = [0.0] * 4
        self.smooth_left = 0
        self.s1 = [[0.0, 0.0] for _ in range(24)]
        self.s2 = [[0.0, 0.0] for _ in range(24)]
        self.even_prev = [0.0, 0.0]
        self.da = [0.0] * 4
        self.db = [0.0] * 4

    def _taps(self) -> list[int]:
        base = self.rate * 0.015
        taps = [int(base)]
        value = base * 1.17
        taps.append(int(value))
        value *= 1.17
        third = value * 1.17
        taps.append(int(value))
        taps.append(int(third))
        value = third * 1.17
        taps.append(int(value))
        value *= 1.17
        sixth = value * 1.17
        taps.append(int(value))
        taps.append(int(sixth))
        taps.append(int(sixth * 1.17))
        return taps

    def set_params(self, size: float, air: float, treble: float, bass: float, cabinet: int):
        sections, gains = set_cabinet(self.tables, size, air, treble, bass, cabinet, self.rate)
        self.target_sections = [list(s) for s in sections]
        self.target_gains = list(gains)
        self.increments = []
        self.gain_increments = []
        for current, target in zip(self.sections[23], self.target_sections[0]):
            step = mul(sub(target, current), self.increment)
            if abs(step) * abs(step) < 1e-25:
                step = 0.0
            self.increments.append(step)
        for index, (current, target) in enumerate(zip(self.gains, self.target_gains)):
            step = mul(sub(target, current), self.increment)
            if abs(step) * abs(step) < 1e-25:
                step = 0.0
            self.gain_increments.append(step)
        self.smooth_left = self.period << 5 if any(
            v != 0.0 for v in self.increments + self.gain_increments
        ) else 0
        if self.smooth_left == 0:
            self.sections[23] = list(self.target_sections[0])
            self.gains = list(self.target_gains)
        for index in range(23):
            self.sections[index] = list(self.target_sections[index + 1])

    def _advance(self):
        if self.smooth_left == 0:
            return
        self.sections[23] = [add(a, b) for a, b in zip(self.sections[23], self.increments)]
        self.gains = [add(a, b) for a, b in zip(self.gains, self.gain_increments)]
        self.smooth_left -= 1
        if self.smooth_left == 0:
            self.sections[23] = list(self.target_sections[0])
            self.gains = list(self.target_gains)
            self.increments = [0.0] * 4
            self.gain_increments = [0.0] * 4

    def _section(self, index: int, value: list[float]) -> list[float]:
        p0, p1, p2, p3 = self.sections[index]
        out = [0.0, 0.0]
        for ch in range(2):
            y = add(value[ch], self.s1[index][ch])
            self.s1[index][ch] = add(add(mul(p0, value[ch]), mul(p2, y)), self.s2[index][ch])
            self.s2[index][ch] = add(mul(p1, value[ch]), mul(p3, y))
            out[ch] = y
        return out

    def process(self, left: float, right: float) -> tuple[float, float]:
        self._advance()
        even = [f32(left), f32(right)]
        for index in range(22, -1, -2):
            even = self._section(index, even)
        odd = self.even_prev
        for index in range(23, 0, -2):
            odd = self._section(index, odd)
        self.even_prev = even

        head = self.head
        p = add(self.l1[(self.taps[0] + head) & self.mask],
                mul(self.l1[(self.taps[4] + head) & self.mask], f32(0.5336501)))
        r = add(self.l2[(self.taps[1] + head) & self.mask],
                mul(self.l2[(self.taps[5] + head) & self.mask], f32(0.5336501)))
        q = add(mul(self.l1[(self.taps[2] + head) & self.mask], f32(-0.7305136)),
                mul(self.l1[(self.taps[6] + head) & self.mask], f32(-0.38983864)))
        s = add(mul(self.l2[(self.taps[3] + head) & self.mask], f32(-0.7305136)),
                mul(self.l2[(self.taps[7] + head) & self.mask], f32(-0.38983864)))
        reflection = (add(p, q), add(r, s))
        total = add(add(p, q), add(r, s))

        direct = (mul(self.gains[0], odd[0]), mul(self.gains[1], odd[1]))
        output = (add(direct[0], reflection[0]), add(direct[1], reflection[1]))

        delta = [
            sub(direct[0], self.da[0]),
            sub(direct[1], self.da[1]),
            sub(add(mul(self.gains[2], odd[0]), mul(f32(0.16), total)), self.da[2]),
            sub(add(mul(self.gains[3], odd[1]), mul(f32(0.16), total)), self.da[3]),
        ]
        self.da = [add(a, mul(self.ca, d)) for a, d in zip(self.da, delta)]
        self.db = [add(a, mul(self.cb, sub(d, a))) for a, d in zip(self.db, delta)]

        self.head = (head - 1) & self.mask
        self.l1[self.head] = self.db[2]
        self.l2[self.head] = self.db[3]
        return output


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path,
                        default=Path(__file__).resolve().parents[5] / "Kontakt-8-REA/Kontakt-8.arm64")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    parser.add_argument("--tables", type=Path,
                        default=Path(__file__).resolve().parents[2] / "src/cabinet_tables.bin")
    args = parser.parse_args()

    binary = args.binary.read_bytes()
    actual = hashlib.sha256(binary).hexdigest()
    if actual != BINARY_SHA256:
        raise SystemExit(f"Unexpected source binary SHA-256: {actual}")
    tables = Tables(binary)
    args.tables.write_bytes(tables.blob())

    cases = []
    for cabinet in range(11):
        for name, params in (
            ("neutral", (0.5, 0.5, 0.5, 0.5)),
            ("minimum", (0.0, 0.0, 0.0, 0.0)),
            ("maximum", (1.0, 1.0, 1.0, 1.0)),
            ("mixed", (0.25, 0.75, 0.2, 0.9)),
        ):
            cases.append((cabinet, name, params))

    coeff_path = args.output / "cabinet-coefficients.tsv"
    with coeff_path.open("w", newline="") as handle:
        handle.write("case\tcabinet\tsize\tair\ttreble\tbass\tsection\tcoefficient\tvalue\n")
        for case_id, (cabinet, name, (size, air, treble, bass)) in enumerate(cases):
            sections, gains = set_cabinet(tables, size, air, treble, bass, cabinet, 48000.0)
            for index, section in enumerate(sections):
                for coefficient, value in enumerate(section):
                    handle.write(f"{case_id}\t{cabinet}\t{size}\t{air}\t{treble}\t{bass}"
                                 f"\t{index}\t{coefficient}\t{value:.9g}\n")
            for index, value in enumerate(gains):
                handle.write(f"{case_id}\t{cabinet}\t{size}\t{air}\t{treble}\t{bass}"
                             f"\t-1\t{index}\t{value:.9g}\n")

    frames_path = args.output / "cabinet-frames.tsv"
    rates = (44100, 48000, 96000)
    with frames_path.open("w", newline="") as handle:
        handle.write("rate\tcabinet\tframe\tleft\tright\texpected_l\texpected_r\n")
        for rate in rates:
            for cabinet, size, air, treble, bass in (
                (0, 0.5, 0.5, 0.5, 0.5),
                (9, 0.25, 0.75, 0.2, 0.9),
            ):
                model = Cabinet(tables, rate)
                model.set_params(size, air, treble, bass, cabinet)
                for frame in range(256):
                    left = f32(0.25 * math.sin(2.0 * math.pi * 440.0 * frame / rate)
                               + (1.0 if frame == 0 else 0.0))
                    right = f32(0.2 * math.sin(2.0 * math.pi * 660.0 * frame / rate)
                                + (0.5 if frame == 0 else 0.0))
                    out_l, out_r = model.process(left, right)
                    handle.write(f"{rate}\t{cabinet}\t{frame}\t{left:.9g}\t{right:.9g}"
                                 f"\t{out_l:.9g}\t{out_r:.9g}\n")

    provenance = dict(
        origin="Independent Python reference from saved static Kontakt evidence; not a native-render capture",
        binary=str(args.binary), binary_sha256=actual,
        addresses=dict(setups=hex(SETUPS_ADDR), filter_map=hex(MAP_ADDR), rows=hex(ROWS_ADDR)),
        row_stride=ROW_STRIDE, row_floats=ROW_FLOATS,
        table_blob_sha256=hashlib.sha256(tables.blob()).hexdigest(),
        tables_sha256=hashlib.sha256(args.tables.read_bytes()).hexdigest(),
        coefficient_fixture_sha256=hashlib.sha256(coeff_path.read_bytes()).hexdigest(),
        frames_fixture_sha256=hashlib.sha256(frames_path.read_bytes()).hexdigest(),
        provisional=dict(
            delay_ka=DELAY_KA, delay_kb=DELAY_KB,
            note="Native __common delay-smoothing poles are not file-backed; provisional constants used.",
        ),
        evidence=dict(
            set_cabinet="Kontakt-8-REA/pseudocode/functions/0x1005c35e8.c",
            process_int="Kontakt-8-REA/pseudocode/functions/0x1005c3b58.c",
            set_sample_rate="Kontakt-8-REA/pseudocode/functions/0x1005c2dbc.c",
            exp_table_init="Kontakt-8-REA/pseudocode/functions/0x101c02f98.c",
        ),
    )
    (args.output / "cabinet_provenance.json").write_text(
        json.dumps(provenance, indent=2, sort_keys=True) + "\n"
    )


if __name__ == "__main__":
    main()
