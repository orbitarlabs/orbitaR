use crate::{
    Findings, case, docs,
    syntax::{self, Tree},
    walk,
};
use proc_macro2::Delimiter;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const KIND: &str = "native";
const PYLOCK: &str = "python/pylock.toml";
const PREFIXES: [&str; 4] = ["crate:", "python:", "mcp:", "tool:"];
const DATA: [&str; 9] = [
    ".md", ".toml", ".json", ".yml", ".yaml", ".cff", ".txt", ".csv", ".lock",
];
const DATA_NAMES: [&str; 2] = [".gitignore", ".gitattributes"];
const BLOCKS: [&str; 2] = ["(not tested: license", "(not tested: platform"];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    status: Status,
    #[serde(rename = "use")]
    provider: String,
    #[serde(default)]
    code: Vec<String>,
    #[serde(default)]
    proof: Vec<String>,
    #[serde(default)]
    tried: Vec<String>,
    version: Option<String>,
    license: Option<String>,
}

#[derive(Deserialize, PartialEq, Eq, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum Status {
    Adopted,
    Planned,
}

type Rows = BTreeMap<String, Row>;

pub(crate) fn check(root: &Path) -> Findings {
    let manifest = match docs::declared(root, KIND) {
        Ok(Some(path)) => match toml::from_str::<Rows>(&walk::read(root, &path)) {
            Ok(rows) => Some((path, rows)),
            Err(e) => return vec![format!("native: {path}: {e}")],
        },
        Ok(None) => {
            println!(
                "native: no local manifest: CLAUDE.local.md declares none, so only the rules without rows run"
            );
            None
        }
        Err(e) => return vec![format!("native: {e}")],
    };
    let rows = manifest.as_ref().map(|(_, rows)| rows);
    let cases = match case::ids(root) {
        Ok(cases) => cases,
        Err(e) if root.join("benchmarks").is_dir() => return vec![format!("native: {e}")],
        Err(_) => Vec::new(),
    };
    let code: Vec<String> = (walk::files(root, ".").into_iter())
        .filter(|f| is_code(f))
        .collect();
    let mut findings = match &manifest {
        Some((path, rows)) => coverage(root, path, rows, &cases, &code),
        None => Vec::new(),
    };
    for file in code.iter().filter(|f| f.ends_with(".rs")) {
        findings.extend(programs(root, file, rows));
    }
    findings.extend(installs(root, rows));
    findings.extend(python_pins(root, rows));
    findings
}

fn coverage(root: &Path, path: &str, rows: &Rows, cases: &[String], code: &[String]) -> Findings {
    let mut findings = Vec::new();
    let dependencies = dependencies(root);
    let mut covered: BTreeMap<String, String> = BTreeMap::new();
    for (name, row) in rows {
        findings.extend(row_findings(root, name, row, &dependencies, cases));
        for file in row.code.iter().flat_map(|entry| expand(root, entry)) {
            if let Some(other) = covered.insert(file.clone(), name.clone()) {
                findings.push(format!("native: {file} is in both [{other}] and [{name}]"));
            }
        }
    }
    for krate in &dependencies {
        let adopted = rows.values().any(|r| {
            r.status == Status::Adopted && r.provider.strip_prefix("crate:") == Some(krate)
        });
        if !adopted {
            findings.push(format!(
                "native: dependency {krate} has no adopted row in {path}; list the options that it beat"
            ));
        }
    }
    for file in code.iter().filter(|f| !covered.contains_key(*f)) {
        findings.push(format!(
            "native: {file} is in no {path} row; list the native options that you tried"
        ));
    }
    findings
}

#[derive(Deserialize)]
struct Pylock {
    #[serde(default)]
    packages: Vec<Pin>,
}

#[derive(Deserialize)]
struct Pin {
    name: String,
    version: String,
}

fn python_pins(root: &Path, rows: Option<&Rows>) -> Findings {
    let text = walk::read(root, PYLOCK);
    if text.is_empty() {
        return Vec::new();
    }
    let lock: Pylock = match toml::from_str(&text) {
        Ok(lock) => lock,
        Err(e) => return vec![format!("native: {PYLOCK}: {e}")],
    };
    let Some(rows) = rows else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for pin in &lock.packages {
        let provider = format!("python:{}", pin.name.to_lowercase());
        let versions: BTreeSet<&str> = (rows.values())
            .filter(|r| r.provider == provider)
            .filter_map(|r| r.version.as_deref())
            .collect();
        if versions.iter().any(|v| *v != pin.version) || versions.is_empty() {
            findings.push(format!(
                "native: {PYLOCK} pins {} {}, but the {provider} rows say {versions:?}; one version for both",
                pin.name, pin.version
            ));
        }
    }
    findings
}

