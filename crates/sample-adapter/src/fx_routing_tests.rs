use super::*;
use nk_format::fx::FxChain;

pub(super) fn spread_spec(spread: f32, pan: f32, bypass: bool) -> FxSpec {
    let mut raw = spread.to_le_bytes().to_vec();
    raw.extend_from_slice(&pan.to_le_bytes());
    raw.push(0);
    FxSpec {
        ser_type: 31,
        version: 0x70,
        raw,
        bypass,
    }
}

fn shaper_spec() -> FxSpec {
    let mut raw = 1.0f32.to_le_bytes().to_vec();
    raw.extend_from_slice(&0i32.to_le_bytes());
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

fn routed_plugin(fixture: &Fixture, chain: FxChain) -> SamplerPlugin {
    let mut plugin = fixture.plugin(false, None);
    plugin.fx = None;
    plugin.groups[0].fx_chain = Some(chain);
    plugin.rebuild_group_fx().unwrap();
    plugin.preload_all();
    plugin
}

#[test]
fn program_sends_are_parallel_and_master_follows_inserts() {
    let inserts = chain(&[spread_spec(0.0, 0.5, false)]);
    // A silent send return must never mute the dry path.
    let mut sends = chain(&[spread_spec(0.0, 0.0, false)]);
    sends.slots[0].as_mut().unwrap().out_level = 0.0;
    let master = chain(&[spread_spec(-1.0, 0.0, false)]);
    let mut fx = PostFx::from_chains(&[inserts, sends, master]).unwrap();
    fx.prepare(48_000.0);
    let mut signal = [0.8, -0.2];
    fx.process(&mut signal, 2, 1);
    // Pan first gives (0.4, -0.3), then mono gives (0.05, 0.05).
    for sample in signal {
        assert!((sample - 0.05).abs() < 1e-6, "{sample}");
    }
    // An empty IFX array still occupies its role; SFX cannot become IFX.
    let mut send_only =
        PostFx::from_chains(&[chain(&[]), chain(&[spread_spec(-1.0, 0.0, false)])]).unwrap();
    let mut untouched = [0.8, -0.2];
    send_only.process(&mut untouched, 2, 1);
    assert_eq!(untouched, [0.8, -0.2]);
    let mut fx = PostFx::from_chains(&[
        chain(&[]),
        chain(&[]),
        chain(&[spread_spec(-1.0, 0.0, false)]),
    ])
    .unwrap();
    let mut signal = [0.8, -0.2];
    fx.process(&mut signal, 2, 1);
    assert!((signal[0] - 0.3).abs() < 1e-6);
    assert_eq!(signal[0], signal[1]);
}

#[test]
fn group_inserts_preserve_order_repeats_bypass_and_slot_gains() {
    let fixture = Fixture::new(10_000);
    let context = ProcessContext {
        block_size: 256,
        ..Default::default()
    };
    let mut dry = fixture.plugin(false, None);
    dry.fx = None;
    dry.preload_all();
    let mut input = [0.0; 512];
    block(
        &mut dry,
        &context,
        &mut input,
        &[Event::note_on(0, 60, 127)],
    );

    // Non-commuting operations, a bypassed muting slot, a second spread,
    // and two distinct slot gains. The old one-effect-per-type path fails.
    let mut slots = chain(&[
        spread_spec(-0.5, 0.25, false),
        shaper_spec(),
        spread_spec(-1.0, 1.0, true),
        spread_spec(0.5, -0.25, false),
    ]);
    slots.slots[0].as_mut().unwrap().out_level = 0.5;
    slots.slots[2].as_mut().unwrap().out_level = 0.0;
    slots.slots[3].as_mut().unwrap().out_level = 0.75;
    let mut plugin = routed_plugin(&fixture, slots);
    let mut output = [0.0; 512];
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_on(0, 60, 127)],
    );
    for frame in 0..256 {
        let (l, r) = (input[frame], input[256 + frame]);
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.25;
        // Classic at shape 1: fourfold input followed by signed 2t-t².
        let soft_clip = |x: f32| {
            let t = (x * 4.0).abs().min(1.0);
            (2.0 * t - t * t).copysign(x)
        };
        let l = soft_clip((mid + side) * 0.75 * 0.5);
        let r = soft_clip((mid - side) * 1.25 * 0.5);
        let mid = (l + r) * 0.5;
        let side = (l - r) * 0.75;
        assert!((output[frame] - (mid + side) * 1.25 * 0.75).abs() < 1e-6);
        assert!((output[256 + frame] - (mid - side) * 0.75 * 0.75).abs() < 1e-6);
    }
}

#[test]
fn group_shaper_stays_per_voice_under_polyphony() {
    let fixture = Fixture::new(10_000);
    let mut one = routed_plugin(&fixture, chain(&[shaper_spec()]));
    let mut two = routed_plugin(&fixture, chain(&[shaper_spec()]));
    let a = advance(&mut one, 48_000.0, 512, &[Event::note_on(0, 60, 127)]);
    let b = advance(
        &mut two,
        48_000.0,
        512,
        &[Event::note_on(0, 60, 127), Event::note_on(0, 60, 127)],
    );
    for (a, b) in a.iter().zip(b) {
        assert!((a * 2.0 - b).abs() < 1e-6);
    }
}

