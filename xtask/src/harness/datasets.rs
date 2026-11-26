use super::rival::Rival;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs,
    path::{Component, Path, PathBuf},
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

pub(crate) const PATH: &str = "benchmarks/DATASETS.toml";
pub(crate) const FAMILIES: &str = "datasets";
pub(crate) const DEFAULT_FAMILY: &str = "jth-2.0";
pub(crate) const ACWF_FUNCTIONAL: &str = "pbe";
pub(crate) const DEFAULT_FUNCTIONAL: &str = "pbe";

#[derive(Deserialize)]
struct Pins {
    #[serde(default)]
    atompaw: Option<toml::Value>,
    #[serde(flatten)]
    rivals: BTreeMap<Rival, toml::Value>,
}

#[derive(Debug)]
pub(crate) struct Family {
    table: String,
    rows: Vec<FamilyRow>,
    tables: Vec<(String, Vec<u8>)>,
}

#[derive(Clone, Debug)]
struct FamilyRow {
    element: String,
    functional: String,
    path: String,
    sha256: String,
    url: Option<String>,
    hint_ha: Option<f64>,
}

#[derive(Deserialize)]
struct External {
    url: String,
    #[serde(flatten)]
    functionals: BTreeMap<String, Layout>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    layout: String,
    element: BTreeMap<String, Pin>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    sha256: String,
    cutoff_hint_ha: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ours {
    base: String,
    functional: String,
    element: BTreeMap<String, OwnFile>,
}

impl Family {
    pub(crate) fn read(root: &Path, name: &str) -> Result<Family, String> {
        Family::from(name, &|file: &str| {
            let path = format!("{FAMILIES}/{file}");
            regular_in(root, &path)
        })
    }

    fn from(
        name: &str,
        source: &dyn Fn(&str) -> Result<Vec<u8>, String>,
    ) -> Result<Family, String> {
        let table = format!("{FAMILIES}/{name}.toml");
        let at = |e: String| format!("{table}: {e}");
        let (value, bytes) = read_table(name, source)?;
        let mut tables = vec![(format!("{name}.toml"), bytes)];
        let rows = match value.contains_key("base") {
            false => external(value).map_err(at)?,
            true => {
                let ours: Ours = value.try_into().map_err(|e| at(format!("{e}")))?;
                section(&ours.functional).map_err(|e| at(format!("functional {e}")))?;
                for (element, own) in &ours.element {
                    engine_row(element, &own.sha256, Some(own.cutoff_hint_ha)).map_err(at)?;
                }
                let (base, bytes) = read_table(&ours.base, source)?;
                if base.contains_key("base") {
                    return Err(at(format!("base {:?} is a family of ours", ours.base)));
                }
                let base_rows =
                    external(base).map_err(|e| format!("{FAMILIES}/{}.toml: {e}", ours.base))?;
                tables.push((format!("{}.toml", ours.base), bytes));
                let mut rows: Vec<FamilyRow> = (base_rows.into_iter())
                    .filter(|row| {
                        row.functional == ours.functional
                            && !ours.element.contains_key(&row.element)
                    })
                    .collect();
                for (element, own) in ours.element {
                    rows.push(FamilyRow {
                        element,
                        functional: ours.functional.clone(),
                        path: own.file,
                        sha256: own.sha256,
                        url: None,
                        hint_ha: Some(own.cutoff_hint_ha),
                    });
                }
                rows
            }
        };
        if let Some(row) = rows.iter().find(|row| !plain(&row.path)) {
            return Err(at(format!(
                "{}: {:?} is not a relative path of plain names",
                row.element, row.path
            )));
        }
        Ok(Family {
            table,
            rows,
            tables,
        })
    }

    fn row(&self, functional: &str, element: &str) -> Option<&FamilyRow> {
        (self.rows.iter()).find(|row| row.functional == functional && row.element == element)
    }

    pub(crate) fn pins(&self) -> BTreeMap<String, String> {
        (self.rows.iter())
            .map(|row| (row.path.clone(), row.sha256.clone()))
            .collect()
    }

    pub(crate) fn hints(&self, functional: &str) -> BTreeMap<String, f64> {
        (self.rows.iter())
            .filter(|row| row.functional == functional)
            .filter_map(|row| Some((row.element.clone(), row.hint_ha?)))
            .collect()
    }

