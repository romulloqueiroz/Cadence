use nk_format::{model::Group, plain_model, xml::Element, Error};

// Public field order from BGroup::doWritePubPars (0x1007b0954): the
// amplifier boundary follows voiceGroup and precedes muted/soloed/quality.
fn binary_group(split: i32) -> Vec<u8> {
    let name: Vec<_> = "Amp Ω".encode_utf16().collect();
    let mut bytes = (name.len() as u32).to_le_bytes().to_vec();
    for unit in name {
        bytes.extend(unit.to_le_bytes());
    }
    for value in [0.4f32, 0.25, 1.5] {
        bytes.extend(value.to_le_bytes());
    }
    bytes.extend([1, 0, 0, 0]); // key tracking, reverse, release flags
    bytes.extend(7i32.to_le_bytes()); // release counter
    bytes.extend(2i16.to_le_bytes()); // MIDI channel
    bytes.extend(3i32.to_le_bytes()); // voice group
    bytes.extend(split.to_le_bytes());
    bytes.extend([1, 0]); // muted, soloed
    bytes.extend(2i32.to_le_bytes()); // ultra interpolation
    bytes
}

#[test]
fn binary_amplifier_boundaries_preserve_following_public_fields() {
    for split in [0, 6, 8] {
        let group = Group::parse(&binary_group(split)).unwrap();
        assert_eq!(group.fx_amp_split_point, split);
        assert_eq!(group.name, "Amp Ω");
        assert_eq!(group.voice_group_index, 3);
        assert_eq!(group.midi_channel, 2);
        assert!(group.muted && !group.soloed);
        assert_eq!(group.interp_quality, 2);
    }
}

#[test]
fn binary_amplifier_boundaries_reject_indices_outside_eight_insert_slots() {
    for split in [-1, 9, i32::MIN, i32::MAX] {
        let error = Group::parse(&binary_group(split)).unwrap_err();
        assert!(matches!(error, Error::Malformed(_)));
        assert!(error.to_string().contains("fxIdxAmpSplitPoint"));
    }
}

fn xml_group(parameter: &str) -> Result<Group, Error> {
    let xml = format!(
        r#"<K2_Container><Programs><K2_Program><Groups>
        <K2_Group index="0"><Parameters>{parameter}</Parameters></K2_Group>
        </Groups></K2_Program></Programs></K2_Container>"#
    );
    let instrument = plain_model::build(&Element::parse(&xml)?)?;
    Ok(instrument.groups.into_iter().next().unwrap())
}

#[test]
fn xml_amplifier_boundary_uses_native_default_and_preserves_all_valid_placements() {
    // BGroup::ResetParameters (0x1003fd448) initializes +0x8f0 to 6.
    assert_eq!(xml_group("").unwrap().fx_amp_split_point, 6);
    for split in [0, 6, 8] {
        let parameter = format!(r#"<V name="fxIdxAmpSplitPoint" value="{split}"/>"#);
        assert_eq!(xml_group(&parameter).unwrap().fx_amp_split_point, split);
    }
}

#[test]
fn xml_amplifier_boundary_rejects_invalid_or_unrepresentable_placements() {
    for split in ["-1", "9", "2147483648", "0.5", "NaN", "", "invalid"] {
        let parameter = format!(r#"<V name="fxIdxAmpSplitPoint" value="{split}"/>"#);
        assert!(xml_group(&parameter).is_err(), "accepted {split:?}");
    }
}
