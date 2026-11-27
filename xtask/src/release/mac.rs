use super::{Install, JTH, JTH_MODULE, Platform, Wheels, install_verdict, run, venv_run};
use crate::{Env, Findings, bench, git};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

const MATURIN_MACOS: [&str; 2] = [
    "5263dda3f71feef2e4122baf5c4620e4b3710dbb7f2121f85a337182de214369",
    "fe78262c2800c92f67d1ce3c0f6463f958a692cc67bfb572e5dbf5b4b696a8ba",
];

const SOURCE_MAP: &str = "/src";
const BUILD_MAP: &str = "/build";
const CARGO_HOME_MAP: &str = "/opt/cargo";
const RUSTUP_HOME_MAP: &str = "/opt/rustup";

const NO_DEBUG_MAP: &str = "-Clink-arg=-Wl,-S";

const PAIR: [&str; 2] = ["orbitar", "orbitar-jth"];

pub struct Built {
    pub orbitar: PathBuf,
    pub data: PathBuf,
    pub commit: String,
}

pub type Installed = BTreeMap<String, (String, Option<String>)>;

pub trait Steps {
    fn build(&mut self) -> Result<Built, String>;
    fn check(&mut self, wheel: &Path) -> Findings;
    fn floor(&mut self, built: &Built) -> Findings;
    fn install(&mut self, built: &Built) -> Result<(Install, Installed), Findings>;
    fn readme(&mut self) -> Result<String, String>;
    fn tests(&mut self) -> Findings;
}

pub fn proof(os: &str, target: &str, steps: &mut impl Steps) -> Findings {
    if os != "macos" {
        return vec![format!(
            "wheel --target {target}: maturin builds a Mac wheel on a Mac alone, not on {os}"
        )];
    }
    let built = match steps.build() {
        Ok(built) => built,
        Err(e) => return vec![e],
    };
    let mut findings: Findings = [&built.orbitar, &built.data]
        .into_iter()
        .flat_map(|wheel| steps.check(wheel))
        .collect();
    findings.extend(steps.floor(&built));
    if !findings.is_empty() {
        return findings;
    }
    let (install, installed) = match steps.install(&built) {
        Ok(done) => done,
        Err(e) => return e,
    };
    findings.extend(install_verdict(&install));
    findings.extend(pair_verdict(&built, &installed));
    if !findings.is_empty() {
        return findings;
    }
    match steps.readme() {
        Ok(outcome) => findings.extend(ase_verdict(&outcome)),
        Err(e) => findings.push(e),
    }
    findings.extend(steps.tests());
    findings
}

pub fn pair_verdict(built: &Built, installed: &Installed) -> Findings {
    let mut findings = Vec::new();
    for (name, wheel) in PAIR.into_iter().zip([&built.orbitar, &built.data]) {
        let file = wheel.file_name().unwrap_or_default().to_string_lossy();
        let version = file.split('-').nth(1).unwrap_or_default();
        let url = format!("file://{}", wheel.display());
        match installed.get(name) {
            None => findings.push(format!("wheel: the venv holds no {name}")),
            Some((got, from)) => {
                if got != version {
                    findings.push(format!(
                        "wheel: the venv holds {name} {got}, not {version} of {file}"
                    ));
                }
                if from.as_deref().map(decoded).as_deref() != Some(url.as_str()) {
                    findings.push(format!(
                        "wheel: the venv's {name} came from {from:?}, not {url}"
                    ));
                }
            }
        }
    }
    findings
}

