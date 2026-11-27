use crate::{
    Findings,
    case::{self, Case, Status},
    layout, lock, results,
    syntax::{self, Tree},
    walk,
};
use proc_macro2::Delimiter;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, path::Path, process::Command};

const SOURCE_TABLE: &str = "src/SOURCE.toml";
type Citations = BTreeMap<String, String>;

const ELEMENTS: &str = "H He Li Be B C N O F Ne Na Mg Al Si P S Cl Ar K Ca Sc Ti V Cr Mn Fe Co Ni Cu Zn Ga Ge As Se Br Kr Rb Sr Y Zr Nb Mo Tc Ru Rh Pd Ag Cd In Sn Sb Te I Xe Cs Ba La Ce Pr Nd Pm Sm Eu Gd Tb Dy Ho Er Tm Yb Lu Hf Ta W Re Os Ir Pt Au Hg Tl Pb Bi Po At Rn Fr Ra Ac Th Pa U Np Pu Am Cm";
const VASP_NAMES: [&str; 26] = [
    "potcar",
    "outcar",
    "oszicar",
    "vasprun.xml",
    "wavecar",
    "chgcar",
    "chg",
    "contcar",
    "xdatcar",
    "locpot",
    "elfcar",
    "procar",
    "doscar",
    "eigenval",
    "ibzkpt",
    "pcdat",
    "report",
    "incar",
    "kpoints",
    "vaspout.h5",
    "vaspwave.h5",
    "vasp_std",
    "vasp_gam",
    "vasp_ncl",
    "main.f",
    "vasp_timing",
];
const DATASET_SUFFIXES: [&str; 3] = [".upf", ".psp8", ".paw"];
const INCAR_TAGS: [&str; 12] = [
    "ENCUT", "ISMEAR", "SIGMA", "IBRION", "ISIF", "NSW", "EDIFF", "PREC", "ALGO", "NCORE", "KPAR",
    "LREAL",
];
const UNIT_WORDS: [&str; 7] = [
    "angstrom",
    "electronvolt",
    "rydberg",
    "kcal",
    "kelvin",
    "kbar",
    "gpa",
];
const UNIT_ABBREVIATIONS: [&str; 4] = ["ev", "ang", "ry", "aa"];
const CONVERSIONS: [f64; 8] = [
    0.529_177_210_903,
    1.889_726_124_626,
    27.211_386_245_988,
    0.036_749_322_175_655,
    13.605_693_122_994,
    627.509_474_277,
    315_775.024_804,
    29_421.015_697,
];
const EMBEDS: [&str; 5] = [
    "include",
    "include_str",
    "include_bytes",
    "env",
    "option_env",
];
const BUILD_FACTS: [(&str, &str); 2] = [
    ("env", "CARGO_PKG_VERSION"),
    ("option_env", "ORBITAR_COMMIT"),
];
const CONTAINMENT: [&str; 5] = [
    "disallowed_methods",
    "disallowed_types",
    "disallowed_macros",
    "style",
    "all",
];
const LIFTS: [&str; 2] = ["src/main.rs", "src/io/run.rs"];
const OUTSIDE: [&str; 5] = ["fs", "process", "env", "net", "os"];

