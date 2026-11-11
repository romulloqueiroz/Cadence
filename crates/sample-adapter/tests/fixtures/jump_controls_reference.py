#!/usr/bin/env python3
"""Independent replay of BEffectJump::setEQFilters / setParamResponse.

Evidence:
  setEQFilters       0x1005cd29c
  setParamResponse   0x1005cac6c
  SetSampleRate      0x1005caf30 (0x4498 = 2*pi/rate, 0x4494 = 1/rate)

Static evidence only; no native audio capture.
"""
from __future__ import annotations

import math
import struct
from pathlib import Path

FIELDS = (
    "eq_b0", "eq_b1", "eq_c0", "eq_c1",
    "f_d8", "f_dc", "f_e8", "f_ec",
    "f_f0", "f_f4", "f_f8",
    "resp_18", "resp_20", "resp_24", "resp_28",
    "resp_c4", "resp_c0",
)


def f32(value: float) -> float:
    return struct.unpack("<f", struct.pack("<f", value))[0]


def add(a, b):
    return f32(a + b)


def sub(a, b):
    return f32(a - b)


def mul(a, b):
    return f32(a * b)


def div(a, b):
    return f32(a / b)


def exp_index(x: float) -> float:
    index = int(x)
    if index >= 2400:
        return f32(2.0 ** 20.0)
    if index < 0:
        return 0.0
    value = f32(math.exp2((index - 1200.0) / 60.0))
    nxt = f32(math.exp2((index + 1 - 1200.0) / 60.0))
    return add(value, mul(f32(nxt - value), f32(x - index)))


def clamp_unit(value: float) -> float:
    return 1.0 if 1.0 < value else value


def set_eq_filters(p3: float, p4: float, p5: float, presence: float, rate_scale: float):
    f3 = f32(p3)
    f5 = f32(p4)
    f9 = f32(p5)
    f6 = f32(sub(add(sub(f3, 0.3), f3), 0.3))
    f8 = 0.0 if f6 < 0.0 else f6
    f6 = 1.0 if 1.0 < f8 else f8
    f8 = add(mul(f6, -20.0), 35.0)
    f6 = add(f8, mul(sub(add(mul(f6, -24.0), 39.0), f8), f5))
    f6 = add(mul(f6, 5.0), 1381.8816)
    f8 = exp_index(f6)

    f7 = add(mul(mul(sub(presence, 1.0), 45.7245), 9.965784), 1200.0)
    f7 = exp_index(f7)

    ratio = div(f5, 0.8)
    ratio = 1.0 if 1.0 < ratio else ratio
    f10 = 0.5 if 0.5 < f3 else f3

    f12 = add(mul(f5, 0.8), 0.1)
    f12 = add(mul(f12, add(mul(f12, add(mul(f12, 0.42947), -1.1055)), 1.249)), -0.10895)

    f16 = add(mul(f3, add(mul(f3, add(mul(f3, -1511.7), 3512.3)), -2636.5)), 946.3)
    f17 = add(mul(f3, -61.350513), 87.30706)
    f6 = div(sub(f7, 0.0051733875), 0.9948266)

    eq_b0 = mul(mul(mul(f12, 1.1220185), sub(1.0, f9)), f6)
    f14 = add(mul(f3, add(mul(f3, add(mul(f3, add(mul(f3, 1.4336), -3.0688)), 1.415)), 0.75803)), 0.26767)
    f15 = add(mul(f5, add(mul(f5, add(mul(f5, add(mul(f5, 1.2364), -3.0531)), 2.3111)), -0.23654)), 0.16086)
    eq_b1 = mul(mul(f9, 0.84139514), f6)

    poly = add(mul(f3, add(mul(f3, add(mul(f3, add(mul(f3, 1.1243), -2.4493)), 1.1882)), 0.5478)), 0.40344)
    low = mul(mul(mul(sub(add(f14, mul(f5, sub(poly, f14))), f12), 1.1220185), sub(1.0, f9)), f6)
    high = mul(mul(mul(add(add(f15, mul(f3, sub(0.63095737, f15))), -1.0), 0.84139514), f9), f6)
    eq_c0 = low
    eq_c1 = high

    value = mul(add(f17, mul(f5, sub(add(mul(f3, -43.339115), 69.295654), f17))), rate_scale)
    f_d8 = clamp_unit(value)
    f_dc = clamp_unit(mul(f8, rate_scale))

    poly = add(mul(f3, add(mul(f3, add(mul(f3, -998.45), 2766.8)), -2538.9)), 947.65)
    f_e8 = clamp_unit(mul(add(f16, mul(f5, sub(poly, f16))), rate_scale))
    f_ec = clamp_unit(mul(add(mul(f10, add(mul(f10, add(mul(f10, 3140.39), -1847.82)), -507.476)), 469.976), rate_scale))

    f9 = mul(add(mul(ratio, -99.00284), 622.25397), rate_scale)
    f_f4 = 1.0 if 1.0 < f9 else f9
    a = add(mul(ratio, -0.16689), 0.42032)
    part1 = add(mul(ratio, a), 0.28409)
    b = add(mul(ratio, add(mul(ratio, add(mul(ratio, add(mul(ratio, 4.2561), -9.8707)), 8.1957)), -2.8836)), -0.23847)
    c = add(mul(ratio, add(mul(ratio, add(mul(ratio, add(mul(ratio, add(mul(ratio, 55.338), -156.56)), 161.2)), -73.88)), 14.981)), -0.47557)
    d = add(mul(ratio, add(mul(ratio, add(mul(ratio, add(mul(ratio, add(mul(ratio, -42.201), 120.15)), -124.14)), 56.677)), -11.296)), 0.57709)
    f_f0 = add(part1, mul(f3, add(b, mul(f3, add(c, mul(f3, d))))))
    f_f8 = div(1.0, add(mul(f5, -0.04), 0.22))
    return eq_b0, eq_b1, eq_c0, eq_c1, f_d8, f_dc, f_e8, f_ec, f_f0, f_f4, f_f8


