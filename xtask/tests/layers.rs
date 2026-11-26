use crate::{has, lever, tree};

#[test]
fn a_layer_may_use_only_layers_before_it() {
    let root = tree(
        "layers",
        &[
            ("src/xc/lda.rs", "use crate::ks::Outcome;\n"),
            ("src/xc/gga.rs", "use super::super::pw::Basis;\n"),
            ("src/xc/meta.rs", "pub use super::super::ks;\n"),
            ("src/xc/brace.rs", "use crate::{model::Cell, ks::run};\n"),
            (
                "src/xc/call.rs",
                "macro_rules! call {\n    () => {\n        $crate::ks::run()\n    };\n}\n",
            ),
            (
                "src/ks/bands.rs",
                "use crate::{model::Cell, pw::{Basis, lowest}};\nuse super::helper;\nuse crate::ks::Outcome;\npub(super) fn shared() {}\n",
            ),
            ("src/extra.rs", ""),
        ],
    );
    let findings = lever(&["layers"], &root);
    assert!(has(&findings, "src/xc/lda.rs uses `ks`"));
    assert!(has(&findings, "src/xc/gga.rs uses `pw`"));
    assert!(has(&findings, "src/xc/meta.rs uses `ks`"), "{findings:?}");
    assert!(has(&findings, "src/xc/brace.rs uses `ks`"), "{findings:?}");
    assert!(
        has(&findings, "src/xc/call.rs uses `ks`"),
        "a macro's `$crate` is the crate: {findings:?}"
    );
    assert!(!has(&findings, "src/ks/bands.rs"));
    assert!(has(&findings, "unknown layer `extra`"));
    assert!(!has(&findings, "which is not a layer"), "{findings:?}");
}

#[test]
fn crate_aliases_and_globs_cannot_hide_an_arrow() {
    let root = tree(
        "layers-alias",
        &[
            ("src/xc/a.rs", "use crate as root;\n"),
            ("src/pw/b.rs", "use crate::*;\n"),
            ("src/pw/c.rs", "use crate::{ self as top };\n"),
            ("src/pw/d.rs", "extern crate self as top;\n"),
            ("src/pw/e.rs", "use super::*;\n"),
            ("src/pw/f.rs", "use self::super::ks;\n"),
            ("src/pw/h.rs", "use self::super::super::ks::run;\n"),
            ("src/pw/g.rs", "use crate::{model::Cell, ks::*};\n"),
        ],
    );
    let findings = lever(&["layers"], &root);
    for needle in [
        "src/xc/a.rs:1 uses `crate as`",
        "src/pw/b.rs:1 uses `crate::*`",
        "src/pw/c.rs:1 uses `crate::{self`",
        "src/pw/d.rs:1 uses `extern crate self`",
        "src/pw/e.rs:1 uses `super::*`",
        "src/pw/f.rs:1 uses `self::super`",
        "src/pw/h.rs uses `ks`",
        "src/pw/g.rs:1 uses `crate::{…, *}`",
    ] {
        assert!(has(&findings, needle), "{needle}: {findings:?}");
    }
    assert!(!has(&findings, "which is not a layer"), "{findings:?}");
}

#[test]
fn only_io_knows_file_formats() {
    let root = tree(
        "layers-format",
        &[
            ("src/model/a.rs", "use toml::Table;\n"),
            (
                "src/model/c.rs",
                "use ::toml as t;\npub fn f(s: &str) -> bool {\n    t::from_str::<t::Table>(s).is_ok()\n}\n",
            ),
            ("src/ks/b.rs", "pub fn f(v: &serde_json::Value) {}\n"),
            (
                "src/paw/d.rs",
                "pub fn f(s: &str) -> bool {\n    roxmltree::Document::parse(s).is_ok()\n}\n",
            ),
            (
                "src/io/c.rs",
                "use toml::Table;\nuse serde::Deserialize;\nuse roxmltree::Document;\n",
            ),
        ],
    );
    let findings = lever(&["layers"], &root);
    assert!(has(
        &findings,
        "src/model/a.rs:1 uses a format crate (`toml`)"
    ));
    assert!(has(
        &findings,
        "src/model/c.rs:1 uses a format crate (`toml`)"
    ));
    assert!(has(
        &findings,
        "src/ks/b.rs:1 uses a format crate (`serde_json`)"
    ));
    assert!(has(
        &findings,
        "src/paw/d.rs:2 uses a format crate (`roxmltree`)"
    ));
    assert!(!has(&findings, "src/io/c.rs"), "{findings:?}");
}

#[test]
fn a_comment_cannot_split_a_path() {
    let root = tree(
        "layers-comment",
        &[(
            "src/pw/eigensolver.rs",
            "pub fn f(spec: &Spec) {\n    crate::/* up */ks::run(spec);\n    crate\n        ::ks::run(spec);\n}\n",
        )],
    );
    let findings = lever(&["layers"], &root);
    assert!(
        has(&findings, "src/pw/eigensolver.rs uses `ks`"),
        "{findings:?}"
    );
}