fn decoded(url: &str) -> String {
    let bytes = url.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let hex = (bytes[i] == b'%')
            .then(|| url.get(i + 1..i + 3))
            .flatten()
            .and_then(|h| u8::from_str_radix(h, 16).ok());
        match hex {
            Some(b) => {
                out.push(b);
                i += 3;
            }
            None => {
                out.push(bytes[i]);
                i += 1;
            }
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

pub fn ase_verdict(outcome: &str) -> Findings {
    let energy = serde_json::from_str::<serde_json::Value>(outcome)
        .ok()
        .and_then(|o| o["free_energy_ha"].as_f64())
        .filter(|e| e.is_finite());
    match energy {
        Some(e) => {
            println!("wheel: README ## ASE gives free_energy_ha {e}");
            Vec::new()
        }
        None => vec![
            "wheel: README ## ASE left no finite free_energy_ha in its outcome.json".to_owned(),
        ],
    }
}

pub fn remaps(roots: &[(PathBuf, &'static str)]) -> Vec<(PathBuf, &'static str)> {
    let mut pairs: Vec<(PathBuf, &'static str)> = Vec::new();
    for (root, to) in roots {
        let canonical = fs::canonicalize(root).unwrap_or_else(|_| root.clone());
        for spelling in [root.clone(), canonical] {
            if !pairs.iter().any(|(p, _)| *p == spelling) {
                pairs.push((spelling, to));
            }
        }
    }
    pairs.sort_by_key(|(p, _)| p.components().count());
    pairs
}

pub(super) fn prove(root: &Path, platform: &Platform, env: &Env) -> Findings {
    let dir = root.join(format!("target/wheel-{}", platform.target));
    let mut host = Host {
        root,
        dir,
        platform,
        env,
        python: PathBuf::new(),
    };
    let findings = proof(std::env::consts::OS, platform.target, &mut host);
    if findings.is_empty() {
        for wheel in wheels(&host.dir.join("dist"), "").into_iter().flatten() {
            let size = fs::metadata(&wheel).map(|m| m.len()).unwrap_or_default();
            println!("wheel: {} ({size} bytes)", wheel.display());
        }
    }
    findings
}

struct Host<'a> {
    root: &'a Path,
    dir: PathBuf,
    platform: &'a Platform,
    env: &'a Env,
    python: PathBuf,
}

impl Steps for Host<'_> {
    fn build(&mut self) -> Result<Built, String> {
        let (orbitar, commit) = build(self.root, &self.dir, self.platform, self.env)?;
        let data = data_wheel(&self.dir, &orbitar)?;
        Ok(Built {
            orbitar,
            data,
            commit,
        })
    }

    fn check(&mut self, wheel: &Path) -> Findings {
        super::check_file(self.root, wheel, self.env)
    }

    fn floor(&mut self, built: &Built) -> Findings {
        let Some(deployment) = self.platform.deployment_target() else {
            return Vec::new();
        };
        let wheels = format!("{}\n{}\n", built.orbitar.display(), built.data.display());
        let input = self.dir.join("floor.txt");
        if let Err(e) = fs::write(&input, wheels) {
            return vec![format!("{}: {e}", input.display())];
        }
        let resolve = Command::new("uv")
            .args([
                "pip",
                "compile",
                "--quiet",
                "--no-header",
                "--only-binary",
                ":all:",
            ])
            .args([
                "--python-version",
                "3.12",
                "--python-platform",
                self.platform.target,
            ])
            .arg(&input)
            .env("MACOSX_DEPLOYMENT_TARGET", &deployment)
            .output();
        match resolve {
            Ok(out) if out.status.success() => Vec::new(),
            Ok(out) => vec![format!(
                "wheel: the wheels do not resolve wheels-only on {} at macOS {deployment}: {}",
                self.platform.target,
                String::from_utf8_lossy(&out.stderr).trim()
            )],
            Err(e) => vec![format!("uv pip compile: {e}")],
        }
    }

    fn install(&mut self, built: &Built) -> Result<(Install, Installed), Findings> {
        let files = [built.orbitar.clone(), built.data.clone()];
        let venv = self.dir.join("venv");
        let (install, python) =
            super::fill(self.root, &venv, &Wheels::Files(&files), &built.commit)?;
        let script = "import json, sys, importlib.metadata as m\nout = {}\nfor n in json.loads(sys.argv[1]):\n    \
            try:\n        d = m.distribution(n)\n    except m.PackageNotFoundError:\n        continue\n    \
            url = json.loads(d.read_text('direct_url.json') or 'null')\n    \
            out[n] = [d.version, url and url.get('url')]\nprint(json.dumps(out))";
        let names = serde_json::to_string(&PAIR).unwrap_or_default();
        let printed = run(
            &mut venv_run(&python, &["python", "-I", "-c", script, &names]),
            "importlib.metadata",
        )
        .map_err(|e| vec![e])?;
        let installed: Installed = serde_json::from_slice(&printed.stdout)
            .map_err(|e| vec![format!("importlib.metadata: {e}")])?;
        self.python = python;
        Ok((install, installed))
    }

    fn readme(&mut self) -> Result<String, String> {
        let work = self.dir.join("ase");
        clear(&work)?;
        fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
        let block = super::readme_block(self.root, "ASE");
        let ase = ["python", "-I", "-c", block.as_str()];
        run(
            venv_run(&self.python, &ase).current_dir(&work),
            "README ## ASE",
        )?;
        Ok(fs::read_to_string(work.join("outcome.json")).unwrap_or_default())
    }

    fn tests(&mut self) -> Findings {
        super::readme_tests(self.root, self.python.clone(), self.env)
    }
}

fn build(
    root: &Path,
    dir: &Path,
    platform: &Platform,
    env: &Env,
) -> Result<(PathBuf, String), String> {
    let commit = super::commit(root)?;
    let (src, dist, cargo) = (dir.join("src"), dir.join("dist"), dir.join("cargo"));
    for old in [&src, &dist] {
        clear(old)?;
    }
    tracked(root, &src)?;
    let path = tools(root, dir, env)?;
    let config = about_config(root, &src, platform.target)?;
    run(
        Command::new("cargo-about")
            .env("PATH", &path)
            .args(["generate", "--locked", "--fail", "--config"])
            .arg(&config)
            .args([
                "--output-file",
                "THIRD-PARTY-NOTICES",
                "container/about.hbs",
            ])
            .current_dir(&src),
        "cargo-about generate",
    )?;
    let home = |var: &str, default: &str| {
        std::env::var_os(var).map_or_else(
            || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(default),
            PathBuf::from,
        )
    };
    let pairs = remaps(&[
        (dir.to_owned(), BUILD_MAP),
        (src.clone(), SOURCE_MAP),
        (home("CARGO_HOME", ".cargo"), CARGO_HOME_MAP),
        (home("RUSTUP_HOME", ".rustup"), RUSTUP_HOME_MAP),
    ]);
    let rust: Vec<String> = (pairs.iter())
        .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
        .chain([NO_DEBUG_MAP.to_owned()])
        .collect();
    let mut command = Command::new("maturin");
    command
        .env("PATH", &path)
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            platform.target,
            "--out",
        ])
        .arg(&dist)
        .env("ORBITAR_COMMIT", &commit)
        .env("SOURCE_DATE_EPOCH", bench::SOURCE_DATE_EPOCH)
        .env("CARGO", &env.cargo)
        .env("CARGO_TARGET_DIR", &cargo)
        .env("CARGO_ENCODED_RUSTFLAGS", rust.join("\x1f"))
        .env(
            format!("CFLAGS_{}", platform.target.replace('-', "_")),
            c_flags(&pairs),
        )
        .env("CC_SHELL_ESCAPED_FLAGS", "1")
        .env_remove("RUSTFLAGS")
        .current_dir(&src);
    if let Some(deployment) = platform.deployment_target() {
        command.env("MACOSX_DEPLOYMENT_TARGET", deployment);
    }
    run(&mut command, "maturin build")?;
    Ok((one_wheel(&dist, "orbitar-")?, commit))
}

pub fn one_wheel(dist: &Path, prefix: &str) -> Result<PathBuf, String> {
    let wheels = wheels(dist, prefix)?;
    match wheels.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(format!(
            "wheel: {} holds {} {prefix}* wheels, not one",
            dist.display(),
            wheels.len()
        )),
    }
}

