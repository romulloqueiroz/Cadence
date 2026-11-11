//! Instrument-wide Kontakt group-start evaluation.
//!
//! The recovered behavior (docs/kontakt-pedal-release-evidence.md) keeps one
//! shared round-robin counter, one shared random draw per note callback, a
//! latched instrument-wide key-switch state, and channel-addressed controllers.
//! Criteria connectors are folded left-to-right; the final connector is unused.

use nk_format::model::{Group, ProgramInfo};
use nk_format::start_criteria::{CriteriaLogic, StartCondition};

const CHANNELS: usize = 16;
const CONTROLLERS: usize = 128;

#[derive(Debug, Clone)]
enum RuntimeCondition {
    Always,
    Key { index: usize },
    Controller { controller: u8, low: u8, high: u8 },
    RoundRobin { position: u32 },
    Random { ordinal: u32 },
}

#[derive(Debug, Clone)]
struct RuntimeCriterion {
    condition: RuntimeCondition,
    next: CriteriaLogic,
}

/// Mutable selection state shared by every group in one instrument.
pub(crate) struct GroupStart {
    groups: Vec<Vec<RuntimeCriterion>>,
    key_ranges: Vec<(u8, u8)>,
    key_flags: Vec<bool>,
    initial_key_flags: Vec<bool>,
    round_robin_modulus: u32,
    round_robin: i64,
    random_count: u32,
    random_state: u64,
    initial_random_state: u64,
    random_draw: u32,
    controllers: [[u8; CONTROLLERS]; CHANNELS],
}

impl GroupStart {
    pub(crate) fn new(groups: &[Group], program: &ProgramInfo) -> Self {
        let mut key_ranges = Vec::new();
        let mut round_robin_modulus = 0u32;
        let mut random_count = 0u32;
        let mut runtime_groups = Vec::with_capacity(groups.len());

        for group in groups {
            let mut criteria = Vec::with_capacity(group.start_criteria.len());
            for criterion in &group.start_criteria {
                let condition = match criterion.condition {
                    StartCondition::Always => RuntimeCondition::Always,
                    StartCondition::Key { low, high } => {
                        let index = key_ranges.len();
                        key_ranges.push((low, high));
                        RuntimeCondition::Key { index }
                    }
                    StartCondition::Controller {
                        controller,
                        low,
                        high,
                    } => RuntimeCondition::Controller {
                        controller,
                        low,
                        high,
                    },
                    StartCondition::RoundRobin { position } => {
                        round_robin_modulus = round_robin_modulus.max(position + 1);
                        RuntimeCondition::RoundRobin { position }
                    }
                    StartCondition::Random => {
                        let ordinal = random_count;
                        random_count += 1;
                        RuntimeCondition::Random { ordinal }
                    }
                };
                criteria.push(RuntimeCriterion {
                    condition,
                    next: criterion.next,
                });
            }
            runtime_groups.push(criteria);
        }

        // A negative stored default selects the lowest configured key-switch
        // bound. The key flags then start as inclusive membership of that key.
        let default_key = program
            .default_key_switch
            .unwrap_or_else(|| key_ranges.iter().map(|(low, _)| *low).min().unwrap_or(0));
        let initial_key_flags = key_ranges
            .iter()
            .map(|(low, high)| *low <= default_key && default_key <= *high)
            .collect::<Vec<_>>();

        let initial_random_state = 0x9E37_79B9_7F4A_7C15;
        GroupStart {
            groups: runtime_groups,
            key_ranges,
            key_flags: initial_key_flags.clone(),
            initial_key_flags,
            round_robin_modulus,
            round_robin: -1,
            random_count,
            random_state: initial_random_state,
            initial_random_state,
            random_draw: 0,
            controllers: [[0; CONTROLLERS]; CHANNELS],
        }
    }

    /// Deterministic seed hook used by playback tests.
    pub(crate) fn seed_random(&mut self, seed: u64) {
        let seed = seed | 1;
        self.random_state = seed;
        self.initial_random_state = seed;
    }

    pub(crate) fn reset(&mut self) {
        self.key_flags.clone_from(&self.initial_key_flags);
        self.round_robin = -1;
        self.random_state = self.initial_random_state;
        self.random_draw = 0;
        self.controllers = [[0; CONTROLLERS]; CHANNELS];
    }

    /// Update latches and advance shared counters for an ordinary note-on.
    pub(crate) fn begin_note(&mut self, note: u8) {
        self.update_key_switch(note);
        if self.round_robin_modulus > 0 {
            self.round_robin = (self.round_robin + 1).rem_euclid(self.round_robin_modulus as i64);
        }
        self.random_draw = self.next_random();
    }

    /// A synthetic release-layer note draws a random index but does not advance
    /// the round-robin counter or update the key-switch latch.
    pub(crate) fn begin_release(&mut self) {
        self.random_draw = self.next_random();
    }

    pub(crate) fn set_controller(&mut self, channel: u8, controller: u8, value: u8) {
        let channel = channel as usize % CHANNELS;
        let controller = controller as usize;
        if controller < CONTROLLERS {
            self.controllers[channel][controller] = value.min(127);
        }
    }

    pub(crate) fn controller(&self, channel: u8, controller: u8) -> u8 {
        let channel = channel as usize % CHANNELS;
        let controller = (controller as usize).min(CONTROLLERS - 1);
        self.controllers[channel][controller]
    }

    /// Whether a group's start conditions currently allow it to sound.
    pub(crate) fn allows(&self, group_index: u32, channel: u8) -> bool {
        let Some(criteria) = self.groups.get(group_index as usize) else {
            return true;
        };
        let Some(first) = criteria.first() else {
            return true;
        };
        let mut result = self.evaluate(&first.condition, channel);
        for index in 1..criteria.len() {
            let value = self.evaluate(&criteria[index].condition, channel);
            result = match criteria[index - 1].next {
                CriteriaLogic::And => result && value,
                CriteriaLogic::AndNot => result && !value,
                CriteriaLogic::Or => result || value,
            };
        }
        result
    }

    fn evaluate(&self, condition: &RuntimeCondition, channel: u8) -> bool {
        match condition {
            RuntimeCondition::Always => true,
            RuntimeCondition::Key { index } => self.key_flags.get(*index).copied().unwrap_or(false),
            RuntimeCondition::Controller {
                controller,
                low,
                high,
            } => {
                let value = self.controller(channel, *controller);
                *low <= value && value <= *high
            }
            RuntimeCondition::RoundRobin { position } => {
                self.round_robin >= 0 && *position as i64 == self.round_robin
            }
            RuntimeCondition::Random { ordinal } => *ordinal == self.random_draw,
        }
    }

    fn update_key_switch(&mut self, note: u8) {
        if self.key_ranges.is_empty() {
            return;
        }
        let low = self.key_ranges.iter().map(|(low, _)| *low).min().unwrap();
        let high = self.key_ranges.iter().map(|(_, high)| *high).max().unwrap();
        // Notes outside the union of configured ranges preserve the prior latch.
        if note < low || note > high {
            return;
        }
        for (flag, (range_low, range_high)) in self.key_flags.iter_mut().zip(self.key_ranges.iter())
        {
            *flag = *range_low <= note && note <= *range_high;
        }
    }

    fn next_random(&mut self) -> u32 {
        if self.random_count == 0 {
            return 0;
        }
        let mut x = self.random_state;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.random_state = x;
        let value = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        ((value >> 33) as u32) % self.random_count
    }
}
