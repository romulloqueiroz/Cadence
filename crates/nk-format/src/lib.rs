//! Reader for Native Instruments Kontakt instrument containers.
//!
//! Scope: plain (user-authored) `.nki` files, i.e. a `0x7FA89012` header
//! followed by a zlib stream containing `K2_Container` XML. Factory content
//! uses a protected/opaque header variant and is reported as such.
//!
//! The format facts here are recovery-backed: the outer container from
//! `BPatchHeaderV42` / `BFile` in the decompiled binary, and the XML payload
//! verified against files written by Kontakt itself. See `SPEC.md`.

pub mod audio;
pub mod container;
pub mod encryption;
pub mod error;
pub mod external_mod;
mod fastlz;
pub mod file_container;
pub mod fx;
pub mod k2_monolith;
pub mod loader;
mod locations;
pub mod model;
pub mod modulator;
pub mod ncw;
pub mod ni_container;
pub mod plain_model;
pub mod preset_chunk;
pub mod program;
pub mod start_criteria;
pub mod stream;
pub mod units;
pub mod wav;
pub mod xml;
pub mod zone;

pub use container::{probe, Container, Header, Kind, PLAIN_MAGIC};
pub use error::Error;
pub use program::{Document, Group, Params, Program, Zone};
pub use xml::Element;

/// Read and parse a container from disk.
pub fn read_path(path: impl AsRef<std::path::Path>) -> Result<Container, Error> {
    let bytes = std::fs::read(path)?;
    Container::parse(&bytes)
}