#[test]
fn group_filters_keep_independent_state_at_their_slot_position() {
    let fixture = Fixture::new(10_000);
    let mut raw = 6i32.to_le_bytes().to_vec();
    raw.extend_from_slice(&6i32.to_le_bytes());
    raw.extend_from_slice(&0.5f32.to_le_bytes());
    raw.extend_from_slice(&0.0f32.to_le_bytes());
    let filter = || FxSpec {
        ser_type: 0x18,
        version: 0x92,
        raw: raw.clone(),
        bypass: false,
    };
    let render = |specs: &[FxSpec]| {
        let mut plugin = routed_plugin(&fixture, chain(specs));
        advance(&mut plugin, 48_000.0, 2_000, &[Event::note_on(0, 60, 127)])
    };
    let before = render(&[filter(), shaper_spec(), filter()]);
    let after = render(&[filter(), filter(), shaper_spec()]);
    assert!(before.iter().zip(&after).any(|(a, b)| (a - b).abs() > 1e-3));
    // Eight six-pole slots require 24 biquads, exceeding the previous cap of
    // eight. Every slot's gain must run even after that old cap is reached.
    let mut raw6 = raw;
    raw6[..8].copy_from_slice(&[12i32.to_le_bytes(), 12i32.to_le_bytes()].concat());
    let specs: Vec<_> = (0..8)
        .map(|_| FxSpec {
            ser_type: 0x18,
            version: 0x92,
            raw: raw6.clone(),
            bypass: false,
        })
        .collect();
    let mut slots = chain(&specs);
    slots.slots[7].as_mut().unwrap().out_level = 0.0;
    let mut plugin = routed_plugin(&fixture, slots);
    let output = advance(&mut plugin, 48_000.0, 2_000, &[Event::note_on(0, 60, 127)]);
    assert!(output.iter().all(|value| *value == 0.0));
}

#[test]
fn ordered_fx_are_block_invariant_and_callbacks_do_not_allocate() {
    let fixture = Fixture::new(10_000);
    let make_plugin = || {
        let mut plugin = routed_plugin(
            &fixture,
            chain(&[
                spread_spec(-0.5, 0.25, false),
                shaper_spec(),
                spread_spec(0.5, -0.25, false),
            ]),
        );
        plugin.fx = PostFx::from_chains(&[
            chain(&[filter_spec(false, 3.0)]),
            chain(&[spread_spec(-1.0, 0.0, false)]),
            chain(&[spread_spec(0.25, -0.1, false)]),
        ]);
        plugin
    };
    let render = |block_size: usize| {
        let mut plugin = make_plugin();
        let mut context = ProcessContext {
            block_size,
            ..Default::default()
        };
        plugin.prepare(&context);
        let mut output = vec![0.0; block_size * 2];
        let mut rendered = Vec::new();
        let events = [Event::note_on(0, 60, 127), Event::note_on(17, 60, 127)];
        for index in 0..1024 / block_size {
            let events = if index == 0 { events.as_slice() } else { &[] };
            ALLOCS.with(|count| count.set(0));
            TRACK.with(|track| track.set(true));
            block(&mut plugin, &context, &mut output, events);
            TRACK.with(|track| track.set(false));
            assert_eq!(ALLOCS.with(Cell::get), 0);
            rendered.extend_from_slice(&output[..block_size]);
            context.sample_pos += block_size as i64;
        }
        ALLOCS.with(|count| count.set(0));
        TRACK.with(|track| track.set(true));
        plugin.reset();
        TRACK.with(|track| track.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0);
        output.fill(0.0);
        block(&mut plugin, &context, &mut output, &[]);
        assert!(output.iter().all(|value| *value == 0.0));
        rendered
    };
    assert_eq!(render(64), render(256));
}

#[test]
fn oversized_group_chain_fails_before_callback_state_can_overflow() {
    let fixture = Fixture::new(1024);
    let mut plugin = fixture.plugin(false, None);
    plugin.groups[0].fx_chain = Some(chain(
        &(0..9)
            .map(|_| spread_spec(0.0, 0.0, false))
            .collect::<Vec<_>>(),
    ));
    assert!(plugin.rebuild_group_fx().is_err());
}

#[test]
fn gainer_and_inverter_apply_to_their_serialized_parameters() {
    let gainer = chain(&[FxSpec {
        ser_type: 19,
        version: 0x50,
        raw: 0.5f32.to_le_bytes().to_vec(),
        bypass: false,
    }]);
    let mut fx = PostFx::from_chains(&[gainer]).unwrap();
    fx.prepare(48_000.0);
    let mut signal = [0.8, -0.2];
    fx.process(&mut signal, 2, 1);
    assert!((signal[0] - 0.4).abs() < 1e-6, "{signal:?}");
    assert!((signal[1] + 0.1).abs() < 1e-6, "{signal:?}");

    let inverter = chain(&[FxSpec {
        ser_type: 26,
        version: 0x60,
        raw: vec![1, 0],
        bypass: false,
    }]);
    let mut fx = PostFx::from_chains(&[inverter]).unwrap();
    fx.prepare(48_000.0);
    let mut signal = [0.8, -0.2];
    fx.process(&mut signal, 2, 1);
    assert!((signal[0] + 0.8).abs() < 1e-6, "{signal:?}");
    assert!((signal[1] + 0.2).abs() < 1e-6, "{signal:?}");
}

