use crate::{
    Env, Findings, cargo,
    case::{Case, Tier},
    harness, lock, node,
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::SystemTime,
};

pub(crate) const BUILD: [&str; 7] = [
    "build",
    "--release",
    "--locked",
    "--package",
    "orbitar",
    "--bin",
    "orbitar",
];

pub(crate) const SOURCE_DATE_EPOCH: &str = "1767225600";

pub(crate) fn tier(root: &Path, name: &str, case: Option<&str>, env: &Env) -> Findings {
    let Some(tier) = Tier::parse(name) else {
        let names: Vec<&str> = Tier::ALL.iter().map(|t| t.name()).collect();
        return vec![format!(
            "unknown tier {name}; use one of {}",
            names.join(", ")
        )];
    };
    if env.case_filter && tier != Tier::Node {
        return vec![harness::filter_refused(tier)];
    }
    if let Some(id) = case
        && let Err(e) = of_tier(root, id, tier)
    {
        return vec![e];
    }
    if tier == Tier::Claim
        && let Err(e) = declared_node(root, env)
    {
        return vec![e];
    }
    let target = root.join("target");
    let bin = match engine(env, root, &target) {
        Ok(bin) => bin,
        Err(findings) => return findings,
    };
    let findings = build_inputs(root, &target.join("release").join("orbitar.d"));
    if !findings.is_empty() {
        return findings;
    }
    let records = records_dir(root, tier);
    clear_records(&records);
    let mut test = vec![
        "test",
        "--locked",
        "--package",
        "xtask",
        "--test",
        "benchmarks",
    ];
    let mut libtest = Vec::new();
    if tier.policy().one_case_at_a_time {
        libtest.push("--test-threads=1");
    }
    if let Some(id) = case {
        libtest.extend(["--exact", id]);
    }
    if !libtest.is_empty() {
        test.push("--");
        test.extend(libtest);
    }
    let vars = [
        ("ORBITAR_BIN", bin.into_os_string()),
        ("ORBITAR_TIER", tier.name().into()),
        ("ORBITAR_RECORD_DIR", records.clone().into_os_string()),
    ];
    let before = lock::fingerprint(root);
    let since = SystemTime::now();
    let mut findings = cargo(env, root, &test, &vars);
    if lock::fingerprint(root) != before {
        findings.push(format!(
            "{}: the run changed benchmarks/LOCK or a locked file",
            tier.name()
        ));
    }
    findings.extend(harness::verify(root, tier, &records, since, case));
    findings
}

fn of_tier(root: &Path, id: &str, tier: Tier) -> Result<(), String> {
    let case = Case::load(root, id)?;
    match tier.includes(case.tier()) {
        true => Ok(()),
        false => Err(format!(
            "{id} runs in the {} tier, not in {}",
            case.tier().name(),
            tier.name()
        )),
    }
}

pub(crate) fn engine(env: &Env, dir: &Path, target: &Path) -> Result<PathBuf, Findings> {
    let target_arg = target.to_string_lossy();
    let build: Vec<&str> = BUILD
        .into_iter()
        .chain(["--target-dir", &target_arg])
        .collect();
    let failed = cargo(
        env,
        dir,
        &build,
        &[("SOURCE_DATE_EPOCH", SOURCE_DATE_EPOCH.into())],
    );
    if !failed.is_empty() {
        return Err(failed);
    }
    Ok(target
        .join("release")
        .join(format!("orbitar{}", std::env::consts::EXE_SUFFIX)))
}

pub(crate) fn records_dir(root: &Path, tier: Tier) -> PathBuf {
    root.join("target/benchmarks").join(tier.name())
}

fn clear_records(dir: &Path) {
    for file in fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = file.path();
        if path.extension().is_some_and(|x| x == "json") {
            let _ = fs::remove_file(path);
        }
    }
}

fn declared_node(root: &Path, env: &Env) -> Result<(), String> {
    let runner = env.runner.as_deref();
    if runner == Some("github-hosted") || (env.ci && runner != Some("self-hosted")) {
        return Err(format!(
            "claim: CI on a shared runner never gives speed (RUNNER_ENVIRONMENT is {}); only a self-hosted runner may claim",
            runner.unwrap_or("unset")
        ));
    }
    if let Some(var) = &env.rustflags {
        return Err(format!(
            "claim: {var} is set; a claim builds the engine by NODE.toml's [orbitar] alone"
        ));
    }
    let node = node::declared(root)?;
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .map_err(|e| format!("claim: git status: {e}"))?;
    if !status.status.success() {
        return Err("claim: git status failed; a claim names one commit of a git checkout".into());
    }
    if !status.stdout.is_empty() {
        return Err("claim: commit every change first; a claim names one commit".into());
    }
    let sweep = (Tier::Claim.policy().speed.into_iter())
        .flat_map(|s| s.threads.iter().map(|p| p.get()))
        .max()
        .unwrap_or(0);
    let problems: Vec<String> = [
        node::check(root, &node, env.rivals.as_deref()),
        node::fits(&node, sweep),
    ]
    .into_iter()
    .filter_map(Result::err)
    .collect();
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems.join("; "))
    }
}

pub(crate) fn records(root: &Path, name: &str) -> Findings {
    let Some(tier) = Tier::parse(name) else {
        return vec![format!("unknown tier {name}")];
    };
    let dir = records_dir(root, tier);
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    if files.is_empty() {
        return vec![format!("records: none in {}", dir.display())];
    }
    for path in files {
        println!("{}", fs::read_to_string(&path).unwrap_or_default());
    }
    Vec::new()
}

fn build_inputs(root: &Path, dep_info: &Path) -> Findings {
    let Ok(text) = fs::read_to_string(dep_info) else {
        return vec![format!(
            "build: {} is missing; the build must show that it read only src/",
            dep_info.display()
        )];
    };
    let resolved = |p: &Path| fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let src = resolved(root).join("src");
    let deps = text
        .lines()
        .next()
        .and_then(|l| l.split_once(": "))
        .map_or("", |(_, d)| d);
    let mut files = Vec::new();
    let mut current = String::new();
    let mut chars = deps.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&' ') => {
                current.push(' ');
                chars.next();
            }
            ' ' => files.extend((!current.is_empty()).then(|| std::mem::take(&mut current))),
            _ => current.push(c),
        }
    }
    files.extend((!current.is_empty()).then_some(current));
    files
        .iter()
        .filter(|f| !resolved(Path::new(f)).starts_with(&src))
        .map(|f| format!("build: the engine build read {f}; it compiles src/ only"))
        .collect()
}