pub(crate) fn check(root: &Path, base: Option<&str>) -> Findings {
    let mut findings = Vec::new();
    let references = reference_numbers(root);
    let ids = match case::ids(root) {
        Ok(ids) => ids,
        Err(e) => return vec![format!("science: S5 {e}")],
    };
    let engine_files = walk::files(root, "src");
    let mut sources: BTreeMap<String, Citations> = BTreeMap::new();
    if engine_files.iter().any(|file| file == SOURCE_TABLE) {
        match toml::from_str(&walk::read(root, SOURCE_TABLE)) {
            Ok(table) => sources = table,
            Err(e) => findings.push(format!("science: {SOURCE_TABLE}: S2 {e}")),
        }
    }
    for (file, entries) in &mut sources {
        entries.retain(|name, citation| {
            let cited = !citation.trim().is_empty();
            if !cited {
                findings.push(format!(
                    "science: {SOURCE_TABLE}: S2 {file}::{name} has an empty citation"
                ));
            }
            cited
        });
    }
    for file in engine_files {
        if file == SOURCE_TABLE {
            continue;
        }
        if !file.ends_with(".rs") {
            findings.push(format!(
                "science: S3 {file} is not Rust; the engine reads data only through src/io/"
            ));
            continue;
        }
        let text = walk::read(root, &file);
        scan(
            &file,
            &text,
            &references,
            &ids,
            Scope::Engine,
            sources.get_mut(&file),
            &mut findings,
        );
    }
    for (file, entries) in sources {
        for name in entries.keys() {
            findings.push(format!(
                "science: {SOURCE_TABLE}: S2 {file}::{name} is not a constant in src/"
            ));
        }
    }
    for file in walk::files(root, "tests/benchmarks")
        .into_iter()
        .chain(["tests/benchmarks.rs".to_owned()])
        .filter(|f| f.ends_with(".rs"))
    {
        let mut text = walk::read(root, &file);
        if file == layout::RUNNER {
            text = without_table(&text);
        }
        scan(
            &file,
            &text,
            &references,
            &ids,
            Scope::Measure,
            None,
            &mut findings,
        );
    }
    for link in walk::links(root, "src") {
        findings.push(format!(
            "science: S3 {link} is a symlink; the engine compiles real files only"
        ));
    }
    findings.extend(lock::findings(root));
    for dir in ["benchmarks", "tests/benchmarks"] {
        for link in walk::links(root, dir) {
            findings.push(format!(
                "science: S5 {link} is a symlink; the lock hashes real files only"
            ));
        }
    }
    findings.extend(forbidden_files(root));
    findings.extend(tracked_but_skipped(root));
    findings.extend(separate_judge(root));
    findings.extend(provenance(root, &ids));
    findings.extend(one_functional(root));
    findings.extend(results::check(root));
    if let Some(base) = base {
        findings.extend(easier(root, base, &ids));
        findings.extend(fixed_families(root, base));
    }
    findings.dedup();
    findings
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scope {
    Engine,
    Measure,
}

fn without_table(text: &str) -> String {
    match layout::current_table(text) {
        Ok((start, end)) => {
            let table: String = text[start..end]
                .chars()
                .map(|c| if c == '\n' { c } else { ' ' })
                .collect();
            format!("{}{table}{}", &text[..start], &text[end..])
        }
        Err(_) => text.to_owned(),
    }
}

fn embeds(trees: &[Tree], main: bool) -> Vec<(&str, usize)> {
    (syntax::macros(trees).into_iter())
        .filter(|(name, _, body)| {
            let fact = main
                && matches!(body, [Tree::Literal(_, Some(text), _)]
                    if BUILD_FACTS.contains(&(*name, text.as_str())));
            EMBEDS.contains(name) && !fact
        })
        .map(|(name, line, _)| (name, line))
        .collect()
}

fn sealed(file: &str, trees: &[Tree], scope: Scope, findings: &mut Findings) {
    let engine = scope == Scope::Engine;
    let reads = if engine {
        "the engine reads input only through src/io/"
    } else {
        "a measure reads only its Evidence"
    };
    for (name, line) in embeds(trees, engine && file == "src/main.rs") {
        {
            findings.push(format!(
                "science: {file}:{line}: S3 `{name}!` reads a file or the build environment; {reads}"
            ));
        }
    }
    for (body, line) in syntax::attributes(trees) {
        if !syntax::assigns(body, "path").is_empty() {
            findings.push(format!(
                "science: {file}:{line}: S3 a `path =` attribute compiles a file from elsewhere; {reads}"
            ));
        }
        if !engine {
            continue;
        }
        let lints = syntax::after_path(body, "clippy");
        let lifted = lints.iter().find_map(|(rest, _)| match rest.first() {
            Some(Tree::Ident(lint, _)) if CONTAINMENT.contains(&lint.as_str()) => {
                Some(lint.clone())
            }
            _ => None,
        });
        let group = lifted
            .as_deref()
            .is_some_and(|l| !l.starts_with("disallowed_"));
        if let Some(lint) = lifted
            && (group || !LIFTS.contains(&file))
        {
            findings.push(format!(
                "science: {file}:{line}: S3 an attribute names `clippy::{lint}`; only src/main.rs and src/io/run.rs lift a containment lint, by name"
            ));
        }
        if syntax::idents(body)
            .iter()
            .any(|(name, _)| *name == "warnings")
        {
            findings.push(format!(
                "science: {file}:{line}: S3 an attribute names `warnings`; it would lift every containment lint"
            ));
        }
    }
    if engine {
        return;
    }
    for (rest, line) in syntax::after_path(trees, "std") {
        let heads: Vec<&str> = match rest.first() {
            Some(Tree::Ident(name, _)) => vec![name.as_str()],
            Some(Tree::Group(Delimiter::Brace, body, _)) => syntax::idents(body)
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            _ => Vec::new(),
        };
        if let Some(module) = heads.into_iter().find(|m| OUTSIDE.contains(m)) {
            findings.push(format!(
                "science: {file}:{line}: S3 a measure names std::{module}; {reads}"
            ));
        }
    }
    for (a, b, line) in syntax::pairs(trees) {
        if a == "std" && b == "as" {
            findings.push(format!(
                "science: {file}:{line}: S3 a measure renames std; name each std path in full"
            ));
        }
    }
}

fn scan(
    file: &str,
    text: &str,
    references: &[f64],
    ids: &[String],
    scope: Scope,
    sources: Option<&mut Citations>,
    findings: &mut Findings,
) {
    let trees = match syntax::parse(text) {
        Ok(trees) => trees,
        Err(e) => {
            findings.push(format!("science: {file} {e}"));
            return;
        }
    };
    sealed(file, &trees, scope, findings);
    let mut tokens = Vec::new();
    flatten(&trees, &mut tokens, sources, false);
    let io = file.starts_with("src/io/") || file == "src/io.rs";
    let engine = scope == Scope::Engine;
    for (k, t) in tokens.iter().enumerate() {
        let at = format!("science: {file}:{}", t.line);
        match &t.kind {
            Kind::Number {
                value,
                digits,
                float,
            } => {
                if requires_citation(*digits, *float) && !t.cited {
                    let fix = if engine {
                        "needs a named const in src/SOURCE.toml"
                    } else {
                        "sits in measure code; read it from the case's reference/ files"
                    };
                    findings.push(format!("{at}: S2 {} {fix}", t.text));
                }
                if references.iter().any(|r| close(*value, *r)) {
                    findings.push(format!(
                        "{at}: S1 {} equals a benchmark reference value",
                        t.text
                    ));
                }
                if engine && !io && *digits >= 3 && conversion(*value) {
                    findings.push(format!(
                        "{at}: S3 {} is a unit conversion factor; convert units only in src/io/",
                        t.text
                    ));
                }
                if let Some(v) = fold(&tokens[k..])
                    && references.iter().any(|r| close(v, *r))
                {
                    findings.push(format!(
                        "{at}: S1 a literal expression evaluates to a benchmark reference value ({v})"
                    ));
                }
            }
            Kind::Str(s) => {
                let trimmed = s.trim();
                for (text, value, digits, float) in numbers_in(s) {
                    if requires_citation(digits, float) {
                        findings.push(format!(
                            "{at}: S2 the string \"{trimmed}\" hides the number {text}; write a named const"
                        ));
                    }
                    if references.iter().any(|r| close(value, *r)) {
                        findings.push(format!(
                            "{at}: S1 the string \"{trimmed}\" holds a benchmark reference value ({text})"
                        ));
                    }
                }
                if !engine {
                    continue;
                }
                if !io && ELEMENTS.split(' ').any(|e| e.eq_ignore_ascii_case(trimmed)) {
                    findings.push(format!(
                        "{at}: S4 \"{s}\" is an element symbol; derive behavior from the dataset"
                    ));
                }
                if s.contains("benchmarks")
                    || s.contains("case.toml")
                    || ids.iter().any(|id| s.contains(id.as_str()))
                {
                    findings.push(format!(
                        "{at}: S3 engine code names a benchmark path or case"
                    ));
                }
                if s.starts_with('/') || s.starts_with('\\') || s.get(1..3) == Some(":\\") {
                    findings.push(format!("{at}: S3 engine code names an absolute path"));
                }
            }
            Kind::Ident(name) => {
                let hides = [
                    "from_bits",
                    "from_le_bytes",
                    "from_be_bytes",
                    "from_ne_bytes",
                ]
                .contains(&name.as_str());
                let literal = || {
                    let argument =
                        (tokens.iter().skip(k + 2)).find(|t| t.text != "*" && t.text != "&");
                    (tokens.get(k + 1)).is_some_and(|t| t.text == "(")
                        && argument.is_some_and(|t| {
                            t.text == "[" || matches!(t.kind, Kind::Number { .. } | Kind::Str(_))
                        })
                };
                if hides && (!io || literal()) {
                    findings.push(format!(
                        "{at}: S2 {name} hides a constant; write a named const in {SOURCE_TABLE}"
                    ));
                }
                if !engine {
                    continue;
                }
                let lower = name.to_lowercase();
                let words: Vec<&str> = lower
                    .split(|c: char| c == '_' || c.is_ascii_digit())
                    .collect();
                let unit = UNIT_WORDS.iter().any(|w| lower.contains(w))
                    || words.iter().any(|w| UNIT_ABBREVIATIONS.contains(w));
                if !io && unit {
                    findings.push(format!(
                        "{at}: S3 `{name}` names a non-atomic unit; convert units only in src/io/"
                    ));
                }
            }
            Kind::Punct => {}
        }
    }
}

fn numbers_in(s: &str) -> Vec<(String, f64, usize, bool)> {
    let c: Vec<char> = s.chars().collect();
    let mut out = Vec::new();
    let mut end = 0;
    for i in 0..c.len() {
        let starts = c[i].is_ascii_digit()
            || (c[i] == '.' && c.get(i + 1).is_some_and(char::is_ascii_digit));
        let inside_word = i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_');
        if i < end || !starts || inside_word {
            continue;
        }
        end = i + number_length(&c[i..]);
        let text: String = c[i..end].iter().collect();
        let text = text.trim_end_matches('.').to_owned();
        if let Kind::Number {
            value,
            digits,
            float,
        } = number(&text)
            && value.is_finite()
        {
            out.push((text, value, digits, float));
        }
    }
    out
}

fn number_length(c: &[char]) -> usize {
    let mut seen_dot = false;
    let mut skip = 0;
    for (j, &x) in c.iter().enumerate() {
        if j < skip {
            continue;
        }
        let exponent = matches!(x, 'e' | 'E')
            && (c.get(j + 1).is_some_and(char::is_ascii_digit)
                || (matches!(c.get(j + 1), Some('+' | '-'))
                    && c.get(j + 2).is_some_and(char::is_ascii_digit)));
        if x.is_ascii_digit() || x == '_' || (x == '.' && !seen_dot) {
            seen_dot |= x == '.';
        } else if exponent {
            skip = j + 2;
        } else {
            return j;
        }
    }
    c.len()
}

fn conversion(value: f64) -> bool {
    CONVERSIONS.iter().any(|c| ((value - c) / c).abs() < 1e-4)
}

fn requires_citation(digits: usize, float: bool) -> bool {
    (float && digits >= 3) || digits >= 7
}

fn close(a: f64, b: f64) -> bool {
    b != 0.0 && ((a - b) / b).abs() <= 1e-6
}

fn forbidden_files(root: &Path) -> Findings {
    let owned = crate::harness::datasets::own_pins(root);
    let ours = owned.pins;
    let mut findings: Findings = (owned.faults.iter())
        .map(|fault| format!("science: S6 {fault}; such a table pins nothing"))
        .collect();
    findings
        .extend((walk::faults(root, ".").into_iter()).map(|fault| format!("science: S6 {fault}")));
    for file in walk::files(root, ".") {
        let lower = file.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower);
        let bytes = match crate::harness::datasets::regular_in(root, &file) {
            Ok(bytes) => bytes,
            Err(e) => {
                findings.push(format!("science: S6 {e}; every file is scanned"));
                continue;
            }
        };
        if std::str::from_utf8(&bytes).is_err() || bytes.contains(&0) {
            findings.push(format!(
                "science: S6 {file} is not UTF-8 text; archives and binaries stay outside the repository"
            ));
            continue;
        }
        let head = String::from_utf8_lossy(&bytes[..bytes.len().min(2048)]).into_owned();
        let banner = head.lines().any(|l| {
            l.trim_start()
                .strip_prefix("vasp.")
                .is_some_and(|r| r.starts_with(|c: char| c.is_ascii_digit()))
        });
        let rust = name.ends_with(".rs");
        let vasp_named = !rust
            && VASP_NAMES.iter().any(|n| {
                name.strip_prefix(n)
                    .is_some_and(|rest| rest.is_empty() || rest.starts_with(['.', '_', '-']))
            });
        let dataset_named = !rust && DATASET_SUFFIXES.iter().any(|s| name.ends_with(s));
        let tags = head
            .lines()
            .filter(|l| {
                let key = l.split('=').next().unwrap_or("").trim();
                l.contains('=') && INCAR_TAGS.contains(&key)
            })
            .count();
        let vasp_signed =
            head.contains("<modeling>") || head.contains("TITEL") || tags >= 2 || banner;
        let paw = head.contains("<paw_dataset");
        let other_dataset = head.contains("<UPF version");
        let ours = paw
            && name.ends_with(".xml")
            && !(vasp_signed || other_dataset)
            && (ours.get(&file)).is_some_and(|(_, sha)| *sha == lock::hex(&Sha256::digest(&bytes)));
        if (vasp_named || dataset_named || vasp_signed || paw || other_dataset) && !ours {
            findings.push(format!(
                "science: S6 {file} is a VASP file, a dataset, or a timing; keep it outside the repository"
            ));
        }
    }
    findings
}

