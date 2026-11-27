mod abinit;
mod qe;

use super::engine::{self, Failure, Run, Stream};
use crate::case::Tier;
use serde::{Deserialize, Serialize};
use sha2::Digest as _;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

pub(crate) const SETTINGS: &str = "benchmarks/RIVALS.toml";
pub(crate) const TUNED: &str = "benchmarks/TUNED.toml";
const CLOCK_SLACK_S: f64 = 0.15;
const CLOCK_SHARE: f64 = 0.02;
const STEP_SLACK_S: f64 = 0.05;
const STEP_SHARE: f64 = 0.5;
const BLAS_POOLS: [(&str, &str); 1] = [("openblas", "OPENBLAS_NUM_THREADS")];

#[derive(Deserialize)]
pub(crate) struct Settings {
    pub(crate) calibration_cells: Vec<String>,
    pub(crate) layout_cells: Vec<String>,
    #[serde(default)]
    pub(crate) mixing_cells: Vec<String>,
    pub(crate) budget_s: f64,
    pub(crate) threads: Vec<usize>,
    pub(crate) target: Target,
    #[serde(flatten)]
    codes: BTreeMap<Rival, Setup>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Target {
    pub(crate) energy_difference_ha_per_atom: f64,
    pub(crate) volume_scale: f64,
    pub(crate) pressure_gpa: f64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Setup {
    pub(crate) ladder_ha: Vec<f64>,
    pub(crate) converged_ha: f64,
    pub(crate) density_ratio: f64,
    #[serde(default)]
    mpiexec: Vec<String>,
    #[serde(default)]
    pub(crate) skip_cells: Vec<Skip>,
    #[serde(default)]
    pub(crate) stop_floor_ha: Option<f64>,
    #[serde(default = "yes")]
    openmp: bool,
}

fn yes() -> bool {
    true
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Skip {
    pub(crate) cell: String,
    pub(crate) reason: String,
}

pub(crate) fn thread_env(layout: &Layout) -> BTreeMap<&'static str, String> {
    (BLAS_POOLS
        .map(|(_, pool)| (pool, "1".to_owned()))
        .into_iter())
    .chain([("OMP_NUM_THREADS", layout.threads.to_string())])
    .collect()
}

pub(crate) fn layout_policy(code: Rival, sweep: &[usize], kpoints: &[usize]) -> String {
    let candidates: Vec<(usize, Vec<Layout>)> = (sweep.iter())
        .map(|&p| (p, code.adapter().candidates(p, kpoints)))
        .collect();
    let policy = format!("{BLAS_POOLS:?} {candidates:?}");
    crate::lock::hex(&sha2::Sha256::digest(policy))
}

pub(crate) fn layout_kpoints(
    root: &Path,
    settings: &Settings,
    code: Rival,
) -> Result<Vec<usize>, String> {
    (settings.layout_cells.iter())
        .filter(|path| settings.skips(code, path).is_none())
        .map(|path| {
            let text = fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
            let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            Ok(Cell::parse(&table)?.mesh_points())
        })
        .collect()
}

pub(crate) fn input_path(id: &str, name: &str) -> String {
    match name {
        "input" => format!("benchmarks/{id}/input.toml"),
        _ => format!("benchmarks/{id}/inputs/{name}.toml"),
    }
}

pub(crate) const HELD_OUT: &str = "held-out-";

pub(crate) fn base_path(id: &str, name: &str) -> String {
    input_path(id, name.strip_prefix(HELD_OUT).unwrap_or(name))
}

pub(crate) fn settings(root: &Path) -> Result<Settings, String> {
    let text = fs::read_to_string(root.join(SETTINGS)).map_err(|e| format!("{SETTINGS}: {e}"))?;
    let settings: Settings = toml::from_str(&text).map_err(|e| format!("{SETTINGS}: {e}"))?;
    let sweep: Vec<usize> = (Tier::Claim.policy().speed)
        .map_or(Vec::new(), |s| s.threads.iter().map(|p| p.get()).collect());
    if let Some(p) = settings.threads.iter().find(|p| !sweep.contains(p)) {
        return Err(format!(
            "{SETTINGS}: threads holds {p}, which the claim sweep {sweep:?} does not"
        ));
    }
    match Rival::all()
        .iter()
        .find(|c| !settings.codes.contains_key(c))
    {
        Some(code) => Err(format!("{SETTINGS}: no [{}] table", code.adapter().name())),
        None => Ok(settings),
    }
}

impl Settings {
    pub(crate) fn setup(&self, code: Rival) -> &Setup {
        &self.codes[&code]
    }

    pub(crate) fn skips(&self, code: Rival, path: &str) -> Option<&str> {
        (self.setup(code).skip_cells.iter())
            .find(|s| s.cell == path)
            .map(|s| s.reason.as_str())
    }

    pub(crate) fn times(&self, threads: usize) -> bool {
        self.threads.contains(&threads)
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Layout {
    pub(crate) ranks: usize,
    pub(crate) threads: usize,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    input: Vec<String>,
}

impl Layout {
    pub(crate) fn lines(&self) -> &[String] {
        &self.input
    }

    pub(crate) fn added_to(&self, candidate: &Layout) -> Vec<String> {
        (self.input.iter())
            .filter(|line| !candidate.input.contains(line))
            .cloned()
            .collect()
    }

    fn new(ranks: usize, threads: usize, args: Vec<String>, input: Vec<String>) -> Layout {
        Layout {
            ranks,
            threads,
            args,
            input,
        }
    }
}

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Tuned {
    #[serde(default)]
    pub(crate) tuning_set: BTreeSet<String>,
    #[serde(flatten)]
    codes: BTreeMap<Rival, Frozen>,
}

#[derive(Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Frozen {
    #[serde(default)]
    pub(crate) cutoff_ha: BTreeMap<String, f64>,
    #[serde(default, deserialize_with = "five_columns")]
    pub(crate) calibration: BTreeMap<String, Vec<Row>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) volume_scale: Option<f64>,
    #[serde(default)]
    pub(crate) layout: BTreeMap<String, Layout>,
    #[serde(default)]
    pub(crate) searched: BTreeMap<String, Vec<String>>,
    #[serde(default, skip_serializing_if = "Mixing::unsearched")]
    pub(crate) mixing: Mixing,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) layout_policy: Option<String>,
}

#[derive(Clone, Default, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Mixing {
    #[serde(default)]
    pub(crate) input: Vec<String>,
    #[serde(default)]
    pub(crate) reason: String,
    #[serde(default)]
    pub(crate) searched: Vec<String>,
}

