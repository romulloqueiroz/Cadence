use super::*;
use crate::shaper_dsp::ShaperState;
use nk_format::fx::{FxChain, ShaperKind, ShaperParams};

fn spec(kind: i32, shape: f32) -> FxSpec {
    let mut raw = shape.to_le_bytes().to_vec();
    raw.extend_from_slice(&kind.to_le_bytes());
    FxSpec {
        ser_type: 29,
        version: 0x80,
        raw,
        bypass: false,
    }
}
fn chain(specs: &[FxSpec]) -> FxChain {
    FxChain::parse(&fx_chain(specs)).unwrap()
}
fn params(kind: i32, shape: f32) -> ShaperParams {
    chain(&[spec(kind, shape)]).slots[0]
        .as_ref()
        .unwrap()
        .child
        .as_ref()
        .unwrap()
        .shaper_params()
        .unwrap()
        .unwrap()
}
fn no_heap(f: impl FnOnce()) {
    ALLOCS.with(|a| a.set(0));
    TRACK.with(|t| t.set(true));
    f();
    TRACK.with(|t| t.set(false));
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "audio/reset allocated or freed memory"
    );
}

#[test]
fn shaper_audio_matches_independent_scalar_and_binary_table_references() {
    for (kind, data) in [
        (0, include_str!("../tests/fixtures/shaper-classic.tsv")),
        (1, include_str!("../tests/fixtures/shaper-drums.tsv")),
        (2, include_str!("../tests/fixtures/shaper-enhanced.tsv")),
    ] {
        let mut state = ShaperState::default();
        let mut previous = "";
        let mut max_error = 0.0f64;
        let mut max_scaled = 0.0f64;
        let mut count = 0;
        for line in data.lines().skip(1) {
            let r: Vec<_> = line.split_whitespace().collect();
            if previous != r[2] {
                state = ShaperState::default();
                previous = r[2];
            }
            let value = |i: usize| r[i].parse::<f32>().unwrap();
            let p = ShaperParams {
                kind: match kind {
                    1 => ShaperKind::Drums,
                    2 => ShaperKind::Enhanced,
                    _ => ShaperKind::Classic,
                },
                shape: value(3),
            };
            let output = state.step(p, value(1), [value(5), value(6)]);
            for ch in 0..2 {
                let expected = value(7 + ch);
                let error = (output[ch] as f64 - expected as f64).abs();
                max_error = max_error.max(error);
                max_scaled = max_scaled.max(error / (1.0 + (expected as f64).abs()));
                assert!(
                    error < 0.00002 * (1.0 + (expected as f64).abs()),
                    "{line}, actual {output:?}, error {error}"
                );
            }
            count += 1;
        }
        eprintln!("SHAPER_REFERENCE {{\"kind\":{kind},\"stereo_frames\":{count},\"max_sample_error\":{max_error},\"max_scaled_error\":{max_scaled}}}");
    }
}

#[test]
fn shaper_records_validate_payload_and_follow_native_mode_fallback() {
    for (id, kind) in [
        (0, ShaperKind::Classic),
        (1, ShaperKind::Drums),
        (2, ShaperKind::Enhanced),
        (-1, ShaperKind::Classic),
        (i32::MAX, ShaperKind::Classic),
    ] {
        assert_eq!(params(id, 3.0), ShaperParams { kind, shape: 1.0 });
        assert_eq!(params(id, -3.0).shape, -1.0);
    }
    let valid = chain(&[spec(2, 0.5)]);
    let child = valid.slots[0].as_ref().unwrap().child.as_ref().unwrap();
    for length in 0..8 {
        let mut bad = child.clone();
        bad.raw.truncate(length);
        assert!(bad.shaper_params().is_err());
    }
    let mut bad = child.clone();
    bad.raw.push(0);
    assert!(bad.shaper_params().is_err());
    for shape in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut bad = chain(&[spec(1, shape)]);
        assert!(bad.slots[0]
            .as_ref()
            .unwrap()
            .child
            .as_ref()
            .unwrap()
            .shaper_params()
            .is_err());
        assert!(PostFx::build(&[bad.clone()], &mut |_| panic!("unexpected IR load")).is_err());
        let fixture = Fixture::new(512);
        let mut plugin = fixture.plugin(false, None);
        plugin.groups[0].fx_chain = Some(bad.clone());
        assert!(plugin.rebuild_group_fx().is_err());
        bad.slots[0].as_mut().unwrap().bypass = true;
        assert!(PostFx::build(&[bad], &mut |_| panic!("unexpected IR load"))
            .unwrap()
            .is_none());
    }
    let mut unsupported = child.clone();
    unsupported.version = 0x70;
    assert!(unsupported.shaper_params().is_err());
    let fixture = Fixture::new(512);
    let mut plugin = fixture.plugin(false, None);
    for gain in [f32::NAN, f32::INFINITY, -65.0, 65.0] {
        let mut bad = valid.clone();
        bad.slots[0].as_mut().unwrap().out_level = gain;
        assert!(PostFx::build(&[bad.clone()], &mut |_| panic!("unexpected IR load")).is_err());
        plugin.groups[0].fx_chain = Some(bad);
        assert!(plugin.rebuild_group_fx().is_err());
    }
}

