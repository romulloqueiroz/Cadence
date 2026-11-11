//! Program and group metadata, assembled into a playable instrument model.
//!
//! Layouts recovered from ConvertWithMoss `Program.java` and `Group.java`.
//! The first program in a preset is used; multi-program instruments are not
//! yet modelled.

pub use crate::start_criteria::{CriteriaLogic, StartCondition, StartCriterion};
use crate::{
    fx::FxChain, modulator, preset_chunk, preset_chunk::PresetChunk, start_criteria, zone, Error,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Cc64Mode {
    #[default]
    SustainAndController,
    SustainOnly,
    ControllerOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum KillMode {
    Any,
    #[default]
    Oldest,
    Newest,
    Highest,
    Lowest,
}

#[derive(Debug, Clone, PartialEq)]
pub struct VoiceGroup {
    pub name: String,
    pub kill_mode: KillMode,
    pub prefer_released: bool,
    /// 0 means no explicit limit.
    pub max_voices: u32,
    pub fade_time_ms: u32,
    /// -1 means no exclusion group.
    pub exclusion_group: i32,
}

impl Default for VoiceGroup {
    fn default() -> Self {
        Self {
            name: String::new(),
            kill_mode: KillMode::Oldest,
            prefer_released: true,
            max_voices: 0,
            fade_time_ms: 10,
            exclusion_group: -1,
        }
    }
}

impl VoiceGroup {
    fn parse(r: &mut Reader<'_>) -> Result<Self, Error> {
        let version = r.u16()?;
        if version != 0x60 {
            return Err(Error::Malformed(format!(
                "unsupported voice group version {version:#x}"
            )));
        }
        let name = r.utf16()?;
        let kill_mode = match r.i16()? {
            0 => KillMode::Any,
            1 => KillMode::Oldest,
            2 => KillMode::Newest,
            3 => KillMode::Highest,
            4 => KillMode::Lowest,
            _ => return Err(Error::Malformed("invalid voice group kill mode".into())),
        };
        let prefer_released = r.u8()? != 0;
        let max_voices = r.i32()?.clamp(0, 1_000_000) as u32;
        let fade_time_ms = r.i32()?.clamp(0, 60_000) as u32;
        let exclusion_group = r.i32()?;
        Ok(Self {
            name,
            kill_mode,
            prefer_released,
            max_voices,
            fade_time_ms,
            exclusion_group,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct VoiceGroups {
    pub default: VoiceGroup,
    /// Explicit per-index overrides from the 128-bit presence mask.
    pub overrides: Vec<(usize, VoiceGroup)>,
}

impl VoiceGroups {
    pub fn get(&self, index: i32) -> &VoiceGroup {
        if index >= 0 {
            if let Some((_, group)) = self
                .overrides
                .iter()
                .find(|(slot, _)| *slot == index as usize)
            {
                return group;
            }
        }
        &self.default
    }

    fn parse(data: &[u8]) -> Result<Self, Error> {
        let mut r = Reader::new(data);
        let default = VoiceGroup::parse(&mut r)?;
        let mask = r.take(16)?;
        let mut overrides = Vec::new();
        for index in 0..128 {
            if mask[index / 8] >> (index % 8) & 1 != 0 {
                // Override BVoiceLimit objects are two-byte aligned; a pad
                // byte follows an odd-length predecessor (observed in the
                // factory Piano Uno preset).
                if r.pos % 2 == 1 {
                    r.u8()?;
                }
                overrides.push((index, VoiceGroup::parse(&mut r)?));
            }
        }
        Ok(Self { default, overrides })
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProgramInfo {
    pub name: String,
    pub midi_transpose: i32,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub key_range: (u16, u16),
    pub velocity_range: (u16, u16),
    /// None selects the lowest configured key-switch key at initialization.
    pub default_key_switch: Option<u8>,
    pub cc64_mode: Cc64Mode,
    pub voice_groups: VoiceGroups,
}

impl ProgramInfo {
    pub fn parse(data: &[u8]) -> Result<ProgramInfo, Error> {
        let mut r = Reader::new(data);
        let name = r.utf16()?;
        let _size_of_all_samples = r.f64()?;
        let raw = r.u8()? as i32;
        let midi_transpose = if raw > 128 { raw - 256 } else { raw };
        let volume = r.f32()?;
        let pan = r.f32()?;
        let tune = r.f32()?;
        let velocity_range = (r.u8()? as u16, r.u8()? as u16);
        let key_range = (r.u8()? as u16, r.u8()? as u16);
        for (low, high) in [velocity_range, key_range] {
            if low > high || high > 127 {
                return Err(Error::Malformed("invalid program MIDI range".into()));
            }
        }
        let default_key_switch = match r.i16()? {
            -1 => None,
            key @ 0..=127 => Some(key as u8),
            _ => return Err(Error::Malformed("invalid defaultKeyKeySwitch".into())),
        };
        Ok(ProgramInfo {
            name,
            midi_transpose,
            volume,
            pan,
            tune,
            key_range,
            velocity_range,
            default_key_switch,
            cc64_mode: Cc64Mode::default(),
            voice_groups: VoiceGroups::default(),
        })
    }

    pub fn parse_chunk(chunk: &PresetChunk) -> Result<Self, Error> {
        let mut program = Self::parse(&chunk.public_data)?;
        if let Some(groups) = chunk
            .children
            .iter()
            .find(|c| c.id == preset_chunk::ID_VOICE_GROUPS)
        {
            if !groups.public_data.is_empty() {
                program.voice_groups = VoiceGroups::parse(&groups.public_data)?;
            }
        }
        if !chunk.private_data.is_empty() {
            // BProgram::doReadPrivPars starts with this enum in each supported
            // version. The stored index differs from the runtime enum (0,1,3).
            if !matches!(chunk.version, Some(0x80 | 0x82 | 0x90..=0x92 | 0xa0..=0xb5)) {
                return Err(Error::Malformed(
                    "unsupported program private version".into(),
                ));
            }
            program.cc64_mode = match Reader::new(&chunk.private_data).i32()? {
                0 => Cc64Mode::SustainAndController,
                1 => Cc64Mode::SustainOnly,
                2 => Cc64Mode::ControllerOnly,
                _ => return Err(Error::Malformed("invalid program cc64Mode".into())),
            };
        }
        Ok(program)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FilterKind {
    Lowpass,
    Highpass,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Filter {
    pub kind: FilterKind,
    /// Hz.
    pub cutoff: f32,
    pub q: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Group {
    pub name: String,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub key_tracking: bool,
    pub reverse: bool,
    /// -1 (binary `All`) or 0 (XML `All`) = every channel; 1..=16 selects a
    /// one-based MIDI channel.
    pub midi_channel: i16,
    /// Voice-group slot (0..127); negative means the program default.
    pub voice_group_index: i32,
    /// 0 normal, 1 high, 2 ultra (from the group public data).
    pub interp_quality: i32,
    pub release_trigger: bool,
    /// Monophonic release-trigger flag from the group public data.
    pub release_trigger_monophonic: bool,
    /// Release-trigger counter/round-robin slot.
    pub release_trigger_counter: i32,
    pub muted: bool,
    pub soloed: bool,
    pub one_shot: bool,
    /// Binary group routing still uses legacy name heuristics. XML has an
    /// explicit releaseTrigger field and must not reinterpret display names.
    pub name_trigger_heuristics: bool,
    pub filter: Option<Filter>,
    /// Group-level insert FX chain from the group private data.
    pub fx_chain: Option<FxChain>,
    /// Amplifier placement in the eight group insert slots: slots before this
    /// boundary run before amplitude/pan, and remaining slots run afterward.
    /// Native `fxIdxAmpSplitPoint` defaults to 6; serialized range is 0..=8.
    pub fx_amp_split_point: i32,
    pub envelope: Option<modulator::Envelope>,
    /// Intensity of the amplitude envelope's volume target. Native stores this
    /// per target in the internal-modulator block 1.
    pub envelope_intensity: f32,
    /// All decoded internal modulators (envelopes and LFOs) for later stages.
    pub internal_modulators: Vec<modulator::InternalModulator>,
    pub start_criteria: Vec<StartCriterion>,
    pub external_modulators: Vec<crate::external_mod::ExternalModulator>,
}

impl Group {
    pub fn parse(data: &[u8]) -> Result<Group, Error> {
        let mut r = Reader::new(data);
        let name = r.utf16()?;
        let volume = r.f32()?;
        let pan = r.f32()?;
        let tune = r.f32()?;
        let key_tracking = r.u8()? > 0;
        let reverse = r.u8()? > 0;
        let release_trigger = r.u8()? > 0;
        let release_trigger_monophonic = r.u8()? > 0;
        let release_trigger_counter = r.i32()?;
        let midi_channel = r.i16()?;
        let voice_group_index = r.i32()?;
        let fx_amp_split_point = r.i32()?;
        if !(0..=8).contains(&fx_amp_split_point) {
            return Err(Error::Malformed(
                "group fxIdxAmpSplitPoint outside 0..=8".into(),
            ));
        }
        let muted = r.u8()? > 0;
        let soloed = r.u8()? > 0;
        let interp_quality = r.i32()?;
        Ok(Group {
            name,
            volume,
            pan,
            tune,
            key_tracking,
            reverse,
            midi_channel,
            voice_group_index,
            interp_quality,
            release_trigger,
            release_trigger_monophonic,
            release_trigger_counter,
            muted,
            soloed,
            one_shot: false,
            name_trigger_heuristics: true,
            filter: None,
            fx_chain: None,
            fx_amp_split_point,
            envelope: None,
            envelope_intensity: 1.0,
            internal_modulators: Vec::new(),
            start_criteria: Vec::new(),
            external_modulators: Vec::new(),
        })
    }

    pub fn parse_chunk(chunk: &PresetChunk) -> Result<Group, Error> {
        let mut group = Group::parse(&chunk.public_data)?;
        group.fx_chain = find_fx_chain(&chunk.private_data);
        let mut saw_criteria = false;
        for child in &chunk.children {
            if child.id == preset_chunk::ID_PARAMETER_ARRAY_32 {
                for modulation in &child.children {
                    if modulation.id == preset_chunk::ID_PAR_EXTERNAL_MOD {
                        if group.external_modulators.len() == crate::external_mod::MAX_MODULATORS {
                            return Err(Error::Malformed(
                                "external modulator count exceeds 64".into(),
                            ));
                        }
                        group
                            .external_modulators
                            .push(crate::external_mod::ExternalModulator::parse(modulation)?);
                    }
                }
                continue;
            }
            if child.id == start_criteria::ID_START_CRITERIA_ARRAY {
                if saw_criteria {
                    return Err(Error::Malformed("duplicate group start array".into()));
                }
                saw_criteria = true;
                group.start_criteria = start_criteria::decode_array(&child.public_data)?;
                continue;
            }
            if child.id != preset_chunk::ID_PARAMETER_ARRAY_16 {
                continue;
            }
            for mod_chunk in &child.children {
                if mod_chunk.id != preset_chunk::ID_PAR_INTERNAL_MOD
                    && mod_chunk.id != preset_chunk::ID_PAR_MOD_BASE
                {
                    continue;
                }
                if let Ok(m) =
                    modulator::InternalModulator::parse(mod_chunk.id, &mod_chunk.public_data)
                {
                    if m.targets_volume() && m.is_envelope() {
                        if let Some(envelope) = &m.envelope {
                            group.envelope = Some(envelope.clone());
                            group.envelope_intensity = m.intensity("volume").unwrap_or(1.0);
                        }
                    }
                    group.internal_modulators.push(m);
                }
            }
        }
        Ok(group)
    }
}

#[derive(Debug, Clone)]
pub struct Instrument {
    pub program: ProgramInfo,
    pub groups: Vec<Group>,
    /// Program-level `PARAMETER_ARRAY_8` FX chains in serialized order:
    /// inserts, parallel sends, then master (master is absent before v0xae).
    /// Empty chains retain their position; a malformed chain is a load error.
    pub fx_chains: Vec<FxChain>,
    pub zones: Vec<zone::Zone>,
    pub sample_files: Vec<String>,
}

impl Instrument {
    pub fn group(&self, index: u32) -> Option<&Group> {
        self.groups.get(index as usize)
    }

    /// Combined linear gain for a zone: program * group * zone volume.
    pub fn zone_gain(&self, zone: &zone::Zone) -> f32 {
        let group = self.group(zone.group_index).map_or(1.0, |g| g.volume);
        self.program.volume * group * zone.volume
    }

    /// Combined pan in `-1.0..=1.0`.
    pub fn zone_pan(&self, zone: &zone::Zone) -> f32 {
        let group = self.group(zone.group_index).map_or(0.0, |g| g.pan);
        (self.program.pan + group + zone.pan).clamp(-1.0, 1.0)
    }

    /// Combined tune ratio. The three stored fields default to 1.0 (unity),
    /// so they compose multiplicatively, not additively.
    pub fn zone_tune(&self, zone: &zone::Zone) -> f32 {
        let group = self.group(zone.group_index).map_or(1.0, |g| g.tune);
        self.program.tune * group * zone.tune
    }
}

/// Assemble the first program, its group list, zones and sample file list.
pub fn build(chunks: &[PresetChunk]) -> Result<Instrument, Error> {
    let chunks = preset_chunk::expanded(chunks);
    let program_chunk = find_chunk(&chunks, preset_chunk::ID_PROGRAM)
        .ok_or_else(|| Error::Malformed("no PROGRAM chunk found".into()))?;
    let program = ProgramInfo::parse_chunk(program_chunk)?;

    let groups = program_chunk
        .children
        .iter()
        .find(|c| c.id == preset_chunk::ID_GROUP_LIST)
        .map(|group_list| {
            group_list
                .children
                .iter()
                .map(Group::parse_chunk)
                .collect::<Result<Vec<_>, Error>>()
        })
        .transpose()?
        .unwrap_or_default();

    let zones = zone::decode_zones(&chunks);
    let sample_files = preset_chunk::summary(&chunks).sample_files;

    let fx_chains = program_chunk
        .children
        .iter()
        .filter(|child| child.id == preset_chunk::ID_PARAMETER_ARRAY_8)
        .map(|child| FxChain::parse(&child.public_data))
        .collect::<Result<Vec<_>, _>>()?;

    Ok(Instrument {
        program,
        groups,
        fx_chains,
        zones,
        sample_files,
    })
}

fn find_fx_chain(data: &[u8]) -> Option<FxChain> {
    if data.len() < 6 {
        return None;
    }
    for position in 0..=data.len() - 6 {
        if data[position] == 0x13
            && data[position + 1] == 0
            && data[position + 2] == 8
            && data[position + 3..position + 6] == [0, 0, 0]
        {
            if let Ok(chain) = FxChain::parse(&data[position..]) {
                if chain.slots.len() == 8 {
                    return Some(chain);
                }
            }
        }
    }
    None
}

fn find_chunk(chunks: &[PresetChunk], id: u32) -> Option<&PresetChunk> {
    for chunk in chunks {
        if chunk.id == id {
            return Some(chunk);
        }
        if let Some(found) = find_chunk(&chunk.children, id) {
            return Some(found);
        }
    }
    None
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

    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
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

    fn f32(&mut self) -> Result<f32, Error> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn f64(&mut self) -> Result<f64, Error> {
        let b = self.take(8)?;
        Ok(f64::from_le_bytes([
            b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7],
        ]))
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
