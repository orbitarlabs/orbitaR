use crate::{git, has, lever, lock_entry, lock_files, tree, write};
use sha2::{Digest, Sha256};
use std::fs;

const CASE: &str = "[case]\ntier = \"push\"\nstatus = \"planned\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n";

#[test]
fn lock_covers_every_case_file_and_the_harness() {
    let root = tree(
        "lock",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", "bands = 16\n"),
            ("tests/benchmarks/1_a/case.rs", "// measure\n"),
            ("xtask/src/harness.rs", "// judge\n"),
        ],
    );
    assert!(has(
        &lever(&["science"], &root),
        "S5 1_a is not in benchmarks/locks"
    ));
    for (reviewer, reason) in [("", ""), ("owner", "  "), (" ", "initial")] {
        assert!(has(
            &lever(&["lock", "--reviewer", reviewer, "--reason", reason], &root),
            "usage"
        ));
    }
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    assert!(!has(&lever(&["science"], &root), "S5"));
    write(&root, "benchmarks/1_a/input.toml", "bands = 1\n");
    assert!(has(
        &lever(&["science"], &root),
        "S5 1_a changed since its lock review: benchmarks/1_a/input.toml;"
    ));
    write(&root, "xtask/src/harness.rs", "// a kinder judge\n");
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review"
    ));
    for data in [
        "benchmarks/DATASETS.toml",
        "benchmarks/ACWF.toml",
        "datasets/jth-2.0.toml",
    ] {
        assert!(lever(&["lock", "--reviewer", "owner", "--reason", "judge"], &root).is_empty());
        write(&root, data, "url = \"file:///elsewhere\"\n");
        assert!(has(
            &lever(&["science"], &root),
            "S5 harness changed since its lock review"
        ));
    }
}

#[test]
fn the_harness_block_hashes_the_default_family_table_and_no_other() {
    let root = tree(
        "lock-families",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("xtask/src/harness.rs", "// judge\n"),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    write(&root, "datasets/ours-v0.toml", "base = \"jth-2.0\"\n");
    write(&root, "datasets/JTH-PBE-v2.0/Si/Si.xml", "si");
    assert!(
        !has(&lever(&["science"], &root), "S5"),
        "a new table needs no harness review"
    );
    write(&root, "datasets/jth-2.0.toml", "url = \"v\"\n");
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review"
    ));
}

#[test]
fn a_family_table_that_an_input_names_is_in_the_block_of_its_case_alone() {
    let named = "[datasets]\nfamily = \"ours-v0\"\n";
    let root = tree(
        "lock-named-family",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/inputs/si.toml", named),
            ("benchmarks/2_b/case.toml", CASE),
            ("benchmarks/2_b/input.toml", "bands = 1\n"),
            ("xtask/src/harness.rs", "// judge\n"),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    write(
        &root,
        "datasets/ours-v0.toml",
        "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n",
    );
    let found = lever(&["science"], &root);
    assert!(
        has(&found, "S5 1_a changed since its lock review"),
        "{found:?}"
    );
    assert!(
        !has(&found, "S5 2_b") && !has(&found, "S5 harness"),
        "{found:?}"
    );
}

#[test]
fn a_workflow_edit_needs_a_harness_review() {
    let root = tree(
        "lock-workflow",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("xtask/src/harness.rs", "// judge\n"),
            (".github/workflows/ci.yml", "on: workflow_dispatch\n"),
        ],
    );
    let f = lever(
        &["lock", "--reviewer", "owner", "--reason", "initial"],
        &root,
    );
    assert!(f.is_empty(), "{f:?}");
    write(
        &root,
        ".github/workflows/ci.yml",
        "on: pull_request_target\n",
    );
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review"
    ));
}

