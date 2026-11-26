use super::{
    Evidence, datasets,
    eos::{self, Bm},
};
use nalgebra::{Matrix3, Vector3};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};
use toml::Value;

pub const SCALES: [f64; 7] = [0.94, 0.96, 0.98, 1.00, 1.02, 1.04, 1.06];
pub const BOHR_ANGSTROM: f64 = 0.52917720859;
pub const HARTREE_EV: f64 = 27.211386245988;
const KPOINT_SPACING: f64 = 0.06;
const CUTOFF_MARGIN_HA: f64 = 20.0;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Crystal {
    cell: [[f64; 3]; 3],
    sites: Vec<(String, [f64; 3])>,
    kinds: BTreeMap<String, String>,
}

#[derive(Debug)]
pub struct Comparison {
    pub fit: Bm,
    pub epsilon: f64,
    pub nu: f64,
}

#[derive(Deserialize)]
struct Results {
    #[serde(rename = "BM_fit_data")]
    fits: BTreeMap<String, Option<Published>>,
    num_atoms_in_sim_cell: BTreeMap<String, usize>,
}

#[derive(Deserialize)]
struct Published {
    min_volume: f64,
    bulk_modulus_ev_ang3: f64,
    bulk_deriv: f64,
}

pub fn published(text: &str) -> Result<impl Fn(&str) -> Result<Bm, String> + use<>, String> {
    let results: Results = serde_json::from_str(text).map_err(|e| e.to_string())?;
    let mut fits: BTreeMap<String, Result<Bm, String>> = BTreeMap::new();
    for (system, fit) in results.fits {
        let atoms = results.num_atoms_in_sim_cell.get(&system).copied();
        let bm = match (fit, atoms) {
            (_, None) => Err(format!("has no num_atoms_in_sim_cell of {system}")),
            (None, _) => Err(format!("has no fit of {system}")),
            (Some(fit), Some(atoms)) => Ok(Bm {
                v0_ang3: fit.min_volume / atoms as f64,
                b0_ev_ang3: fit.bulk_modulus_ev_ang3,
                b1: fit.bulk_deriv,
            }),
        };
        fits.insert(system, bm);
    }
    Ok(move |system: &str| match fits.get(system) {
        Some(Ok(bm)) => Ok(*bm),
        Some(Err(e)) => Err(e.clone()),
        None => Err(format!("has no fit of {system}")),
    })
}

pub fn systems(text: &str) -> Result<Vec<String>, String> {
    let results: Results = serde_json::from_str(text).map_err(|e| e.to_string())?;
    Ok(results.fits.into_keys().collect())
}

const SPLIT: &str = "benchmarks/ACWF.toml";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    Tune,
    Exam,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Split {
    #[serde(rename = "part")]
    types: BTreeMap<String, Part>,
    flagged: BTreeSet<String>,
}

impl Split {
    pub fn read(root: &Path) -> Result<Split, String> {
        let text =
            std::fs::read_to_string(root.join(SPLIT)).map_err(|e| format!("{SPLIT}: {e}"))?;
        let split: Split = toml::from_str(&text).map_err(|e| format!("{SPLIT}: {e}"))?;
        for system in &split.flagged {
            if split.part(system)? != Part::Exam {
                return Err(format!("{SPLIT}: flagged {system} is not in the exam part"));
            }
        }
        Ok(split)
    }

    pub fn part(&self, system: &str) -> Result<Part, String> {
        let (_, structure) = (system.split_once('-'))
            .filter(|(element, _)| !element.is_empty())
            .ok_or_else(|| format!("{system} is not <element>-<structure type>"))?;
        (self.types.get(structure).copied())
            .ok_or_else(|| format!("{system}: structure type {structure} is in no part of {SPLIT}"))
    }

    pub fn blind<'a>(&self, names: impl IntoIterator<Item = &'a str>) -> Result<(), String> {
        let mut exam = BTreeSet::new();
        for name in names {
            let system = system_of(name).unwrap_or(name);
            if system.contains('-') && self.part(system)? == Part::Exam {
                exam.insert(system);
            }
        }
        match exam.is_empty() {
            true => Ok(()),
            false => Err(format!(
                "{} in the ACWF exam part, blind until M5 judges it; the tuning tooling refuses it",
                exam.into_iter().collect::<Vec<_>>().join(", ")
            )),
        }
    }

    pub fn flagged(&self) -> &BTreeSet<String> {
        &self.flagged
    }
}

pub fn system_of(name: &str) -> Option<&str> {
    name.rsplit_once('@').map(|(system, _)| system)
}

