//! External MIDI modulation metadata from Kontakt's type-0x0c objects.
//!
//! Wire ordering and version gates come from the saved Kontakt 8 serializers;
//! see `docs/kontakt-external-mod-wire-evidence.md`. Parsing is independent of
//! playback support: effect/modulator targets and inactive transformations are
//! retained for later engine stages.

use crate::{preset_chunk::PresetChunk, Error};

pub const MAX_MODULATORS: usize = 64;
pub const MAX_TARGETS: usize = 16;
const MAX_STRING_BYTES: usize = 4096;
const MAX_CURVE_POINTS: usize = 32;

#[derive(Debug, Clone, PartialEq)]
pub struct ExternalModulator {
    pub version: u16,
    /// 0 = none, 1 = external source, 2 = zone-envelope source.
    pub kind: i32,
    /// Kontakt runtime source enum, after decoding its serialized mapping.
    pub source: u32,
    pub cc_number: u8,
    pub source_index: Option<i32>,
    pub initial_value: i16,
    pub bypass: bool,
    pub class_id: i32,
    pub simulate_k2: bool,
    /// Retained opaque flags at native offsets +0x4c8 and +0x4c9.
    pub flag_v103: bool,
    pub flag_v104: bool,
    pub name: String,
    pub targets: Vec<Target>,
}

