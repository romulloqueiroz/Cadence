use crate::amp_dsp::{jump_tables, van51_tables, TableBase};

const CASE_BYTES: usize = 4 + 4 * 4 + 256 * 4 + 256 * 4;

fn read_case(data: &[u8], case: usize) -> (i32, [f32; 4], Vec<f32>, Vec<f32>) {
    let start = case * CASE_BYTES;
    let anchor = i32::from_le_bytes(data[start..start + 4].try_into().unwrap());
    let mut fields = [0.0f32; 4];
    for (index, field) in fields.iter_mut().enumerate() {
        let at = start + 4 + index * 4;
        *field = f32::from_le_bytes(data[at..at + 4].try_into().unwrap());
    }
    let rel_at = start + 4 + 4 * 4;
    let slope_at = rel_at + 256 * 4;
    let mut rel = Vec::with_capacity(256);
    let mut slope = Vec::with_capacity(256);
    for index in 0..256 {
        let at = rel_at + index * 4;
        rel.push(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()));
        let at = slope_at + index * 4;
        slope.push(f32::from_le_bytes(data[at..at + 4].try_into().unwrap()));
    }
    (anchor, fields, rel, slope)
}

fn compare(case: usize, table: &TableBase, data: &[u8]) {
    let (anchor, fields, rel, slope) = read_case(data, case);
    assert_eq!(table.anchor, anchor, "case {case} anchor");
    assert_eq!(
        table.fraction.to_bits(),
        fields[0].to_bits(),
        "case {case} fraction"
    );
    assert_eq!(
        table.scale.to_bits(),
        fields[1].to_bits(),
        "case {case} scale"
    );
    assert_eq!(
        table.offset.to_bits(),
        fields[2].to_bits(),
        "case {case} offset"
    );
    assert_eq!(
        table.baseline.to_bits(),
        fields[3].to_bits(),
        "case {case} baseline"
    );
    for index in 0..256 {
        assert_eq!(
            table.entries[index][0].to_bits(),
            rel[index].to_bits(),
            "case {case} entry {index} rel"
        );
        assert_eq!(
            table.entries[index][1].to_bits(),
            slope[index].to_bits(),
            "case {case} entry {index} slope"
        );
    }
}

#[test]
fn jump_tables_match_independent_setup_reference() {
    let data = include_bytes!("../tests/fixtures/jump-setup.bin");
    assert_eq!(data.len(), 6 * CASE_BYTES);
    for (case, table) in jump_tables().iter().enumerate() {
        compare(case, table, data);
    }
    compare(5, &TableBase::power_tube(), data);
}

#[test]
fn van51_tables_match_independent_setup_reference() {
    let data = include_bytes!("../tests/fixtures/van51-setup.bin");
    assert_eq!(data.len(), 12 * CASE_BYTES);
    for (case, table) in van51_tables().iter().enumerate() {
        compare(case, table, data);
    }
}