fn tracked_but_skipped(root: &Path) -> Findings {
    if std::fs::symlink_metadata(root.join(".git"))
        .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
    {
        return Vec::new();
    }
    let out = match Command::new("git")
        .args(["ls-files", "-z"])
        .current_dir(root)
        .output()
    {
        Ok(out) if out.status.success() => out,
        Ok(out) => {
            return vec![format!(
                "science: S6 git ls-files failed: {}",
                String::from_utf8_lossy(&out.stderr).trim()
            )];
        }
        Err(e) => return vec![format!("science: S6 git ls-files failed: {e}")],
    };
    String::from_utf8_lossy(&out.stdout)
        .split('\0')
        .filter(|file| {
            walk::SKIP_AT_ROOT
                .iter()
                .any(|skip| file.strip_prefix(skip).is_some_and(|r| r.starts_with('/')))
        })
        .map(|file| {
            format!("science: S6 git tracks {file}, under a root that every scan skips; untrack it")
        })
        .collect()
}

fn separate_judge(root: &Path) -> Findings {
    let table = match toml::from_str::<toml::Table>(&walk::read(root, "xtask/Cargo.toml")) {
        Ok(table) => table,
        Err(e) => return vec![format!("science: S7 xtask/Cargo.toml: {e}")],
    };
    let mut findings = Vec::new();
    for (section, deps) in crate::manifest::tables(&table) {
        for (name, spec) in deps {
            let field = |key: &str| spec.get(key).and_then(toml::Value::as_str);
            let engine = name == "orbitar"
                || field("package") == Some("orbitar")
                || field("path").is_some_and(|p| p.trim_end_matches('/') == "..");
            if engine {
                findings.push(format!(
                    "science: S7 xtask [{section}] {name} links the engine; the judge must not depend on orbitar"
                ));
            }
        }
    }
    findings
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    kind: String,
    citation: String,
    locator: String,
    convention: String,
    license: String,
}