fn installs(root: &Path, rows: Option<&Rows>) -> Findings {
    let ci = walk::read(root, ".github/workflows/ci.yml");
    let mut findings = Vec::new();
    for (k, line) in ci.lines().enumerate() {
        let Some(rest) = line.split("cargo install ").nth(1) else {
            continue;
        };
        let words: Vec<&str> = rest.split_whitespace().collect();
        let tool = words.first().copied().unwrap_or("");
        let version = (words.windows(2))
            .find(|w| w[0] == "--version")
            .map(|w| w[1])
            .filter(|v| !v.starts_with('-'));
        let at = format!("native: ci.yml:{} installs {tool}", k + 1);
        let Some(version) = version.filter(|_| words.contains(&"--locked")) else {
            findings.push(format!(
                "{at}; pin it with `--version <version>` and `--locked`"
            ));
            continue;
        };
        let provider = format!("tool:{tool}");
        let pinned = rows.is_none_or(|rows| {
            rows.values().any(|r| {
                r.status == Status::Adopted
                    && r.provider == provider
                    && r.version.as_deref() == Some(version)
            })
        });
        if !pinned {
            findings.push(format!(
                "{at} {version}; add an adopted {provider} row at that version"
            ));
        }
    }
    findings
}

fn is_code(file: &str) -> bool {
    let name = file.rsplit('/').next().unwrap_or(file);
    let data = (DATA.iter().any(|s| name.ends_with(s)) && name != ".mcp.json")
        || DATA_NAMES.contains(&name)
        || name.starts_with("LICENSE");
    let test = file.ends_with(".rs")
        && (file.starts_with("xtask/tests/")
            || (file.starts_with("tests/") && !file.starts_with("tests/benchmarks/")));
    let declares =
        file.starts_with("src/") && file.matches('/').count() == 1 && file != "src/main.rs";
    let agents = file.starts_with(".agents/") || file.starts_with(".claude/");
    !(data || test || declares || agents)
}

fn programs(root: &Path, file: &str, rows: Option<&Rows>) -> Findings {
    let Ok(trees) = syntax::parse(&walk::read(root, file)) else {
        return Vec::new();
    };
    let mut findings = Vec::new();
    for (rest, line) in syntax::after_path(&trees, "Command") {
        let [
            Tree::Ident(new, _),
            Tree::Group(Delimiter::Parenthesis, args, _),
            ..,
        ] = rest
        else {
            continue;
        };
        let program = match args.first() {
            Some(Tree::Literal(_, Some(program), _)) => program.clone(),
            _ => {
                let spelled = spelled(args);
                let engine = file == "xtask/src/harness/engine.rs"
                    && matches!(spelled.as_str(), "&engine" | "engine" | "&shim");
                if new == "new" && !(spelled == "&env.cargo" || engine) {
                    findings.push(format!(
                        "native: {file}:{line} runs the program in `{spelled}`; write its name as a literal so its row shows"
                    ));
                }
                continue;
            }
        };
        let provider = format!("tool:{program}");
        let listed = rows.is_none_or(|rows| {
            rows.values()
                .any(|r| r.status == Status::Adopted && r.provider == provider)
        });
        if new == "new" && !listed {
            findings.push(format!(
                "native: {file}:{line} runs `{program}`; add an adopted {provider} row with its version and license"
            ));
        }
    }
    for (a, b, line) in syntax::pairs(&trees) {
        if a == "Command" && b == "as" {
            findings.push(format!(
                "native: {file}:{line} renames Command; name it in full so each program shows"
            ));
        }
    }
    for k in 0..trees.len() {
        if let [
            keyword,
            Tree::Ident(_, line),
            Tree::Punct('=', _),
            rest @ ..,
        ] = &trees[k..]
            && keyword.is_ident("type")
            && rest
                .iter()
                .take_while(|t| !t.is_punct(';'))
                .any(|t| t.is_ident("Command"))
        {
            findings.push(format!(
                "native: {file}:{line} renames Command; name it in full so each program shows"
            ));
        }
    }
    findings
}

fn spelled(trees: &[Tree]) -> String {
    trees
        .iter()
        .take_while(|t| !t.is_punct(','))
        .map(|t| match t {
            Tree::Ident(name, _) => name.clone(),
            Tree::Punct(c, _) => c.to_string(),
            Tree::Literal(text, _, _) => text.clone(),
            Tree::Group(..) => "(…)".to_owned(),
        })
        .collect()
}

fn names(root: &Path, file: &str, ident: &str) -> bool {
    syntax::parse(&walk::read(root, file)).is_ok_and(|trees| {
        syntax::idents(&trees)
            .iter()
            .any(|(name, _)| *name == ident)
    })
}

