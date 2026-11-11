//! K2 XML -> playable model. Field/path facts are documented in SPEC.md; this
//! mapping is independent of the binary preset-chunk layout. Unknown group
//! start conditions fail explicitly instead of playing all conditional layers.

use crate::{
    external_mod::{CurvePoint, ExternalModulator, Target, Transformation},
    model::{Cc64Mode, Filter, FilterKind, Group, Instrument, ProgramInfo, VoiceGroups},
    modulator::{Envelope, InternalModulator, Lfo, LfoKind, TargetParameter},
    start_criteria::{self, StartCondition, StartCriterion},
    xml::Element,
    zone::{Loop, Zone},
    Error,
};
use std::collections::HashMap;

struct Values<'a>(Vec<(&'a str, &'a str)>);
impl<'a> Values<'a> {
    fn direct(element: &'a Element) -> Result<Self, Error> {
        let mut values = Vec::new();
        for value in element.children_named("V") {
            if values.len() == 4096 {
                return Err(bad("parameter count exceeds 4096"));
            }
            let name = value.attr("name").ok_or_else(|| bad("V has no name"))?;
            let text = value
                .attr("value")
                .ok_or_else(|| bad(format!("{name} has no value")))?;
            if values.iter().any(|(key, _)| *key == name) {
                return Err(bad(format!("duplicate parameter {name}")));
            }
            values.push((name, text));
        }
        Ok(Self(values))
    }
    fn parameters(element: &'a Element) -> Result<Self, Error> {
        unique_child(element, "Parameters")?.map_or(Ok(Self(Vec::new())), Self::direct)
    }
    fn get(&self, key: &str) -> Option<&'a str> {
        self.0
            .iter()
            .find(|(name, _)| *name == key)
            .map(|(_, value)| *value)
    }
    fn integer(&self, key: &str, default: i64, low: i64, high: i64) -> Result<i64, Error> {
        let value = self.get(key).map_or(Ok(default), |value| {
            value
                .trim()
                .parse::<i64>()
                .map_err(|_| bad(format!("invalid {key}")))
        })?;
        if !(low..=high).contains(&value) {
            return Err(bad(format!("{key} outside {low}..={high}")));
        }
        Ok(value)
    }
    fn number(&self, key: &str, default: f32, low: f32, high: f32) -> Result<f32, Error> {
        let value = self.get(key).map_or(Ok(default), |value| {
            value
                .trim()
                .parse::<f32>()
                .map_err(|_| bad(format!("invalid {key}")))
        })?;
        if !value.is_finite() || value < low || value > high {
            return Err(bad(format!("invalid or out-of-range {key}")));
        }
        Ok(value)
    }
    fn boolean(&self, key: &str, default: bool) -> Result<bool, Error> {
        match self.get(key).map(str::trim) {
            None => Ok(default),
            Some("yes" | "true" | "1") => Ok(true),
            Some("no" | "false" | "0") => Ok(false),
            _ => Err(bad(format!("invalid {key}"))),
        }
    }
    fn tune(&self, key: &str) -> Result<f32, Error> {
        // K2 ratio fields use 1 for unity; old neutral templates also emit 0.
        let value = self.number(key, 1.0, 0.0, 256.0)?;
        if value == 0.0 && key == "tune" {
            Ok(1.0)
        } else if value >= 1.0 / 256.0 {
            Ok(value)
        } else {
            Err(bad(format!("invalid {key} ratio")))
        }
    }
    fn range(&self, low: &str, high: &str) -> Result<(u16, u16), Error> {
        let a = self.integer(low, 0, 0, 127)? as u16;
        let b = self.integer(high, 127, 0, 127)? as u16;
        if a > b {
            return Err(bad(format!("inverted {low}/{high}")));
        }
        Ok((a, b))
    }
}

fn midi_channel(values: &Values<'_>) -> Result<i16, Error> {
    match values.get("midiChannel").map(str::trim) {
        None => Ok(0),
        Some(value) if value.eq_ignore_ascii_case("all") => Ok(0),
        Some(value) => {
            let channel = value
                .parse::<i16>()
                .map_err(|_| bad("invalid midiChannel"))?;
            if !(-1..=16).contains(&channel) {
                return Err(bad("midiChannel outside -1..=16"));
            }
            Ok(channel)
        }
    }
}

fn bad(message: impl std::fmt::Display) -> Error {
    Error::Malformed(format!("K2 XML: {message}"))
}

