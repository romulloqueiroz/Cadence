use ed25519_dalek::SigningKey;
use host::chain::{render_note, Chain};
use host::demo::SinePlugin;
use host::NoteSpec;
use plugin_api::provider::PluginProvider;
use plugin_discovery::{Feed, Index, Labeler};
use plugin_package::{Manifest, Package};
use plugin_store::Store;
use rand_core::OsRng;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let Some(command) = args.first().map(String::as_str) else {
        return Err(usage().to_string());
    };

    match command {
        "keygen" => keygen(args.get(1).map(Path::new).ok_or("missing key directory")?),
        "pack" => pack(&args[1..]),
        "install" => install(&args[1..]),
        "list" => list(&args[1..]),
        "render" => render(&args[1..]),
        "feed" => feed_command(&args[1..]),
        "subscribe" => subscribe(&args[1..]),
        "discover" => discover(&args[1..]),
        "install-release" => install_release(&args[1..]),
        "index" => index_command(&args[1..]),
        "labeler" => labeler_command(&args[1..]),
        "search" => search(&args[1..]),
        "-h" | "--help" | "help" => {
            println!("{}", usage());
            Ok(())
        }
        other => Err(format!("unknown command {other}\n{}", usage())),
    }
}

fn feed_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("sign") => feed_sign(&args[1..]),
        _ => Err("usage: platform feed sign <key-file> <template.json> <out.json>".into()),
    }
}

fn feed_sign(args: &[String]) -> Result<(), String> {
    let key_path = args.first().ok_or("missing key file")?;
    let template = args.get(1).ok_or("missing feed template")?;
    let output = args.get(2).ok_or("missing output file")?;
    let key = load_key(Path::new(key_path))?;
    let text = fs::read_to_string(template).map_err(|error| format!("{template}: {error}"))?;
    let mut feed = Feed::from_json(&text).map_err(|error| error.to_string())?;
    feed.sign(&key).map_err(|error| error.to_string())?;
    fs::write(output, feed.to_json().map_err(|error| error.to_string())?)
        .map_err(|error| format!("{output}: {error}"))?;
    println!("signed feed {output}");
    Ok(())
}

fn subscribe(args: &[String]) -> Result<(), String> {
    let url = args.first().ok_or("missing feed URL")?;
    let index = Index::new(index_root(args));
    let feed = index.subscribe(url).map_err(|error| error.to_string())?;
    println!("subscribed {} ({})", feed.publisher, feed.public_key);
    for release in feed.releases {
        println!(
            "{}	{}	{}	{}",
            release.id, release.version, release.name, release.kind
        );
    }
    Ok(())
}

fn discover(args: &[String]) -> Result<(), String> {
    let index = Index::new(index_root(args));
    let include_blocked = args.iter().any(|argument| argument == "--all");
    let feeds = match index.refresh_all() {
        Ok(feeds) => feeds,
        Err(error) => {
            eprintln!("warning: refresh failed ({error}); using cached feeds");
            index.cached_feeds().map_err(|error| error.to_string())?
        }
    };
    let releases: Vec<_> = if feeds.is_empty() {
        index.releases().map_err(|error| error.to_string())?
    } else {
        feeds
            .into_iter()
            .flat_map(|feed| {
                feed.releases
                    .clone()
                    .into_iter()
                    .map(move |release| (feed.clone(), release))
            })
            .collect()
    };

    if releases.is_empty() {
        println!("no releases (subscribe to a feed or indexer first)");
        return Ok(());
    }
    for (feed, release) in releases {
        let blocked = index.is_blocked(&release.id).unwrap_or(false);
        if blocked && !include_blocked {
            continue;
        }
        println!(
            "{}\t{}\t{}\t{}\t{}{}",
            release.id,
            release.version,
            release.name,
            release.kind,
            feed.publisher,
            if blocked { "\tBLOCKED" } else { "" }
        );
    }
    Ok(())
}

