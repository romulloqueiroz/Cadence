//! Seek-only sample indexes. Payloads remain in their original file.
use crate::{
    file_container, k2_monolith,
    stream::{bad, SampleLocation, SliceFile},
    Error,
};
use std::{collections::HashSet, path::Path};
const MAX_ENTRIES: usize = 100_000;
const MAX_NAME_BYTES: usize = 16 * 1024 * 1024;

pub(crate) fn file_container(path: &Path) -> Result<Vec<(String, SampleLocation)>, Error> {
    let location = SampleLocation::file(path)?;
    let mut source = SliceFile::open(&location)?;
    if source.bytes(0, 16)? != file_container::HEADER
        || source.bytes(264, 8)? != [0xF0; 8]
        || source.bytes(288, 16)? != file_container::TOC
    {
        return Err(bad("invalid NI file container header"));
    }
    let count = read_u64(&mut source, 272)?;
    if count > MAX_ENTRIES as u64 {
        return Err(bad("file container has too many entries"));
    }
    let trailer = 904 + count * 640;
    let data = trailer + 632;
    source.check(0, data)?;
    if source.bytes(trailer, 8)? != [0xF1; 8]
        || source.bytes(trailer + 24, 16)? != file_container::TOC
    {
        return Err(bad("invalid NI file container end marker"));
    }
    let mut entries = Vec::with_capacity(count as usize);
    let mut names = 0;
    for index in 0..count {
        let descriptor = source.bytes(904 + index * 640, 640)?;
        let id = u64::from_le_bytes(descriptor[..8].try_into().unwrap());
        let name = utf16(&descriptor[24..624]);
        names += name.len();
        if names > MAX_NAME_BYTES {
            return Err(bad("file container names exceed limit"));
        }
        let end = u64::from_le_bytes(descriptor[632..640].try_into().unwrap());
        entries.push((id, name, end));
    }
    entries.sort_by_key(|entry| entry.0);
    let mut previous = 0;
    let mut out = Vec::with_capacity(entries.len());
    for (_, name, end) in entries {
        if end < previous {
            return Err(bad("file container offsets not monotonic"));
        }
        source.check(data + previous, end - previous)?;
        out.push((
            name,
            SampleLocation {
                path: path.to_path_buf(),
                offset: data + previous,
                len: end - previous,
            },
        ));
        previous = end;
    }
    Ok(out)
}

/// Preserve the legacy resource rule: directory-tree sample names pair with
/// sequential sample blocks, rather than interpreting directory pointers as
/// payload positions (they differ in some factory resources).
pub(crate) fn nkr(path: &Path) -> Result<Vec<(String, SampleLocation)>, Error> {
    let location = SampleLocation::file(path)?;
    let mut source = SliceFile::open(&location)?;
    let mut names = Vec::new();
    let mut seen = HashSet::new();
    let mut name_bytes = 0;
    collect_names(
        &mut source,
        0,
        "",
        0,
        &mut seen,
        &mut names,
        &mut name_bytes,
    )?;
    let mut ranges = Vec::new();
    let mut position = 0;
    while position + 4 <= source.len {
        let magic = read_u32(&mut source, position)?;
        match magic {
            k2_monolith::HEADER_MAGIC => {
                position = directory(&mut source, position)?.1;
            }
            k2_monolith::SAMPLE_MAGIC => {
                let length = u64::from(read_u32(&mut source, position + 19)?);
                let data = position + 31;
                source.check(data, length)?;
                ranges.push(SampleLocation {
                    path: path.to_path_buf(),
                    offset: data,
                    len: length,
                });
                if ranges.len() > MAX_ENTRIES {
                    return Err(bad("too many NKR sample blocks"));
                }
                position = data + length;
            }
            k2_monolith::WALLPAPER_MAGIC => {
                let version = read_u32(&mut source, position + 4)?;
                let p = position + 4 + if version >= 0x111 { 10 } else { 14 };
                let length = read_u64(&mut source, p)?;
                source.check(p + 8, length)?;
                position = p + 8 + length;
            }
            k2_monolith::SAMPLE_RAW_MAGIC => {
                position = find_next(&mut source, position + 24)?.unwrap_or(source.len);
            }
            _ => break,
        }
    }
    Ok(names.into_iter().zip(ranges).collect())
}

/// `.nkx` monoliths use the same directory and sample-block layout as `.nkr`.
pub(crate) fn nkx(path: &Path) -> Result<Vec<(String, SampleLocation)>, Error> {
    nkr(path)
}

