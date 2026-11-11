//! Providers are the adapter boundary: each format turns external code or
//! content into the universal [`crate::Plugin`] contract.

use crate::Plugin;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// A plugin compiled into the player.
    Builtin,
    Vst3,
    AudioUnit,
    Clap,
    /// Community plugins in the platform's open format.
    Wasm,
    /// A clean-room reimplementation built against this protocol.
    Rehosted,
    /// A sample-library adapter driving the shared sampler engine.
    SampleLibrary,
}

impl Format {
    pub fn name(&self) -> &'static str {
        match self {
            Format::Builtin => "builtin",
            Format::Vst3 => "vst3",
            Format::AudioUnit => "au",
            Format::Clap => "clap",
            Format::Wasm => "wasm",
            Format::Rehosted => "rehosted",
            Format::SampleLibrary => "sample-library",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LoadError {
    NotFound(String),
    Unsupported(String),
    Failed(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::NotFound(message) => write!(f, "not found: {message}"),
            LoadError::Unsupported(message) => write!(f, "unsupported: {message}"),
            LoadError::Failed(message) => write!(f, "load failed: {message}"),
        }
    }
}

impl std::error::Error for LoadError {}

/// A discoverable source of plugin instances.
pub trait PluginProvider: Send + Sync {
    fn format(&self) -> Format;

    /// Stable id within its format, e.g. a bundle id or a package hash.
    fn id(&self) -> &str;

    fn create(&self) -> Result<Box<dyn Plugin>, LoadError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Missing;

    impl PluginProvider for Missing {
        fn format(&self) -> Format {
            Format::Wasm
        }

        fn id(&self) -> &str {
            "com.example.missing"
        }

        fn create(&self) -> Result<Box<dyn Plugin>, LoadError> {
            Err(LoadError::NotFound("com.example.missing".into()))
        }
    }

    #[test]
    fn provider_reports_missing() {
        let provider = Missing;
        assert_eq!(provider.format().name(), "wasm");
        assert!(matches!(provider.create(), Err(LoadError::NotFound(_))));
    }
}