pub(crate) type Row = [f64; 5];

fn five_columns<'de, D: serde::Deserializer<'de>>(
    d: D,
) -> Result<BTreeMap<String, Vec<Row>>, D::Error> {
    let raw = BTreeMap::<String, Vec<Vec<f64>>>::deserialize(d)?;
    (raw.into_iter())
        .map(|(cell, rows)| {
            let rows = (rows.into_iter())
                .map(|r| {
                    let n = r.len();
                    Row::try_from(r).map_err(|_| {
                        serde::de::Error::custom(format!(
                            "{cell}: a calibration row of {n} columns, not 5 (cutoff, energy difference, pressure, \
                             absolute energy, pressure itself): another rule wrote it; recalibrate: remove the old file and \
                             run `cargo xtask rivals calibrate`, then `rivals layout`"
                        ))
                    })
                })
                .collect::<Result<_, _>>()?;
            Ok((cell, rows))
        })
        .collect()
}

static UNTUNED: Frozen = Frozen {
    cutoff_ha: BTreeMap::new(),
    calibration: BTreeMap::new(),
    volume_scale: None,
    layout: BTreeMap::new(),
    searched: BTreeMap::new(),
    mixing: Mixing {
        input: Vec::new(),
        reason: String::new(),
        searched: Vec::new(),
    },
    layout_policy: None,
};

impl Mixing {
    fn unsearched(&self) -> bool {
        self.searched.is_empty() && self.input.is_empty() && self.reason.is_empty()
    }
}

impl Tuned {
    pub(crate) fn of(&self, code: Rival) -> &Frozen {
        self.codes.get(&code).unwrap_or(&UNTUNED)
    }

