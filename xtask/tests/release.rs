mod mac;

use crate::{has, lever, tree, write};
use sha2::Digest;
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
};

const CARGO: &str =
    "[package]\nname = \"orbitar\"\nversion = \"0.1.0\"\nlicense = \"LicenseRef-x\"\n";
const LICENSE: &str = "the license text\n";
const NOTICES: &str = "Third-party notices\nused by: notice 1.2.3\n";
const PYLOCK: &str = "[[packages]]\nname = \"ase\"\nversion = \"3.29.0\"\n";
const NAME: &str = "orbitar-0.1.0-py3-none-manylinux_2_28_x86_64.whl";
const MAC: &str = "orbitar-0.1.0-py3-none-macosx_12_0_arm64.whl";
const EM_X86_64: u8 = 62;
const EM_AARCH64: u8 = 183;
const CPU_ARM64: u32 = 0x0100_000c;
const CPU_X86_64: u32 = 0x0100_0007;
const METADATA: &str = "Metadata-Version: 2.4\nName: orbitar\nVersion: 0.1.0\nAuthor: The orbitaR developers\n\
     License-Expression: LicenseRef-x\nRequires-Dist: ase==3.29.0\nRequires-Dist: orbitar-jth==2.0.0\n";
const JTH_PROJECT: &str = "[project]\nname = \"orbitar-jth\"\nversion = \"2.0.0\"\n";

fn elf(machine: u8) -> String {
    let mut bytes = b"\x7fELF\x02\x01\x01".to_vec();
    bytes.resize(16, 0);
    bytes.extend([2, 0, machine, 0]);
    bytes.into_iter().map(char::from).collect()
}

const MH_EXECUTE: u32 = 2;
const MH_DYLIB: u32 = 6;
const PLATFORM_MACOS: u32 = 1;
const PLATFORM_IOS: u32 = 2;
const LC_BUILD_VERSION: u32 = 0x32;

fn build_version(platform: u32, (major, minor, patch): (u8, u8, u8)) -> (u32, u32, Vec<u8>) {
    let mut body = platform.to_le_bytes().to_vec();
    body.extend([patch, minor, major, 0, 0, 0, 15, 0, 0, 0, 0, 0]);
    (LC_BUILD_VERSION, 24, body)
}

fn macho_bytes(cpu: u32, filetype: u32, commands: &[(u32, u32, Vec<u8>)]) -> Vec<u8> {
    let size: usize = commands.iter().map(|(_, _, body)| 8 + body.len()).sum();
    let count = u32::try_from(commands.len()).unwrap();
    let words = [
        0xfeed_facf,
        cpu,
        0,
        filetype,
        count,
        u32::try_from(size).unwrap(),
        0,
        0,
    ];
    let mut bytes: Vec<u8> = words.iter().flat_map(|w: &u32| w.to_le_bytes()).collect();
    for (command, declared, body) in commands {
        bytes.extend(command.to_le_bytes());
        bytes.extend(declared.to_le_bytes());
        bytes.extend(body);
    }
    bytes
}

fn latin(bytes: Vec<u8>) -> String {
    bytes.into_iter().map(char::from).collect()
}

fn macho(cpu: u32, major: u8) -> String {
    let command = build_version(PLATFORM_MACOS, (major, 0, 0));
    latin(macho_bytes(cpu, MH_EXECUTE, &[command]))
}

fn wheel(root: &Path, name: &str, members: &[(&str, String)]) {
    let pairs: Vec<[&str; 2]> = members.iter().map(|(p, t)| [*p, t.as_str()]).collect();
    let status = Command::new("python3")
        .args([
            "-I",
            "-c",
            "import json, sys, zipfile\nwith zipfile.ZipFile(sys.argv[1], 'w') as z:\n    \
             [z.writestr(p, t.encode('latin-1') if p.endswith('/scripts/orbitar') else t) \
             for p, t in json.loads(sys.argv[2])]",
        ])
        .arg(root.join(name))
        .arg(serde_json::to_string(&pairs).unwrap())
        .status()
        .unwrap();
    assert!(status.success());
}

const FAMILY: &str = "url = \"https://example.invalid\"\n";

fn checked(name: &str, file: &str, change: impl Fn(&mut Vec<(&str, String)>)) -> Vec<String> {
    checked_beside(name, file, &[], change)
}

fn checked_beside(
    name: &str,
    file: &str,
    beside: &[(&str, &str)],
    change: impl Fn(&mut Vec<(&str, String)>),
) -> Vec<String> {
    let mut files = vec![
        ("Cargo.toml", CARGO),
        ("LICENSE", LICENSE),
        ("datasets/jth-2.0.toml", FAMILY),
        ("python/pylock.toml", PYLOCK),
        ("packages/orbitar-jth/pyproject.toml", JTH_PROJECT),
    ];
    files.extend_from_slice(beside);
    let root = tree(&format!("wheel-{name}"), &files);
    let mut members: Vec<(&str, String)> = [
        ("orbitar/__init__.py", ""),
        ("orbitar/ase.py", ""),
        ("orbitar/datasets.py", ""),
        ("orbitar-0.1.0.data/scripts/orbitar", &elf(EM_X86_64)),
        ("orbitar-0.1.0.dist-info/METADATA", METADATA),
        ("orbitar-0.1.0.dist-info/licenses/LICENSE", LICENSE),
        (
            "orbitar-0.1.0.dist-info/licenses/THIRD-PARTY-NOTICES",
            NOTICES,
        ),
        ("orbitar/families/jth-2.0.toml", FAMILY),
    ]
    .map(|(p, t)| (p, t.to_owned()))
    .into();
    if file.contains("macosx") {
        members[3].1 = macho(CPU_ARM64, 12);
    }
    change(&mut members);
    wheel(&root, file, &members);
    write(
        &root,
        "tree",
        "echo 'orbitar v0.1.0 (/x)'; echo 'notice v1.2.3 (/x/notice)'\n",
    );
    let mut env = crate::env(false);
    env.cargo = "/bin/sh".into();
    xtask::run(&["wheel", "--check", file], &root, &env)
}

#[test]
fn a_wheel_with_the_engine_and_the_package_passes() {
    assert_eq!(checked("good", NAME, |_| {}), Vec::<String>::new());
}

#[test]
fn a_wheel_without_a_member_fails() {
    for member in ["orbitar/ase.py", "orbitar-0.1.0.data/scripts/orbitar"] {
        let found = checked("missing", NAME, |m| m.retain(|(p, _)| *p != member));
        assert!(has(&found, &format!("{member} is missing")), "{found:?}");
    }
}

