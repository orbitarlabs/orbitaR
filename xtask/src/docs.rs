use crate::{
    Findings, commands, git, layers, layout, results,
    syntax::{self, Tree},
    walk,
};
use proc_macro2::{Delimiter, TokenStream, TokenTree};
use pulldown_cmark::{BrokenLink, CowStr, Event, LinkType, Options, Parser, Tag, TagEnd};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Component, Path},
    process::Command,
};

const PROSE: [(&str, usize); 5] = [
    ("AGENTS.md", 22),
    ("CONTRIBUTING.md", 59),
    ("container/README.md", 73),
    ("README.md", 126),
    ("SECURITY.md", 4),
];
const LOCAL: &str = "CLAUDE.local.md";
const NOTEBOOK: &str = "notebooks/quickstart.ipynb";
const NOTEBOOK_CELLS: [&str; 3] = [
    "%pip install orbitar",
    "README.md's `## ASE` block",
    "the printout",
];
const NOTEBOOK_BLOCK: &str = "ASE";
const MARKER: (&str, &str) = ("<!-- local ", " -->");

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Local {
    file: Vec<LocalFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LocalFile {
    path: String,
    cap: usize,
    #[serde(default)]
    headings: Vec<String>,
    #[serde(default)]
    layers: bool,
    #[serde(default)]
    rows: Vec<String>,
}
const PROSE_SUFFIXES: [&str; 23] = [
    ".md",
    ".mdx",
    ".markdown",
    ".qmd",
    ".rst",
    ".adoc",
    ".asciidoc",
    ".textile",
    ".typ",
    ".xhtml",
    ".pptx",
    ".txt",
    ".html",
    ".htm",
    ".org",
    ".tex",
    ".ipynb",
    ".pdf",
    ".docx",
    ".doc",
    ".odt",
    ".rtf",
    ".wiki",
];
const PROSE_NAMES: [&str; 8] = [
    "readme",
    "notes",
    "changelog",
    "todo",
    "contributing",
    "guide",
    "design",
    "history",
];
const MAX_WORDS: usize = 20;
const DIAGRAMS: [&str; 2] = ["text", "mermaid"];
const MAX_LINE: usize = 120;
const MAX_PASSIVE_SHARE: f64 = 0.2;
const TERMS: [(&str, &str); 12] = [
    ("test case", "case"),
    ("benchmark task", "case"),
    ("paw setup", "dataset"),
    ("pseudopotential file", "dataset"),
    ("receipt", "record"),
    ("golden", "reference"),
    ("red run", "failing run"),
    ("fixed node", "declared node"),
    ("[gate]", "[tolerance]"),
    ("knob", "setting"),
    ("dependency order", "layer order"),
    ("deciding test", "proof"),
];
const IRREGULAR: [&str; 24] = [
    "built", "done", "found", "given", "held", "hidden", "kept", "known", "left", "made", "met",
    "put", "read", "run", "seen", "sent", "set", "shown", "taken", "told", "written", "cut",
    "bound", "split",
];
const ROOTS: [&str; 7] = [
    "src/",
    "tests/",
    "benchmarks/",
    "xtask/",
    "scratch/",
    ".github/",
    "target/",
];

pub(crate) fn check(root: &Path) -> Findings {
    let mut findings = Vec::new();
    let local = local(root, &mut findings);
    for file in walk::files(root, ".") {
        let lower = file.to_lowercase();
        let name = lower.rsplit('/').next().unwrap_or(&lower);
        let prose = PROSE_SUFFIXES.iter().any(|e| name.ends_with(e))
            || (!name.contains('.') && PROSE_NAMES.iter().any(|n| name.starts_with(n)));
        let allowed = PROSE.iter().any(|(p, _)| *p == file)
            || file == "CLAUDE.md"
            || file == LOCAL
            || file == results::PAGE
            || file == NOTEBOOK
            || local.iter().any(|(path, _)| *path == file)
            || file.starts_with(".agents/skills/");
        if prose && !allowed {
            findings.push(format!(
                "docs: {file} is extra prose; put the rule in code or delete the file"
            ));
        }
    }
    let links = walk::links(root, ".");
    for file in PROSE.iter().map(|(file, _)| *file).chain(["CLAUDE.md"]) {
        if links.iter().any(|link| link == file) {
            findings.push(format!("docs: {file} is a symlink; write the file itself"));
        }
    }
    if walk::read(root, "CLAUDE.md") != "@AGENTS.md\n" {
        findings.push("docs: CLAUDE.md must be exactly `@AGENTS.md`".into());
    }
    let public = PROSE.iter().map(|&(file, cap)| (file.to_owned(), cap));
    for (file, max_lines) in
        public.chain(local.iter().map(|(path, rules)| (path.clone(), rules.cap)))
    {
        let file = file.as_str();
        let text = walk::read(root, file);
        if text.is_empty() {
            findings.push(format!("docs: {file} is missing"));
            continue;
        }
        if text.lines().count() > max_lines {
            findings.push(format!(
                "docs: {file} has {} lines > {max_lines}",
                text.lines().count()
            ));
        }
        findings.extend(widths(file, &text));
        if file != "README.md" {
            findings.extend(style(file, &text));
        }
        let named = if file == "README.md" {
            Vec::new()
        } else {
            paths(&text)
        };
        let ignored = walk::read(root, ".gitignore");
        for path in named {
            let listed = ignored
                .lines()
                .any(|l| l.trim_matches('/') == path.trim_matches('/'));
            let layer = layers::ORDER.contains(&path.trim_end_matches('/'));
            if !(listed
                || root.join(&path).exists()
                || root.join("src").join(&path).exists()
                || layer)
            {
                findings.push(format!(
                    "docs: {file} names `{path}`, which does not exist; write <name> for a pattern"
                ));
            }
        }
        let known = commands();
        for command in named_commands(&text) {
            if !known.iter().any(|(name, _)| *name == command) {
                findings.push(format!(
                    "docs: {file} names `cargo xtask {command}`, which xtask does not provide"
                ));
            }
        }
    }
    findings.extend(readme(root));
    let readme = walk::read(root, "README.md");
    findings.extend(notebook(root, &readme));
    for (path, rules) in &local {
        let text = walk::read(root, path);
        if !text.is_empty() {
            findings.extend(asked(path, &text, rules, &readme));
        }
    }
    findings
}

pub(crate) fn declared(root: &Path, kind: &str) -> Result<Option<String>, String> {
    let text = walk::read(root, LOCAL);
    let prefix = format!("{}{kind}: ", MARKER.0);
    let marked: Vec<&str> = (text.lines())
        .filter_map(|l| {
            l.trim()
                .strip_prefix(prefix.as_str())?
                .strip_suffix(MARKER.1)
        })
        .collect();
    let path = match marked.as_slice() {
        [] => return Ok(None),
        [one] => one.trim(),
        _ => return Err(format!("{LOCAL} declares local {kind} more than once")),
    };
    if !(plain(path) && resolves_inside(root, path) && root.join(path).is_file()) {
        return Err(format!(
            "{LOCAL} declares `{path}`, which is no file inside the checkout"
        ));
    }
    Ok(Some(path.to_owned()))
}

fn plain(path: &str) -> bool {
    !path.is_empty() && (Path::new(path).components()).all(|c| matches!(c, Component::Normal(_)))
}

fn in_tree(root: &Path, path: &str) -> bool {
    let normal: Vec<&str> = (Path::new(path).components())
        .filter_map(|c| c.as_os_str().to_str())
        .collect();
    !scheme(path) && plain(path) && resolves_inside(root, path) && public(root, &normal.join("/"))
}

fn scheme(path: &str) -> bool {
    path.split_once(':').is_some_and(|(name, _)| {
        name.starts_with(|c: char| c.is_ascii_alphabetic())
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+.-".contains(c))
    })
}

fn public(root: &Path, path: &str) -> bool {
    let literal = ["--literal-pathspecs", "-c", "core.quotePath=false"];
    let Ok(tracked) = git(
        root,
        &[&literal[..], &["ls-files", "-z", "--", path]].concat(),
    ) else {
        return false;
    };
    let dir = format!("{}/", path.trim_end_matches('/'));
    let files: Vec<&str> = (tracked.split('\0'))
        .filter(|f| *f == path || f.starts_with(&dir))
        .collect();
    let within = |file: &str| -> Vec<String> {
        let dirs = (file.match_indices('/')).map(|(at, _)| file[..=at].to_owned());
        dirs.chain([file.to_owned()]).collect()
    };
    let asked: BTreeSet<String> = files.iter().flat_map(|f| within(f)).collect();
    let asked: Vec<&str> = asked.iter().map(String::as_str).collect();
    let ask = [
        &literal[..],
        &["check-attr", "-z", "export-ignore", "--"],
        &asked,
    ]
    .concat();
    let Ok(out) = git(root, &ask) else {
        return false;
    };
    let fields: Vec<&str> = out.split('\0').collect();
    let ignored: BTreeSet<&str> = (fields.chunks(3))
        .filter(|f| matches!(f, [_, "export-ignore", "set"]))
        .map(|f| f[0])
        .collect();
    (files.iter()).any(|f| within(f).iter().all(|p| !ignored.contains(p.as_str())))
}

fn resolves_inside(root: &Path, path: &str) -> bool {
    match (root.canonicalize(), root.join(path).canonicalize()) {
        (Ok(base), Ok(real)) => real.starts_with(base),
        _ => false,
    }
}

fn local(root: &Path, findings: &mut Findings) -> Vec<(String, LocalFile)> {
    let manifest = match declared(root, "docs") {
        Ok(Some(manifest)) => manifest,
        Ok(None) => return Vec::new(),
        Err(e) => {
            findings.push(format!("docs: {e}"));
            return Vec::new();
        }
    };
    let text = walk::read(root, LOCAL);
    let declared: Local = match toml::from_str(&walk::read(root, &manifest)) {
        Ok(declared) => declared,
        Err(e) => {
            findings.push(format!("docs: {manifest}: {e}"));
            return Vec::new();
        }
    };
    let dir = manifest.rsplit_once('/').map_or("", |(dir, _)| dir);
    let imports: Vec<&str> = (text.lines())
        .filter_map(|l| l.trim().strip_prefix('@'))
        .collect();
    let mut out = Vec::new();
    for rules in declared.file {
        let path = match dir {
            "" => rules.path.clone(),
            dir => format!("{dir}/{}", rules.path),
        };
        let missing = std::fs::symlink_metadata(root.join(&path)).is_err();
        if !(plain(&path) && (missing || resolves_inside(root, &path))) {
            findings.push(format!(
                "docs: {manifest} lists `{}`, which is outside the checkout",
                rules.path
            ));
            continue;
        }
        if !imports.contains(&path.as_str()) {
            findings.push(format!("docs: {LOCAL} does not import `@{path}`"));
        }
        out.push((path, rules));
    }
    out
}

fn asked(path: &str, text: &str, rules: &LocalFile, readme: &str) -> Findings {
    let mut findings = headings(path, text, &rules.headings);
    if rules.layers {
        for layer in layers::ORDER {
            if !text.contains(&format!("{layer}/")) {
                findings.push(format!(
                    "docs: {path} must place layer `{layer}/` on the stack"
                ));
            }
        }
    }
    let row = |text: &str, key: &str| -> Option<String> {
        (text.lines())
            .find_map(|l| l.strip_prefix(&format!("{key} ")))
            .map(|rest| rest.trim().to_owned())
    };
    for key in &rules.rows {
        match (row(text, key), row(readme, key)) {
            (None, _) => findings.push(format!("docs: {path} has no `{key}` row")),
            (_, None) => findings.push(format!("docs: README.md has no `{key}` row for {path}")),
            (Some(mine), Some(public)) if mine != public => findings.push(format!(
                "docs: {path}'s `{key}` row differs from README.md's"
            )),
            _ => {}
        }
    }
    findings
}

fn html(root: &Path, line: &str) -> bool {
    let line = line.trim();
    let file = |path: &str| {
        path.strip_prefix("assets/").is_some_and(|rest| {
            (rest.ends_with(".svg") || rest.ends_with(".png"))
                && !rest
                    .split('/')
                    .any(|part| part.is_empty() || part.starts_with('.'))
        }) && in_tree(root, path)
            && root.join(path).is_file()
    };
    let quoted = |rest: &str, key: &str| -> Option<(String, String)> {
        let rest = rest.trim_start().strip_prefix(key)?.strip_prefix("=\"")?;
        let (value, after) = rest.split_once('"')?;
        Some((value.to_owned(), after.to_owned()))
    };
    match line {
        "<p align=\"center\">" | "</p>" | "<picture>" | "</picture>" | "</a>" => true,
        "<details>" | "</details>" => true,
        "<a href=\"benchmarks/README.md\">" => in_tree(root, "benchmarks/README.md"),
        _ if line.starts_with("<summary>") => line
            .strip_prefix("<summary>")
            .and_then(|rest| rest.strip_suffix("</summary>"))
            .is_some_and(|text| !text.trim().is_empty() && !text.contains(['<', '>'])),
        _ if line.starts_with("<source ") => {
            let Some((media, rest)) = quoted(&line["<source".len()..], "media") else {
                return false;
            };
            let Some((path, rest)) = quoted(&rest, "srcset") else {
                return false;
            };
            matches!(
                media.as_str(),
                "(prefers-color-scheme: dark)" | "(prefers-color-scheme: light)"
            ) && file(&path)
                && rest.trim() == ">"
        }
        _ if line.starts_with("<img ") => {
            let Some((path, rest)) = quoted(&line["<img".len()..], "src") else {
                return false;
            };
            let Some((alt, rest)) = quoted(&rest, "alt") else {
                return false;
            };
            let rest = match quoted(&rest, "width") {
                Some((width, after))
                    if !width.is_empty() && width.bytes().all(|b| b.is_ascii_digit()) =>
                {
                    after
                }
                Some(_) => return false,
                None => rest,
            };
            file(&path) && !alt.trim().is_empty() && rest.trim() == ">"
        }
        _ => false,
    }
}

fn readme_block(readme: &str, heading: &str) -> Option<String> {
    let section = readme.split(&format!("\n## {heading}\n")).nth(1)?;
    let example = section.split("```").nth(1)?;
    Some(example.lines().skip(1).collect::<Vec<_>>().join("\n"))
}

fn notebook(root: &Path, readme: &str) -> Findings {
    let text = walk::read(root, NOTEBOOK);
    if text.is_empty() {
        return Vec::new();
    }
    let book: serde_json::Value = match serde_json::from_str(&text) {
        Ok(book) => book,
        Err(e) => return vec![format!("docs: {NOTEBOOK} does not parse: {e}")],
    };
    let mut findings = Vec::new();
    if book["nbformat"] != 4 {
        findings.push(format!("docs: {NOTEBOOK} is no nbformat 4 notebook"));
    }
    let cells = book["cells"].as_array().map_or(&[][..], Vec::as_slice);
    let mut sources = Vec::new();
    for (n, cell) in (1..).zip(cells) {
        if cell["cell_type"] != "code" {
            findings.push(format!(
                "docs: {NOTEBOOK} cell {n} is no code cell; the notebook holds code alone"
            ));
        }
        let outputs = cell["outputs"].as_array().is_some_and(Vec::is_empty);
        if !outputs || !cell["execution_count"].is_null() {
            findings.push(format!(
                "docs: {NOTEBOOK} cell {n} stores outputs or an execution count; clear them"
            ));
        }
        sources.push(match &cell["source"] {
            serde_json::Value::String(line) => line.clone(),
            serde_json::Value::Array(lines) => (lines.iter())
                .filter_map(serde_json::Value::as_str)
                .collect(),
            _ => String::new(),
        });
    }
    if sources.len() != NOTEBOOK_CELLS.len() {
        findings.push(format!(
            "docs: {NOTEBOOK} holds {} cells, not {}: {NOTEBOOK_CELLS:?}",
            sources.len(),
            NOTEBOOK_CELLS.len()
        ));
    }
    if sources.first().map(String::as_str) != Some(NOTEBOOK_CELLS[0]) {
        findings.push(format!(
            "docs: {NOTEBOOK} cell 1 is not `{}`",
            NOTEBOOK_CELLS[0]
        ));
    }
    if sources.get(1).is_none() || sources.get(1) != readme_block(readme, NOTEBOOK_BLOCK).as_ref() {
        findings.push(format!(
            "docs: {NOTEBOOK} cell 2 is not {}, byte for byte",
            NOTEBOOK_CELLS[1]
        ));
    }
    findings
}

fn readme(root: &Path) -> Findings {
    let text = walk::read(root, "README.md");
    let tests: Vec<String> = ["tests", "xtask/tests"]
        .iter()
        .flat_map(|under| walk::files(root, under))
        .filter(|path| path.ends_with(".rs"))
        .map(|path| walk::read(root, &path))
        .collect();
    let parsed = parse_readme(root, &text);
    let mut findings = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    let mut heading = String::new();
    let mut blocks = 0;
    let mut rows = lines.iter().enumerate();
    while let Some((n, line)) = rows.next() {
        if let Some(title) = line.strip_prefix("## ") {
            heading = title.trim().to_owned();
            blocks = 0;
        }
        let fence = line.trim_start();
        if !fence.starts_with("```") {
            let at = format!("docs: README.md:{}", n + 1);
            if fence.starts_with('<') {
                if !html(root, line) {
                    findings.push(format!(
                        "{at}: HTML comes from the allowlist only: a picture under assets/, <details>, <summary>"
                    ));
                }
            } else if inline_html(line) || parsed.html.contains(&(n + 1)) {
                findings.push(format!(
                    "{at}: put HTML on its own line, from the allowlist"
                ));
            }
            continue;
        }
        let body: Vec<&str> = lines[n + 1..]
            .iter()
            .take_while(|l| !l.trim_start().starts_with("```"))
            .copied()
            .collect();
        let at = format!("docs: README.md:{}", n + 2);
        blocks += 1;
        if blocks > 1 {
            findings.push(format!(
                "{at}: `## {heading}` holds one block; give this one its own heading"
            ));
        }
        let named = format!("readme_block(\"{heading}\")");
        let diagram = DIAGRAMS.contains(&fence.trim_start_matches('`').trim());
        if !diagram
            && !tests
                .iter()
                .any(|test| test.contains(&named) || holds(test, &body))
        {
            findings.push(format!(
                "{at}: no test under tests/ or xtask/tests/ names {named} or holds this example"
            ));
        }
        rows.by_ref().take(body.len() + 1).for_each(drop);
    }
    findings.extend(parsed.links);
    findings
}

fn outside_code(line: &str) -> impl Iterator<Item = &str> {
    line.split('`').step_by(2)
}

fn markup(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    chars
        .windows(2)
        .any(|w| w[0] == '<' && (w[1].is_ascii_alphabetic() || matches!(w[1], '/' | '!')))
}

fn inline_html(line: &str) -> bool {
    outside_code(line).any(markup)
}

const REFERENCE: &str =
    "write each link inline as [text](path); a reference link escapes the tree check";

struct Parsed {
    links: Findings,
    html: BTreeSet<usize>,
}

fn mails(text: &str) -> Vec<&str> {
    let local = |c: char| c.is_ascii_alphanumeric() || "._%+-".contains(c);
    let host = |c: char| c.is_ascii_alphanumeric() || ".-".contains(c);
    (text.match_indices('@'))
        .filter_map(|(at, _)| {
            let start = (text[..at].char_indices().rev())
                .find(|&(_, c)| !local(c))
                .map_or(0, |(i, c)| i + c.len_utf8());
            let after = &text[at + 1..];
            let domain = after[..after.find(|c| !host(c)).unwrap_or(after.len())]
                .trim_end_matches(['.', '-']);
            let (_, tld) = domain.rsplit_once('.')?;
            (start < at && tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic()))
                .then(|| &text[start..at + 1 + domain.len()])
        })
        .collect()
}