    pub(crate) fn of_mut(&mut self, code: Rival) -> &mut Frozen {
        self.codes.entry(code).or_default()
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Mode {
    Energy,
    Stress,
}

#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Clocks {
    Loss,
    Warning,
}

pub(crate) struct Rivals {
    root: PathBuf,
    prefix: PathBuf,
    pub(crate) settings: Settings,
    pub(crate) tuned: Tuned,
}

pub(crate) struct Job {
    pub(crate) code: Rival,
    pub(crate) cell: Cell,
    pub(crate) cutoff_ha: f64,
    pub(crate) layout: Layout,
    pub(crate) mixing: Vec<String>,
    pub(crate) mpiexec: Vec<String>,
    pub(crate) claim: Option<ClaimPart>,
}

pub(crate) struct ClaimPart {
    pub(crate) files: BTreeMap<String, String>,
    pub(crate) floor_ha: Option<f64>,
    pub(crate) busy_cores: usize,
}

pub(crate) trait Adapter: Sync {
    fn name(&self) -> &'static str;
    fn command(&self) -> Command;
    fn input_file(&self) -> (&'static str, &'static [&'static str]);
    fn input(
        &self,
        job: &Job,
        ratio: f64,
        stop_ha: f64,
        mode: Mode,
        datasets: &[(String, String, Vec<u8>)],
    ) -> Result<String, String>;
    fn step(&self) -> engine::StepLine;
    fn clock(&self, line: &str) -> Option<Clock>;
    fn unconverged(&self, line: &str) -> bool;
    fn pressure(&self, line: &str) -> Option<f64>;
    fn moments(&self, stdout: &str) -> Option<Vec<f64>>;
    fn valence(&self, bytes: &[u8]) -> Option<f64>;
    fn datasets(
        &self,
        root: &Path,
        prefix: &Path,
        cell: &Cell,
        jth: &[(String, Vec<u8>)],
    ) -> Result<Vec<(String, String, Vec<u8>)>, String>;
    fn calibration_layout(&self, cell: &Cell, cores: usize) -> Layout;
    fn candidates(&self, p: usize, kpoints: &[usize]) -> Vec<Layout>;
    fn mixings(&self) -> Vec<Vec<String>>;
    fn fitted(
        &self,
        layout: Layout,
        _cell: &Cell,
        _jth: &[(String, Vec<u8>)],
    ) -> Result<Layout, String> {
        Ok(layout)
    }
}

#[derive(
    Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum,
)]
#[serde(rename_all = "snake_case")]
pub enum Rival {
    Qe,
    Abinit,
}

impl Rival {
    pub(crate) fn adapter(self) -> &'static dyn Adapter {
        match self {
            Rival::Qe => &qe::Qe,
            Rival::Abinit => &abinit::Abinit,
        }
    }

    pub(crate) fn all() -> &'static [Rival] {
        <Rival as clap::ValueEnum>::value_variants()
    }

    pub(crate) fn pressure(self, stdout: &str) -> Option<f64> {
        (stdout.lines().rev()).find_map(|line| self.adapter().pressure(line))
    }

    pub(crate) fn calibration_layout(self, cell: &Cell) -> Layout {
        let cores = std::thread::available_parallelism().map_or(1, |n| n.get());
        self.adapter().calibration_layout(cell, cores)
    }
}

fn number(text: &str) -> Option<f64> {
    text.split_whitespace()
        .next()?
        .trim_end_matches(']')
        .parse::<f64>()
        .ok()
}

pub(crate) enum Clock {
    Run(f64),
    Since(f64),
    Step(f64),
}

fn float(value: &toml::Value) -> Option<f64> {
    value.as_float().or(value.as_integer().map(|i| i as f64))
}

const READ: [&str; 13] = [
    "structure.cell_bohr",
    "structure.species",
    "structure.positions_fractional",
    "xc.functional",
    "kpoints.mesh",
    "kpoints.shift",
    "kpoints.symmetry",
    "occupations.smearing",
    "occupations.width_ha",
    "spin.moments",
    "bands.count",
    "kpoints.list",
    "scf.max_iterations",
];
const UNUSED: [&str; 7] = [
    "schema",
    "basis",
    "datasets",
    "scf",
    "outputs",
    "parallel",
    "bands.solver",
];

#[derive(Clone, Copy)]
enum Smearing {
    FermiDirac,
    Gaussian,
    MethfesselPaxton,
    MarzariVanderbilt,
}

#[derive(Clone)]
pub(crate) struct Cell {
    cell: [[f64; 3]; 3],
    species: Vec<String>,
    positions: Vec<[f64; 3]>,
    mesh: [i64; 3],
    shifted: [bool; 3],
    symmetry: bool,
    smearing: Option<(Smearing, f64)>,
    moments: Option<Vec<f64>>,
    bands: Option<usize>,
    steps: Option<usize>,
}

