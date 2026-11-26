use crate::{has, lever, tree, write};
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
        "S5 1_a is not in benchmarks/LOCK"
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
        "S5 1_a changed since its LOCK review"
    ));
    write(&root, "xtask/src/harness.rs", "// a kinder judge\n");
    assert!(has(
        &lever(&["science"], &root),
        "S5 harness changed since its LOCK review"
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
            "S5 harness changed since its LOCK review"
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
        "S5 harness changed since its LOCK review"
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
        has(&found, "S5 1_a changed since its LOCK review"),
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
        "S5 harness changed since its LOCK review"
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
fn a_hash_line_needs_a_header() {
    let root = tree(
        "lock-orphan-line",
        &[("benchmarks/LOCK", "0000  benchmarks/1_a/case.toml\n")],
    );
    assert!(has(
        &lever(&["science"], &root),
        "hash line before any header"
    ));
}

#[test]
fn a_repeated_block_is_an_error() {
    let root = tree(
        "lock-twice",
        &[(
            "benchmarks/LOCK",
            "# harness reviewer=a reason=b\n# harness reviewer=c reason=d\n",
        )],
    );
    std::fs::create_dir_all(root.join("benchmarks")).unwrap();
    assert!(has(&lever(&["science"], &root), "lists harness twice"));
}

#[test]
fn a_hand_written_header_without_a_reviewer_is_caught() {
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
    let text = std::fs::read_to_string(root.join("benchmarks/LOCK")).unwrap();
    write(
        &root,
        "benchmarks/LOCK",
        &text.replace(" reviewer=owner", ""),
    );
    assert!(has(
        &lever(&["science"], &root),
        "S5 1_a LOCK header needs reviewer= and reason="
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
        "S5 harness changed since its LOCK review"
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
        has(&findings, "S5 1_eos is not in benchmarks/LOCK"),
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
    let text = std::fs::read_to_string(root.join("benchmarks/LOCK")).unwrap();
    assert!(text.contains("  xtask/src/harness.rs"), "{text}");
    for absent in ["benchmarks/NODE.toml", "clippy.toml", ".cargo/mutants.toml"] {
        assert!(!text.contains(absent), "{absent}: {text}");
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
        "S5 harness changed since its LOCK review"
    ));
}

#[test]
fn a_relock_keeps_the_review_line_of_each_unchanged_block() {
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
    let text = std::fs::read_to_string(root.join("benchmarks/LOCK")).unwrap();
    assert!(
        text.contains("# harness reviewer=owner reason=initial"),
        "{text}"
    );
    assert!(
        text.contains("# 1_a reviewer=owner reason=initial"),
        "{text}"
    );
    assert!(
        text.contains("# 2_b reviewer=second reason=new reference"),
        "{text}"
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
fn lock_that_is_a_fifo_or_cannot_be_read_stops_science_and_lock_and_is_never_rewritten() {
    let root = locked("lock-fifo");
    crate::fifo(&root, "benchmarks/LOCK");
    let path = root.to_path_buf();
    let f = crate::within(5, move || lever(&["science"], &path));
    assert!(has(&f, "S5 benchmarks/LOCK is no regular file"), "{f:?}");
    if crate::root_user() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let root = locked("lock-unreadable");
    let before = fs::read(root.join("benchmarks/LOCK")).unwrap();
    fs::set_permissions(
        root.join("benchmarks/LOCK"),
        fs::Permissions::from_mode(0o200),
    )
    .unwrap();
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
    fs::set_permissions(
        root.join("benchmarks/LOCK"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(has(&f, "lock: benchmarks/LOCK: "), "{f:?}");
    assert_eq!(fs::read(root.join("benchmarks/LOCK")).unwrap(), before);
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
        let before = fs::read(root.join("benchmarks/LOCK")).unwrap();
        let named = format!("1_a: benchmarks/1_a/input.toml names family {name:?}: ");
        let f = lever(&["lock", "--reviewer", "owner", "--reason", "again"], &root);
        assert!(has(&f, &format!("lock: {named}")), "{f:?}");
        assert_eq!(fs::read(root.join("benchmarks/LOCK")).unwrap(), before);
        let f = lever(&["science"], &root);
        assert!(has(&f, &format!("S5 {named}")), "{f:?}");
    }
}