#[test]
fn a_case_never_disappears_and_new_numbers_only_grow() {
    let root = tree(
        "lock-ids",
        &[
            ("benchmarks/2_b/case.toml", CASE),
            ("tests/benchmarks/2_b/case.rs", ""),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    write(&root, "benchmarks/1_a/case.toml", CASE);
    assert!(has(
        &lever(&["lock", "--reviewer", "owner", "--reason", "late"], &root),
        "must use a number above 2"
    ));
    std::fs::remove_dir_all(root.join("benchmarks/1_a")).unwrap();
    std::fs::remove_dir_all(root.join("benchmarks/2_b")).unwrap();
    std::fs::create_dir_all(root.join("benchmarks")).unwrap();
    assert!(has(
        &lever(&["lock", "--reviewer", "owner", "--reason", "drop"], &root),
        "2_b disappeared"
    ));
    assert!(has(
        &lever(&["science"], &root),
        "locked case 2_b disappeared"
    ));
}

#[test]
fn an_old_lock_with_a_hash_line_before_any_header_is_refused() {
    let root = tree(
        "lock-orphan-line",
        &[("benchmarks/LOCK", "0000  benchmarks/1_a/case.toml\n")],
    );
    assert!(has(
        &lever(&["lock", "--reviewer", "owner", "--reason", "move"], &root),
        "lock: benchmarks/LOCK: hash line before any header"
    ));
    assert!(has(
        &lever(&["science"], &root),
        "S5 benchmarks/LOCK is the old one-file lock"
    ));
}

#[test]
fn an_old_lock_that_lists_a_block_twice_or_a_header_without_a_review_is_refused() {
    for (n, (text, why)) in [
        (
            "# harness reviewer=a reason=b\n# harness reviewer=c reason=d\n",
            "lists harness twice",
        ),
        ("# harness reason=b\n", "needs reviewer= and reason="),
        ("# harness reviewer=a\n", "needs reviewer= and reason="),
    ]
    .into_iter()
    .enumerate()
    {
        let root = tree(&format!("lock-twice-{n}"), &[("benchmarks/LOCK", text)]);
        assert!(
            has(
                &lever(&["lock", "--reviewer", "owner", "--reason", "move"], &root),
                why
            ),
            "{text}"
        );
        assert!(root.join("benchmarks/LOCK").exists());
    }
}

#[test]
fn an_entry_without_a_reviewer_a_reason_or_a_hash_is_caught() {
    let root = tree(
        "lock-header",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("tests/benchmarks/1_a/case.rs", ""),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    let at = "benchmarks/locks/1_a/benchmarks/1_a/case.toml.lock";
    let good = fs::read_to_string(root.join(at)).unwrap();
    let sha = lock_entry(&root, "1_a", "benchmarks/1_a/case.toml")["sha256"].clone();
    let sha = sha.as_str().unwrap();
    for (edit, why) in [
        (
            good.replace("reviewer = \"owner\"\n", ""),
            "missing field `reviewer`",
        ),
        (
            good.replace("reviewer = \"owner\"", "reviewer = \" \""),
            "needs a reviewer and a reason",
        ),
        (
            good.replace("reason = \"initial\"", "reason = \"\""),
            "needs a reviewer and a reason",
        ),
        (good.replace(sha, "0000"), "sha256 holds 64 hex digits"),
        (
            good.replace(sha, &sha.to_uppercase()),
            "sha256 holds 64 hex digits",
        ),
        (format!("{good}extra = 1\n"), "unknown field `extra`"),
    ] {
        write(&root, at, &edit);
        let f = lever(&["science"], &root);
        assert!(has(&f, &format!("S5 {at}")) && has(&f, why), "{why}: {f:?}");
        assert!(has(
            &lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root),
            why
        ));
    }
    write(&root, "benchmarks/locks/1_a/stray.txt", "x\n");
    write(&root, at, &good);
    assert!(has(
        &lever(&["science"], &root),
        "S5 benchmarks/locks/1_a/stray.txt is no benchmarks/locks/<block>/<file>.lock entry"
    ));
}

#[test]
fn a_cargo_lock_edit_needs_a_harness_review() {
    let root = tree(
        "lock-cargo",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("xtask/src/harness.rs", "// judge\n"),
            ("Cargo.lock", "version = 4\n"),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    write(
        &root,
        "Cargo.lock",
        "version = 4\n\n[[package]]\nname = \"kinder-judge\"\n",
    );
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review"
    ));
}

#[test]
fn a_new_case_keeps_the_harness_review() {
    let runner = "use xtask::harness::{self, Measure};\n\n// cases: begin (generated by `cargo xtask new-case`; `cargo xtask layout` checks it)\n// cases: end\n\nfn main() {\n    harness::main(CASES);\n}\n";
    let root = tree(
        "lock-new-case",
        &[
            ("tests/benchmarks.rs", runner),
            ("xtask/src/harness.rs", "// judge\n"),
            ("benchmarks/.keep", ""),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    assert!(lever(&["new-case", "eos", "analytic"], &root).is_empty());
    let findings = lever(&["science"], &root);
    assert!(
        has(&findings, "S5 1_eos is not in benchmarks/locks"),
        "{findings:?}"
    );
    assert!(
        !has(&findings, "S5 harness"),
        "the generated table is not the judge: {findings:?}"
    );
}

#[test]
fn the_harness_block_lists_only_files_that_exist() {
    let root = tree(
        "lock-optional",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("xtask/src/harness.rs", "// judge\n"),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    let files = lock_files(&root);
    assert!(
        files.contains_key("benchmarks/locks/harness/xtask/src/harness.rs.lock"),
        "{files:?}"
    );
    for absent in ["benchmarks/NODE.toml", "clippy.toml", ".cargo/mutants.toml"] {
        assert!(
            !files.keys().any(|f| f.contains(absent)),
            "{absent}: {files:?}"
        );
    }
}

#[test]
fn a_runner_edit_outside_the_case_table_needs_a_harness_review() {
    let runner = "use xtask::harness::{self, Measure};\n\n// cases: begin (generated by `cargo xtask new-case`; `cargo xtask layout` checks it)\n// cases: end\n\nfn main() {\n    harness::main(CASES);\n}\n";
    let root = tree(
        "lock-runner",
        &[("tests/benchmarks.rs", runner), ("benchmarks/.keep", "")],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    write(
        &root,
        "tests/benchmarks.rs",
        &runner.replace("harness::main(CASES);", "std::process::exit(0);"),
    );
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review"
    ));
}

#[test]
fn a_relock_keeps_the_review_of_each_unchanged_file() {
    let root = tree(
        "lock-headers",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/2_b/case.toml", CASE),
            ("xtask/src/harness.rs", "// judge\n"),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    write(
        &root,
        "benchmarks/2_b/case.toml",
        &CASE.replace("analytic", "literature"),
    );
    assert!(
        lever(
            &["lock", "--reviewer", "second", "--reason", "new reference"],
            &root
        )
        .is_empty()
    );
    let review = |block: &str, file: &str| {
        let entry = lock_entry(&root, block, file);
        (entry["reviewer"].clone(), entry["reason"].clone())
    };
    let review_of = |reviewer: &str, reason: &str| (reviewer.into(), reason.into());
    assert_eq!(
        review("harness", "xtask/src/harness.rs"),
        review_of("owner", "initial")
    );
    assert_eq!(
        review("1_a", "benchmarks/1_a/case.toml"),
        review_of("owner", "initial")
    );
    assert_eq!(
        review("2_b", "benchmarks/2_b/case.toml"),
        review_of("second", "new reference")
    );
}

fn locked(name: &str) -> crate::Tree {
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("xtask/src/harness.rs", "// judge\n"),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ],
    );
    let f = lever(
        &["lock", "--reviewer", "owner", "--reason", "initial"],
        &root,
    );
    assert!(f.is_empty(), "{f:?}");
    root
}

#[test]
fn a_lock_entry_that_is_a_fifo_or_cannot_be_read_stops_science_and_lock_and_is_never_rewritten() {
    let root = locked("lock-fifo");
    let fifo = "benchmarks/locks/1_a/benchmarks/1_a/input.toml.lock";
    crate::fifo(&root, fifo);
    let path = root.to_path_buf();
    let f = crate::within(5, move || lever(&["science"], &path));
    assert!(has(&f, &format!("S5 {fifo} is no regular file")), "{f:?}");
    if crate::root_user() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let root = locked("lock-unreadable");
    let at = root.join("benchmarks/locks/1_a/benchmarks/1_a/case.toml.lock");
    write(
        &root,
        "benchmarks/1_a/case.toml",
        &CASE.replace("analytic", "literature"),
    );
    let before = fs::read(&at).unwrap();
    fs::set_permissions(&at, fs::Permissions::from_mode(0o200)).unwrap();
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    fs::set_permissions(&at, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(
        has(
            &f,
            "lock: benchmarks/locks/1_a/benchmarks/1_a/case.toml.lock: "
        ),
        "{f:?}"
    );
    assert_eq!(fs::read(&at).unwrap(), before);
}

#[test]
fn lock_refuses_a_linked_datasets_root_and_a_malformed_default_table() {
    use std::os::unix::fs::symlink;
    let root = locked("lock-linked-datasets");
    let outside = tree(
        "lock-linked-datasets-outside",
        &[("jth-2.0.toml", "url = \"u\"\n")],
    );
    fs::remove_dir_all(root.join("datasets")).unwrap();
    symlink(&*outside, root.join("datasets")).unwrap();
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    assert!(has(&f, "lock: harness: datasets is a link"), "{f:?}");
    let root = locked("lock-malformed-default");
    write(&root, "datasets/jth-2.0.toml", "= nonsense");
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    assert!(has(&f, "lock: harness: datasets/jth-2.0.toml: "), "{f:?}");
    let f = lever(&["science"], &root);
    assert!(has(&f, "S5 harness: datasets/jth-2.0.toml: "), "{f:?}");
}

#[test]
fn lock_names_a_case_file_whose_name_it_would_rewrite_and_a_harness_file_below_a_regular_file() {
    let root = locked("lock-case-backslash");
    write(&root, "benchmarks/1_a/a\\b.toml", "x = 1\n");
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    assert!(
        has(&f, "lock: 1_a: benchmarks/1_a/a\\b.toml holds a backslash"),
        "{f:?}"
    );
    let root = locked("lock-enotdir");
    write(&root, ".cargo", "a file where a directory is\n");
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    assert!(has(&f, "lock: harness: .cargo/config.toml: "), "{f:?}");
}

#[test]
fn lock_and_s5_refuse_a_case_family_named_dot_dot_or_empty_whose_table_is_absent() {
    for name in ["..", ""] {
        let root = locked(&format!("lock-family-{}", name.len()));
        write(
            &root,
            "benchmarks/1_a/input.toml",
            &format!("[datasets]\nfamily = \"{name}\"\n"),
        );
        let before = lock_files(&root);
        let named = format!("1_a: benchmarks/1_a/input.toml names family {name:?}: ");
        let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
        assert!(has(&f, &format!("lock: {named}")), "{f:?}");
        assert_eq!(lock_files(&root), before);
        let f = lever(&["science"], &root);
        assert!(has(&f, &format!("S5 {named}")), "{f:?}");
    }
}

fn sha(text: &str) -> String {
    Sha256::digest(text)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[test]
fn the_move_from_the_old_lock_keeps_every_review_verbatim_and_deletes_the_old_file() {
    let judge = "// judge\n";
    let measure = "// measure\n";
    let harness = "harness reviewer=int2-fix, captain to review reason=PR 18 green: a run's peak; reviewer= is kept";
    let case = "1_a reviewer=captain-final-call after codex-review3-16 reason=round 2 → all AGREE";
    let old = format!(
        "# {harness}\n{}  xtask/src/harness.rs\n{}  xtask/src/gone.rs\n# {case}\n{}  benchmarks/1_a/case.toml\n{}  tests/benchmarks/1_a/case.rs\n",
        sha(judge),
        sha("// removed\n"),
        sha(CASE),
        sha("// an older measure\n"),
    );
    let root = tree(
        "lock-move",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("tests/benchmarks/1_a/case.rs", measure),
            ("xtask/src/harness.rs", judge),
            ("benchmarks/LOCK", &old),
        ],
    );
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(f.is_empty(), "{f:?}");
    assert!(!root.join("benchmarks/LOCK").exists());
    let review = |header: &str| {
        let (_, rest) = header.split_once(" reviewer=").unwrap();
        let (reviewer, reason) = rest.split_once(" reason=").unwrap();
        (reviewer.to_owned(), reason.to_owned())
    };
    let kept = [
        ("harness", "xtask/src/harness.rs", judge, harness),
        ("1_a", "benchmarks/1_a/case.toml", CASE, case),
    ];
    for (block, file, body, header) in kept {
        let entry = lock_entry(&root, block, file);
        let (reviewer, reason) = review(header);
        assert_eq!(entry["sha256"].as_str(), Some(sha(body).as_str()));
        assert_eq!(
            format!(
                "{block} reviewer={} reason={}",
                entry["reviewer"].as_str().unwrap(),
                entry["reason"].as_str().unwrap()
            ),
            header
        );
        assert_eq!(
            (entry["reviewer"].as_str(), entry["reason"].as_str()),
            (Some(&*reviewer), Some(&*reason))
        );
    }
    let rehashed = lock_entry(&root, "1_a", "tests/benchmarks/1_a/case.rs");
    assert_eq!(rehashed["sha256"].as_str(), Some(sha(measure).as_str()));
    assert_eq!(rehashed["reviewer"].as_str(), Some("PENDING"));
    let gone = lock_entry(&root, "harness", "xtask/src/gone.rs");
    assert_eq!(gone["sha256"].as_str(), Some("removed"));
    assert_eq!(gone["reason"].as_str(), Some("moved"));
    assert_eq!(lock_files(&root).len(), 4);
    let f = lever(&["science"], &root);
    assert!(!has(&f, "S5"), "{f:?}");
    let moved = lock_files(&root);
    write(&root, "benchmarks/LOCK", &old);
    assert!(has(
        &lever(&["science"], &root),
        "S5 benchmarks/LOCK is the old one-file lock"
    ));
    let tampered = old.replace("reason=round 2", "reason=round 3");
    write(&root, "benchmarks/LOCK", &tampered);
    assert!(has(
        &lever(
            &["lock", "--reviewer", "PENDING", "--reason", "again"],
            &root
        ),
        "lock: benchmarks/LOCK and benchmarks/locks both exist and 1 entries differ, the first 1_a/benchmarks/1_a/case.toml; neither is deleted"
    ));
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        tampered
    );
    assert_eq!(lock_files(&root), moved);
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    write(&root, "benchmarks/LOCK", &old);
    write(&root, "benchmarks/locks.move", &dead.to_string());
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "again"],
        &root,
    );
    assert!(
        f.is_empty(),
        "a move killed after its rename is already moved: {f:?}"
    );
    assert!(!root.join("benchmarks/LOCK").exists());
    assert!(!root.join("benchmarks/locks.move").exists());
    assert_eq!(lock_files(&root), moved);
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "again"],
        &root,
    );
    assert!(f.is_empty(), "{f:?}");
    assert_eq!(lock_files(&root), moved);
}