impl Cell {
    pub(crate) fn parse(table: &toml::Table) -> Result<Cell, String> {
        for (name, value) in table {
            let keys: Vec<String> = match value.as_table() {
                Some(t) => t.keys().map(|k| format!("{name}.{k}")).collect(),
                None => vec![name.clone()],
            };
            if let Some(key) = keys.iter().find(|k| {
                !(READ.contains(&k.as_str())
                    || UNUSED
                        .iter()
                        .any(|u| k == u || k.starts_with(&format!("{u}."))))
            }) {
                return Err(format!(
                    "rivals: the input's {key} has no rival meaning; add it to rival.rs READ or UNUSED"
                ));
            }
        }
        let get = |path: &str| -> Option<&toml::Value> {
            let (t, k) = path.split_once('.')?;
            table.get(t)?.get(k)
        };
        let rows = |path: &str| -> Result<Vec<[f64; 3]>, String> {
            let bad = || format!("rivals: {path} must hold rows of 3 numbers");
            (get(path)
                .and_then(toml::Value::as_array)
                .ok_or_else(bad)?
                .iter())
            .map(|row| {
                let v: Option<Vec<f64>> =
                    row.as_array().and_then(|r| r.iter().map(float).collect());
                v.and_then(|v| <[f64; 3]>::try_from(v).ok()).ok_or_else(bad)
            })
            .collect()
        };
        let cell = <[[f64; 3]; 3]>::try_from(rows("structure.cell_bohr")?)
            .map_err(|_| "rivals: structure.cell_bohr needs 3 rows")?;
        let species: Vec<String> = (get("structure.species").and_then(toml::Value::as_array))
            .and_then(|s| s.iter().map(|v| v.as_str().map(str::to_owned)).collect())
            .ok_or("rivals: the input needs structure.species")?;
        let positions = rows("structure.positions_fractional")?;
        if get("xc.functional")
            .and_then(toml::Value::as_str)
            .is_some_and(|f| f != "pbe")
        {
            return Err(
                "rivals: their datasets are PBE; the input asks for another functional".into(),
            );
        }
        let mesh: Vec<i64> = (get("kpoints.mesh").and_then(toml::Value::as_array))
            .and_then(|m| m.iter().map(toml::Value::as_integer).collect())
            .ok_or("rivals: the input needs a kpoints.mesh")?;
        let mesh = <[i64; 3]>::try_from(mesh).map_err(|_| "rivals: kpoints.mesh needs 3 counts")?;
        let shift: Vec<f64> = match get("kpoints.shift").and_then(toml::Value::as_array) {
            Some(s) => s
                .iter()
                .map(float)
                .collect::<Option<_>>()
                .ok_or("rivals: kpoints.shift must hold numbers")?,
            None => vec![0.0; 3],
        };
        let shifted: Vec<bool> = (shift.iter())
            .map(|&s| match s {
                0.0 => Ok(false),
                0.5 => Ok(true),
                _ => Err(format!(
                    "rivals: kpoints.shift {s} is neither 0 nor 0.5 of a division"
                )),
            })
            .collect::<Result<_, _>>()?;
        let smearing = match get("occupations.smearing").and_then(toml::Value::as_str) {
            None | Some("fixed") => None,
            Some(name) => {
                let kind = match name {
                    "fermi-dirac" => Smearing::FermiDirac,
                    "gaussian" => Smearing::Gaussian,
                    "methfessel-paxton" => Smearing::MethfesselPaxton,
                    "marzari-vanderbilt" => Smearing::MarzariVanderbilt,
                    other => return Err(format!("rivals: no rival smearing for {other}")),
                };
                let width = get("occupations.width_ha")
                    .and_then(float)
                    .ok_or("rivals: a smearing needs width_ha")?;
                Some((kind, width))
            }
        };
        let moments = match get("spin.moments") {
            Some(m) => Some(
                (m.as_array().into_iter().flatten().map(float))
                    .collect::<Option<Vec<f64>>>()
                    .ok_or("rivals: spin.moments must hold numbers")?,
            ),
            None => None,
        };
        if positions.len() != species.len() {
            return Err(format!(
                "rivals: {} positions for {} species entries",
                positions.len(),
                species.len()
            ));
        }
        if let Some(m) = moments.as_ref().filter(|m| m.len() != species.len()) {
            return Err(format!(
                "rivals: {} spin.moments for {} species entries",
                m.len(),
                species.len()
            ));
        }
        let count = |path: &str| match get(path).and_then(toml::Value::as_integer) {
            None => Ok(None),
            Some(n) => (usize::try_from(n).ok().filter(|&n| n > 0))
                .map(Some)
                .ok_or(format!("rivals: {path} {n} is not a positive count")),
        };
        let (bands, steps) = (count("bands.count")?, count("scf.max_iterations")?);
        Ok(Cell {
            cell,
            species,
            positions,
            mesh,
            shifted: <[bool; 3]>::try_from(shifted)
                .map_err(|_| "rivals: kpoints.shift needs 3 numbers")?,
            symmetry: get("kpoints.symmetry")
                .and_then(toml::Value::as_bool)
                .unwrap_or(true),
            smearing,
            moments,
            bands,
            steps,
        })
    }

    pub(crate) fn jth_electrons(&self, jth: &[(String, Vec<u8>)]) -> Result<f64, String> {
        (self.species.iter())
            .map(|e| jth_valence(jth, e).map(|(_, v)| v))
            .sum()
    }

    pub(crate) fn atoms(&self) -> usize {
        self.species.len()
    }