fn row_findings(
    root: &Path,
    name: &str,
    row: &Row,
    dependencies: &BTreeSet<String>,
    cases: &[String],
) -> Findings {
    let at = format!("native: [{name}]");
    let mut findings = Vec::new();
    let target = PREFIXES
        .iter()
        .find_map(|p| row.provider.strip_prefix(p).map(|rest| (*p, rest)));
    let known = matches!(row.provider.as_str(), "std" | "custom")
        || target.is_some_and(|(_, rest)| !rest.is_empty());
    if !known {
        findings.push(format!(
            "{at} use = \"{}\" is not std, custom, or crate:|python:|mcp:|tool:<name>",
            row.provider
        ));
    }
    let needs_search = !(row.provider == "std" && row.code.is_empty());
    if needs_search && row.tried.is_empty() {
        findings.push(format!(
            "{at} lists no options in `tried`; list each option that you tested and its result"
        ));
    }
    if row.tried.iter().any(|t| t.trim().is_empty()) {
        findings.push(format!("{at} has an empty `tried` entry"));
    }
    for entry in &row.tried {
        if entry.contains("(not tested") && !BLOCKS.iter().any(|b| entry.contains(b)) {
            findings.push(format!(
                "{at} `tried` has \"{entry}\"; test the option, or name its block as (not tested: license …) or (not tested: platform …)"
            ));
        }
    }
    if matches!(target, Some(("python:" | "mcp:" | "tool:", _)))
        && (row.version.is_none() || row.license.is_none())
    {
        findings.push(format!(
            "{at} is outside Cargo; pin its `version` and name its `license`"
        ));
    }
    match row.status {
        Status::Adopted => {
            if row.proof.is_empty() {
                findings.push(format!("{at} is adopted; name its proof"));
            }
            if row.provider == "custom" && row.code.is_empty() {
                findings.push(format!("{at} is custom; list its code"));
            }
            if let Some(("crate:", krate)) = target {
                if !dependencies.contains(krate) {
                    findings.push(format!("{at} uses {krate}, which no Cargo.toml depends on"));
                }
                let ident = krate.replace('-', "_");
                let files = row.code.iter().flat_map(|entry| expand(root, entry));
                for file in files.filter(|f| !names(root, f, &ident)) {
                    findings.push(format!(
                        "{at} lists {file}, which never names {krate}; give that file its own row"
                    ));
                }
            }
        }
        Status::Planned => {
            if !row.code.is_empty() {
                findings.push(format!("{at} is planned but lists code; mark it adopted"));
            }
        }
    }
    for path in &row.proof {
        let case_dir = path
            .strip_prefix("benchmarks/")
            .is_some_and(|id| cases.iter().any(|c| c == id.trim_end_matches('/')));
        let test = (path.starts_with("tests/") || path.starts_with("xtask/tests/"))
            && path.ends_with(".rs")
            && !path.contains("..")
            && root.join(path).is_file();
        if !(case_dir || test) {
            findings.push(format!(
                "{at} proof {path} must be a test file (tests/<…>.rs, xtask/tests/<…>.rs) or a benchmarks/<N>_<slug> case"
            ));
        }
    }
    for path in &row.code {
        let segments = path.split('/');
        if path.contains('*') && segments.clone().any(|s| s != "*" && s.contains('*')) {
            findings.push(format!(
                "{at} names {path}; `*` stands for one whole path segment"
            ));
        } else if path.contains('*') && expand(root, path).is_empty() {
            findings.push(format!("{at} names {path}, which matches no file"));
        } else if !path.contains('*') && (path.contains("..") || !root.join(path).is_file()) {
            findings.push(format!(
                "{at} names {path}, which is not a file in the repository"
            ));
        }
    }
    findings
}

fn expand(root: &Path, entry: &str) -> Vec<String> {
    if !entry.contains('*') {
        return vec![entry.to_owned()];
    }
    let pattern: Vec<&str> = entry.split('/').collect();
    walk::files(root, ".")
        .into_iter()
        .filter(|file| {
            let segments: Vec<&str> = file.split('/').collect();
            segments.len() == pattern.len()
                && segments
                    .iter()
                    .zip(&pattern)
                    .all(|(s, p)| *p == "*" || s == p)
        })
        .collect()
}

fn dependencies(root: &Path) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for manifest in crate::manifest::manifests() {
        if let Ok(table) = toml::from_str::<toml::Table>(&walk::read(root, &manifest)) {
            for (_, deps) in crate::manifest::tables(&table) {
                out.extend(deps.keys().cloned());
            }
        }
    }
    out
}