#[test]
fn a_removed_file_leaves_a_reviewed_tombstone_and_a_file_back_at_it_is_stale() {
    let root = locked("lock-tombstone");
    write(&root, "xtask/src/extra.rs", "// extra\n");
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "add"], &root).is_empty());
    fs::remove_file(root.join("xtask/src/extra.rs")).unwrap();
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review: xtask/src/extra.rs;"
    ));
    assert!(
        lever(
            &["lock", "--reviewer", "second", "--reason", "drop extra"],
            &root
        )
        .is_empty()
    );
    let tomb = lock_entry(&root, "harness", "xtask/src/extra.rs");
    assert_eq!(tomb["sha256"].as_str(), Some("removed"));
    assert_eq!(tomb["reviewer"].as_str(), Some("second"));
    assert!(!has(&lever(&["science"], &root), "S5"));
    assert!(lever(&["lock", "--reviewer", "third", "--reason", "noop"], &root).is_empty());
    assert_eq!(
        lock_entry(&root, "harness", "xtask/src/extra.rs")["reviewer"].as_str(),
        Some("second")
    );
    write(&root, "xtask/src/extra.rs", "// back\n");
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its lock review: xtask/src/extra.rs;"
    ));
}

fn merge(root: &std::path::Path, branch: &str) -> (bool, Vec<String>) {
    let run = |args: &[&str]| {
        std::process::Command::new("git")
            .args(["-c", "user.email=x@y", "-c", "user.name=x"])
            .args(args)
            .current_dir(root)
            .output()
            .unwrap()
    };
    let merged = run(&["merge", "-q", "--no-edit", branch]).status.success();
    let conflicted = run(&["diff", "--name-only", "--diff-filter=U"]).stdout;
    let conflicted = String::from_utf8(conflicted).unwrap();
    (merged, conflicted.lines().map(str::to_owned).collect())
}

