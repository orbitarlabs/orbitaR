#![expect(
    special_module_name,
    reason = "tests/main.rs mirrors src/main.rs and drives the binary"
)]

mod atompaw;
mod bench;
mod case;
mod docs;
mod family;
mod harness;
mod layers;
mod layout;
mod lock;
mod main;
mod manifest;
mod native;
mod node;
mod pr_text;
mod publish;
mod release;
mod reproduce;
mod results;
mod same;
mod science;
mod strain;
mod syntax;
mod tune;
mod version;
mod walk;

use std::{
    fs,
    ops::Deref,
    path::{Path, PathBuf},
    process::Command,
};
use xtask::Env;

pub struct Tree(PathBuf);

impl Deref for Tree {
    type Target = Path;
    fn deref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for Tree {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub const NATIVE_AT: &str = "scratch/local/rows.toml";
pub const NATIVE_WIRED: (&str, &str) = (
    "CLAUDE.local.md",
    "# mine\n<!-- local native: scratch/local/rows.toml -->\n",
);

pub const WHY_EXAMPLE: [&str; 2] = [
    "error_max = \"the worst seen is 4e-13; the margin is for another libm\"",
    "status = \"active: its bound passed in the run that promoted it\"",
];

pub fn tree(name: &str, files: &[(&str, &str)]) -> Tree {
    let root = std::env::temp_dir().join(format!("xtask-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    for (path, body) in files {
        write(&root, path, body);
    }
    Tree(root)
}

pub fn write(root: &Path, path: &str, body: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, body).unwrap();
}

pub fn lock_files(root: &Path) -> std::collections::BTreeMap<String, String> {
    walkdir::WalkDir::new(root.join("benchmarks/locks"))
        .into_iter()
        .flatten()
        .filter(|e| e.file_type().is_file())
        .map(|e| {
            let rel = e.path().strip_prefix(root).unwrap();
            (
                rel.to_string_lossy().into_owned(),
                fs::read_to_string(e.path()).unwrap(),
            )
        })
        .collect()
}

pub fn lock_entry(root: &Path, block: &str, file: &str) -> toml::Table {
    let path = root.join(format!("benchmarks/locks/{block}/{file}.lock"));
    fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{}: {e}", path.display()))
        .parse()
        .unwrap()
}

pub fn has(findings: &[String], needle: &str) -> bool {
    findings.iter().any(|f| f.contains(needle))
}

pub fn cargo_stub_read_by_shell(root: &Path, ci: bool, body: &str) -> Env {
    for name in ["build", "test", "deny", "package", "fmt", "clippy"] {
        write(
            root,
            name,
            &format!("set -- \"$(basename \"$0\")\" \"$@\"\n{body}\n"),
        );
    }
    Env {
        ci,
        cargo: "/bin/sh".into(),
        rustflags: None,
        runner: None,
        rivals: None,
        case_filter: false,
        atompaw: None,
        path: None,
    }
}

pub fn lever(args: &[&str], root: &Path) -> Vec<String> {
    xtask::run(args, root, &env(false))
}

pub fn env(ci: bool) -> Env {
    Env {
        ci,
        runner: None,
        rivals: None,
        case_filter: false,
        atompaw: None,
        ..Env::from_process()
    }
}

pub fn git(root: &Path, args: &[&str]) {
    let ok = Command::new("git")
        .args(["-c", "user.email=x@y", "-c", "user.name=x"])
        .args(args)
        .current_dir(root)
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

#[test]
fn every_check_step_is_locked_and_clippy_denies_warnings() {
    for step in xtask::CHECK {
        if step[0] != "fmt" {
            assert!(step.contains(&"--locked"), "{step:?}");
        }
    }
    let clippy = xtask::CHECK.iter().find(|s| s[0] == "clippy").unwrap();
    assert!(clippy.ends_with(&["--", "-D", "warnings"]));
    assert!(clippy.contains(&"--all-targets") && clippy.contains(&"--workspace"));
    assert!(xtask::CHECK.contains(&&["fmt", "--all", "--", "--check"][..]));
    assert!(
        xtask::CHECK.contains(&&["package", "--locked"][..]),
        "CI packages strictly"
    );
}

#[test]
fn the_check_steps_are_exactly_these() {
    let expected: [&[&str]; 5] = [
        &["fmt", "--all", "--", "--check"],
        &[
            "clippy",
            "--workspace",
            "--all-targets",
            "--all-features",
            "--locked",
            "--",
            "-D",
            "warnings",
        ],
        &["test", "--workspace", "--all-features", "--locked"],
        &["build", "--workspace", "--all-features", "--locked"],
        &["package", "--locked"],
    ];
    assert_eq!(xtask::CHECK, expected);
}

fn logging_cargo(root: &Path, ci: bool) -> Env {
    cargo_stub_read_by_shell(root, ci, "echo \"$@\" >> cargo.log")
}

#[test]
fn ci_packages_strictly_and_a_local_check_allows_a_dirty_tree() {
    for (ci, dirty) in [(true, false), (false, true)] {
        let root = tree(&format!("lib-package-{ci}"), &[]);
        let _ = xtask::run(&["check"], &root, &logging_cargo(&root, ci));
        let log = fs::read_to_string(root.join("cargo.log")).unwrap();
        let package: Vec<&str> = log.lines().filter(|l| l.starts_with("package")).collect();
        assert_eq!(package.len(), 2, "{log}");
        for line in package {
            assert_eq!(line.contains("--allow-dirty"), dirty, "ci = {ci}: {line}");
        }
        assert_eq!(
            log.contains("deny check"),
            !ci,
            "CI runs the pinned deny action: {log}"
        );
    }
}

#[test]
fn a_failing_deny_or_package_list_is_a_finding() {
    let root = tree("lib-tools-fail", &[]);
    let body =
        "case \"$1 $2\" in\n  \"deny --version\" | \"package --list\") exit 1 ;;\nesac\nexit 0";
    let env = cargo_stub_read_by_shell(&root, false, body);
    let findings = xtask::run(&["check"], &root, &env);
    assert!(has(&findings, "cargo-deny is missing"), "{findings:?}");
    assert!(
        has(&findings, "cargo package --list exited with"),
        "{findings:?}"
    );
}

#[test]
fn an_unknown_command_or_a_missing_argument_is_a_finding() {
    let root = tree("lib-unknown", &[]);
    assert!(has(
        &lever(&["gate"], &root),
        "unrecognized subcommand 'gate'"
    ));
    assert!(has(&lever(&["lock", "--reviewer"], &root), "--reviewer"));
}

const ESCAPES: &str = r#"pub fn file() -> std::io::Result<String> {
    std::fs::read_to_string("x")
}
pub fn exists() -> bool {
    std::path::Path::new("x").exists()
}
pub fn environment() -> Option<std::ffi::OsString> {
    std::env::var_os("HOME")
}
pub fn process() -> std::process::Command {
    std::process::Command::new("sh")
}
pub fn network() -> std::io::Result<std::net::TcpStream> {
    std::net::TcpStream::connect("127.0.0.1:1")
}
pub fn dns() -> std::io::Result<std::vec::IntoIter<std::net::SocketAddr>> {
    std::net::ToSocketAddrs::to_socket_addrs("example.org:80")
}
pub fn built() -> &'static str {
    env!("CARGO_PKG_NAME")
}
pub fn embedded() -> &'static str {
    include_str!("lib.rs")
}
#[expect(clippy::disallowed_methods, reason = "the one named exception")]
pub fn input() -> std::io::Result<Vec<u8>> {
    std::fs::read("x")
}
"#;

