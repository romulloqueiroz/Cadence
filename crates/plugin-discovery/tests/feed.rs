use ed25519_dalek::SigningKey;
use plugin_api::PluginKind;
use plugin_discovery::{Feed, Index, Release};
use plugin_package::{Manifest, Package};
use std::path::{Path, PathBuf};

fn key(byte: u8) -> SigningKey {
    SigningKey::from_bytes(&[byte; 32])
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "plugin-discovery-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ))
}

fn write_package(directory: &Path, id: &str, version: &str, signing: &SigningKey) {
    let mut package = Package::new(Manifest::new(
        id,
        "Test",
        version,
        PluginKind::Effect,
        "entry.bin",
    ));
    package
        .add_asset("entry.bin", "application/octet-stream", b"code".to_vec())
        .unwrap();
    package.sign(signing).unwrap();
    package.save_to(directory).unwrap();
}

fn feed_for(directory: &Path, id: &str, version: &str) -> Feed {
    Feed {
        schema_version: 1,
        publisher: "Test Publisher".into(),
        public_key: String::new(),
        updated: 0,
        releases: vec![Release {
            id: id.into(),
            name: "Test".into(),
            version: version.into(),
            kind: "effect".into(),
            manifest_url: directory
                .join("manifest.json")
                .to_string_lossy()
                .into_owned(),
            base_url: Some(directory.to_string_lossy().into_owned()),
            notes: None,
        }],
        signature: None,
    }
}

#[test]
fn subscribes_verifies_and_fetches() {
    let root = temp("feed");
    let package_dir = root.join("package");
    let signing = key(7);
    write_package(&package_dir, "com.example.test", "1.0.0", &signing);

    let mut feed = feed_for(&package_dir, "com.example.test", "1.0.0");
    feed.sign(&signing).unwrap();
    let feed_path = root.join("feed.json");
    std::fs::write(&feed_path, feed.to_json().unwrap()).unwrap();

    let index = Index::new(root.join("index"));
    let subscribed = index
        .subscribe(&feed_path.to_string_lossy())
        .expect("subscribe");
    assert_eq!(
        subscribed.public_key,
        hex::encode(signing.verifying_key().to_bytes())
    );
    assert_eq!(index.releases().expect("releases").len(), 1);

    let (feed, release) = index.find("com.example.test").expect("find");
    let package = index.fetch_package(&feed, &release).expect("package");
    assert_eq!(package.manifest().id, "com.example.test");
    assert!(package.validate().is_ok());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rejects_tampered_feed_and_key_change() {
    let root = temp("tamper");
    let package_dir = root.join("package");
    let signing = key(7);
    write_package(&package_dir, "com.example.test", "1.0.0", &signing);

    let feed_path = root.join("feed.json");
    let mut feed = feed_for(&package_dir, "com.example.test", "1.0.0");
    feed.sign(&signing).unwrap();
    std::fs::write(&feed_path, feed.to_json().unwrap()).unwrap();

    let index = Index::new(root.join("index"));
    index
        .subscribe(&feed_path.to_string_lossy())
        .expect("subscribe");

    let mut tampered = feed.clone();
    tampered.publisher = "Attacker".into();
    std::fs::write(&feed_path, tampered.to_json().unwrap()).unwrap();
    assert!(index.subscribe(&feed_path.to_string_lossy()).is_err());

    let mut rotated = feed_for(&package_dir, "com.example.test", "1.0.0");
    rotated.sign(&key(9)).unwrap();
    std::fs::write(&feed_path, rotated.to_json().unwrap()).unwrap();
    assert!(index.refresh().is_err());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn indexer_aggregates_and_labeler_filters() {
    use plugin_discovery::{Label, Labeler};

    let root = temp("moderation");
    let package_dir = root.join("package");
    let signing = key(7);
    write_package(&package_dir, "com.example.test", "1.0.0", &signing);

    let feed_path = root.join("feed.json");
    let mut feed = feed_for(&package_dir, "com.example.test", "1.0.0");
    feed.sign(&signing).unwrap();
    std::fs::write(&feed_path, feed.to_json().unwrap()).unwrap();

    let indexer_path = root.join("indexer.json");
    let indexer = serde_json::json!({
        "schema_version": 1,
        "name": "Test Index",
        "feeds": [feed_path.to_string_lossy()]
    });
    std::fs::write(&indexer_path, indexer.to_string()).unwrap();

    let index = Index::new(root.join("index"));
    let feeds = index
        .add_indexer(&indexer_path.to_string_lossy())
        .expect("indexer");
    assert_eq!(feeds.len(), 1);
    assert_eq!(index.search("test", false).expect("search").len(), 1);

    let labeler_key = key(9);
    let mut labeler = Labeler {
        schema_version: 1,
        name: "Safety".into(),
        public_key: String::new(),
        updated: 0,
        labels: vec![Label {
            id: "com.example.test".into(),
            label: "malware".into(),
            severity: 3,
            reason: Some("test".into()),
        }],
        signature: None,
    };
    labeler.sign(&labeler_key).unwrap();
    let labeler_path = root.join("labeler.json");
    std::fs::write(&labeler_path, labeler.to_json().unwrap()).unwrap();
    index
        .add_labeler(&labeler_path.to_string_lossy())
        .expect("labeler");

    assert!(index.is_blocked("com.example.test").unwrap());
    assert!(index.search("test", false).unwrap().is_empty());
    assert_eq!(index.search("test", true).unwrap().len(), 1);

    let mut tampered = labeler.clone();
    tampered.name = "Changed".into();
    let tampered_path = root.join("tampered-labeler.json");
    std::fs::write(&tampered_path, tampered.to_json().unwrap()).unwrap();
    assert!(index.add_labeler(&tampered_path.to_string_lossy()).is_err());

    let _ = std::fs::remove_dir_all(root);
}
