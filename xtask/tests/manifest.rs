use crate::{has, lever, tree};

const WORKSPACE: &str =
    "[workspace]\nmembers = [\"xtask\"]\n\n[workspace.package]\nrust-version = \"1.99.1\"\n";
const TOOLCHAIN: &str = "[toolchain]\nchannel = \"1.99.1\"\n";
const MEMBER: &str = "[package]\nname = \"xtask\"\nedition.workspace = true\nrust-version.workspace = true\n\n[lints]\nworkspace = true\n";
const CI: &str = "rust-version: \"1.99.1\"\n# cargo-deny 0.20.2\n";

fn manifest(name: &str, root_toml: &str, files: &[(&str, &str)]) -> Vec<String> {
    let mut all = vec![
        ("Cargo.toml", root_toml),
        ("xtask/Cargo.toml", MEMBER),
        ("rust-toolchain.toml", TOOLCHAIN),
        (".github/workflows/ci.yml", CI),
        (".cargo/config.toml", "[alias]\nxtask = \"run\"\n"),
    ];
    all.extend_from_slice(files);
    lever(&["manifest"], &tree(name, &all))
}

#[test]
fn a_clean_manifest_passes() {
    assert!(manifest("manifest-clean", WORKSPACE, &[]).is_empty());
}

#[test]
fn every_dependency_table_needs_exact_pins() {
    let deps = "[dependencies]\ntoml = \"1.1.8\"\nserde = { version = \"=1.0.228\" }\nlocal = { path = \"x\" }\nremote = { git = \"https://example.org/x\" }\nloose = \"=1\"\nwild = \"=1.2.x\"\n";
    let more = "[target.'cfg(unix)'.dependencies]\nlibc = \"0.2\"\n\n[patch.crates-io]\nserde = { path = \"vendor/serde\" }\n";
    let text = format!("{WORKSPACE}\n{deps}\n{more}");
    let findings = manifest("manifest-pins", &text, &[]);
    for needle in [
        "toml must pin",
        "local must pin",
        "remote uses git",
        "loose must pin",
        "wild must pin",
        "target.cfg(unix).dependencies] libc must pin",
        "has [patch]",
    ] {
        assert!(has(&findings, needle), "{needle}: {findings:?}");
    }
    assert!(!has(&findings, "serde must pin"));
    let shared = format!("{WORKSPACE}\n[workspace.dependencies]\nitoa = \"1\"\n");
    assert!(has(
        &manifest("manifest-shared", &shared, &[]),
        "itoa must pin"
    ));
}

#[test]
fn every_member_inherits_the_workspace_lints() {
    let member = [(
        "xtask/Cargo.toml",
        "[package]\nname = \"xtask\"\nedition.workspace = true\nrust-version.workspace = true\n",
    )];
    let findings = manifest("manifest-lints", WORKSPACE, &member);
    assert!(has(
        &findings,
        "xtask/Cargo.toml must set [lints] workspace = true"
    ));
}

#[test]
fn members_build_scripts_and_cargo_env_are_caught() {
    let text = WORKSPACE.replace("\"xtask\"]", "\"xtask\", \"tables\"]")
        + "\n[package]\nbuild = \"tools/gen.rs\"\n";
    let findings = manifest(
        "manifest-build",
        &text,
        &[
            ("build.rs", "fn main() {}\n"),
            (
                ".cargo/config.toml",
                "[alias]\nx = \"run\"\n[env]\nORBITAR_BIN = { value = \"/x\", force = true }\n",
            ),
        ],
    );
    for needle in [
        "workspace members",
        "sets package.build",
        "build.rs can feed",
        "has [env]",
    ] {
        assert!(has(&findings, needle), "{needle}: {findings:?}");
    }
}

#[test]
fn a_second_cargo_or_clippy_config_is_caught() {
    for second in [".cargo/config", ".clippy.toml"] {
        let findings = manifest("manifest-second", WORKSPACE, &[(second, "")]);
        assert_eq!(
            findings,
            [format!(
                "manifest: {second} is a second config that this lever and benchmarks/LOCK miss; remove it"
            )]
        );
    }
}

#[test]
fn ci_builds_with_the_pinned_toolchain() {
    let ci = [(".github/workflows/ci.yml", "rust-version: \"1.98.0\"\n")];
    let findings = manifest("manifest-ci", WORKSPACE, &ci);
    assert!(has(
        &findings,
        "ci.yml rust-version values [\"1.98.0\"] must all be \"1.99.1\""
    ));
    let none = [(".github/workflows/ci.yml", "jobs: {}\n")];
    assert!(has(
        &manifest("manifest-ci-none", WORKSPACE, &none),
        "ci.yml rust-version values []"
    ));
}

#[test]
fn a_tool_version_must_match_everywhere() {
    let findings = manifest(
        "manifest-versions",
        &WORKSPACE.replace("1.99.1", "1.98.0"),
        &[(
            ".github/workflows/ci.yml",
            "jobs:\n  check:\n    steps:\n      - name: Check dependencies with cargo-deny 0.19.0\n        uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25 # v2.1.1\n        with:\n          rust-version: \"1.99.1\"\n      - uses: actions/checkout@v4\n",
        )],
    );
    assert!(has(&findings, "rust-version Some(\"1.98.0\") differs"));
    assert!(has(&findings, "must name cargo-deny 0.20.2"));
    assert!(has(
        &findings,
        "`actions/checkout@v4` must pin a 40-hex commit"
    ));
}

