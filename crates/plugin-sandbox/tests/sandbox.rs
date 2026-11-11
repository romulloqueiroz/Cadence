use host::chain::{render_note, Chain};
use host::demo::SinePlugin;
use host::NoteSpec;
use plugin_api::Plugin;
use plugin_sandbox::SandboxPlugin;
use std::path::PathBuf;

fn binaries() -> Option<(PathBuf, PathBuf)> {
    let base = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/debug");
    let worker = base.join(format!("plugin-worker{}", std::env::consts::EXE_SUFFIX));
    let plugin = base.join(format!(
        "{}gain{}",
        std::env::consts::DLL_PREFIX,
        std::env::consts::DLL_SUFFIX
    ));
    if worker.exists() && plugin.exists() {
        Some((worker, plugin))
    } else {
        None
    }
}

#[test]
fn sandbox_round_trip_if_built() {
    let Some((worker, plugin)) = binaries() else {
        return;
    };

    let mut sandbox = SandboxPlugin::spawn(&worker, &plugin).expect("spawn worker");
    assert_eq!(sandbox.descriptor().id, "com.example.gain");
    assert_eq!(sandbox.capabilities().inputs, 2);

    let state = sandbox.save_state();
    sandbox.load_state(&state).expect("state loads");
    assert_eq!(sandbox.save_state(), state);

    let mut chain = Chain::new(48_000.0, 256);
    chain.add(Box::new(SinePlugin::default()), false);
    chain.add(Box::new(sandbox), false);
    chain.prepare();

    let audio = render_note(&mut chain, NoteSpec::new(69, 100, 0.0, 0.5), 1.0);
    assert!(audio.peak() > 0.05, "peak {}", audio.peak());
    assert!(audio
        .left
        .iter()
        .chain(&audio.right)
        .all(|value| value.is_finite()));
}
