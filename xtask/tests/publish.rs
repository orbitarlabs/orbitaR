use crate::{Tree, has, lever, tree};
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

fn branch(name: &str) -> (Tree, String, String) {
    let root = tree(
        name,
        &[
            (".github/workflows/package.yml", WORKFLOW),
            ("xtask/src/release.rs", RELEASE),
        ],
    );
    let base = on_main(&root);
    let side = commit_as(&root, MAINTAINER, MAINTAINER, &[&base], "side");
    (root, base, side)
}

fn each_head(name: &str, cases: &[(Who, Who, &str, bool, &[&str])]) {
    let (root, base, side) = branch(name);
    for &(by, to, message, merge, needles) in cases {
        let parents: Vec<&str> = if merge {
            vec![&base, &side]
        } else {
            vec![&base]
        };
        let head = commit_as(&root, by, to, &parents, message);
        let found = publish_at(&root, &head);
        for needle in needles {
            assert!(has(&found, needle), "{by:?} {to:?} {message}: {found:?}");
        }
        if needles.is_empty() {
            assert_eq!(found, Vec::<String>::new(), "{by:?} {to:?} {message}");
        }
        for who in [PERSON.0, PERSON.1, "Someone Else"] {
            assert!(
                !has(&found, who),
                "{message}: a finding echoes who: {found:?}"
            );
        }
    }
}

const PERSON: Who = ("Some One", concat!("someone", "@", "example.org"));
const MERGE: &str = "Merge pull request #7 from orbitarlabs/topic\n\nxtask: a step";

#[test]
fn an_outside_contributor_passes_as_author_or_committer_and_through_github() {
    each_head(
        "publish-outside",
        &[
            (PERSON, PERSON, "a step", false, &[]),
            (PERSON, MAINTAINER, "a step", false, &[]),
            (MAINTAINER, PERSON, "a step", false, &[]),
            (PERSON, GITHUB, "an edit on the web", false, &[]),
            (PERSON, GITHUB, MERGE, true, &[]),
            (PERSON, PERSON, "Merge branch 'topic'", true, &[]),
        ],
    );
}

#[test]
fn the_claude_address_anywhere_or_the_claude_name_in_any_case_fails() {
    let loud = format!("NoReply{AT}Anthropic.COM");
    let shouted: Who = ("Someone Else", &loud);
    let trailer = format!("a step\n\nCo-Authored-By: Claude Opus 5.5 <noreply{AT}anthropic.com>");
    let (address, author, committer) = (
        "holds the Claude address",
        "its author is named Claude",
        "its committer is named Claude",
    );
    each_head(
        "publish-claude",
        &[
            (CLAUDE, MAINTAINER, "a step", false, &[address, author]),
            (MAINTAINER, CLAUDE, "a step", false, &[address, committer]),
            (shouted, PERSON, "a step", false, &[address]),
            (("cLAUDE", PERSON.1), PERSON, "a step", false, &[author]),
            (
                PERSON,
                ("claude code", PERSON.1),
                "a step",
                false,
                &[committer],
            ),
            (
                PERSON,
                ("Claude[bot]", PERSON.1),
                "a step",
                false,
                &[committer],
            ),
            (MAINTAINER, MAINTAINER, &trailer, false, &[address]),
            (CLAUDE, GITHUB, MERGE, true, &[address, author]),
        ],
    );
}

#[test]
fn half_of_the_maintainer_or_the_github_identity_fails() {
    let borrowed = |role: &str| format!("its {role} takes the name or the address of");
    let (author, committer) = (borrowed("author"), borrowed("committer"));
    let (author, committer) = (author.as_str(), committer.as_str());
    each_head(
        "publish-borrowed",
        &[
            (
                (PERSON.0, MAINTAINER.1),
                MAINTAINER,
                "a step",
                false,
                &[author],
            ),
            (
                MAINTAINER,
                (MAINTAINER.0, PERSON.1),
                "a step",
                false,
                &[committer],
            ),
            (("github", PERSON.1), PERSON, "a step", false, &[author]),
            (
                PERSON,
                (PERSON.0, "NoReply@GitHub.com"),
                MERGE,
                true,
                &[committer],
            ),
        ],
    );
}