#[test]
fn classic_shaper_has_native_piecewise_transfer_and_bipolar_neutral_gates() {
    // Independently evaluated branch points; the old tanh approximation fails
    // every driven case and incorrectly leaves all negative shapes unchanged.
    for (shape, x, expected) in [
        (-1.0, 0.5, 0.159_090_9),
        (-1.0, 2.0, 7.454_545_5),
        (-0.5, 0.5, 0.37704918),
        (0.0, 2.0, 2.0),
        (0.00009, 2.0, 2.0),
        (-0.0002, 2.0, 2.0), // the Process neutral window is wider below zero
        (0.125, 0.5, 0.625),
        (0.25, 0.5, 0.75),
        (0.5, 0.25, 0.75),
        (1.0, 0.125, 0.75),
        (1.0, 2.0, 1.0),
    ] {
        let mut state = ShaperState::default();
        let output = state.step(params(0, shape), 48_000.0, [x, -x]);
        assert!(
            (output[0] - expected).abs() < 2e-6,
            "{shape}/{x}: {output:?}"
        );
        assert_eq!(output[0], -output[1]);
    }
    let p = params(0, -0.0003);
    assert!(ShaperState::default().step(p, 48000.0, [2.0; 2])[0] > 3.0);
}

#[test]
fn drums_links_stereo_envelope_and_preserves_reset_bypass_and_rate_lifecycle() {
    let p = params(1, 0.7);
    let mut stereo = ShaperState::default();
    let mut mono = ShaperState::default();
    let mut difference = 0.0f32;
    for frame in 0..6000 {
        let left = if frame % 911 < 37 { 1.8 } else { 0.03 };
        let y = stereo.step(p, 48000.0, [left, 0.01]);
        let own = mono.step(p, 48000.0, [0.01; 2]);
        assert!((y[0] / left - y[1] / 0.01).abs() < 2e-6);
        difference = difference.max((y[1] - own[1]).abs());
    }
    assert!(
        difference > 0.005,
        "right detector ignored the left channel"
    );
    let mut frozen = stereo;
    for _ in 0..64 {
        assert_eq!(
            stereo.step(params(1, -1.0), 48000.0, [3.0, -2.0]),
            [3.0, -2.0]
        );
    }
    assert_eq!(
        stereo.step(p, 48000.0, [0.2, 0.1]),
        frozen.step(p, 48000.0, [0.2, 0.1])
    );
    let changed = stereo.step(p, 96000.0, [0.2, 0.1]);
    assert_eq!(changed, ShaperState::default().step(p, 96000.0, [0.2, 0.1]));
    for invalid in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(stereo.step(p, invalid, [1.0; 2]), [0.0; 2]);
    }
    assert_eq!(
        stereo.step(p, 48000.0, [0.2; 2]),
        ShaperState::default().step(p, 48000.0, [0.2; 2])
    );
    // A type change uses Reset; Enhanced's neutral is -1, Classic's is 0.
    assert_eq!(
        stereo.step(params(2, -1.0), 48000.0, [2.0, -2.0]),
        [2.0, -2.0]
    );
    assert_eq!(
        stereo.step(params(0, 0.0), 48000.0, [2.0, -2.0]),
        [2.0, -2.0]
    );
}

#[test]
fn all_shapers_route_through_inserts_aux_and_master_with_native_output_gain() {
    for kind in 0..3 {
        let p = params(kind, 0.6);
        let mut insert = chain(&[spec(kind, 0.6)]);
        insert.slots[0].as_mut().unwrap().out_level = 0.4;
        // Shaper does not prepare a dry cache; serialized dry gain is ignored.
        insert.slots[0].as_mut().unwrap().out_level_dry = 9.0;
        let mut fx = PostFx::from_chains(&[insert.clone()]).unwrap();
        let mut state = ShaperState::default();
        let mut signal = [0.3, -0.07];
        let expected = state.step(p, 48000.0, signal).map(|x| x * 0.4);
        fx.process(&mut signal, 2, 1);
        assert_eq!(signal, expected);

        let mut send_raw = 8i32.to_le_bytes().to_vec();
        for bus in 0..8 {
            send_raw.extend_from_slice(&(if bus == 7 { 0.25f32 } else { 0.0 }).to_le_bytes());
        }
        send_raw.extend_from_slice(&0i32.to_le_bytes());
        let trigger = chain(&[FxSpec {
            ser_type: 23,
            version: 0x51,
            raw: send_raw,
            bypass: false,
        }]);
        let mut sends = FxChain {
            version: insert.version,
            slots: vec![None; 8],
        };
        sends.slots[7] = insert.slots[0].clone();
        let mut master = chain(&[spec(0, 0.0)]);
        master.slots[0].as_mut().unwrap().out_level = 0.5;
        let mut fx = PostFx::from_chains(&[trigger, sends, master]).unwrap();
        let mut signal = [0.3, -0.07];
        let shaped = ShaperState::default().step(p, 48000.0, signal.map(|x| x * 0.25));
        let expected = [
            (signal[0] + shaped[0] * 0.4) * 0.5,
            (signal[1] + shaped[1] * 0.4) * 0.5,
        ];
        no_heap(|| fx.process(&mut signal, 2, 1));
        assert_eq!(signal, expected);
        no_heap(|| fx.reset());
    }
}

