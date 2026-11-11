use nk_format::file_container;
use nk_format::k2_monolith;
use nk_format::model;
use nk_format::ni_container::{self, ChunkData, Item, SubTree};
use nk_format::preset_chunk;
use nk_format::{probe, Container, Document, Kind};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

fn main() -> ExitCode {
    let mut path: Option<PathBuf> = None;
    let mut dump_xml = false;
    let mut as_json = false;
    let mut dump_tree = false;
    let mut preset_tree = false;
    let mut show_zones = false;

    for arg in std::env::args_os().skip(1) {
        match arg.to_str() {
            Some("--xml") => dump_xml = true,
            Some("--json") => as_json = true,
            Some("--tree") => dump_tree = true,
            Some("--preset-tree") => preset_tree = true,
            Some("--zones") => show_zones = true,
            Some("-h" | "--help") => {
                usage();
                return ExitCode::SUCCESS;
            }
            Some(other) if other.starts_with('-') => {
                eprintln!("unknown option: {other}");
                usage();
                return ExitCode::from(2);
            }
            _ => {
                if path.replace(PathBuf::from(arg)).is_some() {
                    eprintln!("only one file may be inspected at a time");
                    return ExitCode::from(2);
                }
            }
        }
    }

    let Some(path) = path else {
        usage();
        return ExitCode::from(2);
    };

    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    match probe(&bytes) {
        Kind::TooShort => {
            eprintln!(
                "{}: file is too short to be a Kontakt container",
                path.display()
            );
            return ExitCode::FAILURE;
        }
        Kind::Container => {
            let item = match ni_container::parse(&bytes) {
                Ok(item) => item,
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            };
            if as_json {
                print_container_json(&path, &item);
            } else {
                print_container_text(&path, &item, dump_tree, preset_tree, show_zones);
            }
            return ExitCode::SUCCESS;
        }
        Kind::FileContainer => {
            let container = match file_container::parse(&bytes) {
                Ok(container) => container,
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            };
            print_file_container(&path, &container);
            return ExitCode::SUCCESS;
        }
        Kind::K2Monolith => {
            let entries = match k2_monolith::parse_tree(&bytes) {
                Ok(entries) => entries,
                Err(e) => {
                    eprintln!("{}: {e}", path.display());
                    return ExitCode::FAILURE;
                }
            };
            println!("file:              {}", path.display());
            println!("kind:              k2-monolith");
            println!("entries:           {}", entries.len());
            for entry in entries.iter().take(200) {
                let extracted = if entry.kind == k2_monolith::EntryType::Sample {
                    k2_monolith::extract_sample(&bytes, entry)
                        .map(|data| format!(", {} bytes", data.len()))
                        .unwrap_or_default()
                } else {
                    String::new()
                };
                println!(
                    "  {} {:?} pointer=0x{:X}{}",
                    entry.kind.name(),
                    entry.name,
                    entry.pointer,
                    extracted
                );
            }
            if entries.len() > 200 {
                println!("  ... {} more", entries.len() - 200);
            }
            match k2_monolith::scan_named_samples(&bytes) {
                Ok(samples) => {
                    println!("embedded samples:  {}", samples.len());
                    for (name, data) in samples.iter().take(20) {
                        let kind = if data.starts_with(b"RIFF") {
                            "wav"
                        } else if data.len() >= 4
                            && u32::from_le_bytes(data[0..4].try_into().unwrap()) == 0xD69E_A801
                        {
                            "ncw"
                        } else {
                            "?"
                        };
                        println!("  {name:?} {} bytes ({kind})", data.len());
                    }
                }
                Err(e) => println!("embedded samples:  <error: {e}>"),
            }
            return ExitCode::SUCCESS;
        }
        Kind::Collection => {
            eprintln!(
                "{}: Kontakt collection container (.nkc); decoding is not implemented yet",
                path.display()
            );
            return ExitCode::from(3);
        }
        Kind::Opaque { magic } => {
            eprintln!(
                "{}: unrecognised container magic 0x{magic:08X}",
                path.display()
            );
            return ExitCode::from(3);
        }
        Kind::Plain => {}
    }

    let container = match Container::parse(&bytes) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("{}: {e}", path.display());
            return ExitCode::FAILURE;
        }
    };

    if dump_xml {
        print!("{}", container.xml);
        return ExitCode::SUCCESS;
    }

    let doc = Document::new(&container.root);
    if as_json {
        print_json(&path, &container, &doc);
    } else {
        print_text(&path, &container, &doc);
    }
    ExitCode::SUCCESS
}

