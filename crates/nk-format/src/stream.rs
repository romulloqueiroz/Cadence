//! Bounded, seekable WAV/NCW sample decoding. All methods perform file I/O and
//! allocation and belong on a loader/disk thread, never an audio callback.
use crate::{audio, ncw, wav, Error};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    sync::Arc,
};

pub const MAX_READ_FRAMES: usize = 1_048_576;

/// A complete file or a byte range inside a monolith/resource file.
#[derive(Debug, Clone)]
pub struct SampleLocation {
    pub path: PathBuf,
    pub offset: u64,
    pub len: u64,
}
impl SampleLocation {
    pub fn file(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let len = std::fs::metadata(&path)?.len();
        Ok(Self {
            path,
            offset: 0,
            len,
        })
    }
    /// Legacy whole-sample read, deliberately opt-in.
    pub fn read_all(&self) -> Result<Vec<u8>, Error> {
        let mut source = SliceFile::open(self)?;
        source.bytes(0, self.len)
    }
}

/// Metadata describes the first two channels (or the one mono channel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SampleInfo {
    pub channels: usize,
    pub sample_rate: u32,
    pub num_frames: usize,
}

pub struct SampleReader {
    source: SliceFile,
    info: SampleInfo,
    encoding: Encoding,
}
enum Encoding {
    Wav {
        format: wav::Format,
        data: u64,
        frame_size: usize,
    },
    Ncw {
        channels: usize,
        bits: u16,
        table: u64,
        data: u64,
        data_len: u64,
    },
}
impl SampleReader {
    pub fn open(location: SampleLocation) -> Result<Self, Error> {
        Self::open_with_pad(location, None)
    }

    /// Open a sample that may be XOR-encrypted with a 64 KiB retail pad.
    pub fn open_with_pad(location: SampleLocation, pad: Option<Arc<[u8]>>) -> Result<Self, Error> {
        let mut source = SliceFile::open_with_pad(&location, pad)?;
        let head = source.bytes(0, 4)?;
        let (info, encoding) = if head == b"RIFF" {
            open_wav(&mut source)?
        } else if u32::from_le_bytes(head.try_into().unwrap()) == ncw::FILE_MAGIC {
            open_ncw(&mut source)?
        } else {
            return Err(bad("unrecognised sample format (expected .ncw or .wav)"));
        };
        // Keep metadata, not one open descriptor per library sample. A range
        // read opens once and releases the handle before returning.
        source.release();
        Ok(Self {
            source,
            info,
            encoding,
        })
    }
    pub fn metadata(&self) -> SampleInfo {
        self.info
    }

    /// Return at most `count` frames, truncated at EOF. The allocation is bounded
    /// by the requested range; at most one NCW block or 256 WAV frames is scratch.
    pub fn read_frames(&mut self, start: usize, count: usize) -> Result<audio::Sample, Error> {
        let result = self.read_frames_inner(start, count);
        self.source.release();
        result
    }

    fn read_frames_inner(&mut self, start: usize, count: usize) -> Result<audio::Sample, Error> {
        if count > MAX_READ_FRAMES {
            return Err(bad("sample read exceeds frame limit"));
        }
        let count = count.min(self.info.num_frames.saturating_sub(start));
        let mut out = audio::Sample {
            channels: self.info.channels,
            sample_rate: self.info.sample_rate,
            num_frames: count,
            samples: vec![vec![0.0; count]; self.info.channels],
        };
        if count == 0 {
            return Ok(out);
        }
        match self.encoding {
            Encoding::Wav {
                format,
                data,
                frame_size,
            } => {
                let bytes_per_sample = usize::from(format.bits_per_sample).div_ceil(8);
                let mut done = 0;
                while done < count {
                    let frames = (count - done).min(256);
                    let bytes = self.source.bytes(
                        data + ((start + done) as u64 * frame_size as u64),
                        (frames * frame_size) as u64,
                    )?;
                    for (i, frame) in bytes.chunks_exact(frame_size).enumerate() {
                        for channel in 0..out.channels {
                            let offset = channel * bytes_per_sample;
                            out.samples[channel][done + i] = wav::decode_sample(
                                format,
                                &frame[offset..offset + bytes_per_sample],
                            );
                        }
                    }
                    done += frames;
                }
            }
            Encoding::Ncw {
                channels,
                bits,
                table,
                data,
                data_len,
            } => {
                let mut done = 0;
                while done < count {
                    let frame = start + done;
                    let block = frame / 512;
                    let pair = self.source.bytes(table + block as u64 * 4, 8)?;
                    let first = u32::from_le_bytes(pair[..4].try_into().unwrap()) as u64;
                    let last = u32::from_le_bytes(pair[4..].try_into().unwrap()) as u64;
                    let max_block = channels as u64 * (16 + 512 * 4);
                    if last > data_len
                        || last < first
                        || last - first > max_block
                        || last - first < channels as u64 * 16
                    {
                        return Err(bad("invalid NCW block range"));
                    }
                    let bytes = self.source.bytes(
                        data.checked_add(first)
                            .ok_or_else(|| bad("NCW block overflow"))?,
                        last - first,
                    )?;
                    let block_frames = (self.info.num_frames - block * 512).min(512);
                    let decoded = ncw::decode_block(&bytes, channels, bits, block_frames)?;
                    let offset = frame % 512;
                    let frames = (block_frames - offset).min(count - done);
                    for (destination, source) in out.samples.iter_mut().zip(decoded) {
                        destination[done..done + frames]
                            .copy_from_slice(&source[offset..offset + frames]);
                    }
                    done += frames;
                }
            }
        }
        Ok(out)
    }
}

