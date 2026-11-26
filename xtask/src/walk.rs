use std::{fs, path::Path};
use walkdir::WalkDir;

pub(crate) const SKIP_AT_ROOT: [&str; 4] = [".git", "target", "scratch", "node_modules"];

pub(crate) fn files(root: &Path, under: &str) -> Vec<String> {
    list(root, under).regular_files
}

pub(crate) fn links(root: &Path, under: &str) -> Vec<String> {
    list(root, under).symlinks
}

pub(crate) fn others(root: &Path, under: &str) -> Vec<String> {
    list(root, under).special_entries
}

pub(crate) fn faults(root: &Path, under: &str) -> Vec<String> {
    list(root, under).faults
}

#[derive(Default)]
struct Listing {
    regular_files: Vec<String>,
    symlinks: Vec<String>,
    special_entries: Vec<String>,
    faults: Vec<String>,
}

fn list(root: &Path, under: &str) -> Listing {
    let start = if under == "." {
        root.to_path_buf()
    } else {
        root.join(under)
    };
    let relative = |path: &Path| -> Result<String, String> {
        let rel = path.strip_prefix(root).map_err(|e| e.to_string())?;
        let text = rel
            .to_str()
            .ok_or_else(|| format!("{} is no UTF-8 name", rel.to_string_lossy()))?;
        match text.contains('\\') {
            true => Err(format!("{text} holds a backslash, which no lever rewrites")),
            false => Ok(text.to_owned()),
        }
    };
    let kept = |entry: &walkdir::DirEntry| {
        let skipped = |rel: String| {
            SKIP_AT_ROOT.contains(&rel.as_str()) && (entry.file_type().is_dir() || rel == ".git")
        };
        (entry.path().strip_prefix(root))
            .is_ok_and(|rel| !skipped(rel.to_string_lossy().into_owned()))
    };
    let mut out = Listing::default();
    if fs::symlink_metadata(&start).is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
        return out;
    }
    for entry in WalkDir::new(start).into_iter().filter_entry(kept) {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                let at = (e.path())
                    .and_then(|p| p.strip_prefix(root).ok())
                    .map(|p| p.to_string_lossy().into_owned())
                    .unwrap_or_else(|| under.to_owned());
                let why = (e.io_error()).map_or_else(|| e.to_string(), ToString::to_string);
                out.faults.push(format!("{at} cannot be walked: {why}"));
                continue;
            }
        };
        let rel = match relative(entry.path()) {
            Ok(rel) => rel,
            Err(e) => {
                out.faults.push(e);
                continue;
            }
        };
        if entry.path_is_symlink() {
            out.symlinks.push(rel);
        } else if entry.file_type().is_file() {
            out.regular_files.push(rel);
        } else if !entry.file_type().is_dir() {
            out.special_entries.push(rel);
        }
    }
    out.regular_files.sort();
    out.symlinks.sort();
    out.special_entries.sort();
    out.faults.sort();
    out
}

pub(crate) fn read(root: &Path, rel: &str) -> String {
    fs::read(root.join(rel))
        .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
        .unwrap_or_default()
}