#[test]
fn jump_controls_match_independent_reference() {
    use crate::amp_dsp::{jump_eq_filters, jump_param_response};
    let data = include_str!("../tests/fixtures/jump-controls.tsv");
    let rate = 48_000.0f32;
    let rate_scale = std::f32::consts::TAU / rate;
    let rate_inv = 1.0 / rate;
    let eq_cases = [
        (0.3, 0.4, 0.5, 0.5),
        (0.5, 0.5, 0.5, 0.5),
        (0.1, 0.9, 0.2, 0.8),
        (0.8, 0.2, 0.9, 0.1),
        (0.0, 0.0, 0.0, 0.0),
        (1.0, 1.0, 1.0, 1.0),
    ];
    let responses = [-0.5, 0.0, 0.25, 0.75, 1.0];
    let mut max_error = 0.0f64;
    let mut values = 0;
    for line in data.lines().skip(1) {
        let fields: Vec<_> = line.split('\t').collect();
        let expected: f32 = fields[3].parse().unwrap();
        let case: usize = fields[1].parse().unwrap();
        let actual = if fields[0] == "eq" {
            let (p3, p4, p5, presence) = eq_cases[case];
            let c = jump_eq_filters(p3, p4, p5, presence, rate_scale);
            let all = [
                c.eq_b0, c.eq_b1, c.eq_c0, c.eq_c1, c.f_d8, c.f_dc, c.f_e8, c.f_ec, c.f_f0, c.f_f4,
                c.f_f8,
            ];
            all[match fields[2] {
                "eq_b0" => 0,
                "eq_b1" => 1,
                "eq_c0" => 2,
                "eq_c1" => 3,
                "f_d8" => 4,
                "f_dc" => 5,
                "f_e8" => 6,
                "f_ec" => 7,
                "f_f0" => 8,
                "f_f4" => 9,
                _ => 10,
            }]
        } else {
            let r = jump_param_response(responses[case], rate_scale, rate_inv, -0.0);
            let all = [r.f18, r.f20, r.f24, r.f28, r.f_c4, r.f_c0];
            all[match fields[2] {
                "resp_18" => 0,
                "resp_20" => 1,
                "resp_24" => 2,
                "resp_28" => 3,
                "resp_c4" => 4,
                _ => 5,
            }]
        };
        let error = (actual - expected).abs() as f64;
        max_error = max_error.max(error);
        assert!(
            error <= 2e-5 * (1.0 + expected.abs() as f64),
            "{line}: actual {actual} expected {expected}"
        );
        values += 1;
    }
    assert!(values > 0);
    eprintln!("JUMP_CONTROLS {{\"values\":{values},\"max_error\":{max_error}}}");
}

#[test]
fn jump_parameter_smoothing_matches_toolbox_semantics() {
    use crate::amp_dsp::Smoothing24;
    let mut smoothing = Smoothing24 {
        factor: 1.0 / 3840.0,
        periods: 30,
        ..Default::default()
    };
    for index in 0..24 {
        smoothing.targets[index] = index as f32 * 0.1;
    }
    smoothing.start_smooth();
    assert!(smoothing.pending);
    assert_eq!(smoothing.remaining, 30);
    for index in 0..24 {
        let expected = smoothing.targets[index] * smoothing.factor;
        assert!((smoothing.increments[index] - expected).abs() < 1e-9);
    }
    let before = smoothing.currents;
    smoothing.advance();
    for ((current, old), increment) in smoothing
        .currents
        .iter()
        .zip(before.iter())
        .zip(smoothing.increments.iter())
    {
        assert_eq!(*current, *old + *increment);
    }
    smoothing.snap();
    assert_eq!(smoothing.currents, smoothing.targets);
    assert!(!smoothing.pending);
    assert_eq!(smoothing.state, 4);

    smoothing.currents = [1e-8 - 1e-9; 24];
    smoothing.targets = [1e-8; 24];
    smoothing.start_smooth();
    assert!(!smoothing.pending);
    assert_eq!(smoothing.state, 4);
    assert_eq!(smoothing.currents, smoothing.targets);
}

#[test]
fn jump_tuning_constants_match_reconstructed_exp_table() {
    use crate::amp_dsp::JumpTuning;
    let t = JumpTuning::new();
    let values: [(f32, f64); 9] = [
        (t.c5e58, 4486.484937381137),
        (t.c5388, 86.30485698538787),
        (t.c5390, 87.30767050737116),
        (t.c63d0, 35479.62514476596),
        (t.c6060, 9956.13322941236),
        (t.c6178, 14917.345830693566),
        (t.c5890, 554.3691428869681),
        (t.c6128, 13289.844376229583),
        (t.c6150, 14080.09954390991),
    ];
    for (actual, expected) in values {
        let error = (actual as f64 - expected).abs() / expected.abs();
        assert!(error < 1e-6, "actual {actual} expected {expected}");
    }
}

#[test]
fn jump_rate_base_matches_reference_values() {
    use crate::amp_dsp::JumpRateBase;
    let base = JumpRateBase::new(48_000.0, 0.0, false);
    assert_eq!(base.period, 30);
    let checks = [
        (base.d0, 0.0028570524882525206),
        (base.d4, 1.0),
        (base.d8, 0.32580405473709106),
        (base.dc, 0.4881545901298523),
        (base.e8, 0.14681555330753326),
        (base.ec, 0.0028242364060133696),
        (base.e0, 0.01814115233719349),
        (base.f0, 0.4348962903022766),
        (base.f4, 0.4348962903022766),
        (base.f8, 0.4607565402984619),
        (base.fc, 0.4607565402984619),
        (base.f918, 0.00026754639111459255),
        (base.f920, 1.0533979548199568e-05),
    ];
    for (actual, expected) in checks {
        let error = (actual as f64 - expected).abs() / (1.0 + expected.abs());
        assert!(error < 1e-4, "actual {actual} expected {expected}");
    }
    assert!((base.smooth_factor - 0.00026041668).abs() < 1e-8);
    assert!((base.state_gain - 100.0 / 192_000.0).abs() < 1e-9);
}