#[test]
fn a_message_that_names_someone_or_holds_a_home_fails_in_merges_too() {
    let link = format!(
        "a step\n\nClaude-Session: https://{}/code/session_0123",
        "claude.ai"
    );
    let home = format!("a step built in /{}/someone/orbitar", "home");
    let (named, homed) = (
        "names a person, an account",
        "holds a home directory; reword the commit",
    );
    each_head(
        "publish-messages",
        &[
            (MAINTAINER, MAINTAINER, &link, false, &[named]),
            (PERSON, PERSON, &link, false, &[named]),
            (PERSON, PERSON, &home, false, &[homed]),
            (PERSON, GITHUB, &home, false, &[homed]),
            (PERSON, GITHUB, &home, true, &[homed]),
            (PERSON, PERSON, &home, true, &[homed]),
        ],
    );
}

#[test]
fn a_bad_commit_under_a_good_head_is_named_by_its_short_hash() {
    let (root, base, _) = branch("publish-traversal");
    let bad = commit_as(&root, (PERSON.0, MAINTAINER.1), MAINTAINER, &[&base], "bad");
    let good = commit_as(&root, PERSON, PERSON, &[&bad], "good");
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
fn a_named_range_reads_only_its_commits_and_a_missing_end_is_reported() {
    let root = tree("publish-range", &[("xtask/src/release.rs", RELEASE)]);
    let base = on_main(&root);
    let pushed = commit_as(&root, CLAUDE, MAINTAINER, &[&base], "pushed");
    let later = commit_as(&root, MAINTAINER, MAINTAINER, &[&pushed], "later");
    let git = |args: &[&str]| git_as(&root, MAINTAINER, MAINTAINER, args);
    git(&["update-ref", "refs/remotes/origin/main", &later]);
    assert_eq!(publish_at(&root, &later), Vec::<String>::new());
    let read = |base: &str, head: &str| lever(&["publish", "--base", base, "--head", head], &root);
    let claude = |found: &[String]| found.len() == 2 && has(found, "holds the Claude address");
    assert!(claude(&read(&base, &pushed)), "{:?}", read(&base, &pushed));
    assert!(claude(&read(&base, "HEAD")), "{:?}", read(&base, "HEAD"));
    assert_eq!(read(&pushed, &later), Vec::<String>::new());
    assert_eq!(read(&later, &later), Vec::<String>::new());
    let missing = |rev: &str| {
        format!(
            "publish: `{rev}` names no commit, so no commit of the branch was read; \
             fetch it (`git fetch origin main`) or name the range with `--base <rev> --head <rev>`"
        )
    };
    assert_eq!(read(&base, "gone"), [missing("gone")]);
    assert_eq!(
        read("0000000000000000000000000000000000000000", &later).len(),
        1
    );
    git(&["update-ref", "-d", "refs/remotes/origin/main"]);
    assert_eq!(lever(&["publish"], &root), [missing("origin/main")]);
}

#[test]
fn ci_reads_the_exact_range_of_a_pull_request_and_of_a_push() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let ci = std::fs::read_to_string(repo.join(".github/workflows/ci.yml")).unwrap();
    let docs = yaml_rust2::YamlLoader::load_from_str(&ci).unwrap();
    let steps: Vec<(String, String, String, String)> =
        (docs[0]["jobs"].as_hash().unwrap().values())
            .flat_map(|job| job["steps"].as_vec().cloned().unwrap_or_default())
            .filter(|step| {
                step["run"]
                    .as_str()
                    .is_some_and(|r| r.contains("xtask publish"))
            })
            .map(|step| {
                let env = |key: &str| step["env"][key].as_str().unwrap_or("").to_owned();
                let (base, head) = match step["if"].as_str() {
                    Some("github.event_name == 'push'") => (env("BEFORE"), env("AFTER")),
                    _ => (env("BASE"), env("PR_HEAD")),
                };
                let when = step["if"].as_str().unwrap_or("").to_owned();
                (when, base, head, step["run"].as_str().unwrap().to_owned())
            })
            .collect();
    let want = |when: &str, base: &str, head: &str, run: &str| {
        (
            when.to_owned(),
            base.to_owned(),
            head.to_owned(),
            run.to_owned(),
        )
    };
    assert_eq!(
        steps,
        [
            want(
                "github.event_name == 'pull_request'",
                "${{ github.event.pull_request.base.sha }}",
                "${{ github.event.pull_request.head.sha }}",
                "cargo xtask publish --base \"$BASE\" --head \"$PR_HEAD\"",
            ),
            want(
                "github.event_name == 'push'",
                "${{ github.event.before }}",
                "${{ github.sha }}",
                "cargo xtask publish --base \"$BEFORE\" --head \"$AFTER\"",
            ),
        ]
    );
}