fn addresses(prose: &str) -> Vec<&str> {
    let edge = |c: char| "\"'()[]<>.,;:!?*_~".contains(c);
    let web = (prose.split_whitespace())
        .map(|word| word.trim_matches(edge))
        .filter(|word| word.contains("://") || word.to_ascii_lowercase().contains("www."));
    web.chain(mails(prose)).collect()
}

fn visible(raw: &str) -> String {
    let mut inside = false;
    (raw.chars())
        .filter(|&c| {
            let shown = !inside && c != '<';
            inside = (inside || c == '<') && c != '>';
            shown
        })
        .collect()
}

fn parse_readme(root: &Path, text: &str) -> Parsed {
    let line = |at: usize| text[..at].matches('\n').count() + 1;
    let gone = |target: &str| format!("the link `{target}` names no file of the tree");
    let unresolved = |_: BrokenLink<'_>| Some((CowStr::from(""), CowStr::from("")));
    let parser = Parser::new_with_broken_link_callback(text, Options::empty(), Some(unresolved));
    let mut found: BTreeSet<(usize, String)> = (parser.reference_definitions().iter())
        .map(|(_, definition)| (line(definition.span.start), REFERENCE.to_owned()))
        .collect();
    let mut html = BTreeSet::new();
    let mut code = 0;
    let mut prose: Option<(usize, String)> = None;
    for (event, span) in parser.into_offset_iter() {
        if let Event::Text(words) = &event
            && code == 0
        {
            prose
                .get_or_insert_with(|| (line(span.start), String::new()))
                .1 += words;
            continue;
        }
        if let Some((at, run)) = prose.take() {
            found.extend(addresses(&run).into_iter().map(|a| (at, gone(a))));
        }
        match event {
            Event::Html(raw) | Event::InlineHtml(raw) => {
                let at = line(span.start);
                if markup(&raw) {
                    html.insert(at);
                }
                let shown = visible(&raw);
                found.extend(addresses(&shown).into_iter().map(|a| (at, gone(a))));
            }
            Event::Start(Tag::CodeBlock(_)) => code += 1,
            Event::End(TagEnd::CodeBlock) => code -= 1,
            Event::Start(
                Tag::Link {
                    link_type,
                    dest_url,
                    ..
                }
                | Tag::Image {
                    link_type,
                    dest_url,
                    ..
                },
            ) => match link_type {
                LinkType::ShortcutUnknown => {}
                LinkType::Inline | LinkType::Autolink | LinkType::Email => {
                    if !in_tree(root, &dest_url) {
                        found.insert((line(span.start), gone(&dest_url)));
                    }
                }
                _ => {
                    found.insert((line(span.start), REFERENCE.to_owned()));
                }
            },
            _ => {}
        }
    }
    if let Some((at, run)) = prose {
        found.extend(addresses(&run).into_iter().map(|a| (at, gone(a))));
    }
    Parsed {
        links: (found.into_iter())
            .map(|(n, what)| format!("docs: README.md:{n}: {what}"))
            .collect(),
        html,
    }
}