    fn bytes(&self, root: &Path, row: &FamilyRow) -> Result<Vec<u8>, String> {
        fetched(
            root,
            &self.table,
            row.url.as_deref(),
            &row.path,
            &row.sha256,
        )
    }
}

fn read_table(
    name: &str,
    source: &dyn Fn(&str) -> Result<Vec<u8>, String>,
) -> Result<(toml::Table, Vec<u8>), String> {
    if matches!(name, "" | "." | "..") || name.contains(['/', '\\']) {
        return Err(format!(
            "datasets: family {name:?} must be a file name: no / or \\, not empty, not . or .."
        ));
    }
    let bytes = source(&format!("{name}.toml"))?;
    let text =
        String::from_utf8(bytes.clone()).map_err(|e| format!("{FAMILIES}/{name}.toml: {e}"))?;
    let value = toml::from_str(&text).map_err(|e| format!("{FAMILIES}/{name}.toml: {e}"))?;
    Ok((value, bytes))
}

fn external(value: toml::Table) -> Result<Vec<FamilyRow>, String> {
    let external: External = value.try_into().map_err(|e| format!("{e}"))?;
    let mut rows = Vec::new();
    for (functional, layout) in external.functionals {
        section(&functional)?;
        if !layout.layout.contains("{element}") {
            return Err(format!(
                "{functional}.layout: {:?} has no {{element}}",
                layout.layout
            ));
        }
        for (element, pin) in layout.element {
            engine_row(&element, &pin.sha256, pin.cutoff_hint_ha)?;
            let path = layout.layout.replace("{element}", &element);
            if !plain(&path) {
                return Err(format!(
                    "{element}: {path:?} is not a relative path of plain names"
                ));
            }
            rows.push(FamilyRow {
                url: Some(format!("{}/{path}", external.url)),
                element,
                functional: functional.clone(),
                path,
                sha256: pin.sha256,
                hint_ha: pin.cutoff_hint_ha,
            });
        }
    }
    Ok(rows)
}

pub(crate) fn judged(root: &Path) -> Result<Vec<String>, String> {
    tables(root, DEFAULT_FAMILY)
}

pub(crate) fn case_tables(root: &Path, case: &[String]) -> Result<Vec<String>, String> {
    let mut out: Vec<String> = Vec::new();
    for file in case.iter().filter(|file| file.ends_with(".toml")) {
        let text = regular_in(root, file)?;
        let text = String::from_utf8(text).map_err(|e| format!("{file}: {e}"))?;
        let table = toml::from_str::<toml::Table>(&text).map_err(|e| format!("{file}: {e}"))?;
        let family = super::Input(&table).family();
        if family != DEFAULT_FAMILY {
            out.extend(
                tables(root, &family)
                    .map_err(|e| format!("{file} names family {family:?}: {e}"))?,
            );
        }
    }
    out.sort();
    out.dedup();
    Ok(out)
}

fn tables(root: &Path, name: &str) -> Result<Vec<String>, String> {
    let path = |file: &str| format!("{FAMILIES}/{file}");
    match Family::read(root, name) {
        Ok(family) => Ok(family.tables.iter().map(|(file, _)| path(file)).collect()),
        Err(e) if !(plain(name) && !name.contains(['/', '\\'])) => Err(e),
        Err(e) => {
            let absent = fs::symlink_metadata(root.join(path(&format!("{name}.toml"))))
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
            match absent {
                true => Ok(Vec::new()),
                false => Err(e),
            }
        }
    }
}

pub(crate) fn checked(path: &Path, sha256: &str) -> Result<Vec<u8>, String> {
    let bytes = fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let sha = digest(&bytes);
    match sha == sha256 {
        true => Ok(bytes),
        false => Err(format!(
            "{}: sha256 {sha}, but {PATH} pins {sha256}",
            path.display()
        )),
    }
}

pub(crate) fn rival_table(root: &Path, code: Rival) -> Result<toml::Value, String> {
    (pins(root)?.rivals.remove(&code))
        .ok_or_else(|| format!("{PATH} has no [{}] table", code.adapter().name()))
}

pub(crate) fn atompaw_table(root: &Path) -> Result<toml::Value, String> {
    (pins(root)?.atompaw).ok_or_else(|| format!("{PATH} has no [atompaw] table"))
}

const ELEMENTS: &str = "H He Li Be B C N O F Ne Na Mg Al Si P S Cl Ar K Ca Sc Ti V Cr Mn Fe Co Ni Cu Zn Ga Ge As Se \
    Br Kr Rb Sr Y Zr Nb Mo Tc Ru Rh Pd Ag Cd In Sn Sb Te I Xe Cs Ba La Ce Pr Nd Pm Sm Eu Gd Tb Dy Ho Er Tm Yb Lu Hf Ta \
    W Re Os Ir Pt Au Hg Tl Pb Bi Po At Rn Fr Ra Ac Th Pa U Np Pu Am Cm Bk Cf Es Fm Md No Lr Rf Db Sg Bh Hs Mt Ds Rg Cn \
    Nh Fl Mc Lv Ts Og";

const SECTIONS: [&str; 2] = ["lda", "pbe"];

pub(crate) fn portable(name: &str) -> bool {
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_');
    !name.is_empty() && !name.starts_with('.') && name.chars().all(plain)
}

#[derive(Debug, Default)]
pub(crate) struct Snapshot {
    tables: BTreeMap<String, (Vec<u8>, toml::Table)>,
    names: Vec<String>,
    faults: BTreeMap<String, String>,
}

pub(crate) fn regular(path: &Path, shown: &str) -> Result<Vec<u8>, String> {
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => fs::read(path).map_err(|e| format!("{shown}: {e}")),
        Ok(_) => Err(format!(
            "{shown} is no regular file (a link, a FIFO, a socket, a device, or a directory); nothing is read there"
        )),
        Err(e) => Err(format!("{shown}: {e}")),
    }
}

