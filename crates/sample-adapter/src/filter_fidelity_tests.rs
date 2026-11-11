use super::filter_dsp::NativeFilterState;
use super::*;
use nk_format::fx::{FxChain, FxChild, NativeFilter, NativeFilterKind};

fn spec(fix: i32, cutoff: f32, resonance: f32) -> FxSpec {
    let raw = [
        fix.to_le_bytes(),
        fix.to_le_bytes(),
        cutoff.to_le_bytes(),
        resonance.to_le_bytes(),
    ]
    .concat();
    FxSpec {
        ser_type: 24,
        version: 0x92,
        raw,
        bypass: false,
    }
}
fn parsed(fix: i32, cutoff: f32, resonance: f32) -> NativeFilter {
    let chain = FxChain::parse(&fx_chain(&[spec(fix, cutoff, resonance)])).unwrap();
    chain.slots[0]
        .as_ref()
        .unwrap()
        .child
        .as_ref()
        .unwrap()
        .native_filter()
        .unwrap()
        .unwrap()
}

#[test]
fn normalized_filter_records_are_strict_and_keep_the_family() {
    for fix in [2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 17, 50, 51] {
        let original = spec(fix, 0.4, 0.7);
        let child = |raw: Vec<u8>| FxChild {
            ser_type: 24,
            version: 0x92,
            private: vec![],
            public: vec![],
            children: vec![],
            raw,
        };
        let filter = child(original.raw.clone())
            .native_filter()
            .unwrap()
            .unwrap();
        assert_eq!(filter.cutoff, 0.4);
        assert_eq!(filter.resonance, 0.7);
        for end in 0..16 {
            assert!(child(original.raw[..end].to_vec()).native_filter().is_err());
        }
        let mut raw = original.raw.clone();
        raw.push(0);
        assert!(child(raw).native_filter().is_err());
        let mut raw = original.raw.clone();
        raw[4..8].copy_from_slice(&99i32.to_le_bytes());
        assert!(child(raw).native_filter().unwrap().is_none());
        let mut raw = original.raw.clone();
        raw[..4].copy_from_slice(&99i32.to_le_bytes());
        assert_eq!(child(raw).native_filter().unwrap(), Some(filter));
        for offset in [8, 12] {
            let mut raw = original.raw.clone();
            raw[offset..offset + 4].copy_from_slice(&f32::NAN.to_le_bytes());
            assert!(child(raw).native_filter().is_err());
        }
        let bounded = parsed(fix, -3.0, 4.0);
        assert_eq!(bounded.cutoff, 0.0);
        assert_eq!(bounded.resonance, 1.0);
    }
    assert_eq!(parsed(2, 0.0, 0.0).kind, NativeFilterKind::LegacyLowpass1);
    assert_eq!(parsed(50, 0.0, 0.0).kind, NativeFilterKind::SvLowpass1);
}

#[test]
fn recovered_filter_impulses_match_independent_complex_transfer_reference() {
    filter_dsp::initialize();
    let rows: Vec<Vec<f64>> = include_str!("../tests/fixtures/filter-responses.tsv")
        .lines()
        .filter(|s| !s.starts_with('#'))
        .map(|s| s.split_whitespace().map(|n| n.parse().unwrap()).collect())
        .collect();
    let mut max_error = 0.0f64;
    for case in rows.chunks(4) {
        let row = &case[0];
        let filter = parsed(row[0] as i32, row[2] as f32, row[3] as f32);
        let mut state = NativeFilterState::default();
        let mut response = [[0.0f64; 2]; 4];
        let mut phase = [[1.0f64, 0.0]; 4];
        let rotations: Vec<_> = case
            .iter()
            .map(|r| {
                let w = -std::f64::consts::TAU * r[4] / r[1];
                [w.cos(), w.sin()]
            })
            .collect();
        for frame in 0..65_536 {
            let output = state.step(
                filter,
                filter.cutoff,
                0.0,
                row[1] as f32,
                [if frame == 0 { 1.0 } else { 0.0 }, 0.0],
            );
            assert!(output[0].is_finite(), "nonfinite case {row:?}");
            assert_eq!(output[1], 0.0, "stereo state leak");
            for i in 0..4 {
                response[i][0] += output[0] as f64 * phase[i][0];
                response[i][1] += output[0] as f64 * phase[i][1];
                phase[i] = [
                    phase[i][0] * rotations[i][0] - phase[i][1] * rotations[i][1],
                    phase[i][0] * rotations[i][1] + phase[i][1] * rotations[i][0],
                ];
            }
        }
        for (actual, r) in response.iter().zip(case) {
            let error = ((actual[0] - r[5]).powi(2) + (actual[1] - r[6]).powi(2)).sqrt();
            max_error = max_error.max(error);
            let magnitude = (r[5] * r[5] + r[6] * r[6]).sqrt();
            // Subtracting nearly equal float feedback sums to normalize the
            // lowest-cutoff legacy two-pole filter is ill-conditioned. Keep
            // its absolute tolerance separate from all ordinary controls.
            let low_cutoff_two_pole = matches!(r[0] as i32, 6 | 7) && r[2] == 0.0;
            let tolerance = if low_cutoff_two_pole {
                0.02
            } else {
                0.0001 + 0.001 * magnitude
            };
            assert!(error < tolerance, "case {r:?}: {actual:?}, error {error}");
        }
    }
    eprintln!(
        "FILTER_REFERENCE {{\"responses\":{},\"max_complex_error\":{max_error}}}",
        rows.len()
    );
}

