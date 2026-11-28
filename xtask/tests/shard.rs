use crate::{cargo_stub_read_by_shell, env, tree, write};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use xtask::shard::{CAP, NAMED, SHARDS, selection};
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
            "{} | test --workspace --all-features --locked --test unit -- --exact {}",
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
fn a_cargo_whose_version_query_fails_is_a_finding_even_when_it_names_a_host() {
    let root = tree("shard-version-fails", &[]);
    let log = root.join("cargo.log");
    write(
        &root,
        "cargo",
        &format!(
            "#!/bin/sh\ncase \"$1\" in\n  -vV) echo 'host: x86_64-unknown-linux-gnu'; exit 3 ;;\n  \
             *) echo \"$*\" >> '{}' ;;\nesac\n",
            log.display()
        ),
    );
    let cargo = root.join("cargo");
    fs::set_permissions(&cargo, fs::Permissions::from_mode(0o755)).unwrap();
    let env = xtask::Env {
        cargo: cargo.into(),
        ..env(true)
    };
    let findings = xtask::run(&["check", "--unit", "1"], &root, &env);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        findings[0].starts_with("cargo -vV exited with ") && findings[0].contains('3'),
        "{findings:?}"
    );
    assert!(!log.exists());
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

#[test]
fn mutants_start_after_lint_and_run_the_shards_that_their_plan_counts() {
    let ci = ci();
    let jobs = &ci["jobs"];
    let needs = |job: &str| -> Vec<String> {
        match &jobs[job]["needs"] {
            Yaml::String(one) => vec![one.clone()],
            Yaml::Array(many) => many
                .iter()
                .filter_map(|n| n.as_str().map(str::to_owned))
                .collect(),
            _ => Vec::new(),
        }
    };
    assert_eq!(needs("mutants-plan"), ["lint"]);
    assert_eq!(needs("mutants"), ["mutants-plan"]);
    assert_eq!(needs("mutants-all"), ["mutants-plan", "mutants"]);
    assert_eq!(
        jobs["mutants"]["strategy"]["matrix"]["shard"].as_str(),
        Some("${{ fromJSON(needs.mutants-plan.outputs.shards) }}")
    );
    let run = runs(&jobs["mutants"]).join("\n");
    assert!(
        run.contains(
            "--shard ${{ matrix.shard }}/${{ strategy.job-total }} --sharding round-robin"
        ),
        "{run}"
    );
    let plan = step_run(&ci, "mutants-plan", "One shard per 8 mutants");
    assert!(plan.starts_with("shards=\"[0,1,2,3,4,5,6,7]\"\n"), "{plan}");
}

fn listed(args: &[&str]) -> Vec<String> {
    let out = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--list", "--format", "terse"])
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "{args:?}: {out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    let mut names: Vec<String> = (text.lines())
        .filter_map(|l| l.strip_suffix(": test"))
        .map(str::to_owned)
        .collect();
    names.sort();
    names
}

#[test]
fn the_shards_partition_the_listing_of_a_libtest_binary_and_each_exits_zero() {
    let all = listed(&[]);
    let mut union: Vec<String> = (1..=SHARDS)
        .flat_map(|k| listed(&selection(k).unwrap().filter))
        .collect();
    union.sort();
    assert_eq!(union, all);
}

#[test]
fn only_the_last_shard_reaches_the_benchmarks_harness_which_runs_every_case_under_it() {
    use clap::Parser as _;
    let table = fs::read_to_string(repo().join("tests/benchmarks.rs")).unwrap();
    let cases: Vec<libtest_mimic::Trial> = (table.lines())
        .filter_map(|l| l.trim().strip_prefix("(\"")?.split_once('"'))
        .map(|(id, _)| libtest_mimic::Trial::test(id, || Ok(())))
        .collect();
    assert!(cases.len() > 1, "{table}");
    for k in 1..SHARDS {
        let shard = selection(k).unwrap();
        assert_eq!(shard.targets, xtask::shard::UNIT);
        let args = std::iter::once("benchmarks").chain(shard.filter.iter().copied());
        assert!(libtest_mimic::Arguments::try_parse_from(args).is_err());
    }
    let last = selection(SHARDS).unwrap();
    assert!(last.targets.is_empty());
    let args = std::iter::once("benchmarks").chain(last.filter.iter().copied());
    let args = libtest_mimic::Arguments::try_parse_from(args).unwrap();
    assert!(cases.iter().all(|c| !args.is_filtered_out(c)));
}

#[test]
fn each_cargo_cache_save_writes_the_key_and_the_paths_that_its_restore_reads() {
    let ci = ci();
    let mut saves = 0;
    for (id, job) in ci["jobs"].as_hash().unwrap() {
        let steps = job["steps"].as_vec().unwrap();
        let uses =
            |s: &Yaml, action: &str| s["uses"].as_str().is_some_and(|u| u.starts_with(action));
        let Some(restore) = (steps.iter()).find(|s| s["id"].as_str() == Some("cargo-cache")) else {
            continue;
        };
        assert!(uses(restore, "actions/cache/restore@"), "{id:?}");
        let save = (steps.iter())
            .find(|s| {
                uses(s, "actions/cache/save@") && s["with"]["path"] == restore["with"]["path"]
            })
            .unwrap_or_else(|| panic!("{id:?}: no save of the restored paths"));
        assert_eq!(
            save["with"]["key"].as_str(),
            Some("${{ steps.cargo-cache.outputs.cache-primary-key }}"),
            "{id:?}"
        );
        let prefix = restore["with"]["restore-keys"].as_str().unwrap().trim();
        assert!(
            restore["with"]["key"].as_str().unwrap().starts_with(prefix),
            "{id:?}"
        );
        saves += 1;
    }
    assert_eq!(saves, 3);
}

