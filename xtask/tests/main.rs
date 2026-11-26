use std::{os::unix::fs::PermissionsExt, process::Command};

#[test]
fn help_prints_every_command_the_layout_and_the_tiers() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("help")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success());
    let commands = xtask::commands();
    assert!(commands.len() >= 13);
    for (name, about) in commands {
        assert!(text.contains(&format!("cargo xtask {name}")), "{name}");
        assert!(!about.is_empty(), "{name} has no documentation");
    }
    for needle in ["tests/benchmarks/<N>_<slug>/", "model < xc", "claim"] {
        assert!(text.contains(needle), "{needle}");
    }
}

#[test]
fn a_binary_built_for_another_checkout_refuses_to_run() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .arg("help")
        .env("CARGO_MANIFEST_DIR", "/elsewhere/xtask")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("cargo clean -p xtask"));
}

#[test]
fn check_names_a_stale_cargo_deny() {
    let dir = std::env::temp_dir().join(format!("xtask-fake-cargo-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let fake = dir.join("cargo");
    let script =
        "#!/bin/sh\n[ \"$1 $2\" = \"deny --version\" ] && echo 'cargo-deny 0.19.0'\nexit 0\n";
    std::fs::write(&fake, script).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let check_with_fresh_cargo_stub = || {
        Command::new(env!("CARGO_BIN_EXE_xtask"))
            .arg("check")
            .env("CARGO", &fake)
            .env_remove("CI")
            .output()
            .unwrap()
    };
    const CHECK_ATTEMPTS_WHILE_STUB_IS_BUSY: usize = 5;
    let output = (0..CHECK_ATTEMPTS_WHILE_STUB_IS_BUSY)
        .map(|_| check_with_fresh_cargo_stub())
        .find(|o| !String::from_utf8_lossy(&o.stderr).contains("Text file busy"))
        .unwrap();
    let _ = std::fs::remove_dir_all(&dir);
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        text.contains("`cargo-deny 0.19.0` is installed; check expects cargo-deny 0.20.2"),
        "{text}"
    );
}

#[test]
fn the_claim_tier_reads_a_github_hosted_runner_from_the_process_environment() {
    let output = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["bench", "claim"])
        .env("RUNNER_ENVIRONMENT", "github-hosted")
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout) + String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(
        text.contains(
            "claim: CI on a shared runner never gives speed (RUNNER_ENVIRONMENT is github-hosted)"
        ),
        "{text}"
    );
}
