use crate::{DENY_ACTION, DENY_VERSION, Findings, walk};
use std::path::Path;
use yaml_rust2::YamlLoader;

const MEMBERS: [&str; 1] = ["xtask"];

pub(crate) fn check(root: &Path) -> Findings {
    let mut findings = Vec::new();
    let workspace = match toml::from_str::<toml::Table>(&walk::read(root, "Cargo.toml")) {
        Ok(table) => table,
        Err(e) => return vec![format!("manifest: Cargo.toml: {e}")],
    };
    let members: Vec<&str> = workspace
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(toml::Value::as_array)
        .map(|a| a.iter().filter_map(toml::Value::as_str).collect())
        .unwrap_or_default();
    if members != MEMBERS {
        findings.push(format!(
            "manifest: workspace members {members:?} differ from {MEMBERS:?} in xtask/src/manifest.rs"
        ));
    }
    for manifest in manifests() {
        let text = walk::read(root, &manifest);
        findings.extend(pins(&manifest, &text));
        let script = manifest.replace("Cargo.toml", "build.rs");
        if root.join(&script).exists() {
            findings.push(format!(
                "manifest: {script} can feed generated data to the build; remove it"
            ));
        }
    }
    findings.extend(config(&walk::read(root, ".cargo/config.toml")));
    for second in [".cargo/config", ".clippy.toml"] {
        if root.join(second).exists() {
            findings.push(format!(
                "manifest: {second} is a second config that this lever and benchmarks/LOCK miss; remove it"
            ));
        }
    }
    findings.extend(versions(root, &workspace));
    findings
}

pub(crate) fn manifests() -> impl Iterator<Item = String> {
    std::iter::once("Cargo.toml".to_owned())
        .chain(MEMBERS.iter().map(|m| format!("{m}/Cargo.toml")))
}

pub(crate) fn tables(table: &toml::Table) -> Vec<(String, &toml::Table)> {
    let mut out = Vec::new();
    let kinds = ["dependencies", "dev-dependencies", "build-dependencies"];
    for kind in kinds {
        if let Some(deps) = table.get(kind).and_then(toml::Value::as_table) {
            out.push((kind.to_owned(), deps));
        }
    }
    if let Some(deps) = table
        .get("workspace")
        .and_then(|w| w.get("dependencies"))
        .and_then(toml::Value::as_table)
    {
        out.push(("workspace.dependencies".to_owned(), deps));
    }
    for (cfg, target) in table
        .get("target")
        .and_then(toml::Value::as_table)
        .into_iter()
        .flatten()
    {
        for kind in kinds {
            if let Some(deps) = target.get(kind).and_then(toml::Value::as_table) {
                out.push((format!("target.{cfg}.{kind}"), deps));
            }
        }
    }
    out
}

fn pins(manifest: &str, text: &str) -> Findings {
    let mut findings = Vec::new();
    let table = match toml::from_str::<toml::Table>(text) {
        Ok(table) => table,
        Err(e) => return vec![format!("manifest: {manifest}: {e}")],
    };
    if table.get("package").and_then(|p| p.get("build")).is_some() {
        findings.push(format!(
            "manifest: {manifest} sets package.build; build scripts are not allowed"
        ));
    }
    for key in ["bin", "example", "bench"] {
        if table.contains_key(key) {
            findings.push(format!(
                "manifest: {manifest} declares [[{key}]]; binaries live only at src/main.rs and xtask/src/main.rs"
            ));
        }
    }
    if table.get("lib").and_then(|l| l.get("path")).is_some() {
        findings.push(format!(
            "manifest: {manifest} moves [lib] path; keep src/lib.rs"
        ));
    }
    for key in ["patch", "replace"] {
        if table.contains_key(key) {
            findings.push(format!(
                "manifest: {manifest} has [{key}]; it swaps a crate behind the pins"
            ));
        }
    }
    let inherits = table
        .get("lints")
        .and_then(|l| l.get("workspace"))
        .and_then(toml::Value::as_bool);
    if table.contains_key("package") && inherits != Some(true) {
        findings.push(format!(
            "manifest: {manifest} must set [lints] workspace = true"
        ));
    }
    if let Some(package) = table.get("package") {
        for key in ["edition", "rust-version"] {
            let shared = package
                .get(key)
                .and_then(|v| v.get("workspace"))
                .and_then(toml::Value::as_bool);
            if shared != Some(true) {
                findings.push(format!(
                    "manifest: {manifest} must set package.{key}.workspace = true"
                ));
            }
        }
    }
    for (section, deps) in tables(&table) {
        for (name, spec) in deps {
            let (version, git) = match spec {
                toml::Value::String(v) => (Some(v.as_str()), false),
                toml::Value::Table(t)
                    if t.get("workspace") == Some(&toml::Value::Boolean(true)) =>
                {
                    continue;
                }
                toml::Value::Table(t) => (
                    t.get("version").and_then(toml::Value::as_str),
                    t.contains_key("git"),
                ),
                _ => (None, false),
            };
            if git {
                findings.push(format!(
                    "manifest: {manifest} [{section}] {name} uses git; use a pinned registry version"
                ));
            }
            if matches!(spec, toml::Value::Table(t) if t.contains_key("path")) {
                findings.push(format!(
                    "manifest: {manifest} [{section}] {name} is a path dependency; vendored code hides from the pins"
                ));
            }
            if !version.is_some_and(exact) {
                findings.push(format!(
                    "manifest: {manifest} [{section}] {name} must pin an exact version (`=MAJOR.MINOR.PATCH`)"
                ));
            }
        }
    }
    findings
}

