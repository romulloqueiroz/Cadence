//! Group amps run per voice around a physical slot split. Native evidence:
//! HandleVoiceEfx 0x1003aa3a8 selects [0,split) or [split,8), and ProcessVoice
//! 0x10046772c calls those ranges on opposite sides of voice gain/pan.
use super::*;
use nk_format::fx::{AmpKind, FxChain};

const MODELS: [AmpKind; 8] = [
    AmpKind::Jump,
    AmpKind::Van51,
    AmpKind::AcBox,
    AmpKind::HotSolo,
    AmpKind::Cat,
    AmpKind::BassPro,
    AmpKind::BassInvader,
    AmpKind::EpPreamps,
];
const FRAMES: usize = 193;
fn amp(kind: AmpKind) -> FxSpec {
    let (ser_type, version, values, flags) = match kind {
        AmpKind::Jump => (77, 0x10, 6, 2),
        AmpKind::Van51 => (82, 0x10, 8, 5),
        AmpKind::AcBox => (83, 0x10, 7, 1),
        AmpKind::HotSolo => (84, 0x10, 8, 2),
        AmpKind::Cat => (85, 0x10, 7, 1),
        AmpKind::BassPro => (97, 0x10, 17, 5),
        AmpKind::BassInvader => (84, 0x52, 7, 4),
        AmpKind::EpPreamps => (108, 0x11, 11, 6),
    };
    let mut controls = [0.5f32; 17];
    controls[0] = 0.72;
    controls[5] = 0.7;
    controls[8] = 0.;
    let mut raw = Vec::new();
    for value in &controls[..values] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    let switches = match kind {
        AmpKind::Jump | AmpKind::HotSolo => [true, false, false, false, false, false],
        AmpKind::Van51 => [true, true, true, false, false, false],
        AmpKind::BassPro => [true, false, true, true, false, false],
        AmpKind::BassInvader => [true, false, true, false, false, false],
        AmpKind::EpPreamps => [true, false, true, false, true, false],
        _ => [false; 6],
    };
    if kind == AmpKind::EpPreamps {
        raw.extend(switches[..2].iter().copied().map(u8::from));
        for mode in [3i32, 4, 5, 3] {
            raw.extend_from_slice(&mode.to_le_bytes());
        }
        raw.extend(switches[2..].iter().copied().map(u8::from));
    } else {
        raw.extend(switches[..flags].iter().copied().map(u8::from));
    }
    FxSpec {
        ser_type,
        version,
        raw,
        bypass: false,
    }
}
fn spread() -> FxSpec {
    super::fx_routing_tests::spread_spec(0., 0., false)
}
fn shaper() -> FxSpec {
    let mut raw = 1.0f32.to_le_bytes().to_vec();
    raw.extend_from_slice(&0i32.to_le_bytes());
    FxSpec {
        ser_type: 29,
        version: 0x80,
        raw,
        bypass: false,
    }
}
fn filter() -> FxSpec {
    let mut raw = 6i32.to_le_bytes().to_vec();
    raw.extend_from_slice(&6i32.to_le_bytes());
    raw.extend_from_slice(&0.46f32.to_le_bytes());
    raw.extend_from_slice(&0.15f32.to_le_bytes());
    FxSpec {
        ser_type: 0x18,
        version: 0x92,
        raw,
        bypass: false,
    }
}
fn send() -> FxSpec {
    let mut raw = 1i32.to_le_bytes().to_vec();
    raw.extend_from_slice(&1.0f32.to_le_bytes());
    raw.extend_from_slice(&0i32.to_le_bytes());
    FxSpec {
        ser_type: 23,
        version: 0x51,
        raw,
        bypass: false,
    }
}
fn slots(specs: Vec<(usize, FxSpec, f32)>) -> FxChain {
    let mut chain = FxChain::parse(&fx_chain(&[])).unwrap();
    chain.slots = vec![None; 8];
    for (index, spec, gain) in specs {
        let mut parsed = FxChain::parse(&fx_chain(&[spec])).unwrap();
        let mut slot = parsed.slots.remove(0);
        slot.as_mut().unwrap().out_level = gain;
        chain.slots[index] = slot;
    }
    chain
}
fn source(index: usize) -> [f32; 2] {
    let x = index as f32;
    [
        (x * 0.071).sin() * 0.43 + (x * 0.023).cos() * 0.12,
        (x * 0.043).cos() * 0.31 - (x * 0.013).sin() * 0.09,
    ]
}
fn configure(
    fixture: &Fixture,
    chain: FxChain,
    split: i32,
    capacity: usize,
    source_scale: f32,
) -> SamplerPlugin {
    let mut p = fixture.plugin(false, None);
    p.voices.truncate(capacity);
    p.fx = None;
    p.gain_db = 0.;
    p.groups[0].fx_chain = Some(chain);
    p.groups[0].fx_amp_split_point = split;
    p.groups[0].key_tracking = false;
    p.groups[0].volume = 1.;
    p.groups[0].pan = 0.;
    p.zones[0].sample_start = 0;
    p.zones[0].sample_end = 0;
    p.zones[0].absolute_end = false;
    p.zones[0].volume = 1.;
    p.zones[0].pan = 0.;
    let frames = 4096;
    let mut channels = [Vec::with_capacity(frames), Vec::with_capacity(frames)];
    for index in 0..frames {
        let sample = source(index);
        channels[0].push(sample[0] * source_scale);
        channels[1].push(sample[1] * source_scale);
    }
    p.samples = vec![SampleAsset {
        info: nk_format::stream::SampleInfo {
            channels: 2,
            sample_rate: 48000,
            num_frames: frames,
        },
        data: SampleData::Resident(audio::Sample {
            channels: 2,
            sample_rate: 48000,
            num_frames: frames,
            samples: channels.into_iter().collect(),
        }),
    }];
    p.zone_samples = vec![Some(0)];
    p.streaming = None;
    rebuild(&mut p);
    p
}
fn rebuild(p: &mut SamplerPlugin) {
    p.selection = GroupStart::new(&p.groups, &p.program);
    p.expression = Modulation::new(&p.groups, p.voices.len()).unwrap();
    p.internal_mod = InternalMod::new(&p.groups);
    p.rebuild_voice_policies();
    p.rebuild_group_fx().unwrap();
}
fn render(
    p: &mut SamplerPlugin,
    rate: f64,
    frames: usize,
    partitions: &[usize],
    events: &[Event],
) -> Vec<[f32; 2]> {
    p.prepare(&ProcessContext {
        sample_rate: rate,
        block_size: 256,
        ..Default::default()
    });
    let mut result = Vec::with_capacity(frames);
    let mut cursor = 0;
    let mut part = 0;
    while cursor < frames {
        let count = partitions[part % partitions.len()].min(frames - cursor);
        let context = ProcessContext {
            sample_rate: rate,
            block_size: count,
            sample_pos: cursor as i64,
            ..Default::default()
        };
        let events: Vec<_> = events
            .iter()
            .filter(|e| (cursor..cursor + count).contains(&(e.frame)))
            .map(|e| {
                let mut e = *e;
                e.frame -= cursor;
                e
            })
            .collect();
        let mut buffer = vec![0.; count * 2];
        block(p, &context, &mut buffer, &events);
        for i in 0..count {
            result.push([buffer[i], buffer[count + i]]);
        }
        cursor += count;
        part += 1;
    }
    result
}
fn close(actual: &[[f32; 2]], expected: &[[f32; 2]], context: &str) {
    assert_eq!(actual.len(), expected.len());
    for (frame, (a, b)) in actual.iter().zip(expected).enumerate() {
        for channel in 0..2 {
            assert!(
                a[channel].is_finite()
                    && (a[channel] - b[channel]).abs() <= 2e-5 * (1. + b[channel].abs()),
                "{context} frame {frame} channel {channel}: {} vs {}",
                a[channel],
                b[channel]
            );
        }
    }
}
fn oracle(chain: &FxChain, split: i32, frames: usize, gain: f32, pan: f32) -> Vec<[f32; 2]> {
    let mut pre = chain.clone();
    let mut post = chain.clone();
    for i in 0..8 {
        if i < split as usize {
            post.slots[i] = None;
        } else {
            pre.slots[i] = None;
        }
    }
    let mut pre = PostFx::from_chains(&[pre]);
    let mut post = PostFx::from_chains(&[post]);
    for p in [&mut pre, &mut post].into_iter().flatten() {
        p.prepare(48000.);
    }
    let mut result = Vec::with_capacity(frames);
    for frame in 0..frames {
        let mut sample = source(frame);
        if let Some(p) = &mut pre {
            p.process(&mut sample, 2, 1);
        }
        let balance = if pan >= 0. {
            [1. - pan, 1.]
        } else {
            [1., 1. + pan]
        };
        sample[0] *= gain * balance[0];
        sample[1] *= gain * balance[1];
        if let Some(p) = &mut post {
            p.process(&mut sample, 2, 1);
        }
        result.push(sample);
    }
    result
}