#[test]
fn jump_rate_coefficients_match_independent_reference() {
    use crate::amp_dsp::JumpRateCoefficients;
    let reference = include_str!("../tests/fixtures/jump-rate.tsv");
    let mut case = None;
    let mut coefficients = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut comparisons = 0;
    let mut max_relative_error = 0.0f64;
    for line in reference.lines().skip(1) {
        let fields: Vec<_> = line.split('\t').collect();
        assert_eq!(fields.len(), 10, "{line}");
        let id: usize = fields[0].parse().unwrap();
        if case != Some(id) {
            case = Some(id);
            let input = |index: usize| fields[index].parse::<f32>().unwrap();
            coefficients = Some(JumpRateCoefficients::new(
                input(1),
                input(2),
                input(3),
                input(4),
                input(5),
                input(6),
                fields[7] == "1",
            ));
        }
        let c = coefficients.as_ref().unwrap();
        let offset = usize::from_str_radix(fields[8].trim_start_matches("0x"), 16).unwrap();
        let expected: f32 = fields[9].parse().unwrap();
        let actual = match offset {
            0x4490 => c.base.rate_x4,
            0x4494 => c.base.rate_inv,
            0x4498 => c.base.rate_scale,
            0x4618 => c.base.period as f32,
            0x461c => c.base.smooth_factor,
            0x2e4 => c.base.state_gain,
            0x453c => c.drive_current,
            0x4630..=0x46ac => {
                let index = (offset - 0x4630) / 4;
                c.prototypes[index / 4][index % 4]
            }
            0x4730..=0x489c => {
                let index = (offset - 0x4730) / 4;
                c.circuit[index / 4][index % 4]
            }
            0x48d0..=0x492c => {
                let index = (offset - 0x48d0) / 4;
                c.base.vectors()[index / 4][index % 4]
            }
            0x49c0 => c.response[0],
            0x49c4 => c.response[1],
            _ => panic!("unmapped offset {offset:x}"),
        };
        assert!(actual.is_finite() && expected.is_finite(), "{line}");
        // Scalar division replaces two NEON reciprocal refinements. Allow a
        // small relative rounding error; require exact zeros to catch bad lane
        // packing, and do not let large prototype magnitudes hide small values.
        if expected == 0.0 {
            assert_eq!(actual, 0.0, "case {id} offset {offset:x}");
        } else {
            let relative = ((actual as f64 - expected as f64) / expected as f64).abs();
            max_relative_error = max_relative_error.max(relative);
            assert!(relative < 2e-6, "case {id} offset {offset:x}: actual {actual}, expected {expected}, relative {relative}");
        }
        assert!(seen.insert((id, offset)), "duplicate reference row {line}");
        comparisons += 1;
    }
    assert!(
        case.unwrap() >= 4,
        "reference must span multiple sample rates"
    );
    // 7 scalar fields + 8 prototype rows + 23 circuit rows + 6 rate rows +
    // 2 response fields, all four lanes of each vector.
    let cases = seen
        .iter()
        .map(|(id, _)| id)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(comparisons, cases.len() * (7 + 32 + 92 + 24 + 2));
    eprintln!("JUMP_RATE {{\"cases\":{},\"values\":{comparisons},\"max_relative_error\":{max_relative_error}}}", cases.len());
}

