use nk_format::{
    audio,
    loader::LoadedInstrument,
    stream::{SampleLocation, SampleReader, MAX_READ_FRAMES},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

struct Temp(PathBuf);
impl Temp {
    fn new(bytes: &[u8]) -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let path = std::env::temp_dir().join(format!(
            "cadence-sample-range-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::write(&path, bytes).unwrap();
        Self(path)
    }
    fn reader(&self) -> SampleReader {
        SampleReader::open(SampleLocation::file(&self.0).unwrap()).unwrap()
    }
}
impl Drop for Temp {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

fn wav(tag: u16, bits: u16, channels: u16, frames: usize, extensible: bool) -> Vec<u8> {
    let sample_bytes = usize::from(bits / 8);
    let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
    out.extend_from_slice(&(if extensible { 40u32 } else { 16 }).to_le_bytes());
    out.extend_from_slice(&(if extensible { 0xfffeu16 } else { tag }).to_le_bytes());
    out.extend_from_slice(&channels.to_le_bytes());
    out.extend_from_slice(&48000u32.to_le_bytes());
    out.extend_from_slice(&(48000u32 * u32::from(channels) * sample_bytes as u32).to_le_bytes());
    out.extend_from_slice(&(channels * bits / 8).to_le_bytes());
    out.extend_from_slice(&bits.to_le_bytes());
    if extensible {
        out.extend_from_slice(&22u16.to_le_bytes());
        out.extend_from_slice(&bits.to_le_bytes());
        out.extend_from_slice(&3u32.to_le_bytes());
        out.extend_from_slice(&tag.to_le_bytes());
        out.extend_from_slice(&[0; 14]);
    }
    out.extend_from_slice(b"data");
    out.extend_from_slice(&((frames * usize::from(channels) * sample_bytes) as u32).to_le_bytes());
    for frame in 0..frames {
        for channel in 0..channels {
            let value = ((frame * 17 + usize::from(channel) * 29) % 201) as i32 - 100;
            let bytes = match (tag, bits) {
                (3, 32) => (value as f32 / 100.0).to_le_bytes().to_vec(),
                (3, 64) => (value as f64 / 100.0).to_le_bytes().to_vec(),
                (_, 8) => vec![(128 + value) as u8],
                _ => value.to_le_bytes()[..sample_bytes].to_vec(),
            };
            out.extend_from_slice(&bytes);
        }
    }
    if out.len() % 2 != 0 {
        out.push(0);
    }
    let len = out.len() as u32 - 8;
    out[4..8].copy_from_slice(&len.to_le_bytes());
    out
}
fn ncw(bits: u16, flags: u16, delta: bool) -> Vec<u8> {
    let channels = 2usize;
    let frames = 1337usize;
    let mut blocks = Vec::new();
    let mut offsets = vec![0u32];
    for block in 0..frames.div_ceil(512) {
        for channel in 0..channels {
            blocks.extend_from_slice(&0x3E9A_0C16u32.to_le_bytes());
            blocks.extend_from_slice(&(20 + block as i32 + channel as i32).to_le_bytes());
            blocks.extend_from_slice(&(if delta { 1i16 } else { 0 }).to_le_bytes());
            blocks.extend_from_slice(&flags.to_le_bytes());
            blocks.extend_from_slice(&0u32.to_le_bytes());
            if delta {
                blocks.extend_from_slice(&[0; 64]);
            } else {
                for frame in 0..512 {
                    let value = ((block * 512 + frame + channel * 11) % 32700) as i32;
                    let encoded = if flags & 2 != 0 {
                        (value as f32 / 32768.0).to_bits() as i32
                    } else {
                        value
                    };
                    blocks.extend_from_slice(&encoded.to_le_bytes()[..usize::from(bits / 8)]);
                }
            }
        }
        offsets.push(blocks.len() as u32);
    }
    let mut out = vec![0; 120];
    out[..4].copy_from_slice(&nk_format::ncw::FILE_MAGIC.to_le_bytes());
    out[4..8].copy_from_slice(&0x130u32.to_le_bytes());
    out[8..10].copy_from_slice(&(channels as u16).to_le_bytes());
    out[10..12].copy_from_slice(&bits.to_le_bytes());
    out[12..16].copy_from_slice(&48000u32.to_le_bytes());
    out[16..20].copy_from_slice(&(frames as u32).to_le_bytes());
    out[20..24].copy_from_slice(&120u32.to_le_bytes());
    out[24..28].copy_from_slice(&(120 + offsets.len() as u32 * 4).to_le_bytes());
    out[28..32].copy_from_slice(&(blocks.len() as u32).to_le_bytes());
    for offset in offsets {
        out.extend_from_slice(&offset.to_le_bytes());
    }
    out.extend_from_slice(&blocks);
    out
}
fn compare_ranges(bytes: &[u8]) {
    let full = audio::load(bytes).unwrap();
    let file = Temp::new(bytes);
    let mut reader = file.reader();
    assert_eq!(reader.metadata().channels, full.channels.min(2));
    assert_eq!(reader.metadata().num_frames, full.num_frames);
    for (start, count) in [
        (0, 0),
        (0, 1),
        (255, 512),
        (507, 531),
        (1023, 400),
        (full.num_frames - 3, 100),
        (full.num_frames, 10),
        (usize::MAX, 10),
    ] {
        let chunk = reader.read_frames(start, count).unwrap();
        let frames = count.min(full.num_frames.saturating_sub(start));
        assert_eq!(chunk.num_frames, frames);
        for channel in 0..chunk.channels {
            if frames > 0 {
                assert_eq!(
                    chunk.samples[channel],
                    full.samples[channel][start..start + frames]
                );
            }
        }
    }
    assert!(reader.read_frames(0, MAX_READ_FRAMES + 1).is_err());
}

#[test]
fn wav_ranges_match_all_supported_encodings_and_channel_layouts() {
    for (tag, bits) in [(1, 8), (1, 16), (1, 24), (1, 32), (3, 32), (3, 64)] {
        for channels in [1, 2, 4] {
            for extensible in [false, true] {
                compare_ranges(&wav(tag, bits, channels, 1401, extensible));
            }
        }
    }
}
#[test]
fn ncw_ranges_match_raw_delta_mid_side_and_float_blocks() {
    for (bits, flags, delta) in [
        (16, 0, false),
        (24, 0, false),
        (32, 0, false),
        (16, 1, false),
        (32, 2, false),
        (16, 0, true),
    ] {
        compare_ranges(&ncw(bits, flags, delta));
    }
}
#[test]
fn bounded_sample_locations_respect_container_edges() {
    let sample = wav(1, 16, 2, 1401, false);
    let mut bytes = vec![42; 23];
    bytes.extend_from_slice(&sample);
    bytes.extend_from_slice(&[9; 50]);
    let file = Temp::new(&bytes);
    let mut reader = SampleReader::open(SampleLocation {
        path: file.0.clone(),
        offset: 23,
        len: sample.len() as u64,
    })
    .unwrap();
    assert_eq!(
        reader.read_frames(512, 17).unwrap().samples,
        audio::load(&sample)
            .unwrap()
            .samples
            .iter()
            .map(|s| s[512..529].to_vec())
            .collect::<Vec<_>>()
    );
    assert!(SampleReader::open(SampleLocation {
        path: file.0.clone(),
        offset: u64::MAX,
        len: 1
    })
    .is_err());
    assert!(SampleReader::open(SampleLocation {
        path: file.0.clone(),
        offset: 23,
        len: sample.len() as u64 - 3
    })
    .is_err());
}
#[test]
fn malformed_metadata_and_blocks_are_rejected_before_large_allocations() {
    let mut bytes = ncw(16, 0, false);
    bytes[16..20].copy_from_slice(&u32::MAX.to_le_bytes());
    assert!(SampleReader::open(SampleLocation::file(&Temp::new(&bytes).0).unwrap()).is_err());
    let mut bytes = ncw(16, 0, false);
    bytes[124..128].copy_from_slice(&u32::MAX.to_le_bytes());
    let file = Temp::new(&bytes);
    assert!(file.reader().read_frames(0, 1).is_err());
    let mut bytes = ncw(16, 0, false);
    let data = u32::from_le_bytes(bytes[24..28].try_into().unwrap()) as usize;
    bytes[data + 8..data + 10].copy_from_slice(&33i16.to_le_bytes());
    let file = Temp::new(&bytes);
    assert!(file.reader().read_frames(0, 1).is_err());
    // An intermediate offset may not escape the table's final payload end,
    // even when the containing file has enough trailing bytes.
    let mut bytes = ncw(16, 0, false);
    let payload_end = u32::from_le_bytes(bytes[132..136].try_into().unwrap());
    bytes[128..132].copy_from_slice(&(payload_end + 1).to_le_bytes());
    bytes.extend_from_slice(&[0; 4096]);
    let file = Temp::new(&bytes);
    assert!(file.reader().read_frames(512, 1).is_err());
    let mut bytes = wav(1, 16, 2, 1401, false);
    bytes[22..24].copy_from_slice(&u16::MAX.to_le_bytes());
    assert!(SampleReader::open(SampleLocation::file(&Temp::new(&bytes).0).unwrap()).is_err());
}

fn monolith(main: &[u8], sample: &[u8]) -> Vec<u8> {
    let mut out = vec![0; 904];
    out[..16].copy_from_slice(nk_format::file_container::HEADER);
    out[264..272].fill(0xf0);
    out[272..280].copy_from_slice(&2u64.to_le_bytes());
    out[288..304].copy_from_slice(nk_format::file_container::TOC);
    for (index, name, end) in [
        (0u64, "instrument.nki", main.len() as u64),
        (1, "sample.wav", (main.len() + sample.len()) as u64),
    ] {
        let mut descriptor = vec![0; 640];
        descriptor[..8].copy_from_slice(&index.to_le_bytes());
        for (i, c) in name.encode_utf16().enumerate() {
            descriptor[24 + i * 2..26 + i * 2].copy_from_slice(&c.to_le_bytes());
        }
        descriptor[632..640].copy_from_slice(&end.to_le_bytes());
        out.extend_from_slice(&descriptor);
    }
    let start = out.len();
    out.resize(start + 632, 0);
    out[start..start + 8].fill(0xf1);
    out[start + 24..start + 40].copy_from_slice(nk_format::file_container::TOC);
    out.extend_from_slice(main);
    out.extend_from_slice(sample);
    out
}
#[test]
fn embedded_monolith_load_uses_seekable_sample_slices() {
    let main = include_bytes!("fixtures/Kontakt680Template.nki");
    let sample = wav(1, 16, 2, 1401, false);
    let file = Temp::new(&monolith(main, &sample));
    let mut loaded = LoadedInstrument::load(&file.0).unwrap();
    let location = loaded.sample_location("Samples/SAMPLE.WAV").unwrap();
    assert!(location.offset > main.len() as u64);
    assert_eq!(location.len, sample.len() as u64);
    assert_eq!(loaded.sample("sample.wav").unwrap(), sample);
    let mut reader = SampleReader::open(location).unwrap();
    assert_eq!(
        reader.read_frames(0, 17).unwrap().samples,
        audio::load(&sample)
            .unwrap()
            .samples
            .iter()
            .map(|s| s[..17].to_vec())
            .collect::<Vec<_>>()
    );
}
#[test]
fn sparse_large_monolith_load_does_not_read_sample_payload() {
    let main = include_bytes!("fixtures/Kontakt680Template.nki");
    let mut bytes = monolith(main, &[]);
    let sample_bytes = 512u64 * 1024 * 1024;
    bytes[904 + 640 + 632..904 + 640 + 640]
        .copy_from_slice(&(main.len() as u64 + sample_bytes).to_le_bytes());
    let file = Temp::new(&bytes);
    fs::OpenOptions::new()
        .write(true)
        .open(&file.0)
        .unwrap()
        .set_len(bytes.len() as u64 + sample_bytes)
        .unwrap();
    let mut loaded = LoadedInstrument::load(&file.0).unwrap();
    assert_eq!(
        loaded.sample_location("sample.wav").unwrap().len,
        sample_bytes
    );
}
#[test]
fn real_piano_ncw_and_resource_ranges_match_full_decode_if_present() {
    let path=Path::new("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Instruments/Piano Uno - Concert.nki");
    if !path.exists() {
        return;
    }
    let mut instrument = LoadedInstrument::load(path).unwrap();
    let references = instrument.instrument.sample_files.clone();
    for reference in references
        .iter()
        .filter(|s| s.to_ascii_lowercase().ends_with(".ncw"))
        .take(3)
    {
        let location = instrument.sample_location(reference).unwrap();
        let full = instrument.decoded_sample(reference).unwrap();
        let mut reader = SampleReader::open(location).unwrap();
        assert_eq!(reader.metadata().num_frames, full.num_frames);
        let chunk = reader.read_frames(507, 1031).unwrap();
        for c in 0..chunk.channels {
            assert_eq!(chunk.samples[c], full.samples[c][507..1538]);
        }
    }
}

#[test]
fn monolith_rejects_truncated_tables_and_invalid_payload_offsets() {
    let main = include_bytes!("fixtures/Kontakt680Template.nki");
    let valid = monolith(main, &wav(1, 16, 2, 1401, false));
    for bytes in [
        valid[..1000].to_vec(),
        {
            let mut bytes = valid.clone();
            bytes[272..280].copy_from_slice(&u64::MAX.to_le_bytes());
            bytes
        },
        {
            let mut bytes = valid.clone();
            bytes[904 + 640 + 632..904 + 640 + 640].copy_from_slice(&u64::MAX.to_le_bytes());
            bytes
        },
        {
            let mut bytes = valid;
            bytes[904 + 640 + 632..904 + 640 + 640].copy_from_slice(&0u64.to_le_bytes());
            bytes
        },
    ] {
        let file = Temp::new(&bytes);
        assert!(LoadedInstrument::load(&file.0).is_err());
    }
}
