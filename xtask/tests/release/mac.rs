use super::{
    CPU_ARM64, CPU_X86_64, EM_AARCH64, EM_X86_64, MAC, MH_DYLIB, MH_EXECUTE, NAME, PLATFORM_IOS,
    PLATFORM_MACOS, build_version, checked, elf, install, latin, macho, macho_bytes,
};
use crate::{has, lever, tree};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
};
use xtask::{Built, Installed, Steps};

const ENGINE: &str = "orbitar-0.1.0.data/scripts/orbitar is no";

fn engine_findings(name: &str, header: &str) -> Vec<String> {
    let found = checked(name, MAC, |m| m[3].1 = header.to_owned());
    found.into_iter().filter(|f| f.contains(ENGINE)).collect()
}

#[test]
fn a_mac_wheel_of_an_arm64_macos_executable_from_macos_12_passes() {
    assert_eq!(checked("mac", MAC, |_| {}), Vec::<String>::new());
}

#[test]
fn a_mac_engine_from_an_older_macos_than_the_tag_passes() {
    assert_eq!(
        engine_findings("mac-older", &macho(CPU_ARM64, 11)),
        Vec::<String>::new()
    );
}

#[test]
fn a_mac_engine_whose_lowest_macos_is_newer_than_12_0_0_fails() {
    for (name, version) in [("mac-patch", (12, 0, 1)), ("mac-14", (14, 0, 0))] {
        let command = build_version(PLATFORM_MACOS, version);
        let header = latin(macho_bytes(CPU_ARM64, MH_EXECUTE, &[command]));
        assert_eq!(engine_findings(name, &header).len(), 1, "{name}");
    }
}

#[test]
fn an_engine_for_another_machine_under_the_mac_tag_fails() {
    for (name, header) in [
        ("mac-elf", elf(EM_X86_64)),
        ("mac-x86", macho(CPU_X86_64, 12)),
    ] {
        assert_eq!(engine_findings(name, &header).len(), 1, "{name}");
    }
}

#[test]
fn a_mac_engine_for_another_platform_than_macos_fails() {
    let command = build_version(PLATFORM_IOS, (12, 0, 0));
    let header = latin(macho_bytes(CPU_ARM64, MH_EXECUTE, &[command]));
    assert_eq!(engine_findings("mac-ios", &header).len(), 1);
}

#[test]
fn a_mac_engine_that_is_no_executable_fails() {
    let command = build_version(PLATFORM_MACOS, (12, 0, 0));
    let header = latin(macho_bytes(CPU_ARM64, MH_DYLIB, &[command]));
    assert_eq!(engine_findings("mac-dylib", &header).len(), 1);
}

#[test]
fn a_mac_engine_whose_load_commands_are_malformed_fails() {
    let build = || build_version(PLATFORM_MACOS, (12, 0, 0));
    let (cmd, _, body) = build();
    let short = (cmd, 16, body[..8].to_vec());
    let past_the_end = (cmd, 64, build().2);
    let under_its_head = (0x19, 4, Vec::new());
    let unaligned = (0x19, 9, vec![0]);
    let tool_missing = {
        let (cmd, size, mut body) = build();
        body[12] = 1;
        (cmd, size, body)
    };
    for (name, commands) in [
        ("no-build-version", vec![]),
        ("short-build-version", vec![short]),
        ("past-sizeofcmds", vec![past_the_end]),
        ("under-its-head", vec![under_its_head, build()]),
        ("two-build-versions", vec![build(), build()]),
        ("unaligned-command", vec![unaligned, build()]),
        (
            "short-build-version-before-a-command",
            vec![(cmd, 16, build().2[..8].to_vec()), (0x19, 8, Vec::new())],
        ),
        ("a-tool-without-its-record", vec![tool_missing]),
    ] {
        let bytes = macho_bytes(CPU_ARM64, MH_EXECUTE, &commands);
        assert_eq!(xtask::machine(&bytes), None, "{name}");
        assert_eq!(engine_findings(name, &latin(bytes)).len(), 1, "{name}");
    }
}

#[test]
fn a_mac_header_reads_its_cpu_file_type_platform_and_full_lowest_version() {
    let command = build_version(PLATFORM_MACOS, (12, 3, 4));
    let bytes = macho_bytes(CPU_ARM64, MH_EXECUTE, &[command]);
    let want = xtask::Machine::MachO {
        cpu: CPU_ARM64,
        filetype: MH_EXECUTE,
        platform: PLATFORM_MACOS,
        minos: (12, 3, 4),
    };
    assert_eq!(xtask::machine(&bytes), Some(want));
}