fn collect_names(
    source: &mut SliceFile,
    offset: u64,
    prefix: &str,
    depth: usize,
    seen: &mut HashSet<u64>,
    names: &mut Vec<String>,
    total: &mut usize,
) -> Result<(), Error> {
    if depth > 16 || !seen.insert(offset) || seen.len() > MAX_ENTRIES {
        return Err(bad("invalid NKR directory recursion"));
    }
    let (entries, _) = directory(source, offset)?;
    for (kind, pointer, name) in entries {
        let name = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        *total += name.len();
        if *total > MAX_NAME_BYTES || names.len() >= MAX_ENTRIES {
            return Err(bad("NKR directory exceeds limits"));
        }
        if kind == 1 {
            collect_names(
                source,
                u64::from(pointer),
                &name,
                depth + 1,
                seen,
                names,
                total,
            )?;
        } else if kind == 2 {
            names.push(name);
        }
    }
    Ok(())
}

type Directory = Vec<(i16, u32, String)>;
fn directory(source: &mut SliceFile, offset: u64) -> Result<(Directory, u64), Error> {
    let header = source.bytes(offset, 22)?;
    if u32::from_le_bytes(header[..4].try_into().unwrap()) != k2_monolith::HEADER_MAGIC {
        return Err(bad("invalid NKR directory header"));
    }
    let count = u32::from_le_bytes(header[14..18].try_into().unwrap()) as usize;
    if count > MAX_ENTRIES {
        return Err(bad("NKR directory has too many entries"));
    }
    source.check(offset + 22, count as u64 * 8)?;
    let mut position = offset + 22;
    let mut out = Vec::with_capacity(count.min(4096));
    let mut names = 0;
    for _ in 0..count {
        let entry = source.bytes(position, 8)?;
        let length = u16::from_le_bytes(entry[..2].try_into().unwrap()) as u64;
        if length < 8 {
            return Err(bad("short NKR directory entry"));
        }
        let pointer = u32::from_le_bytes(entry[2..6].try_into().unwrap());
        let kind = i16::from_le_bytes(entry[6..8].try_into().unwrap());
        let name = utf16(&source.bytes(position + 8, length - 8)?);
        names += name.len();
        if names > MAX_NAME_BYTES {
            return Err(bad("NKR directory names exceed limit"));
        }
        out.push((kind, pointer, name));
        position += length;
    }
    Ok((out, position))
}