fn branch(root: &std::path::Path, name: &str, edits: &[(&str, &str)], reviewer: &str) {
    git(root, &["checkout", "-q", "-b", name, "base"]);
    for (file, body) in edits {
        write(root, file, body);
    }
    let f = lever(&["lock", "--reviewer", reviewer, "--reason", name], root);
    assert!(f.is_empty(), "{f:?}");
    git(root, &["add", "-A"]);
    git(root, &["commit", "-qm", name]);
}

#[test]
fn two_branches_that_change_different_files_merge_cleanly_and_two_that_change_one_file_conflict() {
    let root = tree(
        "lock-merge",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", "bands = 16\n"),
            ("tests/benchmarks/1_a/case.rs", "// measure\n"),
            ("xtask/src/a.rs", "// a\n"),
            ("xtask/src/b.rs", "// b\n"),
            ("xtask/src/harness.rs", "// judge\n"),
        ],
    );
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "initial"],
            &root
        )
        .is_empty()
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    git(&root, &["branch", "base"]);
    branch(
        &root,
        "left",
        &[
            ("xtask/src/a.rs", "// a, left\n"),
            ("benchmarks/1_a/input.toml", "bands = 8\n"),
        ],
        "left-review",
    );
    branch(
        &root,
        "right",
        &[
            ("xtask/src/b.rs", "// b, right\n"),
            ("tests/benchmarks/1_a/case.rs", "// right\n"),
        ],
        "right-review",
    );
    assert_eq!(merge(&root, "left"), (true, Vec::new()));
    assert!(!has(&lever(&["science"], &root), "S5"));
    for (block, file, reviewer) in [
        ("harness", "xtask/src/a.rs", "left-review"),
        ("harness", "xtask/src/b.rs", "right-review"),
        ("1_a", "benchmarks/1_a/input.toml", "left-review"),
        ("1_a", "tests/benchmarks/1_a/case.rs", "right-review"),
        ("harness", "xtask/src/harness.rs", "owner"),
    ] {
        assert_eq!(
            lock_entry(&root, block, file)["reviewer"].as_str(),
            Some(reviewer)
        );
    }
    branch(
        &root,
        "one",
        &[("benchmarks/1_a/input.toml", "bands = 1\n")],
        "one",
    );
    branch(
        &root,
        "two",
        &[("benchmarks/1_a/input.toml", "bands = 2\n")],
        "two",
    );
    let (merged, conflicted) = merge(&root, "one");
    assert!(!merged);
    assert!(
        conflicted.contains(&"benchmarks/locks/1_a/benchmarks/1_a/input.toml.lock".to_owned()),
        "{conflicted:?}"
    );
}

