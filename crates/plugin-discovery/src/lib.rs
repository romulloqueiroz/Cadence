//! Decentralized discovery: signed publisher feeds and a local index.
//!
//! Creators host a feed (static JSON) and their packages (manifest + assets).
//! A user subscribes to a feed URL; the first fetch pins the publisher key.
//! Releases are fetched and verified against both the feed key and the
//! manifest signature before installation.

use ed25519_dalek::SigningKey;
use plugin_api::PluginKind;
use plugin_package::{
    sha256, sign_payload, verify_payload, Manifest, Package, PackageError, PublisherSignature,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use url::Url;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Feed {
    pub schema_version: u32,
    pub publisher: String,
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub releases: Vec<Release>,
    #[serde(default)]
    pub signature: Option<PublisherSignature>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Release {
    pub id: String,
    pub name: String,
    pub version: String,
    pub kind: String,
    pub manifest_url: String,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub notes: Option<String>,
}

impl Feed {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DiscoveryError> {
        let mut unsigned = self.clone();
        unsigned.signature = None;
        serde_json::to_vec(&unsigned).map_err(DiscoveryError::Json)
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), DiscoveryError> {
        self.signature = None;
        self.public_key = hex::encode(key.verifying_key().to_bytes());
        let bytes = self.canonical_bytes()?;
        self.signature = Some(sign_payload(&bytes, key));
        Ok(())
    }

    /// Verify the feed signature. Returns the publisher key hex.
    pub fn verify(&self) -> Result<String, DiscoveryError> {
        let signature = self
            .signature
            .as_ref()
            .ok_or_else(|| DiscoveryError::Invalid("feed is not signed".into()))?;
        if signature.public_key != self.public_key {
            return Err(DiscoveryError::PinnedKeyMismatch {
                expected: self.public_key.clone(),
                found: signature.public_key.clone(),
            });
        }
        verify_payload(&self.canonical_bytes()?, signature)?;
        Ok(self.public_key.clone())
    }

    pub fn to_json(&self) -> Result<String, DiscoveryError> {
        serde_json::to_string_pretty(self).map_err(DiscoveryError::Json)
    }

    pub fn from_json(text: &str) -> Result<Self, DiscoveryError> {
        serde_json::from_str(text).map_err(DiscoveryError::Json)
    }
}

impl Release {
    pub fn plugin_kind(&self) -> Option<PluginKind> {
        match self.kind.as_str() {
            "instrument" => Some(PluginKind::Instrument),
            "effect" => Some(PluginKind::Effect),
            "midi-effect" => Some(PluginKind::MidiEffect),
            _ => None,
        }
    }
}

#[derive(Debug)]
pub enum DiscoveryError {
    Io(std::io::Error),
    Fetch(String),
    Json(serde_json::Error),
    Package(PackageError),
    Invalid(String),
    PinnedKeyMismatch { expected: String, found: String },
    NotFound(String),
}

impl fmt::Display for DiscoveryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DiscoveryError::Io(error) => write!(f, "I/O error: {error}"),
            DiscoveryError::Fetch(message) => write!(f, "fetch failed: {message}"),
            DiscoveryError::Json(error) => write!(f, "JSON error: {error}"),
            DiscoveryError::Package(error) => write!(f, "{error}"),
            DiscoveryError::Invalid(message) => write!(f, "invalid feed: {message}"),
            DiscoveryError::PinnedKeyMismatch { expected, found } => write!(
                f,
                "publisher key changed: expected {expected}, found {found}"
            ),
            DiscoveryError::NotFound(id) => write!(f, "no release for {id}"),
        }
    }
}

impl std::error::Error for DiscoveryError {}

impl From<std::io::Error> for DiscoveryError {
    fn from(error: std::io::Error) -> Self {
        DiscoveryError::Io(error)
    }
}

impl From<serde_json::Error> for DiscoveryError {
    fn from(error: serde_json::Error) -> Self {
        DiscoveryError::Json(error)
    }
}

impl From<PackageError> for DiscoveryError {
    fn from(error: PackageError) -> Self {
        DiscoveryError::Package(error)
    }
}

pub trait Fetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, DiscoveryError>;
}

pub struct FileFetcher;

impl Fetcher for FileFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, DiscoveryError> {
        let path = url.strip_prefix("file://").unwrap_or(url);
        std::fs::read(path).map_err(|error| DiscoveryError::Fetch(format!("{path}: {error}")))
    }
}

pub struct HttpFetcher;