#[test]
fn delay_echo_and_feedback_follow_the_serialized_fields() {
    let mut raw = Vec::new();
    raw.extend_from_slice(&10.0f32.to_le_bytes()); // time ms
    raw.extend_from_slice(&0.5f32.to_le_bytes()); // feedback
    raw.extend_from_slice(&0.0f32.to_le_bytes()); // pan
    raw.extend_from_slice(&0.0f32.to_le_bytes()); // damping
    raw.extend_from_slice(&0.0f32.to_le_bytes()); // noteValue_time
    let delay = chain(&[FxSpec {
        ser_type: 16,
        version: 0x50,
        raw,
        bypass: false,
    }]);
    let mut fx = PostFx::from_chains(&[delay]).unwrap();
    fx.prepare(48_000.0);
    let frames = 2000;
    let mut buffer = vec![0.0f32; frames * 2];
    buffer[0] = 1.0;
    buffer[frames] = 1.0;
    fx.process(&mut buffer, 2, frames);
    let at = |index: usize| buffer[index];
    assert!(at(0).abs() < 1e-4, "dry must stay muted: {}", at(0));
    assert!((at(480) - 1.0).abs() < 1e-3, "first echo {}", at(480));
    assert!((at(960) - 0.49).abs() < 1e-3, "feedback echo {}", at(960));
}

#[test]
fn modulated_effects_run_and_stay_finite() {
    let mut chorus = Vec::new();
    chorus.extend_from_slice(&0.5f32.to_le_bytes()); // depth
    chorus.extend_from_slice(&2.0f32.to_le_bytes()); // speed Hz
    chorus.extend_from_slice(&0.0f32.to_le_bytes()); // phase
    chorus.extend_from_slice(&0.0f32.to_le_bytes()); // noteValue_speed
    let chain = chain(&[FxSpec {
        ser_type: 17,
        version: 0x50,
        raw: chorus,
        bypass: false,
    }]);
    let mut fx = PostFx::from_chains(&[chain]).unwrap();
    fx.prepare(48_000.0);
    let frames = 4096;
    let mut buffer = vec![0.0f32; frames * 2];
    for frame in 0..frames {
        let value = (frame as f32 * 0.05).sin();
        buffer[frame] = value;
        buffer[frames + frame] = value;
    }
    fx.process(&mut buffer, 2, frames);
    assert!(buffer.iter().all(|value| value.is_finite()));
    assert!(buffer.iter().any(|value| value.abs() > 1e-6));
}

#[test]
fn flanger_and_phaser_run_and_stay_finite() {
    let mut flanger = Vec::new();
    flanger.extend_from_slice(&0.5f32.to_le_bytes()); // depth
    flanger.extend_from_slice(&0.5f32.to_le_bytes()); // feedback
    flanger.extend_from_slice(&1.0f32.to_le_bytes()); // speed
    flanger.extend_from_slice(&0.0f32.to_le_bytes()); // phase
    flanger.extend_from_slice(&0.5f32.to_le_bytes()); // color
    flanger.extend_from_slice(&0.0f32.to_le_bytes()); // noteValue_speed
    let flanger = chain(&[FxSpec {
        ser_type: 18,
        version: 0x50,
        raw: flanger,
        bypass: false,
    }]);

    let mut phaser = Vec::new();
    phaser.extend_from_slice(&0.8f32.to_le_bytes()); // depth
    phaser.extend_from_slice(&0.5f32.to_le_bytes()); // feedback
    phaser.extend_from_slice(&0.7f32.to_le_bytes()); // speed
    phaser.extend_from_slice(&0.0f32.to_le_bytes()); // phase
    phaser.extend_from_slice(&0.0f32.to_le_bytes()); // noteValue_speed
    let phaser = chain(&[FxSpec {
        ser_type: 20,
        version: 0x50,
        raw: phaser,
        bypass: false,
    }]);

    for effect in [flanger, phaser] {
        let mut fx = PostFx::from_chains(&[effect]).unwrap();
        fx.prepare(48_000.0);
        let frames = 2048;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = (frame as f32 * 0.05).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
    }
}

