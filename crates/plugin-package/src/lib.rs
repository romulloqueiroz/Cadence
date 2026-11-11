//! Plugin package manifest, content-addressed assets and Ed25519 signatures.
//!
//! A package is a manifest plus immutable assets (IRs, model weights, code).
//! Assets are addressed by SHA-256; the manifest is signed by the publisher's
//! Ed25519 key. Verification is independent of OS code signing (Apple
//! notarization is a separate distribution concern).

pub mod sha256;

use ed25519_dalek::{Signature as EdSignature, Signer, SigningKey, Verifier, VerifyingKey};
use plugin_api::{AssetProvider, PluginKind, API_VERSION};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AssetRef {
    pub name: String,
    pub digest: String,
    pub size: u64,
    pub media_type: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublisherSignature {
    pub algorithm: String,
    pub public_key: String,
    pub signature: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub schema_version: u32,
    pub id: String,
    pub name: String,
    pub version: String,
    /// `instrument`, `effect`, or `midi-effect`.
    pub kind: String,
    pub api_version: u32,
    pub entry: String,
    #[serde(default)]
    pub assets: Vec<AssetRef>,
    #[serde(default)]
    pub publisher: Option<String>,
    #[serde(default)]
    pub signature: Option<PublisherSignature>,
}

impl Manifest {
    pub fn new(
        id: impl Into<String>,
        name: impl Into<String>,
        version: impl Into<String>,
        kind: PluginKind,
        entry: impl Into<String>,
    ) -> Self {
        Manifest {
            schema_version: 1,
            id: id.into(),
            name: name.into(),
            version: version.into(),
            kind: kind_name(kind).to_string(),
            api_version: API_VERSION,
            entry: entry.into(),
            assets: Vec::new(),
            publisher: None,
            signature: None,
        }
    }

    pub fn plugin_kind(&self) -> Option<PluginKind> {
        kind_from_name(&self.kind)
    }

    pub fn is_supported(&self) -> bool {
        self.schema_version == 1
            && self.api_version <= API_VERSION
            && !self.id.is_empty()
            && !self.name.is_empty()
            && !self.version.is_empty()
            && !self.entry.is_empty()
            && self.plugin_kind().is_some()
    }

    /// Canonical bytes covered by the signature: the manifest with the
    /// signature removed.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, PackageError> {
        let mut unsigned = self.clone();
        unsigned.signature = None;
        serde_json::to_vec(&unsigned).map_err(PackageError::Json)
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), PackageError> {
        self.signature = None;
        let bytes = self.canonical_bytes()?;
        self.signature = Some(sign_payload(&bytes, key));
        Ok(())
    }

    /// Verify the publisher signature. Returns the public key hex on success.
    pub fn verify(&self) -> Result<String, PackageError> {
        let signature = self
            .signature
            .as_ref()
            .ok_or(PackageError::MissingSignature)?;
        verify_payload(&self.canonical_bytes()?, signature)?;
        Ok(signature.public_key.clone())
    }

    pub fn to_json(&self) -> Result<String, PackageError> {
        serde_json::to_string_pretty(self).map_err(PackageError::Json)
    }

    pub fn from_json(text: &str) -> Result<Self, PackageError> {
        serde_json::from_str(text).map_err(PackageError::Json)
    }
}

#[derive(Debug)]
pub enum PackageError {
    UnsupportedSchema(u32),
    UnsupportedApi(u32),
    MissingAsset(String),
    MissingSignature,
    SizeMismatch {
        name: String,
        expected: u64,
        actual: u64,
    },
    DigestMismatch(String),
    AssetDeclaredTwice(String),
    UnsafePath(String),
    Signature(String),
    Json(serde_json::Error),
    Io(std::io::Error),
}

impl fmt::Display for PackageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackageError::UnsupportedSchema(version) => {
                write!(f, "unsupported manifest schema {version}")
            }
            PackageError::UnsupportedApi(version) => {
                write!(f, "package needs API {version}, supported {API_VERSION}")
            }
            PackageError::MissingAsset(name) => write!(f, "missing asset {name:?}"),
            PackageError::MissingSignature => write!(f, "manifest is not signed"),
            PackageError::SizeMismatch {
                name,
                expected,
                actual,
            } => write!(f, "asset {name:?} size {actual}, expected {expected}"),
            PackageError::DigestMismatch(name) => write!(f, "asset {name:?} digest mismatch"),
            PackageError::AssetDeclaredTwice(name) => {
                write!(f, "asset {name:?} declared more than once")
            }
            PackageError::UnsafePath(name) => write!(f, "unsafe asset path {name:?}"),
            PackageError::Signature(message) => write!(f, "signature error: {message}"),
            PackageError::Json(error) => write!(f, "manifest JSON error: {error}"),
            PackageError::Io(error) => write!(f, "I/O error: {error}"),
        }
    }
}

impl std::error::Error for PackageError {}

impl From<std::io::Error> for PackageError {
    fn from(error: std::io::Error) -> Self {
        PackageError::Io(error)
    }
}

