use nk_format::ni_container::{self, ChunkData};
use nk_format::{file_container, k2_monolith, preset_chunk, probe, Container, Document, Kind};
use std::path::{Path, PathBuf};

#[derive(Default)]
struct Counts {
    plain: usize,
    plain_zones: usize,
    containers: usize,
    encrypted: usize,
    playable: usize,
    file_containers: usize,
    k2_monoliths: usize,
    k2_samples: usize,
    collections: usize,
    errors: usize,
    zone_total: usize,
}

fn main() {
    let mut roots: Vec<PathBuf> = std::env::args().skip(1).map(PathBuf::from).collect();
    if roots.is_empty() {
        roots = default_roots();
    }

    let mut files = Vec::new();
    for root in &roots {
        walk(root, &mut files);
    }
    files.sort();
    files.dedup();

    println!("scanning {} files\n", files.len());
    let mut counts = Counts::default();

    for path in &files {
        match analyze(path) {
            Ok(info) => {
                report(&info);
                counts.apply(&info);
            }
            Err(error) => {
                counts.errors += 1;
                println!("[err] {error}\t{}", path.display());
            }
        }
    }

    println!("\n== summary ==");
    println!("files:             {}", files.len());
    println!(
        "plain XML:         {} ({} zones, playable 0)",
        counts.plain, counts.plain_zones
    );
    println!(
        "NI containers:     {} (encrypted {}, playable {}, zones {})",
        counts.containers, counts.encrypted, counts.playable, counts.zone_total
    );
    println!("file containers:   {}", counts.file_containers);
    println!(
        "K2 monoliths:      {} ({} samples extracted)",
        counts.k2_monoliths, counts.k2_samples
    );
    println!("collections (.nkc):{}", counts.collections);
    println!("errors:            {}", counts.errors);
}

struct Info {
    status: &'static str,
    kind: String,
    detail: String,
    path: PathBuf,
    zones: usize,
    container: bool,
    encrypted: bool,
    playable: bool,
    plain: bool,
    file_container: bool,
    k2: bool,
    collection: bool,
    k2_samples: usize,
}

fn report(info: &Info) {
    println!(
        "[{}] {} {} {}\t{}",
        info.status,
        info.kind,
        info.detail,
        if info.encrypted { "ENCRYPTED" } else { "" },
        info.path.display()
    );
}

impl Counts {
    fn apply(&mut self, info: &Info) {
        if info.plain {
            self.plain += 1;
            self.plain_zones += info.zones;
        }
        if info.container {
            self.containers += 1;
            self.zone_total += info.zones;
            if info.encrypted {
                self.encrypted += 1;
            }
            if info.playable {
                self.playable += 1;
            }
        }
        if info.file_container {
            self.file_containers += 1;
        }
        if info.k2 {
            self.k2_monoliths += 1;
            self.k2_samples += info.k2_samples;
        }
        if info.collection {
            self.collections += 1;
        }
    }
}

