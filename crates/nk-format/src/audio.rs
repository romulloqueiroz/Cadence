//! Unified decoded sample representation and format sniffing.

use crate::{ncw, wav, Error};

#[derive(Debug, Clone)]
pub struct Sample {
    pub channels: usize,
    pub sample_rate: u32,
    pub num_frames: usize,
    /// De-interleaved, normalized to `-1.0..=1.0` except for float sources.
    pub samples: Vec<Vec<f32>>,
}

impl From<ncw::Ncw> for Sample {
    fn from(n: ncw::Ncw) -> Self {
        Sample {
            channels: n.channels,
            sample_rate: n.sample_rate,
            num_frames: n.num_frames,
            samples: n.samples,
        }
    }
}

/// Decode a sample by content, not by file extension.
pub fn load(bytes: &[u8]) -> Result<Sample, Error> {
    if bytes.len() >= 4 && u32::from_le_bytes(bytes[0..4].try_into().unwrap()) == ncw::FILE_MAGIC {
        return Ok(Sample::from(ncw::parse(bytes)?));
    }
    if bytes.starts_with(b"RIFF") {
        return wav::parse(bytes);
    }
    Err(Error::Malformed(
        "unrecognised sample format (expected .ncw or .wav)".into(),
    ))
}
