use crate::cabinet_dsp::{set_cabinet, CabinetState};
use nk_format::fx::CabinetParams;

fn params(size: f32, air: f32, treble: f32, bass: f32, cabinet_type: i32) -> CabinetParams {
    CabinetParams {
        size,
        air,
        treble,
        bass,
        cabinet_type,
    }
}

#[test]
fn cabinet_coefficients_match_independent_static_reference() {
    let data = include_str!("../tests/fixtures/cabinet-coefficients.tsv");
    let mut case: Vec<(f32, f32, f32, f32, i32)> = Vec::new();
    let mut sections: Vec<[[f32; 4]; 24]> = Vec::new();
    let mut gains: Vec<[f32; 4]> = Vec::new();
    let mut max_error = 0.0f64;
    let mut rows = 0;
    for line in data.lines().skip(1) {
        let r: Vec<_> = line.split('\t').collect();
        let case_id: usize = r[0].parse().unwrap();
        let cabinet: i32 = r[1].parse().unwrap();
        let size: f32 = r[2].parse().unwrap();
        let air: f32 = r[3].parse().unwrap();
        let treble: f32 = r[4].parse().unwrap();
        let bass: f32 = r[5].parse().unwrap();
        let section: i32 = r[6].parse().unwrap();
        let coefficient: usize = r[7].parse().unwrap();
        let value: f32 = r[8].parse().unwrap();
        if case_id == case.len() {
            case.push((size, air, treble, bass, cabinet));
            sections.push([[0.0; 4]; 24]);
            gains.push([0.0; 4]);
            let (_, expected) = set_cabinet(size, air, treble, bass, cabinet, 48_000.0);
            gains[case_id] = expected;
        }
        assert!(case_id < case.len());
        if section < 0 {
            let error = (gains[case_id][coefficient] - value).abs() as f64;
            max_error = max_error.max(error);
            assert!(error <= 1e-6 * (1.0 + value.abs() as f64), "{line}");
        } else {
            sections[case_id][section as usize][coefficient] = value;
            rows += 1;
        }
    }
    for (case_id, &(size, air, treble, bass, cabinet)) in case.iter().enumerate() {
        let (actual, _) = set_cabinet(size, air, treble, bass, cabinet, 48_000.0);
        for section in 0..24 {
            for coefficient in 0..4 {
                let expected = sections[case_id][section][coefficient];
                let error = (actual[section][coefficient] - expected).abs() as f64;
                max_error = max_error.max(error);
                assert!(
                    error <= 1e-6 * (1.0 + expected.abs() as f64),
                    "case {case_id} section {section} coefficient {coefficient}: \
                     actual {} expected {expected}",
                    actual[section][coefficient],
                );
            }
        }
    }
    assert!(rows > 0);
    eprintln!(
        "CABINET_COEFFICIENTS {{\"cases\":{},\"values\":{},\"max_error\":{max_error}}}",
        case.len(),
        rows + case.len() * 4,
    );
}

#[test]
fn cabinet_process_matches_independent_static_reference() {
    let data = include_str!("../tests/fixtures/cabinet-frames.tsv");
    let mut max_error = 0.0f64;
    let mut frames = 0;
    let mut current: Option<(i32, i32, CabinetState)> = None;
    let mut sequence = String::new();
    let mut group_frame = 0u32;
    for line in data.lines().skip(1) {
        let r: Vec<_> = line.split('\t').collect();
        let rate: f32 = r[0].parse().unwrap();
        let cabinet: i32 = r[1].parse().unwrap();
        let frame: u32 = r[2].parse().unwrap();
        let left: f32 = r[3].parse().unwrap();
        let right: f32 = r[4].parse().unwrap();
        let expected_l: f32 = r[5].parse().unwrap();
        let expected_r: f32 = r[6].parse().unwrap();
        let key = format!("{rate}:{cabinet}");
        if sequence != key {
            let (size, air, treble, bass) = if cabinet == 0 {
                (0.5, 0.5, 0.5, 0.5)
            } else {
                (0.25, 0.75, 0.2, 0.9)
            };
            let mut state = CabinetState::new(params(size, air, treble, bass, cabinet));
            state.prepare(rate).unwrap();
            current = Some((rate as i32, cabinet, state));
            sequence = key;
            group_frame = 0;
        }
        assert_eq!(frame, group_frame);
        group_frame += 1;
        let (_, _, state) = current.as_mut().unwrap();
        let actual = state.process(left, right);
        for (actual, expected) in [(actual[0], expected_l), (actual[1], expected_r)] {
            let error = (actual - expected).abs() as f64;
            max_error = max_error.max(error);
            assert!(
                error <= 2e-6 * (1.0 + expected.abs() as f64),
                "rate {rate} cabinet {cabinet} frame {frame}: actual {actual} expected {expected}"
            );
        }
        frames += 1;
    }
    assert!(frames > 0);
    eprintln!("CABINET_FRAMES {{\"frames\":{frames},\"max_error\":{max_error}}}");
}

#[test]
fn cabinet_wire_layout_reads_four_floats_and_a_signed_type() {
    let mut raw = Vec::new();
    for value in [0.1f32, 0.2, 0.3, 0.4] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend_from_slice(&(-3i32).to_le_bytes());
    let child = nk_format::fx::FxChild {
        ser_type: 36,
        version: 0x50,
        private: Vec::new(),
        public: Vec::new(),
        children: Vec::new(),
        raw,
    };
    let parsed = child.cabinet_params().unwrap();
    assert_eq!(parsed.size, 0.1);
    assert_eq!(parsed.air, 0.2);
    assert_eq!(parsed.treble, 0.3);
    assert_eq!(parsed.bass, 0.4);
    assert_eq!(parsed.cabinet_type, -3);

    let mut bad = child.clone();
    bad.raw[0..4].copy_from_slice(&f32::NAN.to_le_bytes());
    assert!(bad.cabinet_params().is_none());
    let mut old = child;
    old.version = 0x51;
    assert!(old.cabinet_params().is_none());
}

#[test]
fn cabinet_chain_parses_the_serialized_child() {
    use nk_format::fx::FxChain;
    let mut raw = Vec::new();
    for value in [0.5f32, 0.25, 0.75, 0.1] {
        raw.extend_from_slice(&value.to_le_bytes());
    }
    raw.extend_from_slice(&9i32.to_le_bytes());
    let chain = FxChain::parse(&super::fx_chain(&[super::FxSpec {
        ser_type: 36,
        version: 0x50,
        raw,
        bypass: false,
    }]))
    .unwrap();
    let params = chain.slots[0]
        .as_ref()
        .unwrap()
        .child
        .as_ref()
        .unwrap()
        .cabinet_params()
        .unwrap();
    assert_eq!(params.size, 0.5);
    assert_eq!(params.cabinet_type, 9);
}
