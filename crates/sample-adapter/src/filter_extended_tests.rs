use super::*;
use nk_format::fx::NativeFilterKind::*;

type Complex = [f64; 2];
fn add(a: Complex, b: Complex) -> Complex {
    [a[0] + b[0], a[1] + b[1]]
}
fn mul(a: Complex, b: Complex) -> Complex {
    [a[0] * b[0] - a[1] * b[1], a[0] * b[1] + a[1] * b[0]]
}
fn div(a: Complex, b: Complex) -> Complex {
    let d = b[0] * b[0] + b[1] * b[1];
    [
        (a[0] * b[0] + a[1] * b[1]) / d,
        (a[1] * b[0] - a[0] * b[1]) / d,
    ]
}
fn norm(a: Complex) -> f64 {
    (a[0] * a[0] + a[1] * a[1]).sqrt()
}
fn rows(text: &str) -> Vec<Vec<f64>> {
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split_whitespace().map(|s| s.parse().unwrap()).collect())
        .collect()
}
fn kind(fix: i32) -> NativeFilterKind {
    match fix {
        4 => LegacyBandpass2,
        8 => LegacyLowpass4,
        9 => LegacyHighpass4,
        10 => LegacyBandpass4,
        11 => LegacyBandreject4,
        12 => LegacyLowpass6,
        17 => LegacyLadder,
        _ => panic!("unexpected fixture ID"),
    }
}
fn filter(fix: i32, cutoff: f32, resonance: f32) -> NativeFilter {
    NativeFilter {
        kind: kind(fix),
        cutoff,
        resonance,
    }
}
fn response(state: &NativeFilterState, fix: i32, rate: f64, freq: f64) -> Complex {
    let angle = -std::f64::consts::TAU * freq / rate;
    let z = [angle.cos(), angle.sin()];
    let z2 = mul(z, z);
    let mut output = if fix == 11 { [0.0, 0.0] } else { [1.0, 0.0] };
    let count = if fix == 12 { 3 } else { 2 };
    for c in state.extended.sections.iter().take(count) {
        let numerator = add(add([c[0], 0.0], mul([c[1], 0.0], z)), mul([c[2], 0.0], z2));
        let denominator = add(add([1.0, 0.0], mul([c[3], 0.0], z)), mul([c[4], 0.0], z2));
        let stage = div(numerator, denominator);
        output = if fix == 11 {
            add(output, stage)
        } else {
            mul(output, stage)
        };
    }
    mul(output, [state.extended.output as f64, 0.0])
}

#[test]
fn higher_order_coefficients_match_independent_prototype_response() {
    initialize();
    let data = rows(include_str!(
        "../tests/fixtures/higher-filter-responses.tsv"
    ));
    let mut max_error = 0.0f64;
    let mut ordinary_max = 0.0f64;
    let mut worst_case = Vec::new();
    let mut worst_actual = [0.0; 2];
    for r in &data {
        let fix = r[0] as i32;
        let f = filter(fix, r[2] as f32, r[3] as f32);
        let mut state = NativeFilterState::default();
        state.step(f, f.cutoff, 0.0, r[1] as f32, [0.0; 2]);
        let actual = response(&state, fix, r[1], r[4]);
        let expected = [r[5], r[6]];
        let error = norm([actual[0] - expected[0], actual[1] - expected[1]]);
        if error > max_error {
            max_error = error;
            worst_case = r.clone();
            worst_actual = actual;
        }
        let corner = r[2] == 0.0 && matches!(fix, 8 | 9 | 10 | 12);
        if !corner {
            ordinary_max = ordinary_max.max(error);
        }
        let bound = if corner {
            0.05 + 0.05 * norm(expected)
        } else {
            0.001 + 0.003 * norm(expected)
        };
        assert!(
            error < bound,
            "fix {fix}, case {r:?}: actual {actual:?}, error {error}"
        );
    }
    eprintln!("HIGHER_FILTER_REFERENCE {{\"responses\":{},\"max_complex_error\":{max_error},\"ordinary_max_complex_error\":{ordinary_max},\"worst_case\":{worst_case:?},\"worst_actual\":{worst_actual:?}}}",data.len());
}

