use crate::{NATIVE_AT as AT, NATIVE_WIRED as WIRED, has, lever, tree};

const MANIFEST: &str = "[package]\nname = \"orbitar\"\n\n[dependencies]\ntoml = \"=1.1.8\"\n";
const ROWS: &str = "[run-spec]\nstatus = \"adopted\"\nuse = \"crate:toml\"\ncode = [\"src/io/spec.rs\"]\nproof = [\"tests/io/spec.rs\"]\ntried = [\"toml 1.1.8: typed fields\"]\n";

fn native(name: &str, rows: &str) -> Vec<String> {
    let root = tree(
        name,
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, rows),
            ("src/io/spec.rs", "use toml::Table;\n"),
            ("tests/io/spec.rs", ""),
        ],
    );
    lever(&["native"], &root)
}

#[test]
fn a_covered_tree_passes() {
    assert!(
        native("native-clean", ROWS).is_empty(),
        "{:?}",
        native("native-clean", ROWS)
    );
}

#[test]
fn every_dependency_and_source_file_needs_a_row() {
    let findings = native("native-missing", "");
    assert!(has(&findings, "dependency toml has no adopted row"));
    assert!(has(
        &findings,
        "src/io/spec.rs is in no scratch/local/rows.toml row"
    ));
    let root = tree(
        "native-entry",
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, ROWS),
            ("src/io/spec.rs", "use toml::Table;\n"),
            ("tests/io/spec.rs", ""),
            ("src/main.rs", "fn main() {}\n"),
            ("src/lib.rs", "pub mod io;\n"),
            ("src/io.rs", "mod spec;\n"),
            ("xtask/src/fit.rs", "pub fn fit() {}\n"),
        ],
    );
    let findings = lever(&["native"], &root);
    assert!(has(
        &findings,
        "src/main.rs is in no scratch/local/rows.toml row"
    ));
    assert!(has(
        &findings,
        "xtask/src/fit.rs is in no scratch/local/rows.toml row"
    ));
    assert!(!has(&findings, "src/lib.rs"), "{findings:?}");
    assert!(!has(&findings, "src/io.rs "), "{findings:?}");
}

#[test]
fn custom_code_needs_a_search_and_adopted_rows_need_proof() {
    let rows = "[run-spec]\nstatus = \"adopted\"\nuse = \"custom\"\ncode = [\"src/io/spec.rs\"]\n\n[parse]\nstatus = \"adopted\"\nuse = \"crate:toml\"\nproof = [\"tests/io/missing.rs\"]\n";
    let findings = native("native-custom", rows);
    assert!(has(&findings, "[run-spec] lists no options in `tried`"));
    assert!(has(&findings, "[parse] lists no options in `tried`"));
    assert!(has(&findings, "[run-spec] is adopted; name its proof"));
    assert!(has(
        &findings,
        "proof tests/io/missing.rs must be a test file"
    ));
}

#[test]
fn a_custom_row_lists_its_code() {
    let rows = format!(
        "{ROWS}\n[mixer]\nstatus = \"adopted\"\nuse = \"custom\"\nproof = [\"tests/io/spec.rs\"]\ntried = [\"none fits\"]\n"
    );
    assert!(has(
        &native("native-nocode", &rows),
        "[mixer] is custom; list its code"
    ));
}

#[test]
fn planned_rows_record_the_search_and_hold_no_code() {
    let rows = format!(
        "{ROWS}\n[symmetry]\nstatus = \"planned\"\nuse = \"crate:moyo\"\ncode = [\"src/io/spec.rs\"]\n"
    );
    let findings = native("native-planned", &rows);
    assert!(has(&findings, "[symmetry] is planned but lists code"));
    assert!(has(&findings, "[symmetry] lists no options in `tried`"));
    assert!(has(&findings, "src/io/spec.rs is in both"));
}

#[test]
fn an_option_outside_cargo_needs_a_version_and_a_license() {
    let rows = format!(
        "{ROWS}\n[eos]\nstatus = \"adopted\"\nuse = \"python:ase\"\nproof = [\"tests/io/spec.rs\"]\ntried = [\"ase 3.29.0: V0 within 6e-12\"]\n"
    );
    let findings = native("native-pins", &rows);
    assert!(has(&findings, "[eos] is outside Cargo"));
    let pinned = rows.replace(
        "tried = [\"ase",
        "version = \"3.29.0\"\nlicense = \"LGPL-2.1-or-later\"\ntried = [\"ase",
    );
    assert!(!has(&native("native-pinned", &pinned), "[eos]"));
}

