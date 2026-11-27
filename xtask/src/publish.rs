use crate::{Findings, git, release, walk};
use std::path::Path;
use yaml_rust2::{Yaml, YamlLoader};

const ACTIONS: [&str; 7] = [
    "actions/checkout",
    "actions/cache/restore",
    "actions/cache/save",
    "actions/upload-artifact",
    "astral-sh/setup-uv",
    "taiki-e/install-action",
    "EmbarkStudios/cargo-deny-action",
];

const DOCKER: [&str; 4] = ["build", "run", "history", "image inspect"];

const STEPS: [&str; 26] = [
    "docker push",
    "docker image push",
    "docker login",
    "podman push",
    "skopeo copy",
    "type=registry",
    "docker/login-action",
    "build-push-action",
    "--push",
    "push: true",
    "twine upload",
    "maturin publish",
    "maturin upload",
    "uv publish",
    "poetry publish",
    "cargo publish",
    "npm publish",
    "gh release",
    "/releases",
    "create-release",
    "release-action",
    "action-gh-release",
    "gh-action-pypi-publish",
    "upload-release-asset",
    "git push",
    "git tag",
];

const WORKFLOWS: &str = ".github/workflows";
const SCANNED: [&str; 3] = ["container", "xtask/src/release.rs", "README.md"];
const RELEASE: &str = "xtask/src/release.rs";
const OWNERS: [&str; 3] = [".github/CODEOWNERS", "CODEOWNERS", "docs/CODEOWNERS"];

fn uncommented(line: &str) -> &str {
    let cut = (line.char_indices())
        .find(|&(i, c)| c == '#' && (i == 0 || line[..i].ends_with([' ', '\t'])))
        .map_or(line.len(), |(i, _)| i);
    line[..cut].trim_end()
}

fn permissions(file: &str, lines: &[&str]) -> Findings {
    let mut findings = Vec::new();
    let indent = |l: &str| l.len() - l.trim_start().len();
    if !lines.iter().any(|l| l.starts_with("permissions:")) {
        findings.push(format!(
            "publish: {file} has no top-level `permissions:`; a workflow holds `contents: read` only"
        ));
    }
    for (k, line) in lines.iter().enumerate() {
        let Some(value) = line.trim_start().strip_prefix("permissions:") else {
            continue;
        };
        let value = value.trim();
        let entries: Vec<String> = if let Some(inner) = value.strip_prefix('{') {
            (inner.trim_end_matches('}').split(','))
                .map(str::to_owned)
                .collect()
        } else if value.is_empty() {
            (lines[k + 1..].iter())
                .take_while(|l| l.trim().is_empty() || indent(l) > indent(line))
                .filter(|l| !l.trim().is_empty())
                .map(|l| l.trim().to_owned())
                .collect()
        } else {
            vec![value.to_owned()]
        };
        let normal = |e: &str| {
            e.replace(['"', '\''], "")
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
        };
        let granted: Vec<String> = entries
            .iter()
            .map(|e| normal(e))
            .filter(|e| !e.is_empty())
            .collect();
        if granted != ["contents: read"] {
            findings.push(format!(
                "publish: {file}:{} grants {granted:?}; a workflow holds `contents: read` only",
                k + 1
            ));
        }
    }
    findings
}

fn steps(file: &str, lines: &[&str]) -> Findings {
    let mut findings = Vec::new();
    for (k, line) in lines.iter().enumerate() {
        let code = line.trim_start().trim_start_matches("- ").trim();
        if let Some(action) = code.strip_prefix("uses:") {
            let name = action.trim().trim_matches(['"', '\'']);
            let name = name.split('@').next().unwrap_or(name);
            if !ACTIONS.contains(&name) {
                findings.push(format!(
                    "publish: {file}:{} uses `{name}`, which is not among the actions a workflow may use",
                    k + 1
                ));
            }
        }
        let script = code.split_whitespace().any(|w| {
            let w = w.trim_matches(['"', '\'', ';', '(', ')']);
            w.starts_with("./") || w.ends_with(".sh") || w.ends_with(".py") || w.ends_with(".bash")
        });
        if script {
            findings.push(format!(
                "publish: {file}:{} runs a script; a workflow runs the levers, `cargo xtask <command>`",
                k + 1
            ));
        }
    }
    findings
}

const HOSTED: [&str; 11] = [
    "ubuntu-latest",
    "ubuntu-24.04",
    "ubuntu-26.04",
    "ubuntu-24.04-arm",
    "ubuntu-26.04-arm",
    "macos-latest",
    "macos-15",
    "macos-26",
    "windows-latest",
    "windows-2025",
    "windows-11-arm",
];

