use crate::{Tree, git, has, lever, tree, write};

const CASE: &str = "[case]\ntier = \"push\"\nstatus = \"planned\"\nclaims = [\"correctness\"]\nreference = \"literature\"\n";
const SOURCE: &str = "kind = \"literature\"\ncitation = \"OEIS A085469\"\nlocator = \"oeis:A085469\"\nconvention = \"nearest neighbor\"\nlicense = \"facts\"\n";
const REFERENCE: &str = "{\"NaCl\": 1.747564594633182}";

fn engine(name: &str, file: &str, body: &str) -> Vec<String> {
    let root = tree(
        name,
        &[
            (file, body),
            ("benchmarks/2_madelung/case.toml", CASE),
            ("benchmarks/2_madelung/reference/madelung.json", REFERENCE),
            ("benchmarks/2_madelung/reference/SOURCE.toml", SOURCE),
        ],
    );
    lever(&["science"], &root)
}

#[test]
fn s1_catches_a_reference_value_split_into_a_sum_or_a_string() {
    let f = engine(
        "s1",
        "src/ks/m.rs",
        "pub fn m() -> f64 {\n    1.747_56 + 4.594_63e-6 + 3.182e-12\n}\npub fn n() -> f64 {\n    \"1.747564594633182\".parse().unwrap()\n}\n",
    );
    assert!(has(&f, "S1 a literal expression"));
    assert!(has(&f, "S1 the string \"1.747564594633182\""));
}

#[test]
fn s1_s2_read_numbers_inside_free_text_strings() {
    let body =
        "pub fn m() -> &'static str {\n    \"madelung nacl = 1.747564594633182 (cached)\"\n}\n";
    let f = engine("s1-text", "src/ks/m.rs", body);
    assert!(has(&f, "S2 the string"));
    assert!(has(&f, "S1 the string"));
}

#[test]
fn s2_applies_to_measurement_code_too() {
    let body = "const FROZEN: f64 = 17.436_502_871;\npub fn measure() -> f64 {\n    FROZEN\n}\n";
    let f = engine("s2-case", "tests/benchmarks/2_madelung/case.rs", body);
    assert!(has(
        &f,
        "tests/benchmarks/2_madelung/case.rs:1: S2 17.436_502_871"
    ));
}

#[test]
fn s2_frees_no_number_in_measure_code() {
    let body = "/// source: the engine's last good run.\nconst FROZEN: f64 = 17.436_502_871;\npub fn measure() -> f64 {\n    FROZEN\n}\n";
    let f = engine(
        "s2-case-sourced",
        "tests/benchmarks/2_madelung/case.rs",
        body,
    );
    assert!(has(&f, "S2 17.436_502_871 sits in measure code"), "{f:?}");
}

#[test]
fn s3_rejects_a_symlinked_engine_file() {
    let root = tree("s3-link", &[("src/lib.rs", ""), ("benchmarks/.keep", "")]);
    std::fs::create_dir_all(root.join("src/ks")).unwrap();
    std::os::unix::fs::symlink("/etc/hostname", root.join("src/ks/alias.rs")).unwrap();
    assert!(has(
        &lever(&["science"], &root),
        "S3 src/ks/alias.rs is a symlink"
    ));
}

#[test]
fn s3_catches_a_bare_case_id_and_s2_a_long_integer() {
    let body = "pub fn a() -> &'static str {\n    \"2_madelung\"\n}\npub fn b() -> u64 {\n    1234567\n}\n";
    let f = engine("s3-id", "src/ks/m.rs", body);
    assert!(
        has(&f, "S3 engine code names a benchmark path or case"),
        "{f:?}"
    );
    assert!(has(&f, "S2 1234567 needs a named const"), "{f:?}");
}

#[test]
fn s3_lets_only_src_main_rs_name_the_version_and_the_commit() {
    let facts = "pub fn v() -> (&'static str, Option<&'static str>) {\n    (env!(\"CARGO_PKG_VERSION\"), option_env!(\"ORBITAR_COMMIT\"))\n}\n";
    let f = engine("s3-facts", "src/main.rs", facts);
    assert!(!has(&f, "S3 `"), "{f:?}");
    let f = engine("s3-facts-ks", "src/ks/m.rs", facts);
    assert!(has(&f, "S3 `env!` reads"), "{f:?}");
    assert!(has(&f, "S3 `option_env!` reads"), "{f:?}");
    let other = "pub fn h() -> &'static str {\n    env!(\"HOME\")\n}\n";
    let f = engine("s3-facts-other", "src/main.rs", other);
    assert!(has(&f, "S3 `env!` reads"), "{f:?}");
    let twin = "pub fn t() -> (&'static str, &'static str, Option<&'static str>, Option<&'static str>) {\n    \
                (env!(\"CARGO_PKG_VERSION\"), env!(\"HOME\"), option_env!(\"ORBITAR_COMMIT\"), option_env!(\"PATH\"))\n}\n";
    let f = engine("s3-facts-twin", "src/main.rs", twin);
    assert!(has(&f, "src/main.rs:2: S3 `env!` reads"), "{f:?}");
    assert!(has(&f, "src/main.rs:2: S3 `option_env!` reads"), "{f:?}");
    assert_eq!(f.iter().filter(|x| x.contains("S3 `")).count(), 2, "{f:?}");
}

#[test]
fn s3_rejects_data_files_in_the_engine() {
    let f = engine("s3-data", "src/ks/table.json", "[1.0]\n");
    assert!(has(&f, "S3 src/ks/table.json is not Rust"), "{f:?}");
}

#[test]
fn s2_in_io_catches_a_decoded_literal_but_not_a_decoded_runtime_value() {
    let decode = "pub fn a(b: [u8; 8]) -> f64 {\n    f64::from_le_bytes(b)\n}\n";
    let literals = [
        "pub fn c() -> f64 {\n    f64::from_le_bytes([0, 0, 0, 0, 0, 0, 240, 63])\n}\n",
        "pub fn c() -> f64 {\n    f64::from_bits(4607182418800017408)\n}\n",
        "pub fn c() -> u64 {\n    u64::from_le_bytes(*b\"orbitaR!\")\n}\n",
        "pub fn c() -> i32 {\n    i32::from_be_bytes(b\"ipi!\".to_owned())\n}\n",
    ];
    let f = engine("s2-io-runtime", "src/io/ipi.rs", decode);
    assert!(!has(&f, "S2"), "{f:?}");
    for (n, literal) in literals.iter().enumerate() {
        let f = engine(&format!("s2-io-literal-{n}"), "src/io/ipi.rs", literal);
        assert!(has(&f, "S2 from_"), "{literal}: {f:?}");
    }
    let f = engine("s2-ks-runtime", "src/ks/m.rs", decode);
    assert!(has(&f, "S2 from_le_bytes"), "{f:?}");
}

#[test]
fn s2_catches_uncited_and_hidden_constants() {
    let f = engine(
        "s2",
        "src/ks/m.rs",
        "pub fn a() -> f64 {\n    5.13\n}\npub fn b() -> f64 {\n    f64::from_le_bytes([0; 8])\n}\nconst GAMMA: f64 = -0.1423;\n",
    );
    assert!(has(&f, "S2 5.13"));
    assert!(has(&f, "S2 from_le_bytes"));
    assert!(has(&f, "S2 0.1423"), "{f:?}");
}

const GAMMA: &str = "GAMMA = \"Perdew and Zunger, Phys. Rev. B 23, 5048 (1981), Table XII.\"\n";

fn cited_engine(name: &str, body: &str, entries: &str) -> Vec<String> {
    let table = format!("[\"src/xc/lda.rs\"]\n{entries}");
    let root = tree(
        name,
        &[
            ("src/SOURCE.toml", &table),
            ("src/xc/lda.rs", body),
            ("benchmarks/.keep", ""),
        ],
    );
    lever(&["science"], &root)
}

#[test]
fn s2_accepts_the_listed_const_and_checks_the_next_expression() {
    let f = cited_engine(
        "s2-listed",
        "#[allow(dead_code)]\npub(in crate::xc) const GAMMA: [f64; 2] = [\n    -0.1423, 0.1423\n]; fn other() -> f64 { 0.1424 }\n",
        GAMMA,
    );
    assert!(!has(&f, "S2 0.1423"), "{f:?}");
    assert!(has(&f, "src/xc/lda.rs:4: S2 0.1424"), "{f:?}");
    assert!(!has(&f, "src/SOURCE.toml:"), "{f:?}");
}

#[test]
fn s2_lists_stale_citations_for_renamed_constants_and_missing_files() {
    let entries = format!("{GAMMA}[\"src/xc/gone.rs\"]\n{GAMMA}");
    let f = cited_engine("s2-stale", "const DELTA: f64 = 0.1423;\n", &entries);
    for file in ["src/xc/lda.rs", "src/xc/gone.rs"] {
        assert!(
            has(&f, &format!("S2 {file}::GAMMA is not a constant")),
            "{f:?}"
        );
    }
    assert!(has(&f, "S2 0.1423"), "{f:?}");
}

#[test]
fn s2_an_empty_or_blank_citation_frees_no_constant() {
    for (name, citation) in [("s2-empty", ""), ("s2-blank", " \\t ")] {
        let f = cited_engine(
            name,
            "const GAMMA: f64 = -0.1423;\n",
            &format!("GAMMA = \"{citation}\"\n"),
        );
        assert!(
            has(&f, "S2 src/xc/lda.rs::GAMMA has an empty citation"),
            "{f:?}"
        );
        assert!(has(&f, "src/xc/lda.rs:1: S2 0.1423"), "{f:?}");
    }
}

#[test]
fn s2_checks_stale_citations_when_the_table_is_the_only_source_file() {
    let table = format!("[\"src/xc/lda.rs\"]\n{GAMMA}");
    let root = tree(
        "s2-table-only",
        &[("src/SOURCE.toml", &table), ("benchmarks/.keep", "")],
    );
    let findings = lever(&["science"], &root);
    assert!(
        has(&findings, "S2 src/xc/lda.rs::GAMMA is not a constant"),
        "{findings:?}"
    );
}

#[test]
fn s2_rejects_duplicate_citation_keys() {
    let f = cited_engine(
        "s2-duplicate",
        "const GAMMA: f64 = -0.1423;\n",
        &GAMMA.repeat(2),
    );
    assert!(has(&f, "src/SOURCE.toml: S2"), "{f:?}");
    assert!(has(&f, "duplicate key"), "{f:?}");
    assert!(has(&f, "S2 0.1423"), "{f:?}");
}

#[test]
fn s2_ignores_a_source_comment_on_a_const() {
    let f = engine(
        "s2-comment",
        "src/xc/lda.rs",
        "/// source: Perdew and Zunger 1981, Table XII.\nconst GAMMA: f64 = -0.1423;\n",
    );
    assert!(has(&f, "src/xc/lda.rs:2: S2 0.1423"), "{f:?}");
}

