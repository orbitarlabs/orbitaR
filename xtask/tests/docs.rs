use crate::{Tree, git, has, lever, tree, write};

const GATES: &str = "## Gate 0\n- [ ] a\n## Gate 1\n- [ ] a\n## Gate 2\n- [ ] a\n## Gate 3\n- [ ] a\n## Gate 4\n- [ ] a\n## Gate 5\n- [ ] a\n## Gate 6\n- [ ] a\n";
const STACK: &str = "# I\n```text\nmodel/ xc/ pw/ paw/ ks/ io/\n```\n";
const README: &str = "# R\n\n## Run\n\n```sh\norbitar run input.toml\n```\n";

const LOCAL: &str = "# mine\n<!-- local docs: scratch/local/docs.toml -->\n@scratch/local/notes.md\n@scratch/local/stack.md\n@scratch/local/gated.md\n";
const MANIFEST: &str = "[[file]]\npath = \"notes.md\"\ncap = 27\n\n[[file]]\npath = \"stack.md\"\ncap = 98\nlayers = true\n\n[[file]]\npath = \"gated.md\"\ncap = 127\nheadings = [\"Gate 0\", \"Gate 1\", \"Gate 2\", \"Gate 3\", \"Gate 4\", \"Gate 5\", \"Gate 6\"]\n";

fn docs(name: &str, agents: &str, gated: &str, stack: &str, extra: &[(&str, &str)]) -> Vec<String> {
    lever(&["docs"], &docs_tree(name, agents, gated, stack, extra))
}

fn tracked(root: &Tree) -> &Tree {
    git(root, &["init", "-q"]);
    git(root, &["add", "-A"]);
    root
}

fn readme_findings(name: &str, lines: &[&str], extra: &[(&str, &str)]) -> Vec<String> {
    let readme = format!("{README}\n{}\n", lines.join("\n"));
    let mut files = vec![("README.md", readme.as_str())];
    files.extend_from_slice(extra);
    let root = docs_tree(name, "# A\n", &format!("# R\n{GATES}"), STACK, &files);
    lever(&["docs"], tracked(&root))
}

fn readme_line(n: usize, what: &str) -> String {
    format!("docs: README.md:{}: {what}", README.lines().count() + 2 + n)
}

fn docs_tree(name: &str, agents: &str, gated: &str, stack: &str, extra: &[(&str, &str)]) -> Tree {
    let mut files = vec![
        ("AGENTS.md", agents),
        ("CLAUDE.local.md", LOCAL),
        ("scratch/local/docs.toml", MANIFEST),
        ("scratch/local/notes.md", "# M\n"),
        ("scratch/local/gated.md", gated),
        ("scratch/local/stack.md", stack),
        ("CLAUDE.md", "@AGENTS.md\n"),
        ("README.md", README),
        ("container/README.md", "# C\n"),
        ("SECURITY.md", "# S\n"),
        ("CONTRIBUTING.md", "# C\n"),
        ("tests/main.rs", "readme_block(\"Run\")\n"),
    ];
    for layer in ["model", "xc", "pw", "paw", "ks", "io"] {
        files.push((Box::leak(format!("src/{layer}/f.rs").into_boxed_str()), ""));
    }
    files.extend_from_slice(extra);
    tree(name, &files)
}

