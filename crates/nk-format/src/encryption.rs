//! Retail Kontakt library decryption.
//!
//! Retail `.nki` presets and `.nkx` samples are XOR-encrypted with a 64 KiB
//! per-product pad. The pad is AES-CTR over a static seed embedded in Kontakt:
//!
//! ```text
//! pad = AES-CTR(key = JDX, iv = HU) over bEncOneTimePad (64 KiB)
//! ```
//!
//! `JDX` and `HU` ship in the library's `.nicnt` metadata (or are installed in
//! the NI registry by Native Access). This module derives the pad and applies
//! it; it never writes to the library or the Kontakt installation.

use crate::Error;
use std::path::{Path, PathBuf};

const PAD_BYTES: usize = 0x10000;
const DEFAULT_KONTAKT: &str =
    "/Applications/Native Instruments/Kontakt 8/Kontakt 8.app/Contents/MacOS/Kontakt 8";

#[derive(Debug, Clone)]
pub struct ProductKey {
    pub snpid: String,
    pub reg_key: String,
    pub hu: [u8; 16],
    pub jdx: Vec<u8>,
    pad: Vec<u8>,
}

impl ProductKey {
    pub fn pad(&self) -> &[u8] {
        &self.pad
    }

    /// XOR a range that starts at offset 0 of the encrypted payload. The pad is
    /// reused every 64 KiB, matching `BFileEncrypter::DecryptBuffer`.
    pub fn xor(&self, data: &[u8]) -> Vec<u8> {
        data.iter()
            .enumerate()
            .map(|(index, byte)| byte ^ self.pad[index % self.pad.len()])
            .collect()
    }
}

#[derive(Debug, Clone)]
struct NicntProduct {
    snpid: String,
    reg_key: String,
    hu: [u8; 16],
    jdx: Vec<u8>,
}

/// Find the retail key that matches any of the instrument's authorization PIDs.
/// Searches the library root (up to four ancestors) for `.nicnt` metadata.
pub fn find_key(path: &Path, pids: &[String]) -> Result<Option<ProductKey>, Error> {
    let mut nicnts = Vec::new();
    for ancestor in path.ancestors().skip(1).take(4) {
        let Ok(entries) = std::fs::read_dir(ancestor) else {
            continue;
        };
        for entry in entries.flatten() {
            let candidate = entry.path();
            if candidate
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("nicnt"))
            {
                nicnts.push(candidate);
            }
        }
    }
    nicnts.sort();
    nicnts.dedup();

    let mut fallback = None;
    let multiple = nicnts.len() > 1;
    for nicnt in nicnts {
        for product in products(&nicnt)? {
            let matches = pids
                .iter()
                .any(|pid| pid.eq_ignore_ascii_case(&product.snpid));
            if !matches && (!pids.is_empty() || multiple) {
                continue;
            }
            let pad = derive_pad(&product.jdx, &product.hu)?;
            let key = ProductKey {
                snpid: product.snpid,
                reg_key: product.reg_key,
                hu: product.hu,
                jdx: product.jdx,
                pad,
            };
            if matches {
                return Ok(Some(key));
            }
            fallback.get_or_insert(key);
        }
    }
    Ok(fallback)
}

fn products(path: &Path) -> Result<Vec<NicntProduct>, Error> {
    let bytes = std::fs::read(path)?;
    let text = String::from_utf8_lossy(&bytes);
    let mut out = Vec::new();
    let mut rest = text.as_ref();
    while let Some(start) = rest.find("<Product") {
        let Some(length) = rest[start..].find("</Product>") else {
            break;
        };
        let block = &rest[start..start + length];
        let snpid = tag(block, "SNPID").unwrap_or_default();
        let reg_key = tag(block, "RegKey").unwrap_or_default();
        let hu = tag(block, "HU").and_then(|value| hex_bytes::<16>(&value));
        let jdx = tag(block, "JDX").and_then(|value| hex_vec(&value));
        if let (Some(hu), Some(jdx)) = (hu, jdx) {
            if jdx.len() >= 16 {
                out.push(NicntProduct {
                    snpid,
                    reg_key,
                    hu,
                    jdx,
                });
            }
        }
        rest = &rest[start + length + "</Product>".len()..];
    }
    Ok(out)
}