#[test]
fn s2_checks_const_generics_even_when_the_name_has_a_citation() {
    let f = cited_engine(
        "s2-const-generic",
        "fn gamma<#[allow(dead_code)] const GAMMA: usize>() -> f64 { -0.1423 }\n",
        GAMMA,
    );
    assert!(
        has(&f, "S2 src/xc/lda.rs::GAMMA is not a constant"),
        "{f:?}"
    );
    assert!(has(&f, "S2 0.1423"), "{f:?}");
}

#[test]
fn s2_leaves_a_constant_like_macro_argument_uncited() {
    let findings = cited_engine(
        "s2-macro-argument",
        "macro_rules! consume {\n    (tag const $name:ident: $kind:ty = $value:expr) => {};\n}\nconsume!(tag const GAMMA: f64 = 0.1423);\n",
        GAMMA,
    );
    assert!(
        has(&findings, "S2 src/xc/lda.rs::GAMMA is not a constant"),
        "{findings:?}"
    );
    assert!(has(&findings, "src/xc/lda.rs:4: S2 0.1423"), "{findings:?}");
}

#[test]
fn s2_counts_an_anonymous_const_as_an_expression() {
    let f = cited_engine(
        "s2-anonymous",
        "const _: f64 = 0.1423;\n",
        "_ = \"No name.\"\n",
    );
    assert!(has(&f, "S2 src/xc/lda.rs::_ is not a constant"), "{f:?}");
    assert!(has(&f, "S2 0.1423"), "{f:?}");
}

#[test]
fn s3_catches_unit_names_in_any_spelling() {
    let body = "pub fn a(energy_electronvolts: f64, length_ang: f64) -> f64 {\n    energy_electronvolts + length_ang\n}\n/// source: CODATA 2018.\npub const HA2EV: f64 = 2.0;\n";
    let f = engine("s3-spelling", "src/ks/m.rs", body);
    for name in ["energy_electronvolts", "length_ang", "HA2EV"] {
        assert!(
            has(&f, &format!("S3 `{name}` names a non-atomic unit")),
            "{name}: {f:?}"
        );
    }
}

#[test]
fn s3_keeps_unit_conversion_factors_in_io() {
    let constant = "/// source: CODATA 2018.\npub const BOHR_ANGSTROM: f64 = 0.529_177_210_903;\n";
    let f = engine("s3-unit", "src/ks/m.rs", constant);
    assert!(
        has(&f, "S3 0.529_177_210_903 is a unit conversion factor"),
        "{f:?}"
    );
    let f = engine("s3-unit-io", "src/io/units.rs", constant);
    assert!(!has(&f, "unit conversion"), "{f:?}");
}

#[test]
fn s3_catches_paths_case_names_and_units_outside_io() {
    let f = engine(
        "s3",
        "src/ks/m.rs",
        "#[path = \"../../probe.rs\"]\nmod probe;\nconst HARTREE_EV: f64 = 1.0;\npub fn a() -> &'static str {\n    \"/opt/orbitaR/benchmarks/2_madelung\"\n}\n",
    );
    assert!(has(&f, "src/ks/m.rs:1: S3 a `path =` attribute"), "{f:?}");
    assert!(has(&f, "S3 `HARTREE_EV` names a non-atomic unit"));
    assert!(has(&f, "S3 engine code names an absolute path"));
    assert!(has(&f, "S3 engine code names a benchmark path or case"));
    let io = engine(
        "s3-io",
        "src/io/units.rs",
        "pub(in crate::io) const BOHR_PER_ANGSTROM: f64 = 2.0;\n",
    );
    assert!(!has(&io, "S3"));
}

#[test]
fn s4_catches_element_literals_in_any_case_outside_io() {
    let f = engine(
        "s4",
        "src/ks/m.rs",
        "pub fn a(s: &str) -> bool {\n    \"na\" == s || s.eq(\"Cl\")\n}\n",
    );
    assert!(has(&f, "S4 \"na\""));
    assert!(has(&f, "S4 \"Cl\""));
    assert!(!has(
        &engine(
            "s4-io",
            "src/io/m.rs",
            "pub fn a() -> &'static str {\n    \"Si\"\n}\n"
        ),
        "S4"
    ));
}

#[test]
fn s6_catches_vasp_files_by_name_and_by_content() {
    let root = tree(
        "s6",
        &[
            ("benchmarks/1_a/baseline/POTCAR", "PAW_PBE Si"),
            (
                "benchmarks/1_a/run.log",
                " vasp.6.4.2 20Jul23 (build Oct 01 2024) complex\n",
            ),
            ("notes/si.dat", "  TITEL  = PAW_PBE Si 05Jan2001\n"),
            ("benchmarks/1_a/KPOINTS", "Gamma\n"),
        ],
    );
    let f = lever(&["science"], &root);
    for file in [
        "benchmarks/1_a/baseline/POTCAR",
        "benchmarks/1_a/run.log",
        "notes/si.dat",
        "benchmarks/1_a/KPOINTS",
    ] {
        assert!(has(&f, &format!("S6 {file}")), "{file}: {f:?}");
    }
}

#[test]
fn s6_catches_renamed_compressed_and_tagged_files() {
    let root = tree(
        "s6-more",
        &[
            ("benchmarks/1_a/OUTCAR.relax", "x"),
            ("benchmarks/1_a/vasprun.xml.bak", "x"),
            ("notes/run1.xml", "<?xml version=\"1.0\"?>\n<modeling>\n"),
            ("notes/params.txt", "ENCUT = 520\nISMEAR = 0\n"),
            ("notes/encut.txt", "ENCUT = 520\n"),
            ("notes/plain.toml", "alpha = 1\nbeta = 2\n"),
            ("notes/si_paw.txt", "<paw_dataset version=\"0.7\">\n"),
            ("notes/si_upf.txt", "<UPF version=\"2.0.1\">\n"),
            ("src/model/kpoints.rs", "pub struct Mesh;\n"),
        ],
    );
    std::fs::write(root.join("notes/blob.bin"), [0x1f, 0x8b, 0x08, 0x00]).unwrap();
    std::fs::write(
        root.join("notes/si.zip"),
        b"PK\x03\x04\x14\x00\x00\x00POTCAR",
    )
    .unwrap();
    std::fs::write(root.join("notes/a.out"), b"\x7fELF\x02\x01\x01\x00").unwrap();
    std::fs::write(root.join("notes/runs.tar"), b"OUTCAR\x00\x00\x00 vasp.6\n").unwrap();
    let f = lever(&["science"], &root);
    for file in [
        "benchmarks/1_a/OUTCAR.relax",
        "benchmarks/1_a/vasprun.xml.bak",
        "notes/run1.xml",
        "notes/params.txt",
        "notes/si_paw.txt",
        "notes/si_upf.txt",
    ] {
        assert!(has(&f, &format!("S6 {file}")), "{file}: {f:?}");
    }
    assert!(
        !has(&f, "S6 notes/plain.toml"),
        "two settings are not two INCAR tags: {f:?}"
    );
    for file in [
        "notes/blob.bin",
        "notes/si.zip",
        "notes/a.out",
        "notes/runs.tar",
    ] {
        assert!(
            has(&f, &format!("S6 {file} is not UTF-8 text")),
            "{file}: {f:?}"
        );
    }
    assert!(
        !has(&f, "S6 notes/encut.txt"),
        "one tag is not an INCAR: {f:?}"
    );
    assert!(
        !has(&f, "S6 src/model/kpoints.rs"),
        "a module name is code: {f:?}"
    );
}

#[test]
fn s1_reads_raw_strings_and_ignores_raw_identifiers() {
    let body = "pub fn m() -> &'static str {\n    r#\"1.747564594633182\"#\n}\npub fn r#type() -> u8 {\n    1\n}\n";
    let f = engine("s1-raw", "src/ks/m.rs", body);
    assert!(has(&f, "S1 the string \"1.747564594633182\""), "{f:?}");
    assert!(!has(&f, "does not tokenize"), "{f:?}");
}

#[test]
fn s1_s2_decode_escapes_before_they_read_a_string() {
    let body = "pub fn m() -> f64 {\n    \"1\\x2e747564594633182\".parse().unwrap()\n}\npub fn n() -> &'static [u8] {\n    b\"\\x31.747564594633182\"\n}\npub fn o() -> &'static str {\n    \"\\u{31}.7475\\u{36}4594633182\"\n}\npub fn p() -> &'static core::ffi::CStr {\n    c\"\\x31.747564594633182\"\n}\n";
    let f = engine("s1-escape", "src/ks/m.rs", body);
    for line in [2, 5, 8, 11] {
        let needle = format!("src/ks/m.rs:{line}: S1 the string \"1.747564594633182\"");
        assert!(has(&f, &needle), "{needle}: {f:?}");
    }
}

#[test]
fn a_file_that_does_not_tokenize_is_a_finding() {
    let f = engine("s-lex", "src/ks/m.rs", "pub fn m() {\n    \"open\n}\n");
    assert!(has(&f, "science: src/ks/m.rs does not tokenize"), "{f:?}");
}

