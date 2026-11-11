use nk_format::{
    loader::LoadedInstrument,
    plain_model::{build, decode_sample_path},
    xml::Element,
    PLAIN_MAGIC,
};
use std::{
    io::Write,
    sync::atomic::{AtomicUsize, Ordering},
};
const XML: &str = include_str!("fixtures/plain-playback.xml");
static SERIAL: AtomicUsize = AtomicUsize::new(0);

fn model(xml: &str) -> Result<nk_format::model::Instrument, nk_format::Error> {
    build(&Element::parse(xml)?)
}
fn container(xml: &str) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(xml.as_bytes()).unwrap();
    let mut bytes = vec![0; 0xaa];
    bytes[..4].copy_from_slice(&PLAIN_MAGIC.to_le_bytes());
    bytes[8..10].copy_from_slice(&0x100u16.to_le_bytes());
    bytes.extend(encoder.finish().unwrap());
    bytes
}

#[test]
fn maps_sparse_group_ids_and_xml_units_without_display_name_heuristics() {
    let instrument = model(XML).unwrap();
    assert_eq!(instrument.program.name, "XML & Playback");
    assert_eq!(instrument.program.midi_transpose, 12);
    assert_eq!(instrument.groups.len(), 2);
    assert_eq!(instrument.zones.len(), 2);
    let zone = &instrument.zones[0];
    assert_eq!(zone.group_index, 0);
    assert_eq!(instrument.zones[1].group_index, 1);
    assert_eq!(instrument.sample_files, ["Samples/tone Ω.wav"]);
    assert_eq!((zone.sample_start, zone.end_frame(100)), (3, 10));
    assert_eq!(
        (
            zone.low_key,
            zone.high_key,
            zone.low_velocity,
            zone.high_velocity
        ),
        (60, 72, 10, 120)
    );
    assert!((instrument.zone_gain(zone) - 0.1).abs() < 1e-6);
    assert!(instrument.zone_pan(zone).abs() < 1e-6);
    assert!((instrument.zone_tune(zone) - 1.0).abs() < 1e-6);
    assert!(!instrument.groups[0].release_trigger);
    assert!(!instrument.groups[0].name_trigger_heuristics);
    assert!(instrument.groups[1].release_trigger);
}

#[test]
fn single_frame_endpoints_and_missing_end_are_unambiguous() {
    let same = model(&XML.replace(
        "name=\"sampleEnd\" value=\"9\"",
        "name=\"sampleEnd\" value=\"3\"",
    ))
    .unwrap();
    assert_eq!(same.zones[0].end_frame(100), 4);
    let zero = model(
        &XML.replace(
            "name=\"sampleStart\" value=\"3\"",
            "name=\"sampleStart\" value=\"0\"",
        )
        .replace(
            "name=\"sampleEnd\" value=\"9\"",
            "name=\"sampleEnd\" value=\"0\"",
        ),
    )
    .unwrap();
    assert_eq!(zero.zones[0].end_frame(100), 1);
    assert_eq!(zero.zones[1].end_frame(100), 100);
}

#[test]
fn program_ranges_preserve_zone_extents_and_banks_never_silently_drop_slots() {
    let xml = XML.replace("<Parameters>\n      <V name=\"volume\"", "<Parameters><V name=\"lowKey\" value=\"65\"/><V name=\"highKey\" value=\"67\"/>\n      <V name=\"volume\"");
    let instrument = model(&xml).unwrap();
    assert_eq!(instrument.program.key_range, (65, 67));
    assert_eq!(
        (instrument.zones[0].low_key, instrument.zones[0].high_key),
        (60, 72)
    );
    let bank = format!(
        "<K2_Bank>{}</K2_Bank>",
        XML.trim_start_matches("<?xml version=\"1.0\"?>")
    );
    assert!(model(&bank).is_ok());
    let second = XML.replace(
        "</K2_Program></Programs>",
        "</K2_Program><K2_Program name=\"second\"/></Programs>",
    );
    assert!(model(&second)
        .unwrap_err()
        .to_string()
        .contains("multi-program"));
}

#[test]
fn sample_path_grammar_decodes_folders_unicode_and_parent_components() {
    for (encoded, expected) in [
        (r"Samples\plain.wav", "Samples/plain.wav"),
        ("@d003subbf000000tone.wav", "sub/../tone.wav"),
        ("@v002C:d007Samplesf000000tone.wav", "C:/Samples/tone.wav"),
        (
            "@d007SamplesF00000000000tone%0020%03A9.wav",
            "Samples/tone Ω.wav",
        ),
        ("@f000000%D83D%DE00.wav", "😀.wav"),
        ("literal%0041.wav", "literal%0041.wav"),
    ] {
        assert_eq!(decode_sample_path(encoded).unwrap(), expected, "{encoded}");
    }
    for invalid in [
        "",
        "@",
        "@d",
        "@dxyzfoo",
        "@d009short",
        "@f000",
        "@qfoo",
        "@m003libf000000a.wav",
        "@f000000%D800.wav",
        "@f000000%0000.wav",
        "folder/",
    ] {
        assert!(decode_sample_path(invalid).is_err(), "accepted {invalid:?}");
    }
}

