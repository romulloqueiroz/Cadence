use super::*;
use nk_format::{
    external_mod::{CurvePoint, ExternalModulator, Target, Transformation},
    fx::FxChain,
};

fn source(source: u32, name: &str, depth: f32, object: Option<i8>) -> ExternalModulator {
    ExternalModulator {
        source,
        targets: vec![Target {
            name: name.into(),
            intensity: depth,
            flags: 16,
            target_obj_idx: object,
            ..Default::default()
        }],
        ..Default::default()
    }
}

fn filter(fix: i32, cutoff: f32, bypass: bool) -> FxSpec {
    let mut raw = fix.to_le_bytes().to_vec();
    raw.extend_from_slice(&fix.to_le_bytes());
    raw.extend_from_slice(&cutoff.to_le_bytes());
    raw.extend_from_slice(&0f32.to_le_bytes());
    FxSpec {
        ser_type: 0x18,
        version: 0x92,
        raw,
        bypass,
    }
}

fn sparse_filters(hp_cutoff: f32) -> FxChain {
    let mut chain = FxChain::parse(&fx_chain(&[
        filter(6, 0.5, false),
        filter(50, 0.6, true),
        filter(7, hp_cutoff, false),
    ]))
    .unwrap();
    let slots = std::mem::take(&mut chain.slots);
    chain.slots = vec![None; 8];
    chain.slots[1] = slots[1].clone(); // bypassed, must retain its own address
    chain.slots[3] = slots[0].clone();
    chain.slots[7] = slots[2].clone();
    chain
}

fn configured(fixture: &Fixture, cutoff: f32, routes: Vec<ExternalModulator>) -> SamplerPlugin {
    let mut plugin = fixture.plugin(false, None);
    plugin.fx = None;
    plugin.groups[0].fx_chain = Some(sparse_filters(cutoff));
    plugin.groups[0].external_modulators = routes;
    plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
    plugin.rebuild_group_fx().unwrap();
    plugin
}

#[test]
fn key_position_uses_octaves_and_note_coordinates_for_tables_and_curves() {
    let fixture = Fixture::new(1024);
    let mut group = fixture.plugin(false, None).groups.remove(0);
    for transform in [
        Transformation::None,
        Transformation::Table {
            active: true,
            values: Box::new(std::array::from_fn(|i| i as f32 / 127.0)),
        },
        Transformation::Curve {
            active: true,
            points: vec![
                CurvePoint {
                    x: 0.0,
                    y: 0.0,
                    curvature: 0.0,
                },
                CurvePoint {
                    x: 1.0,
                    y: 1.0,
                    curvature: 0.0,
                },
            ],
        },
    ] {
        for invert in [false, true] {
            let mut route = source(7, "pitch", 0.25, None);
            route.targets[0].transform = transform.clone();
            route.targets[0].flags |= if invert { 2 } else { 0 };
            group.external_modulators = vec![route];
            let mut modulation = Modulation::new(std::slice::from_ref(&group), 2).unwrap();
            for (slot, note) in [0, 48, 60, 72, 127].into_iter().enumerate() {
                let signal = if matches!(transform, Transformation::None) {
                    (note as f32 - 60.0) / 12.0
                } else {
                    2.0 * note as f32 / 127.0 - 1.0
                };
                let actual = modulation.evaluate(slot % 2, 0, 0, note, 127, None).octaves;
                let expected = signal * if invert { -0.25 } else { 0.25 };
                assert!(
                    (actual - expected).abs() < 1e-6,
                    "note {note}: {actual} != {expected}"
                );
            }
        }
    }
}

#[test]
fn key_position_volume_cubes_only_its_own_attenuation() {
    let fixture = Fixture::new(1024);
    let mut group = fixture.plugin(false, None).groups.remove(0);
    let mut cc = source(4, "volume", 1.0, None);
    cc.cc_number = 11;
    group.external_modulators = vec![source(7, "volume", 0.25, None), cc];
    let mut modulation = Modulation::new(&[group], 1).unwrap();
    modulation.controller(0, 11, 64);
    for note in [48, 60, 72] {
        // Bipolar raw key position maps to 0/0.5/1 at these three notes.
        let key_level = (note as f32 - 48.0) / 24.0;
        let expected = (1.0 - (1.0 - key_level) * 0.25).powi(3) * 64.0 / 127.0;
        let actual = modulation.evaluate(0, 0, 0, note, 127, None).gain;
        assert!((actual - expected).abs() < 1e-6);
    }
}