#[test]
fn an_identity_line_that_git_fsck_refuses_fails_and_the_claude_address_in_any_header_fails() {
    let root = tree("publish-objects", &[("xtask/src/release.rs", RELEASE)]);
    let base = on_main(&root);
    let git = |args: &[&str]| git_as(&root, MAINTAINER, MAINTAINER, args);
    let tree_id = git(&["rev-parse", "HEAD^{tree}"]);
    let me = PERSON.1;
    let who = format!("orbitar-admin <{}> 1764100000 +0000", MAINTAINER.1);
    let object = |author: &str, extra: &str| {
        format!(
            "tree {tree_id}\nparent {base}\nauthor {author}\ncommitter {who}\n{extra}\na step\n"
        )
    };
    let written = |object: &str| {
        let path = root.join("object.txt");
        std::fs::write(&path, object).unwrap();
        let strict = Command::new("git")
            .args(["hash-object", "-t", "commit", "object.txt"])
            .current_dir(&root)
            .output()
            .unwrap()
            .status
            .success();
        let id = git(&[
            "hash-object",
            "-t",
            "commit",
            "-w",
            "--literally",
            "object.txt",
        ]);
        std::fs::remove_file(&path).unwrap();
        (id, strict)
    };
    let bad = "its author line is no `name <address> seconds zone`, as git fsck --strict reads it";
    for (author, fine) in [
        (format!("x <{me}> 0 +0000"), true),
        (format!("x <{me}> 1764100000 -0500"), true),
        ("x 1764100000 +0000".to_owned(), false),
        (format!("<{me}> 1764100000 +0000"), false),
        (format!("x<{me}> 1764100000 +0000"), false),
        (format!("x> <{me}> 1764100000 +0000"), false),
        (format!("x <{me} 1764100000 +0000"), false),
        (format!("x <a<{me}> 1764100000 +0000"), false),
        (format!("x <{me}>1764100000 +0000"), false),
        (format!("x <{me}> 01764100000 +0000"), false),
        (format!("x <{me}> +1764100000 +0000"), false),
        (format!("x <{me}> 99999999999999999999 +0000"), false),
        (format!("x <{me}> soon +0000"), false),
        (format!("x <{me}>  +0000"), false),
        (format!("x <{me}> 1764100000 UTC"), false),
        (format!("x <{me}> 1764100000 +00a0"), false),
        (format!("x <{me}> 1764100000 +00000"), false),
        (format!("x <{me}> 1764100000 +0000 x"), false),
        (format!("x <{me}> 1764100000"), false),
    ] {
        let (id, strict) = written(&object(&author, "\n"));
        assert_eq!(strict, fine, "git and this test disagree on `{author}`");
        let found = publish_at(&root, &id);
        let want: Vec<String> = if fine {
            Vec::new()
        } else {
            vec![format!(
                "publish: commit {}: {bad}",
                git(&["rev-parse", "--short", &id])
            )]
        };
        assert_eq!(found, want, "`{author}`");
    }
    let loud = format!("NoReply{AT}Anthropic.COM");
    for (extra, needle) in [
        (
            format!("author {who}\n\n"),
            "it holds 2 author lines, not one",
        ),
        (format!("x-note {loud}\n\n"), "holds the Claude address"),
        (
            format!("gpgsig -----BEGIN-----\n {loud}\n -----END-----\n\n"),
            "holds the Claude address",
        ),
    ] {
        let (id, _) = written(&object(&who, &extra));
        let found = publish_at(&root, &id);
        assert_eq!(found.len(), 1, "{extra}: {found:?}");
        assert!(has(&found, needle), "{extra}: {found:?}");
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