fn search(args: &[String]) -> Result<(), String> {
    let query = args.first().ok_or("missing search query")?;
    let include_blocked = args.iter().any(|argument| argument == "--all");
    let index = Index::new(index_root(args));
    if let Err(error) = index.refresh_all() {
        eprintln!("warning: refresh failed ({error}); using cached feeds");
    }
    let results = index
        .search(query, include_blocked)
        .map_err(|error| error.to_string())?;
    if results.is_empty() {
        println!("no matches");
    }
    for (feed, release) in results {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            release.id, release.version, release.name, release.kind, feed.publisher
        );
    }
    Ok(())
}

fn index_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("add") => {
            let url = args.get(1).ok_or("missing indexer URL")?;
            let index = Index::new(index_root(args));
            let feeds = index.add_indexer(url).map_err(|error| error.to_string())?;
            println!("added indexer {url} ({} feeds)", feeds.len());
            Ok(())
        }
        Some("list") => {
            let index = Index::new(index_root(args));
            let indexers = index.indexers().map_err(|error| error.to_string())?;
            if indexers.is_empty() {
                println!("no indexers");
            }
            for indexer in indexers {
                println!("{}\t{}", indexer.name, indexer.url);
            }
            Ok(())
        }
        Some("refresh") => {
            let index = Index::new(index_root(args));
            let feeds = index
                .refresh_indexers()
                .map_err(|error| error.to_string())?;
            println!("refreshed {} feeds", feeds.len());
            Ok(())
        }
        _ => Err("usage: platform index <add|list|refresh> [url]".into()),
    }
}

fn labeler_command(args: &[String]) -> Result<(), String> {
    match args.first().map(String::as_str) {
        Some("sign") => {
            let key_path = args.get(1).ok_or("missing key file")?;
            let template = args.get(2).ok_or("missing labeler template")?;
            let output = args.get(3).ok_or("missing output file")?;
            let key = load_key(Path::new(key_path))?;
            let text =
                fs::read_to_string(template).map_err(|error| format!("{template}: {error}"))?;
            let mut labeler = Labeler::from_json(&text).map_err(|error| error.to_string())?;
            labeler.sign(&key).map_err(|error| error.to_string())?;
            fs::write(
                output,
                labeler.to_json().map_err(|error| error.to_string())?,
            )
            .map_err(|error| format!("{output}: {error}"))?;
            println!("signed labeler {output}");
            Ok(())
        }
        Some("add") => {
            let url = args.get(1).ok_or("missing labeler URL")?;
            let index = Index::new(index_root(args));
            let labeler = index.add_labeler(url).map_err(|error| error.to_string())?;
            println!(
                "added labeler {} ({}) with {} labels",
                labeler.name,
                labeler.public_key,
                labeler.labels.len()
            );
            Ok(())
        }
        Some("list") => {
            let index = Index::new(index_root(args));
            let labelers = index
                .labeler_subscriptions()
                .map_err(|error| error.to_string())?;
            if labelers.is_empty() {
                println!("no labelers");
            }
            for labeler in labelers {
                println!("{}\t{}\t{}", labeler.name, labeler.public_key, labeler.url);
            }
            Ok(())
        }
        Some("refresh") => {
            let index = Index::new(index_root(args));
            let labelers = index
                .refresh_labelers()
                .map_err(|error| error.to_string())?;
            println!("refreshed {} labelers", labelers.len());
            Ok(())
        }
        _ => Err("usage: platform labeler <add|list|refresh> [url]".into()),
    }
}

fn install_release(args: &[String]) -> Result<(), String> {
    let id = args.first().ok_or("missing plugin id")?;
    let index = Index::new(index_root(args));
    if let Err(error) = index.refresh() {
        eprintln!("warning: refresh failed ({error}); using cached feeds");
    }
    if index.is_blocked(id).unwrap_or(false)
        && !args.iter().any(|argument| argument == "--allow-blocked")
    {
        return Err(format!(
            "{id} is blocked by a subscribed labeler (use --allow-blocked to override)"
        ));
    }
    let (feed, release) = index.find(id).map_err(|error| error.to_string())?;
    let package = index
        .fetch_package(&feed, &release)
        .map_err(|error| error.to_string())?;
    let store = Store::new(store_root(args), worker_path(args));
    let record = store.install(&package).map_err(|error| error.to_string())?;
    println!(
        "installed {} {} -> {}",
        record.manifest.id,
        record.manifest.version,
        record.path.display()
    );
    Ok(())
}