pub(crate) fn no_linked_ancestor(root: &Path, rel: &str) -> Result<(), String> {
    let parts: Vec<&str> = rel.split('/').filter(|part| !part.is_empty()).collect();
    let mut at = String::new();
    for part in parts.iter().take(parts.len().saturating_sub(1)) {
        at = if at.is_empty() {
            (*part).to_owned()
        } else {
            format!("{at}/{part}")
        };
        if fs::symlink_metadata(root.join(&at)).is_ok_and(|meta| meta.file_type().is_symlink()) {
            return Err(format!(
                "{at} is a link; nothing below it is read or written"
            ));
        }
    }
    Ok(())
}

pub(crate) fn regular_in(root: &Path, rel: &str) -> Result<Vec<u8>, String> {
    no_linked_ancestor(root, rel)?;
    regular(&root.join(rel), rel)
}

fn regular_at(root: &Path, path: &Path) -> Result<Vec<u8>, String> {
    match path.strip_prefix(root).ok().and_then(Path::to_str) {
        Some(rel) => regular_in(root, rel),
        None => regular(path, &path.display().to_string()),
    }
}

pub(crate) fn listing(path: &Path, shown: &str) -> Result<Vec<(String, fs::FileType)>, String> {
    let fail = |e: std::io::Error| format!("{shown} cannot be listed: {e}");
    match fs::symlink_metadata(path) {
        Ok(meta) if meta.is_dir() => {}
        Ok(_) => {
            return Err(format!(
                "{shown} is a link or no directory; it must be a real directory"
            ));
        }
        Err(e) => return Err(fail(e)),
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(path).map_err(fail)? {
        let entry = entry.map_err(fail)?;
        let kind = entry.file_type().map_err(fail)?;
        let name = (entry.file_name().into_string())
            .map_err(|name| format!("{shown}/{} is no UTF-8 name", name.to_string_lossy()))?;
        out.push((name, kind));
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    Ok(out)
}

pub(crate) fn snapshot(root: &Path) -> Snapshot {
    let mut snapshot = Snapshot::default();
    let dir = root.join(FAMILIES);
    let meta = fs::symlink_metadata(&dir);
    if meta
        .as_ref()
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return snapshot;
    }
    if meta.is_ok_and(|meta| !meta.is_dir()) {
        snapshot.faults.insert(
            String::new(),
            format!("{FAMILIES} is a link or no directory; the tables lie in a real directory"),
        );
        return snapshot;
    }
    let entries = match listing(&dir, FAMILIES) {
        Ok(entries) => entries,
        Err(e) => {
            snapshot.faults.insert(String::new(), e);
            return snapshot;
        }
    };
    for (file, kind) in entries {
        let bytes = file.as_bytes();
        let Some(name) = (bytes.len() >= 5
            && bytes[bytes.len() - 5..].eq_ignore_ascii_case(b".toml"))
        .then(|| file[..file.len() - 5].to_owned()) else {
            continue;
        };
        snapshot.names.push(name.clone());
        let table = format!("{FAMILIES}/{file}");
        if !file.ends_with(".toml") {
            snapshot
                .faults
                .insert(name, format!("{table}: a table is named <name>.toml"));
            continue;
        }
        if !kind.is_file() {
            snapshot.faults.insert(
                name,
                format!("{table} is a link or no regular file; a table is a regular file"),
            );
            continue;
        }
        let parsed = regular(&dir.join(&file), &table).and_then(|bytes| {
            let text = String::from_utf8(bytes.clone()).map_err(|e| format!("{table}: {e}"))?;
            let parsed =
                toml::from_str::<toml::Table>(&text).map_err(|e| format!("{table}: {e}"))?;
            Ok((bytes, parsed))
        });
        match parsed {
            Ok(table) => {
                snapshot.tables.insert(name, table);
            }
            Err(e) => {
                snapshot.faults.insert(name, e);
            }
        }
    }
    snapshot.names.sort();
    snapshot
}

fn section(key: &str) -> Result<(), String> {
    match SECTIONS.contains(&key) {
        true => Ok(()),
        false => Err(format!(
            "{key:?} keys no dataset section ({})",
            SECTIONS.join(", ")
        )),
    }
}

fn engine_row(element: &str, sha256: &str, hint_ha: Option<f64>) -> Result<(), String> {
    if !ELEMENTS.split_whitespace().any(|symbol| symbol == element) {
        return Err(format!("{element:?} is not an element symbol"));
    }
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "{element}: sha256 {sha256:?} is not 64 lowercase hex digits"
        ));
    }
    if hint_ha.is_some_and(|h| !(h > 0.0 && h.is_finite())) {
        return Err(format!("{element}: cutoff_hint_ha must be positive"));
    }
    Ok(())
}

