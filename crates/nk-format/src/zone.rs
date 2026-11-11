//! Zone parameter decoding for Kontakt 5+ programs.
//!
//! A zone is a child of `ZONE_LIST`; its chunk id is the group index and its
//! public data is the parameter blob decoded here. Layout recovered from
//! ConvertWithMoss `Zone.java`; versions `0x93`, `0x98`, `0x99`, `0x9A..=0x9C`
//! are recognised.

use crate::{preset_chunk, Error};

#[derive(Debug, Clone, PartialEq)]
pub struct Loop {
    pub mode: i32,
    pub start: u32,
    pub length: u32,
    pub count: u32,
    pub alternating: bool,
    pub tuning: f32,
    pub crossfade_length: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Zone {
    pub group_index: u32,
    pub version: u16,
    pub sample_start: u32,
    pub sample_end: u32,
    /// XML stores an absolute inclusive frame endpoint; preset chunks store
    /// a frame count trimmed from the end. XML's missing endpoint is u32::MAX.
    pub absolute_end: bool,
    pub one_shot: bool,
    pub low_velocity: u16,
    pub high_velocity: u16,
    pub low_key: u16,
    pub high_key: u16,
    pub fade_low_velocity: u16,
    pub fade_high_velocity: u16,
    pub fade_low_key: u16,
    pub fade_high_key: u16,
    pub root_key: u16,
    pub volume: f32,
    pub pan: f32,
    pub tune: f32,
    pub filename_id: Option<u32>,
    pub sample_resolution: u32,
    pub sample_rate: u32,
    pub channels: u8,
    pub num_frames: u32,
    pub root_note: u32,
    pub tuning: f32,
    pub sample_size: u32,
    pub loops: Vec<Loop>,
}

impl Zone {
    /// Selection and launch gain for a mapped MIDI note and attack velocity.
    ///
    /// Kontakt's zone fades occupy the inside of the inclusive key/velocity
    /// ranges. An edge width `n` uses the `n` interior MIDI positions nearest
    /// that edge, with gains `(distance + 1) / (n + 1)`. All four edge factors
    /// multiply; overlapping fades on one zone are not normalized or replaced
    /// with an equal-power curve. See docs/kontakt-crossfade-evidence.md.
    ///
    /// Program/group conditions are separate gates. The caller must preserve
    /// the original zone bounds rather than clipping them to a program range.
    pub fn selection_gain(&self, note: i32, velocity: u16) -> f32 {
        if !(0..=127).contains(&note)
            || velocity > 127
            || [
                self.low_key,
                self.high_key,
                self.low_velocity,
                self.high_velocity,
                self.fade_low_key,
                self.fade_high_key,
                self.fade_low_velocity,
                self.fade_high_velocity,
            ]
            .iter()
            .any(|value| *value > 127)
            || note < self.low_key as i32
            || note > self.high_key as i32
            || velocity < self.low_velocity
            || velocity > self.high_velocity
        {
            return 0.0;
        }
        let mut gain = 1.0;
        for (distance, width) in [
            (note as u16 - self.low_key, self.fade_low_key),
            (self.high_key - note as u16, self.fade_high_key),
            (velocity - self.low_velocity, self.fade_low_velocity),
            (self.high_velocity - velocity, self.fade_high_velocity),
        ] {
            if width != 0 && distance < width {
                gain = gain * (1.0 / (width as f32 + 1.0)) * (distance as f32 + 1.0);
            }
        }
        gain
    }

    pub fn end_frame(&self, frames: usize) -> usize {
        if self.absolute_end {
            frames.min((self.sample_end as usize).saturating_add(1))
        } else {
            frames.saturating_sub(self.sample_end as usize)
        }
    }

