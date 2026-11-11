use nk_format::{
    external_mod::Transformation,
    model::Group,
    plain_model,
    preset_chunk::{self, PresetChunk},
    Element,
};

fn string(bytes: &mut Vec<u8>, text: &str) {
    bytes.extend((text.len() as u32).to_le_bytes());
    bytes.extend(text.as_bytes());
}

// Independently authored from the saved native serializer, including the
// source's display name and the external private-version suffix.
fn external() -> Vec<u8> {
    let mut private = 1i32.to_le_bytes().to_vec();
    string(&mut private, "pitch");
    private.extend(0.5f32.to_le_bytes());
    private.extend((-1i16).to_le_bytes());
    private.push(20);
    private.extend((-10i16).to_le_bytes());
    string(&mut private, "");
    private.extend([0, 0]); // tableOpen, no transformation
    string(&mut private, "Bend");
    private.extend(1i32.to_le_bytes()); // external source
    private.extend(1i32.to_le_bytes()); // serialized pitch bend
    private.extend((-1i16).to_le_bytes());
    private.extend([0, 1]);
    private.extend(2i32.to_le_bytes());
    private.extend([1, 1]);
    let mut object = vec![1, 5, 1];
    object.extend((private.len() as u32).to_le_bytes());
    object.extend(private);
    object.extend([0; 8]);
    chunk(0x0c, &object)
}

fn chunk(id: u16, body: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_le_bytes().to_vec();
    bytes.extend((body.len() as u32).to_le_bytes());
    bytes.extend(body);
    bytes
}

fn group_data() -> Vec<u8> {
    let mut bytes = vec![0; 4];
    for value in [1f32, 0., 1.] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([1, 0, 0, 0]);
    bytes.extend([0; 4 + 2 + 8 + 2 + 4]);
    bytes
}

fn array(version: u16, count: usize, selected: usize) -> Vec<u8> {
    let mut bytes = vec![0];
    bytes.extend(version.to_le_bytes());
    if version >= 0x13 {
        bytes.extend((count as u32).to_le_bytes());
    }
    for index in 0..count {
        bytes.push(u8::from(index == selected));
        if index == selected {
            bytes.extend(external());
        }
    }
    bytes
}

#[test]
fn external_array_reaches_slot_63_and_older_implicit_slot_31() {
    for (version, count, selected) in [(0x13, 64, 63), (0x12, 32, 31)] {
        let mut wire = chunk(0x3c, &array(version, count, selected));
        wire.extend(chunk(0x777, &[9, 8, 7]));
        let children = preset_chunk::parse_all(&wire).unwrap();
        assert_eq!(children[1].public_data, [9, 8, 7]);
        let group = Group::parse_chunk(&PresetChunk {
            public_data: group_data(),
            children,
            ..Default::default()
        })
        .unwrap();
        assert_eq!(group.external_modulators.len(), 1);
        let modulation = &group.external_modulators[0];
        assert_eq!(modulation.source, 1);
        assert_eq!(modulation.name, "Bend");
        assert_eq!(modulation.initial_value, -1);
        assert!(modulation.simulate_k2);
        assert_eq!(modulation.targets[0].intensity, 0.5);
        assert_eq!(modulation.targets[0].smoothing, -10);
    }
}

#[test]
fn malformed_external_array_never_silently_drops_routes() {
    let valid = array(0x13, 64, 63);
    for length in 0..valid.len() {
        assert!(
            preset_chunk::parse_all(&chunk(0x3c, &valid[..length])).is_err(),
            "accepted prefix {length}"
        );
    }
    for mutation in [(1, 0x14), (3, 65), (7, 2)] {
        let mut invalid = valid.clone();
        invalid[mutation.0] = mutation.1;
        assert!(preset_chunk::parse_all(&chunk(0x3c, &invalid)).is_err());
    }
    let mut trailing = valid;
    trailing.push(0);
    assert!(preset_chunk::parse_all(&chunk(0x3c, &trailing)).is_err());
}

