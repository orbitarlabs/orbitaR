use crate::{Findings, git};
use std::path::Path;

pub(crate) const STEPS: &str = "xtask/version-steps.toml";

const CITATION: &str = "CITATION.cff";

fn version(text: &str, at: &str) -> Result<(u64, u64, u64), String> {
    let manifest: toml::Value = toml::from_str(text).map_err(|e| format!("version: {at}: {e}"))?;
    let v = manifest
        .get("package")
        .and_then(|p| p.get("version"))
        .and_then(toml::Value::as_str)
        .ok_or_else(|| format!("version: {at} has no [package] version"))?;
    let parts: Vec<u64> = v
        .split('.')
        .map(str::parse)
        .collect::<Result<_, _>>()
        .map_err(|_| format!("version: {at} version {v} is not <major>.<minor>.<patch>"))?;
    match parts.as_slice() {
        [a, b, c] => Ok((*a, *b, *c)),
        _ => Err(format!(
            "version: {at} version {v} is not <major>.<minor>.<patch>"
        )),
    }
}

pub(crate) fn check(root: &Path) -> Findings {
    let mut findings = Vec::new();
    let head = match version(&crate::walk::read(root, "Cargo.toml"), "Cargo.toml") {
        Ok(v) => v,
        Err(e) => return vec![e],
    };
    let cited = format!("{}.{}.{}", head.0, head.1, head.2);
    findings.extend(citation(root, &cited, &crate::release::license(root)));
    match git(root, &["show", "origin/main:Cargo.toml"]) {
        Err(e) => findings.push(format!(
            "version: no origin/main:Cargo.toml to step from ({e}); fetch main"
        )),
        Ok(text) => match version(&text, "origin/main:Cargo.toml") {
            Err(e) => findings.push(e),
            Ok(base) => {
                let next = (base.0, base.1, base.2 + 1);
                let dotted = |v: (u64, u64, u64)| format!("{}.{}.{}", v.0, v.1, v.2);
                let step = format!("{} -> {}", dotted(base), dotted(head));
                let steps: toml::Table = match toml::from_str(&crate::walk::read(root, STEPS)) {
                    Ok(steps) => steps,
                    Err(e) => return vec![format!("version: {STEPS}: {e}")],
                };
                let named = (steps.get(&step).and_then(toml::Value::as_str))
                    .is_some_and(|r| !r.trim().is_empty());
                if head != base && head != next && !named {
                    let how = if head < base {
                        "goes back"
                    } else {
                        "steps by more than patch + 1"
                    };
                    findings.push(format!(
                        "version: {step} {how}; only {STEPS} may name such a step, as \"{step}\" with the owner's reason"
                    ));
                }
            }
        },
    }
    let text = crate::walk::read(root, "pyproject.toml");
    let pyproject: toml::Table = match toml::from_str(&text) {
        Ok(table) => table,
        Err(e) => {
            findings.push(format!("version: pyproject.toml does not parse: {e}"));
            toml::Table::new()
        }
    };
    if let Some(project) = pyproject.get("project") {
        let dynamic = project.get("dynamic").and_then(toml::Value::as_array);
        let reads_cargo = dynamic.is_some_and(|d| d.iter().any(|v| v.as_str() == Some("version")));
        if project.get("version").is_some() || !reads_cargo {
            findings.push(
                "version: pyproject.toml sets its own version; keep dynamic = [\"version\"] so maturin reads Cargo.toml".into(),
            );
        }
    }
    findings.extend(crate::release::project_tables(root));
    findings
}

fn citation(root: &Path, version: &str, license: &str) -> Findings {
    let text = crate::walk::read(root, CITATION);
    let mut findings = Vec::new();
    for (key, want) in [
        ("version", version),
        ("license", license),
        ("repository-code", crate::release::SOURCE),
        ("url", crate::release::HOMEPAGE),
    ] {
        let values: Vec<&str> = (text.lines())
            .filter_map(|l| l.strip_prefix(key)?.strip_prefix(':'))
            .map(|v| v.trim().trim_matches(['"', '\'']))
            .collect();
        if values != [want] {
            findings.push(format!(
                "version: {CITATION} {key} {values:?} must be {want:?} (Cargo.toml, release.rs), once at the top level"
            ));
        }
    }
    findings
}
