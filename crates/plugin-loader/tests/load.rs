use plugin_api::Plugin;
use plugin_loader::LoadedPlugin;
use std::path::PathBuf;

#[test]
fn loads_native_gain_if_built() {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/debug")
        .join(format!(
            "{}gain{}",
            std::env::consts::DLL_PREFIX,
            std::env::consts::DLL_SUFFIX
        ));
    if !path.exists() {
        return;
    }

    let plugin = LoadedPlugin::load(&path).expect("plugin loads");
    assert_eq!(plugin.descriptor().id, "com.example.gain");
    assert_eq!(plugin.capabilities().inputs, 2);
}
