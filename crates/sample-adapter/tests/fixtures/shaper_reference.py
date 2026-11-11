#!/usr/bin/env python3
"""Independent, static-evidence Shaper reference; requires only Python stdlib.

Uses saved Kontakt pseudocode and shipped f32 tables, never Cadence DSP code.
All arithmetic helpers round to IEEE binary32 after each operation. This models
the scalar expressions; compiler FMA contraction and native audio parity are
not established by these fixtures. See provenance.json for evidence and hashes.
"""
from __future__ import annotations

import argparse
import csv
import ctypes
import hashlib
import json
import math
import struct
from collections import Counter
from pathlib import Path


def f32(value):
    return struct.unpack("<f", struct.pack("<f", value))[0]


def add(a, b):
    return f32(a + b)


def sub(a, b):
    return f32(a - b)


def mul(a, b):
    return f32(a * b)


def div(a, b):
    return f32(a / b)


def lerp(a, b, fraction):
    return add(a, mul(sub(b, a), fraction))


LIBC = ctypes.CDLL(None)
LIBC.exp2f.argtypes = [ctypes.c_float]
LIBC.exp2f.restype = ctypes.c_float
EPSILON = f32(1e-12)
SHAPE_EPSILON = f32(0.0001)
BYPASS_THRESHOLD = f32(-0.9999)
TABLE_LAYOUT = (
    ("compress_curves", 0x9933300, 51 * 128),
    ("compress_gain", 0x9939900, 128),
    ("drive_amount", 0x9939B00, 128),
    ("drive_mix", 0x9939D00, 128),
    ("drive_gain", 0x9939F00, 128),
)
BINARY_SHA256 = "42a3216b527e8eaaa2b53122acb9f11cdc1db5fbe9b7571777f362b5cc8d4ec3"
RATES = (44100, 48000, 96000)
SHAPES = (-1.0, -0.9998, -0.6, 0.0, 0.4, 1.0)
KINDS = ("classic", "drums", "enhanced")


def coefficient(rate, time_ms):
    duration = sub(mul(mul(f32(rate), f32(0.001)), f32(time_ms)), 1.0)
    exponent = div(f32(-6.643856), max(1.0, duration))
    return sub(1.0, LIBC.exp2f(exponent))


def smooth(old, goal, coefficient_value):
    # Native source explicitly adds then subtracts the denormal offset.
    return sub(add(add(old, mul(coefficient_value, sub(goal, old))), EPSILON), EPSILON)


class Reference:
    def __init__(self, kind, rate, shape, tables):
        self.kind, self.tables = kind, tables
        self.shape_coeff = coefficient(rate, 50.0)
        self.gain_coeff = coefficient(rate, f32(2.67874))
        self.release_coeff = coefficient(rate, f32(25.4764))
        self.envelope, self.gain = 1.0, 1.0
        self.current = 0.0
        self.set_shape(shape)
        # Reset establishes current position at the initial shape's target.
        self.current = self.target

    def set_shape(self, shape):
        self.shape = max(-1.0, min(1.0, f32(shape)))
        self.half = mul(self.shape, 0.5)
        self.target = mul(mul(add(self.shape, 1.0), 0.5),
                          50.0 if self.kind == "drums" else 127.0)
        self.dirty = True

    def position(self):
        if self.dirty:
            delta = mul(sub(self.target, self.current), self.shape_coeff)
            if abs(delta) >= SHAPE_EPSILON:
                self.current = add(self.current, delta)
            else:
                self.current, self.dirty = self.target, False
        index = int(self.current)
        return index, sub(self.current, f32(index))

    def classic(self, x):
        r = mul(add(self.half, 0.5), 8.0)
        if abs(self.shape) <= SHAPE_EPSILON or f32(3.999) < r < f32(4.0001):
            return x
        if r < 4.0:
            a = add(mul(r, f32(0.975)), f32(0.1))
            reciprocal = div(1.0, add(a, 1.0))
            return mul(reciprocal, add(mul(a, x), mul(mul(x, x), x)))
        amount = sub(r, 4.0)
        magnitude = min(abs(mul(amount, x) if r >= 5.0 else x), 1.0)
        result = sub(mul(magnitude, 2.0), mul(magnitude, magnitude))
        if x < 0.0:
            result = -result
        if r < 5.0:
            return add(mul(amount, result), mul(sub(1.0, amount), x))
        return add(result, mul(x, 0.0))

    def process(self, left, right):
        if self.kind == "classic":
            return self.classic(left), self.classic(right)
        if self.shape <= BYPASS_THRESHOLD:
            # Native bypass copies input without advancing position/env/gain.
            return left, right
        if self.kind == "enhanced":
            i, frac = self.position()
            def lookup(name):
                values = self.tables[name]
                return lerp(values[i], values[min(i + 1, 127)], frac)
            amount, mix, correction = (lookup(name) for name in
                                      ("drive_amount", "drive_mix", "drive_gain"))
            def channel(x):
                driven = max(-8.0, min(8.0, mul(x, amount)))
                curved = mul(mul(correction, driven), add(mul(abs(driven), -0.0625), 1.0))
                return lerp(x, curved, mix)
            return channel(left), channel(right)
        peak = max(abs(left), abs(right))
        self.envelope = (smooth(self.envelope, peak, self.release_coeff)
                         if peak < self.envelope else peak)
        level = min(add(self.envelope, f32(1e-5)), 1.0)
        column = mul(level, 127.0)
        j, column_fraction = int(column), sub(column, f32(int(column)))
        i, row_fraction = self.position()
        def row(r):
            values = self.tables["compress_curves"]
            r = min(r, 50)
            return lerp(values[r * 128 + j], values[r * 128 + min(j + 1, 127)],
                        column_fraction)
        curve = lerp(row(i), row(i + 1), row_fraction)
        self.gain = smooth(self.gain, div(curve, level), self.gain_coeff)
        correction_values = self.tables["compress_gain"]
        correction = lerp(correction_values[i], correction_values[i + 1], row_fraction)
        output_gain = mul(self.gain, correction)
        return mul(output_gain, left), mul(output_gain, right)


