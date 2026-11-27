use crate::{git, has, lever, tree, write};
use std::path::Path;

const LICENSE: &str = "PolyForm-Noncommercial-1.0.0";

fn cargo(version: &str) -> String {
    format!("[package]\nname = \"orbitar\"\nversion = \"{version}\"\nlicense = \"{LICENSE}\"\n")
}

const LINKED: &str =
    "repository-code: \"https://github.com/orbitarlabs/orbitaR\"\nurl: https://orbitarlabs.org\n";
const URLS: &str = "[project.urls]\nHomepage = \"https://orbitarlabs.org\"\n\
     Source = \"https://github.com/orbitarlabs/orbitaR\"\nIssues = \"https://github.com/orbitarlabs/orbitaR/issues\"\n";
const JTH: &str = "packages/orbitar-jth/pyproject.toml";
const AUTHORS: &str = "authors = [{ name = \"The orbitaR developers\" }]\n";
const JTH_URLS: &str = "[project.urls]\nHomepage = \"https://orbitarlabs.org\"\n\
     Source = \"https://github.com/orbitarlabs/orbitaR\"\n";

fn citation(version: &str, license: &str) -> String {
    format!("cff-version: 1.2.0\ntitle: orbitaR\n{LINKED}version: {version}\nlicense: {license}\n")
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
        &format!("[project]\nname = \"orbitar\"\ndynamic = [\"version\"]\n{AUTHORS}\n{URLS}"),
    );
    assert_eq!(lever(&["version"], &root), Vec::<String>::new());
}

fn linked(name: &str, pyproject: &str, jth: &str) -> Vec<String> {
    authored(name, AUTHORS, pyproject, jth)
}

fn authored(name: &str, authors: &str, pyproject: &str, jth: &str) -> Vec<String> {
    let project =
        format!("[project]\nname = \"orbitar\"\ndynamic = [\"version\"]\n{authors}\n{pyproject}");
    let jth = format!("[project]\nname = \"orbitar-jth\"\nversion = \"2.0.0\"\n{authors}\n{jth}");
    let root = tree(
        &format!("version-links-{name}"),
        &[
            ("Cargo.toml", &cargo("0.0.1")),
            ("CITATION.cff", &citation("0.0.1", LICENSE)),
            ("pyproject.toml", &project),
            (JTH, &jth),
        ],
    );
    let at: &Path = &root;
    git(at, &["init", "-q"]);
    git(at, &["add", "-A"]);
    git(at, &["commit", "-q", "-m", "base"]);
    git(at, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
    lever(&["version"], &root)
}

#[test]
fn pyproject_files_that_name_exactly_the_project_links_pass() {
    assert_eq!(linked("good", URLS, JTH_URLS), Vec::<String>::new());
}

#[test]
fn a_pyproject_with_another_url_a_missing_link_or_the_data_package_with_issues_fails() {
    let private = URLS.replace("orbitaR\"\nIssues", "orbitaR-fork\"\nIssues");
    let extra = format!("{URLS}Docs = \"https://example.invalid\"\n");
    let missing = URLS.replace("Homepage = \"https://orbitarlabs.org\"\n", "");
    for (name, urls) in [
        ("private", private.as_str()),
        ("extra", extra.as_str()),
        ("missing", missing.as_str()),
        ("none", ""),
    ] {
        let found = linked(name, urls, JTH_URLS);
        assert!(
            has(&found, "release: pyproject.toml [project.urls] is"),
            "{name}: {found:?}"
        );
    }
    let found = linked("jth-issues", URLS, URLS);
    assert!(
        has(&found, &format!("release: {JTH} [project.urls] is")),
        "{found:?}"
    );
    let found = linked("jth-bad", URLS, "[project\n");
    assert!(
        has(&found, &format!("release: {JTH} does not parse")),
        "{found:?}"
    );
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
    let quoted = format!("cff-version: 1.2.0\n{LINKED}version: \"0.0.1\"\nlicense: '{LICENSE}'\n");
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
            "private repository",
            citation("0.0.1", LICENSE).replace("orbitaR\"", "orbitaR-fork\""),
            "CITATION.cff repository-code",
        ),
        (
            "no url",
            citation("0.0.1", LICENSE).replace("url: https://orbitarlabs.org\n", ""),
            "CITATION.cff url",
        ),
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
    assert!(has(&found, "CITATION.cff repository-code"), "{found:?}");
}

#[test]
fn a_pyproject_with_another_author_an_author_address_or_a_maintainer_fails() {
    for (name, authors) in [
        (
            "author-person",
            &"authors = [{ name = \"A Person\" }]\n".to_owned(),
        ),
        (
            "author-second",
            &"authors = [{ name = \"The orbitaR developers\" }, { name = \"A Person\" }]\n"
                .to_owned(),
        ),
        (
            "author-address",
            &format!(
                "authors = [{{ name = \"The orbitaR developers\", email = \"x{}example.invalid\" }}]\n",
                '@'
            ),
        ),
        ("author-none", &String::new()),
    ] {
        let found = authored(name, authors, URLS, JTH_URLS);
        for path in ["pyproject.toml", JTH] {
            assert!(
                has(&found, &format!("release: {path} [project] authors is")),
                "{name} {path}: {found:?}"
            );
        }
    }
    let maintained = format!("{AUTHORS}maintainers = [{{ name = \"A Person\" }}]\n");
    let found = authored("author-maintainer", &maintained, URLS, JTH_URLS);
    for path in ["pyproject.toml", JTH] {
        assert!(
            has(
                &found,
                &format!("release: {path} [project] names maintainers")
            ),
            "{path}: {found:?}"
        );
    }
}
