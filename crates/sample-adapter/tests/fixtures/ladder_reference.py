"""Independent static reference for serialized ID 17, BFilterLadder_legacy.

Evidence (saved Kontakt 8.14.1 ARM64 Ghidra export):
  0x100628488 Initialize; 0x100628754 SetFilterInt;
  0x100626c7c scalar ProcessInt_sca; 0x100628500 Reset.

No Rust production code is imported. Mapping/coefficient constants use their
recovered f32 values. The unrolled audio equations below use f64 intermediates
and histories, so their result is a numerical reference, not a native audio
capture or a promise of native SIMD/f32 operation-order parity.

Linear responses solve the six-state small-signal system with saturation
replaced by its derivative at zero and denormal offsets omitted. Nonlinear
fixtures retain the saturator and offsets, but only 128 frames per case:
long resonant trajectories can amplify floating-point/phase differences.
Suggested comparison bounds: complex response abs <= 2e-4 + 2e-3*|H|;
nonlinear samples abs <= 3e-4 + 2e-5*|reference|. Re-evaluate bounds rather
than claiming native parity when a production arithmetic order differs.

Run this file to regenerate both TSVs and validate matrix residuals, the
linearization, an impulse-to-frequency cross-check, and saturation coverage.
Only Python's standard library is required.
"""
from __future__ import annotations

import cmath
import json
import math
from pathlib import Path
import struct


def f32(value: float) -> float:
    return struct.unpack('<f', struct.pack('<f', value))[0]


SAT_CURVE = f32(0.01)
DENORMAL_OFFSET = f32(1e-12)
STAGE_FEED = f32(0.3)
STAGE_SCALE = f32(0.7692308)


def coefficients(rate: float, cutoff: float, resonance: float) -> tuple[float, float, float]:
    """Return p, a and feedback from the recovered normalized parameter map."""
    cutoff = min(1.0, max(0.0, f32(cutoff)))
    resonance = min(1.0, max(0.0, f32(resonance)))
    midi = f32(f32(cutoff * f32(100.0)) + f32(20.0))
    position = f32(f32(midi * f32(5.0)) + f32(1381.8816))
    index = int(position)
    assert 0 <= index < 2400
    base = f32(2.0 ** (index / 60.0 - 20.0))
    successor = f32(2.0 ** ((index + 1) / 60.0 - 20.0))
    difference = f32(successor - base)
    hz = f32(base + f32(difference * f32(position - index)))
    angular = f32(hz * f32(f32(6.283) / f32(rate)))
    p = f32(min(f32(1.57), max(0.0, angular)) * f32(0.5))
    return p, f32(p * STAGE_SCALE), f32(resonance * f32(4.0))


def saturate(value: float) -> float:
    if abs(value) > 50.0:
        return math.copysign(25.0, value)
    return value * (1.0 - SAT_CURVE * abs(value))


def step(state: list[float], sample: float, params: tuple[float, float, float],
         *, linear: bool = False) -> tuple[list[float], float, float]:
    """Unrolled higher-precision transcription of the original scalar equations.

    State order: previous input, stage 1..4 state, previous feedback.
    Return new state, native output (updated stage-4 state), maximum raw stage.
    """
    p, a, feedback = params
    previous, z1, z2, z3, z4, previous_feedback = state
    beta = f32(p - 1.0)
    epsilon = 0.0 if linear else DENORMAL_OFFSET
    curve = (lambda value: value) if linear else saturate

    d1 = a * ((sample + previous) * 0.5 - previous_feedback)
    v1 = z1 + d1
    a1 = -(v1 * beta) + d1 * STAGE_FEED + epsilon
    d2 = a * curve(v1)
    v2 = z2 + d2
    a2 = -(v2 * beta) + d2 * STAGE_FEED + epsilon
    d3 = a * curve(v2)
    v3 = z3 + d3
    a3 = -(v3 * beta) + d3 * STAGE_FEED + epsilon
    d4 = a * curve(v3)
    v4 = z4 + d4
    a4 = -(v4 * beta) + d4 * STAGE_FEED + epsilon

    d5 = a * (sample - (feedback * curve(v4) + epsilon))
    v5 = a1 + d5
    b1 = -(v5 * beta) + d5 * STAGE_FEED + epsilon
    d6 = a * curve(v5)
    v6 = a2 + d6
    b2 = -(v6 * beta) + d6 * STAGE_FEED + epsilon
    d7 = a * curve(v6)
    v7 = a3 + d7
    b3 = -(v7 * beta) + d7 * STAGE_FEED + epsilon
    d8 = a * curve(v7)
    v8 = a4 + d8
    b4 = -(v8 * beta) + d8 * STAGE_FEED + epsilon

    updated_feedback = feedback * curve(v8) + epsilon
    updated = [sample, b1, b2, b3, b4, updated_feedback]
    return updated, b4, max(abs(v) for v in (v1, v2, v3, v4, v5, v6, v7, v8))


def state_matrix(params: tuple[float, float, float]) -> tuple[list[list[float]], list[float]]:
    """Recover A and B by applying basis states to the linearized equations."""
    columns = []
    for column in range(6):
        basis = [float(i == column) for i in range(6)]
        columns.append(step(basis, 0.0, params, linear=True)[0])
    matrix = [[columns[col][row] for col in range(6)] for row in range(6)]
    forcing = step([0.0] * 6, 1.0, params, linear=True)[0]
    # Independent linearity check, away from the basis probes.
    state = [0.17, -0.23, 0.41, -0.07, 0.31, -0.13]
    actual = step(state, -0.29, params, linear=True)[0]
    predicted = [sum(matrix[row][col] * state[col] for col in range(6))
                 - 0.29 * forcing[row] for row in range(6)]
    assert max(abs(a - b) for a, b in zip(actual, predicted)) < 5e-14
    return matrix, forcing


