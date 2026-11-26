use crate::{cargo_stub_read_by_shell, git, has, lever, tree, write};
use std::path::Path;
use xtask::Env;

const NODE: &str = "cpu = \"nowhere\"\nlogical_cpus = 1\n";

#[test]
fn an_unknown_tier_is_a_finding() {
    let root = tree("bench-tier", &[("src/main.rs", ENGINE)]);
    assert!(has(&lever(&["bench", "fast"], &root), "unknown tier fast"));
    assert!(has(
        &lever(&["records", "fast"], &root),
        "unknown tier fast"
    ));
    assert!(has(&lever(&["records", "push"], &root), "records: none in"));
}

#[test]
fn the_claim_tier_runs_in_ci_only_on_a_self_hosted_runner() {
    let root = tree("bench-runner", &[("benchmarks/NODE.toml", NODE)]);
    for (ci, runner, refused) in [
        (true, None, Some("unset")),
        (true, Some("github-hosted"), Some("github-hosted")),
        (false, Some("github-hosted"), Some("github-hosted")),
        (true, Some("self-hosted"), None),
        (false, None, None),
    ] {
        let env = Env {
            runner: runner.map(str::to_owned),
            ..crate::env(ci)
        };
        let f = xtask::run(&["bench", "claim"], &root, &env);
        match refused {
            Some(shown) => assert!(
                has(
                    &f,
                    &format!(
                        "claim: CI on a shared runner never gives speed (RUNNER_ENVIRONMENT is {shown})"
                    )
                ),
                "{ci} {runner:?}: {f:?}"
            ),
            None => assert!(
                !has(&f, "never gives speed") && has(&f, "claim: "),
                "{ci} {runner:?}: {f:?}"
            ),
        }
    }
}

#[test]
fn the_claim_tier_refuses_extra_rust_flags_and_names_them() {
    let root = tree("bench-rustflags", &[("benchmarks/NODE.toml", NODE)]);
    for var in ["RUSTFLAGS", "CARGO_ENCODED_RUSTFLAGS"] {
        let env = Env {
            rustflags: Some(var.into()),
            ..crate::env(false)
        };
        let f = xtask::run(&["bench", "claim"], &root, &env);
        assert!(has(&f, &format!("claim: {var} is set")), "{f:?}");
    }
}

#[test]
fn the_claim_tier_needs_a_declared_node() {
    let root = tree("bench-node", &[("benchmarks/LOCK", "")]);
    assert!(has(
        &lever(&["bench", "claim"], &root),
        "benchmarks/NODE.toml is missing"
    ));
}

#[test]
fn an_unreadable_node_file_is_named_not_called_missing() {
    let root = tree("bench-node-dir", &[("benchmarks/NODE.toml/x", "")]);
    let f = lever(&["bench", "claim"], &root);
    assert!(has(&f, "claim: benchmarks/NODE.toml: "), "{f:?}");
    assert!(!has(&f, "is missing"), "{f:?}");
}

#[test]
fn the_claim_tier_needs_one_clean_commit() {
    let root = tree("bench-git", &[("benchmarks/NODE.toml", NODE)]);
    assert!(has(&lever(&["bench", "claim"], &root), "git status failed"));
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "node"]);
    write(&root, "notes", "uncommitted\n");
    assert!(has(
        &lever(&["bench", "claim"], &root),
        "commit every change first"
    ));
}

#[test]
fn the_claim_tier_refuses_another_host() {
    let root = tree("bench-host", &[("benchmarks/NODE.toml", NODE)]);
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "node"]);
    assert!(has(
        &lever(&["bench", "claim"], &root),
        "is not the declared node"
    ));
}

const ACTIVE: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nerror_max = 1e-12\n";

fn fake_cargo(root: &Path, body: &str) -> Env {
    fake_build(root, "$PWD/src/main.rs", body)
}

const ENGINE: &str = "fn main() {}\n";

fn fake_build(root: &Path, inputs: &str, body: &str) -> Env {
    write(root, "src/main.rs", ENGINE);
    let build = format!(
        "if [ \"$1\" = build ]; then mkdir -p target/release && printf '%b\\n' \"$PWD/target/release/orbitar: {inputs}\" > target/release/orbitar.d; fi"
    );
    cargo_stub_read_by_shell(root, false, &format!("{build}\n{body}"))
}