#[test]
fn s3_catches_embedded_files_and_attribute_paths() {
    let files = [
        (
            "src/ks/a.rs",
            "pub fn a() -> f64 {\n    include!(\"../../vendor/value.in\")\n}\n",
        ),
        (
            "src/ks/b.rs",
            "#[cfg_attr(all(), path = \"../../vendor/leak.rs\")]\nmod leak;\n",
        ),
        (
            "src/ks/c.rs",
            "#[doc = include_str!(\"../../README.md\")]\npub fn c() {}\n",
        ),
        ("src/ks/d.rs", "pub const D: &str = env!(\"HOME\");\n"),
        (
            "src/ks/e.rs",
            "pub const E: &[u8] = std::include_bytes!(\"blob\");\n",
        ),
        ("src/ks/f.rs", "macro_rules! keep {\n    () => {};\n}\n"),
        ("benchmarks/1_a/case.toml", CASE),
    ];
    let f = lever(&["science"], &tree("s3-embed", &files));
    for needle in [
        "src/ks/a.rs:2: S3 `include!`",
        "src/ks/b.rs:1: S3 a `path =` attribute",
        "src/ks/c.rs:1: S3 `include_str!`",
        "src/ks/d.rs:1: S3 `env!`",
        "src/ks/e.rs:1: S3 `include_bytes!`",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
    assert!(!has(&f, "src/ks/f.rs"), "{f:?}");
}

#[test]
fn s3_lets_only_two_files_lift_a_containment_lint() {
    let lift =
        "#[expect(clippy::disallowed_methods, reason = \"reads the input\")]\npub fn r() {}\n";
    let files = [
        ("src/ks/q.rs", lift),
        ("src/io/run.rs", lift),
        (
            "src/main.rs",
            "#[expect(clippy::style, reason = \"x\")]\nfn main() {}\n",
        ),
        ("src/ks/w.rs", "#![cfg_attr(all(), expect(warnings))]\n"),
        (
            "src/ks/t.rs",
            "#[cfg_attr(unix, expect(clippy::disallowed_types))]\npub fn t() {}\n",
        ),
        (
            "src/ks/u.rs",
            "#[expect(clippy::too_many_arguments, reason = \"x\")]\npub fn u() {}\n",
        ),
        ("benchmarks/1_a/case.toml", CASE),
    ];
    let f = lever(&["science"], &tree("s3-lift", &files));
    assert!(
        !has(&f, "science: src/ks/u.rs"),
        "any other lint may be expected: {f:?}"
    );
    for needle in [
        "src/ks/q.rs:1: S3 an attribute names `clippy::disallowed_methods`",
        "src/main.rs:1: S3 an attribute names `clippy::style`",
        "src/ks/w.rs:1: S3 an attribute names `warnings`",
        "src/ks/t.rs:1: S3 an attribute names `clippy::disallowed_types`",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
    assert!(!has(&f, "science: src/io/run.rs"), "{f:?}");
}

#[test]
fn s3_seals_a_measure_from_files_processes_and_golden_runs() {
    let runner = "use xtask::harness::{self, Measure};\n\n// cases: begin (generated by `cargo xtask new-case`; `cargo xtask layout` checks it)\n#[path = \"benchmarks/1_a/case.rs\"]\nmod n1_a;\n\nconst CASES: &[(&str, Measure)] = &[(\"1_a\", n1_a::measure)];\n// cases: end\n\n#[path = \"../scratch/golden.rs\"]\nmod golden;\n\nfn main() {\n    harness::main(CASES);\n}\n";
    let measure = "use std::{collections::BTreeMap, fs};\nuse std as s;\npub fn measure() -> usize {\n    let golden = include_str!(\"golden.txt\");\n    let m: BTreeMap<u8, u8> = BTreeMap::new();\n    golden.len() + m.len() + fs::read(\"x\").map_or(0, |b| b.len())\n}\npub fn run() {\n    let _ = ::std::process::Command::new(\"cat\").status();\n}\n";
    let files = [
        ("tests/benchmarks.rs", runner),
        ("tests/benchmarks/1_a/case.rs", measure),
        ("benchmarks/1_a/case.toml", CASE),
    ];
    let f = lever(&["science"], &tree("s3-measure", &files));
    for needle in [
        "tests/benchmarks.rs:10: S3 a `path =` attribute",
        "tests/benchmarks/1_a/case.rs:1: S3 a measure names std::fs",
        "tests/benchmarks/1_a/case.rs:2: S3 a measure renames std",
        "tests/benchmarks/1_a/case.rs:4: S3 `include_str!`",
        "tests/benchmarks/1_a/case.rs:9: S3 a measure names std::process",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
    assert!(
        !has(&f, "tests/benchmarks.rs:4"),
        "the generated table is layout's: {f:?}"
    );
}

#[test]
fn s6_catches_a_file_tracked_under_a_skipped_root() {
    let root = tree(
        "s6-tracked",
        &[
            ("target/POTCAR", "PAW_PBE Si"),
            ("scratch/notes.txt", "x"),
            ("benchmarks/1_a/case.toml", CASE),
        ],
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-f", "target/POTCAR"]);
    let f = lever(&["science"], &root);
    assert!(has(&f, "S6 git tracks target/POTCAR"), "{f:?}");
    assert!(
        !has(&f, "scratch/notes.txt"),
        "an untracked private file is fine: {f:?}"
    );
}

#[test]
fn s7_catches_a_judge_that_links_the_engine() {
    let root = tree(
        "s7",
        &[(
            "xtask/Cargo.toml",
            "[package]\nname = \"xtask\"\n\n[dev-dependencies]\nengine = { package = \"orbitar\", path = \"..\" }\n",
        )],
    );
    std::fs::create_dir_all(root.join("benchmarks")).unwrap();
    assert!(has(
        &lever(&["science"], &root),
        "S7 xtask [dev-dependencies] engine links the engine"
    ));
}

#[test]
fn s8_needs_a_typed_source_whose_kind_matches_the_case() {
    let root = tree(
        "s8",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/reference/values.json", "[1.234]"),
            ("benchmarks/2_b/case.toml", CASE),
            ("benchmarks/2_b/reference/values.json", "[1.234]"),
            (
                "benchmarks/2_b/reference/SOURCE.toml",
                &SOURCE
                    .replace("literature", "oracle")
                    .replace("oeis:", "see "),
            ),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(has(
        &f,
        "S8 benchmarks/1_a/reference/SOURCE.toml needs kind"
    ));
    assert!(has(
        &f,
        "S8 benchmarks/2_b/reference/SOURCE.toml locator must start"
    ));
    assert!(has(
        &f,
        "kind oracle differs from case.toml reference literature"
    ));
}

const ACTIVE: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nerror_max = 1e-12\n";

fn base(name: &str) -> Tree {
    base_with(name, ACTIVE)
}

fn base_with(name: &str, case: &str) -> Tree {
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", case),
            ("benchmarks/2_b/case.toml", ACTIVE),
            ("tests/benchmarks/1_a/case.rs", "// measure\n"),
            ("xtask/src/harness.rs", "// judge\n"),
        ],
    );
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "base"], &root).is_empty());
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    root
}

fn eased(root: &Tree, file: &str, body: &str) -> Vec<String> {
    let old = crate::lock_files(root);
    write(root, file, body);
    forge(root, &old)
}

fn forge(root: &Tree, old: &std::collections::BTreeMap<String, String>) -> Vec<String> {
    assert!(lever(&["lock", "--reviewer", "self", "--reason", "rehash"], root).is_empty());
    for (path, text) in crate::lock_files(root) {
        let Some(was) = old.get(&path) else { continue };
        let was: toml::Table = was.parse().unwrap();
        let mut entry: toml::Table = text.parse().unwrap();
        for key in ["reviewer", "reason"] {
            entry.insert(key.into(), was[key].clone());
        }
        write(root, &path, &toml::to_string(&entry).unwrap());
    }
    lever(&["science", "--base", "HEAD"], root)
}

const STRICT: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\ntimeout_s = 5\nrepeats = 3\n\n[tolerance]\nerror_max = 1e-12\nrate_min = 2.0\n";

#[test]
fn s9_names_each_way_a_case_can_get_easier() {
    let edits = [
        (
            ("status = \"active\"", "status = \"planned\""),
            "status left active",
        ),
        (("tier = \"push\"", "tier = \"pr\""), "tier push → pr"),
        (
            ("\"analytic\"", "\"literature\""),
            "reference analytic → literature",
        ),
        (("[\"correctness\"]", "[\"accuracy\"]"), "claims changed"),
        (
            ("timeout_s = 5", "timeout_s = 10"),
            "timeout or repeats got easier",
        ),
        (
            ("repeats = 3", "repeats = 1"),
            "timeout or repeats got easier",
        ),
        (("rate_min = 2.0\n", ""), "rate_min removed"),
        (("rate_min = 2.0", "rate_min = 1.0"), "rate_min 2 → 1"),
    ];
    for (n, ((old, new), detail)) in edits.into_iter().enumerate() {
        let root = base_with(&format!("s9-each-{n}"), STRICT);
        let f = eased(&root, "benchmarks/1_a/case.toml", &STRICT.replace(old, new));
        assert!(
            has(&f, &format!("S9 1_a got easier ({detail}")),
            "{detail}: {f:?}"
        );
    }
}

#[test]
fn s9_accepts_the_first_import_onto_a_base_without_benchmarks() {
    let root = tree("s9-first", &[("README.md", "# before\n")]);
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    write(&root, "benchmarks/1_a/case.toml", ACTIVE);
    assert!(
        lever(
            &["lock", "--reviewer", "owner", "--reason", "import"],
            &root
        )
        .is_empty()
    );
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "S9"), "{f:?}");
}

#[test]
fn s9_needs_a_lock_once_the_base_has_benchmarks() {
    let root = tree("s9-unlocked", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "base"]);
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "late"], &root).is_empty());
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(has(&f, "S9 cannot read benchmarks/locks at HEAD"), "{f:?}");
    let f = lever(&["science", "--base", "no-such-base"], &root);
    assert!(
        has(&f, "S9 cannot read benchmarks/locks at no-such-base"),
        "{f:?}"
    );
}

#[test]
fn s9_catches_a_lost_id_and_a_reused_number() {
    let root = base("s9-ids");
    std::fs::remove_dir_all(root.join("benchmarks/2_b")).unwrap();
    write(&root, "benchmarks/2_c/case.toml", ACTIVE);
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(has(&f, "S9 2_b exists at HEAD but not here"));
    assert!(has(&f, "S9 new case 2_c must use a number above 2"));
}

#[test]
fn s9_catches_easing_that_keeps_the_old_review_line() {
    let root = base("s9-easier");
    let old = crate::lock_files(&root);
    write(
        &root,
        "benchmarks/1_a/case.toml",
        &ACTIVE.replace("1e-12", "1e-6"),
    );
    write(&root, "xtask/src/harness.rs", "// a kinder judge\n");
    let f = forge(&root, &old);
    assert!(
        has(
            &f,
            "S9 the harness changed without a new lock review: xtask/src/harness.rs"
        ),
        "{f:?}"
    );
    assert!(has(&f, "S9 1_a got easier"), "{f:?}");
    assert!(!has(&f, "S5"), "{f:?}");
}

#[test]
fn s9_catches_a_rewritten_measure_that_keeps_the_old_review_line() {
    let root = base("s9-measure");
    let f = eased(
        &root,
        "tests/benchmarks/1_a/case.rs",
        "// a kinder measure\n",
    );
    assert!(
        has(
            &f,
            "S9 1_a changed without a new lock review: tests/benchmarks/1_a/case.rs"
        ),
        "{f:?}"
    );
    assert!(!has(&f, "S9 2_b"), "{f:?}");
    assert!(!has(&f, "S9 the harness changed"), "{f:?}");
}

fn legacy_base(name: &str) -> Tree {
    let root = base(name);
    let mut old = String::new();
    for (block, files) in [
        ("harness", vec!["xtask/src/harness.rs"]),
        (
            "1_a",
            vec!["benchmarks/1_a/case.toml", "tests/benchmarks/1_a/case.rs"],
        ),
        ("2_b", vec!["benchmarks/2_b/case.toml"]),
    ] {
        let entry = crate::lock_entry(&root, block, files[0]);
        old.push_str(&format!(
            "# {block} reviewer={} reason={}\n",
            entry["reviewer"].as_str().unwrap(),
            entry["reason"].as_str().unwrap()
        ));
        for file in files {
            let sha = crate::lock_entry(&root, block, file)["sha256"].clone();
            old.push_str(&format!("{}  {file}\n", sha.as_str().unwrap()));
        }
    }
    let moved = crate::lock_files(&root);
    std::fs::remove_dir_all(root.join("benchmarks/locks")).unwrap();
    write(&root, "benchmarks/LOCK", &old);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "old lock"]);
    std::fs::remove_file(root.join("benchmarks/LOCK")).unwrap();
    for (path, text) in &moved {
        write(&root, path, text);
    }
    root
}