#[test]
fn indexed_cutoff_changes_only_the_original_filter_slot() {
    let fixture = Fixture::new(10_000);
    let routes = vec![source(7, "filterCutoff", 0.2, Some(7))];
    let mut routed = configured(&fixture, 0.2, routes);
    // At note 72 the source is +1, so slot 7 must move from 0.2 to 0.4.
    // Slot 3 is unchanged, and the bypassed slot 1 does not renumber either.
    let mut reference = configured(&fixture, 0.4, vec![]);
    let mut dry = configured(&fixture, 0.2, vec![]);
    for plugin in [&mut routed, &mut reference, &mut dry] {
        plugin.preload_all();
    }
    let events = [Event::note_on(13, 72, 127)];
    let actual = advance(&mut routed, 48_000.0, 2_000, &events);
    let expected = advance(&mut reference, 48_000.0, 2_000, &events);
    let unmodulated = advance(&mut dry, 48_000.0, 2_000, &events);
    let error = actual
        .iter()
        .zip(&expected)
        .map(|(a, b)| (a - b).abs())
        .fold(0f32, f32::max);
    assert!(error < 2e-6, "slot-specific audio error {error}");
    assert!(actual
        .iter()
        .zip(unmodulated)
        .any(|(a, b)| (a - b).abs() > 0.01));
    let value = routed.expression.evaluate(0, 0, 0, 72, 127, None);
    assert_eq!(value.filter_octaves, 0.0);
    assert_eq!(value.filter_slots[7], 0.2);
    assert!(value.filter_slots[..7].iter().all(|v| *v == 0.0));
}

#[test]
fn invalid_indexed_cutoff_routes_fail_and_bypassed_slots_do_not_leak() {
    let fixture = Fixture::new(1024);
    let plugin = configured(&fixture, 0.2, vec![]);
    let mut group = plugin.groups[0].clone();
    for index in [-128, -2, 0, 8, 127] {
        group.external_modulators = vec![source(7, "filterCutoff", 1.0, Some(index))];
        assert!(
            Modulation::new(std::slice::from_ref(&group), 1).is_err(),
            "accepted {index}"
        );
    }
    // A bypassed but valid filter is still a legal address, not slot 3.
    group.external_modulators = vec![source(7, "filterCutoff", 1.0, Some(1))];
    let mut modulation = Modulation::new(&[group], 1).unwrap();
    let output = modulation.evaluate(0, 0, 0, 72, 127, None);
    assert_eq!(output.filter_slots[1], 1.0);
    assert_eq!(output.filter_slots[3], 0.0);
    assert_eq!(output.filter_slots[7], 0.0);
}

#[test]
fn indexed_cutoff_is_exact_across_blocks_and_streaming_and_allocates_nothing() {
    let fixture = Fixture::new(60_000);
    let render = |block_size: usize, streamed: bool| {
        let mut cc = source(4, "filterCutoff", 0.15, Some(7));
        cc.cc_number = 11;
        cc.targets[0].smoothing = 10;
        let mut plugin = configured(
            &fixture,
            0.2,
            vec![source(7, "filterCutoff", 0.05, Some(7)), cc],
        );
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
            (13, Event::note_on(0, 72, 127)),
            (300, Event::control_change(0, 11, 127)),
            (9000, Event::note_off(0, 72)),
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
            done += count;
        }
        if streamed {
            assert!(plugin.streaming_stats().pages_read > 0);
        }
        assert_eq!(plugin.streaming_stats().underrun_frames, 0);
        ALLOCS.with(|c| c.set(0));
        TRACK.with(|c| c.set(true));
        plugin.reset();
        TRACK.with(|c| c.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0);
        all
    };
    let reference = render(64, false);
    for streamed in [false, true] {
        for block in [64, 257] {
            let output = render(block, streamed);
            let first = reference.iter().zip(&output).position(|(a, b)| a != b);
            assert!(
                first.is_none(),
                "first differing frame {first:?}, block {block}, streamed {streamed}"
            );
        }
    }
}

#[test]
fn bilinear_one_pole_reference_has_first_order_frequency_response() {
    for (fix, highpass) in [(50, false), (51, true)] {
        let chain = FxChain::parse(&fx_chain(&[filter(fix, 0.5, false)])).unwrap();
        let filter = chain.simple_filters()[0];
        assert_eq!(filter.poles, 1);
        assert_eq!(
            filter.shape,
            if highpass {
                nk_format::fx::FilterShape::Highpass
            } else {
                nk_format::fx::FilterShape::Lowpass
            }
        );
        let c = biquad_coeffs(filter.shape, filter.poles, 1000.0, 0.707, 0.0, 48_000.0);
        for frequency in [100.0f32, 1000.0, 10_000.0] {
            let mut state = [0.0; 2];
            let mut sum = 0f64;
            for frame in 0..48_000 {
                let input = (std::f32::consts::TAU * frequency * frame as f32 / 48_000.0).sin();
                let output = biquad_step(input, &mut state, &c);
                if frame >= 24_000 {
                    sum += (output as f64).powi(2);
                }
            }
            let warped = (std::f64::consts::PI * frequency as f64 / 48_000.0).tan()
                / (std::f64::consts::PI * 1000.0 / 48_000.0).tan();
            let expected = if highpass { warped } else { 1.0 } / (1.0 + warped * warped).sqrt();
            let actual = (2.0 * sum / 24_000.0).sqrt();
            assert!(
                (actual - expected).abs() < 2e-4,
                "fix {fix}, {frequency} Hz: {actual} != {expected}"
            );
        }
    }
}
