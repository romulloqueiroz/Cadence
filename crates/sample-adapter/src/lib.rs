//! Kontakt/sample-library adapter for the universal plugin protocol.
//!
//! This is the compatibility path: `nk-format` reads the library, and
//! `SamplerPlugin` presents it as a normal [`plugin_api::Plugin`] so it runs
//! through the same host, parameter, state and conformance machinery as any
//! future adapter.
//!
//! Production loading uses resident attacks plus a bounded background page
//! cache. Realtime processing never accesses files, waits, or allocates.

use nk_format::audio;
use nk_format::loader::LoadedInstrument;
use nk_format::model;
use nk_format::modulator::Envelope;
use nk_format::zone::Zone;
use plugin_api::provider::LoadError;
use plugin_api::{
    AudioBuffer, Capabilities, Descriptor, Event, EventKind, ParamFlags, ParameterInfo, Plugin,
    PluginKind, ProcessContext, State, StateError, UiNode, Unit,
};
mod streaming;
use nk_format::stream::{SampleInfo, SampleReader};
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::Path;
use std::sync::Arc;
use streaming::{PreloadPage, StreamEngine, PAGE_FRAMES};
mod acbox_dsp;
mod amp_dsp;
mod amp_resampler;
mod bassinvader_dsp;
mod basspro_amp;
mod cabinet_dsp;
mod cat_amp;
mod convolution;
mod curve;
mod epiano_amp;
mod epiano_dsp;
mod expression;
mod filter_dsp;
mod filter_extended;
mod fx_dsp;
mod group_amp;
mod group_fx;
mod group_start;
mod hotsolo_dsp;
mod jump_controls;
mod jump_dsp;
mod shaper_dsp;
mod van51_amp;
mod van51_dsp;
mod van51_process;
use group_fx::GroupFxStage;
mod internal_mod;
use expression::Modulation;
use fx_dsp::{AuxFrame, PostFx, AUX_CHUNK};
use group_start::GroupStart;
use internal_mod::InternalMod;
use nk_format::fx::FilterShape;
pub use streaming::{StreamingConfig, StreamingStats};

const VOICE_POOL: usize = 64;
// Includes unmapped/release-only attacks. Saturation retires the oldest
// lifetime and its voices; callback storage never grows.
const NOTE_POOL: usize = 16 * 128;

const GAIN: ParameterInfo = ParameterInfo {
    id: 1,
    name: "Gain",
    min: -60.0,
    max: 12.0,
    default: 0.0,
    unit: Unit::Decibels,
    flags: ParamFlags {
        stepped: false,
        read_only: false,
        bypass: false,
    },
};

#[derive(Clone, Copy)]
struct LoopState {
    start: usize,
    end: usize,
    alternating: bool,
    tuning: f64,
    crossfade: usize,
    /// None is infinite; Some(n) allows n further wraps.
    remaining: Option<u32>,
}

// Eight group slots, up to three biquads each, plus the legacy XML filter.
const MAX_FILTER_BIQUADS: usize = group_fx::GROUP_FX_SLOTS * 3 + 1;

#[derive(Clone, Copy, Default)]
struct FilterState {
    left: [f32; 2],
    right: [f32; 2],
    coeffs: [f32; 5],
    cached_octaves: f32,
    valid: bool,
}

struct Voice {
    sample: usize,
    stream_page: usize,
    last_output: (f32, f32),
    recovery: f32,
    position: f64,
    direction: f64,
    age: u64,
    release_age: Option<u64>,
    /// (elapsed frames, total frames) for a voice-group fade-out.
    choke_fade: Option<(u32, u32)>,
    note: u8,
    channel: u8,
    note_id: u64,
    velocity: u8,
    release_velocity: f32,
    group: usize,
    base_pan: f32,
    mod_octaves: f32,
    mod_pitch: f64,
    is_release: bool,
    one_shot: bool,
    loop_until_release: bool,
    gain: f32,
    pan_left: f32,
    pan_right: f32,
    speed: f64,
    start: usize,
    end: usize,
    loop_state: Option<LoopState>,
    release_loop: Option<LoopState>,
    filter_states: [FilterState; MAX_FILTER_BIQUADS],
    native_filter_states: [filter_dsp::NativeFilterState; group_fx::GROUP_FX_SLOTS],
    shaper_states: [shaper_dsp::ShaperState; group_fx::GROUP_FX_SLOTS],
    interp_quality: u8,
    envelope: Option<Envelope>,
    envelope_intensity: f32,
}

#[derive(Clone, Copy)]
struct NoteLifetime {
    id: u64,
    note: u8,
    velocity: u8,
    channel: u8,
    release_velocity: f32,
    sustained: bool,
}

#[derive(Clone, Copy)]
struct NoteVelocity {
    attack: u8,
    release: f32,
}

#[derive(Clone, Copy)]
struct VoicePolicy {
    group_index: i32,
    max_voices: u32,
    kill_mode: model::KillMode,
    prefer_released: bool,
    fade_time_ms: u32,
    exclusion_group: i32,
}

impl VoicePolicy {
    fn from_group(program: &model::ProgramInfo, group: &model::Group) -> Self {
        let voice_group = program.voice_groups.get(group.voice_group_index);
        Self {
            group_index: group.voice_group_index,
            max_voices: voice_group.max_voices,
            kill_mode: voice_group.kill_mode,
            prefer_released: voice_group.prefer_released,
            fade_time_ms: voice_group.fade_time_ms,
            exclusion_group: voice_group.exclusion_group,
        }
    }
}

enum SampleData {
    Resident(audio::Sample),
    Stream { id: usize, pages: Vec<PreloadPage> },
}

struct SampleAsset {
    info: SampleInfo,
    data: SampleData,
}

impl SampleAsset {
    fn resident_frame(&self, index: usize) -> Option<(f32, f32)> {
        match &self.data {
            SampleData::Resident(sample) => Some((
                sample.samples[0][index],
                sample.samples[sample.channels.min(2) - 1][index],
            )),
            SampleData::Stream { pages, .. } => {
                let start = index / PAGE_FRAMES * PAGE_FRAMES;
                let page = pages
                    .binary_search_by_key(&start, |p| p.start)
                    .ok()
                    .map(|i| &pages[i])?;
                let offset = (index - start) * 2;
                Some((page.data[offset], page.data[offset + 1]))
            }
        }
    }
}

pub struct SamplerPlugin {
    loaded: LoadedInstrument,
    zones: Vec<Zone>,
    groups: Vec<model::Group>,
    voice_policies: Vec<VoicePolicy>,
    group_fx: Vec<Vec<GroupFxStage>>,
    group_amps: Option<group_amp::GroupAmps>,
    has_solo: bool,
    sample_files: Vec<String>,
    program: model::ProgramInfo,
    cache: HashMap<String, usize>,
    samples: Vec<SampleAsset>,
    zone_samples: Vec<Option<usize>>,
    streaming: Option<StreamEngine>,
    resident_bytes: usize,
    underrun_frames: u64,
    underrun_blocks: u64,
    offline: bool,
    voices: Vec<Option<Voice>>,
    notes: Box<[Option<NoteLifetime>]>,
    next_note_id: u64,
    sustain_down: [bool; 16],
    selection: GroupStart,
    expression: Modulation,
    internal_mod: InternalMod,
    fx: Option<PostFx>,
    aux: Box<[AuxFrame]>,
    all_sound_fade: Option<(u32, u32)>,
    gain_db: f32,
    sample_rate: f32,
}

impl SamplerPlugin {
    pub fn load(path: impl AsRef<Path>) -> Result<Self, LoadError> {
        Self::load_with_sample_roots(path, &[])
    }

