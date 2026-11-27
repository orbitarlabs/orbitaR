use crate::{Findings, case, harness::datasets::regular_in, walk};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    io::Write as _,
    path::Path,
    process::{Command, Stdio},
};

pub(crate) const DIR: &str = "benchmarks/locks";
pub(crate) const LEGACY: &str = "benchmarks/LOCK";
pub(crate) const STAGED: &str = "benchmarks/locks.partial";
pub(crate) const HARNESS: &str = "harness";
const SUFFIX: &str = ".lock";
const REMOVED: &str = "removed";
const HARNESS_FILES: [&str; 14] = [
    "xtask/",
    "tests/benchmarks.rs",
    ".cargo/config.toml",
    ".cargo/mutants.toml",
    ".config/nextest.toml",
    "clippy.toml",
    ".github/workflows/",
    "Cargo.toml",
    "Cargo.lock",
    "benchmarks/NODE.toml",
    "benchmarks/DATASETS.toml",
    "benchmarks/RIVALS.toml",
    "benchmarks/TUNED.toml",
    "benchmarks/ACWF.toml",
];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    sha256: String,
    reviewer: String,
    reason: String,
}

pub(crate) type Lock = BTreeMap<String, BTreeMap<String, Entry>>;

impl Entry {
    fn new(sha256: &str, reviewer: &str, reason: &str) -> Self {
        Self {
            sha256: sha256.to_owned(),
            reviewer: reviewer.trim().to_owned(),
            reason: reason.replace('\n', " ").trim().to_owned(),
        }
    }

    fn removed(&self) -> bool {
        self.sha256 == REMOVED
    }

    fn review(&self) -> (String, String) {
        let plain = |text: &str| text.split_whitespace().collect::<Vec<_>>().join(" ");
        (plain(&self.reviewer), plain(&self.reason))
    }