#[test]
fn jump_processing_and_coefficient_ramps_allocate_nothing() {
    use crate::amp_dsp::JumpAmp;
    use nk_format::fx::{AmpKind, AmpSettings};
    let mut settings = AmpSettings {
        kind: AmpKind::Jump,
        values: [0.5; 17],
        value_count: 6,
        flags: [false; 8],
        flag_count: 2,
        modes: [0; 4],
    };
    let mut amp = JumpAmp::new(settings);
    amp.prepare(48000.0).unwrap();
    // Preparing setters is off the realtime thread; processing their fade must
    // remain allocation-free, including the first drive coefficient rebuild.
    settings.values[0] = 0.9;
    settings.flags[0] = true;
    amp.set_settings(settings);
    super::ALLOCS.with(|v| v.set(0));
    super::TRACK.with(|v| v.set(true));
    let mut finite = true;
    for _ in 0..2048 {
        finite &= amp.process(0.1, -0.2).iter().all(|v| v.is_finite());
    }
    super::TRACK.with(|v| v.set(false));
    assert!(finite);
    assert_eq!(super::ALLOCS.with(|v| v.get()), 0);
}

#[test]
fn van51_processing_and_coefficient_ramps_allocate_nothing() {
    use crate::van51_amp::Van51Amp;
    use nk_format::fx::{AmpKind, AmpSettings};
    let mut settings = AmpSettings {
        kind: AmpKind::Van51,
        values: [0.3; 17],
        value_count: 8,
        flags: [false; 8],
        flag_count: 5,
        modes: [0; 4],
    };
    let mut amp = Van51Amp::new(settings);
    settings.values[..8].fill(0.8);
    settings.flags[..4].fill(true);
    amp.set_settings(settings);
    super::ALLOCS.with(|v| v.set(0));
    super::TRACK.with(|v| v.set(true));
    let mut finite = true;
    for _ in 0..2048 {
        finite &= amp.process(0.1, -0.2).iter().all(|v| v.is_finite());
    }
    amp.reset();
    super::TRACK.with(|v| v.set(false));
    assert!(finite);
    assert_eq!(super::ALLOCS.with(|v| v.get()), 0);
}

// Integration tests exercise the serialized effect boundary. Numerical parity
// belongs to each circuit's independent native fixtures, above and in its module.
// Reset checks target the sampler's deterministic rewind contract. EPPreamps
// deliberately uses its prepared-state restart because native onReset retains
// some state; these checks do not claim equivalence to that native lifecycle.
use super::{fx_chain, FxSpec, PostFx};
use nk_format::fx::{AmpKind, AmpSettings, FxChain};