pub fn inputs(
    crystals_json: &str,
    hints: &BTreeMap<String, f64>,
) -> Result<Vec<(String, String)>, String> {
    let crystals: BTreeMap<String, Crystal> =
        serde_json::from_str(crystals_json).map_err(|e| format!("crystals.json: {e}"))?;
    let mut out = Vec::new();
    for (system, crystal) in crystals {
        out.extend(
            crystal
                .inputs(&system, hints)
                .map_err(|e| format!("{system}: {e}"))?,
        );
    }
    Ok(out)
}

pub(crate) fn expand(
    root: &Path,
    text: &str,
    table: Option<&datasets::Table>,
) -> Result<Vec<(String, String)>, String> {
    let Some(table) = table else {
        return inputs(
            text,
            &datasets::hints(root, datasets::DEFAULT_FAMILY, datasets::ACWF_FUNCTIONAL)?,
        );
    };
    let crystals: BTreeMap<String, Crystal> =
        serde_json::from_str(text).map_err(|e| format!("crystals.json: {e}"))?;
    let hints = table.hints();
    for (system, crystal) in &crystals {
        if let Some(element) = (crystal.species()?.into_iter()).find(|e| !hints.contains_key(*e)) {
            return Err(format!("{system}: the dataset table has no {element}"));
        }
    }
    inputs(text, &hints)
}

impl Crystal {
    fn lattice(&self) -> Matrix3<f64> {
        Matrix3::from_fn(|i, j| self.cell[i][j])
    }

    fn mesh(inverse: &Matrix3<f64>) -> [i64; 3] {
        [0, 1, 2].map(|i| {
            let b = 2.0 * std::f64::consts::PI * inverse.column(i).norm();
            ((b / KPOINT_SPACING).ceil() as i64).max(1)
        })
    }

    fn species(&self) -> Result<Vec<&str>, String> {
        (self.sites.iter())
            .map(|(kind, _)| {
                (self.kinds.get(kind).map(String::as_str))
                    .ok_or_else(|| format!("site kind {kind} has no element in kinds"))
            })
            .collect()
    }

    fn cutoff_ha(&self, hints: &BTreeMap<String, f64>) -> Result<f64, String> {
        let highest = (hints.values().copied())
            .reduce(f64::max)
            .ok_or("no cutoff hints")?;
        let hint = (self.species()?.into_iter())
            .map(|element| hints.get(element).copied().unwrap_or(highest))
            .fold(f64::NEG_INFINITY, f64::max);
        Ok(hint + CUTOFF_MARGIN_HA)
    }

    fn inputs(
        &self,
        system: &str,
        hints: &BTreeMap<String, f64>,
    ) -> Result<Vec<(String, String)>, String> {
        let species = self.species()?;
        let cutoff = self.cutoff_ha(hints)?;
        let lattice = self.lattice();
        let inverse = lattice.try_inverse().ok_or("the cell is singular")?;
        let fractional: Vec<[f64; 3]> = (self.sites.iter())
            .map(|(_, r)| (inverse.transpose() * Vector3::from(*r)).into())
            .collect();
        let mesh = Crystal::mesh(&inverse);
        SCALES
            .iter()
            .map(|scale| {
                let cell = lattice * (scale.cbrt() / BOHR_ANGSTROM);
                let rows: Vec<[f64; 3]> = cell.row_iter().map(|r| [r[0], r[1], r[2]]).collect();
                let input = table([
                    ("schema", Value::Integer(1)),
                    (
                        "structure",
                        table([
                            ("cell_bohr", value(&rows)?),
                            ("species", value(&species)?),
                            ("positions_fractional", value(&fractional)?),
                        ]),
                    ),
                    (
                        "basis",
                        table([
                            ("cutoff_ha", Value::Float(cutoff)),
                            ("density_cutoff_ha", Value::Float(2.0 * cutoff)),
                        ]),
                    ),
                    ("kpoints", table([("mesh", value(&mesh)?)])),
                    (
                        "xc",
                        table([("functional", value(&datasets::ACWF_FUNCTIONAL)?)]),
                    ),
                    (
                        "datasets",
                        table([("family", value(&datasets::DEFAULT_FAMILY)?)]),
                    ),
                    (
                        "occupations",
                        table([
                            ("smearing", value(&"fermi-dirac")?),
                            ("width_ha", Value::Float(0.00225)),
                        ]),
                    ),
                    (
                        "scf",
                        table([
                            ("energy_tolerance_ha", Value::Float(1e-9)),
                            ("max_iterations", Value::Integer(500)),
                        ]),
                    ),
                ]);
                let text = toml::to_string(&input).map_err(|e| format!("{system}: {e}"))?;
                Ok((format!("{system}@{scale:.2}"), text))
            })
            .collect()
    }
}