fn holds(file: &str, example: &[&str]) -> bool {
    let trimmed = |lines: Vec<&str>| -> Vec<String> {
        lines
            .into_iter()
            .map(|l| l.trim().to_owned())
            .filter(|l| !l.is_empty())
            .collect()
    };
    let (file, example) = (trimmed(file.lines().collect()), trimmed(example.to_vec()));
    !example.is_empty() && file.windows(example.len()).any(|w| w == example.as_slice())
}

fn headings(path: &str, review: &str, wanted: &[String]) -> Findings {
    let mut findings = Vec::new();
    let mut last = 0;
    for gate in wanted {
        let heading = format!("## {gate}");
        let Some(at) = review.find(&heading) else {
            findings.push(format!("docs: {path} lacks the heading `{heading}`"));
            continue;
        };
        if at < last {
            findings.push(format!("docs: {path} lists `{heading}` out of order"));
        }
        last = at;
        let body = &review[at + heading.len()..];
        let body = &body[..body.find("\n## ").unwrap_or(body.len())];
        if !body.lines().any(|l| {
            let l = l.trim_start();
            l.starts_with("- [ ] ") || l.starts_with("* [ ] ")
        }) {
            findings.push(format!("docs: {path} `{heading}` has no `- [ ]` item"));
        }
    }
    findings
}