    fn parse(stored: &str, text: &str) -> Result<Self, String> {
        let entry: Self = toml::from_str(text).map_err(|e| format!("{stored}: {e}"))?;
        let hex = entry.sha256.len() == 64
            && (entry.sha256.bytes()).all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !(hex || entry.removed()) {
            return Err(format!(
                "{stored}: sha256 holds 64 hex digits or {REMOVED:?}"
            ));
        }
        if entry.reviewer.trim().is_empty() || entry.reason.trim().is_empty() {
            return Err(format!("{stored} needs a reviewer and a reason"));
        }
        Ok(entry)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Reviewed,
    Stale(Vec<String>),
    Unlocked,
    Broken(String),
}

fn stored_at(block: &str, file: &str) -> String {
    format!("{DIR}/{block}/{file}{SUFFIX}")
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Move {
    Done,
    Start,
    Finish,
}

fn place(stored: &str) -> Result<(String, String), String> {
    (stored.strip_prefix(DIR))
        .and_then(|rest| rest.strip_prefix('/'))
        .and_then(|rest| rest.split_once('/'))
        .and_then(|(block, file)| Some((block, file.strip_suffix(SUFFIX)?)))
        .filter(|(block, file)| !block.is_empty() && !file.is_empty())
        .filter(|_| !stored.chars().any(char::is_control))
        .map(|(block, file)| (block.to_owned(), file.to_owned()))
        .ok_or_else(|| format!("{stored} is no {DIR}/<block>/<file>{SUFFIX} entry"))
}

fn parse(stored: impl IntoIterator<Item = (String, Vec<u8>)>) -> Result<Lock, String> {
    let mut lock = Lock::new();
    for (path, bytes) in stored {
        let (block, file) = place(&path)?;
        let text = String::from_utf8(bytes).map_err(|e| format!("{path}: {e}"))?;
        let entry = Entry::parse(&path, &text)?;
        lock.entry(block).or_default().insert(file, entry);
    }
    Ok(lock)
}

pub(crate) fn legacy(text: &str) -> Result<Lock, String> {
    let mut lock = Lock::new();
    let mut current: Option<(String, Entry)> = None;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("# ") {
            let (block, review) = (header.split_once(" reviewer="))
                .and_then(|(block, rest)| Some((block, rest.split_once(" reason=")?)))
                .ok_or_else(|| format!("{LEGACY}: {header:?} needs reviewer= and reason="))?;
            if lock.insert(block.to_owned(), BTreeMap::new()).is_some() {
                return Err(format!("{LEGACY} lists {block} twice"));
            }
            let review = Entry {
                sha256: String::new(),
                reviewer: review.0.to_owned(),
                reason: review.1.to_owned(),
            };
            current = Some((block.to_owned(), review));
        } else if !line.trim().is_empty() {
            let Some((block, review)) = &current else {
                return Err(format!("{LEGACY}: hash line before any header: {line}"));
            };
            let (sha, file) = (line.split_once("  "))
                .ok_or_else(|| format!("{LEGACY}: {line:?} is no `<sha256>  <file>` line"))?;
            let entry = Entry {
                sha256: sha.to_owned(),
                ..review.clone()
            };
            (lock.entry(block.clone()).or_default()).insert(file.to_owned(), entry);
        }
    }
    Ok(lock)
}

fn present(root: &Path, path: &str) -> bool {
    !std::fs::symlink_metadata(root.join(path))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
}

fn on_disk(root: &Path) -> Result<Lock, String> {
    if let Some(fault) = walk::faults(root, DIR).into_iter().next() {
        return Err(fault);
    }
    if let Some(odd) = (walk::links(root, DIR).into_iter())
        .chain(walk::others(root, DIR))
        .next()
    {
        return Err(format!("{odd} is no regular file"));
    }
    let files = (walk::files(root, DIR).into_iter())
        .map(|path| Ok((path.clone(), regular_in(root, &path)?)))
        .collect::<Result<Vec<_>, String>>()?;
    parse(files)
}

pub(crate) fn read(root: &Path) -> Result<Lock, String> {
    if present(root, LEGACY) {
        return Err(format!(
            "{LEGACY} is the old one-file lock; `cargo xtask lock --reviewer <name> --reason <text>` moves it into {DIR}"
        ));
    }
    on_disk(root)
}

fn stored(root: &Path) -> Result<(Lock, Move), String> {
    if !present(root, LEGACY) {
        return on_disk(root).map(|lock| (lock, Move::Done));
    }
    let text =
        String::from_utf8(regular_in(root, LEGACY)?).map_err(|e| format!("{LEGACY}: {e}"))?;
    let old = legacy(&text)?;
    let now = on_disk(root)?;
    if now.is_empty() {
        return Ok((old, Move::Start));
    }
    let moved = old.iter().all(|(block, files)| {
        files.iter().all(|(file, was)| {
            (now.get(block).and_then(|n| n.get(file)))
                .is_some_and(|is| is.sha256 != was.sha256 || is == was)
        })
    });
    match moved {
        true => Ok((now, Move::Finish)),
        false => Err(format!(
            "{LEGACY} and {DIR} both exist and {DIR} lacks a review that {LEGACY} holds; keep the one to trust and delete the other"
        )),
    }
}
pub(crate) fn at(root: &Path, base: &str, listed: &[String]) -> Result<Option<Lock>, String> {
    let prefix = format!("{DIR}/");
    let stored: Vec<String> = (listed.iter())
        .filter(|path| path.starts_with(&prefix))
        .cloned()
        .collect();
    if !stored.is_empty() {
        let blobs = blobs(root, base, &stored)?;
        return parse(stored.into_iter().zip(blobs)).map(Some);
    }
    if !listed.iter().any(|path| path == LEGACY) {
        return Ok(None);
    }
    let bytes = blobs(root, base, &[LEGACY.to_owned()])?.concat();
    let text = String::from_utf8(bytes).map_err(|e| format!("{LEGACY} at {base}: {e}"))?;
    legacy(&text).map(Some)
}

fn blobs(root: &Path, base: &str, paths: &[String]) -> Result<Vec<Vec<u8>>, String> {
    let fail = |e: &dyn std::fmt::Display| format!("git cat-file at {base}: {e}");
    let mut child = Command::new("git")
        .args(["cat-file", "-z", "--batch=%(objecttype) %(objectsize)"])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| fail(&e))?;
    let asked: String = paths.iter().map(|p| format!("{base}:{p}\0")).collect();
    let mut stdin = child.stdin.take().ok_or_else(|| fail(&"no stdin"))?;
    let writer = std::thread::spawn(move || stdin.write_all(asked.as_bytes()));
    let out = child.wait_with_output().map_err(|e| fail(&e))?;
    (writer.join())
        .map_err(|_| fail(&"the writer panicked"))?
        .map_err(|e| fail(&e))?;
    let mut rest = out.stdout.as_slice();
    let mut blobs = Vec::new();
    for path in paths {
        let end = (rest.iter().position(|b| *b == b'\n')).ok_or_else(|| fail(&path))?;
        let header = String::from_utf8_lossy(&rest[..end]).into_owned();
        let size: usize = match header.split(' ').collect::<Vec<_>>().as_slice() {
            ["blob", size] => size.parse().map_err(|_| fail(&header))?,
            _ => return Err(fail(&format!("{path}: {header}"))),
        };
        let body = (rest.get(end + 1..end + 1 + size)).ok_or_else(|| fail(&path))?;
        blobs.push(body.to_vec());
        rest = rest.get(end + 2 + size..).unwrap_or_default();
    }
    Ok(blobs)
}