impl Fetcher for HttpFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, DiscoveryError> {
        let response = ureq::get(url)
            .call()
            .map_err(|error| DiscoveryError::Fetch(format!("{url}: {error}")))?;
        let mut bytes = Vec::new();
        response
            .into_reader()
            .read_to_end(&mut bytes)
            .map_err(|error| DiscoveryError::Fetch(format!("{url}: {error}")))?;
        Ok(bytes)
    }
}

pub struct DefaultFetcher;

impl Fetcher for DefaultFetcher {
    fn fetch(&self, url: &str) -> Result<Vec<u8>, DiscoveryError> {
        if url.starts_with("http://") || url.starts_with("https://") {
            HttpFetcher.fetch(url)
        } else {
            FileFetcher.fetch(url)
        }
    }
}

/// Fetch and verify a release package: manifest signature, publisher match,
/// declared id/version/kind, and every asset hash.
pub fn fetch_package(
    fetcher: &dyn Fetcher,
    feed: &Feed,
    release: &Release,
) -> Result<Package, DiscoveryError> {
    let text = String::from_utf8(fetcher.fetch(&release.manifest_url)?)
        .map_err(|_| DiscoveryError::Invalid("manifest is not UTF-8".into()))?;
    let manifest = Manifest::from_json(&text)?;
    let publisher = manifest.verify()?;
    if publisher != feed.public_key {
        return Err(DiscoveryError::PinnedKeyMismatch {
            expected: feed.public_key.clone(),
            found: publisher,
        });
    }
    if manifest.id != release.id || manifest.version != release.version {
        return Err(DiscoveryError::Invalid(format!(
            "manifest {} {} does not match release {} {}",
            manifest.id, manifest.version, release.id, release.version
        )));
    }
    if release
        .plugin_kind()
        .is_some_and(|kind| manifest.plugin_kind() != Some(kind))
    {
        return Err(DiscoveryError::Invalid(
            "release and manifest kinds differ".into(),
        ));
    }

    let base = release
        .base_url
        .clone()
        .unwrap_or_else(|| parent_url(&release.manifest_url));
    let mut assets = HashMap::new();
    for asset in &manifest.assets {
        let url = resolve(&base, &format!("assets/{}", asset.name))?;
        assets.insert(asset.name.clone(), fetcher.fetch(&url)?);
    }

    let package = Package::from_parts(manifest, assets);
    package.validate()?;
    Ok(package)
}

pub struct Index {
    directory: PathBuf,
    fetcher: Box<dyn Fetcher>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub url: String,
    pub public_key: String,
    pub publisher: String,
    pub updated: u64,
}

impl Index {
    pub fn new(directory: impl Into<PathBuf>) -> Self {
        Index {
            directory: directory.into(),
            fetcher: Box::new(DefaultFetcher),
        }
    }

    pub fn with_fetcher(directory: impl Into<PathBuf>, fetcher: Box<dyn Fetcher>) -> Self {
        Index {
            directory: directory.into(),
            fetcher,
        }
    }

    pub fn default_directory() -> PathBuf {
        if let Ok(home) = std::env::var("PLATFORM_HOME") {
            return PathBuf::from(home).join("index");
        }
        if let Ok(home) = std::env::var("HOME") {
            return PathBuf::from(home).join(".local/share/platform/index");
        }
        PathBuf::from("platform-index")
    }

    pub fn subscribe(&self, url: &str) -> Result<Feed, DiscoveryError> {
        let (feed, key) = self.load_feed(url)?;
        let mut subscriptions = self.subscriptions()?;

        if let Some(existing) = subscriptions.iter().find(|entry| entry.url == url) {
            if existing.public_key != key {
                return Err(DiscoveryError::PinnedKeyMismatch {
                    expected: existing.public_key.clone(),
                    found: key,
                });
            }
        } else {
            subscriptions.push(Subscription {
                url: url.to_string(),
                public_key: key.clone(),
                publisher: feed.publisher.clone(),
                updated: feed.updated,
            });
        }
        self.write_subscriptions(&subscriptions)?;
        self.cache_feed(url, &feed)?;
        Ok(feed)
    }

    pub fn refresh(&self) -> Result<Vec<Feed>, DiscoveryError> {
        let mut feeds = Vec::new();
        for subscription in self.subscriptions()? {
            let (feed, key) = self.load_feed(&subscription.url)?;
            if key != subscription.public_key {
                return Err(DiscoveryError::PinnedKeyMismatch {
                    expected: subscription.public_key,
                    found: key,
                });
            }
            self.cache_feed(&subscription.url, &feed)?;
            feeds.push(feed);
        }
        Ok(feeds)
    }

