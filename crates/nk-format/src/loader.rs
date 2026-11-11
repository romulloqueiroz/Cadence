//! High-level instrument loader: parse a container, build the playable model,
//! and resolve sample bytes from disk, embedded monolith data, or `.nkr`.

use crate::{
    audio, encryption, file_container, locations, model, ni_container, preset_chunk, probe,
    stream::SampleLocation, Error, Kind,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub struct LoadedInstrument {
    pub instrument: model::Instrument,
    pub path: PathBuf,
    embedded: Option<HashMap<String, SampleLocation>>,
    nkr: Option<HashMap<String, SampleLocation>>,
    nkx: HashMap<PathBuf, HashMap<String, SampleLocation>>,
    retail: Option<encryption::ProductKey>,
}

impl LoadedInstrument {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        use std::io::Read;
        let mut file = std::fs::File::open(&path)?;
        let mut head = [0; 16];
        file.read_exact(&mut head)?;

        // `probe` validates an NI item's declared length against the supplied
        // bytes. With a header-only read, validate that field against the file.
        let declared = u64::from_le_bytes(head[..8].try_into().unwrap());
        let kind = if head[8..12] == [1, 0, 0, 0]
            && &head[12..16] == b"hsin"
            && declared >= 8
            && declared <= file.metadata()?.len()
        {
            Kind::Container
        } else {
            probe(&head)
        };
        match kind {
            Kind::Container => {
                let bytes = read_instrument(&SampleLocation::file(&path)?)?;
                let (instrument, retail) = build_from_container(&bytes, &path)?;
                Ok(LoadedInstrument {
                    instrument,
                    path,
                    embedded: None,
                    nkr: None,
                    nkx: HashMap::new(),
                    retail,
                })
            }
            Kind::FileContainer => {
                let files = locations::file_container(&path)?;
                let (_, main) = files
                    .iter()
                    .find(|(name, _)| {
                        let name = name.to_ascii_lowercase();
                        name.ends_with(".nki") || name.ends_with(".nkm")
                    })
                    .ok_or_else(|| Error::Malformed("file container has no .nki/.nkm".into()))?;
                let bytes = read_instrument(main)?;
                let (instrument, retail) = build_from_container(&bytes, &path)?;
                let embedded = files
                    .into_iter()
                    .map(|(name, location)| (file_container::basename_lower(&name), location))
                    .collect();
                Ok(LoadedInstrument {
                    instrument,
                    path,
                    embedded: Some(embedded),
                    nkr: None,
                    nkx: HashMap::new(),
                    retail,
                })
            }
            Kind::Plain => {
                let bytes = read_instrument(&SampleLocation::file(&path)?)?;
                let (instrument, retail) = build_from_container(&bytes, &path)?;
                Ok(LoadedInstrument {
                    instrument,
                    path,
                    embedded: None,
                    nkr: None,
                    nkx: HashMap::new(),
                    retail,
                })
            }
            _ => Err(Error::Malformed(
                "unsupported instrument container (expected NI container or file container)".into(),
            )),
        }
    }

    /// Resolve a seekable file range without reading the sample payload.
    pub fn sample_location(&mut self, sample_ref: &str) -> Result<SampleLocation, Error> {
        let key = file_container::basename_lower(sample_ref);
        if let Some(embedded) = &self.embedded {
            return embedded
                .get(&key)
                .cloned()
                .ok_or_else(|| Error::Malformed(format!("embedded sample {key:?} not found")));
        }
        // Retail `.nkx` references concatenate the monolith path and the entry
        // name, for example `../Samples/GroveBass_0.nkxSamples/Bass_01.ncw`.
        if let Some((monolith_ref, entry)) = split_nkx(sample_ref) {
            if let Some(monolith) = resolve_sample_path(&self.path, monolith_ref) {
                if !self.nkx.contains_key(&monolith) {
                    let samples = locations::nkx(&monolith).unwrap_or_default();
                    let map = samples
                        .into_iter()
                        .map(|(name, location)| (file_container::basename_lower(&name), location))
                        .collect();
                    self.nkx.insert(monolith.clone(), map);
                }
                if let Some(location) = self
                    .nkx
                    .get(&monolith)
                    .and_then(|map| map.get(&file_container::basename_lower(entry)))
                {
                    return Ok(location.clone());
                }
            }
        }
        if let Some(path) = resolve_sample_path(&self.path, sample_ref) {
            if let Ok(location) = SampleLocation::file(path) {
                return Ok(location);
            }
        }
        if self.nkr.is_none() {
            self.nkr = Some(build_nkr_map(&self.path));
        }
        self.nkr
            .as_ref()
            .and_then(|map| map.get(&key))
            .cloned()
            .ok_or_else(|| {
                Error::Malformed(format!(
                    "sample {key:?} not found on disk or in .nkr/.nkx resources"
                ))
            })
    }

    /// Resolve and return the complete raw sample bytes (legacy opt-in API).
    pub fn sample(&mut self, sample_ref: &str) -> Result<Vec<u8>, Error> {
        let location = self.sample_location(sample_ref)?;
        let mut bytes = location.read_all()?;
        if let Some(key) = &self.retail {
            // Encrypted samples are XORed from offset 0 of the sample payload.
            // Leave already-plain NCW/WAV payloads untouched.
            if !looks_like_audio(&bytes) {
                bytes = key.xor(&bytes);
            }
        }
        Ok(bytes)
    }

    /// Decode a sample reference to the normalized `audio::Sample` form.
    pub fn decoded_sample(&mut self, sample_ref: &str) -> Result<audio::Sample, Error> {
        let bytes = self.sample(sample_ref)?;
        audio::load(&bytes)
    }

    pub fn retail_key(&self) -> Option<&encryption::ProductKey> {
        self.retail.as_ref()
    }
}