#[test]
fn a_wheel_whose_family_tables_are_not_datasets_toml_fails() {
    let want = "orbitar/families/ is not datasets/*.toml byte for byte";
    let found = checked("no-families", NAME, |m| {
        m.retain(|(p, _)| !p.starts_with("orbitar/families/"))
    });
    assert!(has(&found, want), "{found:?}");
    let found = checked("stale-family", NAME, |m| m[7].1 = "url = \"x\"\n".into());
    assert!(has(&found, want), "{found:?}");
    let found = checked("extra-family", NAME, |m| {
        m.push(("orbitar/families/x.toml", String::new()))
    });
    assert!(has(&found, want), "{found:?}");
}

#[test]
fn a_wheel_that_holds_a_dataset_fails() {
    let found = checked("dataset", NAME, |m| {
        m.push(("orbitar/Si.GGA_PBE-JTH.xml", "<paw/>".into()))
    });
    assert!(has(&found, "datasets never enter the wheel"), "{found:?}");
}

#[test]
fn a_wheel_member_with_a_paw_xml_root_under_a_plain_name_fails() {
    let found = checked("plain-paw", NAME, |m| {
        m.push((
            "orbitar/O.xml",
            "<?xml version=\"1.0\"?>\n<!-- O -->\n<paw_dataset version=\"0.7\">\n".into(),
        ))
    });
    assert!(
        has(&found, "orbitar/O.xml opens with a PAW-XML root"),
        "{found:?}"
    );
}

#[test]
fn a_wheel_member_with_a_paw_xml_root_after_a_prolog_longer_than_the_image_window_fails() {
    let comment = "x".repeat(70_000);
    let found = checked("long-prolog", NAME, |m| {
        m.push((
            "orbitar/long.xml",
            format!("<?xml version=\"1.0\"?>\n<!-- {comment} -->\n<paw_dataset version=\"0.7\">\n"),
        ))
    });
    assert!(
        has(&found, "orbitar/long.xml opens with a PAW-XML root"),
        "{found:?}"
    );
}

#[test]
fn a_wheel_with_a_member_path_twice_fails() {
    let found = checked("twice", NAME, |m| m.push(("orbitar/ase.py", "x".into())));
    assert!(
        has(&found, "orbitar/ase.py is a member 2 times"),
        "{found:?}"
    );
}

#[test]
fn a_wheel_whose_metadata_names_a_person_a_url_another_version_or_an_extra_fails() {
    for (field, needle) in [
        (
            format!("Author-email: someone{}example.invalid\n", '@'),
            "no person, address, or URL",
        ),
        (
            "Project-URL: Source, https://example.invalid\n".into(),
            "no person, address, or URL",
        ),
        (
            "Author: A Person\n".into(),
            "not `Author: The orbitaR developers`",
        ),
    ] {
        let metadata = format!("{METADATA}{field}");
        let found = checked("personal", NAME, |m| m[4].1 = metadata.clone());
        assert!(has(&found, needle), "{found:?}");
    }
    let found = checked("version", NAME, |m| {
        m[4].1 = METADATA.replace("Version: 0.1.0", "Version: 0.2.0")
    });
    assert!(has(&found, "Version: 0.1.0 (Cargo.toml)"), "{found:?}");
    let found = checked("requires", NAME, |m| {
        m[4].1 = METADATA.replace("ase==3.29.0", "ase==3.28.0")
    });
    assert!(has(&found, "differs from python/pylock.toml"), "{found:?}");
    for metadata in [
        METADATA.replace("Requires-Dist: orbitar-jth==2.0.0\n", ""),
        METADATA.replace("orbitar-jth==2.0.0", "orbitar-jth==1.0.0"),
    ] {
        let found = checked("requires-jth", NAME, |m| m[4].1 = metadata.clone());
        assert!(has(&found, "and packages/orbitar-jth"), "{found:?}");
    }
    for (field, needle) in [
        ("Provides-Extra: mcp\n", "provides the extra `mcp`"),
        (
            "Requires-Dist: mcp==2.3.0 ; extra == 'mcp'\n",
            "has a marker; the wheel has no extra",
        ),
    ] {
        let metadata = format!("{METADATA}{field}");
        let found = checked("extra", NAME, |m| m[4].1 = metadata.clone());
        assert!(has(&found, needle), "{field}: {found:?}");
    }
}

fn engine_holding(name: &str, word: &str) -> Vec<String> {
    checked(name, NAME, |m| {
        m[3].1 = elf(EM_X86_64) + &format!("\0{word}\0\x03");
    })
}

#[test]
fn a_linux_engine_that_names_a_person_or_an_account_fails() {
    let at = '@';
    for (i, word) in [
        format!("someone{at}example.org"),
        format!("https://github.com/{}/orbitar", "someone"),
    ]
    .into_iter()
    .enumerate()
    {
        let found = engine_holding(&format!("identity-{i}"), &word);
        let want = format!(
            "wheel orbitar-0.1.0.data/scripts/orbitar: `{word}` names a person, an account, or a host's account path"
        );
        assert_eq!(found, [want]);
    }
}

#[test]
fn a_linux_engine_that_holds_a_home_directory_fails() {
    for (i, word) in [
        format!("/{}/someone/.cargo/registry/src/x.rs", "home"),
        format!("/{}/someone/src/main.rs", "Users"),
    ]
    .into_iter()
    .enumerate()
    {
        let found = engine_holding(&format!("home-{i}"), &word);
        let want = format!(
            "wheel orbitar-0.1.0.data/scripts/orbitar: `{word}` holds a home directory; build the wheel with `cargo xtask wheel`"
        );
        assert_eq!(found, [want]);
    }
}

#[test]
fn an_install_whose_venv_cannot_be_removed_fails_before_any_process() {
    let root = tree(
        "install-venv",
        &[("target/wheel/venv", "a file where the venv goes")],
    );
    let found = xtask::install(&root, "abc").err().unwrap();
    let venv = root.join("target/wheel/venv").display().to_string();
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(found[0].starts_with(&format!("{venv}: ")), "{found:?}");
    assert!(root.join("target/wheel/venv").is_file());
}