#[test]
fn citation_metadata_is_data_and_needs_no_row() {
    let root = tree(
        "native-citation",
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, ROWS),
            ("CITATION.cff", "cff-version: 1.2.0\n"),
            ("tools/plot.py", "print(1)\n"),
        ],
    );
    let findings = lever(&["native"], &root);
    assert!(
        has(
            &findings,
            "tools/plot.py is in no scratch/local/rows.toml row"
        ),
        "{findings:?}"
    );
    assert!(
        !has(
            &findings,
            "CITATION.cff is in no scratch/local/rows.toml row"
        ),
        "{findings:?}"
    );
}

#[test]
fn scripts_and_mcp_servers_are_code_too() {
    let root = tree(
        "native-scripts",
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, ROWS),
            ("src/io/spec.rs", "use toml::Table;\n"),
            ("tests/io/spec.rs", ""),
            ("tools/plot.py", "print(1)\n"),
            (".mcp.json", "{}\n"),
        ],
    );
    let findings = lever(&["native"], &root);
    assert!(has(
        &findings,
        "tools/plot.py is in no scratch/local/rows.toml row"
    ));
    assert!(has(
        &findings,
        ".mcp.json is in no scratch/local/rows.toml row"
    ));
    let root = tree(
        "native-case",
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, ROWS),
            ("src/io/spec.rs", "use toml::Table;\n"),
            ("tests/io/spec.rs", ""),
            (
                "tests/benchmarks/5_eos/case.rs",
                "fn birch_murnaghan() {}\n",
            ),
        ],
    );
    assert!(has(
        &lever(&["native"], &root),
        "tests/benchmarks/5_eos/case.rs is in no scratch/local/rows.toml row"
    ));
}

#[test]
fn a_crate_row_names_its_crate_and_proves_with_a_test_or_a_case() {
    let rows = ROWS.replace("tests/io/spec.rs\"]\ntried", "Cargo.toml\"]\ntried");
    let root = tree(
        "native-proof",
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, &rows),
            ("src/io/spec.rs", "pub fn parse() {}\n"),
            ("tests/io/spec.rs", ""),
        ],
    );
    let findings = lever(&["native"], &root);
    assert!(has(
        &findings,
        "[run-spec] lists src/io/spec.rs, which never names toml"
    ));
    assert!(has(
        &findings,
        "[run-spec] proof Cargo.toml must be a test file"
    ));
}

#[test]
fn providers_must_be_known_and_real() {
    let rows = format!(
        "{ROWS}\n[fft]\nstatus = \"adopted\"\nuse = \"crate:rustfft\"\nproof = [\"tests/io/spec.rs\"]\n\n[x]\nstatus = \"planned\"\nuse = \"magic\"\ntried = [\"none\"]\n\n[y]\nstatus = \"planned\"\nuse = \"crate:\"\ntried = [\"none\"]\n"
    );
    let findings = native("native-provider", &rows);
    assert!(has(
        &findings,
        "uses rustfft, which no Cargo.toml depends on"
    ));
    assert!(has(&findings, "use = \"magic\" is not"));
    assert!(has(&findings, "[y] use = \"crate:\" is not"));
}

fn native_with(name: &str, rows: &str, files: &[(&str, &str)]) -> Vec<String> {
    let mut all = vec![
        ("Cargo.toml", MANIFEST),
        WIRED,
        (AT, rows),
        ("src/io/spec.rs", "use toml::Table;\n"),
        ("tests/io/spec.rs", ""),
    ];
    all.extend_from_slice(files);
    lever(&["native"], &tree(name, &all))
}