#[test]
fn a_build_version_with_its_tool_records_reads() {
    let (cmd, _, mut body) = build_version(PLATFORM_MACOS, (12, 0, 0));
    body[12] = 1;
    body.extend([3, 0, 0, 0, 0, 0, 0x7e, 0x04]);
    let bytes = macho_bytes(CPU_ARM64, MH_EXECUTE, &[(cmd, 32, body)]);
    assert!(xtask::machine(&bytes).is_some());
}

#[test]
fn a_script_under_the_mac_tag_fails() {
    assert_eq!(engine_findings("mac-text", "#!/bin/sh\n").len(), 1);
}

#[test]
fn a_mac_engine_under_the_linux_tag_fails() {
    for (name, header) in [
        ("linux-macho", macho(CPU_ARM64, 12)),
        ("linux-arm", elf(EM_AARCH64)),
    ] {
        let found = checked(name, NAME, |m| m[3].1.clone_from(&header));
        assert!(has(&found, ENGINE), "{name}: {found:?}");
    }
}

/// The findings of the Mac wheel check when the engine holds `path` after a valid header.
fn mac_engine_holding(name: &str, path: &str) -> Vec<String> {
    let header = macho(CPU_ARM64, 12) + "\0" + path + "\0";
    checked(name, MAC, |m| m[3].1.clone_from(&header))
}

/// The one finding of the Mac host-path scan for `held`.
fn host_finding(held: &str) -> Vec<String> {
    vec![format!(
        "wheel orbitar-0.1.0.data/scripts/orbitar: `{held}` is a path on the build host; remap its root (release/mac.rs)"
    )]
}

#[test]
fn a_mac_engine_that_holds_a_path_under_a_host_root_fails_with_one_finding() {
    let home = format!("/{}/someone/src/main.rs", "Users");
    let user = format!("/{}/someone", "Users");
    for (path, held) in [
        (home.as_str(), user.as_str()),
        ("/Volumes/disk/build/src/main.rs", "/Volumes/disk"),
        (
            "/private/var/folders/xy/T/build/src/main.rs",
            "/private/var",
        ),
        ("/var/folders/xy/T/build/src/main.rs", "/var/folders/xy"),
        ("/tmp/build/src/main.rs", "/tmp/build"),
    ] {
        assert_eq!(
            mac_engine_holding("host-path", path),
            host_finding(held),
            "{path}"
        );
    }
}

#[test]
fn a_mac_engine_that_holds_a_host_path_with_a_non_ascii_component_fails() {
    let found = mac_engine_holding("host-utf8", "/Volumes/\u{c3}\u{a9}tude/src/main.rs");
    assert_eq!(found, host_finding("/Volumes/\u{e9}tude"));
}

#[test]
fn a_mac_engine_whose_host_path_component_starts_with_a_delimiter_fails() {
    for (path, held) in [
        (
            "/Volumes/ \u{c3}\u{a9}tude/src/main.rs",
            "/Volumes/ \u{e9}tude",
        ),
        ("/Volumes/\"disk\"/src/main.rs", "/Volumes/\"disk\""),
        ("/Volumes/'disk'/src/main.rs", "/Volumes/'disk'"),
    ] {
        assert_eq!(
            mac_engine_holding("host-delimiter", path),
            host_finding(held),
            "{path}"
        );
    }
}

#[test]
fn a_mac_engine_whose_host_root_is_spelled_in_another_case_fails() {
    let user = format!("/{}/someone/src/main.rs", "users");
    for (path, held) in [
        (user.as_str(), format!("/{}/someone", "users")),
        ("/VOLUMES/disk/src/main.rs", "/VOLUMES/disk".to_owned()),
        ("/Private/var/src/main.rs", "/Private/var".to_owned()),
    ] {
        assert_eq!(
            mac_engine_holding("host-case", path),
            host_finding(&held),
            "{path}"
        );
    }
}

#[test]
fn a_configured_build_root_in_the_engine_is_a_host_path_in_any_case() {
    let roots = ["/opt/ci/job 7/target/wheel".to_owned()];
    let spans = "x\n/OPT/CI/Job 7/target/wheel/src/src/main.rs\n/build/src/main.rs";
    let found = xtask::host_paths("engine", spans, &roots);
    assert_eq!(
        found,
        [
            "engine: `/OPT/CI/Job 7/target/wheel/src` is a path on the build host; remap its root (release/mac.rs)"
        ]
    );
    assert_eq!(
        xtask::host_paths("engine", "/build/src/main.rs", &roots),
        Vec::<String>::new()
    );
}