#[test]
fn the_linux_install_plan_takes_pylock_first_then_the_local_wheels_alone() {
    let dirs = [
        PathBuf::from("/r/target/wheel/wheel"),
        PathBuf::from("/r/target/wheel/jth-wheel"),
    ];
    let wheels = xtask::Wheels::Pylock {
        interpreter: OsStr::new("3.12"),
        dirs: &dirs,
    };
    let readme = [
        "pip install orbitar".to_owned(),
        "orbitar --version".to_owned(),
    ];
    let plan = xtask::install_plan(
        Path::new("/r"),
        Path::new("/r/target/wheel/venv"),
        &wheels,
        &readme,
    );
    let words: Vec<(&str, Vec<String>)> = plan
        .iter()
        .map(|(what, step)| (what.as_str(), step.words()))
        .collect();
    let py = "/r/target/wheel/venv/bin/python";
    let pip = |rest: &[&str]| {
        ["pip", "install", "--quiet", "--python", py]
            .iter()
            .chain(rest)
            .map(|w| (*w).to_owned())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        words,
        [
            (
                "uv venv",
                [
                    "venv",
                    "--quiet",
                    "--python",
                    "3.12",
                    "/r/target/wheel/venv"
                ]
                .map(String::from)
                .to_vec()
            ),
            (
                "python/pylock.toml",
                pip(&["--no-build", "-r", "python/pylock.toml"])
            ),
            (
                "pip install orbitar",
                pip(&[
                    "--no-index",
                    "--no-deps",
                    "--find-links",
                    "/r/target/wheel/wheel",
                    "--find-links",
                    "/r/target/wheel/jth-wheel",
                    "orbitar",
                    "orbitar-jth",
                ])
            ),
            (
                "orbitar --version",
                [
                    "run",
                    "--no-project",
                    "--quiet",
                    "--python",
                    py,
                    "--",
                    "sh",
                    "-c",
                    "orbitar --version"
                ]
                .map(String::from)
                .to_vec()
            ),
        ]
    );
    assert!(
        plan.iter()
            .all(|(_, step)| step.program == xtask::Program::Uv)
    );
    assert!(
        plan[1..]
            .iter()
            .all(|(_, step)| step.dir.as_deref() == Some(Path::new("/r")))
    );
}

#[test]
fn the_files_install_plan_takes_the_two_files_by_path_wheels_only() {
    let files = [
        PathBuf::from("/d/orbitar-0.1.0-py3-none-macosx_12_0_arm64.whl"),
        PathBuf::from("/d/orbitar_jth-2.0.0-py3-none-any.whl"),
    ];
    let wheels = xtask::Wheels::Files {
        interpreter: OsStr::new("/py/bin/python3.12"),
        files: &files,
    };
    let plan = xtask::install_plan(
        Path::new("/r"),
        Path::new("/w/venv"),
        &wheels,
        &["pip install orbitar".to_owned()],
    );
    let words: Vec<Vec<String>> = plan.iter().map(|(_, step)| step.words()).collect();
    assert_eq!(words.len(), 2);
    assert_eq!(
        words[0],
        [
            "venv",
            "--quiet",
            "--python",
            "/py/bin/python3.12",
            "/w/venv"
        ]
    );
    assert_eq!(
        words[1],
        [
            "pip",
            "install",
            "--quiet",
            "--python",
            "/w/venv/bin/python",
            "--only-binary",
            ":all:",
            "/d/orbitar-0.1.0-py3-none-macosx_12_0_arm64.whl",
            "/d/orbitar_jth-2.0.0-py3-none-any.whl",
        ]
    );
}

#[test]
fn a_wheel_member_with_build_paths_of_the_container_and_no_home_passes() {
    let found = checked("container-paths", NAME, |m| {
        m[3].1 = elf(EM_X86_64)
            + "\0/opt/cargo/registry/src/index.crates.io-1/faer-0.24.4/src/lib.rs\0\x01/rustc/0a1b/library/std/src/io.rs\0\
                  /home/\0/Users//x\0src/main.rs\0";
    });
    assert_eq!(found, Vec::<String>::new());
}

#[test]
fn a_wheel_of_an_unknown_platform_tag_fails() {
    let other = "orbitar-0.1.0-py3-none-manylinux_2_39_x86_64.whl";
    let found = checked("platform", other, |_| {});
    assert!(
        has(
            &found,
            &format!("{other} is not orbitar-<version>-py3-none-<tag>.whl")
        ),
        "{found:?}"
    );
}

#[test]
fn a_wheel_of_another_version_than_cargo_toml_fails() {
    let found = checked(
        "version",
        "orbitar-0.2.0-py3-none-macosx_12_0_arm64.whl",
        |_| {},
    );
    assert!(has(&found, &format!("is not {MAC}")), "{found:?}");
}

#[test]
fn a_wheel_without_the_license_or_a_notice_for_a_linked_crate_fails() {
    let found = checked("no-notice", NAME, |m| {
        m[6].1 = "Third-party notices\n".into()
    });
    assert!(
        has(
            &found,
            "holds no notice for notice 1.2.3, which the binary links"
        ),
        "{found:?}"
    );
    let found = checked("no-notices", NAME, |m| m.truncate(6));
    assert!(has(&found, "THIRD-PARTY-NOTICES is missing"), "{found:?}");
    let found = checked("other-license", NAME, |m| m[5].1 = "MIT\n".into());
    assert!(
        has(&found, "licenses/LICENSE is not LICENSE byte for byte"),
        "{found:?}"
    );
    let found = checked("expression", NAME, |m| {
        m[4].1 = METADATA.replace("LicenseRef-x", "MIT")
    });
    assert!(
        has(&found, "License-Expression: LicenseRef-x (Cargo.toml)"),
        "{found:?}"
    );
}

#[test]
fn only_the_engine_binary_reads_the_scp_form_at_a_domain_host_alone() {
    let at = '@';
    let engine = "orbitar-0.1.0.data/scripts/orbitar";
    let module = "orbitar/ase.py";
    let metadata = "orbitar-0.1.0.dist-info/METADATA";
    let code = format!("t3{at}G:4");
    let unqualified = format!("me{at}laptop:src/x");
    let domain = format!("git{at}example.org:o/r");
    for (word, flagged) in [
        (&code, [false, true, true]),
        (&unqualified, [false, true, true]),
        (&domain, [true, true, true]),
    ] {
        let found = checked(&format!("scp-{}", word.len()), NAME, |m| {
            for (path, text) in m.iter_mut() {
                match *path {
                    p if p == engine => *text = elf(EM_X86_64) + &format!("\0{word}\0\x03"),
                    p if p == module => *text = format!("HOST = \"{word}\"\n"),
                    p if p == metadata => text.push_str(&format!("Summary: {word}\n")),
                    _ => {}
                }
            }
        });
        for (member, flag) in [engine, module, metadata].into_iter().zip(flagged) {
            let finding = format!("wheel {member}: `{word}` names a person, an account");
            assert_eq!(has(&found, &finding), flag, "{member} {word}: {found:?}");
        }
    }
}

#[test]
fn a_script_without_the_elf_magic_or_machine_code_outside_scripts_keeps_the_scp_form_at_any_host() {
    let at = '@';
    let engine = "orbitar-0.1.0.data/scripts/orbitar";
    let module = "orbitar/ase.py";
    for word in [format!("t3{at}G:4"), format!("me{at}laptop:src/x")] {
        let found = checked(&format!("half-{}", word.len()), NAME, |m| {
            for (path, text) in m.iter_mut() {
                match *path {
                    p if p == engine => *text = format!("#!/bin/sh\n# {word}\n"),
                    p if p == module => *text = elf(EM_X86_64) + &format!("\0{word}\0\x03"),
                    _ => {}
                }
            }
        });
        for member in [engine, module] {
            let finding = format!("wheel {member}: `{word}` names a person, an account");
            assert!(has(&found, &finding), "{member} {word}: {found:?}");
        }
    }
}

