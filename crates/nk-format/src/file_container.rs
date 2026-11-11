//! Native Instruments file container (`/\ NI FC MTD  /\`), used by Kontakt 5+
//! monolith `.nkx`/`.nki` files. It stores a table of contents followed by the
//! concatenated file data.
//!
//! Layout recovered from ConvertWithMoss `Kontakt5MonolithFormat.java`.

use crate::Error;

pub const HEADER: &[u8; 16] = b"/\\ NI FC MTD  /\\";
pub const TOC: &[u8; 16] = b"/\\ NI FC TOC  /\\";
const HEADER_END: [u8; 8] = [0xF0; 8];
const FILES_END: [u8; 8] = [0xF1; 8];

#[derive(Debug, Clone)]
pub struct MonolithFile {
    pub index: u64,
    pub name: String,
    pub data: Vec<u8>,
}

#[derive(Debug, Clone, Default)]
pub struct FileContainer {
    pub files: Vec<MonolithFile>,
}

pub fn is_container(bytes: &[u8]) -> bool {
    bytes.starts_with(HEADER)
}

pub fn parse(bytes: &[u8]) -> Result<FileContainer, Error> {
    let mut r = Reader::new(bytes);

    let header = r.take(16)?;
    if header != HEADER {
        return Err(Error::Malformed("not an NI file container".into()));
    }
    r.skip(248)?;
    if r.take(8)? != HEADER_END {
        return Err(Error::Malformed("file container header marker".into()));
    }

    let file_count = r.u64()?;
    let _total_size = r.u64()?;
    if file_count > 1_000_000 {
        return Err(Error::Malformed(format!(
            "implausible file count {file_count}"
        )));
    }

    if r.take(16)? != TOC {
        return Err(Error::Malformed("file container TOC marker".into()));
    }
    r.skip(600)?;

    let mut descriptors = Vec::with_capacity(file_count as usize);
    for _ in 0..file_count {
        let index = r.u64()?;
        r.skip(16)?;
        let name_bytes = r.take(600)?;
        let name = decode_utf16(name_bytes);
        let _padding = r.u64()?;
        let end_offset = r.u64()?;
        descriptors.push((index, name, end_offset));
    }

    if r.take(8)? != FILES_END {
        return Err(Error::Malformed("file container files marker".into()));
    }
    r.skip(16)?;
    if r.take(16)? != TOC {
        return Err(Error::Malformed("file container end TOC marker".into()));
    }
    r.skip(592)?;

    descriptors.sort_by_key(|(index, _, _)| *index);

    let mut files = Vec::with_capacity(descriptors.len());
    let mut previous_end = 0u64;
    for (index, name, end_offset) in descriptors {
        if end_offset < previous_end {
            return Err(Error::Malformed(
                "file container offsets not monotonic".into(),
            ));
        }
        let length = usize::try_from(end_offset - previous_end)
            .map_err(|_| Error::Malformed("file container file too large".into()))?;
        let data = r.take(length)?.to_vec();
        files.push(MonolithFile { index, name, data });
        previous_end = end_offset;
    }

    Ok(FileContainer { files })
}

/// Basename without directory, lowercased, accepting `\` and `/` separators.
pub fn basename_lower(path: &str) -> String {
    let trimmed = path.trim_end_matches(['\0', ' ']);
    let base = trimmed.rsplit(['/', '\\']).next().unwrap_or(trimmed).trim();
    base.to_ascii_lowercase()
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

    fn u64(&mut self) -> Result<u64, Error> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }
}