impl Snapshot {
    pub(crate) fn external(&self, name: &str) -> Result<Family, String> {
        if let Some(fault) = self.faults.get("").or_else(|| self.faults.get(name)) {
            return Err(fault.clone());
        }
        if !(self.tables.get(name)).is_some_and(|(_, table)| !table.contains_key("base")) {
            return Err(format!("{name:?} is no external table of {FAMILIES}/"));
        }
        Family::from(name, &|file: &str| {
            let stem = file.strip_suffix(".toml").unwrap_or(file);
            (self.tables.get(stem))
                .map(|(bytes, _)| bytes.clone())
                .ok_or_else(|| format!("{FAMILIES}/{file}: not in the snapshot"))
        })
    }

    fn own(
        &self,
        root: &Path,
        name: &str,
        table: &toml::Table,
    ) -> Result<Vec<(String, String)>, String> {
        if !portable(name) {
            return Err(format!(
                "{name:?}: a family name holds letters, digits, `.`, `-`, `_`"
            ));
        }
        if let Some(other) =
            (self.names.iter()).find(|other| *other != name && other.eq_ignore_ascii_case(name))
        {
            return Err(format!(
                "{FAMILIES}/{other}.toml names the same directory in another case"
            ));
        }
        let ours: Ours = table
            .clone()
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string())?;
        if !portable(&ours.base) {
            return Err(format!("base {:?} is no plain table name", ours.base));
        }
        if let Err(e) = self.external(&ours.base) {
            return Err(format!(
                "base {:?} is no external table of {FAMILIES}/: {e}",
                ours.base
            ));
        }
        section(&ours.functional).map_err(|e| format!("functional {e}"))?;
        let mut rows = Vec::new();
        for (element, own) in &ours.element {
            engine_row(element, &own.sha256, Some(own.cutoff_hint_ha))?;
            let canonical = format!("{name}/{element}.xml");
            if own.file != canonical {
                return Err(format!("{element}: {:?} is not {canonical:?}", own.file));
            }
            rows.push((element.clone(), own.sha256.clone()));
        }
        let dir = format!("{FAMILIES}/{name}");
        if !fs::symlink_metadata(root.join(&dir)).is_ok_and(|m| m.is_dir()) {
            return Err(format!("{dir} is no directory"));
        }
        for (element, _) in &rows {
            let file = format!("{dir}/{element}.xml");
            if !fs::symlink_metadata(root.join(&file)).is_ok_and(|m| m.is_file()) {
                return Err(format!("{element}: {file} is no regular file"));
            }
        }
        Ok(rows)
    }

    pub(crate) fn check(&self, root: &Path, name: &str, text: &str) -> Result<(), String> {
        let at = |e: String| format!("{FAMILIES}/{name}.toml: {e}");
        let table: toml::Table = toml::from_str(text).map_err(|e| at(e.to_string()))?;
        self.own(root, name, &table).map(|_| ()).map_err(at)
    }
}

