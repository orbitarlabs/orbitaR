use crate::{
    Env, Findings, git,
    harness::datasets::{self, Family},
    lock::hex,
    walk,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const DOCKERFILE: &str = "container/Dockerfile";
const PYLOCK: &str = "python/pylock.toml";
const CASE_INPUT: &str = "benchmarks/1_free_electron/input.toml";
const PLATFORM: &str = "manylinux_2_28_x86_64";
const PERSONAL: [&str; 6] = [
    "Author-email:",
    "Maintainer:",
    "Maintainer-email:",
    "Home-page:",
    "Project-URL:",
    "Download-URL:",
];
const AUTHOR: &str = "Author: The orbitaR developers";
const DATASET_SUFFIXES: [&str; 3] = ["-JTH.xml", ".UPF", ".upf"];

const LICENSE: &str = "LICENSE";
const NOTICES: &str = "THIRD-PARTY-NOTICES";

const JTH: &str = "packages/orbitar-jth";
const JTH_MODULE: &str = "orbitar_jth";
const JTH_FAMILY: &str = "jth-2.0";
const JTH_LICENSE: &str = "GPL-3.0-only";
const JTH_NOTICE: &str = "NOTICE";
const JTH_INFO: [&str; 4] = ["METADATA", "WHEEL", "RECORD", "entry_points.txt"];

#[derive(Deserialize)]
struct Listing {
    names: Vec<String>,
    metadata: String,
    #[serde(default)]
    families: BTreeMap<String, String>,
    license: Option<String>,
    notices: Option<String>,
    runs: Vec<(String, String)>,
    elf: Vec<String>,
    #[serde(default)]
    sha256: BTreeMap<String, String>,
    #[serde(default)]
    entry_points: Option<String>,
    #[serde(default)]
    paw: Vec<String>,
}

fn version(root: &Path) -> Result<String, String> {
    let text =
        fs::read_to_string(root.join("Cargo.toml")).map_err(|e| format!("Cargo.toml: {e}"))?;
    let manifest: toml::Value = toml::from_str(&text).map_err(|e| format!("Cargo.toml: {e}"))?;
    manifest["package"]["version"]
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| "Cargo.toml: no [package] version".into())
}

fn jth_version(root: &Path) -> Result<String, String> {
    let path = format!("{JTH}/pyproject.toml");
    let text = fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
    let manifest: toml::Value = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
    (manifest.get("project").and_then(|p| p.get("version")))
        .and_then(toml::Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| format!("{path}: no [project] version"))
}

#[derive(Deserialize)]
struct Pylock {
    #[serde(default)]
    packages: Vec<Locked>,
}

#[derive(Deserialize)]
struct Locked {
    name: String,
    version: String,
    #[serde(default)]
    wheels: Vec<LockedWheel>,
}

#[derive(Deserialize)]
struct LockedWheel {
    url: String,
    hashes: BTreeMap<String, String>,
}

fn locked(root: &Path) -> Result<Vec<Locked>, String> {
    let text = fs::read_to_string(root.join(PYLOCK)).map_err(|e| format!("{PYLOCK}: {e}"))?;
    let lock: Pylock = toml::from_str(&text).map_err(|e| format!("{PYLOCK}: {e}"))?;
    Ok(lock.packages)
}

fn pylock(root: &Path) -> Result<BTreeSet<String>, String> {
    Ok((locked(root)?.iter())
        .map(|p| format!("{}=={}", p.name, p.version))
        .collect())
}

pub(crate) fn license(root: &Path) -> String {
    let text = fs::read_to_string(root.join("Cargo.toml")).unwrap_or_default();
    let manifest: toml::Value = toml::from_str(&text).unwrap_or(toml::Value::Integer(0));
    (manifest.get("workspace").and_then(|w| w.get("package")))
        .or_else(|| manifest.get("package"))
        .and_then(|p| p.get("license"))
        .and_then(toml::Value::as_str)
        .unwrap_or("")
        .to_owned()
}

fn linked(root: &Path, env: &Env) -> Result<BTreeSet<String>, String> {
    let tree = [
        "tree",
        "--offline",
        "--package",
        "orbitar",
        "--edges",
        "normal",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--prefix",
        "none",
        "--format",
        "{p}",
    ];
    let out = run(
        Command::new(&env.cargo).args(tree).current_dir(root),
        "cargo tree",
    )?;
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.split(" (").next())
        .filter_map(|p| p.split_once(" v"))
        .filter(|(name, _)| *name != "orbitar")
        .map(|(name, version)| format!("{name} {version}"))
        .collect())
}

fn commit(root: &Path) -> Result<String, String> {
    let head = git(root, &["rev-parse", "HEAD"])?;
    let dirty = !git(root, &["status", "--porcelain", "--untracked-files=no"])?.is_empty();
    Ok(if dirty { format!("{head}-dirty") } else { head })
}

fn run(command: &mut Command, what: &str) -> Result<Output, String> {
    let out = command.output().map_err(|e| format!("{what}: {e}"))?;
    if out.status.success() {
        Ok(out)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        let tail: Vec<&str> = stderr.lines().rev().take(20).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        Err(format!(
            "{what} exited with {}:\n{}",
            out.status,
            tail.join("\n")
        ))
    }
}

