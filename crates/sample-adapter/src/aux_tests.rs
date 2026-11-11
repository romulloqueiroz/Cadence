use super::*;
use nk_format::fx::{ConvolutionParams, FxChain};

fn ir_params() -> ConvolutionParams {
    ConvolutionParams {
        rate_factor: -1.0,
        block_size: 0,
        predelay_ms: 0.0,
        early_length: 1.0,
        early_highpass: 20.0,
        early_lowpass: 20_000.0,
        late_length: 1.0,
        late_highpass: 20.0,
        late_lowpass: 20_000.0,
        crossover: -1.0,
        reverse: false,
        auto_gain: false,
        preserve_length: true,
        bypass_latency_compensation: false,
        envelope_active: false,
        envelope_times: vec![],
        envelope_levels: vec![],
        filename_id: 0,
    }
}
fn ir_spec(params: &ConvolutionParams) -> FxSpec {
    let mut raw = params.rate_factor.to_le_bytes().to_vec();
    raw.extend_from_slice(&params.block_size.to_le_bytes());
    for v in [
        params.predelay_ms,
        params.early_length,
        params.early_highpass,
        params.early_lowpass,
        params.late_length,
        params.late_highpass,
        params.late_lowpass,
        params.crossover,
    ] {
        raw.extend_from_slice(&v.to_le_bytes());
    }
    raw.extend_from_slice(&[
        params.reverse as u8,
        params.auto_gain as u8,
        params.preserve_length as u8,
        params.bypass_latency_compensation as u8,
        params.envelope_active as u8,
    ]);
    for values in [&params.envelope_times, &params.envelope_levels] {
        raw.extend_from_slice(&(values.len() as i32).to_le_bytes());
        for v in values {
            raw.extend_from_slice(&v.to_le_bytes());
        }
    }
    raw.extend_from_slice(&params.filename_id.to_le_bytes());
    FxSpec {
        ser_type: 22,
        version: 0x70,
        raw,
        bypass: false,
    }
}
fn send_spec(levels: &[f32]) -> FxSpec {
    let mut raw = (levels.len() as i32).to_le_bytes().to_vec();
    for value in levels {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend_from_slice(&0i32.to_le_bytes());
    FxSpec {
        ser_type: 23,
        version: 0x51,
        raw,
        bypass: false,
    }
}
fn chain(specs: &[FxSpec]) -> FxChain {
    FxChain::parse(&fx_chain(specs)).unwrap()
}
fn sample(channels: Vec<Vec<f32>>, rate: u32) -> audio::Sample {
    audio::Sample {
        channels: channels.len(),
        sample_rate: rate,
        num_frames: channels[0].len(),
        samples: channels,
    }
}
fn build(chains: &[FxChain], ir: &audio::Sample) -> PostFx {
    PostFx::build(chains, &mut |_| Ok(ir.clone()))
        .unwrap()
        .unwrap()
}

#[test]
fn send_and_ir_wire_fields_are_bounded_and_truncation_is_an_error() {
    let mut p = ir_params();
    p.reverse = true;
    p.auto_gain = true;
    p.predelay_ms = 12.5;
    p.envelope_times = vec![0.0, 1.0];
    p.envelope_levels = vec![0.0, -60.0];
    p.filename_id = 3;
    let spec = ir_spec(&p);
    let child = chain(&[spec]).slots[0]
        .as_ref()
        .unwrap()
        .child
        .clone()
        .unwrap();
    assert_eq!(child.convolution().unwrap(), Some(p));
    for end in 0..child.raw.len() {
        let mut short = child.clone();
        short.raw.truncate(end);
        assert!(short.convolution().is_err(), "accepted IRC prefix {end}");
    }
    let child = chain(&[send_spec(&[0.25, 0.5])]).slots[0]
        .as_ref()
        .unwrap()
        .child
        .clone()
        .unwrap();
    assert_eq!(
        child.send_levels().unwrap().unwrap().levels,
        [0.25, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    );
    for end in 0..child.raw.len() {
        let mut short = child.clone();
        short.raw.truncate(end);
        assert!(short.send_levels().is_err(), "accepted send prefix {end}");
    }
    let mut invalid = child.clone();
    invalid.raw[..4].copy_from_slice(&9i32.to_le_bytes());
    assert!(invalid.send_levels().is_err());
    let mut invalid = child;
    invalid.raw[4..8].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(invalid.send_levels().is_err());
}

#[test]
fn partitioned_convolution_matches_direct_fir_and_keeps_stereo_independent() {
    let mut taps = vec![0.0f32; 1051];
    for (index, value) in [
        (0, 0.5),
        (1, -0.125),
        (255, 0.25),
        (256, -0.3),
        (511, 0.1),
        (512, 0.2),
        (1049, 0.125),
        (1050, 0.01),
    ] {
        taps[index] = value;
    }
    let right: Vec<_> = taps.iter().map(|v| v * -0.75).collect();
    let ir = sample(vec![taps.clone(), right.clone()], 48000);
    let mut fx = build(&[chain(&[ir_spec(&ir_params())])], &ir);
    let frames = 4000;
    let left: Vec<f32> = (0..frames)
        .map(|i| {
            if i < 1200 {
                (i as f32 * 0.17).sin() * 0.1
            } else {
                0.0
            }
        })
        .collect();
    let mut input = left.clone();
    input.extend(vec![0.0; frames]);
    fx.process(&mut input, 2, frames);
    for i in 0..frames {
        let expected = (0..taps.len().min(i + 1))
            .map(|k| taps[k] as f64 * left[i - k] as f64)
            .sum::<f64>() as f32;
        assert!(
            (input[i] - expected).abs() < 2e-6,
            "FIR sample {i}: {} vs {expected}",
            input[i]
        );
        assert_eq!(input[frames + i], 0.0, "left signal leaked to right");
    }
    assert_eq!(fx.tail(), 1050);
    assert!(input[2250..frames].iter().all(|v| *v == 0.0));
    fx.reset();
    let mut impulse = vec![0.0; frames * 2];
    impulse[frames] = 1.0;
    fx.process(&mut impulse, 2, frames);
    for (i, tap) in right.iter().enumerate() {
        assert!((impulse[frames + i] - tap).abs() < 1e-6);
    }
    assert!(impulse[..frames].iter().all(|v| *v == 0.0));
}

#[test]
fn convolution_wet_dry_predelay_reverse_autogain_and_rate_change() {
    let mut p = ir_params();
    p.predelay_ms = 1.0;
    p.reverse = true;
    p.auto_gain = true;
    let ir = sample(vec![vec![1.0, 0.5]], 48000);
    let mut slots = chain(&[ir_spec(&p)]);
    slots.slots[0].as_mut().unwrap().out_level = 0.5;
    slots.slots[0].as_mut().unwrap().out_level_dry = 0.25;
    let mut fx = build(&[slots], &ir);
    let mut data = [0.0f32; 128];
    data[0] = 1.0;
    data[64] = 1.0;
    fx.process(&mut data, 2, 64);
    let gain = (1.0f32 / 2.5).sqrt();
    assert_eq!(data[0], 0.25);
    assert!((data[48] - 0.5 * 0.5 * gain).abs() < 1e-6);
    assert!((data[49] - 0.5 * gain).abs() < 1e-6);
    assert_eq!(fx.tail(), 49);
    let resampled_ir = sample(vec![vec![1.0]], 24000);
    let mut resampled = build(&[chain(&[ir_spec(&ir_params())])], &resampled_ir);
    let mut signal = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    resampled.process(&mut signal, 2, 4);
    assert_eq!(signal, [1.0, 0.5, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
    fx.prepare(96_000.0);
    assert!(!fx.failed());
    assert_eq!(fx.tail(), 99); // 4 resampled taps and 96 frames of predelay
    fx.prepare(0.0);
    assert!(fx.failed());
    let mut out = [1.0; 4];
    fx.process(&mut out, 2, 2);
    assert_eq!(out, [0.0; 4]);
    fx.prepare(48000.0);
    assert!(!fx.failed());
    let mut out = [0.0; 128];
    fx.process(&mut out, 2, 64);
    assert_eq!(out, [0.0; 128]);
}

#[test]
fn sends_tap_their_slot_feed_parallel_returns_then_master_processes_the_sum() {
    let ir = sample(vec![vec![0.0, 1.0]], 48000);
    let sends = chain(&[ir_spec(&ir_params()), ir_spec(&ir_params())]);
    let mut master = chain(&[super::fx_routing_tests::spread_spec(-1.0, 0.0, false)]);
    master.slots[0].as_mut().unwrap().out_level = 2.0;
    let inserts = chain(&[send_spec(&[0.5, 0.25])]);
    let mut fx = build(&[inserts, sends, master], &ir);
    let mut input = [1.0, 0.0, 0.0, 0.0, 0.0, 0.0];
    fx.process(&mut input, 2, 3);
    // Dry impulse + 0.5 and 0.25 parallel delayed returns, then mono and x2.
    assert_eq!(input, [1.0, 0.75, 0.0, 1.0, 0.75, 0.0]);
    let mut sparse = chain(&[ir_spec(&ir_params())]);
    let return_slot = sparse.slots.remove(0);
    sparse.slots = vec![None; 8];
    sparse.slots[7] = return_slot;
    let mut fx = build(
        &[
            chain(&[send_spec(&[0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.5])]),
            sparse,
        ],
        &ir,
    );
    let mut impulse = [1.0, 0.0, 0.0, 0.0];
    fx.process(&mut impulse, 2, 2);
    assert_eq!(impulse, [1.0, 0.5, 0.0, 0.0]);
    // A send before a muting insert keeps its tap; after it is silent.
    let mut mute = chain(&[super::fx_routing_tests::spread_spec(0.0, 0.0, false)])
        .slots
        .remove(0)
        .unwrap();
    mute.out_level = 0.0;
    let send = chain(&[send_spec(&[1.0])]).slots.remove(0).unwrap();
    for (slots, expected) in [
        (vec![Some(send.clone()), Some(mute.clone())], 1.0),
        (vec![Some(mute), Some(send)], 0.0),
    ] {
        let mut fx = build(
            &[
                FxChain {
                    version: 0x13,
                    slots,
                },
                chain(&[ir_spec(&ir_params())]),
            ],
            &ir,
        );
        let mut input = [1.0, 0.0, 0.0, 0.0];
        fx.process(&mut input, 2, 2);
        assert_eq!(input[1], expected);
    }
}

#[test]
fn group_sends_preserve_group_isolation_and_callback_storage() {
    let fixture = Fixture::new(16000);
    let ir = sample(vec![vec![0.0, 0.5, 0.25]], 48000);
    let configure = || {
        let mut plugin = fixture.plugin(false, None);
        plugin.groups[0].fx_chain = Some(chain(&[send_spec(&[1.0])]));
        // A second group plays the same note dry, without a send slot.
        let mut dry_group = plugin.groups[0].clone();
        dry_group.fx_chain = None;
        plugin.groups.push(dry_group);
        let mut zone = plugin.zones[0].clone();
        zone.group_index = 1;
        plugin.zones.push(zone);
        plugin.zone_samples = vec![None; 2];
        plugin.selection = GroupStart::new(&plugin.groups, &plugin.program);
        plugin.expression = Modulation::new(&plugin.groups, VOICE_POOL).unwrap();
        plugin.internal_mod = InternalMod::new(&plugin.groups);
        plugin.rebuild_voice_policies();
        plugin.rebuild_group_fx().unwrap();
        plugin.fx = Some(build(&[chain(&[]), chain(&[ir_spec(&ir_params())])], &ir));
        plugin.preload_all();
        plugin
    };
    let mut dry = fixture.plugin(false, None);
    dry.fx = None;
    dry.preload_all();
    let expected = advance(&mut dry, 48000.0, 1024, &[Event::note_on(0, 60, 127)]);
    let mut plugin = configure();
    let actual = advance(&mut plugin, 48000.0, 1024, &[Event::note_on(0, 60, 127)]);
    for i in 2..1024 {
        assert!(
            (actual[i] - 2.0 * expected[i] - 0.5 * expected[i - 1] - 0.25 * expected[i - 2]).abs()
                < 1e-6
        );
    }
    let mut context = ProcessContext {
        block_size: 64,
        ..Default::default()
    };
    plugin.reset();
    plugin.prepare(&context);
    let mut output = [0.0; 128];
    ALLOCS.with(|c| c.set(0));
    TRACK.with(|c| c.set(true));
    block(
        &mut plugin,
        &context,
        &mut output,
        &[Event::note_on(13, 60, 127)],
    );
    plugin.reset();
    TRACK.with(|c| c.set(false));
    assert_eq!(ALLOCS.with(Cell::get), 0);
    context.sample_pos = 64;
    block(&mut plugin, &context, &mut output, &[]);
    assert_eq!(output, [0.0; 128]);
}

#[test]
fn long_ir_block_boundaries_are_exact_and_callbacks_allocate_nothing() {
    let taps: Vec<_> = (0..1800)
        .map(|i| (i as f32 * 0.17).cos() * (-0.003 * i as f32).exp() * 0.01)
        .collect();
    let ir = sample(vec![taps], 48000);
    let render = |block: usize| {
        let mut fx = build(&[chain(&[ir_spec(&ir_params())])], &ir);
        let mut result = Vec::new();
        let mut output = vec![0.0; block * 2];
        for start in (0..6000).step_by(block) {
            let count = block.min(6000 - start);
            output.fill(0.0);
            for (i, sample) in output.iter_mut().take(count).enumerate() {
                if start + i < 900 {
                    *sample = (0.11 * (start + i) as f32).sin() * 0.2;
                }
            }
            ALLOCS.with(|c| c.set(0));
            TRACK.with(|c| c.set(true));
            fx.process(&mut output[..count * 2], 2, count);
            TRACK.with(|c| c.set(false));
            assert_eq!(ALLOCS.with(Cell::get), 0);
            result.extend_from_slice(&output[..count]);
        }
        ALLOCS.with(|c| c.set(0));
        TRACK.with(|c| c.set(true));
        fx.reset();
        TRACK.with(|c| c.set(false));
        assert_eq!(ALLOCS.with(Cell::get), 0);
        result
    };
    let reference = render(64);
    for size in [1, 127, 257, 1024] {
        let out = render(size);
        assert!(reference.iter().zip(out).all(|(a, b)| *a == b));
    }
}

#[test]
fn ir_resolution_is_explicit_and_supports_normal_sample_resource_paths() {
    let fixture = Fixture::new(1024);
    let mut plugin = fixture.plugin(false, None);
    plugin.loaded.instrument.fx_chains = vec![chain(&[ir_spec(&ir_params())])];
    plugin.loaded.instrument.sample_files = vec!["missing-ir.wav".into()];
    assert!(PostFx::load(&mut plugin.loaded).is_err());
    plugin.loaded.instrument.sample_files = vec!["sample.wav".into()];
    let mut fx = PostFx::load(&mut plugin.loaded).unwrap().unwrap();
    assert_eq!(fx.tail(), 1023);
    let mut output = vec![0.0; 1200 * 2];
    output[0] = 1.0;
    fx.process(&mut output, 2, 1200);
    let original = audio::load(&fs::read(&fixture.sample).unwrap()).unwrap();
    for (a, b) in output[..1024].iter().zip(&original.samples[0]) {
        assert!((a - b).abs() < 2e-6);
    }
    assert!(output[1200..].iter().all(|v| *v == 0.0));
    let mut bad = ir_params();
    bad.early_length = 0.75;
    assert!(PostFx::build(&[chain(&[ir_spec(&bad)])], &mut |_| Ok(original.clone())).is_err());
}

#[test]
fn bypass_missing_ir_feedback_and_preparation_bounds_are_explicit() {
    let ir = sample(vec![vec![1.0]], 48000);
    let mut bypass = ir_spec(&ir_params());
    bypass.bypass = true;
    assert!(PostFx::build(&[chain(&[bypass])], &mut |_| panic!(
        "bypassed IR was loaded"
    ))
    .unwrap()
    .is_none());
    assert!(
        PostFx::build(&[chain(&[]), chain(&[send_spec(&[1.0])])], &mut |_| Ok(
            ir.clone()
        ))
        .is_err()
    );
    let mut nonstereo = chain(&[ir_spec(&ir_params())]);
    nonstereo.slots[0].as_mut().unwrap().send_partition = 3;
    assert!(PostFx::build(&[chain(&[]), nonstereo], &mut |_| Ok(ir.clone())).is_err());
    let mut empty = ir_params();
    empty.filename_id = -1;
    let mut slots = chain(&[ir_spec(&empty)]);
    slots.slots[0].as_mut().unwrap().out_level_dry = 0.5;
    let mut fx = build(&[slots], &ir);
    let mut input = [1.0, -0.5];
    fx.process(&mut input, 2, 1);
    assert_eq!(input, [0.5, -0.25]);
    let mut bad_gain = chain(&[ir_spec(&empty)]);
    bad_gain.slots[0].as_mut().unwrap().out_level_dry = f32::NAN;
    assert!(PostFx::build(&[bad_gain], &mut |_| panic!(
        "unassigned IR resolved an asset"
    ))
    .is_err());
    let oversized_preparation = sample(vec![vec![0.0; 300_000], vec![0.0; 300_000]], 8000);
    let mut bounded =
        super::super::convolution::Convolution::new(oversized_preparation, ir_params()).unwrap();
    assert!(bounded.prepare(48_000.0, &mut 0).is_err());

    let too_large = audio::Sample {
        num_frames: usize::MAX,
        ..ir.clone()
    };
    assert!(super::super::convolution::Convolution::new(too_large, ir_params()).is_err());
    let mut fx = build(&[chain(&[ir_spec(&ir_params())])], &ir);
    fx.process(&mut [1.0, 1.0], 2, 1);
    fx.prepare(f32::NAN);
    assert!(fx.failed());
    fx.prepare(48000.0);
    let mut silence = [0.0; 16];
    fx.process(&mut silence, 2, 8);
    assert_eq!(silence, [0.0; 16]);
}

#[test]
fn piano_ir_is_resolved_and_has_a_tail_after_all_voices_are_killed_if_present() {
    let path = Path::new("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki");
    match fs::metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("SKIP Piano IR: absent fixture");
            return;
        }
        Err(e) => panic!("Piano IR metadata: {e}"),
        Ok(_) => {}
    }
    let mut plugin = SamplerPlugin::load(path).expect("installed Piano with IR must load");
    assert!(plugin.preload_note(60, 100) > 0);
    let context = ProcessContext {
        block_size: 256,
        ..Default::default()
    };
    plugin.prepare(&context);
    assert!(plugin.current_tail_samples() > 48000 * 3);
    assert!(plugin.convolution_memory_bytes() > 1_000_000);
    let mut output = [0.0; 512];
    let mut late_peak = 0f32;
    for index in 0..100 {
        let events = match index {
            0 => vec![Event::note_on(13, 60, 100)],
            2 => vec![Event::control_change(0, 120, 0)],
            _ => vec![],
        };
        block(&mut plugin, &context, &mut output, &events);
        if index > 10 {
            assert!(plugin.voices.iter().all(Option::is_none));
            late_peak = output.iter().fold(late_peak, |peak, v| peak.max(v.abs()));
        }
    }
    assert!(
        late_peak > 1e-5,
        "IR tail was absent after voices ended: {late_peak}"
    );
    println!("PIANO_IR {{\"tail_samples\":{},\"convolution_memory_bytes\":{},\"late_peak\":{late_peak}}}",plugin.current_tail_samples(),plugin.convolution_memory_bytes());
}