    pub fn load_with_sample_roots(
        path: impl AsRef<Path>,
        roots: &[std::path::PathBuf],
    ) -> Result<Self, LoadError> {
        let mut loaded = LoadedInstrument::load_with_sample_roots(path, roots)
            .map_err(|e| LoadError::Failed(e.to_string()))?;
        let zones = loaded.instrument.zones.clone();
        let groups = loaded.instrument.groups.clone();
        let sample_files = loaded.instrument.sample_files.clone();
        let program = loaded.instrument.program.clone();
        let has_solo = groups.iter().any(|group| group.soloed);
        let selection = GroupStart::new(&groups, &program);
        let expression = Modulation::new(&groups, VOICE_POOL)?;
        let internal_mod = InternalMod::new(&groups);
        let fx = PostFx::load(&mut loaded)?;
        let mut plugin = SamplerPlugin {
            loaded,
            zone_samples: vec![None; zones.len()],
            zones,
            groups,
            voice_policies: Vec::new(),
            group_fx: Vec::new(),
            group_amps: None,
            has_solo,
            sample_files,
            program,
            cache: HashMap::new(),
            samples: Vec::new(),
            streaming: None,
            resident_bytes: 0,
            underrun_frames: 0,
            underrun_blocks: 0,
            offline: false,
            voices: (0..VOICE_POOL).map(|_| None).collect(),
            notes: vec![None; NOTE_POOL].into_boxed_slice(),
            next_note_id: 0,
            sustain_down: [false; 16],
            selection,
            expression,
            internal_mod,
            fx,
            aux: vec![[[0.0; 2]; 8]; AUX_CHUNK].into_boxed_slice(),
            all_sound_fade: None,
            gain_db: 0.0,
            sample_rate: 48_000.0,
        };
        plugin.rebuild_voice_policies();
        plugin.rebuild_group_fx()?;
        Ok(plugin)
    }

    /// Deterministic random-selection seed, for tests and offline renders.
    pub fn seed_random(&mut self, seed: u64) {
        self.selection.seed_random(seed);
    }

    /// Re-resolve voice-group limits after a test or host changes the program.
    pub(crate) fn rebuild_voice_policies(&mut self) {
        self.voice_policies = self
            .groups
            .iter()
            .map(|group| VoicePolicy::from_group(&self.program, group))
            .collect();
    }

    fn rebuild_group_fx(&mut self) -> Result<(), LoadError> {
        let stages: Vec<Vec<GroupFxStage>> = self
            .groups
            .iter()
            .map(group_fx::compile)
            .collect::<Result<_, _>>()?;
        let mut layout = vec![[None; group_fx::GROUP_FX_SLOTS]; stages.len()];
        let mut has_amps = false;
        for (slots, stages) in layout.iter_mut().zip(&stages) {
            for stage in stages {
                if let GroupFxStage::Amp { settings, slot, .. } = *stage {
                    slots[slot] = Some(settings);
                    has_amps = true;
                }
            }
        }
        let mut amps = if has_amps {
            Some(
                group_amp::GroupAmps::new(&layout, self.voices.len(), self.sample_rate)
                    .map_err(|error| LoadError::Failed(error.into()))?,
            )
        } else {
            None
        };
        if let Some(amps) = &mut amps {
            for (index, voice) in self.voices.iter().enumerate() {
                if let Some(voice) = voice {
                    amps.activate(voice.group, index);
                }
            }
        }
        self.group_fx = stages;
        self.group_amps = amps;
        Ok(())
    }

    /// Load every sample needed for this note/velocity, including release
    /// layers. Returns the number of newly cached samples.
    pub fn preload_note(&mut self, note: i32, velocity: u16) -> usize {
        let mut loaded = 0;
        for index in 0..self.zones.len() {
            if !self.zone_matches(index, note, velocity) {
                continue;
            }
            if self.preload_zone(index) {
                loaded += 1;
            }
        }
        loaded
    }

    /// Explicit full-RAM reference/development path. Production uses stream_all.
    pub fn preload_all(&mut self) -> usize {
        let mut loaded = 0;
        for index in 0..self.zones.len() {
            if self.preload_zone(index) {
                loaded += 1;
            }
        }
        loaded
    }

    /// Owned IR samples, FFT kernels and histories, excluding allocator overhead.
    /// These are separate from sample-streaming telemetry's sample/page budget.
    pub fn convolution_memory_bytes(&self) -> usize {
        self.fx.as_ref().map_or(0, PostFx::convolution_memory_bytes)
    }

    pub fn cached_samples(&self) -> usize {
        self.cache.len()
    }

    pub fn program_name(&self) -> &str {
        &self.program.name
    }

    /// Open/index samples and make zone attacks and loop boundaries resident.
    /// Call only outside processing. No instrument is published until this returns.
    /// Cache storage is fixed at 16 MiB for 64 voices, independent of library size.
    pub fn stream_all(&mut self, config: StreamingConfig) -> Result<StreamingStats, LoadError> {
        if config.preload_frames == 0 || config.preload_frames > 1_048_576 {
            return Err(LoadError::Failed(
                "preload_frames must be in 1..=1048576".into(),
            ));
        }
        self.reset();
        self.streaming = None;
        self.samples.clear();
        self.cache.clear();
        self.zone_samples.fill(None);
        self.resident_bytes = 0;
        let decrypt = self
            .loaded
            .retail_key()
            .map(|key| Arc::<[u8]>::from(key.pad()));
        let mut needed: BTreeMap<String, BTreeSet<usize>> = BTreeMap::new();
        for (index, zone) in self.zones.iter().enumerate() {
            let Some(reference) = self.sample_reference(index) else {
                continue;
            };
            let pages = needed.entry(reference.to_string()).or_default();
            let start = zone.sample_start as usize / PAGE_FRAMES * PAGE_FRAMES;
            // Include the partially consumed start page in addition to the full
            // attack window so arbitrary sample-start offsets keep the budgeted lead.
            let count = config.preload_frames.div_ceil(PAGE_FRAMES) + 1;
            for i in 0..count {
                pages.insert(start.saturating_add(i * PAGE_FRAMES));
            }
            for loop_info in zone
                .loops
                .iter()
                .filter(|l| l.length > 0 && (l.mode == 1 || l.mode == 2))
            {
                for boundary in [
                    loop_info.start as usize,
                    (loop_info.start as usize)
                        .saturating_add(loop_info.length as usize)
                        .saturating_sub(1),
                ] {
                    let base = boundary / PAGE_FRAMES * PAGE_FRAMES;
                    pages.insert(base);
                    pages.insert(base.saturating_add(PAGE_FRAMES));
                    pages.insert(base.saturating_sub(PAGE_FRAMES));
                }
            }
        }
        let mut readers = Vec::new();
        for (reference, starts) in needed {
            let location = match self.loaded.sample_location(&reference) {
                Ok(location) => location,
                Err(error) => return Err(LoadError::Failed(format!("{reference}: {error}"))),
            };
            let mut reader = SampleReader::open_with_pad(location, decrypt.clone())
                .map_err(|e| LoadError::Failed(format!("{reference}: {e}")))?;
            let info = reader.metadata();
            if info.num_frames == 0 {
                continue;
            }
            for (index, zone) in self.zones.iter().enumerate() {
                if zone.absolute_end
                    && self.sample_reference(index) == Some(reference.as_str())
                    && (zone.sample_start as usize >= info.num_frames
                        || (zone.sample_end != u32::MAX
                            && zone.sample_end as usize >= info.num_frames)
                        || zone.loops.iter().any(|l| {
                            (l.start as usize).saturating_add(l.length as usize) > info.num_frames
                        }))
                {
                    return Err(LoadError::Failed(format!(
                        "{reference}: XML zone/loop lies outside sample frames"
                    )));
                }
            }
            let mut starts = starts;
            for (zone_index, zone) in self.zones.iter().enumerate() {
                if self.sample_reference(zone_index) != Some(reference.as_str())
                    || !self
                        .groups
                        .get(zone.group_index as usize)
                        .is_some_and(|g| g.reverse)
                {
                    continue;
                }
                let end = zone.end_frame(info.num_frames);
                let base = end.saturating_sub(1) / PAGE_FRAMES * PAGE_FRAMES;
                for i in 0..config.preload_frames.div_ceil(PAGE_FRAMES) + 1 {
                    starts.insert(base.saturating_sub(i * PAGE_FRAMES));
                }
            }
            let mut pages = Vec::new();
            for start in starts.into_iter().filter(|start| *start < info.num_frames) {
                let bytes = PAGE_FRAMES * 2 * std::mem::size_of::<f32>();
                if bytes > config.max_preload_bytes.saturating_sub(self.resident_bytes) {
                    return Err(LoadError::Failed(format!(
                        "sample attack/loop preload exceeds {} byte budget",
                        config.max_preload_bytes
                    )));
                }
                let sample = reader
                    .read_frames(start, PAGE_FRAMES)
                    .map_err(|e| LoadError::Failed(format!("{reference}: {e}")))?;
                pages.push(PreloadPage {
                    start,
                    data: streaming::stereo_page(&sample),
                });
                self.resident_bytes += bytes;
            }
            let id = readers.len();
            readers.push(reader);
            let index = self.samples.len();
            self.samples.push(SampleAsset {
                info,
                data: SampleData::Stream { id, pages },
            });
            self.cache.insert(reference, index);
        }
        for index in 0..self.zones.len() {
            self.zone_samples[index] = self
                .sample_reference(index)
                .and_then(|r| self.cache.get(r))
                .copied();
        }
        if !readers.is_empty() {
            let mut engine = StreamEngine::new(readers, VOICE_POOL)
                .map_err(|e| LoadError::Failed(e.to_string()))?;
            engine.set_offline(self.offline);
            self.streaming = Some(engine);
        }
        Ok(self.streaming_stats())
    }