fn stdout(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

fn build(root: &Path, target: &str, commit: &str, extra: &[&str]) -> Result<(), String> {
    let mut command = Command::new("docker");
    command
        .args(["build", "--file", DOCKERFILE, "--target", target])
        .args(["--build-arg", &format!("COMMIT={commit}")])
        .args(extra)
        .current_dir(root);
    if let Ok(proxy) = std::env::var("HTTPS_PROXY") {
        command.args([
            "--network",
            "host",
            "--build-arg",
            &format!("HTTPS_PROXY={proxy}"),
        ]);
    }
    if let Some(ca) = std::env::var_os("SSL_CERT_FILE") {
        let mut secret = OsString::from("id=ca,src=");
        secret.push(ca);
        command.arg("--secret").arg(secret);
    }
    command.arg(".");
    run(&mut command, &format!("docker build --target {target}")).map(|_| ())
}

fn contents(root: &Path, file: &str, listing: &Listing, crates: &BTreeSet<String>) -> Findings {
    let mut findings = Vec::new();
    let version = match version(root) {
        Ok(v) => v,
        Err(e) => return vec![e],
    };
    let want = format!("orbitar-{version}-py3-none-{PLATFORM}.whl");
    if file != want {
        findings.push(format!("wheel: {file} is not {want}"));
    }
    let names: BTreeSet<&str> = listing.names.iter().map(String::as_str).collect();
    let script = format!("orbitar-{version}.data/scripts/orbitar");
    for member in [
        "orbitar/__init__.py",
        "orbitar/ase.py",
        "orbitar/datasets.py",
        script.as_str(),
    ] {
        if !names.contains(member) {
            findings.push(format!("wheel: {member} is missing"));
        }
    }
    let tables: BTreeMap<String, String> = (walk::files(root, "datasets").into_iter())
        .filter_map(|file| {
            let name = file.strip_prefix("datasets/")?;
            (name.ends_with(".toml") && !name.contains('/'))
                .then(|| (format!("orbitar/families/{name}"), walk::read(root, &file)))
        })
        .collect();
    if listing.families != tables {
        findings.push(
            "wheel: orbitar/families/ is not datasets/*.toml byte for byte (python/orbitar/families links to it)"
                .to_owned(),
        );
    }
    findings.extend(duplicates(file, &listing.names));
    for name in &listing.paw {
        findings.push(format!(
            "wheel: {name} opens with a PAW-XML root; datasets never enter the wheel"
        ));
    }
    for name in &names {
        if DATASET_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            findings.push(format!(
                "wheel: {name} is a dataset; datasets never enter the wheel"
            ));
        }
    }
    let fields: Vec<&str> = listing
        .metadata
        .lines()
        .take_while(|l| !l.is_empty())
        .collect();
    if !fields.contains(&format!("Version: {version}").as_str()) {
        findings.push(format!(
            "wheel: METADATA does not hold Version: {version} (Cargo.toml)"
        ));
    }
    for field in &fields {
        if PERSONAL.iter().any(|p| field.starts_with(p)) {
            findings.push(format!(
                "wheel: METADATA holds `{field}`; no person, address, or URL"
            ));
        }
        if field.starts_with("Author:") && *field != AUTHOR {
            findings.push(format!("wheel: METADATA holds `{field}`, not `{AUTHOR}`"));
        }
    }
    let expression = license(root);
    if !fields.contains(&format!("License-Expression: {expression}").as_str()) {
        findings.push(format!(
            "wheel: METADATA does not hold License-Expression: {expression} (Cargo.toml)"
        ));
    }
    let text = fs::read_to_string(root.join(LICENSE)).unwrap_or_default();
    if listing.license.as_deref() != Some(text.as_str()) {
        findings.push(format!(
            "wheel: .dist-info/licenses/{LICENSE} is not {LICENSE} byte for byte"
        ));
    }
    match &listing.notices {
        None => findings.push(format!("wheel: .dist-info/licenses/{NOTICES} is missing")),
        Some(notices) => {
            let named: BTreeSet<&str> = notices
                .lines()
                .filter_map(|l| l.strip_prefix("used by: "))
                .collect();
            for krate in crates.iter().filter(|c| !named.contains(c.as_str())) {
                findings.push(format!(
                    "wheel: {NOTICES} holds no notice for {krate}, which the binary links"
                ));
            }
        }
    }
    let mut requires = BTreeSet::new();
    for field in &fields {
        if let Some(extra) = field.strip_prefix("Provides-Extra: ") {
            findings.push(format!(
                "wheel: METADATA provides the extra `{extra}`; the wheel has none"
            ));
        }
    }
    for dist in fields
        .iter()
        .filter_map(|f| f.strip_prefix("Requires-Dist: "))
    {
        let dist = dist.replace([' ', '"'], "").replace('\'', "");
        if dist.contains(';') {
            findings.push(format!(
                "wheel: Requires-Dist {dist} has a marker; the wheel has no extra"
            ));
        } else {
            requires.insert(dist);
        }
    }
    match (pylock(root), jth_version(root)) {
        (Ok(mut pinned), Ok(jth)) => {
            let data = format!("orbitar-jth=={jth}");
            pinned.insert(data.clone());
            let unpinned: BTreeSet<&String> = requires.difference(&pinned).collect();
            if !unpinned.is_empty() || !requires.contains(&data) {
                findings.push(format!(
                    "wheel: Requires-Dist {requires:?} differs from {PYLOCK} and {JTH}: {unpinned:?} has no pin, {data} is required"
                ));
            }
        }
        (Err(e), _) | (_, Err(e)) => findings.push(e),
    }
    for (member, runs) in &listing.runs {
        let what = format!("wheel {member}");
        let binary = member.contains(".data/scripts/") && listing.elf.contains(member);
        findings.extend(named_in(&what, runs, binary));
        findings.extend(homes(
            &what,
            runs,
            "build the wheel with `cargo xtask wheel`",
        ));
    }
    findings
}