    pub(crate) fn same_state(&self, moments: Option<&[f64]>, converged: &[f64]) -> bool {
        let Some(start) = &self.moments else {
            return true;
        };
        (start.iter().enumerate())
            .filter(|(_, s)| **s != 0.0)
            .all(
                |(i, _)| match (moments.and_then(|m| m.get(i)), converged.get(i)) {
                    (Some(m), Some(c)) => m * c > c * c / 2.0,
                    _ => false,
                },
            )
    }

    pub(crate) fn scaled(&self, volume_scale: f64) -> Cell {
        let s = volume_scale.cbrt();
        Cell {
            cell: self.cell.map(|row| row.map(|x| x * s)),
            ..self.clone()
        }
    }

    pub(crate) fn mesh_points(&self) -> usize {
        self.mesh.iter().product::<i64>() as usize
    }

    pub(crate) fn k_spins(&self) -> usize {
        self.mesh_points() * if self.moments.is_some() { 2 } else { 1 }
    }

    pub(crate) fn elements(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = Vec::new();
        for s in &self.species {
            if !seen.contains(&s.as_str()) {
                seen.push(s);
            }
        }
        seen
    }

    fn types(&self) -> (Vec<(String, String, f64)>, Vec<usize>) {
        let mut types: Vec<(String, String, f64)> = Vec::new();
        let mut of = Vec::new();
        for (i, element) in self.species.iter().enumerate() {
            let m = self.moments.as_ref().map_or(0.0, |m| m[i]);
            let at = match types.iter().position(|(_, e, mt)| e == element && *mt == m) {
                Some(at) => at,
                None => {
                    let k = types.iter().filter(|(_, e, _)| e == element).count();
                    let label = if k == 0 {
                        element.clone()
                    } else {
                        format!("{element}{k}")
                    };
                    types.push((label, element.clone(), m));
                    types.len() - 1
                }
            };
            of.push(at);
        }
        (types, of)
    }
}

fn rows(out: &mut String, rows: &[[f64; 3]]) {
    for [x, y, z] in rows {
        let _ = writeln!(out, "  {x:?} {y:?} {z:?}");
    }
}

fn attribute(bytes: &[u8], tag: &str, name: &str) -> Option<f64> {
    let text = std::str::from_utf8(bytes).ok()?;
    let doc = roxmltree::Document::parse(text).ok()?;
    let node = doc.descendants().find(|n| n.has_tag_name(tag))?;
    node.attribute(name)?.trim().parse().ok()
}

pub(crate) const JTH_FAMILY: &str = "JTH-PBE-v2.0";

pub(crate) fn jth_file<'a>(
    jth: &'a [(String, Vec<u8>)],
    element: &str,
) -> Result<&'a (String, Vec<u8>), String> {
    (jth.iter())
        .find(|(path, _)| {
            path.split('/').next() == Some(JTH_FAMILY)
                && super::datasets::element(path) == Some(element)
        })
        .ok_or_else(|| {
            format!(
                "{}/{}.toml holds no {JTH_FAMILY} file for {element}",
                super::datasets::FAMILIES,
                super::datasets::DEFAULT_FAMILY
            )
        })
}

fn jth_valence<'a>(
    jth: &'a [(String, Vec<u8>)],
    element: &str,
) -> Result<(&'a (String, Vec<u8>), f64), String> {
    let file = jth_file(jth, element)?;
    let valence = (attribute(&file.1, "atom", "valence"))
        .ok_or_else(|| format!("{}: no <atom valence>", file.0))?;
    Ok((file, valence))
}

fn same_valence(
    code: Rival,
    cell: &Cell,
    datasets: &[(String, String, Vec<u8>)],
    jth: &[(String, Vec<u8>)],
) -> Result<(), Failure> {
    let name = code.adapter().name();
    let fail = |why: String| Failure::Harness(format!("rivals: {why}"));
    let mut each = Vec::new();
    for e in cell.elements() {
        let (_, file, bytes) = (datasets.iter())
            .find(|(element, _, _)| element == e)
            .ok_or_else(|| fail(format!("{name} has no dataset for {e}")))?;
        let theirs = (code.adapter().valence(bytes))
            .ok_or_else(|| fail(format!("{file}: {name} reads no valence in it")))?;
        let ((path, _), jths) = jth_valence(jth, e).map_err(fail)?;
        let atoms = cell.species.iter().filter(|s| *s == e).count() as f64;
        each.push((e, atoms, (file, theirs), (path, jths)));
    }
    type Each<'a> = (&'a str, f64, (&'a String, f64), (&'a String, f64));
    let total = |pick: &dyn Fn(&Each) -> f64| each.iter().map(|x| x.1 * pick(x)).sum::<f64>();
    let (theirs, jths) = (total(&|x| x.2.1), total(&|x| x.3.1));
    if theirs == jths {
        return Ok(());
    }
    let differ: Vec<String> = (each.iter())
        .filter(|(_, _, (_, a), (_, b))| a != b)
        .map(|(e, _, (file, a), (path, b))| format!("{e} {a} in {file}, {b} in {path}"))
        .collect();
    Err(Failure::Engine(format!(
        "the cell holds {theirs} valence electrons in {}'s datasets and {jths} in JTH's: {}",
        code.adapter().name(),
        differ.join("; ")
    )))
}