#[test]
fn sv_one_pole_retains_integrator_state_when_cutoff_changes() {
    filter_dsp::initialize();
    for fix in [50, 51] {
        let filter = parsed(fix, 0.2, 0.0);
        let mut state = NativeFilterState::default();
        let g0 = filter_dsp::coefficients(filter, 0.2, 0.0, 48_000.0)[0] as f64;
        let g1 = filter_dsp::coefficients(filter, 0.8, 0.0, 48_000.0)[0] as f64;
        // Closed-form integrator charge after an impulse followed by silence.
        let s = g0 / (1.0 + g0) * ((1.0 - g0) / (1.0 + g0)).powi(30);
        for frame in 0..31 {
            state.step(
                filter,
                0.2,
                0.0,
                48_000.0,
                [if frame == 0 { 1.0 } else { 0.0 }, 0.0],
            );
        }
        let y = state.step(filter, 0.8, 0.0, 48_000.0, [0.0, 0.0])[0] as f64;
        // Frame 31 precedes the next 32-frame control tick. At frame 32,
        // the first coefficient increment starts a 96-frame ramp at 48 kHz.
        let sign = if fix == 51 { -1.0 } else { 1.0 };
        let expected = 2.0 * s / (1.0 + g0) * sign;
        assert!((y - expected).abs() < 1e-7, "{y} != {expected}");
        let y = state.step(filter, 0.8, 0.0, 48_000.0, [0.0, 0.0])[0] as f64;
        let first_g = g0 + (g1 - g0) / 96.0;
        let expected = 2.0 * s * (1.0 - g0) / (1.0 + g0) / (1.0 + first_g) * sign;
        assert!((y - expected).abs() < 1e-7, "{y} != {expected}");
    }
}

