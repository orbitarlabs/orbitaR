use super::{
    Install, Invocation, JTH, JTH_MODULE, Platform, Program, Wheels, clear, execute, in_venv,
    install_verdict, mac_platform,
};
use crate::{Env, Findings, bench, git};
use std::{
    collections::BTreeMap,
    ffi::{OsStr, OsString},
    fs,
    path::{Path, PathBuf},
};

pub const PYTHON: &str = "3.12.16";

pub const MATURIN: &str = "maturin==1.11.5";

const MATURIN_MACOS: [&str; 2] = [
    "2a596eab137cb3e169b97e89a739515abfa7a8755e2e5f0fc91432ef446f74f4",
    "1c27a2eb47821edf26c75d100b3150b52dca2c1a5f074d7514af06f7a7acb9d5",
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
        interpreter: PathBuf::new(),
        roots: Vec::new(),
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
    interpreter: PathBuf,
    roots: Vec<String>,
    python: PathBuf,
}

impl Host<'_> {
    fn run(&self, plan: Vec<(String, Invocation)>) -> Result<String, String> {
        let mut out = String::new();
        for (what, step) in plan {
            let done = execute(&step, Some(self.env), &what)?;
            out = String::from_utf8_lossy(&done.stdout).trim().to_owned();
        }
        Ok(out)
    }
}

impl Steps for Host<'_> {
    fn build(&mut self) -> Result<Built, String> {
        self.interpreter = PathBuf::from(self.run(python_plan())?);
        let commit = super::commit(self.root)?;
        let (src, dist) = (self.dir.join("src"), self.dir.join("dist"));
        for old in [&src, &dist, &self.dir.join("tools")] {
            clear(old)?;
        }
        tracked(self.root, &src)?;
        let requirement = self.dir.join("maturin.txt");
        fs::write(&requirement, maturin_requirement(self.root)?)
            .map_err(|e| format!("{}: {e}", requirement.display()))?;
        let version = about_pin(self.root)?;
        self.run(tools_plan(
            &self.dir,
            &self.interpreter,
            &requirement,
            &version,
        ))?;
        let path = tool_path(&self.dir)?;
        let config = about_config(self.root, &src, self.platform.target)?;
        self.run(vec![(
            "cargo-about generate".into(),
            about_plan(&config, &path, &src),
        )])?;
        let home = |var: &str, default: &str| {
            std::env::var_os(var).map_or_else(
                || PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(default),
                PathBuf::from,
            )
        };
        let pairs = remaps(&[
            (self.dir.clone(), BUILD_MAP),
            (src.clone(), SOURCE_MAP),
            (home("CARGO_HOME", ".cargo"), CARGO_HOME_MAP),
            (home("RUSTUP_HOME", ".rustup"), RUSTUP_HOME_MAP),
        ]);
        self.roots = (pairs.iter())
            .map(|(from, _)| from.display().to_string())
            .collect();
        let maturin = Build {
            target: self.platform.target,
            src: &src,
            dist: &dist,
            cargo_dir: &self.dir.join("cargo"),
            commit: &commit,
            cargo: &self.env.cargo,
            path: &path,
            pairs: &pairs,
        };
        self.run(vec![("maturin build".into(), maturin_plan(&maturin)?)])?;
        let orbitar = one_wheel(&dist, "orbitar-")?;
        for old in [self.dir.join("fetch"), self.dir.join("fetched")] {
            clear(&old)?;
        }
        self.run(data_plan(&self.dir, &self.interpreter, &orbitar))?;
        let data = one_wheel(&dist, &format!("{JTH_MODULE}-"))?;
        Ok(Built {
            orbitar,
            data,
            commit,
        })
    }

    fn check(&mut self, wheel: &Path) -> Findings {
        let python = self.dir.join("tools/bin/python");
        let scan = super::Scan {
            python: Some(&python),
            build_roots: &self.roots,
        };
        super::check_file_with(self.root, wheel, self.env, &scan)
    }

    fn floor(&mut self, built: &Built) -> Findings {
        let wheels = format!("{}\n{}\n", built.orbitar.display(), built.data.display());
        let input = self.dir.join("floor.txt");
        if let Err(e) = fs::write(&input, wheels) {
            return vec![format!("{}: {e}", input.display())];
        }
        match floor_plan(self.platform.target, &input) {
            Ok(step) => match execute(&step, Some(self.env), "uv pip compile") {
                Ok(_) => Vec::new(),
                Err(e) => vec![format!(
                    "wheel: the wheels do not resolve wheels-only on {} at its macOS floor: {e}",
                    self.platform.target
                )],
            },
            Err(e) => vec![e],
        }
    }

    fn install(&mut self, built: &Built) -> Result<(Install, Installed), Findings> {
        let files = [built.orbitar.clone(), built.data.clone()];
        let wheels = Wheels::Files {
            interpreter: self.interpreter.as_os_str(),
            files: &files,
        };
        let (install, python) =
            super::fill(self.root, &self.dir.join("venv"), &wheels, &built.commit)?;
        let printed = self.run(vec![("importlib.metadata".into(), metadata_plan(&python))]);
        let installed: Installed = serde_json::from_str(&printed.map_err(|e| vec![e])?)
            .map_err(|e| vec![format!("importlib.metadata: {e}")])?;
        self.python = python;
        Ok((install, installed))
    }

    fn readme(&mut self) -> Result<String, String> {
        let work = self.dir.join("ase");
        clear(&work)?;
        fs::create_dir_all(&work).map_err(|e| format!("{}: {e}", work.display()))?;
        let block = super::readme_block(self.root, "ASE");
        self.run(vec![(
            "README ## ASE".into(),
            readme_plan(&self.python, &block, &work),
        )])?;
        Ok(fs::read_to_string(work.join("outcome.json")).unwrap_or_default())
    }

    fn tests(&mut self) -> Findings {
        super::readme_tests(self.root, self.python.clone(), self.env)
    }
}