fn print_file_container(path: &Path, container: &file_container::FileContainer) {
    println!("file:              {}", path.display());
    println!("kind:              ni-file-container");
    println!("files:             {}", container.files.len());
    for file in container.files.iter().take(100) {
        println!(
            "  [{}] {:?} {} bytes",
            file.index,
            file.name,
            file.data.len()
        );
    }
    if container.files.len() > 100 {
        println!("  ... {} more", container.files.len() - 100);
    }
}

fn print_container_text(
    path: &Path,
    item: &Item,
    dump_tree: bool,
    preset_tree: bool,
    show_zones: bool,
) {
    println!("file:              {}", path.display());
    println!("kind:              ni-container");
    println!("item version:      {}", item.version);
    println!("uuid:              {}", hex(&item.uuid));
    println!("chunks:            {}", count_chunks(item));
    println!("children:          {}", item.children.len());

    if let Some(chunk) = item.find_chunk(ni_container::CHUNK_AUTHORING_APPLICATION) {
        if let ChunkData::AuthoringApplication {
            compressed,
            application,
            application_id,
            application_version,
        } = &chunk.data
        {
            println!(
                "authoring app:     {} (id {application_id}), version {application_version}, compressed={compressed}",
                application.name()
            );
        }
    }

    if let Some(chunk) = item.find_chunk(ni_container::CHUNK_CONTAINER_ROOT) {
        if let ChunkData::Root {
            major,
            minor,
            patch,
        } = chunk.data
        {
            println!("ni sound version:  {major}.{minor}.{patch}");
        }
    }

    if let Some(chunk) = item.find_chunk(ni_container::CHUNK_AUTHORIZATION) {
        if let ChunkData::Authorization { pid_content, pids } = &chunk.data {
            if pids.is_empty() {
                println!("authorization:     pid_content={pid_content}, no serial PIDs");
            } else {
                println!(
                    "authorization:     pid_content={pid_content}, ENCRYPTED for {} PID(s): {}",
                    pids.len(),
                    pids.join(", ")
                );
            }
        }
    }

    if let Some(chunk) = item.find_chunk(ni_container::CHUNK_PRESET_CHUNK_ITEM) {
        if let ChunkData::Preset { preset_data, .. } = &chunk.data {
            println!("preset chunk:      {} bytes", preset_data.len());
            match preset_chunk::parse_all(preset_data) {
                Ok(chunks) => {
                    let chunks = preset_chunk::expanded(&chunks);
                    let summary = preset_chunk::summary(&chunks);
                    println!("programs:          {}", summary.programs);
                    println!("groups:            {}", summary.groups);
                    println!("zones:             {}", summary.zones);
                    println!("sample files:      {}", summary.sample_files.len());
                    for file in summary.sample_files.iter().take(8) {
                        println!("  {file}");
                    }
                    if summary.sample_files.len() > 8 {
                        println!("  ... {} more", summary.sample_files.len() - 8);
                    }
                    if preset_tree {
                        println!();
                        for chunk in &chunks {
                            print!("{}", chunk.dump());
                        }
                    }
                    if show_zones {
                        match model::build(&chunks) {
                            Ok(instrument) => {
                                let p = &instrument.program;
                                println!(
                                    "program:           {:?} transpose={} vol={:.3} pan={:+.3} tune={:+.3}",
                                    p.name, p.midi_transpose, p.volume, p.pan, p.tune
                                );
                                for (i, g) in instrument.groups.iter().enumerate() {
                                    let count = instrument
                                        .zones
                                        .iter()
                                        .filter(|z| z.group_index == i as u32)
                                        .count();
                                    println!(
                                        "group[{i}]          {:?} vol={:.3} pan={:+.3} tune={:+.3} zones={count} muted={} release={} env={}",
                                        g.name,
                                        g.volume,
                                        g.pan,
                                        g.tune,
                                        g.muted,
                                        g.release_trigger,
                                        g.envelope.as_ref().map_or("-".to_string(), |e| format!(
                                            "A{:.0} H{:.0} D{:.0} S{:.2} R{:.0}ms",
                                            e.attack, e.hold, e.decay, e.sustain, e.release
                                        ))
                                    );
                                }
                                let zones = &instrument.zones;
                                println!("zone detail:       {} decoded", zones.len());
                                for z in zones.iter().take(200) {
                                    let file = z
                                        .filename_id
                                        .and_then(|id| instrument.sample_files.get(id as usize))
                                        .map(String::as_str)
                                        .unwrap_or("");
                                    println!(
                                        "  g{:<3} key {:>3}-{:<3} vel {:>3}-{:<3} root {:>3} \
v{} {:>5}Hz {}ch {:>6}f gain {:.4} pan {:+.2} tune {:+.2} loops={} {}",
                                        z.group_index,
                                        z.low_key,
                                        z.high_key,
                                        z.low_velocity,
                                        z.high_velocity,
                                        z.root_key,
                                        z.version,
                                        z.sample_rate,
                                        z.channels,
                                        z.num_frames,
                                        instrument.zone_gain(z),
                                        instrument.zone_pan(z),
                                        instrument.zone_tune(z),
                                        z.loops.len(),
                                        file
                                    );
                                }
                                if zones.len() > 200 {
                                    println!("  ... {} more", zones.len() - 200);
                                }
                            }
                            Err(e) => println!("zone detail:       <model error: {e}>"),
                        }
                    }
                }
                Err(e) => println!("preset tree:       <parse error: {e}>"),
            }
        }
    }

    let mut subtrees = Vec::new();
    item.find_all_chunks(ni_container::CHUNK_SUB_TREE_ITEM, &mut subtrees);
    let compressed = subtrees
        .iter()
        .filter(|c| {
            matches!(
                c.data,
                ChunkData::SubTree(SubTree::CompressedUndecoded { .. })
            )
        })
        .count();
    println!(
        "subtrees:          {} ({} compressed/undecoded)",
        subtrees.len(),
        compressed
    );

    if dump_tree {
        println!("\n{}", item.dump());
    }
}