#[test]
fn native_filters_work_in_program_slots_with_wet_dry_reset_and_rate_changes() {
    for fix in [2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 17, 50, 51] {
        let filter = parsed(fix, 0.4, 0.7);
        let mut chain = FxChain::parse(&fx_chain(&[spec(fix, 0.4, 0.7)])).unwrap();
        chain.slots[0].as_mut().unwrap().out_level = 0.6;
        chain.slots[0].as_mut().unwrap().out_level_dry = 0.25;
        let mut fx = PostFx::from_chains(&[chain]).unwrap();
        for rate in [48_000.0, 96_000.0, 44_100.0] {
            fx.prepare(rate);
            fx.reset();
            let mut reference = NativeFilterState::default();
            for frame in 0..1024 {
                let input = [
                    if frame == 0 { 1.0 } else { 0.0 },
                    if frame == 7 { 0.5 } else { 0.0 },
                ];
                let expected = reference.step(filter, filter.cutoff, 0.0, rate, input);
                let mut output = input;
                fx.process(&mut output, 2, 1);
                for i in 0..2 {
                    assert!((output[i] - (expected[i] * 0.6 + input[i] * 0.25)).abs() < 1e-7);
                }
            }
        }
        ALLOCS.with(|c| c.set(0));
        TRACK.with(|c| c.set(true));
        fx.reset();
        TRACK.with(|c| c.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0);
        let mut silence = [0.0; 2];
        fx.process(&mut silence, 2, 1);
        if fix == 17 {
            assert!(silence.iter().all(|x| x.abs() < 1e-7));
        } else {
            assert_eq!(silence, [0.0; 2]);
        }
        for role in [1, 2] {
            let mut chains = vec![FxChain::parse(&fx_chain(&[])).unwrap(); 3];
            chains[role] = FxChain::parse(&fx_chain(&[spec(fix, 0.4, 0.7)])).unwrap();
            let mut fx = PostFx::from_chains(&chains).unwrap();
            let mut reference = NativeFilterState::default();
            for frame in 0..128 {
                let input = [if frame == 0 { 1.0 } else { 0.0 }, 0.0];
                let expected = reference.step(filter, filter.cutoff, 0.0, 48_000.0, input);
                let mut output = if role == 1 { [0.0; 2] } else { input };
                let mut aux = [[[0.0; 2]; 8]];
                if role == 1 {
                    aux[0][0] = input;
                }
                fx.process_range(&mut output, 2, 1, 0, &mut aux);
                assert_eq!(output, expected, "fix {fix}, role {role}");
            }
        }
    }
}

