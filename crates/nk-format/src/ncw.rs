//! Decoder for Kontakt's `.ncw` lossless-compressed samples.
//!
//! Format recovered from ConvertWithMoss `NcwFile.java`:
//! 32-byte header, 88 bytes padding, an offset table, then blocks of 512
//! frames. Each block holds one chunk per channel; a chunk is a 16-byte header
//! plus bit-packed data (delta, truncated, or raw).

use crate::Error;

pub const FILE_MAGIC: u32 = 0xD69E_A801;
const BLOCK_MAGIC: u32 = 0x3E9A_0C16;
const BLOCK_FRAMES: usize = 512;

const FLAG_MID_SIDE: u16 = 1;
const FLAG_IEEE_FLOAT: u16 = 2;

#[derive(Debug, Clone)]
pub struct Ncw {
    pub channels: usize,
    pub bits_per_sample: u16,
    pub sample_rate: u32,
    pub num_frames: usize,
    /// De-interleaved, normalized to `-1.0..=1.0` unless the source is float.
    pub samples: Vec<Vec<f32>>,
}

pub fn parse(bytes: &[u8]) -> Result<Ncw, Error> {
    let mut r = Reader::new(bytes);

    let magic = r.u32()?;
    if magic != FILE_MAGIC {
        return Err(Error::Malformed(format!(
            "not an NCW file (magic 0x{magic:08X})"
        )));
    }
    let version = r.u32()?;
    if version != 0x130 && version != 0x131 {
        return Err(Error::Malformed(format!(
            "unsupported NCW version 0x{version:03X}"
        )));
    }

    let channels = r.u16()? as usize;
    let bits_per_sample = r.u16()?;
    let sample_rate = r.u32()?;
    let num_frames = r.u32()? as usize;
    let offset_address = r.u32()? as usize;
    let offset_data = r.u32()? as usize;
    let _size_of_blocks = r.u32()?;
    r.skip(88)?;

    if channels == 0 || channels > 32 {
        return Err(Error::Malformed(format!(
            "implausible channel count {channels}"
        )));
    }
    if !matches!(bits_per_sample, 16 | 24 | 32) {
        return Err(Error::Malformed(format!(
            "unsupported NCW bit depth {bits_per_sample}"
        )));
    }
    if offset_address < r.pos || offset_data < offset_address || offset_data > bytes.len() {
        return Err(Error::Malformed("invalid NCW block offsets".into()));
    }

    let num_blocks = (offset_data - offset_address) / 4;
    if num_blocks == 0 || num_blocks > 10_000_000 {
        return Err(Error::Malformed(format!(
            "implausible NCW block count {num_blocks}"
        )));
    }

    r.seek(offset_address)?;
    let mut offsets = Vec::with_capacity(num_blocks);
    for _ in 0..num_blocks {
        offsets.push(r.u32()? as usize);
    }
    if offsets[0] != 0 {
        return Err(Error::Malformed("NCW first block offset is not 0".into()));
    }

    let block_data = &bytes[offset_data..];
    let mut ints = vec![vec![0i32; num_frames]; channels];
    let mut is_float = false;

    for block in 0..num_blocks - 1 {
        let start = offsets[block];
        let end = offsets[block + 1];
        if end < start || end > block_data.len() {
            return Err(Error::Malformed(format!(
                "NCW block {block} out of range ({start}..{end})"
            )));
        }
        let decoded = parse_block(
            &block_data[start..end],
            block,
            channels,
            bits_per_sample,
            num_frames,
            &mut ints,
        )?;
        is_float |= decoded;
    }

    let scale = 1.0f32 / (1i64 << (bits_per_sample as i64 - 1)) as f32;
    let samples = if is_float {
        ints.into_iter()
            .map(|channel| {
                channel
                    .into_iter()
                    .map(|v| f32::from_bits(v as u32))
                    .collect()
            })
            .collect()
    } else {
        ints.into_iter()
            .map(|channel| {
                channel
                    .into_iter()
                    .map(|v| v as f32 * scale)
                    .collect::<Vec<f32>>()
            })
            .collect()
    };

    Ok(Ncw {
        channels,
        bits_per_sample,
        sample_rate,
        num_frames,
        samples,
    })
}