fn exact(version: &str) -> bool {
    let Some(rest) = version.strip_prefix('=') else {
        return false;
    };
    let core = rest.split(['-', '+']).next().unwrap_or("");
    let parts: Vec<&str> = core.split('.').collect();
    parts.len() == 3
        && parts
            .iter()
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

fn config(text: &str) -> Findings {
    match toml::from_str::<toml::Table>(text) {
        Ok(table) => table
            .keys()
            .filter(|k| *k != "alias")
            .map(|k| format!("manifest: .cargo/config.toml has [{k}]; only [alias] is allowed"))
            .collect(),
        Err(e) => vec![format!("manifest: .cargo/config.toml: {e}")],
    }
}

fn versions(root: &Path, workspace: &toml::Table) -> Findings {
    let mut findings = Vec::new();
    let channel = toml::from_str::<toml::Table>(&walk::read(root, "rust-toolchain.toml"))
        .ok()
        .and_then(|t| {
            t.get("toolchain")
                .and_then(|c| c.get("channel"))
                .and_then(toml::Value::as_str)
                .map(str::to_owned)
        });
    let Some(channel) = channel else {
        return vec!["manifest: rust-toolchain.toml needs [toolchain] channel".into()];
    };
    let rust_version = workspace
        .get("workspace")
        .and_then(|w| w.get("package"))
        .and_then(|p| p.get("rust-version"))
        .and_then(toml::Value::as_str);
    if rust_version != Some(channel.as_str()) {
        findings.push(format!(
            "manifest: workspace.package rust-version {rust_version:?} differs from rust-toolchain.toml {channel}"
        ));
    }
    let ci = walk::read(root, ".github/workflows/ci.yml");
    findings.extend(workflow(&ci, &channel));
    findings
}

fn workflow(ci: &str, channel: &str) -> Findings {
    let mut findings = Vec::new();
    let code = |line: &str| line.split(" #").next().unwrap_or("").trim().to_owned();
    let versions: Vec<String> = ci
        .lines()
        .filter_map(|l| {
            code(l)
                .strip_prefix("rust-version:")
                .map(|v| v.trim().trim_matches('"').to_owned())
        })
        .collect();
    if versions.is_empty() || versions.iter().any(|v| v != channel) {
        findings.push(format!(
            "manifest: ci.yml rust-version values {versions:?} must all be \"{channel}\""
        ));
    }
    let mut deny_actions = 0;
    for line in ci.lines() {
        let stripped = code(line);
        let stripped = stripped.strip_prefix("- ").unwrap_or(&stripped);
        let Some(action) = stripped.strip_prefix("uses:").map(|a| a.trim().to_owned()) else {
            continue;
        };
        let sha = action
            .rsplit_once('@')
            .map(|(_, s)| s.to_owned())
            .unwrap_or_default();
        let pinned = sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_hexdigit());
        let tagged = line.split_once(" # v").is_some();
        if !(pinned && tagged) {
            findings.push(format!(
                "manifest: ci.yml `{action}` must pin a 40-hex commit with a `# v<tag>` comment"
            ));
        }
        if is_cargo_deny_action(&action) {
            deny_actions += 1;
            if action != DENY_ACTION {
                findings.push(format!(
                    "manifest: ci.yml's cargo-deny action must be {DENY_ACTION}, the pin in xtask/src/lib.rs"
                ));
            }
        }
    }
    if deny_actions != 0 && !deny_steps_name_local_version(ci, deny_actions) {
        findings.push(format!(
            "manifest: ci.yml's cargo-deny action must name cargo-deny {DENY_VERSION}, the version in xtask/src/lib.rs, in its step name"
        ));
    }
    findings
}

fn is_cargo_deny_action(action: &str) -> bool {
    action.starts_with("EmbarkStudios/cargo-deny-action@")
}

fn deny_steps_name_local_version(ci: &str, count: usize) -> bool {
    YamlLoader::load_from_str(ci).is_ok_and(|docs| {
        let steps: Vec<_> = docs
            .iter()
            .filter_map(|doc| doc["jobs"].as_hash())
            .flat_map(|jobs| jobs.values())
            .filter_map(|job| job["steps"].as_vec())
            .flatten()
            .filter(|step| step["uses"].as_str().is_some_and(is_cargo_deny_action))
            .collect();
        steps.len() == count
            && steps.iter().all(|step| {
                step["name"].as_str().is_some_and(|name| {
                    let words: Vec<_> = name.split_ascii_whitespace().collect();
                    words
                        .windows(2)
                        .any(|pair| pair == ["cargo-deny", DENY_VERSION])
                })
            })
    })
}