#[test]
fn group_amps_decode_render_and_observe_the_physical_gain_split() {
    let fixture = Fixture::new(4096);
    for kind in MODELS {
        let chain = slots(vec![(6, amp(kind), 0.8)]);
        for split in [0, 6, 8] {
            let mut plugin = configure(&fixture, chain.clone(), split, 2, 1.);
            plugin.program.volume = 0.8;
            plugin.groups[0].volume = 0.6;
            plugin.zones[0].volume = 0.5;
            plugin.groups[0].pan = 0.35;
            let actual = render(
                &mut plugin,
                48000.,
                FRAMES,
                &[FRAMES],
                &[Event::note_on(0, 60, 127)],
            );
            let expected = oracle(&chain, split, FRAMES, 0.8 * 0.6 * 0.5, 0.35);
            close(&actual, &expected, &format!("{kind:?} split {split}"));
            assert!(
                actual.iter().flatten().any(|v| v.abs() > 1e-6),
                "silent group amp {kind:?} split {split}"
            );
        }
    }
}

#[test]
fn group_nonlinear_amps_sum_independent_voices_before_the_program_bus() {
    let fixture = Fixture::new(4096);
    for kind in MODELS {
        let chain = slots(vec![(0, amp(kind), 1.)]);
        let one = {
            let mut p = configure(&fixture, chain.clone(), 8, 2, 1.);
            render(
                &mut p,
                48000.,
                FRAMES,
                &[FRAMES],
                &[Event::note_on(0, 60, 127)],
            )
        };
        let two = {
            let mut p = configure(&fixture, chain.clone(), 8, 2, 1.);
            render(
                &mut p,
                48000.,
                FRAMES,
                &[FRAMES],
                &[Event::note_on(0, 60, 127), Event::note_on(0, 61, 127)],
            )
        };
        let sum: Vec<_> = one.iter().map(|v| [v[0] * 2., v[1] * 2.]).collect();
        close(&two, &sum, &format!("independent voices {kind:?}"));
        let doubled_input = {
            let mut p = configure(&fixture, chain, 8, 2, 2.);
            render(
                &mut p,
                48000.,
                FRAMES,
                &[FRAMES],
                &[Event::note_on(0, 60, 127)],
            )
        };
        let nonlinear = two
            .iter()
            .zip(doubled_input)
            .map(|(a, b)| (a[0] - b[0]).abs() + (a[1] - b[1]).abs())
            .fold(0.0f32, f32::max);
        assert!(
            nonlinear > 1e-5,
            "test input did not exercise nonlinearity {kind:?}: {nonlinear}"
        );
    }
}