#[test]
fn every_member_shares_one_edition_and_toolchain() {
    let member = [(
        "xtask/Cargo.toml",
        "[package]\nname = \"xtask\"\nedition = \"2021\"\nrust-version.workspace = true\n\n[lints]\nworkspace = true\n",
    )];
    let findings = manifest("manifest-edition", WORKSPACE, &member);
    assert!(
        has(
            &findings,
            "xtask/Cargo.toml must set package.edition.workspace = true"
        ),
        "{findings:?}"
    );
    assert!(!has(&findings, "package.rust-version"), "{findings:?}");
}

#[test]
fn extra_targets_a_moved_lib_and_a_replaced_crate_are_caught() {
    let text = format!(
        "{WORKSPACE}\n[[bin]]\nname = \"probe\"\npath = \"tools/probe.rs\"\n\n[[example]]\nname = \"eos\"\n\n[[bench]]\nname = \"fft\"\n\n[replace]\n\"serde:1.0.228\" = {{ path = \"vendor/serde\" }}\n"
    );
    let member = [(
        "xtask/Cargo.toml",
        &*format!("{MEMBER}\n[lib]\npath = \"other/lib.rs\"\n"),
    )];
    let findings = manifest("manifest-targets", &text, &member);
    for needle in [
        "Cargo.toml declares [[bin]]",
        "Cargo.toml declares [[example]]",
        "Cargo.toml declares [[bench]]",
        "Cargo.toml has [replace]",
        "xtask/Cargo.toml moves [lib] path",
    ] {
        assert!(has(&findings, needle), "{needle}: {findings:?}");
    }
}

#[test]
fn a_path_dependency_fails_even_with_an_exact_version() {
    let text =
        format!("{WORKSPACE}\n[dependencies]\nlocal = {{ path = \"x\", version = \"=1.0.0\" }}\n");
    let findings = manifest("manifest-path", &text, &[]);
    assert!(has(&findings, "local is a path dependency"), "{findings:?}");
    assert!(!has(&findings, "local must pin"), "{findings:?}");
}

#[test]
fn an_action_needs_a_full_sha_and_a_tag_comment() {
    let sha = "11bd71901bbe5b1630ceea73d27597364c9af683";
    let ci = format!(
        "rust-version: \"1.99.1\"\n# cargo-deny 0.20.2\n      - uses: actions/checkout@{sha}\n      - uses: actions/cache@{short} # v4.2.0\n      - uses: actions/setup-python@{sha} # v5.6.0\n",
        short = &sha[..12]
    );
    let findings = manifest(
        "manifest-sha",
        WORKSPACE,
        &[(".github/workflows/ci.yml", &ci)],
    );
    assert!(
        has(&findings, &format!("`actions/checkout@{sha}` must pin")),
        "{findings:?}"
    );
    assert!(
        has(&findings, "`actions/cache@11bd71901bbe` must pin"),
        "{findings:?}"
    );
    assert!(!has(&findings, "setup-python"), "{findings:?}");
}

#[test]
fn the_deny_action_is_the_commit_that_xtask_pins() {
    let zero = "0".repeat(40);
    let ci = format!(
        "jobs:\n  check:\n    steps:\n      - name: Check dependencies with cargo-deny 0.20.2\n        uses: EmbarkStudios/cargo-deny-action@{zero} # v2.1.1\n        with:\n          rust-version: \"1.99.1\"\n"
    );
    let findings = manifest(
        "manifest-deny-sha",
        WORKSPACE,
        &[(".github/workflows/ci.yml", &ci)],
    );
    assert!(
        has(
            &findings,
            "cargo-deny action must be EmbarkStudios/cargo-deny-action@3c6349835b2b"
        ),
        "{findings:?}"
    );
}

#[test]
fn the_deny_version_belongs_to_the_step_that_uses_it() {
    let workflow = "jobs:\n  check:\n    steps:\n      - name: Check dependencies with cargo-deny 0.20.2\n        run: true\n      - uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25 # v2.1.1\n        with:\n          rust-version: \"1.99.1\"\n";
    let findings = manifest(
        "manifest-deny-name",
        WORKSPACE,
        &[(".github/workflows/ci.yml", workflow)],
    );
    assert!(
        has(&findings, "must name cargo-deny 0.20.2"),
        "{findings:?}"
    );
}

#[test]
fn exact_string_pins_shared_pins_and_pinned_actions_pass() {
    let text = format!(
        "{WORKSPACE}\n[workspace.dependencies]\nserde = \"=1.0.228\"\n\n[dependencies]\nitoa = \"=1.0.15\"\n"
    );
    let member = format!("{MEMBER}\n[dependencies]\nserde = {{ workspace = true }}\n");
    let ci = "jobs:\n  check:\n    steps:\n      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n      - name: Check dependencies with cargo-deny 0.20.2\n        uses: EmbarkStudios/cargo-deny-action@3c6349835b2b7b196a839186cb8b78e02f7b5f25 # v2.1.1\n        with:\n          rust-version: \"1.99.1\"\n";
    let findings = manifest(
        "manifest-exact",
        &text,
        &[
            ("xtask/Cargo.toml", &member),
            (".github/workflows/ci.yml", ci),
        ],
    );
    assert!(findings.is_empty(), "{findings:?}");
}
