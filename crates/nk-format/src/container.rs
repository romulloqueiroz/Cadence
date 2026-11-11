//! Reader for the outer Kontakt instrument container.
//!
//! A plain (user-authored) `.nki` is a short little-endian header followed by a
//! zlib stream that inflates to K2 XML. This layout was recovered from the
//! shipped binary (`BPatchHeaderV42`, `BFile`) and verified against real files
//! written by Kontakt; see `SPEC.md`.

use crate::{file_container, ni_container, xml::Element, Error};
use std::io::Read;

/// Little-endian magic of a plain instrument container: bytes `12 90 A8 7F`.
pub const PLAIN_MAGIC: u32 = 0x7FA8_9012;
/// Payload offset observed for the `0x0100` container revision.
pub const DEFAULT_PAYLOAD_OFFSET: usize = 0xAA;
/// Hard cap on inflated XML to keep hostile files bounded.
pub const MAX_XML_BYTES: usize = 64 * 1024 * 1024;

const HEADER_MIN: usize = 0x20;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Plain container with a zlib XML payload.
    Plain,
    /// Native Instruments file container (`hsin` item tree), used by Kontakt 5+.
    Container,
    /// NI file container (`/\ NI FC MTD  /\`), used by monoliths.
    FileContainer,
    /// Kontakt 2 monolith/resource container (`.nkr`).
    K2Monolith,
    /// Kontakt collection container (`.nkc`).
    Collection,
    /// Unrecognised header (a variant not yet reverse engineered).
    Opaque {
        magic: u32,
    },
    TooShort,
}

/// Classify a file without committing to a full parse.
pub fn probe(bytes: &[u8]) -> Kind {
    if ni_container::is_container(bytes) {
        return Kind::Container;
    }
    if file_container::is_container(bytes) {
        return Kind::FileContainer;
    }
    match u32_at(bytes, 0) {
        Ok(PLAIN_MAGIC) => Kind::Plain,
        Ok(0x5E70_AC54) => Kind::K2Monolith,
        Ok(0x3FE1_107A) => Kind::Collection,
        Ok(magic) => Kind::Opaque { magic },
        Err(Error::Truncated { .. }) => Kind::TooShort,
        Err(_) => Kind::TooShort,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Header {
    pub magic: u32,
    /// Unknown in the recovery; preserved verbatim.
    pub field_04: u32,
    /// Container revision, e.g. `0x0100`.
    pub format_version: u16,
    /// Unknown in the recovery; preserved verbatim.
    pub field_0a: u32,
    /// Header flags; `1` in observed files.
    pub flags: u16,
    /// Packed application version (see [`Header::app_version_octets`]).
    pub app_version: u32,
    pub app_signature: u32,
    /// Unix timestamp of the last save, as stored.
    pub saved_at: i32,
    /// Unknown in the recovery; preserved verbatim.
    pub field_1c: u32,
    /// Byte offset where the zlib payload begins.
    pub payload_offset: usize,
}

impl Header {
    /// Four octets packed by `BPatchHeaderV42::createAppVersionNumber`.
    pub fn app_version_octets(&self) -> [u8; 4] {
        [
            (self.app_version >> 24) as u8,
            (self.app_version >> 16) as u8,
            (self.app_version >> 8) as u8,
            self.app_version as u8,
        ]
    }
}

#[derive(Debug)]
pub struct Container {
    pub header: Header,
    pub xml: String,
    pub root: Element,
}

impl Container {
    pub fn parse(bytes: &[u8]) -> Result<Container, Error> {
        let magic = u32_at(bytes, 0)?;
        if magic != PLAIN_MAGIC {
            return Err(Error::UnsupportedMagic { magic });
        }
        if bytes.len() < HEADER_MIN {
            return Err(Error::Truncated {
                needed: HEADER_MIN,
                len: bytes.len(),
            });
        }

        let (payload_offset, payload) = locate_payload(bytes)?;
        let xml = String::from_utf8(payload)?;
        let root = Element::parse(&xml)?;
        if root.name != "K2_Container" && root.name != "K2_Bank" {
            return Err(Error::UnexpectedRoot {
                found: root.name.clone(),
            });
        }

        Ok(Container {
            header: Header {
                magic,
                field_04: u32_at(bytes, 0x04)?,
                format_version: u16_at(bytes, 0x08)?,
                field_0a: u32_at(bytes, 0x0A)?,
                flags: u16_at(bytes, 0x0E)?,
                app_version: u32_at(bytes, 0x10)?,
                app_signature: u32_at(bytes, 0x14)?,
                saved_at: i32_at(bytes, 0x18)?,
                field_1c: u32_at(bytes, 0x1C)?,
                payload_offset,
            },
            xml,
            root,
        })
    }
}

fn locate_payload(bytes: &[u8]) -> Result<(usize, Vec<u8>), Error> {
    let mut candidates = vec![DEFAULT_PAYLOAD_OFFSET];
    let scan_end = bytes.len().min(0x1000);
    for off in 0x80..scan_end {
        let is_zlib = bytes[off] == 0x78
            && bytes
                .get(off + 1)
                .is_some_and(|b| matches!(b, 0x01 | 0x5E | 0x9C | 0xDA));
        if is_zlib && !candidates.contains(&off) {
            candidates.push(off);
        }
    }

    for off in candidates {
        if off >= bytes.len() {
            continue;
        }
        match inflate(&bytes[off..]) {
            Ok(payload) if looks_like_xml(&payload) => return Ok((off, payload)),
            _ => continue,
        }
    }
    Err(Error::NoPayload)
}

fn inflate(data: &[u8]) -> Result<Vec<u8>, Error> {
    let mut decoder = flate2::read::ZlibDecoder::new(data);
    let mut out = Vec::new();
    let mut limited = (&mut decoder).take(MAX_XML_BYTES as u64 + 1);
    limited.read_to_end(&mut out).map_err(Error::Io)?;
    if out.len() > MAX_XML_BYTES {
        return Err(Error::PayloadTooLarge {
            limit: MAX_XML_BYTES,
        });
    }
    if out.is_empty() {
        return Err(Error::NoPayload);
    }
    Ok(out)
}

fn looks_like_xml(bytes: &[u8]) -> bool {
    let start = bytes
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(bytes.len());
    let head = &bytes[start..];
    head.starts_with(b"<?xml") || head.starts_with(b"<K2_") || head.starts_with(b"<Kontakt")
}

fn u16_at(bytes: &[u8], off: usize) -> Result<u16, Error> {
    let s = bytes.get(off..off + 2).ok_or(Error::Truncated {
        needed: off + 2,
        len: bytes.len(),
    })?;
    Ok(u16::from_le_bytes([s[0], s[1]]))
}

fn u32_at(bytes: &[u8], off: usize) -> Result<u32, Error> {
    let s = bytes.get(off..off + 4).ok_or(Error::Truncated {
        needed: off + 4,
        len: bytes.len(),
    })?;
    Ok(u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
}

fn i32_at(bytes: &[u8], off: usize) -> Result<i32, Error> {
    u32_at(bytes, off).map(|v| v as i32)
}