    pub fn cached_feeds(&self) -> Result<Vec<Feed>, DiscoveryError> {
        let directory = self.directory.join("feeds");
        let mut feeds = Vec::new();
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Ok(feeds);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(feed) = Feed::from_json(&text) else {
                continue;
            };
            if feed.verify().is_ok() {
                feeds.push(feed);
            }
        }
        Ok(feeds)
    }

    pub fn releases(&self) -> Result<Vec<(Feed, Release)>, DiscoveryError> {
        let mut releases = Vec::new();
        for feed in self.cached_feeds()? {
            for release in &feed.releases {
                releases.push((feed.clone(), release.clone()));
            }
        }
        Ok(releases)
    }

    pub fn find(&self, id: &str) -> Result<(Feed, Release), DiscoveryError> {
        self.releases()?
            .into_iter()
            .filter(|(_, release)| release.id == id)
            .max_by(|(_, a), (_, b)| version_key(&a.version).cmp(&version_key(&b.version)))
            .ok_or_else(|| DiscoveryError::NotFound(id.to_string()))
    }

    pub fn fetch_package(&self, feed: &Feed, release: &Release) -> Result<Package, DiscoveryError> {
        fetch_package(self.fetcher.as_ref(), feed, release)
    }

    fn subscriptions(&self) -> Result<Vec<Subscription>, DiscoveryError> {
        let path = self.directory.join("subscriptions.json");
        if !path.exists() {
            return Ok(Vec::new());
        }
        let text = std::fs::read_to_string(path)?;
        Ok(serde_json::from_str(&text)?)
    }

    fn write_subscriptions(&self, subscriptions: &[Subscription]) -> Result<(), DiscoveryError> {
        std::fs::create_dir_all(&self.directory)?;
        let text = serde_json::to_string_pretty(subscriptions)?;
        std::fs::write(self.directory.join("subscriptions.json"), text)?;
        Ok(())
    }

    fn cache_feed(&self, url: &str, feed: &Feed) -> Result<(), DiscoveryError> {
        let directory = self.directory.join("feeds");
        std::fs::create_dir_all(&directory)?;
        let filename = format!("{}.json", &sha256::hex(url.as_bytes())[..32]);
        std::fs::write(directory.join(filename), feed.to_json()?)?;
        Ok(())
    }

    fn load_feed(&self, url: &str) -> Result<(Feed, String), DiscoveryError> {
        let bytes = self.fetcher.fetch(url)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| DiscoveryError::Invalid("feed is not UTF-8".into()))?;
        let feed = Feed::from_json(&text)?;
        let key = feed.verify()?;
        Ok((feed, key))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexerDocument {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub feeds: Vec<String>,
}

impl IndexerDocument {
    pub fn to_json(&self) -> Result<String, DiscoveryError> {
        serde_json::to_string_pretty(self).map_err(DiscoveryError::Json)
    }

