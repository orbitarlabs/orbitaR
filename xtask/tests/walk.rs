use crate::{NATIVE_AT, NATIVE_WIRED, has, lever, tree};

#[test]
fn only_root_level_private_trees_are_skipped() {
    let root = tree(
        "walk",
        &[
            ("target/OUTCAR", ""),
            ("scratch/OUTCAR", ""),
            ("benchmarks/1_a/target/POTCAR", ""),
        ],
    );
    let findings = lever(&["science"], &root);
    assert!(has(&findings, "S6 benchmarks/1_a/target/POTCAR"));
    assert!(!has(&findings, "S6 target/OUTCAR"));
    assert!(!has(&findings, "S6 scratch/OUTCAR"));
}

#[test]
fn a_root_git_file_of_a_worktree_is_skipped() {
    let root = tree(
        "walk-worktree",
        &[
            (".git", "gitdir: /elsewhere/.git/worktrees/w\n"),
            ("stray.rs", ""),
            NATIVE_WIRED,
            (NATIVE_AT, ""),
        ],
    );
    let findings = lever(&["native"], &root);
    assert!(has(&findings, "native: stray.rs is in no"));
    assert!(!has(&findings, ".git is in no"));
}

#[test]
fn a_root_file_named_like_a_private_tree_is_scanned() {
    let incar = "ENCUT = 400\nISMEAR = 0\n";
    let root = tree(
        "walk-root-file",
        &[
            ("target", incar),
            ("scratch", incar),
            ("node_modules", incar),
            ("benchmarks/README.md", ""),
        ],
    );
    let findings = lever(&["science"], &root);
    for name in ["target", "scratch", "node_modules"] {
        assert!(
            has(&findings, &format!("S6 {name} is a VASP file")),
            "{findings:?}"
        );
    }
}

#[test]
fn symlinks_are_listed_apart_from_files() {
    let root = tree(
        "walk-link",
        &[
            (".agents/skills/x/SKILL.md", ""),
            ("benchmarks/1_a/input.toml", ""),
        ],
    );
    std::os::unix::fs::symlink("../.agents/skills", root.join(".claude-skills")).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", root.join("benchmarks/1_a/reference.json"))
        .unwrap();
    let findings = lever(&["science"], &root);
    assert!(has(
        &findings,
        "S5 benchmarks/1_a/reference.json is a symlink"
    ));
    assert!(!has(&findings, ".claude-skills"));
}