#[derive(Debug, Default)]
pub(crate) struct Owned {
    pub(crate) pins: BTreeMap<String, (String, String)>,
    pub(crate) faults: Vec<String>,
    pub(crate) names: Vec<String>,
}

pub(crate) fn own_pins(root: &Path) -> Owned {
    let snapshot = snapshot(root);
    let mut owned = Owned {
        faults: snapshot.faults.values().cloned().collect(),
        names: snapshot.names.clone(),
        ..Owned::default()
    };
    for (name, (_, table)) in &snapshot.tables {
        if !table.contains_key("base") {
            continue;
        }
        match snapshot.own(root, name, table) {
            Ok(rows) => {
                for (element, sha256) in rows {
                    let file = format!("{FAMILIES}/{name}/{element}.xml");
                    owned.pins.insert(file, (name.clone(), sha256));
                }
            }
            Err(e) => owned.faults.push(format!("{FAMILIES}/{name}.toml: {e}")),
        }
    }
    owned
}

pub(crate) fn new_family(root: &Path, name: &str) -> Result<(), String> {
    let snapshot = snapshot(root);
    if let Some(fault) = snapshot.faults.values().next() {
        return Err(format!(
            "{fault}; a new family needs a {FAMILIES}/ without a fault"
        ));
    }
    if !portable(name) {
        return Err(format!(
            "{name:?}: a family name holds letters, digits, `.`, `-`, `_`"
        ));
    }
    let base = snapshot.external(DEFAULT_FAMILY)?;
    if let Some(top) = (directories(&base)?.into_iter()).find(|top| top.eq_ignore_ascii_case(name))
    {
        return Err(format!(
            "{name} names the directory {top} of {DEFAULT_FAMILY}; give the family another name"
        ));
    }
    let exists =
        format!("{name} exists; a family never changes, so give the new version a new name");
    if snapshot
        .names
        .iter()
        .any(|listed| listed.eq_ignore_ascii_case(name))
    {
        return Err(exists);
    }
    let lookup = fs::symlink_metadata(root.join(FAMILIES).join(format!("{name}.toml")));
    match lookup.as_ref().err().map(std::io::Error::kind) == Some(std::io::ErrorKind::NotFound) {
        true => Ok(()),
        false => Err(exists),
    }
}

fn pins(root: &Path) -> Result<Pins, String> {
    let text = String::from_utf8(regular_in(root, PATH)?).map_err(|e| format!("{PATH}: {e}"))?;
    toml::from_str(&text).map_err(|e| format!("{PATH}: {e}"))
}

pub(crate) fn hints(
    root: &Path,
    name: &str,
    functional: &str,
) -> Result<BTreeMap<String, f64>, String> {
    let hints = Family::read(root, name)?.hints(functional);
    if hints.is_empty() {
        return Err(format!("{FAMILIES}/{name}.toml holds no {functional} hint"));
    }
    Ok(hints)
}

pub(crate) fn for_species(
    root: &Path,
    name: &str,
    species: &[String],
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if species.is_empty() {
        return Ok(Vec::new());
    }
    let family = Family::read(root, name)?;
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for symbol in species.iter().collect::<std::collections::BTreeSet<_>>() {
        let rows: Vec<&FamilyRow> = (family.rows.iter())
            .filter(|row| row.element == *symbol)
            .collect();
        if rows.is_empty() {
            return Err(format!("{} holds no file for {symbol}", family.table));
        }
        for row in rows {
            files.push((row.path.clone(), family.bytes(root, row)?));
        }
    }
    files.extend(family.tables);
    Ok(files)
}

