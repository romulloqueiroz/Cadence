use nk_format::{probe, Container, Document, Error, Kind, PLAIN_MAGIC};
use std::io::Write;

const SAMPLE_XML: &str = r#"<?xml version="1.0"?>
<K2_Container index="0" name="Test Kit" type="single_program" version="0.50">
  <Programs>
    <K2_Program index="0" name="Kit" version="0.80">
      <Parameters>
        <V name="volume" value="0.75"/>
        <V name="transpose" value="-2"/>
        <V name="groupSolo" value="no"/>
      </Parameters>
      <Groups>
        <K2_Group index="0" name="G1">
          <Parameters><V name="pan" value="-0.5"/></Parameters>
        </K2_Group>
      </Groups>
      <Zones>
        <K2_Zone index="0">
          <Parameters>
            <V name="lowKey" value="36"/>
            <V name="highKey" value="48"/>
            <V name="lowVelocity" value="1"/>
            <V name="highVelocity" value="127"/>
            <V name="fileName" value="C:\Samples\kick.wav"/>
          </Parameters>
        </K2_Zone>
      </Zones>
    </K2_Program>
  </Programs>
</K2_Container>"#;

fn zlib(data: &[u8]) -> Vec<u8> {
    let mut encoder = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    encoder.write_all(data).unwrap();
    encoder.finish().unwrap()
}

fn container_at(xml: &str, payload_offset: usize, format_version: u16) -> Vec<u8> {
    let compressed = zlib(xml.as_bytes());
    let mut buf = vec![0u8; payload_offset];
    buf[0..4].copy_from_slice(&PLAIN_MAGIC.to_le_bytes());
    buf[4..8].copy_from_slice(&(compressed.len() as u32).to_le_bytes());
    buf[8..10].copy_from_slice(&format_version.to_le_bytes());
    buf[0x0E..0x10].copy_from_slice(&1u16.to_le_bytes());
    buf[0x10..0x14].copy_from_slice(&0x0309_09FFu32.to_le_bytes());
    buf[0x14..0x18].copy_from_slice(&0x4B6F_6E33u32.to_le_bytes());
    buf[0x18..0x1C].copy_from_slice(&1_752_082_820i32.to_le_bytes());
    buf[0x1C..0x20].copy_from_slice(&0x3Au32.to_le_bytes());
    buf.extend_from_slice(&compressed);
    buf
}

fn container(xml: &str) -> Vec<u8> {
    container_at(xml, 0xAA, 0x0100)
}

#[test]
fn parses_header_payload_and_program_tree() {
    let c = Container::parse(&container(SAMPLE_XML)).expect("parse");

    assert_eq!(c.header.magic, PLAIN_MAGIC);
    assert_eq!(c.header.format_version, 0x0100);
    assert_eq!(c.header.flags, 1);
    assert_eq!(c.header.app_version, 0x0309_09FF);
    assert_eq!(c.header.app_version_octets(), [3, 9, 9, 255]);
    assert_eq!(c.header.app_signature, 0x4B6F_6E33);
    assert_eq!(c.header.field_1c, 0x3A);
    assert_eq!(c.header.payload_offset, 0xAA);

    let doc = Document::new(&c.root);
    assert_eq!(doc.name(), "Test Kit");
    assert_eq!(doc.kind(), Some("single_program"));
    assert_eq!(doc.version(), Some("0.50"));

    let programs = doc.programs();
    assert_eq!(programs.len(), 1);
    let program = &programs[0];
    assert_eq!(program.name(), "Kit");
    assert_eq!(program.index(), Some(0));

    let params = program.params();
    assert_eq!(params.f32("volume"), Some(0.75));
    assert_eq!(params.i32("transpose"), Some(-2));
    assert_eq!(params.bool("groupSolo"), Some(false));

    let groups = program.groups();
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].name(), "G1");
    assert_eq!(groups[0].params().f32("pan"), Some(-0.5));

    let zones = program.zones();
    assert_eq!(zones.len(), 1);
    assert_eq!(zones[0].index(), Some(0));
    assert_eq!(zones[0].key_range(), Some((36, 48)));
    assert_eq!(zones[0].velocity_range(), Some((1, 127)));
    assert_eq!(zones[0].sample_refs(), vec![r"C:\Samples\kick.wav"]);
}