#[test]
fn group_split_preserves_mixed_stage_order_holes_and_bypass() {
    let fixture = Fixture::new(4096);
    let mut bypass = amp(AmpKind::Cat);
    bypass.bypass = true;
    let chain = slots(vec![
        (0, shaper(), 1.),
        (2, bypass, 0.),
        (5, filter(), 1.),
        (6, amp(AmpKind::Cat), 0.7),
        (7, spread(), 0.8),
    ]);
    let mut outputs = Vec::new();
    for split in [0, 6, 8] {
        let mut plugin = configure(&fixture, chain.clone(), split, 2, 1.);
        plugin.groups[0].volume = 0.3;
        plugin.groups[0].pan = -0.4;
        let actual = render(
            &mut plugin,
            48000.,
            FRAMES,
            &[1, 31, 2, 63, 7],
            &[Event::note_on(0, 60, 127)],
        );
        let expected = oracle(&chain, split, FRAMES, 0.3, -0.4);
        close(&actual, &expected, &format!("mixed split {split}"));
        outputs.push(actual);
    }
    assert!(outputs[0]
        .iter()
        .zip(&outputs[1])
        .any(|(a, b)| (a[0] - b[0]).abs() > 1e-4));
    assert!(outputs[1]
        .iter()
        .zip(&outputs[2])
        .any(|(a, b)| (a[0] - b[0]).abs() > 1e-4));
}