#[derive(Clone, Copy, Debug)]
struct AmpRecord {
    kind: AmpKind,
    ser_type: u16,
    version: u16,
    values: usize,
    flags: usize,
}
const AMP_RECORDS: [AmpRecord; 9] = [
    AmpRecord {
        kind: AmpKind::Jump,
        ser_type: 77,
        version: 0x10,
        values: 6,
        flags: 2,
    },
    AmpRecord {
        kind: AmpKind::Van51,
        ser_type: 82,
        version: 0x10,
        values: 8,
        flags: 5,
    },
    AmpRecord {
        kind: AmpKind::AcBox,
        ser_type: 83,
        version: 0x10,
        values: 7,
        flags: 1,
    },
    AmpRecord {
        kind: AmpKind::HotSolo,
        ser_type: 84,
        version: 0x10,
        values: 8,
        flags: 2,
    },
    AmpRecord {
        kind: AmpKind::BassInvader,
        ser_type: 84,
        version: 0x51,
        values: 7,
        flags: 3,
    },
    AmpRecord {
        kind: AmpKind::BassInvader,
        ser_type: 84,
        version: 0x52,
        values: 7,
        flags: 4,
    },
    AmpRecord {
        kind: AmpKind::Cat,
        ser_type: 85,
        version: 0x10,
        values: 7,
        flags: 1,
    },
    AmpRecord {
        kind: AmpKind::BassPro,
        ser_type: 97,
        version: 0x10,
        values: 17,
        flags: 5,
    },
    AmpRecord {
        kind: AmpKind::EpPreamps,
        ser_type: 108,
        version: 0x11,
        values: 11,
        flags: 6,
    },
];
fn serialized_amp(record: AmpRecord, bypass: bool) -> (FxSpec, AmpSettings) {
    let values = [
        0.41f32, 0.53, 0.37, 0.64, 0.48, 0.55, 0.26, 0.73, 0., 0.12, 0.27, 0.61, 0.46, 0.36, 0.57,
        0.68, 0.44,
    ];
    let flags = match record.kind {
        AmpKind::Jump | AmpKind::HotSolo => [true, false, false, false, false, false],
        AmpKind::Van51 => [true, false, true, false, false, false],
        AmpKind::BassInvader => [true, false, true, false, false, false],
        AmpKind::BassPro => [true, false, true, true, false, false],
        AmpKind::EpPreamps => [true, false, true, false, true, false],
        _ => [false; 6],
    };
    let modes = [3i32, 4, 5, 3];
    let mut raw = Vec::new();
    for value in &values[..record.values] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    if record.kind == AmpKind::EpPreamps {
        raw.extend(flags[..2].iter().copied().map(u8::from));
        for mode in modes {
            raw.extend_from_slice(&mode.to_le_bytes());
        }
        raw.extend(flags[2..].iter().copied().map(u8::from));
    } else {
        raw.extend(flags[..record.flags].iter().copied().map(u8::from));
    }
    let spec = FxSpec {
        ser_type: record.ser_type,
        version: record.version,
        raw,
        bypass,
    };
    let parsed = FxChain::parse(&fx_chain(std::slice::from_ref(&spec))).unwrap();
    let settings = parsed.slots[0]
        .as_ref()
        .unwrap()
        .child
        .as_ref()
        .unwrap()
        .amp_settings()
        .unwrap();
    assert_eq!(settings.kind, record.kind);
    assert_eq!(settings.values(), &values[..record.values]);
    assert_eq!(&settings.flags()[..record.flags], &flags[..record.flags]);
    if record.version == 0x51 {
        assert!(settings.flags()[3], "legacy BassInvader defaults to mono");
    }
    if record.kind == AmpKind::EpPreamps {
        assert_eq!(settings.modes, modes);
    }
    (spec, settings)
}
fn amp_chain(record: AmpRecord) -> (FxChain, AmpSettings) {
    let (spec, settings) = serialized_amp(record, false);
    (FxChain::parse(&fx_chain(&[spec])).unwrap(), settings)
}
trait AmpCircuit {
    fn reset(&mut self);
    fn set_settings(&mut self, settings: AmpSettings);
    fn prepare(&mut self, rate: f32) -> Result<(), &'static str>;
    fn process(&mut self, left: f32, right: f32) -> [f32; 2];
}
macro_rules! circuit {
    ($ty:path) => {
        impl AmpCircuit for $ty {
            fn reset(&mut self) {
                <$ty>::reset(self)
            }
            fn set_settings(&mut self, settings: AmpSettings) {
                <$ty>::set_settings(self, settings)
            }
            fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
                <$ty>::prepare(self, rate)
            }
            fn process(&mut self, left: f32, right: f32) -> [f32; 2] {
                <$ty>::process(self, left, right)
            }
        }
    };
}
circuit!(crate::jump_dsp::JumpAmp);
circuit!(crate::van51_amp::Van51Amp);
circuit!(crate::acbox_dsp::AcBoxAmp);
circuit!(crate::hotsolo_dsp::HotSoloAmp);
circuit!(crate::bassinvader_dsp::BassInvaderAmp);
circuit!(crate::cat_amp::CatAmp);
circuit!(crate::basspro_amp::BassProAmp);
circuit!(crate::epiano_amp::EPianoAmp);
fn direct_amp(settings: AmpSettings) -> Box<dyn AmpCircuit> {
    match settings.kind {
        AmpKind::Jump => Box::new(crate::jump_dsp::JumpAmp::new(settings)),
        AmpKind::Van51 => Box::new(crate::van51_amp::Van51Amp::new(settings)),
        AmpKind::AcBox => Box::new(crate::acbox_dsp::AcBoxAmp::new(settings)),
        AmpKind::HotSolo => Box::new(crate::hotsolo_dsp::HotSoloAmp::new(settings)),
        AmpKind::BassInvader => Box::new(crate::bassinvader_dsp::BassInvaderAmp::new(settings)),
        AmpKind::Cat => Box::new(crate::cat_amp::CatAmp::new(settings)),
        AmpKind::BassPro => Box::new(crate::basspro_amp::BassProAmp::new(settings)),
        AmpKind::EpPreamps => Box::new(crate::epiano_amp::EPianoAmp::new(settings)),
    }
}
fn amp_input(frames: usize) -> Vec<f32> {
    let mut result = vec![0.; frames * 2];
    for i in 0..frames {
        result[i] = (i as f32 * 0.071).sin() * 0.12;
        result[frames + i] = (i as f32 * 0.043).cos() * 0.07;
    }
    result
}