macro_rules! paw_root {
    () => {
        r#"rb"\A(?:\xef\xbb\xbf)?(?:\s|<\?(?:[^?]|\?(?!>))*\?>|<!--(?:[^-]|-(?!->))*-->)*<paw_dataset[\s>/]""#
    };
}

fn listing(wheel: &Path) -> Result<Listing, String> {
    let script = concat!(
        "import hashlib, json, re, sys, zipfile\n\
        paw = re.compile(",
        paw_root!(),
        ")\n\
        z = zipfile.ZipFile(sys.argv[1])\n\
        names = z.namelist()\n\
        read = lambda end: next((z.read(n).decode() for n in names if n.endswith(end)), None)\n\
        runs = lambda n: '\\n'.join(r.decode() for r in re.findall(rb'[ -~]{4,}', z.read(n)))\n\
        print(json.dumps({'names': names, 'metadata': read('.dist-info/METADATA') or '', \
        'families': {n: z.read(n).decode() for n in names if n.startswith('orbitar/families/')}, 'license': read('.dist-info/licenses/LICENSE'), \
        'notices': read('.dist-info/licenses/THIRD-PARTY-NOTICES'), 'runs': [[n, runs(n)] for n in names if not n.startswith('orbitar_jth/')], \
        'elf': [n for n in names if z.read(n)[:4] == b'\\x7fELF'], \
        'sha256': {n: hashlib.sha256(z.read(n)).hexdigest() for n in names}, \
        'entry_points': read('.dist-info/entry_points.txt'), \
        'paw': [n for n in names if paw.match(z.read(n))]}))"
    );
    let out = run(
        Command::new("python3")
            .args(["-I", "-c", script])
            .arg(wheel),
        "python3 zipfile",
    )?;
    serde_json::from_slice(&out.stdout).map_err(|e| format!("{}: {e}", wheel.display()))
}

fn check_file(root: &Path, wheel: &Path, env: &Env) -> Findings {
    let file = wheel
        .file_name()
        .map(|f| f.to_string_lossy().into_owned())
        .unwrap_or_default();
    if file.starts_with(&format!("{JTH_MODULE}-")) {
        return match listing(wheel) {
            Ok(listing) => jth_contents(root, &file, &listing),
            Err(e) => vec![e],
        };
    }
    match (listing(wheel), linked(root, env)) {
        (Ok(listing), Ok(crates)) => contents(root, &file, &listing, &crates),
        (Err(e), _) | (_, Err(e)) => vec![e],
    }
}

fn duplicates(file: &str, names: &[String]) -> Findings {
    let mut seen = BTreeMap::new();
    for name in names {
        *seen.entry(name.as_str()).or_insert(0) += 1;
    }
    (seen.into_iter())
        .filter(|(_, count)| *count > 1)
        .map(|(name, count)| {
            format!("wheel {file}: {name} is a member {count} times; a wheel holds each path once")
        })
        .collect()
}

fn jth_contents(root: &Path, file: &str, listing: &Listing) -> Findings {
    let version = match jth_version(root) {
        Ok(v) => v,
        Err(e) => return vec![e],
    };
    let mut findings = duplicates(file, &listing.names);
    let want = format!("{JTH_MODULE}-{version}-py3-none-any.whl");
    if file != want {
        findings.push(format!("wheel: {file} is not {want}"));
    }
    let info = format!("{JTH_MODULE}-{version}.dist-info");
    let mut pinned = BTreeMap::new();
    match Family::read(root, JTH_FAMILY) {
        Ok(family) => {
            for (path, sha) in family.pins() {
                pinned.insert(format!("{JTH_MODULE}/{path}"), sha);
            }
        }
        Err(e) => findings.push(e),
    }
    for (member, source) in [
        (
            format!("{JTH_MODULE}/{JTH_FAMILY}.toml"),
            format!("datasets/{JTH_FAMILY}.toml"),
        ),
        (
            format!("{JTH_MODULE}/__init__.py"),
            format!("{JTH}/{JTH_MODULE}/__init__.py"),
        ),
        (
            format!("{info}/licenses/{LICENSE}"),
            format!("{JTH}/{LICENSE}"),
        ),
        (
            format!("{info}/licenses/{JTH_NOTICE}"),
            format!("{JTH}/{JTH_NOTICE}"),
        ),
    ] {
        match fs::read(root.join(&source)) {
            Ok(bytes) => {
                pinned.insert(member, hex(&Sha256::digest(&bytes)));
            }
            Err(e) => findings.push(format!(
                "wheel {file}: {source}, the reference of {member}, cannot be read: {e}"
            )),
        }
    }
    for (member, sha) in &pinned {
        match listing.sha256.get(member) {
            None => findings.push(format!("wheel {file}: {member} is missing")),
            Some(got) if got != sha => findings.push(format!(
                "wheel {file}: {member} has sha256 {got}, not its pin {sha}"
            )),
            Some(_) => {}
        }
    }
    let generated: BTreeSet<String> = JTH_INFO.iter().map(|n| format!("{info}/{n}")).collect();
    for name in &listing.names {
        if !name.ends_with('/') && !pinned.contains_key(name) && !generated.contains(name) {
            findings.push(format!(
                "wheel {file}: {name} is extra; the data wheel holds the {JTH_FAMILY} files and their table alone"
            ));
        }
    }
    let fields: Vec<&str> = (listing.metadata.lines())
        .take_while(|l| !l.is_empty())
        .collect();
    for field in [
        "Name: orbitar-jth".to_owned(),
        format!("Version: {version}"),
        format!("License-Expression: {JTH_LICENSE}"),
        AUTHOR.to_owned(),
    ] {
        if !fields.contains(&field.as_str()) {
            findings.push(format!("wheel {file}: METADATA does not hold `{field}`"));
        }
    }
    for field in &fields {
        let personal = PERSONAL.iter().any(|p| field.starts_with(p));
        let author = field.starts_with("Author:") && *field != AUTHOR;
        if personal || author || field.starts_with("Requires-Dist:") {
            findings.push(format!(
                "wheel {file}: METADATA holds `{field}`; one author, no person, URL, or dependency"
            ));
        }
    }
    let entry = format!("{JTH_FAMILY} = {JTH_MODULE}");
    let points = listing.entry_points.as_deref().unwrap_or_default();
    let group = points
        .split("[orbitar.datasets]")
        .nth(1)
        .unwrap_or_default();
    let section = group.split('[').next().unwrap_or_default();
    if !section.lines().any(|l| l.trim() == entry) {
        findings.push(format!(
            "wheel {file}: entry_points.txt has no `{entry}` under [orbitar.datasets] (python/orbitar/ase.py GROUP)"
        ));
    }
    for (member, runs) in &listing.runs {
        findings.extend(named_in(&format!("wheel {member}"), runs, false));
    }
    findings
}