fn approved(old: &Lock) -> BTreeSet<(String, String)> {
    old.values()
        .flat_map(BTreeMap::values)
        .map(Entry::review)
        .collect()
}

pub(crate) fn unreviewed(old: &Lock, new: &Lock) -> BTreeMap<String, Vec<String>> {
    let approved = approved(old);
    let was = |block: &str, file: &str| old.get(block).and_then(|files| files.get(file));
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (block, files) in new {
        for (file, is) in files {
            let changed = was(block, file).is_none_or(|was| was.sha256 != is.sha256);
            if changed && approved.contains(&is.review()) {
                out.entry(block.clone()).or_default().push(file.clone());
            }
        }
    }
    for (block, files) in old {
        for (file, was) in files {
            let dropped = new.get(block).is_none_or(|files| !files.contains_key(file));
            if dropped && !was.removed() {
                out.entry(block.clone()).or_default().push(file.clone());
            }
        }
    }
    out
}

pub(crate) fn kept_review(old: &Lock, new: &Lock, block: &str, file: &str) -> bool {
    let is = new.get(block).and_then(|files| files.get(file));
    is.is_none_or(|is| approved(old).contains(&is.review()))
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn hashes(root: &Path, id: &str) -> Result<Vec<String>, String> {
    let walked: Vec<String> = if id == HARNESS {
        (HARNESS_FILES.iter())
            .filter_map(|f| f.strip_suffix('/').map(str::to_owned))
            .collect()
    } else {
        vec![format!("benchmarks/{id}"), format!("tests/benchmarks/{id}")]
    };
    if let Some(fault) = walked.iter().flat_map(|dir| walk::faults(root, dir)).next() {
        return Err(fault);
    }
    let files: Vec<String> = if id == HARNESS {
        let mut files = Vec::new();
        for f in HARNESS_FILES {
            match f.strip_suffix('/') {
                Some(dir) => files.extend(walk::files(root, dir)),
                None => match std::fs::symlink_metadata(root.join(f)) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    _ => files.push(f.to_owned()),
                },
            }
        }
        files.extend(crate::harness::datasets::judged(root)?);
        files
    } else {
        let case = walk::files(root, &format!("benchmarks/{id}"));
        let named = crate::harness::datasets::case_tables(root, &case)?;
        case.into_iter()
            .chain(walk::files(root, &format!("tests/benchmarks/{id}")))
            .chain(named)
            .collect()
    };
    files
        .into_iter()
        .map(|file| {
            if file.chars().any(char::is_control) {
                return Err(format!(
                    "{file:?} holds a control character, which no lock entry may name"
                ));
            }
            let mut bytes = regular_in(root, &file)?;
            if file == crate::layout::RUNNER {
                bytes = without_case_table(&bytes);
            }
            Ok(format!("{}  {file}", hex(&Sha256::digest(&bytes))))
        })
        .collect()
}