#[test]
fn unescapes_attribute_entities() {
    let xml = r#"<K2_Container name="A &amp; B" type="single_program">
      <Programs><K2_Program index="0" name="&lt;instrument&gt;"/></Programs>
    </K2_Container>"#;
    let c = Container::parse(&container(xml)).unwrap();
    let doc = Document::new(&c.root);
    assert_eq!(doc.name(), "A & B");
    assert_eq!(doc.programs()[0].name(), "<instrument>");
}

#[test]
fn finds_payload_when_offset_is_not_default() {
    let c = Container::parse(&container_at(SAMPLE_XML, 0xC0, 0x0100)).expect("parse");
    assert_eq!(c.header.payload_offset, 0xC0);
    assert_eq!(Document::new(&c.root).name(), "Test Kit");
}

#[test]
fn reports_opaque_factory_magic() {
    let mut bytes = vec![0u8; 0x40];
    bytes[0..4].copy_from_slice(&0x0000_EA9Eu32.to_le_bytes());
    assert_eq!(probe(&bytes), Kind::Opaque { magic: 0xEA9E });
    assert!(matches!(
        Container::parse(&bytes),
        Err(Error::UnsupportedMagic { magic: 0xEA9E })
    ));
}

#[test]
fn rejects_truncated_input() {
    assert_eq!(probe(&[0x12, 0x90]), Kind::TooShort);
    assert!(matches!(
        Container::parse(&[0x12, 0x90, 0xA8, 0x7F]),
        Err(Error::Truncated { .. })
    ));
}

#[test]
fn rejects_payload_that_is_not_k2_xml() {
    assert!(matches!(
        Container::parse(&container("<html></html>")),
        Err(Error::NoPayload)
    ));
}

#[test]
fn rejects_wrong_root_element() {
    assert!(matches!(
        Container::parse(&container("<NotK2/>")),
        Err(Error::NoPayload) | Err(Error::UnexpectedRoot { .. })
    ));
}

#[test]
fn parses_real_kontakt_default_if_present() {
    let candidates = [
        "/Users/rom/Library/Application Support/Native Instruments/Kontakt 8/default/kontakt_def.nki",
        "/Users/rom/Library/Application Support/Native Instruments/Kontakt 7/default/kontakt_def.nki",
    ];
    let Some(path) = candidates.iter().find(|p| std::path::Path::new(p).exists()) else {
        return;
    };
    let c = nk_format::read_path(path).expect("real file parses");
    let doc = Document::new(&c.root);
    assert_eq!(doc.kind(), Some("single_program"));
    assert!(!doc.programs().is_empty());
}

#[test]
fn parses_ni_container_fixture() {
    let path = "tests/fixtures/Kontakt680Template.nki";
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(_) => return,
    };
    assert_eq!(probe(&bytes), Kind::Container);

    let item = nk_format::ni_container::parse(&bytes).expect("container parses");
    let app = item
        .find_chunk(nk_format::ni_container::CHUNK_AUTHORING_APPLICATION)
        .and_then(|chunk| match &chunk.data {
            nk_format::ni_container::ChunkData::AuthoringApplication { application, .. } => {
                Some(*application)
            }
            _ => None,
        });
    assert_eq!(app.map(|a| a.name()), Some("Kontakt"));

    let mut subtrees = Vec::new();
    item.find_all_chunks(nk_format::ni_container::CHUNK_SUB_TREE_ITEM, &mut subtrees);
    assert!(subtrees.iter().any(|chunk| matches!(
        chunk.data,
        nk_format::ni_container::ChunkData::SubTree(nk_format::ni_container::SubTree::Decoded(_))
    )));

    let preset = item
        .find_chunk(nk_format::ni_container::CHUNK_PRESET_CHUNK_ITEM)
        .and_then(|chunk| match &chunk.data {
            nk_format::ni_container::ChunkData::Preset { preset_data, .. } => {
                Some(preset_data.clone())
            }
            _ => None,
        })
        .expect("preset chunk present");

    let chunks = nk_format::preset_chunk::parse_all(&preset).expect("preset tree parses");
    assert!(!chunks.is_empty());
}