pub(crate) fn wheel(root: &Path, check: Option<&Path>, env: &Env) -> Findings {
    match check {
        Some(file) => check_file(root, &root.join(file), env),
        None => prove_wheel(root, env),
    }
}

const WHEELS: [&str; 2] = ["wheel", "jth-wheel"];

fn prove_wheel(root: &Path, env: &Env) -> Findings {
    let commit = match commit(root) {
        Ok(c) => c,
        Err(e) => return vec![e],
    };
    let mut wheels = Vec::new();
    for target in WHEELS {
        match build_wheel(root, target, &commit) {
            Ok(wheel) => wheels.push(wheel),
            Err(e) => return vec![e],
        }
    }
    let mut findings: Findings = (wheels.iter())
        .flat_map(|wheel| check_file(root, wheel, env))
        .collect();
    if findings.is_empty() {
        match install(root, &commit) {
            Ok((install, python)) => {
                findings.extend(install_verdict(&install));
                findings.extend(readme_tests(root, python, env));
            }
            Err(e) => findings.extend(e),
        }
    }
    if findings.is_empty() {
        for wheel in &wheels {
            let size = fs::metadata(wheel).map(|m| m.len()).unwrap_or_default();
            println!("wheel: {} ({size} bytes)", wheel.display());
        }
    }
    findings
}

fn build_wheel(root: &Path, target: &str, commit: &str) -> Result<PathBuf, String> {
    let out = root.join("target/wheel").join(target);
    let _ = fs::remove_dir_all(&out);
    let dest = format!("type=local,dest={}", out.display());
    build(root, target, commit, &["--output", &dest])?;
    let wheels: Vec<PathBuf> = (fs::read_dir(&out)
        .map_err(|e| format!("{}: {e}", out.display()))?)
    .flatten()
    .map(|e| e.path())
    .filter(|p| p.extension().is_some_and(|x| x == "whl"))
    .collect();
    match wheels.as_slice() {
        [one] => Ok(one.clone()),
        _ => Err(format!(
            "wheel: {} holds {} wheels, not one",
            out.display(),
            wheels.len()
        )),
    }
}

pub struct Install {
    pub venv: String,
    pub imported: String,
    pub datasets: String,
    pub version_printed: String,
    pub binary: Vec<u8>,
    pub version_wanted: String,
}

pub fn install_verdict(install: &Install) -> Findings {
    let mut findings = Vec::new();
    if !install.imported.starts_with(&install.venv) {
        findings.push(format!(
            "wheel: orbitar imports from {}, not the venv {}",
            install.imported, install.venv
        ));
    }
    if !install.datasets.starts_with(&install.venv) {
        findings.push(format!(
            "wheel: orbitar.ase finds the {JTH_FAMILY} files at `{}`, not in the venv {}",
            install.datasets, install.venv
        ));
    }
    if install.version_printed != install.version_wanted {
        findings.push(format!(
            "wheel: `orbitar --version` prints `{}`, not `{}`",
            install.version_printed, install.version_wanted
        ));
    }
    findings.extend(crate::reproduce::stamped(
        "the wheel's orbitar",
        &install.binary,
    ));
    findings
}

pub fn install(root: &Path, commit: &str) -> Result<(Install, PathBuf), Findings> {
    let one = |e: String| vec![e];
    let venv = root.join("target/wheel/venv");
    let python = venv.join("bin/python");
    run(
        Command::new("uv")
            .args(["venv", "--quiet", "--clear", "--python", "3.12"])
            .arg(&venv),
        "uv venv",
    )
    .map_err(one)?;
    let uv_install = |args: &[&str]| {
        let mut command = Command::new("uv");
        command
            .args(["pip", "install", "--quiet", "--no-cache", "--python"])
            .arg(&python)
            .args(args)
            .current_dir(root);
        command
    };
    wheelhouse(root).map_err(one)?;
    let mut local = vec!["--no-index", "--no-build"];
    let dirs: Vec<String> = (std::iter::once(HOUSE))
        .chain(WHEELS)
        .map(|t| format!("target/wheel/{t}"))
        .collect();
    for dir in &dirs {
        local.extend(["--find-links", dir.as_str()]);
    }
    for line in readme_lines(root, "Install") {
        match line.strip_prefix("pip install ") {
            Some(args) => {
                let words: Vec<&str> = (local.iter().copied())
                    .chain(args.split_whitespace())
                    .collect();
                run(&mut uv_install(&words), &line)
            }
            None => run(
                venv_run(&python, &["sh", "-c", line.as_str()]).current_dir(root),
                &line,
            ),
        }
        .map_err(one)?;
    }
    let version_printed = run(
        &mut venv_run(&python, &["orbitar", "--version"]),
        "orbitar --version",
    )
    .map(|o| stdout(&o))
    .map_err(one)?;
    let import = "import orbitar.ase as a, orbitar.datasets; print(a.__file__); print(a.installed(a.FAMILY))";
    let printed = run(
        &mut venv_run(&python, &["python", "-I", "-c", import]),
        "import orbitar",
    )
    .map(|o| stdout(&o))
    .map_err(one)?;
    let (imported, datasets) = printed.split_once('\n').unwrap_or((&printed, ""));
    let bin = venv.join("bin/orbitar");
    let binary = fs::read(&bin).map_err(|e| one(format!("{}: {e}", bin.display())))?;
    let version_wanted = format!("orbitar {} (commit {commit})", version(root).map_err(one)?);
    let install = Install {
        venv: venv.to_string_lossy().into_owned(),
        imported: imported.to_owned(),
        datasets: datasets.to_owned(),
        version_printed,
        binary,
        version_wanted,
    };
    Ok((install, python))
}