#[test]
fn higher_filter_audio_and_compression_match_independent_polynomial_simulation() {
    initialize();
    let data = rows(include_str!(
        "../tests/fixtures/higher-filter-nonlinear-frames.tsv"
    ));
    let mut max_error = 0.0f64;
    let mut max_relative = 0.0f64;
    let mut state = NativeFilterState::default();
    for r in &data {
        if r[5] == 0.0 {
            state = NativeFilterState::default();
        }
        let f = filter(r[0] as i32, r[2] as f32, r[3] as f32);
        let x = if (r[5] as usize) % 31 < 13 {
            r[4] as f32
        } else {
            -r[4] as f32 * 0.375
        };
        let output = state.step(f, f.cutoff, 0.0, r[1] as f32, [x, -x * 0.5]);
        for ch in 0..2 {
            let error = (output[ch] as f64 - r[6 + ch]).abs();
            max_error = max_error.max(error);
            max_relative = max_relative.max(error / (1.0 + r[6 + ch].abs()));
            assert!(
                error < 0.001 * (1.0 + r[6 + ch].abs()),
                "case {r:?}, channel {ch}: {output:?}, error {error}"
            );
        }
    }
    eprintln!("HIGHER_FILTER_NONLINEAR {{\"stereo_frames\":{},\"max_sample_error\":{max_error},\"max_scaled_error\":{max_relative}}}",data.len());
}

#[test]
fn higher_filter_impulse_audio_has_the_declared_transfer_and_stereo_isolation() {
    initialize();
    let data = rows(include_str!(
        "../tests/fixtures/higher-filter-responses.tsv"
    ));
    for case in data
        .chunks(4)
        .filter(|c| c[0][2] == 0.4 && c[0][1] <= 96000.0 && (c[0][3] == 0.7 || c[0][0] == 4.0))
    {
        let r = &case[0];
        let f = filter(r[0] as i32, r[2] as f32, r[3] as f32);
        let mut state = NativeFilterState::default();
        let mut sums = [[0.0f64; 2]; 4];
        let mut phases = [[1.0f64, 0.0]; 4];
        let rotations: Vec<_> = case
            .iter()
            .map(|r| {
                let w = -std::f64::consts::TAU * r[4] / r[1];
                [w.cos(), w.sin()]
            })
            .collect();
        let mut peak = 0.0f32;
        for frame in 0..65536 {
            let y = state.step(
                f,
                f.cutoff,
                0.0,
                r[1] as f32,
                [if frame == 0 { 1e-4 } else { 0.0 }, 0.0],
            );
            assert_eq!(y[1], 0.0);
            assert!(y[0].is_finite());
            peak = peak.max(y[0].abs());
            for i in 0..4 {
                sums[i] = add(sums[i], mul([y[0] as f64 / 1e-4, 0.0], phases[i]));
                phases[i] = mul(phases[i], rotations[i]);
            }
        }
        assert!(
            peak < 0.1,
            "reference impulse triggered nonlinear compression"
        );
        for (actual, r) in sums.iter().zip(case) {
            let error = norm([actual[0] - r[5], actual[1] - r[6]]);
            assert!(
                error < 0.001 + 0.003 * norm([r[5], r[6]]),
                "audio fix {}, case {r:?}: {actual:?}, error {error}",
                r[0]
            );
        }
    }
}

#[test]
fn legacy_ladder_audio_matches_independent_unrolled_reference() {
    initialize();
    let data = rows(include_str!(
        "../tests/fixtures/ladder-nonlinear-frames.tsv"
    ));
    let mut state = NativeFilterState::default();
    let mut max_error = 0.0f64;
    let mut max_scaled = 0.0f64;
    for r in &data {
        if r[1] == 0.0 {
            state = NativeFilterState::default();
        }
        let f = filter(17, r[3] as f32, r[4] as f32);
        let actual = state.step(f, f.cutoff, 0.0, r[2] as f32, [r[5] as f32, r[6] as f32]);
        for ch in 0..2 {
            let error = (actual[ch] as f64 - r[7 + ch]).abs();
            max_error = max_error.max(error);
            max_scaled = max_scaled.max(error / (1.0 + r[7 + ch].abs()));
            assert!(
                error < 3e-4 + 2e-5 * r[7 + ch].abs(),
                "ladder case {r:?}: {actual:?}, error {error}"
            );
        }
    }
    eprintln!("LADDER_NONLINEAR_REFERENCE {{\"stereo_frames\":{},\"max_sample_error\":{max_error},\"max_scaled_error\":{max_scaled}}}",data.len());
}