#[test]
fn parses_real_factory_instrument_if_present() {
    let path = "/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki";
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    if probe(&bytes) != Kind::Container {
        return;
    }

    let item = nk_format::ni_container::parse(&bytes).expect("container parses");
    let preset = item
        .find_chunk(nk_format::ni_container::CHUNK_PRESET_CHUNK_ITEM)
        .and_then(|chunk| match &chunk.data {
            nk_format::ni_container::ChunkData::Preset { preset_data, .. } => {
                Some(preset_data.clone())
            }
            _ => None,
        })
        .expect("preset chunk present");

    let chunks = nk_format::preset_chunk::parse_all(&preset).expect("preset tree parses");
    let summary = nk_format::preset_chunk::summary(&chunks);
    assert!(summary.programs >= 1);
    assert!(summary.zones > 0);
    assert!(!summary.sample_files.is_empty());

    let instrument = nk_format::model::build(&chunks).expect("model");
    assert_eq!(instrument.groups.len(), 6);
    assert_eq!(instrument.groups[5].name, "Grand Piano");
    assert!(instrument.groups[5].envelope.is_some());
    let filter = instrument
        .fx_chains
        .iter()
        .flat_map(|chain| chain.slots.iter().flatten())
        .find_map(|slot| {
            slot.child
                .as_ref()
                .filter(|child| child.ser_type == 0x18)
                .map(|child| (slot.fix_type, child.version, child.raw.len()))
        })
        .expect("factory preset has a filter FX child");
    assert_eq!(filter.0, 7);
    assert_eq!(filter.1, 0x92);
    assert!(filter.2 > 0);
    let sustain = instrument
        .groups
        .iter()
        .find(|group| group.name == "Sustain")
        .expect("Sustain group");
    let group_filters = sustain
        .fx_chain
        .as_ref()
        .expect("group FX chain")
        .simple_filters();
    assert!(group_filters.iter().any(|filter| {
        filter.shape == nk_format::fx::FilterShape::Highpass && filter.poles == 2
    }));
}

#[test]
fn decodes_zone_and_ncw_sample_if_present() {
    use nk_format::ncw;
    use std::ffi::OsStr;
    use std::path::Path;

    let nki = "/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki";
    let Ok(bytes) = std::fs::read(nki) else {
        return;
    };
    if probe(&bytes) != Kind::Container {
        return;
    }

    let item = nk_format::ni_container::parse(&bytes).expect("container parses");
    let preset = item
        .find_chunk(nk_format::ni_container::CHUNK_PRESET_CHUNK_ITEM)
        .and_then(|chunk| match &chunk.data {
            nk_format::ni_container::ChunkData::Preset { preset_data, .. } => {
                Some(preset_data.clone())
            }
            _ => None,
        })
        .expect("preset chunk present");

    let chunks = nk_format::preset_chunk::parse_all(&preset).expect("preset tree parses");
    let summary = nk_format::preset_chunk::summary(&chunks);
    let zones = nk_format::zone::decode_zones(&chunks);
    assert!(!zones.is_empty());

    let zone = zones
        .iter()
        .find(|z| z.filename_id.is_some() && z.num_frames > 0 && z.sample_rate > 0)
        .expect("a zone with a sample");

    let sample_ref = &summary.sample_files[zone.filename_id.unwrap() as usize];
    let index = sample_ref.find("Content/").expect("content-relative path");
    let content_root = Path::new(nki)
        .ancestors()
        .find(|a| a.file_name() == Some(OsStr::new("Content")))
        .expect("content root");
    let sample_path = content_root.join(&sample_ref[index + "Content/".len()..]);

    let sample =
        ncw::parse(&std::fs::read(&sample_path).expect("sample readable")).expect("ncw decodes");
    assert_eq!(sample.num_frames, zone.num_frames as usize);
    assert!(sample.samples[0].iter().any(|v| v.abs() > 0.0001));
}