#[test]
fn clean_docs_pass() {
    let f = docs(
        "docs-clean",
        "# A\n- Run `cargo xtask smoke`.\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[],
    );
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn contributing_md_is_a_doc_file_under_the_prose_rules() {
    let review = format!("# R\n{GATES}");
    let paragraph = [("CONTRIBUTING.md", "# C\nA paragraph line.\n")];
    let f = docs("docs-contributing", "# A\n", &review, STACK, &paragraph);
    assert!(has(&f, "CONTRIBUTING.md:2: paragraph line"), "{f:?}");
    assert!(!has(&f, "CONTRIBUTING.md is extra prose"), "{f:?}");
    let long = "# C\n".to_owned() + &"- Run `cargo xtask help`.\n".repeat(60);
    let f = docs(
        "docs-contributing-long",
        "# A\n",
        &review,
        STACK,
        &[("CONTRIBUTING.md", &long)],
    );
    assert!(has(&f, "CONTRIBUTING.md has 61 lines >"), "{f:?}");
}

#[test]
fn prose_rules_flag_each_violation() {
    let long = "- one two three four five six seven eight nine ten eleven twelve thirteen fourteen fifteen sixteen seventeen eighteen nineteen twenty twentyone.";
    let agents = format!(
        "# T\nA paragraph line.\n- Run the case, then read the record.\n- Write the test case.\n- Turn the knob.\n{long}\n- The record is kept here.\n- Each case is written once.\n"
    );
    let f = docs("docs-style", &agents, &format!("# R\n{GATES}"), STACK, &[]);
    let wide = format!("```text\n{}\n```\n", "x".repeat(121));
    assert!(has(
        &docs(
            "docs-wide",
            &format!("# A\n{wide}"),
            &format!("# R\n{GATES}"),
            STACK,
            &[]
        ),
        "AGENTS.md:3: 121 characters > 120"
    ));
    for needle in [
        "paragraph",
        "one action",
        "write `case`",
        "write `setting`",
        "21 words",
        "read as passive",
    ] {
        assert!(has(&f, needle), "missing {needle}: {f:?}");
    }
}

#[test]
fn every_listed_heading_needs_its_line_and_a_checkbox() {
    let review = GATES
        .replace("## Gate 6\n- [ ] a\n", "")
        .replace("## Gate 2\n- [ ] a\n", "## Gate 2\n- a\n");
    let f = docs("docs-gates", "# A\n", &format!("# R\n{review}"), STACK, &[]);
    assert!(has(&f, "lacks the heading `## Gate 6`"));
    assert!(has(&f, "`## Gate 2` has no `- [ ]` item"));
}

#[test]
fn a_prose_file_may_not_outgrow_its_cap() {
    let agents = format!("# A\n{}", "- Run `cargo xtask smoke`.\n".repeat(40));
    let f = docs("docs-cap", &agents, &format!("# R\n{GATES}"), STACK, &[]);
    assert!(has(&f, "AGENTS.md has 41 lines >"), "{f:?}");
}

#[test]
fn security_md_is_prose_of_at_most_four_lines() {
    let agents = "# A\n- Run `cargo xtask smoke`.\n";
    let gates = format!("# R\n{GATES}");
    let f = docs("docs-security", agents, &gates, STACK, &[]);
    assert!(!has(&f, "SECURITY.md"), "{f:?}");
    let long = [("SECURITY.md", "# S\n\n- a.\n- b.\n- c.\n")];
    let f = docs("docs-security-long", agents, &gates, STACK, &long);
    assert!(has(&f, "SECURITY.md has 5 lines > 4"), "{f:?}");
}

#[test]
fn listed_headings_appear_in_order() {
    let review = GATES.replace(
        "## Gate 0\n- [ ] a\n## Gate 1\n- [ ] a\n",
        "## Gate 1\n- [ ] a\n## Gate 0\n- [ ] a\n",
    );
    let f = docs("docs-order", "# A\n", &format!("# R\n{review}"), STACK, &[]);
    assert!(has(&f, "lists `## Gate 1` out of order"), "{f:?}");
}

#[test]
fn a_layered_local_file_must_place_every_layer() {
    let f = docs(
        "docs-layers",
        "# A\n",
        &format!("# R\n{GATES}"),
        "# I\n```text\nks/ io/\n```\n",
        &[],
    );
    assert!(has(&f, "layer `xc/`"));
}

#[test]
fn docs_may_name_a_layer_before_its_first_file() {
    let root = tree(
        "docs-layer-declared",
        &[
            (
                "AGENTS.md",
                "# A\n- Run `cargo xtask smoke`.\n- Keep `model/ fft/` apart.\n",
            ),
            ("CLAUDE.md", "@AGENTS.md\n"),
        ],
    );
    let f = lever(&["docs"], &root);
    for layer in ["model", "xc", "pw", "paw", "ks", "io"] {
        assert!(!has(&f, &format!("`{layer}/`")), "{layer}: {f:?}");
    }
    assert!(
        has(&f, "AGENTS.md names `fft/`, which does not exist"),
        "{f:?}"
    );
}

#[test]
fn names_must_exist_and_extra_prose_is_rejected() {
    let agents =
        "# A\n- Edit `src/pw/basis.rs` and `src/<dir>/<file>.rs`.\n```sh\ncargo xtask gate\n```\n";
    let extra = [
        ("docs/notes.md", "# N\n"),
        ("docs/design.html", "<h1>N</h1>\n"),
        ("docs/plan.org", "* N\n"),
        ("src/NOTES", "n\n"),
        ("CLAUDE.md", "read AGENTS.md\n"),
    ];
    let f = docs(
        "docs-names",
        agents,
        &format!("# R\n{GATES}"),
        STACK,
        &extra,
    );
    assert!(has(&f, "names `src/pw/basis.rs`, which does not exist"));
    assert!(!has(&f, "src/<dir>"));
    assert!(has(&f, "`cargo xtask gate`, which xtask does not provide"));
    for file in [
        "docs/notes.md",
        "docs/design.html",
        "docs/plan.org",
        "src/NOTES",
    ] {
        assert!(has(&f, &format!("{file} is extra prose")), "{file}: {f:?}");
    }
    assert!(has(&f, "CLAUDE.md must be exactly"));
}

#[test]
fn readme_code_needs_a_test_that_proves_it_and_a_diagram_needs_none() {
    let readme = format!(
        "{README}\nA paragraph.\n\n## Rust\n\n```rust\nlet x = 1;\n```\n\n## Socket\n\n```python\nserver.run()\n```\n\n## Held\n\n```rust\nlet y = 2;\n```\n\n## Named\n\n```sh\nnamed\n```\n\n## Map\n\n```text\na ─▶ b\n```\n\n## Flow\n\n```mermaid\ngraph LR\n```\n\n## Bare\n\n```\nbare\n```\n"
    );
    let root = tree(
        "docs-readme",
        &[
            ("AGENTS.md", "# A\n"),
            ("CLAUDE.md", "@AGENTS.md\n"),
            ("README.md", &readme),
            ("tests/main.rs", "readme_block(\"Run\")\n"),
            ("tests/notes.txt", "readme_block(\"Socket\")\n"),
            ("tests/lib.rs", "fn f() {\n    let y = 2;\n}\n"),
            ("xtask/tests/named.rs", "readme_block(\"Named\")\n"),
        ],
    );
    let f = lever(&["docs"], &root);
    let unproved = |n: usize, heading: &str| {
        format!(
            "README.md:{n}: no test under tests/ or xtask/tests/ names readme_block(\"{heading}\")"
        )
    };
    let readme: Vec<&String> = (f.iter()).filter(|x| x.contains("README.md:")).collect();
    assert_eq!(
        readme,
        [
            format!("docs: {}", unproved(14, "Rust")),
            format!("docs: {}", unproved(20, "Socket")),
            format!("docs: {}", unproved(50, "Bare")),
        ]
        .map(|finding| finding + " or holds this example")
        .iter()
        .collect::<Vec<_>>()
    );
}

#[test]
fn a_doc_file_may_not_be_a_symlink() {
    let root = tree(
        "docs-link",
        &[
            ("AGENTS.md", "# A\n"),
            ("CLAUDE.md", "@AGENTS.md\n"),
            ("tests/main.rs", ""),
        ],
    );
    std::os::unix::fs::symlink("AGENTS.md", root.join("README.md")).unwrap();
    let findings = lever(&["docs"], &root);
    assert!(
        has(&findings, "docs: README.md is a symlink"),
        "{findings:?}"
    );
}

#[test]
fn a_doc_at_its_cap_passes_and_one_line_more_fails() {
    let lines = |n: usize| format!("# A\n{}", "- Run `cargo xtask smoke`.\n".repeat(n - 1));
    let review = format!("# R\n{GATES}");
    let at = docs("docs-cap-at", &lines(22), &review, STACK, &[]);
    assert!(!has(&at, "AGENTS.md has"), "{at:?}");
    let over = docs("docs-cap-over", &lines(23), &review, STACK, &[]);
    assert!(has(&over, "AGENTS.md has 23 lines > 22"), "{over:?}");
}

#[test]
fn only_the_readme_may_name_a_file_of_the_user() {
    let agents = "# A\n- Read `outcome.json`.\n";
    let readme = "# R\n\n## Run\n\n```sh\norbitar run input.toml > outcome.json\n```\n";
    let f = docs(
        "docs-user-files",
        agents,
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", readme)],
    );
    assert!(
        has(&f, "AGENTS.md names `outcome.json`, which does not exist"),
        "{f:?}"
    );
    assert!(!has(&f, "README.md names"), "{f:?}");
}

#[test]
fn a_doc_may_name_a_root_file_or_an_ignored_path() {
    let agents = "# A\n- Edit `deny.toml`.\n- Keep `scratch/` private.\n";
    let f = docs(
        "docs-root-file",
        agents,
        &format!("# R\n{GATES}"),
        STACK,
        &[("deny.toml", ""), (".gitignore", "/target/\n/scratch/\n")],
    );
    assert!(f.is_empty(), "{f:?}");
    let f = docs(
        "docs-not-ignored",
        "# A\n- Keep `gone/` private.\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[(".gitignore", "/target/\n/scratch/\n")],
    );
    assert!(has(&f, "names `gone/`, which does not exist"), "{f:?}");
}

#[test]
fn boundaries_of_the_docs_rules() {
    let exact = format!("# A\n```text\n{}\n```\n", "x".repeat(120));
    let f = docs("docs-bounds", &exact, GATES, STACK, &[]);
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn a_readme_example_needs_a_rust_test_under_tests_that_names_or_holds_it() {
    let review = format!("# R\n{GATES}");
    let cases = [
        ("tests/main.rs", "readme_block(\"Run\")\n", true),
        ("xtask/tests/run.rs", "readme_block(\"Run\")\n", true),
        (
            "tests/io/spec.rs",
            "fn f() {\n    orbitar run input.toml\n}\n",
            true,
        ),
        ("tests/main.rs", "readme_block(\"Other\")\n", false),
        ("tests/notes.txt", "readme_block(\"Run\")\n", false),
        ("src/main.rs", "readme_block(\"Run\")\n", false),
    ];
    for (n, (path, body, proves)) in cases.into_iter().enumerate() {
        let extra = [("tests/main.rs", ""), (path, body)];
        let f = docs(&format!("docs-proof-{n}"), "# A\n", &review, STACK, &extra);
        let found = has(
            &f,
            "README.md:6: no test under tests/ or xtask/tests/ names readme_block(\"Run\")",
        );
        assert_eq!(found, !proves, "{path}: {f:?}");
    }
}

#[test]
fn sentence_rules_count_words_actions_and_passives_at_their_bounds() {
    let review = format!("# R\n{GATES}");
    let words = |n: usize| {
        (1..=n)
            .map(|i| format!("w{i}"))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let fine = format!(
        "# A\n- Run {}.\n- Read {}. Keep {}.\n- Run `{}` now.\n- Version 1.5 runs.\n",
        words(19),
        words(14),
        words(14),
        words(25)
    );
    let f = docs("docs-sentence-fine", &fine, &review, STACK, &[]);
    assert!(f.is_empty(), "{f:?}");
    let chained = "# A\n- Run the case and then read the record.\n";
    let f = docs("docs-and-then", chained, &review, STACK, &[]);
    assert!(has(&f, "one action per sentence"), "{f:?}");
    let share = |passives: usize| {
        let mut lines = vec!["# A".to_owned()];
        for i in 0..4 {
            lines.push(if i < passives {
                format!("- The record {i} is kept.")
            } else {
                format!("- Read record {i}.")
            });
        }
        lines.join("\n") + "\n"
    };
    let f = docs("docs-passive-1", &share(1), &review, STACK, &[]);
    assert!(!has(&f, "read as passive"), "{f:?}");
    let f = docs("docs-passive-2", &share(2), &review, STACK, &[]);
    assert!(has(&f, "2/5 sentences read as passive"), "{f:?}");
}

#[test]
fn passive_needs_a_be_verb_and_a_participle() {
    let review = format!("# R\n{GATES}");
    let reads_passive = |n: usize, sentence: &str| {
        let f = docs(
            &format!("docs-passive-word-{n}"),
            &format!("- {sentence}\n"),
            &review,
            STACK,
            &[],
        );
        has(&f, "read as passive")
    };
    for (n, (sentence, passive)) in [
        ("The record is tested.", true),
        ("The record is written.", true),
        ("The record is kept.", true),
        ("The case is good.", false),
        ("The case is red.", false),
        ("Read the logged values.", false),
        ("Run the tested case.", false),
    ]
    .into_iter()
    .enumerate()
    {
        assert_eq!(reads_passive(n, sentence), passive, "{sentence}");
    }
}

#[test]
fn flags_and_patterns_are_not_paths() {
    let agents = "# A\n- Pass `--out-dir/` and `<case>/input.toml` and `https://example.org/x.json`.\n- Read `missing/`, not `a/`.\n";
    let f = docs(
        "docs-patterns",
        agents,
        &format!("# R\n{GATES}"),
        STACK,
        &[],
    );
    assert!(has(&f, "names `missing/`, which does not exist"), "{f:?}");
    assert!(
        !has(&f, "--out-dir") && !has(&f, "<case>") && !has(&f, "example.org"),
        "{f:?}"
    );
    assert!(
        !has(&f, "`a/`"),
        "two characters are too short for a path: {f:?}"
    );
}

#[test]
fn only_words_after_cargo_xtask_name_a_command() {
    let agents = "# A\n\n```sh\ncargo build --release\ncargo xtask smoke; cargo xtask check\ncargo xtask …\n```\n";
    let f = docs(
        "docs-commands",
        agents,
        &format!("# R\n{GATES}"),
        STACK,
        &[],
    );
    assert!(f.is_empty(), "{f:?}");
    let typo = "# A\n- Run `cargo xtask smok`.\n";
    let f = docs(
        "docs-command-typo",
        typo,
        &format!("# R\n{GATES}"),
        STACK,
        &[],
    );
    assert!(
        has(&f, "names `cargo xtask smok`, which xtask does not provide"),
        "{f:?}"
    );
}

#[test]
fn a_heading_holds_one_block() {
    let readme = format!(
        "{README}\n## Outcome\n\n```jsonc\n{{}}\n```\n\n```text\na ─▶ b\n```\n\n# A long heading is the owner's prose and passes\n"
    );
    let extra = [
        ("README.md", readme.as_str()),
        (
            "tests/main.rs",
            "readme_block(\"Run\")\nreadme_block(\"Outcome\")\n",
        ),
    ];
    let f = docs(
        "docs-readme-one",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &extra,
    );
    assert_eq!(
        f,
        ["docs: README.md:16: `## Outcome` holds one block; give this one its own heading"]
    );
}

#[test]
fn readme_prose_escapes_the_sentence_rules_but_not_the_line_length_or_the_cap() {
    let prose = "**Bold words.**\n\nA paragraph that is written in the passive voice, then turns the knob\n\
                 of a test case with far more than twenty words in its one long sentence.\n\n- A bullet of prose.\n";
    let readme = format!("{README}\n{prose}");
    let f = docs(
        "docs-readme-prose",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", &readme)],
    );
    assert!(f.is_empty(), "{f:?}");
    let wide = format!("{README}\n{}\n", "x".repeat(121));
    let f = docs(
        "docs-readme-wide",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", &wide)],
    );
    assert_eq!(
        f,
        ["docs: README.md:9: 121 characters > 120; shorten the line"]
    );
    let lines = |n: usize| format!("{README}{}", "- prose\n".repeat(n - README.lines().count()));
    let at = docs(
        "docs-readme-cap-at",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", &lines(126))],
    );
    assert!(at.is_empty(), "{at:?}");
    let over = docs(
        "docs-readme-cap-over",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", &lines(127))],
    );
    assert_eq!(over, ["docs: README.md has 127 lines > 126"]);
}

#[test]
fn a_readme_link_names_a_file_of_the_tree() {
    let lines = [
        "- [License](LICENSE): kept",
        "- [Cases](benchmarks/): a directory",
        "- `[code](gone)` is code, no link",
        "- [Gone](packages/gone/LICENSE)",
        "- [Site](https://example.com)",
        "- [Up](../LICENSE)",
        "- [Absolute](/etc/hosts)",
        "![Chart](assets/missing.svg)",
        "- An autolink: <https://example.com/x>",
    ];
    let f = readme_findings(
        "docs-readme-links",
        &lines,
        &[("LICENSE", "x\n"), ("benchmarks/x.toml", "")],
    );
    let gone = |n: usize, target: &str| {
        readme_line(n, &format!("the link `{target}` names no file of the tree"))
    };
    assert_eq!(
        f,
        [
            readme_line(8, "put HTML on its own line, from the allowlist"),
            gone(3, "packages/gone/LICENSE"),
            gone(4, "https://example.com"),
            gone(5, "../LICENSE"),
            gone(6, "/etc/hosts"),
            gone(7, "assets/missing.svg"),
            gone(8, "https://example.com/x"),
        ]
    );
}

#[test]
fn a_readme_link_or_picture_to_a_file_outside_the_public_tree_is_refused() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n";
    let lines = [
        "- [Tracked](LICENSE)",
        "- [Untracked](NOTES)",
        "- [Ignored](private/plan.toml)",
        "- [Ignored directory](private/)",
        "<img src=\"assets/tracked.svg\" alt=\"Logo\">",
        "<img src=\"assets/untracked.svg\" alt=\"Logo\">",
        "<a href=\"benchmarks/README.md\">",
        "</a>",
        "",
        "- [Fixture](probe*.bin)",
        "- [Odd](odd*name.txt)",
        "- [Dot](benchmarks/./README.md) and [Double](benchmarks//README.md)",
        "- [Web](https://example.com) and [Mail](ftp:LICENSE); [Colon](a_b:notes.txt) and [Digit](1abc:notes.txt) pass",
    ];
    let readme = format!("{README}\n{}\n", lines.join("\n"));
    let root = docs_tree(
        "docs-readme-public",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[
            ("README.md", &readme),
            ("LICENSE", "x\n"),
            ("private/plan.toml", ""),
            (".gitattributes", "private/ export-ignore\n"),
            ("assets/tracked.svg", svg),
            ("benchmarks/README.md", "# B\n"),
            ("probe-tracked.bin", ""),
            ("odd*name.txt", ""),
            ("https:/example.com", ""),
            ("ftp:LICENSE", ""),
            ("a_b:notes.txt", ""),
            ("1abc:notes.txt", ""),
        ],
    );
    tracked(&root);
    write(&root, "NOTES", "x\n");
    write(&root, "probe*.bin", "");
    write(&root, "assets/untracked.svg", svg);
    let html = "HTML comes from the allowlist only: a picture under assets/, <details>, <summary>";
    let gone = |n: usize, target: &str| {
        readme_line(n, &format!("the link `{target}` names no file of the tree"))
    };
    let readme_only = |f: Vec<String>| -> Vec<String> {
        f.into_iter().filter(|x| x.contains("README.md:")).collect()
    };
    assert_eq!(
        readme_only(lever(&["docs"], &root)),
        [
            readme_line(5, html),
            gone(1, "NOTES"),
            gone(2, "private/plan.toml"),
            gone(3, "private/"),
            gone(9, "probe*.bin"),
            gone(12, "ftp:LICENSE"),
            gone(12, "https://example.com"),
        ]
    );
    git(&root, &["rm", "-q", "--cached", "benchmarks/README.md"]);
    assert_eq!(
        readme_only(lever(&["docs"], &root)),
        [
            readme_line(5, html),
            readme_line(6, html),
            gone(1, "NOTES"),
            gone(2, "private/plan.toml"),
            gone(3, "private/"),
            gone(9, "probe*.bin"),
            gone(11, "benchmarks/./README.md"),
            gone(11, "benchmarks//README.md"),
            gone(12, "ftp:LICENSE"),
            gone(12, "https://example.com"),
        ]
    );
}

#[test]
fn readme_html_that_the_parser_sees_must_stand_on_its_own_line_whatever_the_backticks() {
    let f = readme_findings(
        "docs-readme-backtick",
        &[
            "- An unmatched ` and then <a href=\"https://example.com\">a site</a>",
            "- Code `<a href=\"https://example.com\">` is no tag.",
        ],
        &[],
    );
    assert_eq!(
        f,
        [readme_line(
            0,
            "put HTML on its own line, from the allowlist"
        )]
    );
}

#[test]
fn any_web_or_mail_address_outside_code_is_a_link_out_of_the_tree() {
    let at = |text: &str| text.replace('#', "@");
    let lines = [
        "- See https://example.com/a for more.".to_owned(),
        "- Or (www.example.com) and *HTTP://EXAMPLE.COM*.".to_owned(),
        "- See(https://example.org/) for details.".to_owned(),
        at("- Mail someone#example.org or (ops#example.org)."),
        "- [https://example.com/text](LICENSE) is a link with an address in its text.".to_owned(),
        at("- Write(name#example.org) now, or name#example.net; more text."),
        at(
            "- Code `https://example.com`, `www.example.com`, and `a#example.org` pass; so does [text](LICENSE).",
        ),
        at("- A handle #name, a#b, #example.org, a#example.c, and a#host.c0 are no address."),
        String::new(),
        "<a href=\"benchmarks/README.md\">".to_owned(),
        "Results at https://example.org/results".to_owned(),
        "</a>".to_owned(),
        String::new(),
        "```text".to_owned(),
        at("https://example.com/in/a/block and a#example.org"),
        "```".to_owned(),
    ];
    let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
    let f = readme_findings(
        "docs-readme-addresses",
        &lines,
        &[("LICENSE", "x\n"), ("benchmarks/README.md", "# B\n")],
    );
    let gone = |n: usize, target: &str| {
        readme_line(
            n,
            &format!("the link `{}` names no file of the tree", at(target)),
        )
    };
    let links: Vec<String> = f.into_iter().filter(|x| x.contains("the link")).collect();
    assert_eq!(
        links,
        [
            gone(0, "https://example.com/a"),
            gone(1, "HTTP://EXAMPLE.COM"),
            gone(1, "www.example.com"),
            gone(2, "See(https://example.org/"),
            gone(3, "ops#example.org"),
            gone(3, "someone#example.org"),
            gone(4, "https://example.com/text"),
            gone(5, "name#example.net"),
            gone(5, "name#example.org"),
            gone(10, "https://example.org/results"),
        ]
    );
}

#[test]
fn a_tag_on_a_later_line_of_an_html_block_is_found_on_its_line() {
    let f = readme_findings(
        "docs-readme-html-block",
        &[
            "<details>",
            "<summary>Every key</summary>",
            "An unmatched ` and then <b>a tag</b>",
            "</details>",
        ],
        &[],
    );
    let readme: Vec<&String> = f.iter().filter(|x| x.contains("README.md:")).collect();
    assert_eq!(
        readme,
        [&readme_line(
            2,
            "put HTML on its own line, from the allowlist"
        )]
    );
}

#[test]
fn an_allowlisted_anchor_around_plain_text_passes() {
    let f = readme_findings(
        "docs-readme-anchor",
        &[
            "<a href=\"benchmarks/README.md\">",
            "Benchmark results",
            "</a>",
            "",
            "- A plain line after it.",
        ],
        &[("benchmarks/README.md", "# B\n")],
    );
    let readme: Vec<&String> = f.iter().filter(|x| x.contains("README.md:")).collect();
    assert!(readme.is_empty(), "{readme:?}");
}

#[test]
fn prose_hides_in_no_other_file_kind() {
    let extra = [
        ("docs/plan.qmd", "x\n"),
        ("HISTORY", "x\n"),
        ("src/ks/history.rs", ""),
    ];
    let f = docs(
        "docs-kinds",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &extra,
    );
    assert!(has(&f, "docs/plan.qmd is extra prose"), "{f:?}");
    assert!(has(&f, "HISTORY is extra prose"), "{f:?}");
    assert!(!has(&f, "history.rs"), "{f:?}");
}

#[test]
fn a_public_checkout_passes_without_local_docs() {
    let public = [
        ("AGENTS.md", "# A\n- Run `cargo xtask smoke`.\n"),
        ("CLAUDE.md", "@AGENTS.md\n"),
        ("README.md", README),
        ("container/README.md", "# C\n"),
        ("SECURITY.md", "# S\n"),
        ("CONTRIBUTING.md", "# C\n"),
        ("tests/main.rs", "readme_block(\"Run\")\n"),
    ];
    let f = lever(&["docs"], &tree("docs-public", &public));
    assert!(f.is_empty(), "{f:?}");
    let mut note = public.to_vec();
    let personal = ["- Prefer short answers.", "@AGENTS.md", "@notes/me.md", ""].join("\n");
    note.push(("CLAUDE.local.md", &personal));
    let f = lever(&["docs"], &tree("docs-public-note", &note));
    assert!(f.is_empty(), "{f:?}");
    let mut leaked = public.to_vec();
    leaked.push(("stack.md", STACK));
    let f = lever(&["docs"], &tree("docs-public-leak", &leaked));
    assert!(has(&f, "docs: stack.md is extra prose"), "{f:?}");
}

#[test]
fn a_declaration_needs_one_manifest_inside_the_checkout_and_an_import_per_file() {
    let review = format!("# R\n{GATES}");
    let without = LOCAL.replace("@scratch/local/stack.md\n", "");
    let f = docs(
        "docs-local-half",
        "# A\n",
        &review,
        STACK,
        &[("CLAUDE.local.md", &without)],
    );
    assert_eq!(
        f,
        ["docs: CLAUDE.local.md does not import `@scratch/local/stack.md`"]
    );
    let gone = LOCAL.replace("@scratch/local/stack.md", "@scratch/gone/stack.md");
    let gone = [
        ("CLAUDE.local.md", gone.as_str()),
        ("scratch/local/stack.md", ""),
    ];
    let f = docs("docs-local-gone", "# A\n", &review, STACK, &gone);
    assert_eq!(
        f,
        [
            "docs: CLAUDE.local.md does not import `@scratch/local/stack.md`",
            "docs: scratch/local/stack.md is missing"
        ]
    );
    for (n, manifest) in ["/etc/docs.toml", "../docs.toml", "scratch/none.toml"]
        .iter()
        .enumerate()
    {
        let local = LOCAL.replace("scratch/local/docs.toml", manifest);
        let f = docs(
            &format!("docs-local-out-{n}"),
            "# A\n",
            &review,
            STACK,
            &[("CLAUDE.local.md", &local)],
        );
        assert_eq!(
            f,
            [format!(
                "docs: CLAUDE.local.md declares `{manifest}`, which is no file inside the checkout"
            )]
        );
    }
    let out = MANIFEST.replace("path = \"notes.md\"", "path = \"../../../x.md\"");
    let f = docs(
        "docs-local-escape",
        "# A\n",
        &review,
        STACK,
        &[("scratch/local/docs.toml", &out)],
    );
    assert!(
        has(
            &f,
            "docs: scratch/local/docs.toml lists `../../../x.md`, which is outside the checkout"
        ),
        "{f:?}"
    );
    let twice = format!("{LOCAL}<!-- local docs: scratch/local/docs.toml -->\n");
    let f = docs(
        "docs-local-twice",
        "# A\n",
        &review,
        STACK,
        &[("CLAUDE.local.md", &twice)],
    );
    assert_eq!(
        f,
        ["docs: CLAUDE.local.md declares local docs more than once"]
    );
    let bad = [("scratch/local/docs.toml", "[[file]]\npath = \"notes.md\"\n")];
    let f = docs("docs-local-bad", "# A\n", &review, STACK, &bad);
    assert!(
        has(&f, "docs: scratch/local/docs.toml: ") && has(&f, "cap"),
        "{f:?}"
    );
}

#[test]
fn local_files_meet_the_prose_rules_and_the_caps_of_their_manifest() {
    let review = format!("# R\n{GATES}{}", "- [ ] a\n".repeat(128 - 15));
    let notes = [("scratch/local/notes.md", "# M\nA paragraph line.\n")];
    let f = docs("docs-local-rules", "# A\n", &review, STACK, &notes);
    assert!(
        has(&f, "docs: scratch/local/gated.md has 128 lines > 127"),
        "{f:?}"
    );
    assert!(
        has(&f, "docs: scratch/local/notes.md:2: paragraph line"),
        "{f:?}"
    );
    let raised = MANIFEST.replace("cap = 127", "cap = 128");
    let f = docs(
        "docs-local-raised",
        "# A\n",
        &review,
        STACK,
        &[("scratch/local/docs.toml", &raised)],
    );
    assert!(f.is_empty(), "{f:?}");
    let named = format!("# R\n{GATES}- [ ] Edit `src/gone.rs`.\n");
    let f = docs("docs-local-names", "# A\n", &named, STACK, &[]);
    assert!(
        has(
            &f,
            "scratch/local/gated.md names `src/gone.rs`, which does not exist"
        ),
        "{f:?}"
    );
}

#[test]
fn a_listed_row_must_be_in_the_local_file_and_equal_readmes() {
    let review = format!("# R\n{GATES}");
    let readme = format!("{README}\n## Scope\n\n```text\nlater    GPU · MPI\n```\n");
    let manifest = MANIFEST.replace("layers = true", "layers = true\nrows = [\"later\"]");
    let extra = |readme: &str| -> Vec<(String, String)> {
        vec![
            ("README.md".into(), readme.to_owned()),
            (
                "tests/main.rs".into(),
                "readme_block(\"Run\")\nreadme_block(\"Scope\")\n".into(),
            ),
            ("scratch/local/docs.toml".into(), manifest.clone()),
        ]
    };
    let run = |name: &str, stack: &str, readme: &str| {
        let owned = extra(readme);
        let files: Vec<(&str, &str)> = owned
            .iter()
            .map(|(a, b)| (a.as_str(), b.as_str()))
            .collect();
        docs(name, "# A\n", &review, stack, &files)
    };
    let same = format!("{STACK}```text\nlater  GPU · MPI\n```\n");
    assert!(run("docs-row-same", &same, &readme).is_empty());
    let other = format!("{STACK}```text\nlater  GPU\n```\n");
    assert_eq!(
        run("docs-row-other", &other, &readme),
        ["docs: scratch/local/stack.md's `later` row differs from README.md's"]
    );
    assert_eq!(
        run("docs-row-gone", STACK, &readme),
        ["docs: scratch/local/stack.md has no `later` row"]
    );
    let bare = readme.replace("later    GPU · MPI\n", "x\n");
    assert_eq!(
        run("docs-row-readme", &same, &bare),
        ["docs: README.md has no `later` row for scratch/local/stack.md"]
    );
}

#[test]
fn local_files_copied_into_the_checkout_are_no_extra_prose() {
    let review = format!("# R\n{GATES}").replace("## Gate 6\n- [ ] a\n", "");
    let local = [
        (
            "CLAUDE.local.md",
            "# local\n<!-- local docs: local/docs.toml -->\n@local/notes.md\n@local/stack.md\n@local/gated.md\n",
        ),
        ("local/docs.toml", MANIFEST),
        ("local/notes.md", "# M\n"),
        ("local/stack.md", STACK),
        ("local/gated.md", &review),
        ("local/OTHER.md", "# N\n"),
    ];
    let f = docs("docs-local-copy", "# A\n", "", "", &local);
    assert_eq!(
        f,
        [
            "docs: local/OTHER.md is extra prose; put the rule in code or delete the file",
            "docs: local/gated.md lacks the heading `## Gate 6`",
        ]
    );
}

#[test]
fn a_local_path_that_links_out_of_the_checkout_is_refused() {
    let review = format!("# R\n{GATES}");
    let outside = tree("docs-local-outside", &[("docs.toml", MANIFEST)]);
    let root = tree("docs-local-linked", &[]);
    let f = {
        std::os::unix::fs::symlink(&*outside, root.join("away")).unwrap();
        let local = LOCAL.replace("scratch/local/docs.toml", "away/docs.toml");
        let files = [("CLAUDE.local.md", local.as_str())];
        for (path, body) in [
            ("AGENTS.md", "# A\n"),
            ("CLAUDE.md", "@AGENTS.md\n"),
            ("README.md", README),
            ("container/README.md", "# C\n"),
            ("SECURITY.md", "# S\n"),
            ("CONTRIBUTING.md", "# C\n"),
            ("tests/main.rs", "readme_block(\"Run\")\n"),
            ("scratch/local/gated.md", review.as_str()),
        ]
        .into_iter()
        .chain(files)
        {
            crate::write(&root, path, body);
        }
        lever(&["docs"], &root)
    };
    assert_eq!(
        f,
        ["docs: CLAUDE.local.md declares `away/docs.toml`, which is no file inside the checkout"]
    );
}

#[test]
fn readme_html_is_a_picture_under_assets_or_a_fold_one_tag_per_line() {
    let good = [
        "<p align=\"center\">",
        "  <picture>",
        "    <source media=\"(prefers-color-scheme: dark)\" srcset=\"assets/brand/logo-dark.svg\">",
        "    <img src=\"assets/brand/logo-light.svg\" alt=\"Logo\" width=\"490\">",
        "  </picture>",
        "</p>",
        "<a href=\"benchmarks/README.md\">",
        "</a>",
        "<details>",
        "<summary>Every key</summary>",
        "</details>",
    ];
    let bad = [
        "<img src=\"assets/brand/missing.svg\" alt=\"Logo\">",
        "<img src=\"assets/brand/.hidden.svg\" alt=\"Logo\">",
        "<img src=\"https://example.com/x.svg\" alt=\"Logo\">",
        "<img src=\"assets/brand/logo-light.svg\" alt=\"\">",
        "<img src=\"assets/brand/logo-light.svg\" alt=\"Logo\" width=\"wide\">",
        "<source media=\"(min-width: 1px)\" srcset=\"assets/brand/logo-dark.svg\">",
        "<a href=\"https://example.com\">",
        "<p>Some prose</p>",
        "<summary></summary>",
        "<summary><a href=\"https://example.com\">x</a></summary>",
        "<summary>Open",
        "<details open>",
    ];
    let inline = ["Some <b>bold</b> prose", "A <https://example.com> link"];
    let readme = format!(
        "{README}\n{}\n{}\n{}\n\n- Code `<b>` is no tag, nor is 1 < 2.\n",
        good.join("\n"),
        bad.join("\n"),
        inline.join("\n")
    );
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n";
    let root = tree(
        "docs-readme-picture",
        &[
            ("AGENTS.md", "# A\n"),
            ("CLAUDE.md", "@AGENTS.md\n"),
            ("README.md", &readme),
            ("assets/brand/logo-light.svg", svg),
            ("assets/brand/logo-dark.svg", svg),
            ("assets/brand/.hidden.svg", svg),
            ("benchmarks/README.md", "# B\n"),
            ("tests/main.rs", "readme_block(\"Run\")\n"),
        ],
    );
    let f = lever(&["docs"], tracked(&root));
    let first = README.lines().count() + 2;
    let refused = |n: usize| format!("docs: README.md:{n}: HTML comes from the allowlist only");
    let inline_at = |n: usize| format!("docs: README.md:{n}: put HTML on its own line");
    let mut want: Vec<String> = (first + good.len()..first + good.len() + bad.len())
        .map(refused)
        .collect();
    want.extend((0..inline.len()).map(|i| inline_at(first + good.len() + bad.len() + i)));
    let f: Vec<&String> = (f.iter()).filter(|x| x.contains("README.md:")).collect();
    assert_eq!(f.len(), want.len(), "{f:?}");
    for (got, want) in f.iter().zip(&want) {
        assert!(got.starts_with(want.as_str()), "{got} is not {want}: {f:?}");
    }
}

#[test]
fn readme_refuses_reference_links_and_their_definitions_inside_containers_too() {
    let lines = [
        "- [Inline](LICENSE)",
        "- [Full][license]",
        "- [Collapsed][]",
        "",
        "[license]: ../outside",
        "> [quoted]: ../outside",
        "",
        "- > 1. [nested]: ../outside",
        "",
        "> [split]:",
        "> ../outside",
        "",
        "- [listed]:",
        "  ../outside",
        "- [Quoted]",
        "- Code `[a][b]` and a task `[ ]` are no links.",
        "- [ ] A task: no link.",
        "- [unknown] is no link: no definition holds it.",
    ];
    let readme = format!("{README}\n{}\n", lines.join("\n"));
    let root = docs_tree(
        "docs-readme-reference-links",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", &readme), ("LICENSE", "x\n")],
    );
    let f = lever(&["docs"], tracked(&root));
    let first = README.lines().count() + 2;
    let refused = |n: usize| {
        format!(
            "docs: README.md:{}: write each link inline as [text](path); a reference link escapes the tree check",
            first + n
        )
    };
    assert_eq!(f, [1, 2, 4, 5, 7, 9, 12, 14].map(refused), "{f:?}");
}

#[test]
fn readme_refuses_an_inline_link_whose_target_is_split_over_lines() {
    let lines = [
        "- [Split](",
        "  ../outside)",
        "> ![Quoted](",
        "> assets/gone.svg)",
    ];
    let readme = format!("{README}\n{}\n", lines.join("\n"));
    let f = docs(
        "docs-readme-split-links",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[("README.md", &readme)],
    );
    let first = README.lines().count() + 2;
    assert_eq!(
        f,
        [
            format!("docs: README.md:{first}: the link `../outside` names no file of the tree"),
            format!(
                "docs: README.md:{}: the link `assets/gone.svg` names no file of the tree",
                first + 2
            ),
        ]
    );
}

#[test]
fn readme_refuses_a_picture_whose_asset_is_a_symlink_out_of_the_checkout() {
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\"/>\n";
    let readme = format!(
        "{README}\n<img src=\"assets/brand/inside.svg\" alt=\"Logo\">\n<img src=\"assets/brand/outside.svg\" alt=\"Logo\">\n"
    );
    let root = tree(
        "docs-readme-picture-symlink",
        &[
            ("AGENTS.md", "# A\n"),
            ("CLAUDE.md", "@AGENTS.md\n"),
            ("README.md", &readme),
            ("assets/brand/inside.svg", svg),
            ("tests/main.rs", "readme_block(\"Run\")\n"),
        ],
    );
    let outside = tree("docs-readme-picture-target", &[("logo.svg", svg)]);
    std::os::unix::fs::symlink(
        outside.join("logo.svg"),
        root.join("assets/brand/outside.svg"),
    )
    .unwrap();
    let first = README.lines().count() + 2;
    let f: Vec<String> = lever(&["docs"], tracked(&root))
        .into_iter()
        .filter(|x| x.contains("README.md:"))
        .collect();
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(
        f[0].starts_with(&format!(
            "docs: README.md:{}: HTML comes from the allowlist only",
            first + 1
        )),
        "{f:?}"
    );
}

#[test]
fn a_listed_local_file_that_is_a_broken_link_is_refused_not_missing() {
    let root = docs_tree(
        "docs-local-dangling",
        "# A\n",
        &format!("# R\n{GATES}"),
        STACK,
        &[],
    );
    let notes = root.join("scratch/local/notes.md");
    std::fs::remove_file(&notes).unwrap();
    std::os::unix::fs::symlink("/nonexistent/orbitar/notes.md", &notes).unwrap();
    assert_eq!(
        lever(&["docs"], &root),
        ["docs: scratch/local/docs.toml lists `notes.md`, which is outside the checkout"]
    );
}

const PIN: &str = "3d3c42e5aac5ba805825da76410c181273ba90b1";

fn comment_diff(name: &str, before: &[(&str, &str)], after: &[(&str, &str)]) -> Vec<String> {
    let root = tree(name, before);
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base", "--allow-empty"]);
    for (file, text) in after {
        write(&root, file, text);
    }
    git(&root, &["add", "."]);
    let prose = lever(&["docs"], &root);
    let mut findings = lever(&["docs", "--base", "HEAD"], &root);
    findings.retain(|finding| !prose.contains(finding));
    findings
}

fn comment_findings(findings: &[String]) -> Vec<&str> {
    findings
        .iter()
        .filter(|f| f.contains("number's source in src/SOURCE.toml"))
        .map(String::as_str)
        .collect()
}

#[test]
fn docs_base_keeps_only_program_read_comments() {
    let workflow = format!("steps:\n  - uses: actions/checkout@{PIN} # v7.0.1\n");
    let files = [
        (
            "xtask/src/lib.rs",
            "/// The commands.\n#[derive(clap::Parser)]\nenum Cli {\n    /// Run the check.\n    Check {\n        /** The source\n         * to compare. */\n        base: String,\n    },\n}\n#[derive(clap::Subcommand)]\nenum Jobs {\n    /// Run a job.\n    Job,\n}\n",
        ),
        (
            "python/tool.py",
            "@server.tool()\ndef execute():\n    \"\"\"Run the tool.\n\n    Return its output.\n    \"\"\"\n    return 1\n",
        ),
        (".github/workflows/ci.yml", &workflow),
        (
            ".cargo/mutants.toml",
            "exclude_re = [\n  # The paths produce equal results.\n  \"x\",\n]\n",
        ),
        (
            "tests/benchmarks.rs",
            "// cases: begin (generated by `cargo xtask new-case`; `cargo xtask layout` checks it)\n// cases: end\n",
        ),
        ("a.sh", "#!/bin/sh\ntrue\n"),
        ("b.py", "#!/usr/bin/env python3\nprint(1)\n"),
    ];
    let f = comment_diff("docs-comments-kept", &[], &files);
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn docs_base_refuses_unread_comments_in_each_code_format() {
    let files = [
        ("src/lib.rs", "fn f() {} // Explain f.\n"),
        ("python/a.py", "# Explain x.\nx = 1\n"),
        ("Cargo.toml", "x = 1# Explain x.\n"),
        ("a.yml", "x: 1 # Explain x.\n"),
        ("b.yaml", "x: 1 # Explain x.\n"),
        ("c.sh", "true;# Explain true.\n"),
        (
            "container/Dockerfile",
            "# Explain the image.\nFROM scratch\n",
        ),
    ];
    let f = comment_diff("docs-comments-formats", &[], &files);
    for (file, _) in files {
        assert!(has(&f, &format!("{file}:1: an added comment")), "{f:?}");
    }
    assert_eq!(comment_findings(&f).len(), files.len(), "{f:?}");
}

#[test]
fn docs_base_refuses_unread_rust_docs_and_case_marks() {
    let files = [
        (
            "xtask/src/lib.rs",
            "//! Module prose.\n/// A type.\nenum Flag { Bit }\n#[derive(clap::Parser)]\nenum Cli {\n    // The first variant.\n    Fly,\n}\n/// Outside the enum.\nfn f() {}\n",
        ),
        (
            "xtask/src/other.rs",
            "#[derive(clap::Parser)]\nenum Cli {\n    /// A variant.\n    Fly,\n}\n",
        ),
        ("src/lib.rs", "// cases: end\n"),
        (
            "tests/benchmarks.rs",
            "// cases: begin arbitrary\n// cases: ending\nfn f() {} // cases: end\n",
        ),
    ];
    let f = comment_diff("docs-comments-rust", &[], &files);
    for (file, line) in [
        ("xtask/src/lib.rs", 1),
        ("xtask/src/lib.rs", 2),
        ("xtask/src/lib.rs", 6),
        ("xtask/src/lib.rs", 9),
        ("xtask/src/other.rs", 3),
        ("src/lib.rs", 1),
        ("tests/benchmarks.rs", 1),
        ("tests/benchmarks.rs", 2),
        ("tests/benchmarks.rs", 3),
    ] {
        assert!(
            has(&f, &format!("{file}:{line}: an added comment")),
            "{f:?}"
        );
    }
    assert_eq!(comment_findings(&f).len(), 9, "{f:?}");
}

#[test]
fn docs_base_limits_clap_attributes_to_the_current_enum() {
    let body = "mod nested {\n    #[derive(clap::Parser)]\n    enum Inner { Run }\n}\nenum Ordinary {\n    /// Regular Rust docs.\n    Idle,\n}\n";
    let findings = comment_diff(
        "docs-comments-enum-attributes",
        &[],
        &[("xtask/src/lib.rs", body)],
    );
    assert!(
        has(&findings, "xtask/src/lib.rs:6: an added comment"),
        "{findings:?}"
    );
}

#[test]
fn docs_base_checks_only_docstrings_that_python_exposes() {
    let body = r#""""Module prose."""

class Test:
    """Class prose."""

@other.tool()
def f():
    """Function prose."""

@server.tool('other')
def g():
    """Extra arguments change the decorator."""

@server.tool()
async def h():
    'The genuine tool description.'
    return """String output.
    # A literal line.
    """
"#;
    let f = comment_diff("docs-comments-python", &[], &[("a.py", body)]);
    for line in [1, 4, 8, 12] {
        assert!(has(&f, &format!("a.py:{line}: an added comment")), "{f:?}");
    }
    assert_eq!(comment_findings(&f).len(), 4, "{f:?}");
}

#[test]
fn docs_base_requires_a_workflow_path_and_an_action_pin() {
    let workflow = format!(
        "steps:\n  - uses: actions/checkout@{PIN} # version\n  - uses: actions/checkout@v7.0.1 # v7.0.1\n  - uses: actions/checkout@z{} # v7.0.1\n  - uses: checkout@{PIN} # v7.0.1\n  - name: {PIN} # v7.0.1\n  - uses: actions/checkout@{PIN} # v7 and prose\n  - uses: \"actions/checkout@{PIN}\" # v7.0.1-rc.3+build\n  - uses: 'actions/checkout@{PIN}' # v7.0.1\n",
        &PIN[1..],
    );
    let wrong_path = format!("uses: actions/checkout@{PIN} # v7\n");
    let files = [
        (".github/workflows/ci.yml", workflow.as_str()),
        (".github/workflows/build.yaml", wrong_path.as_str()),
        (".github/workflows/nested/ci.yml", wrong_path.as_str()),
        ("elsewhere/ci.yml", wrong_path.as_str()),
    ];
    let f = comment_diff("docs-comments-workflow", &[], &files);
    for line in 2..=7 {
        assert!(
            has(
                &f,
                &format!(".github/workflows/ci.yml:{line}: an added comment")
            ),
            "{f:?}"
        );
    }
    for (file, _) in &files[1..] {
        assert!(has(&f, &format!("{file}:1: an added comment")), "{f:?}");
    }
    assert_eq!(comment_findings(&f).len(), 9, "{f:?}");
}

#[test]
fn docs_base_checks_task_markers_and_dates_even_in_kept_comments() {
    let workflow = format!("uses: actions/checkout@{PIN} # v7-HACK\n");
    let files = [
        (
            "xtask/src/lib.rs",
            "#[derive(clap::Parser)]\nenum Cli {\n    /// TODO ask for review.\n    Run,\n}\n",
        ),
        (
            "python/tool.py",
            "@server.tool()\ndef f():\n    \"\"\"Run it.\n    FIXME later.\n    \"\"\"\n",
        ),
        (".github/workflows/ci.yml", &workflow),
        (".cargo/mutants.toml", "# Changed 2026-11-26.\nx = 1\n"),
        ("a.sh", "#!/usr/bin/env XXX\ntrue\n"),
        ("src/script.rs", "#! /usr/bin/env HACK\nfn main() {}\n"),
    ];
    let f = comment_diff("docs-comments-task-markers", &[], &files);
    for ((file, _), line) in files.into_iter().zip([3, 4, 1, 1, 1, 1]) {
        assert!(
            has(
                &f,
                &format!("{file}:{line}: a comment contains a date or a task marker")
            ),
            "{f:?}"
        );
    }
    assert_eq!(comment_findings(&f).len(), 6, "{f:?}");
}

#[test]
fn docs_base_refuses_a_task_marker_at_the_end_of_a_clap_block_doc() {
    let findings = comment_diff(
        "docs-comments-clap-block-marker",
        &[],
        &[(
            "xtask/src/lib.rs",
            "#[derive(clap::Subcommand)]\nenum Cli {\n    /** Run the check. TODO */\n    Check,\n}\n",
        )],
    );
    assert!(
        has(
            &findings,
            "xtask/src/lib.rs:3: a comment contains a date or a task marker"
        ),
        "{findings:?}"
    );
    assert_eq!(comment_findings(&findings).len(), 1, "{findings:?}");
}

#[test]
fn docs_base_checks_added_lines_inside_an_existing_block_comment() {
    let f = comment_diff(
        "docs-comments-block",
        &[(
            "src/lib.rs",
            "fn a() {}\n/* Unchanged.\n * Nested below.\n */\nfn b() {}\n",
        )],
        &[(
            "src/lib.rs",
            "fn a() {}\n/* Unchanged.\n * Nested below.\n /* Added. */\n * Added after the nested comment.\n */\nfn b() {} // Added.\n",
        )],
    );
    for line in [4, 5, 7] {
        assert!(
            has(&f, &format!("src/lib.rs:{line}: an added comment")),
            "{f:?}"
        );
    }
    assert_eq!(comment_findings(&f).len(), 3, "{f:?}");
}

#[test]
fn docs_base_spares_strings_and_unchanged_files() {
    let old = "fn a() {} // An unchanged comment.\n";
    let files = [
        (
            "src/new.rs",
            r####"static A: &str = "https://test.invalid/*x*/";
static B: &str = r###"quote " // This line is inside the literal
/* So is this line. */"###;
static C: char = '\'';
fn f<'a, 'b>(a: &'a str, b: &'b str) {
    let _ = "https://test.invalid/nested";
}
"####,
        ),
        (
            "a.sh",
            "printf '%s\\n' '# text' escaped\\#word \"# quoted\"\n",
        ),
        (
            "Cargo.toml",
            r#"text = 'A number # in a string'
escaped = "\" # inside the string."
multi = """
A quote " # inside the multiline string.
"""
"#,
        ),
        (
            "a.yml",
            r#"link: https://test.invalid#section
text: "A hash # inside the string"
single: ''' at the start, '' in the middle, and a hash # inside the string'
"#,
        ),
        (
            "Dockerfile",
            "FROM scratch\nLABEL text=\"Inside # a label\"\n",
        ),
        ("NOTES.md", "# 2026-11-26\n"),
    ];
    let f = comment_diff("docs-comments-unchanged", &[("src/old.rs", old)], &files);
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn docs_base_refuses_misplaced_shebangs_and_mutants_inline_comments() {
    let files = [
        ("a.sh", " #!/bin/sh\n#!/usr/bin/env sh\n#!no_interpreter\n"),
        (".cargo/mutants.toml", "x = 1 # Explain x.\n"),
        ("mutants.toml", "# Explain x.\n"),
    ];
    let f = comment_diff("docs-comments-not-directives", &[], &files);
    for ((file, _), line) in files.into_iter().zip([1, 1, 1]) {
        assert!(
            has(&f, &format!("{file}:{line}: an added comment")),
            "{f:?}"
        );
    }
    for line in [2, 3] {
        assert!(has(&f, &format!("a.sh:{line}: an added comment")), "{f:?}");
    }
    assert_eq!(comment_findings(&f).len(), 5, "{f:?}");
}

#[test]
fn docs_without_base_keeps_the_prose_only_and_a_bad_base_fails() {
    let root = tree(
        "docs-comments-no-base",
        &[("src/lib.rs", "// A comment.\n")],
    );
    let f = lever(&["docs"], &root);
    assert!(comment_findings(&f).is_empty(), "{f:?}");
    git(&root, &["init", "-q"]);
    let f = lever(&["docs", "--base", "some-missing-ref"], &root);
    assert!(has(&f, "docs: fatal:"), "{f:?}");
}

#[test]
fn docs_base_skips_a_symlinked_family_table_and_checks_its_target() {
    let root = tree(
        "docs-comments-symlink",
        &[("datasets/family.toml", "x = 1\n")],
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base"]);
    std::fs::create_dir_all(root.join("python/orbitar/families")).unwrap();
    std::os::unix::fs::symlink(
        "../../../datasets/family.toml",
        root.join("python/orbitar/families/family.toml"),
    )
    .unwrap();
    write(&root, "datasets/family.toml", "x = 1 # Explain x.\n");
    git(&root, &["add", "."]);
    let f = lever(&["docs", "--base", "HEAD"], &root);
    assert!(
        !f.iter().any(|x| x.contains("python/orbitar/families")),
        "{f:?}"
    );
    assert!(has(&f, "datasets/family.toml:1: an added comment"), "{f:?}");
    assert_eq!(comment_findings(&f).len(), 1, "{f:?}");
}

#[test]
fn docs_base_counts_only_added_lines_in_a_renamed_file() {
    let text = "// Unchanged comment.\n".to_owned() + &"fn prior() {}\n".repeat(12);
    let root = tree("docs-comments-renamed", &[("src/old.rs", &text)]);
    git(&root, &["init", "-q"]);
    git(&root, &["add", "."]);
    git(&root, &["commit", "-m", "base"]);
    git(&root, &["mv", "src/old.rs", "src/new.rs"]);
    write(&root, "src/new.rs", &(text + "// Added comment.\n"));
    let f = lever(&["docs", "--base", "HEAD"], &root);
    assert!(has(&f, "src/new.rs:14: an added comment"), "{f:?}");
    assert_eq!(comment_findings(&f).len(), 1, "{f:?}");
}

#[test]
fn docs_base_finds_comments_after_escaped_and_multiline_quotes() {
    let files = [
        ("container/script.sh", "printf '%s\\n' \\\" # Shell note.\n"),
        (
            "a.toml",
            "text = \"\"\"Ends in two quotes\"\"\"\"\" # TOML note.\n",
        ),
        ("basic.toml", "text = \"A # in a string\"# TOML note.\n"),
        (
            "trailing.toml",
            "text = \"\"\"Ends in a quote\"\"\"\" # TOML note.\n",
        ),
        (
            "b.yaml",
            "text: 'don''t read # inside this quote' # YAML note.\n",
        ),
        ("c.yml", "text: don't misread an apostrophe # YAML note.\n"),
        (
            "xtask/src/lib.rs",
            "#[derive(clap::Parser)]\nenum Cli {\n    #[doc = \"Run it.\"] /* A regular comment. */\n    Run,\n}\n",
        ),
    ];
    let f = comment_diff("docs-comments-quoted", &[], &files);
    for (file, _) in &files[..files.len() - 1] {
        assert!(has(&f, &format!("{file}:1: an added comment")), "{f:?}");
    }
    assert!(has(&f, "xtask/src/lib.rs:3: an added comment"), "{f:?}");
    assert_eq!(comment_findings(&f).len(), files.len(), "{f:?}");
}

#[test]
fn docs_base_finds_a_comment_after_a_yaml_single_quoted_value() {
    let findings = comment_diff(
        "docs-comments-after-yaml-single-quote",
        &[],
        &[("a.yaml", "text: 'A plain value' # YAML note.\n")],
    );
    assert!(has(&findings, "a.yaml:1: an added comment"), "{findings:?}");
    assert_eq!(comment_findings(&findings).len(), 1, "{findings:?}");
}

const ASE: &str = "from ase.build import bulk\n# a line\natoms = bulk(\"Si\")";
const NOTEBOOK: &str = "notebooks/quickstart.ipynb";

fn cell(source: &str) -> serde_json::Value {
    serde_json::json!({
        "cell_type": "code", "execution_count": null, "id": "c", "metadata": {}, "outputs": [],
        "source": source.split_inclusive('\n').collect::<Vec<_>>(),
    })
}

fn notebook_findings(name: &str, cells: Vec<serde_json::Value>) -> Vec<String> {
    let book =
        serde_json::json!({"cells": cells, "metadata": {}, "nbformat": 4, "nbformat_minor": 5});
    let text = serde_json::to_string_pretty(&book).unwrap();
    readme_findings(
        name,
        &["## ASE", "", "```python", ASE, "```"],
        &[
            (NOTEBOOK, &text),
            ("tests/ase.rs", "readme_block(\"ASE\")\n"),
        ],
    )
}

fn quickstart() -> Vec<serde_json::Value> {
    vec![
        cell("%pip install orbitar"),
        cell(ASE),
        cell("print(atoms)"),
    ]
}

#[test]
fn a_quickstart_notebook_that_installs_and_runs_the_readme_ase_block_without_outputs_passes() {
    let f = notebook_findings("notebook-good", quickstart());
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn a_quickstart_notebook_that_drifts_from_the_readme_or_stores_outputs_fails() {
    let mut drifted = quickstart();
    drifted[1] = cell(&ASE.replace("Si", "Ge"));
    let f = notebook_findings("notebook-drift", drifted);
    assert!(
        has(
            &f,
            &format!("{NOTEBOOK} cell 2 is not README.md's `## ASE` block")
        ),
        "{f:?}"
    );
    let mut run = quickstart();
    run[2]["outputs"] =
        serde_json::json!([{"output_type": "stream", "name": "stdout", "text": ["x"]}]);
    run[0]["execution_count"] = serde_json::json!(1);
    let f = notebook_findings("notebook-outputs", run);
    for n in [1, 3] {
        assert!(
            has(&f, &format!("{NOTEBOOK} cell {n} stores outputs")),
            "{n}: {f:?}"
        );
    }
    let mut prose = quickstart();
    prose.push(
        serde_json::json!({"cell_type": "markdown", "id": "m", "metadata": {}, "source": ["x"]}),
    );
    prose[0] = cell("%pip install orbitar==0.0.0");
    let f = notebook_findings("notebook-prose", prose);
    for needle in [
        "cell 4 is no code cell",
        "holds 4 cells, not 3",
        "cell 1 is not `%pip install orbitar`",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
}

#[test]
fn a_notebook_that_does_not_parse_or_another_notebook_fails() {
    let f = readme_findings(
        "notebook-other",
        &[],
        &[(NOTEBOOK, "{\n"), ("notebooks/other.ipynb", "{}\n")],
    );
    assert!(has(&f, &format!("{NOTEBOOK} does not parse")), "{f:?}");
    assert!(has(&f, "notebooks/other.ipynb is extra prose"), "{f:?}");
    assert!(!has(&f, &format!("{NOTEBOOK} is extra prose")), "{f:?}");
}

fn invalid(name: &str, text: &str) -> Vec<String> {
    readme_findings(
        name,
        &["## ASE", "", "```python", ASE, "```"],
        &[
            (NOTEBOOK, text),
            ("tests/ase.rs", "readme_block(\"ASE\")\n"),
        ],
    )
}

#[test]
fn an_empty_quickstart_or_one_of_no_valid_nbformat_4_structure_fails_before_any_comparison() {
    for text in ["", " \n"] {
        let f = invalid("notebook-empty", text);
        assert!(has(&f, &format!("{NOTEBOOK} is empty")), "{text:?}: {f:?}");
    }
    let good = serde_json::json!({"cells": quickstart(), "metadata": {}, "nbformat": 4, "nbformat_minor": 5});
    let mut mixed = good.clone();
    mixed["cells"][1]["source"] =
        serde_json::json!(["from ase.build import bulk\n", 7, "atoms = bulk(\"Si\")"]);
    let mut old = good.clone();
    old["nbformat"] = serde_json::json!(3);
    let mut bare = good.clone();
    bare.as_object_mut().unwrap().remove("metadata");
    let mut no_outputs = good.clone();
    no_outputs["cells"][2]
        .as_object_mut()
        .unwrap()
        .remove("outputs");
    let mut no_id = good.clone();
    no_id["cells"][0].as_object_mut().unwrap().remove("id");
    for (book, reason) in [
        (mixed, "cell 2 source holds a non-string element"),
        (old, "nbformat is not 4"),
        (bare, "metadata is no object"),
        (no_outputs, "code cell 3 lacks an outputs list"),
        (no_id, "cell 1 has no string id"),
        (serde_json::json!([]), "its top level is no object"),
    ] {
        let f = invalid("notebook-invalid", &book.to_string());
        let want = format!("docs: {NOTEBOOK} is no valid nbformat 4 notebook: {reason}");
        assert!(f.len() == 1 && f[0].starts_with(&want), "{reason}: {f:?}");
    }
}

#[test]
fn the_readme_ase_block_runs_from_its_python_fence_to_the_closing_fence_whatever_backticks_it_holds()
 {
    let ase = "atoms = 1  # a ``` inside\nenergy = 2";
    let block = ["## ASE", "", "```python", ase, "```"];
    let findings = |name: &str, code: &str| {
        let mut cells = quickstart();
        cells[1] = cell(code);
        let book =
            serde_json::json!({"cells": cells, "metadata": {}, "nbformat": 4, "nbformat_minor": 5});
        readme_findings(
            name,
            &block,
            &[
                (NOTEBOOK, &book.to_string()),
                ("tests/ase.rs", "readme_block(\"ASE\")\n"),
            ],
        )
    };
    let f = findings("notebook-fence-full", ase);
    assert!(f.is_empty(), "{f:?}");
    let f = findings("notebook-fence-cut", "atoms = 1  # a ");
    assert!(
        has(
            &f,
            &format!("{NOTEBOOK} cell 2 is not README.md's `## ASE` block")
        ),
        "{f:?}"
    );
}