fn analyze(path: &Path) -> Result<Info, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("read: {e}"))?;
    let mut info = Info {
        status: "ok",
        kind: String::new(),
        detail: String::new(),
        path: path.to_path_buf(),
        zones: 0,
        container: false,
        encrypted: false,
        playable: false,
        plain: false,
        file_container: false,
        k2: false,
        collection: false,
        k2_samples: 0,
    };

    match probe(&bytes) {
        Kind::Plain => {
            let container = Container::parse(&bytes).map_err(|e| format!("plain: {e}"))?;
            let doc = Document::new(&container.root);
            let programs = doc.programs();
            let zones: usize = programs.iter().map(|p| p.zones().len()).sum();
            info.plain = true;
            info.kind = "plain-xml".into();
            info.detail = format!(
                "programs={} zones={zones} kind={:?}",
                programs.len(),
                doc.kind().unwrap_or("")
            );
            info.zones = zones;
            info.status = "ok";
        }
        Kind::Container => {
            let item = ni_container::parse(&bytes).map_err(|e| format!("container: {e}"))?;
            info.container = true;
            info.kind = "ni-container".into();

            let (app, version) = item
                .find_chunk(ni_container::CHUNK_AUTHORING_APPLICATION)
                .and_then(|c| match &c.data {
                    ChunkData::AuthoringApplication {
                        application,
                        application_version,
                        ..
                    } => Some((application.name(), application_version.clone())),
                    _ => None,
                })
                .unwrap_or(("?", "?".into()));

            let pids = item
                .find_chunk(ni_container::CHUNK_AUTHORIZATION)
                .and_then(|c| match &c.data {
                    ChunkData::Authorization { pids, .. } => Some(pids.len()),
                    _ => None,
                })
                .unwrap_or(0);

            let mut subtrees = Vec::new();
            item.find_all_chunks(ni_container::CHUNK_SUB_TREE_ITEM, &mut subtrees);
            let undecoded = subtrees
                .iter()
                .filter(|c| {
                    matches!(
                        c.data,
                        ChunkData::SubTree(ni_container::SubTree::CompressedUndecoded { .. })
                    )
                })
                .count();

            info.encrypted = pids > 0 || undecoded > 0;

            let mut programs = 0;
            let mut groups = 0;
            let mut zones = 0;
            let mut files = 0;
            let mut preset_ok = false;
            if let Some(chunk) = item.find_chunk(ni_container::CHUNK_PRESET_CHUNK_ITEM) {
                if let ChunkData::Preset { preset_data, .. } = &chunk.data {
                    if let Ok(chunks) = preset_chunk::parse_all(preset_data) {
                        let chunks = preset_chunk::expanded(&chunks);
                        let summary = preset_chunk::summary(&chunks);
                        programs = summary.programs;
                        groups = summary.groups;
                        zones = summary.zones;
                        files = summary.sample_files.len();
                        preset_ok = true;
                    }
                }
            }

            info.zones = zones;
            info.playable = preset_ok && zones > 0 && files > 0 && !info.encrypted;
            info.detail = format!(
                "app={app} {version} pids={pids} undecoded_subtrees={undecoded} prog={programs} grp={groups} zone={zones} files={files}"
            );
            if info.encrypted {
                info.status = "enc";
            } else if !info.playable {
                info.status = "part";
            }
        }
        Kind::FileContainer => {
            let container =
                file_container::parse(&bytes).map_err(|e| format!("file container: {e}"))?;
            info.file_container = true;
            info.kind = "file-container".into();
            let nki = container
                .files
                .iter()
                .filter(|f| {
                    let n = f.name.to_ascii_lowercase();
                    n.ends_with(".nki") || n.ends_with(".nkm")
                })
                .count();
            let samples = container
                .files
                .iter()
                .filter(|f| {
                    let n = f.name.to_ascii_lowercase();
                    n.ends_with(".ncw") || n.ends_with(".wav")
                })
                .count();
            info.detail = format!(
                "files={} instruments={nki} samples={samples}",
                container.files.len()
            );
            info.status = "part";
        }
        Kind::K2Monolith => {
            let entries = k2_monolith::parse_tree(&bytes).map_err(|e| format!("nkr: {e}"))?;
            let samples = k2_monolith::scan_named_samples(&bytes)
                .map(|s| s.len())
                .unwrap_or(0);
            info.k2 = true;
            info.k2_samples = samples;
            info.kind = "k2-monolith".into();
            info.detail = format!("entries={} samples={samples}", entries.len());
            info.status = "info";
        }
        Kind::Collection => {
            info.collection = true;
            info.kind = "nkc-collection".into();
            info.detail = "not decoded".into();
            info.status = "n/a";
        }
        Kind::Opaque { magic } => {
            return Err(format!("unknown magic 0x{magic:08X}"));
        }
        Kind::TooShort => return Err("too short".into()),
    }

    Ok(info)
}

fn default_roots() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Library/Application Support/Native Instruments"),
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Library/Application Support/Native Instruments"),
        PathBuf::from(std::env::var("HOME").unwrap_or_default())
            .join("Documents/Native Instruments"),
        PathBuf::from("/Applications/Native Instruments"),
    ]
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            walk(&path, out);
        } else if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            if matches!(
                ext.to_ascii_lowercase().as_str(),
                "nki" | "nkm" | "nkc" | "nkr" | "nkx"
            ) {
                out.push(path);
            }
        }
    }
}