fn step_run<'a>(ci: &'a Yaml, job: &str, name: &str) -> &'a str {
    (ci["jobs"][job]["steps"].as_vec().unwrap().iter())
        .find(|s| s["name"].as_str().is_some_and(|n| n.starts_with(name)))
        .and_then(|s| s["run"].as_str())
        .unwrap_or_else(|| panic!("{job}: no step {name}"))
}

fn bash(script: &str, dir: &Path, vars: &[(&str, &str)]) -> std::process::ExitStatus {
    write(dir, "step.sh", script);
    let path = format!(
        "{}:{}",
        dir.join("bin").display(),
        std::env::var("PATH").unwrap()
    );
    std::process::Command::new("bash")
        .args(["--noprofile", "--norc", "-eo", "pipefail"])
        .arg(dir.join("step.sh"))
        .env("PATH", path)
        .env("RUNNER_TEMP", dir)
        .env("GITHUB_OUTPUT", dir.join("output"))
        .envs(vars.iter().copied())
        .status()
        .unwrap()
}

fn plan(name: &str, listing: &str, listing_status: i32, diff_status: i32) -> (String, String) {
    let dir = tree(name, &[]);
    for (tool, body) in [
        (
            "git",
            format!(
                "echo \"$*\" >> '{}/git.log'\nexit {diff_status}",
                dir.display()
            ),
        ),
        (
            "cargo",
            format!("printf '%s' '{listing}'\nexit {listing_status}"),
        ),
    ] {
        write(
            &dir,
            &format!("bin/{tool}"),
            &format!("#!/bin/sh\n{body}\n"),
        );
        fs::set_permissions(
            dir.join("bin").join(tool),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
    }
    let ci = ci();
    let status = bash(
        step_run(&ci, "mutants-plan", "One shard per 8 mutants"),
        &dir,
        &[("BASE", "b45e"), ("MERGE", "3e7e")],
    );
    assert!(status.success(), "{name}: {status}");
    let output = fs::read_to_string(dir.join("output")).unwrap();
    let shards = output.strip_prefix("shards=").unwrap().trim().to_owned();
    (
        shards,
        fs::read_to_string(dir.join("git.log")).unwrap_or_default(),
    )
}

fn aggregate(name: &str, plan: &str, shards: &str, mutants: &str) -> bool {
    let dir = tree(name, &[]);
    let ci = ci();
    let vars = [
        ("RESULT", mutants),
        ("PLAN", plan),
        ("SHARDS", shards),
        ("LOCAL", ""),
        ("HEAD", "3e7e"),
        ("BODY", ""),
    ];
    bash(
        step_run(&ci, "mutants-all", "Every mutants shard passed"),
        &dir,
        &vars,
    )
    .success()
}

fn array(n: usize) -> String {
    format!("[{}]", vec!["{}"; n].join(","))
}

#[test]
fn the_plan_counts_the_mutants_of_the_pull_requests_own_diff_and_sizes_the_shards_to_them() {
    let all = "[0,1,2,3,4,5,6,7]";
    for (name, listing, shards) in [
        ("plan-none", String::new(), "[]"),
        ("plan-empty", array(0), "[]"),
        ("plan-one", array(1), "[0]"),
        ("plan-eight", array(8), "[0]"),
        ("plan-nine", array(9), "[0,1]"),
        ("plan-many", array(65), all),
    ] {
        let (got, git) = plan(name, &listing, 0, 0);
        assert_eq!(got, shards, "{name}");
        assert_eq!(git, "diff b45e 3e7e\n", "{name}");
        let ran = if got == "[]" { "skipped" } else { "success" };
        assert!(
            aggregate(&format!("{name}-all"), "success", &got, ran),
            "{name}"
        );
    }
    assert!(!step_run(&ci(), "mutants", "A test fails for each mutant").contains("origin/main"));
}

#[test]
fn a_count_that_cannot_be_read_runs_every_shard_and_never_passes_with_none_run() {
    let all = "[0,1,2,3,4,5,6,7]";
    for (name, listing, listing_status, diff_status) in [
        ("plan-fails", array(0), 7, 0),
        ("plan-null", "null".to_owned(), 0, 0),
        ("plan-object", "{}".to_owned(), 0, 0),
        ("plan-two", format!("{}{}", array(1), array(2)), 0, 0),
        ("plan-null-then-empty", format!("null{}", array(0)), 0, 0),
        ("plan-object-then-empty", format!("{{}}{}", array(0)), 0, 0),
        ("plan-text", "no mutants".to_owned(), 0, 0),
        ("plan-no-diff", array(0), 0, 1),
    ] {
        let (got, _) = plan(name, &listing, listing_status, diff_status);
        assert_eq!(got, all, "{name}");
        assert!(
            !aggregate(&format!("{name}-all"), "success", &got, "skipped"),
            "{name}"
        );
    }
    for (plan, shards) in [("failure", ""), ("skipped", ""), ("cancelled", "")] {
        assert!(
            !aggregate(&format!("plan-{plan}-all"), plan, shards, "skipped"),
            "{plan}"
        );
    }
}