fn keygen(directory: &Path) -> Result<(), String> {
    fs::create_dir_all(directory).map_err(|error| error.to_string())?;
    let key = SigningKey::generate(&mut OsRng);
    fs::write(directory.join("publisher.key"), hex::encode(key.to_bytes()))
        .map_err(|error| error.to_string())?;
    let public = hex::encode(key.verifying_key().to_bytes());
    fs::write(directory.join("publisher.pub"), &public).map_err(|error| error.to_string())?;
    println!("publisher public key: {public}");
    Ok(())
}

fn pack(args: &[String]) -> Result<(), String> {
    let key_path = args.first().ok_or("missing key file")?;
    let staging = PathBuf::from(args.get(1).ok_or("missing staging directory")?);
    let mut entry = None;
    let mut index = 2;
    while index < args.len() {
        if args[index] == "--entry" {
            entry = args.get(index + 1).cloned();
            index += 2;
        } else {
            return Err(format!("unknown option {}", args[index]));
        }
    }
    let entry = entry.ok_or("missing --entry <asset>")?;

    let key = load_key(Path::new(key_path))?;
    let text = fs::read_to_string(staging.join("manifest.json"))
        .map_err(|error| format!("{}: {error}", staging.display()))?;
    let mut manifest = Manifest::from_json(&text).map_err(|error| error.to_string())?;
    manifest.assets.clear();
    manifest.signature = None;
    manifest.entry = entry.clone();

    let mut package = Package::new(manifest);
    let assets_root = staging.join("assets");
    let mut files = Vec::new();
    collect_files(&assets_root, &assets_root, &mut files).map_err(|error| error.to_string())?;
    for (name, path) in files {
        let data = fs::read(&path).map_err(|error| format!("{}: {error}", path.display()))?;
        package
            .add_asset(&name, media_type(&name), data)
            .map_err(|error| error.to_string())?;
    }

    if !package
        .manifest()
        .assets
        .iter()
        .any(|asset| asset.name == entry)
    {
        return Err(format!("entry asset {entry:?} not found under assets/"));
    }

    package.sign(&key).map_err(|error| error.to_string())?;
    package
        .save_to(&staging)
        .map_err(|error| error.to_string())?;
    println!(
        "packed {} {} into {}",
        package.manifest().id,
        package.manifest().version,
        staging.display()
    );
    Ok(())
}

fn install(args: &[String]) -> Result<(), String> {
    let directory = args.first().ok_or("missing package directory")?;
    let store = Store::new(store_root(args), worker_path(args));
    let record = store
        .install_dir(directory)
        .map_err(|error| error.to_string())?;
    println!(
        "installed {} {} -> {}",
        record.manifest.id,
        record.manifest.version,
        record.path.display()
    );
    Ok(())
}

fn list(args: &[String]) -> Result<(), String> {
    let store = Store::new(store_root(args), worker_path(args));
    let records = store.scan().map_err(|error| error.to_string())?;
    if records.is_empty() {
        println!("no plugins installed");
        return Ok(());
    }
    for record in records {
        let publisher = record
            .manifest
            .signature
            .as_ref()
            .map(|signature| signature.public_key.chars().take(16).collect::<String>())
            .unwrap_or_else(|| "unsigned".into());
        println!(
            "{}\t{}\t{}\t{}\t{}",
            record.manifest.id,
            record.manifest.version,
            record.manifest.name,
            publisher,
            record.path.display()
        );
    }
    Ok(())
}