fn both_bases(name: &str) -> [Tree; 2] {
    [
        base(&format!("{name}-split")),
        legacy_base(&format!("{name}-legacy")),
    ]
}

fn review(root: &Tree, block: &str, file: &str, reviewer: &str, reason: &str) {
    let path = format!("benchmarks/locks/{block}/{file}.lock");
    let mut entry: toml::Table = std::fs::read_to_string(root.join(&path))
        .unwrap()
        .parse()
        .unwrap();
    entry.insert("reviewer".into(), reviewer.into());
    entry.insert("reason".into(), reason.into());
    write(root, &path, &toml::to_string(&entry).unwrap());
}

#[test]
fn s9_reads_an_old_one_file_lock_at_the_base() {
    let root = legacy_base("s9-legacy");
    let moved = crate::lock_files(&root);
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "S9") && !has(&f, "S5"), "{f:?}");
    let f = forge(&root, &moved);
    assert!(!has(&f, "S9"), "{f:?}");
    let f = eased(&root, "xtask/src/harness.rs", "// a kinder judge\n");
    assert!(
        has(
            &f,
            "S9 the harness changed without a new lock review: xtask/src/harness.rs"
        ),
        "{f:?}"
    );
}

#[test]
fn s9_needs_a_new_review_for_a_new_entry_whatever_base_review_it_copies() {
    for root in both_bases("s9-added") {
        write(&root, "xtask/src/probe.rs", "// probe\n");
        assert!(lever(&["lock", "--reviewer", "self", "--reason", "probe"], &root).is_empty());
        review(&root, "harness", "xtask/src/probe.rs", "owner", "base");
        let f = lever(&["science", "--base", "HEAD"], &root);
        assert!(
            has(
                &f,
                "S9 the harness changed without a new lock review: xtask/src/probe.rs"
            ),
            "{f:?}"
        );
        review(&root, "harness", "xtask/src/probe.rs", "second", "a probe");
        let f = lever(&["science", "--base", "HEAD"], &root);
        assert!(!has(&f, "S9") && !has(&f, "S5"), "{f:?}");
    }
}

#[test]
fn s9_needs_a_new_review_for_a_file_that_moves_between_cases_and_reads_a_review_without_its_spacing()
 {
    for root in both_bases("s9-moved") {
        std::fs::remove_file(root.join("tests/benchmarks/1_a/case.rs")).unwrap();
        write(&root, "tests/benchmarks/2_b/case.rs", "// measure\n");
        assert!(lever(&["lock", "--reviewer", "mover", "--reason", "moved"], &root).is_empty());
        let f = lever(&["science", "--base", "HEAD"], &root);
        assert!(!has(&f, "S9") && !has(&f, "S5"), "{f:?}");
        review(
            &root,
            "2_b",
            "tests/benchmarks/2_b/case.rs",
            "owner",
            "base",
        );
        let f = lever(&["science", "--base", "HEAD"], &root);
        assert!(
            has(
                &f,
                "S9 2_b changed without a new lock review: tests/benchmarks/2_b/case.rs"
            ),
            "{f:?}"
        );
        assert!(!has(&f, "S9 1_a"), "{f:?}");
        write(&root, "xtask/src/harness.rs", "// a kinder judge\n");
        assert!(lever(&["lock", "--reviewer", "self", "--reason", "rehash"], &root).is_empty());
        review(
            &root,
            "harness",
            "xtask/src/harness.rs",
            " owner",
            "base \n",
        );
        let f = lever(&["science", "--base", "HEAD"], &root);
        assert!(
            has(
                &f,
                "S9 the harness changed without a new lock review: xtask/src/harness.rs"
            ),
            "{f:?}"
        );
    }
}

#[test]
fn s9_needs_a_tombstone_for_a_dropped_entry_and_lets_a_tombstone_go() {
    let root = base("s9-dropped");
    let entry = "benchmarks/locks/1_a/tests/benchmarks/1_a/case.rs.lock";
    std::fs::remove_file(root.join("tests/benchmarks/1_a/case.rs")).unwrap();
    std::fs::remove_file(root.join(entry)).unwrap();
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(
        has(
            &f,
            "S9 1_a changed without a new lock review: tests/benchmarks/1_a/case.rs"
        ),
        "{f:?}"
    );
    git(&root, &["checkout", "--", entry]);
    assert!(lever(&["lock", "--reviewer", "second", "--reason", "drop"], &root).is_empty());
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "S9 1_a") && !has(&f, "S5"), "{f:?}");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "tombstone"]);
    std::fs::remove_file(root.join(entry)).unwrap();
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "S9 1_a"), "{f:?}");
}

#[test]
fn s1_folds_through_a_cast_and_reads_exponents_inside_strings() {
    let body = "pub fn m() -> f64 {\n    1 as f64 + 0.747_564_594_633_182\n}\npub fn n() -> &'static str {\n    \"m = 17.475_645_946_331_82e-1, then 1.0e+0\"\n}\npub fn k() -> f32 {\n    1 as f32 + 0.747_564_594_633_182\n}\n";
    let f = engine("s1-cast", "src/ks/m.rs", body);
    for line in [2, 8] {
        assert!(
            has(
                &f,
                &format!(
                    "src/ks/m.rs:{line}: S1 a literal expression evaluates to a benchmark reference value"
                )
            ),
            "{line}: {f:?}"
        );
    }
    assert!(has(&f, "src/ks/m.rs:5: S1 the string"), "{f:?}");
}

#[test]
fn s3_lets_a_measure_rename_anything_but_std() {
    let measure = "use xtask::harness::{Evidence as E, Metrics};\nuse std::collections::BTreeMap as Map;\npub fn measure(_: &E) -> Result<Metrics, String> {\n    let _m: Map<u8, u8> = Map::new();\n    Err(String::new())\n}\n";
    let files = [
        ("tests/benchmarks/1_a/case.rs", measure),
        ("benchmarks/1_a/case.toml", CASE),
    ];
    let f = lever(&["science"], &tree("s3-measure-rename", &files));
    assert!(!has(&f, "tests/benchmarks/1_a/case.rs"), "{f:?}");
}

#[test]
fn s2_spares_short_floats_and_integers_below_seven_digits() {
    let body = "pub const N: usize = 512;\npub fn half() -> f64 {\n    0.5 * 1.5 + 123_456.0 * 0.0\n}\npub const BIG: u64 = 1_234_567;\n";
    let f = engine("s2-short", "src/ks/m.rs", body);
    assert!(
        !has(&f, "src/ks/m.rs:1"),
        "a 3-digit integer is fine: {f:?}"
    );
    assert!(
        has(&f, "src/ks/m.rs:3: S2 123_456.0"),
        "a 6-digit float needs a source: {f:?}"
    );
    assert!(
        !has(&f, "S2 0.5") && !has(&f, "S2 1.5 "),
        "two digits are fine: {f:?}"
    );
    assert!(has(&f, "src/ks/m.rs:5: S2 1_234_567"), "{f:?}");
}

#[test]
fn s3_flags_a_conversion_factor_but_not_its_multiple() {
    let body = "pub const TWICE: f64 = 54.422_772_491_976;\npub const EV: f64 = 27.211_386_245_988;\npub const ROUNDED: f64 = 27.211_5;\n";
    let sources = "TWICE = \"Twice the Hartree energy in electron volts (CODATA 2018).\"\nEV = \"CODATA 2018, Hartree energy in eV.\"\nROUNDED = \"Rounded Hartree energy in eV (CODATA 2018).\"\n";
    let f = cited_engine("s3-multiple", body, sources);
    assert!(
        !has(&f, "src/xc/lda.rs:1"),
        "twice a factor is not a factor: {f:?}"
    );
    assert!(
        has(
            &f,
            "src/xc/lda.rs:2: S3 27.211_386_245_988 is a unit conversion factor"
        ),
        "{f:?}"
    );
    assert!(
        has(
            &f,
            "src/xc/lda.rs:3: S3 27.211_5 is a unit conversion factor"
        ),
        "a rounded factor is one too: {f:?}"
    );
}

#[test]
fn s2_reads_the_same_digit_rule_inside_strings() {
    let body = "pub fn s() -> [&'static str; 5] {\n    [\"0.5\", \"512\", \"123456\", \"1.25\", \"1234567\"]\n}\n";
    let f = engine("s2-strings", "src/ks/m.rs", body);
    for short in ["0.5", "512", "123456"] {
        assert!(
            !has(&f, &format!("hides the number {short};")),
            "{short}: {f:?}"
        );
    }
    for long in ["1.25", "1234567"] {
        assert!(
            has(&f, &format!("hides the number {long};")),
            "{long}: {f:?}"
        );
    }
}

#[test]
fn s2_finds_where_a_number_in_a_string_starts_and_ends() {
    let cases = [
        (".125", ".125"),
        ("1.25e5", "1.25e5"),
        ("1.25e-5", "1.25e-5"),
        ("123.e5", "123.e5"),
        ("1.25e5 eV", "1.25e5"),
        ("1.25e-x", "1.25"),
        ("1.25,5", "1.25"),
        ("1.25.5", "1.25"),
        ("1234567 atoms", "1234567"),
        ("v1234567", ""),
        ("id_1234567", ""),
        ("_1234567", ""),
    ];
    let body: String = cases
        .iter()
        .enumerate()
        .map(|(k, (s, _))| format!("pub const TEXT_{k}: &str = \"{s}\";\n"))
        .collect();
    let f = engine("s2-edges", "src/ks/m.rs", &body);
    for (k, (s, number)) in cases.iter().enumerate() {
        let at = format!(
            "src/ks/m.rs:{}: S2 the string \"{s}\" hides the number",
            k + 1
        );
        if number.is_empty() {
            assert!(!has(&f, &at), "{s}: {f:?}");
        } else {
            assert!(has(&f, &format!("{at} {number};")), "{s}: {f:?}");
        }
    }
}

#[test]
fn s3_reads_each_benchmark_and_absolute_path_marker_alone() {
    let body = r#"pub const BENCH: &str = "benchmarks";
pub const CASE: &str = "case.toml";
pub const UNIX: &str = "/data";
pub const UNC: &str = "\\\\server\\share";
pub const DRIVE: &str = "C:\\data";
pub const RELATIVE: &str = "data/x";
"#;
    let f = engine("s3-markers", "src/ks/m.rs", body);
    for line in [1, 2] {
        assert!(
            has(
                &f,
                &format!("src/ks/m.rs:{line}: S3 engine code names a benchmark path or case")
            ),
            "{line}: {f:?}"
        );
    }
    for line in [3, 4, 5] {
        assert!(
            has(
                &f,
                &format!("src/ks/m.rs:{line}: S3 engine code names an absolute path")
            ),
            "{line}: {f:?}"
        );
    }
    assert!(!has(&f, "src/ks/m.rs:6: S3"), "{f:?}");
}

