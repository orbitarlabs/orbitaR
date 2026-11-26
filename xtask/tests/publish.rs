use crate::{has, lever, tree};
use std::{path::Path, process::Command};

const WORKFLOW: &str = "name: package\npermissions:\n  contents: read\njobs:\n  package:\n    runs-on: ubuntu-latest # never self-hosted\n    steps:\n      - uses: actions/checkout@0123 # v7\n      - run: cargo xtask wheel\n";
const RELEASE: &str = "let a = Command::new(\"docker\").args([\"build\", \".\"]);\nlet b = Command::new(\"docker\").args([\"image\", \"inspect\", t]);\n";

fn published(name: &str, files: &[(&str, &str)]) -> Vec<String> {
    let mut all = vec![
        (".github/workflows/package.yml", WORKFLOW),
        ("xtask/src/release.rs", RELEASE),
    ];
    all.extend_from_slice(files);
    lever(&["publish"], &tree(&format!("publish-{name}"), &all))
}

#[test]
fn workflows_that_build_and_read_pass() {
    let clean = published(
        "clean",
        &[(
            "container/Dockerfile",
            "FROM x\n# docker push is never run\n",
        )],
    );
    assert_eq!(clean, Vec::<String>::new());
    let commented = "name: x\n# run tools/ship.sh by hand, never from here\npermissions:\n  contents: read # the levers read the tree\nconcurrency: package-${{ github.ref }}-and-the-rest\njobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo xtask check\n";
    let found = published("commented", &[(".github/workflows/more.yml", commented)]);
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_push_an_upload_a_login_a_publish_or_a_release_fails_wherever_it_runs() {
    let job = |step: &str| format!("{WORKFLOW}      - run: {step}\n");
    for (file, body, step) in [
        (
            ".github/workflows/out.yml",
            job("docker image push registry.invalid/y"),
            "docker image push",
        ),
        (
            ".github/workflows/out.yml",
            job("podman push registry.invalid/y"),
            "podman push",
        ),
        (
            ".github/workflows/out.yml",
            job("skopeo copy a b"),
            "skopeo copy",
        ),
        (
            ".github/workflows/out.yml",
            job("docker buildx build --output type=registry ."),
            "type=registry",
        ),
        (
            ".github/workflows/out.yml",
            job("gh api repos/x/y/releases -f tag=v1"),
            "/releases",
        ),
        (
            ".github/workflows/out.yml",
            job("poetry publish"),
            "poetry publish",
        ),
        (
            ".github/workflows/out.yml",
            job("Maturin Publish"),
            "maturin publish",
        ),
        (
            "container/bench.sh",
            "twine upload dist/*\n".into(),
            "twine upload",
        ),
        (
            "container/Dockerfile",
            "RUN cargo publish\n".into(),
            "cargo publish",
        ),
        (
            "README.md",
            "```sh\ncargo xtask wheel\nuv publish target/wheel/x.whl\n```\n".into(),
            "uv publish",
        ),
    ] {
        let found = published("step", &[(file, &body)]);
        assert!(has(&found, &format!("holds `{step}`")), "{file}: {found:?}");
    }
}

#[test]
fn an_action_outside_the_list_or_a_script_call_fails() {
    let release = format!("{WORKFLOW}      - uses: ncipollo/release-action@0123\n");
    let found = published("action", &[(".github/workflows/out.yml", &release)]);
    assert!(
        has(&found, "uses `ncipollo/release-action`, which is not among"),
        "{found:?}"
    );
    let create = format!("{WORKFLOW}      - uses: \"actions/create-release@v1\"\n");
    let found = published("create", &[(".github/workflows/out.yml", &create)]);
    assert!(has(&found, "uses `actions/create-release`"), "{found:?}");
    for call in [
        "sh tools/ship.sh",
        "python3 tools/ship.py",
        "bash tools/ship.bash",
        "./ship",
    ] {
        let script = format!("{WORKFLOW}      - run: {call}\n");
        let found = published("script", &[(".github/workflows/out.yml", &script)]);
        assert!(
            has(&found, "runs a script; a workflow runs the levers"),
            "{call}: {found:?}"
        );
    }
}

#[test]
fn a_workflow_that_grants_more_than_contents_read_or_nothing_fails() {
    let body = |permissions: &str| {
        format!(
            "name: x\n{permissions}jobs:\n  a:\n    runs-on: ubuntu-latest\n    steps:\n      - run: cargo xtask check\n"
        )
    };
    for form in [
        "permissions:\n  contents: write # note\n",
        "permissions: { contents: write }\n",
        "permissions:\n  contents: \"write\"\n",
        "permissions:\n  contents: read\n  packages: write\n",
        "permissions: write-all\n",
    ] {
        let found = published("grant", &[(".github/workflows/more.yml", &body(form))]);
        assert!(
            has(&found, "a workflow holds `contents: read` only"),
            "{form}: {found:?}"
        );
    }
    let found = published("none", &[(".github/workflows/more.yml", &body(""))]);
    assert!(has(&found, "has no top-level `permissions:`"), "{found:?}");
    let inline = published(
        "inline",
        &[(
            ".github/workflows/more.yml",
            &body("permissions: { contents: 'read' }\n"),
        )],
    );
    assert_eq!(inline, Vec::<String>::new());
}

#[test]
fn a_job_runs_on_one_hosted_label_and_anything_else_fails_by_file_and_job() {
    let head = "name: x\npermissions:\n  contents: read\n";
    let job = |runs_on: &str| {
        format!(
            "{head}jobs:\n  build:\n    runs-on:{runs_on}\n    steps:\n      - run: cargo xtask check\n"
        )
    };
    for (body, what) in [
        (job(" [self-hosted, linux]"), "2 labels"),
        (job("\n      - self-hosted\n      - linux"), "2 labels"),
        (
            job(" [self-hosted]"),
            "`self-hosted`, which is outside this repository's allowlist of hosted labels",
        ),
        (
            job(" ubuntu-node"),
            "`ubuntu-node`, which is outside this repository's allowlist of hosted labels",
        ),
        (
            job(" ubuntu-22.04"),
            "`ubuntu-22.04`, which is outside this repository's allowlist of hosted labels",
        ),
        (
            job(" macos-13"),
            "`macos-13`, which is outside this repository's allowlist of hosted labels",
        ),
        (
            job(" macos-14"),
            "`macos-14`, which is outside this repository's allowlist of hosted labels",
        ),
        (job(" { group: node, labels: [linux] }"), "a runner group"),
        (job("\n      group: node"), "a runner group"),
        (
            job(" ${{ inputs.runner }}"),
            "the expression `${{ inputs.runner }}`",
        ),
        (job(" 7"), "Integer(7), which is no label"),
        (
            format!("{head}jobs:\n  build:\n    \"runs-on\": self-hosted\n    steps: []\n"),
            "`self-hosted`",
        ),
        (
            format!("{head}jobs: {{ build: {{ runs-on: [self-hosted], steps: [] }} }}\n"),
            "`self-hosted`",
        ),
        (
            format!("{head}jobs:\n  build:\n    steps: []\n"),
            "no `runs-on`",
        ),
        (
            format!("{head}jobs:\n  build:\n    uses: x/y/.github/workflows/z.yml@0123\n"),
            "a reusable workflow",
        ),
    ] {
        let found = published("runner", &[(".github/workflows/node.yml", &body)]);
        assert!(
            has(&found, &format!("node.yml job `build` runs on {what}"))
                && has(
                    &found,
                    "a job here runs on one label of HOSTED, the curated GitHub-hosted runners for public repositories; never a self-hosted runner, which a fork pull request could reach"
                ),
            "{body}: {found:?}"
        );
    }
    for (body, what) in [
        (
            format!("{head}jobs:\n  build:\n    runs-on: a\n    runs-on: b\n"),
            "is not YAML that the runner check reads",
        ),
        (format!("{head}jobs: [build]\n"), "has no `jobs:` mapping"),
        (format!("{head}---\n{head}"), "holds 2 YAML documents"),
    ] {
        let found = published("unread", &[(".github/workflows/node.yml", &body)]);
        assert!(
            has(&found, &format!("node.yml {what}")),
            "{body}: {found:?}"
        );
    }
    for runs_on in [
        " ubuntu-24.04",
        " [ubuntu-latest]",
        " \"windows-2025\"",
        " macos-15 # hosted",
    ] {
        let hosted = published("hosted", &[(".github/workflows/node.yml", &job(runs_on))]);
        assert_eq!(hosted, Vec::<String>::new(), "{runs_on}");
    }
}

#[test]
fn release_rs_runs_no_docker_subcommand_but_build_run_history_and_image_inspect() {
    for sub in ["push", "login", "image\", \"push", "buildx"] {
        let code = format!("{RELEASE}let c = Command::new(\"docker\").args([\"{sub}\", \"x\"]);\n");
        let found = published("docker", &[("xtask/src/release.rs", &code)]);
        assert!(has(&found, "release.rs runs only"), "{sub}: {found:?}");
    }
}

const AT: char = '@';

#[test]
fn a_file_that_names_someone_or_holds_a_home_or_an_owners_file_fails() {
    let named = "names a person, an account";
    let home = "holds a home directory; write the path from the repository root";
    for (line, word, needle) in [
        (
            format!("see someone{AT}example.org"),
            format!("someone{AT}example.org"),
            named,
        ),
        (
            format!("see https://github.com/{}/orbitar", "someone"),
            format!("https://github.com/{}/orbitar", "someone"),
            named,
        ),
        (
            format!("see https://{}/code/session_0123", "claude.ai"),
            format!("https://{}/code/session_0123", "claude.ai"),
            named,
        ),
        (
            format!("see /{}/someone/orbitar/README.md", "home"),
            format!("/{}/someone/orbitar/README.md", "home"),
            home,
        ),
        (
            format!("HOME=/{}/someone cargo build", "home"),
            format!("/{}/someone", "home"),
            home,
        ),
        (
            format!("cd /{}/someone;", "Users"),
            format!("/{}/someone", "Users"),
            home,
        ),
    ] {
        let found = published("named", &[("docs/notes.md", &format!("{line}\n"))]);
        assert!(
            has(
                &found,
                &format!("publish: docs/notes.md: `{word}` {needle}")
            ),
            "{line}: {found:?}"
        );
    }
    let found = published("owners", &[(".github/CODEOWNERS", "* @someone\n")]);
    assert!(
        has(
            &found,
            "publish: .github/CODEOWNERS names the accounts that own the code"
        ),
        "{found:?}"
    );
}

#[test]
fn a_file_with_the_maintainer_noreply_a_source_plain_urls_or_no_user_passes() {
    let text = format!(
        "by orbitar-admin <360476282+orbitar-admin@users.noreply.github.com>\nmerged by NoReply@GitHub.com\n\
         source = \"registry+https://github.com/rust-lang/crates.io-index\"\n\
         locator = \"https://github.com/molmod/DeltaCodesDFT/tree/f9b6693763dea92d43ee48f172c02bc506876f06\"\n\
         see https://docs.rs/ndarray and /work/case/input.toml\n\
         HOME=/{0}/ and /{1}//x and /{0} and --home-dir /{0}/orbitar\n",
        "home", "Users"
    );
    let found = published("noreply", &[("docs/notes.md", &text)]);
    assert_eq!(found, Vec::<String>::new());
}

type Who<'a> = (&'a str, &'a str);
const MAINTAINER: Who = (
    "orbitar-admin",
    "360476282+orbitar-admin@users.noreply.github.com",
);
const CLAUDE: Who = ("Claude", concat!("noreply", "@", "anthropic.com"));
const GITHUB: Who = ("GitHub", "noreply@github.com");

fn git_as(root: &Path, author: Who, committer: Who, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(args)
        .env("GIT_AUTHOR_NAME", author.0)
        .env("GIT_AUTHOR_EMAIL", author.1)
        .env("GIT_COMMITTER_NAME", committer.0)
        .env("GIT_COMMITTER_EMAIL", committer.1)
        .current_dir(root)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}");
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

#[test]
fn a_branch_commit_by_a_person_or_whose_message_names_one_fails_and_a_github_merge_is_skipped() {
    let root = tree(
        "publish-commits",
        &[
            (".github/workflows/package.yml", WORKFLOW),
            ("xtask/src/release.rs", RELEASE),
        ],
    );
    let git = |author: Who, committer: Who, args: &[&str]| git_as(&root, author, committer, args);
    let commit = |author: Who, committer: Who, parents: &[&str], message: &str| {
        let mut args = vec!["commit-tree", "HEAD^{tree}", "-m", message];
        for parent in parents {
            args.extend(["-p", parent]);
        }
        git(author, committer, &args)
    };
    git(MAINTAINER, MAINTAINER, &["init", "-q"]);
    git(MAINTAINER, MAINTAINER, &["add", "-A"]);
    git(MAINTAINER, MAINTAINER, &["commit", "-qm", "base"]);
    let main = ["update-ref", "refs/remotes/origin/main", "HEAD"];
    git(MAINTAINER, MAINTAINER, &main);
    let base = git(MAINTAINER, MAINTAINER, &["rev-parse", "HEAD"]);
    let side = commit(MAINTAINER, MAINTAINER, &[&base], "side");
    let email = format!("someone{AT}example.org");
    let person: Who = ("Some One", &email);
    let renamed: Who = ("Some One", MAINTAINER.1);
    let trailer = format!("a step\n\nCo-Authored-By: Claude Opus 5.5 <noreply{AT}anthropic.com>");
    let link = format!(
        "a step\n\nClaude-Session: https://{}/code/session_0123",
        "claude.ai"
    );
    let home = format!("a step built in /{}/someone/orbitar", "home");
    let (author, committer) = ("its author is neither", "its committer is neither");
    for (by, to, message, merge, needles) in [
        (
            MAINTAINER,
            MAINTAINER,
            trailer.as_str(),
            false,
            vec!["anthropic.com` names a person"],
        ),
        (CLAUDE, MAINTAINER, "a step", false, vec![author]),
        (MAINTAINER, CLAUDE, "a step", false, vec![committer]),
        (person, MAINTAINER, "a step", false, vec![author]),
        (MAINTAINER, person, "a step", false, vec![committer]),
        (renamed, MAINTAINER, "a step", false, vec![author]),
        (MAINTAINER, renamed, "a step", false, vec![committer]),
        (person, GITHUB, "Merge side into base", true, vec![]),
        (
            CLAUDE,
            GITHUB,
            "Merge side into base",
            true,
            vec!["holds the Claude identity"],
        ),
        (
            person,
            renamed,
            "Merge side into base",
            true,
            vec![author, committer],
        ),
        (person, GITHUB, "an edit on the web", false, vec![author]),
        (
            MAINTAINER,
            MAINTAINER,
            &link,
            false,
            vec!["names a person, an account"],
        ),
        (
            MAINTAINER,
            MAINTAINER,
            &home,
            false,
            vec!["holds a home directory; reword the commit"],
        ),
    ] {
        let parents: Vec<&str> = if merge {
            vec![&base, &side]
        } else {
            vec![&base]
        };
        let head = commit(by, to, &parents, message);
        git(MAINTAINER, MAINTAINER, &["update-ref", "HEAD", &head]);
        let found = lever(&["publish"], &root);
        for needle in &needles {
            assert!(has(&found, needle), "{message}: {found:?}");
        }
        if needles.is_empty() {
            assert_eq!(found, Vec::<String>::new(), "{message}");
        }
        assert!(
            !has(&found, &email) && !has(&found, "Some One"),
            "{message}: a finding echoes who: {found:?}"
        );
    }
    let bad = commit(person, MAINTAINER, &[&base], "bad");
    let good = commit(MAINTAINER, MAINTAINER, &[&bad], "good");
    git(MAINTAINER, MAINTAINER, &["update-ref", "HEAD", &good]);
    let short = |sha: &str| git(MAINTAINER, MAINTAINER, &["rev-parse", "--short", sha]);
    assert_eq!(
        lever(&["publish"], &root),
        [format!(
            "publish: commit {}: its author is neither orbitar-admin <360476282+orbitar-admin@users.noreply.github.com> \
             nor GitHub <noreply@github.com>; commit as orbitar-admin <360476282+orbitar-admin@users.noreply.github.com>",
            short(&bad)
        )]
    );
    git(
        MAINTAINER,
        MAINTAINER,
        &["update-ref", "-d", "refs/remotes/origin/main"],
    );
    assert_eq!(lever(&["publish"], &root), Vec::<String>::new());
}

#[test]
fn a_sentence_that_holds_the_claude_identity_is_refused_whatever_its_case_or_punctuation() {
    let address = format!("NoReply{AT}Anthropic.com");
    let root = tree(
        "publish-claude-prose",
        &[("docs/notes.md", &format!("written by {address}.\n"))],
    );
    let found = lever(&["publish"], &root);
    assert!(
        has(&found, "docs/notes.md holds the Claude identity"),
        "{found:?}"
    );
}