fn scanned(name: &str, text: &str) -> Vec<String> {
    let root = tree(&format!("image-scan-{name}"), &[("history.txt", text)]);
    lever(&["image", "--scan", "history.txt"], &root)
}

#[test]
fn image_metadata_with_an_email_or_an_account_url_fails() {
    let at = '@';
    for word in [
        format!("someone{at}example.org"),
        format!("email:someone{at}example.org"),
        format!("https://github.com/{}/orbitar", "someone"),
        format!("https://GitHub.COM/{}/orbitar", "Someone"),
        format!("LABEL=src=https://gitlab.com/{}/project", "group"),
        format!("ghcr.io/{}/image:1", "someone"),
        format!("https://someone:token{at}example.org/x"),
        format!("git{at}example.org:someone/repo.git"),
        format!("mail{}someone", "to:"),
        format!("https://app.{}/p/Some-Page-0123456789abcdef", "notion.com"),
        format!("https://team.{}/page", "notion.site"),
        format!("https://docs.{}/document/d/x", "google.com"),
        format!("https://{}/code/session_0123", "claude.ai"),
        format!("someone{at}163.com"),
        format!("someone{at}mail.example.org"),
        format!("someone{at}1.mail.example.org"),
    ] {
        let found = scanned("named", &format!("RUN /bin/sh -c echo {word}\n"));
        assert!(
            has(&found, "names a person, an account"),
            "{word}: {found:?}"
        );
    }
}

#[test]
fn image_metadata_with_digests_and_plain_urls_passes() {
    let text = "FROM python@sha256:935c3c5b\nRUN curl https://static.rust-lang.org/rustup/archive/1.28.2/rustup-init\n\
                source = \"registry+https://github.com/rust-lang/crates.io-index\"\nCOPY Si@1.00.toml /work/\n\
                FROM quay.io/pypa/manylinux_2_28_x86_64@sha256:d086\n\
                ENV PATH=/opt/cargo/bin HOME=/work\nRUN uv pip install ase==3.29.0 # https://github.com\n\
                LABEL by=\"orbitar-admin <360476282+orbitar-admin@users.noreply.github.com>\" merged=NoReply@GitHub.com\n";
    assert_eq!(scanned("plain", text), Vec::<String>::new());
}

#[test]
fn an_image_of_a_tree_that_differs_from_its_commit_is_refused_before_any_build() {
    let root = tree("image-dirty", &[("a.txt", "a\n")]);
    crate::git(&root, &["init", "-q"]);
    crate::git(&root, &["add", "-A"]);
    crate::git(&root, &["commit", "-q", "-m", "a"]);
    write(&root, "a.txt", "b\n");
    let found = lever(&["image"], &root);
    assert!(
        has(&found, "-dirty; bench bakes one commit, so commit first"),
        "{found:?}"
    );
}

#[test]
fn bench_bakes_a_clean_commit_and_refuses_a_dirty_one_or_none() {
    assert_eq!(xtask::baked(Ok("abc123".into())), Ok("abc123".into()));
    let refused = xtask::baked(Ok("abc123-dirty".into())).unwrap_err();
    assert!(has(&refused, "differs from abc123-dirty"), "{refused:?}");
    let none = xtask::baked(Err("git: not a repository".into())).unwrap_err();
    assert_eq!(none, vec!["git: not a repository".to_owned()]);
}

#[test]
fn the_readme_lines_of_a_section_are_its_first_block_without_comments() {
    let readme = "# x\n\n## Install\n\n```sh\ncargo xtask wheel\n\nuv pip install a.whl  # a comment\n```\n\n```sh\nnot this block\n```\n\n## Container\n\n```sh\ncargo xtask image\n```\n";
    let root = tree("readme-lines", &[("README.md", readme)]);
    assert_eq!(
        xtask::readme_lines(&root, "Install"),
        ["cargo xtask wheel", "uv pip install a.whl"]
    );
    assert_eq!(
        xtask::readme_lines(&root, "Container"),
        ["cargo xtask image"]
    );
    assert_eq!(xtask::readme_lines(&root, "Missing"), Vec::<String>::new());
}

#[test]
fn the_readme_install_is_the_one_pip_line_that_the_install_proof_runs() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let install = "\
pip install orbitar
";
    assert_eq!(
        xtask::readme_lines(repo, "Install"),
        install.lines().collect::<Vec<_>>()
    );
}

fn engine() -> Vec<u8> {
    b"\x7fELF\0Jan  1 2026\x0000:00:00\0".to_vec()
}

fn install() -> xtask::Install {
    xtask::Install {
        venv: "/w/venv".into(),
        imported: "/w/venv/lib/python3.12/site-packages/orbitar/__init__.py".into(),
        datasets: "/w/venv/lib/python3.12/site-packages/orbitar_jth".into(),
        version_printed: "orbitar 0.0.1 (commit abc)".into(),
        binary: engine(),
        version_wanted: "orbitar 0.0.1 (commit abc)".into(),
    }
}

#[test]
fn an_install_from_the_venv_with_the_version_and_the_fixed_date_passes() {
    assert_eq!(xtask::install_verdict(&install()), Vec::<String>::new());
}

#[test]
fn an_install_that_imports_elsewhere_prints_another_version_or_embeds_its_build_day_fails() {
    let mut seeded = install();
    seeded.imported = "/checkout/python/orbitar/__init__.py".into();
    assert!(has(
        &xtask::install_verdict(&seeded),
        "imports from /checkout"
    ));
    let mut seeded = install();
    seeded.datasets = String::new();
    assert!(has(
        &xtask::install_verdict(&seeded),
        "finds the jth-2.0 files at ``"
    ));
    let mut seeded = install();
    seeded.version_printed = "orbitar 0.0.1 (commit unknown)".into();
    assert!(has(
        &xtask::install_verdict(&seeded),
        "prints `orbitar 0.0.1 (commit unknown)`"
    ));
    let mut seeded = install();
    seeded.binary = b"Nov 15 2026\0".to_vec();
    assert!(has(
        &xtask::install_verdict(&seeded),
        "embeds `Nov 15 2026`"
    ));
}

type Seed = fn(&mut xtask::Image);

