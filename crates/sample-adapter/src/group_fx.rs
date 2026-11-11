//! Ordered group inserts. Kontakt's `BDsp::HandleVoiceEfx` runs these per
//! voice; they must not be collapsed into one instance per effect type.

use nk_format::{
    fx::{AmpSettings, SimpleFilter},
    model,
};
use plugin_api::provider::LoadError;

pub(crate) const GROUP_FX_SLOTS: usize = 8;

#[derive(Clone, Copy)]
pub(crate) enum GroupFxStage {
    NativeFilter {
        filter: nk_format::fx::NativeFilter,
        slot: usize,
        modulated: bool,
        out_level: f32,
    },
    Filter {
        filter: SimpleFilter,
        /// Original binary slot; None denotes legacy XML filter parameters.
        slot: Option<usize>,
        /// Base normalized cutoff for bounded, additive object modulation.
        normalized_cutoff: f32,
        out_level: f32,
    },
    Spread {
        slot: usize,
        spread: f32,
        pan: f32,
        out_level: f32,
    },
    Send {
        slot: usize,
        levels: [f32; 8],
    },
    Shaper {
        params: nk_format::fx::ShaperParams,
        slot: usize,
        out_level: f32,
    },
    Amp {
        settings: AmpSettings,
        slot: usize,
        out_level: f32,
    },
}

impl GroupFxStage {
    /// Physical serialized slot, including empty/bypassed positions. Legacy
    /// XML filter parameters precede the binary chain and remain pre-amplifier.
    pub(crate) fn slot(&self) -> Option<usize> {
        match *self {
            Self::Filter { slot, .. } => slot,
            Self::NativeFilter { slot, .. }
            | Self::Spread { slot, .. }
            | Self::Send { slot, .. }
            | Self::Shaper { slot, .. }
            | Self::Amp { slot, .. } => Some(slot),
        }
    }
}

pub(crate) fn compile(group: &model::Group) -> Result<Vec<GroupFxStage>, LoadError> {
    if !(0..=GROUP_FX_SLOTS as i32).contains(&group.fx_amp_split_point) {
        return Err(LoadError::Failed(
            "group amplifier split is out of range".into(),
        ));
    }
    super::filter_dsp::initialize();
    let mut stages = Vec::new();
    // Legacy XML filter parameters precede the binary insert slots. Native
    // presets use the slots; XML FX arrays are not decoded yet.
    if let Some(filter) = group.filter {
        stages.push(GroupFxStage::Filter {
            filter: SimpleFilter {
                shape: match filter.kind {
                    model::FilterKind::Lowpass => nk_format::fx::FilterShape::Lowpass,
                    model::FilterKind::Highpass => nk_format::fx::FilterShape::Highpass,
                },
                poles: 2,
                cutoff_hz: filter.cutoff,
                q: filter.q,
            },
            slot: None,
            normalized_cutoff: 0.0,
            out_level: 1.0,
        });
    }
    if let Some(chain) = &group.fx_chain {
        if chain.slots.len() > GROUP_FX_SLOTS {
            return Err(LoadError::Failed(format!(
                "group {:?} has more than {GROUP_FX_SLOTS} FX slots",
                group.name
            )));
        }
        for (index, slot) in chain.slots.iter().enumerate() {
            let Some(slot) = slot.as_ref().filter(|slot| !slot.bypass) else {
                continue;
            };
            let Some(child) = &slot.child else { continue };
            let out_level = slot.out_level;
            if !out_level.is_finite() || out_level.abs() > 64.0 {
                return Err(LoadError::Failed(
                    "group FX output gain is out of range".into(),
                ));
            }
            if let Some(levels) = child
                .send_levels()
                .map_err(|e| LoadError::Failed(e.to_string()))?
            {
                stages.push(GroupFxStage::Send {
                    slot: index,
                    levels: levels.levels,
                });
                continue;
            }
            if child.ser_type == 22 {
                return Err(LoadError::Failed(
                    "per-voice group convolution is unsupported; use program inserts/sends".into(),
                ));
            }
            if let Some(settings) = child.amp_settings() {
                stages.push(GroupFxStage::Amp {
                    settings,
                    slot: index,
                    out_level,
                });
            } else if matches!(child.ser_type, 77 | 82 | 83 | 84 | 85 | 97 | 108) {
                return Err(LoadError::Failed(format!(
                    "unsupported or malformed group amp type {} version {}",
                    child.ser_type, child.version
                )));
            } else if let Some(filter) = child
                .native_filter()
                .map_err(|e| LoadError::Failed(e.to_string()))?
            {
                stages.push(GroupFxStage::NativeFilter {
                    filter,
                    slot: index,
                    modulated: cutoff_modulated(group, index),
                    out_level,
                });
            } else if let Some(filter) = child.simple_filter() {
                stages.push(GroupFxStage::Filter {
                    filter,
                    slot: Some(index),
                    // Inverse of nk-format's existing cutoff approximation.
                    // Exact native coefficient/frequency mapping is separate.
                    normalized_cutoff: (filter.cutoff_hz / 20.0).ln() / 1000f32.ln(),
                    out_level,
                });
            } else if let Some((spread, pan)) = child.stereo_spread() {
                stages.push(GroupFxStage::Spread {
                    slot: index,
                    spread,
                    pan,
                    out_level,
                });
            } else if let Some(params) = child
                .shaper_params()
                .map_err(|e| LoadError::Failed(e.to_string()))?
            {
                stages.push(GroupFxStage::Shaper {
                    params,
                    slot: index,
                    out_level,
                });
            }
        }
    }
    Ok(stages)
}

fn cutoff_modulated(group: &model::Group, slot: usize) -> bool {
    group.external_modulators.iter().any(|m| {
        !m.bypass
            && m.kind == 1
            && m.source != 0
            && m.targets.iter().any(|t| {
                t.name == "filterCutoff"
                    && t.intensity != 0.0
                    && (t.target_obj_idx.unwrap_or(-1) < 0 || t.target_obj_idx == Some(slot as i8))
            })
    }) || group.internal_modulators.iter().any(|m| {
        m.parameters
            .iter()
            .any(|p| p.name == "filterCutoff" && p.intensity != 0.0)
    })
}