fn wheels(dist: &Path, prefix: &str) -> Result<Vec<PathBuf>, String> {
    Ok(
        (fs::read_dir(dist).map_err(|e| format!("{}: {e}", dist.display()))?)
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|x| x == "whl"))
            .filter(|p| (p.file_name()).is_some_and(|n| n.to_string_lossy().starts_with(prefix)))
            .collect(),
    )
}

fn data_wheel(dir: &Path, wheel: &Path) -> Result<PathBuf, String> {
    let (venv, fetched) = (dir.join("fetch"), dir.join("fetched"));
    let python = venv.join("bin/python");
    for old in [&venv, &fetched] {
        clear(old)?;
    }
    run(
        Command::new("uv")
            .args(["venv", "--quiet", "--python", "3.12"])
            .arg(&venv),
        "uv venv fetch",
    )?;
    run(
        Command::new("uv")
            .args(["pip", "install", "--quiet", "--no-deps", "--python"])
            .arg(&python)
            .arg(wheel),
        "uv pip install orbitar --no-deps",
    )?;
    let package = dir.join("src").join(JTH);
    let stage = package.join("stage.py").to_string_lossy().into_owned();
    let mut command = venv_run(&python, &["python", "-I", stage.as_str()]);
    run(command.arg(&fetched), "packages/orbitar-jth/stage.py")?;
    run(
        Command::new("uv")
            .args([
                "build",
                "--quiet",
                "--offline",
                "--no-cache",
                "--wheel",
                "--out-dir",
            ])
            .arg(dir.join("dist"))
            .current_dir(&package),
        "uv build orbitar-jth",
    )?;
    one_wheel(&dir.join("dist"), &format!("{JTH_MODULE}-"))
}