#[test]
fn a_source_directory_named_private_or_tmp_inside_a_path_passes() {
    let header = macho(CPU_ARM64, 12)
        + "\0/opt/cargo/registry/src/serde-1.0.228/src/private/de.rs\0/src/tests/tmp/x.rs\0";
    assert_eq!(
        checked("segments", MAC, |m| m[3].1.clone_from(&header)),
        Vec::<String>::new()
    );
}

#[test]
fn a_mac_engine_with_the_remapped_build_paths_alone_passes() {
    let header = macho(CPU_ARM64, 12)
        + "\0/src/src/main.rs\0/opt/cargo/registry/src/x/lib.rs\0/build/cargo/out\0";
    assert_eq!(
        checked("remapped", MAC, |m| m[3].1.clone_from(&header)),
        Vec::<String>::new()
    );
}

#[test]
fn wheel_target_builds_only_a_mac_target() {
    let root = tree("wheel-target", &[]);
    let found = lever(&["wheel", "--target", "x86_64-unknown-linux-gnu"], &root);
    assert!(
        has(
            &found,
            "no Mac platform; the Mac targets are [\"aarch64-apple-darwin (macOS 12.0)\"]"
        ),
        "{found:?}"
    );
}

fn built() -> Built {
    Built {
        orbitar: "/w/dist/orbitar-0.1.0-py3-none-macosx_12_0_arm64.whl".into(),
        data: "/w/dist/orbitar_jth-2.0.0-py3-none-any.whl".into(),
        commit: "abc".into(),
    }
}

fn installed() -> Installed {
    [
        (
            "orbitar",
            "0.1.0",
            "file:///w/dist/orbitar-0.1.0-py3-none-macosx_12_0_arm64.whl",
        ),
        (
            "orbitar-jth",
            "2.0.0",
            "file:///w/dist/orbitar_jth-2.0.0-py3-none-any.whl",
        ),
    ]
    .map(|(n, v, u)| (n.to_owned(), (v.to_owned(), Some(u.to_owned()))))
    .into()
}

struct Fake {
    taken: Vec<&'static str>,
    build: Result<(), String>,
    check: Vec<String>,
    floor: Vec<String>,
    install: Result<Installed, Vec<String>>,
    readme: Result<String, String>,
    tests: Vec<String>,
}

impl Default for Fake {
    fn default() -> Self {
        Self {
            taken: Vec::new(),
            build: Ok(()),
            check: Vec::new(),
            floor: Vec::new(),
            install: Ok(installed()),
            readme: Ok("{\"free_energy_ha\": -7.88}".into()),
            tests: Vec::new(),
        }
    }
}

impl Steps for Fake {
    fn build(&mut self) -> Result<Built, String> {
        self.taken.push("build");
        self.build.clone().map(|()| built())
    }
    fn check(&mut self, _: &Path) -> Vec<String> {
        self.taken.push("check");
        self.check.clone()
    }
    fn floor(&mut self, _: &Built) -> Vec<String> {
        self.taken.push("floor");
        self.floor.clone()
    }
    fn install(&mut self, _: &Built) -> Result<(xtask::Install, Installed), Vec<String>> {
        self.taken.push("install");
        let mut good = install();
        good.version_printed = good.version_wanted.clone();
        self.install.clone().map(|i| (good, i))
    }
    fn readme(&mut self) -> Result<String, String> {
        self.taken.push("readme");
        self.readme.clone()
    }
    fn tests(&mut self) -> Vec<String> {
        self.taken.push("tests");
        self.tests.clone()
    }
}

fn proved(mut fake: Fake) -> (Vec<String>, Vec<&'static str>) {
    let found = xtask::proof("macos", "aarch64-apple-darwin", &mut fake);
    (found, fake.taken)
}

#[test]
fn off_a_mac_the_proof_takes_no_step() {
    let mut fake = Fake::default();
    let found = xtask::proof("linux", "aarch64-apple-darwin", &mut fake);
    assert!(
        has(&found, "builds a Mac wheel on a Mac alone, not on linux"),
        "{found:?}"
    );
    assert!(fake.taken.is_empty());
}