#[test]
fn compressor_reduces_loud_material_and_passes_quiet_material() {
    let mut raw = Vec::new();
    raw.extend_from_slice(&0i32.to_le_bytes()); // model classic
    raw.extend_from_slice(&(-20.0f32).to_le_bytes()); // threshold dB
    raw.extend_from_slice(&0.25f32.to_le_bytes()); // 1/ratio -> 4:1
    raw.extend_from_slice(&1.0f32.to_le_bytes()); // attack ms
    raw.extend_from_slice(&100.0f32.to_le_bytes()); // release ms
    raw.push(1); // stereo link
    let compressor = chain(&[FxSpec {
        ser_type: 25,
        version: 0x70,
        raw,
        bypass: false,
    }]);
    let frames = 9600;
    let render = |amplitude: f32| {
        let mut fx = PostFx::from_chains(std::slice::from_ref(&compressor)).unwrap();
        fx.prepare(48_000.0);
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = amplitude * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        let tail = &buffer[frames - 1000..frames];
        (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt()
    };
    let loud = render(0.8);
    let quiet = render(0.01);
    assert!(loud < 0.3, "loud compressed rms {loud}");
    assert!(
        (quiet - 0.01 / 2f32.sqrt()).abs() < 1e-3,
        "quiet rms {quiet}"
    );
}

#[test]
fn classic_compressor_reaches_the_exported_static_ceiling() {
    let mut raw = Vec::new();
    raw.extend_from_slice(&0i32.to_le_bytes()); // model classic
    raw.extend_from_slice(&(-20.0f32).to_le_bytes()); // threshold dB
    raw.extend_from_slice(&0.25f32.to_le_bytes()); // 1/ratio -> 4:1
    raw.extend_from_slice(&1.0f32.to_le_bytes()); // attack ms
    raw.extend_from_slice(&100.0f32.to_le_bytes()); // release ms
    raw.push(1);
    let compressor = chain(&[FxSpec {
        ser_type: 25,
        version: 0x70,
        raw,
        bypass: false,
    }]);
    let mut fx = PostFx::from_chains(std::slice::from_ref(&compressor)).unwrap();
    fx.prepare(48_000.0);
    let frames = 48_000;
    let mut buffer = vec![0.0f32; frames * 2];
    for frame in 0..frames {
        buffer[frame] = 1.0;
        buffer[frames + frame] = 1.0;
    }
    fx.process(&mut buffer, 2, frames);
    let tail = &buffer[frames - 1000..frames];
    let rms = (tail.iter().map(|v| v * v).sum::<f32>() / tail.len() as f32).sqrt();
    let threshold = 2f32.powf(-20.0 / 6.0);
    let expected = threshold.powf(0.75); // T^(1-1/R)
    assert!(
        (rms - expected).abs() < expected * 0.05,
        "compressed {rms}, expected {expected}"
    );
}

#[test]
fn limiter_release_is_the_native_ln2_ramp() {
    let mut raw = Vec::new();
    raw.extend_from_slice(&0.0f32.to_le_bytes()); // in gain, dB
    raw.extend_from_slice(&10.0f32.to_le_bytes()); // release ms
    let limiter = chain(&[FxSpec {
        ser_type: 28,
        version: 0x60,
        raw,
        bypass: false,
    }]);
    let mut fx = PostFx::from_chains(std::slice::from_ref(&limiter)).unwrap();
    fx.prepare(48_000.0);
    let n = 480; // 10 ms at 48 kHz
    let frames = 1 + 2 * n;
    let mut buffer = vec![0.0f32; frames * 2];
    buffer[0] = 2.0;
    buffer[frames] = 2.0;
    for frame in 1..frames {
        buffer[frame] = 0.5;
        buffer[frames + frame] = 0.5;
    }
    fx.process(&mut buffer, 2, frames);
    let first = buffer[1];
    let after_n = buffer[1 + n];
    let ratio = (0.5 - first) / (0.5 - after_n);
    assert!(
        (ratio - 2.0).abs() < 0.05,
        "release residual ratio {ratio}, expected 2"
    );
}

#[test]
fn distortion_and_skreamer_shape_and_stay_finite() {
    let mut distortion = Vec::new();
    distortion.extend_from_slice(&0i32.to_le_bytes()); // tube
    distortion.extend_from_slice(&1.0f32.to_le_bytes()); // drive
    distortion.extend_from_slice(&0.0f32.to_le_bytes()); // damping
    let distortion = chain(&[FxSpec {
        ser_type: 30,
        version: 0x60,
        raw: distortion,
        bypass: false,
    }]);

    let mut skreamer = Vec::new();
    for value in [0.5f32, 1.0, 0.5, 0.5, 1.0] {
        skreamer.extend_from_slice(&value.to_le_bytes());
    }
    let skreamer = chain(&[FxSpec {
        ser_type: 33,
        version: 0x50,
        raw: skreamer,
        bypass: false,
    }]);

    for effect in [distortion, skreamer] {
        let mut fx = PostFx::from_chains(&[effect]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 1.5 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 1.5));
    }
}