#[test]
fn every_file_outside_data_tests_and_module_roots_is_code() {
    let findings = native_with(
        "native-allowlist",
        ROWS,
        &[
            ("examples/eos.rs", "fn main() {}\n"),
            ("benches/fft.rs", "fn main() {}\n"),
            ("build.rs", "fn main() {}\n"),
            ("tools/eos.R", "fit <- function(v, e) lm(e ~ poly(v, 2))\n"),
            ("README.md", "# x\n"),
            ("LICENSE", "PolyForm Noncommercial License 1.0.0\n"),
            (".github/workflows/ci.yml", "on: push\n"),
            ("benchmarks/1_a/case.toml", "[case]\n"),
            ("benchmarks/locks/harness/x.rs.lock", ""),
            ("Cargo.lock", ""),
            ("tests/benchmarks.rs", "fn main() {}\n"),
            ("xtask/tests/lib.rs", ""),
            (".agents/skills/x/SKILL.md", "# x\n"),
            ("src/lib.rs", "pub mod io;\n"),
        ],
    );
    for file in [
        "examples/eos.rs",
        "benches/fft.rs",
        "build.rs",
        "tools/eos.R",
    ] {
        assert!(
            has(
                &findings,
                &format!("{file} is in no scratch/local/rows.toml row")
            ),
            "{file}: {findings:?}"
        );
    }
    for file in [
        "README.md",
        "LICENSE",
        "ci.yml",
        "case.toml",
        "x.rs.lock",
        "Cargo.lock",
        "tests/benchmarks.rs",
        "xtask/tests/lib.rs",
        "SKILL.md",
        "src/lib.rs",
    ] {
        assert!(!has(&findings, file), "{file}: {findings:?}");
    }
}

#[test]
fn each_program_that_code_runs_needs_an_adopted_tool_row() {
    let code = "use std::process::Command;\npub fn f() {\n    let _ = Command::new(\"python3\").status();\n    let _ = std::process::Command::new(\"git\").status();\n}\n";
    let rows = format!(
        "{ROWS}\n[fit]\nstatus = \"adopted\"\nuse = \"custom\"\ncode = [\"xtask/src/fit.rs\", \"xtask/src/alias.rs\"]\nproof = [\"tests/io/spec.rs\"]\ntried = [\"none fits\"]\n"
    );
    let files = [
        ("xtask/src/fit.rs", code),
        (
            "xtask/src/alias.rs",
            "use std::process::Command as Run;\npub fn g() {\n    let _ = Run::new(\"curl\");\n}\n",
        ),
    ];
    let findings = native_with("native-tools", &rows, &files);
    assert!(
        has(&findings, "xtask/src/fit.rs:3 runs `python3`"),
        "{findings:?}"
    );
    assert!(
        has(&findings, "xtask/src/fit.rs:4 runs `git`"),
        "{findings:?}"
    );
    assert!(
        has(&findings, "xtask/src/alias.rs:1 renames Command"),
        "{findings:?}"
    );
    let git = format!(
        "{rows}\n[git]\nstatus = \"adopted\"\nuse = \"tool:git\"\nversion = \"2.43.0\"\nlicense = \"GPL-2.0-only\"\nproof = [\"tests/io/spec.rs\"]\ntried = [\"git 2.43.0: works\"]\n"
    );
    let findings = native_with("native-tools-git", &git, &files);
    assert!(!has(&findings, "runs `git`"), "{findings:?}");
    assert!(has(&findings, "runs `python3`"), "{findings:?}");
}

#[test]
fn every_file_of_a_crate_row_names_the_crate_outside_comments() {
    let rows = ROWS.replace(
        "code = [\"src/io/spec.rs\"]",
        "code = [\"src/io/spec.rs\", \"src/pw/fft.rs\"]",
    );
    let files = [(
        "src/pw/fft.rs",
        "// toml::from_str is not used here\npub fn transform() {}\n",
    )];
    let findings = native_with("native-crate-files", &rows, &files);
    assert!(
        has(
            &findings,
            "[run-spec] lists src/pw/fft.rs, which never names toml"
        ),
        "{findings:?}"
    );
    assert!(!has(&findings, "lists src/io/spec.rs"), "{findings:?}");
}

#[test]
fn a_proof_is_a_rust_test_file_not_a_directory_or_data() {
    let proofs = "proof = [\"tests/io/\", \"tests/io/fixture.toml\", \"tests/io/cases.rs\"]";
    let rows = ROWS.replace("proof = [\"tests/io/spec.rs\"]", proofs);
    let files = [
        ("tests/io/fixture.toml", "x = 1\n"),
        ("tests/io/cases.rs/keep.txt", ""),
    ];
    let findings = native_with("native-proof-dir", &rows, &files);
    for proof in ["tests/io/", "tests/io/fixture.toml", "tests/io/cases.rs"] {
        let needle = format!("[run-spec] proof {proof} must be a test file");
        assert!(has(&findings, &needle), "{needle}: {findings:?}");
    }
}