pub fn clear(dir: &Path) -> Result<(), String> {
    match fs::remove_dir_all(dir) {
        Err(e) if e.kind() != std::io::ErrorKind::NotFound => {
            Err(format!("{}: {e}", dir.display()))
        }
        _ => Ok(()),
    }
}

pub fn c_flags(pairs: &[(PathBuf, &str)]) -> String {
    let quoted = |word: String| format!("'{}'", word.replace('\'', r"'\''"));
    (pairs.iter())
        .map(|(from, to)| quoted(format!("-ffile-prefix-map={}={to}", from.display())))
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn tracked(root: &Path, src: &Path) -> Result<(), String> {
    let files = git(root, &["ls-files", "-z"])?;
    for file in files.split('\0').filter(|f| !f.is_empty()) {
        let (from, to) = (root.join(file), src.join(file));
        let fail = |e: std::io::Error| format!("{file}: {e}");
        fs::create_dir_all(to.parent().unwrap_or(src)).map_err(fail)?;
        match fs::read_link(&from) {
            Ok(link) => std::os::unix::fs::symlink(link, &to).map_err(fail)?,
            Err(_) if from.is_file() => fs::copy(&from, &to).map(drop).map_err(fail)?,
            Err(_) => {}
        }
    }
    Ok(())
}

fn tools(root: &Path, dir: &Path, env: &Env) -> Result<OsString, String> {
    let venv = dir.join("tools");
    if !venv.join("bin/python").exists() {
        run(
            Command::new("uv")
                .args(["venv", "--quiet", "--python", "3.12"])
                .arg(&venv),
            "uv venv tools",
        )?;
    }
    let hashes: Vec<String> = (MATURIN_MACOS.iter())
        .map(|h| format!("--hash=sha256:{h}"))
        .collect();
    let requirement = dir.join("maturin.txt");
    let line = format!("{} {}\n", maturin_pin(root)?, hashes.join(" "));
    fs::write(&requirement, line).map_err(|e| format!("{}: {e}", requirement.display()))?;
    run(
        Command::new("uv")
            .args(["pip", "install", "--quiet", "--no-deps", "--require-hashes"])
            .args(["--only-binary", ":all:", "--python"])
            .arg(venv.join("bin/python"))
            .arg("-r")
            .arg(&requirement),
        "uv pip install maturin",
    )?;
    let about = dir.join("about");
    run(
        Command::new(&env.cargo)
            .args(["install", "--quiet", "--locked", "--version"])
            .arg(about_pin(root)?)
            .args(["--features", "cli", "--root"])
            .arg(&about)
            .arg("cargo-about"),
        "cargo install cargo-about",
    )?;
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs = [venv.join("bin"), about.join("bin")];
    std::env::join_paths(dirs.into_iter().chain(std::env::split_paths(&path)))
        .map_err(|e| format!("PATH: {e}"))
}

pub fn maturin_pin(root: &Path) -> Result<String, String> {
    let text = fs::read_to_string(root.join("pyproject.toml"))
        .map_err(|e| format!("pyproject.toml: {e}"))?;
    let manifest: toml::Value =
        toml::from_str(&text).map_err(|e| format!("pyproject.toml: {e}"))?;
    (manifest["build-system"]["requires"]
        .as_array()
        .into_iter()
        .flatten())
    .filter_map(toml::Value::as_str)
    .find(|r| r.starts_with("maturin=="))
    .map(str::to_owned)
    .ok_or_else(|| "pyproject.toml: build-system requires no maturin==<version>".into())
}

pub fn about_pin(root: &Path) -> Result<String, String> {
    let text = fs::read_to_string(root.join(super::DOCKERFILE)).unwrap_or_default();
    (text.lines())
        .filter(|l| l.contains("cargo install") && l.contains("cargo-about"))
        .find_map(|l| l.split("--version ").nth(1)?.split_whitespace().next())
        .map(str::to_owned)
        .ok_or_else(|| {
            format!(
                "{}: no `cargo install --version <version> cargo-about`",
                super::DOCKERFILE
            )
        })
}

pub fn about_config(root: &Path, src: &Path, target: &str) -> Result<PathBuf, String> {
    let path = "container/about.toml";
    let text = fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
    let mut config: toml::Table = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    config.insert("targets".into(), toml::Value::Array(vec![target.into()]));
    let out = src.join("target-about.toml");
    let text = toml::to_string(&config).map_err(|e| format!("{path}: {e}"))?;
    fs::write(&out, text).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(out)
}