fn provenance(root: &Path, ids: &[String]) -> Findings {
    let mut findings = Vec::new();
    for id in ids {
        let dir = format!("benchmarks/{id}/reference");
        let files = walk::files(root, &dir);
        if files.is_empty() {
            continue;
        }
        let path = format!("{dir}/SOURCE.toml");
        let source: Source = match toml::from_str(&walk::read(root, &path)) {
            Ok(source) => source,
            Err(e) => {
                findings.push(format!(
                    "science: S8 {path} needs kind, citation, locator, convention, license: {e}"
                ));
                continue;
            }
        };
        let schemes = ["doi:", "https://", "oeis:", "arxiv:", "isbn:"];
        if !schemes.iter().any(|s| source.locator.starts_with(s)) {
            findings.push(format!(
                "science: S8 {path} locator must start with {}",
                schemes.join(", ")
            ));
        }
        for (key, value) in [
            ("citation", &source.citation),
            ("convention", &source.convention),
            ("license", &source.license),
        ] {
            if value.trim().is_empty() {
                findings.push(format!("science: S8 {path} has an empty {key}"));
            }
        }
        match Case::load(root, id) {
            Ok(case) if case.reference().name() != source.kind => findings.push(format!(
                "science: S8 {path} kind {} differs from case.toml reference {}",
                source.kind,
                case.reference().name()
            )),
            Ok(_) => {}
            Err(e) => findings.push(format!("science: S8 {path}: the case does not load: {e}")),
        }
    }
    findings
}