#[test]
fn a_move_that_fails_partway_leaves_the_old_lock_alone_and_a_retry_finishes_it() {
    let long = format!("xtask/src/{}.rs", "l".repeat(251));
    let header = "harness reviewer=first reason=kept";
    let old = format!("# {header}\n{}  xtask/src/harness.rs\n", sha("// judge\n"));
    let root = tree(
        "lock-move-partway",
        &[
            ("benchmarks/.keep", ""),
            ("xtask/src/harness.rs", "// judge\n"),
            (&long, "// long\n"),
            ("benchmarks/LOCK", &old),
        ],
    );
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    let staged = format!(
        "lock: benchmarks/locks.{}.partial/harness/",
        std::process::id()
    );
    assert!(has(&f, &staged), "{f:?}");
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        old
    );
    assert!(!root.join("benchmarks/locks").exists());
    assert_eq!(moves_left(&root), Vec::<String>::new());
    fs::remove_file(root.join(&long)).unwrap();
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(f.is_empty(), "{f:?}");
    assert!(!root.join("benchmarks/LOCK").exists());
    let kept = lock_entry(&root, "harness", "xtask/src/harness.rs");
    assert_eq!(kept["reviewer"].as_str(), Some("first"));
    assert_eq!(kept["reason"].as_str(), Some("kept"));
    assert_eq!(lock_files(&root).len(), 1);
    assert!(!has(&lever(&["science"], &root), "S5"));
}