#[test]
fn dstortion_fuzz_and_big_fuzz_run_and_stay_finite() {
    let mut dstortion = Vec::new();
    for value in [0.7f32, 0.5, 0.5, 0.5, 0.5, 1.0] {
        dstortion.extend_from_slice(&value.to_le_bytes());
    }
    dstortion.push(0);
    let mut fuzz = Vec::new();
    for value in [0.8f32, 0.5, 1.0] {
        fuzz.extend_from_slice(&value.to_le_bytes());
    }
    fuzz.push(0);
    let mut big_fuzz = Vec::new();
    for value in [0.8f32, 0.5, 0.5, 1.0] {
        big_fuzz.extend_from_slice(&value.to_le_bytes());
    }
    big_fuzz.push(0);
    let specs = [
        FxSpec {
            ser_type: 86,
            version: 0x10,
            raw: dstortion,
            bypass: false,
        },
        FxSpec {
            ser_type: 110,
            version: 0x10,
            raw: fuzz,
            bypass: false,
        },
        FxSpec {
            ser_type: 111,
            version: 0x10,
            raw: big_fuzz,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn chainsaw_skreamer_dlx_and_kolor_run_and_stay_finite() {
    let mut chainsaw = Vec::new();
    for value in [0.8f32, 0.5, 0.6, 1.0] {
        chainsaw.extend_from_slice(&value.to_le_bytes());
    }
    chainsaw.push(0);
    let mut skreamer_dlx = Vec::new();
    skreamer_dlx.extend_from_slice(&0i32.to_le_bytes());
    for value in [0.8f32, 0.5, 0.5, 0.5, 1.0] {
        skreamer_dlx.extend_from_slice(&value.to_le_bytes());
    }
    skreamer_dlx.push(0);
    let mut kolor = Vec::new();
    kolor.extend_from_slice(&0i32.to_le_bytes());
    for value in [0.7f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 0.5, 0.5] {
        kolor.extend_from_slice(&value.to_le_bytes());
    }
    kolor.push(0);
    let specs = [
        FxSpec {
            ser_type: 119,
            version: 0x10,
            raw: chainsaw,
            bypass: false,
        },
        FxSpec {
            ser_type: 120,
            version: 0x10,
            raw: skreamer_dlx,
            bypass: false,
        },
        FxSpec {
            ser_type: 121,
            version: 0x10,
            raw: kolor,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn replika_gater_vibrato_and_wow_flutter_run_and_stay_finite() {
    let mut replika = Vec::new();
    replika.extend_from_slice(&0i32.to_le_bytes());
    for value in [10.0f32, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0] {
        replika.extend_from_slice(&value.to_le_bytes());
    }
    replika.extend_from_slice(&[0, 0, 0]);
    for _ in 0..3 {
        replika.extend_from_slice(&0.0f32.to_le_bytes());
    }
    replika.push(0);

    let mut gater = Vec::new();
    for value in [4.0f32, 1.0, 0.0, 0.5] {
        gater.extend_from_slice(&value.to_le_bytes());
    }
    gater.extend_from_slice(&[0, 0, 0, 0]);

    let mut vibrato = Vec::new();
    vibrato.extend_from_slice(&0i32.to_le_bytes());
    vibrato.extend_from_slice(&0i32.to_le_bytes());
    for value in [2.0f32, 0.5, 0.5, 0.0] {
        vibrato.extend_from_slice(&value.to_le_bytes());
    }
    vibrato.push(0);

    let mut wow = Vec::new();
    for value in [0.5f32, 0.5, 6.0, 0.3, 0.5, 1.0, 0.0] {
        wow.extend_from_slice(&value.to_le_bytes());
    }
    wow.push(0);
    wow.extend_from_slice(&0.0f32.to_le_bytes());

    let specs = [
        FxSpec {
            ser_type: 90,
            version: 0x12,
            raw: replika,
            bypass: false,
        },
        FxSpec {
            ser_type: 114,
            version: 1,
            raw: gater,
            bypass: false,
        },
        FxSpec {
            ser_type: 105,
            version: 0x10,
            raw: vibrato,
            bypass: false,
        },
        FxSpec {
            ser_type: 106,
            version: 0x10,
            raw: wow,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn tape_trans_psyche_twin_and_superfast_run_and_stay_finite() {
    let mut tape = Vec::new();
    for value in [0.6f32, 0.5, 1.0] {
        tape.extend_from_slice(&value.to_le_bytes());
    }
    tape.push(0);
    let mut trans = Vec::new();
    for value in [0.8f32, 0.5, 1.0] {
        trans.extend_from_slice(&value.to_le_bytes());
    }
    trans.extend_from_slice(&0i32.to_le_bytes());
    let mut psyche = Vec::new();
    for value in [10.0f32, 0.4, 0.0, 0.0, 0.0] {
        psyche.extend_from_slice(&value.to_le_bytes());
    }
    psyche.extend_from_slice(&[0, 0, 0]);
    for _ in 0..3 {
        psyche.extend_from_slice(&0.0f32.to_le_bytes());
    }
    psyche.push(0);
    let mut twin = Vec::new();
    for value in [10.0f32, 0.4, 0.0] {
        twin.extend_from_slice(&value.to_le_bytes());
    }
    twin.push(0);
    let mut superfast = Vec::new();
    for value in [0.7f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0] {
        superfast.extend_from_slice(&value.to_le_bytes());
    }
    superfast.extend_from_slice(&[0, 0, 0, 0]);
    let specs = [
        FxSpec {
            ser_type: 66,
            version: 0x10,
            raw: tape,
            bypass: false,
        },
        FxSpec {
            ser_type: 67,
            version: 0x10,
            raw: trans,
            bypass: false,
        },
        FxSpec {
            ser_type: 99,
            version: 0x10,
            raw: psyche,
            bypass: false,
        },
        FxSpec {
            ser_type: 107,
            version: 0x10,
            raw: twin,
            bypass: false,
        },
        FxSpec {
            ser_type: 118,
            version: 0x10,
            raw: superfast,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn performance_effects_run_and_stay_finite() {
    let mut stereotune = Vec::new();
    for value in [0.5f32, 5.0, 0.5, 1.0] {
        stereotune.extend_from_slice(&value.to_le_bytes());
    }
    let mut masher = Vec::new();
    for value in [0.25f32, 1.0, 0.5] {
        masher.extend_from_slice(&value.to_le_bytes());
    }
    masher.extend_from_slice(&[0, 0, 0]);
    masher.extend_from_slice(&0i32.to_le_bytes());
    let mut slicer = Vec::new();
    for value in [8.0f32, 1.0] {
        slicer.extend_from_slice(&value.to_le_bytes());
    }
    slicer.extend_from_slice(&[0, 0, 0]);
    slicer.extend_from_slice(&0i32.to_le_bytes());
    slicer.extend_from_slice(&0i32.to_le_bytes());
    let mut reverse = Vec::new();
    for value in [0.25f32, 1.0, 0.0, 0.0] {
        reverse.extend_from_slice(&value.to_le_bytes());
    }
    reverse.extend_from_slice(&[0, 0, 0]);
    let mut stretch = Vec::new();
    for value in [0.5f32, 1.0, 0.0, 0.0] {
        stretch.extend_from_slice(&value.to_le_bytes());
    }
    stretch.extend_from_slice(&[0, 0, 0]);
    let specs = [
        FxSpec {
            ser_type: 112,
            version: 0x10,
            raw: stereotune,
            bypass: false,
        },
        FxSpec {
            ser_type: 113,
            version: 1,
            raw: masher,
            bypass: false,
        },
        FxSpec {
            ser_type: 115,
            version: 1,
            raw: slicer,
            bypass: false,
        },
        FxSpec {
            ser_type: 116,
            version: 1,
            raw: reverse,
            bypass: false,
        },
        FxSpec {
            ser_type: 117,
            version: 1,
            raw: stretch,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 8192;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn modulation_and_lofi_effects_run_and_stay_finite() {
    let mut phasis = Vec::new();
    for value in [0.5f32, 1.0, 0.0, 0.3, 0.5, 0.5, 0.5, 0.5, 0.5] {
        phasis.extend_from_slice(&value.to_le_bytes());
    }
    phasis.push(0);
    let flair = phasis.clone();
    let mut choral = Vec::new();
    for value in [0.5f32, 1.0, 0.0, 0.3, 0.5, 0.5, 0.5] {
        choral.extend_from_slice(&value.to_le_bytes());
    }
    choral.extend_from_slice(&[0, 0]);
    let mut ring = Vec::new();
    ring.push(1);
    for value in [220.0f32, 0.8, 0.5, 0.5] {
        ring.extend_from_slice(&value.to_le_bytes());
    }
    ring.extend_from_slice(&0i32.to_le_bytes());
    for value in [0.0f32, 0.0, 0.0] {
        ring.extend_from_slice(&value.to_le_bytes());
    }
    let mut lofi = Vec::new();
    for value in [0.7f32, 0.5] {
        lofi.extend_from_slice(&value.to_le_bytes());
    }
    lofi.push(0);
    for value in [1.0f32, 0.5] {
        lofi.extend_from_slice(&value.to_le_bytes());
    }
    let mut rotator = Vec::new();
    for value in [1.0f32, 0.5, 0.0, 0.5, 0.5, 0.5] {
        rotator.extend_from_slice(&value.to_le_bytes());
    }
    let specs = [
        FxSpec {
            ser_type: 91,
            version: 0x51,
            raw: phasis,
            bypass: false,
        },
        FxSpec {
            ser_type: 92,
            version: 0x51,
            raw: flair,
            bypass: false,
        },
        FxSpec {
            ser_type: 93,
            version: 0x51,
            raw: choral,
            bypass: false,
        },
        FxSpec {
            ser_type: 100,
            version: 0x50,
            raw: ring,
            bypass: false,
        },
        FxSpec {
            ser_type: 32,
            version: 0x70,
            raw: lofi,
            bypass: false,
        },
        FxSpec {
            ser_type: 34,
            version: 0x50,
            raw: rotator,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn dynamics_effects_run_and_stay_finite() {
    let mut dyx = Vec::new();
    for value in [0.1f32, 0.5] {
        dyx.extend_from_slice(&value.to_le_bytes());
    }
    dyx.extend_from_slice(&0i32.to_le_bytes());
    dyx.push(0);
    let mut bus = Vec::new();
    for value in [0.1f32, 0.5, 10.0, 100.0, 1.0, 0.5] {
        bus.extend_from_slice(&value.to_le_bytes());
    }
    bus.extend_from_slice(&[0, 0]);
    bus.extend_from_slice(&1.0f32.to_le_bytes());
    let mut fb = Vec::new();
    for value in [0.1f32, 0.5, 10.0, 100.0, 1.0, 0.5, 0.5] {
        fb.extend_from_slice(&value.to_le_bytes());
    }
    fb.extend_from_slice(&[0, 0, 0]);
    let mut hilbert = Vec::new();
    for value in [0.1f32, 100.0, 1.0] {
        hilbert.extend_from_slice(&value.to_le_bytes());
    }
    hilbert.push(0);
    let mut supercharger = Vec::new();
    for value in [0.1f32, 0.5, 10.0, 100.0, 0.5, 0.5, 0.5] {
        supercharger.extend_from_slice(&value.to_le_bytes());
    }
    for _ in 0..4 {
        supercharger.extend_from_slice(&0i32.to_le_bytes());
    }
    supercharger.push(0);
    let specs = [
        FxSpec {
            ser_type: 27,
            version: 0x60,
            raw: dyx,
            bypass: false,
        },
        FxSpec {
            ser_type: 70,
            version: 0x12,
            raw: bus,
            bypass: false,
        },
        FxSpec {
            ser_type: 76,
            version: 0x10,
            raw: fb,
            bypass: false,
        },
        FxSpec {
            ser_type: 95,
            version: 0x51,
            raw: hilbert,
            bypass: false,
        },
        FxSpec {
            ser_type: 96,
            version: 0x51,
            raw: supercharger,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 8192;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn filter_and_drive_effects_run_and_stay_finite() {
    let mut twang = Vec::new();
    for value in [0.6f32, 0.5, 0.5, 1.0] {
        twang.extend_from_slice(&value.to_le_bytes());
    }
    twang.extend_from_slice(&[0, 0]);
    let mut wah = Vec::new();
    wah.extend_from_slice(&0.5f32.to_le_bytes());
    wah.push(1);
    let mut bite = Vec::new();
    bite.extend_from_slice(&0i32.to_le_bytes());
    for value in [0.6f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 0.5, 0.5] {
        bite.extend_from_slice(&value.to_le_bytes());
    }
    let mut dirt = Vec::new();
    for value in [0.6f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 0.5] {
        dirt.extend_from_slice(&value.to_le_bytes());
    }
    dirt.extend_from_slice(&[0, 0]);
    let mut freak = Vec::new();
    for value in [
        0.6f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5,
    ] {
        freak.extend_from_slice(&value.to_le_bytes());
    }
    freak.push(0);
    let specs = [
        FxSpec {
            ser_type: 35,
            version: 0x50,
            raw: twang,
            bypass: false,
        },
        FxSpec {
            ser_type: 88,
            version: 0x10,
            raw: wah,
            bypass: false,
        },
        FxSpec {
            ser_type: 102,
            version: 0x10,
            raw: bite,
            bypass: false,
        },
        FxSpec {
            ser_type: 103,
            version: 0x10,
            raw: dirt,
            bypass: false,
        },
        FxSpec {
            ser_type: 104,
            version: 0x10,
            raw: freak,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn van51_routed_playback_matches_circuit_across_ranges_and_reset() {
    use crate::van51_amp::Van51Amp;
    use nk_format::fx::{AmpKind, AmpSettings};
    let values = [0.63f32, 0.48, 0.37, 0.56, 0.71, 0.42, 0.67, 0.31];
    let flags = [true, false, true, false, false];
    let mut raw = Vec::new();
    for value in values {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend(flags.map(u8::from));
    let mut wire = AmpSettings {
        kind: AmpKind::Van51,
        values: [0.0; 17],
        value_count: 8,
        flags: [false; 8],
        flag_count: 5,
        modes: [0; 4],
    };
    wire.values[..8].copy_from_slice(&values);
    wire.flags[..5].copy_from_slice(&flags);
    let mut inserts = chain(&[FxSpec {
        ser_type: 82,
        version: 0x10,
        raw,
        bypass: false,
    }]);
    inserts.slots[0].as_mut().unwrap().out_level = 0.7;
    inserts.slots[0].as_mut().unwrap().out_level_dry = 0.0;
    let mut routed = PostFx::from_chains(&[inserts]).unwrap();
    let mut direct = Van51Amp::new(wire);
    routed.prepare(44_100.0);
    direct.prepare(44_100.0).unwrap();
    let frames = 769;
    let input: Vec<_> = (0..2 * frames)
        .map(|index| (index as f32 * 0.053).sin() * 0.16)
        .collect();
    let mut expected = input.clone();
    for frame in 0..frames {
        let output = direct.process(input[frame], input[frames + frame]);
        expected[frame] = output[0] * 0.7;
        expected[frames + frame] = output[1] * 0.7;
    }
    let mut actual = input.clone();
    let mut start = 0;
    for count in [17, 31, 128, 256, 337] {
        routed.process_range(
            &mut actual,
            2,
            frames,
            start,
            &mut vec![[[0.0; 2]; 8]; count],
        );
        start += count;
    }
    assert_eq!(start, frames);
    assert_eq!(actual, expected);
    assert_ne!(actual, input);
    routed.reset();
    actual.copy_from_slice(&input);
    routed.process(&mut actual, 2, frames);
    assert_eq!(actual, expected);
}

#[test]
fn amp_family_runs_and_stays_finite() {
    fn raw(floats: &[f32], flags: &[bool]) -> Vec<u8> {
        let mut raw = Vec::new();
        for value in floats {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        for flag in flags {
            raw.push(*flag as u8);
        }
        raw
    }
    let mut ep = Vec::new();
    for value in [0.6f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0] {
        ep.extend_from_slice(&value.to_le_bytes());
    }
    ep.extend_from_slice(&[1, 1]);
    for mode in [3i32, 4, 5, 3] {
        ep.extend_from_slice(&mode.to_le_bytes());
    }
    ep.extend_from_slice(&[1, 0, 1, 0]);
    let specs = [
        (36u16, 0x50u16, {
            let mut cabinet = Vec::new();
            for value in [0.6f32, 0.5, 0.5, 0.5] {
                cabinet.extend_from_slice(&value.to_le_bytes());
            }
            cabinet.extend_from_slice(&0i32.to_le_bytes());
            cabinet
        }),
        (
            77,
            0x10,
            raw(&[0.6, 0.5, 0.5, 0.5, 0.5, 1.0], &[true, false]),
        ),
        (
            82,
            0x10,
            raw(&[0.6, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0], &[true; 5]),
        ),
        (83, 0x10, raw(&[0.6, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0], &[true])),
        (
            84,
            0x52,
            raw(&[0.6, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0], &[true; 4]),
        ),
        (
            84,
            0x10,
            raw(&[0.6, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0], &[true, false]),
        ),
        (85, 0x10, raw(&[0.6, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0], &[true])),
        (
            97,
            0x10,
            raw(
                &[
                    0.6, 0.5, 0.5, 0.5, 0.5, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                    0.0,
                ],
                &[true; 5],
            ),
        ),
        (108, 0x11, ep),
    ];
    for (ser_type, version, raw) in specs {
        let mut fx = PostFx::from_chains(&[chain(&[FxSpec {
            ser_type,
            version,
            raw,
            bypass: false,
        }])])
        .unwrap();
        fx.prepare(48_000.0);
        let frames = 4096;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
    }
}

#[test]
fn panner_core_cell_and_reverbs_run_and_stay_finite() {
    let mut panner = Vec::new();
    panner.extend_from_slice(&1i32.to_le_bytes());
    panner.extend_from_slice(&0.5f32.to_le_bytes());
    panner.extend_from_slice(&0.0f32.to_le_bytes());
    panner.push(1);
    let mut core = Vec::new();
    for value in [0.6f32, 0.5, 0.5, 0.5, 0.5, 1.0, 0.5, 0.5, 0.5, 0.5] {
        core.extend_from_slice(&value.to_le_bytes());
    }
    let mut reverb = Vec::new();
    for value in [0.8f32, 0.5, 0.5, 0.5, 0.5] {
        reverb.extend_from_slice(&value.to_le_bytes());
    }
    let plate = reverb.clone();
    let mut galois = Vec::new();
    galois.extend_from_slice(&0i32.to_le_bytes());
    for value in [0.8f32, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5] {
        galois.extend_from_slice(&value.to_le_bytes());
    }
    let raum = galois.clone();
    let mut delight = Vec::new();
    for value in [0.8f32, 0.5, 0.5] {
        delight.extend_from_slice(&value.to_le_bytes());
    }
    delight.push(0);
    for value in [0.5f32, 0.5, 0.5] {
        delight.extend_from_slice(&value.to_le_bytes());
    }
    delight.push(0);
    delight.extend_from_slice(&0i32.to_le_bytes());
    let specs = [
        FxSpec {
            ser_type: 29,
            version: 0x70,
            raw: panner,
            bypass: false,
        },
        FxSpec {
            ser_type: 94,
            version: 0x51,
            raw: core,
            bypass: false,
        },
        FxSpec {
            ser_type: 21,
            version: 0x50,
            raw: reverb,
            bypass: false,
        },
        FxSpec {
            ser_type: 87,
            version: 0x10,
            raw: plate,
            bypass: false,
        },
        FxSpec {
            ser_type: 89,
            version: 0x10,
            raw: galois,
            bypass: false,
        },
        FxSpec {
            ser_type: 101,
            version: 0x10,
            raw: raum,
            bypass: false,
        },
        FxSpec {
            ser_type: 122,
            version: 0x10,
            raw: delight,
            bypass: false,
        },
    ];
    for spec in specs {
        let mut fx = PostFx::from_chains(&[chain(&[spec])]).unwrap();
        fx.prepare(48_000.0);
        let frames = 8192;
        let mut buffer = vec![0.0f32; frames * 2];
        for frame in 0..frames {
            let value = 0.8 * (frame as f32 * 0.1).sin();
            buffer[frame] = value;
            buffer[frames + frame] = value;
        }
        fx.process(&mut buffer, 2, frames);
        assert!(buffer.iter().all(|value| value.is_finite()));
        assert!(buffer.iter().any(|value| value.abs() > 1e-6));
        assert!(buffer.iter().all(|value| value.abs() < 8.0));
    }
}

#[test]
fn base_reverb_pre_delay_shifts_the_wet_onset() {
    let onset = |pre_delay_ms: f32| {
        let mut raw = Vec::new();
        for value in [pre_delay_ms, 0.5f32, 0.0, 0.0, 0.0] {
            raw.extend_from_slice(&value.to_le_bytes());
        }
        let spec = chain(&[FxSpec {
            ser_type: 21,
            version: 0x50,
            raw,
            bypass: false,
        }]);
        let mut fx = PostFx::from_chains(std::slice::from_ref(&spec)).unwrap();
        fx.prepare(48_000.0);
        let frames = 12_000;
        let mut buffer = vec![0.0f32; frames * 2];
        buffer[0] = 1.0;
        buffer[frames] = 1.0;
        fx.process(&mut buffer, 2, frames);
        buffer[..frames]
            .iter()
            .position(|value| value.abs() > 1e-7)
            .expect("reverb produced no wet signal")
    };
    let without = onset(0.0);
    let with = onset(50.0);
    // 50 ms at 48 kHz is 2400 samples; allow for the nearest line tap.
    assert!(
        (with as isize - without as isize - 2400).abs() < 64,
        "pre-delay moved onset from {without} to {with}"
    );
}

#[test]
fn cabinet_runs_in_a_group_chain_and_stays_finite() {
    let fixture = Fixture::new(10_000);
    let mut raw = Vec::new();
    for value in [0.5f32, 0.5, 0.5, 0.5] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend_from_slice(&0i32.to_le_bytes());
    let cabinet = FxSpec {
        ser_type: 36,
        version: 0x50,
        raw,
        bypass: false,
    };
    let mut plugin = routed_plugin(&fixture, chain(&[cabinet]));
    let signal = advance(&mut plugin, 48_000.0, 3_000, &[Event::note_on(0, 60, 127)]);
    assert!(signal.iter().all(|value| value.is_finite()));
    assert!(signal.iter().any(|value| value.abs() > 1e-5));
}