fn fixed_families(root: &Path, base: &str) -> Findings {
    let git = |args: &[&str]| {
        Command::new("git")
            .args(args)
            .current_dir(root)
            .output()
            .ok()
            .filter(|out| out.status.success())
            .map(|out| out.stdout)
    };
    let Some(listed) = git(&["ls-tree", "-r", "-z", base, "datasets/"]) else {
        return vec![format!("science: S9 cannot list datasets/ at {base}")];
    };
    let mut findings = Vec::new();
    let mut entries: Vec<(String, String)> = Vec::new();
    for record in listed.split(|b| *b == 0).filter(|r| !r.is_empty()) {
        let record = match String::from_utf8(record.to_vec()) {
            Ok(record) => record,
            Err(e) => {
                findings.push(format!(
                    "science: S9 {} at {base} is no UTF-8 path; a family's files have UTF-8 names",
                    String::from_utf8_lossy(e.as_bytes())
                ));
                continue;
            }
        };
        if let Some((meta, path)) = record.split_once('\t')
            && let Some(mode) = meta.split(' ').next()
        {
            entries.push((mode.to_owned(), path.to_owned()));
        }
    }
    let paths: Vec<&str> = entries.iter().map(|(_, path)| path.as_str()).collect();
    let names: Vec<&str> = (paths.iter())
        .filter_map(|p| p.strip_prefix("datasets/")?.strip_suffix(".toml"))
        .filter(|name| !name.contains('/'))
        .collect();
    let ours = |path: &str| {
        (path
            .strip_prefix("datasets/")
            .and_then(|rest| rest.split_once('/')))
        .is_some_and(|(dir, _)| names.contains(&dir))
    };
    let never = "a family never changes: add a new version with `cargo xtask family <new-name> --from <table.toml>`";
    for (mode, path) in entries
        .iter()
        .filter(|(_, p)| (p.ends_with(".toml") && p.matches('/').count() == 1) || ours(p))
    {
        let here = root.join(path);
        let Ok(meta) = std::fs::symlink_metadata(&here) else {
            findings.push(format!("science: S9 {path} differs from {base}; {never}"));
            continue;
        };
        if meta.file_type().is_symlink() || !(meta.is_file() || meta.is_dir()) {
            continue;
        }
        use std::os::unix::fs::PermissionsExt as _;
        let now = match meta.permissions().mode() & 0o111 {
            0 => "100644",
            _ => "100755",
        };
        if now != *mode {
            findings.push(format!(
                "science: S9 {path} has mode {now}, {base} has {mode}; a family never changes"
            ));
        }
        let Some(old) = git(&["show", &format!("{base}:{path}")]) else {
            findings.push(format!("science: S9 cannot read {path} at {base}"));
            continue;
        };
        if crate::harness::datasets::regular_in(root, path).ok() != Some(old) {
            findings.push(format!("science: S9 {path} differs from {base}; {never}"));
        }
    }
    for name in &names {
        for file in walk::files(root, &format!("datasets/{name}")) {
            if !paths.contains(&file.as_str()) {
                findings.push(format!(
                    "science: S9 {file} is new in {name} of {base}; a family never changes: add a new version"
                ));
            }
        }
    }
    for link in walk::links(root, "datasets") {
        findings.push(format!(
            "science: S9 {link} is a link; a family holds regular files"
        ));
    }
    for fault in walk::faults(root, "datasets") {
        findings.push(format!("science: S9 {fault}"));
    }
    for other in walk::others(root, "datasets") {
        findings.push(format!(
            "science: S9 {other} is no regular file or directory; a family holds regular files"
        ));
    }
    for file in walk::files(root, "datasets") {
        let Some(table) = (file.strip_prefix("datasets/"))
            .and_then(|f| f.strip_suffix(".toml"))
            .filter(|t| !t.contains('/') && !names.contains(t))
        else {
            continue;
        };
        let text = match crate::harness::datasets::regular_in(root, &file) {
            Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
            Err(e) => {
                findings.push(format!("science: S9 {e}"));
                continue;
            }
        };
        let rows = (toml::from_str::<toml::Table>(&text).ok())
            .and_then(|t| t.get("element")?.as_table().cloned())
            .unwrap_or_default();
        let named = rows.values().filter_map(|row| row.get("file")?.as_str());
        for row in named.map(str::to_owned).collect::<Vec<_>>() {
            let dir = row.split('/').next().unwrap_or_default();
            if let Some(owner) = names.iter().find(|n| n.eq_ignore_ascii_case(dir)) {
                findings.push(format!(
                    "science: S9 {file} names {row} in the directory of {owner} of {base}; a family never changes"
                ));
            }
        }
        if let Some(owner) = names.iter().find(|n| n.eq_ignore_ascii_case(table)) {
            findings.push(format!(
                "science: S9 {file} names the directory of {owner} of {base} in another case; a family never changes"
            ));
        }
    }
    findings
}