#[test]
fn a_hashed_path_with_a_control_character_is_refused_and_odd_names_survive_the_base() {
    let root = locked("lock-control");
    write(&root, "xtask/src/a\nb.rs", "// two lines\n");
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    assert!(has(&f, "holds a control character"), "{f:?}");
    assert!(has(
        &lever(&["science"], &root),
        "holds a control character"
    ));
    fs::remove_file(root.join("xtask/src/a\nb.rs")).unwrap();
    write(&root, "xtask/src/a b\"c'd.rs", "// odd\n");
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "odd"], &root).is_empty());
    let f = lever(&["science"], &root);
    assert!(!has(&f, "S5"), "{f:?}");
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "S9") && !has(&f, "S5"), "{f:?}");
}

fn moves_left(root: &std::path::Path) -> Vec<String> {
    let mut left: Vec<String> = fs::read_dir(root.join("benchmarks"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with("locks"))
        .collect();
    left.sort();
    left
}

fn legacy_tree(name: &str, extra: &str) -> (crate::Tree, String) {
    let old = format!(
        "# harness reviewer=first reason=kept\n{}  xtask/src/harness.rs\n{extra}",
        sha("// judge\n")
    );
    let root = tree(
        name,
        &[
            ("benchmarks/.keep", ""),
            ("xtask/src/harness.rs", "// judge\n"),
            ("benchmarks/LOCK", &old),
        ],
    );
    (root, old)
}

#[test]
fn a_move_refuses_a_lock_directory_that_holds_anything_and_deletes_nothing_in_it() {
    let (root, old) = legacy_tree("lock-move-empty-dirs", "");
    let leaf = root.join("benchmarks/locks/harness/xtask/src/harness.rs.lock");
    fs::create_dir_all(&leaf).unwrap();
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(
        has(
            &f,
            "lock: benchmarks/locks is not empty (harness); the move publishes only into an empty or absent benchmarks/locks, and benchmarks/LOCK is kept"
        ),
        "{f:?}"
    );
    assert!(leaf.is_dir());
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        old
    );
    assert_eq!(moves_left(&root), vec!["locks".to_owned()]);
    fs::remove_dir_all(root.join("benchmarks/locks/harness")).unwrap();
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(
        f.is_empty(),
        "an empty benchmarks/locks takes the move: {f:?}"
    );
    assert_eq!(
        lock_entry(&root, "harness", "xtask/src/harness.rs")["reviewer"].as_str(),
        Some("first")
    );
    assert!(!root.join("benchmarks/LOCK").exists());
    let (root, old) = legacy_tree("lock-move-stray", "");
    write(&root, "benchmarks/locks/harness/stray.txt", "x\n");
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(has(&f, "benchmarks/locks/harness/stray.txt"), "{f:?}");
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        old
    );
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/locks/harness/stray.txt")).unwrap(),
        "x\n"
    );
}