/// Build one program. Multi-program selection/expansion is deliberately explicit:
/// a bank containing several programs must not silently play only its first slot.
pub fn build(root: &Element) -> Result<Instrument, Error> {
    let mut programs = Vec::new();
    match root.name.as_str() {
        "K2_Container" => collect_programs(root, &mut programs),
        "K2_Bank" => {
            for container in root.children_named("K2_Container") {
                collect_programs(container, &mut programs);
            }
        }
        _ => return Err(bad("expected K2_Container or K2_Bank")),
    }
    if programs.len() != 1 {
        return Err(bad(format!(
            "expected one program, found {}; multi-program playback is not supported",
            programs.len()
        )));
    }
    let program = programs[0];
    let values = Values::parameters(program)?;
    let key_range = values.range("lowKey", "highKey")?;
    let velocity_range = values.range("lowVelocity", "highVelocity")?;
    let info = ProgramInfo {
        name: program.attr("name").unwrap_or("Untitled").into(),
        midi_transpose: values.integer("transpose", 0, -127, 127)? as i32,
        volume: values.number("volume", 1.0, 0.0, 64.0)?,
        pan: values.number("pan", 0.0, -1.0, 1.0)?,
        tune: values.tune("tune")?,
        key_range,
        velocity_range,
        default_key_switch: match values.integer("defaultKeyKeySwitch", -1, -1, 127)? {
            -1 => None,
            key => Some(key as u8),
        },
        cc64_mode: match values.get("cc64Mode") {
            None | Some("cc64_mode_sustain_plus_controller") => Cc64Mode::SustainAndController,
            Some("cc64_mode_sustain_without_controller") => Cc64Mode::SustainOnly,
            Some("cc64_mode_controller_only") => Cc64Mode::ControllerOnly,
            Some(other) => return Err(bad(format!("unsupported cc64Mode {other}"))),
        },
        voice_groups: VoiceGroups::default(),
    };
    let mut groups = Vec::new();
    let mut group_ids = HashMap::new();
    if let Some(list) = unique_child(program, "Groups")? {
        for element in list.children_named("K2_Group") {
            let id = element
                .attr("index")
                .ok_or_else(|| bad("group has no index"))?
                .parse::<u32>()
                .map_err(|_| bad("invalid group index"))?;
            if group_ids.insert(id, groups.len() as u32).is_some() {
                return Err(bad("duplicate group index"));
            }
            let start_criteria = group_start(element)?;
            let p = Values::parameters(element)?;
            let (internal_modulators, envelope, envelope_intensity, one_shot) =
                internal_modulators(element)?;
            groups.push(Group {
                name: element.attr("name").unwrap_or("").into(),
                volume: p.number("volume", 1.0, 0.0, 64.0)?,
                pan: p.number("pan", 0.0, -1.0, 1.0)?,
                tune: p.tune("tune")?,
                key_tracking: p.boolean("keyTracking", true)?,
                reverse: p.boolean("reverse", false)?,
                midi_channel: midi_channel(&p)?,
                voice_group_index: p.integer("voiceGroup", -1, -1, 127)? as i32,
                interp_quality: match p.get("interpQuality") {
                    None | Some("normal") => 0,
                    Some("high") => 1,
                    Some("ultra") => 2,
                    Some(other) => return Err(bad(format!("unsupported interpQuality {other}"))),
                },
                release_trigger: p.boolean("releaseTrigger", false)?,
                release_trigger_monophonic: p.boolean("releaseTriggerNoteMonophonic", false)?,
                release_trigger_counter: p.integer("releaseTriggerCounter", 0, 0, 127)? as i32,
                muted: p.boolean("muted", false)?,
                soloed: p.boolean("soloed", false)?,
                filter: filter(&p)?,
                fx_chain: None,
                fx_amp_split_point: p.integer("fxIdxAmpSplitPoint", 6, 0, 8)? as i32,
                envelope,
                envelope_intensity,
                internal_modulators,
                one_shot,
                name_trigger_heuristics: false,
                start_criteria,
                external_modulators: external_modulators(element)?,
            });
        }
    }
    let mut sample_files = Vec::new();
    let mut sample_ids = HashMap::new();
    let mut zones = Vec::new();
    if let Some(list) = unique_child(program, "Zones")? {
        for element in list.children_named("K2_Zone") {
            let id = element
                .attr("groupIdx")
                .ok_or_else(|| bad("zone has no groupIdx"))?
                .parse::<u32>()
                .map_err(|_| bad("invalid groupIdx"))?;
            let group_index = *group_ids
                .get(&id)
                .ok_or_else(|| bad(format!("zone refers to missing group {id}")))?;
            let p = Values::parameters(element)?;
            let keys = p.range("lowKey", "highKey")?;
            let velocities = p.range("lowVelocity", "highVelocity")?;
            // Preserve the source extents: fades are measured from these
            // edges; program ranges are independent playback gates.
            let (low_key, high_key) = keys;
            let (low_velocity, high_velocity) = velocities;
            let sample_start = p.integer("sampleStart", 0, 0, u32::MAX as i64)? as u32;
            let sample_end = p.integer("sampleEnd", u32::MAX as i64, 0, u32::MAX as i64)? as u32;
            if sample_end < sample_start {
                return Err(bad("sampleEnd must not precede sampleStart"));
            }
            let filename_id = if let Some(sample) = unique_child(element, "Sample")? {
                let sample = Values::direct(sample)?;
                let raw = sample.get("file_ex2").or_else(|| sample.get("file"));
                match raw {
                    Some(raw) if !raw.is_empty() => {
                        let path = decode_sample_path(raw)?;
                        let next = sample_files.len() as u32;
                        Some(*sample_ids.entry(path.clone()).or_insert_with(|| {
                            sample_files.push(path);
                            next
                        }))
                    }
                    _ => return Err(bad("Sample has no file_ex2/file path")),
                }
            } else {
                None
            };
            let mut loops = Vec::new();
            let mut one_shot = false;
            if let Some(list) = unique_child(element, "Loops")? {
                for element in list.children_named("Loop") {
                    let p = Values::direct(element)?;
                    let mode = match p.get("mode").unwrap_or("until_end") {
                        "until_end" => 1,
                        "until_release" => 2,
                        "oneshot" => {
                            one_shot = true;
                            continue;
                        }
                        "off" => continue,
                        other => return Err(bad(format!("unsupported loop mode {other}"))),
                    };
                    let start = p.integer("loopStart", 0, 0, u32::MAX as i64)? as u32;
                    let length = p.integer("loopLength", 0, 0, u32::MAX as i64)? as u32;
                    let count = p.integer("loopCount", 0, 0, u32::MAX as i64)? as u32;
                    let tuning = p.tune("loopTuning")?;
                    let crossfade_length = p.integer("xfadeLength", 0, 0, u32::MAX as i64)? as u32;
                    if length == 0 {
                        continue;
                    }
                    let end = start
                        .checked_add(length)
                        .ok_or_else(|| bad("loop endpoint overflow"))?;
                    if start < sample_start || end as u64 > sample_end as u64 + 1 {
                        return Err(bad("loop outside zone playback range"));
                    }
                    loops.push(Loop {
                        mode,
                        start,
                        length,
                        count,
                        alternating: p.boolean("alternatingLoop", false)?,
                        tuning,
                        crossfade_length,
                    });
                }
            }
            if loops.len() > 1 || (one_shot && !loops.is_empty()) {
                return Err(bad(
                    "multiple active loops or mixed one-shot/loop modes are not supported",
                ));
            }
            zones.push(Zone {
                group_index,
                version: 0,
                sample_start,
                sample_end,
                absolute_end: true,
                one_shot,
                low_velocity,
                high_velocity,
                low_key,
                high_key,
                fade_low_velocity: p.integer("fadeLowVelo", 0, 0, 127)? as u16,
                fade_high_velocity: p.integer("fadeHighVelo", 0, 0, 127)? as u16,
                fade_low_key: p.integer("fadeLowKey", 0, 0, 127)? as u16,
                fade_high_key: p.integer("fadeHighKey", 0, 0, 127)? as u16,
                root_key: p.integer("rootKey", 60, 0, 127)? as u16,
                volume: p.number("zoneVolume", 1.0, 0.0, 64.0)?,
                pan: p.number("zonePan", 0.0, -1.0, 1.0)?,
                tune: p.tune("zoneTune")?,
                filename_id,
                sample_resolution: 0,
                sample_rate: 0,
                channels: 0,
                num_frames: 0,
                root_note: 0,
                tuning: 1.0,
                sample_size: 0,
                loops,
            });
        }
    }
    Ok(Instrument {
        program: info,
        groups,
        fx_chains: Vec::new(),
        zones,
        sample_files,
    })
}