fn widths(file: &str, text: &str) -> Findings {
    (text.lines().enumerate())
        .filter_map(|(n, line)| {
            let width = line.chars().count();
            (width > MAX_LINE).then(|| {
                format!(
                    "docs: {file}:{}: {width} characters > {MAX_LINE}; shorten the line",
                    n + 1
                )
            })
        })
        .collect()
}

fn style(file: &str, text: &str) -> Findings {
    let mut findings = Vec::new();
    let (mut sentences, mut passive) = (0usize, 0usize);
    let mut fenced = false;
    for (n, line) in text.lines().enumerate() {
        let at = format!("docs: {file}:{}", n + 1);
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        let trimmed = line.trim();
        if fenced || trimmed.is_empty() || trimmed.starts_with('|') {
            continue;
        }
        let Some(body) = bullet_or_heading(trimmed) else {
            findings.push(format!(
                "{at}: paragraph line; use a bullet, table, or code block"
            ));
            continue;
        };
        let lower = body.to_lowercase();
        for (bad, good) in TERMS {
            if lower.contains(bad) {
                findings.push(format!("{at}: write `{good}`, not `{bad}`"));
            }
        }
        for sentence in sentences_of(body) {
            sentences += 1;
            let words = sentence
                .split_whitespace()
                .filter(|w| w.chars().any(char::is_alphanumeric))
                .count();
            if words > MAX_WORDS {
                findings.push(format!("{at}: {words} words > {MAX_WORDS}: \"{sentence}\""));
            }
            let s = format!(" {} ", sentence.to_lowercase());
            if s.contains(", then ") || s.contains(" and then ") {
                findings.push(format!("{at}: one action per sentence: \"{sentence}\""));
            }
            if is_passive(&s) {
                passive += 1;
            }
        }
    }
    if passive as f64 > MAX_PASSIVE_SHARE * sentences as f64 {
        findings.push(format!(
            "docs: {file}: {passive}/{sentences} sentences read as passive (> {MAX_PASSIVE_SHARE})"
        ));
    }
    findings
}