fn assert_amp_audio(actual: &[f32], expected: &[f32], record: AmpRecord, context: &str) {
    assert_eq!(actual.len(), expected.len());
    for (index, (actual, expected)) in actual.iter().zip(expected).enumerate() {
        assert!(
            actual == expected,
            "{context}: {record:?} sample {index}: {actual} vs {expected}"
        );
    }
}

#[test]
fn serialized_amps_reach_their_circuits_in_inserts_master_and_sends() {
    let frames = 257;
    for record in AMP_RECORDS {
        let (chain, settings) = amp_chain(record);
        let mut direct = direct_amp(settings);
        direct.prepare(44_100.).unwrap();
        let input = amp_input(frames);
        let mut expected = vec![0.; frames * 2];
        for i in 0..frames {
            let out = direct.process(input[i], input[frames + i]);
            expected[i] = out[0] * 0.65;
            expected[frames + i] = out[1] * 0.65;
        }
        for role in 0..3 {
            let mut selected = chain.clone();
            let slot = selected.slots[0].as_mut().unwrap();
            slot.out_level = 0.65;
            // Native amp wrappers use SmoothVolumeAndOut without the dry-mix path.
            slot.out_level_dry = 0.125;
            let mut chains = vec![FxChain::parse(&fx_chain(&[])).unwrap(); role + 1];
            chains[role] = selected;
            let mut fx = PostFx::from_chains(&chains).unwrap();
            fx.prepare(44_100.);
            assert!(!fx.failed(), "{record:?}");
            let mut actual = if role == 1 {
                vec![0.; frames * 2]
            } else {
                input.clone()
            };
            let mut aux = vec![[[0.; 2]; 8]; frames];
            if role == 1 {
                for i in 0..frames {
                    aux[i][0] = [input[i], input[frames + i]];
                }
            }
            fx.process_range(&mut actual, 2, frames, 0, &mut aux);
            assert_amp_audio(
                &actual,
                &expected,
                record,
                &format!("serialized circuit/routing role {role}"),
            );
        }
        let (bypass, _) = serialized_amp(record, true);
        let (active, _) = serialized_amp(record, false);
        let mut slots = FxChain::parse(&fx_chain(&[bypass, active])).unwrap();
        slots.slots[0].as_mut().unwrap().out_level = 0.;
        slots.slots[0].as_mut().unwrap().out_level_dry = 0.;
        let mut fx = PostFx::from_chains(&[slots]).unwrap();
        fx.prepare(44_100.);
        let mut actual = input.clone();
        fx.process(&mut actual, 2, frames);
        let mut reference = PostFx::from_chains(&[chain]).unwrap();
        reference.prepare(44_100.);
        let mut expected = input;
        reference.process(&mut expected, 2, frames);
        assert_amp_audio(
            &actual,
            &expected,
            record,
            "bypassed amp changes serial chain",
        );
    }
}

#[test]
fn serialized_amps_preserve_state_across_irregular_process_ranges_without_heap_activity() {
    let frames = 769;
    let partitions = [1usize, 31, 2, 63, 5, 127, 3, 7];
    for record in AMP_RECORDS {
        let (chain, _) = amp_chain(record);
        let mut whole = PostFx::from_chains(std::slice::from_ref(&chain)).unwrap();
        let mut split = PostFx::from_chains(&[chain]).unwrap();
        whole.prepare(96_000.);
        split.prepare(96_000.);
        let mut expected = amp_input(frames);
        let mut actual = expected.clone();
        whole.process(&mut expected, 2, frames);
        let mut aux = [[[0.; 2]; 8]; 127];
        let mut cursor = 0;
        let mut partition = 0;
        super::ALLOCS.with(|v| v.set(0));
        super::TRACK.with(|v| v.set(true));
        while cursor < frames {
            let count = partitions[partition % partitions.len()].min(frames - cursor);
            split.process_range(&mut actual, 2, frames, cursor, &mut aux[..count]);
            cursor += count;
            partition += 1;
        }
        split.reset();
        super::TRACK.with(|v| v.set(false));
        let allocations = super::ALLOCS.with(|v| v.get());
        assert_eq!(
            allocations, 0,
            "processing/reset allocated or freed heap storage: {record:?}"
        );
        assert_amp_audio(
            &actual,
            &expected,
            record,
            "block partition changes amp state",
        );
        let mut repeated = amp_input(frames);
        split.process(&mut repeated, 2, frames);
        assert_amp_audio(
            &repeated,
            &expected,
            record,
            "reset does not restore circuit/converter history",
        );
    }
}

