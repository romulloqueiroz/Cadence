use super::*;

fn filter(kind: NativeFilterKind, cutoff: f32, resonance: f32) -> NativeFilter {
    NativeFilter {
        kind,
        cutoff,
        resonance,
    }
}
fn control(frame: usize) -> f32 {
    match frame {
        0..=12 => 0.2,
        13..=108 => 0.8,
        109..=162 => 0.1,
        163..=176 => 1.0,
        177..=511 => 0.1,
        _ => 0.5,
    }
}
fn audio(frame: usize) -> f32 {
    if (frame / 23) % 2 == 0 {
        0.25
    } else {
        -0.125
    }
}

#[test]
fn sv_control_and_audio_match_independent_closed_form_timeline() {
    initialize();
    let rows: Vec<Vec<f64>> = include_str!("../tests/fixtures/filter-control-responses.tsv")
        .lines()
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split_whitespace().map(|x| x.parse().unwrap()).collect())
        .collect();
    let mut max_audio_error = 0.0f64;
    let mut max_g_error = 0.0f64;
    let mut offset = 0;
    for rate in [8000.0, 44100.0, 48000.0, 96000.0, 192000.0] {
        for modulated in [false, true] {
            let f = filter(NativeFilterKind::SvLowpass1, 0.2, 0.0);
            let mut state = NativeFilterState::default();
            for frame in 0..1536 {
                let input = [audio(frame), -audio(frame) * 0.5];
                let output = state.step_inner(f, control(frame), 0.0, rate, input, modulated);
                if offset < rows.len()
                    && rows[offset][0] == rate as f64
                    && rows[offset][1] == modulated as u8 as f64
                    && rows[offset][2] == frame as f64
                {
                    let r = &rows[offset];
                    offset += 1;
                    assert_eq!(
                        state.cutoff, r[3] as f32,
                        "sampled cutoff, {rate}/{modulated}/{frame}"
                    );
                    // The last tiny-step snap can move one tick when the
                    // closed-form f64 reference meets float accumulation.
                    if !modulated || r[5] * r[5] > 4e-15 {
                        assert_eq!(
                            state.ramp_ticks, r[6] as u32,
                            "ramp ticks, {rate}/{modulated}/{frame}"
                        );
                    }
                    let g_error = (state.coeffs[0] as f64 - r[4]).abs();
                    max_g_error = max_g_error.max(g_error);
                    assert!(
                        g_error < 2e-6 * (1.0 + r[4].abs()),
                        "g at {rate}/{modulated}/{frame}: {} vs {}",
                        state.coeffs[0],
                        r[4]
                    );
                    assert!((state.delta_g as f64 - r[5]).abs() < 1e-7);
                    for ch in 0..2 {
                        let error = (output[ch] as f64 - r[7 + ch]).abs();
                        max_audio_error = max_audio_error.max(error);
                        assert!(
                            // Repeated float additions differ from closed-form
                            // f64 ramps, especially at the 8 kHz high-g corner.
                            error < 1e-5,
                            "audio at {rate}/{modulated}/{frame}: {output:?}, ref {r:?}"
                        );
                    }
                }
            }
        }
    }
    assert_eq!(offset, rows.len());
    eprintln!("FILTER_CONTROL_REFERENCE {{\"points\":{},\"max_g_error\":{max_g_error},\"max_audio_error\":{max_audio_error}}}",rows.len());
}

#[test]
fn sv_ramp_duration_retargets_and_tiny_steps_have_native_boundaries() {
    initialize();
    let f = filter(NativeFilterKind::SvLowpass1, 0.2, 0.0);
    for rate in [8000.0f32, 44100.0, 48000.0, 96000.0, 192000.0] {
        let ticks = ((rate * 0.002 / 32.0 + 0.5) as u32).max(1);
        let mut state = NativeFilterState::default();
        for _ in 0..32 {
            state.step(f, 0.2, 0.0, rate, [0.0; 2]);
        }
        let start = state.coeffs[0];
        let target = coefficients(f, 0.8, 0.0, rate)[0];
        state.step(f, 0.8, 0.0, rate, [0.0; 2]);
        assert_eq!(state.ramp_ticks, ticks);
        let increment = (target - start) * (1.0 / (ticks * 32) as f32);
        assert_eq!(state.delta_g, increment);
        assert_eq!(state.coeffs[0], start + increment);
        for _ in 1..ticks * 32 {
            state.step(f, 0.8, 0.0, rate, [0.0; 2]);
        }
        assert!((state.coeffs[0] - target).abs() < 2e-6);
        assert_ne!(state.delta_g, 0.0);
        state.step(f, 0.8, 0.0, rate, [0.0; 2]);
        assert_eq!(state.coeffs[0], target);
        assert_eq!(state.delta_g, 0.0);
        assert_eq!(state.ramp_ticks, 0);
        // Repeated modulation targets restart from the current coefficient.
        let mut repeated = NativeFilterState::default();
        for _ in 0..32 {
            repeated.step_modulated(f, 0.2, 0.0, rate, [0.0; 2]);
        }
        for _ in 0..32 {
            repeated.step_modulated(f, 0.8, 0.0, rate, [0.0; 2]);
        }
        let boundary = repeated.coeffs[0];
        repeated.step_modulated(f, 0.8, 0.0, rate, [0.0; 2]);
        let requested = (target - boundary) * (1.0 / (ticks * 32) as f32);
        if requested * requested < 1e-15 {
            assert_eq!(repeated.delta_g, 0.0);
            assert_eq!(repeated.ramp_ticks, 0);
            assert_eq!(repeated.coeffs[0], target);
        } else {
            assert_eq!(repeated.delta_g, requested);
            assert_eq!(repeated.ramp_ticks, ticks);
        }
    }
    let mut tiny = NativeFilterState::default();
    for _ in 0..32 {
        tiny.step(f, 0.2, 0.0, 48000.0, [0.0; 2]);
    }
    let target = coefficients(f, 0.20001, 0.0, 48000.0)[0];
    assert_ne!(tiny.coeffs[0], target);
    tiny.step(f, 0.20001, 0.0, 48000.0, [0.0; 2]);
    assert_eq!(tiny.delta_g, 0.0);
    assert_eq!(tiny.ramp_ticks, 0);
    assert_eq!(tiny.coeffs[0], target);
}