impl From<serde_json::Error> for PackageError {
    fn from(error: serde_json::Error) -> Self {
        PackageError::Json(error)
    }
}

pub struct Package {
    manifest: Manifest,
    assets: HashMap<String, Vec<u8>>,
}

impl Package {
    pub fn new(manifest: Manifest) -> Self {
        Package {
            manifest,
            assets: HashMap::new(),
        }
    }

    pub fn from_parts(manifest: Manifest, assets: HashMap<String, Vec<u8>>) -> Self {
        Package { manifest, assets }
    }

    pub fn manifest(&self) -> &Manifest {
        &self.manifest
    }

    pub fn manifest_mut(&mut self) -> &mut Manifest {
        &mut self.manifest
    }

    pub fn add_asset(
        &mut self,
        name: &str,
        media_type: &str,
        data: Vec<u8>,
    ) -> Result<&mut Self, PackageError> {
        validate_asset_name(name)?;
        if self.assets.contains_key(name) {
            return Err(PackageError::AssetDeclaredTwice(name.to_string()));
        }
        let digest = sha256::hex(&data);
        let size = data.len() as u64;
        self.manifest.assets.push(AssetRef {
            name: name.to_string(),
            digest,
            size,
            media_type: media_type.to_string(),
        });
        self.assets.insert(name.to_string(), data);
        Ok(self)
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), PackageError> {
        self.manifest.sign(key)
    }

    pub fn verify(&self) -> Result<String, PackageError> {
        self.manifest.verify()
    }

    pub fn validate(&self) -> Result<(), PackageError> {
        if self.manifest.schema_version != 1 {
            return Err(PackageError::UnsupportedSchema(
                self.manifest.schema_version,
            ));
        }
        if self.manifest.api_version > API_VERSION {
            return Err(PackageError::UnsupportedApi(self.manifest.api_version));
        }
        if !self.manifest.is_supported() {
            return Err(PackageError::UnsupportedSchema(
                self.manifest.schema_version,
            ));
        }
        if !self.assets.contains_key(&self.manifest.entry) {
            return Err(PackageError::MissingAsset(self.manifest.entry.clone()));
        }

        for asset in &self.manifest.assets {
            let data = self
                .assets
                .get(&asset.name)
                .ok_or_else(|| PackageError::MissingAsset(asset.name.clone()))?;
            if data.len() as u64 != asset.size {
                return Err(PackageError::SizeMismatch {
                    name: asset.name.clone(),
                    expected: asset.size,
                    actual: data.len() as u64,
                });
            }
            if sha256::hex(data) != asset.digest {
                return Err(PackageError::DigestMismatch(asset.name.clone()));
            }
        }

        Ok(())
    }

    pub fn save_to(&self, directory: impl AsRef<Path>) -> Result<(), PackageError> {
        self.validate()?;
        let directory = directory.as_ref();
        std::fs::create_dir_all(directory)?;
        std::fs::write(directory.join("manifest.json"), self.manifest.to_json()?)?;

        let assets = directory.join("assets");
        for (name, data) in &self.assets {
            let path = safe_join(&assets, name)?;
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(path, data)?;
        }
        Ok(())
    }

    pub fn load_from(directory: impl AsRef<Path>) -> Result<Self, PackageError> {
        let directory = directory.as_ref();
        let text = std::fs::read_to_string(directory.join("manifest.json"))?;
        let manifest = Manifest::from_json(&text)?;
        manifest.verify()?;

        let assets_root = directory.join("assets");
        let mut assets = HashMap::new();
        for asset in &manifest.assets {
            let path = safe_join(&assets_root, &asset.name)?;
            let data =
                std::fs::read(path).map_err(|_| PackageError::MissingAsset(asset.name.clone()))?;
            assets.insert(asset.name.clone(), data);
        }

        let package = Package::from_parts(manifest, assets);
        package.validate()?;
        Ok(package)
    }
}

impl AssetProvider for Package {
    fn read(&self, name: &str) -> Option<Vec<u8>> {
        self.assets.get(name).cloned()
    }

    fn list(&self) -> Vec<String> {
        let mut names: Vec<String> = self.assets.keys().cloned().collect();
        names.sort();
        names
    }
}

/// Sign arbitrary canonical bytes with a publisher key.
pub fn sign_payload(payload: &[u8], key: &SigningKey) -> PublisherSignature {
    let signature = key.sign(payload);
    PublisherSignature {
        algorithm: "ed25519".into(),
        public_key: hex::encode(key.verifying_key().to_bytes()),
        signature: hex::encode(signature.to_bytes()),
    }
}