const HOUSE: &str = "house";

fn wheelhouse(root: &Path) -> Result<(), String> {
    let house = root.join("target/wheel").join(HOUSE);
    fs::create_dir_all(&house).map_err(|e| format!("{}: {e}", house.display()))?;
    let mut kept = BTreeSet::new();
    for wheel in locked(root)?.iter().flat_map(|p| &p.wheels) {
        let file = (wheel.url.rsplit('/').next())
            .filter(|f| f.ends_with(".whl"))
            .ok_or_else(|| format!("{PYLOCK}: {} names no wheel file", wheel.url))?;
        let sha = (wheel.hashes.get("sha256"))
            .ok_or_else(|| format!("{PYLOCK}: {file} has no sha256"))?;
        let local = house.join(file);
        if datasets::cached(&local, sha)?.is_none() {
            datasets::download(&local, &wheel.url, sha, PYLOCK)?;
        }
        kept.insert(OsString::from(file));
    }
    let entries = fs::read_dir(&house).map_err(|e| format!("{}: {e}", house.display()))?;
    for entry in entries.flatten() {
        if !kept.contains(&entry.file_name()) {
            fs::remove_file(entry.path())
                .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        }
    }
    Ok(())
}

fn readme_tests(root: &Path, python: PathBuf, env: &Env) -> Findings {
    let vars: [(&str, OsString); 1] = [("ORBITAR_PYTHON", python.into_os_string())];
    let test = [
        "test",
        "--locked",
        "--package",
        "orbitar",
        "--test",
        "unit",
        "--",
        "main::the_readme_ase",
    ];
    crate::cargo(env, root, &test, &vars)
}