fn lines(root: &Path, id: &str) -> String {
    match hashes(root, id) {
        Ok(lines) => lines.join("\n"),
        Err(e) => format!("! {e}"),
    }
}

fn without_case_table(bytes: &[u8]) -> Vec<u8> {
    let text = String::from_utf8_lossy(bytes);
    let Some(start) = text.find("// cases: begin") else {
        return bytes.to_vec();
    };
    let Some(length) = text[start..].find("// cases: end") else {
        return bytes.to_vec();
    };
    format!("{}{}", &text[..start], &text[start + length..]).into_bytes()
}

pub(crate) fn digest(root: &Path, id: &str) -> String {
    hex(&Sha256::digest(lines(root, id)))
}

pub(crate) fn fingerprint(root: &Path) -> String {
    let mut text = String::new();
    let stored = std::iter::once(LEGACY.to_owned())
        .chain(walk::files(root, DIR))
        .chain(walk::links(root, DIR))
        .chain(walk::others(root, DIR));
    for path in stored {
        let bytes = regular_in(root, &path).unwrap_or_else(|e| format!("! {e}").into_bytes());
        text.push_str(&path);
        text.push_str(&String::from_utf8_lossy(&bytes));
    }
    let ids = case::ids(root).unwrap_or_default();
    for id in std::iter::once(HARNESS).chain(ids.iter().map(String::as_str)) {
        text.push_str(&lines(root, id));
    }
    hex(&Sha256::digest(text))
}

fn hashed(root: &Path, id: &str) -> Result<BTreeMap<String, String>, String> {
    Ok((hashes(root, id)?.into_iter())
        .filter_map(|line| {
            let (sha, file) = line.split_once("  ")?;
            Some((file.to_owned(), sha.to_owned()))
        })
        .collect())
}

pub(crate) fn state(root: &Path, id: &str) -> Result<State, String> {
    Ok(state_in(&read(root)?, root, id))
}

fn state_in(lock: &Lock, root: &Path, id: &str) -> State {
    let hashed = match hashed(root, id) {
        Ok(hashed) => hashed,
        Err(e) => return State::Broken(e),
    };
    let none = BTreeMap::new();
    let entries = match lock.get(id) {
        Some(entries) => entries,
        None if hashed.is_empty() => &none,
        None => return State::Unlocked,
    };
    let changed = (hashed.iter())
        .filter(|(file, sha)| entries.get(*file).is_none_or(|e| e.sha256 != **sha))
        .map(|(file, _)| file)
        .chain(
            (entries.iter())
                .filter(|(file, e)| !e.removed() && !hashed.contains_key(*file))
                .map(|(file, _)| file),
        );
    let stale: Vec<String> = changed.cloned().collect();
    match stale.is_empty() {
        true => State::Reviewed,
        false => State::Stale(stale),
    }
}

pub(crate) fn findings(root: &Path) -> Findings {
    let locked = match read(root) {
        Ok(locked) => locked,
        Err(e) => return vec![format!("science: S5 {e}")],
    };
    let ids = match case::ids(root) {
        Ok(ids) => ids,
        Err(e) => return vec![format!("science: S5 {e}")],
    };
    let mut findings = Vec::new();
    for id in locked.keys() {
        if id != HARNESS && !ids.contains(id) {
            findings.push(format!("science: S5 locked case {id} disappeared; set status = \"retired\" instead of deleting or renaming"));
        }
    }
    for id in std::iter::once(HARNESS).chain(ids.iter().map(String::as_str)) {
        match state_in(&locked, root, id) {
            State::Unlocked => findings.push(format!(
                "science: S5 {id} is not in {DIR}; run `cargo xtask lock --reviewer <name> --reason <text>`"
            )),
            State::Stale(files) => findings.push(format!(
                "science: S5 {id} changed since its lock review: {}; run `cargo xtask lock` and get a science review",
                files.join(", ")
            )),
            State::Reviewed => {}
            State::Broken(e) => findings.push(format!("science: S5 {id}: {e}")),
        }
    }
    findings
}