#[test]
fn clippy_toml_keeps_the_engine_inside_its_one_input() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let copy = |name: &str| fs::read_to_string(repo.join(name)).unwrap();
    let root = tree(
        "lib-clippy",
        &[
            (
                "Cargo.toml",
                "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2024\"\npublish = false\n\n[lib]\npath = \"lib.rs\"\n",
            ),
            ("clippy.toml", &copy("clippy.toml")),
            ("rust-toolchain.toml", &copy("rust-toolchain.toml")),
            ("lib.rs", ESCAPES),
        ],
    );
    let output = Command::new("cargo")
        .args(["clippy", "--quiet", "--offline", "--", "-D", "warnings"])
        .current_dir(&root)
        .env("CARGO_TARGET_DIR", root.join("target"))
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success(), "{text}");
    for denied in [
        "method `std::fs::read_to_string`",
        "method `std::path::Path::exists`",
        "method `std::env::var_os`",
        "type `std::process::Command`",
        "type `std::net::TcpStream`",
        "method `std::net::ToSocketAddrs::to_socket_addrs`",
        "macro `std::env`",
        "macro `std::include_str`",
    ] {
        assert!(
            text.contains(&format!("disallowed {denied}")),
            "{denied}: {text}"
        );
    }
    assert!(!text.contains("method `std::fs::read`"), "{text}");
}

#[test]
fn check_keeps_scratch_out_of_the_package() {
    let manifest = "[package]\nname = \"probe\"\nversion = \"0.0.0\"\nedition = \"2021\"\nlicense = \"MIT\"\ndescription = \"probe\"\n";
    let root = tree(
        "lib-scratch",
        &[
            ("Cargo.toml", manifest),
            ("src/lib.rs", ""),
            ("scratch/notes.rs", ""),
        ],
    );
    assert!(has(
        &lever(&["check"], &root),
        "scratch/notes.rs is private scratch"
    ));
}

pub fn fifo(root: &Path, path: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let _ = fs::remove_file(&path);
    let ok = Command::new("mkfifo")
        .arg(&path)
        .status()
        .unwrap()
        .success();
    assert!(ok, "mkfifo {}", path.display());
}

pub fn within<T: Send + 'static>(seconds: u64, f: impl FnOnce() -> T + Send + 'static) -> T {
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = send.send(f());
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(seconds))
        .unwrap_or_else(|_| panic!("blocked for {seconds} s: a read waited on a non-regular file"))
}

pub fn root_user() -> bool {
    Command::new("id")
        .arg("-u")
        .output()
        .is_ok_and(|out| String::from_utf8_lossy(&out.stdout).trim() == "0")
}