fn bullet_or_heading(line: &str) -> Option<&str> {
    for prefix in ["- [ ] ", "- [x] ", "* [ ] ", "* [x] ", "- ", "* "] {
        if let Some(rest) = line.strip_prefix(prefix) {
            return Some(rest);
        }
    }
    line.starts_with('#')
        .then(|| line.trim_start_matches('#').trim())
}

fn sentences_of(body: &str) -> Vec<String> {
    let mut plain = String::new();
    for (i, part) in body.split('`').enumerate() {
        plain.push_str(if i % 2 == 1 { "CODE" } else { part });
    }
    let plain = plain.replace("e.g.", "eg").replace("i.e.", "ie");
    let chars: Vec<char> = plain.chars().collect();
    let mut out = Vec::new();
    let mut current = String::new();
    for (i, c) in chars.iter().enumerate() {
        current.push(*c);
        let end = matches!(c, '.' | '?' | '!' | ';')
            && chars.get(i + 1).is_none_or(|n| n.is_whitespace());
        if end {
            out.push(std::mem::take(&mut current));
        }
    }
    out.push(current);
    out.into_iter()
        .map(|s| {
            s.trim()
                .trim_end_matches(['.', '?', '!', ';'])
                .trim()
                .to_owned()
        })
        .filter(|s| s.chars().any(char::is_alphanumeric))
        .collect()
}