    pub fn streaming_stats(&self) -> StreamingStats {
        StreamingStats {
            resident_bytes: self.resident_bytes,
            underrun_frames: self.underrun_frames,
            underrun_blocks: self.underrun_blocks,
            ..self
                .streaming
                .as_ref()
                .map_or_else(StreamingStats::default, StreamEngine::stats)
        }
    }

    fn preload_zone(&mut self, index: usize) -> bool {
        let Some(reference) = self.sample_reference(index).map(str::to_string) else {
            return false;
        };
        if let Some(&sample) = self.cache.get(&reference) {
            self.zone_samples[index] = Some(sample);
            return false;
        }
        // Full-RAM loading cannot mutate a running streaming engine's sample IDs.
        if self.streaming.is_some() {
            return false;
        }
        match self.loaded.decoded_sample(&reference) {
            Ok(sample) if sample.channels > 0 && sample.num_frames > 0 => {
                let info = SampleInfo {
                    channels: sample.channels.min(2),
                    sample_rate: sample.sample_rate,
                    num_frames: sample.num_frames,
                };
                self.resident_bytes += sample
                    .samples
                    .iter()
                    .map(|s| s.len() * std::mem::size_of::<f32>())
                    .sum::<usize>();
                let id = self.samples.len();
                self.samples.push(SampleAsset {
                    info,
                    data: SampleData::Resident(sample),
                });
                self.cache.insert(reference, id);
                self.zone_samples[index] = Some(id);
                true
            }
            _ => false,
        }
    }

    fn sample_reference(&self, index: usize) -> Option<&str> {
        let zone = self.zones.get(index)?;
        let id = zone.filename_id? as usize;
        self.sample_files.get(id).map(String::as_str)
    }

    /// Launch gain for a zone, or `None` when it must not sound.
    ///
    /// Program key/velocity ranges are independent gates and must not clip the
    /// zone's own bounds, because the fade widths are measured from the zone
    /// edges (see docs/kontakt-crossfade-evidence.md). The note passed here is
    /// the input key; the program transpose is applied before both gates.
    fn zone_selection(&self, index: usize, note: i32, velocity: u16) -> Option<f32> {
        let zone = self.zones.get(index)?;
        if !self.group_enabled(zone.group_index) || zone.filename_id.is_none() {
            return None;
        }
        let mapped = note + self.program.midi_transpose;
        let (low_key, high_key) = self.program.key_range;
        let (low_velocity, high_velocity) = self.program.velocity_range;
        if mapped < low_key as i32
            || mapped > high_key as i32
            || velocity < low_velocity
            || velocity > high_velocity
        {
            return None;
        }
        let gain = zone.selection_gain(mapped, velocity);
        (gain > 0.0).then_some(gain)
    }

    fn zone_matches(&self, index: usize, note: i32, velocity: u16) -> bool {
        self.zone_selection(index, note, velocity).is_some()
    }

    fn group_enabled(&self, index: u32) -> bool {
        self.groups.get(index as usize).map_or(true, |group| {
            !group.muted && (!self.has_solo || group.soloed)
        })
    }

    fn is_release_group(&self, group_index: u32) -> bool {
        // Native `GenerateSysScript` builds `%rlsTrigGroups` from the group's
        // serialized release-trigger flag (runtime +0x65b), not display names.
        self.groups
            .get(group_index as usize)
            .is_some_and(|group| group.release_trigger)
    }

    fn is_pedal_group(&self, group_index: u32) -> bool {
        self.groups.get(group_index as usize).is_some_and(|group| {
            group.name_trigger_heuristics && contains_ascii_case_insensitive(&group.name, b"pedal")
        })
    }

    fn master_gain(&self) -> f32 {
        10f32.powf(self.gain_db / 20.0)
    }

    fn sustain_enabled(&self) -> bool {
        self.program.cc64_mode != model::Cc64Mode::ControllerOnly
    }