    pub fn parse(group_index: u32, data: &[u8], version: u16) -> Result<Zone, Error> {
        if version > 0x9C {
            return Err(Error::Malformed(format!(
                "unsupported zone version 0x{version:02X}"
            )));
        }
        let mut r = Reader::new(data);

        let sample_start = r.u32()?;
        let sample_end = r.u32()?;
        let _sample_start_mod = r.u32()?;

        let low_velocity = r.u16()?;
        let high_velocity = r.u16()?;
        let low_key = r.u16()?;
        let high_key = r.u16()?;
        let fade_low_velocity = r.u16()?;
        let fade_high_velocity = r.u16()?;
        let fade_low_key = r.u16()?;
        let fade_high_key = r.u16()?;
        let root_key = r.u16()?;
        let volume = r.f32()?;
        let pan = r.f32()?;
        let tune = r.f32()?;

        let mut zone = Zone {
            group_index,
            version,
            sample_start,
            sample_end,
            absolute_end: false,
            one_shot: false,
            low_velocity,
            high_velocity,
            low_key,
            high_key,
            fade_low_velocity,
            fade_high_velocity,
            fade_low_key,
            fade_high_key,
            root_key,
            volume,
            pan,
            tune,
            filename_id: None,
            sample_resolution: 0,
            sample_rate: 0,
            channels: 0,
            num_frames: 0,
            root_note: 0,
            tuning: 1.0,
            sample_size: 0,
            loops: Vec::new(),
        };

        if version >= 0x9A {
            r.u8()?;
            r.u8()?;
            r.u32()?;
            if r.remaining() == 0 {
                return Ok(zone);
            }
        }

        zone.filename_id = Some(r.u32()?);
        zone.sample_resolution = r.u32()?;
        zone.sample_rate = r.u32()?;
        zone.channels = r.u8()?;
        zone.num_frames = r.u32()?;
        let _unknown = r.u32()?;
        if version <= 0x93 {
            let _unknown = r.u32()?;
        }
        zone.root_note = r.u32()?;
        zone.tuning = r.f32()?;
        let _unknown = r.u8()?;
        zone.sample_size = r.u32()?;

        Ok(zone)
    }
}

/// Decode every zone found under any `ZONE_LIST` in the chunk tree.
pub fn decode_zones(chunks: &[preset_chunk::PresetChunk]) -> Vec<Zone> {
    let mut out = Vec::new();
    for chunk in chunks {
        collect_zones(chunk, &mut out);
    }
    out
}

fn collect_zones(chunk: &preset_chunk::PresetChunk, out: &mut Vec<Zone>) {
    if chunk.id == preset_chunk::ID_ZONE_LIST {
        for zone_chunk in &chunk.children {
            if let Some(version) = zone_chunk.version {
                if let Ok(mut zone) = Zone::parse(zone_chunk.id, &zone_chunk.public_data, version) {
                    for child in &zone_chunk.children {
                        if child.id == preset_chunk::ID_LOOP_ARRAY {
                            zone.loops = parse_loops(&child.public_data);
                        }
                    }
                    out.push(zone);
                }
            }
        }
    }
    for child in &chunk.children {
        collect_zones(child, out);
    }
}

fn parse_loops(data: &[u8]) -> Vec<Loop> {
    let mut r = Reader::new(data);
    let mut loops = Vec::new();
    let Ok(enablement) = r.u16() else {
        return loops;
    };
    for i in 0..8 {
        if enablement & (1 << i) == 0 {
            continue;
        }
        let Ok(marker) = r.u16() else { break };
        if marker != 0x60 {
            break;
        }
        let Ok(mode) = r.i32() else { break };
        let Ok(start) = r.u32() else { break };
        let Ok(length) = r.u32() else { break };
        let Ok(count) = r.u32() else { break };
        let Ok(alternating) = r.u8() else { break };
        let Ok(tuning) = r.f32() else { break };
        let Ok(crossfade_length) = r.u32() else { break };
        if r.remaining() > 0 {
            let _ = r.skip(1);
        }
        loops.push(Loop {
            mode,
            start,
            length,
            count,
            alternating: alternating > 0,
            tuning,
            crossfade_length,
        });
    }
    loops
}

struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    fn new(data: &'a [u8]) -> Self {
        Reader { data, pos: 0 }
    }

    fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    fn skip(&mut self, n: usize) -> Result<(), Error> {
        self.take(n).map(|_| ())
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
}

#[cfg(test)]
mod selection_tests {
    use super::*;

    fn zone() -> Zone {
        // A valid v0x9c zone with no sample attachment. Zone selection math is
        // independent of decoded sample metadata and filesystem access.
        let mut zone = Zone::parse(0, &[0; 48], 0x9c).unwrap();
        zone.high_key = 127;
        zone.high_velocity = 127;
        zone
    }

    fn close(actual: f32, expected: f32) {
        assert!((actual - expected).abs() < 1e-6, "{actual} != {expected}");
    }

    #[test]
    fn unfaded_selection_uses_inclusive_bounds_and_rejects_invalid_midi() {
        let mut zone = zone();
        zone.low_key = 60;
        zone.high_key = 72;
        zone.low_velocity = 20;
        zone.high_velocity = 100;
        for (key, velocity, expected) in [
            (59, 20, 0.0),
            (60, 19, 0.0),
            (60, 20, 1.0),
            (72, 100, 1.0),
            (73, 100, 0.0),
            (72, 101, 0.0),
            (-1, 100, 0.0),
            (128, 100, 0.0),
            (60, 128, 0.0),
            (i32::MIN, u16::MAX, 0.0),
            (i32::MAX, 0, 0.0),
        ] {
            assert_eq!(zone.selection_gain(key, velocity), expected);
        }
    }