#[test]
fn a_proof_whose_steps_pass_takes_every_step_and_passes() {
    let (found, taken) = proved(Fake::default());
    assert_eq!(found, Vec::<String>::new());
    assert_eq!(
        taken,
        [
            "build", "check", "check", "floor", "install", "readme", "tests"
        ]
    );
}

#[test]
fn a_failed_build_ends_the_proof_with_its_error() {
    let (found, taken) = proved(Fake {
        build: Err("maturin build exited".into()),
        ..Fake::default()
    });
    assert_eq!(found, ["maturin build exited"]);
    assert_eq!(taken, ["build"]);
}

#[test]
fn a_check_finding_stops_the_proof_before_the_install() {
    let (found, taken) = proved(Fake {
        check: vec!["wheel: x".into()],
        ..Fake::default()
    });
    assert_eq!(found, ["wheel: x", "wheel: x"]);
    assert_eq!(taken, ["build", "check", "check", "floor"]);
}

#[test]
fn a_floor_finding_stops_the_proof_before_the_install() {
    let (found, taken) = proved(Fake {
        floor: vec!["wheel: no scipy".into()],
        ..Fake::default()
    });
    assert_eq!(found, ["wheel: no scipy"]);
    assert_eq!(taken, ["build", "check", "check", "floor"]);
}

#[test]
fn a_failed_install_ends_the_proof_with_its_findings() {
    let (found, taken) = proved(Fake {
        install: Err(vec!["uv pip install".into()]),
        ..Fake::default()
    });
    assert_eq!(found, ["uv pip install"]);
    assert_eq!(taken, ["build", "check", "check", "floor", "install"]);
}

#[test]
fn an_install_of_another_file_stops_the_proof_before_readme() {
    let mut other = installed();
    other.get_mut("orbitar").unwrap().1 = Some("https://files.example/orbitar-0.1.0.whl".into());
    let (found, taken) = proved(Fake {
        install: Ok(other),
        ..Fake::default()
    });
    assert!(has(&found, "the venv's orbitar came from"), "{found:?}");
    assert_eq!(taken, ["build", "check", "check", "floor", "install"]);
}

#[test]
fn a_readme_outcome_without_a_finite_energy_fails() {
    let (found, taken) = proved(Fake {
        readme: Ok("{}".into()),
        ..Fake::default()
    });
    assert_eq!(
        found,
        ["wheel: README ## ASE left no finite free_energy_ha in its outcome.json"]
    );
    assert_eq!(taken.last(), Some(&"tests"));
}

#[test]
fn a_readme_run_that_exits_with_an_error_fails_with_it() {
    let (found, taken) = proved(Fake {
        readme: Err("README ## ASE exited with 1".into()),
        ..Fake::default()
    });
    assert_eq!(found, ["README ## ASE exited with 1"]);
    assert_eq!(taken.last(), Some(&"tests"));
}

#[test]
fn a_failed_readme_test_fails_the_proof() {
    let (found, _) = proved(Fake {
        tests: vec!["cargo test exited".into()],
        ..Fake::default()
    });
    assert_eq!(found, ["cargo test exited"]);
}

#[test]
fn the_pair_installed_from_its_files_at_their_versions_passes() {
    assert_eq!(
        xtask::pair_verdict(&built(), &installed()),
        Vec::<String>::new()
    );
}

#[test]
fn a_pair_whose_file_url_escapes_its_path_passes() {
    let built = Built {
        data: "/w/my dist/orbitar_jth-2.0.0-py3-none-any.whl".into(),
        ..built()
    };
    let mut got = installed();
    got.get_mut("orbitar-jth").unwrap().1 =
        Some("file:///w/my%20dist/orbitar_jth-2.0.0-py3-none-any.whl".into());
    assert_eq!(xtask::pair_verdict(&built, &got), Vec::<String>::new());
}

#[test]
fn a_pair_missing_a_distribution_fails() {
    let mut got = installed();
    got.remove("orbitar-jth");
    assert_eq!(
        xtask::pair_verdict(&built(), &got),
        ["wheel: the venv holds no orbitar-jth"]
    );
}

#[test]
fn a_pair_member_at_another_version_fails() {
    let mut got = installed();
    got.get_mut("orbitar").unwrap().0 = "0.2.0".into();
    let found = xtask::pair_verdict(&built(), &got);
    assert!(has(&found, "holds orbitar 0.2.0, not 0.1.0"), "{found:?}");
}

