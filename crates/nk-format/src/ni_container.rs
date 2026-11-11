//! Reader for Native Instruments file containers ("NI FC"), used by Kontakt 5+
//! and other NI products.
//!
//! Structure (all little-endian), recovered from the independent LGPL
//! implementation ConvertWithMoss and validated against real `.nki` files:
//!
//! ```text
//! block64 (u64 length incl. the 8 length bytes) {
//!     u32 header_version = 1
//!     ascii4 "hsin"
//!     u32 1
//!     u32 0
//!     uuid[16]
//!     DataChunk chain       // nested block64 records
//!     u32 version
//!     u32 num_children
//!     child* { u32 index, ascii4 domain, u32 chunk_type, Item }
//! }
//! ```
//!
//! A `DataChunk` is itself a block64 containing `domain`, `chunk_type`,
//! `version`, then the *next* chunk (nested), then its own payload. Payloads of
//! `SUB_TREE_ITEM` may be FastLZ-compressed; when decompression is not
//! attempted the subtree is reported as compressed/undecoded.

use crate::Error;

pub const DOMAIN_ITEM: &[u8; 4] = b"hsin";

pub const CHUNK_TERMINATOR: u32 = 1;
pub const CHUNK_AUTHORING_APPLICATION: u32 = 101;
pub const CHUNK_AUTHORIZATION: u32 = 106;
pub const CHUNK_SOUNDINFO_ITEM: u32 = 108;
pub const CHUNK_PRESET_CHUNK_ITEM: u32 = 109;
pub const CHUNK_SUB_TREE_ITEM: u32 = 115;
pub const CHUNK_ENCRYPTION_ITEM: u32 = 116;
pub const CHUNK_CONTAINER_ROOT: u32 = 118;