pub fn python_plan() -> Vec<(String, Invocation)> {
    let install = Invocation::new(Program::Uv).args(["python", "install", PYTHON]);
    let find = Invocation::new(Program::Uv).args([
        "python",
        "find",
        "--no-project",
        "--python-preference",
        "only-managed",
        PYTHON,
    ]);
    vec![
        (format!("uv python install {PYTHON}"), install),
        (format!("uv python find {PYTHON}"), find),
    ]
}

pub fn tools_plan(
    dir: &Path,
    interpreter: &Path,
    requirement: &Path,
    about_version: &str,
) -> Vec<(String, Invocation)> {
    let venv = dir.join("tools");
    let create = Invocation::new(Program::Uv)
        .args(["venv", "--quiet", "--python"])
        .arg(interpreter)
        .arg(&venv);
    let maturin = Invocation::new(Program::Uv)
        .args(["pip", "install", "--quiet", "--no-deps", "--require-hashes"])
        .args(["--only-binary", ":all:", "--python"])
        .arg(venv.join("bin/python"))
        .arg("-r")
        .arg(requirement);
    let about = Invocation::new(Program::Cargo)
        .args(["install", "--quiet", "--locked", "--version", about_version])
        .args(["--features", "cli", "--root"])
        .arg(dir.join("about"))
        .arg("cargo-about");
    vec![
        ("uv venv tools".into(), create),
        ("uv pip install maturin".into(), maturin),
        ("cargo install cargo-about".into(), about),
    ]
}

fn tool_path(dir: &Path) -> Result<OsString, String> {
    let path = std::env::var_os("PATH").unwrap_or_default();
    let dirs = [dir.join("tools/bin"), dir.join("about/bin")];
    std::env::join_paths(dirs.into_iter().chain(std::env::split_paths(&path)))
        .map_err(|e| format!("PATH: {e}"))
}

pub fn about_plan(config: &Path, path: &OsStr, src: &Path) -> Invocation {
    Invocation::new(Program::CargoAbout)
        .env("PATH", path)
        .args(["generate", "--locked", "--fail", "--config"])
        .arg(config)
        .args([
            "--output-file",
            "THIRD-PARTY-NOTICES",
            "container/about.hbs",
        ])
        .dir(src)
}

pub struct Build<'a> {
    pub target: &'a str,
    pub src: &'a Path,
    pub dist: &'a Path,
    pub cargo_dir: &'a Path,
    pub commit: &'a str,
    pub cargo: &'a OsStr,
    pub path: &'a OsStr,
    pub pairs: &'a [(PathBuf, &'a str)],
}