fn render(args: &[String]) -> Result<(), String> {
    let id = args.first().ok_or("missing plugin id")?;
    let output = args.get(1).ok_or("missing output wav")?;
    let mut note = 69u8;
    let mut velocity = 100u8;
    let mut seconds = 2.0f64;
    let mut rate = 48_000f64;
    let mut block = 256usize;
    let mut index = 2;
    while index < args.len() {
        let value = args.get(index + 1).cloned();
        match args[index].as_str() {
            "--note" => {
                note = value.and_then(|value| value.parse().ok()).unwrap_or(note);
                index += 2;
            }
            "--velocity" => {
                velocity = value
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(velocity);
                index += 2;
            }
            "--seconds" => {
                seconds = value
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(seconds);
                index += 2;
            }
            "--rate" => {
                rate = value.and_then(|value| value.parse().ok()).unwrap_or(rate);
                index += 2;
            }
            "--block" => {
                block = value.and_then(|value| value.parse().ok()).unwrap_or(block);
                index += 2;
            }
            "--store" | "--worker" => index += 2,
            other => return Err(format!("unknown option {other}")),
        }
    }

    let store = Store::new(store_root(args), worker_path(args));
    let record = store.find(id).map_err(|error| error.to_string())?;
    let plugin = store
        .provider(&record)
        .create()
        .map_err(|error| error.to_string())?;

    let mut chain = Chain::new(rate, block);
    chain.add(Box::new(SinePlugin::default()), false);
    chain.add(plugin, false);
    chain.prepare();

    let audio = render_note(
        &mut chain,
        NoteSpec::new(note, velocity, 0.1, seconds * 0.6),
        seconds,
    );
    audio
        .write_wav(output)
        .map_err(|error| format!("{output}: {error}"))?;
    println!(
        "{}: {} frames, peak {:.3}, rms {:.4} -> {output}",
        record.manifest.name,
        audio.frames(),
        audio.peak(),
        audio.rms()
    );
    Ok(())
}

fn load_key(path: &Path) -> Result<SigningKey, String> {
    let text = fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let bytes: [u8; 32] = hex::decode(text.trim())
        .map_err(|error| format!("{}: {error}", path.display()))?
        .try_into()
        .map_err(|_| format!("{}: expected a 32-byte key", path.display()))?;
    Ok(SigningKey::from_bytes(&bytes))
}

fn collect_files(
    root: &Path,
    directory: &Path,
    out: &mut Vec<(String, PathBuf)>,
) -> std::io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let path = entry?.path();
        if path.is_dir() {
            collect_files(root, &path, out)?;
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            out.push((relative, path));
        }
    }
    Ok(())
}

fn media_type(name: &str) -> &'static str {
    let lower = name.to_ascii_lowercase();
    if lower.ends_with(".wav") {
        "audio/wav"
    } else if lower.ends_with(".ncw") {
        "audio/x-ncw"
    } else if lower.ends_with(".dylib") || lower.ends_with(".so") || lower.ends_with(".dll") {
        "application/vnd.platform.plugin"
    } else {
        "application/octet-stream"
    }
}

fn option(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|argument| argument == name)
        .and_then(|index| args.get(index + 1))
        .cloned()
}

fn index_root(args: &[String]) -> PathBuf {
    option(args, "--index")
        .map(PathBuf::from)
        .unwrap_or_else(Index::default_directory)
}

fn store_root(args: &[String]) -> PathBuf {
    option(args, "--store")
        .map(PathBuf::from)
        .unwrap_or_else(Store::default_root)
}

fn worker_path(args: &[String]) -> PathBuf {
    if let Some(path) = option(args, "--worker") {
        return PathBuf::from(path);
    }
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            return directory.join(format!("plugin-worker{}", std::env::consts::EXE_SUFFIX));
        }
    }
    PathBuf::from("plugin-worker")
}

fn usage() -> &'static str {
    "usage:\n  platform keygen <dir>\n  platform pack <key-file> <staging-dir> --entry <asset>\n  platform install <package-dir> [--store <dir>] [--worker <path>]\n  platform list [--store <dir>]\n  platform render <id> <out.wav> [--store <dir>] [--note 69] [--seconds 2]\n  platform feed sign <key-file> <template.json> <out.json>\n  platform subscribe <feed-url> [--index <dir>]\n  platform discover [--index <dir>]\n  platform install-release <id> [--index <dir>] [--store <dir>] [--allow-blocked]\n  platform index <add|list|refresh> [url]\n  platform labeler <add|list|refresh> [url]\n  platform search <query> [--index <dir>] [--all]"
}