#[test]
fn a_pair_member_from_the_index_fails() {
    let mut got = installed();
    got.get_mut("orbitar-jth").unwrap().1 = None;
    let found = xtask::pair_verdict(&built(), &got);
    assert!(
        has(&found, "the venv's orbitar-jth came from None"),
        "{found:?}"
    );
}

#[test]
fn an_outcome_with_a_finite_free_energy_passes_and_any_other_fails() {
    assert_eq!(
        xtask::ase_verdict("{\"free_energy_ha\": -7.88}"),
        Vec::<String>::new()
    );
    assert_eq!(xtask::ase_verdict("{\"free_energy_ha\": null}").len(), 1);
}

#[test]
fn remaps_hold_each_root_and_its_canonical_spelling_the_widest_first() {
    let base = std::env::temp_dir().join(format!("xtask-remaps-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    std::fs::create_dir_all(base.join("real/src")).unwrap();
    std::os::unix::fs::symlink(base.join("real"), base.join("link")).unwrap();
    let pairs = xtask::remaps(&[
        (base.join("link/src"), "/src"),
        (base.join("link"), "/build"),
    ]);
    let canonical = std::fs::canonicalize(base.join("real")).unwrap();
    let want: Vec<(PathBuf, &str)> = vec![
        (base.join("link"), "/build"),
        (canonical.clone(), "/build"),
        (base.join("link/src"), "/src"),
        (canonical.join("src"), "/src"),
    ];
    let mut got = pairs.clone();
    got.sort_by_key(|(p, _)| p.components().count());
    assert_eq!(pairs, got, "the widest first");
    for pair in &want {
        assert!(pairs.contains(pair), "{pair:?} in {pairs:?}");
    }
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-mac-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn one_wheel_takes_the_one_wheel_of_its_prefix() {
    let dist = scratch("one-wheel");
    for file in [
        "orbitar-0.1.0-py3-none-any.whl",
        "orbitar_jth-2.0.0-py3-none-any.whl",
        "orbitar-0.1.0.tar.gz",
    ] {
        std::fs::write(dist.join(file), "").unwrap();
    }
    assert_eq!(
        xtask::one_wheel(&dist, "orbitar_jth-"),
        Ok(dist.join("orbitar_jth-2.0.0-py3-none-any.whl"))
    );
    assert_eq!(
        xtask::one_wheel(&dist, "orbitar-"),
        Ok(dist.join("orbitar-0.1.0-py3-none-any.whl"))
    );
}

#[test]
fn one_wheel_refuses_none_or_two_of_its_prefix() {
    let dist = scratch("two-wheels");
    let none = xtask::one_wheel(&dist, "orbitar-").unwrap_err();
    assert!(
        none.ends_with("holds 0 orbitar-* wheels, not one"),
        "{none}"
    );
    for file in [
        "orbitar-0.1.0-py3-none-any.whl",
        "orbitar-0.2.0-py3-none-any.whl",
    ] {
        std::fs::write(dist.join(file), "").unwrap();
    }
    let two = xtask::one_wheel(&dist, "orbitar-").unwrap_err();
    assert!(two.ends_with("holds 2 orbitar-* wheels, not one"), "{two}");
}

#[test]
fn maturin_pin_is_pyproject_toml_build_system_requirement() {
    let root = tree(
        "maturin-pin",
        &[(
            "pyproject.toml",
            "[build-system]\nrequires = [\"other==1\", \"maturin==1.9.6\"]\n",
        )],
    );
    assert_eq!(xtask::maturin_pin(&root), Ok("maturin==1.9.6".to_owned()));
    let none = tree(
        "maturin-none",
        &[(
            "pyproject.toml",
            "[build-system]\nrequires = [\"maturin>=1\"]\n",
        )],
    );
    assert!(xtask::maturin_pin(&none).is_err());
}

#[test]
fn about_pin_is_the_dockerfile_cargo_about_version() {
    let line =
        "RUN cargo install --locked --version 0.9.3 --features cli --root /opt/about cargo-about\n";
    let root = tree("about-pin", &[("container/Dockerfile", line)]);
    assert_eq!(xtask::about_pin(&root), Ok("0.9.3".to_owned()));
    let none = tree(
        "about-none",
        &[(
            "container/Dockerfile",
            "RUN cargo install --locked --version 1 other\n",
        )],
    );
    assert!(xtask::about_pin(&none).is_err());
}

#[test]
fn about_config_keeps_the_config_and_sets_its_target_alone() {
    let config = "accepted = [\"MIT\"]\ntargets = [\"x86_64-unknown-linux-gnu\"]\n";
    let root = tree("about-config", &[("container/about.toml", config)]);
    let out = xtask::about_config(&root, &root, "aarch64-apple-darwin").unwrap();
    let got: toml::Table = toml::from_str(&std::fs::read_to_string(out).unwrap()).unwrap();
    assert_eq!(
        got["targets"],
        toml::Value::Array(vec!["aarch64-apple-darwin".into()])
    );
    assert_eq!(got["accepted"], toml::Value::Array(vec!["MIT".into()]));
}

#[test]
fn tracked_copies_the_tracked_files_and_links_and_no_untracked_file() {
    let root = scratch("tracked-root");
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "--quiet"]);
    std::fs::create_dir_all(root.join("python")).unwrap();
    std::fs::write(root.join("python/a.py"), "a").unwrap();
    std::os::unix::fs::symlink("a.py", root.join("python/b.py")).unwrap();
    git(&["add", "python"]);
    std::fs::write(root.join("python/untracked.py"), "u").unwrap();
    let src = scratch("tracked-src");
    xtask::tracked(&root, &src).unwrap();
    assert_eq!(
        std::fs::read_to_string(src.join("python/a.py")).unwrap(),
        "a"
    );
    assert_eq!(
        std::fs::read_link(src.join("python/b.py")).unwrap(),
        Path::new("a.py")
    );
    assert!(!src.join("python/untracked.py").exists());
    assert!(!src.join(".git").exists());
}

#[test]
fn clear_removes_a_directory_and_takes_a_missing_one_as_clear() {
    let dir = scratch("clear");
    std::fs::write(dir.join("x"), "").unwrap();
    assert_eq!(xtask::clear(&dir), Ok(()));
    assert!(!dir.exists());
    assert_eq!(xtask::clear(&dir), Ok(()));
}

#[test]
fn clear_reports_a_path_that_it_cannot_remove() {
    let dir = scratch("clear-file");
    let file = dir.join("not-a-directory");
    std::fs::write(&file, "").unwrap();
    let refused = xtask::clear(&file).unwrap_err();
    assert!(
        refused.starts_with(&file.display().to_string()),
        "{refused}"
    );
}

#[test]
fn tracked_skips_a_tracked_file_that_the_worktree_deleted() {
    let root = scratch("tracked-deleted");
    let git = |args: &[&str]| {
        assert!(
            std::process::Command::new("git")
                .args(args)
                .current_dir(&root)
                .status()
                .unwrap()
                .success()
        )
    };
    git(&["init", "--quiet"]);
    std::fs::write(root.join("kept.py"), "k").unwrap();
    std::fs::write(root.join("gone.py"), "g").unwrap();
    git(&["add", "kept.py", "gone.py"]);
    std::fs::remove_file(root.join("gone.py")).unwrap();
    let src = scratch("tracked-deleted-src");
    assert_eq!(xtask::tracked(&root, &src), Ok(()));
    assert!(src.join("kept.py").is_file());
    assert!(!src.join("gone.py").exists());
}

#[test]
fn c_flags_keep_a_path_with_a_space_or_a_quote_one_shell_word() {
    let pairs = [
        (PathBuf::from("/w/my dist"), "/build"),
        (PathBuf::from("/w/it's"), "/src"),
    ];
    let flags = xtask::c_flags(&pairs);
    let out = std::process::Command::new("python3")
        .args([
            "-I",
            "-c",
            "import json, shlex, sys; print(json.dumps(shlex.split(sys.argv[1])))",
            &flags,
        ])
        .output()
        .unwrap();
    let words: Vec<String> = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(
        words,
        [
            "-ffile-prefix-map=/w/my dist=/build",
            "-ffile-prefix-map=/w/it's=/src"
        ]
    );
}

#[test]
fn a_mac_header_with_more_commands_than_its_region_holds_is_malformed() {
    let mut bytes = macho_bytes(
        CPU_ARM64,
        MH_EXECUTE,
        &[build_version(PLATFORM_MACOS, (12, 0, 0))],
    );
    bytes[16] = 2;
    bytes.extend([0x19, 0, 0, 0, 8, 0, 0, 0]);
    assert_eq!(xtask::machine(&bytes), None);
}

fn words(step: &xtask::Invocation) -> Vec<String> {
    step.words()
}

#[test]
fn the_interpreter_plan_installs_and_finds_the_exact_managed_python() {
    let plan = xtask::python_plan();
    let got: Vec<Vec<String>> = plan.iter().map(|(_, s)| words(s)).collect();
    assert_eq!(
        got,
        [
            vec!["python", "install", "3.12.16"],
            vec![
                "python",
                "find",
                "--no-project",
                "--python-preference",
                "only-managed",
                "3.12.16"
            ]
        ]
    );
    assert_eq!(xtask::PYTHON, "3.12.16");
}

#[test]
fn the_tools_plan_takes_the_exact_interpreter_maturin_by_hash_and_cargo_about_locked() {
    let plan = xtask::tools_plan(
        Path::new("/w"),
        Path::new("/py/bin/python3.12"),
        Path::new("/w/maturin.txt"),
        "0.9.3",
    );
    let got: Vec<(xtask::Program, Vec<String>)> =
        plan.iter().map(|(_, s)| (s.program, words(s))).collect();
    assert_eq!(
        got,
        [
            (
                xtask::Program::Uv,
                [
                    "venv",
                    "--quiet",
                    "--python",
                    "/py/bin/python3.12",
                    "/w/tools"
                ]
                .map(String::from)
                .to_vec()
            ),
            (
                xtask::Program::Uv,
                [
                    "pip",
                    "install",
                    "--quiet",
                    "--no-deps",
                    "--require-hashes",
                    "--only-binary",
                    ":all:",
                    "--python",
                    "/w/tools/bin/python",
                    "-r",
                    "/w/maturin.txt"
                ]
                .map(String::from)
                .to_vec()
            ),
            (
                xtask::Program::Cargo,
                [
                    "install",
                    "--quiet",
                    "--locked",
                    "--version",
                    "0.9.3",
                    "--features",
                    "cli",
                    "--root",
                    "/w/about",
                    "cargo-about"
                ]
                .map(String::from)
                .to_vec()
            ),
        ]
    );
}

#[test]
fn the_maturin_requirement_pins_pyproject_maturin_by_both_mac_hashes() {
    let root = tree(
        "maturin-requirement",
        &[(
            "pyproject.toml",
            "[build-system]\nrequires = [\"maturin==1.9.6\"]\n",
        )],
    );
    let line = xtask::maturin_requirement(&root).unwrap();
    assert_eq!(
        line,
        "maturin==1.9.6 --hash=sha256:5263dda3f71feef2e4122baf5c4620e4b3710dbb7f2121f85a337182de214369 \
         --hash=sha256:fe78262c2800c92f67d1ce3c0f6463f958a692cc67bfb572e5dbf5b4b696a8ba\n"
    );
}

#[test]
fn the_about_plan_generates_the_notices_with_its_config_in_the_source() {
    let step = xtask::about_plan(
        Path::new("/w/src/target-about.toml"),
        OsStr::new("/w/about/bin"),
        Path::new("/w/src"),
    );
    assert_eq!(step.program, xtask::Program::CargoAbout);
    assert_eq!(
        words(&step),
        [
            "generate",
            "--locked",
            "--fail",
            "--config",
            "/w/src/target-about.toml",
            "--output-file",
            "THIRD-PARTY-NOTICES",
            "container/about.hbs"
        ]
    );
    assert_eq!(step.var("PATH").as_deref(), Some("/w/about/bin"));
    assert_eq!(step.dir.as_deref(), Some(Path::new("/w/src")));
}

/// A maturin build of `target` with fixed paths.
fn maturin(target: &str) -> Result<xtask::Invocation, String> {
    let pairs = [(PathBuf::from("/w/my src"), "/src")];
    xtask::maturin_plan(&xtask::Build {
        target,
        src: Path::new("/w/src"),
        dist: Path::new("/w/dist"),
        cargo_dir: Path::new("/w/cargo"),
        commit: "abc",
        cargo: OsStr::new("/bin/cargo"),
        path: OsStr::new("/w/tools/bin"),
        pairs: &pairs,
    })
}

#[test]
fn the_maturin_plan_builds_the_mac_target_locked_at_its_deployment_floor() {
    let step = maturin("aarch64-apple-darwin").unwrap();
    assert_eq!(step.program, xtask::Program::Maturin);
    assert_eq!(
        words(&step),
        [
            "build",
            "--release",
            "--locked",
            "--target",
            "aarch64-apple-darwin",
            "--out",
            "/w/dist"
        ]
    );
    for (key, value) in [
        ("MACOSX_DEPLOYMENT_TARGET", "12.0"),
        ("ORBITAR_COMMIT", "abc"),
        ("SOURCE_DATE_EPOCH", "1767225600"),
        ("CARGO", "/bin/cargo"),
        ("CARGO_TARGET_DIR", "/w/cargo"),
        (
            "CARGO_ENCODED_RUSTFLAGS",
            "--remap-path-prefix=/w/my src=/src\x1f-Clink-arg=-Wl,-S",
        ),
        (
            "CFLAGS_aarch64_apple_darwin",
            "'-ffile-prefix-map=/w/my src=/src'",
        ),
        ("CC_SHELL_ESCAPED_FLAGS", "1"),
        ("PATH", "/w/tools/bin"),
    ] {
        assert_eq!(step.var(key).as_deref(), Some(value), "{key}");
    }
    assert_eq!(step.unset, [OsStr::new("RUSTFLAGS")]);
    assert_eq!(step.dir.as_deref(), Some(Path::new("/w/src")));
}

#[test]
fn the_maturin_plan_refuses_a_target_that_is_no_mac_platform() {
    let refused = maturin("x86_64-unknown-linux-gnu").unwrap_err();
    assert!(refused.contains("no Mac platform"), "{refused}");
}

#[test]
fn the_data_plan_installs_the_engine_alone_stages_and_builds_offline() {
    let plan = xtask::data_plan(
        Path::new("/w"),
        Path::new("/py/bin/python3.12"),
        Path::new("/w/dist/orbitar.whl"),
    );
    let got: Vec<Vec<String>> = plan.iter().map(|(_, s)| words(s)).collect();
    assert_eq!(
        got,
        [
            [
                "venv",
                "--quiet",
                "--python",
                "/py/bin/python3.12",
                "/w/fetch"
            ]
            .map(String::from)
            .to_vec(),
            [
                "pip",
                "install",
                "--quiet",
                "--no-deps",
                "--python",
                "/w/fetch/bin/python",
                "/w/dist/orbitar.whl"
            ]
            .map(String::from)
            .to_vec(),
            [
                "run",
                "--no-project",
                "--quiet",
                "--python",
                "/w/fetch/bin/python",
                "--",
                "python",
                "-I",
                "/w/src/packages/orbitar-jth/stage.py",
                "/w/fetched",
            ]
            .map(String::from)
            .to_vec(),
            [
                "build",
                "--quiet",
                "--offline",
                "--no-cache",
                "--wheel",
                "--out-dir",
                "/w/dist"
            ]
            .map(String::from)
            .to_vec(),
        ]
    );
    assert_eq!(
        plan[3].1.dir.as_deref(),
        Some(Path::new("/w/src/packages/orbitar-jth"))
    );
}

#[test]
fn the_floor_plan_resolves_wheels_only_at_the_exact_python_and_the_deployment_floor() {
    let step = xtask::floor_plan("aarch64-apple-darwin", Path::new("/w/floor.txt")).unwrap();
    assert_eq!(
        words(&step),
        [
            "pip",
            "compile",
            "--quiet",
            "--no-header",
            "--only-binary",
            ":all:",
            "--python-version",
            "3.12.16",
            "--python-platform",
            "aarch64-apple-darwin",
            "/w/floor.txt",
        ]
    );
    assert_eq!(
        step.var("MACOSX_DEPLOYMENT_TARGET").as_deref(),
        Some("12.0")
    );
    assert!(xtask::floor_plan("x86_64-unknown-linux-gnu", Path::new("/w/floor.txt")).is_err());
}

#[test]
fn the_venv_plans_run_python_isolated_in_the_venv() {
    let metadata = words(&xtask::metadata_plan(Path::new("/w/venv/bin/python")));
    assert_eq!(
        metadata[..8],
        [
            "run",
            "--no-project",
            "--quiet",
            "--python",
            "/w/venv/bin/python",
            "--",
            "python",
            "-I"
        ]
    );
    assert_eq!(
        metadata.last().map(String::as_str),
        Some("[\"orbitar\",\"orbitar-jth\"]")
    );
    let readme = xtask::readme_plan(
        Path::new("/w/venv/bin/python"),
        "print(1)",
        Path::new("/w/ase"),
    );
    assert_eq!(
        words(&readme),
        [
            "run",
            "--no-project",
            "--quiet",
            "--python",
            "/w/venv/bin/python",
            "--",
            "python",
            "-I",
            "-c",
            "print(1)"
        ]
    );
    assert_eq!(readme.dir.as_deref(), Some(Path::new("/w/ase")));
}