#[test]
fn native_filter_sweeps_are_block_streaming_and_allocation_independent() {
    use nk_format::external_mod::{ExternalModulator, Target};
    let fixture = Fixture::new(60_000);
    let render = |block_size: usize, streamed: bool| {
        let mut plugin = fixture.plugin(false, None);
        plugin.fx = None;
        plugin.groups[0].fx_chain = Some(
            FxChain::parse(&fx_chain(&[
                spec(2, 0.8, 0.0),
                spec(3, 0.1, 0.0),
                spec(6, 0.9, 0.7),
                spec(7, 0.1, 0.7),
                spec(50, 0.6, 0.0),
                spec(51, 0.1, 0.0),
            ]))
            .unwrap(),
        );
        plugin.groups[0].external_modulators = vec![ExternalModulator {
            source: 4,
            cc_number: 11,
            targets: vec![Target {
                name: "filterCutoff".into(),
                intensity: 0.25,
                flags: 16,
                smoothing: 10,
                target_obj_idx: Some(4),
                ..Default::default()
            }],
            ..Default::default()
        }];
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        plugin.rebuild_group_fx().unwrap();
        if streamed {
            plugin
                .stream_all(StreamingConfig {
                    preload_frames: 8192,
                    ..Default::default()
                })
                .unwrap();
            plugin.set_offline_rendering(true);
        } else {
            plugin.preload_all();
        }
        let mut context = ProcessContext {
            block_size,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut all = Vec::new();
        let mut output = vec![0.0; block_size * 2];
        let mut events = Vec::with_capacity(3);
        let changes = [
            (13, Event::note_on(0, 60, 127)),
            (300, Event::control_change(0, 11, 127)),
            (9000, Event::note_off(0, 60)),
        ];
        let mut done = 0;
        while done < 10_000 {
            let count = block_size.min(10_000 - done);
            context.block_size = count;
            context.sample_pos = done as i64;
            events.clear();
            for (frame, mut event) in changes {
                if frame >= done && frame < done + count {
                    event.frame = frame - done;
                    events.push(event);
                }
            }
            ALLOCS.with(|c| c.set(0));
            TRACK.with(|c| c.set(true));
            block(&mut plugin, &context, &mut output[..count * 2], &events);
            TRACK.with(|c| c.set(false));
            assert_eq!(ALLOCS.with(Cell::get), 0);
            all.extend_from_slice(&output[..count]);
            all.extend_from_slice(&output[count..count * 2]);
            done += count;
        }
        // Convert callback-planar audio to a stable interleaved comparison.
        let mut interleaved = Vec::new();
        let mut done = 0;
        let mut offset = 0;
        while done < 10_000 {
            let count = block_size.min(10_000 - done);
            for i in 0..count {
                interleaved.push(all[offset + i]);
                interleaved.push(all[offset + count + i]);
            }
            offset += count * 2;
            done += count;
        }
        assert!(interleaved.iter().all(|v| v.is_finite()));
        assert!(interleaved.iter().any(|v| v.abs() > 1e-5));
        assert_eq!(plugin.streaming_stats().underrun_frames, 0);
        if streamed {
            assert!(plugin.streaming_stats().pages_read > 0);
        }
        ALLOCS.with(|c| c.set(0));
        TRACK.with(|c| c.set(true));
        plugin.reset();
        TRACK.with(|c| c.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0);
        interleaved
    };
    let reference = render(64, false);
    for streamed in [false, true] {
        for block_size in [1, 64, 257, 1024] {
            assert_eq!(
                reference,
                render(block_size, streamed),
                "block {block_size}, streamed {streamed}"
            );
        }
    }
}

#[test]
fn native_filters_stay_finite_at_extremes_and_recover_after_invalid_rate() {
    filter_dsp::initialize();
    for fix in [2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 17, 50, 51] {
        for rate in [8000.0, 192_000.0] {
            for cutoff in [0.0, 1.0] {
                for resonance in [0.0, 1.0] {
                    let filter = parsed(fix, cutoff, resonance);
                    let mut state = NativeFilterState::default();
                    for i in 0..40_000 {
                        let y = state.step(
                            filter,
                            cutoff,
                            0.0,
                            rate,
                            [if i == 0 { 1.0 } else { 0.0 }, 0.0],
                        );
                        assert!(
                            y[0].is_finite() && y[0].abs() < 100.0,
                            "fix {fix}, {rate}/{cutoff}/{resonance}: {y:?}"
                        );
                    }
                    for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
                        assert_eq!(state.step(filter, cutoff, 0.0, invalid, [1.0; 2]), [0.0; 2]);
                    }
                    let recovered = state.step(filter, cutoff, 0.0, rate, [1.0; 2]);
                    let mut fresh = NativeFilterState::default();
                    assert_eq!(recovered, fresh.step(filter, cutoff, 0.0, rate, [1.0; 2]));
                    for octaves in [-16.0, 16.0] {
                        let mut extreme = NativeFilterState::default();
                        for _ in 0..64 {
                            let y = extreme.step(filter, cutoff, octaves, rate, [0.1, -0.1]);
                            assert!(
                                y.iter().all(|v| v.is_finite()),
                                "fix {fix}, octaves {octaves}"
                            );
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn group_cutoff_buffers_keep_the_voice_clock_and_refresh_sv_targets() {
    use nk_format::external_mod::{ExternalModulator, Target};
    let fixture = Fixture::new(10_000);
    let context = ProcessContext {
        block_size: 512,
        ..Default::default()
    };
    let mut dry = fixture.plugin(false, None);
    dry.fx = None;
    dry.preload_all();
    let mut input = [0.0; 1024];
    block(
        &mut dry,
        &context,
        &mut input,
        &[Event::note_on(13, 60, 127)],
    );
    for fix in [2, 3, 4, 6, 7, 8, 9, 10, 11, 12, 17, 50, 51] {
        let mut plugin = fixture.plugin(false, None);
        plugin.fx = None;
        plugin.groups[0].fx_chain =
            Some(FxChain::parse(&fx_chain(&[spec(fix, 0.2, 0.0)])).unwrap());
        plugin.groups[0].external_modulators = vec![ExternalModulator {
            source: 4,
            cc_number: 11,
            targets: vec![Target {
                name: "filterCutoff".into(),
                intensity: 0.6,
                flags: 16,
                target_obj_idx: Some(0),
                ..Default::default()
            }],
            ..Default::default()
        }];
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        plugin.rebuild_group_fx().unwrap();
        plugin.preload_all();
        assert!(matches!(
            plugin.group_fx[0][0],
            GroupFxStage::NativeFilter {
                modulated: true,
                ..
            }
        ));
        let mut output = [0.0; 1024];
        block(
            &mut plugin,
            &context,
            &mut output,
            &[
                Event::note_on(13, 60, 127),
                Event::control_change(300, 11, 127),
            ],
        );
        let f = parsed(fix, 0.2, 0.0);
        let mut state = NativeFilterState::default();
        let mut static_state = NativeFilterState::default();
        let mut refresh_difference = 0f32;
        for frame in 13..512 {
            let cutoff = if frame < 300 { 0.2 } else { 0.8 };
            let expected =
                state.step_modulated(f, cutoff, 0.0, 48000.0, [input[frame], input[512 + frame]]);
            let scalar =
                static_state.step(f, cutoff, 0.0, 48000.0, [input[frame], input[512 + frame]]);
            for ch in 0..2 {
                assert!(
                    (output[ch * 512 + frame] - expected[ch]).abs() < 2e-6,
                    "fix {fix}, frame {frame}, ch {ch}"
                );
            }
            refresh_difference = refresh_difference.max((expected[0] - scalar[0]).abs());
        }
        assert!(
            refresh_difference > 1e-5,
            "fix {fix}: routed target behaved like an unbound scalar"
        );
    }
}

#[test]
fn higher_and_ladder_routed_playback_is_exact_across_blocks_streaming_and_reset() {
    use nk_format::external_mod::{ExternalModulator, Target};
    let fixture = Fixture::new(60_000);
    let render = |fix: i32, block_size: usize, streamed: bool| {
        let mut plugin = fixture.plugin(false, None);
        plugin.fx = None;
        let mut chain = FxChain::parse(&fx_chain(&[spec(fix, 0.35, 0.7)])).unwrap();
        let slot = chain.slots.remove(0);
        chain.slots = vec![None; 8];
        chain.slots[7] = slot;
        plugin.groups[0].fx_chain = Some(chain);
        plugin.groups[0].external_modulators = vec![ExternalModulator {
            source: 4,
            cc_number: 11,
            targets: vec![Target {
                name: "filterCutoff".into(),
                intensity: 0.35,
                flags: 16,
                smoothing: 10,
                target_obj_idx: Some(7),
                ..Default::default()
            }],
            ..Default::default()
        }];
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        plugin.rebuild_group_fx().unwrap();
        if streamed {
            plugin
                .stream_all(StreamingConfig {
                    preload_frames: 8192,
                    ..Default::default()
                })
                .unwrap();
            plugin.set_offline_rendering(true);
        } else {
            plugin.preload_all();
        }
        let mut context = ProcessContext {
            block_size,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut result = Vec::new();
        let mut output = vec![0.0; block_size * 2];
        let mut events = Vec::with_capacity(4);
        let changes = [
            (13, Event::note_on(0, 60, 127)),
            (300, Event::control_change(0, 11, 127)),
            (5000, Event::control_change(0, 11, 0)),
            (9000, Event::note_off(0, 60)),
        ];
        let mut done = 0;
        while done < 10_000 {
            let count = block_size.min(10_000 - done);
            context.block_size = count;
            context.sample_pos = done as i64;
            events.clear();
            for (frame, mut event) in changes {
                if frame >= done && frame < done + count {
                    event.frame = frame - done;
                    events.push(event);
                }
            }
            ALLOCS.with(|c| c.set(0));
            TRACK.with(|c| c.set(true));
            block(&mut plugin, &context, &mut output[..count * 2], &events);
            TRACK.with(|c| c.set(false));
            assert_eq!(ALLOCS.with(Cell::get), 0, "fix {fix}");
            for i in 0..count {
                result.push(output[i]);
                result.push(output[count + i]);
            }
            done += count;
        }
        assert!(result.iter().all(|x| x.is_finite()));
        assert!(result.iter().any(|x| x.abs() > 1e-5));
        assert_eq!(plugin.streaming_stats().underrun_frames, 0);
        if streamed {
            assert!(plugin.streaming_stats().pages_read > 0);
        }
        ALLOCS.with(|c| c.set(0));
        TRACK.with(|c| c.set(true));
        plugin.reset();
        TRACK.with(|c| c.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0);
        result
    };
    for fix in [4, 8, 9, 10, 11, 12, 17] {
        let reference = render(fix, 64, false);
        for streamed in [false, true] {
            for size in [1, 64, 257, 1024] {
                assert_eq!(
                    reference,
                    render(fix, size, streamed),
                    "fix {fix}, block {size}, streamed {streamed}"
                );
            }
        }
    }
}
