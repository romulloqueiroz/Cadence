//! Explicit, load-time sample recovery inside folders selected by the user.
use crate::Error;
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

#[derive(Default)]
pub(crate) struct SampleRelocation {
    roots: Vec<PathBuf>,
    index: Option<HashMap<String, Vec<PathBuf>>>,
}

impl SampleRelocation {
    pub(crate) fn new(roots: &[PathBuf]) -> Result<Self, Error> {
        if roots.len() > 8 {
            return Err(bad("Choose at most eight sample folders"));
        }
        let mut selected = Vec::new();
        for root in roots {
            let root = root.canonicalize()?;
            if !root.is_dir() {
                return Err(bad("The selected sample location is not a folder"));
            }
            if !selected.contains(&root) {
                selected.push(root);
            }
        }
        Ok(Self {
            roots: selected,
            index: None,
        })
    }

    pub(crate) fn find(&mut self, reference: &str) -> Result<Option<PathBuf>, Error> {
        if self.roots.is_empty() {
            return Ok(None);
        }
        if self.index.is_none() {
            let mut index = HashMap::<String, Vec<PathBuf>>::new();
            let mut visited = 0;
            for root in &self.roots {
                visit(root, root, 0, &mut visited, &mut index)?;
            }
            self.index = Some(index);
        }
        let parts = components(reference);
        let Some(name) = parts.last() else {
            return Ok(None);
        };
        let Some(candidates) = self.index.as_ref().and_then(|i| i.get(name)) else {
            return Ok(None);
        };
        let mut best = 0;
        let mut matches = Vec::new();
        for path in candidates {
            let candidate = components(&path.to_string_lossy());
            let score = candidate
                .iter()
                .rev()
                .zip(parts.iter().rev())
                .take_while(|(a, b)| a == b)
                .count();
            if score > best {
                best = score;
                matches.clear();
            }
            if score == best && !matches.contains(path) {
                matches.push(path.clone());
            }
        }
        if matches.len() > 1 {
            return Err(bad(format!("Multiple samples match {reference:?}. Choose a narrower sample folder; no file was selected.")));
        }
        Ok(matches.pop())
    }
}

fn visit(
    root: &Path,
    directory: &Path,
    depth: usize,
    visited: &mut usize,
    index: &mut HashMap<String, Vec<PathBuf>>,
) -> Result<(), Error> {
    if depth > 32 {
        return Err(bad(
            "Sample folder exceeds 32 directory levels; choose a narrower folder",
        ));
    }
    for entry in std::fs::read_dir(directory)? {
        let entry = entry?;
        *visited += 1;
        if *visited > 100_000 {
            return Err(bad(
                "Sample search exceeds 100000 entries; choose a narrower folder",
            ));
        }
        let kind = entry.file_type()?;
        let path = entry.path();
        // Do not follow directory links out of, or back into, a selected tree.
        if kind.is_dir() {
            visit(root, &path, depth + 1, visited, index)?;
        } else if kind.is_file() || kind.is_symlink() {
            let extension = path
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "wav" | "ncw" | "nkx") {
                continue;
            }
            let resolved = path.canonicalize()?;
            if resolved.starts_with(root) && resolved.is_file() {
                if let Some(name) = path.file_name().and_then(|s| s.to_str()) {
                    index
                        .entry(name.to_ascii_lowercase())
                        .or_default()
                        .push(resolved);
                }
            }
        }
    }
    Ok(())
}

fn components(path: &str) -> Vec<String> {
    path.replace('\\', "/")
        .split('/')
        .filter(|s| !s.is_empty() && *s != "." && *s != "..")
        .map(str::to_ascii_lowercase)
        .collect()
}
fn bad(text: impl Into<String>) -> Error {
    Error::Malformed(text.into())
}