    #[test]
    fn key_edge_ramps_include_nonzero_outer_endpoint_and_n_plus_one_denominator() {
        let mut zone = zone();
        zone.low_key = 60;
        zone.high_key = 80;
        zone.fade_low_key = 3;
        zone.fade_high_key = 3;
        for (key, gain) in [
            (60, 0.25),
            (61, 0.5),
            (62, 0.75),
            (63, 1.0),
            (77, 1.0),
            (78, 0.75),
            (79, 0.5),
            (80, 0.25),
        ] {
            close(zone.selection_gain(key, 100), gain);
        }
        assert_eq!(zone.selection_gain(59, 100), 0.0);
        assert_eq!(zone.selection_gain(81, 100), 0.0);
    }

    #[test]
    fn velocity_edge_ramps_are_inside_the_zone_and_use_discrete_midi_steps() {
        let mut zone = zone();
        zone.low_velocity = 40;
        zone.high_velocity = 100;
        zone.fade_low_velocity = 4;
        zone.fade_high_velocity = 4;
        for (velocity, gain) in [
            (40, 0.2),
            (41, 0.4),
            (42, 0.6),
            (43, 0.8),
            (44, 1.0),
            (96, 1.0),
            (97, 0.8),
            (98, 0.6),
            (99, 0.4),
            (100, 0.2),
        ] {
            close(zone.selection_gain(60, velocity), gain);
        }
        assert_eq!(zone.selection_gain(60, 39), 0.0);
        assert_eq!(zone.selection_gain(60, 101), 0.0);
    }

    #[test]
    fn complementary_zone_overlap_preserves_linear_amplitude_for_keys_and_velocities() {
        let mut low = zone();
        let mut high = zone();
        low.high_key = 67;
        low.fade_high_key = 8;
        high.low_key = 60;
        high.fade_low_key = 8;
        for key in 0..128 {
            close(
                low.selection_gain(key, 100) + high.selection_gain(key, 100),
                1.0,
            );
        }

        let mut low = zone();
        let mut high = zone();
        low.high_velocity = 67;
        low.fade_high_velocity = 8;
        high.low_velocity = 60;
        high.fade_low_velocity = 8;
        for velocity in 0..128 {
            close(
                low.selection_gain(60, velocity) + high.selection_gain(60, velocity),
                1.0,
            );
        }
    }

    #[test]
    fn four_edges_multiply_including_overlapping_fades_on_a_single_zone() {
        let mut zone = zone();
        zone.low_key = 60;
        zone.high_key = 62;
        zone.low_velocity = 20;
        zone.high_velocity = 22;
        zone.fade_low_key = 4;
        zone.fade_high_key = 4;
        zone.fade_low_velocity = 4;
        zone.fade_high_velocity = 4;
        close(zone.selection_gain(61, 21), 16.0 / 625.0);
        close(zone.selection_gain(60, 20), 9.0 / 625.0);
    }

    #[test]
    fn binary_fade_fields_feed_the_same_gain_mapping() {
        let mut bytes = [0u8; 48];
        for (offset, value) in [
            (12, 40u16),
            (14, 100),
            (16, 60),
            (18, 80),
            (20, 4),
            (22, 2),
            (24, 3),
            (26, 1),
        ] {
            bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        }
        let zone = Zone::parse(7, &bytes, 0x9c).unwrap();
        assert_eq!(zone.group_index, 7);
        close(zone.selection_gain(60, 40), 1.0 / 20.0);
        close(zone.selection_gain(80, 100), 1.0 / 6.0);
        close(zone.selection_gain(65, 65), 1.0);
    }

    #[test]
    fn range_gates_must_not_replace_the_original_fade_origin() {
        let mut zone = zone();
        zone.low_key = 60;
        zone.high_key = 80;
        zone.fade_low_key = 10;
        // A separate program range beginning at 65 must gate the note without
        // rewriting zone.low_key to 65 and moving the original fade origin.
        close(zone.selection_gain(65, 100), 6.0 / 11.0);
    }

    #[test]
    fn maximum_fades_stay_finite_and_malformed_zone_fields_are_silent() {
        let mut zone = zone();
        zone.fade_low_key = 127;
        zone.fade_high_key = 127;
        zone.fade_low_velocity = 127;
        zone.fade_high_velocity = 127;
        for key in 0..128 {
            for velocity in 0..128 {
                let gain = zone.selection_gain(key, velocity);
                assert!(gain.is_finite() && (0.0..=1.0).contains(&gain));
            }
        }
        zone.fade_low_key = u16::MAX;
        assert_eq!(zone.selection_gain(60, 100), 0.0);
        zone.fade_low_key = 0;
        zone.high_key = u16::MAX;
        assert_eq!(zone.selection_gain(60, 100), 0.0);
        zone.high_key = 20;
        zone.low_key = 30;
        assert_eq!(zone.selection_gain(25, 100), 0.0);
    }
}
