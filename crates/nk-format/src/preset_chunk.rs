//! The Kontakt preset chunk tree found inside `PRESET_CHUNK_ITEM` payloads.
//!
//! Wire format (little-endian), recovered from ConvertWithMoss:
//!
//! ```text
//! chunk { u16 id, u32 size, [size]object }
//! object = 0x00 + publicData                     // unstructured
//!        | 0x01 + u16 version
//!               + u32 len + privateData
//!               + u32 len + publicData
//!               + u32 len + child*                  // structured
//! ```
//!
//! `GROUP_LIST` and `ZONE_LIST` are arrays; `PARAMETER_ARRAY_8/16/32` are
//! fixed-size arrays of optional children. Programs (0x28), groups (0x04) and
//! zones (children of `ZONE_LIST`) live here, and `FILENAME_LIST` carries the
//! sample paths.

use crate::Error;

pub const ID_PAR_MOD_BASE: u32 = 0x00;
pub const ID_BANK: u32 = 0x03;
pub const ID_GROUP: u32 = 0x04;
pub const ID_PAR_SCRIPT: u32 = 0x06;
pub const ID_PAR_EXTERNAL_MOD: u32 = 0x0C;
pub const ID_PAR_INTERNAL_MOD: u32 = 0x0D;
pub const ID_PAR_FX_SEND_LEVELS: u32 = 0x17;
pub const ID_PROGRAM: u32 = 0x28;
pub const ID_PROGRAM_CONTAINER: u32 = 0x29;
pub const ID_PROGRAM_LIST: u32 = 0x36;
pub const ID_ZONE: u32 = 0x2C;
pub const ID_VOICE_GROUPS: u32 = 0x32;
pub const ID_SLOT_LIST: u32 = 0x37;
pub const ID_GROUP_LIST: u32 = 0x33;
pub const ID_ZONE_LIST: u32 = 0x34;
pub const ID_PARAMETER_ARRAY_8: u32 = 0x3A;
pub const ID_PARAMETER_ARRAY_16: u32 = 0x3B;
pub const ID_PARAMETER_ARRAY_32: u32 = 0x3C;
pub const ID_LOOP_ARRAY: u32 = 0x39;
pub const ID_FILENAME_LIST: u32 = 0x3D;
pub const ID_INSERT_BUS: u32 = 0x45;
pub const ID_SAVE_SETTINGS: u32 = 0x47;
pub const ID_FILENAME_LIST_EX: u32 = 0x4B;
pub const ID_QUICK_BROWSE_DATA: u32 = 0x4E;

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PresetChunk {
    /// Chunk id, or the array reference for zones.
    pub id: u32,
    pub version: Option<u16>,
    pub public_data: Vec<u8>,
    pub private_data: Vec<u8>,
    pub children: Vec<PresetChunk>,
}

impl PresetChunk {
    pub fn name(&self) -> &'static str {
        chunk_name(self.id)
    }
}

pub fn chunk_name(id: u32) -> &'static str {
    match id {
        ID_PAR_MOD_BASE => "PAR_MOD_BASE",
        ID_BANK => "BANK",
        ID_GROUP => "GROUP",
        ID_PAR_SCRIPT => "PAR_SCRIPT",
        ID_PAR_EXTERNAL_MOD => "PAR_EXTERNAL_MOD",
        ID_PAR_INTERNAL_MOD => "PAR_INTERNAL_MOD",
        ID_PAR_FX_SEND_LEVELS => "PAR_FX_SEND_LEVELS",
        ID_PROGRAM => "PROGRAM",
        ID_PROGRAM_CONTAINER => "PROGRAM_CONTAINER",
        ID_PROGRAM_LIST => "PROGRAM_LIST",
        ID_SLOT_LIST => "SLOT_LIST",
        ID_ZONE => "ZONE",
        ID_VOICE_GROUPS => "VOICE_GROUPS",
        ID_GROUP_LIST => "GROUP_LIST",
        ID_ZONE_LIST => "ZONE_LIST",
        ID_PARAMETER_ARRAY_8 => "PARAMETER_ARRAY_8",
        ID_PARAMETER_ARRAY_16 => "PARAMETER_ARRAY_16",
        ID_PARAMETER_ARRAY_32 => "PARAMETER_ARRAY_32",
        ID_LOOP_ARRAY => "LOOP_ARRAY",
        ID_FILENAME_LIST => "FILENAME_LIST",
        ID_INSERT_BUS => "INSERT_BUS",
        ID_SAVE_SETTINGS => "SAVE_SETTINGS",
        ID_FILENAME_LIST_EX => "FILENAME_LIST_EX",
        ID_QUICK_BROWSE_DATA => "QUICK_BROWSE_DATA",
        _ => "unknown",
    }
}