    fn note_on(&mut self, note: u8, velocity: u8, channel: u8) {
        // Avoid a live ID collision even after the counter wraps.
        if self.next_note_id == u64::MAX {
            self.reset();
        }
        let slot = self
            .notes
            .iter()
            .position(Option::is_none)
            .unwrap_or_else(|| {
                self.notes
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, note)| note.unwrap().id)
                    .map(|(index, _)| index)
                    .unwrap_or(0)
            });
        if let Some(retired) = self.notes[slot] {
            for voice in &mut self.voices {
                if voice
                    .as_ref()
                    .is_some_and(|voice| voice.note_id == retired.id)
                {
                    *voice = None;
                }
            }
        }
        let note_id = self.next_note_id;
        self.next_note_id += 1;
        self.notes[slot] = Some(NoteLifetime {
            id: note_id,
            note,
            velocity,
            channel,
            release_velocity: 0.0,
            sustained: false,
        });
        self.selection.begin_note(note);
        let note_value = note as i32;
        let velocity_value = velocity as u16;

        for index in 0..self.zones.len() {
            if !self.zone_matches(index, note_value, velocity_value) {
                continue;
            }
            let group_index = self.zones[index].group_index;
            if self.is_release_group(group_index) || self.is_pedal_group(group_index) {
                continue;
            }
            self.spawn(
                index,
                note,
                NoteVelocity {
                    attack: velocity,
                    release: 0.0,
                },
                false,
                channel,
                note_id,
            );
        }
    }

    fn note_off(&mut self, note: u8, channel: u8, release_velocity: f32) {
        // Kontakt's external note-off scans all eligible original note events.
        // Already deferred events keep their identity and are not replayed.
        for index in 0..self.notes.len() {
            if self.notes[index].is_some_and(|entry| {
                entry.note == note && entry.channel == channel && !entry.sustained
            }) {
                self.notes[index].as_mut().unwrap().release_velocity = release_velocity;
                self.release_note(index);
            }
        }
    }

    fn release_note(&mut self, index: usize) {
        let Some(note) = self.notes[index] else {
            return;
        };
        if self.sustain_enabled() && self.sustain_down[note.channel as usize] {
            self.notes[index].as_mut().unwrap().sustained = true;
        } else {
            self.notes[index] = None;
            self.release_voices(note);
            self.spawn_release_layers(note);
        }
    }

    fn release_voices(&mut self, note: NoteLifetime) {
        for voice in self.voices.iter_mut().flatten() {
            if voice.note_id == note.id
                && voice.channel == note.channel
                && voice.note == note.note
                && !voice.is_release
                && !voice.one_shot
                && voice.release_age.is_none()
            {
                voice.release_age = Some(voice.age);
                voice.release_velocity = note.release_velocity;
                if let Some(release_loop) = voice.release_loop {
                    voice.loop_state = Some(release_loop);
                } else if voice.loop_until_release {
                    voice.loop_state = None;
                }
            }
        }
    }

    fn spawn_release_layers(&mut self, note: NoteLifetime) {
        self.selection.begin_release();
        let note_value = note.note as i32;
        let velocity_value = note.velocity as u16;
        for index in 0..self.zones.len() {
            if !self.zone_matches(index, note_value, velocity_value) {
                continue;
            }
            if self.is_release_group(self.zones[index].group_index) {
                self.spawn(
                    index,
                    note.note,
                    NoteVelocity {
                        attack: note.velocity,
                        release: note.release_velocity,
                    },
                    true,
                    note.channel,
                    note.id,
                );
            }
        }
    }

    fn pedal_up(&mut self, channel: u8) {
        for index in 0..self.notes.len() {
            if self.notes[index].is_some_and(|note| note.channel == channel && note.sustained) {
                self.release_note(index);
            }
        }
    }

    fn control_change(&mut self, controller: u8, value: u8, channel: u8) {
        self.selection.set_controller(channel, controller, value);
        self.expression.controller(channel, controller, value);
        match controller {
            64 if self.sustain_enabled() => {
                let channel_index = (channel as usize) % 16;
                let down = value >= 64;
                let was_down = self.sustain_down[channel_index];
                self.sustain_down[channel_index] = down;
                if was_down && !down {
                    self.pedal_up(channel);
                }
            }
            // All Sound Off. The generated system script fades all events out
            // over 10 ms and then requests note-off. Notes are cleared first so
            // the fade cannot spawn synthetic release layers; sounding voices
            // ramp to silence in `render_voices` and are then retired.
            120 => {
                self.notes.fill(None);
                let frames = (self.sample_rate * 0.010).round().max(1.0) as u32;
                self.all_sound_fade = Some((0, frames));
            }
            // The generated system controller script has no CC121 branch, so
            // it is delivered to instrument-defined routes as an ordinary
            // controller. Cadence deliberately adds no reset-controllers
            // policy here.
            121 => {}
            // The generated system script requests note-off for all events;
            // the system pedal can still defer effective termination.
            123 => {
                for index in 0..self.notes.len() {
                    if self.notes[index].is_some_and(|note| !note.sustained) {
                        self.release_note(index);
                    }
                }
            }
            _ => {}
        }
    }

    fn spawn(
        &mut self,
        zone_index: usize,
        note: u8,
        velocity: NoteVelocity,
        is_release: bool,
        channel: u8,
        note_id: u64,
    ) {
        let Some(group_index) = self.zones.get(zone_index).map(|zone| zone.group_index) else {
            return;
        };
        if !self.selection.allows(group_index, channel) {
            return;
        }
        let Some(selection) = self.zone_selection(zone_index, note as i32, velocity.attack as u16)
        else {
            return;
        };
        let Some(sample_id) = self.zone_samples[zone_index] else {
            return;
        };
        let sample = &self.samples[sample_id].info;
        let zone = &self.zones[zone_index];
        let group = self.groups.get(zone.group_index as usize);
        // Group MIDI channel: binary `All` is stored as -1 and XML `All`
        // parses as 0; 1..=16 selects a one-based channel.
        if group.is_some_and(|group| {
            (1..=16).contains(&group.midi_channel) && group.midi_channel - 1 != channel as i16
        }) {
            return;
        }

        let gain = self.program.volume * group.map_or(1.0, |g| g.volume) * zone.volume * selection;
        let pan = (self.program.pan + group.map_or(0.0, |g| g.pan) + zone.pan).clamp(-1.0, 1.0);
        let (pan_left, pan_right) = linear_balance(pan);
        let tune = self.program.tune * group.map_or(1.0, |g| g.tune) * zone.tune;
        let semitones = if group.map_or(true, |group| group.key_tracking) {
            note as f32 + self.program.midi_transpose as f32 - zone.root_key as f32
        } else {
            0.0
        };
        let speed = 2f32.powf(semitones / 12.0) * tune * sample.sample_rate as f32
            / self.sample_rate.max(1.0);

        let start = (zone.sample_start as usize).min(sample.num_frames);
        let end = zone.end_frame(sample.num_frames);
        if end <= start {
            return;
        }
        let sustain_loop = zone.loops.iter().find(|l| l.length > 0 && l.mode == 1);
        let release_loop_source = zone.loops.iter().find(|l| l.length > 0 && l.mode == 2);
        let (loop_state, release_loop, loop_until_release) = if let Some(base) = sustain_loop {
            (
                Self::make_loop(base, start, end),
                release_loop_source.and_then(|l| Self::make_loop(l, start, end)),
                false,
            )
        } else if let Some(base) = release_loop_source {
            (Self::make_loop(base, start, end), None, true)
        } else {
            (None, None, false)
        };
        let reverse = group.is_some_and(|g| g.reverse);
        let envelope = group.and_then(|g| g.envelope.clone());
        let envelope_intensity = group.map_or(1.0, |group| group.envelope_intensity);
        let one_shot = zone.one_shot || group.is_some_and(|group| group.one_shot);
        let interp_quality = group.map_or(0, |group| group.interp_quality.clamp(0, 2) as u8);

        self.enforce_voice_limits(group_index as usize);
        let slot = self.free_slot();
        self.expression.start_voice(slot);
        self.expression.inherit_poly_pressure(
            slot,
            group_index as usize,
            channel,
            note,
            &self.voices,
        );
        if let Some(amps) = &mut self.group_amps {
            amps.activate(group_index as usize, slot);
        }
        self.voices[slot] = Some(Voice {
            sample: sample_id,
            stream_page: usize::MAX,
            last_output: (0.0, 0.0),
            recovery: 1.0,
            position: if reverse {
                (end - 1) as f64
            } else {
                start as f64
            },
            direction: if reverse { -1.0 } else { 1.0 },
            age: 0,
            release_age: None,
            choke_fade: None,
            note,
            channel,
            note_id,
            velocity: velocity.attack,
            release_velocity: velocity.release,
            group: group_index as usize,
            base_pan: pan,
            mod_octaves: 0.0,
            mod_pitch: 1.0,
            is_release,
            one_shot,
            loop_until_release,
            gain,
            pan_left,
            pan_right,
            speed: speed as f64,
            start,
            end,
            loop_state,
            release_loop,
            filter_states: [FilterState::default(); MAX_FILTER_BIQUADS],
            native_filter_states: [filter_dsp::NativeFilterState::default();
                group_fx::GROUP_FX_SLOTS],
            shaper_states: [shaper_dsp::ShaperState::default(); group_fx::GROUP_FX_SLOTS],
            interp_quality,
            envelope,
            envelope_intensity,
        });
    }

    fn make_loop(loop_info: &nk_format::zone::Loop, start: usize, end: usize) -> Option<LoopState> {
        let loop_start = (loop_info.start as usize).clamp(start, end);
        let loop_end = (loop_info.start as usize)
            .saturating_add(loop_info.length as usize)
            .clamp(loop_start, end);
        if loop_end <= loop_start {
            return None;
        }
        Some(LoopState {
            start: loop_start,
            end: loop_end,
            alternating: loop_info.alternating,
            tuning: if loop_info.tuning.is_finite() && loop_info.tuning > 0.0 {
                loop_info.tuning as f64
            } else {
                1.0
            },
            crossfade: (loop_info.crossfade_length as usize).min(loop_end - loop_start),
            remaining: if loop_info.count == 0 {
                None
            } else {
                Some(loop_info.count)
            },
        })
    }

    fn enforce_voice_limits(&mut self, new_group: usize) {
        let Some(policy) = self.voice_policies.get(new_group).copied() else {
            return;
        };
        // Exclusion groups choke other voice groups in the same non-negative
        // exclusion slot.
        if policy.exclusion_group >= 0 {
            for slot in 0..self.voices.len() {
                let Some(voice) = self.voices[slot].as_ref() else {
                    continue;
                };
                let Some(other) = self.voice_policies.get(voice.group).copied() else {
                    continue;
                };
                if other.exclusion_group == policy.exclusion_group
                    && other.group_index != policy.group_index
                {
                    self.choke_voice(slot, other.fade_time_ms);
                }
            }
        }
        if policy.max_voices == 0 {
            return;
        }
        let active = self
            .voices
            .iter()
            .flatten()
            .filter(|voice| {
                self.voice_policies
                    .get(voice.group)
                    .is_some_and(|voice_policy| voice_policy.group_index == policy.group_index)
            })
            .count();
        if active as u32 >= policy.max_voices {
            if let Some(slot) = self.select_voice_victim(policy) {
                self.choke_voice(slot, policy.fade_time_ms);
            }
        }
    }

    fn select_voice_victim(&self, policy: VoicePolicy) -> Option<usize> {
        let mut candidates = [false; VOICE_POOL];
        let mut any_released = false;
        for (slot, voice) in self.voices.iter().enumerate() {
            let Some(voice) = voice else {
                continue;
            };
            let Some(voice_policy) = self.voice_policies.get(voice.group) else {
                continue;
            };
            if voice_policy.group_index != policy.group_index {
                continue;
            }
            candidates[slot] = true;
            any_released |= voice.release_age.is_some() || voice.is_release;
        }
        let prefer_released = policy.prefer_released && any_released;
        let mut best = None;
        for (slot, voice) in self.voices.iter().enumerate() {
            if !candidates[slot] {
                continue;
            }
            let voice = voice.as_ref().unwrap();
            if prefer_released && voice.release_age.is_none() && !voice.is_release {
                continue;
            }
            let Some(best_slot) = best else {
                best = Some(slot);
                continue;
            };
            let best_voice = self.voices[best_slot].as_ref().unwrap();
            let replace = match policy.kill_mode {
                model::KillMode::Any | model::KillMode::Oldest => voice.age > best_voice.age,
                model::KillMode::Newest => voice.age < best_voice.age,
                model::KillMode::Highest => voice.note > best_voice.note,
                model::KillMode::Lowest => voice.note < best_voice.note,
            };
            if replace {
                best = Some(slot);
            }
        }
        best
    }

    fn choke_voice(&mut self, slot: usize, fade_time_ms: u32) {
        let frames = (self.sample_rate.max(1.0) * fade_time_ms as f32 / 1000.0).round() as u32;
        if frames == 0 {
            self.voices[slot] = None;
            return;
        }
        // Fade in place when another slot is available; otherwise retire this
        // voice immediately so the incoming attack has a slot.
        if self.voices.iter().any(Option::is_none) {
            if let Some(voice) = self.voices[slot].as_mut() {
                voice.choke_fade = Some((0, frames.max(1)));
            }
        } else {
            self.voices[slot] = None;
        }
    }

    fn free_slot(&self) -> usize {
        if let Some(index) = self.voices.iter().position(Option::is_none) {
            return index;
        }
        let mut oldest = 0;
        let mut age = 0;
        for (index, voice) in self.voices.iter().enumerate() {
            if let Some(voice) = voice {
                if voice.age >= age {
                    age = voice.age;
                    oldest = index;
                }
            }
        }
        oldest
    }

    fn render_voices(&mut self, audio: &mut AudioBuffer<'_>, start: usize, end: usize) {
        for at in (start..end).step_by(AUX_CHUNK) {
            self.render_segment(audio, at, (at + AUX_CHUNK).min(end));
        }
    }

    fn render_segment(&mut self, audio: &mut AudioBuffer<'_>, start: usize, end: usize) {
        if end <= start {
            return;
        }
        let count = end - start;
        self.aux[..count].fill([[0.0; 2]; 8]);
        let sample_rate = self.sample_rate.max(1.0);
        let master_gain = self.master_gain();

        for (voice_index, slot) in self.voices.iter_mut().enumerate() {
            let Some(voice) = slot else {
                continue;
            };
            let finished = render_voice(
                voice,
                VoiceResources {
                    sample: &self.samples[voice.sample],
                    streaming: self.streaming.as_mut(),
                    voice_index,
                    master_gain,
                    underrun_frames: &mut self.underrun_frames,
                    expression: &mut self.expression,
                    internal_mod: &self.internal_mod,
                    group_fx: &self.group_fx,
                    group_amps: self.group_amps.as_mut(),
                    amp_split: self
                        .groups
                        .get(voice.group)
                        .map_or(6, |group| group.fx_amp_split_point as usize),
                    all_sound_fade: self.all_sound_fade,
                    aux: &mut self.aux[..count],
                },
                audio,
                start,
                count,
                sample_rate,
            );
            voice.age = voice.age.saturating_add(count as u64);
            if finished {
                *slot = None;
            }
        }

        // CC120's shared clock advances once per frame. Each voice applies
        // the fade at its amplifier boundary, before post-amplifier effects.
        if let Some((elapsed, total)) = self.all_sound_fade {
            let next = elapsed.saturating_add(count as u32);
            if next >= total {
                self.voices.iter_mut().for_each(|slot| *slot = None);
                self.all_sound_fade = None;
            } else {
                self.all_sound_fade = Some((next, total));
            }
        }
        if let Some(fx) = &mut self.fx {
            fx.process_range(
                audio.outputs,
                audio.output_channels,
                audio.frames,
                start,
                &mut self.aux[..count],
            );
        }
    }
}