fn is_passive(s: &str) -> bool {
    let words: Vec<&str> = s
        .split_whitespace()
        .map(|w| w.trim_matches(|c: char| !c.is_alphanumeric()))
        .collect();
    words.windows(2).any(|w| {
        [
            "is", "are", "was", "were", "be", "been", "being", "gets", "got",
        ]
        .contains(&w[0])
            && ((w[1].len() > 3 && (w[1].ends_with("ed") || w[1].ends_with("en")))
                || IRREGULAR.contains(&w[1]))
    })
}

fn paths(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for span in spans(text) {
        for token in
            span.split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | ',' | '"' | '='))
        {
            let token = token.trim_end_matches([':', '.', ';']);
            let lower = token
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b));
            let looks = lower
                && !token.starts_with("--")
                && (ROOTS.iter().any(|r| token.starts_with(r))
                    || token.ends_with('/')
                    || [".rs", ".toml", ".json"].iter().any(|e| token.ends_with(e)));
            if looks && token.len() > 2 && !out.contains(&token.to_owned()) {
                out.push(token.to_owned());
            }
        }
    }
    out
}

fn named_commands(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for span in spans(text) {
        let words: Vec<&str> = span.split_whitespace().collect();
        for w in words.windows(3) {
            if w[0] == "cargo" && w[1] == "xtask" {
                let name = w[2]
                    .trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '-')
                    .to_owned();
                if !name.is_empty() && !out.contains(&name) {
                    out.push(name);
                }
            }
        }
    }
    out
}

fn spans(text: &str) -> Vec<&str> {
    let mut spans = Vec::new();
    let mut fenced = false;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
            continue;
        }
        if fenced {
            spans.push(line);
        } else {
            spans.extend(line.split('`').skip(1).step_by(2));
        }
    }
    spans
}

const COMMENT_FIX: &str = "Put what it does in a name, an invariant in a type or assert, a number's source in src/SOURCE.toml, why a value fits in a test or case record, and a rule to keep in a test or lever. Delete history, dates, TODOs, and doc pointers.";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Code {
    Rust,
    Python,
    Toml,
    Yaml,
    Shell,
    Dockerfile,
}

impl Code {
    fn of(file: &str) -> Option<Self> {
        let path = Path::new(file);
        if path.file_name().and_then(|name| name.to_str()) == Some("Dockerfile") {
            return Some(Self::Dockerfile);
        }
        match path.extension().and_then(|ext| ext.to_str())? {
            "rs" => Some(Self::Rust),
            "py" => Some(Self::Python),
            "toml" => Some(Self::Toml),
            "yml" | "yaml" => Some(Self::Yaml),
            "sh" => Some(Self::Shell),
            _ => None,
        }
    }
}

#[derive(Deserialize)]
struct Comment {
    line: usize,
    column: usize,
    text: String,
    read_by_program: bool,
}

const GIT_SYMLINK_MODE: &str = "120000";

pub(crate) fn added_comments(root: &Path, base: &str) -> Findings {
    match comment_diff(root, base) {
        Ok(findings) => findings,
        Err(error) => vec![format!("docs: {error}")],
    }
}

fn comment_diff(root: &Path, base: &str) -> Result<Findings, String> {
    let base = git(
        root,
        &[
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{base}^{{commit}}"),
        ],
    )?;
    let status = git(
        root,
        &[
            "diff",
            "--raw",
            "-z",
            "--find-renames",
            "--diff-filter=AMRT",
            &base,
            "--",
        ],
    )?;
    let mut files = status.split_terminator('\0');
    let mut findings = Vec::new();
    while let (Some(entry), Some(path)) = (files.next(), files.next()) {
        let mut fields = entry.split(' ');
        let new_mode = fields.nth(1).unwrap_or_default();
        let status = fields.nth(2).unwrap_or_default();
        let mut paths = vec![path];
        if status.starts_with('R')
            && let Some(renamed) = files.next()
        {
            paths.push(renamed);
        }
        let file = *paths.last().unwrap_or(&path);
        if new_mode == GIT_SYMLINK_MODE {
            continue;
        }
        let Some(code) = Code::of(file) else {
            continue;
        };
        let mut args = vec![
            "--literal-pathspecs",
            "diff",
            "--no-ext-diff",
            "--no-textconv",
            "--no-color",
            "--find-renames",
            "--unified=0",
            "--inter-hunk-context=0",
            &base,
            "--",
        ];
        args.extend(paths);
        let changed = added_lines(&git(root, &args)?);
        if changed.is_empty() {
            continue;
        }
        let bytes = crate::harness::datasets::regular_in(root, file)?;
        let text = String::from_utf8(bytes).map_err(|error| format!("{file}: {error}"))?;
        let comments = match code {
            Code::Python => python_comments(root, file)?,
            _ => lexical_comments(&text, code, file)?,
        };
        let lines: Vec<&str> = text.lines().collect();
        for comment in comments {
            if !changed.contains(&comment.line) {
                continue;
            }
            let prefix = &lines[comment.line - 1][..comment.column];
            let reason = if has_comment_marker(&comment.text) {
                "a comment contains a date or a task marker"
            } else if kept_comment(file, prefix, &comment) {
                continue;
            } else {
                "an added comment has no program reader"
            };
            findings.push(format!(
                "docs: {file}:{}: {reason}. {COMMENT_FIX}",
                comment.line
            ));
        }
    }
    Ok(findings)
}