fn group_start(group: &Element) -> Result<Vec<StartCriterion>, Error> {
    let Some(start) = unique_child(group, "GroupStart")? else {
        return Ok(Vec::new());
    };
    if start.children.len() > 4 {
        return Err(bad("GroupStart exceeds four criteria"));
    }
    let mut result = Vec::new();
    for criterion in &start.children {
        if criterion.name != "StartCriteria" {
            return Err(bad("unknown GroupStart child"));
        }
        let p = Values::direct(criterion)?;
        let mode = match p.get("mode") {
            None => 0,
            Some("on_key") => 1,
            Some("on_controller") => 2,
            Some("cycle_round_robin") => 3,
            Some("cycle_random") => 4,
            Some("slice_trigger") => 5,
            Some(other) => return Err(bad(format!("unsupported GroupStart mode {other}"))),
        };
        let legacy_key = p.integer("key", 24, 0, 127)?;
        let key = (
            p.integer("key_min", legacy_key, 0, 127)? as i16,
            p.integer("key_max", legacy_key, 0, 127)? as i16,
        );
        let controller = p.integer("controller", 1, 0, 127)? as i16;
        let cc = (
            p.integer("cc_min", 0, 0, 127)? as i16,
            p.integer("cc_max", 64, 0, 127)? as i16,
        );
        let cycle = p.integer("cycleClass", 0, i32::MIN as i64, i32::MAX as i64)? as i32;
        let condition = start_criteria::condition(mode, key, controller, cc, cycle)?;
        let next = start_criteria::logic(match p.get("nextCriteria") {
            None | Some("and") => 0,
            Some("and_not") => 1,
            Some("or") => 2,
            Some(other) => return Err(bad(format!("unknown nextCriteria {other}"))),
        })?;
        if condition != StartCondition::Always {
            result.push(StartCriterion { condition, next });
        }
    }
    Ok(result)
}