const POLICY: &str = "a job here runs on one label of HOSTED, the curated GitHub-hosted runners for public \
                      repositories; never a self-hosted runner, which a fork pull request could reach";

fn runner(job: &Yaml) -> Result<(), String> {
    let label = |y: &Yaml| match y {
        Yaml::String(l) if l.contains("${{") => Err(format!("the expression `{l}`")),
        Yaml::String(l) if HOSTED.contains(&l.as_str()) => Ok(()),
        Yaml::String(l) => Err(format!(
            "`{l}`, which is outside this repository's allowlist of hosted labels"
        )),
        other => Err(format!("{other:?}, which is no label")),
    };
    match &job["runs-on"] {
        Yaml::BadValue if !job["uses"].is_badvalue() => {
            Err("a reusable workflow, whose runner this check cannot see".to_owned())
        }
        Yaml::BadValue => Err("no `runs-on`".to_owned()),
        Yaml::Array(labels) => match labels.as_slice() {
            [one] => label(one),
            _ => Err(format!(
                "{} labels, which no GitHub-hosted runner carries together",
                labels.len()
            )),
        },
        Yaml::Hash(_) => Err("a runner group or label set (a mapping)".to_owned()),
        other => label(other),
    }
}

fn runners(file: &str, text: &str) -> Findings {
    let docs = match YamlLoader::load_from_str(text) {
        Ok(docs) => docs,
        Err(e) => {
            return vec![format!(
                "publish: {file} is not YAML that the runner check reads: {e}"
            )];
        }
    };
    let [doc] = docs.as_slice() else {
        return vec![format!(
            "publish: {file} holds {} YAML documents; a workflow is one",
            docs.len()
        )];
    };
    let Yaml::Hash(jobs) = &doc["jobs"] else {
        return vec![format!(
            "publish: {file} has no `jobs:` mapping for the runner check"
        )];
    };
    let mut findings = Vec::new();
    for (id, job) in jobs {
        let id = id.as_str().map_or_else(|| format!("{id:?}"), str::to_owned);
        if let Err(what) = runner(job) {
            findings.push(format!(
                "publish: {file} job `{id}` runs on {what}; {POLICY}"
            ));
        }
    }
    findings
}

fn docker(text: &str) -> Findings {
    let mut findings = Vec::new();
    let call = "Command::new(\"docker\")";
    for (at, _) in text.match_indices(call) {
        let rest = &text[at + call.len()..];
        let literals: Vec<&str> = rest.split('"').skip(1).step_by(2).take(2).collect();
        let sub = match literals.as_slice() {
            ["image", next, ..] => format!("image {next}"),
            [first, ..] => (*first).to_owned(),
            [] => String::new(),
        };
        if !DOCKER.contains(&sub.as_str()) {
            let line = text[..at].lines().count();
            findings.push(format!(
                "publish: {RELEASE}:{line} runs `docker {sub}`; release.rs runs only {DOCKER:?}"
            ));
        }
    }
    findings
}

fn files(root: &Path) -> Findings {
    let mut findings = Vec::new();
    for file in walk::files(root, ".") {
        let what = format!("publish: {file}");
        if OWNERS.contains(&file.as_str()) {
            findings.push(format!(
                "{what} names the accounts that own the code; the PR body records each review (CONTRIBUTING.md)"
            ));
            continue;
        }
        let text = walk::read(root, &file);
        if claude_identity(&text) {
            findings.push(format!(
                "{what} holds the Claude identity; commit and write as the maintainer"
            ));
        }
        findings.extend(release::named(&what, &text));
        findings.extend(release::homes(
            &what,
            &text,
            "write the path from the repository root",
        ));
    }
    findings
}

pub(crate) const BASE: &str = "origin/main";

struct Who<'a> {
    name: &'a str,
    address: &'a str,
}

struct Header<'a> {
    parents: usize,
    author: Who<'a>,
    committer: Who<'a>,
}

fn who(value: &str) -> Option<Who<'_>> {
    let (name, rest) = value.split_once('<')?;
    let (address, when) = rest.split_once('>')?;
    let mut when = when.split_whitespace();
    let seconds = when.next()?;
    let zone = when.next()?;
    let zone_ok = zone.len() == 5
        && zone.starts_with(['+', '-'])
        && zone[1..].bytes().all(|b| b.is_ascii_digit());
    (seconds.bytes().all(|b| b.is_ascii_digit()) && zone_ok && when.next().is_none()).then(|| Who {
        name: name.trim(),
        address,
    })
}