fn open_wav(source: &mut SliceFile) -> Result<(SampleInfo, Encoding), Error> {
    if source.bytes(8, 4)? != b"WAVE" {
        return Err(bad("not a RIFF/WAVE file"));
    }
    let mut pos = 12;
    let mut format = None;
    let mut data = None;
    while pos + 8 <= source.len {
        let header = source.bytes(pos, 8)?;
        let size = u32::from_le_bytes(header[4..].try_into().unwrap()) as u64;
        let body = pos + 8;
        source.check(body, size)?;
        match &header[..4] {
            b"fmt " => {
                if size < 16 {
                    return Err(bad("short WAV fmt chunk"));
                }
                format = Some(wav::parse_fmt(&source.bytes(body, size.min(40))?)?);
            }
            b"data" => data = Some((body, size)),
            _ => {}
        }
        pos = body
            .checked_add(size)
            .and_then(|n| n.checked_add(size & 1))
            .ok_or_else(|| bad("WAV chunk overflow"))?;
    }
    let format = format.ok_or_else(|| bad("WAV has no fmt chunk"))?;
    let (data, len) = data.ok_or_else(|| bad("WAV has no data chunk"))?;
    if format.channels == 0
        || format.channels > 64
        || format.sample_rate == 0
        || format.sample_rate > 768_000
    {
        return Err(bad("implausible WAV channel count/sample rate"));
    }
    if !matches!(
        (format.format_tag, format.bits_per_sample),
        (1, 8 | 16 | 24 | 32) | (3, 32 | 64)
    ) {
        return Err(bad("unsupported WAV encoding"));
    }
    let frame_size = usize::from(format.block_align)
        .max(usize::from(format.bits_per_sample).div_ceil(8) * usize::from(format.channels));
    let frames =
        usize::try_from(len / frame_size as u64).map_err(|_| bad("WAV frame count overflow"))?;
    Ok((
        SampleInfo {
            channels: usize::from(format.channels).min(2),
            sample_rate: format.sample_rate,
            num_frames: frames,
        },
        Encoding::Wav {
            format,
            data,
            frame_size,
        },
    ))
}

fn open_ncw(source: &mut SliceFile) -> Result<(SampleInfo, Encoding), Error> {
    let header = source.bytes(0, 120)?;
    let u16_at = |p| u16::from_le_bytes(header[p..p + 2].try_into().unwrap());
    let u32_at = |p| u32::from_le_bytes(header[p..p + 4].try_into().unwrap());
    let version = u32_at(4);
    let channels = usize::from(u16_at(8));
    let bits = u16_at(10);
    let rate = u32_at(12);
    let frames = u32_at(16) as usize;
    let table = u64::from(u32_at(20));
    let data = u64::from(u32_at(24));
    if !matches!(version, 0x130 | 0x131)
        || channels == 0
        || channels > 32
        || !matches!(bits, 16 | 24 | 32)
        || rate == 0
        || rate > 768_000
    {
        return Err(bad("unsupported NCW header"));
    }
    let entries = frames.div_ceil(512) as u64 + 1;
    if table < 120 || data < table || data > source.len || data - table != entries * 4 {
        return Err(bad("invalid NCW block table size"));
    }
    let first = source.bytes(table, 4)?;
    if first != [0; 4] {
        return Err(bad("NCW first block offset is not 0"));
    }
    let last = source.bytes(data - 4, 4)?;
    let payload = u64::from(u32::from_le_bytes(last.try_into().unwrap()));
    source.check(data, payload)?;
    Ok((
        SampleInfo {
            channels: channels.min(2),
            sample_rate: rate,
            num_frames: frames,
        },
        Encoding::Ncw {
            channels,
            bits,
            table,
            data,
            data_len: payload,
        },
    ))
}