impl Default for ExternalModulator {
    fn default() -> Self {
        Self {
            version: 0x105,
            kind: 1,
            source: 0,
            cc_number: 0,
            source_index: None,
            initial_value: 0,
            bypass: false,
            class_id: -1,
            simulate_k2: false,
            flag_v103: true,
            flag_v104: true,
            name: String::new(),
            targets: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Target {
    pub name: String,
    pub intensity: f32,
    pub slot_idx: i16,
    pub flags: u8,
    pub smoothing: i16,
    pub target_obj_idx: Option<i8>,
    pub display_name: String,
    pub table_open: bool,
    pub transform: Transformation,
}

impl Default for Target {
    fn default() -> Self {
        Self {
            name: String::new(),
            intensity: 1.0,
            slot_idx: -1,
            flags: 0,
            smoothing: 0,
            target_obj_idx: None,
            display_name: String::new(),
            table_open: false,
            transform: Transformation::None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Default)]
pub enum Transformation {
    #[default]
    None,
    Table {
        active: bool,
        values: Box<[f32; 128]>,
    },
    Curve {
        active: bool,
        points: Vec<CurvePoint>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurvePoint {
    pub x: f32,
    pub y: f32,
    pub curvature: f32,
}

impl ExternalModulator {
    pub fn parse(chunk: &PresetChunk) -> Result<Self, Error> {
        if chunk.id != crate::preset_chunk::ID_PAR_EXTERNAL_MOD {
            return Err(bad("expected PAR_EXTERNAL_MOD"));
        }
        if let Some(version) = chunk.version {
            if !chunk.public_data.is_empty() || !chunk.children.is_empty() {
                return Err(bad("unexpected external modulation public/child data"));
            }
            return Self::parse_private(version, &chunk.private_data);
        }
        // The existing preset-tree parser deliberately retains these objects
        // opaquely. Decode the normal serializable wrapper without changing it.
        let mut r = Reader::new(&chunk.public_data);
        if r.u8()? != 1 {
            return Err(bad("expected structured external modulator"));
        }
        let version = r.u16()?;
        let private = r.block()?;
        if !r.block()?.is_empty() || !r.block()?.is_empty() {
            return Err(bad("unexpected external modulation public/child data"));
        }
        r.finish()?;
        Self::parse_private(version, private)
    }

    fn parse_private(version: u16, data: &[u8]) -> Result<Self, Error> {
        if !(0x100..=0x105).contains(&version) {
            return Err(bad(format!(
                "unsupported external modulator version {version:#x}"
            )));
        }
        let mut r = Reader::new(data);
        let count = r.i32()?;
        if count < 0 || count as usize > MAX_TARGETS {
            return Err(bad("external modulator target count exceeds 16"));
        }
        let mut targets = Vec::with_capacity(count as usize);
        for _ in 0..count {
            let name = r.string()?;
            let intensity = r.f32()?;
            let slot_idx = r.i16()?;
            let flags = r.u8()?;
            let smoothing = r.i16()?;
            let display_name = r.string()?;
            let target_obj_idx = if target_has_object_index(&name) {
                Some(r.u8()? as i8)
            } else {
                None
            };
            let table_open = r.boolean()?;
            targets.push(Target {
                name,
                intensity,
                slot_idx,
                flags,
                smoothing,
                target_obj_idx,
                display_name,
                table_open,
                transform: Transformation::None,
            });
        }
        // Native serialization writes every target's base metadata first,
        // followed by all transformations; they are not interleaved.
        for target in &mut targets {
            target.transform = match r.u8()? {
                0 => Transformation::None,
                1 => {
                    let active = r.boolean()?;
                    let mut values = Box::new([0.0; 128]);
                    for value in values.iter_mut() {
                        *value = r.f32()?;
                    }
                    Transformation::Table { active, values }
                }
                2 => {
                    let active = r.boolean()?;
                    let count = r.u8()? as i8;
                    if count < 0 || count as usize > MAX_CURVE_POINTS {
                        return Err(bad("external modulation curve exceeds 32 points"));
                    }
                    let mut points = Vec::with_capacity(count as usize);
                    for _ in 0..count {
                        points.push(CurvePoint {
                            x: r.f32()?,
                            y: r.f32()?,
                            curvature: r.f32()?,
                        });
                    }
                    Transformation::Curve { active, points }
                }
                other => return Err(bad(format!("unknown modulation transformation {other}"))),
            };
        }
        let name = r.string()?;
        let kind = r.i32()?;
        if !(0..=2).contains(&kind) {
            return Err(bad("invalid external modulation kind"));
        }
        let mut result = Self {
            version,
            name,
            kind,
            targets,
            ..Self::default()
        };
        if kind == 1 {
            // These are not identity mappings after serialized source 4.
            const SOURCES: [u32; 19] = [
                0, 1, 2, 3, 4, 7, 8, 9, 10, 11, 12, 13, 5, 16, 17, 18, 19, 20, 21,
            ];
            let source = r.i32()?;
            result.source = *SOURCES
                .get(source as usize)
                .ok_or_else(|| bad("unknown external modulation source"))?;
            if result.source == 4 {
                result.cc_number = r.u8()?;
                if result.cc_number > 127 {
                    return Err(bad("external modulation CC exceeds 127"));
                }
            }
            if result.source == 5 && version >= 0x102 {
                result.source_index = Some(r.i32()?);
            }
            result.initial_value = r.i16()?;
        }
        result.bypass = r.boolean()?;
        result.simulate_k2 = r.boolean()?;
        result.class_id = r.i32()?;
        if version >= 0x103 {
            result.flag_v103 = r.boolean()?;
        }
        if version >= 0x104 {
            result.flag_v104 = r.boolean()?;
        }
        r.finish()?;
        Ok(result)
    }
}

// Populated below from BModTargetBase::staticInit and native effect/modulator
// classification. Unknown names map to native target 0 (no object-index byte).
fn target_has_object_index(name: &str) -> bool {
    matches!(
        name,
        "filterCutoff"
            | "filterQ"
            | "filterBandwidth"
            | "formantTalk"
            | "formantSize"
            | "formantSharp"
            | "cutoffLP"
            | "cutoffHP"
            | "resoLP"
            | "resoHP"
            | "filterFeedback"
            | "filterFM"
            | "intensity"
            | "frequency"
            | "pulseWidth"
            | "startPhase"
            | "eqFreq1"
            | "eqBandWidth1"
            | "eqGain1"
            | "eqFreq2"
            | "eqBandWidth2"
            | "eqGain2"
            | "eqFreq3"
            | "eqBandWidth3"
            | "eqGain3"
            | "vfShift"
            | "vfType"
            | "vfBypass"
            | "sendLevel0"
            | "sendLevel1"
            | "sendLevel2"
            | "sendLevel3"
            | "sendLevel4"
            | "sendLevel5"
            | "sendLevel6"
            | "sendLevel7"
            | "scLevelB00"
            | "scLevelB01"
            | "scLevelB02"
            | "scLevelB03"
            | "scLevelB04"
            | "scLevelB05"
            | "scLevelB06"
            | "scLevelB07"
            | "scLevelB08"
            | "scLevelB09"
            | "scLevelB10"
            | "scLevelB11"
            | "scLevelB12"
            | "scLevelB13"
            | "scLevelB14"
            | "scLevelB15"
            | "scLevelPGM"
            | "distortionIntensity"
            | "crywahPedal"
            | "dyx_morph"
            | "dyx_dry"
            | "skr_tone"
            | "skr_drive"
            | "sslgeq_hp_freq"
            | "sslgeq_lf_gain"
            | "sslgeq_lf_freq"
            | "sslgeq_lmf_gain"
            | "sslgeq_lmf_freq"
            | "sslgeq_lmf_q"
            | "sslgeq_hmf_gain"
            | "sslgeq_hmf_freq"
            | "sslgeq_hmf_q"
            | "sslgeq_hf_gain"
            | "sslgeq_hf_freq"
            | "sslgeq_lp_freq"
            | "tape_gain"
            | "tape_warmth"
            | "tape_hfrolloff"
            | "stereoSpreadIntensity"
            | "stereoPan"
            | "shaper"
            | "bitdepth"
            | "downsample"
            | "noiselevel"
            | "noisecolor"
            | "off_distance"
            | "off_azimuth"
            | "off_x"
            | "off_y"
            | "ahdsr_attack"
            | "ahdsr_hold"
            | "ahdsr_decay"
            | "ahdsr_sustain"
            | "ahdsr_release"
            | "dbd_decay1"
            | "dbd_break"
            | "dbd_decay2"
            | "fm7_level1"
            | "fm7_level2"
            | "fm7_level3"
            | "fm7_level4"
            | "fm7_level5"
            | "fm7_level6"
            | "fm7_level7"
            | "fm7_level8"
            | "fm7_time1"
            | "fm7_time2"
            | "fm7_time3"
            | "fm7_time4"
            | "fm7_time5"
            | "fm7_time6"
            | "fm7_time7"
            | "fm7_time8"
            | "fm7_slope1"
            | "fm7_slope2"
            | "fm7_slope3"
            | "fm7_slope4"
            | "fm7_slope5"
            | "fm7_slope6"
            | "fm7_slope7"
            | "fm7_slope8"
            | "arp32step_v1"
            | "arp32step_v2"
            | "arp32step_v3"
            | "arp32step_v4"
            | "arp32step_v5"
            | "arp32step_v6"
            | "arp32step_v7"
            | "arp32step_v8"
            | "arp32step_v9"
            | "arp32step_v10"
            | "arp32step_v11"
            | "arp32step_v12"
            | "arp32step_v13"
            | "arp32step_v14"
            | "arp32step_v15"
            | "arp32step_v16"
            | "arp32step_v17"
            | "arp32step_v18"
            | "arp32step_v19"
            | "arp32step_v20"
            | "arp32step_v21"
            | "arp32step_v22"
            | "arp32step_v23"
            | "arp32step_v24"
            | "arp32step_v25"
            | "arp32step_v26"
            | "arp32step_v27"
            | "arp32step_v28"
            | "arp32step_v29"
            | "arp32step_v30"
            | "arp32step_v31"
            | "arp32step_v32"
            | "intensity_nc"
            | "intensity_upper"
            | "sendLevelDelay"
            | "sendLevelChorus"
            | "sendLevelFlanger"
            | "sendLevelPhaser"
            | "sendLevelReverb"
    )
}

fn bad(message: impl Into<String>) -> Error {
    Error::Malformed(format!("external modulation: {}", message.into()))
}

struct Reader<'a> {
    data: &'a [u8],
    position: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, position: 0 }
    }
    fn take(&mut self, count: usize) -> Result<&'a [u8], Error> {
        let end = self
            .position
            .checked_add(count)
            .filter(|end| *end <= self.data.len())
            .ok_or_else(|| bad("truncated data"))?;
        let result = &self.data[self.position..end];
        self.position = end;
        Ok(result)
    }
    fn u8(&mut self) -> Result<u8, Error> {
        Ok(self.take(1)?[0])
    }
    fn boolean(&mut self) -> Result<bool, Error> {
        match self.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(bad("invalid boolean")),
        }
    }
    fn u16(&mut self) -> Result<u16, Error> {
        Ok(u16::from_le_bytes(self.take(2)?.try_into().unwrap()))
    }
    fn i16(&mut self) -> Result<i16, Error> {
        Ok(self.u16()? as i16)
    }
    fn u32(&mut self) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.take(4)?.try_into().unwrap()))
    }
    fn i32(&mut self) -> Result<i32, Error> {
        Ok(self.u32()? as i32)
    }
    fn f32(&mut self) -> Result<f32, Error> {
        let result = f32::from_bits(self.u32()?);
        if !result.is_finite() {
            return Err(bad("non-finite modulation value"));
        }
        Ok(result)
    }
    fn block(&mut self) -> Result<&'a [u8], Error> {
        let count = self.u32()? as usize;
        self.take(count)
    }
    fn string(&mut self) -> Result<String, Error> {
        let count = self.u32()? as usize;
        if count > MAX_STRING_BYTES {
            return Err(bad(
                "modulation string exceeds 4096 bytes or uses legacy negative sentinel",
            ));
        }
        let bytes = self.take(count)?;
        if bytes.contains(&0) {
            return Err(bad("embedded NUL in modulation string"));
        }
        String::from_utf8(bytes.to_vec()).map_err(|_| bad("invalid modulation UTF-8"))
    }
    fn finish(&self) -> Result<(), Error> {
        if self.position != self.data.len() {
            return Err(bad("trailing data"));
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn string(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(&(value.len() as u32).to_le_bytes());
        bytes.extend_from_slice(value.as_bytes());
    }

    fn target(bytes: &mut Vec<u8>, name: &str, depth: f32, object: Option<i8>) {
        string(bytes, name);
        bytes.extend_from_slice(&depth.to_le_bytes());
        bytes.extend_from_slice(&(-1i16).to_le_bytes());
        bytes.push(0x12);
        bytes.extend_from_slice(&23i16.to_le_bytes());
        string(bytes, "route label");
        if let Some(object) = object {
            bytes.push(object as u8);
        }
        bytes.push(1);
    }

    fn suffix(bytes: &mut Vec<u8>, version: u16, kind: i32, source: i32) {
        string(bytes, "source label");
        bytes.extend_from_slice(&kind.to_le_bytes());
        if kind == 1 {
            bytes.extend_from_slice(&source.to_le_bytes());
            if source == 4 {
                bytes.push(11);
            }
            if source == 12 && version >= 0x102 {
                bytes.extend_from_slice(&7i32.to_le_bytes());
            }
            bytes.extend_from_slice(&(-1i16).to_le_bytes());
        }
        bytes.extend_from_slice(&[0, 1]);
        bytes.extend_from_slice(&(-1i32).to_le_bytes());
        if version >= 0x103 {
            bytes.push(0);
        }
        if version >= 0x104 {
            bytes.push(1);
        }
    }

    fn object(version: u16, private: &[u8]) -> PresetChunk {
        let mut object = vec![1];
        object.extend_from_slice(&version.to_le_bytes());
        object.extend_from_slice(&(private.len() as u32).to_le_bytes());
        object.extend_from_slice(private);
        object.extend_from_slice(&[0; 8]);
        let mut wire = (crate::preset_chunk::ID_PAR_EXTERNAL_MOD as u16)
            .to_le_bytes()
            .to_vec();
        wire.extend_from_slice(&(object.len() as u32).to_le_bytes());
        wire.extend(object);
        crate::preset_chunk::parse_all(&wire).unwrap().remove(0)
    }

    fn one_target(version: u16, source: i32) -> PresetChunk {
        let mut bytes = 1i32.to_le_bytes().to_vec();
        target(&mut bytes, "pitch", 0.25, None);
        bytes.push(0);
        suffix(&mut bytes, version, 1, source);
        object(version, &bytes)
    }

    #[test]
    fn private_versions_and_source_mapping_preserve_midi_route_metadata() {
        for version in 0x100..=0x105 {
            for (wire, runtime) in [
                (1, 1),
                (2, 2),
                (3, 3),
                (4, 4),
                (5, 7),
                (6, 8),
                (7, 9),
                (12, 5),
            ] {
                let parsed = ExternalModulator::parse(&one_target(version, wire)).unwrap();
                assert_eq!(parsed.version, version);
                assert_eq!(parsed.source, runtime);
                assert_eq!(parsed.initial_value, -1);
                assert!(!parsed.bypass);
                assert!(parsed.simulate_k2);
                assert_eq!(parsed.class_id, -1);
                assert_eq!(parsed.flag_v103, version < 0x103);
                assert!(parsed.flag_v104);
                assert_eq!(parsed.name, "source label");
                assert_eq!(parsed.cc_number, if wire == 4 { 11 } else { 0 });
                assert_eq!(
                    parsed.source_index,
                    if wire == 12 && version >= 0x102 {
                        Some(7)
                    } else {
                        None
                    }
                );
                let route = &parsed.targets[0];
                assert_eq!(route.name, "pitch");
                assert_eq!(route.intensity, 0.25);
                assert_eq!(route.slot_idx, -1);
                assert_eq!(route.flags, 0x12);
                assert_eq!(route.smoothing, 23);
                assert_eq!(route.display_name, "route label");
                assert!(route.table_open);
                assert_eq!(route.transform, Transformation::None);
            }
        }
    }

    #[test]
    fn transforms_follow_all_metadata_and_keep_effect_object_indices() {
        let mut bytes = 2i32.to_le_bytes().to_vec();
        target(&mut bytes, "volume", -0.5, None);
        target(&mut bytes, "filterCutoff", 0.75, Some(3));
        bytes.extend_from_slice(&[1, 0]);
        for value in 0..128 {
            bytes.extend_from_slice(&(value as f32 / 127.0).to_le_bytes());
        }
        bytes.extend_from_slice(&[2, 1, 2]);
        for point in [[0.0f32, -1.0, 0.25], [1.0, 1.0, -0.5]] {
            for value in point {
                bytes.extend_from_slice(&value.to_le_bytes());
            }
        }
        suffix(&mut bytes, 0x105, 1, 4);
        let parsed = ExternalModulator::parse(&object(0x105, &bytes)).unwrap();
        assert_eq!(parsed.targets[1].target_obj_idx, Some(3));
        match &parsed.targets[0].transform {
            Transformation::Table { active, values } => {
                assert!(!active);
                assert_eq!(values[0], 0.0);
                assert_eq!(values[127], 1.0);
            }
            _ => panic!("missing table"),
        }
        match &parsed.targets[1].transform {
            Transformation::Curve { active, points } => {
                assert!(*active);
                assert_eq!(points.len(), 2);
                assert_eq!(
                    points[0],
                    CurvePoint {
                        x: 0.0,
                        y: -1.0,
                        curvature: 0.25
                    }
                );
                assert_eq!(points[1].curvature, -0.5);
            }
            _ => panic!("missing curve"),
        }
    }

    #[test]
    fn zone_envelope_kind_does_not_consume_external_source_fields() {
        let mut bytes = 0i32.to_le_bytes().to_vec();
        suffix(&mut bytes, 0x105, 2, 0);
        let parsed = ExternalModulator::parse(&object(0x105, &bytes)).unwrap();
        assert_eq!(parsed.kind, 2);
        assert_eq!(parsed.source, 0);
        assert!(parsed.targets.is_empty());
    }

    #[test]
    fn truncated_wrappers_and_private_data_are_explicit_errors() {
        let full = one_target(0x105, 4);
        for end in 0..full.public_data.len() {
            let mut truncated = full.clone();
            truncated.public_data.truncate(end);
            assert!(
                ExternalModulator::parse(&truncated).is_err(),
                "accepted {end}"
            );
        }
        let private_len = u32::from_le_bytes(full.public_data[3..7].try_into().unwrap()) as usize;
        let private = &full.public_data[7..7 + private_len];
        for end in 0..private.len() {
            assert!(
                ExternalModulator::parse(&object(0x105, &private[..end])).is_err(),
                "accepted private prefix {end}"
            );
        }
    }

    #[test]
    fn unsupported_versions_counts_transforms_and_nonfinite_depths_fail() {
        let full = one_target(0x105, 1);
        for version in [0, 0xff, 0x106, u16::MAX] {
            let mut invalid = full.clone();
            invalid.public_data[1..3].copy_from_slice(&version.to_le_bytes());
            assert!(ExternalModulator::parse(&invalid).is_err());
        }
        for count in [-1i32, 17, i32::MAX] {
            let mut invalid = full.clone();
            invalid.public_data[7..11].copy_from_slice(&count.to_le_bytes());
            assert!(ExternalModulator::parse(&invalid).is_err());
        }
        let mut bytes = 1i32.to_le_bytes().to_vec();
        target(&mut bytes, "pitch", f32::NAN, None);
        bytes.push(0);
        suffix(&mut bytes, 0x105, 1, 1);
        assert!(ExternalModulator::parse(&object(0x105, &bytes)).is_err());
        for transform in [vec![3], vec![2, 1, 33], vec![2, 1, 255]] {
            let mut bytes = 1i32.to_le_bytes().to_vec();
            target(&mut bytes, "pitch", 0.5, None);
            bytes.extend(transform);
            suffix(&mut bytes, 0x105, 1, 1);
            assert!(ExternalModulator::parse(&object(0x105, &bytes)).is_err());
        }
    }
}