#[test]
fn s2_refuses_an_expression_below_a_source_comment() {
    let body = "pub fn a() -> f64 {\n    // source: a test value.\n    1.25\n}\n";
    let f = engine("s2-let", "src/ks/m.rs", body);
    assert!(
        has(&f, "src/ks/m.rs:3: S2 1.25 needs a named const"),
        "{f:?}"
    );
}

#[test]
fn s1_flags_a_value_near_a_reference_but_not_its_double() {
    let body = "pub fn near() -> f64 {\n    1.747_565_468_415_479\n}\npub fn double() -> f64 {\n    3.495_129_189_266_364\n}\n";
    let f = engine("s1-near", "src/ks/m.rs", body);
    assert!(
        has(
            &f,
            "src/ks/m.rs:2: S1 1.747_565_468_415_479 equals a benchmark reference value"
        ),
        "{f:?}"
    );
    assert!(!has(&f, "src/ks/m.rs:5: S1"), "{f:?}");
}

#[test]
fn s7_reads_each_way_a_dependency_can_name_the_engine() {
    let root = tree(
        "s7-ways",
        &[(
            "xtask/Cargo.toml",
            "[package]\nname = \"xtask\"\n\n[dependencies]\norbitar = \"0.1\"\nalias = { package = \"orbitar\" }\nparent = { path = \"../\" }\ntoml = \"1\"\nvendored = { path = \"vendor/toml\" }\n",
        )],
    );
    std::fs::create_dir_all(root.join("benchmarks")).unwrap();
    let f = lever(&["science"], &root);
    for name in ["orbitar", "alias", "parent"] {
        assert!(
            has(
                &f,
                &format!("S7 xtask [dependencies] {name} links the engine")
            ),
            "{name}: {f:?}"
        );
    }
    for name in ["toml", "vendored"] {
        assert!(
            !has(&f, &format!("[dependencies] {name} ")),
            "{name}: {f:?}"
        );
    }
}

#[test]
fn s1_folds_each_operator_and_reports_a_lone_literal_once() {
    let body = "pub fn a() -> f64 {\n    0.25 * (4.0 - 0.504_870_810_733_636) * 2.0\n}\npub fn b() -> f64 {\n    3.495_129_189_266_364 / 2.0\n}\npub fn c() -> f64 {\n    0.5 * -(0.504_870_810_733_636 - 4.0)\n}\npub fn d(x: f64) -> f64 {\n    1.747_564_594_633_182 * x\n}\n";
    let f = engine("s1-fold", "src/ks/m.rs", body);
    for line in [2, 5, 8] {
        assert!(
            has(&f, &format!("src/ks/m.rs:{line}: S1 a literal expression")),
            "{line}: {f:?}"
        );
    }
    assert!(
        has(&f, "src/ks/m.rs:11: S1 1.747_564_594_633_182 equals"),
        "{f:?}"
    );
    assert!(
        !has(&f, "src/ks/m.rs:11: S1 a literal expression"),
        "a lone literal is reported once: {f:?}"
    );
}

#[test]
fn s2_reads_suffixed_floats_and_skips_char_literals() {
    let body = "pub fn a() -> f64 {\n    1.25f64\n}\npub fn b() -> f32 {\n    125f32\n}\npub fn c() -> char {\n    '\\u{123E}'\n}\n/// Perdew and Zunger 1981 fit: gamma = -0.1423.\npub fn d() {}\n";
    let f = engine("s2-suffix", "src/ks/m.rs", body);
    assert!(has(&f, "src/ks/m.rs:2: S2 1.25f64"), "{f:?}");
    assert!(has(&f, "src/ks/m.rs:5: S2 125f32"), "{f:?}");
    assert!(!has(&f, "src/ks/m.rs:8:"), "a char is not a number: {f:?}");
    assert!(!has(&f, "0.1423"), "a doc comment may cite a number: {f:?}");
}

#[test]
fn s1_takes_reference_values_from_reference_data_only() {
    let case = format!("{CASE}budget_s = 125\n");
    let source = SOURCE.replace("OEIS A085469", "Madelung 1918");
    let body = "/// source: a test value.\npub const YEAR: f64 = 1918.0;\n/// source: a test value.\npub const BUDGET: f64 = 125.0;\n";
    let root = tree(
        "s1-reference-only",
        &[
            ("src/ks/m.rs", body),
            ("benchmarks/2_madelung/case.toml", case.as_str()),
            ("benchmarks/2_madelung/reference/madelung.json", REFERENCE),
            (
                "benchmarks/2_madelung/reference/SOURCE.toml",
                source.as_str(),
            ),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        !has(&f, "S1"),
        "a case or a citation holds no reference value: {f:?}"
    );
}

#[test]
fn s3_reads_only_a_path_key_as_a_path_attribute() {
    let body = "/// A doc comment is `doc = \"…\"`.\n#[cfg(feature = \"x\")]\npub fn a() {}\n#[cfg_attr(test, path = \"other.rs\")]\nmod b;\n";
    let f = engine("s3-path-key", "src/ks/m.rs", body);
    assert!(has(&f, "src/ks/m.rs:4: S3 a `path =` attribute"), "{f:?}");
    for line in [1, 2] {
        assert!(
            !has(&f, &format!("src/ks/m.rs:{line}: S3 a `path =`")),
            "{line}: {f:?}"
        );
    }
}

#[test]
fn s1_reads_an_integer_in_any_radix_by_its_value() {
    let body = "/// source: a test value.\npub const SCALE: f64 = 0x6356696e299de_u64 as f64 / 1e15;\npub const MASK: u8 = 0xFF;\n";
    let f = engine("s1-radix", "src/ks/m.rs", body);
    assert!(
        has(
            &f,
            "src/ks/m.rs:2: S1 a literal expression evaluates to a benchmark reference value"
        ),
        "{f:?}"
    );
    assert!(!has(&f, "src/ks/m.rs:3"), "{f:?}");
}

#[test]
fn s9_keeps_each_family_table_of_the_base_byte_for_byte() {
    let root = base("s9-families");
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "datasets/"), "{f:?}");
    let f = lever(&["science", "--base", "no-such-base"], &root);
    assert!(has(&f, "S9 cannot list datasets/ at no-such-base"), "{f:?}");
    write(&root, "datasets/jth-2.0.toml", "url = \"u\"\n");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family"]);
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "a family never changes"), "{f:?}");
    write(&root, "datasets/ours-v0.toml", "base = \"jth-2.0\"\n");
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "a family never changes"), "{f:?}");
    write(&root, "datasets/jth-2.0.toml", "url = \"v\"\n");
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(
        has(
            &f,
            "S9 datasets/jth-2.0.toml differs from HEAD; a family never changes"
        ),
        "{f:?}"
    );
    std::fs::remove_file(root.join("datasets/jth-2.0.toml")).unwrap();
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(
        has(&f, "S9 datasets/jth-2.0.toml differs from HEAD"),
        "{f:?}"
    );
}