#[test]
fn group_pre_and_post_sends_tap_the_amp_on_opposite_sides_of_voice_controls() {
    let fixture = Fixture::new(4096);
    let chain = slots(vec![
        (0, amp(AmpKind::Cat), 1.),
        (1, send(), 1.),
        (7, spread(), 0.),
    ]);
    let run = |split, controlled: bool, enveloped: bool| {
        let mut p = configure(&fixture, chain.clone(), split, 1, 1.);
        if controlled {
            p.program.volume = 0.8;
            p.groups[0].volume = 0.5;
            p.zones[0].volume = 0.25;
            p.zones[0].fade_low_velocity = 127;
            p.groups[0].pan = 1.;
        }
        if enveloped {
            p.groups[0].envelope = Some(Envelope {
                attack: 20.,
                hold: 0.,
                decay: 0.,
                release: 1.,
                sustain: 1.,
                curve: 0.,
                ahd_only: false,
            });
        }
        let empty = FxChain::parse(&fx_chain(&[])).unwrap();
        let ret = slots(vec![(0, spread(), 1.)]);
        p.fx = PostFx::from_chains(&[empty, ret]);
        let velocity = if controlled { 63 } else { 127 };
        render(
            &mut p,
            48000.,
            FRAMES,
            &[31, 1, 64, 7],
            &[Event::note_on(0, 60, velocity)],
        )
    };
    let pre = run(2, false, false);
    let pre_controlled = run(2, true, true);
    close(
        &pre_controlled,
        &pre,
        "pre send must bypass later gain/velocity/envelope/pan",
    );
    let post = run(1, true, false);
    let expected: Vec<_> = pre.iter().map(|v| [0., v[1] * 0.05]).collect();
    close(
        &post,
        &expected,
        "post send must include gain/velocity/pan once",
    );
    let post_envelope = run(1, true, true);
    let expected: Vec<_> = pre
        .iter()
        .enumerate()
        .map(|(frame, v)| [0., v[1] * 0.05 * frame as f32 / 960.])
        .collect();
    close(
        &post_envelope,
        &expected,
        "post send includes the linear attack envelope once",
    );
    assert!(
        pre.iter().any(|v| v[0].abs() > 1e-4),
        "tap signal must exercise muted left pan"
    );
}

#[test]
fn group_master_gain_does_not_change_amp_drive_at_either_split() {
    let fixture = Fixture::new(4096);
    for split in [0, 8] {
        let chain = slots(vec![(0, amp(AmpKind::Cat), 1.)]);
        let full = {
            let mut p = configure(&fixture, chain.clone(), split, 1, 1.);
            render(
                &mut p,
                48000.,
                FRAMES,
                &[FRAMES],
                &[Event::note_on(0, 60, 127)],
            )
        };
        let attenuated = {
            let mut p = configure(&fixture, chain, split, 1, 1.);
            p.gain_db = -6.;
            render(
                &mut p,
                48000.,
                FRAMES,
                &[FRAMES],
                &[Event::note_on(0, 60, 127)],
            )
        };
        let gain = 10f32.powf(-6. / 20.);
        let expected: Vec<_> = full
            .into_iter()
            .map(|v| [v[0] * gain, v[1] * gain])
            .collect();
        close(
            &attenuated,
            &expected,
            &format!("master gain outside split {split}"),
        );
    }
}

fn switching_groups(fixture: &Fixture) -> SamplerPlugin {
    let mut p = configure(fixture, slots(vec![(0, amp(AmpKind::Cat), 0.8)]), 8, 1, 1.);
    p.zones[0].low_key = 60;
    p.zones[0].high_key = 60;
    let mut second = p.groups[0].clone();
    second.name = "second amp template".into();
    let mut other = amp(AmpKind::Cat);
    other.raw[..4].copy_from_slice(&0.19f32.to_le_bytes());
    second.fx_chain = Some(slots(vec![(0, other, 0.6)]));
    p.groups.push(second);
    let mut zone = p.zones[0].clone();
    zone.group_index = 1;
    zone.low_key = 61;
    zone.high_key = 61;
    p.zones.push(zone);
    p.zone_samples.push(Some(0));
    rebuild(&mut p);
    p
}
#[test]
fn group_switching_voice_stealing_and_reset_reuse_prepared_states_without_heap_activity() {
    let fixture = Fixture::new(4096);
    let a = {
        let mut p = switching_groups(&fixture);
        render(&mut p, 48000., 128, &[128], &[Event::note_on(0, 60, 127)])
    };
    let b = {
        let mut p = switching_groups(&fixture);
        render(&mut p, 48000., 128, &[128], &[Event::note_on(0, 61, 127)])
    };
    let mut p = switching_groups(&fixture);
    let mut context = ProcessContext {
        block_size: 128,
        ..Default::default()
    };
    p.prepare(&context);
    let mut buffer = [0.; 256];
    let mut actual = [[0.; 2]; 512];
    ALLOCS.with(|v| v.set(0));
    TRACK.with(|v| v.set(true));
    for (segment, note) in [60, 61, 60].into_iter().enumerate() {
        context.sample_pos = (segment * 128) as i64;
        block(
            &mut p,
            &context,
            &mut buffer,
            &[Event::note_on(0, note, 127)],
        );
        for i in 0..128 {
            actual[segment * 128 + i] = [buffer[i], buffer[128 + i]];
        }
    }
    p.reset();
    context.sample_pos = 384;
    block(&mut p, &context, &mut buffer, &[Event::note_on(0, 60, 127)]);
    for i in 0..128 {
        actual[384 + i] = [buffer[i], buffer[128 + i]];
    }
    TRACK.with(|v| v.set(false));
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "activation/stealing/reset allocated or freed heap storage"
    );
    close(&actual[..128], &a, "first group");
    close(
        &actual[128..256],
        &b,
        "stolen voice must use the new group's fresh template",
    );
    close(
        &actual[256..384],
        &a,
        "returning to a group must not revive its old DSP history",
    );
    close(
        &actual[384..],
        &a,
        "sampler reset and voice activation must rewind group amp state",
    );
}