fn image() -> xtask::Image {
    xtask::Image {
        target: "bench".into(),
        uid: "1000".into(),
        held: String::new(),
        paw: String::new(),
        shipped: Vec::new(),
        notices: "/opt/x/orbitar-0.0.1.dist-info/licenses/THIRD-PARTY-NOTICES\n".into(),
        metadata: "RUN /bin/sh -c cargo build\n".into(),
        archived: Some("[package]\nname = \"orbitar\"\n".into()),
        engines: vec![("/opt/orbitar".into(), engine())],
        outcome: "{\"eigenvalues_ha\": [[[0.0]]]}".into(),
        compiled: Some("    Finished `release` profile\n".into()),
        ase: "{\"without\": \"Orbitar: no dataset directory for family jth-2.0: no datasets= given\", \
              \"given\": \"/work/datasets\"}"
            .into(),
        requirements: "orbitar 0.0.1 requires orbitar-jth, which is not installed.\nexit 1\n".into(),
        requires: "ase==3.29.0\nnumpy==2.5.4\nscipy==1.18.1\norbitar-jth==2.0.0\n".into(),
        jth_wanted: "2.0.0".into(),
    }
}

#[test]
fn an_image_that_keeps_every_rule_passes() {
    assert_eq!(xtask::image_verdict(&image()), Vec::<String>::new());
}

#[test]
fn bench_holds_each_pinned_family_file_at_its_pin_and_no_other_paw_xml_file() {
    let (own, pin) = ("/opt/orbitar/datasets/ours-v0/O.xml", "b9ad".repeat(16));
    let other = "0".repeat(64);
    let mut bench = image();
    bench.shipped = vec![(own.into(), pin.clone())];
    let verdict = |paw: String| {
        let mut seeded = image();
        seeded.shipped = bench.shipped.clone();
        seeded.paw = paw;
        xtask::image_verdict(&seeded)
    };
    assert_eq!(verdict(format!("{pin}  {own}\n")), Vec::<String>::new());
    for (paw, needle) in [
        (
            format!("{pin}  {own}\n{other}  /opt/fetched/O.GGA_X_PBE+GGA_C_PBE-paw.xml\n"),
            "image bench: holds a PAW-XML dataset that no pinned family of the checkout owns: \
             /opt/fetched/O.GGA_X_PBE+GGA_C_PBE-paw.xml"
                .to_owned(),
        ),
        (
            format!("{pin}  {own}\n{pin}  /opt/orbitar/datasets/ours-v0/O2.data\n"),
            "image bench: holds a PAW-XML dataset that no pinned family of the checkout owns: \
             /opt/orbitar/datasets/ours-v0/O2.data"
                .to_owned(),
        ),
        (
            format!("{other}  {own}\n"),
            format!("image bench: holds {own} with sha256 {other}, not its family pin {pin}"),
        ),
        (
            String::new(),
            format!("image bench: lacks {own}, which its family pins and its archive holds"),
        ),
    ] {
        assert_eq!(verdict(paw.clone()), [needle], "{paw}");
    }
    let mut runtime = image();
    runtime.target = "runtime".into();
    runtime.paw = format!("{pin}  {own}\n");
    assert_eq!(
        xtask::image_verdict(&runtime),
        [format!(
            "image runtime: holds a PAW-XML dataset that no pinned family of the checkout owns: {own}"
        )]
    );
}

#[test]
fn a_path_the_paw_xml_scan_cannot_inspect_fails_the_image() {
    let mut runtime = image();
    runtime.paw = "error  /opt/x/locked.xml: Permission denied\n".into();
    assert_eq!(
        xtask::image_verdict(&runtime),
        [
            "image bench: the PAW-XML scan could not inspect /opt/x/locked.xml: Permission denied; an unread file is no \
          proof of no dataset"
        ]
    );
    runtime.paw = "something else\n".into();
    assert!(has(
        &xtask::image_verdict(&runtime),
        "the PAW-XML scan printed `something else`, neither a file nor an error"
    ));
}