#[test]
fn a_move_runs_alone_and_a_retry_after_a_dead_move_clears_only_that_moves_stage() {
    let (root, old) = legacy_tree("lock-move-alone", "");
    write(
        &root,
        "benchmarks/locks.move",
        &std::process::id().to_string(),
    );
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(
        has(
            &f,
            &format!(
                "benchmarks/locks.move: process {} is moving",
                std::process::id()
            )
        ),
        "{f:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        old
    );
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    write(&root, "benchmarks/locks.move", &dead.to_string());
    write(
        &root,
        &format!("benchmarks/locks.{dead}.partial/harness/half.lock"),
        "half",
    );
    write(
        &root,
        "benchmarks/locks.1.partial/harness/other.lock",
        "other",
    );
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(f.is_empty(), "{f:?}");
    assert_eq!(
        moves_left(&root),
        vec!["locks".to_owned(), "locks.1.partial".to_owned()]
    );
    assert_eq!(lock_files(&root).len(), 1);
    assert!(!root.join("benchmarks/LOCK").exists());
}

#[test]
fn a_dead_moves_stage_that_cannot_be_cleared_stops_the_move_and_keeps_the_old_lock() {
    let (root, old) = legacy_tree("lock-move-stuck-stage", "");
    let mut gone = std::process::Command::new("true").spawn().unwrap();
    let dead = gone.id();
    gone.wait().unwrap();
    write(&root, "benchmarks/locks.move", &dead.to_string());
    let stage = format!("benchmarks/locks.{dead}.partial");
    write(&root, &stage, "a file where a stage directory belongs\n");
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(has(&f, "lock: benchmarks/locks.move: "), "{f:?}");
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        old
    );
    assert!(root.join(&stage).is_file());
    assert!(!root.join("benchmarks/locks").exists());
}