fn tag(block: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = block.find(&open)? + open.len();
    let end = block[start..].find(&close)? + start;
    Some(block[start..end].trim().to_string())
}

fn hex_bytes<const N: usize>(value: &str) -> Option<[u8; N]> {
    let bytes = hex_vec(value)?;
    bytes.try_into().ok()
}

fn hex_vec(value: &str) -> Option<Vec<u8>> {
    let clean: Vec<u8> = value
        .bytes()
        .filter(|byte| !byte.is_ascii_whitespace())
        .collect();
    if clean.len() % 2 != 0 {
        return None;
    }
    clean
        .chunks_exact(2)
        .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).ok()?, 16).ok())
        .collect()
}

fn derive_pad(jdx: &[u8], hu: &[u8; 16]) -> Result<Vec<u8>, Error> {
    if !matches!(jdx.len(), 16 | 24 | 32) {
        return Err(Error::Malformed(format!(
            "unsupported retail key length {}",
            jdx.len()
        )));
    }
    let seed = seed()?;
    aes_ctr(jdx, hu, &seed)
}

/// The static 64 KiB seed. `CADENCE_ENC_PAD` may point at a raw seed file;
/// otherwise the seed is read from the installed Kontakt binary.
fn seed() -> Result<Vec<u8>, Error> {
    if let Some(path) = std::env::var_os("CADENCE_ENC_PAD") {
        let bytes = std::fs::read(PathBuf::from(path))?;
        if bytes.len() < PAD_BYTES {
            return Err(Error::Malformed("encryption seed is too short".into()));
        }
        return Ok(bytes[..PAD_BYTES].to_vec());
    }
    if let Some(path) = std::env::var_os("CADENCE_KONTAKT_BINARY") {
        return extract_seed(&PathBuf::from(path));
    }
    // The installed Kontakt binary is ~450 MiB; read it once per process.
    static DEFAULT_SEED: std::sync::OnceLock<Option<Vec<u8>>> = std::sync::OnceLock::new();
    DEFAULT_SEED
        .get_or_init(|| extract_seed(Path::new(DEFAULT_KONTAKT)).ok())
        .clone()
        .ok_or_else(|| {
            Error::Malformed(
                "retail decryption needs a licensed Kontakt 8 installation or CADENCE_ENC_PAD"
                    .into(),
            )
        })
}