/// Expand bank slot lists into their embedded programs.
pub fn expanded(chunks: &[PresetChunk]) -> Vec<PresetChunk> {
    let mut out = chunks.to_vec();
    expand_slots(&mut out);
    out
}

fn expand_slots(chunks: &mut [PresetChunk]) {
    for chunk in chunks.iter_mut() {
        expand_slots(&mut chunk.children);
        if chunk.id != ID_BANK {
            continue;
        }
        let slot_list = chunk
            .children
            .iter()
            .find(|c| c.id == ID_SLOT_LIST)
            .cloned();
        if let Some(slot_list) = slot_list {
            if let Ok(programs) = parse_slot_programs(&slot_list) {
                chunk.children.extend(programs);
            }
        }
    }
}

fn parse_slot_programs(slot_list: &PresetChunk) -> Result<Vec<PresetChunk>, Error> {
    let mut reader = Reader::new(&slot_list.public_data);
    let flags = reader.take(8)?;
    let mut programs = Vec::new();

    for slot in 0..64 {
        if flags[slot / 8] & (1 << (slot % 8)) == 0 {
            continue;
        }
        let container = parse_chunk(&mut reader, 0)?;
        if container.id != ID_PROGRAM_CONTAINER {
            continue;
        }
        for child in &container.children {
            if child.id != ID_PROGRAM_LIST {
                continue;
            }
            let mut list = Reader::new(&child.public_data);
            let count = list.u32()? as usize;
            if count > 10_000 {
                return Err(Error::Malformed(format!(
                    "implausible program count {count}"
                )));
            }
            for _ in 0..count {
                let mut program = PresetChunk {
                    id: ID_PROGRAM,
                    ..PresetChunk::default()
                };
                program.read_structure(&mut list, child.public_data.len(), 0)?;
                programs.push(program);
            }
        }
    }

    Ok(programs)
}

pub fn parse_all(data: &[u8]) -> Result<Vec<PresetChunk>, Error> {
    let mut reader = Reader::new(data);
    let mut out = Vec::new();
    while reader.remaining() > 0 {
        out.push(parse_chunk(&mut reader, 0)?);
    }
    Ok(out)
}

#[derive(Debug, Default)]
pub struct Summary {
    pub programs: usize,
    pub groups: usize,
    pub zones: usize,
    pub sample_files: Vec<String>,
}

pub fn summary(chunks: &[PresetChunk]) -> Summary {
    let mut out = Summary::default();
    for chunk in chunks {
        collect(chunk, &mut out);
    }
    out
}

fn collect(chunk: &PresetChunk, out: &mut Summary) {
    match chunk.id {
        ID_PROGRAM => out.programs += 1,
        ID_GROUP_LIST => out.groups += chunk.children.len(),
        ID_ZONE => out.zones += 1,
        ID_ZONE_LIST => out.zones += chunk.children.len(),
        _ => {}
    }
    if chunk.id == ID_FILENAME_LIST || chunk.id == ID_FILENAME_LIST_EX {
        if let Ok(files) = parse_file_list(chunk) {
            out.sample_files.extend(files);
        }
    }
    for child in &chunk.children {
        collect(child, out);
    }
}

impl PresetChunk {
    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_into(&mut out, 0);
        out
    }

    fn dump_into(&self, out: &mut String, level: usize) {
        let pad = "  ".repeat(level);
        out.push_str(&format!(
            "{pad}0x{:02X} {} (public {} B, private {} B, {} children)\n",
            self.id,
            self.name(),
            self.public_data.len(),
            self.private_data.len(),
            self.children.len()
        ));
        for child in &self.children {
            child.dump_into(out, level + 1);
        }
    }
}

/// Extract the sample file paths from a `FILENAME_LIST` / `FILENAME_LIST_EX`.
pub fn parse_file_list(chunk: &PresetChunk) -> Result<Vec<String>, Error> {
    match chunk.id {
        ID_FILENAME_LIST | ID_FILENAME_LIST_EX => {}
        _ => return Err(Error::Malformed("not a file list chunk".into())),
    }
    let mut r = Reader::new(&chunk.public_data);

    if chunk.id == ID_FILENAME_LIST_EX {
        let version = r.u16()?;
        if version == 3 {
            return read_files_v3(&mut r);
        }
        if !(2..=3).contains(&version) {
            return Err(Error::Malformed(format!(
                "unsupported FILENAME_LIST_EX version {version}"
            )));
        }
    }

    let _special = read_files(&mut r)?;
    read_files(&mut r)
}