#[test]
fn malformed_mapping_and_unsupported_conditions_fail_explicitly() {
    for (from, to) in [
        ("value=\"0.5\"", "value=\"NaN\""),
        ("name=\"transpose\" value=\"12\"", "name=\"transpose\" value=\"200\""),
        ("name=\"highKey\" value=\"72\"", "name=\"highKey\" value=\"40\""),
        ("name=\"sampleEnd\" value=\"9\"", "name=\"sampleEnd\" value=\"2\""),
        ("groupIdx=\"17\"", "groupIdx=\"99\""),
        ("index=\"3\" name=\"Actual release\"", "index=\"17\" name=\"Actual release\""),
        ("<GroupStart/>", "<GroupStart><StartCriteria><V name=\"mode\" value=\"cycle\"/></StartCriteria></GroupStart>"),
        ("<V name=\"volume\" value=\"0.8\"/>", "<V name=\"volume\" value=\"0.8\"/><V name=\"volume\" value=\"1\"/>"),
        ("@d007Samplesf000000tone%0020%03A9.wav", "@m003libf000000a.wav"),
    ] { assert!(model(&XML.replace(from,to)).is_err(), "accepted replacement {to}"); }
}

#[test]
fn xml_loader_uses_normalized_sample_paths_without_reading_payloads() {
    let directory = std::env::temp_dir().join(format!(
        "cadence-plain-{}-{}",
        std::process::id(),
        SERIAL.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(directory.join("Samples")).unwrap();
    std::fs::write(
        directory.join("Samples/tone Ω.wav"),
        b"not decoded until preparation",
    )
    .unwrap();
    let path = directory.join("instrument.nki");
    std::fs::write(&path, container(XML)).unwrap();
    let mut loaded = LoadedInstrument::load(&path).unwrap();
    let location = loaded.sample_location("Samples\\tone Ω.wav").unwrap();
    assert_eq!(location.len, 29);
    assert_eq!(loaded.instrument.program.name, "XML & Playback");
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn xml_nesting_is_bounded_before_recursive_model_walk_or_drop() {
    let nested = format!(
        "{}{}",
        "<K2_Container>".repeat(129),
        "</K2_Container>".repeat(129)
    );
    assert!(Element::parse(&nested)
        .unwrap_err()
        .to_string()
        .contains("budget"));
}

#[test]
fn nki_render_cli_accepts_plain_xml_and_rejects_unsupported_group_conditions() {
    let directory = std::env::temp_dir().join(format!("cadence-plain-cli-{}", std::process::id()));
    std::fs::create_dir_all(directory.join("Samples")).unwrap();
    let mut wav = Vec::new();
    let frames = 128u32;
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + frames * 2).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&48_000u32.to_le_bytes());
    wav.extend_from_slice(&96_000u32.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(frames * 2).to_le_bytes());
    for _ in 0..frames {
        wav.extend_from_slice(&10_000i16.to_le_bytes());
    }
    std::fs::write(directory.join("Samples/tone Ω.wav"), wav).unwrap();
    let path = directory.join("plain.nki");
    let output = directory.join("render.wav");
    std::fs::write(&path, container(XML)).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_nki-render"))
        .arg(&path)
        .arg(&output)
        .args(["--note", "48", "--seconds", "0.1"])
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let audio = nk_format::audio::load(&std::fs::read(&output).unwrap()).unwrap();
    assert_eq!(audio.num_frames, 4800);
    assert!(audio.samples[0].iter().any(|v| v.abs() > 1e-4));
    std::fs::remove_file(&output).unwrap();
    let conditional=XML.replace("<GroupStart/>","<GroupStart><StartCriteria><V name=\"mode\" value=\"cycle\"/></StartCriteria></GroupStart>");
    std::fs::write(&path, container(&conditional)).unwrap();
    let result = std::process::Command::new(env!("CARGO_BIN_EXE_nki-render"))
        .arg(&path)
        .arg(&output)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert!(!output.exists());
    assert!(String::from_utf8_lossy(&result.stderr).contains("GroupStart"));
    std::fs::remove_dir_all(directory).unwrap();
}
