use plugin_api::{Capabilities, Descriptor, ParamFlags, ParameterInfo, Plugin, PluginKind, Unit};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterMetadata {
    pub id: u32,
    pub key: String,
    pub name: String,
    pub min: f32,
    pub max: f32,
    pub default: f32,
    pub stepped: bool,
    pub read_only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PluginVariant {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub version: String,
    pub kind: u32,
    pub inputs: usize,
    pub outputs: usize,
    pub sidechain_inputs: usize,
    pub latency_samples: usize,
    pub tail_samples: usize,
    pub parameters: Vec<ParameterMetadata>,
    #[serde(default)]
    pub variants: Vec<PluginVariant>,
    #[serde(default)]
    pub notices: Vec<String>,
}

impl PluginMetadata {
    pub fn from_plugin(plugin: &dyn Plugin) -> Self {
        let d = plugin.descriptor();
        let c = plugin.capabilities();
        Self {
            id: d.id.into(),
            name: d.name.into(),
            vendor: d.vendor.into(),
            version: d.version.into(),
            kind: plugin_api::abi::kind_code(d.kind),
            inputs: c.inputs,
            outputs: c.outputs,
            sidechain_inputs: c.sidechain_inputs,
            latency_samples: c.latency_samples,
            tail_samples: c.tail_samples,
            parameters: plugin
                .parameters()
                .iter()
                .map(|p| ParameterMetadata {
                    id: p.id,
                    key: plugin.parameter_key(p.id),
                    name: p.name.into(),
                    min: p.min,
                    max: p.max,
                    default: p.default,
                    stepped: p.flags.stepped,
                    read_only: p.flags.read_only,
                })
                .collect(),
            variants: plugin
                .variants()
                .into_iter()
                .map(|v| PluginVariant {
                    id: v.id.into(),
                    name: v.name.into(),
                    vendor: v.vendor.into(),
                    version: v.version.into(),
                    kind: plugin_api::abi::kind_code(v.kind),
                })
                .collect(),
            notices: plugin
                .load_notices()
                .into_iter()
                .take(16)
                .map(|s| s.chars().take(512).collect())
                .collect(),
        }
    }

    pub fn capabilities(&self) -> Capabilities {
        Capabilities {
            inputs: self.inputs,
            outputs: self.outputs,
            sidechain_inputs: self.sidechain_inputs,
            latency_samples: self.latency_samples,
            tail_samples: self.tail_samples,
        }
    }

    pub fn is_valid(&self) -> bool {
        !self.id.is_empty()
            && !self.name.is_empty()
            && self.kind <= 2
            && self.inputs <= crate::shared::MAX_CHANNELS
            && self.outputs <= crate::shared::MAX_CHANNELS
            && self.sidechain_inputs <= crate::shared::MAX_CHANNELS
            && self.capabilities().is_valid()
            && self.parameters.len() <= plugin_api::MAX_PARAMETERS
            && self.notices.len() <= 16
            && self.notices.iter().all(|n| n.len() <= 2048)
            && self.parameters.iter().enumerate().all(|(i, p)| {
                p.id <= u16::MAX as u32
                    && !p.name.is_empty()
                    && !p.key.is_empty()
                    && p.min.is_finite()
                    && p.max.is_finite()
                    && p.default.is_finite()
                    && p.min < p.max
                    && (p.min..=p.max).contains(&p.default)
                    && !self.parameters[..i]
                        .iter()
                        .any(|old| old.id == p.id || old.key == p.key)
            })
            && self.variants.len() <= 256
            && self.variants.iter().enumerate().all(|(i, v)| {
                !v.id.is_empty()
                    && !v.name.is_empty()
                    && v.kind <= 2
                    && !self.variants[..i].iter().any(|old| old.id == v.id)
            })
    }

    // Plugin's reference trait uses static strings. These conversions happen
    // once per sandbox startup, never in an audio callback.
    pub fn descriptor(&self) -> Descriptor {
        Descriptor {
            id: intern(&self.id),
            name: intern(&self.name),
            vendor: intern(&self.vendor),
            version: intern(&self.version),
            kind: plugin_api::abi::kind_from_code(self.kind).unwrap_or(PluginKind::Effect),
        }
    }
    pub fn parameters(&self) -> Vec<ParameterInfo> {
        self.parameters
            .iter()
            .map(|p| ParameterInfo {
                id: p.id,
                name: intern(&p.name),
                min: p.min,
                max: p.max,
                default: p.default,
                unit: Unit::None,
                flags: ParamFlags {
                    stepped: p.stepped,
                    read_only: p.read_only,
                    bypass: false,
                },
            })
            .collect()
    }
}

fn intern(text: &str) -> &'static str {
    Box::leak(text.to_owned().into_boxed_str())
}