fn read_files(r: &mut Reader<'_>) -> Result<Vec<String>, Error> {
    let mut files = Vec::new();
    if r.remaining() == 0 {
        return Ok(files);
    }
    let count = r.i32()?;
    if !(0..=1_000_000).contains(&count) {
        return Err(Error::Malformed(format!("implausible file count {count}")));
    }
    for _ in 0..count {
        files.push(read_file(r)?);
    }
    Ok(files)
}

fn read_files_v3(r: &mut Reader<'_>) -> Result<Vec<String>, Error> {
    let mut files = Vec::new();
    if r.remaining() == 0 {
        return Ok(files);
    }
    let count = r.i32()?;
    if !(1..=1_000_000).contains(&count) {
        return Err(Error::Malformed(format!("implausible file count {count}")));
    }
    r.skip(8)?;
    for _ in 0..count - 1 {
        files.push(read_file(r)?);
        if r.remaining() > 0 {
            r.skip(4 + 4 + 20)?;
        }
    }
    r.skip(24).ok();
    Ok(files)
}

fn read_file(r: &mut Reader<'_>) -> Result<String, Error> {
    let segments = r.u32()?;
    if segments > 10_000 {
        return Err(Error::Malformed(format!(
            "implausible path segment count {segments}"
        )));
    }
    let mut path = String::new();
    for _ in 0..segments {
        read_segment(r, &mut path)?;
    }
    Ok(path)
}

fn read_segment(r: &mut Reader<'_>, path: &mut String) -> Result<(), Error> {
    match r.u8()? {
        0 => {
            let drive = String::from_utf8_lossy(r.take(2)?).trim().to_string();
            if drive.is_empty() {
                path.push('/');
            } else {
                path.push_str(&drive);
                path.push_str(":/");
            }
        }
        1 => {
            let drive = r.utf16()?;
            if drive.is_empty() {
                path.push('/');
            } else {
                path.push_str(&drive);
                path.push_str(":/");
            }
        }
        2 => {
            path.push_str(&r.utf16()?);
            path.push('/');
        }
        3 => path.push_str("../"),
        4 | 8 | 9 => path.push_str(&r.utf16()?),
        6 => {}
        other => {
            return Err(Error::Malformed(format!(
                "unsupported file list segment {other}"
            )))
        }
    }
    Ok(())
}

fn parse_chunk(reader: &mut Reader<'_>, depth: usize) -> Result<PresetChunk, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Malformed("preset chunk nesting too deep".into()));
    }
    let id = reader.u16()? as u32;
    let size = reader.u32()? as usize;
    let object = reader
        .take(size)
        .map_err(|e| Error::Malformed(format!("preset chunk 0x{id:02X} size {size}: {e}")))?;
    let mut r = Reader::new(object);

    let mut chunk = PresetChunk {
        id,
        ..PresetChunk::default()
    };

    match id {
        ID_GROUP_LIST => {
            chunk.read_array(&mut r, size, false, depth)?;
            for child in &mut chunk.children {
                child.id = ID_GROUP;
            }
        }
        ID_ZONE_LIST => chunk.read_array(&mut r, size, true, depth)?,
        ID_BANK
        | ID_PROGRAM_CONTAINER
        | ID_PROGRAM
        | ID_PAR_SCRIPT
        | ID_PAR_FX_SEND_LEVELS
        | ID_VOICE_GROUPS
        | ID_PARAMETER_ARRAY_8
        | ID_INSERT_BUS
        | ID_SAVE_SETTINGS
        | ID_QUICK_BROWSE_DATA => chunk.read_structure(&mut r, size, depth)?,
        ID_PARAMETER_ARRAY_16 => chunk.read_fixed_array(&mut r, 16, depth)?,
        ID_PARAMETER_ARRAY_32 => chunk.read_fixed_array(&mut r, 32, depth)?,
        _ => {
            chunk.public_data = r.take(r.remaining())?.to_vec();
        }
    }

    Ok(chunk)
}