/// Read the `g_aPrivateKey`/`bEncOneTimePad` 64 KiB constant from a Mach-O
/// binary using its symbol table and segment load commands.
fn extract_seed(binary: &Path) -> Result<Vec<u8>, Error> {
    let file = std::fs::read(binary).map_err(|error| {
        Error::Malformed(format!(
            "cannot read Kontakt for retail decryption at {}: {error}",
            binary.display()
        ))
    })?;
    if file.len() < 32 {
        return Err(Error::Malformed("Kontakt binary is too short".into()));
    }
    // The installed Kontakt is a universal binary; select the arm64 slice so
    // its load-command file offsets stay slice-relative.
    let fat = u32::from_be_bytes(file[0..4].try_into().unwrap());
    let bytes: &[u8] = if fat == 0xCAFE_BABE {
        let count = u32::from_be_bytes(file[4..8].try_into().unwrap()) as usize;
        let mut slice = None;
        for index in 0..count {
            let base = 8 + index * 20;
            if base + 20 > file.len() {
                break;
            }
            let cpu = u32::from_be_bytes(file[base..base + 4].try_into().unwrap());
            let offset = u32::from_be_bytes(file[base + 8..base + 12].try_into().unwrap()) as usize;
            let size = u32::from_be_bytes(file[base + 12..base + 16].try_into().unwrap()) as usize;
            if cpu == 0x0100_000C {
                slice =
                    Some(file.get(offset..offset + size).ok_or_else(|| {
                        Error::Malformed("Kontakt arm64 slice is truncated".into())
                    })?);
                break;
            }
        }
        slice.ok_or_else(|| Error::Malformed("Kontakt has no arm64 slice".into()))?
    } else {
        &file[..]
    };
    if bytes.len() < 32 {
        return Err(Error::Malformed("Kontakt binary is too short".into()));
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    let (header, commands_offset) = match magic {
        0xFEEDFACF => (32usize, 16usize),
        0xFEEDFACE => (28usize, 16usize),
        _ => {
            return Err(Error::Malformed(
                "Kontakt is not a supported Mach-O binary".into(),
            ))
        }
    };
    let command_count = u32::from_le_bytes(
        bytes[commands_offset..commands_offset + 4]
            .try_into()
            .unwrap(),
    ) as usize;
    let mut offset = header;
    let mut segments = Vec::new();
    let mut symtab = None;
    for _ in 0..command_count {
        if offset + 8 > bytes.len() {
            break;
        }
        let command = u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap());
        let size = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        if size < 8 {
            break;
        }
        if command == 0x19 && offset + 56 <= bytes.len() {
            let vmaddr = u64::from_le_bytes(bytes[offset + 24..offset + 32].try_into().unwrap());
            let vmsize = u64::from_le_bytes(bytes[offset + 32..offset + 40].try_into().unwrap());
            let fileoff = u64::from_le_bytes(bytes[offset + 40..offset + 48].try_into().unwrap());
            let filesize = u64::from_le_bytes(bytes[offset + 48..offset + 56].try_into().unwrap());
            segments.push((vmaddr, vmsize, fileoff, filesize));
        } else if command == 0x2 && offset + 24 <= bytes.len() {
            let symoff =
                u32::from_le_bytes(bytes[offset + 8..offset + 12].try_into().unwrap()) as usize;
            let nsyms =
                u32::from_le_bytes(bytes[offset + 12..offset + 16].try_into().unwrap()) as usize;
            let stroff =
                u32::from_le_bytes(bytes[offset + 16..offset + 20].try_into().unwrap()) as usize;
            symtab = Some((symoff, nsyms, stroff));
        }
        offset += size;
    }
    let (symoff, nsyms, stroff) =
        symtab.ok_or_else(|| Error::Malformed("Kontakt has no symbol table".into()))?;
    let targets: [&[u8]; 2] = [b"__ZN2NI5SOUND6DETAIL13g_aPrivateKeyE", b"_bEncOneTimePad"];
    for index in 0..nsyms {
        let entry = symoff + index * 16;
        if entry + 16 > bytes.len() {
            break;
        }
        let name_offset = u32::from_le_bytes(bytes[entry..entry + 4].try_into().unwrap()) as usize;
        let address = u64::from_le_bytes(bytes[entry + 8..entry + 16].try_into().unwrap());
        let name_start = stroff.saturating_add(name_offset);
        let Some(name_end) = bytes[name_start..]
            .iter()
            .position(|byte| *byte == 0)
            .map(|length| name_start + length)
        else {
            continue;
        };
        let name = &bytes[name_start..name_end];
        if !targets.contains(&name) {
            continue;
        }
        for (vmaddr, vmsize, fileoff, filesize) in &segments {
            if address >= *vmaddr && address < vmaddr.saturating_add(*vmsize) {
                let start = *fileoff + (address - vmaddr);
                let end = start.saturating_add(PAD_BYTES as u64);
                if end <= start.saturating_add(*filesize) {
                    let start = usize::try_from(start).map_err(|_| {
                        Error::Malformed("Kontakt seed offset is out of range".into())
                    })?;
                    return bytes
                        .get(start..start + PAD_BYTES)
                        .map(|slice| slice.to_vec())
                        .ok_or_else(|| Error::Malformed("Kontakt seed is truncated".into()));
                }
            }
        }
    }
    Err(Error::Malformed(
        "Kontakt retail seed symbol was not found".into(),
    ))
}