#[test]
fn s6_passes_our_file_only_at_the_pin_of_its_family_table() {
    let ours = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    use sha2::Digest as _;
    let sha = crate::harness::hex(&sha2::Sha256::digest(ours.as_bytes()));
    let table = format!(
        "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n\
         O = {{ file = \"ours-v0/O.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 17.5 }}\n\
         N = {{ file = \"ours-v0/N.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 17.5 }}\n"
    );
    let pins = format!(
        "[atompaw]\nversion = \"4.2.0.4\"\n\n[atompaw.files]\n\"ours-v0/O/O.xml\" = \"{sha}\"\n"
    );
    let jth_o = crate::harness::jth("file:///nowhere", &[("pbe", "O", ours, None)]);
    let root = tree(
        "s6-ours",
        &[
            ("benchmarks/DATASETS.toml", &pins),
            ("datasets/jth-2.0.toml", &jth_o),
            ("datasets/ours-v0.toml", &table),
            ("datasets/ours-v0/O.xml", ours),
            ("datasets/ours-v0/N.xml", &format!("{ours} ")),
            ("datasets/ours-v0/Si.xml", ours),
            ("datasets/atompaw/O.xml", ours),
            ("datasets/O.xml", ours),
            ("notes/O.xml", ours),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(!has(&f, "S6 datasets/ours-v0/O.xml"), "{f:?}");
    for file in [
        "datasets/ours-v0/N.xml",
        "datasets/ours-v0/Si.xml",
        "datasets/atompaw/O.xml",
        "datasets/O.xml",
        "notes/O.xml",
    ] {
        assert!(
            has(&f, &format!("S6 {file} is a VASP file, a dataset")),
            "{file}: {f:?}"
        );
    }
    write(
        &root,
        "datasets/ours-v0.toml",
        &table.replace("base =", "base"),
    );
    let f = lever(&["science"], &root);
    assert!(has(&f, "S6 datasets/ours-v0/O.xml"), "{f:?}");
}

#[test]
fn s9_keeps_each_file_of_a_family_of_ours_byte_for_byte() {
    let root = base("s9-family-files");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/ours-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/ours-v0/O.xml", "o"),
        ("datasets/ours-v0/O.in", "O 8\n"),
        ("datasets/atompaw/O.in", "O 8\n"),
        ("datasets/atompaw/t.toml", "[element]\n"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family of ours"]);
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "a family never changes"), "{f:?}");
    std::fs::remove_dir_all(root.join("datasets/atompaw")).unwrap();
    write(&root, "datasets/ours-v1.toml", "base = \"jth-2.0\"\n");
    write(&root, "datasets/ours-v1/O.in", "O 8\n");
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(!has(&f, "a family never changes"), "{f:?}");
    write(&root, "datasets/ours-v0/O.xml", "p");
    std::fs::remove_file(root.join("datasets/ours-v0/O.in")).unwrap();
    write(&root, "datasets/ours-v0/Mg.in", "Mg 12\n");
    let f = lever(&["science", "--base", "HEAD"], &root);
    for (file, how) in [
        ("datasets/ours-v0/O.xml", "differs from HEAD"),
        ("datasets/ours-v0/O.in", "differs from HEAD"),
        ("datasets/ours-v0/Mg.in", "is new in ours-v0 of HEAD"),
    ] {
        assert!(
            has(&f, &format!("S9 {file} {how}; a family never changes")),
            "{file}: {f:?}"
        );
    }
}

#[test]
fn s6_grants_the_exception_only_to_a_valid_table_of_ours_and_never_to_a_vasp_file() {
    use sha2::Digest as _;
    let paw = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    let titel = "<paw_dataset version=\"0.7\">\n TITEL = PAW_PBE O 08Apr2002\n";
    let potcar = "PAW_PBE O 08Apr2002\n";
    let sha = |text: &str| crate::harness::hex(&sha2::Sha256::digest(text.as_bytes()));
    let row = |element: &str, file: &str, text: &str| {
        format!(
            "{element} = {{ file = \"{file}\", sha256 = \"{}\", cutoff_hint_ha = 17.5 }}\n",
            sha(text)
        )
    };
    let table = |base: &str, rows: &str| {
        format!("base = \"{base}\"\nfunctional = \"pbe\"\n\n[element]\n{rows}")
    };
    let root = tree(
        "s6-valid-table",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            (
                "datasets/lost-v0.toml",
                &table("nope", &row("O", "lost-v0/O.xml", paw)),
            ),
            ("datasets/lost-v0/O.xml", paw),
            (
                "datasets/far-v0.toml",
                &table(
                    "jth-2.0",
                    &(row("O", "elsewhere/O.xml", paw) + &row("N", "POTCAR", potcar)),
                ),
            ),
            ("datasets/elsewhere/O.xml", paw),
            ("datasets/POTCAR", potcar),
            ("datasets/far-v0/POTCAR", potcar),
            (
                "datasets/ours-v0.toml",
                &table(
                    "jth-2.0",
                    &(row("O", "ours-v0/O.xml", paw) + &row("N", "ours-v0/N.xml", titel)),
                ),
            ),
            ("datasets/ours-v0/O.xml", paw),
            ("datasets/ours-v0/N.xml", titel),
            ("notes/si.psp8", "plain\n"),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(!has(&f, "S6 datasets/ours-v0/O.xml"), "{f:?}");
    for file in [
        "datasets/lost-v0/O.xml",
        "datasets/elsewhere/O.xml",
        "datasets/POTCAR",
        "datasets/far-v0/POTCAR",
        "datasets/ours-v0/N.xml",
        "notes/si.psp8",
    ] {
        assert!(
            has(&f, &format!("S6 {file} is a VASP file, a dataset")),
            "{file}: {f:?}"
        );
    }
}

#[test]
fn s9_catches_a_link_a_mode_and_a_new_table_that_names_a_family_of_the_base() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = base("s9-family-links");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/ours-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/ours-v0/O.xml", "o"),
        ("datasets/ours-v0/O.in", "O 8\n"),
        ("notes/O.xml", "o"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family of ours"]);
    std::fs::remove_file(root.join("datasets/ours-v0/O.xml")).unwrap();
    symlink("../../notes/O.xml", root.join("datasets/ours-v0/O.xml")).unwrap();
    symlink("O.in", root.join("datasets/ours-v0/N.in")).unwrap();
    std::fs::set_permissions(
        root.join("datasets/ours-v0/O.in"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    write(
        &root,
        "datasets/zz-v1.toml",
        "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n\
         N = { file = \"ours-v0/N.xml\", sha256 = \"0\", cutoff_hint_ha = 1.0 }\n",
    );
    let f = lever(&["science", "--base", "HEAD"], &root);
    for needle in [
        "S9 datasets/ours-v0/O.xml is a link; a family holds regular files",
        "S9 datasets/ours-v0/N.in is a link; a family holds regular files",
        "S9 datasets/ours-v0/O.in has mode 100755, HEAD has 100644; a family never changes",
        "S9 datasets/zz-v1.toml names ours-v0/N.xml in the directory of ours-v0 of HEAD; a family never changes",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
}

fn own_probe(probe: &str, name: &str, base: &str, body: &str) -> Vec<String> {
    use sha2::Digest as _;
    let paw = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    let sha = crate::harness::hex(&sha2::Sha256::digest(paw.as_bytes()));
    let table = format!("base = \"{base}\"\n{}", body.replace("{sha}", &sha));
    let root = tree(
        probe,
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            ("notes/ext.toml", "url = \"u\"\n"),
            (&format!("datasets/{name}.toml"), &table),
            (&format!("datasets/{name}/O.xml"), paw),
        ],
    );
    lever(&["science"], &root)
}

const OWN_BODY: &str = "functional = \"pbe\"\n\n[element]\nO = { file = \"NAME/O.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 17.5 }\n";

#[test]
fn s6_names_a_table_of_ours_whose_base_is_no_plain_external_sibling_and_grants_it_nothing() {
    let body = |name: &str| OWN_BODY.replace("NAME", name);
    let f = own_probe("s6-own-valid", "ours-v0", "jth-2.0", &body("ours-v0"));
    assert!(!has(&f, "S6"), "{f:?}");
    for (probe, base, why) in [
        (
            "s6-own-outside",
            "../notes/ext",
            "base \"../notes/ext\" is no plain table name",
        ),
        (
            "s6-own-self",
            "ours-v0",
            "base \"ours-v0\" is no external table of datasets/",
        ),
    ] {
        let f = own_probe(probe, "ours-v0", base, &body("ours-v0"));
        assert!(
            has(&f, &format!("S6 datasets/ours-v0.toml: {why}")),
            "{probe}: {f:?}"
        );
        assert!(
            has(&f, "S6 datasets/ours-v0/O.xml is a VASP file, a dataset"),
            "{probe}: {f:?}"
        );
    }
}

#[test]
fn s6_names_a_base_reached_through_a_link_and_a_cycle_of_ours_without_a_crash() {
    use std::os::unix::fs::symlink;
    let root = tree(
        "s6-own-links",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("notes/ext.toml", "url = \"u\"\n"),
        ],
    );
    use sha2::Digest as _;
    let paw = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    let sha = crate::harness::hex(&sha2::Sha256::digest(paw.as_bytes()));
    write(&root, "datasets/a-v0/O.xml", paw);
    symlink("../notes/ext.toml", root.join("datasets/ext.toml")).unwrap();
    for (name, base) in [("a-v0", "ext"), ("b-v0", "c-v0"), ("c-v0", "b-v0")] {
        write(
            &root,
            &format!("datasets/{name}.toml"),
            &format!(
                "base = \"{base}\"\n{}",
                OWN_BODY.replace("NAME", name).replace("{sha}", &sha)
            ),
        );
    }
    let f = lever(&["science"], &root);
    for needle in [
        "S6 datasets/ext.toml is a link or no regular file; a table is a regular file",
        "S6 datasets/a-v0.toml: base \"ext\" is no external table of datasets/",
        "S6 datasets/b-v0.toml: base \"c-v0\" is no external table of datasets/",
        "S6 datasets/c-v0.toml: base \"b-v0\" is no external table of datasets/",
        "S6 datasets/a-v0/O.xml is a VASP file, a dataset",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
}

#[test]
fn s6_names_a_table_of_ours_that_breaks_the_engines_row_rules() {
    for (probe, body, why) in [
        (
            "s6-own-functional",
            OWN_BODY.replace("\"pbe\"", "\"pbx\""),
            "functional \"pbx\" keys no dataset section (lda, pbe)",
        ),
        (
            "s6-own-cutoff",
            OWN_BODY.replace("17.5", "-1.0"),
            "O: cutoff_hint_ha must be positive",
        ),
        (
            "s6-own-case",
            format!(
                "{OWN_BODY}o = {{ file = \"NAME/o.xml\", sha256 = \"{{sha}}\", cutoff_hint_ha = 1.0 }}\n"
            ),
            "\"o\" is not an element symbol",
        ),
        (
            "s6-own-slash",
            OWN_BODY.replace(
                "O = { file = \"NAME/O.xml\"",
                "\"O/x\" = { file = \"NAME/O/x.xml\"",
            ),
            "\"O/x\" is not an element symbol",
        ),
        (
            "s6-own-sha",
            OWN_BODY.replace("{sha}", "ABC"),
            "O: sha256 \"ABC\" is not 64 lowercase hex digits",
        ),
        (
            "s6-own-sha-upper",
            OWN_BODY.replace("{sha}", &"A".repeat(64)),
            "O: sha256 \"AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA\" is not 64 lowercase hex digits",
        ),
        (
            "s6-own-cutoff-zero",
            OWN_BODY.replace("17.5", "0.0"),
            "O: cutoff_hint_ha must be positive",
        ),
    ] {
        let f = own_probe(
            probe,
            "ours-v0",
            "jth-2.0",
            &body.replace("NAME", "ours-v0"),
        );
        assert!(
            has(&f, &format!("S6 datasets/ours-v0.toml: {why}")),
            "{probe}: {f:?}"
        );
        assert!(
            has(&f, "S6 datasets/ours-v0/O.xml is a VASP file, a dataset"),
            "{probe}: {f:?}"
        );
    }
}

#[test]
fn s6_names_a_table_of_ours_without_its_directory_or_a_rows_file() {
    let body = OWN_BODY.replace("NAME", "ours-v0")
        + "N = { file = \"ours-v0/N.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 1.0 }\n";
    let f = own_probe("s6-own-orphan-row", "ours-v0", "jth-2.0", &body);
    assert!(
        has(
            &f,
            "S6 datasets/ours-v0.toml: N: datasets/ours-v0/N.xml is no regular file"
        ),
        "{f:?}"
    );
    let root = tree(
        "s6-own-orphan",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            (
                "datasets/lone-v0.toml",
                &format!(
                    "base = \"jth-2.0\"\n{}",
                    OWN_BODY
                        .replace("NAME", "lone-v0")
                        .replace("{sha}", &"0".repeat(64))
                ),
            ),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        has(
            &f,
            "S6 datasets/lone-v0.toml: datasets/lone-v0 is no directory"
        ),
        "{f:?}"
    );
}

#[test]
fn s6_names_a_datasets_directory_that_is_a_link() {
    use std::os::unix::fs::symlink;
    let root = tree(
        "s6-linked-datasets",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("notes/tables/jth-2.0.toml", "url = \"u\"\n"),
        ],
    );
    symlink("notes/tables", root.join("datasets")).unwrap();
    let f = lever(&["science"], &root);
    assert!(
        has(
            &f,
            "S6 datasets is a link or no directory; the tables lie in a real directory"
        ),
        "{f:?}"
    );
}

#[test]
fn science_names_a_fifo_at_a_case_family_table_without_blocking() {
    let root = base("s-fifo-case-table");
    write(&root, "datasets/jth-2.0.toml", "url = \"u\"\n");
    write(
        &root,
        "benchmarks/1_a/input.toml",
        "[datasets]\nfamily = \"ours-v0\"\n",
    );
    crate::fifo(&root, "datasets/ours-v0.toml");
    let path = root.to_path_buf();
    let f = crate::within(5, move || lever(&["science"], &path));
    assert!(
        has(
            &f,
            "S6 datasets/ours-v0.toml is a link or no regular file; a table is a regular file"
        ),
        "{f:?}"
    );
}

#[test]
fn s9_names_a_fifo_in_place_of_a_base_family_file_or_added_to_it_without_blocking() {
    let root = base("s9-fifo-family");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/ours-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/ours-v0/O.in", "O 8\n"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family of ours"]);
    crate::fifo(&root, "datasets/ours-v0/O.in");
    crate::fifo(&root, "datasets/ours-v0/N.in");
    let path = root.to_path_buf();
    let f = crate::within(5, move || lever(&["science", "--base", "HEAD"], &path));
    for file in ["datasets/ours-v0/O.in", "datasets/ours-v0/N.in"] {
        assert!(
            has(
                &f,
                &format!("S9 {file} is no regular file or directory; a family holds regular files")
            ),
            "{file}: {f:?}"
        );
    }
}

#[test]
fn s6_names_a_datasets_directory_that_cannot_be_listed() {
    use std::os::unix::fs::PermissionsExt;
    if crate::root_user() {
        return;
    }
    let root = tree(
        "s6-unlistable",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ],
    );
    let dir = root.join("datasets");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o300)).unwrap();
    let f = lever(&["science"], &root);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(has(&f, "S6 datasets cannot be listed: "), "{f:?}");
}