fn table<const N: usize>(entries: [(&str, Value); N]) -> Value {
    Value::Table(
        entries
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
    )
}

fn value<T: serde::Serialize + ?Sized>(x: &T) -> Result<Value, String> {
    Value::try_from(x).map_err(|e| e.to_string())
}

#[derive(Debug)]
pub struct Curve {
    pub atoms: usize,
    pub points: Vec<(f64, f64)>,
}

impl Evidence {
    pub fn curves(&self) -> Result<BTreeMap<String, Curve>, String> {
        let mut curves: BTreeMap<String, Curve> = BTreeMap::new();
        for name in self.names() {
            let Some(system) = system_of(name) else {
                continue;
            };
            let input = self.input(name)?;
            let cell = input.cell_bohr()?;
            let curve = curves.entry(system.to_owned()).or_insert(Curve {
                atoms: input.species()?.len(),
                points: Vec::new(),
            });
            curve.points.push((
                Matrix3::from_fn(|i, j| cell[i][j]).determinant().abs() * BOHR_ANGSTROM.powi(3),
                self.output(name)?.energy_ha("free_energy_ha")? * HARTREE_EV,
            ));
        }
        Ok(curves)
    }

    pub fn acwf(&self, file: &str) -> Result<BTreeMap<String, Comparison>, String> {
        let fits =
            published(&self.reference(file)?).map_err(|e| format!("reference/{file}: {e}"))?;
        (self.curves()?.into_iter())
            .map(|(system, Curve { atoms, points })| {
                let fit = eos::fit(&points).map_err(|e| format!("{system}: {e}"))?;
                let theirs = fits(&system).map_err(|e| format!("reference/{file}: {e}"))?;
                let ours = fit.per(atoms as f64);
                let comparison = Comparison {
                    fit,
                    epsilon: eos::epsilon(&ours, &theirs),
                    nu: eos::nu(&ours, &theirs),
                };
                Ok((system, comparison))
            })
            .collect()
    }
}

const SHARE_EPSILON: f64 = 0.06;
const LOST_EPSILON: f64 = 1.0;
const AE: &str = "acwf-ae-average.json";
const VASP: &str = "acwf-vasp.json";

type Ran = BTreeMap<String, Result<(f64, f64), String>>;

fn whose(records: &serde_json::Value, own: &serde_json::Value) -> String {
    match records {
        serde_json::Value::Null => "none".to_owned(),
        sha if sha == own => format!("{sha} (the case's own)"),
        sha => format!("{sha} (a candidate, not the case's own)"),
    }
}