fn unique_child<'a>(element: &'a Element, name: &str) -> Result<Option<&'a Element>, Error> {
    let mut children = element.children.iter().filter(|child| child.name == name);
    let first = children.next();
    if children.next().is_some() {
        return Err(bad(format!("duplicate {name} element")));
    }
    Ok(first)
}

fn collect_programs<'a>(container: &'a Element, programs: &mut Vec<&'a Element>) {
    for list in container.children_named("Programs") {
        programs.extend(list.children_named("K2_Program"));
    }
}

fn filter(values: &Values<'_>) -> Result<Option<Filter>, Error> {
    let Some(cutoff) = values.get("filterCutoff") else {
        return Ok(None);
    };
    if values.boolean("filterBypass", false)? {
        return Ok(None);
    }
    let cutoff = cutoff
        .trim()
        .parse::<f32>()
        .map_err(|_| bad("invalid filterCutoff"))?;
    if !cutoff.is_finite() || !(10.0..=24_000.0).contains(&cutoff) {
        return Err(bad("filterCutoff outside 10..=24000"));
    }
    let q = values.number("filterQ", 0.707, 0.1, 20.0)?;
    let kind = match values.get("filterType").unwrap_or("lowpass") {
        "lowpass" | "lp" => FilterKind::Lowpass,
        "highpass" | "hp" => FilterKind::Highpass,
        other => return Err(bad(format!("unsupported filterType {other}"))),
    };
    Ok(Some(Filter { kind, cutoff, q }))
}

