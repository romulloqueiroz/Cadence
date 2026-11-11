//! Prepared amp templates and voice-owned circuit banks for group insert slots.
//!
//! Heap owners live here for the sampler lifetime. A voice activation copies a
//! prepared image into an existing bank slot; it never constructs or drops DSP.
use crate::{
    acbox_dsp::AcBoxAmp, amp_dsp::JumpAmp, bassinvader_dsp::BassInvaderAmp,
    basspro_amp::BassProAmp, cat_amp::CatAmp, epiano_amp::EPianoAmp, hotsolo_dsp::HotSoloAmp,
    van51_amp::Van51Amp,
};
use nk_format::fx::{AmpKind, AmpSettings};

macro_rules! amps {
    ($($variant:ident => $owner:ident => $kind:ident),+ $(,)?) => {
        enum AmpState { $($variant(Box<$owner>)),+ }
        impl AmpState {
            fn prepared(settings: AmpSettings, rate: f32) -> Result<Self, &'static str> {
                let mut state = match settings.kind { $(AmpKind::$kind => Self::$variant(Box::new($owner::new(settings)))),+ };
                if rate != 48_000.0 { state.prepare(rate)?; }
                Ok(state)
            }
            fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
                match self { $(Self::$variant(state) => state.prepare(rate)),+ }
            }
            fn duplicate_prepared(&self) -> Self {
                match self { $(Self::$variant(state) => Self::$variant(Box::new(state.duplicate_prepared()))),+ }
            }
            fn copy_prepared_from(&mut self, template: &Self) {
                match (self, template) {
                    $((Self::$variant(state), Self::$variant(source)) => state.copy_prepared_from(source)),+,
                    _ => unreachable!("group amp template and bank kinds differ"),
                }
            }
            fn process(&mut self, input: [f32; 2]) -> [f32; 2] {
                match self { $(Self::$variant(state) => state.process(input[0], input[1])),+ }
            }
        }
    };
}
amps! {
    Jump => JumpAmp => Jump,
    Van51 => Van51Amp => Van51,
    AcBox => AcBoxAmp => AcBox,
    HotSolo => HotSoloAmp => HotSolo,
    BassInvader => BassInvaderAmp => BassInvader,
    Cat => CatAmp => Cat,
    BassPro => BassProAmp => BassPro,
    EPiano => EPianoAmp => EpPreamps,
}

fn kind_index(kind: AmpKind) -> usize {
    match kind {
        AmpKind::Jump => 0,
        AmpKind::Van51 => 1,
        AmpKind::AcBox => 2,
        AmpKind::HotSolo => 3,
        AmpKind::BassInvader => 4,
        AmpKind::Cat => 5,
        AmpKind::BassPro => 6,
        AmpKind::EpPreamps => 7,
    }
}
struct Template {
    settings: AmpSettings,
    state: AmpState,
}
struct Bank {
    kind: AmpKind,
    occurrence: usize,
    template: usize,
    voices: Vec<AmpState>,
}
#[derive(Clone, Copy)]
struct Route {
    bank: usize,
    template: usize,
}

