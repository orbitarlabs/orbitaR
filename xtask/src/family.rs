use crate::Findings;
use crate::harness::datasets::{self, ACWF_FUNCTIONAL, DEFAULT_FAMILY, FAMILIES, portable};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write as _, path::Path};

pub(crate) const PACKAGED: &str = "python/orbitar/families";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Tuning {
    element: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Family(String),
    File(Own),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Own {
    file: String,
    sha256: String,
    cutoff_hint_ha: f64,
}

pub fn write(root: &Path, name: &str, from: &Path) -> Findings {
    match family(root, name, from) {
        Ok(()) => Vec::new(),
        Err(e) => vec![format!("family: {e}")],
    }
}

fn folded(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn family(root: &Path, name: &str, from: &Path) -> Result<(), String> {
    datasets::new_family(root, name)?;
    let snapshot = datasets::snapshot(root);
    let table = root.join(FAMILIES).join(format!("{name}.toml"));
    let link = root.join(PACKAGED).join(format!("{name}.toml"));
    datasets::no_linked_ancestor(root, &format!("{PACKAGED}/{name}.toml"))?;
    let links = match fs::symlink_metadata(root.join(PACKAGED)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        _ => datasets::listing(&root.join(PACKAGED), PACKAGED)?,
    };
    if (links.iter()).any(|(file, _)| folded(file) == folded(&format!("{name}.toml"))) {
        return Err(format!(
            "{name} exists; a family never changes, so give the new version a new name"
        ));
    }
    let path = root.join(from);
    let home = Path::new(FAMILIES).join(name);
    let shown = match path.strip_prefix(root) {
        Ok(relative) if relative.parent() == Some(home.as_path()) => relative.display().to_string(),
        _ => {
            return Err(format!(
                "{}: a tuning table of {name} lies in {}/, beside its own files",
                from.display(),
                home.display()
            ));
        }
    };
    for dir in [Path::new(FAMILIES), home.as_path()] {
        if !fs::symlink_metadata(root.join(dir)).is_ok_and(|m| m.is_dir()) {
            return Err(format!("{} is a link or no directory", dir.display()));
        }
    }
    let regular = |relative: &Path| match fs::symlink_metadata(root.join(relative)) {
        Ok(m) if !m.is_file() => Err(format!("{} is a link or no file", relative.display())),
        _ => Ok(()),
    };
    regular(Path::new(&shown))?;
    let text = String::from_utf8(datasets::regular_in(root, &shown)?)
        .map_err(|e| format!("{shown}: {e}"))?;
    let tuning: Tuning = toml::from_str(&text).map_err(|e| format!("{shown}: {e}"))?;
    let mut owns = Vec::new();
    let mut names = BTreeMap::new();
    for (element, entry) in tuning.element {
        let own = match entry {
            Entry::Family(base) if base == DEFAULT_FAMILY => continue,
            Entry::Family(other) => {
                return Err(format!(
                    "{shown}: {element} names {other:?}; a family of ours reads {DEFAULT_FAMILY} alone"
                ));
            }
            Entry::File(own) => own,
        };
        let base = own.file.as_str();
        if base != format!("{element}.xml") {
            return Err(format!(
                "{shown}: {element}: {base:?} must be {element}.xml"
            ));
        }
        if !portable(base) {
            return Err(format!(
                "{shown}: {element}: {base:?}: an own file name holds letters, digits, `.`, `-`, `_`"
            ));
        }
        if let Some(seen) = names.insert(folded(base), base.to_owned()) {
            return Err(format!(
                "{shown}: two own files are named {seen} and {base}; the family directory holds one"
            ));
        }
        owns.push((element, own));
    }
    let mut rows = String::new();
    for (element, own) in owns {
        let base = own.file.as_str();
        regular(&home.join(base))?;
        let file = root.join(&home).join(base);
        let bytes = datasets::regular_in(root, &home.join(base).to_string_lossy())
            .map_err(|e| format!("{}: {e}", file.display()))?;
        let sha = crate::lock::hex(&Sha256::digest(&bytes));
        if sha != own.sha256 {
            return Err(format!(
                "{}: sha256 {sha}, but {shown} gives {}",
                file.display(),
                own.sha256
            ));
        }
        rows.push_str(&format!(
            "{element} = {{ file = \"{name}/{base}\", sha256 = \"{sha}\", cutoff_hint_ha = {:?} }}\n",
            own.cutoff_hint_ha
        ));
    }
    if rows.is_empty() {
        return Err(format!("{shown}: no row names a file of its own"));
    }
    let body = format!(
        "# Written by `cargo xtask family {name} --from {shown}`; a family never changes.\n\
         base = \"{DEFAULT_FAMILY}\"\nfunctional = \"{ACWF_FUNCTIONAL}\"\n\n[element]\n{rows}"
    );
    snapshot.check(root, name, &body)?;
    for dir in [FAMILIES, PACKAGED] {
        fs::create_dir_all(root.join(dir)).map_err(|e| format!("{dir}: {e}"))?;
    }
    let mut file = (fs::OpenOptions::new().write(true).create_new(true))
        .open(&table)
        .map_err(|e| format!("{}: {e}", table.display()))?;
    let target = format!("../../../{FAMILIES}/{name}.toml");
    let written = (file.write_all(body.as_bytes()))
        .map_err(|e| format!("{}: {e}", table.display()))
        .and_then(|()| {
            std::os::unix::fs::symlink(&target, &link)
                .map_err(|e| format!("{}: {e}", link.display()))
        });
    if written.is_err() {
        let _ = fs::remove_file(&table);
    }
    written
}