pub fn maturin_plan(build: &Build) -> Result<Invocation, String> {
    let platform = mac_platform(build.target)?;
    let deployment = platform.deployment_target().unwrap_or_default();
    let rust: Vec<String> = (build.pairs.iter())
        .map(|(from, to)| format!("--remap-path-prefix={}={to}", from.display()))
        .chain([NO_DEBUG_MAP.to_owned()])
        .collect();
    Ok(Invocation::new(Program::Maturin)
        .env("PATH", build.path)
        .args([
            "build",
            "--release",
            "--locked",
            "--target",
            build.target,
            "--out",
        ])
        .arg(build.dist)
        .env("ORBITAR_COMMIT", build.commit)
        .env("SOURCE_DATE_EPOCH", bench::SOURCE_DATE_EPOCH)
        .env("CARGO", build.cargo)
        .env("CARGO_TARGET_DIR", build.cargo_dir)
        .env("CARGO_ENCODED_RUSTFLAGS", rust.join("\x1f"))
        .env(
            format!("CFLAGS_{}", build.target.replace('-', "_")),
            c_flags(build.pairs),
        )
        .env("CC_SHELL_ESCAPED_FLAGS", "1")
        .env("MACOSX_DEPLOYMENT_TARGET", deployment)
        .unset("RUSTFLAGS")
        .dir(build.src))
}

pub fn data_plan(dir: &Path, interpreter: &Path, wheel: &Path) -> Vec<(String, Invocation)> {
    let venv = dir.join("fetch");
    let python = venv.join("bin/python");
    let package = dir.join("src").join(JTH);
    let stage = package.join("stage.py").to_string_lossy().into_owned();
    let fetched = dir.join("fetched").to_string_lossy().into_owned();
    let create = Invocation::new(Program::Uv)
        .args(["venv", "--quiet", "--python"])
        .arg(interpreter)
        .arg(&venv);
    let engine = Invocation::new(Program::Uv)
        .args(["pip", "install", "--quiet", "--no-deps", "--python"])
        .arg(&python)
        .arg(wheel);
    let staged = in_venv(&python, &["python", "-I", stage.as_str(), fetched.as_str()]);
    let built = Invocation::new(Program::Uv)
        .args([
            "build",
            "--quiet",
            "--offline",
            "--no-cache",
            "--wheel",
            "--python",
        ])
        .arg(interpreter)
        .arg("--out-dir")
        .arg(dir.join("dist"))
        .dir(&package);
    vec![
        ("uv venv fetch".into(), create),
        ("uv pip install orbitar --no-deps".into(), engine),
        ("packages/orbitar-jth/stage.py".into(), staged),
        ("uv build orbitar-jth".into(), built),
    ]
}

pub fn floor_plan(target: &str, input: &Path) -> Result<Invocation, String> {
    let deployment = mac_platform(target)?
        .deployment_target()
        .unwrap_or_default();
    Ok(Invocation::new(Program::Uv)
        .args([
            "pip",
            "compile",
            "--quiet",
            "--no-header",
            "--only-binary",
            ":all:",
        ])
        .args(["--python-version", PYTHON, "--python-platform", target])
        .arg(input)
        .env("MACOSX_DEPLOYMENT_TARGET", deployment))
}

pub fn metadata_plan(python: &Path) -> Invocation {
    let script = "import json, sys, importlib.metadata as m\nout = {}\nfor n in json.loads(sys.argv[1]):\n    \
        try:\n        d = m.distribution(n)\n    except m.PackageNotFoundError:\n        continue\n    \
        url = json.loads(d.read_text('direct_url.json') or 'null')\n    \
        out[n] = [d.version, url and url.get('url')]\nprint(json.dumps(out))";
    let names = serde_json::to_string(&PAIR).unwrap_or_default();
    in_venv(python, &["python", "-I", "-c", script, &names])
}

pub fn readme_plan(python: &Path, block: &str, work: &Path) -> Invocation {
    in_venv(python, &["python", "-I", "-c", block]).dir(work)
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

pub fn maturin_requirement(root: &Path) -> Result<String, String> {
    let hashes: Vec<String> = (MATURIN_MACOS.iter())
        .map(|h| format!("--hash=sha256:{h}"))
        .collect();
    let pin = maturin_pin(root)?;
    if pin != MATURIN {
        return Err(format!(
            "pyproject.toml pins {pin}, but release/mac.rs holds the macOS wheel hashes of {MATURIN}"
        ));
    }
    Ok(format!("{MATURIN} {}\n", hashes.join(" ")))
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