def solve(matrix: list[list[complex]], forcing: list[complex]) -> list[complex]:
    """Small complex Gaussian elimination with partial pivoting."""
    size = len(forcing)
    work = [list(row) + [value] for row, value in zip(matrix, forcing)]
    for column in range(size):
        pivot = max(range(column, size), key=lambda row: abs(work[row][column]))
        work[column], work[pivot] = work[pivot], work[column]
        divisor = work[column][column]
        assert abs(divisor) > 1e-15, (column, divisor)
        work[column] = [value / divisor for value in work[column]]
        for row in range(size):
            if row != column:
                multiple = work[row][column]
                work[row] = [a - multiple * b for a, b in zip(work[row], work[column])]
    answer = [row[-1] for row in work]
    residual = max(abs(sum(a * x for a, x in zip(row, answer)) - value)
                   for row, value in zip(matrix, forcing))
    assert residual < 1e-10, residual
    return answer


def response(params: tuple[float, float, float], fraction: float) -> complex:
    matrix, forcing = state_matrix(params)
    delay = cmath.exp(-2j * math.pi * fraction)
    # The output is state[4] AFTER the current frame's update:
    # new_state = A*(delay*new_state) + B*input.
    system = [[complex(row == col) - delay * matrix[row][col]
               for col in range(6)] for row in range(6)]
    return solve(system, [complex(value) for value in forcing])[4]


def signal(frame: int, level: float) -> tuple[float, float]:
    left = 1.0 if frame % 32 < 16 else -1.0
    if frame in (0, 64):
        left += 0.75
    right = 0.625 if frame % 20 < 7 else -0.375
    if frame in (7, 83):
        right += 1.5
    return f32(level * left), f32(level * right)


def generate() -> dict[str, float | int]:
    destination = Path(__file__).parent
    linear_rows = [
        '# Static small-signal reference: six-state solve, no denormal offset or saturation.',
        '# serialized_id sample_rate cutoff resonance frequency_hz response_real response_imag',
    ]
    fractions = [0.001, 0.03, 0.2, 0.45]
    max_impulse_error = 0.0
    linear_cases = 0
    for rate in (8000, 48000, 96000, 192000):
        for cutoff in (0.1, 0.5, 0.9, 1.0):
            for resonance in (0.0, 0.5):
                params = coefficients(rate, cutoff, resonance)
                expected = [response(params, fraction) for fraction in fractions]
                # Cross-check the matrix solve using an independent impulse sum.
                # Selected resonances are below the self-oscillation boundary.
                state = [0.0] * 6
                phases = [1 + 0j] * 4
                rotations = [cmath.exp(-2j * math.pi * f) for f in fractions]
                measured = [0j] * 4
                for frame in range(65536):
                    state, value, _ = step(state, float(frame == 0), params, linear=True)
                    assert math.isfinite(value) and abs(value) < 100.0
                    for i in range(4):
                        measured[i] += value * phases[i]
                        phases[i] *= rotations[i]
                error = max(abs(a - b) for a, b in zip(expected, measured))
                max_impulse_error = max(max_impulse_error, error)
                assert error < 2e-5, (rate, cutoff, resonance, error)
                for fraction, value in zip(fractions, expected):
                    linear_rows.append(f'17 {rate} {cutoff:.9g} {resonance:.9g} '
                                       f'{rate * fraction:.12g} {value.real:.14g} {value.imag:.14g}')
                linear_cases += 1
    (destination / 'ladder-linear-responses.tsv').write_text('\n'.join(linear_rows) + '\n')

    nonlinear_rows = [
        '# f64 state/intermediate reference; native f32 coefficient constants; offsets retained.',
        '# case_id frame sample_rate cutoff resonance input_left input_right output_left output_right',
    ]
    case = 0
    max_raw_stage = 0.0
    max_output = 0.0
    for rate in (8000, 48000, 96000, 192000):
        for cutoff in (0.1, 0.5, 1.0):
            for resonance in (0.0, 0.5, 1.0):
                for level in (0.25, 80.0):
                    params = coefficients(rate, cutoff, resonance)
                    states = [[0.0] * 6, [0.0] * 6]
                    for frame in range(128):
                        inputs = signal(frame, level)
                        outputs = []
                        for channel in range(2):
                            states[channel], value, peak = step(states[channel], inputs[channel], params)
                            assert math.isfinite(value) and abs(value) < 1000.0
                            max_raw_stage = max(max_raw_stage, peak)
                            max_output = max(max_output, abs(value))
                            outputs.append(value)
                        nonlinear_rows.append(f'{case} {frame} {rate} {cutoff:.9g} {resonance:.9g} '
                                              f'{inputs[0]:.12g} {inputs[1]:.12g} '
                                              f'{outputs[0]:.14g} {outputs[1]:.14g}')
                    case += 1
    assert max_raw_stage > 50.0, 'fixture must enter the saturator clamp branch'
    assert len(nonlinear_rows) - 2 <= 15000
    (destination / 'ladder-nonlinear-frames.tsv').write_text('\n'.join(nonlinear_rows) + '\n')
    return {'linear_cases': linear_cases, 'linear_response_rows': len(linear_rows) - 2,
            'nonlinear_cases': case, 'nonlinear_frame_rows': len(nonlinear_rows) - 2,
            'max_matrix_vs_impulse_error': max_impulse_error,
            'max_raw_stage': max_raw_stage, 'max_nonlinear_output': max_output}


if __name__ == '__main__':
    print(json.dumps(generate(), indent=2))
