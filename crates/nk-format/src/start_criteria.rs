//! Kontakt group-start conditions. See docs/kontakt-group-start-evidence.md.

use crate::Error;

/// The connector following a criterion; the final connector is unused.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CriteriaLogic {
    #[default]
    And,
    AndNot,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartCondition {
    Always,
    /// An inclusive, latched key-switch interval, not a held-note test.
    Key {
        low: u8,
        high: u8,
    },
    Controller {
        controller: u8,
        low: u8,
        high: u8,
    },
    /// Zero-based position in the instrument-wide round-robin cycle.
    RoundRobin {
        position: u32,
    },
    /// Each random criterion receives its own instrument-wide ordinal.
    Random,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StartCriterion {
    pub condition: StartCondition,
    pub next: CriteriaLogic,
}

pub(crate) const ID_START_CRITERIA_ARRAY: u32 = 0x38;

fn bad(message: impl std::fmt::Display) -> Error {
    Error::Malformed(format!("group start: {message}"))
}

pub(crate) fn condition(
    mode: i32,
    key: (i16, i16),
    controller: i16,
    cc: (i16, i16),
    cycle: i32,
) -> Result<StartCondition, Error> {
    let midi = |value: i16| -> Result<u8, Error> {
        u8::try_from(value)
            .ok()
            .filter(|v| *v <= 127)
            .ok_or_else(|| bad("MIDI value outside 0..=127"))
    };
    let range = |(low, high): (i16, i16)| -> Result<(u8, u8), Error> {
        let pair = (midi(low)?, midi(high)?);
        if pair.0 > pair.1 {
            return Err(bad("inverted criterion range"));
        }
        Ok(pair)
    };
    Ok(match mode {
        0 => StartCondition::Always,
        1 => {
            let (low, high) = range(key)?;
            StartCondition::Key { low, high }
        }
        2 => {
            let (low, high) = range(cc)?;
            StartCondition::Controller {
                controller: midi(controller)?,
                low,
                high,
            }
        }
        3 if (0..i32::MAX).contains(&cycle) => StartCondition::RoundRobin {
            position: cycle as u32,
        },
        3 => return Err(bad("invalid round-robin cycleClass")),
        4 => StartCondition::Random,
        5 => return Err(bad("slice_trigger is not supported")),
        _ => return Err(bad(format!("unknown mode {mode}"))),
    })
}

pub(crate) fn logic(value: i32) -> Result<CriteriaLogic, Error> {
    match value {
        0 => Ok(CriteriaLogic::And),
        1 => Ok(CriteriaLogic::AndNot),
        2 => Ok(CriteriaLogic::Or),
        _ => Err(bad(format!("unknown nextCriteria {value}"))),
    }
}

/// Chunk 0x38 has a presence byte and four optional raw SER records. It is
/// deliberately decoded here instead of treating it as a structured chunk.
pub(crate) fn decode_array(data: &[u8]) -> Result<Vec<StartCriterion>, Error> {
    let mut pos = 0;
    fn take<'a>(data: &'a [u8], pos: &mut usize, n: usize) -> Result<&'a [u8], Error> {
        let end = pos
            .checked_add(n)
            .ok_or_else(|| bad("record size overflow"))?;
        let out = data.get(*pos..end).ok_or(Error::Truncated {
            needed: end,
            len: data.len(),
        })?;
        *pos = end;
        Ok(out)
    }
    let mask = take(data, &mut pos, 1)?[0];
    if mask & 0xf0 != 0 {
        return Err(bad("presence mask has more than four slots"));
    }
    let mut result = Vec::new();
    for slot in 0..4 {
        if mask & (1 << slot) == 0 {
            continue;
        }
        let header = take(data, &mut pos, 3)?;
        if header != [0, 0x70, 0] {
            return Err(bad("unsupported SER record kind/version"));
        }
        let body = take(data, &mut pos, 31)?;
        let s16 = |at| i16::from_le_bytes(body[at..at + 2].try_into().unwrap());
        let s32 = |at| i32::from_le_bytes(body[at..at + 4].try_into().unwrap());
        let condition = condition(
            s32(0),
            (s16(8), s16(10)),
            s16(12),
            (s16(14), s16(16)),
            s32(18),
        )?;
        let next = logic(s32(4))?;
        if body[30] > 1 {
            return Err(bad("invalid sequencerOnly boolean"));
        }
        // Mode zero is a vacant slot; Kontakt compacts these before evaluation.
        if condition != StartCondition::Always {
            result.push(StartCriterion { condition, next });
        }
    }
    if pos != data.len() {
        return Err(bad("trailing bytes in criteria array"));
    }
    Ok(result)
}