impl Plugin for SamplerPlugin {
    fn load_notices(&self) -> Vec<String> {
        let mut notices = vec!["Kontakt playback subset: custom interfaces, scripts, time-stretch and formant engines are not supported. Use the instrument's saved sample mapping; full Kontakt behavior is not guaranteed.".into()];
        notices.extend(self.loaded.compatibility_issues.iter().cloned());
        notices
    }
    fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: "com.platform.kontakt",
            name: "Kontakt Instrument",
            vendor: "Platform",
            version: "0.1.0",
            kind: PluginKind::Instrument,
        }
    }

    fn capabilities(&self) -> Capabilities {
        let mut capabilities = Capabilities::stereo_instrument();
        capabilities.tail_samples = self.fx.as_ref().map_or(0, PostFx::tail);
        capabilities
    }

    fn parameters(&self) -> Vec<ParameterInfo> {
        vec![GAIN]
    }

    fn ui(&self) -> Vec<UiNode> {
        vec![UiNode::Group {
            label: "Output".into(),
            children: vec![UiNode::Knob {
                param: 1,
                label: "Gain".into(),
            }],
        }]
    }

    fn set_offline_rendering(&mut self, offline: bool) {
        self.offline = offline;
        if let Some(streaming) = &mut self.streaming {
            streaming.set_offline(offline);
        }
    }

    fn processing_error(&self) -> Option<&'static str> {
        if self.fx.as_ref().is_some_and(PostFx::failed) {
            return Some(
                "Convolution preparation failed: unsupported sample rate or IR budget exceeded",
            );
        }
        self.streaming
            .as_ref()
            .is_some_and(StreamEngine::has_error)
            .then_some("Sample streaming failed: sample read error or offline page timeout")
    }

    fn source_underrun_frames(&self) -> u64 {
        self.underrun_frames
    }

    fn streaming_telemetry(&self) -> Option<plugin_api::StreamingTelemetry> {
        self.streaming.as_ref()?;
        let stats = self.streaming_stats();
        Some(plugin_api::StreamingTelemetry {
            resident_bytes: stats.resident_bytes as u64,
            cache_capacity_bytes: stats.page_cache_bytes as u64,
            cache_ready_bytes: stats.cache_ready_bytes as u64,
            peak_cache_ready_bytes: stats.peak_cache_ready_bytes as u64,
            pages_read: stats.pages_read,
            read_errors: stats.read_errors,
            underrun_frames: stats.underrun_frames,
            underrun_blocks: stats.underrun_blocks,
            read_count: stats.read_count,
            read_total_ns: stats.read_total_ns,
            last_read_ns: stats.last_read_ns,
            max_read_ns: stats.max_read_ns,
            active_voices: self.voices.iter().flatten().count() as u32,
            voice_capacity: VOICE_POOL as u32,
            pending_reads: stats.pending_reads,
            flags: 1,
        })
    }

    fn prepare(&mut self, context: &ProcessContext) {
        let sample_rate = context.sample_rate as f32;
        if sample_rate != self.sample_rate {
            for voice in self.voices.iter_mut().flatten() {
                for state in &mut voice.filter_states {
                    state.valid = false;
                }
            }
        }
        self.sample_rate = sample_rate;
        self.expression.prepare(self.sample_rate);
        if let Some(fx) = &mut self.fx {
            fx.prepare(self.sample_rate);
        }
        if let Some(amps) = &mut self.group_amps {
            if amps.prepare(self.sample_rate).is_ok() {
                for (index, voice) in self.voices.iter().enumerate() {
                    if let Some(voice) = voice {
                        amps.activate(voice.group, index);
                    }
                }
            }
        }
    }

    fn reset(&mut self) {
        self.voices.iter_mut().for_each(|slot| *slot = None);
        self.notes.fill(None);
        self.next_note_id = 0;
        self.sustain_down = [false; 16];
        self.all_sound_fade = None;
        self.selection.reset();
        self.expression.reset();
        if let Some(amps) = &mut self.group_amps {
            amps.reset();
        }
        if let Some(fx) = &mut self.fx {
            fx.reset();
        }
    }

    fn process(
        &mut self,
        _context: &ProcessContext,
        audio: &mut AudioBuffer<'_>,
        events: &[Event],
    ) {
        let frames = audio.frames;
        if frames == 0 || audio.output_channels == 0 {
            return;
        }

        let missing_before = self.underrun_frames;
        let mut cursor = 0;
        for event in events {
            let frame = event.frame.min(frames);
            if frame > cursor {
                self.render_voices(audio, cursor, frame);
                cursor = frame;
            }
            self.handle_event(event);
        }
        if cursor < frames {
            self.render_voices(audio, cursor, frames);
        }
        if self.underrun_frames > missing_before {
            self.underrun_blocks = self.underrun_blocks.saturating_add(1);
        }
    }

    fn save_state(&self) -> State {
        State {
            schema_version: 1,
            bytes: self.gain_db.to_le_bytes().to_vec(),
        }
    }

    fn load_state(&mut self, state: &State) -> Result<(), StateError> {
        if state.schema_version > 1 {
            return Err(StateError::SchemaTooNew {
                found: state.schema_version,
                supported: 1,
            });
        }
        if state.bytes.len() != 4 {
            return Err(StateError::Malformed("expected 4 bytes".into()));
        }
        self.gain_db = f32::from_le_bytes(state.bytes[..4].try_into().unwrap());
        Ok(())
    }
}