def signal(frame, rate):
    """Deterministic stereo transient/silence/high-level stress sequence."""
    phase = frame % 192
    if 64 <= phase < 112:
        return 0.0, 0.0
    amplitudes = (0.001, 0.05, 0.25, 0.9, 1.0, 1.2, 2.0, 4.0, 12.0)
    amplitude = amplitudes[(frame // 8) % len(amplitudes)]
    left = amplitude * math.sin(2.0 * math.pi * 997.0 * frame / rate + 0.31)
    right = amplitude * 0.73 * math.sin(2.0 * math.pi * 613.0 * frame / rate - 0.79)
    if phase in (0, 31, 112, 160):
        left = (1.4 if phase == 112 else 0.7) * (-1.0 if phase == 31 else 1.0)
    if phase in (16, 48, 128, 176):
        right = -4.0 if phase == 128 else 0.85
    return f32(left), f32(right)


def sequences():
    for kind in KINDS:
        for rate in RATES:
            for shape in SHAPES:
                yield kind, rate, f"{kind}-{rate}-fixed-{shape}", [shape] * 192, False
            transition = (-0.6, 1.0, 0.4, -1.0, -1.0, 0.4,
                          -0.9998, 0.0, -0.6, 1.0, -1.0, 1.0)
            yield kind, rate, f"{kind}-{rate}-transitions", [s for s in transition for _ in range(64)], False
    # Gate boundaries include the wider negative Classic neutral window and
    # the r=5 crossover at shape .25. These are scalar stereo transfer sweeps.
    boundaries = (-0.00026, -0.000251, -0.00025, -0.000249,
                  -0.000101, -0.0001, -0.000099, 0.0000249, 0.000025,
                  0.000026, 0.000099, 0.0001, 0.000101, 0.2499, 0.25, 0.2501)
    for rate in RATES:
        for shape in boundaries:
            yield "classic", rate, f"classic-{rate}-boundary-{shape}", [shape] * 64, True


def sha256(value):
    return hashlib.sha256(value).hexdigest()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path,
                        default=Path(__file__).resolve().parents[5] / "Kontakt-8-REA/Kontakt-8.arm64")
    parser.add_argument("--output", type=Path, default=Path(__file__).resolve().parent)
    args = parser.parse_args()
    binary = args.binary.read_bytes()
    actual_hash = sha256(binary)
    if actual_hash != BINARY_SHA256:
        raise SystemExit(f"Unexpected source binary SHA-256: {actual_hash}")
    tables, table_provenance = {}, {}
    for name, offset, count in TABLE_LAYOUT:
        raw = binary[offset:offset + count * 4]
        tables[name] = struct.unpack(f"<{count}f", raw)
        table_provenance[name] = dict(file_offset=hex(offset), f32_count=count, sha256=sha256(raw))
    args.output.mkdir(parents=True, exist_ok=True)
    counters, sequence_counts = Counter(), Counter()
    paths = {kind: args.output / f"{kind}.tsv" for kind in KINDS}
    handles = {kind: path.open("w", newline="") for kind, path in paths.items()}
    writers = {kind: csv.writer(handle, delimiter="\t", lineterminator="\n")
               for kind, handle in handles.items()}
    for writer in writers.values():
        writer.writerow(("kind", "rate", "sequence_id", "shape", "frame", "left", "right",
                         "expected_l", "expected_r"))
    try:
        for kind, rate, sequence_id, shapes, scalar_sweep in sequences():
            reference = Reference(kind, rate, shapes[0], tables)
            previous = f32(shapes[0])
            sequence_counts[kind] += 1
            for frame, shape in enumerate(shapes):
                shape = f32(shape)
                if shape != previous:
                    reference.set_shape(shape)
                    previous = shape
                if scalar_sweep:
                    left = f32(-4.0 + 8.0 * frame / 63.0)
                    right = f32(-left * 0.271)
                else:
                    left, right = signal(frame, rate)
                expected_l, expected_r = reference.process(left, right)
                assert math.isfinite(expected_l) and math.isfinite(expected_r)
                floats = (shape, left, right, expected_l, expected_r)
                encoded = [format(value, ".9g") for value in floats]
                assert all(f32(float(value)) == original for value, original in zip(encoded, floats))
                writers[kind].writerow((kind, rate, sequence_id, encoded[0], frame, *encoded[1:]))
                counters[kind] += 1
    finally:
        for handle in handles.values():
            handle.close()
    provenance = dict(
        origin="Independent Python scalar reconstruction from saved static Kontakt evidence; not a native-render capture",
        binary=str(args.binary), binary_sha256=actual_hash,
        table_blob_sha256=sha256(binary[0x9933300:0x993A100]), tables=table_provenance,
        rates=list(RATES), fixed_shapes=list(SHAPES), frame_counts=dict(counters),
        sequence_counts=dict(sequence_counts), total_frames=sum(counters.values()),
        generator_sha256=sha256(Path(__file__).read_bytes()),
        tsv_sha256={kind: sha256(path.read_bytes()) for kind, path in paths.items()},
        evidence={
            "classic": "Kontakt-8-REA/pseudocode/functions/0x1005c0940.c:117-440",
            "enhanced": "Kontakt-8-REA/pseudocode/functions/0x1005c1120.c:79-121",
            "drums": "Kontakt-8-REA/pseudocode/functions/0x1005c13ec.c:43-150",
            "controls": "Kontakt-8-REA/pseudocode/functions/0x10040cefc.c:145-177",
            "initialize": "Kontakt-8-REA/pseudocode/functions/0x10060114c.c:24-54",
            "reset": "Kontakt-8-REA/pseudocode/functions/0x100601338.c",
        },
        semantics=[
            "Reset at each new sequence_id: initial current position equals initial target; gain=1 and envelope=1.",
            "A shape change preserves envelope/gain/current position, sets dirty, and recalculates target.",
            "Enhanced/Drums bypass shape <= f32(-.9999) copies audio and freezes all state.",
            "Classic abs(shape)<=f32(.0001), or f32(3.999)<r<f32(4.0001), returns input.",
            "Native endpoint zero-weight out-of-range neighbor reads replaced with clamped grid/table indices.",
            "All operations rounded binary32; exp2f called through platform libc; no Kontakt runtime invoked.",
            "Output columns use 9 significant digits, verified to round-trip exactly to binary32.",
        ],
        limits=[
            "Static-derived independent fixtures do not establish native audio or bitwise parity.",
            "No SIMD/FMA contraction, control modulation lanes, wet/dry volume smoothing, or serialized automation timing emulated.",
            "Fixtures model two-channel effect core with finite inputs; host mixing is outside scope.",
        ],
    )
    (args.output / "provenance.json").write_text(json.dumps(provenance, indent=2) + "\n")
    print(json.dumps({"frames": dict(counters), "sequences": dict(sequence_counts),
                      "total_frames": sum(counters.values())}, indent=2))


if __name__ == "__main__":
    main()
