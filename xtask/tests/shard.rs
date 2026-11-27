use crate::{cargo_stub_read_by_shell, env, tree, write};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use xtask::shard::{CAP, NAMED, SHARDS};
use yaml_rust2::{Yaml, YamlLoader};

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

fn stub(root: &Path, version: &str) -> xtask::Env {
    let log = root.join("cargo.log");
    write(
        root,
        "cargo",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  -vV) printf '{version}' ;;\n  \
             *) echo \"$CARGO_TARGET_X86_64_UNKNOWN_LINUX_GNU_RUNNER | $*\" >> '{}' ;;\nesac\n",
            log.display()
        ),
    );
    let cargo = root.join("cargo");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    xtask::Env {
        cargo: cargo.into(),
        ..env(true)
    }
}

fn shard_log(name: &str, k: &str) -> (Vec<String>, Vec<String>) {
    let root = tree(name, &[]);
    let env = stub(&root, "release: 1.99.1\\nhost: x86_64-unknown-linux-gnu\\n");
    let findings = xtask::run(&["check", "--unit", k], &root, &env);
    let lines = fs::read_to_string(root.join("cargo.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_owned)
        .collect();
    (findings, lines)
}

#[test]
fn a_named_shard_runs_the_test_step_on_its_tests_alone_under_the_mutants_cap() {
    for (k, names) in NAMED.iter().enumerate() {
        let (findings, log) = shard_log(&format!("shard-named-{k}"), &(k + 1).to_string());
        assert!(findings.is_empty(), "{findings:?}");
        let want = format!(
            "{} | test --workspace --all-features --locked -- --exact {}",
            CAP.join(" "),
            names.join(" ")
        );
        assert_eq!(log, [want]);
    }
}

#[test]
fn the_last_shard_runs_the_test_step_on_every_test_that_no_named_shard_holds() {
    let (findings, log) = shard_log("shard-rest", &SHARDS.to_string());
    assert!(findings.is_empty(), "{findings:?}");
    let skips: Vec<String> = (NAMED.iter().flat_map(|s| s.iter()))
        .map(|t| format!("--skip {t}"))
        .collect();
    let want = format!(
        "{} | test --workspace --all-features --locked -- --exact {}",
        CAP.join(" "),
        skips.join(" ")
    );
    assert_eq!(log, [want]);
}

#[test]
fn a_shard_outside_the_count_is_a_finding_and_runs_nothing() {
    for k in [0, SHARDS + 1] {
        let (findings, log) = shard_log(&format!("shard-outside-{k}"), &k.to_string());
        assert_eq!(
            findings,
            [format!("check --unit {k}: name a shard 1 to {SHARDS}")]
        );
        assert!(log.is_empty(), "{log:?}");
    }
}

#[test]
fn a_cargo_that_names_no_host_is_a_finding() {
    let root = tree("shard-no-host", &[]);
    let env = stub(&root, "release: 1.99.1\\n");
    let findings = xtask::run(&["check", "--unit", "1"], &root, &env);
    assert_eq!(findings, ["check --unit: `cargo -vV` names no host"]);
    assert!(!root.join("cargo.log").exists());
}

#[test]
fn lint_runs_every_check_step_but_the_test_step() {
    let root = tree("shard-lint", &[]);
    let env = cargo_stub_read_by_shell(&root, true, "echo \"$@\" >> cargo.log");
    let _ = xtask::run(&["check", "--lint"], &root, &env);
    let log = fs::read_to_string(root.join("cargo.log")).unwrap();
    let steps: Vec<&str> = log.lines().filter_map(|l| l.split(' ').next()).collect();
    assert_eq!(
        steps,
        ["fmt", "clippy", "build", "package", "package"],
        "{log}"
    );
}

#[test]
fn every_named_test_is_one_test_of_the_engine_suite() {
    let mut seen = std::collections::BTreeSet::new();
    for name in NAMED.iter().flat_map(|s| s.iter()) {
        assert!(seen.insert(name), "{name} is named twice");
        let (path, test) = name.rsplit_once("::").unwrap();
        let file = repo().join(format!("tests/{}.rs", path.replace("::", "/")));
        let text = fs::read_to_string(&file).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_eq!(
            text.matches(&format!("#[test]\nfn {test}()")).count(),
            1,
            "{name}: {}",
            file.display()
        );
    }
}

fn ci() -> Yaml {
    let text = fs::read_to_string(repo().join(".github/workflows/ci.yml")).unwrap();
    YamlLoader::load_from_str(&text).unwrap().remove(0)
}

fn runs(job: &Yaml) -> Vec<&str> {
    (job["steps"].as_vec().unwrap().iter())
        .filter_map(|s| s["run"].as_str())
        .collect()
}

#[test]
fn ci_runs_lint_and_every_unit_shard_and_its_check_job_needs_them_all() {
    let ci = ci();
    let jobs = &ci["jobs"];
    assert!(runs(&jobs["lint"]).contains(&"cargo xtask check --lint"));
    let shards: Vec<i64> = (jobs["unit"]["strategy"]["matrix"]["shard"].as_vec())
        .unwrap()
        .iter()
        .filter_map(Yaml::as_i64)
        .collect();
    assert_eq!(shards, (1..=SHARDS as i64).collect::<Vec<_>>());
    assert!(runs(&jobs["unit"]).contains(&"cargo xtask check --unit ${{ matrix.shard }}"));
    let needs: Vec<&str> = (jobs["check"]["needs"].as_vec().unwrap().iter())
        .filter_map(Yaml::as_str)
        .collect();
    let parts: Vec<&str> = (jobs.as_hash().unwrap().keys())
        .filter_map(Yaml::as_str)
        .filter(|id| !["check", "nightly"].contains(id) && !id.starts_with("mutants"))
        .collect();
    assert_eq!(needs, parts);
}

#[test]
fn the_mutants_jobs_run_their_tests_under_the_cap_of_the_unit_shards() {
    assert_eq!(
        ci()["env"]["TEST_RUNNER"].as_str(),
        Some(CAP.join(" ").as_str())
    );
}