def set_param_response(response: float, rate_scale: float, rate_inv: float, offset: float):
    f4 = add(mul(response, 48.0), -24.0)
    index = add(mul(f4, 5.0), 1381.8816)
    f7 = exp_index(index)
    f6 = rate_scale
    resp18 = min(f7 * f6, 1.0)

    index = add(mul(sub(f4, 56.0), 5.0), 1381.8816)
    if index >= 2400.0:
        base = f32(2.0 ** 20.0)
        resp20 = min(f6 * base, 1.0)
        resp24 = resp20
    elif index < 0.0:
        base = 0.0
        resp20 = 0.0
        resp24 = 0.0
    else:
        base = exp_index(index)
        resp20 = min(f6 * base, 1.0)
        resp24 = resp20
    resp28 = min(f6 * base, 1.0)

    index = add(mul(sub(mul(f4, 0.5), 40.0), 9.965784), 1200.0)
    resp_c4 = mul(mul(exp_index(index), -1000.0), rate_inv)
    index = add(mul(add(sub(mul(f4, 0.5), 13.0), offset), 9.965784), 1200.0)
    resp_c0 = mul(mul(exp_index(index), rate_inv), 1000.0)
    return resp18, resp20, resp24, resp28, resp_c4, resp_c0


def main():
    output = Path(__file__).resolve().parent / "jump-controls.tsv"
    rate = 48000.0
    rate_scale = f32(6.2831855 / rate)
    rate_inv = f32(1.0 / rate)
    with output.open("w") as handle:
        handle.write("kind\tcase\tfield\tvalue\n")
        cases = [
            (0.3, 0.4, 0.5, 0.5),
            (0.5, 0.5, 0.5, 0.5),
            (0.1, 0.9, 0.2, 0.8),
            (0.8, 0.2, 0.9, 0.1),
            (0.0, 0.0, 0.0, 0.0),
            (1.0, 1.0, 1.0, 1.0),
        ]
        for case, (p3, p4, p5, presence) in enumerate(cases):
            values = set_eq_filters(p3, p4, p5, presence, rate_scale)
            for field, value in zip(FIELDS[:11], values):
                handle.write(f"eq\t{case}\t{field}\t{value:.9g}\n")
        for case, response in enumerate([-0.5, 0.0, 0.25, 0.75, 1.0]):
            values = set_param_response(response, rate_scale, rate_inv, -0.0)
            for field, value in zip(FIELDS[11:], values):
                handle.write(f"resp\t{case}\t{field}\t{value:.9g}\n")


if __name__ == "__main__":
    main()
