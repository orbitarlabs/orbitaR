use crate::{NATIVE_AT, NATIVE_WIRED, has, lever, tree};

const CASE: &str = "[case]\ntier = \"push\"\nstatus = \"planned\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n";

#[test]
fn a_comment_or_a_line_break_never_splits_a_path_or_a_call() {
    let files = [
        (
            "src/pw/a.rs",
            "pub fn f(s: &Spec) {\n    crate:: // up\n        ks::run(s);\n}\n",
        ),
        (
            "xtask/src/fit.rs",
            "pub fn g() {\n    let _ = std::process::Command\n        ::new(/* python */ \"python3\");\n}\n",
        ),
        (
            "src/ks/b.rs",
            "pub fn h() -> &'static str {\n    include_str\n        !(\"x\")\n}\n",
        ),
        ("benchmarks/1_a/case.toml", CASE),
        NATIVE_WIRED,
        (NATIVE_AT, ""),
    ];
    let root = tree("syntax-split", &files);
    assert!(has(&lever(&["layers"], &root), "src/pw/a.rs uses `ks`"));
    assert!(has(
        &lever(&["native"], &root),
        "xtask/src/fit.rs:2 runs `python3`"
    ));
    assert!(has(
        &lever(&["science"], &root),
        "src/ks/b.rs:2: S3 `include_str!`"
    ));
}

#[test]
fn doc_comments_and_strings_never_count_as_code() {
    let files = [
        (
            "src/pw/a.rs",
            "/// #[test] in a doc comment.\npub fn f() -> &'static str {\n    \"crate::ks::run\"\n}\n",
        ),
        ("tests/pw/a.rs", ""),
        ("src/lib.rs", "mod pw;\n"),
        ("src/pw.rs", "mod a;\n"),
        ("tests/lib.rs", "mod pw;\n"),
        ("tests/pw.rs", "mod a;\n"),
    ];
    let root = tree("syntax-quiet", &files);
    let layout = lever(&["layout"], &root);
    assert!(!has(&layout, "inline test"), "{layout:?}");
    assert!(lever(&["layers"], &root).is_empty());
}

#[test]
fn every_lever_reads_a_string_with_its_escapes_decoded() {
    let files = [
        (
            "xtask/src/fit.rs",
            "pub fn g() {\n    let _ = std::process::Command::new(\"\\x67it\");\n}\n",
        ),
        (
            "src/ks/m.rs",
            "pub const M: &str = \"1\\x2e747564594633182\";\n",
        ),
        ("benchmarks/1_a/case.toml", CASE),
        (
            "benchmarks/1_a/reference/m.json",
            "{\"m\": 1.747564594633182}",
        ),
        NATIVE_WIRED,
        (NATIVE_AT, ""),
    ];
    let root = tree("syntax-escape", &files);
    assert!(has(
        &lever(&["native"], &root),
        "xtask/src/fit.rs:2 runs `git`"
    ));
    assert!(has(
        &lever(&["science"], &root),
        "src/ks/m.rs:1: S1 the string \"1.747564594633182\""
    ));
}