#[test]
fn a_planned_option_outside_cargo_pins_its_version_and_license_too() {
    let rows = format!(
        "{ROWS}\n[xc-reference]\nstatus = \"planned\"\nuse = \"python:pyscf\"\nversion = \"2.14.0\"\ntried = [\"PySCF 2.14.0: LDA within 4.4e-16\"]\n"
    );
    let findings = native_with("native-planned-pins", &rows, &[]);
    assert!(
        has(&findings, "[xc-reference] is outside Cargo"),
        "{findings:?}"
    );
    let pinned = rows.replace(
        "version = \"2.14.0\"",
        "version = \"2.14.0\"\nlicense = \"Apache-2.0\"",
    );
    assert!(!has(
        &native_with("native-planned-pinned", &pinned, &[]),
        "[xc-reference]"
    ));
}

#[test]
fn an_untested_option_names_a_license_or_platform_block() {
    let rows = format!(
        "{ROWS}\n[symmetry]\nstatus = \"planned\"\nuse = \"crate:moyo\"\ntried = [\"moyo 0.21.0: Si 227\", \"spglib (not tested): a build script\"]\n"
    );
    let findings = native_with("native-untested", &rows, &[]);
    assert!(
        has(&findings, "[symmetry] `tried` has \"spglib (not tested)"),
        "{findings:?}"
    );
    let blocked = rows.replace("(not tested):", "(not tested: platform):");
    assert!(!has(
        &native_with("native-blocked", &blocked, &[]),
        "[symmetry]"
    ));
}

#[test]
fn std_needs_tried_only_when_it_lists_code() {
    let rows = format!(
        "{ROWS}\n[threads]\nstatus = \"adopted\"\nuse = \"std\"\nproof = [\"tests/io/spec.rs\"]\n\n[basis]\nstatus = \"adopted\"\nuse = \"std\"\ncode = [\"src/pw/basis.rs\"]\nproof = [\"tests/io/spec.rs\"]\n"
    );
    let findings = native_with(
        "native-std",
        &rows,
        &[("src/pw/basis.rs", "pub fn f() {}\n")],
    );
    assert!(!has(&findings, "[threads]"), "{findings:?}");
    assert!(
        has(&findings, "[basis] lists no options in `tried`"),
        "{findings:?}"
    );
}