pub fn readme_lines(root: &Path, heading: &str) -> Vec<String> {
    let text = fs::read_to_string(root.join("README.md")).unwrap_or_default();
    let section = text
        .split(&format!("\n## {heading}\n"))
        .nth(1)
        .unwrap_or("");
    let block = section.split("```").nth(1).unwrap_or("");
    (block.lines().skip(1))
        .map(|l| l.split_once(" #").map_or(l, |(code, _)| code).trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

fn venv_run(python: &Path, args: &[&str]) -> Command {
    let mut command = Command::new("uv");
    command
        .args(["run", "--no-project", "--quiet", "--python"])
        .arg(python)
        .arg("--")
        .args(args);
    command
}

const ACCOUNT_HOSTS: [&str; 10] = [
    "github.com",
    "gist.github.com",
    "raw.githubusercontent.com",
    "gitlab.com",
    "bitbucket.org",
    "codeberg.org",
    "hub.docker.com",
    "docker.io",
    "ghcr.io",
    "quay.io",
];

const WORKSPACE_HOSTS: [&str; 8] = [
    "claude.ai",
    "notion.so",
    "notion.site",
    "notion.com",
    "docs.google.com",
    "drive.google.com",
    "slack.com",
    "atlassian.net",
];

const SOURCES: [&str; 6] = [
    "github.com/rust-lang/crates.io-index",
    "raw.githubusercontent.com/abinit/paw_jth_datasets/",
    "raw.githubusercontent.com/dalcorso/pslibrary/",
    "github.com/molmod/deltacodesdft/",
    "quay.io/pypa/manylinux_2_28_x86_64",
    "ghcr.io/astral-sh/uv",
];

fn email(word: &str) -> bool {
    word.rsplit_once('@').is_some_and(|(user, domain)| {
        let user = user.rsplit([':', '/']).next().unwrap_or(user);
        !user.is_empty()
            && user
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "._%+-".contains(c))
            && is_domain(domain)
    })
}

fn is_domain(domain: &str) -> bool {
    let tld = domain.rsplit('.').next().unwrap_or("");
    let labels: Vec<&str> = domain.split('.').collect();
    let digits = |l: &str| !l.is_empty() && l.chars().all(|c| c.is_ascii_digit());
    let decimal = digits(labels[0]) && labels.get(1).is_some_and(|l| digits(l));
    domain.contains('.')
        && !decimal
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-".contains(c))
        && tld.len() >= 2
        && tld.chars().all(|c| c.is_ascii_alphabetic())
}

fn account(word: &str, binary: bool) -> bool {
    let lower = word.to_lowercase();
    let bare = lower
        .split_once("://")
        .map_or(lower.as_str(), |(_, rest)| rest);
    if SOURCES.iter().any(|s| bare.starts_with(s)) {
        return false;
    }
    let (authority, path) = bare.split_once('/').unwrap_or((bare, ""));
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let host = host
        .split(':')
        .next()
        .unwrap_or(host)
        .trim_start_matches("www.");
    let userinfo = lower.contains("://") && authority.contains('@');
    let scp = !lower.contains("://")
        && authority.split_once('@').is_some_and(|(user, rest)| {
            !user.is_empty()
                && !rest.starts_with("sha256:")
                && rest
                    .split_once(':')
                    .is_some_and(|(h, p)| !p.is_empty() && (!binary || is_domain(h)))
        });
    let hosted = ACCOUNT_HOSTS.contains(&host) && !path.split('/').next().unwrap_or("").is_empty();
    let workspace = WORKSPACE_HOSTS
        .iter()
        .any(|w| host == *w || host.ends_with(&format!(".{w}")))
        && !path.is_empty();
    lower.contains(concat!("mail", "to:")) || userinfo || scp || hosted || workspace
}

fn words(text: &str) -> impl Iterator<Item = &str> {
    let split = |c: char| c.is_whitespace() || "\"'<>()[]{},;=`\\".contains(c);
    text.split(split).filter(|w| !w.is_empty())
}

pub(crate) const MAINTAINER: (&str, &str) = (
    "orbitar-admin",
    "360476282+orbitar-admin@users.noreply.github.com",
);
pub(crate) const GITHUB: (&str, &str) = ("GitHub", "noreply@github.com");
pub(crate) const NOREPLY: [(&str, &str); 2] = [MAINTAINER, GITHUB];

pub(crate) fn noreply(word: &str) -> bool {
    (NOREPLY.iter()).any(|(_, address)| word.eq_ignore_ascii_case(address))
}

fn identities(text: &str, binary: bool) -> Vec<&str> {
    (words(text).filter(|w| !noreply(w)))
        .filter(|w| email(w) || account(w, binary))
        .collect()
}

pub(crate) fn named(what: &str, text: &str) -> Findings {
    named_in(what, text, false)
}

fn named_in(what: &str, text: &str, binary: bool) -> Findings {
    let words: BTreeSet<&str> = identities(text, binary).into_iter().collect();
    words
        .into_iter()
        .map(|w| format!("{what}: `{w}` names a person, an account, or a host's account path"))
        .collect()
}

const IMAGE_USER: &str = "orbitar";

fn home(word: &str) -> bool {
    ["/home/", "/Users/"].iter().any(|prefix| {
        (word.match_indices(prefix)).any(|(i, _)| {
            let user = (word[i + prefix.len()..].split('/').next()).unwrap_or_default();
            !user.is_empty() && user != IMAGE_USER
        })
    })
}

pub(crate) fn homes(what: &str, text: &str, fix: &str) -> Findings {
    let held: BTreeSet<&str> = words(text).filter(|w| home(w)).collect();
    (held.into_iter())
        .map(|w| format!("{what}: `{w}` holds a home directory; {fix}"))
        .collect()
}

pub(crate) fn image(root: &Path, scan: Option<&Path>) -> Findings {
    if let Some(file) = scan {
        return match fs::read_to_string(root.join(file)) {
            Ok(text) => named(&format!("image {}", file.display()), &text),
            Err(e) => vec![format!("{}: {e}", file.display())],
        };
    }
    match baked(commit(root)) {
        Ok(commit) => prove_images(root, &commit),
        Err(findings) => findings,
    }
}

pub fn baked(commit: Result<String, String>) -> Result<String, Findings> {
    match commit {
        Ok(c) if c.ends_with("-dirty") => Err(vec![format!(
            "image: the tree differs from {c}; bench bakes one commit, so commit first"
        )]),
        Ok(c) => Ok(c),
        Err(e) => Err(vec![e]),
    }
}

fn prove_images(root: &Path, commit: &str) -> Findings {
    let mut findings = Vec::new();
    for target in ["runtime", "bench"] {
        let tag = format!("orbitar-local:{target}");
        let observed = build(root, target, commit, &["--tag", &tag])
            .map_err(|e| vec![e])
            .and_then(|()| observe_image(root, target, &tag));
        match observed {
            Ok(image) => findings.extend(image_verdict(&image)),
            Err(e) => findings.extend(e),
        }
    }
    findings
}

pub struct Image {
    pub target: String,
    pub uid: String,
    pub held: String,
    pub paw: String,
    pub shipped: Vec<(String, String)>,
    pub notices: String,
    pub metadata: String,
    pub archived: Option<String>,
    pub engines: Vec<(String, Vec<u8>)>,
    pub outcome: String,
    pub compiled: Option<String>,
    pub ase: String,
    pub requirements: String,
    pub requires: String,
    pub jth_wanted: String,
}

const REQUIRES: &str =
    "import importlib.metadata as m; print('\\n'.join(m.requires('orbitar') or []))";

const PIP_CHECK: &str = "python3 -m pip --disable-pip-version-check check 2>&1; echo \"exit $?\"";

const IMAGE_ASE: &str = "import json, tomllib\nfrom ase.build import bulk\nfrom orbitar.ase import Orbitar\n\
    atoms = bulk('Si', 'diamond', a=5.43)\nfixed = {'cutoff_ha': 10.0, 'kpoints': {'mesh': [1, 1, 1]}}\n\
    try:\n    Orbitar(directory='/tmp/without', **fixed).write_inputfiles(atoms, ['energy'])\n    without = 'written'\n\
    except ValueError as refusal:\n    without = str(refusal)\n\
    Orbitar(directory='/tmp/given', datasets=IMAGE_DATASETS, **fixed).write_inputfiles(atoms, ['energy'])\n\
    given = tomllib.load(open('/tmp/given/input.toml', 'rb'))['datasets']['directory']\n\
    print(json.dumps({'without': without, 'given': given}))";
pub const IMAGE_DATASETS: &str = "/work/datasets";
pub const NO_DATASETS: &str = "Orbitar: no dataset directory for family jth-2.0";

pub fn image_verdict(image: &Image) -> Findings {
    let target = &image.target;
    let mut findings = Vec::new();
    if image.uid != "1000" {
        findings.push(format!(
            "image {target}: runs as uid {}, not 1000",
            image.uid
        ));
    }
    if !image.held.trim().is_empty() {
        findings.push(format!(
            "image {target}: holds a dataset, a rival code, or git history: {}",
            image.held.trim()
        ));
    }
    let mut found = BTreeMap::new();
    for line in image.paw.lines().filter(|l| !l.trim().is_empty()) {
        match line.split_once("  ") {
            Some(("error", what)) => findings.push(format!(
                "image {target}: the PAW-XML scan could not inspect {what}; an unread file is no proof of no dataset"
            )),
            Some((sha, path)) => {
                found.insert(path.trim(), sha.trim());
            }
            None => findings.push(format!(
                "image {target}: the PAW-XML scan printed `{line}`, neither a file nor an error"
            )),
        }
    }
    for (path, sha) in &found {
        match image.shipped.iter().find(|(p, _)| p == path) {
            Some((_, pin)) if pin == sha => {}
            Some((_, pin)) => findings.push(format!(
                "image {target}: holds {path} with sha256 {sha}, not its family pin {pin}"
            )),
            None => findings.push(format!(
                "image {target}: holds a PAW-XML dataset that no pinned family of the checkout owns: {path}"
            )),
        }
    }
    for (path, _) in image
        .shipped
        .iter()
        .filter(|(p, _)| !found.contains_key(p.as_str()))
    {
        findings.push(format!(
            "image {target}: lacks {path}, which its family pins and its archive holds"
        ));
    }
    if image.notices.trim().is_empty() {
        findings.push(format!(
            "image {target}: holds no THIRD-PARTY-NOTICES of the orbitar wheel"
        ));
    }
    findings.extend(named(&format!("image {target}"), &image.metadata));
    if let Some(archived) = &image.archived {
        findings.extend(named(&format!("image {target} archived tree"), archived));
    }
    for (path, bytes) in &image.engines {
        findings.extend(crate::reproduce::stamped(
            &format!("image {target} {path}"),
            bytes,
        ));
    }
    match serde_json::from_str::<serde_json::Value>(&image.outcome) {
        Ok(json) if json["eigenvalues_ha"].is_array() => {}
        _ => findings.push(format!(
            "image {target}: orbitar run printed `{}`, no outcome",
            image.outcome
        )),
    }
    if let Some(stderr) = &image.compiled
        && stderr.contains("Compiling")
    {
        findings.push(format!(
            "image {target}: cargo compiled at run time:\n{stderr}"
        ));
    }
    let lines: Vec<&str> = image.requirements.lines().collect();
    let missing = |l: &str| {
        (l.strip_prefix("orbitar "))
            .and_then(|l| l.strip_suffix(" requires orbitar-jth, which is not installed."))
            .is_some_and(|version| !version.is_empty() && !version.contains(' '))
    };
    if !matches!(lines.as_slice(), [line, "exit 1"] if missing(line)) {
        findings.push(format!(
            "image {target}: pip check printed `{}`, not orbitar's one missing requirement orbitar-jth alone",
            image.requirements.trim()
        ));
    }
    let pins: Vec<String> = (image.requires.lines())
        .map(|l| l.replace(' ', ""))
        .filter(|l| l.starts_with("orbitar-jth"))
        .collect();
    let pin = format!("orbitar-jth=={}", image.jth_wanted);
    if !matches!(pins.as_slice(), [one] if *one == pin) {
        findings.push(format!(
            "image {target}: the installed orbitar requires {pins:?}, not `{pin}` alone (packages/orbitar-jth)"
        ));
    }
    let ase: serde_json::Value = serde_json::from_str(&image.ase).unwrap_or_default();
    if !ase["without"]
        .as_str()
        .is_some_and(|w| w.starts_with(NO_DATASETS))
    {
        findings.push(format!(
            "image {target}: an ASE calculator without datasets= printed `{}`, not `{NO_DATASETS}…`; the image holds no data package",
            image.ase
        ));
    }
    if ase["given"] != IMAGE_DATASETS {
        findings.push(format!(
            "image {target}: an ASE calculator with datasets={IMAGE_DATASETS} printed `{}`, not that directory",
            image.ase
        ));
    }
    findings
}

pub const PAW_SCAN: &str = concat!(
    r#"
import hashlib, os, re, stat, sys
LIMIT = 65536
root = re.compile("#,
    paw_root!(),
    r#")
top_dir = sys.argv[1] if len(sys.argv) > 1 else "/"
def error(path, e):
    print("error  " + path + ": " + (e.strerror or str(e)))
dev = os.lstat(top_dir).st_dev
for top, dirs, files in os.walk(top_dir, onerror=lambda e: error(e.filename or "?", e)):
    kept = []
    for d in dirs:
        path = os.path.join(top, d)
        try:
            if os.lstat(path).st_dev == dev:
                kept.append(d)
        except OSError as e:
            error(path, e)
    dirs[:] = kept
    for name in files:
        path = os.path.join(top, name)
        try:
            if not stat.S_ISREG(os.lstat(path).st_mode):
                continue
            with open(path, "rb") as f:
                if root.match(f.read(LIMIT)):
                    f.seek(0)
                    print(hashlib.sha256(f.read()).hexdigest() + "  " + path)
        except OSError as e:
            error(path, e)
"#
);

pub const PAW_SCAN_BYTES: usize = 65536;

fn observe_image(root: &Path, target: &str, tag: &str) -> Result<Image, Findings> {
    let one = |e: String| vec![e];
    let work = root.join("target/image").join(target);
    let _ = fs::remove_dir_all(&work);
    fs::create_dir_all(&work).map_err(|e| one(format!("{}: {e}", work.display())))?;
    fs::copy(root.join(CASE_INPUT), work.join("input.toml"))
        .map_err(|e| one(format!("{CASE_INPUT}: {e}")))?;
    for (path, mode) in [(work.clone(), 0o777), (work.join("input.toml"), 0o666)] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode))
            .map_err(|e| one(format!("{}: {e}", path.display())))?;
    }
    let volume = format!("{}:/work", work.display());
    let docker = |root: bool, entry: &str, args: &[&str]| -> Result<Output, String> {
        let mut command = Command::new("docker");
        command.args([
            "run",
            "--rm",
            "--read-only",
            "--tmpfs",
            "/tmp",
            "--volume",
            &volume,
        ]);
        if root {
            command.args(["--user", "0"]);
        }
        command.args(["--entrypoint", entry, tag]).args(args);
        run(&mut command, &format!("docker run {tag} {entry}"))
    };
    let text =
        |root: bool, entry: &str, args: &[&str]| docker(root, entry, args).map(|o| stdout(&o));
    let bench = target == "bench";
    let uid = text(false, "id", &["-u"]).map_err(one)?;
    let find = "find / -xdev \\( -name '*-JTH.xml' -o -iname '*.upf' -o -name .git -o -type f -name abinit \\
                -o -type f -name pw.x \\) -print 2>/dev/null; true";
    let held = text(true, "sh", &["-c", find]).map_err(one)?;
    let paw = text(true, "python3", &["-I", "-c", PAW_SCAN]).map_err(one)?;
    let shipped = if bench {
        let owned = crate::harness::datasets::own_pins(root);
        if let Some(fault) = owned.faults.first() {
            return Err(one(format!(
                "image: datasets/ has a fault, so no family pins its files: {fault}"
            )));
        }
        (owned.pins.into_iter())
            .map(|(path, (_, sha))| (format!("/opt/orbitar/{path}"), sha))
            .collect()
    } else {
        Vec::new()
    };
    let find = "find / -xdev -path '*orbitar-*.dist-info/licenses/THIRD-PARTY-NOTICES' -print 2>/dev/null; true";
    let notices = text(true, "sh", &["-c", find]).map_err(one)?;
    let inspect = run(
        Command::new("docker").args(["image", "inspect", tag]),
        "docker inspect",
    )
    .map_err(one)?;
    let format = "{{.CreatedBy}} {{.Comment}}";
    let history = run(
        Command::new("docker").args(["history", "--no-trunc", "--format", format, tag]),
        "docker history",
    )
    .map_err(one)?;
    let metadata = stdout(&inspect) + "\n" + &stdout(&history);
    let archived = if bench {
        let tree =
            "cd /opt/orbitar && find . -path ./target -prune -o -type f -print0 | xargs -0 cat";
        let out = docker(false, "sh", &["-c", tree]).map_err(one)?;
        Some(String::from_utf8_lossy(&out.stdout).into_owned())
    } else {
        None
    };
    let paths: &[&str] = if bench {
        &[
            "/opt/python/cp312-cp312/bin/orbitar",
            "/opt/orbitar/target/release/orbitar",
        ]
    } else {
        &["/usr/local/bin/orbitar"]
    };
    let mut engines = Vec::new();
    for path in paths {
        engines.push((
            (*path).to_owned(),
            docker(false, "cat", &[path]).map_err(one)?.stdout,
        ));
    }
    let outcome = if bench {
        text(false, "orbitar", &["run", "input.toml"]).map_err(one)?
    } else {
        let mut command = Command::new("docker");
        command
            .args([
                "run",
                "--rm",
                "--read-only",
                "--user",
                "1000",
                "--volume",
                &volume,
            ])
            .args([tag, "orbitar", "run", "input.toml"])
            .current_dir(&work);
        let line = format!("docker run {tag} orbitar run input.toml");
        run(&mut command, &line).map(|o| stdout(&o)).map_err(one)?
    };
    let script = IMAGE_ASE.replace("IMAGE_DATASETS", &format!("{IMAGE_DATASETS:?}"));
    let ase = text(false, "python3", &["-I", "-c", &script]).map_err(one)?;
    let requirements = text(false, "sh", &["-c", PIP_CHECK]).map_err(one)?;
    let requires = text(false, "python3", &["-I", "-c", REQUIRES]).map_err(one)?;
    let jth_wanted = jth_version(root).map_err(one)?;
    let compiled = if bench {
        let engine = text(false, "cat", &["/opt/engine.sha256"]).map_err(one)?;
        println!("image bench: engine {engine}");
        let builds = "orbitar-bench help >/dev/null && cd /work/orbitar \\
                      && cargo build --release --locked --package orbitar --bin orbitar \\
                      && cargo test --locked --package xtask --test benchmarks --no-run";
        let out = docker(false, "sh", &["-c", builds]).map_err(one)?;
        Some(String::from_utf8_lossy(&out.stderr).into_owned())
    } else {
        None
    };
    Ok(Image {
        target: target.to_owned(),
        uid,
        held,
        paw,
        shipped,
        notices,
        metadata,
        archived,
        engines,
        outcome,
        compiled,
        ase,
        requirements,
        requires,
        jth_wanted,
    })
}
