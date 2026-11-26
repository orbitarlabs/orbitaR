use crate::{Findings, harness::datasets, lock::hex, release};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::ffi::OsStr;
use std::{
    collections::BTreeMap,
    f64::consts::PI,
    fs,
    path::Path,
    process::{Command, Stdio},
};

pub(crate) const INPUTS: &str = datasets::FAMILIES;
pub(crate) const GAP_RAD: f64 = 0.1;
pub(crate) const NUMERICS: [(&str, &str); 2] = [
    ("OPENBLAS_CORETYPE", "HASWELL"),
    ("OPENBLAS_NUM_THREADS", "1"),
];
const EMIN_RY: f64 = -5.0;
const BELOW_RY: f64 = 1.0;
const EMAX_RY: f64 = 4.95;
pub(crate) const STEP_RY: f64 = 0.05;
const STEP_SLACK_RY: f64 = 1e-9;
pub(crate) const REBUILD_REL: f64 = 1e-6;
const DATE: &str = "  PAW functions generated on ";
const CONTACT: [&str; 2] = ["  Contact info: ", "  email: "];

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Pin {
    version: String,
    build: String,
    sha256: String,
    packages: Vec<Package>,
    pinned_date: String,
    binaries: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Package {
    url: String,
    sha256: String,
}

#[derive(Debug, PartialEq)]
pub(crate) struct Wave {
    pub(crate) l: usize,
    pub(crate) window_ry: [f64; 2],
    pub(crate) points: usize,
    pub(crate) step_ry: f64,
    pub(crate) gap_rad: f64,
    pub(crate) poles_ae: usize,
    pub(crate) poles_paw: usize,
}

pub(crate) fn run(
    root: &Path,
    install: Option<&Path>,
    chosen: &[String],
    prefix: Option<&Path>,
    path: Option<&OsStr>,
    write: bool,
) -> Findings {
    let pin = match pin(root) {
        Ok(pin) => pin,
        Err(e) => return vec![format!("atompaw: {e}")],
    };
    let Some(prefix) = install.or(prefix) else {
        return vec![
            "atompaw: set ORBITAR_ATOMPAW to the install's prefix, or pass --install <dir>".into(),
        ];
    };
    if !prefix.is_absolute() {
        return vec![format!(
            "atompaw: the install's prefix {} must be an absolute path",
            prefix.display()
        )];
    }
    if install.is_some() {
        let findings = install_into(&pin, prefix, path);
        if !findings.is_empty() {
            return findings;
        }
    }
    let findings = pinned(&pin, prefix);
    if !findings.is_empty() || (install.is_some() && chosen.is_empty()) {
        return findings;
    }
    let all = match inputs(root) {
        Ok(all) => all,
        Err(e) => {
            return vec![format!(
                "atompaw: {INPUTS} holds no <family>/<element>.in: {e}"
            )];
        }
    };
    let mut inputs = Vec::new();
    for word in chosen {
        let picked: Vec<_> = (all.iter())
            .filter(|(family, element)| *word == *family || *word == format!("{family}/{element}"))
            .cloned()
            .collect();
        if picked.is_empty() {
            return vec![format!(
                "atompaw: {word}: no {INPUTS}/<family>/<element>.in; name <family> or <family>/<element>"
            )];
        }
        inputs.extend(picked);
    }
    if chosen.is_empty() {
        inputs = all;
    }
    if inputs.is_empty() {
        return vec![format!("atompaw: {INPUTS} holds no <family>/<element>.in")];
    }
    inputs
        .iter()
        .flat_map(
            |(family, element)| match build(root, &pin, prefix, family, element, write) {
                Ok(findings) => findings,
                Err(e) => vec![format!("atompaw: {family}/{element}: {e}")],
            },
        )
        .collect()
}

pub(crate) fn pin(root: &Path) -> Result<Pin, String> {
    let value = datasets::atompaw_table(root)?;
    let pin: Pin = value
        .try_into()
        .map_err(|e| format!("{} [atompaw]: {e}", datasets::PATH))?;
    let name = format!("/atompaw-{}-{}.conda", pin.version, pin.build);
    match pin
        .packages
        .iter()
        .filter(|p| p.url.ends_with(&name))
        .collect::<Vec<_>>()[..]
    {
        [package] if package.sha256 == pin.sha256 => Ok(pin),
        [_] => Err(format!(
            "{} [atompaw]: the package of {name} has another sha256 than [atompaw] sha256",
            datasets::PATH
        )),
        _ => Err(format!(
            "{} [atompaw]: packages must hold {name} once",
            datasets::PATH
        )),
    }
}

fn inputs(root: &Path) -> Result<Vec<(String, String)>, String> {
    let mut out = Vec::new();
    for (family, kind) in datasets::listing(&root.join(INPUTS), INPUTS)? {
        if !kind.is_dir() {
            continue;
        }
        let dir = format!("{INPUTS}/{family}");
        for (file, kind) in datasets::listing(&root.join(&dir), &dir)? {
            if let Some(element) = file.strip_suffix(".in").filter(|_| kind.is_file()) {
                out.push((family.clone(), element.to_owned()));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn install_into(pin: &Pin, dir: &Path, path: Option<&OsStr>) -> Findings {
    let spec = dir.with_extension("explicit.txt");
    let lines: Vec<String> = (pin.packages.iter())
        .map(|p| format!("{}#{}", p.url, p.sha256))
        .collect();
    if let Err(e) = fs::write(&spec, format!("@EXPLICIT\n{}\n", lines.join("\n"))) {
        return vec![format!("atompaw: {}: {e}", spec.display())];
    }
    let mut micromamba = Command::new("micromamba");
    if let Some(path) = path {
        micromamba.env("PATH", path);
    }
    let status = micromamba
        .args(["create", "--yes", "--quiet", "--prefix"])
        .arg(dir)
        .arg("--file")
        .arg(&spec)
        .status();
    let _ = fs::remove_file(&spec);
    match status {
        Ok(s) if s.success() => Vec::new(),
        Ok(s) => vec![format!("atompaw: micromamba create {}: {s}", dir.display())],
        Err(e) => vec![format!("atompaw: micromamba: {e}")],
    }
}

pub(crate) fn pinned(pin: &Pin, prefix: &Path) -> Findings {
    let at = format!("atompaw: {}", prefix.display());
    let mut findings = Vec::new();
    let meta = prefix.join("conda-meta");
    let mut found: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    for entry in fs::read_dir(&meta).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|e| e == "json") {
            let json = fs::read_to_string(&path)
                .ok()
                .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
                .unwrap_or_default();
            let url = json["url"].as_str().unwrap_or_default().to_owned();
            if found.insert(url.clone(), json).is_some() {
                findings.push(format!("{at}: two conda-meta records name {url}"));
            }
        }
    }
    for package in &pin.packages {
        match found.remove(&package.url) {
            None => findings.push(format!("{at} lacks {}", package.url)),
            Some(json) if json["sha256"].as_str() != Some(&package.sha256) => {
                findings.push(format!(
                    "{at}: {} has sha256 {}, but {} pins {}",
                    package.url,
                    json["sha256"],
                    datasets::PATH,
                    package.sha256
                ))
            }
            Some(_) => {}
        }
    }
    findings.extend(
        found
            .keys()
            .map(|url| format!("{at} holds {url}, which {} does not pin", datasets::PATH)),
    );
    findings.extend(binaries(&at, pin, prefix));
    findings
}

fn binaries(at: &str, pin: &Pin, prefix: &Path) -> Findings {
    if !pin.binaries.contains_key("bin/atompaw") {
        return vec![format!(
            "{} [atompaw.binaries] pins no bin/atompaw",
            datasets::PATH
        )];
    }
    let mut findings = Vec::new();
    for (path, sha) in &pin.binaries {
        match fs::canonicalize(prefix.join(path)).and_then(fs::read) {
            Ok(bytes) if hex(&Sha256::digest(&bytes)) == *sha => {}
            Ok(bytes) => findings.push(format!(
                "{at}: {path} has sha256 {}, but {} pins {sha}",
                hex(&Sha256::digest(&bytes)),
                datasets::PATH
            )),
            Err(e) => findings.push(format!("{at}: {path}: {e}")),
        }
    }
    findings
}

fn build(
    root: &Path,
    pin: &Pin,
    prefix: &Path,
    family: &str,
    element: &str,
    write_file: bool,
) -> Result<Findings, String> {
    let input_name = format!("{INPUTS}/{family}/{element}.in");
    let input = datasets::regular(&root.join(&input_name), &input_name)?;
    let work = root.join("target/atompaw").join(family).join(element);
    let first = generate(prefix, &input, &work.join("run-1"), &pin.pinned_date)?;
    let second = generate(prefix, &input, &work.join("run-2"), &pin.pinned_date)?;
    let mut findings = Vec::new();
    if first.1 != second.1 {
        findings.push(format!(
            "atompaw: {element}: two runs of the pinned ATOMPAW wrote different bytes; see {}",
            work.display()
        ));
    }
    let what = format!("atompaw: {element}: {}", first.0);
    findings.extend(release::named(&what, &String::from_utf8_lossy(&first.1)));
    let path = format!("{family}/{element}.xml");
    let sha = hex(&Sha256::digest(&first.1));
    let committed = Path::new(INPUTS).join(&path);
    let mut largest_rel = None;
    if !write_file {
        match committed_file(root, family, &path) {
            Ok(text) => match rebuild(&text, &String::from_utf8_lossy(&first.1)) {
                Ok(rel) if rel <= REBUILD_REL => largest_rel = Some(rel),
                Ok(rel) => {
                    largest_rel = Some(rel);
                    findings.push(format!(
                        "atompaw: {element}: a number of the rebuild departs {rel:.2e} from {}, above {REBUILD_REL:e}",
                        committed.display()
                    ));
                }
                Err(e) => findings.push(format!(
                    "atompaw: {element}: the rebuild differs from {}: {e}",
                    committed.display()
                )),
            },
            Err(e) => findings.push(format!("atompaw: {e}")),
        }
    }
    let (lmax, low_ry) = valence(&first.1)?;
    let floor_ry = EMIN_RY.min(low_ry - BELOW_RY);
    let mut waves = Vec::new();
    for l in 0..=lmax + 1 {
        let name = format!("logderiv.{l}");
        let [one, two] = ["run-1", "run-2"].map(|run| fs::read(work.join(run).join(&name)));
        let one = one.map_err(|e| {
            format!("ATOMPAW wrote no {name} (the XML's lmax is {lmax}, so l = 0 to {} each need one): {e}", lmax + 1)
        })?;
        if two.ok().as_ref() != Some(&one) {
            findings.push(format!(
                "atompaw: {element}: two runs of the pinned ATOMPAW wrote different {name}; see {}",
                work.display()
            ));
        }
        let text = String::from_utf8_lossy(&one);
        waves.push(wave(l, &text).map_err(|e| format!("{name}: {e}"))?);
    }
    for w in &waves {
        if w.window_ry[0] > floor_ry || w.window_ry[1] < EMAX_RY {
            findings.push(format!(
                "atompaw: {element}: l = {}: the window {:?} Ry must reach from {floor_ry} to {EMAX_RY} Ry",
                w.l, w.window_ry
            ));
        }
        if w.step_ry > STEP_RY + STEP_SLACK_RY {
            findings.push(format!(
                "atompaw: {element}: l = {}: a step of {} Ry, above {STEP_RY}",
                w.l, w.step_ry
            ));
        }
        if w.gap_rad > GAP_RAD {
            findings.push(format!(
                "atompaw: {element}: l = {}: the PAW log derivative departs {:.4} rad from the all-electron one, above {GAP_RAD}",
                w.l, w.gap_rad
            ));
        }
        if w.poles_paw != w.poles_ae {
            findings.push(format!(
                "atompaw: {element}: l = {}: the PAW log derivative has {} poles, the all-electron one {}: a ghost state",
                w.l, w.poles_paw, w.poles_ae
            ));
        }
    }
    let record = json!({
        "file": path,
        "sha256": sha,
        "input": input_name,
        "input_sha256": hex(&Sha256::digest(&input)),
        "atompaw": { "version": pin.version, "build": pin.build, "sha256": pin.sha256 },
        "gap_bound_rad": GAP_RAD,
        "window_bound_ry": [floor_ry, EMAX_RY],
        "step_bound_ry": STEP_RY,
        "rebuild_bound_rel": REBUILD_REL,
        "rebuild_largest_rel": largest_rel,
        "lmax": lmax,
        "lowest_valence_ry": low_ry,
        "logderiv": waves.iter().map(|w| json!({
            "l": w.l, "window_ry": w.window_ry, "points": w.points, "step_ry": w.step_ry, "gap_rad": w.gap_rad,
            "poles_ae": w.poles_ae, "poles_paw": w.poles_paw,
        })).collect::<Vec<_>>(),
        "findings": findings,
    });
    let text = serde_json::to_string_pretty(&record).map_err(|e| e.to_string())?;
    write(
        &root
            .join("target/atompaw")
            .join(family)
            .join(format!("{element}.json")),
        text.as_bytes(),
    )?;
    if write_file && findings.is_empty() {
        write_own(root, family, element, &first.1)?;
        println!(
            "atompaw: wrote {}, sha256 {sha}; give it a row of a tuning table beside it, then write the family table \
             with `cargo xtask family {family} --from {INPUTS}/{family}/<table.toml>`",
            committed.display()
        );
    }
    Ok(findings)
}

fn own_pin(root: &Path, family: &str, element: &str) -> Result<Option<String>, String> {
    let table = format!("{INPUTS}/{family}.toml");
    let committed = format!("{INPUTS}/{family}/{element}.xml");
    let owned = datasets::own_pins(root);
    if let Some((owner, sha)) = owned.pins.get(&committed)
        && owner == family
    {
        return Ok(Some(sha.clone()));
    }
    let ours = |fault: &&String| {
        (fault.strip_prefix(&table)).is_some_and(|rest| rest.starts_with([':', ' ']))
    };
    if let Some(fault) = owned.faults.iter().find(ours) {
        return Err(format!("{committed}: {fault}; such a table pins nothing"));
    }
    if let Some(listed) = (owned.names.iter()).find(|name| name.eq_ignore_ascii_case(family)) {
        return Err(format!(
            "{committed}: {INPUTS}/{listed}.toml pins no such file; write it with --write, then the table with \
             `cargo xtask family`"
        ));
    }
    datasets::new_family(root, family).map_err(|e| format!("{committed}: {e}"))?;
    Ok(None)
}

fn write_own(root: &Path, family: &str, element: &str, bytes: &[u8]) -> Result<(), String> {
    let table = format!("{INPUTS}/{family}.toml");
    let committed = format!("{INPUTS}/{family}/{element}.xml");
    let sha = hex(&Sha256::digest(bytes));
    let missing = format!("{committed}: {table} pins no such file");
    match own_pin(root, family, element) {
        Ok(Some(pin)) if pin != sha => {
            return Err(format!(
                "{committed}: the build has sha256 {sha}, but {table} pins {pin}; a family never changes: build a new one"
            ));
        }
        Err(e) if e.starts_with(&missing) => {
            return Err(format!(
                "{missing}; a family never changes: build a new one"
            ));
        }
        Err(e) => return Err(e),
        Ok(_) => {}
    }
    let target = root.join(&committed);
    if fs::symlink_metadata(&target).is_ok_and(|m| !m.is_file()) {
        return Err(format!(
            "{committed} is a link; our files are regular files"
        ));
    }
    let part = root.join(format!(
        "{INPUTS}/{family}/.{element}.xml.{}.part",
        std::process::id()
    ));
    let mut file = (fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&part))
    .map_err(|e| format!("{}: {e}", part.display()))?;
    let written =
        std::io::Write::write_all(&mut file, bytes).and_then(|()| fs::rename(&part, &target));
    if let Err(e) = written {
        let _ = fs::remove_file(&part);
        return Err(format!("{committed}: {e}"));
    }
    Ok(())
}

fn committed_file(root: &Path, family: &str, path: &str) -> Result<String, String> {
    let table = format!("{INPUTS}/{family}.toml");
    let committed = format!("{INPUTS}/{path}");
    let element = (path.rsplit('/').next())
        .and_then(|f| f.strip_suffix(".xml"))
        .unwrap_or_default();
    let Some(sha) = own_pin(root, family, element)? else {
        return Err(format!(
            "{committed}: {table} pins no such file; write it with --write, then the table with `cargo xtask family`"
        ));
    };
    if fs::symlink_metadata(root.join(&committed)).is_ok_and(|m| !m.is_file()) {
        return Err(format!(
            "{committed} is a link; our files are regular files"
        ));
    }
    let bytes = datasets::regular_in(root, &committed)?;
    let found = hex(&Sha256::digest(&bytes));
    if found != sha {
        return Err(format!(
            "{committed}: sha256 {found}, but {table} pins {sha}"
        ));
    }
    String::from_utf8(bytes).map_err(|e| format!("{committed}: {e}"))
}

pub(crate) fn rebuild(committed: &str, rebuilt: &str) -> Result<f64, String> {
    let parse = |which: &str, text| {
        roxmltree::Document::parse(text).map_err(|e| format!("the {which} XML: {e}"))
    };
    let (a, b) = (parse("committed", committed)?, parse("rebuilt", rebuilt)?);
    let (a, b): (Vec<_>, Vec<_>) = (a.descendants().collect(), b.descendants().collect());
    let mut largest: f64 = 0.0;
    for (x, y) in a.iter().zip(&b) {
        let at = place(x);
        if x.node_type() != y.node_type() || x.tag_name() != y.tag_name() {
            return Err(format!(
                "{at} in the committed file, {} in the rebuild",
                place(y)
            ));
        }
        let (m, n): (Vec<_>, Vec<_>) = (
            x.attributes().map(|a| a.name()).collect(),
            y.attributes().map(|a| a.name()).collect(),
        );
        if m != n {
            return Err(format!("{at}: attributes {m:?}, the rebuild's {n:?}"));
        }
        for (p, q) in x.attributes().zip(y.attributes()) {
            let rel = match (number(p.value()), number(q.value())) {
                (Some(u), Some(v)) => relative(u, v, u.abs()),
                _ if p.value() == q.value() => Some(0.0),
                _ => None,
            };
            largest = largest.max(rel.ok_or_else(|| {
                format!(
                    "{at}: {}={:?}, the rebuild's {:?}",
                    p.name(),
                    p.value(),
                    q.value()
                )
            })?);
        }
        if let (Some(s), Some(t)) = (x.text().filter(|_| x.is_text() || x.is_comment()), y.text()) {
            let (s, t): (Vec<_>, Vec<_>) = (
                s.split_whitespace().collect(),
                t.split_whitespace().collect(),
            );
            let count = |w: &[&str]| w.iter().filter(|w| number(w).is_some()).count();
            if s.len() != t.len() || count(&s) != count(&t) {
                return Err(format!(
                    "{at}: {} numbers in {} words, the rebuild's {} in {}",
                    count(&s),
                    s.len(),
                    count(&t),
                    t.len()
                ));
            }
            let scale = (s.iter().filter_map(|w| number(w)))
                .map(f64::abs)
                .fold(0.0, f64::max);
            for (u, v) in s.iter().zip(&t) {
                let rel = match (number(u), number(v)) {
                    (Some(u), Some(v)) => relative(u, v, scale),
                    (None, None) if u == v => Some(0.0),
                    _ => None,
                };
                largest = largest.max(
                    rel.ok_or_else(|| format!("{at}: {u:?} in the text, the rebuild's {v:?}"))?,
                );
            }
        }
    }
    if a.len() != b.len() {
        return Err(format!(
            "the committed file has {} nodes, the rebuild {}",
            a.len(),
            b.len()
        ));
    }
    Ok(largest)
}

fn number(word: &str) -> Option<f64> {
    word.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

fn relative(u: f64, v: f64, scale: f64) -> Option<f64> {
    let d = (u - v).abs();
    if scale == 0.0 {
        return (d == 0.0).then_some(0.0);
    }
    Some(d / scale).filter(|r| r.is_finite())
}

fn place(node: &roxmltree::Node) -> String {
    let element = match node.is_element() {
        true => Some(*node),
        false => node.parent_element(),
    };
    let Some(e) = element else {
        return "the document".into();
    };
    let kind = match node.node_type() {
        roxmltree::NodeType::Text => " text",
        roxmltree::NodeType::Comment => " comment",
        _ => "",
    };
    match e.attribute("state") {
        Some(state) => format!("<{} state={state:?}>{kind}", e.tag_name().name()),
        None => format!("<{}>{kind}", e.tag_name().name()),
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("{}: no directory", path.display()))?;
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    fs::write(path, bytes).map_err(|e| format!("{}: {e}", path.display()))
}

fn generate(
    prefix: &Path,
    input: &[u8],
    dir: &Path,
    date: &str,
) -> Result<(String, Vec<u8>), String> {
    let _ = fs::remove_dir_all(dir);
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let stdin = dir.join("input");
    fs::write(&stdin, input).map_err(|e| format!("{}: {e}", stdin.display()))?;
    let stdin = fs::File::open(&stdin).map_err(|e| format!("{}: {e}", stdin.display()))?;
    let log = dir.join("stdout");
    let stdout = fs::File::create(&log).map_err(|e| format!("{}: {e}", log.display()))?;
    let status = Command::new("atompaw")
        .env_clear()
        .env("PATH", prefix.join("bin"))
        .envs(NUMERICS)
        .current_dir(dir)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(Stdio::inherit())
        .status()
        .map_err(|e| format!("{}: {e}", prefix.join("bin/atompaw").display()))?;
    if !status.success() {
        return Err(format!("atompaw {status}; see {}", log.display()));
    }
    let shown = dir.display().to_string();
    let names: Vec<String> = (datasets::listing(dir, &shown)?.into_iter())
        .map(|(name, _)| name)
        .filter(|n| n.ends_with("-paw.xml"))
        .collect();
    let [name] = &names[..] else {
        return Err(format!(
            "{}: ATOMPAW wrote {} *-paw.xml files, not 1",
            dir.display(),
            names.len()
        ));
    };
    let text = fs::read_to_string(dir.join(name)).map_err(|e| format!("{name}: {e}"))?;
    Ok((
        name.clone(),
        dated(&text, date)
            .and_then(|text| uncontacted(&text))
            .map_err(|e| format!("{name}: {e}"))?
            .into_bytes(),
    ))
}

pub(crate) fn dated(text: &str, date: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with(DATE))
        .collect();
    let [i] = at[..] else {
        return Err(format!("{} lines start with {DATE:?}, not 1", at.len()));
    };
    let end = if lines[i].ends_with('\n') { "\n" } else { "" };
    let stamp = lines[i][DATE.len()..].strip_suffix(end).unwrap_or_default();
    for (what, given) in [("the date line", stamp), ("pinned_date", date)] {
        if !is_stamp(given) {
            return Err(format!(
                "{what} holds {given:?}, not a stamp MM/DD/YYYY, hh:mm:ss.sss"
            ));
        }
    }
    let line = format!("{DATE}{date}{end}");
    Ok(lines
        .iter()
        .enumerate()
        .map(|(j, l)| if j == i { line.as_str() } else { l })
        .collect())
}

pub(crate) fn uncontacted(text: &str) -> Result<String, String> {
    let lines: Vec<&str> = text.split_inclusive('\n').collect();
    let at: Vec<usize> = (0..lines.len())
        .filter(|&i| lines[i].starts_with(CONTACT[0]))
        .collect();
    let [i] = at[..] else {
        return Err(format!(
            "{} lines start with {:?}, not 1",
            at.len(),
            CONTACT[0]
        ));
    };
    if !lines.get(i + 1).is_some_and(|l| l.starts_with(CONTACT[1])) {
        return Err(format!(
            "the line after {:?} does not start with {:?}",
            CONTACT[0], CONTACT[1]
        ));
    }
    Ok((lines[..i].iter().chain(&lines[i + 2..]))
        .copied()
        .collect())
}

fn is_stamp(text: &str) -> bool {
    const PATTERN: &str = "99/99/9999, 99:99:99.999";
    text.len() == PATTERN.len()
        && (text.bytes().zip(PATTERN.bytes())).all(|(t, p)| {
            if p == b'9' {
                t.is_ascii_digit()
            } else {
                t == p
            }
        })
}

pub(crate) fn wave(l: usize, text: &str) -> Result<Wave, String> {
    let mut rows: Vec<[f64; 3]> = Vec::new();
    for (n, line) in text
        .lines()
        .enumerate()
        .filter(|(_, l)| !l.trim().is_empty())
    {
        let values: Vec<f64> = (line.split_whitespace())
            .map(str::parse::<f64>)
            .collect::<Result<_, _>>()
            .map_err(|e| format!("line {}: {e}", n + 1))?;
        match values[..] {
            [e, _, _, ae, paw] if [e, ae, paw].iter().all(|x| x.is_finite()) => {
                rows.push([e, ae, paw])
            }
            _ => return Err(format!("line {}: want 5 finite numbers", n + 1)),
        }
    }
    let (Some(first), Some(last)) = (rows.first(), rows.last()) else {
        return Err("no rows".into());
    };
    let gap_rad = (rows.iter())
        .map(|[_, ae, paw]| {
            let d = paw - ae;
            (d - PI * (d / PI).round()).abs()
        })
        .fold(0.0, f64::max);
    let step_ry = match rows.len() {
        0 | 1 => f64::INFINITY,
        _ => (rows.windows(2))
            .map(|pair| pair[1][0] - pair[0][0])
            .fold(f64::NEG_INFINITY, f64::max),
    };
    let poles = |column: usize| {
        (rows.windows(2))
            .map(|pair| (branch(pair[1][column]) - branch(pair[0][column])).abs())
            .sum::<f64>() as usize
    };
    Ok(Wave {
        l,
        window_ry: [first[0], last[0]],
        points: rows.len(),
        step_ry,
        gap_rad,
        poles_ae: poles(1),
        poles_paw: poles(2),
    })
}

fn branch(phase: f64) -> f64 {
    ((phase - PI / 2.0) / PI).floor()
}

fn valence(xml: &[u8]) -> Result<(usize, f64), String> {
    let text = String::from_utf8_lossy(xml);
    let doc = roxmltree::Document::parse(&text).map_err(|e| format!("the built XML: {e}"))?;
    let states: Vec<(usize, f64)> = (doc.descendants())
        .filter(|n| {
            n.has_tag_name("state") && n.parent().is_some_and(|p| p.has_tag_name("valence_states"))
        })
        .map(|n| {
            let l = n
                .attribute("l")
                .and_then(|v| v.trim().parse::<usize>().ok());
            let e = n.attribute("e").and_then(|v| v.trim().parse::<f64>().ok());
            l.zip(e.filter(|e| e.is_finite())).ok_or_else(|| {
                "the built XML has a valence state without a numeric l and e".to_owned()
            })
        })
        .collect::<Result<_, _>>()?;
    let lmax = states.iter().map(|s| s.0).max();
    let low = states.iter().map(|s| s.1).fold(f64::INFINITY, f64::min);
    lmax.map(|lmax| (lmax, 2.0 * low))
        .ok_or_else(|| "the built XML holds no <valence_states> state".into())
}