fn xml(body: &str) -> Result<nk_format::model::Instrument, nk_format::Error> {
    let original = include_str!("fixtures/plain-playback.xml");
    plain_model::build(&Element::parse(&original.replacen(
        "</K2_Group>",
        &format!("<ExtModulators>{body}</ExtModulators></K2_Group>"),
        1,
    ))?)
}

#[test]
fn xml_preserves_external_sources_target_depth_flags_and_sparse_table() {
    let instrument = xml(r#"<K2_ExtMod index="8"><Targets><Target index="0">
      <V name="target" value="pitch"/><V name="intensity" value="0.5"/>
      <V name="flags" value="22"/><V name="smoothingCoef" value="-10"/>
      <V name="t0" value="0"/><V name="t127" value="1"/>
      <V name="tableActive" value="yes"/>
      </Target></Targets><V name="source" value="pitchBend"/>
      <V name="initialValue" value="-1"/><V name="bypass" value="yes"/>
      </K2_ExtMod>"#)
    .unwrap();
    let modulation = &instrument.groups[0].external_modulators[0];
    assert_eq!(modulation.source, 1);
    assert!(modulation.bypass);
    assert_eq!(modulation.targets[0].flags, 22);
    assert_eq!(modulation.targets[0].smoothing, -10);
    match &modulation.targets[0].transform {
        Transformation::Table { active, values } => {
            assert!(active);
            assert_eq!(values[0], 0.0);
            assert_eq!(values[127], 1.0);
            assert!((values[64] - 64.0 / 127.0).abs() < 1e-6);
        }
        _ => panic!("table lost"),
    }
}

#[test]
fn malformed_external_xml_routes_are_explicit_errors() {
    let valid = r#"<K2_ExtMod index="0"><Targets><Target index="0"><V name="target" value="volume"/></Target></Targets><V name="source" value="midiCC"/><V name="ccNumber" value="11"/></K2_ExtMod>"#;
    for (old, new) in [
        ("value=\"11\"", "value=\"128\""),
        ("value=\"midiCC\"", "value=\"unknown\""),
        ("<Target index=\"0\">", "<Target index=\"16\">"),
        (
            "</Target>",
            "<V name=\"intensity\" value=\"NaN\"/></Target>",
        ),
        ("</Target>", "<V name=\"x2\" value=\"1\"/></Target>"),
    ] {
        assert!(xml(&valid.replace(old, new)).is_err(), "accepted {new}");
    }
    assert!(xml(&valid.repeat(2)).is_err());
    assert!(xml(&valid.repeat(65)).is_err());
    assert!(xml(&valid.replace(
        "<K2_ExtMod index=\"0\">",
        "<K2_ExtMod version=\"0.9\" index=\"0\">"
    ))
    .is_err());
}

#[test]
fn binary_template_retains_its_real_velocity_and_bend_routes() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/Kontakt680Template.nki");
    let loaded = nk_format::loader::LoadedInstrument::load(path).unwrap();
    let routes = &loaded.instrument.groups[0].external_modulators;
    assert_eq!(routes.len(), 2);
    let velocity = &routes[0];
    assert_eq!(
        (velocity.version, velocity.source, velocity.class_id),
        (0x102, 8, 1)
    );
    assert_eq!(velocity.name, "VEL_VOLUME");
    assert!((velocity.targets[0].intensity - 0.393_701).abs() < 1e-7);
    let bend = &routes[1];
    assert_eq!((bend.version, bend.source, bend.class_id), (0x102, 1, 2));
    assert_eq!(bend.name, "PB_PITCH");
    assert!((bend.targets[0].intensity - 0.166_667).abs() < 1e-7);
    assert_eq!(
        (bend.targets[0].flags, bend.targets[0].smoothing),
        (20, 250)
    );
}