fn find_next(source: &mut SliceFile, mut position: u64) -> Result<Option<u64>, Error> {
    while position + 4 <= source.len {
        let count = (source.len - position).min(64 * 1024);
        let bytes = source.bytes(position, count)?;
        for (index, magic) in bytes.windows(4).enumerate() {
            let magic = u32::from_le_bytes(magic.try_into().unwrap());
            if matches!(
                magic,
                k2_monolith::HEADER_MAGIC
                    | k2_monolith::SAMPLE_MAGIC
                    | k2_monolith::SAMPLE_RAW_MAGIC
                    | k2_monolith::NKI_MAGIC
                    | k2_monolith::WALLPAPER_MAGIC
            ) {
                return Ok(Some(position + index as u64));
            }
        }
        position += count - 3;
    }
    Ok(None)
}
fn read_u32(source: &mut SliceFile, position: u64) -> Result<u32, Error> {
    Ok(u32::from_le_bytes(
        source.bytes(position, 4)?.try_into().unwrap(),
    ))
}
fn read_u64(source: &mut SliceFile, position: u64) -> Result<u64, Error> {
    Ok(u64::from_le_bytes(
        source.bytes(position, 8)?.try_into().unwrap(),
    ))
}
fn utf16(bytes: &[u8]) -> String {
    let words: Vec<_> = bytes
        .chunks_exact(2)
        .map(|pair| u16::from_le_bytes(pair.try_into().unwrap()))
        .collect();
    String::from_utf16_lossy(&words)
        .trim_end_matches(['\0', ' '])
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
    };
    struct Temp(std::path::PathBuf);
    impl Temp {
        fn new(bytes: &[u8]) -> Self {
            static NEXT: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "cadence-location-index-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::write(&path, bytes).unwrap();
            Self(path)
        }
    }
    impl Drop for Temp {
        fn drop(&mut self) {
            let _ = fs::remove_file(&self.0);
        }
    }
    fn directory_bytes(entries: &[(i16, u32, &str)]) -> Vec<u8> {
        let mut out = vec![0; 22];
        out[..4].copy_from_slice(&k2_monolith::HEADER_MAGIC.to_le_bytes());
        out[14..18].copy_from_slice(&(entries.len() as u32).to_le_bytes());
        for (kind, pointer, name) in entries {
            let words: Vec<_> = name.encode_utf16().collect();
            out.extend_from_slice(&(8u16 + words.len() as u16 * 2).to_le_bytes());
            out.extend_from_slice(&pointer.to_le_bytes());
            out.extend_from_slice(&kind.to_le_bytes());
            for word in words {
                out.extend_from_slice(&word.to_le_bytes());
            }
        }
        out
    }
    fn sample_block(data: &[u8]) -> Vec<u8> {
        let mut out = vec![0; 31];
        out[..4].copy_from_slice(&k2_monolith::SAMPLE_MAGIC.to_le_bytes());
        out[19..23].copy_from_slice(&(data.len() as u32).to_le_bytes());
        out.extend_from_slice(data);
        out
    }
    #[test]
    fn nkr_indexes_preserve_tree_names_paired_with_sequential_payloads() {
        let mut bytes = directory_bytes(&[(1, 0, "nested"), (2, 0, "second.wav")]);
        let child = bytes.len() as u32;
        bytes[24..28].copy_from_slice(&child.to_le_bytes());
        bytes.extend_from_slice(&directory_bytes(&[(2, 0, "first.wav")]));
        // Sample directory pointers are deliberately zero: legacy named pairing
        // is sequential, not a direct dereference of these fields.
        bytes.extend_from_slice(&sample_block(b"first payload"));
        let mut wallpaper = vec![0; 22];
        wallpaper[..4].copy_from_slice(&k2_monolith::WALLPAPER_MAGIC.to_le_bytes());
        wallpaper[4..8].copy_from_slice(&0x111u32.to_le_bytes());
        wallpaper[14..22].copy_from_slice(&3u64.to_le_bytes());
        wallpaper.extend_from_slice(b"png");
        bytes.extend_from_slice(&wallpaper);
        let mut raw = vec![0; 24];
        raw[..4].copy_from_slice(&k2_monolith::SAMPLE_RAW_MAGIC.to_le_bytes());
        raw.extend_from_slice(&vec![99; 65535]);
        bytes.extend_from_slice(&raw);
        bytes.extend_from_slice(&sample_block(b"second payload"));
        let legacy = k2_monolith::scan_named_samples(&bytes).unwrap();
        let file = Temp::new(&bytes);
        let indexed = nkr(&file.0).unwrap();
        assert_eq!(indexed.len(), 2);
        for ((name, location), (old_name, data)) in indexed.into_iter().zip(legacy) {
            assert_eq!(name, old_name);
            assert_eq!(location.read_all().unwrap(), data);
        }
    }
    #[test]
    fn nkr_rejects_cycles_truncated_samples_and_absurd_directories() {
        for bytes in [
            directory_bytes(&[(1, 0, "cycle")]),
            {
                let mut bytes = directory_bytes(&[(2, 0, "sample.wav")]);
                let mut block = sample_block(b"short");
                block[19..23].copy_from_slice(&u32::MAX.to_le_bytes());
                bytes.extend_from_slice(&block);
                bytes
            },
            {
                let mut bytes = directory_bytes(&[]);
                bytes[14..18].copy_from_slice(&u32::MAX.to_le_bytes());
                bytes
            },
        ] {
            let file = Temp::new(&bytes);
            assert!(nkr(&file.0).is_err());
        }
    }
    #[test]
    fn real_nkr_indexes_equal_legacy_payloads_if_present() {
        let path=Path::new("/Library/Application Support/Native Instruments/Kontakt 8/Content/Presets/Piano Uno/Samples/Piano Uno 1.0.nkr");
        if !path.exists() {
            return;
        }
        let bytes = fs::read(path).unwrap();
        let legacy = k2_monolith::scan_named_samples(&bytes).unwrap();
        let indexed = nkr(path).unwrap();
        assert!(!indexed.is_empty());
        assert_eq!(legacy.len(), indexed.len());
        for ((name, location), (old_name, data)) in indexed.into_iter().zip(legacy) {
            assert_eq!(name, old_name);
            assert_eq!(location.read_all().unwrap(), data);
            let mut reader = crate::stream::SampleReader::open(location).unwrap();
            let chunk = reader.read_frames(11, 25).unwrap();
            let decoded = crate::audio::load(&data).unwrap();
            for c in 0..chunk.channels {
                assert_eq!(chunk.samples[c], decoded.samples[c][11..36]);
            }
        }
    }
}