#[test]
fn shapers_keep_separate_voice_and_original_slot_histories() {
    let fixture = Fixture::new(10_000);
    for kind in 0..3 {
        let make = || {
            let mut plugin = fixture.plugin(false, None);
            plugin.fx = None;
            let mut c = chain(&[spec(kind, 0.7), spec(kind, -0.4)]);
            let last = c.slots[1].take();
            c.slots.resize(8, None);
            c.slots[7] = last;
            plugin.groups[0].fx_chain = Some(c);
            plugin.rebuild_group_fx().unwrap();
            plugin.preload_all();
            plugin
        };
        let mut one = make();
        let mut two = make();
        let a = advance(&mut one, 48000.0, 3000, &[Event::note_on(0, 60, 127)]);
        let b = advance(
            &mut two,
            48000.0,
            3000,
            &[Event::note_on(0, 60, 127), Event::note_on(0, 60, 127)],
        );
        for (a, b) in a.iter().zip(b) {
            assert!((a * 2.0 - b).abs() < 2e-6);
        }
        let mut dry = fixture.plugin(false, None);
        dry.fx = None;
        dry.preload_all();
        let input = advance(&mut dry, 48000.0, 3000, &[Event::note_on(0, 60, 127)]);
        assert!(a.iter().zip(input).any(|(a, b)| (a - b).abs() > 0.01));

        // Check the two slot histories against separate stereo processors.
        // Accidentally sharing one state between the slots fails this check.
        dry.reset();
        one.reset();
        let context = ProcessContext {
            block_size: 512,
            ..Default::default()
        };
        let mut input = [0.0; 1024];
        let mut actual = [0.0; 1024];
        block(
            &mut dry,
            &context,
            &mut input,
            &[Event::note_on(0, 60, 127)],
        );
        block(
            &mut one,
            &context,
            &mut actual,
            &[Event::note_on(0, 60, 127)],
        );
        let mut first = ShaperState::default();
        let mut last = ShaperState::default();
        for frame in 0..512 {
            let expected = first.step(
                params(kind, 0.7),
                48000.0,
                [input[frame], input[512 + frame]],
            );
            let expected = last.step(params(kind, -0.4), 48000.0, expected);
            for ch in 0..2 {
                assert!(
                    (actual[ch * 512 + frame] - expected[ch]).abs() < 2e-6,
                    "kind {kind}, slot-state frame {frame}, channel {ch}"
                );
            }
        }
    }
}

#[test]
fn shaper_playback_is_exact_across_callbacks_streaming_event_splits_and_reset() {
    let fixture = Fixture::new(60_000);
    let render = |kind, block_size: usize, streamed| {
        let mut plugin = fixture.plugin(false, None);
        let c = chain(&[spec(kind, 0.3), spec(kind, 0.8)]);
        plugin.groups[0].fx_chain = Some(c.clone());
        plugin.rebuild_group_fx().unwrap();
        plugin.fx = PostFx::from_chains(&[c.clone(), chain(&[]), c]);
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
        let mut output = vec![0.0; block_size * 2];
        let mut events = Vec::with_capacity(4);
        let changes = [
            (13, Event::note_on(0, 60, 127)),
            (300, Event::control_change(0, 11, 127)),
            (1900, Event::note_on(0, 64, 90)),
            (9000, Event::note_off(0, 60)),
        ];
        let mut runs = Vec::new();
        for _ in 0..2 {
            let mut result = Vec::new();
            for done in (0..10_000).step_by(block_size) {
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
                no_heap(|| block(&mut plugin, &context, &mut output[..count * 2], &events));
                for frame in 0..count {
                    result.extend_from_slice(&[output[frame], output[count + frame]]);
                }
            }
            assert_eq!(plugin.streaming_stats().underrun_frames, 0);
            no_heap(|| plugin.reset());
            runs.push(result);
        }
        assert_eq!(runs[0], runs[1], "reset retained shaper state");
        runs.remove(0)
    };
    for kind in 0..3 {
        let reference = render(kind, 64, false);
        for streamed in [false, true] {
            for block_size in [1, 64, 257, 1024] {
                assert_eq!(
                    reference,
                    render(kind, block_size, streamed),
                    "kind {kind}, block {block_size}, streamed {streamed}"
                );
            }
        }
    }
}
