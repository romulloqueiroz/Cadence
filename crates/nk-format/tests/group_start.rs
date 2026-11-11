use nk_format::{
    model::{Cc64Mode, Group, ProgramInfo},
    plain_model,
    preset_chunk::{self, PresetChunk},
    start_criteria::{CriteriaLogic as Logic, StartCondition as Condition, StartCriterion},
    Element,
};

const XML: &str = include_str!("fixtures/plain-playback.xml");

fn xml_criteria(criteria: &str) -> Result<nk_format::model::Instrument, nk_format::Error> {
    plain_model::build(&Element::parse(&XML.replacen(
        "<GroupStart/>",
        &format!("<GroupStart>{criteria}</GroupStart>"),
        1,
    ))?)
}

#[test]
fn xml_maps_all_supported_conditions_connectors_and_zero_based_cycles() {
    let instrument = xml_criteria(r#"
      <StartCriteria><V name="mode" value="on_key"/><V name="key_min" value="24"/><V name="key_max" value="25"/><V name="nextCriteria" value="and_not"/></StartCriteria>
      <StartCriteria><V name="mode" value="on_controller"/><V name="controller" value="1"/><V name="cc_min" value="64"/><V name="cc_max" value="127"/><V name="nextCriteria" value="or"/></StartCriteria>
      <StartCriteria><V name="mode" value="cycle_round_robin"/><V name="cycleClass" value="2"/></StartCriteria>
      <StartCriteria><V name="mode" value="cycle_random"/><V name="cycleClass" value="99"/></StartCriteria>"#).unwrap();
    assert_eq!(
        instrument.groups[0].start_criteria,
        [
            StartCriterion {
                condition: Condition::Key { low: 24, high: 25 },
                next: Logic::AndNot
            },
            StartCriterion {
                condition: Condition::Controller {
                    controller: 1,
                    low: 64,
                    high: 127
                },
                next: Logic::Or
            },
            StartCriterion {
                condition: Condition::RoundRobin { position: 2 },
                next: Logic::And
            },
            StartCriterion {
                condition: Condition::Random,
                next: Logic::And
            },
        ]
    );
    assert!(instrument.groups[1].start_criteria.is_empty());
    let legacy = xml_criteria(r#"<StartCriteria><V name="mode" value="on_key"/><V name="key" value="35"/></StartCriteria>"#).unwrap();
    assert_eq!(
        legacy.groups[0].start_criteria[0].condition,
        Condition::Key { low: 35, high: 35 }
    );
}

#[test]
fn xml_rejects_unknown_slice_excess_and_invalid_conditional_layers() {
    for body in [
        r#"<V name="mode" value="slice_trigger"/>"#,
        r#"<V name="mode" value="round_robin"/>"#,
        r#"<V name="mode" value="on_controller"/><V name="controller" value="128"/>"#,
        r#"<V name="mode" value="on_key"/><V name="key_min" value="50"/><V name="key_max" value="20"/>"#,
        r#"<V name="mode" value="cycle_round_robin"/><V name="cycleClass" value="-1"/>"#,
        r#"<V name="mode" value="cycle_random"/><V name="nextCriteria" value="xor"/>"#,
        r#"<V name="mode" value="cycle_random"/><V name="mode" value="on_key"/>"#,
    ] {
        assert!(
            xml_criteria(&format!("<StartCriteria>{body}</StartCriteria>")).is_err(),
            "accepted {body}"
        );
    }
    assert!(xml_criteria(&"<StartCriteria/>".repeat(5)).is_err());
    assert!(xml_criteria("<Unknown/>").is_err());
    assert!(xml_criteria("</GroupStart><GroupStart>").is_err());
}

// Synthetic bytes derived from the recovered BParStartCriteria doRead/doWrite
// layout, not a Kontakt-produced active-criteria fixture.
fn record(
    mode: i32,
    next: i32,
    keys: (i16, i16),
    controller: i16,
    cc: (i16, i16),
    cycle: i32,
) -> Vec<u8> {
    let mut b = vec![0, 0x70, 0]; // unstructured SER object + structure version
    for n in [mode, next] {
        b.extend(n.to_le_bytes());
    }
    for n in [keys.0, keys.1, controller, cc.0, cc.1] {
        b.extend(n.to_le_bytes());
    }
    for n in [cycle, -1, -1] {
        b.extend(n.to_le_bytes());
    }
    b.push(0);
    assert_eq!(b.len(), 34);
    b
}

fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
    let mut b = id.to_le_bytes().to_vec();
    b.extend((body.len() as u32).to_le_bytes());
    b.extend(body);
    b
}

fn group_data() -> Vec<u8> {
    let mut b = 0u32.to_le_bytes().to_vec(); // empty UTF16 name
    for n in [1f32, 0., 1.] {
        b.extend(n.to_le_bytes());
    }
    b.extend([1, 0, 0, 0]);
    b.extend(0i32.to_le_bytes());
    b.extend(0i16.to_le_bytes());
    b.extend([0; 8]);
    b.extend([0, 0]);
    b.extend(0i32.to_le_bytes());
    b
}

fn binary_group(criteria: &[u8]) -> Result<Group, nk_format::Error> {
    // Include a following unrelated child: the mask and embedded SER records
    // must not consume or flatten its framing.
    let mut wire = chunk(0x38, criteria);
    wire.extend(chunk(0x777, &[9, 8, 7]));
    let children = preset_chunk::parse_all(&wire)?;
    assert_eq!(children.len(), 2);
    assert_eq!(children[1].public_data, [9, 8, 7]);
    Group::parse_chunk(&PresetChunk {
        public_data: group_data(),
        children,
        ..PresetChunk::default()
    })
}

#[test]
fn binary_presence_mask_uses_four_raw_ser_records_and_preserves_order() {
    let mut payload = vec![0b1101];
    payload.extend(record(1, 1, (24, 25), -1, (-1, -1), -1));
    payload.extend(record(2, 2, (-1, -1), 11, (20, 90), -1));
    payload.extend(record(3, 0, (-1, -1), -1, (-1, -1), 7));
    assert_eq!(
        binary_group(&payload).unwrap().start_criteria,
        [
            StartCriterion {
                condition: Condition::Key { low: 24, high: 25 },
                next: Logic::AndNot
            },
            StartCriterion {
                condition: Condition::Controller {
                    controller: 11,
                    low: 20,
                    high: 90
                },
                next: Logic::Or
            },
            StartCriterion {
                condition: Condition::RoundRobin { position: 7 },
                next: Logic::And
            },
        ]
    );
    assert!(binary_group(&[0]).unwrap().start_criteria.is_empty());
    let mut payload = vec![3];
    payload.extend(record(0, 0, (-1, -1), -1, (-1, -1), -1));
    payload.extend(record(4, 2, (-1, -1), -1, (-1, -1), 99));
    assert_eq!(
        binary_group(&payload).unwrap().start_criteria,
        [StartCriterion {
            condition: Condition::Random,
            next: Logic::Or
        }]
    );
}

#[test]
fn binary_unknown_versions_modes_masks_and_truncated_records_fail_explicitly() {
    let valid = record(3, 0, (24, 24), 1, (0, 64), 0);
    for (offset, value) in [(0, 1), (1, 0x71), (3, 6), (7, 3)] {
        let mut invalid = valid.clone();
        invalid[offset] = value;
        let mut payload = vec![1];
        payload.extend(invalid);
        assert!(binary_group(&payload).is_err(), "accepted offset {offset}");
    }
    for mode in [1, 2, 3, 5] {
        let mut payload = vec![1];
        payload.extend(record(mode, 0, (127, 0), 128, (127, 0), -1));
        assert!(binary_group(&payload).is_err());
    }
    for payload in [vec![], vec![16], vec![1], vec![0, 0]] {
        assert!(binary_group(&payload).is_err());
    }
    for len in 0..valid.len() {
        let mut payload = vec![1];
        payload.extend(&valid[..len]);
        assert!(binary_group(&payload).is_err());
    }
}

fn program_data() -> Vec<u8> {
    let mut b = 0u32.to_le_bytes().to_vec();
    b.extend(0f64.to_le_bytes());
    b.push(0);
    for n in [1f32, 0., 1.] {
        b.extend(n.to_le_bytes());
    }
    b.extend([1, 120, 10, 100]);
    b.extend(24i16.to_le_bytes());
    b
}

#[test]
fn binary_and_xml_program_ranges_key_defaults_and_cc64_modes_are_explicit() {
    let info = ProgramInfo::parse(&program_data()).unwrap();
    assert_eq!(
        (info.key_range, info.velocity_range, info.default_key_switch),
        ((10, 100), (1, 120), Some(24))
    );
    for (stored, expected) in [
        (0i32, Cc64Mode::SustainAndController),
        (1, Cc64Mode::SustainOnly),
        (2, Cc64Mode::ControllerOnly),
    ] {
        let info = ProgramInfo::parse_chunk(&PresetChunk {
            public_data: program_data(),
            private_data: stored.to_le_bytes().to_vec(),
            version: Some(0xa5),
            ..PresetChunk::default()
        })
        .unwrap();
        assert_eq!(info.cc64_mode, expected);
    }
    for (name, expected) in [
        (
            "cc64_mode_sustain_plus_controller",
            Cc64Mode::SustainAndController,
        ),
        (
            "cc64_mode_sustain_without_controller",
            Cc64Mode::SustainOnly,
        ),
        ("cc64_mode_controller_only", Cc64Mode::ControllerOnly),
    ] {
        let xml = XML.replacen("<Parameters>", &format!("<Parameters><V name=\"cc64Mode\" value=\"{name}\"/><V name=\"defaultKeyKeySwitch\" value=\"24\"/>"), 1);
        let info = plain_model::build(&Element::parse(&xml).unwrap())
            .unwrap()
            .program;
        assert_eq!(
            (info.cc64_mode, info.default_key_switch),
            (expected, Some(24))
        );
    }
    let xml = XML.replacen(
        "<Parameters>",
        "<Parameters><V name=\"cc64Mode\" value=\"mystery\"/>",
        1,
    );
    assert!(plain_model::build(&Element::parse(&xml).unwrap()).is_err());
}