impl SamplerPlugin {
    fn handle_event(&mut self, event: &Event) {
        // Invalid wire values must not alias another channel or wrap into a
        // valid MIDI note/controller through integer truncation.
        if event.kind != EventKind::Parameter
            && (event.channel >= 16 || event.data1 > 127 || event.data2 > 127)
        {
            return;
        }
        match event.kind {
            EventKind::NoteOn if event.data2 > 0 => {
                self.note_on(event.data1 as u8, event.data2 as u8, event.channel)
            }
            // MIDI1 note-on with velocity zero synthesizes release velocity
            // 0x8000 in Kontakt's 16-bit domain (~0.5 normalized).
            EventKind::NoteOn => self.note_off(event.data1 as u8, event.channel, 0.5),
            EventKind::NoteOff => {
                self.note_off(event.data1 as u8, event.channel, event.data2 as f32 / 127.0)
            }
            EventKind::ControlChange => {
                self.control_change(event.data1 as u8, event.data2 as u8, event.channel)
            }
            EventKind::PitchBend => self.expression.bend(event.channel, event.value),
            EventKind::ChannelPressure => {
                self.expression
                    .pressure(event.channel, event.value, event.data1)
            }
            EventKind::PolyPressure => self.expression.poly_pressure(
                &self.voices,
                event.channel,
                event.data1 as u8,
                event.value,
            ),
            EventKind::Parameter if event.data1 as u32 == GAIN.id => {
                self.gain_db = GAIN.denormalize(event.value);
            }
            _ => {}
        }
    }
}

struct VoiceResources<'a> {
    sample: &'a SampleAsset,
    streaming: Option<&'a mut StreamEngine>,
    voice_index: usize,
    master_gain: f32,
    underrun_frames: &'a mut u64,
    expression: &'a mut Modulation,
    internal_mod: &'a InternalMod,
    group_fx: &'a [Vec<GroupFxStage>],
    group_amps: Option<&'a mut group_amp::GroupAmps>,
    amp_split: usize,
    all_sound_fade: Option<(u32, u32)>,
    aux: &'a mut [AuxFrame],
}

/// One sample of the physical group chain. The same filter state cursor is
/// retained across the two phases; each stage advances exactly once per frame.
struct GroupFrame<'a> {
    voice: &'a mut Voice,
    amps: Option<&'a mut group_amp::GroupAmps>,
    voice_index: usize,
    filter_octaves: f32,
    filter_slots: &'a [f32; 8],
    sample_rate: f32,
    state_index: usize,
}
impl GroupFrame<'_> {
    fn process(
        &mut self,
        stages: &[GroupFxStage],
        split: usize,
        before_amp: bool,
        signal: &mut [f32; 2],
        sends: &mut AuxFrame,
    ) {
        let [mut left, mut right] = *signal;
        for stage in stages {
            if stage.slot().map_or(true, |slot| slot < split) != before_amp {
                continue;
            }
            let out_level = match *stage {
                GroupFxStage::NativeFilter {
                    filter,
                    slot,
                    modulated,
                    out_level,
                } => {
                    let state = &mut self.voice.native_filter_states[slot];
                    let step = if modulated {
                        filter_dsp::NativeFilterState::step_modulated
                    } else {
                        filter_dsp::NativeFilterState::step
                    };
                    let output = step(
                        state,
                        filter,
                        filter.cutoff + self.filter_slots[slot],
                        self.filter_octaves,
                        self.sample_rate,
                        [left, right],
                    );
                    [left, right] = output;
                    out_level
                }
                GroupFxStage::Filter {
                    filter,
                    slot,
                    normalized_cutoff,
                    out_level,
                } => {
                    // Indexed native cutoff routes add in normalized
                    // filter space. Translate that bounded delta through
                    // the existing cutoff approximation, retaining the
                    // XML/internal octave routes independently.
                    let effective_octaves = self.filter_octaves
                        + slot.map_or(0.0, |index| {
                            let next =
                                (normalized_cutoff + self.filter_slots[index]).clamp(0.0, 1.0);
                            (next - normalized_cutoff) * 1000f32.log2()
                        });
                    let biquads = match filter.poles {
                        1 | 2 => 1,
                        4 => 2,
                        _ => 3,
                    };
                    for _ in 0..biquads {
                        let state = &mut self.voice.filter_states[self.state_index];
                        // Persist the control threshold across host/event
                        // segments so coefficient updates cannot depend on
                        // block boundaries. Rate changes invalidate it.
                        if !state.valid || (effective_octaves - state.cached_octaves).abs() > 0.01 {
                            state.coeffs = biquad_coeffs(
                                filter.shape,
                                filter.poles,
                                filter.cutoff_hz,
                                filter.q,
                                effective_octaves,
                                self.sample_rate,
                            );
                            state.cached_octaves = effective_octaves;
                            state.valid = true;
                        }
                        left = biquad_step(left, &mut state.left, &state.coeffs);
                        right = biquad_step(right, &mut state.right, &state.coeffs);
                        self.state_index += 1;
                    }
                    out_level
                }
                GroupFxStage::Spread {
                    spread,
                    pan,
                    out_level,
                    ..
                } => {
                    let width = (1.0 + spread).clamp(0.0, 2.0);
                    let mid = (left + right) * 0.5;
                    let side = (left - right) * 0.5 * width;
                    left = (mid + side) * (1.0 - pan.clamp(-1.0, 1.0)).max(0.0);
                    right = (mid - side) * (1.0 + pan.clamp(-1.0, 1.0)).max(0.0);
                    out_level
                }
                GroupFxStage::Send { levels, .. } => {
                    for (send, level) in sends.iter_mut().zip(levels) {
                        send[0] += left * level;
                        send[1] += right * level;
                    }
                    1.0
                }
                GroupFxStage::Amp {
                    slot, out_level, ..
                } => {
                    [left, right] = self.amps.as_deref_mut().map_or([0.0; 2], |amps| {
                        amps.process(self.voice.group, slot, self.voice_index, [left, right])
                    });
                    out_level
                }
                GroupFxStage::Shaper {
                    params,
                    slot,
                    out_level,
                } => {
                    [left, right] = self.voice.shaper_states[slot].step(
                        params,
                        self.sample_rate,
                        [left, right],
                    );
                    out_level
                }
            };
            left *= out_level;
            right *= out_level;
        }

        *signal = [left, right];
    }
}