#[test]
fn s6_names_a_table_whose_suffix_is_toml_in_another_case() {
    let root = tree(
        "s6-toml-case",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            ("datasets/ours-v0.ToML", "base = \"jth-2.0\"\n"),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        has(&f, "S6 datasets/ours-v0.ToML: a table is named <name>.toml"),
        "{f:?}"
    );
}

#[test]
fn s6_names_a_table_of_ours_whose_base_breaks_the_engines_row_rules() {
    let body = OWN_BODY.replace("NAME", "ours-v0");
    for (probe, base, why) in [
        (
            "s6-base-pbx",
            "[pbx]\nlayout = \"X/{element}.xml\"\n\n[pbx.element]\nO = { sha256 = \"AAA\" }\n",
            "\"pbx\" keys no dataset section (lda, pbe)",
        ),
        (
            "s6-base-layout",
            "[pbe]\nlayout = \"X/O.xml\"\n\n[pbe.element]\n",
            "pbe.layout: \"X/O.xml\" has no {element}",
        ),
        (
            "s6-base-symbol",
            "[pbe]\nlayout = \"X/{element}.xml\"\n\n[pbe.element]\nXx = { sha256 = \"SHA\" }\n",
            "\"Xx\" is not an element symbol",
        ),
        (
            "s6-base-hint",
            "[pbe]\nlayout = \"X/{element}.xml\"\n\n[pbe.element]\nSi = { sha256 = \"SHA\", cutoff_hint_ha = 0.0 }\n",
            "Si: cutoff_hint_ha must be positive",
        ),
    ] {
        let root = tree(
            probe,
            &[
                ("benchmarks/DATASETS.toml", ""),
                (
                    "datasets/jth-2.0.toml",
                    &format!("url = \"u\"\n\n{}", base.replace("SHA", &"a".repeat(64))),
                ),
                (
                    "datasets/ours-v0.toml",
                    &format!(
                        "base = \"jth-2.0\"\n{}",
                        body.replace("{sha}", &"b".repeat(64))
                    ),
                ),
                ("datasets/ours-v0/O.xml", "<paw_dataset version=\"0.7\">\n"),
            ],
        );
        let f = lever(&["science"], &root);
        assert!(
            has(&f, "S6 datasets/ours-v0.toml: base \"jth-2.0\""),
            "{probe}: {f:?}"
        );
        assert!(has(&f, why), "{probe}: {why}: {f:?}");
    }
}

#[test]
fn s9_names_a_removed_base_family_file_whose_name_holds_a_space_or_a_non_ascii_letter() {
    let root = base("s9-quoted-names");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/ours-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/ours-v0/a note.txt", "a\n"),
        ("datasets/ours-v0/r\u{e9}f\u{e9}rence.txt", "r\n"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family of ours"]);
    for file in [
        "datasets/ours-v0/a note.txt",
        "datasets/ours-v0/r\u{e9}f\u{e9}rence.txt",
    ] {
        std::fs::remove_file(root.join(file)).unwrap();
    }
    let f = lever(&["science", "--base", "HEAD"], &root);
    for file in [
        "datasets/ours-v0/a note.txt",
        "datasets/ours-v0/r\u{e9}f\u{e9}rence.txt",
    ] {
        assert!(
            has(&f, &format!("S9 {file} differs from HEAD")),
            "{file}: {f:?}"
        );
    }
}

#[test]
fn s6_grants_nothing_to_a_family_name_that_is_not_portable_or_a_row_that_claims_another_path() {
    use sha2::Digest as _;
    let paw = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    let sha = crate::harness::hex(&sha2::Sha256::digest(paw.as_bytes()));
    let row = |name: &str, file: &str| {
        format!(
            "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\nO = {{ file = \"{file}\", sha256 = \"{sha}\", cutoff_hint_ha = 17.5 }}\n"
        )
        .replace("NAME", name)
    };
    let root = tree(
        "s6-controls",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            ("datasets/bad name.toml", &row("bad name", "bad name/O.xml")),
            ("datasets/bad name/O.xml", paw),
            ("datasets/far-v0.toml", &row("far-v0", "far-v0/other.xml")),
            ("datasets/far-v0/O.xml", paw),
            ("datasets/far-v0/other.xml", paw),
        ],
    );
    let f = lever(&["science"], &root);
    for needle in [
        "S6 datasets/bad name.toml: \"bad name\": a family name holds letters, digits",
        "S6 datasets/bad name/O.xml is a VASP file, a dataset",
        "S6 datasets/far-v0.toml: O: \"far-v0/other.xml\" is not \"far-v0/O.xml\"",
        "S6 datasets/far-v0/O.xml is a VASP file, a dataset",
    ] {
        assert!(has(&f, needle), "{needle}: {f:?}");
    }
}

#[test]
fn s6_grants_nothing_to_two_tables_whose_names_differ_only_in_case() {
    use sha2::Digest as _;
    let paw = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    let sha = crate::harness::hex(&sha2::Sha256::digest(paw.as_bytes()));
    let table = |name: &str| {
        format!(
            "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\nO = {{ file = \"{name}/O.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 17.5 }}\n"
        )
    };
    let root = tree(
        "s6-case-tables",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            ("datasets/ours-v0.toml", &table("ours-v0")),
            ("datasets/ours-v0/O.xml", paw),
            ("datasets/OURS-V0.toml", &table("OURS-V0")),
            ("datasets/OURS-V0/O.xml", paw),
        ],
    );
    let tables = (std::fs::read_dir(root.join("datasets")).unwrap())
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".toml")
        })
        .count();
    if tables < 3 {
        return;
    }
    let f = lever(&["science"], &root);
    assert!(
        has(
            &f,
            "S6 datasets/ours-v0.toml: datasets/OURS-V0.toml names the same directory in another case"
        ),
        "{f:?}"
    );
    assert!(
        has(&f, "S6 datasets/ours-v0/O.xml is a VASP file, a dataset"),
        "{f:?}"
    );
}

#[test]
fn s6_names_a_digest_of_65_hex_digits_an_infinite_hint_and_a_base_that_is_a_family_of_ours() {
    let body = OWN_BODY.replace("NAME", "ours-v0");
    for (probe, body, why) in [
        (
            "s6-own-sha-65",
            body.replace("{sha}", &"a".repeat(65)),
            "O: sha256 \"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\" is not 64 lowercase hex digits",
        ),
        (
            "s6-own-hint-inf",
            body.replace("17.5", "inf"),
            "O: cutoff_hint_ha must be positive",
        ),
    ] {
        let f = own_probe(probe, "ours-v0", "jth-2.0", &body);
        assert!(
            has(&f, &format!("S6 datasets/ours-v0.toml: {why}")),
            "{probe}: {f:?}"
        );
    }
    use sha2::Digest as _;
    let paw = "<paw_dataset version=\"0.7\">\n<!-- ours -->\n";
    let sha = crate::harness::hex(&sha2::Sha256::digest(paw.as_bytes()));
    let table = |name: &str, base: &str| {
        format!(
            "base = \"{base}\"\n{}",
            OWN_BODY.replace("NAME", name).replace("{sha}", &sha)
        )
    };
    let root = tree(
        "s6-own-chain",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            ("datasets/mid-v0.toml", &table("mid-v0", "jth-2.0")),
            ("datasets/mid-v0/O.xml", paw),
            ("datasets/ours-v0.toml", &table("ours-v0", "mid-v0")),
            ("datasets/ours-v0/O.xml", paw),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        has(
            &f,
            "S6 datasets/ours-v0.toml: base \"mid-v0\" is no external table of datasets/: \"mid-v0\" is no external table"
        ),
        "{f:?}"
    );
    assert!(!has(&f, "S6 datasets/mid-v0/O.xml"), "{f:?}");
    assert!(
        has(&f, "S6 datasets/ours-v0/O.xml is a VASP file, a dataset"),
        "{f:?}"
    );
}

#[test]
fn s5_names_a_case_selected_family_table_or_base_that_is_a_fifo_without_blocking() {
    for (probe, fifo, why) in [
        (
            "s5-fifo-table",
            "datasets/ours-v0.toml",
            "datasets/ours-v0.toml is no regular file",
        ),
        (
            "s5-fifo-base",
            "datasets/jth-2.0.toml",
            "datasets/jth-2.0.toml is no regular file",
        ),
    ] {
        let root = base(probe);
        write(&root, "datasets/jth-2.0.toml", "url = \"u\"\n");
        write(
            &root,
            "datasets/ours-v0.toml",
            "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n",
        );
        write(
            &root,
            "benchmarks/1_a/input.toml",
            "[datasets]\nfamily = \"ours-v0\"\n",
        );
        crate::fifo(&root, fifo);
        let path = root.to_path_buf();
        let f = crate::within(5, move || lever(&["science"], &path));
        assert!(
            has(
                &f,
                &format!("S5 1_a: benchmarks/1_a/input.toml names family \"ours-v0\": {why}")
            ),
            "{probe}: {f:?}"
        );
    }
}

#[test]
fn s5_and_lock_refuse_a_case_family_name_that_is_no_file_name() {
    let root = base("s5-family-path");
    write(&root, "x.toml", "url = \"u\"\n");
    write(
        &root,
        "benchmarks/1_a/input.toml",
        "[datasets]\nfamily = \"../x\"\n",
    );
    let f = lever(&["science"], &root);
    assert!(
        has(
            &f,
            "S5 1_a: benchmarks/1_a/input.toml names family \"../x\": datasets: family \"../x\" must be a file name"
        ),
        "{f:?}"
    );
    let f = lever(&["lock", "--reviewer", "owner", "--reason", "x"], &root);
    assert!(
        has(
            &f,
            "lock: 1_a: benchmarks/1_a/input.toml names family \"../x\""
        ),
        "{f:?}"
    );
    assert!(
        !crate::lock_files(&root)
            .keys()
            .any(|f| f.contains("x.toml"))
    );
}

