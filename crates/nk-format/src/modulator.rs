//! Internal modulators and their AHDSR envelopes.
//!
//! Ported from ConvertWithMoss `InternalModulator.java`. Block 1 lists the
//! modulated parameters and the source name; block 2 carries the envelope
//! times. A small padding heuristic is required because a few parameter/source
//! combinations have 0–2 unaccounted bytes.

use crate::{preset_chunk, Error};

#[derive(Debug, Clone, PartialEq)]
pub struct Envelope {
    /// Times are in milliseconds, as stored.
    pub attack: f32,
    pub hold: f32,
    pub decay: f32,
    pub release: f32,
    pub sustain: f32,
    pub curve: f32,
    pub ahd_only: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TargetParameter {
    pub name: String,
    pub intensity: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LfoKind {
    Sine,
    Rectangle,
    Triangle,
    Sawtooth,
    Random,
    Multi,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Lfo {
    pub kind: LfoKind,
    /// Milliseconds, as stored.
    pub delay: f32,
    /// Free-running frequency in Hz. Tempo-synced units are not decoded yet.
    pub frequency: f32,
    pub pulse_width: f32,
    /// Normalized 0..1 phase offset.
    pub start_phase: f32,
    pub normalize: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct InternalModulator {
    pub parameters: Vec<TargetParameter>,
    pub source_name: String,
    pub source_index: u32,
    pub envelope: Option<Envelope>,
    pub lfo: Option<Lfo>,
}

impl InternalModulator {
    pub fn parse(id: u32, data: &[u8]) -> Result<InternalModulator, Error> {
        if id != preset_chunk::ID_PAR_INTERNAL_MOD && id != preset_chunk::ID_PAR_MOD_BASE {
            return Err(Error::Malformed("not an internal modulator".into()));
        }

        let mut r = Reader::new(data);
        if id == preset_chunk::ID_PAR_MOD_BASE {
            r.u32()?;
        }

        let mut main_version = r.u8()?;
        if main_version == 0 {
            main_version = r.u8()?;
        }
        if main_version != 1 {
            return Err(Error::Malformed("unknown internal modulator header".into()));
        }

        let version = r.u16()?;
        if !(0x80..=0x81).contains(&version) {
            return Err(Error::Malformed(format!(
                "unknown internal modulator version {version}"
            )));
        }

        let size_block1 = r.u32()? as usize;
        let block1 = r.take(size_block1)?;
        if r.u32()? != 0 {
            return Err(Error::Malformed(
                "internal modulator separator not 0".into(),
            ));
        }
        let size_block2 = r.u32()? as usize;
        let block2 = r.take(size_block2)?;

        let (parameters, source_name, source_index) = parse_block1(block1, version)?;
        let (envelope, lfo) =
            parse_block2(block2, &parameters, &source_name, source_index, version);

        Ok(InternalModulator {
            parameters,
            source_name,
            source_index,
            envelope,
            lfo,
        })
    }

    pub fn targets_volume(&self) -> bool {
        self.targets("volume")
    }

    pub fn targets(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name)
    }

    pub fn intensity(&self, name: &str) -> Option<f32> {
        self.parameters
            .iter()
            .find(|p| p.name == name)
            .map(|p| p.intensity)
    }

    pub fn is_envelope(&self) -> bool {
        self.source_index == 0 || self.source_index == 2
    }
}

fn parse_block1(data: &[u8], version: u16) -> Result<(Vec<TargetParameter>, String, u32), Error> {
    let mut r = Reader::new(data);
    let count = r.u32()? as usize;
    if count > 10_000 {
        return Err(Error::Malformed(format!(
            "implausible modulator parameter count {count}"
        )));
    }

    let mut parameters = Vec::new();
    let mut last_had_curve = false;

    for i in 0..count {
        let name = r.ascii_len()?;
        let intensity = r.f32()?;
        if r.u16()? != 0xFFFF {
            return Err(Error::Malformed("internal modulator block1 marker".into()));
        }
        let flags = r.u8()?;
        let _lag = r.i16()?;

        if flags & 8 != 0 {
            r.skip(5)?;
            last_had_curve = false;
        } else {
            let description = r.ascii_len()?;
            let padding = needs_padding(i, &name, &description);
            if padding > 0 {
                r.skip(padding)?;
            }

            let has_shaper = r.u8()? > 0;
            last_had_curve = false;
            if has_shaper {
                let kind = r.u8()?;
                let _active = r.u8()?;
                match kind {
                    0 => {
                        return Err(Error::Malformed(
                            "internal modulator modulation shaper layout".into(),
                        ))
                    }
                    1 => r.skip(128 * 4)?,
                    2 => {
                        let points = r.u8()? as usize;
                        last_had_curve = true;
                        r.skip(points * 3 * 4)?;
                    }
                    _ => {}
                }
            }
            parameters.push(TargetParameter { name, intensity });
        }
    }

    let mut source_name = String::new();
    let mut source_index = 0;
    if !parameters.is_empty() {
        if version != 0x81 || !last_had_curve {
            r.u8()?;
        }
        let _mod_section_open = r.u8()?;
        let _bypassed = r.u8()?;
        let _retrigger = r.u8()?;
        let _ = r.u8()?;
        let _ = r.u32()?;
        source_name = r.ascii_len()?;
        source_index = r.u32()?;
    }

    Ok((parameters, source_name, source_index))
}

fn parse_block2(
    data: &[u8],
    parameters: &[TargetParameter],
    source_name: &str,
    source_index: u32,
    _version: u16,
) -> (Option<Envelope>, Option<Lfo>) {
    let has_target = parameters.iter().any(|p| {
        p.name == "volume" || p.name == "filterCutoff" || p.name == "pitch" || p.name == "pan"
    });
    if !has_target {
        return (None, None);
    }

    // Native layout: a 34-byte base header, then the source-specific payload.
    // Layouts below follow `BParEnv_AHDSR::doRead`, `BParEnv_DBD::doRead` and
    // `BParLFO::doRead`; only AHDSR and LFO payloads are decoded here.
    if source_index != 0 && source_index != 2 {
        // Source index 1 is the LFO slot in the recovered internal-modulator
        // model; some older payloads have no source name.
        let unnamed = source_name.is_empty() || source_name == "<none>";
        if source_name.starts_with("LFO") || (source_index == 1 && unnamed) {
            return (None, parse_lfo(data));
        }
        return (None, None);
    }

    let mut r = Reader::new(data);
    if r.remaining() < 59 {
        return (None, None);
    }

    if source_name == "ENV_AHDSR" || source_name == "<none>" || source_name.is_empty() {
        r.skip(34).ok();
        let Some(curve) = r.f32().ok() else {
            return (None, None);
        };
        let Some(attack) = r.f32().ok() else {
            return (None, None);
        };
        let Some(decay) = r.f32().ok() else {
            return (None, None);
        };
        let Some(hold) = r.f32().ok() else {
            return (None, None);
        };
        let Some(release) = r.f32().ok() else {
            return (None, None);
        };
        let Some(sustain) = r.f32().ok() else {
            return (None, None);
        };
        let Some(ahd_only) = r.u8().ok() else {
            return (None, None);
        };
        (
            Some(Envelope {
                attack,
                hold,
                decay,
                release,
                sustain,
                curve,
                ahd_only: ahd_only > 0,
            }),
            None,
        )
    } else {
        // ENV_DBD and other envelope types are not decoded yet.
        (None, None)
    }
}

fn parse_lfo(data: &[u8]) -> Option<Lfo> {
    let mut r = Reader::new(data);
    r.skip(34).ok()?;
    let kind = match r.u32().ok()? {
        0 => LfoKind::Sine,
        1 => LfoKind::Rectangle,
        2 => LfoKind::Triangle,
        3 => LfoKind::Sawtooth,
        4 => LfoKind::Random,
        5 | 6 => LfoKind::Multi,
        _ => return None,
    };
    let delay = r.f32().ok()?;
    let frequency = r.f32().ok()?;
    let pulse_width = r.f32().ok()?;
    let start_phase = r.f32().ok()?;
    let normalize = r.u8().ok()? > 0;
    if !delay.is_finite()
        || !frequency.is_finite()
        || !pulse_width.is_finite()
        || !start_phase.is_finite()
        || delay < 0.0
        || frequency < 0.0
    {
        return None;
    }
    Some(Lfo {
        kind,
        delay,
        frequency,
        pulse_width,
        start_phase,
        normalize,
    })
}

fn needs_padding(position: usize, name: &str, description: &str) -> usize {
    if position > 0 {
        if name == "filterCutoff" && description == "ENV_AHDSR_CUTOFF" {
            return 2;
        }
        if name == "volume" && (description == "LFO_SINE_VOLUME" || description == "STEP_VOLUME") {
            return 1;
        }
        if (name == "pan" && description == "ENV_DBD_PAN")
            || (name == "filterCutoff" && description == "LFO_MULTI_CUTOFF")
        {
            return 2;
        }
        if name == "vfType" && description == "LFO_MULTI_3X2_TYPE" {
            return 2;
        }
    }

    if needs_padding_table(name, description) {
        return 1;
    }
    if no_padding_table(name, description) {
        return 0;
    }

    usize::from((name.len() + description.len()) % 2 == 1)
}

fn needs_padding_table(name: &str, description: &str) -> bool {
    match (name, description) {
        ("eqGain1", "LFO_SINE_EQ_GAIN_1")
        | ("formantTalk", "LFO_SINE_TALK")
        | ("intensity", "LFO_SINE_FREQUENCY")
        | ("loopStart", "ENV_AHDSR_LOOP_START")
        | ("pitch", "LFO_RECT_PITCH")
        | ("vfType", "LFO_MULTI_3X2_TYPE")
        | ("bitdepth", "ENV_AHDSR_BITDEPTH")
        | ("dyx_dry", "ENV_AHDSR_MORPH")
        | ("downsample", "LFO_SINE_SAMPLE_RATE") => true,
        ("filterCutoff", d) => matches!(
            d,
            "" | "<none>"
                | "LFO_SINE_CUTOFF"
                | "STEP_CUTOFF"
                | "ENV_AHDSR_CUTOFF"
                | "LFO_SAW_CUTOFF"
                | "LFO_RECT_CUTOFF"
                | "LFO_MULTI_CUTOFF"
                | "ENV_DBD_CUTOFF"
                | "ENV FLW_CUTOFF"
        ),
        ("filterQ", d) => matches!(d, "LFO_SINE_RESONANCE" | "STEP_RESONANCE"),
        ("frequency", d) => matches!(d, "ENV_DBD_FREQUENCY" | "ENV_AHDSR_FREQUENCY"),
        ("volume", d) => matches!(d, "GLIDE_VOLUME" | "LFO_SAW_VOLUME"),
        ("dyx_morph", d) => matches!(d, "ENV_AHDSR_MORPH" | "LFO_MULTI_MORPH" | "LFO_TRI_MORPH"),
        _ => false,
    }
}

fn no_padding_table(name: &str, description: &str) -> bool {
    match (name, description) {
        ("filterCutoff", d) => matches!(d, "ENV FLW_CUTOFF" | "ENV_FILTER"),
        ("formantShift", d) => matches!(d, "<none>" | "ENV_AHDSR_FORMANT"),
        ("grainSpeed", d) => matches!(
            d,
            "LFO_MULTI_CUTOFF" | "LFO_MULTI_SPEED" | "LFO_SAW_SPEED" | "ENV_AHDSR_SPEED"
        ),
        ("pan", d) => matches!(
            d,
            "<none>"
                | "LFO_SINE_PAN"
                | "LFO_TRI_PAN"
                | "LFO_RECT_PAN"
                | "LFO_TRI_VOLUME"
                | "ENV_DBD_PAN"
        ),
        ("pitch", d) => matches!(
            d,
            "" | "<none>"
                | "GLIDE_PITCH"
                | "LFO_SINE_PITCH"
                | "LFO_TRI_PITCH"
                | "LFO_RAND_PITCH"
                | "STEP_PITCH"
                | "ENV_AHDSR_PITCH"
                | "ENV_DBD_PITCH"
                | "ENV_PITCH_MIX"
        ),
        ("volume", d) => matches!(
            d,
            "<none>"
                | "ENV_AHDSR_VOLUME"
                | "GLIDE_VOLUME"
                | "LFO_SINE_VOLUME"
                | "LFO_RECT_VOLUME"
                | "STEP_VOLUME"
                | "VOL_GATE"
        ),
        _ => false,
    }
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

    fn f32(&mut self) -> Result<f32, Error> {
        let b = self.take(4)?;
        Ok(f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
    }

    fn ascii_len(&mut self) -> Result<String, Error> {
        let length = self.u32()? as usize;
        let bytes = self.take(length)?;
        Ok(String::from_utf8_lossy(bytes).into_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lfo_block(kind: u32, frequency: f32, phase: f32) -> Vec<u8> {
        let mut bytes = vec![0; 34];
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&12.0f32.to_le_bytes());
        bytes.extend_from_slice(&frequency.to_le_bytes());
        bytes.extend_from_slice(&0.25f32.to_le_bytes());
        bytes.extend_from_slice(&phase.to_le_bytes());
        bytes.push(1);
        bytes
    }

    #[test]
    fn lfo_payload_decodes_kind_timing_and_phase() {
        let lfo = parse_lfo(&lfo_block(2, 3.5, 0.125)).unwrap();
        assert_eq!(lfo.kind, LfoKind::Triangle);
        assert_eq!(lfo.delay, 12.0);
        assert_eq!(lfo.frequency, 3.5);
        assert_eq!(lfo.pulse_width, 0.25);
        assert_eq!(lfo.start_phase, 0.125);
        assert!(lfo.normalize);
    }

    #[test]
    fn lfo_payload_rejects_unknown_kind_and_negative_frequency() {
        assert!(parse_lfo(&lfo_block(9, 1.0, 0.0)).is_none());
        assert!(parse_lfo(&lfo_block(0, -1.0, 0.0)).is_none());
        assert!(parse_lfo(&[0; 20]).is_none());
    }
}
