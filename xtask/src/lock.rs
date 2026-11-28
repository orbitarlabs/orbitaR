use crate::{Findings, case, walk};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fmt::Write as _, path::Path};

pub(crate) const PATH: &str = "benchmarks/LOCK";
pub(crate) const HARNESS: &str = "harness";
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

pub(crate) struct Block {
    pub(crate) header: String,
    pub(crate) lines: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum State {
    Reviewed,
    Stale,
    Unlocked,
    Broken(String),
}

pub(crate) fn blocks(text: &str) -> Result<BTreeMap<String, Block>, String> {
    let mut blocks: BTreeMap<String, Block> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in text.lines() {
        if let Some(header) = line.strip_prefix("# ") {
            let id = header.split_whitespace().next().unwrap_or("").to_owned();
            if blocks.contains_key(&id) {
                return Err(format!("{PATH} lists {id} twice"));
            }
            blocks.insert(
                id.clone(),
                Block {
                    header: header.to_owned(),
                    lines: Vec::new(),
                },
            );
            current = Some(id);
        } else if !line.trim().is_empty() {
            let Some(block) = current.as_ref().and_then(|id| blocks.get_mut(id)) else {
                return Err(format!("{PATH}: hash line before any header: {line}"));
            };
            block.lines.push(line.to_owned());
        }
    }
    Ok(blocks)
}

pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

pub(crate) fn text(root: &Path) -> Result<String, String> {
    if std::fs::symlink_metadata(root.join(PATH))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Ok(String::new());
    }
    let bytes = crate::harness::datasets::regular_in(root, PATH)?;
    String::from_utf8(bytes).map_err(|e| format!("{PATH}: {e}"))
}

pub(crate) fn hashes(root: &Path, id: &str) -> Result<Vec<String>, String> {
    let tuning = id.starts_with(&format!("{}/", crate::layout::TUNING));
    let walked: Vec<String> = if id == HARNESS {
        (HARNESS_FILES.iter())
            .filter_map(|f| f.strip_suffix('/').map(str::to_owned))
            .collect()
    } else if tuning {
        vec![id.to_owned()]
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
    } else if tuning {
        walk::files(root, id)
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
            let mut bytes = crate::harness::datasets::regular_in(root, &file)?;
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
    let mut text = text(root).unwrap_or_else(|e| format!("! {e}"));
    for id in ids(root).unwrap_or_default() {
        text.push_str(&lines(root, &id));
    }
    hex(&Sha256::digest(text))
}

fn ids(root: &Path) -> Result<Vec<String>, String> {
    let cases = case::ids(root)?;
    Ok((std::iter::once(HARNESS.to_owned()))
        .chain(cases)
        .chain(crate::layout::tuning_settings(root))
        .collect())
}

fn is_case(id: &str) -> bool {
    case::parse_id(id).is_ok()
}

pub(crate) fn state(root: &Path, id: &str) -> Result<State, String> {
    Ok(state_in(&blocks(&text(root)?)?, root, id))
}

fn state_in(blocks: &BTreeMap<String, Block>, root: &Path, id: &str) -> State {
    let lines = match hashes(root, id) {
        Ok(lines) => lines,
        Err(e) => return State::Broken(e),
    };
    match blocks.get(id) {
        Some(block) if block.lines == lines => State::Reviewed,
        Some(_) => State::Stale,
        None => State::Unlocked,
    }
}

pub(crate) fn findings(root: &Path) -> Findings {
    let locked = match text(root).and_then(|text| blocks(&text)) {
        Ok(locked) => locked,
        Err(e) => return vec![format!("science: S5 {e}")],
    };
    let ids = match ids(root) {
        Ok(ids) => ids,
        Err(e) => return vec![format!("science: S5 {e}")],
    };
    let mut findings = Vec::new();
    for id in locked.keys().filter(|id| !ids.contains(id)) {
        findings.push(if is_case(id) {
            format!("science: S5 locked case {id} disappeared; set status = \"retired\" instead of deleting or renaming")
        } else {
            format!("science: S5 locked {id} disappeared; a tuned setting keeps the inputs it was tuned on")
        });
    }
    for id in ids.iter().map(String::as_str) {
        match state_in(&locked, root, id) {
            State::Unlocked => findings.push(format!(
                "science: S5 {id} is not in {PATH}; run `cargo xtask lock --reviewer <name> --reason <text>`"
            )),
            State::Stale => findings.push(format!(
                "science: S5 {id} changed since its LOCK review; run `cargo xtask lock` and get a science review"
            )),
            State::Reviewed => {}
            State::Broken(e) => findings.push(format!("science: S5 {id}: {e}")),
        }
        if let Some(block) = locked.get(id)
            && !(block.header.contains(" reviewer=") && block.header.contains(" reason="))
        {
            findings.push(format!(
                "science: S5 {id} LOCK header needs reviewer= and reason="
            ));
        }
    }
    findings
}

pub(crate) fn write(root: &Path, reviewer: &str, reason: &str) -> Findings {
    if reviewer.trim().is_empty() || reason.trim().is_empty() {
        return vec!["usage: cargo xtask lock --reviewer <name> --reason <text>".into()];
    }
    let locked = match text(root).and_then(|text| blocks(&text)) {
        Ok(locked) => locked,
        Err(e) => return vec![format!("lock: {e}")],
    };
    let ids = match ids(root) {
        Ok(ids) => ids,
        Err(e) => return vec![format!("lock: {e}")],
    };
    let max_locked = locked
        .keys()
        .filter_map(|id| case::parse_id(id).ok())
        .max()
        .unwrap_or(0);
    let mut findings = Vec::new();
    for id in locked.keys().filter(|id| !ids.contains(id)) {
        findings.push(if is_case(id) {
            format!("lock: {id} disappeared; restore it with status = \"retired\"")
        } else {
            format!("lock: {id} disappeared; restore the inputs it was tuned on")
        });
    }
    for id in (ids.iter()).filter(|id| is_case(id) && !locked.contains_key(*id)) {
        if case::parse_id(id).unwrap_or(0) <= max_locked {
            findings.push(format!(
                "lock: new case {id} must use a number above {max_locked}"
            ));
        }
    }
    if !findings.is_empty() {
        return findings;
    }
    let line = format!(
        "reviewer={} reason={}",
        reviewer.trim(),
        reason.replace('\n', " ").trim()
    );
    let mut out = String::new();
    for id in ids.iter().map(String::as_str) {
        let lines = match hashes(root, id) {
            Ok(lines) => lines,
            Err(e) => return vec![format!("lock: {id}: {e}")],
        };
        let header = match locked.get(id) {
            Some(block) if block.lines == lines => block.header.clone(),
            _ => format!("{id} {line}"),
        };
        let _ = writeln!(out, "# {header}");
        for line in lines {
            let _ = writeln!(out, "{line}");
        }
    }
    match std::fs::write(root.join(PATH), out) {
        Ok(()) => Vec::new(),
        Err(e) => vec![format!("lock: {PATH}: {e}")],
    }
}