#[test]
fn group_amp_pool_activation_and_prepared_copy_are_realtime_safe_for_all_models() {
    let mut layout = [None; 8];
    for (slot, kind) in MODELS.into_iter().enumerate() {
        let chain = slots(vec![(0, amp(kind), 1.)]);
        layout[slot] = chain.slots[0]
            .as_ref()
            .unwrap()
            .child
            .as_ref()
            .unwrap()
            .amp_settings();
    }
    let mut pool = crate::group_amp::GroupAmps::new(&[layout], 4, 48000.).unwrap();
    assert_eq!(pool.pool_counts(), (8, 8, 32));
    let mut expected = [[[0.; 2]; 64]; 8];
    pool.activate(0, 0);
    for frame in 0..64 {
        for (slot, values) in expected.iter_mut().enumerate() {
            values[frame] = pool.process(0, slot, 0, source(frame));
        }
    }
    let mut valid = true;
    ALLOCS.with(|v| v.set(0));
    TRACK.with(|v| v.set(true));
    for voice in [1, 2, 3, 0] {
        pool.activate(0, voice);
        for frame in 0..64 {
            for (slot, values) in expected.iter().enumerate() {
                let actual = pool.process(0, slot, voice, source(frame));
                valid &= actual == values[frame] && actual.iter().all(|v| v.is_finite());
            }
        }
    }
    pool.reset();
    pool.activate(0, 0);
    for frame in 0..64 {
        for (slot, values) in expected.iter().enumerate() {
            valid &= pool.process(0, slot, 0, source(frame)) == values[frame];
        }
    }
    TRACK.with(|v| v.set(false));
    assert_eq!(
        ALLOCS.with(Cell::get),
        0,
        "copying/processing/resetting prepared amp states allocated or freed heap storage"
    );
    assert!(
        valid,
        "voice-owned prepared copy must match each independent model's fresh render"
    );
}

#[test]
fn group_amps_survive_irregular_blocks_reset_and_rate_changes() {
    let fixture = Fixture::new(4096);
    let chain = slots(vec![(0, amp(AmpKind::Cat), 0.8), (6, shaper(), 1.)]);
    let events = [
        Event::note_on(0, 60, 127),
        Event::note_off(127, 60),
        Event::note_on(191, 61, 100),
    ];
    for rate in [44100., 48000., 96000., 192000.] {
        let whole = {
            let mut p = configure(&fixture, chain.clone(), 6, 3, 1.);
            render(&mut p, rate, 385, &[256], &events)
        };
        let mut p = configure(&fixture, chain.clone(), 6, 3, 1.);
        let split = render(&mut p, rate, 385, &[1, 31, 2, 63, 7, 127], &events);
        close(
            &split,
            &whole,
            &format!("group amp block partition at {rate}"),
        );
        p.reset();
        let repeat = render(&mut p, rate, 385, &[256], &events);
        close(&repeat, &whole, &format!("group amp reset at {rate}"));
    }
    let mut p = configure(&fixture, chain, 6, 2, 1.);
    let _ = render(&mut p, 48000., 128, &[128], &[Event::note_on(0, 60, 127)]);
    let after = render(&mut p, 96000., 128, &[128], &[]);
    assert!(after.iter().flatten().all(|v| v.is_finite()));
    assert!(
        after.iter().flatten().any(|v| v.abs() > 1e-5),
        "rate preparation must reactivate existing voice-owned amp state"
    );
}
