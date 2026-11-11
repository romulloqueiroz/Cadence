//! Local plugin store: install signed packages, scan them, and create sandboxed
//! plugin instances.

use plugin_api::provider::{Format, LoadError, PluginProvider};
use plugin_api::Plugin;
use plugin_package::{Manifest, Package, PackageError};
use plugin_sandbox::SandboxPlugin;
use std::fmt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum StoreError {
    Package(PackageError),
    Io(std::io::Error),
    InvalidName(String),
    NotFound(String),
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::Package(error) => write!(f, "{error}"),
            StoreError::Io(error) => write!(f, "I/O error: {error}"),
            StoreError::InvalidName(name) => write!(f, "invalid package name {name:?}"),
            StoreError::NotFound(name) => write!(f, "not installed: {name}"),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<PackageError> for StoreError {
    fn from(error: PackageError) -> Self {
        StoreError::Package(error)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(error: std::io::Error) -> Self {
        StoreError::Io(error)
    }
}

#[derive(Debug, Clone)]
pub struct InstalledRecord {
    pub manifest: Manifest,
    pub path: PathBuf,
}

impl InstalledRecord {
    pub fn entry_path(&self) -> PathBuf {
        self.path.join("assets").join(&self.manifest.entry)
    }
}

pub struct Store {
    root: PathBuf,
    worker: PathBuf,
}

impl Store {
    pub fn new(root: impl Into<PathBuf>, worker: impl Into<PathBuf>) -> Self {
        Store {
            root: root.into(),
            worker: worker.into(),
        }
    }

    pub fn default_root() -> PathBuf {
        if let Ok(home) = std::env::var("PLATFORM_HOME") {
            return PathBuf::from(home).join("plugins");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local/share/platform/plugins");
        }
        PathBuf::from("platform-plugins")
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn install_dir(&self, directory: impl AsRef<Path>) -> Result<InstalledRecord, StoreError> {
        let package = Package::load_from(directory)?;
        self.install(&package)
    }

    pub fn install(&self, package: &Package) -> Result<InstalledRecord, StoreError> {
        package.validate()?;
        let public_key = package.verify()?;

        let id = sanitize(&package.manifest().id)?;
        let version = sanitize(&package.manifest().version)?;
        let publisher = format!("key-{}", &public_key[..public_key.len().min(16)]);
        let destination = self.root.join(&publisher).join(&id).join(&version);

        let temporary = self.root.join(format!(
            ".tmp-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));

        if temporary.exists() {
            std::fs::remove_dir_all(&temporary)?;
        }
        package.save_to(&temporary)?;

        if destination.exists() {
            std::fs::remove_dir_all(&destination)?;
        }
        if let Some(parent) = destination.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::rename(&temporary, &destination)?;

        Ok(InstalledRecord {
            manifest: package.manifest().clone(),
            path: destination,
        })
    }

    pub fn scan(&self) -> Result<Vec<InstalledRecord>, StoreError> {
        let mut records = Vec::new();
        if !self.root.exists() {
            return Ok(records);
        }
        visit(&self.root, &mut records);
        records.sort_by(|a, b| {
            a.manifest
                .id
                .cmp(&b.manifest.id)
                .then_with(|| a.manifest.version.cmp(&b.manifest.version))
        });
        Ok(records)
    }

    pub fn find(&self, id: &str) -> Result<InstalledRecord, StoreError> {
        let records = self.scan()?;
        records
            .into_iter()
            .filter(|record| record.manifest.id == id)
            .max_by(|a, b| a.manifest.version.cmp(&b.manifest.version))
            .ok_or_else(|| StoreError::NotFound(id.to_string()))
    }

    pub fn provider(&self, record: &InstalledRecord) -> StorePluginProvider {
        StorePluginProvider {
            record: record.clone(),
            worker: self.worker.clone(),
        }
    }
}

pub struct StorePluginProvider {
    record: InstalledRecord,
    worker: PathBuf,
}

impl PluginProvider for StorePluginProvider {
    fn format(&self) -> Format {
        Format::Rehosted
    }

    fn id(&self) -> &str {
        &self.record.manifest.id
    }

    fn create(&self) -> Result<Box<dyn Plugin>, LoadError> {
        let entry = self.record.entry_path();
        if !entry.exists() {
            return Err(LoadError::NotFound(entry.display().to_string()));
        }
        let plugin = SandboxPlugin::spawn(&self.worker, &entry)
            .map_err(|error| LoadError::Failed(error.to_string()))?;
        Ok(Box::new(plugin))
    }
}

fn visit(directory: &Path, records: &mut Vec<InstalledRecord>) {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            visit(&path, records);
        } else if path.file_name().and_then(|name| name.to_str()) == Some("manifest.json") {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(manifest) = Manifest::from_json(&text) else {
                continue;
            };
            if manifest.verify().is_err() {
                continue;
            }
            if let Some(parent) = path.parent() {
                records.push(InstalledRecord {
                    manifest,
                    path: parent.to_path_buf(),
                });
            }
        }
    }
}

fn sanitize(name: &str) -> Result<String, StoreError> {
    if name.is_empty()
        || !name.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_')
        })
    {
        return Err(StoreError::InvalidName(name.to_string()));
    }
    Ok(name.to_string())
}
