use nk_format::loader::LoadedInstrument;
use std::{fs, io::Write, path::PathBuf};

struct Fixture {
    root: PathBuf,
    instrument: PathBuf,
}
impl Fixture {
    fn new() -> Self {
        Self::xml(include_str!("fixtures/plain-playback.xml"))
    }
    fn xml(xml: &str) -> Self {
        let root = std::env::temp_dir().join(format!(
            "cadence-relocation-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        let instrument = root.join("instrument.nki");
        let mut encoder =
            flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(xml.as_bytes()).unwrap();
        let mut bytes = vec![0; 0xaa];
        bytes[..4].copy_from_slice(&nk_format::PLAIN_MAGIC.to_le_bytes());
        bytes.extend(encoder.finish().unwrap());
        fs::write(&instrument, bytes).unwrap();
        Self { root, instrument }
    }
    fn file(&self, relative: &str) -> PathBuf {
        let path = self.root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, relative).unwrap();
        path
    }
    fn load(&self, roots: &[PathBuf]) -> LoadedInstrument {
        LoadedInstrument::load_with_sample_roots(&self.instrument, roots).unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

#[test]
fn explicit_recovery_finds_moved_sample_without_changing_instrument() {
    let f = Fixture::new();
    let original = fs::read(&f.instrument).unwrap();
    let recovered = f.file("new-library/Samples/tone Ω.wav");
    assert!(f
        .load(&[])
        .sample_location("Samples/tone Ω.wav")
        .unwrap_err()
        .to_string()
        .contains("Locate samples"));
    let roots = [f.root.join("new-library")];
    let mut loaded = f.load(&roots);
    let location = loaded.sample_location("Samples\\tone Ω.wav").unwrap();
    assert_eq!(location.path, recovered.canonicalize().unwrap());
    assert_eq!(location.read_all().unwrap(), fs::read(&recovered).unwrap());
    assert_eq!(fs::read(&f.instrument).unwrap(), original);
    // The serialized root settings can be replayed for project recall.
    assert_eq!(
        f.load(&roots)
            .sample_location("Samples/tone Ω.wav")
            .unwrap()
            .path,
        location.path
    );
}

#[test]
fn suffix_disambiguates_but_equal_matches_never_choose_arbitrarily() {
    let f = Fixture::new();
    let expected = f.file("library/A/Samples/tone Ω.wav");
    f.file("library/B/Other/tone Ω.wav");
    let roots = [f.root.join("library")];
    assert_eq!(
        f.load(&roots)
            .sample_location("/missing/A/Samples/tone Ω.wav")
            .unwrap()
            .path,
        expected.canonicalize().unwrap()
    );
    f.file("library/C/Samples/tone Ω.wav");
    let error = f
        .load(&roots)
        .sample_location("Samples/tone Ω.wav")
        .unwrap_err()
        .to_string();
    assert!(error.contains("Multiple samples match"), "{error}");
    assert!(f
        .load(&roots)
        .sample_location("Samples/absent.wav")
        .is_err());
}

#[test]
fn working_original_wins_and_recovery_is_bounded_to_selected_roots() {
    let f = Fixture::new();
    let original = f.file("Samples/tone Ω.wav");
    f.file("library/X/tone Ω.wav");
    f.file("library/Y/tone Ω.wav");
    let roots = [f.root.join("library")];
    assert_eq!(
        f.load(&roots)
            .sample_location("Samples/tone Ω.wav")
            .unwrap()
            .path,
        original
    );
    assert!(
        LoadedInstrument::load_with_sample_roots(&f.instrument, &vec![f.root.clone(); 9]).is_err()
    );
    assert!(LoadedInstrument::load_with_sample_roots(
        &f.instrument,
        std::slice::from_ref(&f.instrument)
    )
    .is_err());
    assert!(f.load(&roots).sample_location("outside.wav").is_err());
}

#[cfg(unix)]
#[test]
fn recovery_does_not_follow_directory_cycles_or_external_file_links() {
    let f = Fixture::new();
    let root = f.root.join("selected");
    fs::create_dir(&root).unwrap();
    let outside = f.file("outside.wav");
    std::os::unix::fs::symlink(&outside, root.join("escape.wav")).unwrap();
    std::os::unix::fs::symlink(&root, root.join("loop")).unwrap();
    assert!(f.load(&[root]).sample_location("escape.wav").is_err());
}

#[test]
fn notices_detect_script_data_without_claiming_it_executes() {
    let xml = include_str!("fixtures/plain-playback.xml").replace(
        "</K2_Container>",
        "<Scripts><Script>on note</Script></Scripts></K2_Container>",
    );
    let f = Fixture::xml(&xml);
    let loaded = f.load(&[]);
    assert!(loaded
        .compatibility_issues
        .iter()
        .any(|s| s.contains("script data") && s.contains("not executed")));
    assert!(Fixture::new().load(&[]).compatibility_issues.is_empty());
}