fn cell_cutoff(frozen: &BTreeMap<String, f64>, code: Rival, cell: &Cell) -> Result<f64, String> {
    (cell.elements().into_iter())
        .map(|e| {
            frozen.get(e).copied().ok_or_else(|| {
                format!(
                    "{TUNED} has no {} cutoff for {e}; run `cargo xtask rivals calibrate` on the node",
                    code.adapter().name()
                )
            })
        })
        .try_fold(0.0_f64, |m, c| Ok(m.max(c?)))
}

pub(crate) struct ClaimCutoff {
    cutoff_ha: BTreeMap<String, f64>,
    density_ratio: f64,
}

#[derive(Deserialize)]
struct CutoffsOnly {
    #[serde(default)]
    abinit: Option<AbinitCutoffs>,
}

#[derive(Deserialize)]
struct AbinitCutoffs {
    #[serde(default)]
    cutoff_ha: BTreeMap<String, f64>,
}

impl ClaimCutoff {
    pub(crate) fn load(root: &Path) -> Result<ClaimCutoff, String> {
        let text = fs::read_to_string(root.join(TUNED)).map_err(|e| format!("{TUNED}: {e}"))?;
        let only: CutoffsOnly = toml::from_str(&text).map_err(|e| format!("{TUNED}: {e}"))?;
        Ok(ClaimCutoff {
            cutoff_ha: only.abinit.map(|a| a.cutoff_ha).unwrap_or_default(),
            density_ratio: settings(root)?.setup(Rival::Abinit).density_ratio,
        })
    }

    pub(crate) fn of(&self, cell: &Cell) -> Result<(f64, f64), String> {
        let cutoff = cell_cutoff(&self.cutoff_ha, Rival::Abinit, cell)?;
        Ok((cutoff, cutoff * self.density_ratio))
    }
}

impl Rivals {
    pub(crate) fn load(root: &Path, prefix: &Path) -> Result<Rivals, String> {
        let settings = settings(root)?;
        let tuned = match fs::read_to_string(root.join(TUNED)) {
            Ok(text) => toml::from_str(&text).map_err(|e| format!("{TUNED}: {e}"))?,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Tuned::default(),
            Err(e) => return Err(format!("{TUNED}: {e}")),
        };
        for &code in Rival::all() {
            let frozen = tuned.of(code);
            let scale = settings.target.volume_scale;
            if !frozen.calibration.is_empty() && frozen.volume_scale != Some(scale) {
                return Err(format!(
                    "{TUNED}: the {} calibration ran at volume_scale {:?}, but {SETTINGS} asks for {scale}; recalibrate: \
                     remove its rows and run `cargo xtask rivals calibrate`",
                    code.adapter().name(),
                    frozen.volume_scale
                ));
            }
            let setup = settings.setup(code);
            let ascending = setup.ladder_ha.windows(2).all(|w| w[0] < w[1]);
            if !ascending
                || setup
                    .ladder_ha
                    .last()
                    .is_some_and(|&top| top >= setup.converged_ha)
            {
                return Err(format!(
                    "{SETTINGS}: [{}] ladder_ha must ascend and stay below converged_ha",
                    code.adapter().name()
                ));
            }
        }
        Ok(Rivals {
            root: root.to_owned(),
            prefix: prefix.to_owned(),
            settings,
            tuned,
        })
    }

    pub(crate) fn cutoff(&self, code: Rival, cell: &Cell) -> Result<f64, String> {
        cell_cutoff(&self.tuned.of(code).cutoff_ha, code, cell)
    }

    pub(crate) fn claim_cutoff(&self) -> ClaimCutoff {
        let abinit = Rival::Abinit;
        ClaimCutoff {
            cutoff_ha: self.tuned.of(abinit).cutoff_ha.clone(),
            density_ratio: self.settings.setup(abinit).density_ratio,
        }
    }