fn internal_modulators(
    group: &Element,
) -> Result<(Vec<InternalModulator>, Option<Envelope>, f32, bool), Error> {
    let mut modulators = Vec::new();
    let mut amplitude = None;
    let mut amplitude_intensity = 1.0;
    let mut one_shot = false;
    if let Some(list) = unique_child(group, "IntModulators")? {
        for element in list.children_named("K2_IntMod") {
            if Values::direct(element)?.boolean("bypass", false)? {
                continue;
            }
            let mut targets = Vec::new();
            let mut volume_intensity = None;
            if let Some(list) = element.child("Targets") {
                for target in list.children_named("Target") {
                    let target = Values::direct(target)?;
                    let Some(name) = target.get("target") else {
                        continue;
                    };
                    let intensity = target.number("intensity", 1.0, -64.0, 64.0)?;
                    if name == "volume" && intensity != 0.0 && volume_intensity.is_none() {
                        volume_intensity = Some(intensity);
                    }
                    targets.push(TargetParameter {
                        name: name.into(),
                        intensity,
                    });
                }
            }
            if let Some(envelope) = element.child("Envelope") {
                let ahd_only = match envelope.attr("type").unwrap_or("ahdsr") {
                    "ahdsr" => false,
                    "ahd" => true,
                    other => return Err(bad(format!("unsupported amplitude envelope {other}"))),
                };
                let p = Values::direct(envelope)?;
                let note_off_less = p.boolean("noteOffLessMode", false)?;
                let envelope = Envelope {
                    attack: p.number("attack", 0.0, 0.0, 3_600_000.0)?,
                    hold: p.number("hold", 0.0, 0.0, 3_600_000.0)?,
                    decay: p.number("decay", 0.0, 0.0, 3_600_000.0)?,
                    release: p.number("release", 50.0, 0.0, 3_600_000.0)?,
                    sustain: p.number("sustain", 1.0, 0.0, 1.0)?,
                    curve: p.number("atkCurving", 0.0, -1.0, 1.0)?,
                    ahd_only,
                };
                if let Some(intensity) = volume_intensity {
                    if amplitude.is_some() {
                        return Err(bad("multiple amplitude envelopes are not supported"));
                    }
                    amplitude = Some(envelope.clone());
                    amplitude_intensity = intensity;
                    one_shot = note_off_less;
                }
                modulators.push(InternalModulator {
                    parameters: targets,
                    source_name: "ENV_AHDSR".into(),
                    source_index: 0,
                    envelope: Some(envelope),
                    lfo: None,
                });
            } else if let Some(lfo) = element.child("LFO") {
                let p = Values::direct(lfo)?;
                let kind = match lfo.attr("type").or_else(|| p.get("type")).unwrap_or("sine") {
                    "sine" => LfoKind::Sine,
                    "rectangle" => LfoKind::Rectangle,
                    "triangle" => LfoKind::Triangle,
                    "sawtooth" => LfoKind::Sawtooth,
                    "random" => LfoKind::Random,
                    "multi" => LfoKind::Multi,
                    other => return Err(bad(format!("unsupported LFO type {other}"))),
                };
                let lfo = Lfo {
                    kind,
                    delay: p.number("delay", 0.0, 0.0, 3_600_000.0)?,
                    frequency: p.number("frequency", 1.0, 0.0, 20_000.0)?,
                    pulse_width: p.number("pulseWidth", 0.5, 0.0, 1.0)?,
                    start_phase: p.number("startPhase", 0.0, 0.0, 1.0)?,
                    normalize: p.boolean("normalizeMultiLFO", false)?,
                };
                modulators.push(InternalModulator {
                    parameters: targets,
                    source_name: "LFO".into(),
                    source_index: 1,
                    envelope: None,
                    lfo: Some(lfo),
                });
            }
        }
    }
    Ok((modulators, amplitude, amplitude_intensity, one_shot))
}