#[test]
fn legacy_ladder_small_signal_impulse_matches_independent_state_space_response() {
    initialize();
    let data = rows(include_str!(
        "../tests/fixtures/ladder-linear-responses.tsv"
    ));
    let mut max_error = 0.0f64;
    for case in data.chunks(4) {
        let r = &case[0];
        let f = filter(17, r[2] as f32, r[3] as f32);
        let mut state = NativeFilterState::default();
        let mut sums = [[0.0f64; 2]; 4];
        let mut phases = [[1.0f64, 0.0]; 4];
        let rotations: Vec<_> = case
            .iter()
            .map(|r| {
                let w = -std::f64::consts::TAU * r[4] / r[1];
                [w.cos(), w.sin()]
            })
            .collect();
        for frame in 0..65536 {
            let y = state.step(
                f,
                f.cutoff,
                0.0,
                r[1] as f32,
                [if frame == 0 { 1e-4 } else { 0.0 }, 0.0],
            );
            assert!(
                y[1].abs() < 1e-7,
                "ladder stereo leak beyond native denormal floor"
            );
            assert!(y[0].is_finite());
            for i in 0..4 {
                // The linearized matrix omits the native denormal offsets.
                // Subtract the independently evolving silent-channel floor.
                sums[i] = add(sums[i], mul([(y[0] - y[1]) as f64 / 1e-4, 0.0], phases[i]));
                phases[i] = mul(phases[i], rotations[i]);
            }
        }
        for (actual, r) in sums.iter().zip(case) {
            let error = norm([actual[0] - r[5], actual[1] - r[6]]);
            max_error = max_error.max(error);
            assert!(
                error < 0.001 + 0.002 * norm([r[5], r[6]]),
                "ladder response {r:?}: {actual:?}, error {error}"
            );
        }
    }
    eprintln!(
        "LADDER_LINEAR_REFERENCE {{\"responses\":{},\"max_complex_error\":{max_error}}}",
        data.len()
    );
}

#[test]
fn higher_and_ladder_extremes_modulation_reset_and_rate_changes_stay_finite() {
    initialize();
    for fix in [4, 8, 9, 10, 11, 12, 17] {
        for rate in [8000.0, 192000.0] {
            for cutoff in [0.0, 1.0] {
                for resonance in [0.0, 1.0] {
                    let f = filter(fix, cutoff, resonance);
                    let mut state = NativeFilterState::default();
                    for frame in 0..40000 {
                        let input = if frame == 0 {
                            16.0
                        } else if frame % 17 == 0 {
                            -4.0
                        } else {
                            0.0
                        };
                        let y = state.step(f, cutoff, 0.0, rate, [input, 0.0]);
                        assert!(y.iter().all(|x|x.is_finite()&&x.abs()<1e6),"fix {fix}, rate {rate}, cutoff {cutoff}, res {resonance}, frame {frame}: {y:?}");
                    }
                    for frame in 0..2000 {
                        let target = if frame % 257 < 128 { 0.0 } else { 1.0 };
                        let octave = if frame % 193 < 96 { -16.0 } else { 16.0 };
                        let y = state.step_modulated(f, target, octave, rate, [0.25, -0.5]);
                        assert!(y.iter().all(|x| x.is_finite()));
                    }
                    state = NativeFilterState::default();
                    let mut fresh = NativeFilterState::default();
                    assert_eq!(
                        state.step(f, cutoff, 0.0, rate, [1.0; 2]),
                        fresh.step(f, cutoff, 0.0, rate, [1.0; 2])
                    );
                    let y = state.step(f, cutoff, 0.0, 48000.0, [0.0; 2]);
                    assert!(y.iter().all(|x| x.is_finite()));
                }
            }
        }
    }
}