    pub(crate) fn calibrated(
        &self,
        code: Rival,
        path: &str,
        cell: &Cell,
        cutoff: f64,
    ) -> Result<Option<(Row, String)>, String> {
        let rows = &self.tuned.of(code).calibration;
        let at = |cell: &str| {
            (rows.get(cell).into_iter().flatten())
                .find(|r| r[0] == cutoff && !r[1].is_nan())
                .map(|r| (*r, cell.to_owned()))
        };
        if rows.contains_key(path) {
            return Ok(at(path));
        }
        let (mut worst, mut worst_p): (Option<(Row, String)>, f64) = (None, f64::NEG_INFINITY);
        for other in rows.keys() {
            let text =
                fs::read_to_string(self.root.join(other)).map_err(|e| format!("{other}: {e}"))?;
            let table = toml::from_str(&text).map_err(|e| format!("{other}: {e}"))?;
            let held = Cell::parse(&table)?;
            if !held.elements().iter().all(|e| cell.elements().contains(e)) {
                continue;
            }
            if let Some(r) = (rows.get(other).into_iter().flatten()).find(|r| r[0] == cutoff) {
                worst_p = match r[2].is_nan() || worst_p.is_nan() {
                    true => f64::NAN,
                    false => worst_p.max(r[2]),
                };
            }
            if let Some(row) = at(other)
                && worst.as_ref().is_none_or(|w| row.0[1] > w.0[1])
            {
                worst = Some(row);
            }
        }
        Ok(worst.map(|(mut row, from)| {
            row[2] = worst_p;
            (row, from)
        }))
    }

    pub(crate) fn layout(&self, code: Rival, threads: usize) -> Result<Layout, String> {
        (self.tuned.of(code).layout.get(&threads.to_string())).cloned().ok_or_else(|| {
            format!("{TUNED} has no {} layout at {threads} threads; run `cargo xtask rivals layout` on the node", code.adapter().name())
        })
    }

    pub(crate) fn job(
        &self,
        code: Rival,
        cell: Cell,
        threads: usize,
        jth: &[(String, Vec<u8>)],
    ) -> Result<Job, String> {
        let layout = self.layout(code, threads)?;
        let kpoints = layout_kpoints(&self.root, &self.settings, code)?;
        let policy = layout_policy(code, &self.settings.threads, &kpoints);
        let frozen = self.tuned.of(code).layout_policy.as_deref();
        if frozen != Some(policy.as_str()) {
            return Err(format!(
                "{TUNED}: the {} layouts were searched under another candidate set or thread policy ({} here, {} frozen); run `cargo xtask rivals layout` on the node",
                code.adapter().name(),
                policy,
                frozen.unwrap_or("none")
            ));
        }
        if layout.ranks * layout.threads != threads {
            return Err(format!(
                "{TUNED}: the {} layout at {threads} threads runs {} ranks × {} threads",
                code.adapter().name(),
                layout.ranks,
                layout.threads
            ));
        }
        let setup = self.settings.setup(code);
        let claim = ClaimPart {
            files: self.pins(code, &cell, jth)?,
            floor_ha: setup.stop_floor_ha,
            busy_cores: layout.ranks * if setup.openmp { layout.threads } else { 1 },
        };
        Ok(Job {
            claim: Some(claim),
            ..self.tune_job(code, &cell, self.cutoff(code, &cell)?, layout, jth)?
        })
    }

    pub(crate) fn tune_job(
        &self,
        code: Rival,
        cell: &Cell,
        cutoff_ha: f64,
        layout: Layout,
        jth: &[(String, Vec<u8>)],
    ) -> Result<Job, String> {
        let layout = code.adapter().fitted(layout, cell, jth)?;
        let mpiexec = match layout.ranks > 1 {
            true => self.settings.setup(code).mpiexec.clone(),
            false => Vec::new(),
        };
        Ok(Job {
            code,
            cell: cell.clone(),
            cutoff_ha,
            layout,
            mixing: self.tuned.of(code).mixing.input.clone(),
            mpiexec,
            claim: None,
        })
    }

    pub(crate) fn run(
        &self,
        job: &Job,
        jth: &[(String, Vec<u8>)],
        stop_ha: f64,
        mode: Mode,
        clocks: Clocks,
        timeout: Duration,
    ) -> Result<Run, Failure> {
        let (code, adapter) = (job.code, job.code.adapter());
        let datasets = (adapter.datasets(&self.root, &self.prefix, &job.cell, jth))
            .map_err(Failure::Harness)?;
        same_valence(code, &job.cell, &datasets, jth)?;
        let ratio = self.settings.setup(code).density_ratio;
        let (name, _) = adapter.input_file();
        let text =
            (adapter.input(job, ratio, stop_ha, mode, &datasets)).map_err(Failure::Harness)?;
        let files: Vec<(String, &[u8])> = std::iter::once((name.to_owned(), text.as_bytes()))
            .chain((datasets.iter()).map(|(_, path, bytes)| (path.clone(), bytes.as_slice())))
            .collect();
        let build = |_: &Path| Ok(self.command(job));
        engine::cold(
            &files,
            build,
            Stream::Stdout,
            code.adapter().step(),
            timeout,
        )
        .and_then(|run| agreed(code, run, clocks, job.cell.steps))
    }

