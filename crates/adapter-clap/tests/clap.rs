use adapter_clap::ClapModule;

const SPECTRUM: &str = "/Library/Audio/Plug-Ins/CLAP/MiniMeters - Spectrum.clap";

#[test]
fn loads_installed_clap_if_present() {
    use plugin_api::Plugin;

    if !std::path::Path::new(SPECTRUM).exists() {
        return;
    }
    let module = ClapModule::load(SPECTRUM).expect("load CLAP");
    let descriptors = module.descriptors();
    assert!(descriptors.iter().any(|d| d.name.contains("MiniMeters")));

    let plugin = module.create(None).expect("create instance");
    assert_eq!(plugin.capabilities().outputs, 2);
}
