use ed25519_dalek::SigningKey;
use plugin_api::provider::PluginProvider;
use plugin_api::PluginKind;
use plugin_package::{Manifest, Package};
use plugin_store::Store;
use std::path::PathBuf;

fn key() -> SigningKey {
    SigningKey::from_bytes(&[7u8; 32])
}

fn temp(name: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "plugin-store-{name}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ))
}

fn package(id: &str, entry: &str, code: &[u8]) -> Package {
    let mut package = Package::new(Manifest::new(
        id,
        "Test",
        "1.0.0",
        PluginKind::Effect,
        entry,
    ));
    package
        .add_asset(entry, "application/octet-stream", code.to_vec())
        .unwrap();
    package.sign(&key()).unwrap();
    package
}

#[test]
fn installs_scans_and_finds() {
    let root = temp("install");
    let store = Store::new(&root, "plugin-worker");
    let record = store
        .install(&package("com.example.test", "entry.bin", b"data"))
        .expect("install");

    assert!(record.entry_path().exists());
    assert_eq!(store.scan().expect("scan").len(), 1);
    assert_eq!(
        store.find("com.example.test").expect("find").manifest.id,
        "com.example.test"
    );

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn rejects_tampered_manifest_and_unsafe_id() {
    let root = temp("reject");
    let store = Store::new(&root, "plugin-worker");

    let mut tampered = package("com.example.test", "entry.bin", b"data");
    tampered.manifest_mut().name = "Changed".into();
    assert!(store.install(&tampered).is_err());

    let mut unsafe_id = package("com.example.test", "entry.bin", b"data");
    unsafe_id.manifest_mut().id = "../evil".into();
    unsafe_id.sign(&key()).unwrap();
    assert!(store.install(&unsafe_id).is_err());

    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn sandboxed_render_from_store_if_built() {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug");
    let worker = base.join(format!("plugin-worker{}", std::env::consts::EXE_SUFFIX));
    let dylib = base.join(format!(
        "{}gain{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    if !worker.exists() || !dylib.exists() {
        return;
    }

    let code = std::fs::read(&dylib).expect("read dylib");
    let entry = format!("gain{}", std::env::consts::DLL_SUFFIX);
    let root = temp("render");
    let store = Store::new(&root, &worker);
    let record = store
        .install(&package("com.example.gain", &entry, &code))
        .expect("install");

    let plugin = store
        .provider(&record)
        .create()
        .expect("create sandboxed plugin");
    let mut chain = host::chain::Chain::new(48_000.0, 256);
    chain.add(Box::new(host::demo::SinePlugin::default()), false);
    chain.add(plugin, false);
    chain.prepare();
    let audio = host::chain::render_note(&mut chain, host::NoteSpec::new(69, 100, 0.0, 0.5), 1.0);
    assert!(audio.peak() > 0.05, "peak {}", audio.peak());

    let _ = std::fs::remove_dir_all(root);
}
