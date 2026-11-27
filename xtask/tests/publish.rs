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
    let root = tree(&format!("publish-{name}"), &all);
    on_main(&root);
    lever(&["publish"], &root)
}

fn on_main(root: &Path) -> String {
    let git = |args: &[&str]| git_as(root, MAINTAINER, MAINTAINER, args);
    git(&["init", "-q"]);
    git(&["add", "-A"]);
    git(&["commit", "-qm", "base"]);
    git(&["update-ref", "refs/remotes/origin/main", "HEAD"]);
    git(&["rev-parse", "HEAD"])
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

fn commit_as(root: &Path, author: Who, committer: Who, parents: &[&str], message: &str) -> String {
    let mut args = vec!["commit-tree", "HEAD^{tree}", "-m", message];
    for parent in parents {
        args.extend(["-p", parent]);
    }
    git_as(root, author, committer, &args)
}

fn publish_at(root: &Path, head: &str) -> Vec<String> {
    git_as(root, MAINTAINER, MAINTAINER, &["update-ref", "HEAD", head]);
    lever(&["publish"], root)
}

#[test]
fn a_branch_commit_by_anyone_passes_but_the_claude_identity_a_borrowed_identity_or_a_named_message_fails()
 {
    let root = tree(
        "publish-commits",
        &[
            (".github/workflows/package.yml", WORKFLOW),
            ("xtask/src/release.rs", RELEASE),
        ],
    );
    let base = on_main(&root);
    let side = commit_as(&root, MAINTAINER, MAINTAINER, &[&base], "side");
    let email = format!("someone{AT}example.org");
    let person: Who = ("Some One", &email);
    let renamed: Who = ("Some One", MAINTAINER.1);
    let readdressed: Who = (MAINTAINER.0, &email);
    let loud = format!("NoReply{AT}Anthropic.COM");
    let shouted: Who = ("Someone Else", &loud);
    let trailer = format!("a step\n\nCo-Authored-By: Claude Opus 5.5 <noreply{AT}anthropic.com>");
    let link = format!(
        "a step\n\nClaude-Session: https://{}/code/session_0123",
        "claude.ai"
    );
    let home = format!("a step built in /{}/someone/orbitar", "home");
    let claude = "holds the Claude identity";
    let borrowed = |role: &str| format!("its {role} takes the name or the address of");
    let (author, committer) = (borrowed("author"), borrowed("committer"));
    let (author, committer) = (author.as_str(), committer.as_str());
    for (by, to, message, merge, needles) in [
        (person, person, "a step", false, vec![]),
        (person, MAINTAINER, "a step", false, vec![]),
        (MAINTAINER, person, "a step", false, vec![]),
        (person, GITHUB, "an edit on the web", false, vec![]),
        (person, GITHUB, "Merge side into base", true, vec![]),
        (
            MAINTAINER,
            MAINTAINER,
            trailer.as_str(),
            false,
            vec![claude],
        ),
        (CLAUDE, MAINTAINER, "a step", false, vec![claude]),
        (MAINTAINER, CLAUDE, "a step", false, vec![claude]),
        (shouted, person, "a step", false, vec![claude]),
        (CLAUDE, GITHUB, "Merge side into base", true, vec![claude]),
        (renamed, MAINTAINER, "a step", false, vec![author]),
        (MAINTAINER, readdressed, "a step", false, vec![committer]),
        (
            person,
            renamed,
            "Merge side into base",
            true,
            vec![committer],
        ),
        (
            MAINTAINER,
            MAINTAINER,
            &link,
            false,
            vec!["names a person, an account"],
        ),
        (
            person,
            person,
            &link,
            false,
            vec!["names a person, an account"],
        ),
        (
            person,
            person,
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
        let head = commit_as(&root, by, to, &parents, message);
        let found = publish_at(&root, &head);
        for needle in &needles {
            assert!(has(&found, needle), "{message}: {found:?}");
        }
        if needles.is_empty() {
            assert_eq!(found, Vec::<String>::new(), "{message}");
        }
        assert!(
            !has(&found, &email) && !has(&found, "Some One") && !has(&found, "Someone Else"),
            "{message}: a finding echoes who: {found:?}"
        );
    }
    let bad = commit_as(&root, renamed, MAINTAINER, &[&base], "bad");
    let good = commit_as(&root, person, person, &[&bad], "good");
    let short = git_as(
        &root,
        MAINTAINER,
        MAINTAINER,
        &["rev-parse", "--short", &bad],
    );
    assert_eq!(
        publish_at(&root, &good),
        [format!(
            "publish: commit {short}: its author takes the name or the address of orbitar-admin \
             <360476282+orbitar-admin@users.noreply.github.com> or GitHub <noreply@github.com>, not both; \
             commit under your own name and address"
        )]
    );
}

#[test]
fn without_its_base_the_lever_reports_it_and_a_named_base_checks_a_push_range() {
    let root = tree("publish-base", &[("xtask/src/release.rs", RELEASE)]);
    let base = on_main(&root);
    let pushed = commit_as(&root, CLAUDE, MAINTAINER, &[&base], "pushed");
    let later = commit_as(&root, MAINTAINER, MAINTAINER, &[&pushed], "later");
    git_as(
        &root,
        MAINTAINER,
        MAINTAINER,
        &["update-ref", "refs/remotes/origin/main", &later],
    );
    assert_eq!(publish_at(&root, &later), Vec::<String>::new());
    let range = lever(&["publish", "--base", &base], &root);
    assert_eq!(range.len(), 1, "{range:?}");
    assert!(has(&range, "holds the Claude identity"), "{range:?}");
    assert_eq!(
        lever(&["publish", "--base", &pushed], &root),
        Vec::<String>::new()
    );
    git_as(
        &root,
        MAINTAINER,
        MAINTAINER,
        &["update-ref", "-d", "refs/remotes/origin/main"],
    );
    assert_eq!(
        lever(&["publish"], &root),
        [
            "publish: `origin/main` names no commit, so no commit of the branch was read; \
          fetch it (`git fetch origin main`) or name the base with `--base <rev>`"
        ]
    );
}

#[test]
fn a_commit_object_with_malformed_metadata_or_the_claude_identity_in_any_header_fails() {
    let root = tree("publish-objects", &[("xtask/src/release.rs", RELEASE)]);
    let base = on_main(&root);
    let tree_id = git_as(&root, MAINTAINER, MAINTAINER, &["rev-parse", "HEAD^{tree}"]);
    let who = format!("orbitar-admin <{}> 1764100000 +0000", MAINTAINER.1);
    let header = |author: &str, extra: &str| {
        format!(
            "tree {tree_id}\nparent {base}\nauthor {author}\ncommitter {who}\n{extra}\na step\n"
        )
    };
    let loud = format!("NoReply{AT}Anthropic.COM");
    for (object, needle) in [
        (
            header("orbitar-admin 1764100000 +0000", "\n"),
            "its author line is no `name <address> time zone`",
        ),
        (
            header(&format!("x <{}> soon +0000", MAINTAINER.1), "\n"),
            "its author line is no",
        ),
        (
            header(&format!("x <{}> 1764100000 UTC", MAINTAINER.1), "\n"),
            "its author line is no",
        ),
        (
            header(&format!("x <{}> 1764100000 +00a0", MAINTAINER.1), "\n"),
            "its author line is no",
        ),
        (
            header(&format!("x <{}> 1764100000 +0000 x", MAINTAINER.1), "\n"),
            "its author line is no",
        ),
        (
            header(&format!("x <{}> 1764100000", MAINTAINER.1), "\n"),
            "its author line is no",
        ),
        (
            header(&format!("x <{}", MAINTAINER.1), "\n"),
            "its author line is no",
        ),
        (
            header(&who, &format!("author {who}\n\n")),
            "it holds 2 author lines, not one",
        ),
        (
            header(&who, &format!("x-note {loud}\n\n")),
            "holds the Claude identity",
        ),
        (
            header(
                &who,
                &format!("gpgsig -----BEGIN-----\n {loud}\n -----END-----\n\n"),
            ),
            "holds the Claude identity",
        ),
    ] {
        let path = root.join("object.txt");
        std::fs::write(&path, &object).unwrap();
        let id = git_as(
            &root,
            MAINTAINER,
            MAINTAINER,
            &[
                "hash-object",
                "-t",
                "commit",
                "-w",
                "--literally",
                "object.txt",
            ],
        );
        std::fs::remove_file(&path).unwrap();
        let found = publish_at(&root, &id);
        assert_eq!(found.len(), 1, "{object}: {found:?}");
        assert!(has(&found, needle), "{object}: {found:?}");
    }
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