pub(crate) fn files(
    root: &Path,
    table: Option<&Table>,
    input: &super::Input,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    if input
        .0
        .get("datasets")
        .and_then(|d| d.get("directory"))
        .is_some()
    {
        return Err(
            "datasets.directory: the harness stages each dataset at datasets/; a case input leaves this key out"
                .into(),
        );
    }
    let species = input.species()?;
    match table {
        Some(table) => table.files(root, &species),
        None => for_species(root, &input.family(), &species),
    }
}

pub fn agree(stdout: &str, input: &str, given: &[(String, Vec<u8>)]) -> Result<(), String> {
    let input: toml::Table = toml::from_str(input).map_err(|e| e.to_string())?;
    let input = super::Input(&input);
    let species = input.species()?;
    let outcome: serde_json::Value = serde_json::from_str(stdout).unwrap_or_default();
    let none = serde_json::Map::new();
    let named = match outcome.get("datasets") {
        None => &none,
        Some(map) => map
            .as_object()
            .ok_or("datasets: the outcome's datasets field is no map")?,
    };
    let placed = |file: &str| {
        (given.iter())
            .find(|(p, _)| p == file)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| format!("{file}: the harness placed no such table"))
    };
    let distinct: std::collections::BTreeSet<&String> = species.iter().collect();
    if let Some(other) = named.keys().find(|key| !distinct.contains(key)) {
        return Err(format!(
            "datasets: the outcome names {other}, which is no species of the input"
        ));
    }
    let word = input.functional().unwrap_or(DEFAULT_FUNCTIONAL);
    let mut family: Option<Family> = None;
    for element in distinct {
        let (expected, pin) = match input.dataset_file(element) {
            Some(path) => (path, None),
            None => {
                if family.is_none() {
                    family = Some(Family::from(&input.family(), &placed)?);
                }
                let family = family.as_ref().expect("read above");
                let row = family.row(word, element).ok_or_else(|| {
                    format!(
                        "datasets: {} has no {word} section for {element}",
                        family.table
                    )
                })?;
                (row.path.clone(), Some(row.sha256.clone()))
            }
        };
        let file = &named.get(element.as_str()).cloned().unwrap_or_default();
        let (path, sha) = (file["path"].as_str(), file["sha256"].as_str());
        let bytes = (given.iter()).find(|(p, _)| Some(p.as_str()) == path);
        let agrees = path == Some(expected.as_str())
            && pin.as_deref().is_none_or(|pin| Some(pin) == sha)
            && bytes.is_some_and(|(_, bytes)| Some(digest(bytes).as_str()) == sha);
        if !agrees {
            return Err(format!(
                "datasets: the outcome names {element} at {file}, not one of {:?} with the sha256 of the \
                 bytes that the harness placed",
                [expected]
            ));
        }
    }
    Ok(())
}

pub(crate) fn element(path: &str) -> Option<&str> {
    let parts: Vec<_> = Path::new(path).components().collect();
    match parts.as_slice() {
        [
            Component::Normal(_),
            Component::Normal(element),
            Component::Normal(_),
        ] => element.to_str(),
        _ => None,
    }
}

fn plain(path: &str) -> bool {
    (path.split('/')).all(|name| !matches!(name, "" | "." | ".."))
}

pub(crate) fn directories(family: &Family) -> Result<Vec<String>, String> {
    let placed = |file: &str| {
        (family.tables.iter())
            .find(|(f, _)| f == file)
            .map(|(_, bytes)| bytes.clone())
            .ok_or_else(|| format!("{FAMILIES}/{file}: not placed"))
    };
    let mut tops = Vec::new();
    for (file, _) in &family.tables {
        let name = file.strip_suffix(".toml").unwrap_or(file);
        for row in Family::from(name, &placed)?.rows {
            tops.push(row.path.split('/').next().unwrap_or_default().to_owned());
        }
    }
    Ok(tops)
}

