use crate::{git, has, lever, tree, write};
use std::path::Path;

const LICENSE: &str = "PolyForm-Noncommercial-1.0.0";

fn cargo(version: &str) -> String {
    format!("[package]\nname = \"orbitar\"\nversion = \"{version}\"\nlicense = \"{LICENSE}\"\n")
}

fn citation(version: &str, license: &str) -> String {
    format!("cff-version: 1.2.0\ntitle: orbitaR\nversion: {version}\nlicense: {license}\n")
}

fn stepped(name: &str, base: &str, head: &str, steps: &str) -> Vec<String> {
    let root = tree(&format!("version-{name}"), &[("Cargo.toml", &cargo(base))]);
    let at: &Path = &root;
    git(at, &["init", "-q"]);
    git(at, &["add", "-A"]);
    git(at, &["commit", "-q", "-m", "base"]);
    git(at, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    write(at, "Cargo.toml", &cargo(head));
    write(at, "CITATION.cff", &citation(head, LICENSE));
    write(at, "xtask/version-steps.toml", steps);
    lever(&["version"], &root)
}

#[test]
fn the_same_version_or_patch_plus_one_passes() {
    assert_eq!(stepped("same", "0.0.1", "0.0.1", ""), Vec::<String>::new());
    assert_eq!(stepped("patch", "0.0.1", "0.0.2", ""), Vec::<String>::new());
}

#[test]
fn a_minor_a_major_a_double_patch_or_a_step_back_fails() {
    for head in ["0.1.0", "1.0.0", "0.0.3", "0.0.0"] {
        let found = stepped("refused", "0.0.1", head, "");
        assert!(
            has(&found, &format!("version: 0.0.1 -> {head} ")),
            "{head}: {found:?}"
        );
    }
}

#[test]
fn a_step_that_the_reviewed_list_names_with_a_reason_passes() {
    let steps = "\"0.0.1 -> 0.1.0\" = \"the owner orders the first minor step\"\n";
    assert_eq!(
        stepped("listed", "0.0.1", "0.1.0", steps),
        Vec::<String>::new()
    );
    let found = stepped("other", "0.0.1", "0.2.0", steps);
    assert!(
        has(&found, "only xtask/version-steps.toml may name"),
        "{found:?}"
    );
    let found = stepped(
        "no-reason",
        "0.0.1",
        "0.1.0",
        "\"0.0.1 -> 0.1.0\" = \" \"\n",
    );
    assert!(has(&found, "0.0.1 -> 0.1.0 steps by more"), "{found:?}");
    let found = stepped("other-base", "0.5.3", "0.1.0", steps);
    assert!(has(&found, "0.5.3 -> 0.1.0 goes back"), "{found:?}");
    let found = stepped("bad-list", "0.0.1", "0.0.2", "not toml = \n");
    assert!(has(&found, "xtask/version-steps.toml:"), "{found:?}");
}

#[test]
fn a_version_that_is_not_three_numbers_fails() {
    let found = stepped("form", "0.0.1", "0.0.2-rc1", "");
    assert!(has(&found, "is not <major>.<minor>.<patch>"), "{found:?}");
}

#[test]
fn a_pyproject_with_its_own_version_fails() {
    let root = tree(
        "version-pyproject",
        &[
            ("Cargo.toml", &cargo("0.0.1")),
            ("CITATION.cff", &citation("0.0.1", LICENSE)),
        ],
    );
    let at: &Path = &root;
    git(at, &["init", "-q"]);
    git(at, &["add", "-A"]);
    git(at, &["commit", "-q", "-m", "base"]);
    git(at, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    write(
        at,
        "pyproject.toml",
        "[project]\nname = \"orbitar\"\nversion = \"0.0.1\"\n",
    );
    let found = lever(&["version"], &root);
    assert!(
        has(&found, "pyproject.toml sets its own version"),
        "{found:?}"
    );
    for (name, project) in [
        ("both", "version = \"0.0.1\"\ndynamic = [\"version\"]\n"),
        ("neither", "description = \"x\"\n"),
        ("another dynamic key", "dynamic = [\"description\"]\n"),
    ] {
        let body = format!("[project]\nname = \"orbitar\"\n{project}");
        write(at, "pyproject.toml", &body);
        let found = lever(&["version"], &root);
        assert!(
            has(&found, "pyproject.toml sets its own version"),
            "{name}: {found:?}"
        );
    }
    write(at, "pyproject.toml", "[project\n");
    let found = lever(&["version"], &root);
    assert!(has(&found, "pyproject.toml does not parse"), "{found:?}");
    write(
        at,
        "pyproject.toml",
        "[project]\nname = \"orbitar\"\ndynamic = [\"version\"]\n",
    );
    assert_eq!(lever(&["version"], &root), Vec::<String>::new());
}

#[test]
fn no_origin_main_fails() {
    let root = tree("version-no-base", &[("Cargo.toml", &cargo("0.0.1"))]);
    git(&root, &["init", "-q"]);
    let found = lever(&["version"], &root);
    assert!(has(&found, "no origin/main:Cargo.toml"), "{found:?}");
}

#[test]
fn a_citation_that_cites_another_version_or_license_than_cargo_toml_fails() {
    let root = tree("version-citation", &[("Cargo.toml", &cargo("0.0.1"))]);
    let at: &Path = &root;
    git(at, &["init", "-q"]);
    git(at, &["add", "-A"]);
    git(at, &["commit", "-q", "-m", "base"]);
    git(at, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    let quoted = format!("cff-version: 1.2.0\nversion: \"0.0.1\"\nlicense: '{LICENSE}'\n");
    write(at, "CITATION.cff", &quoted);
    assert_eq!(lever(&["version"], &root), Vec::<String>::new());
    for (name, text, needle) in [
        (
            "version",
            citation("0.0.2", LICENSE),
            "CITATION.cff version",
        ),
        ("license", citation("0.0.1", "MIT"), "CITATION.cff license"),
        (
            "twice",
            citation("0.0.1", LICENSE) + "version: 0.0.1\n",
            "CITATION.cff version",
        ),
        (
            "nested",
            "authors:\n  version: 0.0.1\n".to_owned(),
            "CITATION.cff version",
        ),
    ] {
        write(at, "CITATION.cff", &text);
        let found = lever(&["version"], &root);
        assert!(has(&found, needle), "{name}: {found:?}");
    }
    std::fs::remove_file(at.join("CITATION.cff")).unwrap();
    let found = lever(&["version"], &root);
    assert!(has(&found, "CITATION.cff version"), "{found:?}");
    assert!(has(&found, "CITATION.cff license"), "{found:?}");
}