fn print_container_json(path: &Path, item: &Item) {
    let app = item
        .find_chunk(ni_container::CHUNK_AUTHORING_APPLICATION)
        .and_then(|c| match &c.data {
            ChunkData::AuthoringApplication {
                application,
                application_version,
                ..
            } => Some((application.name(), application_version.clone())),
            _ => None,
        });
    let pids = item
        .find_chunk(ni_container::CHUNK_AUTHORIZATION)
        .and_then(|c| match &c.data {
            ChunkData::Authorization { pids, .. } => Some(pids.clone()),
            _ => None,
        })
        .unwrap_or_default();
    let preset_bytes = item
        .find_chunk(ni_container::CHUNK_PRESET_CHUNK_ITEM)
        .and_then(|c| match &c.data {
            ChunkData::Preset { preset_data, .. } => Some(preset_data.len()),
            _ => None,
        });

    println!(
        "{{\"file\":{},\"kind\":\"ni-container\",\"itemVersion\":{},\"uuid\":{},\
         \"chunks\":{},\"children\":{},\"authoringApp\":{},\"authoringVersion\":{},\
         \"encryptedPids\":{},\"presetBytes\":{}}}",
        json_str(&path.to_string_lossy()),
        item.version,
        json_str(&hex(&item.uuid)),
        count_chunks(item),
        item.children.len(),
        app.as_ref().map_or("null".into(), |(n, _)| json_str(n)),
        app.as_ref().map_or("null".into(), |(_, v)| json_str(v)),
        pids.len(),
        preset_bytes.map_or("null".to_string(), |v| v.to_string()),
    );
}