#[test]
fn s9_names_a_removed_base_family_table_whose_name_git_quotes() {
    let root = base("s9-quoted-table");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/r\u{e9}f-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/plain-v0.toml", "base = \"jth-2.0\"\n"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "two families"]);
    std::fs::remove_file(root.join("datasets/r\u{e9}f-v0.toml")).unwrap();
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(
        has(&f, "S9 datasets/r\u{e9}f-v0.toml differs from HEAD"),
        "{f:?}"
    );
}

#[test]
fn science_names_a_directory_it_cannot_walk_and_a_name_it_would_rewrite() {
    let paw = "<paw_dataset version=\"0.7\">\n";
    let root = tree(
        "s6-walk-names",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("notes/x.xml", "plain\n"),
            ("notes\\x.xml", paw),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(has(&f, "S6 notes\\x.xml holds a backslash"), "{f:?}");
    if crate::root_user() {
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    write(&root, "notes/closed/POTCAR", "PAW_PBE\n");
    std::fs::set_permissions(
        root.join("notes/closed"),
        std::fs::Permissions::from_mode(0o000),
    )
    .unwrap();
    let f = lever(&["science"], &root);
    std::fs::set_permissions(
        root.join("notes/closed"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    assert!(has(&f, "S6 notes/closed cannot be walked: "), "{f:?}");
}

#[test]
fn science_names_a_git_index_it_cannot_list_and_a_base_blob_it_cannot_read() {
    let root = base("s-git-errors");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/ours-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/ours-v0/O.in", "O 8\n"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family"]);
    let object = |spec: &str| {
        let out = std::process::Command::new("git")
            .args(["rev-parse", spec])
            .current_dir(&*root)
            .output()
            .unwrap();
        let hash = String::from_utf8(out.stdout).unwrap().trim().to_owned();
        root.join(".git/objects").join(&hash[..2]).join(&hash[2..])
    };
    std::fs::remove_file(object("HEAD:datasets/ours-v0/O.in")).unwrap();
    write(&root, "datasets/ours-v0/O.in", "");
    std::fs::remove_file(object("HEAD:benchmarks/1_a/case.toml")).unwrap();
    let f = lever(&["science", "--base", "HEAD"], &root);
    assert!(
        has(&f, "S9 cannot read datasets/ours-v0/O.in at HEAD"),
        "{f:?}"
    );
    assert!(
        has(&f, "S9 cannot read benchmarks/1_a/case.toml at HEAD"),
        "{f:?}"
    );
    std::fs::write(root.join(".git/index"), "garbage").unwrap();
    let f = lever(&["science"], &root);
    assert!(has(&f, "S6 git ls-files failed"), "{f:?}");
}

#[test]
fn science_takes_a_non_ascii_name_in_datasets_without_a_panic() {
    let root = tree(
        "s6-non-ascii",
        &[
            ("benchmarks/DATASETS.toml", ""),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            ("datasets/\u{e9}abcd", "x\n"),
        ],
    );
    let path = root.to_path_buf();
    let f = crate::within(5, move || {
        std::panic::catch_unwind(|| lever(&["science"], &path))
            .map_err(|_| "science panicked".to_owned())
    });
    assert!(f.is_ok(), "{f:?}");
}

#[test]
fn s8_names_a_case_that_does_not_load_beside_its_typed_source() {
    let root = tree(
        "s8-unknown-kind",
        &[
            (
                "benchmarks/1_a/case.toml",
                &CASE.replace("reference = \"literature\"", "reference = \"unknown-kind\""),
            ),
            ("benchmarks/1_a/reference/values.json", "[1.234]"),
            ("benchmarks/1_a/reference/SOURCE.toml", SOURCE),
        ],
    );
    assert!(
        std::fs::read_to_string(root.join("benchmarks/1_a/case.toml"))
            .unwrap()
            .contains("unknown-kind")
    );
    let f = lever(&["science"], &root);
    assert!(
        has(
            &f,
            "S8 benchmarks/1_a/reference/SOURCE.toml: the case does not load: "
        ),
        "{f:?}"
    );
}

#[test]
fn s8_takes_a_source_whose_kind_equals_the_case_reference() {
    let root = tree(
        "s8-equal-kind",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/reference/values.json", "[1.234]"),
            ("benchmarks/1_a/reference/SOURCE.toml", SOURCE),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(!has(&f, "S8"), "{f:?}");
}

#[test]
fn s9_names_a_link_or_a_special_file_in_place_of_a_base_family_file_once() {
    use std::os::unix::fs::symlink;
    let root = base("s9-named-once");
    for (path, text) in [
        ("datasets/jth-2.0.toml", "url = \"u\"\n"),
        ("datasets/ours-v0.toml", "base = \"jth-2.0\"\n"),
        ("datasets/ours-v0/O.in", "O 8\n"),
        ("datasets/ours-v0/N.in", "N 7\n"),
        ("notes/O.in", "O 8\n"),
    ] {
        write(&root, path, text);
    }
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-qm", "a family"]);
    std::fs::remove_file(root.join("datasets/ours-v0/O.in")).unwrap();
    symlink("../../notes/O.in", root.join("datasets/ours-v0/O.in")).unwrap();
    crate::fifo(&root, "datasets/ours-v0/N.in");
    let path = root.to_path_buf();
    let f = crate::within(5, move || lever(&["science", "--base", "HEAD"], &path));
    assert!(has(&f, "S9 datasets/ours-v0/O.in is a link"), "{f:?}");
    assert!(
        has(
            &f,
            "S9 datasets/ours-v0/N.in is no regular file or directory"
        ),
        "{f:?}"
    );
    for file in ["O.in", "N.in"] {
        assert!(
            !has(&f, &format!("S9 datasets/ours-v0/{file} differs")),
            "{file}: {f:?}"
        );
        assert!(
            !has(&f, &format!("S9 datasets/ours-v0/{file} has mode")),
            "{file}: {f:?}"
        );
    }
}

#[test]
fn science_finds_no_fault_in_a_checkout_without_datasets() {
    let root = tree("s6-no-datasets", &[("benchmarks/DATASETS.toml", "")]);
    let f = lever(&["science"], &root);
    assert!(!has(&f, "datasets cannot be listed"), "{f:?}");
    assert!(!has(&f, "S6 datasets"), "{f:?}");
}

const FUNCTIONAL: &str = "pub enum Functional {\n    /// One.\n    Lda,\n    Pbe,\n}\npub fn functional(f: Option<Functional>) -> Functional {\n    f.unwrap_or(Functional::Pbe)\n}\n";

fn functional_tree(name: &str, file: &str, body: &str) -> Vec<String> {
    let root = tree(
        name,
        &[
            ("src/model/spec.rs", FUNCTIONAL),
            (file, body),
            ("benchmarks/.keep", ""),
        ],
    );
    lever(&["science"], &root)
}

#[test]
fn s10_catches_a_functional_variant_named_outside_xc() {
    let body = "use crate::model::Functional;\npub fn f(x: Functional) -> u8 {\n    match x {\n        Functional::Pbe => 1,\n        _ => 0,\n    }\n}\n";
    let f = functional_tree("s10-path", "src/io/run.rs", body);
    assert!(
        has(&f, "src/io/run.rs:4: S10 names Functional::Pbe"),
        "{f:?}"
    );
    assert!(!has(&f, "src/model/spec.rs"), "{f:?}");
}

#[test]
fn s10_lets_xc_name_every_variant() {
    let body = "pub fn f(x: Functional) -> bool {\n    x == Functional::Lda\n}\n";
    let f = functional_tree("s10-xc", "src/xc/functional.rs", body);
    assert!(!has(&f, "S10"), "{f:?}");
}

#[test]
fn s10_catches_an_import_of_the_variants_a_rename_and_self_in_an_impl() {
    let body = "use crate::model::Functional::*;\nuse crate::model::Functional::{Lda};\nuse crate::model::Functional as F;\nimpl Show for Functional {\n    fn show(&self) -> bool {\n        *self == Self::Lda\n    }\n}\n";
    let f = functional_tree("s10-hidden", "src/pw/xc.rs", body);
    assert!(has(&f, "src/pw/xc.rs:1: S10 imports the variants"), "{f:?}");
    assert!(has(&f, "src/pw/xc.rs:2: S10 imports the variants"), "{f:?}");
    assert!(has(&f, "src/pw/xc.rs:3: S10 renames Functional"), "{f:?}");
    assert!(has(&f, "src/pw/xc.rs:6: S10 names Self::Lda"), "{f:?}");
}

#[test]
fn s10_frees_only_the_default_of_the_enum_file() {
    let spec = format!("{FUNCTIONAL}pub fn lda() -> Functional {{\n    Functional::Lda\n}}\n");
    let root = tree(
        "s10-home",
        &[
            ("src/model/spec.rs", spec.as_str()),
            ("benchmarks/.keep", ""),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        has(&f, "src/model/spec.rs:10: S10 names Functional::Lda"),
        "{f:?}"
    );
    assert!(!has(&f, "src/model/spec.rs:7:"), "{f:?}");
}

#[test]
fn s10_follows_the_enum_to_the_file_that_defines_it() {
    let root = tree(
        "s10-moved",
        &[
            ("src/model/kinds.rs", FUNCTIONAL),
            (
                "src/ks/run.rs",
                "pub fn f() -> bool {\n    Functional::Lda == Functional::Pbe\n}\n",
            ),
            ("benchmarks/.keep", ""),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        has(&f, "src/ks/run.rs:2: S10 names Functional::Lda"),
        "{f:?}"
    );
    assert!(
        has(&f, "src/ks/run.rs:2: S10 names Functional::Pbe"),
        "{f:?}"
    );
    assert!(!has(&f, "src/model/kinds.rs"), "{f:?}");
}

#[test]
fn s10_passes_other_paths_the_table_and_impls_of_other_types() {
    let body = "pub fn f() -> usize {\n    let _ = Rung::Lda;\n    Functional::ALL.len()\n}\nimpl Rung {\n    fn g(self) -> bool {\n        self == Self::Lda\n    }\n}\n";
    let f = functional_tree("s10-quiet", "src/io/spec.rs", body);
    assert!(!has(&f, "S10"), "{f:?}");
}

#[test]
fn s10_reads_the_variants_of_the_functional_enum_not_of_an_enum_before_it() {
    let spec = format!("pub enum Rung {{\n    Gga,\n}}\n{FUNCTIONAL}");
    let root = tree(
        "s10-two-enums",
        &[
            ("src/model/spec.rs", spec.as_str()),
            (
                "src/io/run.rs",
                "pub fn f() -> bool {\n    Functional::Lda == Functional::Pbe\n}\n",
            ),
            ("benchmarks/.keep", ""),
        ],
    );
    let f = lever(&["science"], &root);
    assert!(
        has(&f, "src/io/run.rs:2: S10 names Functional::Lda"),
        "{f:?}"
    );
}
