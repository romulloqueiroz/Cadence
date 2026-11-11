use host::demo::SinePlugin;
use host::{render_note, Host, NoteSpec};

#[test]
fn renders_a_finite_note() {
    let mut host = Host::new(Box::new(SinePlugin::default()), 48_000.0, 256);
    host.prepare();

    let audio = render_note(&mut host, NoteSpec::new(69, 100, 0.0, 0.5), 1.0);
    assert_eq!(audio.frames(), 48_000);
    assert!(audio.peak() > 0.05, "peak {}", audio.peak());
    assert!(audio.rms() > 0.001);
    assert!(audio
        .left
        .iter()
        .chain(&audio.right)
        .all(|v| v.is_finite() && v.abs() <= 1.0));
}

#[test]
fn output_is_block_size_invariant() {
    let note = NoteSpec::new(69, 100, 0.0, 0.512);

    let mut small = Host::new(Box::new(SinePlugin::default()), 48_000.0, 64);
    small.prepare();
    let a = render_note(&mut small, note, 1.0);

    let mut large = Host::new(Box::new(SinePlugin::default()), 48_000.0, 256);
    large.prepare();
    let b = render_note(&mut large, note, 1.0);

    assert_eq!(a.frames(), b.frames());
    let max_diff = a
        .left
        .iter()
        .zip(&b.left)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0f32, f32::max);
    assert!(max_diff < 1.0e-4, "max diff {max_diff}");
}

#[test]
fn note_off_releases_to_silence() {
    let mut host = Host::new(Box::new(SinePlugin::default()), 48_000.0, 256);
    host.prepare();

    let audio = render_note(&mut host, NoteSpec::new(69, 100, 0.0, 0.25), 1.0);
    let tail = &audio.left[20_000..];
    let tail_peak = tail.iter().fold(0.0f32, |peak, v| peak.max(v.abs()));
    assert!(tail_peak < 0.01, "tail peak {tail_peak}");
}