    pub(crate) fn pins(
        &self,
        code: Rival,
        cell: &Cell,
        jth: &[(String, Vec<u8>)],
    ) -> Result<BTreeMap<String, String>, String> {
        Ok((code
            .adapter()
            .datasets(&self.root, &self.prefix, cell, jth)?
            .into_iter())
        .map(|(_, path, bytes)| (path, crate::lock::hex(&sha2::Sha256::digest(&bytes))))
        .collect())
    }

    fn command(&self, job: &Job) -> Command {
        let (adapter, layout) = (job.code.adapter(), &job.layout);
        let bin = self.prefix.join(adapter.name()).join("bin");
        let mut command = match layout.ranks > 1 {
            true => {
                let mut c = Command::new("mpiexec");
                c.args(&job.mpiexec)
                    .arg("-n")
                    .arg(layout.ranks.to_string())
                    .arg(bin.join(adapter.command().get_program()));
                c
            }
            false => adapter.command(),
        };
        command.args(&layout.args).args(adapter.input_file().1);
        command
            .env("PATH", format!("{}:/usr/bin:/bin", bin.display()))
            .env("GFORTRAN_UNBUFFERED_PRECONNECTED", "y")
            .envs(thread_env(layout));
        command
    }
}

fn agreed(code: Rival, run: Run, clocks: Clocks, cap: Option<usize>) -> Result<Run, Failure> {
    let name = code.adapter().name();
    let lost = |why: String| Err(Failure::Engine(format!("{name}: {why}")));
    for (_, line) in &run.lines {
        if code.adapter().unconverged(line) {
            let changes: Vec<String> =
                (run.steps.windows(2).skip(run.steps.len().saturating_sub(5)))
                    .map(|w| format!("{:.1e}", w[1].free_energy_ha - w[0].free_energy_ha))
                    .collect();
            let why = format!(
                "its SCF stopped unconverged: {} ({} steps; the last energy changes {} Ha)",
                line.trim(),
                run.steps.len(),
                changes.join(", ")
            );
            return match cap {
                Some(cap) => Err(Failure::Harness(format!(
                    "{name}: {why}, at the input's step cap scf.max_iterations = {cap}, which every code shares: \
                     the cap, not {name}, stopped it, so it is no loss; raise the cap"
                ))),
                None => lost(why),
            };
        }
    }
    match (clocked(code, &run), clocks) {
        (Ok(()), _) => Ok(run),
        (Err(why), Clocks::Loss) => lost(why),
        (Err(why), Clocks::Warning) => {
            println!("rivals: {}: warning: {why}", code.adapter().name());
            Ok(run)
        }
    }
}

fn clocked(code: Rival, run: &Run) -> Result<(), String> {
    let first = run.lines.first().map_or(0.0, |(at, _)| *at);
    let (mut own, mut since) = (None, None);
    let mut steps: Vec<(f64, Option<f64>)> = Vec::new();
    let mut previous: Option<f64> = None;
    for (at, line) in &run.lines {
        match code.adapter().clock(line.trim()) {
            Some(Clock::Run(seconds)) => own = own.or(Some((*at, seconds))),
            Some(Clock::Since(seconds)) => since = Some(seconds),
            Some(Clock::Step(seconds)) => {
                if let Some(last) = steps.last_mut() {
                    last.1 = Some(seconds);
                }
            }
            None if (code.adapter().step())(line).is_some() => {
                let took = since
                    .take()
                    .and_then(|now| Some(now - previous.replace(now)?));
                steps.push((*at, took));
            }
            None => {}
        }
    }
    let (at, own) = own.ok_or("no line of its own wall clock")?;
    let span = at - first;
    if (span - own).abs() > CLOCK_SLACK_S + CLOCK_SHARE * own {
        return Err(format!(
            "its lines span {span:.3} s, but its own clock reads {own:.3} s: its output buffers"
        ));
    }
    for (k, pair) in steps.windows(2).enumerate() {
        let gap = pair[1].0 - pair[0].0;
        if let Some(took) = pair[1].1
            && gap < STEP_SHARE * took - STEP_SLACK_S
        {
            return Err(format!(
                "its step {} arrived {gap:.3} s after the one before, but its own clock gives {took:.3} s: its output buffers",
                k + 2
            ));
        }
    }
    Ok(())
}