fn external_modulators(group: &Element) -> Result<Vec<ExternalModulator>, Error> {
    let mut modulators = Vec::new();
    let Some(list) = unique_child(group, "ExtModulators")? else {
        return Ok(modulators);
    };
    let mut indices = Vec::new();
    for element in list.children_named("K2_ExtMod") {
        if modulators.len() == 64 {
            return Err(bad("external modulator count exceeds 64"));
        }
        if let Some(index) = element.attr("index") {
            let index = index
                .parse::<u8>()
                .map_err(|_| bad("invalid external modulator index"))?;
            if index >= 64 || indices.contains(&index) {
                return Err(bad("duplicate or out-of-range external modulator index"));
            }
            indices.push(index);
        }
        let p = Values::direct(element)?;
        let version_text = p
            .get("version")
            .or_else(|| element.attr("version"))
            .unwrap_or("1.0");
        if p.get("version")
            .zip(element.attr("version"))
            .is_some_and(|(a, b)| a != b)
        {
            return Err(bad("conflicting external modulation versions"));
        }
        let (major, minor) = version_text
            .split_once('.')
            .ok_or_else(|| bad("invalid external modulation version"))?;
        let major = u8::from_str_radix(major, 16)
            .map_err(|_| bad("invalid external modulation version"))?;
        let minor = u8::from_str_radix(minor, 16)
            .map_err(|_| bad("invalid external modulation version"))?;
        let version = (major as u16) << 8 | minor as u16;
        if !(0x100..=0x105).contains(&version) {
            return Err(bad(
                "unsupported external modulation version; legacy migration is not implemented",
            ));
        }
        let source = match p.get("source") {
            None => 0,
            Some("pitchBend") => 1,
            Some("polyAfterTouch") => 2,
            Some("monoAfterTouch") => 3,
            Some("midiCC") => 4,
            Some("keyPos") => 7,
            Some("velocity") => 8,
            Some("releaseVelocity") => 9,
            Some("rlsTrigCounter") => 10,
            Some("constVal") => 11,
            Some("randomValUni") => 12,
            Some("randomValBi") => 13,
            Some(other) => {
                return Err(bad(format!(
                    "unsupported external modulation source {other}"
                )))
            }
        };
        let mut targets = Vec::new();
        let mut target_indices = Vec::new();
        if let Some(list) = unique_child(element, "Targets")? {
            for element in list.children_named("Target") {
                if targets.len() == 16 {
                    return Err(bad("external modulation target count exceeds 16"));
                }
                if let Some(index) = element.attr("index") {
                    let index = index
                        .parse::<u8>()
                        .map_err(|_| bad("invalid modulation target index"))?;
                    if index >= 16 || target_indices.contains(&index) {
                        return Err(bad("duplicate or out-of-range modulation target index"));
                    }
                    target_indices.push(index);
                }
                let p = Values::direct(element)?;
                let name = p
                    .get("target")
                    .ok_or_else(|| bad("modulation target has no name"))?;
                let limit = if name == "filterCutoff" { 16.0 } else { 1.0 };
                targets.push(Target {
                    name: name.into(),
                    intensity: p.number("intensity", 1.0, -limit, limit)?,
                    slot_idx: p.integer("slotIdx", -1, -1, i16::MAX as i64)? as i16,
                    flags: p.integer("flags", 0, 0, 255)? as u8,
                    smoothing: p.integer("smoothingCoef", 0, i16::MIN as i64, i16::MAX as i64)?
                        as i16,
                    target_obj_idx: p
                        .get("targetObjIdx")
                        .map(|_| p.integer("targetObjIdx", -1, -1, 127).map(|v| v as i8))
                        .transpose()?,
                    display_name: p.get("name").unwrap_or("").into(),
                    table_open: p.boolean("tableOpen", false)?,
                    transform: external_transform(&p)?,
                });
            }
        }
        modulators.push(ExternalModulator {
            version,
            kind: match p.get("type").unwrap_or("extMod") {
                "extMod" => 1,
                "zenvMod" => 2,
                other => return Err(bad(format!("unsupported external modulation type {other}"))),
            },
            source,
            cc_number: p.integer("ccNumber", 0, 0, 127)? as u8,
            initial_value: p.integer("initialValue", 0, -1, 127)? as i16,
            bypass: p.boolean("bypass", false)?,
            simulate_k2: p.boolean("simulateK2EnvTimeModulation", false)?,
            class_id: p.integer("classID", -1, -1, i32::MAX as i64)? as i32,
            name: p.get("name").unwrap_or("").into(),
            targets,
            ..Default::default()
        });
    }
    Ok(modulators)
}