pub(crate) struct GroupAmps {
    groups: Vec<[Option<Route>; 8]>,
    templates: Vec<Template>,
    banks: Vec<Bank>,
    active_groups: Vec<Option<usize>>,
    voice_capacity: usize,
    valid: bool,
}
impl GroupAmps {
    pub(crate) fn new(
        layout: &[[Option<AmpSettings>; 8]],
        voice_capacity: usize,
        rate: f32,
    ) -> Result<Self, &'static str> {
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        let mut pool = Self {
            groups: Vec::with_capacity(layout.len()),
            templates: Vec::new(),
            banks: Vec::new(),
            active_groups: vec![None; voice_capacity],
            voice_capacity,
            valid: false,
        };
        for slots in layout {
            let mut routes = [None; 8];
            let mut occurrences = [0; 8];
            for (slot, settings) in slots
                .iter()
                .enumerate()
                .filter_map(|(i, value)| value.map(|v| (i, v)))
            {
                let template = if let Some(index) =
                    pool.templates.iter().position(|t| t.settings == settings)
                {
                    index
                } else {
                    let index = pool.templates.len();
                    pool.templates.push(Template {
                        settings,
                        state: AmpState::prepared(settings, rate)?,
                    });
                    index
                };
                let kind = kind_index(settings.kind);
                let occurrence = occurrences[kind];
                occurrences[kind] += 1;
                let bank = if let Some(index) = pool
                    .banks
                    .iter()
                    .position(|b| b.kind == settings.kind && b.occurrence == occurrence)
                {
                    index
                } else {
                    let index = pool.banks.len();
                    let voices = (0..voice_capacity)
                        .map(|_| pool.templates[template].state.duplicate_prepared())
                        .collect();
                    pool.banks.push(Bank {
                        kind: settings.kind,
                        occurrence,
                        template,
                        voices,
                    });
                    index
                };
                routes[slot] = Some(Route { bank, template });
            }
            pool.groups.push(routes);
        }
        pool.valid = true;
        Ok(pool)
    }

    /// Rebuild matching-rate images on the loading/preparation thread. Failure
    /// invalidates all voice routes so callbacks cannot play stale-rate state.
    pub(crate) fn prepare(&mut self, rate: f32) -> Result<(), &'static str> {
        self.valid = false;
        self.reset();
        if !rate.is_finite() || rate <= 0.0 {
            return Err("invalid amp sample rate");
        }
        for template in &mut self.templates {
            template.state.prepare(rate)?;
        }
        for bank in &mut self.banks {
            bank.voices = (0..self.voice_capacity)
                .map(|_| self.templates[bank.template].state.duplicate_prepared())
                .collect();
        }
        self.valid = true;
        Ok(())
    }
    /// Mark voices inactive. Their heap-backed states stay owned by the pool.
    pub(crate) fn reset(&mut self) {
        self.active_groups.fill(None);
    }

    pub(crate) fn activate(&mut self, group_index: usize, voice_index: usize) {
        let Some(active) = self.active_groups.get_mut(voice_index) else {
            return;
        };
        *active = None;
        if !self.valid {
            return;
        }
        let Some(slots) = self.groups.get(group_index) else {
            return;
        };
        for route in slots.iter().flatten() {
            self.banks[route.bank].voices[voice_index]
                .copy_prepared_from(&self.templates[route.template].state);
        }
        *active = Some(group_index);
    }
    pub(crate) fn process(
        &mut self,
        group_index: usize,
        slot: usize,
        voice_index: usize,
        input: [f32; 2],
    ) -> [f32; 2] {
        if !self.valid {
            return [0.0; 2];
        }
        let Some(route) = self
            .groups
            .get(group_index)
            .and_then(|slots| slots.get(slot))
            .copied()
            .flatten()
        else {
            return input;
        };
        if self.active_groups.get(voice_index).copied().flatten() != Some(group_index) {
            return [0.0; 2];
        }
        self.banks[route.bank].voices[voice_index].process(input)
    }
    #[cfg(test)]
    pub(crate) fn pool_counts(&self) -> (usize, usize, usize) {
        (
            self.templates.len(),
            self.banks.len(),
            self.banks.iter().map(|bank| bank.voices.len()).sum(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings(kind: AmpKind, value: f32) -> AmpSettings {
        let (values, flags) = match kind {
            AmpKind::Jump => (6, 2),
            AmpKind::Van51 => (8, 5),
            AmpKind::AcBox => (7, 1),
            AmpKind::HotSolo => (8, 2),
            AmpKind::BassInvader => (7, 4),
            AmpKind::Cat => (7, 1),
            AmpKind::BassPro => (17, 3),
            AmpKind::EpPreamps => (17, 6),
        };
        AmpSettings {
            kind,
            values: [value; 17],
            value_count: values,
            flags: [false; 8],
            flag_count: flags,
            modes: [0; 4],
        }
    }
    #[test]
    fn group_amp_templates_dedupe_and_banks_follow_chain_occurrences() {
        let a = settings(AmpKind::Jump, 0.2);
        let b = settings(AmpKind::Jump, 0.8);
        let mut first = [None; 8];
        first[1] = Some(a);
        first[4] = Some(b);
        let mut shifted = [None; 8];
        shifted[0] = Some(a);
        shifted[7] = Some(b);
        let mut third = [None; 8];
        third[2] = Some(settings(AmpKind::Jump, 0.4));
        let mut pool = GroupAmps::new(&[first, shifted, third], 4, 48000.0).unwrap();
        assert_eq!(pool.pool_counts(), (3, 2, 8));
        pool.activate(0, 0);
        pool.activate(1, 1);
        for frame in 0..256 {
            let x = (frame as f32 * 0.071).sin() * 0.2;
            let l = pool.process(0, 1, 0, [x, -x * 0.3]);
            let l = pool.process(0, 4, 0, l);
            let r = pool.process(1, 0, 1, [x, -x * 0.3]);
            let r = pool.process(1, 7, 1, r);
            assert_eq!(l, r);
        }
        pool.activate(1, 0);
        pool.activate(0, 2);
        for _ in 0..64 {
            let l = pool.process(1, 0, 0, [0.2, -0.07]);
            let l = pool.process(1, 7, 0, l);
            let r = pool.process(0, 1, 2, [0.2, -0.07]);
            let r = pool.process(0, 4, 2, r);
            assert_eq!(l, r);
        }
    }
    #[test]
    fn group_amp_all_kinds_copy_exact_prepared_state() {
        let kinds = [
            AmpKind::Jump,
            AmpKind::Van51,
            AmpKind::AcBox,
            AmpKind::HotSolo,
            AmpKind::BassInvader,
            AmpKind::Cat,
            AmpKind::BassPro,
            AmpKind::EpPreamps,
        ];
        let layout = [kinds.map(|kind| Some(settings(kind, 0.5)))];
        let mut pool = GroupAmps::new(&layout, 2, 48000.0).unwrap();
        assert_eq!(pool.pool_counts(), (8, 8, 16));
        pool.activate(0, 0);
        for slot in 0..8 {
            for _ in 0..32 {
                pool.process(0, slot, 0, [0.2, -0.07]);
            }
        }
        pool.activate(0, 0);
        pool.activate(0, 1);
        for slot in 0..8 {
            for _ in 0..64 {
                assert_eq!(
                    pool.process(0, slot, 0, [0.13, -0.03]),
                    pool.process(0, slot, 1, [0.13, -0.03])
                );
            }
        }
    }
    #[test]
    fn group_amp_reuses_each_kind_bank_across_different_templates_and_rates() {
        let kinds = [
            AmpKind::Jump,
            AmpKind::Van51,
            AmpKind::AcBox,
            AmpKind::HotSolo,
            AmpKind::BassInvader,
            AmpKind::Cat,
            AmpKind::BassPro,
            AmpKind::EpPreamps,
        ];
        let a = kinds.map(|kind| Some(settings(kind, 0.2)));
        let mut b = kinds.map(|kind| Some(settings(kind, 0.8)));
        b[7].as_mut().unwrap().modes = [4, 5, 6, 4];
        let mut pool = GroupAmps::new(&[a, b], 3, 48000.0).unwrap();
        assert_eq!(pool.pool_counts(), (16, 8, 24));
        for rate in [48000.0, 192000.0] {
            if rate != 48000.0 {
                pool.prepare(rate).unwrap();
            }
            pool.activate(0, 0);
            for slot in 0..8 {
                for _ in 0..64 {
                    pool.process(0, slot, 0, [0.2, -0.1]);
                }
            }
            pool.activate(1, 0);
            pool.activate(1, 2);
            for slot in 0..8 {
                for _ in 0..64 {
                    assert_eq!(
                        pool.process(1, slot, 0, [0.13, -0.03]),
                        pool.process(1, slot, 2, [0.13, -0.03]),
                        "slot{slot} rate{rate}"
                    );
                }
            }
        }
    }
    #[test]
    fn group_amp_invalid_prepare_silences_until_reprepared_and_activated() {
        let mut row = [None; 8];
        row[3] = Some(settings(AmpKind::Cat, 0.5));
        let mut pool = GroupAmps::new(&[row], 2, 48000.0).unwrap();
        pool.activate(0, 0);
        assert!(pool.prepare(f32::NAN).is_err());
        pool.activate(0, 0);
        assert_eq!(pool.process(0, 3, 0, [1.0, 1.0]), [0.0; 2]);
        pool.prepare(96000.0).unwrap();
        assert_eq!(pool.process(0, 3, 0, [1.0, 1.0]), [0.0; 2]);
        pool.activate(0, 0);
        pool.activate(0, 1);
        for _ in 0..64 {
            assert_eq!(
                pool.process(0, 3, 0, [0.2, -0.1]),
                pool.process(0, 3, 1, [0.2, -0.1])
            );
        }
        pool.reset();
        assert_eq!(pool.process(0, 3, 0, [1.0, 1.0]), [0.0; 2]);
        assert_eq!(pool.process(0, 4, 0, [1.0, 1.0]), [1.0; 2]);
    }
}