fn split_nkx(reference: &str) -> Option<(&str, &str)> {
    let lower = reference.to_ascii_lowercase();
    let index = lower.find(".nkx")?;
    let end = index + 4;
    let entry = &reference[end..];
    (!entry.is_empty()).then_some((&reference[..end], entry))
}

fn looks_like_audio(bytes: &[u8]) -> bool {
    bytes.starts_with(b"RIFF")
        || bytes.get(..4).is_some_and(|magic| {
            u32::from_le_bytes(magic.try_into().unwrap()) == crate::ncw::FILE_MAGIC
        })
}

fn build_from_container(
    bytes: &[u8],
    path: &Path,
) -> Result<(model::Instrument, Option<encryption::ProductKey>), Error> {
    if probe(bytes) == Kind::Plain {
        let container = crate::Container::parse(bytes)?;
        return Ok((crate::plain_model::build(&container.root)?, None));
    }
    let item = ni_container::parse(bytes)?;
    let mut retail = None;
    let mut preset = preset_data(&item);
    if preset.is_none() {
        let pids = authorization_pids(&item);
        if let Some(key) = encryption::find_key(path, &pids)? {
            let decrypted = ni_container::parse_with_pad(bytes, Some(key.pad()))?;
            preset = preset_data(&decrypted);
            if preset.is_some() {
                retail = Some(key);
            }
        }
    }
    let preset = preset.ok_or_else(|| {
        if item.is_encrypted() {
            Error::Malformed(
                "preset subtree is encrypted or uses unsupported compression; \
                 this library cannot be decoded without the original key"
                    .into(),
            )
        } else {
            Error::Malformed("no preset chunk found".into())
        }
    })?;
    let chunks = preset_chunk::parse_all(&preset)?;
    Ok((model::build(&chunks)?, retail))
}

fn preset_data(item: &ni_container::Item) -> Option<Vec<u8>> {
    item.find_chunk(ni_container::CHUNK_PRESET_CHUNK_ITEM)
        .and_then(|chunk| match &chunk.data {
            ni_container::ChunkData::Preset { preset_data, .. } => Some(preset_data.clone()),
            _ => None,
        })
}

fn authorization_pids(item: &ni_container::Item) -> Vec<String> {
    let mut chunks = Vec::new();
    item.find_all_chunks(ni_container::CHUNK_AUTHORIZATION, &mut chunks);
    chunks
        .into_iter()
        .flat_map(|chunk| match &chunk.data {
            ni_container::ChunkData::Authorization { pids, .. } => pids.clone(),
            _ => Vec::new(),
        })
        .collect()
}

fn resolve_sample_path(nki: &Path, sample_ref: &str) -> Option<PathBuf> {
    let normalized = sample_ref.replace('\\', "/");
    let sample_ref = normalized.as_str();
    let direct = PathBuf::from(sample_ref);
    if direct.is_absolute() && direct.exists() {
        return Some(direct);
    }

    if let Some(index) = sample_ref.find("Content/") {
        if let Some(content_root) = ancestor_named(nki, "Content") {
            let candidate = content_root.join(&sample_ref[index + "Content/".len()..]);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    if let Some(parent) = nki.parent() {
        let candidate = parent.join(sample_ref);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    None
}

fn ancestor_named(path: &Path, name: &str) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if ancestor.file_name().and_then(|n| n.to_str()) == Some(name) {
            return Some(ancestor.to_path_buf());
        }
    }
    None
}

fn build_nkr_map(nki: &Path) -> HashMap<String, SampleLocation> {
    let mut map = HashMap::new();
    let Some(root) = nki.parent().and_then(Path::parent) else {
        return map;
    };
    visit_nkr(root, 0, &mut map);
    map
}

fn visit_nkr(dir: &Path, depth: usize, map: &mut HashMap<String, SampleLocation>) {
    if depth > 4 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit_nkr(&path, depth + 1, map);
        } else if path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("nkr"))
        {
            if let Ok(samples) = locations::nkr(&path) {
                for (name, location) in samples {
                    map.insert(file_container::basename_lower(&name), location);
                }
            }
        }
    }
}

fn read_instrument(location: &SampleLocation) -> Result<Vec<u8>, Error> {
    const MAX_INSTRUMENT_BYTES: u64 = 128 * 1024 * 1024;
    if location.len > MAX_INSTRUMENT_BYTES {
        return Err(Error::Malformed(
            "instrument metadata exceeds 128 MiB".into(),
        ));
    }
    location.read_all()
}