#[test]
fn a_locks_directory_whose_pid_is_no_number_is_no_stage_and_is_named_as_a_case() {
    let root = tree(
        "lock-reserved-pid",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/locks.12.partial/harness/half.lock", "half"),
            ("benchmarks/locks.x.partial/harness/half.lock", "half"),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(has(&f, "benchmarks/locks.x.partial"), "{f:?}");
    assert!(!has(&f, "benchmarks/locks.12.partial"), "{f:?}");
}

#[test]
fn a_new_case_takes_a_number_above_a_case_that_only_the_lock_still_names() {
    let runner = "use xtask::harness::{self, Measure};\n\n// cases: begin (generated by `cargo xtask new-case`; `cargo xtask layout` checks it)\n// cases: end\n\nfn main() {\n    harness::main(CASES);\n}\n";
    let root = tree(
        "lock-new-case-number",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("tests/benchmarks/1_a/case.rs", ""),
            ("tests/benchmarks.rs", runner),
            (
                "benchmarks/locks/3_c/benchmarks/3_c/case.toml.lock",
                "sha256 = \"removed\"\nreviewer = \"first\"\nreason = \"retired\"\n",
            ),
        ],
    );
    let f = lever(&["new-case", "d", "analytic"], &root);
    assert!(f.is_empty(), "{f:?}");
    assert!(root.join("benchmarks/4_d/case.toml").is_file());
    assert!(!root.join("benchmarks/2_d").exists());
}

#[test]
fn an_old_lock_entry_with_a_control_character_stops_the_move_with_its_name() {
    let (root, old) = legacy_tree(
        "lock-move-tab",
        &format!("{}  xtask/former\tfile.txt\n", sha("// former\n")),
    );
    let f = lever(
        &["lock", "--reviewer", "PENDING", "--reason", "moved"],
        &root,
    );
    assert!(
        has(
            &f,
            "lock: benchmarks/LOCK: \"xtask/former\\tfile.txt\" holds a control character"
        ),
        "{f:?}"
    );
    assert_eq!(
        fs::read_to_string(root.join("benchmarks/LOCK")).unwrap(),
        old
    );
    assert_eq!(moves_left(&root), Vec::<String>::new());
}