fn base_benchmarks(root: &Path, base: &str) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .args(["ls-tree", "-r", "-z", "--name-only", base, "benchmarks/"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("git ls-tree {base}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "git ls-tree {base}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    (out.stdout.split(|b| *b == 0).filter(|p| !p.is_empty()))
        .map(|p| String::from_utf8(p.to_vec()).map_err(|_| format!("a path of {base} is no UTF-8")))
        .collect()
}

fn easier(root: &Path, base: &str, ids: &[String]) -> Findings {
    let show = |path: &str| -> Option<String> {
        let out = Command::new("git")
            .args(["show", &format!("{base}:{path}")])
            .current_dir(root)
            .output()
            .ok()?;
        out.status
            .success()
            .then(|| String::from_utf8_lossy(&out.stdout).into_owned())
    };
    let listed = match base_benchmarks(root, base) {
        Ok(listed) => listed,
        Err(e) => {
            return vec![format!(
                "science: S9 cannot read {} at {base}: {e}",
                lock::DIR
            )];
        }
    };
    let old_lock = match lock::at(root, base, &listed) {
        Ok(Some(old)) => old,
        Ok(None) if listed.is_empty() => return Vec::new(),
        Ok(None) => return vec![format!("science: S9 cannot read {} at {base}", lock::DIR)],
        Err(e) => return vec![format!("science: S9 {e}")],
    };
    let new_lock = match lock::read(root) {
        Ok(new) => new,
        Err(e) => return vec![format!("science: S9 {e}")],
    };
    let mut findings = Vec::new();
    let base_max = old_lock
        .keys()
        .filter_map(|id| case::parse_id(id).ok())
        .max()
        .unwrap_or(0);
    for id in old_lock.keys().filter(|id| *id != lock::HARNESS) {
        if !ids.contains(id) {
            findings.push(format!(
                "science: S9 {id} exists at {base} but not here; retire a case, never delete or renumber it"
            ));
        }
    }
    for id in ids.iter().filter(|id| !old_lock.contains_key(*id)) {
        if case::parse_id(id).unwrap_or(0) <= base_max {
            findings.push(format!(
                "science: S9 new case {id} must use a number above {base_max}, the largest at {base}"
            ));
        }
    }
    for (block, files) in lock::unreviewed(&old_lock, &new_lock) {
        let name = match block == lock::HARNESS {
            true => "the harness",
            false => block.as_str(),
        };
        findings.push(format!(
            "science: S9 {name} changed without a new lock review: {}",
            files.join(", ")
        ));
    }
    for id in ids {
        let path = format!("benchmarks/{id}/case.toml");
        if !listed.contains(&path) {
            continue;
        }
        let Some(old_text) = show(&path) else {
            findings.push(format!("science: S9 cannot read {path} at {base}"));
            continue;
        };
        let (old, new) = match (Case::parse(&old_text), Case::load(root, id)) {
            (Ok(old), Ok(new)) => (old, new),
            (Err(e), _) => {
                findings.push(format!("science: S9 {id} at {base}: {e}"));
                continue;
            }
            (_, Err(e)) => {
                findings.push(format!("science: S9 {e}"));
                continue;
            }
        };
        let mut looser = Vec::new();
        for (key, old_value) in old.tolerance() {
            match new.tolerance().get(key) {
                None => looser.push(format!("{key} removed")),
                Some(v) if key.ends_with("_max") && v > old_value => {
                    looser.push(format!("{key} {old_value} → {v}"))
                }
                Some(v) if key.ends_with("_min") && v < old_value => {
                    looser.push(format!("{key} {old_value} → {v}"))
                }
                _ => {}
            }
        }
        if old.status() == Status::Active && new.status() != Status::Active {
            looser.push("status left active".into());
        }
        if new.tier() > old.tier() {
            looser.push(format!(
                "tier {} → {}",
                old.tier().name(),
                new.tier().name()
            ));
        }
        if new.reference() != old.reference() {
            looser.push(format!(
                "reference {} → {}",
                old.reference().name(),
                new.reference().name()
            ));
        }
        if new.claims() != old.claims() {
            looser.push("claims changed".into());
        }
        if new.timeout_s() > old.timeout_s() || new.repeats() < old.repeats() {
            looser.push("timeout or repeats got easier".into());
        }
        if !looser.is_empty() && lock::kept_review(&old_lock, &new_lock, id, &path) {
            findings.push(format!(
                "science: S9 {id} got easier ({}) without a new lock review of {path}",
                looser.join(", ")
            ));
        }
    }
    findings
}

#[derive(Clone, Debug, PartialEq)]
enum Kind {
    Number {
        value: f64,
        digits: usize,
        float: bool,
    },
    Str(String),
    Ident(String),
    Punct,
}

#[derive(Clone, Debug)]
struct Token {
    kind: Kind,
    text: String,
    line: usize,
    cited: bool,
}

fn constant_name(trees: &[Tree], index: usize) -> Option<&str> {
    let (before, declaration) = trees.split_at(index);
    let [
        Tree::Ident(keyword, _),
        Tree::Ident(name, _),
        Tree::Punct(':', _),
        ..,
    ] = declaration
    else {
        return None;
    };
    if keyword != "const" || name == "_" {
        return None;
    }
    let mut before = match before {
        [rest @ .., Tree::Ident(visibility, _)]
        | [
            rest @ ..,
            Tree::Ident(visibility, _),
            Tree::Group(Delimiter::Parenthesis, _, _),
        ] if visibility == "pub" => rest,
        _ => before,
    };
    while let [
        rest @ ..,
        Tree::Punct('#', _),
        Tree::Group(Delimiter::Bracket, _, _),
    ]
    | [
        rest @ ..,
        Tree::Punct('#', _),
        Tree::Punct('!', _),
        Tree::Group(Delimiter::Bracket, _, _),
    ] = before
    {
        before = rest;
    }
    matches!(
        before,
        [] | [.., Tree::Punct(';', _)] | [.., Tree::Group(Delimiter::Brace, _, _)]
    )
    .then(|| name.strip_prefix("r#").unwrap_or(name))
}

fn flatten(
    trees: &[Tree],
    out: &mut Vec<Token>,
    mut sources: Option<&mut Citations>,
    enclosing_citation: bool,
) {
    let mut cited = enclosing_citation;
    for (index, tree) in trees.iter().enumerate() {
        if let Some(name) = constant_name(trees, index) {
            cited = sources
                .as_deref_mut()
                .is_some_and(|entries| entries.remove(name).is_some());
        } else if tree.is_punct(';') {
            cited = enclosing_citation;
        }
        let token = |kind, text, line| Token {
            kind,
            text,
            line,
            cited,
        };
        match tree {
            Tree::Group(delimiter, body, line) => {
                let doc = *delimiter == Delimiter::Bracket
                    && body.first().is_some_and(|t| t.is_ident("doc"));
                if doc {
                    continue;
                }
                let (open, close) = match delimiter {
                    Delimiter::Parenthesis => ("(", ")"),
                    Delimiter::Bracket => ("[", "]"),
                    Delimiter::Brace => ("{", "}"),
                    Delimiter::None => ("", ""),
                };
                if !open.is_empty() {
                    out.push(token(Kind::Punct, open.into(), *line));
                }
                flatten(body, out, sources.as_deref_mut(), cited);
                if !close.is_empty() {
                    out.push(token(Kind::Punct, close.into(), *line));
                }
            }
            Tree::Ident(text, line) => {
                let name = text.strip_prefix("r#").unwrap_or(text).to_owned();
                out.push(token(Kind::Ident(name.clone()), name, *line));
            }
            Tree::Punct(c, line) => out.push(token(Kind::Punct, c.to_string(), *line)),
            Tree::Literal(_, Some(value), line) => {
                out.push(token(Kind::Str(value.clone()), value.clone(), *line));
            }
            Tree::Literal(text, None, _) if text.starts_with('\'') || text.starts_with("b'") => {}
            Tree::Literal(text, None, line) => {
                let decimal = syntax::decimal(text).unwrap_or_else(|| text.clone());
                out.push(token(number(&decimal), text.clone(), *line));
            }
        }
    }
}

fn number(text: &str) -> Kind {
    let clean: String = text.chars().filter(|c| *c != '_').collect();
    let suffixes = [
        "f64", "f32", "u8", "u16", "u32", "u64", "u128", "usize", "i8", "i16", "i32", "i64",
        "i128", "isize",
    ];
    let suffix = suffixes.iter().find(|s| clean.ends_with(**s)).copied();
    let body = suffix.map_or(clean.as_str(), |s| &clean[..clean.len() - s.len()]);
    let float =
        body.contains('.') || body.contains(['e', 'E']) || matches!(suffix, Some("f64" | "f32"));
    let mantissa = body.split(['e', 'E']).next().unwrap_or("");
    let digits: String = mantissa.chars().filter(char::is_ascii_digit).collect();
    let significant = digits
        .trim_start_matches('0')
        .trim_end_matches('0')
        .len()
        .max(1);
    Kind::Number {
        value: body.parse::<f64>().unwrap_or(f64::NAN),
        digits: significant,
        float,
    }
}

fn fold(tokens: &[Token]) -> Option<f64> {
    let mut items: Vec<&Token> = Vec::new();
    let mut skip = 0;
    for (k, t) in tokens.iter().enumerate() {
        if k < skip {
            continue;
        }
        let next = tokens.get(k + 1).map(|n| n.text.as_str());
        if matches!((t.text.as_str(), next), ("as", Some("f64" | "f32"))) {
            skip = k + 2;
            continue;
        }
        let ok = matches!(t.kind, Kind::Number { .. })
            || ["+", "-", "*", "/", "(", ")"].contains(&t.text.as_str());
        if !ok {
            break;
        }
        items.push(t);
    }
    let operators = items
        .iter()
        .filter(|t| matches!(t.text.as_str(), "+" | "-" | "*" | "/"))
        .count();
    if operators == 0 {
        return None;
    }
    let (value, used) = expr(&items, 0)?;
    (used > 1 && value.is_finite()).then_some(value)
}

fn expr(items: &[&Token], at: usize) -> Option<(f64, usize)> {
    let (mut acc, mut k) = term(items, at)?;
    while let Some(op) = items
        .get(k)
        .map(|t| t.text.as_str())
        .filter(|o| *o == "+" || *o == "-")
    {
        let Some((rhs, next)) = term(items, k + 1) else {
            break;
        };
        acc = if op == "+" { acc + rhs } else { acc - rhs };
        k = next;
    }
    Some((acc, k))
}

fn term(items: &[&Token], at: usize) -> Option<(f64, usize)> {
    let (mut acc, mut k) = factor(items, at)?;
    while let Some(op) = items
        .get(k)
        .map(|t| t.text.as_str())
        .filter(|o| *o == "*" || *o == "/")
    {
        let Some((rhs, next)) = factor(items, k + 1) else {
            break;
        };
        acc = if op == "*" { acc * rhs } else { acc / rhs };
        k = next;
    }
    Some((acc, k))
}

fn factor(items: &[&Token], at: usize) -> Option<(f64, usize)> {
    let t = items.get(at)?;
    if let Kind::Number { value, .. } = t.kind {
        return Some((value, at + 1));
    }
    match t.text.as_str() {
        "-" => factor(items, at + 1).map(|(v, k)| (-v, k)),
        "(" => {
            let (v, k) = expr(items, at + 1)?;
            (items.get(k)?.text == ")").then_some((v, k + 1))
        }
        _ => None,
    }
}

fn reference_numbers(root: &Path) -> Vec<f64> {
    walk::files(root, "benchmarks")
        .into_iter()
        .filter(|f| f.contains("/reference/") && !f.ends_with("SOURCE.toml"))
        .filter(|f| !f.starts_with(lock::DIR))
        .flat_map(|file| numbers_in(&walk::read(root, &file)))
        .filter(|(_, _, digits, _)| *digits >= 3)
        .map(|(_, value, _, _)| value)
        .collect()
}

const FUNCTIONAL: &str = "Functional";

fn one_functional(root: &Path) -> Findings {
    let mut findings = Vec::new();
    let parsed: Vec<(String, Vec<Tree>)> = (walk::files(root, "src").into_iter())
        .filter(|file| file.ends_with(".rs"))
        .filter_map(|file| {
            let trees = syntax::parse(&walk::read(root, &file)).ok()?;
            Some((file, trees))
        })
        .collect();
    let Some((home, variants)) = parsed.iter().find_map(|(file, trees)| {
        let variants = enum_variants(trees, FUNCTIONAL);
        (!variants.is_empty()).then_some((file.as_str(), variants))
    }) else {
        return findings;
    };
    for (file, trees) in &parsed {
        if file == "src/xc.rs" || file.starts_with("src/xc/") {
            continue;
        }
        let mut named = Vec::new();
        functional_paths(trees, None, &variants, &mut named);
        for (what, line, default) in named {
            if !(default && file == home) {
                findings.push(format!(
                    "science: {file}:{line}: S10 {what}; only src/xc/ names a functional: read its row of {FUNCTIONAL}::ALL"
                ));
            }
        }
        for (a, b, line) in syntax::pairs(trees) {
            if (a, b) == (FUNCTIONAL, "as") {
                findings.push(format!(
                    "science: {file}:{line}: S10 renames {FUNCTIONAL}, which hides its variants from this rule"
                ));
            }
        }
        for item in syntax::items(trees) {
            let Some((Tree::Group(Delimiter::Brace, body, _), head)) = item.split_last() else {
                continue;
            };
            if !(head.first().is_some_and(|t| t.is_ident("impl"))
                && head.iter().any(|t| t.is_ident(FUNCTIONAL)))
            {
                continue;
            }
            for (rest, line) in syntax::after_path(body, "Self") {
                if let Some(Tree::Ident(v, _)) = rest.first()
                    && variants.contains(v)
                {
                    findings.push(format!(
                        "science: {file}:{line}: S10 names Self::{v} in an impl for {FUNCTIONAL}; only src/xc/ names a functional"
                    ));
                }
            }
        }
    }
    findings
}

fn enum_variants(trees: &[Tree], name: &str) -> Vec<String> {
    for item in syntax::items(trees) {
        let declares = item
            .windows(2)
            .any(|w| w[0].is_ident("enum") && w[1].is_ident(name));
        if let (true, Some(Tree::Group(Delimiter::Brace, body, _))) = (declares, item.last()) {
            let mut variants = Vec::new();
            for (k, tree) in body.iter().enumerate() {
                let starts = k == 0
                    || body[k - 1].is_punct(',')
                    || matches!(&body[k - 1], Tree::Group(Delimiter::Bracket, ..));
                if let (true, Tree::Ident(v, _)) = (starts, tree) {
                    variants.push(v.clone());
                }
            }
            return variants;
        }
    }
    Vec::new()
}

fn functional_paths(
    trees: &[Tree],
    call: Option<&str>,
    variants: &[String],
    out: &mut Vec<(String, usize, bool)>,
) {
    for (k, tree) in trees.iter().enumerate() {
        let path = tree.is_ident(FUNCTIONAL)
            && trees.get(k + 1).is_some_and(|t| t.is_punct(':'))
            && trees.get(k + 2).is_some_and(|t| t.is_punct(':'));
        match (path, trees.get(k + 3)) {
            (true, Some(Tree::Ident(v, line))) if variants.contains(v) => {
                let default = call == Some("unwrap_or") && k == 0 && trees.len() == 4;
                out.push((format!("names {FUNCTIONAL}::{v}"), *line, default));
            }
            (true, Some(Tree::Group(Delimiter::Brace, _, line) | Tree::Punct('*', line))) => {
                out.push((
                    format!("imports the variants of {FUNCTIONAL}"),
                    *line,
                    false,
                ));
            }
            _ => {}
        }
        if let Tree::Group(delimiter, body, _) = tree {
            let name = match (delimiter, k.checked_sub(1).map(|j| &trees[j])) {
                (Delimiter::Parenthesis, Some(Tree::Ident(name, _))) => Some(name.as_str()),
                _ => None,
            };
            functional_paths(body, name, variants, out);
        }
    }
}