#[test]
fn legacy_control_slew_direction_q_lag_and_idle_clock_match_recovered_rules() {
    initialize();
    for kind in [
        NativeFilterKind::LegacyLowpass1,
        NativeFilterKind::LegacyHighpass1,
        NativeFilterKind::LegacyLowpass2,
        NativeFilterKind::LegacyHighpass2,
    ] {
        let f = filter(kind, 0.2, 0.0);
        let mut state = NativeFilterState::default();
        for _ in 0..32 {
            state.step_modulated(f, 0.2, 0.0, 48000.0, [0.0; 2]);
        }
        state.step_modulated(f, 0.9, 0.0, 48000.0, [0.0; 2]);
        assert_eq!(state.cutoff, 0.45);
        for _ in 1..32 {
            state.step_modulated(f, 0.9, 0.0, 48000.0, [0.0; 2]);
        }
        state.step_modulated(f, 0.9, 0.0, 48000.0, [0.0; 2]);
        assert_eq!(state.cutoff, 0.7);
        for _ in 1..32 {
            state.step_modulated(f, 0.9, 0.0, 48000.0, [0.0; 2]);
        }
        state.step_modulated(f, 0.1, 0.0, 48000.0, [0.0; 2]);
        assert!((state.cutoff - 0.1).abs() < 1e-7);
        // A static parameter update caps motion in both directions, while the
        // idle clock remains frozen independently of callback segmentation.
        let mut idle = NativeFilterState::default();
        for _ in 0..79 {
            idle.step(f, 0.2, 0.0, 48000.0, [0.0; 2]);
        }
        assert_eq!(idle.until_control, 0);
        idle.step(f, 0.9, 0.0, 48000.0, [0.0; 2]);
        assert_eq!(idle.cutoff, 0.45);
        let mut q = NativeFilterState::default();
        q.step(f, 0.2, 0.0, 48000.0, [0.0; 2]);
        let changed = filter(kind, 0.9, 1.0);
        q.step(changed, 0.9, 0.0, 48000.0, [0.0; 2]);
        assert_eq!(q.resonance, 0.015);
        assert_eq!(q.cutoff, 0.45);
        for _ in 1..32 {
            q.step(changed, 0.9, 0.0, 48000.0, [0.0; 2]);
        }
        q.step(changed, 0.9, 0.0, 48000.0, [0.0; 2]);
        assert!((q.resonance - (1.0 - 0.985f32.powi(2))).abs() < 1e-7);
        let expected = 0.45 + 1.0 / (0.015 * 31.0 + 4.0);
        assert!((q.cutoff - expected).abs() < 1e-7);
    }
}

#[test]
fn timing_state_recovers_on_rate_change_reset_and_model_change() {
    initialize();
    let f = filter(NativeFilterKind::SvLowpass1, 0.2, 0.0);
    let mut state = NativeFilterState::default();
    for frame in 0..80 {
        state.step_modulated(
            f,
            if frame < 13 { 0.2 } else { 0.9 },
            0.0,
            48000.0,
            [0.5, -0.25],
        );
    }
    assert_ne!(state.delta_g, 0.0);
    assert_ne!(state.left[0], 0.0);
    let mut fresh = NativeFilterState::default();
    let changed = state.step_modulated(f, 0.7, 0.0, 96000.0, [0.25, -0.5]);
    assert_eq!(
        changed,
        fresh.step_modulated(f, 0.7, 0.0, 96000.0, [0.25, -0.5])
    );
    assert_eq!(state.until_control, 31);
    assert_eq!(state.delta_g, 0.0);
    state = NativeFilterState::default();
    assert_eq!(state.step(f, 0.2, 0.0, 48000.0, [0.0; 2]), [0.0; 2]);
    let legacy = filter(NativeFilterKind::LegacyLowpass2, 0.6, 0.7);
    let mut fresh = NativeFilterState::default();
    assert_eq!(
        state.step(legacy, 0.6, 0.0, 48000.0, [1.0; 2]),
        fresh.step(legacy, 0.6, 0.0, 48000.0, [1.0; 2])
    );
}