pub(crate) fn bad(message: &str) -> Error {
    Error::Malformed(message.into())
}

pub(crate) struct SliceFile {
    file: Option<File>,
    path: PathBuf,
    offset: u64,
    pub(crate) len: u64,
    pad: Option<Arc<[u8]>>,
}
impl SliceFile {
    pub(crate) fn open(location: &SampleLocation) -> Result<Self, Error> {
        Self::open_with_pad(location, None)
    }

    pub(crate) fn open_with_pad(
        location: &SampleLocation,
        pad: Option<Arc<[u8]>>,
    ) -> Result<Self, Error> {
        let file = File::open(&location.path)?;
        let size = file.metadata()?.len();
        if location
            .offset
            .checked_add(location.len)
            .filter(|end| *end <= size)
            .is_none()
        {
            return Err(bad("sample location outside file"));
        }
        Ok(Self {
            file: Some(file),
            path: location.path.clone(),
            offset: location.offset,
            len: location.len,
            pad,
        })
    }
    fn release(&mut self) {
        self.file = None;
    }
    pub(crate) fn check(&self, position: u64, size: u64) -> Result<(), Error> {
        if position
            .checked_add(size)
            .filter(|end| *end <= self.len)
            .is_none()
        {
            return Err(bad("sample range outside file slice"));
        }
        Ok(())
    }
    pub(crate) fn bytes(&mut self, position: u64, size: u64) -> Result<Vec<u8>, Error> {
        self.check(position, size)?;
        let size = usize::try_from(size).map_err(|_| bad("read size overflow"))?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(size)
            .map_err(|_| bad("sample read allocation failed"))?;
        bytes.resize(size, 0);
        if self.file.is_none() {
            let file = File::open(&self.path)?;
            let size = file.metadata()?.len();
            if self
                .offset
                .checked_add(self.len)
                .filter(|end| *end <= size)
                .is_none()
            {
                return Err(bad("sample file truncated since indexing"));
            }
            self.file = Some(file);
        }
        let file = self.file.as_mut().unwrap();
        file.seek(SeekFrom::Start(self.offset + position))?;
        file.read_exact(&mut bytes)?;
        if let Some(pad) = &self.pad {
            if !pad.is_empty() {
                for (index, byte) in bytes.iter_mut().enumerate() {
                    *byte ^= pad[(position as usize + index) % pad.len()];
                }
            }
        }
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn indexed_readers_release_file_handles_after_metadata_reads_and_errors() {
        let path =
            std::env::temp_dir().join(format!("cadence-reader-handles-{}", std::process::id()));
        let mut wav = b"RIFF\x26\0\0\0WAVEfmt \x10\0\0\0\x01\0\x01\0\x80\xbb\0\0\0\x77\x01\0\x02\0\x10\0data\x02\0\0\0".to_vec();
        wav.extend_from_slice(&1000i16.to_le_bytes());
        std::fs::write(&path, wav).unwrap();
        let mut readers: Vec<_> = (0..1024)
            .map(|_| SampleReader::open(SampleLocation::file(&path).unwrap()).unwrap())
            .collect();
        assert!(readers.iter().all(|reader| reader.source.file.is_none()));
        for reader in &mut readers {
            assert_eq!(reader.read_frames(0, 1).unwrap().num_frames, 1);
            assert!(reader.source.file.is_none());
        }
        std::fs::remove_file(&path).unwrap();
        assert!(readers[0].read_frames(0, 1).is_err());
        assert!(readers[0].source.file.is_none());
    }
}