#[test]
fn smoke_fails_when_the_case_runner_fails() {
    let root = tree("bench-runner", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    let env = fake_cargo(&root, "[ \"$1\" = test ] && exit 101\nexit 0");
    let f = xtask::run(&["smoke"], &root, &env);
    assert!(
        has(
            &f,
            "cargo test --locked --package xtask --test benchmarks exited with"
        ),
        "{f:?}"
    );
}

#[test]
fn smoke_judges_only_the_records_of_this_run() {
    let stale = "{\"schema\": 3, \"case\": \"1_a\", \"outcome\": \"pass\", \"started_unix_ms\": 1}";
    let root = tree(
        "bench-stale",
        &[
            ("benchmarks/1_a/case.toml", ACTIVE),
            ("target/benchmarks/push/1_a.json", stale),
            ("target/benchmarks/push/keep.txt", "mine\n"),
        ],
    );
    let f = xtask::run(&["smoke"], &root, &fake_cargo(&root, "exit 0"));
    assert!(has(&f, "push: 1_a left no readable record"), "{f:?}");
    assert!(root.join("target/benchmarks/push/keep.txt").exists());
}

#[test]
fn a_run_that_rewrites_the_lock_fails() {
    let root = tree("bench-lock", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    let body =
        "[ \"$1\" = test ] && echo '# 1_a reviewer=case reason=easier' >> benchmarks/LOCK\nexit 0";
    let f = xtask::run(&["smoke"], &root, &fake_cargo(&root, body));
    assert!(
        has(&f, "the run changed benchmarks/LOCK or a locked file"),
        "{f:?}"
    );
}

#[test]
fn the_engine_build_reads_only_src() {
    let root = tree("bench-dep-info", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    let env = fake_build(&root, "$PWD/src/main.rs $PWD/vendor/value.in", "exit 0");
    let f = xtask::run(&["smoke"], &root, &env);
    assert!(has(&f, "/vendor/value.in; it compiles src/ only"), "{f:?}");
    let silent = cargo_stub_read_by_shell(&root, false, "exit 0");
    std::fs::remove_file(root.join("target/release/orbitar.d")).unwrap();
    let f = xtask::run(&["smoke"], &root, &silent);
    assert!(has(&f, "orbitar.d is missing"), "{f:?}");
}

#[test]
fn a_root_behind_a_symlink_reads_the_same_src() {
    let base = tree(
        "bench-dep-link",
        &[("repo/benchmarks/1_a/case.toml", ACTIVE)],
    );
    write(&base, "outside.rs", ENGINE);
    std::os::unix::fs::symlink(base.join("repo"), base.join("link")).unwrap();
    let real = std::fs::canonicalize(base.join("repo")).unwrap();
    let root = base.join("link");
    let inputs = format!(
        "{0}/src/main.rs {1}/src/main.rs",
        real.display(),
        root.display()
    );
    let f = xtask::run(&["smoke"], &root, &fake_build(&root, &inputs, "exit 0"));
    assert!(!has(&f, "it compiles src/ only"), "{f:?}");
    assert!(has(&f, "push: 1_a left no readable record"), "{f:?}");
    let inputs = format!("{0}/src/main.rs {0}/../outside.rs", root.display());
    let f = xtask::run(&["smoke"], &root, &fake_build(&root, &inputs, "exit 0"));
    assert!(has(&f, "/outside.rs; it compiles src/ only"), "{f:?}");
}

#[test]
fn the_claim_tier_refuses_a_host_that_differs_from_the_declared_node_in_any_key() {
    for (n, change) in ["", "cores", "cpu", "kernel", "governor", "memory_bytes"]
        .into_iter()
        .enumerate()
    {
        let root = tree(&format!("bench-node-match-{n}"), &[("benchmarks/LOCK", "")]);
        assert!(lever(&["node"], &root).is_empty());
        if !change.is_empty() {
            let path = root.join("benchmarks/NODE.toml");
            let mut node: toml::Table = std::fs::read_to_string(&path).unwrap().parse().unwrap();
            let host = node["host"].as_table_mut().unwrap();
            let other = match &host[change] {
                toml::Value::Integer(i) => toml::Value::Integer(i + (1 << 30)),
                _ => toml::Value::String("another".into()),
            };
            host.insert(change.into(), other);
            std::fs::write(&path, toml::to_string(&node).unwrap()).unwrap();
        }
        let env = fake_cargo(&root, "exit 0");
        git(&root, &["init", "-q"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "node"]);
        let f = xtask::run(&["bench", "claim"], &root, &env);
        assert_eq!(
            has(&f, "is not the declared node"),
            !change.is_empty(),
            "{change}: {f:?}"
        );
        assert!(
            change.is_empty() || has(&f, &format!("host.{change} is ")),
            "{f:?}"
        );
    }
}

#[test]
fn a_tier_writes_its_records_under_target_benchmarks() {
    let root = tree("bench-record-dir", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    let env = fake_cargo(
        &root,
        "[ \"$1\" = test ] && echo \"$ORBITAR_RECORD_DIR\" > record-dir.txt\nexit 0",
    );
    let _ = xtask::run(&["smoke"], &root, &env);
    let dir = std::fs::read_to_string(root.join("record-dir.txt")).unwrap();
    assert!(dir.trim().ends_with("target/benchmarks/push"), "{dir}");
    assert!(dir.trim().starts_with('/'), "{dir}");
}

#[test]
fn the_node_tier_runs_on_a_ci_runner_one_case_at_a_time() {
    let root = tree("bench-node-tier", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    let env = Env {
        ci: true,
        ..fake_cargo(
            &root,
            "[ \"$1\" = test ] && echo \"$ORBITAR_TIER $*\" > test-args.txt\nexit 0",
        )
    };
    let _ = xtask::run(&["bench", "node"], &root, &env);
    let args = std::fs::read_to_string(root.join("test-args.txt")).unwrap();
    assert!(
        args.starts_with("node ") && args.trim_end().ends_with("-- --test-threads=1"),
        "{args}"
    );
}

#[test]
fn records_lists_only_json_records() {
    let root = tree(
        "bench-records-json",
        &[
            ("src/main.rs", ENGINE),
            ("target/benchmarks/push/keep.txt", "mine\n"),
        ],
    );
    assert!(has(&lever(&["records", "push"], &root), "records: none in"));
    write(&root, "target/benchmarks/push/1_a.json", "{}\n");
    assert!(lever(&["records", "push"], &root).is_empty());
}

#[test]
fn a_source_path_with_a_space_is_one_path() {
    let root = tree("bench-dep-space", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    let env = fake_build(&root, "$PWD/src/main.rs $PWD/src/odd\\ name.rs", "exit 0");
    let f = xtask::run(&["smoke"], &root, &env);
    assert!(!has(&f, "it compiles src/ only"), "{f:?}");
    let env = fake_build(&root, "$PWD/src/odd\\ name.rs $PWD/vendor/x.rs", "exit 0");
    let f = xtask::run(&["smoke"], &root, &env);
    assert!(has(&f, "/vendor/x.rs; it compiles src/ only"), "{f:?}");
    let env = fake_build(&root, "$PWD/src/main.rs /opt/pass\\\\wd.rs", "exit 0");
    let f = xtask::run(&["smoke"], &root, &env);
    assert!(
        has(&f, "read /opt/pass\\wd.rs; it compiles src/ only"),
        "{f:?}"
    );
}

#[test]
fn the_claim_tier_names_a_cpu_list_in_node_toml_that_does_not_parse() {
    for (n, (allowed, cores, key)) in [
        ("", "\"0\"", "host.cpus_allowed"),
        ("0-7", "\"0,x\"", "host.core_cpus"),
    ]
    .into_iter()
    .enumerate()
    {
        let node = format!(
            "[host]\ncpu = \"nowhere\"\ncpus_allowed = \"{allowed}\"\ncore_cpus = [{cores}]\n"
        );
        let root = tree(
            &format!("bench-node-list-{n}"),
            &[("benchmarks/NODE.toml", &node)],
        );
        git(&root, &["init", "-q"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "node"]);
        let f = lever(&["bench", "claim"], &root);
        assert!(
            has(&f, &format!("claim: NODE.toml {key} holds no cpulist")),
            "{f:?}"
        );
    }
}

#[test]
fn the_claim_tier_counts_only_the_cores_and_cpus_inside_cpus_allowed() {
    let single: Vec<String> = (0..16).map(|c| format!("\"{c}\"")).collect();
    let siblings: Vec<String> = (0..8).map(|c| format!("\"{c},{}\"", c + 8)).collect();
    for (n, (cores, allowed, counted, logical)) in [
        (&single[..8], "0-7", 8, 8),
        (&single[..8], "0-3", 4, 4),
        (&single[..], "0-8", 9, 9),
        (&siblings[..], "0-3,8-11", 4, 8),
        (&siblings[..], "0-15", 8, 16),
        (&siblings[..], "0-7", 8, 8),
    ]
    .into_iter()
    .enumerate()
    {
        let node = format!(
            "[host]\ncpu = \"nowhere\"\ncpus_allowed = \"{allowed}\"\ncore_cpus = [{}]\n",
            cores.join(", ")
        );
        let root = tree(
            &format!("bench-node-fits-{n}"),
            &[("benchmarks/NODE.toml", &node)],
        );
        git(&root, &["init", "-q"]);
        git(&root, &["add", "-A"]);
        git(&root, &["commit", "-qm", "node"]);
        let f = lever(&["bench", "claim"], &root);
        let refused = counted < 8 || logical < 8;
        let named = format!(
            "claim: the sweep runs 8 threads, but NODE.toml allows {counted} cores and {logical} logical CPUs"
        );
        assert_eq!(has(&f, &named), refused, "{allowed}: {f:?}");
        assert_eq!(has(&f, "the sweep runs"), refused, "{allowed}: {f:?}");
    }
}

#[test]
fn a_tier_but_node_refuses_a_case_filter_before_it_builds_the_engine() {
    let root = crate::tree("bench-filter", &[]);
    let env = xtask::Env {
        case_filter: true,
        ..crate::env(false)
    };
    for tier in ["push", "pr", "nightly", "claim"] {
        let findings = xtask::run(&["bench", tier], &root, &env);
        assert_eq!(
            findings,
            [format!(
                "ORBITAR_CASE_FILTER runs only in the node tier, where case 10's shards run; a {tier} run covers every input"
            )],
        );
    }
}