pub fn readout(
    root: &Path,
    id: &str,
    dir: &Path,
    baseline: Option<&Path>,
    selectors: &[String],
) -> Result<String, String> {
    use std::fmt::Write;
    let mut out = String::new();
    let split = Split::read(root)?;
    split.blind(selectors.iter().map(String::as_str))?;
    let case = root.join("benchmarks").join(id);
    let (inputs, _) = super::inputs(root, &case, None)?;
    let names: Vec<&str> = inputs.iter().map(|(name, _)| name.as_str()).collect();
    split.blind(names.iter().copied())?;
    let chosen = match selectors {
        [] => names.iter().map(|n| (*n).to_owned()).collect(),
        _ => super::selected(&names, selectors).map_err(|e| format!("the selector list {e}"))?,
    };
    let systems: BTreeSet<&str> = chosen.iter().filter_map(|n| system_of(n)).collect();
    let reference = |file: &str| {
        let path = case.join("reference").join(file);
        std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let (ae, vasp) = (published(&reference(AE)?)?, published(&reference(VASP)?)?);
    let own = super::datasets::Table::of_case(root, &case, None)?
        .map_or(serde_json::Value::Null, |t| t.named()["sha256"].clone());
    let (ours, table) = ran(&split, id, dir)?;
    let theirs = baseline.map(|b| ran(&split, id, b)).transpose()?;
    let lost = |r: Option<&Result<(f64, f64), String>>| match r {
        Some(Ok((epsilon, _))) => *epsilon,
        _ => f64::INFINITY,
    };
    let mut rows: Vec<(&str, [f64; 3])> = Vec::new();
    let _ = writeln!(
        out,
        "epsilon: {id}, {} crystals; records {} at table {}",
        systems.len(),
        dir.display(),
        whose(&table, &own)
    );
    if let (Some(b), Some((_, table))) = (baseline, &theirs) {
        let _ = writeln!(
            out,
            "baseline {} at table {}",
            b.display(),
            whose(table, &own)
        );
    }
    let _ = writeln!(
        out,
        "system  orbitar_epsilon  orbitar_nu  vasp_epsilon  baseline_epsilon"
    );
    for &system in &systems {
        let vasp = (vasp(system).ok())
            .zip(ae(system).ok())
            .map_or(f64::INFINITY, |(v, a)| eos::epsilon(&v, &a));
        let mine = ours.get(system);
        let base = theirs
            .as_ref()
            .map_or(f64::NAN, |(t, _)| lost(t.get(system)));
        let nu = match mine {
            Some(Ok((_, nu))) => *nu,
            _ => f64::NAN,
        };
        let _ = writeln!(
            out,
            "{system}  {:.4}  {nu:.4}  {vasp:.4}  {base:.4}",
            lost(mine)
        );
        if let Some(Err(why)) = mine {
            let _ = writeln!(out, "  lost {system}: {why}");
        } else if mine.is_none() {
            let _ = writeln!(
                out,
                "  lost {system}: no record in {} ran it",
                dir.display()
            );
        }
        rows.push((system, [lost(mine), vasp, base]));
    }
    let mut groups: BTreeMap<&str, Vec<[f64; 3]>> = BTreeMap::new();
    for (system, row) in &rows {
        let element = system.split_once('-').map_or(*system, |(e, _)| e);
        groups.entry(element).or_default().push(*row);
    }
    let all: Vec<[f64; 3]> = rows.iter().map(|(_, row)| *row).collect();
    for (name, group) in groups.iter().map(|(e, g)| (*e, g)).chain([("set", &all)]) {
        let stats: Vec<String> = (["orbitar", "vasp", "baseline"].into_iter().enumerate())
            .filter(|(i, _)| *i < 2 || baseline.is_some())
            .map(|(i, code)| {
                let values: Vec<f64> = group.iter().map(|row| row[i]).collect();
                let share = values.iter().filter(|&&e| e <= SHARE_EPSILON).count();
                let above = values.iter().filter(|&&e| e > LOST_EPSILON).count();
                format!(
                    "{code} median {:.4} share {:.2} above_1 {above}",
                    super::speed::median(&values),
                    share as f64 / values.len() as f64
                )
            })
            .collect();
        let _ = writeln!(out, "{name} ({}): {}", group.len(), stats.join(" · "));
    }
    Ok(out)
}

fn ran(split: &Split, id: &str, dir: &Path) -> Result<(Ran, serde_json::Value), String> {
    let mut files: Vec<_> = (std::fs::read_dir(dir)
        .map_err(|e| format!("{}: {e}", dir.display()))?)
    .flatten()
    .map(|e| e.path())
    .filter(|p| p.extension().is_some_and(|x| x == "json"))
    .collect();
    files.sort();
    let (mut out, mut runs, mut sha) = (Ran::new(), BTreeSet::new(), serde_json::Value::Null);
    for path in files {
        let at = path.display();
        let text = std::fs::read_to_string(&path).map_err(|e| format!("{at}: {e}"))?;
        let record: serde_json::Value =
            serde_json::from_str(&text).map_err(|e| format!("{at}: {e}"))?;
        if record["case"].as_str() != Some(id) {
            continue;
        }
        let inputs: Vec<&str> = (record["inputs"].as_array().into_iter().flatten())
            .filter_map(serde_json::Value::as_str)
            .collect();
        split
            .blind(inputs.iter().copied())
            .map_err(|e| format!("{at}: {e}"))?;
        let table = &record["table"]["sha256"];
        sha = table.clone();
        runs.insert([&record["commit"], &record["engine_sha256"], table].map(|v| v.to_string()));
        if runs.len() > 1 {
            return Err(format!(
                "{}: its records differ in commit, engine or dataset table: {runs:?}",
                dir.display()
            ));
        }
        let why = (record["failures"]
            .get(0)
            .and_then(serde_json::Value::as_str))
        .unwrap_or("no failure");
        for system in inputs
            .iter()
            .filter_map(|n| system_of(n))
            .collect::<BTreeSet<_>>()
        {
            let metric = |key: &str| record["metrics"][format!("{key}/{system}")].as_f64();
            let volumes = (SCALES.iter())
                .filter(|s| inputs.contains(&format!("{system}@{s:.2}").as_str()))
                .count();
            let row = match (metric("epsilon"), metric("nu")) {
                _ if volumes < SCALES.len() => Err(format!(
                    "{at} holds {volumes} of {} volumes of {system}",
                    SCALES.len()
                )),
                (Some(epsilon), Some(nu)) => Ok((epsilon, nu)),
                _ => Err(format!(
                    "{at} holds no ε (outcome {}: {why})",
                    record["outcome"]
                )),
            };
            if out.insert(system.to_owned(), row).is_some() {
                return Err(format!(
                    "{}: {system} runs in more than one record",
                    dir.display()
                ));
            }
        }
    }
    Ok((out, sha))
}