#[test]
fn serialized_amps_prepare_rejects_invalid_rates_and_recovers_deterministically() {
    let frames = 385;
    for record in AMP_RECORDS {
        let (chain, _) = amp_chain(record);
        let mut fx = PostFx::from_chains(&[chain]).unwrap();
        for rate in [44_100., 48_000., 96_000., 192_000.] {
            fx.prepare(rate);
            assert!(!fx.failed(), "valid rate {rate} rejected: {record:?}");
            let mut first = amp_input(frames);
            fx.process(&mut first, 2, frames);
            assert!(
                first.iter().all(|v| v.is_finite()),
                "nonfinite output rate {rate}: {record:?}"
            );
            fx.reset();
            let mut repeated = amp_input(frames);
            fx.process(&mut repeated, 2, frames);
            assert_amp_audio(&repeated, &first, record, &format!("reset at rate {rate}"));
            fx.prepare(rate);
            let mut prepared_again = amp_input(frames);
            fx.process(&mut prepared_again, 2, frames);
            assert_amp_audio(
                &prepared_again,
                &first,
                record,
                &format!("prepare at rate {rate}"),
            );
        }
        for rate in [0., -1., f32::NAN, f32::INFINITY] {
            fx.prepare(rate);
            assert!(fx.failed(), "invalid rate accepted {rate}: {record:?}");
            let mut muted = amp_input(frames);
            fx.process(&mut muted, 2, frames);
            assert!(
                muted.iter().all(|v| *v == 0.),
                "failed prepare must silence the route: {record:?}"
            );
        }
        fx.prepare(48_000.);
        assert!(!fx.failed(), "valid prepare did not recover {record:?}");
        let mut recovered = amp_input(frames);
        fx.process(&mut recovered, 2, frames);
        assert!(recovered.iter().all(|v| v.is_finite()));
        assert!(
            recovered.iter().any(|v| v.abs() > 1e-8),
            "recovered amp is silent {record:?}"
        );
    }
}

#[test]
fn amp_live_control_ramps_and_resets_remain_allocation_free() {
    for record in AMP_RECORDS {
        let (_, mut settings) = amp_chain(record);
        let mut amp = direct_amp(settings);
        amp.prepare(48_000.).unwrap();
        settings.values[0] = 0.68;
        settings.values[1] = 0.29;
        settings.values[2] = 0.71;
        for flag in settings.flags.iter_mut().take(settings.flag_count as usize) {
            *flag = !*flag;
        }
        if record.kind == AmpKind::EpPreamps {
            settings.modes = [4, 5, 6, 4];
        }
        // Targets are supplied off the callback; deferred coefficient rebuilds,
        // smoothing and history resets must not allocate or free audio storage.
        amp.set_settings(settings);
        super::ALLOCS.with(|v| v.set(0));
        super::TRACK.with(|v| v.set(true));
        let mut finite = true;
        for frame in 0..1024 {
            let x = (frame as f32 * 0.071).sin() * 0.1;
            finite &= amp.process(x, -x * 0.3).iter().all(|v| v.is_finite());
        }
        amp.reset();
        for _ in 0..64 {
            finite &= amp.process(0.05, -0.03).iter().all(|v| v.is_finite());
        }
        super::TRACK.with(|v| v.set(false));
        let allocations = super::ALLOCS.with(|v| v.get());
        assert_eq!(
            allocations, 0,
            "amp ramp/reset allocated or freed heap storage {record:?}"
        );
        assert!(finite, "nonfinite amp control transition {record:?}");
    }
}