const MAX_DEPTH: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub uuid: [u8; 16],
    pub version: u32,
    pub data: DataChunk,
    pub children: Vec<Child>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Child {
    pub index: u32,
    pub domain: String,
    pub chunk_type: u32,
    pub item: Item,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DataChunk {
    pub domain: String,
    pub chunk_type: u32,
    pub version: u32,
    pub next: Option<Box<DataChunk>>,
    pub data: ChunkData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChunkData {
    Terminator,
    AuthoringApplication {
        compressed: bool,
        application: App,
        application_id: u32,
        application_version: String,
    },
    Authorization {
        pid_content: u32,
        pids: Vec<String>,
    },
    Preset {
        dictionary_type: u32,
        preset_data: Vec<u8>,
    },
    SubTree(SubTree),
    Root {
        major: u8,
        minor: u8,
        patch: u16,
    },
    Raw(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubTree {
    Decoded(Box<Item>),
    CompressedUndecoded {
        uncompressed_size: u32,
        compressed_size: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum App {
    GuitarRig,
    Kontakt,
    Kore,
    Reaktor,
    Maschine,
    Absynth,
    Massive,
    Fm8,
    Battery,
    KompleteKontrol,
    Sc,
    Traktor,
    Other(u32),
}

impl App {
    pub fn from_id(id: u32) -> Self {
        match id {
            1 => App::GuitarRig,
            2 => App::Kontakt,
            3 => App::Kore,
            4 => App::Reaktor,
            5 => App::Maschine,
            6 => App::Absynth,
            7 => App::Massive,
            8 => App::Fm8,
            9 => App::Battery,
            10 => App::KompleteKontrol,
            11 => App::Sc,
            31 => App::Traktor,
            other => App::Other(other),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            App::GuitarRig => "GuitarRig",
            App::Kontakt => "Kontakt",
            App::Kore => "KORE",
            App::Reaktor => "Reaktor",
            App::Maschine => "Maschine",
            App::Absynth => "Absynth",
            App::Massive => "Massive",
            App::Fm8 => "FM8",
            App::Battery => "Battery",
            App::KompleteKontrol => "Komplete Kontrol",
            App::Sc => "SC",
            App::Traktor => "Traktor",
            App::Other(_) => "Unknown",
        }
    }
}

pub fn chunk_name(id: u32) -> &'static str {
    match id {
        CHUNK_TERMINATOR => "Terminator",
        CHUNK_AUTHORING_APPLICATION => "Authoring Application",
        CHUNK_AUTHORIZATION => "Authorization",
        CHUNK_SOUNDINFO_ITEM => "Soundinfo Item",
        CHUNK_PRESET_CHUNK_ITEM => "Preset Chunk Item",
        CHUNK_SUB_TREE_ITEM => "Subtree Item",
        CHUNK_CONTAINER_ROOT => "Container Root",
        3 => "BNI Sound Preset",
        4 => "BNI Sound Header",
        100 => "Bank",
        102 => "Bank Container",
        103 => "Preset Container",
        104 => "Binary Chunk Item",
        110 => "External File Reference",
        111 => "Resources",
        112 => "Audio Sample Item",
        113 => "Internal Resource Reference",
        114 => "Picture Item",
        116 => "Encryption Item",
        117 => "App Specific",
        120 => "Automation Parameters",
        121 => "Controller Assignments",
        122 => "Module",
        123 => "Module Bank",
        _ => "Unknown",
    }
}

/// True if `bytes` look like a top-level NI container item.
pub fn is_container(bytes: &[u8]) -> bool {
    if bytes.len() < 16 {
        return false;
    }
    let len = u64::from_le_bytes(bytes[0..8].try_into().unwrap());
    if len < 8 || len as usize > bytes.len() {
        return false;
    }
    bytes[8..12] == [1, 0, 0, 0] && bytes.get(12..16) == Some(DOMAIN_ITEM.as_slice())
}

pub fn parse(bytes: &[u8]) -> Result<Item, Error> {
    parse_with_pad(bytes, None)
}

/// Parse a container, optionally supplying a 64 KiB retail decryption pad.
/// The pad is only used when a compressed subtree is not plain FastLZ, so
/// unencrypted containers parse identically.
pub fn parse_with_pad(bytes: &[u8], pad: Option<&[u8]>) -> Result<Item, Error> {
    let mut reader = Reader::new(bytes);
    parse_item(&mut reader, 0, pad)
}

impl Item {
    /// First chunk of the given type, searching this item's chain and subtrees,
    /// then children depth-first.
    pub fn find_chunk(&self, chunk_type: u32) -> Option<&DataChunk> {
        let mut current = Some(&self.data);
        while let Some(chunk) = current {
            if chunk.chunk_type == chunk_type {
                return Some(chunk);
            }
            if let ChunkData::SubTree(SubTree::Decoded(item)) = &chunk.data {
                if let Some(found) = item.find_chunk(chunk_type) {
                    return Some(found);
                }
            }
            current = chunk.next.as_deref();
        }
        for child in &self.children {
            if let Some(found) = child.item.find_chunk(chunk_type) {
                return Some(found);
            }
        }
        None
    }

    /// All chunks of the given type, depth-first.
    pub fn find_all_chunks<'a>(&'a self, chunk_type: u32, out: &mut Vec<&'a DataChunk>) {
        let mut current = Some(&self.data);
        while let Some(chunk) = current {
            if chunk.chunk_type == chunk_type {
                out.push(chunk);
            }
            if let ChunkData::SubTree(SubTree::Decoded(item)) = &chunk.data {
                item.find_all_chunks(chunk_type, out);
            }
            current = chunk.next.as_deref();
        }
        for child in &self.children {
            child.item.find_all_chunks(chunk_type, out);
        }
    }

    /// True when the container carries an encryption item or a subtree this
    /// reader could not decode. Retail library content is often encrypted with
    /// the installed product key; report that distinctly from a malformed file.
    pub fn is_encrypted(&self) -> bool {
        self.find_chunk(CHUNK_ENCRYPTION_ITEM).is_some() || self.has_undecoded_subtree()
    }

    fn has_undecoded_subtree(&self) -> bool {
        let mut current = Some(&self.data);
        while let Some(chunk) = current {
            if matches!(
                &chunk.data,
                ChunkData::SubTree(SubTree::CompressedUndecoded { .. })
            ) {
                return true;
            }
            current = chunk.next.as_deref();
        }
        self.children
            .iter()
            .any(|child| child.item.has_undecoded_subtree())
    }

    pub fn dump(&self) -> String {
        let mut out = String::new();
        self.dump_into(&mut out, 0);
        out
    }

    fn dump_into(&self, out: &mut String, level: usize) {
        let pad = "  ".repeat(level);
        out.push_str(&format!(
            "{pad}Item version={} uuid={}\n",
            self.version,
            hex(&self.uuid)
        ));

        let mut chunk = Some(&self.data);
        while let Some(c) = chunk {
            out.push_str(&format!(
                "{pad}  Chunk {} ({}), v{}\n",
                c.chunk_type,
                chunk_name(c.chunk_type),
                c.version
            ));
            match &c.data {
                ChunkData::Terminator => {}
                ChunkData::AuthoringApplication {
                    compressed,
                    application,
                    application_id,
                    application_version,
                } => out.push_str(&format!(
                    "{pad}    app={} ({application_id}), version={application_version}, compressed={compressed}\n",
                    application.name()
                )),
                ChunkData::Authorization { pid_content, pids } => out.push_str(&format!(
                    "{pad}    pid_content={pid_content}, pids={pids:?}\n"
                )),
                ChunkData::Preset {
                    dictionary_type,
                    preset_data,
                } => out.push_str(&format!(
                    "{pad}    dictionary_type={dictionary_type}, preset_data={} bytes\n",
                    preset_data.len()
                )),
                ChunkData::SubTree(SubTree::Decoded(item)) => {
                    out.push_str(&format!("{pad}    subtree:\n"));
                    item.dump_into(out, level + 3);
                }
                ChunkData::SubTree(SubTree::CompressedUndecoded {
                    uncompressed_size,
                    compressed_size,
                }) => out.push_str(&format!(
                    "{pad}    compressed subtree: {compressed_size} -> {uncompressed_size} bytes\n"
                )),
                ChunkData::Root {
                    major,
                    minor,
                    patch,
                } => out.push_str(&format!("{pad}    ni sound version {major}.{minor}.{patch}\n")),
                ChunkData::Raw(bytes) => {
                    out.push_str(&format!("{pad}    raw {} bytes\n", bytes.len()))
                }
            }
            chunk = c.next.as_deref();
        }

        for child in &self.children {
            out.push_str(&format!(
                "{pad}  Child[{}] {} type={} ({})\n",
                child.index,
                child.domain,
                child.chunk_type,
                chunk_name(child.chunk_type)
            ));
            child.item.dump_into(out, level + 2);
        }
    }
}

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

fn parse_item(reader: &mut Reader<'_>, depth: usize, pad: Option<&[u8]>) -> Result<Item, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Malformed("container nesting too deep".into()));
    }
    let block = read_block64(reader)?;
    let mut r = Reader::new(block);

    let header_version = r.u32()?;
    if header_version != 1 {
        return Err(Error::Malformed(format!(
            "item header version {header_version}, expected 1"
        )));
    }
    let domain = r.ascii4()?;
    if domain.as_bytes() != DOMAIN_ITEM {
        return Err(Error::Malformed(format!(
            "item domain {domain:?}, expected \"hsin\""
        )));
    }
    let _ = r.u32()?;
    let _ = r.u32()?;
    let uuid: [u8; 16] = r.take(16)?.try_into().unwrap();

    let data = parse_chunk(&mut r, depth, pad)?;
    let version = r.u32()?;
    let num_children = r.u32()? as usize;
    if num_children > 1_000_000 {
        return Err(Error::Malformed(format!(
            "implausible child count {num_children}"
        )));
    }

    let mut children = Vec::with_capacity(num_children.min(1024));
    for _ in 0..num_children {
        let index = r.u32()?;
        let domain = r.ascii4()?;
        let chunk_type = r.u32()?;
        let item = parse_item(&mut r, depth + 1, pad)?;
        children.push(Child {
            index,
            domain,
            chunk_type,
            item,
        });
    }

    Ok(Item {
        uuid,
        version,
        data,
        children,
    })
}

fn parse_chunk(
    reader: &mut Reader<'_>,
    depth: usize,
    pad: Option<&[u8]>,
) -> Result<DataChunk, Error> {
    if depth > MAX_DEPTH {
        return Err(Error::Malformed("chunk nesting too deep".into()));
    }
    let block = read_block64(reader)?;
    let mut r = Reader::new(block);

    let domain = r.ascii4()?;
    let chunk_type = r.u32()?;
    let version = r.u32()?;
    if version != 1 {
        return Err(Error::Malformed(format!(
            "chunk {chunk_type} version {version}, expected 1"
        )));
    }

    let next = if chunk_type != CHUNK_TERMINATOR {
        Some(Box::new(parse_chunk(&mut r, depth + 1, pad)?))
    } else {
        None
    };

    let data = parse_chunk_data(&mut r, chunk_type, depth, pad)?;

    Ok(DataChunk {
        domain,
        chunk_type,
        version,
        next,
        data,
    })
}

fn parse_chunk_data(
    r: &mut Reader<'_>,
    chunk_type: u32,
    depth: usize,
    pad: Option<&[u8]>,
) -> Result<ChunkData, Error> {
    match chunk_type {
        CHUNK_TERMINATOR => {
            expect_version(r)?;
            Ok(ChunkData::Terminator)
        }
        CHUNK_AUTHORING_APPLICATION => {
            expect_version(r)?;
            let compressed = r.u8()? != 0;
            let application_id = r.u32()?;
            let _unknown = r.u32()?;
            let application_version = r.utf16()?;
            Ok(ChunkData::AuthoringApplication {
                compressed,
                application: App::from_id(application_id),
                application_id,
                application_version,
            })
        }
        CHUNK_AUTHORIZATION => {
            expect_version(r)?;
            let pid_content = r.u32()?;
            let mut pids = Vec::new();
            if pid_content == 1 {
                let _license_blocks = r.u32()?;
                let num_pids = r.u32()?;
                if num_pids > 1_000_000 {
                    return Err(Error::Malformed(format!(
                        "implausible pid count {num_pids}"
                    )));
                }
                for _ in 0..num_pids {
                    pids.push(r.utf16()?);
                }
                let _unknown1 = r.u32()?;
                let _unknown2 = r.u32()?;
                let _checksum = r.u32()?;
            }
            Ok(ChunkData::Authorization { pid_content, pids })
        }
        CHUNK_PRESET_CHUNK_ITEM => {
            expect_version(r)?;
            let dictionary_type = r.u32()?;
            let number_of_items = r.u32()?;
            if number_of_items != 1 {
                return Err(Error::Malformed(format!(
                    "preset dictionary has {number_of_items} entries, expected 1"
                )));
            }
            let size_of_item = r.u32()? as usize;
            let _unknown = r.u32()?;
            let preset_data = r.take(size_of_item)?.to_vec();
            let _padding = r.u32()?;
            let _magic = r.u32()?;
            Ok(ChunkData::Preset {
                dictionary_type,
                preset_data,
            })
        }
        CHUNK_SUB_TREE_ITEM => {
            expect_version(r)?;
            let compressed = r.u8()? != 0;
            if compressed {
                let uncompressed_size = r.u32()?;
                let compressed_size = r.u32()?;
                let data = r.take(compressed_size as usize)?;
                let mut decoded = crate::fastlz::decompress(data, uncompressed_size as usize).ok();
                if decoded.is_none() {
                    if let Some(pad) = pad.filter(|pad| !pad.is_empty()) {
                        let decrypted: Vec<u8> = data
                            .iter()
                            .enumerate()
                            .map(|(index, byte)| byte ^ pad[index % pad.len()])
                            .collect();
                        decoded =
                            crate::fastlz::decompress(&decrypted, uncompressed_size as usize).ok();
                    }
                }
                match decoded {
                    Some(uncompressed) => {
                        let item = parse_item(&mut Reader::new(&uncompressed), depth + 1, pad)?;
                        Ok(ChunkData::SubTree(SubTree::Decoded(Box::new(item))))
                    }
                    None => Ok(ChunkData::SubTree(SubTree::CompressedUndecoded {
                        uncompressed_size,
                        compressed_size,
                    })),
                }
            } else {
                let item = parse_item(r, depth + 1, pad)?;
                Ok(ChunkData::SubTree(SubTree::Decoded(Box::new(item))))
            }
        }
        CHUNK_CONTAINER_ROOT => {
            expect_version(r)?;
            let ni_sound_version = r.u32()?;
            let _repository_magic = r.u32()?;
            let _repository_type = r.u32()?;
            Ok(ChunkData::Root {
                major: ((ni_sound_version >> 20) & 0xFF) as u8,
                minor: ((ni_sound_version >> 12) & 0xFF) as u8,
                patch: (ni_sound_version & 0xFFF) as u16,
            })
        }
        _ => Ok(ChunkData::Raw(r.take(r.remaining())?.to_vec())),
    }
}

fn expect_version(r: &mut Reader<'_>) -> Result<(), Error> {
    let version = r.u32()?;
    if version != 1 {
        return Err(Error::Malformed(format!(
            "chunk payload version {version}, expected 1"
        )));
    }
    Ok(())
}

fn read_block64<'a>(r: &mut Reader<'a>) -> Result<&'a [u8], Error> {
    let len = r.u64()?;
    if len < 8 {
        return Err(Error::Malformed(format!("block64 length {len} < 8")));
    }
    let n = usize::try_from(len - 8)
        .map_err(|_| Error::Malformed(format!("block64 length {len} too large")))?;
    r.take(n)
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

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }

    fn u32(&mut self) -> Result<u32, Error> {
        let b = self.take(4)?;
        Ok(u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn u64(&mut self) -> Result<u64, Error> {
        let b = self.take(8)?;
        Ok(u64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
    }

    fn ascii4(&mut self) -> Result<String, Error> {
        let b = self.take(4)?;
        Ok(b.iter().map(|&c| c as char).collect())
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