fn render_voice(
    voice: &mut Voice,
    resources: VoiceResources<'_>,
    audio: &mut AudioBuffer<'_>,
    start: usize,
    count: usize,
    sample_rate: f32,
) -> bool {
    let VoiceResources {
        sample,
        mut streaming,
        voice_index,
        master_gain,
        underrun_frames,
        expression,
        internal_mod,
        group_fx,
        mut group_amps,
        amp_split,
        all_sound_fade,
        aux,
    } = resources;
    let output_channels = audio.output_channels.min(2);
    let release_seconds = voice.envelope.as_ref().map_or(0.05, |envelope| {
        ((if envelope.ahd_only {
            envelope.decay
        } else {
            envelope.release
        }) / 1000.0)
            .max(0.0)
    });

    for (i, aux_frame) in aux.iter_mut().take(count).enumerate() {
        if voice.position >= voice.end as f64 || voice.position < voice.start as f64 {
            return true;
        }

        let index = voice.position.floor() as usize;
        let page = index / PAGE_FRAMES * PAGE_FRAMES;
        if voice.stream_page != page {
            if let (SampleData::Stream { id, .. }, Some(engine)) =
                (&sample.data, streaming.as_mut())
            {
                let candidates = if voice.direction >= 0.0 {
                    [
                        page.checked_sub(PAGE_FRAMES),
                        Some(page),
                        page.checked_add(PAGE_FRAMES),
                        page.checked_add(2 * PAGE_FRAMES),
                    ]
                } else {
                    [
                        Some(page),
                        page.checked_sub(PAGE_FRAMES),
                        page.checked_sub(2 * PAGE_FRAMES),
                        page.checked_add(PAGE_FRAMES),
                    ]
                };
                let pages = candidates.map(|p| {
                    p.filter(|p| *p < sample.info.num_frames && sample.resident_frame(*p).is_none())
                });
                engine.plan(voice_index, *id, &pages);
            }
            voice.stream_page = page;
        }
        let mut value = sample_at(
            sample,
            &mut streaming,
            voice_index,
            voice.position,
            voice.end,
            voice.interp_quality,
        );
        // Forward-loop seam crossfade: blend the loop tail with the head of
        // the next pass over the configured crossfade length.
        if let Some(loop_state) = voice.loop_state {
            if !loop_state.alternating && loop_state.crossfade > 0 && voice.direction > 0.0 {
                let tail = loop_state.end as f64 - loop_state.crossfade as f64;
                if voice.position >= tail {
                    let head_position = voice.position - (loop_state.end - loop_state.start) as f64;
                    // Outside the pre-loop material there is nothing to fade
                    // from, so keep the main sample.
                    if head_position >= voice.start as f64 {
                        let head = sample_at(
                            sample,
                            &mut streaming,
                            voice_index,
                            head_position,
                            voice.end,
                            voice.interp_quality,
                        );
                        if let (Some(main), Some(head)) = (value, head) {
                            let mix = ((voice.position - tail) / loop_state.crossfade as f64)
                                .clamp(0.0, 1.0) as f32;
                            value = Some((
                                main.0 * (1.0 - mix) + head.0 * mix,
                                main.1 * (1.0 - mix) + head.1 * mix,
                            ));
                        }
                    }
                }
            }
        }
        let (left, right) = match value {
            Some(value) => {
                voice.recovery = (voice.recovery + 1.0 / 64.0).min(1.0);
                voice.last_output = value;
                (value.0 * voice.recovery, value.1 * voice.recovery)
            }
            None => {
                *underrun_frames = underrun_frames.saturating_add(1);
                voice.recovery = 0.0;
                voice.last_output.0 *= 0.94;
                voice.last_output.1 *= 0.94;
                voice.last_output
            }
        };

        let age = voice.age + i as u64;
        let time = age as f32 / sample_rate;
        // Release-velocity routes become available at termination (and on
        // release-triggered voices), not during the held attack stage.
        let release_velocity =
            (voice.release_age.is_some() || voice.is_release).then_some(voice.release_velocity);
        let modulation = expression.evaluate(
            voice_index,
            voice.group,
            voice.channel,
            voice.note,
            voice.velocity,
            release_velocity,
        );
        let internal = internal_mod.evaluate(voice.group, age, voice.release_age, sample_rate);
        let octaves = modulation.octaves + internal.octaves;
        if octaves != voice.mod_octaves {
            voice.mod_octaves = octaves;
            voice.mod_pitch = 2f64.powf(octaves as f64);
        }
        let mut amplitude = voice.gain * modulation.gain * internal.gain;
        if let Some(envelope) = &voice.envelope {
            let level = envelope_level(envelope, age, voice.release_age, sample_rate);
            // Amplitude-envelope intensity scales the reduction from unity.
            let depth = voice.envelope_intensity.clamp(0.0, 1.0);
            amplitude *= 1.0 - (1.0 - level) * depth;
        }
        if let Some((elapsed, total)) = voice.choke_fade {
            if elapsed >= total {
                return true;
            }
            amplitude *= 1.0 - elapsed as f32 / total as f32;
        }

        if let Some((elapsed, total)) = all_sound_fade {
            let position = elapsed.saturating_add(i as u32);
            amplitude *= if position >= total {
                0.0
            } else {
                1.0 - position as f32 / total as f32
            };
        }

        let output_index = start + i;
        let pan = modulation.pan + internal.pan;
        let (pan_left, pan_right) = if pan == 0.0 {
            (voice.pan_left, voice.pan_right)
        } else {
            linear_balance((voice.base_pan + pan).clamp(-1.0, 1.0))
        };
        let mut signal = [left, right];
        let mut sends = [[0.0f32; 2]; 8];
        let stages = group_fx.get(voice.group).map_or(&[][..], Vec::as_slice);
        {
            let mut group_frame = GroupFrame {
                voice,
                amps: group_amps.as_deref_mut(),
                voice_index,
                filter_octaves: modulation.filter_octaves + internal.filter_octaves,
                filter_slots: &modulation.filter_slots,
                sample_rate,
                state_index: 0,
            };
            group_frame.process(stages, amp_split, true, &mut signal, &mut sends);
            signal[0] *= amplitude * pan_left;
            signal[1] *= amplitude * pan_right;
            group_frame.process(stages, amp_split, false, &mut signal, &mut sends);
        }
        let [left, right] = signal;
        for (bus, send) in aux_frame.iter_mut().zip(sends) {
            bus[0] += send[0] * master_gain;
            bus[1] += send[1] * master_gain;
        }
        match output_channels {
            0 => {}
            1 => audio.output_mut(0)[output_index] += 0.5 * (left + right) * master_gain,
            _ => {
                audio.output_mut(0)[output_index] += left * master_gain;
                audio.output_mut(1)[output_index] += right * master_gain;
            }
        }

        let loop_tuning = voice.loop_state.map_or(1.0, |loop_state| loop_state.tuning);
        voice.position += voice.speed * voice.mod_pitch * voice.direction * loop_tuning;

        if let Some(mut loop_state) = voice.loop_state {
            let low = loop_state.start as f64;
            let high = (loop_state.end - 1) as f64;
            let length = (loop_state.end - loop_state.start) as f64;
            let crossed = if loop_state.alternating {
                (voice.direction > 0.0 && voice.position >= high)
                    || (voice.direction < 0.0 && voice.position < low)
            } else {
                (voice.direction > 0.0 && voice.position >= loop_state.end as f64)
                    || (voice.direction < 0.0 && voice.position < low)
            };
            if crossed {
                let keep_looping = match &mut loop_state.remaining {
                    None => true,
                    Some(0) => false,
                    Some(remaining) => {
                        *remaining -= 1;
                        true
                    }
                };
                if keep_looping {
                    if loop_state.alternating {
                        let span = high - low;
                        if span == 0.0 {
                            voice.position = low;
                        } else {
                            let phase = (voice.position - low).rem_euclid(2.0 * span);
                            voice.position = if phase < span {
                                low + phase
                            } else {
                                high - (phase - span)
                            };
                            if phase >= span {
                                voice.direction = -voice.direction;
                            }
                        }
                    } else {
                        voice.position = low + (voice.position - low).rem_euclid(length);
                    }
                    voice.stream_page = usize::MAX;
                    voice.loop_state = Some(loop_state);
                } else {
                    voice.loop_state = None;
                }
            }
        }

        if let Some(release_age) = voice.release_age {
            if time >= release_age as f32 / sample_rate + release_seconds {
                return true;
            }
        }
        if let Some((elapsed, _)) = &mut voice.choke_fade {
            *elapsed = elapsed.saturating_add(1);
        }
    }

    false
}

fn read_asset(
    sample: &SampleAsset,
    streaming: &mut Option<&mut StreamEngine>,
    voice_index: usize,
    index: usize,
) -> Option<(f32, f32)> {
    if let Some(frame) = sample.resident_frame(index) {
        return Some(frame);
    }
    if let SampleData::Stream { id, .. } = &sample.data {
        streaming.as_mut()?.frame(voice_index, *id, index)
    } else {
        None
    }
}

// Kontakt normal quality: BSample::ResampleChannelQuad_L_SSE2 (0x1004dc2a4).
// Preserve the recovered polynomial grouping and f32 constants. High/ultra
// retain their existing approximations until their sinc kernels are recovered.
fn lagrange4(y0: (f32, f32), y1: (f32, f32), y2: (f32, f32), y3: (f32, f32), t: f32) -> (f32, f32) {
    let x = t + 1.0;
    let y = x - 1.0;
    let z = y - 1.0;
    let channel = |a: f32, b: f32, c: f32, d: f32| {
        (z - 1.0) * (x * b * z - y * x * c) * 0.5
            + (z * y * x * d - (z - 1.0) * z * a * y) * 0.16666667
    };
    (
        channel(y0.0, y1.0, y2.0, y3.0),
        channel(y0.1, y1.1, y2.1, y3.1),
    )
}