    pub fn from_json(text: &str) -> Result<Self, DiscoveryError> {
        serde_json::from_str(text).map_err(DiscoveryError::Json)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Label {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub severity: u8,
    #[serde(default)]
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Labeler {
    pub schema_version: u32,
    pub name: String,
    #[serde(default)]
    pub public_key: String,
    #[serde(default)]
    pub updated: u64,
    #[serde(default)]
    pub labels: Vec<Label>,
    #[serde(default)]
    pub signature: Option<PublisherSignature>,
}

impl Labeler {
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, DiscoveryError> {
        let mut unsigned = self.clone();
        unsigned.signature = None;
        serde_json::to_vec(&unsigned).map_err(DiscoveryError::Json)
    }

    pub fn sign(&mut self, key: &SigningKey) -> Result<(), DiscoveryError> {
        self.signature = None;
        self.public_key = hex::encode(key.verifying_key().to_bytes());
        let bytes = self.canonical_bytes()?;
        self.signature = Some(sign_payload(&bytes, key));
        Ok(())
    }

    pub fn verify(&self) -> Result<String, DiscoveryError> {
        let signature = self
            .signature
            .as_ref()
            .ok_or_else(|| DiscoveryError::Invalid("labeler is not signed".into()))?;
        if signature.public_key != self.public_key {
            return Err(DiscoveryError::PinnedKeyMismatch {
                expected: self.public_key.clone(),
                found: signature.public_key.clone(),
            });
        }
        verify_payload(&self.canonical_bytes()?, signature)?;
        Ok(self.public_key.clone())
    }

    pub fn to_json(&self) -> Result<String, DiscoveryError> {
        serde_json::to_string_pretty(self).map_err(DiscoveryError::Json)
    }

    pub fn from_json(text: &str) -> Result<Self, DiscoveryError> {
        serde_json::from_str(text).map_err(DiscoveryError::Json)
    }

    pub fn labels_for(&self, id: &str) -> Vec<Label> {
        self.labels
            .iter()
            .filter(|label| label.id == id)
            .cloned()
            .collect()
    }

    pub fn is_blocking(&self, id: &str) -> bool {
        self.labels_for(id).iter().any(is_blocking_label)
    }
}

fn is_blocking_label(label: &Label) -> bool {
    label.severity >= 3
        || matches!(
            label.label.to_ascii_lowercase().as_str(),
            "malware" | "blocked" | "scam" | "virus"
        )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexerSubscription {
    pub url: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LabelerSubscription {
    pub url: String,
    pub public_key: String,
    pub name: String,
}

impl Index {
    pub fn add_indexer(&self, url: &str) -> Result<Vec<Feed>, DiscoveryError> {
        let document = self.load_indexer(url)?;
        let mut feeds = Vec::new();
        for feed_url in &document.feeds {
            match self.subscribe(feed_url) {
                Ok(feed) => feeds.push(feed),
                Err(error) => eprintln!("feed {feed_url}: {error}"),
            }
        }

        let mut subscriptions = self.indexers()?;
        if !subscriptions.iter().any(|entry| entry.url == url) {
            subscriptions.push(IndexerSubscription {
                url: url.to_string(),
                name: document.name,
            });
            self.write_indexers(&subscriptions)?;
        }
        Ok(feeds)
    }

    pub fn indexers(&self) -> Result<Vec<IndexerSubscription>, DiscoveryError> {
        read_json(&self.directory.join("indexers.json"))
    }

    pub fn refresh_indexers(&self) -> Result<Vec<Feed>, DiscoveryError> {
        let mut feeds = Vec::new();
        for subscription in self.indexers()? {
            let document = self.load_indexer(&subscription.url)?;
            for feed_url in &document.feeds {
                if let Ok(feed) = self.subscribe(feed_url) {
                    feeds.push(feed);
                }
            }
        }
        Ok(feeds)
    }

    pub fn add_labeler(&self, url: &str) -> Result<Labeler, DiscoveryError> {
        let (labeler, key) = self.load_labeler(url)?;
        let mut subscriptions = self.labeler_subscriptions()?;
        if let Some(existing) = subscriptions.iter().find(|entry| entry.url == url) {
            if existing.public_key != key {
                return Err(DiscoveryError::PinnedKeyMismatch {
                    expected: existing.public_key.clone(),
                    found: key,
                });
            }
        } else {
            subscriptions.push(LabelerSubscription {
                url: url.to_string(),
                public_key: key.clone(),
                name: labeler.name.clone(),
            });
            self.write_labeler_subscriptions(&subscriptions)?;
        }
        self.cache_labeler(url, &labeler)?;
        Ok(labeler)
    }

    pub fn labeler_subscriptions(&self) -> Result<Vec<LabelerSubscription>, DiscoveryError> {
        read_json(&self.directory.join("labelers.json"))
    }

    pub fn refresh_labelers(&self) -> Result<Vec<Labeler>, DiscoveryError> {
        let mut labelers = Vec::new();
        for subscription in self.labeler_subscriptions()? {
            let (labeler, key) = self.load_labeler(&subscription.url)?;
            if key != subscription.public_key {
                return Err(DiscoveryError::PinnedKeyMismatch {
                    expected: subscription.public_key,
                    found: key,
                });
            }
            self.cache_labeler(&subscription.url, &labeler)?;
            labelers.push(labeler);
        }
        Ok(labelers)
    }

    pub fn cached_labelers(&self) -> Result<Vec<Labeler>, DiscoveryError> {
        let directory = self.directory.join("labelers");
        let mut labelers = Vec::new();
        let Ok(entries) = std::fs::read_dir(directory) else {
            return Ok(labelers);
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|value| value.to_str()) != Some("json") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(labeler) = Labeler::from_json(&text) else {
                continue;
            };
            if labeler.verify().is_ok() {
                labelers.push(labeler);
            }
        }
        Ok(labelers)
    }

    pub fn labels_for(&self, id: &str) -> Result<Vec<Label>, DiscoveryError> {
        let mut labels = Vec::new();
        for labeler in self.cached_labelers()? {
            labels.extend(labeler.labels_for(id));
        }
        Ok(labels)
    }

    pub fn is_blocked(&self, id: &str) -> Result<bool, DiscoveryError> {
        Ok(self.labels_for(id)?.iter().any(is_blocking_label))
    }

    pub fn search(
        &self,
        query: &str,
        include_blocked: bool,
    ) -> Result<Vec<(Feed, Release)>, DiscoveryError> {
        let query = query.to_ascii_lowercase();
        let released = self.releases()?;
        let mut results = Vec::new();
        for (feed, release) in released {
            let haystack = format!(
                "{} {} {} {} {}",
                release.id,
                release.name,
                release.version,
                release.notes.clone().unwrap_or_default(),
                feed.publisher
            )
            .to_ascii_lowercase();
            if !query.is_empty() && !haystack.contains(&query) {
                continue;
            }
            if !include_blocked && self.is_blocked(&release.id)? {
                continue;
            }
            results.push((feed, release));
        }
        Ok(results)
    }

    /// Refresh indexers, feeds and labelers in one pass.
    pub fn refresh_all(&self) -> Result<Vec<Feed>, DiscoveryError> {
        if let Err(error) = self.refresh_indexers() {
            eprintln!("indexers: {error}");
        }
        let feeds = self.refresh()?;
        if let Err(error) = self.refresh_labelers() {
            eprintln!("labelers: {error}");
        }
        Ok(feeds)
    }

    fn write_indexers(&self, subscriptions: &[IndexerSubscription]) -> Result<(), DiscoveryError> {
        std::fs::create_dir_all(&self.directory)?;
        std::fs::write(
            self.directory.join("indexers.json"),
            serde_json::to_string_pretty(subscriptions)?,
        )?;
        Ok(())
    }

    fn write_labeler_subscriptions(
        &self,
        subscriptions: &[LabelerSubscription],
    ) -> Result<(), DiscoveryError> {
        std::fs::create_dir_all(&self.directory)?;
        std::fs::write(
            self.directory.join("labelers.json"),
            serde_json::to_string_pretty(subscriptions)?,
        )?;
        Ok(())
    }

    fn load_indexer(&self, url: &str) -> Result<IndexerDocument, DiscoveryError> {
        let bytes = self.fetcher.fetch(url)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| DiscoveryError::Invalid("indexer is not UTF-8".into()))?;
        IndexerDocument::from_json(&text)
    }

    fn load_labeler(&self, url: &str) -> Result<(Labeler, String), DiscoveryError> {
        let bytes = self.fetcher.fetch(url)?;
        let text = String::from_utf8(bytes)
            .map_err(|_| DiscoveryError::Invalid("labeler is not UTF-8".into()))?;
        let labeler = Labeler::from_json(&text)?;
        let key = labeler.verify()?;
        Ok((labeler, key))
    }

    fn cache_labeler(&self, url: &str, labeler: &Labeler) -> Result<(), DiscoveryError> {
        let directory = self.directory.join("labelers");
        std::fs::create_dir_all(&directory)?;
        let filename = format!("{}.json", &sha256::hex(url.as_bytes())[..32]);
        std::fs::write(directory.join(filename), labeler.to_json()?)?;
        Ok(())
    }
}

fn read_json<T: for<'de> Deserialize<'de> + Default>(path: &Path) -> Result<T, DiscoveryError> {
    if !path.exists() {
        return Ok(T::default());
    }
    let text = std::fs::read_to_string(path)?;
    Ok(serde_json::from_str(&text)?)
}

fn version_key(version: &str) -> Vec<u64> {
    version
        .split(|character: char| !character.is_ascii_digit())
        .filter_map(|part| part.parse().ok())
        .collect()
}

fn is_remote(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://") || url.starts_with("file://")
}

fn parent_url(url: &str) -> String {
    if is_remote(url) {
        Url::parse(url)
            .ok()
            .and_then(|parsed| parsed.join(".").ok())
            .map(|parsed| parsed.to_string())
            .unwrap_or_else(|| url.to_string())
    } else {
        Path::new(url)
            .parent()
            .map(|parent| parent.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

fn resolve(base: &str, relative: &str) -> Result<String, DiscoveryError> {
    if is_remote(base) {
        let base = Url::parse(base)
            .map_err(|error| DiscoveryError::Invalid(format!("bad base URL {base}: {error}")))?;
        Ok(base
            .join(relative)
            .map_err(|error| DiscoveryError::Invalid(format!("bad URL join: {error}")))?
            .to_string())
    } else {
        Ok(Path::new(base)
            .join(relative)
            .to_string_lossy()
            .into_owned())
    }
}