/// Verify a detached signature over arbitrary canonical bytes.
pub fn verify_payload(payload: &[u8], signature: &PublisherSignature) -> Result<(), PackageError> {
    if signature.algorithm != "ed25519" {
        return Err(PackageError::Signature(format!(
            "unsupported signature algorithm {:?}",
            signature.algorithm
        )));
    }

    let public_key: [u8; 32] = hex::decode(&signature.public_key)
        .map_err(|error| PackageError::Signature(error.to_string()))?
        .try_into()
        .map_err(|_| PackageError::Signature("public key must be 32 bytes".into()))?;
    let signature_bytes: [u8; 64] = hex::decode(&signature.signature)
        .map_err(|error| PackageError::Signature(error.to_string()))?
        .try_into()
        .map_err(|_| PackageError::Signature("signature must be 64 bytes".into()))?;

    let verifying = VerifyingKey::from_bytes(&public_key)
        .map_err(|error| PackageError::Signature(error.to_string()))?;
    verifying
        .verify(payload, &EdSignature::from_bytes(&signature_bytes))
        .map_err(|_| PackageError::Signature("invalid signature".into()))?;
    Ok(())
}

fn kind_name(kind: PluginKind) -> &'static str {
    match kind {
        PluginKind::Instrument => "instrument",
        PluginKind::Effect => "effect",
        PluginKind::MidiEffect => "midi-effect",
    }
}

fn kind_from_name(name: &str) -> Option<PluginKind> {
    match name {
        "instrument" => Some(PluginKind::Instrument),
        "effect" => Some(PluginKind::Effect),
        "midi-effect" => Some(PluginKind::MidiEffect),
        _ => None,
    }
}

fn validate_asset_name(name: &str) -> Result<(), PackageError> {
    if name.is_empty() {
        return Err(PackageError::UnsafePath(name.to_string()));
    }
    let path = Path::new(name);
    if path.is_absolute() {
        return Err(PackageError::UnsafePath(name.to_string()));
    }
    for component in path.components() {
        if !matches!(component, Component::Normal(_)) {
            return Err(PackageError::UnsafePath(name.to_string()));
        }
    }
    Ok(())
}

fn safe_join(root: &Path, name: &str) -> Result<PathBuf, PackageError> {
    validate_asset_name(name)?;
    Ok(root.join(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> Manifest {
        Manifest::new(
            "com.example.eq",
            "Example EQ",
            "1.0.0",
            PluginKind::Effect,
            "plugin.dylib",
        )
    }

    fn key() -> SigningKey {
        SigningKey::from_bytes(&[7u8; 32])
    }

    #[test]
    fn package_round_trips_assets() {
        let mut package = Package::new(manifest());
        package
            .add_asset("plugin.dylib", "application/octet-stream", b"code".to_vec())
            .unwrap();
        package
            .add_asset("ir.wav", "audio/wav", b"RIFFdata".to_vec())
            .unwrap();
        package.sign(&key()).unwrap();
        assert!(package.validate().is_ok());
        assert!(package.verify().is_ok());
        assert_eq!(package.read("ir.wav"), Some(b"RIFFdata".to_vec()));
        assert_eq!(
            package.manifest().assets[1].digest,
            sha256::hex(b"RIFFdata")
        );
    }

    #[test]
    fn rejects_tampered_assets_and_manifests() {
        let mut package = Package::new(manifest());
        package
            .add_asset("plugin.dylib", "application/octet-stream", b"code".to_vec())
            .unwrap();
        package.sign(&key()).unwrap();

        let mut assets = HashMap::new();
        assets.insert("plugin.dylib".to_string(), b"evil".to_vec());
        let tampered = Package::from_parts(package.manifest().clone(), assets);
        assert!(matches!(
            tampered.validate(),
            Err(PackageError::DigestMismatch(_))
        ));

        let mut renamed = package.manifest().clone();
        renamed.name = "Changed".into();
        let renamed = Package::from_parts(renamed, package_assets(&package));
        assert!(matches!(renamed.verify(), Err(PackageError::Signature(_))));
    }

    fn package_assets(package: &Package) -> HashMap<String, Vec<u8>> {
        package
            .list()
            .into_iter()
            .filter_map(|name| package.read(&name).map(|data| (name, data)))
            .collect()
    }

    #[test]
    fn save_and_load_round_trip() {
        let mut package = Package::new(manifest());
        package
            .add_asset("plugin.dylib", "application/octet-stream", b"code".to_vec())
            .unwrap();
        package.sign(&key()).unwrap();

        let directory = std::env::temp_dir().join(format!(
            "plugin-package-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        package.save_to(&directory).unwrap();
        let loaded = Package::load_from(&directory).unwrap();
        assert_eq!(loaded.manifest(), package.manifest());
        assert_eq!(loaded.read("plugin.dylib"), Some(b"code".to_vec()));
        let _ = std::fs::remove_dir_all(directory);
    }

    #[test]
    fn rejects_duplicate_and_unsafe_paths() {
        let mut package = Package::new(manifest());
        package
            .add_asset("plugin.dylib", "application/octet-stream", b"code".to_vec())
            .unwrap();
        assert!(matches!(
            package.add_asset("plugin.dylib", "x", b"y".to_vec()),
            Err(PackageError::AssetDeclaredTwice(_))
        ));
        assert!(matches!(
            package.add_asset("../evil", "x", b"y".to_vec()),
            Err(PackageError::UnsafePath(_))
        ));

        let mut future = manifest();
        future.schema_version = 99;
        let package = Package::new(future);
        assert!(matches!(
            package.validate(),
            Err(PackageError::UnsupportedSchema(99))
        ));
    }
}