#[cfg(target_os = "macos")]
fn aes_ctr(key: &[u8], iv: &[u8; 16], input: &[u8]) -> Result<Vec<u8>, Error> {
    use std::os::raw::{c_int, c_uint, c_void};

    type Cryptor = *mut c_void;
    #[link(name = "System")]
    extern "C" {
        fn CCCryptorCreateWithMode(
            operation: c_uint,
            mode: c_uint,
            algorithm: c_uint,
            padding: c_uint,
            iv: *const c_void,
            key: *const c_void,
            key_length: usize,
            tweak: *const c_void,
            tweak_length: usize,
            rounds: c_int,
            options: c_uint,
            cryptor: *mut Cryptor,
        ) -> c_int;
        fn CCCryptorUpdate(
            cryptor: Cryptor,
            data_in: *const c_void,
            data_in_length: usize,
            data_out: *mut c_void,
            data_out_available: usize,
            data_out_moved: *mut usize,
        ) -> c_int;
        fn CCCryptorRelease(cryptor: Cryptor) -> c_int;
    }

    const ENCRYPT: c_uint = 0;
    const MODE_CTR: c_uint = 4;
    const ALGORITHM_AES: c_uint = 0;
    const PADDING_NONE: c_uint = 0;
    const OPTION_CTR_BE: c_uint = 0x0002;

    let mut cryptor: Cryptor = std::ptr::null_mut();
    let status = unsafe {
        CCCryptorCreateWithMode(
            ENCRYPT,
            MODE_CTR,
            ALGORITHM_AES,
            PADDING_NONE,
            iv.as_ptr().cast(),
            key.as_ptr().cast(),
            key.len(),
            std::ptr::null(),
            0,
            0,
            OPTION_CTR_BE,
            &mut cryptor,
        )
    };
    if status != 0 || cryptor.is_null() {
        return Err(Error::Malformed(format!(
            "AES-CTR setup failed (status {status})"
        )));
    }
    let mut output = vec![0u8; input.len()];
    let mut moved = 0usize;
    let status = unsafe {
        CCCryptorUpdate(
            cryptor,
            input.as_ptr().cast(),
            input.len(),
            output.as_mut_ptr().cast(),
            output.len(),
            &mut moved,
        )
    };
    unsafe {
        CCCryptorRelease(cryptor);
    }
    if status != 0 {
        return Err(Error::Malformed(format!(
            "AES-CTR update failed (status {status})"
        )));
    }
    output.truncate(moved);
    Ok(output)
}

#[cfg(not(target_os = "macos"))]
fn aes_ctr(_key: &[u8], _iv: &[u8; 16], _input: &[u8]) -> Result<Vec<u8>, Error> {
    Err(Error::Malformed(
        "retail Kontakt decryption currently requires macOS CommonCrypto".into(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aes_ctr_matches_grovebass_pad_vector() {
        let Ok(path) = std::env::var("CADENCE_ENC_PAD") else {
            return;
        };
        let seed = std::fs::read(path).unwrap();
        let hu: [u8; 16] = hex_bytes::<16>("BD7BB10C7C8F390D4424FBBF4B4421CE").unwrap();
        let jdx =
            hex_vec("F87ADC91B65E92B06483B3D84A4C78ACF9BC06189EB60AD439AC1FD58E0BEC2C").unwrap();
        let pad = aes_ctr(&jdx, &hu, &seed[..PAD_BYTES]).unwrap();
        let expected =
            hex_vec("c6be1ca86d984a93b71cac88ce9ea0c5458a883563af36294176b71a54c2d7e2").unwrap();
        assert_eq!(&pad[..32], &expected[..]);
    }
}