fn one<'a>(role: &str, values: &[&'a str]) -> Result<Who<'a>, String> {
    match values {
        [value] => {
            who(value).ok_or_else(|| format!("its {role} line is no `name <address> time zone`"))
        }
        _ => Err(format!("it holds {} {role} lines, not one", values.len())),
    }
}

fn header(headers: &str) -> Result<Header<'_>, String> {
    let mut parents = 0;
    let (mut authors, mut committers) = (Vec::new(), Vec::new());
    for line in headers.lines().filter(|l| !l.starts_with(' ')) {
        let (key, value) = line.split_once(' ').unwrap_or((line, ""));
        match key {
            "parent" => parents += 1,
            "author" => authors.push(value),
            "committer" => committers.push(value),
            _ => {}
        }
    }
    let author = one("author", &authors)?;
    let committer = one("committer", &committers)?;
    Ok(Header {
        parents,
        author,
        committer,
    })
}

fn borrowed(who: &Who<'_>) -> bool {
    (release::NOREPLY.iter()).any(|(name, address)| {
        let (same_name, same_address) = (
            who.name.eq_ignore_ascii_case(name),
            who.address.eq_ignore_ascii_case(address),
        );
        same_name != same_address
    })
}

fn commit(short: &str, object: &str, headers: &str, message: &str) -> Findings {
    let what = format!("publish: commit {short}");
    let mut findings = Vec::new();
    if claude_identity(object) {
        findings.push(format!(
            "{what}: it holds the Claude identity; commit under your own name or as the maintainer"
        ));
    }
    let header = match header(headers) {
        Ok(header) => header,
        Err(e) => {
            findings.push(format!("{what}: {e}; git writes no such commit"));
            return findings;
        }
    };
    let ((maintainer, by), (github, merges)) = (release::MAINTAINER, release::GITHUB);
    for (role, who) in [("author", &header.author), ("committer", &header.committer)] {
        if borrowed(who) {
            findings.push(format!(
                "{what}: its {role} takes the name or the address of {maintainer} <{by}> or {github} <{merges}>, \
                 not both; commit under your own name and address"
            ));
        }
    }
    let github_merge =
        (header.committer.name, header.committer.address) == release::GITHUB && header.parents > 1;
    if !github_merge {
        findings.extend(release::named(&what, message));
        findings.extend(release::homes(&what, message, "reword the commit"));
    }
    findings
}

fn commits(root: &Path, base: &str) -> Findings {
    if git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--quiet",
            &format!("{base}^{{commit}}"),
        ],
    )
    .is_err()
    {
        return vec![format!(
            "publish: `{base}` names no commit, so no commit of the branch was read; \
             fetch it (`git fetch origin main`) or name the base with `--base <rev>`"
        )];
    }
    let range = format!("{base}..HEAD");
    match crate::commits(root, &range) {
        Ok(all) => (all.iter())
            .flat_map(|c| {
                let (headers, message) = c.headers_and_message();
                commit(&c.short, c.object(), headers, message)
            })
            .collect(),
        Err(e) => vec![format!("publish: git log {range}: {e}")],
    }
}

pub(crate) fn check(root: &Path, base: &str) -> Findings {
    let mut findings = files(root);
    findings.extend(commits(root, base));
    for file in walk::files(root, WORKFLOWS) {
        let text = walk::read(root, &file);
        let lines: Vec<&str> = text.lines().map(uncommented).collect();
        findings.extend(permissions(&file, &lines));
        findings.extend(steps(&file, &lines));
        findings.extend(runners(&file, &text));
    }
    findings.extend(docker(&walk::read(root, RELEASE)));
    let scanned = (walk::files(root, WORKFLOWS).into_iter())
        .chain(SCANNED.iter().flat_map(|p| walk::files(root, p)));
    for file in scanned {
        for (k, line) in walk::read(root, &file).lines().enumerate() {
            let code = line.trim();
            if code.starts_with('#') || code.starts_with("//") {
                continue;
            }
            let lower = code.to_lowercase();
            for step in STEPS.iter().filter(|s| lower.contains(*s)) {
                findings.push(format!(
                    "publish: {file}:{} holds `{step}`; nothing is published from this repository",
                    k + 1
                ));
            }
        }
    }
    findings
}

fn claude_identity(text: &str) -> bool {
    text.to_ascii_lowercase()
        .contains(concat!("noreply", "@", "anthropic.com"))
}
