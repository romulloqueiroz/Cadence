//! Kontakt 2 monolith/resource container (`.nkr`): a directory of referenced
//! items (samples, NKIs, sub-directories, wallpapers) plus the referenced data.
//!
//! Layout recovered from ConvertWithMoss `Directory.java` /
//! `DirectoryEntry.java` / `Kontakt2Monolith.java`.

use crate::Error;

pub const HEADER_MAGIC: u32 = 0x5E70_AC54;
pub const SAMPLE_MAGIC: u32 = 0x16CC_F80A;
pub const SAMPLE_RAW_MAGIC: u32 = 0x0040_179F;
pub const NKI_MAGIC: u32 = 0x4916_E63C;
pub const WALLPAPER_MAGIC: u32 = 0x2AE9_05FA;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryType {
    End,
    Directory,
    Sample,
    Nki,
    Wallpaper,
    Unknown(i16),
}

impl EntryType {
    fn from_raw(raw: i16) -> Self {
        match raw {
            0 => EntryType::End,
            1 => EntryType::Directory,
            2 => EntryType::Sample,
            3 => EntryType::Nki,
            4 => EntryType::Wallpaper,
            other => EntryType::Unknown(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            EntryType::End => "end",
            EntryType::Directory => "directory",
            EntryType::Sample => "sample",
            EntryType::Nki => "nki",
            EntryType::Wallpaper => "wallpaper",
            EntryType::Unknown(_) => "unknown",
        }
    }
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub length: u16,
    pub pointer: u32,
    pub kind: EntryType,
    pub name: String,
}

pub fn parse(bytes: &[u8]) -> Result<Vec<Entry>, Error> {
    parse_at(bytes, 0)
}

/// Flatten the directory tree, prefixing nested entry names with their folders.
pub fn parse_tree(bytes: &[u8]) -> Result<Vec<Entry>, Error> {
    let mut out = Vec::new();
    collect(bytes, 0, "", &mut out, 0)?;
    Ok(out)
}

fn collect(
    bytes: &[u8],
    offset: usize,
    prefix: &str,
    out: &mut Vec<Entry>,
    depth: usize,
) -> Result<(), Error> {
    if depth > 16 {
        return Err(Error::Malformed(
            "K2 monolith directory nesting too deep".into(),
        ));
    }
    for entry in parse_at(bytes, offset)? {
        let name = if prefix.is_empty() {
            entry.name.clone()
        } else {
            format!("{prefix}/{}", entry.name)
        };
        if entry.kind == EntryType::Directory {
            collect(bytes, entry.pointer as usize, &name, out, depth + 1)?;
        } else {
            out.push(Entry { name, ..entry });
        }
    }
    Ok(())
}

fn parse_at(bytes: &[u8], offset: usize) -> Result<Vec<Entry>, Error> {
    parse_at_with_end(bytes, offset).map(|(entries, _)| entries)
}

fn parse_at_with_end(bytes: &[u8], offset: usize) -> Result<(Vec<Entry>, usize), Error> {
    let mut r = Reader::new(bytes);
    r.skip(offset)?;
    let magic = r.u32()?;
    if magic != HEADER_MAGIC {
        return Err(Error::Malformed(format!(
            "not a K2 monolith (magic 0x{magic:08X})"
        )));
    }

    r.skip(10)?;
    let num_items = r.u32()? as usize;
    r.skip(4)?;
    if num_items > 1_000_000 {
        return Err(Error::Malformed(format!(
            "implausible directory item count {num_items}"
        )));
    }

    let mut entries = Vec::with_capacity(num_items.min(4096));
    for _ in 0..num_items {
        let length = r.u16()?;
        let pointer = r.u32()?;
        let kind = EntryType::from_raw(r.i16()?);
        if length < 8 {
            return Err(Error::Malformed(format!(
                "directory entry length {length} < 8"
            )));
        }
        let content = r.take((length - 8) as usize)?;
        entries.push(Entry {
            length,
            pointer,
            kind,
            name: decode_utf16(content),
        });
    }

    let end = r.pos;
    Ok((entries, end))
}

/// Extract the raw bytes referenced by a `Sample` entry, if the block layout is
/// one of the recognised variants.
pub fn extract_sample(bytes: &[u8], entry: &Entry) -> Option<Vec<u8>> {
    let start = entry.pointer as usize;
    let data = bytes.get(start..)?;

    let magic = u32::from_le_bytes(data.get(0..4)?.try_into().ok()?);
    if magic != SAMPLE_MAGIC {
        return None;
    }

    // u32 magic, u16 version, 13 bytes, u32 data length, 8 bytes
    let mut pos = 4 + 2 + 13;
    let length = u32::from_le_bytes(data.get(pos..pos + 4)?.try_into().ok()?) as usize;
    pos += 4 + 8;
    data.get(pos..pos + length).map(|s| s.to_vec())
}

fn decode_utf16(bytes: &[u8]) -> String {
    let words: Vec<u16> = bytes
        .chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect();
    String::from_utf16_lossy(&words)
        .trim_end_matches(['\0', ' '])
        .to_string()
}

/// Scan the monolith sequentially for sample blocks, in file order.
pub fn scan_samples(bytes: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    let mut out = Vec::new();
    let mut pos = 0usize;
    while pos + 4 <= bytes.len() {
        let magic = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        match magic {
            HEADER_MAGIC => {
                let (_, end) = parse_at_with_end(bytes, pos)?;
                pos = end;
            }
            WALLPAPER_MAGIC => {
                let version = bytes
                    .get(pos + 4..pos + 8)
                    .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
                    .unwrap_or(0);
                // Version 0x111+ uses a 10-byte header, older files 14 bytes.
                let header = if version >= 0x111 { 10 } else { 14 };
                let mut p = pos + 4 + header;
                let length = u64::from_le_bytes(
                    bytes
                        .get(p..p + 8)
                        .ok_or(Error::Truncated {
                            needed: p + 8,
                            len: bytes.len(),
                        })?
                        .try_into()
                        .unwrap(),
                );
                p += 8;
                pos = p
                    .checked_add(length as usize)
                    .ok_or_else(|| Error::Malformed("monolith wallpaper overflow".into()))?;
            }
            NKI_MAGIC => break,
            SAMPLE_MAGIC => {
                let mut p = pos + 4 + 2 + 13;
                let length = u32::from_le_bytes(
                    bytes
                        .get(p..p + 4)
                        .ok_or(Error::Truncated {
                            needed: p + 4,
                            len: bytes.len(),
                        })?
                        .try_into()
                        .unwrap(),
                ) as usize;
                p += 4 + 8;
                let data = bytes
                    .get(p..p + length)
                    .ok_or(Error::Truncated {
                        needed: p + length,
                        len: bytes.len(),
                    })?
                    .to_vec();
                out.push(data);
                pos = p + length;
            }
            SAMPLE_RAW_MAGIC => {
                pos = find_next_block(bytes, pos + 4 + 2 + 18).unwrap_or(bytes.len());
            }
            _ => break,
        }
    }
    Ok(out)
}

/// Pair directory `Sample` entries with the sequentially scanned sample data.
pub fn scan_named_samples(bytes: &[u8]) -> Result<Vec<(String, Vec<u8>)>, Error> {
    let entries = parse_tree(bytes)?;
    let names: Vec<String> = entries
        .into_iter()
        .filter(|e| e.kind == EntryType::Sample)
        .map(|e| e.name)
        .collect();
    let samples = scan_samples(bytes)?;
    Ok(names.into_iter().zip(samples).collect())
}

fn find_next_block(bytes: &[u8], start: usize) -> Option<usize> {
    let mut pos = start;
    while pos + 4 <= bytes.len() {
        let magic = u32::from_le_bytes(bytes[pos..pos + 4].try_into().unwrap());
        if matches!(
            magic,
            HEADER_MAGIC | SAMPLE_MAGIC | SAMPLE_RAW_MAGIC | NKI_MAGIC | WALLPAPER_MAGIC
        ) {
            return Some(pos);
        }
        pos += 1;
    }
    None
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
        let end = self.pos.checked_add(n).ok_or(Error::Truncated {
            needed: usize::MAX,
            len: self.data.len(),
        })?;
        if end > self.data.len() {
            return Err(Error::Truncated {
                needed: end,
                len: self.data.len(),
            });
        }
        let slice = &self.data[self.pos..end];
        self.pos = end;
        Ok(slice)
    }

    fn skip(&mut self, n: usize) -> Result<(), Error> {
        self.take(n).map(|_| ())
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn i16(&mut self) -> Result<i16, Error> {
        self.u16().map(|v| v as i16)
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }
}