fn fetched(
    root: &Path,
    table: &str,
    url: Option<&str>,
    path: &str,
    sha: &str,
) -> Result<Vec<u8>, String> {
    let local = root.join("target/datasets").join(path);
    let cached = match fs::symlink_metadata(&local) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        _ => Some(regular(&local, &local.display().to_string())?),
    };
    if let Some(bytes) = cached
        && digest(&bytes) == sha
    {
        return Ok(bytes);
    }
    let Some(source) = url else {
        let own = format!("{FAMILIES}/{path}");
        return match regular_in(root, &own) {
            Ok(bytes) if digest(&bytes) == sha => Ok(bytes),
            Ok(bytes) => Err(format!(
                "{path}: {own} has sha256 {}, but {table} pins {sha}",
                digest(&bytes)
            )),
            Err(e)
                if !fs::symlink_metadata(root.join(&own))
                    .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
            {
                Err(format!("{path}: {e}"))
            }
            Err(_) => Err(format!(
                "{path}: {table} names no source; place the file with sha256 {sha} at {own} or {}",
                local.display()
            )),
        };
    };
    let dir = local
        .parent()
        .ok_or_else(|| format!("{path}: no directory"))?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    static FETCHES: AtomicUsize = AtomicUsize::new(0);
    let part = local.with_extension(format!(
        "{}-{}.part",
        std::process::id(),
        FETCHES.fetch_add(1, Ordering::Relaxed)
    ));
    let status = Command::new("curl")
        .args([
            "--fail",
            "--silent",
            "--show-error",
            "--location",
            "--retry",
            "2",
            "--connect-timeout",
            "30",
            "--max-time",
            "600",
            "--output",
        ])
        .arg(&part)
        .arg(source)
        .status()
        .map_err(|e| format!("curl {source}: {e}"))?;
    let bytes = match regular(&part, &part.display().to_string()) {
        Ok(bytes) => bytes,
        Err(e) => {
            let _ = fs::remove_file(&part);
            return Err(format!("{source}: curl {status}: {e}"));
        }
    };
    if !status.success() || digest(&bytes) != sha {
        let _ = fs::remove_file(&part);
        return Err(format!(
            "{source}: curl {status}, sha256 {} but {table} pins {sha}",
            digest(&bytes)
        ));
    }
    fs::rename(&part, &local).map_err(|e| format!("{}: {e}", local.display()))?;
    Ok(bytes)
}

fn digest(bytes: &[u8]) -> String {
    crate::lock::hex(&Sha256::digest(bytes))
}

pub(crate) const TABLE: &str = "table.toml";

#[derive(Debug)]
pub(crate) struct Table {
    path: PathBuf,
    sha256: String,
    entries: BTreeMap<String, Row>,
    tables: Vec<(String, Vec<u8>)>,
}

#[derive(Debug)]
struct Row {
    at: String,
    source: Source,
    hint_ha: f64,
}

#[derive(Debug)]
enum Source {
    Family {
        table: String,
        url: Option<String>,
        sha256: String,
    },
    Own(PathBuf, String),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TableFile {
    element: BTreeMap<String, Entry>,
}

#[derive(Deserialize)]
#[serde(untagged)]
enum Entry {
    Family(String),
    File(OwnFile),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnFile {
    file: String,
    sha256: String,
    cutoff_hint_ha: f64,
}

impl Table {
    pub(crate) fn of_case(
        root: &Path,
        dir: &Path,
        candidate: Option<&Path>,
    ) -> Result<Option<Table>, String> {
        let tracked = dir.join(TABLE);
        let present = !fs::symlink_metadata(&tracked)
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound);
        match (present, candidate) {
            (false, None) => Ok(None),
            (false, Some(path)) => Err(format!(
                "{}: the case has no {TABLE}, so it takes no candidate table",
                path.display()
            )),
            (true, path) => Table::read(root, path.unwrap_or(&tracked)).map(Some),
        }
    }