fn external_transform(p: &Values<'_>) -> Result<Transformation, Error> {
    let table = p.0.iter().any(|(key, _)| {
        key.strip_prefix('t')
            .is_some_and(|suffix| suffix.parse::<usize>().is_ok())
    });
    let curve = p.0.iter().any(|(key, _)| {
        key.strip_prefix(['x', 'y', 'c'])
            .is_some_and(|suffix| suffix.parse::<usize>().is_ok())
    });
    if table && curve {
        return Err(bad("mixed external modulation table/curve"));
    }
    if table {
        let mut values = Box::new(std::array::from_fn(|index| index as f32 / 128.0));
        let mut previous = None;
        for (key, _) in &p.0 {
            if let Some(index) = key.strip_prefix('t').and_then(|v| v.parse::<usize>().ok()) {
                if index >= 128 || previous.is_some_and(|before| before >= index) {
                    return Err(bad("modulation table indices must increase within 0..127"));
                }
                let value = p.number(key, 0.0, -16.0, 16.0)?;
                if let Some(before) = previous {
                    let start = values[before];
                    for i in before + 1..index {
                        values[i] =
                            start + (value - start) * (i - before) as f32 / (index - before) as f32;
                    }
                }
                values[index] = value;
                previous = Some(index);
            }
        }
        return Ok(Transformation::Table {
            active: p.boolean("tableActive", true)?,
            values,
        });
    }
    if curve {
        let mut points = Vec::new();
        for index in 0..32 {
            let key = format!("x{index}");
            if p.get(&key).is_none() {
                break;
            }
            let x = p.number(&key, 0.0, 0.0, 1.0)?;
            let ykey = format!("y{index}");
            if p.get(&ykey).is_none()
                || points.last().is_some_and(|point: &CurvePoint| point.x >= x)
            {
                return Err(bad("invalid external modulation curve point"));
            }
            points.push(CurvePoint {
                x,
                y: p.number(&ykey, 0.0, -16.0, 16.0)?,
                curvature: p.number(&format!("c{index}"), 0.0, -1.0, 1.0)?,
            });
        }
        for (key, _) in &p.0 {
            if let Some(index) = key
                .strip_prefix(['x', 'y', 'c'])
                .and_then(|v| v.parse::<usize>().ok())
            {
                if index >= points.len() {
                    return Err(bad("sparse or out-of-range external curve points"));
                }
            }
        }
        if points.len() < 2 {
            return Err(bad(
                "external modulation curve requires at least two points",
            ));
        }
        return Ok(Transformation::Curve {
            active: p.boolean("curveActive", true)?,
            points,
        });
    }
    Ok(Transformation::None)
}

/// Decode NI's @v/@b/@d/@f path grammar, without filesystem access. Library
/// references are explicit errors; they must not become ordinary file paths.
pub fn decode_sample_path(raw: &str) -> Result<String, Error> {
    let mut path = if let Some(encoded) = raw.strip_prefix('@') {
        let units: Vec<u16> = encoded.encode_utf16().collect();
        let mut pos = 0;
        let mut out = String::new();
        loop {
            let tag = *units
                .get(pos)
                .ok_or_else(|| bad("encoded sample path has no filename"))?;
            pos += 1;
            match tag {
                118 | 100 => {
                    let digits = units
                        .get(pos..pos + 3)
                        .ok_or_else(|| bad("truncated path length"))?;
                    if digits.iter().any(|d| !(48..=57).contains(d)) {
                        return Err(bad("invalid path length"));
                    }
                    let len = digits
                        .iter()
                        .fold(0usize, |n, d| n * 10 + (*d - 48) as usize);
                    pos += 3;
                    let folder = String::from_utf16(
                        units
                            .get(pos..pos + len)
                            .ok_or_else(|| bad("truncated path folder"))?,
                    )
                    .map_err(|_| bad("invalid path Unicode"))?;
                    pos += len;
                    if tag == 118 && !folder.contains(':') {
                        out.push('/');
                    }
                    out.push_str(&folder);
                    out.push('/');
                }
                98 => out.push_str("../"),
                109 => return Err(bad("NKS library sample paths are not supported")),
                102 | 70 => {
                    pos += if tag == 102 { 6 } else { 11 };
                    let file = units
                        .get(pos..)
                        .filter(|file| !file.is_empty())
                        .ok_or_else(|| bad("truncated path filename"))?;
                    out.push_str(
                        &String::from_utf16(file).map_err(|_| bad("invalid filename Unicode"))?,
                    );
                    break out;
                }
                _ => return Err(bad("unknown encoded path component")),
            }
        }
    } else {
        raw.to_owned()
    };
    if raw.starts_with('@') {
        let units: Vec<u16> = path.encode_utf16().collect();
        let mut decoded = Vec::new();
        let mut pos = 0;
        while pos < units.len() {
            if units[pos] == 37 && pos + 4 < units.len() {
                let digits = &units[pos + 1..pos + 5];
                let value = digits.iter().try_fold(0u16, |n, d| {
                    char::from_u32(*d as u32)?
                        .to_digit(16)
                        .map(|v| n * 16 + v as u16)
                });
                if let Some(value) = value {
                    decoded.push(value);
                    pos += 5;
                    continue;
                }
            }
            decoded.push(units[pos]);
            pos += 1;
        }
        path = String::from_utf16(&decoded).map_err(|_| bad("invalid escaped filename Unicode"))?;
    }
    if path.is_empty() || path.contains('\0') || path.ends_with(['/', '\\']) {
        return Err(bad("invalid sample filename"));
    }
    Ok(path.replace('\\', "/"))
}