fn count_chunks(item: &Item) -> usize {
    let mut count = 0;
    let mut current = Some(&item.data);
    while let Some(chunk) = current {
        count += 1;
        if let ChunkData::SubTree(SubTree::Decoded(subtree)) = &chunk.data {
            count += count_chunks(subtree);
        }
        current = chunk.next.as_deref();
    }
    for child in &item.children {
        count += count_chunks(&child.item);
    }
    count
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

fn print_text(path: &Path, c: &Container, doc: &Document<'_>) {
    let h = &c.header;
    let [a, b, cc, d] = h.app_version_octets();
    println!("file:              {}", path.display());
    println!("kind:              plain");
    println!("magic:             0x{:08X}", h.magic);
    println!("format version:    0x{:04X}", h.format_version);
    println!("flags:             {}", h.flags);
    println!(
        "app version:       0x{:08X} ({a}.{b}.{cc}.{d})",
        h.app_version
    );
    println!("app signature:     0x{:08X}", h.app_signature);
    println!("saved at:          {}", h.saved_at);
    println!(
        "payload:           zlib @ 0x{:X} -> {} bytes XML",
        h.payload_offset,
        c.xml.len()
    );
    println!(
        "container:         name={:?} type={:?} version={:?}",
        doc.name(),
        doc.kind().unwrap_or(""),
        doc.version().unwrap_or("")
    );

    let programs = doc.programs();
    println!("programs:          {}", programs.len());
    for (i, program) in programs.iter().enumerate() {
        let groups = program.groups();
        let zones = program.zones();
        let label = if program.name().is_empty() {
            "(unnamed)".to_string()
        } else {
            program.name().to_string()
        };
        println!(
            "  [{i}] {label:?} index={:?} groups={} zones={}",
            program.index(),
            groups.len(),
            zones.len()
        );
        for group in &groups {
            println!(
                "        group[{}] {:?}",
                group.index().unwrap_or_default(),
                group.name()
            );
        }
    }
}

fn print_json(path: &Path, c: &Container, doc: &Document<'_>) {
    let h = &c.header;
    let programs = doc.programs();
    let programs_json = programs
        .iter()
        .map(|p| {
            format!(
                "{{\"name\":{},\"index\":{},\"groups\":{},\"zones\":{}}}",
                json_str(p.name()),
                p.index().map_or("null".to_string(), |v| v.to_string()),
                p.groups().len(),
                p.zones().len()
            )
        })
        .collect::<Vec<_>>()
        .join(",");

    println!(
        "{{\"file\":{},\"kind\":\"plain\",\"magic\":\"0x{magic:08X}\",\
         \"formatVersion\":{version},\"appVersion\":{app},\"payloadOffset\":{off},\
         \"xmlBytes\":{xml},\"container\":{{\"name\":{name},\"type\":{typ},\"version\":{ver}}},\
         \"programs\":[{programs_json}]}}",
        json_str(&path.to_string_lossy()),
        magic = h.magic,
        version = h.format_version,
        app = h.app_version,
        off = h.payload_offset,
        xml = c.xml.len(),
        name = json_str(doc.name()),
        typ = json_str(doc.kind().unwrap_or("")),
        ver = json_str(doc.version().unwrap_or("")),
    );
}

fn json_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn usage() {
    eprintln!(
        "usage: nk-inspect <file.nki> [--xml] [--json] [--tree] [--preset-tree] [--zones]\n\
         \n\
         Reads a plain Kontakt instrument container, inflates its K2 XML, and\n\
         reports the header and program tree. --xml prints the raw XML (plain files); --tree dumps the NI container chunk tree; --preset-tree dumps the program/group/zone tree; --zones decodes zone parameters."
    );
}