fn parse_block(
    block: &[u8],
    block_index: usize,
    channels: usize,
    bits_per_sample: u16,
    num_frames: usize,
    out: &mut [Vec<i32>],
) -> Result<bool, Error> {
    let mut r = Reader::new(block);
    let offset = block_index * BLOCK_FRAMES;
    let length = BLOCK_FRAMES.min(num_frames.saturating_sub(offset));
    if length == 0 {
        return Ok(false);
    }

    let mut decoded = Vec::with_capacity(channels);
    let mut is_float = false;
    let mut is_mid_side = false;

    for _ in 0..channels {
        let magic = r.u32()?;
        if magic != BLOCK_MAGIC {
            return Err(Error::Malformed(format!("NCW block magic 0x{magic:08X}")));
        }
        let base = r.i32()?;
        let bits = r.i16()?;
        let flags = r.u16()?;
        if flags > 3 {
            return Err(Error::Malformed(format!("unsupported NCW flags {flags}")));
        }
        is_float |= flags & FLAG_IEEE_FLOAT != 0;
        is_mid_side |= flags & FLAG_MID_SIDE != 0;
        r.skip(4)?;

        let precision = bits.unsigned_abs() as usize;
        if precision > 32 {
            return Err(Error::Malformed("NCW precision exceeds 32 bits".into()));
        }
        let data_len = if precision == 0 {
            BLOCK_FRAMES * (bits_per_sample as usize / 8)
        } else {
            BLOCK_FRAMES * precision / 8
        };
        let data = r.take(data_len)?;

        let samples = if bits > 0 {
            decode_delta(base, data, precision)
        } else if bits < 0 {
            decode_truncated(data, precision)
        } else {
            decode_raw(data, bits_per_sample)
        };
        decoded.push(samples);
    }

    if is_mid_side && channels == 2 {
        let (first, rest) = decoded.split_at_mut(1);
        for (a, b) in first[0].iter_mut().zip(rest[0].iter_mut()).take(length) {
            let mid = *a;
            let side = *b;
            *a = mid.wrapping_add(side);
            *b = mid.wrapping_sub(side);
        }
    }

    for (channel, samples) in decoded.iter().enumerate() {
        out[channel][offset..offset + length].copy_from_slice(&samples[..length]);
    }

    Ok(is_float)
}

fn decode_delta(base: i32, data: &[u8], precision: usize) -> Vec<i32> {
    let deltas = read_packed_signed(data, precision);
    let mut samples = vec![0i32; BLOCK_FRAMES];
    let mut previous = base;
    for (i, delta) in deltas.iter().take(BLOCK_FRAMES).enumerate() {
        samples[i] = previous;
        previous = previous.wrapping_add(*delta);
    }
    samples
}

fn decode_truncated(data: &[u8], precision: usize) -> Vec<i32> {
    read_packed_signed(data, precision)
}

fn decode_raw(data: &[u8], bits_per_sample: u16) -> Vec<i32> {
    let bytes_per_sample = bits_per_sample as usize / 8;
    let mut samples = vec![0i32; BLOCK_FRAMES];
    for (i, sample) in samples.iter_mut().enumerate() {
        let start = i * bytes_per_sample;
        let Some(slice) = data.get(start..start + bytes_per_sample) else {
            break;
        };
        let mut value = 0i64;
        for (byte_index, byte) in slice.iter().enumerate() {
            value |= (*byte as i64) << (byte_index * 8);
        }
        let shift = 64 - bits_per_sample as i64;
        *sample = ((value << shift) >> shift) as i32;
    }
    samples
}

fn read_packed_signed(data: &[u8], precision: usize) -> Vec<i32> {
    let mut values = Vec::with_capacity(BLOCK_FRAMES);
    if precision == 0 || precision > 32 {
        return vec![0; BLOCK_FRAMES];
    }

    let mut accumulator: u64 = 0;
    let mut bits_in_accumulator = 0usize;
    let mut byte_index = 0usize;
    let mask = (1u64 << precision) - 1;

    while values.len() < BLOCK_FRAMES {
        if bits_in_accumulator < precision {
            let Some(byte) = data.get(byte_index) else {
                break;
            };
            accumulator |= (*byte as u64) << bits_in_accumulator;
            bits_in_accumulator += 8;
            byte_index += 1;
            continue;
        }

        let mut value = (accumulator & mask) as i64;
        if value & (1i64 << (precision - 1)) != 0 {
            value |= !0i64 << precision;
        }
        values.push(value as i32);
        accumulator >>= precision;
        bits_in_accumulator -= precision;
    }

    values.resize(BLOCK_FRAMES, 0);
    values
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

    fn seek(&mut self, pos: usize) -> Result<(), Error> {
        if pos > self.data.len() {
            return Err(Error::Truncated {
                needed: pos,
                len: self.data.len(),
            });
        }
        self.pos = pos;
        Ok(())
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

    fn i32(&mut self) -> Result<i32, Error> {
        self.u32().map(|v| v as i32)
    }
}

/// Decode one independently addressable block, retaining bounded scratch space.
pub(crate) fn decode_block(
    block: &[u8],
    channels: usize,
    bits: u16,
    frames: usize,
) -> Result<Vec<Vec<f32>>, Error> {
    let mut ints = vec![vec![0i32; frames]; channels];
    let float = parse_block(block, 0, channels, bits, frames, &mut ints)?;
    let scale = 1.0 / (1i64 << (bits - 1)) as f32;
    Ok(ints
        .into_iter()
        .take(2)
        .map(|channel| {
            channel
                .into_iter()
                .map(|v| {
                    if float {
                        f32::from_bits(v as u32)
                    } else {
                        v as f32 * scale
                    }
                })
                .collect()
        })
        .collect())
}
