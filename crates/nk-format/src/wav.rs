//! Minimal RIFF/WAVE reader used for Kontakt libraries that reference `.wav`
//! samples. Supports PCM 8/16/24/32, IEEE float 32/64, and WAVE_FORMAT_EXTENSIBLE.

use crate::{audio::Sample, Error};

pub fn parse(bytes: &[u8]) -> Result<Sample, Error> {
    if bytes.len() < 12 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WAVE" {
        return Err(Error::Malformed("not a RIFF/WAVE file".into()));
    }

    let mut format: Option<Format> = None;
    let mut data: Option<&[u8]> = None;
    let mut pos = 12usize;

    while pos + 8 <= bytes.len() {
        let id = &bytes[pos..pos + 4];
        let size = u32::from_le_bytes(bytes[pos + 4..pos + 8].try_into().unwrap()) as usize;
        let body_start = pos + 8;
        let body_end = body_start.saturating_add(size).min(bytes.len());
        let body = &bytes[body_start..body_end];

        match id {
            b"fmt " => format = Some(parse_fmt(body)?),
            b"data" => data = Some(body),
            _ => {}
        }

        pos = body_start + size + (size & 1);
    }

    let format = format.ok_or_else(|| Error::Malformed("WAV has no fmt chunk".into()))?;
    let data = data.ok_or_else(|| Error::Malformed("WAV has no data chunk".into()))?;
    decode(format, data)
}

#[derive(Debug, Clone, Copy)]
pub(crate) struct Format {
    pub(crate) format_tag: u16,
    pub(crate) channels: u16,
    pub(crate) sample_rate: u32,
    pub(crate) bits_per_sample: u16,
    pub(crate) block_align: u16,
}

pub(crate) fn parse_fmt(body: &[u8]) -> Result<Format, Error> {
    if body.len() < 16 {
        return Err(Error::Malformed("short WAV fmt chunk".into()));
    }
    let mut format_tag = u16::from_le_bytes([body[0], body[1]]);
    let channels = u16::from_le_bytes([body[2], body[3]]);
    let sample_rate = u32::from_le_bytes([body[4], body[5], body[6], body[7]]);
    let block_align = u16::from_le_bytes([body[12], body[13]]);
    let bits_per_sample = u16::from_le_bytes([body[14], body[15]]);

    if format_tag == 0xFFFE && body.len() >= 26 {
        // WAVE_FORMAT_EXTENSIBLE: the real tag is the first two bytes of the
        // sub-format GUID at offset 24.
        format_tag = u16::from_le_bytes([body[24], body[25]]);
    }

    Ok(Format {
        format_tag,
        channels,
        sample_rate,
        bits_per_sample,
        block_align,
    })
}

fn decode(format: Format, data: &[u8]) -> Result<Sample, Error> {
    let channels = format.channels as usize;
    if channels == 0 || channels > 64 {
        return Err(Error::Malformed(format!(
            "implausible WAV channel count {channels}"
        )));
    }
    let bytes_per_sample = (format.bits_per_sample as usize).div_ceil(8);
    let frame_size = if format.block_align as usize >= bytes_per_sample * channels {
        format.block_align as usize
    } else {
        bytes_per_sample * channels
    };
    if frame_size == 0 {
        return Err(Error::Malformed("zero WAV frame size".into()));
    }
    let num_frames = data.len() / frame_size;

    let mut samples = vec![vec![0f32; num_frames]; channels];

    for (frame, chunk) in data.chunks_exact(frame_size).enumerate() {
        for (channel, out) in samples.iter_mut().enumerate() {
            let start = channel * bytes_per_sample;
            let slice = &chunk[start..start + bytes_per_sample];
            out[frame] = decode_sample(format, slice);
        }
    }

    Ok(Sample {
        channels,
        sample_rate: format.sample_rate,
        num_frames,
        samples,
    })
}

pub(crate) fn decode_sample(format: Format, bytes: &[u8]) -> f32 {
    let is_float = format.format_tag == 3;
    match (is_float, format.bits_per_sample) {
        (true, 32) => f32::from_le_bytes(bytes[0..4].try_into().unwrap()),
        (true, 64) => f64::from_le_bytes(bytes[0..8].try_into().unwrap()) as f32,
        (false, 8) => (bytes[0] as f32 - 128.0) / 128.0,
        (false, 16) => i16::from_le_bytes([bytes[0], bytes[1]]) as f32 / 32768.0,
        (false, 24) => {
            let value = (bytes[0] as i32) | ((bytes[1] as i32) << 8) | ((bytes[2] as i32) << 16);
            let value = (value << 8) >> 8;
            value as f32 / 8_388_608.0
        }
        (false, 32) => {
            i32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as f32 / 2_147_483_648.0
        }
        _ => 0.0,
    }
}