#[test]
fn a_code_pattern_covers_each_measure_and_must_match() {
    let rows = format!(
        "{ROWS}\n[case-measures]\nstatus = \"adopted\"\nuse = \"custom\"\ncode = [\"tests/benchmarks/*/case.rs\"]\nproof = [\"tests/io/spec.rs\"]\ntried = [\"closed forms\"]\n"
    );
    let measures = [
        ("tests/benchmarks/1_a/case.rs", "pub fn measure() {}\n"),
        ("tests/benchmarks/2_b/case.rs", "pub fn measure() {}\n"),
    ];
    assert!(native_with("native-glob", &rows, &measures).is_empty());
    let loose = rows.replace("*/case.rs", "*/*.rs\", \"tests/bench*/x.rs");
    let findings = native_with("native-glob-loose", &loose, &measures);
    assert!(
        has(
            &findings,
            "names tests/bench*/x.rs; `*` stands for one whole path segment"
        ),
        "{findings:?}"
    );
    let empty = rows.replace(
        "tests/benchmarks/*/case.rs",
        "tests/benchmarks/*/measure.rs",
    );
    let findings = native_with("native-glob-empty", &empty, &measures);
    assert!(
        has(
            &findings,
            "names tests/benchmarks/*/measure.rs, which matches no file"
        ),
        "{findings:?}"
    );
    assert!(
        has(
            &findings,
            "tests/benchmarks/1_a/case.rs is in no scratch/local/rows.toml row"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_dependency_needs_an_adopted_row_not_a_planned_one() {
    let rows = ROWS
        .replace("status = \"adopted\"", "status = \"planned\"")
        .replace("code = [\"src/io/spec.rs\"]\n", "");
    let findings = native_with("native-planned-dep", &rows, &[]);
    assert!(
        has(&findings, "dependency toml has no adopted row"),
        "{findings:?}"
    );
}

#[test]
fn agent_skills_hold_no_product_code() {
    let files = [
        (".agents/skills/x/scripts/fetch.py", "print(1)\n"),
        (".claude/hooks/check.sh", "exit 0\n"),
    ];
    let findings = native_with("native-agents", ROWS, &files);
    assert!(!has(&findings, ".agents/"), "{findings:?}");
    assert!(!has(&findings, ".claude/"), "{findings:?}");
}

#[test]
fn only_a_renamed_command_hides_a_program() {
    let rows = format!(
        "{ROWS}\n[paths]\nstatus = \"adopted\"\nuse = \"std\"\ncode = [\"xtask/src/paths.rs\"]\nproof = [\"tests/io/spec.rs\"]\ntried = [\"std::path\"]\n"
    );
    let files = [(
        "xtask/src/paths.rs",
        "use std::path::Path as P;\npub fn f(p: &P) -> bool {\n    p.is_absolute()\n}\n",
    )];
    let findings = native_with("native-other-alias", &rows, &files);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_tool_that_ci_installs_pins_the_version_of_its_row() {
    let rows = format!(
        "{ROWS}\n[seeded-failures]\nstatus = \"adopted\"\nuse = \"tool:cargo-mutants\"\nversion = \"27.1.0\"\nlicense = \"MIT\"\nproof = [\"tests/io/spec.rs\"]\ntried = [\"cargo-mutants 27.1.0: works\"]\n"
    );
    let ci = |line: &str| {
        [(
            ".github/workflows/ci.yml",
            format!("jobs:\n  m:\n    steps:\n      - run: {line}\n"),
        )]
    };
    let good = ci("cargo install cargo-mutants --version 27.1.0 --locked");
    let good: Vec<(&str, &str)> = good.iter().map(|(p, b)| (*p, b.as_str())).collect();
    assert!(native_with("native-install", &rows, &good).is_empty());
    for (n, line) in [
        "cargo install cargo-mutants --locked",
        "cargo install cargo-mutants --version 27.0.0 --locked",
        "cargo install cargo-mutants --version 27.1.0",
        "cargo install cargo-nextest --version 0.9.0 --locked",
        "cargo install cargo-mutants --version --locked",
    ]
    .into_iter()
    .enumerate()
    {
        let files = ci(line);
        let files: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
        let findings = native_with(&format!("native-install-{n}"), &rows, &files);
        assert!(
            has(&findings, "ci.yml:4 installs cargo-"),
            "{line}: {findings:?}"
        );
    }
}

#[test]
fn each_python_pin_has_a_row_at_the_same_version() {
    let rows = format!(
        "{ROWS}\n[cells]\nstatus = \"planned\"\nuse = \"python:ase\"\nversion = \"3.29.0\"\nlicense = \"LGPL-2.1-or-later\"\ntried = [\"ASE 3.29.0: builds the cells\"]\n"
    );
    let lock = |pins: &str| {
        [(
            "python/pylock.toml",
            format!("lock-version = \"1.0\"\n{pins}"),
        )]
    };
    let pin = |name: &str, version: &str| {
        format!("\n[[packages]]\nname = \"{name}\"\nversion = \"{version}\"\n")
    };
    let check = |name: &str, pins: String| {
        let files = lock(&pins);
        let files: Vec<(&str, &str)> = files.iter().map(|(p, b)| (*p, b.as_str())).collect();
        native_with(name, &rows, &files)
    };
    assert!(check("native-pin", pin("ase", "3.29.0")).is_empty());
    let other = check("native-pin-other", pin("ase", "3.28.0"));
    assert_eq!(
        other,
        [
            "native: python/pylock.toml pins ase 3.28.0, but the python:ase rows say {\"3.29.0\"}; one version for both"
        ]
    );
    let missing = check(
        "native-pin-missing",
        pin("ase", "3.29.0") + &pin("SciPy", "1.18.1"),
    );
    assert_eq!(
        missing,
        [
            "native: python/pylock.toml pins SciPy 1.18.1, but the python:scipy rows say {}; one version for both"
        ]
    );
}

#[test]
fn a_case_proof_names_an_existing_case() {
    let rows = ROWS.replace(
        "proof = [\"tests/io/spec.rs\"]",
        "proof = [\"benchmarks/1_a\", \"benchmarks/9_gone\"]",
    );
    let files = [("benchmarks/1_a/case.toml", "[case]\n")];
    let findings = native_with("native-case-proof", &rows, &files);
    assert!(
        has(&findings, "proof benchmarks/9_gone must be"),
        "{findings:?}"
    );
    assert!(!has(&findings, "proof benchmarks/1_a "), "{findings:?}");
}

#[test]
fn a_code_path_may_not_climb_out_and_back() {
    let rows = ROWS.replace(
        "code = [\"src/io/spec.rs\"]",
        "code = [\"src/../src/io/spec.rs\"]",
    );
    let findings = native_with("native-climb", &rows, &[]);
    assert!(
        has(
            &findings,
            "names src/../src/io/spec.rs, which is not a file in the repository"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_bad_case_name_is_named_once_not_as_seven_proofs() {
    let root = tree(
        "native-bad-id",
        &[
            ("Cargo.toml", MANIFEST),
            WIRED,
            (AT, ROWS),
            ("src/io/spec.rs", "use toml::Table;\n"),
            ("tests/io/spec.rs", ""),
            ("benchmarks/05_x/case.toml", ""),
        ],
    );
    let f = lever(&["native"], &root);
    assert_eq!(f.len(), 1, "{f:?}");
    assert!(has(&f, "05_x"), "{f:?}");
}

#[test]
fn only_the_engine_runner_may_name_the_engine_and_its_shim_by_value() {
    let rows = format!(
        "{ROWS}\n[runs]\nstatus = \"adopted\"\nuse = \"std\"\ncode = [\"xtask/src/runs.rs\", \"xtask/src/harness/engine.rs\"]\nproof = [\"tests/io/spec.rs\"]\ntried = [\"std::process\"]\n"
    );
    let body = "use std::process::Command;\npub fn f(engine: &str, other: &str, shim: &str) {\n    let _ = Command::new(&engine);\n    let _ = Command::new(&other);\n    let _ = Command::new(engine);\n    let _ = Command::new(&shim);\n}\n";
    let findings = native_with(
        "native-engine",
        &rows,
        &[
            ("xtask/src/runs.rs", body),
            ("xtask/src/harness/engine.rs", body),
        ],
    );
    for line in [3, 5, 6] {
        assert!(
            !has(&findings, &format!("xtask/src/harness/engine.rs:{line}")),
            "{findings:?}"
        );
    }
    for (at, spelled) in [
        ("xtask/src/harness/engine.rs:4", "&other"),
        ("xtask/src/runs.rs:3", "&engine"),
        ("xtask/src/runs.rs:4", "&other"),
        ("xtask/src/runs.rs:5", "engine"),
        ("xtask/src/runs.rs:6", "&shim"),
    ] {
        assert!(
            has(&findings, &format!("{at} runs the program in `{spelled}`")),
            "{at}: {findings:?}"
        );
    }
}

#[test]
fn a_program_shows_only_by_a_literal_name() {
    let rows = format!(
        "{ROWS}\n[runs]\nstatus = \"adopted\"\nuse = \"std\"\ncode = [\"xtask/src/runs.rs\"]\nproof = [\"tests/io/spec.rs\"]\ntried = [\"std::process\"]\n"
    );
    let body = "use std::process::Command;\nconst PYTHON: &str = \"python3\";\ntype Run = std::process::Command;\npub fn f(env: &Env) {\n    let _ = Command::new(PYTHON);\n    let _ = Run::new(\"curl\");\n    let _ = Command::new(&env.cargo);\n}\n";
    let findings = native_with("native-literal", &rows, &[("xtask/src/runs.rs", body)]);
    assert!(
        has(
            &findings,
            "xtask/src/runs.rs:5 runs the program in `PYTHON`"
        ),
        "{findings:?}"
    );
    assert!(
        has(&findings, "xtask/src/runs.rs:3 renames Command"),
        "{findings:?}"
    );
    assert!(!has(&findings, ":7"), "{findings:?}");
}

#[test]
fn a_checkout_without_a_local_manifest_passes() {
    let public = [
        ("Cargo.toml", MANIFEST),
        ("src/io/spec.rs", "use toml::Table;\n"),
        ("xtask/src/fit.rs", "pub fn fit() {}\n"),
    ];
    let f = lever(&["native"], &tree("native-public", &public));
    assert!(f.is_empty(), "{f:?}");
    let mut docs_only = public.to_vec();
    docs_only.push((
        "CLAUDE.local.md",
        "<!-- local docs: scratch/docs.toml -->\n",
    ));
    docs_only.push(("scratch/docs.toml", ROWS));
    let f = lever(&["native"], &tree("native-docs-only", &docs_only));
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn a_declared_manifest_is_one_file_inside_the_checkout() {
    let places = [
        "scratch/local/gone.toml",
        "../rows.toml",
        "/etc/hosts",
        "scratch/local",
    ];
    for (n, at) in places.into_iter().enumerate() {
        let wired = format!("<!-- local native: {at} -->\n");
        let root = tree(
            &format!("native-out-{n}"),
            &[("CLAUDE.local.md", &wired), (AT, ROWS)],
        );
        assert_eq!(
            lever(&["native"], &root),
            [format!(
                "native: CLAUDE.local.md declares `{at}`, which is no file inside the checkout"
            )]
        );
    }
}

#[test]
fn a_manifest_declared_twice_is_refused() {
    let twice = format!("{}{}", WIRED.1, WIRED.1);
    let root = tree("native-twice", &[("CLAUDE.local.md", &twice), (AT, ROWS)]);
    assert_eq!(
        lever(&["native"], &root),
        ["native: CLAUDE.local.md declares local native more than once"]
    );
}

fn public(name: &str, files: &[(&str, &str)]) -> Vec<String> {
    let mut all = vec![
        ("Cargo.toml", MANIFEST),
        ("src/io/spec.rs", "use toml::Table;\n"),
    ];
    all.extend_from_slice(files);
    lever(&["native"], &tree(name, &all))
}

#[test]
fn without_a_manifest_a_program_still_shows_by_a_literal_name() {
    let body = "use std::process::Command;\nconst PYTHON: &str = \"python3\";\npub fn f(env: &Env) {\n    let _ = Command::new(PYTHON);\n    let _ = Command::new(&env.cargo);\n    let _ = Command::new(\"git\");\n}\n";
    let f = public("native-public-literal", &[("xtask/src/runs.rs", body)]);
    assert_eq!(
        f,
        [
            "native: xtask/src/runs.rs:4 runs the program in `PYTHON`; write its name as a literal so its row shows"
        ]
    );
}

#[test]
fn without_a_manifest_command_still_keeps_its_name() {
    let used =
        "use std::process::Command as Run;\npub fn g() {\n    let _ = Run::new(\"curl\");\n}\n";
    let typed =
        "type Run = std::process::Command;\npub fn h() {\n    let _ = Run::new(\"curl\");\n}\n";
    let f = public(
        "native-public-alias",
        &[("xtask/src/used.rs", used), ("xtask/src/typed.rs", typed)],
    );
    assert_eq!(
        f,
        [
            "native: xtask/src/typed.rs:1 renames Command; name it in full so each program shows",
            "native: xtask/src/used.rs:1 renames Command; name it in full so each program shows",
        ]
    );
}

#[test]
fn without_a_manifest_each_install_still_pins_its_version_and_lock() {
    let ci = |line: &str| format!("jobs:\n  m:\n    steps:\n      - run: {line}\n");
    let good = ci("cargo install cargo-mutants --version 27.1.0 --locked");
    let f = public(
        "native-public-install",
        &[(".github/workflows/ci.yml", &good)],
    );
    assert!(f.is_empty(), "{f:?}");
    for (n, line) in [
        "cargo install cargo-mutants --locked",
        "cargo install cargo-mutants --version 27.1.0",
        "cargo install cargo-mutants --version --locked",
        "cargo install cargo-mutants --locked --version",
    ]
    .into_iter()
    .enumerate()
    {
        let bad = ci(line);
        let f = public(
            &format!("native-public-install-{n}"),
            &[(".github/workflows/ci.yml", &bad)],
        );
        assert_eq!(
            f,
            [
                "native: ci.yml:4 installs cargo-mutants; pin it with `--version <version>` and `--locked`"
            ],
            "{line}"
        );
    }
}

#[test]
fn without_a_manifest_the_python_lock_must_still_read() {
    let good = "lock-version = \"1.0\"\n\n[[packages]]\nname = \"ase\"\nversion = \"3.29.0\"\n";
    let f = public("native-public-pylock", &[("python/pylock.toml", good)]);
    assert!(f.is_empty(), "{f:?}");
    let f = public(
        "native-public-pylock-bad",
        &[("python/pylock.toml", "[[packages]]\nname = \"ase\"\n")],
    );
    assert!(
        f.len() == 1 && has(&f, "native: python/pylock.toml: "),
        "{f:?}"
    );
}