#[test]
fn parses_wav_and_audio_sniffer() {
    let frames = 8usize;
    let rate = 48_000u32;
    let channels = 2u16;
    let mut data = Vec::new();
    for i in 0..frames {
        for _ in 0..channels {
            let value = (i as i16 - 4) * 1000;
            data.extend_from_slice(&value.to_le_bytes());
        }
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&channels.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * channels as u32 * 2).to_le_bytes());
    wav.extend_from_slice(&(channels * 2).to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);

    let sample = nk_format::audio::load(&wav).expect("wav decodes");
    assert_eq!(sample.channels, 2);
    assert_eq!(sample.sample_rate, rate);
    assert_eq!(sample.num_frames, frames);
    assert!((sample.samples[0][0] - (-4.0 * 1000.0 / 32768.0)).abs() < 1e-6);
    assert!((sample.samples[0][7] - (3.0 * 1000.0 / 32768.0)).abs() < 1e-6);
}

fn build_file_container(name: &str, data: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(nk_format::file_container::HEADER);
    out.extend(std::iter::repeat(0u8).take(248));
    out.extend_from_slice(&[0xF0; 8]);
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(nk_format::file_container::TOC);
    out.extend(std::iter::repeat(0u8).take(600));
    out.extend_from_slice(&1u64.to_le_bytes());
    out.extend(std::iter::repeat(0u8).take(16));
    let mut name_bytes = vec![0u8; 600];
    for (i, unit) in name.encode_utf16().take(299).enumerate() {
        name_bytes[i * 2..i * 2 + 2].copy_from_slice(&unit.to_le_bytes());
    }
    out.extend_from_slice(&name_bytes);
    out.extend_from_slice(&0u64.to_le_bytes());
    out.extend_from_slice(&(data.len() as u64).to_le_bytes());
    out.extend_from_slice(&[0xF1; 8]);
    out.extend(std::iter::repeat(0u8).take(16));
    out.extend_from_slice(nk_format::file_container::TOC);
    out.extend(std::iter::repeat(0u8).take(592));
    out.extend_from_slice(data);
    out
}

#[test]
fn parses_file_container() {
    let payload = b"embedded bytes";
    let bytes = build_file_container("Samples/test.nki", payload);
    assert_eq!(probe(&bytes), Kind::FileContainer);
    let container = nk_format::file_container::parse(&bytes).expect("file container parses");
    assert_eq!(container.files.len(), 1);
    assert_eq!(container.files[0].name, "Samples/test.nki");
    assert_eq!(container.files[0].data, payload);
    assert_eq!(
        nk_format::file_container::basename_lower("Samples\\test.NKI"),
        "test.nki"
    );
}

#[test]
fn scans_k2_monolith_samples_if_present() {
    let path = "/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Samples/Piano Uno 1.0.nkr";
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };

    let entries = nk_format::k2_monolith::parse_tree(&bytes).expect("directory tree");
    assert!(entries
        .iter()
        .any(|e| e.kind == nk_format::k2_monolith::EntryType::Sample));

    let samples = nk_format::k2_monolith::scan_named_samples(&bytes).expect("samples");
    assert!(!samples.is_empty());
    for (name, data) in &samples {
        assert!(name.to_ascii_lowercase().ends_with(".wav"), "{name}");
        assert!(data.starts_with(b"RIFF"), "{name}");
    }
}

#[test]
fn expands_nkm_multis_if_present() {
    let path = "/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Multis/Chords/A Myth.nkm";
    let Ok(bytes) = std::fs::read(path) else {
        return;
    };
    if probe(&bytes) != Kind::Container {
        return;
    }

    let item = nk_format::ni_container::parse(&bytes).expect("container");
    let preset = item
        .find_chunk(nk_format::ni_container::CHUNK_PRESET_CHUNK_ITEM)
        .and_then(|chunk| match &chunk.data {
            nk_format::ni_container::ChunkData::Preset { preset_data, .. } => {
                Some(preset_data.clone())
            }
            _ => None,
        })
        .expect("preset");

    let chunks = nk_format::preset_chunk::parse_all(&preset).expect("preset tree");
    assert_eq!(nk_format::preset_chunk::summary(&chunks).programs, 0);

    let expanded = nk_format::preset_chunk::expanded(&chunks);
    let summary = nk_format::preset_chunk::summary(&expanded);
    assert!(summary.programs >= 1);
    assert!(summary.zones > 0);
}