#[test]
fn the_paw_xml_scan_finds_a_root_within_its_signature_and_names_what_it_cannot_read() {
    use std::os::unix::fs::PermissionsExt;
    let limit = xtask::PAW_SCAN_BYTES;
    assert!(xtask::PAW_SCAN.contains(&format!("LIMIT = {limit}\n")));
    let root = std::env::temp_dir().join(format!("xtask-paw-scan-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let file = |name: &str, body: &[u8]| {
        let path = root.join(name);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, body).unwrap();
        path
    };
    let long = format!(
        "<?xml version=\"1.0\"?>\n<?xml-stylesheet href=\"x\"?>\n<!--{}-->\n<paw_dataset version=\"0.7\">\n",
        "c".repeat(10_000)
    );
    file("a/long.data", long.as_bytes());
    let beyond = format!(
        "<!--{}-->\n<paw_dataset version=\"0.7\">\n",
        "c".repeat(limit)
    );
    file("a/beyond.xml", beyond.as_bytes());
    file(
        "a/quoted.rs",
        b"let tag = \"<paw_dataset version=\\\"0.7\\\">\";\n",
    );
    let locked = file("b/locked.xml", b"<paw_dataset/>");
    let closed = file("c/inside.xml", b"<paw_dataset/>")
        .parent()
        .unwrap()
        .to_owned();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o000)).unwrap();
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o000)).unwrap();
    let unreadable = std::fs::File::open(&locked).is_err();
    let out = std::process::Command::new("python3")
        .args(["-I", "-c", xtask::PAW_SCAN])
        .arg(&root)
        .output()
        .expect("python3 runs the image scan; CI's check job installs it");
    std::fs::set_permissions(&closed, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(out.status.success(), "{out:?}");
    let printed = String::from_utf8(out.stdout).unwrap();
    let sha = crate::harness::hex(&<sha2::Sha256 as sha2::Digest>::digest(long.as_bytes()));
    let at = |name: &str| root.join(name).display().to_string();
    assert!(
        printed.contains(&format!("{sha}  {}\n", at("a/long.data"))),
        "{printed}"
    );
    assert!(
        !printed.contains("beyond.xml") && !printed.contains("quoted.rs"),
        "{printed}"
    );
    if unreadable {
        assert!(
            printed.contains(&format!("error  {}: ", at("b/locked.xml"))),
            "{printed}"
        );
        assert!(
            printed.contains(&format!("error  {}: ", at("c"))),
            "{printed}"
        );
        let mut seeded = image();
        seeded.paw = printed;
        assert!(has(&xtask::image_verdict(&seeded), "could not inspect"));
    }
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_paw_xml_scan_reads_a_prolog_of_many_instructions_in_linear_time() {
    let root = std::env::temp_dir().join(format!("xtask-paw-scan-many-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(&root).unwrap();
    let many = "<?a?><!-- b -->\n".repeat(xtask::PAW_SCAN_BYTES / 16);
    std::fs::write(root.join("many.xml"), &many[..xtask::PAW_SCAN_BYTES]).unwrap();
    let mut child = std::process::Command::new("python3")
        .args(["-I", "-c", xtask::PAW_SCAN])
        .arg(&root)
        .stdout(std::process::Stdio::piped())
        .spawn()
        .expect("python3 runs the image scan; CI's check job installs it");
    let start = std::time::Instant::now();
    while child.try_wait().unwrap().is_none() {
        if start.elapsed() > std::time::Duration::from_secs(30) {
            child.kill().unwrap();
            panic!("the scan of one 64 KiB prolog took over 30 s");
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    let out = child.wait_with_output().unwrap();
    assert!(out.status.success() && out.stdout.is_empty(), "{out:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn an_image_that_breaks_a_rule_fails_with_that_rule() {
    let seeds: [(&str, Seed); 9] = [
        ("runs as uid 0, not 1000", |i| i.uid = "0".into()),
        (
            "holds a dataset, a rival code, or git history: /opt/orbitar/.git",
            |i| i.held = "/opt/orbitar/.git\n".into(),
        ),
        ("holds no THIRD-PARTY-NOTICES", |i| {
            i.notices = String::new()
        }),
        ("names a person, an account", |i| {
            i.metadata = format!("LABEL x=https://github.com/{}/y", "someone")
        }),
        ("names a person, an account", |i| {
            i.archived = Some(format!("contact = \"someone{}example.org\"", '@'))
        }),
        ("embeds `Nov 15 2026`", |i| {
            i.engines[0].1 = b"Nov 15 2026\0".to_vec()
        }),
        ("no outcome", |i| i.outcome = "usage: orbitar run".into()),
        ("printed `{}`, no outcome", |i| i.outcome = "{}".into()),
        ("cargo compiled at run time", |i| {
            i.compiled = Some("   Compiling libmimalloc-sys v0.1.49\n".into())
        }),
    ];
    for (needle, seed) in seeds {
        let mut seeded = image();
        seed(&mut seeded);
        let found = xtask::image_verdict(&seeded);
        assert!(has(&found, needle), "{needle}: {found:?}");
    }
}

#[test]
fn an_image_whose_ase_calculator_finds_data_without_datasets_or_drops_a_given_directory_fails() {
    let mut seeded = image();
    seeded.ase = "{\"without\": \"written\", \"given\": \"/work/datasets\"}".into();
    let found = xtask::image_verdict(&seeded);
    assert!(has(&found, "the image holds no data package"), "{found:?}");
    let mut seeded = image();
    seeded.ase = seeded.ase.replace("/work/datasets", "/opt/x");
    let found = xtask::image_verdict(&seeded);
    assert!(
        has(&found, "with datasets=/work/datasets printed"),
        "{found:?}"
    );
}

#[test]
fn an_image_whose_pip_check_names_anything_but_orbitar_jth_missing_fails() {
    for requirements in [
        "No broken requirements found.\nexit 0\n",
        "orbitar 0.0.1 requires orbitar-jth, which is not installed.\n\
         ase 3.29.0 requires scipy, which is not installed.\nexit 1\n",
        "orbitar 0.0.1 has requirement numpy==2.5.4, but you have numpy 2.0.0.\nexit 1\n",
    ] {
        let mut seeded = image();
        seeded.requirements = requirements.into();
        let found = xtask::image_verdict(&seeded);
        assert!(
            has(
                &found,
                "not orbitar's one missing requirement orbitar-jth alone"
            ),
            "{requirements}: {found:?}"
        );
    }
}

#[test]
fn an_image_whose_orbitar_pins_another_data_package_version_or_none_fails() {
    for requires in [
        "ase==3.29.0\norbitar-jth==1.0.0\n",
        "ase==3.29.0\n",
        "orbitar-jth==2.0.0\norbitar-jth>=2\n",
    ] {
        let mut seeded = image();
        seeded.requires = requires.into();
        let found = xtask::image_verdict(&seeded);
        assert!(
            has(&found, "not `orbitar-jth==2.0.0` alone"),
            "{requires}: {found:?}"
        );
    }
}

#[test]
fn a_wheel_holds_the_top_level_family_tables_and_no_table_of_a_family_directory() {
    let found = checked_beside(
        "family-directory",
        NAME,
        &[("datasets/ours-v0/t.toml", "[element]\n")],
        |_| {},
    );
    assert_eq!(found, Vec::<String>::new());
}

const JTH_NAME: &str = "orbitar_jth-2.0.0-py3-none-any.whl";
const JTH_TABLE: &str = "url = \"https://example.invalid/pseudos\"\n\n[pbe]\n\
     layout = \"JTH-PBE-v2.0/{element}/{element}.GGA_PBE-JTH.xml\"\n\n[pbe.element]\n\
     Si = { sha256 = \"42b1f7186c03492b75c244346650ea638d7a85f2865ebd51356176bc8fa61769\", cutoff_hint_ha = 10.0 }\n\
     C = { sha256 = \"97846ece8c14b93164d1e820ccf589680b75e8054d77420a22e80e0b3f17a53f\" }\n";
const JTH_METADATA: &str = "Metadata-Version: 2.4\nName: orbitar-jth\nVersion: 2.0.0\n\
     Author: The orbitaR developers\nLicense-Expression: GPL-3.0-only\n";
const JTH_POINTS: &str = "[orbitar.datasets]\njth-2.0 = orbitar_jth\n";

fn jth_checked(name: &str, file: &str, change: impl Fn(&mut Vec<(&str, String)>)) -> Vec<String> {
    jth_checked_in(name, file, false, change)
}

fn jth_checked_in(
    name: &str,
    file: &str,
    without_notice: bool,
    change: impl Fn(&mut Vec<(&str, String)>),
) -> Vec<String> {
    let mut files = vec![
        ("datasets/jth-2.0.toml", JTH_TABLE),
        ("packages/orbitar-jth/pyproject.toml", JTH_PROJECT),
        (
            "packages/orbitar-jth/LICENSE",
            "GNU GENERAL PUBLIC LICENSE\n",
        ),
        ("packages/orbitar-jth/NOTICE", "the source and its commit\n"),
        (
            "packages/orbitar-jth/orbitar_jth/__init__.py",
            "\"\"\"doc\"\"\"\n",
        ),
    ];
    if without_notice {
        files.retain(|(p, _)| !p.ends_with("NOTICE"));
    }
    let root = tree(&format!("jth-wheel-{name}"), &files);
    let mut members: Vec<(&str, String)> = vec![
        ("orbitar_jth/", String::new()),
        ("orbitar_jth/__init__.py", "\"\"\"doc\"\"\"\n".into()),
        ("orbitar_jth/jth-2.0.toml", JTH_TABLE.into()),
        (
            "orbitar_jth/JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml",
            "<paw_dataset Si/>\n".into(),
        ),
        (
            "orbitar_jth/JTH-PBE-v2.0/C/C.GGA_PBE-JTH.xml",
            "<paw_dataset C/>\n".into(),
        ),
        ("orbitar_jth-2.0.0.dist-info/METADATA", JTH_METADATA.into()),
        (
            "orbitar_jth-2.0.0.dist-info/entry_points.txt",
            JTH_POINTS.into(),
        ),
        (
            "orbitar_jth-2.0.0.dist-info/WHEEL",
            "Wheel-Version: 1.0\n".into(),
        ),
        ("orbitar_jth-2.0.0.dist-info/RECORD", String::new()),
        (
            "orbitar_jth-2.0.0.dist-info/licenses/LICENSE",
            "GNU GENERAL PUBLIC LICENSE\n".into(),
        ),
        (
            "orbitar_jth-2.0.0.dist-info/licenses/NOTICE",
            "the source and its commit\n".into(),
        ),
    ];
    change(&mut members);
    wheel(&root, file, &members);
    xtask::run(&["wheel", "--check", file], &root, &crate::env(false))
}

fn at(members: &[(&str, String)], member: &str) -> usize {
    members.iter().position(|(p, _)| *p == member).unwrap()
}

#[test]
fn a_data_wheel_with_each_family_file_at_its_pin_the_table_the_license_and_the_notice_passes() {
    assert_eq!(jth_checked("good", JTH_NAME, |_| {}), Vec::<String>::new());
}

const JTH_SI: &str = "orbitar_jth/JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml";

#[test]
fn a_data_wheel_with_a_family_file_off_its_pin_fails() {
    let found = jth_checked("off-pin", JTH_NAME, |m| {
        let i = at(m, JTH_SI);
        m[i].1 = "<paw_dataset other/>\n".into();
    });
    assert!(has(&found, &format!("{JTH_SI} has sha256")), "{found:?}");
}

#[test]
fn a_data_wheel_without_a_family_file_fails() {
    let found = jth_checked("missing", JTH_NAME, |m| m.retain(|(p, _)| *p != JTH_SI));
    assert!(has(&found, &format!("{JTH_SI} is missing")), "{found:?}");
}

#[test]
fn a_data_wheel_with_a_member_that_no_pin_names_fails() {
    for extra in [
        "orbitar_jth/orbitaR-PBE-v0.toml",
        "orbitar_jth/JTH-PBE-v2.0/O/O.GGA_PBE-JTH.xml",
    ] {
        let found = jth_checked("extra", JTH_NAME, |m| m.push((extra, String::new())));
        assert!(has(&found, &format!("{extra} is extra")), "{found:?}");
    }
}

#[test]
fn a_data_wheel_whose_family_table_differs_from_the_checkouts_fails() {
    let table = "orbitar_jth/jth-2.0.toml";
    let found = jth_checked("stale-table", JTH_NAME, |m| {
        let i = at(m, table);
        m[i].1 = "url = \"x\"\n".into();
    });
    assert!(has(&found, &format!("{table} has sha256")), "{found:?}");
}

#[test]
fn a_data_wheel_with_a_pinned_path_twice_fails_though_one_copy_is_at_its_pin() {
    let found = jth_checked("twice", JTH_NAME, |m| {
        let i = at(m, JTH_SI);
        m.insert(i, (JTH_SI, "<paw_dataset other/>\n".into()));
    });
    assert!(
        has(&found, &format!("{JTH_SI} is a member 2 times")),
        "{found:?}"
    );
}

#[test]
fn a_data_wheel_whose_reference_notice_cannot_be_read_fails_by_its_path() {
    let found = jth_checked_in("no-reference", JTH_NAME, true, |_| {});
    assert!(
        has(&found, "packages/orbitar-jth/NOTICE, the reference of"),
        "{found:?}"
    );
}

#[test]
fn a_data_wheel_without_the_license_text_or_the_notice_of_its_source_fails() {
    for member in [
        "orbitar_jth-2.0.0.dist-info/licenses/LICENSE",
        "orbitar_jth-2.0.0.dist-info/licenses/NOTICE",
    ] {
        let found = jth_checked("no-license", JTH_NAME, |m| m.retain(|(p, _)| *p != member));
        assert!(has(&found, &format!("{member} is missing")), "{found:?}");
        let found = jth_checked("other-license", JTH_NAME, |m| {
            let i = at(m, member);
            m[i].1 = "MIT\n".into();
        });
        assert!(has(&found, &format!("{member} has sha256")), "{found:?}");
    }
}

#[test]
fn a_data_wheel_whose_metadata_names_another_license_a_person_or_a_dependency_fails() {
    let metadata = "orbitar_jth-2.0.0.dist-info/METADATA";
    for (from, to, needle) in [
        (
            "GPL-3.0-only",
            "PolyForm-Noncommercial-1.0.0",
            "does not hold `License-Expression: GPL-3.0-only`",
        ),
        (
            "Author: The orbitaR developers",
            "Author: A Person",
            "holds `Author: A Person`",
        ),
        (
            "Version: 2.0.0\n",
            "Version: 2.0.0\nRequires-Dist: orbitar==0.0.1\n",
            "holds `Requires-Dist: orbitar==0.0.1`",
        ),
    ] {
        let found = jth_checked("metadata", JTH_NAME, |m| {
            let i = at(m, metadata);
            m[i].1 = JTH_METADATA.replace(from, to);
        });
        assert!(has(&found, needle), "{needle}: {found:?}");
    }
}

#[test]
fn a_data_wheel_without_the_family_entry_point_or_at_another_version_fails() {
    let points = "orbitar_jth-2.0.0.dist-info/entry_points.txt";
    for text in [
        "",
        "[console_scripts]\njth-2.0 = orbitar_jth\n",
        "[orbitar.datasets]\njth-1.0 = orbitar_jth\n",
    ] {
        let found = jth_checked("points", JTH_NAME, |m| {
            let i = at(m, points);
            m[i].1 = text.into();
        });
        assert!(
            has(&found, "has no `jth-2.0 = orbitar_jth`"),
            "{text}: {found:?}"
        );
    }
    let found = jth_checked("version", "orbitar_jth-1.0.0-py3-none-any.whl", |_| {});
    assert!(
        has(&found, "is not orbitar_jth-2.0.0-py3-none-any.whl"),
        "{found:?}"
    );
}

fn installable(name: &str, data: bool) -> crate::Tree {
    let root = tree(
        &format!("install-{name}"),
        &[
            ("Cargo.toml", CARGO),
            (
                "python/pylock.toml",
                "lock-version = \"1.0\"\ncreated-by = \"test\"\npackages = []\n",
            ),
            (
                "README.md",
                "# x\n\n## Install\n\n```sh\npip install orbitar\n```\n",
            ),
            ("target/wheel/wheel/.keep", ""),
            ("target/wheel/jth-wheel/.keep", ""),
        ],
    );
    let tag = "Wheel-Version: 1.0\nGenerator: test\nRoot-Is-Purelib: true\nTag: py3-none-any\n";
    let ase = "from importlib.metadata import entry_points\nfrom pathlib import Path\nFAMILY = 'jth-2.0'\n\
               def installed(family):\n    for point in entry_points(group='orbitar.datasets', name=family):\n        \
               return Path(point.load().__file__).parent\n";
    let orbitar: Vec<(&str, String)> = vec![
        ("orbitar/__init__.py", String::new()),
        ("orbitar/ase.py", ase.into()),
        ("orbitar/datasets.py", String::new()),
        (
            "orbitar-0.1.0.data/scripts/orbitar",
            "#!/bin/sh\necho 'orbitar 0.1.0 (commit abc)'\n".into(),
        ),
        (
            "orbitar-0.1.0.dist-info/METADATA",
            "Metadata-Version: 2.1\nName: orbitar\nVersion: 0.1.0\nRequires-Dist: orbitar-jth==2.0.0\n".into(),
        ),
        ("orbitar-0.1.0.dist-info/WHEEL", tag.into()),
        ("orbitar-0.1.0.dist-info/RECORD", String::new()),
    ];
    wheel(
        &root,
        "target/wheel/wheel/orbitar-0.1.0-py3-none-any.whl",
        &orbitar,
    );
    if data {
        let jth: Vec<(&str, String)> = vec![
            ("orbitar_jth/__init__.py", String::new()),
            (
                "orbitar_jth-2.0.0.dist-info/METADATA",
                "Metadata-Version: 2.1\nName: orbitar-jth\nVersion: 2.0.0\n".into(),
            ),
            ("orbitar_jth-2.0.0.dist-info/WHEEL", tag.into()),
            (
                "orbitar_jth-2.0.0.dist-info/entry_points.txt",
                JTH_POINTS.into(),
            ),
            ("orbitar_jth-2.0.0.dist-info/RECORD", String::new()),
        ];
        wheel(
            &root,
            "target/wheel/jth-wheel/orbitar_jth-2.0.0-py3-none-any.whl",
            &jth,
        );
    }
    root
}

#[test]
fn pip_install_orbitar_from_the_local_wheels_brings_the_data_package_into_the_venv() {
    let root = installable("both", true);
    let (install, python) = xtask::install(&root, "abc").unwrap();
    assert!(python.starts_with(&install.venv), "{}", python.display());
    let site = "target/wheel/venv/lib/python3.12/site-packages";
    assert!(
        install
            .imported
            .ends_with(&format!("{site}/orbitar/ase.py")),
        "{}",
        install.imported
    );
    assert!(
        install.datasets.ends_with(&format!("{site}/orbitar_jth")),
        "{}",
        install.datasets
    );
    assert_eq!(install.version_printed, "orbitar 0.1.0 (commit abc)");
    assert_eq!(install.version_wanted, install.version_printed);
}

#[test]
fn pip_install_orbitar_without_the_local_data_wheel_fails_and_never_asks_an_index() {
    let root = installable("no-data", false);
    let Err(found) = xtask::install(&root, "abc") else {
        panic!("orbitar installed without orbitar-jth")
    };
    assert!(has(&found, "pip install orbitar exited with"), "{found:?}");
    assert!(has(&found, "orbitar-jth"), "{found:?}");
}

#[test]
fn an_elf_engine_of_another_class_or_byte_order_fails() {
    for (name, class, order) in [("elf32", 1, 1), ("big-endian", 2, 2)] {
        let mut bytes = b"\x7fELF".to_vec();
        bytes.extend([class, order, 1]);
        bytes.resize(16, 0);
        bytes.extend([2, 0, EM_X86_64, 0]);
        assert_eq!(xtask::machine(&bytes), None, "{name}");
        let header = latin(bytes);
        let found = checked(name, NAME, |m| m[3].1.clone_from(&header));
        assert!(
            has(&found, "orbitar-0.1.0.data/scripts/orbitar is no"),
            "{name}: {found:?}"
        );
    }
}

fn files_under(dir: &Path, below: &Path) -> Vec<String> {
    let mut found = Vec::new();
    for entry in std::fs::read_dir(dir.join(below)).unwrap().flatten() {
        let path = below.join(entry.file_name());
        if entry.path().is_dir() {
            found.extend(files_under(dir, &path));
        } else {
            found.push(path.display().to_string());
        }
    }
    found.sort();
    found
}

fn stage_command(root: &Path, rows: &str) -> Command {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let package = root.join("package");
    std::fs::create_dir_all(package.join("orbitar_jth")).unwrap();
    for file in ["pyproject.toml", "stage.py", "orbitar_jth/__init__.py"] {
        std::fs::copy(
            repo.join("packages/orbitar-jth").join(file),
            package.join(file),
        )
        .unwrap();
    }
    std::fs::write(root.join("rows.json"), rows).unwrap();
    let fake = root.join("orbitar");
    std::fs::write(
        &fake,
        format!(
            "#!/bin/sh\necho \"$*\" >> {0}/args\nexec cat {0}/rows.json\n",
            root.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(
        &fake,
        <std::fs::Permissions as std::os::unix::fs::PermissionsExt>::from_mode(0o755),
    )
    .unwrap();
    let run = format!(
        "import runpy, sys; sys.path.insert(0, {:?}); sys.argv = sys.argv[1:]; runpy.run_path(sys.argv[0], run_name='__main__')",
        repo.join("python").display().to_string()
    );
    let mut command = Command::new("python3");
    command
        .args(["-I", "-B", "-c", &run])
        .arg(package.join("stage.py"))
        .arg(root.join("fetched"))
        .arg("--orbitar")
        .arg(&fake)
        .current_dir(root);
    command
}

#[test]
fn stage_puts_the_family_files_and_its_table_alone_into_the_data_package() {
    let root = tree("stage", &[("source/JTH-PBE-v2.0/Si/Si.xml", "si")]);
    let url = format!(
        "file://{}",
        root.join("source/JTH-PBE-v2.0/Si/Si.xml").display()
    );
    let sha: String = sha2::Sha256::digest(b"si")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let rows = format!(
        "[{{\"element\": \"Si\", \"functional\": \"pbe\", \"path\": \"JTH-PBE-v2.0/Si/Si.xml\", \"url\": \"{url}\", \"sha256\": \"{sha}\"}}]"
    );
    let out = stage_command(&root, &rows).output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let module = root.join("package/orbitar_jth");
    let got = files_under(&module, Path::new(""));
    assert_eq!(
        got,
        ["JTH-PBE-v2.0/Si/Si.xml", "__init__.py", "jth-2.0.toml"]
    );
    let args = std::fs::read_to_string(root.join("args")).unwrap();
    assert!(args.contains("--family jth-2.0"), "{args}");
}

#[test]
fn stage_refuses_a_fetch_directory_that_exists() {
    let root = tree("stage-exists", &[("fetched/stale.xml", "old")]);
    let out = stage_command(&root, "[]").output().unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("FileExistsError"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(!root.join("package/orbitar_jth/stale.xml").exists());
}