fn added_lines(diff: &str) -> BTreeSet<usize> {
    let mut line = None;
    let mut added = BTreeSet::new();
    for row in diff.lines() {
        if row.starts_with("diff --git ") {
            line = None;
        } else if let Some(header) = row.strip_prefix("@@ -") {
            line = header
                .split_once(" +")
                .and_then(|(_, range)| range.split([',', ' ']).next())
                .and_then(|start| start.parse().ok());
        } else if let Some(current) = &mut line
            && row.starts_with('+')
        {
            added.insert(*current);
            *current += 1;
        }
    }
    added
}

fn has_comment_marker(text: &str) -> bool {
    ["TODO", "FIXME", "XXX", "HACK"]
        .iter()
        .any(|marker| text.contains(marker))
        || text.as_bytes().windows(10).any(|date| {
            date.iter().enumerate().all(|(i, byte)| {
                if matches!(i, 4 | 7) {
                    *byte == b'-'
                } else {
                    byte.is_ascii_digit()
                }
            })
        })
}

fn kept_comment(file: &str, prefix: &str, comment: &Comment) -> bool {
    let text = comment.text.trim_end();
    comment.read_by_program
        || (comment.line == 1 && prefix.is_empty() && shebang(text))
        || (file == layout::RUNNER
            && prefix.is_empty()
            && [layout::BEGIN.trim_end(), layout::END.trim_end()].contains(&text))
        || (file == ".cargo/mutants.toml" && prefix.trim().is_empty())
        || workflow_tag(file, prefix, text)
}

fn shebang(text: &str) -> bool {
    text.strip_prefix("#!")
        .is_some_and(|path| path.trim_start().starts_with('/'))
}

fn workflow_tag(file: &str, prefix: &str, text: &str) -> bool {
    let Some(name) = file.strip_prefix(".github/workflows/") else {
        return false;
    };
    if !name.ends_with(".yml") || name.contains('/') {
        return false;
    }
    let Some(tag) = text.strip_prefix("# v") else {
        return false;
    };
    let key = prefix.trim_start();
    let key = key.strip_prefix("- ").unwrap_or(key).trim_start();
    let Some(pin) = key.strip_prefix("uses:") else {
        return false;
    };
    let pin = pin.trim();
    let pin = pin
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| pin.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(pin);
    pin.rsplit_once('@').is_some_and(|(action, sha)| {
        action.contains('/') && sha.len() == 40 && sha.bytes().all(|byte| byte.is_ascii_hexdigit())
    }) && tag.starts_with(|c: char| c.is_ascii_digit())
        && tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b".-_+".contains(&byte))
}

fn clap_doc_lines(text: &str) -> BTreeSet<usize> {
    let Ok(trees) = syntax::parse(text) else {
        return BTreeSet::new();
    };
    let mut lines = BTreeSet::new();
    let mut start = 0;
    for (index, tree) in trees.iter().enumerate() {
        if tree.is_ident("enum")
            && let [
                Tree::Ident(_, _),
                Tree::Group(Delimiter::Brace, body, _),
                ..,
            ] = &trees[index + 1..]
        {
            let attrs = syntax::attributes(&trees[start..index]);
            let command_enum = attrs.iter().any(|(attr, _)| {
                matches!(attr, [derive, Tree::Group(Delimiter::Parenthesis, args, _)]
                    if derive.is_ident("derive") && args.iter().any(|arg|
                        arg.is_ident("Parser") || arg.is_ident("Subcommand")))
            });
            if command_enum {
                lines.extend(
                    attrs
                        .into_iter()
                        .chain(syntax::attributes(body))
                        .filter(|(attr, _)| attr.first().is_some_and(|item| item.is_ident("doc")))
                        .map(|(_, line)| line),
                );
            }
        }
        if tree.is_punct(';') || matches!(tree, Tree::Group(Delimiter::Brace, _, _)) {
            start = index + 1;
        }
    }
    lines
}

fn lexical_comments(text: &str, code: Code, file: &str) -> Result<Vec<Comment>, String> {
    let clap = if file == "xtask/src/lib.rs" {
        clap_doc_lines(text)
    } else {
        BTreeSet::new()
    };
    let literals = if code == Code::Rust {
        let tokens = text.parse().map_err(|error| format!("{file}: {error}"))?;
        let mut literals = BTreeMap::new();
        rust_literals(text, tokens, &mut literals);
        literals
    } else {
        BTreeMap::new()
    };
    let mut rest = text;
    let mut line = 1;
    let mut column = 0;
    let mut comments = Vec::new();
    while let Some(first) = rest.chars().next() {
        let start = text.len() - rest.len();
        let prefix = &text[start - column..start];
        let end_of_line = || rest.find('\n').unwrap_or(rest.len());
        let comment_end = match code {
            _ if line == 1 && column == 0 && shebang(rest) => Some(end_of_line()),
            Code::Rust if rest.starts_with("//") => Some(end_of_line()),
            Code::Rust if rest.starts_with("/*") => Some(rust_block_end(rest)),
            Code::Rust | Code::Python => None,
            _ if first == '#' && hash_starts_comment(code, prefix) => Some(end_of_line()),
            _ => None,
        };
        let width = if let Some(end) = comment_end {
            let value = &rest[..end];
            let clap_reads = clap.contains(&line)
                && ((value.starts_with("///") && !value.starts_with("////"))
                    || (value.starts_with("/**") && !value.starts_with("/***")));
            comments.extend(value.lines().enumerate().map(|(offset, text)| Comment {
                line: line + offset,
                column: if offset == 0 { column } else { 0 },
                text: text.to_owned(),
                read_by_program: clap_reads,
            }));
            end
        } else {
            let literal = if code == Code::Rust {
                literals.get(&start).copied()
            } else if code == Code::Dockerfile {
                None
            } else {
                quoted_width(rest, code, prefix)
            };
            literal.unwrap_or(first.len_utf8())
        };
        let consumed = &rest[..width];
        if let Some((_, after_newline)) = consumed.rsplit_once('\n') {
            line += consumed.bytes().filter(|byte| *byte == b'\n').count();
            column = after_newline.len();
        } else {
            column += width;
        }
        rest = &rest[width..];
    }
    Ok(comments)
}