impl PresetChunk {
    fn read_structure(
        &mut self,
        r: &mut Reader<'_>,
        object_size: usize,
        depth: usize,
    ) -> Result<(), Error> {
        let structured = r.u8()? != 0;
        if !structured {
            if object_size > 0 {
                let n = (object_size - 1).min(r.remaining());
                self.public_data = r.take(n)?.to_vec();
            }
            return Ok(());
        }

        self.version = Some(r.u16()?);

        let size = r.u32()? as usize;
        self.private_data = r.take(size)?.to_vec();

        let size = r.u32()? as usize;
        self.public_data = r.take(size)?.to_vec();

        let size = r.u32()? as usize;
        let children_data = r.take(size)?;
        let mut c = Reader::new(children_data);
        while c.remaining() > 0 {
            let child = parse_chunk(&mut c, depth + 1)
                .map_err(|e| Error::Malformed(format!("children of 0x{:02X}: {e}", self.id)))?;
            self.children.push(child);
        }
        Ok(())
    }

    fn read_array(
        &mut self,
        r: &mut Reader<'_>,
        object_size: usize,
        has_reference: bool,
        depth: usize,
    ) -> Result<(), Error> {
        let len = r.u32()? as usize;
        if len > 1_000_000 {
            return Err(Error::Malformed(format!("implausible array length {len}")));
        }
        self.version = Some(0);
        for _ in 0..len {
            let reference = if has_reference { r.u32()? } else { ID_GROUP };
            let mut child = PresetChunk {
                id: reference,
                ..PresetChunk::default()
            };
            child.read_structure(r, object_size, depth + 1)?;
            self.children.push(child);
        }
        Ok(())
    }

    fn read_fixed_array(
        &mut self,
        r: &mut Reader<'_>,
        size: usize,
        _depth: usize,
    ) -> Result<(), Error> {
        if r.u8()? != 0 {
            return Err(Error::Malformed(
                "fixed parameter array starts with non-zero byte".into(),
            ));
        }
        self.version = Some(r.u16()?);

        // Type 0x3c is BParameterArraySer<BParExternalMod,64>. Its older
        // versions have 32 implicit slots; v0x13 writes an explicit count.
        // A count of 64 must not be mistaken for an old presence bitmap.
        if self.id == ID_PARAMETER_ARRAY_32 {
            let version = self.version.unwrap();
            if version > 0x13 {
                return Err(Error::Malformed(
                    "unsupported external modulation array version".into(),
                ));
            }
            let count = if version >= 0x13 {
                r.u32()? as usize
            } else {
                32
            };
            if count > 64 {
                return Err(Error::Malformed(
                    "external modulation array exceeds 64".into(),
                ));
            }
            for _ in 0..count {
                match r.u8()? {
                    0 => continue,
                    1 => {}
                    _ => {
                        return Err(Error::Malformed(
                            "invalid external modulation presence flag".into(),
                        ))
                    }
                }
                let id = r.u16()? as u32;
                if id != ID_PAR_EXTERNAL_MOD {
                    return Err(Error::Malformed(
                        "external modulation array has unexpected element type".into(),
                    ));
                }
                let size = r.u32()? as usize;
                self.children.push(PresetChunk {
                    id,
                    public_data: r.take(size)?.to_vec(),
                    ..Default::default()
                });
            }
            if r.remaining() != 0 {
                return Err(Error::Malformed(
                    "trailing external modulation array data".into(),
                ));
            }
            return Ok(());
        }

        // Observed layout: u32 entry count, then that many optional entries.
        // Older files omit the count and always use the fixed size.
        let count = if r.remaining() >= 4 {
            let save = r.pos;
            let candidate = r.u32()? as usize;
            if candidate <= size && candidate <= 64 {
                candidate
            } else {
                r.pos = save;
                size
            }
        } else {
            0
        };

        for _ in 0..count {
            if r.remaining() == 0 {
                break;
            }
            if r.u8()? > 0 {
                if r.remaining() < 6 {
                    break;
                }
                let id = r.u16()? as u32;
                let data_size = r.u32()? as usize;
                if data_size > r.remaining() {
                    break;
                }
                let public_data = r.take(data_size)?.to_vec();
                self.children.push(PresetChunk {
                    id,
                    public_data,
                    ..PresetChunk::default()
                });
            }
        }
        Ok(())
    }
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
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

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, Error> {
        let b = self.take(2)?;
        Ok(u16::from_le_bytes([b[0], b[1]]))
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn i32(&mut self) -> Result<i32, Error> {
        self.u32().map(|v| v as i32)
    }

    fn utf16(&mut self) -> Result<String, Error> {
        let units = self.u32()? as usize;
        let bytes = self.take(
            units
                .checked_mul(2)
                .ok_or_else(|| Error::Malformed("UTF-16 length overflow".into()))?,
        )?;
        let words: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        String::from_utf16(&words).map_err(|_| Error::Malformed("invalid UTF-16 string".into()))
    }
}