fn cubic_interp(
    y0: (f32, f32),
    y1: (f32, f32),
    y2: (f32, f32),
    y3: (f32, f32),
    t: f32,
) -> (f32, f32) {
    let t2 = t * t;
    let t3 = t2 * t;
    let channel = |a: f32, b: f32, c: f32, d: f32| {
        0.5 * (2.0 * b
            + (-a + c) * t
            + (2.0 * a - 5.0 * b + 4.0 * c - d) * t2
            + (-a + 3.0 * b - 3.0 * c + d) * t3)
    };
    (
        channel(y0.0, y1.0, y2.0, y3.0),
        channel(y0.1, y1.1, y2.1, y3.1),
    )
}

fn lagrange6(
    y0: (f32, f32),
    y1: (f32, f32),
    y2: (f32, f32),
    y3: (f32, f32),
    y4: (f32, f32),
    y5: (f32, f32),
    t: f32,
) -> (f32, f32) {
    // Nodes at -2, -1, 0, 1, 2, 3; evaluate at t (0..1).
    const X: [f32; 6] = [-2.0, -1.0, 0.0, 1.0, 2.0, 3.0];
    let mut weights = [0.0f32; 6];
    for (j, weight) in weights.iter_mut().enumerate() {
        let mut numerator = 1.0;
        let mut denominator = 1.0;
        for (m, node) in X.iter().enumerate() {
            if m == j {
                continue;
            }
            numerator *= t - node;
            denominator *= X[j] - node;
        }
        *weight = numerator / denominator;
    }
    let ys = [y0, y1, y2, y3, y4, y5];
    let mut left = 0.0;
    let mut right = 0.0;
    for (y, weight) in ys.iter().zip(weights.iter()) {
        left += y.0 * weight;
        right += y.1 * weight;
    }
    (left, right)
}

fn biquad_coeffs(
    shape: FilterShape,
    poles: u8,
    cutoff_hz: f32,
    q: f32,
    octave_offset: f32,
    sample_rate: f32,
) -> [f32; 5] {
    let nyquist = sample_rate.max(1.0) * 0.45;
    let cutoff = (cutoff_hz * 2f32.powf(octave_offset.clamp(-16.0, 16.0))).clamp(10.0, nyquist);
    if poles == 1 {
        // Bilinear one-pole transfer. This is also the steady-coefficient
        // transfer of FilterSV1's recovered trapezoidal integrator; native
        // normalized frequency mapping/control-rate smoothing remain separate.
        let g = (std::f32::consts::PI * cutoff / sample_rate.max(1.0)).tan();
        let denominator = 1.0 + g;
        let (b0, b1) = match shape {
            FilterShape::Highpass => (1.0 / denominator, -1.0 / denominator),
            _ => (g / denominator, g / denominator),
        };
        return [b0, b1, 0.0, (g - 1.0) / denominator, 0.0];
    }
    let w0 = std::f32::consts::TAU * cutoff / sample_rate.max(1.0);
    let cos_w0 = w0.cos();
    let sin_w0 = w0.sin();
    let alpha = sin_w0 / (2.0 * q.max(0.1));
    let a0 = 1.0 + alpha;
    let (b0, b1, b2) = match shape {
        FilterShape::Lowpass => ((1.0 - cos_w0) * 0.5, 1.0 - cos_w0, (1.0 - cos_w0) * 0.5),
        FilterShape::Highpass => ((1.0 + cos_w0) * 0.5, -(1.0 + cos_w0), (1.0 + cos_w0) * 0.5),
        FilterShape::Bandpass => (alpha, 0.0, -alpha),
        FilterShape::Bandreject => (1.0, -2.0 * cos_w0, 1.0),
    };
    [
        b0 / a0,
        b1 / a0,
        b2 / a0,
        -2.0 * cos_w0 / a0,
        (1.0 - alpha) / a0,
    ]
}

fn biquad_step(input: f32, state: &mut [f32; 2], coeffs: &[f32; 5]) -> f32 {
    let output = coeffs[0] * input + state[0];
    state[0] = coeffs[1] * input - coeffs[3] * output + state[1];
    state[1] = coeffs[2] * input - coeffs[4] * output;
    output
}

fn sample_at(
    sample: &SampleAsset,
    streaming: &mut Option<&mut StreamEngine>,
    voice_index: usize,
    position: f64,
    end: usize,
    quality: u8,
) -> Option<(f32, f32)> {
    if !position.is_finite() || position < 0.0 || end == 0 {
        return None;
    }
    let index = position.floor() as usize;
    if index >= end {
        return None;
    }
    let fraction = position.fract() as f32;
    let next = (index + 1).min(end - 1);
    let y1 = read_asset(sample, streaming, voice_index, index)?;
    let y2 = read_asset(sample, streaming, voice_index, next)?;
    if quality == 0 && index > 0 && index + 2 < end {
        if let (Some(y0), Some(y3)) = (
            read_asset(sample, streaming, voice_index, index - 1),
            read_asset(sample, streaming, voice_index, index + 2),
        ) {
            return Some(lagrange4(y0, y1, y2, y3, fraction));
        }
    }
    if quality >= 2 && index >= 2 && index + 3 < end {
        let taps = (
            read_asset(sample, streaming, voice_index, index - 2),
            read_asset(sample, streaming, voice_index, index - 1),
            read_asset(sample, streaming, voice_index, index + 2),
            read_asset(sample, streaming, voice_index, index + 3),
        );
        if let (Some(y0), Some(y3), Some(y4), Some(y5)) = taps {
            return Some(lagrange6(y0, y3, y1, y2, y4, y5, fraction));
        }
    }
    if quality > 0 && index > 0 && index + 2 < end {
        if let (Some(y0), Some(y3)) = (
            read_asset(sample, streaming, voice_index, index - 1),
            read_asset(sample, streaming, voice_index, index + 2),
        ) {
            return Some(cubic_interp(y0, y1, y2, y3, fraction));
        }
    }
    Some((
        y1.0 * (1.0 - fraction) + y2.0 * fraction,
        y1.1 * (1.0 - fraction) + y2.1 * fraction,
    ))
}

pub(crate) fn envelope_level(
    envelope: &Envelope,
    age: u64,
    note_off_age: Option<u64>,
    sample_rate: f32,
) -> f32 {
    let sample_rate = sample_rate.max(1.0) as f64;
    let time = age as f64 / sample_rate;
    let attack = (envelope.attack / 1000.0).max(0.0) as f64;
    let hold = (envelope.hold / 1000.0).max(0.0) as f64;
    let decay = (envelope.decay / 1000.0).max(0.0) as f64;
    let sustain = if envelope.ahd_only {
        0.0
    } else {
        envelope.sustain.clamp(0.0, 1.0) as f64
    };
    let release = ((if envelope.ahd_only {
        envelope.decay
    } else {
        envelope.release
    }) / 1000.0)
        .max(0.0) as f64;

    let sustain_level = |t: f64| -> f64 {
        if t < attack {
            if attack <= 0.0 {
                1.0
            } else {
                // Only the attack segment carries the decoded `atkCurve`
                // value. Hold/decay/sustain/release remain linear until their
                // recovered curvature fields are decoded.
                curve::shaped_fraction((t / attack) as f32, envelope.curve) as f64
            }
        } else if t < attack + hold {
            1.0
        } else if t < attack + hold + decay {
            if decay <= 0.0 {
                sustain
            } else {
                1.0 + (sustain - 1.0) * ((t - attack - hold) / decay)
            }
        } else {
            sustain
        }
    };

    match note_off_age {
        None => sustain_level(time) as f32,
        Some(off) => {
            let level = sustain_level(off as f64 / sample_rate);
            // Compute the release delta from integer frame counts so a long
            // held note does not lose release precision to f32 cancellation.
            let delta = age.saturating_sub(off) as f64 / sample_rate;
            if delta >= release {
                0.0
            } else {
                (level * (1.0 - delta / release)) as f32
            }
        }
    }
}

fn linear_balance(pan: f32) -> (f32, f32) {
    if pan >= 0.0 {
        (1.0 - pan, 1.0)
    } else {
        (1.0, 1.0 + pan)
    }
}

fn contains_ascii_case_insensitive(value: &str, needle: &[u8]) -> bool {
    value
        .as_bytes()
        .windows(needle.len())
        .any(|s| s.eq_ignore_ascii_case(needle))
}

#[cfg(test)]
mod tests;