fn rust_literals(text: &str, tokens: TokenStream, literals: &mut BTreeMap<usize, usize>) {
    for token in tokens {
        match token {
            TokenTree::Literal(literal) => {
                let span = literal.span().byte_range();
                if !text[span.clone()].starts_with('/') {
                    literals.insert(span.start, span.len());
                }
            }
            TokenTree::Group(group) => rust_literals(text, group.stream(), literals),
            _ => (),
        }
    }
}

fn hash_starts_comment(code: Code, prefix: &str) -> bool {
    match code {
        Code::Toml => true,
        Code::Dockerfile => prefix.trim().is_empty(),
        Code::Shell => {
            prefix.is_empty() || prefix.ends_with([' ', '\t', ';', '|', '&', '(', ')', '<', '>'])
        }
        _ => prefix.is_empty() || prefix.ends_with([' ', '\t']),
    }
}

fn quoted_width(rest: &str, code: Code, prefix: &str) -> Option<usize> {
    let first = rest.as_bytes()[0];
    if code == Code::Shell && first == b'\\' {
        return Some(1 + rest[1..].chars().next().map_or(0, char::len_utf8));
    }
    if !matches!(first, b'\'' | b'"')
        || (code == Code::Yaml
            && prefix.ends_with(|c: char| !c.is_whitespace() && !"[{,:".contains(c)))
    {
        return None;
    }
    let bytes = rest.as_bytes();
    let width = if code == Code::Toml && bytes.get(1..3) == Some(&[first; 2]) {
        3
    } else {
        1
    };
    let quoted = &[first; 3][..width];
    let mut index = quoted.len();
    while let Some(&byte) = bytes.get(index) {
        if byte == b'\\' && first == b'"' {
            index += 2;
        } else if bytes[index..].starts_with(quoted) {
            if code == Code::Yaml && first == b'\'' && bytes.get(index + 1) == Some(&b'\'') {
                index += 2;
            } else {
                let end = if width == 3 {
                    index
                        + bytes[index..]
                            .iter()
                            .take_while(|byte| **byte == first)
                            .count()
                } else {
                    index + width
                };
                return Some(end);
            }
        } else {
            index += 1;
        }
    }
    Some(rest.len())
}

fn rust_block_end(rest: &str) -> usize {
    let mut pairs = rest.as_bytes()[2..].windows(2).enumerate();
    let mut depth = 1;
    while let Some((index, pair)) = pairs.next() {
        match pair {
            b"/*" => depth += 1,
            b"*/" => depth -= 1,
            _ => continue,
        }
        if depth == 0 {
            return index + 4;
        }
        pairs.next();
    }
    rest.len()
}

const PYTHON_COMMENTS: &str = r#"
import ast
import io
import json
import sys
import tokenize

with tokenize.open(sys.argv[1]) as file:
    source = file.read()

def comment(line, column, text, read_by_program=False):
    return dict(line=line, column=column, text=text, read_by_program=read_by_program)

comments = [
    comment(t.start[0], len(t.line[:t.start[1]].encode()), t.string)
    for t in tokenize.generate_tokens(io.StringIO(source).readline)
    if t.type == tokenize.COMMENT
]
for node in ast.walk(ast.parse(source)):
    if not isinstance(node, (ast.Module, ast.ClassDef, ast.FunctionDef, ast.AsyncFunctionDef)):
        continue
    if ast.get_docstring(node, clean=False) is None:
        continue
    tool = isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and any(
        isinstance(d, ast.Call) and not d.args and not d.keywords
        and isinstance(d.func, ast.Attribute) and d.func.attr == "tool"
        and isinstance(d.func.value, ast.Name) and d.func.value.id == "server"
        for d in node.decorator_list
    )
    expr = node.body[0]
    comments.extend(
        comment(expr.lineno + i, 0 if i else expr.col_offset, text, tool)
        for i, text in enumerate(ast.get_source_segment(source, expr).splitlines())
    )
print(json.dumps(comments))
"#;

fn python_comments(root: &Path, file: &str) -> Result<Vec<Comment>, String> {
    let out = Command::new("python3")
        .args(["-I", "-c", PYTHON_COMMENTS, file])
        .current_dir(root)
        .output()
        .map_err(|error| format!("{file}: python3: {error}"))?;
    if !out.status.success() {
        return Err(format!(
            "{file}: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    serde_json::from_slice(&out.stdout).map_err(|error| format!("{file}: {error}"))
}