    pub(crate) fn read(root: &Path, path: &Path) -> Result<Table, String> {
        let at = path.display();
        let bytes = regular_at(root, path)?;
        let text = String::from_utf8(bytes).map_err(|e| format!("{at}: {e}"))?;
        let file: TableFile = toml::from_str(&text).map_err(|e| format!("{at}: {e}"))?;
        let family = Family::read(root, DEFAULT_FAMILY).map_err(|e| format!("{at}: {e}"))?;
        let tops = directories(&family)?;
        let mut entries = BTreeMap::new();
        for (element, entry) in file.element {
            let row = match entry {
                Entry::Family(name) if name != DEFAULT_FAMILY => {
                    return Err(format!(
                        "{at}: {element}: {name:?} is not {DEFAULT_FAMILY:?}, the family of the ACWF inputs; give a file"
                    ));
                }
                Entry::Family(_) => {
                    let row = (family.row(ACWF_FUNCTIONAL, &element)).ok_or_else(|| {
                        format!(
                            "{at}: {element}: {} holds no {ACWF_FUNCTIONAL} file of it",
                            family.table
                        )
                    })?;
                    let highest = family.hints(ACWF_FUNCTIONAL).into_values().reduce(f64::max);
                    let hint = (row.hint_ha.or(highest)).ok_or_else(|| {
                        format!(
                            "{at}: {element}: {} holds no {ACWF_FUNCTIONAL} hint",
                            family.table
                        )
                    })?;
                    Row {
                        at: row.path.clone(),
                        source: Source::Family {
                            table: family.table.clone(),
                            url: row.url.clone(),
                            sha256: row.sha256.clone(),
                        },
                        hint_ha: hint,
                    }
                }
                Entry::File(OwnFile { cutoff_hint_ha, .. })
                    if cutoff_hint_ha.is_nan() || cutoff_hint_ha <= 0.0 =>
                {
                    return Err(format!(
                        "{at}: {element}: cutoff_hint_ha {cutoff_hint_ha} is not above 0"
                    ));
                }
                Entry::File(OwnFile { file, .. })
                    if !plain(&file)
                        || tops
                            .iter()
                            .any(|top| file.split('/').next() == Some(top.as_str())) =>
                {
                    return Err(format!(
                        "{at}: {element}: file {file:?} must be a relative path of plain names outside the \
                         directory of a family"
                    ));
                }
                Entry::File(OwnFile {
                    file,
                    sha256,
                    cutoff_hint_ha,
                }) => {
                    let beside = path.parent().unwrap_or(Path::new("")).join(&file);
                    Row {
                        at: file,
                        source: Source::Own(beside, sha256),
                        hint_ha: cutoff_hint_ha,
                    }
                }
            };
            entries.insert(element, row);
        }
        Ok(Table {
            path: path.to_owned(),
            sha256: digest(text.as_bytes()),
            entries,
            tables: family.tables,
        })
    }

    pub(crate) fn with_files(&self, input: &str, species: &[String]) -> Result<String, String> {
        let files: toml::Table = (self.entries.iter())
            .filter(|(element, row)| {
                matches!(row.source, Source::Own(..)) && species.contains(element)
            })
            .map(|(element, row)| (element.clone(), toml::Value::from(row.at.as_str())))
            .collect();
        if files.is_empty() {
            return Ok(input.to_owned());
        }
        let mut table: toml::Table = toml::from_str(input).map_err(|e| e.to_string())?;
        let datasets = (table.entry("datasets"))
            .or_insert_with(|| toml::Table::new().into())
            .as_table_mut()
            .ok_or("datasets is not a table")?;
        if datasets.contains_key("files") {
            return Err(format!(
                "datasets.files: {} gives each file, so the input sets none",
                self.path.display()
            ));
        }
        datasets.insert("files".to_owned(), files.into());
        toml::to_string(&table).map_err(|e| e.to_string())
    }

    pub(crate) fn hints(&self) -> BTreeMap<String, f64> {
        (self.entries.iter())
            .map(|(element, row)| (element.clone(), row.hint_ha))
            .collect()
    }

    pub(crate) fn files(
        &self,
        root: &Path,
        species: &[String],
    ) -> Result<Vec<(String, Vec<u8>)>, String> {
        let mut files: Vec<(String, Vec<u8>)> = Vec::new();
        for element in species.iter().collect::<std::collections::BTreeSet<_>>() {
            let Row { at, source, .. } = (self.entries.get(element))
                .ok_or_else(|| format!("{}: the table has no {element}", self.path.display()))?;
            let bytes = match source {
                Source::Family { table, url, sha256 } => {
                    fetched(root, table, url.as_deref(), at, sha256)?
                }
                Source::Own(path, sha) => {
                    let bytes = regular_at(root, path)?;
                    if digest(&bytes) != *sha {
                        return Err(format!(
                            "{}: sha256 {}, but {} gives {sha}",
                            path.display(),
                            digest(&bytes),
                            self.path.display()
                        ));
                    }
                    bytes
                }
            };
            files.push((at.clone(), bytes));
        }
        if !files.is_empty() {
            files.extend(self.tables.iter().cloned());
        }
        Ok(files)
    }

    pub(crate) fn named(&self) -> serde_json::Value {
        serde_json::json!({ "path": self.path.display().to_string(), "sha256": self.sha256 })
    }
}