pub(crate) fn blocks(root: &Path) -> Vec<String> {
    read(root)
        .map(Lock::into_keys)
        .into_iter()
        .flatten()
        .collect()
}

pub(crate) fn write(root: &Path, reviewer: &str, reason: &str) -> Findings {
    if reviewer.trim().is_empty() || reason.trim().is_empty() {
        return vec!["usage: cargo xtask lock --reviewer <name> --reason <text>".into()];
    }
    let (locked, moved) = match stored(root) {
        Ok(stored) => stored,
        Err(e) => return vec![format!("lock: {e}")],
    };
    let ids = match case::ids(root) {
        Ok(ids) => ids,
        Err(e) => return vec![format!("lock: {e}")],
    };
    let max_locked = locked
        .keys()
        .filter_map(|id| case::parse_id(id).ok())
        .max()
        .unwrap_or(0);
    let mut findings = Vec::new();
    for id in locked
        .keys()
        .filter(|id| *id != HARNESS && !ids.contains(id))
    {
        findings.push(format!(
            "lock: {id} disappeared; restore it with status = \"retired\""
        ));
    }
    for id in ids.iter().filter(|id| !locked.contains_key(*id)) {
        if case::parse_id(id).unwrap_or(0) <= max_locked {
            findings.push(format!(
                "lock: new case {id} must use a number above {max_locked}"
            ));
        }
    }
    if !findings.is_empty() {
        return findings;
    }
    let mut next = Lock::new();
    for id in std::iter::once(HARNESS).chain(ids.iter().map(String::as_str)) {
        let hashed = match hashed(root, id) {
            Ok(hashed) => hashed,
            Err(e) => return vec![format!("lock: {id}: {e}")],
        };
        let kept = locked.get(id).cloned().unwrap_or_default();
        let block = next.entry(id.to_owned()).or_default();
        for (file, sha) in &hashed {
            let entry = match kept.get(file) {
                Some(entry) if entry.sha256 == *sha => entry.clone(),
                _ => Entry::new(sha, reviewer, reason),
            };
            block.insert(file.clone(), entry);
        }
        for (file, entry) in kept.into_iter().filter(|(f, _)| !hashed.contains_key(f)) {
            let entry = match entry.removed() {
                true => entry,
                false => Entry::new(REMOVED, reviewer, reason),
            };
            block.insert(file, entry);
        }
    }
    let (written, into) = match moved {
        Move::Start => (Lock::new(), STAGED),
        Move::Done | Move::Finish => (locked, DIR),
    };
    if moved == Move::Start
        && let Err(e) = clear(root, STAGED)
    {
        return vec![format!("lock: {STAGED}: {e}")];
    }
    for (block, files) in &next {
        for (file, entry) in files {
            if written.get(block).and_then(|w| w.get(file)) == Some(entry) {
                continue;
            }
            let at = stored_at(block, file).replacen(DIR, into, 1);
            let path = root.join(&at);
            let wrote = (path.parent().map_or(Ok(()), std::fs::create_dir_all))
                .and_then(|()| toml::to_string(entry).map_err(std::io::Error::other))
                .and_then(|text| std::fs::write(&path, text));
            if let Err(e) = wrote {
                let _ = clear(root, STAGED);
                return vec![format!("lock: {at}: {e}")];
            }
        }
    }
    let published = match moved {
        Move::Start => std::fs::rename(root.join(STAGED), root.join(DIR))
            .map_err(|e| format!("lock: {STAGED} → {DIR}: {e}")),
        Move::Done | Move::Finish => Ok(()),
    };
    let finished = published.and_then(|()| match moved {
        Move::Done => Ok(()),
        Move::Start | Move::Finish => std::fs::remove_file(root.join(LEGACY)).map_err(|e| {
            format!("lock: {LEGACY}: {e}; run `cargo xtask lock` again to finish the move")
        }),
    });
    finished.err().into_iter().collect()
}

fn clear(root: &Path, dir: &str) -> std::io::Result<()> {
    match std::fs::remove_dir_all(root.join(dir)) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(e),
        _ => Ok(()),
    }
}
