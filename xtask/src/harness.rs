pub mod acwf;
pub(crate) mod datasets;
pub(crate) mod engine;
pub mod eos;
mod record;
pub mod relax;
pub(crate) mod rival;
pub(crate) mod speed;

pub use crate::case::Tier;
pub use datasets::agree;
pub use engine::{SHIM, end_group, polled, shim};

use crate::case::{Case, Claim, Status};
use libtest_mimic::{Arguments, Failed, Trial};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet},
    ffi::OsString,
    fs,
    hash::{BuildHasher, RandomState},
    io::ErrorKind,
    num::NonZeroUsize,
    path::{Component, Path, PathBuf},
    process::Command,
    time::{Duration, Instant, SystemTime},
};

pub type Metrics = BTreeMap<String, f64>;

pub fn worst(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else {
        a.max(b)
    }
}

const RANDOM_SEEDS: u64 = 3;
pub type Measure = fn(&Evidence) -> Result<Metrics, String>;

type Reads = RefCell<BTreeSet<(String, String)>>;

pub struct Evidence {
    dir: PathBuf,
    inputs: Vec<(String, toml::Table)>,
    outputs: Vec<(String, Value)>,
    reads: Reads,
    missing: RefCell<BTreeSet<String>>,
}

pub struct Input<'a>(&'a toml::Table);

pub type Matrices = Vec<Vec<Vec<f64>>>;

pub struct Output<'a> {
    name: &'a str,
    value: &'a Value,
    reads: &'a Reads,
}

#[derive(Clone, Copy)]
enum Move {
    Up,
    Down,
    Random { seed: u64, mirror: bool },
}

impl Evidence {
    pub fn names(&self) -> Vec<&str> {
        self.inputs.iter().map(|(name, _)| name.as_str()).collect()
    }

    pub fn input(&self, name: &str) -> Result<Input<'_>, String> {
        self.inputs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, table)| Input(table))
            .ok_or_else(|| format!("no input named {name}"))
    }

    pub fn output(&self, name: &str) -> Result<Output<'_>, String> {
        self.outputs
            .iter()
            .find(|(n, _)| n == name)
            .map(|(name, value)| Output {
                name,
                value,
                reads: &self.reads,
            })
            .ok_or_else(|| format!("no output for input {name}"))
    }

    pub fn warm(&self, name: &str) -> Result<Output<'_>, String> {
        self.output(&warm_name(name))
            .map_err(|_| format!("no warm outcome for {name}: name it in case.toml [relax]"))
    }

    pub fn reference(&self, file: &str) -> Result<String, String> {
        let inside = !file.is_empty()
            && Path::new(file)
                .components()
                .all(|c| matches!(c, Component::Normal(_)));
        if !inside {
            return Err(format!("reference/{file}: name a file inside reference/"));
        }
        fs::read_to_string(self.dir.join("reference").join(file)).map_err(|e| {
            if e.kind() == ErrorKind::NotFound {
                self.missing
                    .borrow_mut()
                    .insert(format!("reference/{file}"));
            }
            format!("reference/{file}: {e}")
        })
    }

    pub fn full_mesh_reference<T: serde::de::DeserializeOwned>(
        &self,
        name: &str,
    ) -> Result<T, String> {
        if self.input(name)?.symmetry()? != Some(false) {
            return Err(format!(
                "{name}: ABINIT runs the full k mesh: set kpoints.symmetry = false"
            ));
        }
        serde_json::from_str(&self.reference(&format!("{name}.json"))?)
            .map_err(|e| format!("reference/{name}.json: {e}"))
    }

    fn perturbed(&self, input: &str, field: &str, how: Move) -> Evidence {
        let mut state = match how {
            Move::Random { seed, .. } => seed,
            _ => 0,
        };
        let outputs = self
            .outputs
            .iter()
            .map(|(name, value)| {
                let mut value = value.clone();
                if name == input
                    && let Some(slot) = value.get_mut(field)
                {
                    *slot = perturb(slot, &mut state, how);
                }
                (name.clone(), value)
            })
            .collect();
        Evidence::new(&self.dir, self.inputs.clone(), outputs)
    }

    fn new(
        dir: &Path,
        inputs: Vec<(String, toml::Table)>,
        outputs: Vec<(String, Value)>,
    ) -> Evidence {
        Evidence {
            dir: dir.to_owned(),
            inputs,
            outputs,
            reads: Reads::default(),
            missing: RefCell::default(),
        }
    }
}

fn warm_name(name: &str) -> String {
    format!("{name}/warm")
}

fn perturb(value: &Value, state: &mut u64, how: Move) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|v| perturb(v, state, how)).collect()),
        Value::Object(map) => Value::Object(
            map.iter()
                .map(|(k, v)| (k.clone(), perturb(v, state, how)))
                .collect(),
        ),
        Value::Number(n) => {
            let sign = match how {
                Move::Up => 1.0,
                Move::Down => -1.0,
                Move::Random { mirror, .. } => {
                    *state = state
                        .wrapping_mul(6_364_136_223_846_793_005)
                        .wrapping_add(1);
                    let sign = if *state >> 63 == 0 { 1.0 } else { -1.0 };
                    if mirror { -sign } else { sign }
                }
            };
            if n.is_f64() {
                let x = n.as_f64().unwrap_or(0.0);
                json!(x + sign * 1e-3 * (1.0 + x.abs()))
            } else {
                json!(n.as_i64().unwrap_or(0) + sign as i64)
            }
        }
        other => other.clone(),
    }
}

fn float(value: &toml::Value) -> Option<f64> {
    value.as_float().or(value.as_integer().map(|i| i as f64))
}

impl Input<'_> {
    pub fn equal_apart_from(&self, other: &Input, keys: &[&str]) -> bool {
        let without = |input: &Input| {
            let mut table = input.0.clone();
            for key in keys {
                match key.split_once('.') {
                    Some((name, inner)) => {
                        if let Some(t) = table.get_mut(name).and_then(toml::Value::as_table_mut) {
                            t.remove(inner);
                        }
                    }
                    None => {
                        table.remove(*key);
                    }
                }
            }
            table
        };
        without(self) == without(other)
    }

    fn get(&self, path: &str) -> Option<&toml::Value> {
        let (table, key) = path.split_once('.')?;
        self.0.get(table)?.as_table()?.get(key)
    }

    fn numbers(&self, key: &str) -> Result<Vec<Vec<f64>>, String> {
        let rows = self
            .get(key)
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("input has no array {key}"))?;
        rows.iter()
            .map(|row| {
                row.as_array()
                    .ok_or_else(|| format!("{key} rows must be arrays"))?
                    .iter()
                    .map(|x| float(x).ok_or_else(|| format!("{key} must hold numbers")))
                    .collect()
            })
            .collect()
    }

    fn triples(&self, key: &str) -> Result<Vec<[f64; 3]>, String> {
        self.numbers(key)?
            .into_iter()
            .map(|row| <[f64; 3]>::try_from(row).map_err(|_| format!("{key} rows need 3 numbers")))
            .collect()
    }

    pub fn species(&self) -> Result<Vec<String>, String> {
        if self.get("probe.rho_ij").is_some() {
            return Ok(vec![self.probe_rho_ij()?.0]);
        }
        let Some(value) = self.get("structure.species") else {
            return Ok(Vec::new());
        };
        value
            .as_array()
            .and_then(|list| list.iter().map(|v| v.as_str().map(str::to_owned)).collect())
            .ok_or_else(|| "structure.species must be a list of element symbols".into())
    }

    pub(crate) fn dataset_file(&self, element: &str) -> Option<String> {
        let files = self.0.get("datasets")?.get("files")?;
        files.get(element)?.as_str().map(str::to_owned)
    }

    pub fn family(&self) -> String {
        (self.get("datasets.family").and_then(toml::Value::as_str))
            .unwrap_or(datasets::DEFAULT_FAMILY)
            .to_owned()
    }

    pub fn probe_rho_ij(&self) -> Result<(String, Matrices), String> {
        let error = || "probe.rho_ij must name one element's [spin][i][j] numbers".to_owned();
        let table = self
            .get("probe.rho_ij")
            .and_then(toml::Value::as_table)
            .ok_or_else(error)?;
        let [(element, spins)] = table.iter().collect::<Vec<_>>()[..] else {
            return Err(error());
        };
        fn numbers(v: &toml::Value) -> Result<&Vec<toml::Value>, String> {
            v.as_array().ok_or_else(|| {
                "probe.rho_ij must name one element's [spin][i][j] numbers".to_owned()
            })
        }
        let rho = (numbers(spins)?.iter())
            .map(|m| {
                (numbers(m)?.iter())
                    .map(|row| {
                        numbers(row)?
                            .iter()
                            .map(|x| float(x).ok_or_else(error))
                            .collect()
                    })
                    .collect()
            })
            .collect::<Result<_, String>>()?;
        Ok((element.clone(), rho))
    }

    pub fn positions_fractional(&self) -> Result<Vec<[f64; 3]>, String> {
        self.triples("structure.positions_fractional")
    }

    pub fn cell_bohr(&self) -> Result<[[f64; 3]; 3], String> {
        <[[f64; 3]; 3]>::try_from(self.triples("structure.cell_bohr")?)
            .map_err(|_| "structure.cell_bohr needs 3 rows".to_owned())
    }

    pub fn kpoints(&self) -> Result<Vec<[f64; 3]>, String> {
        self.triples("kpoints.list")
    }

    pub fn mesh(&self) -> Result<[u64; 3], String> {
        let counts: Option<Vec<u64>> = (self.get("kpoints.mesh").and_then(toml::Value::as_array))
            .and_then(|m| {
                m.iter()
                    .map(|n| u64::try_from(n.as_integer()?).ok())
                    .collect()
            });
        (counts.and_then(|m| <[u64; 3]>::try_from(m).ok()))
            .ok_or_else(|| "input has no kpoints.mesh of 3 counts".to_owned())
    }

    pub fn number(&self, key: &str) -> Result<f64, String> {
        self.get(key)
            .and_then(float)
            .ok_or_else(|| format!("input has no number {key}"))
    }

    pub fn cutoff_ha(&self) -> Result<f64, String> {
        self.get("basis.cutoff_ha")
            .and_then(float)
            .ok_or_else(|| "input has no number basis.cutoff_ha".to_owned())
    }

    pub fn probe_charges(&self) -> Result<Vec<f64>, String> {
        self.list("probe.charges")
    }

    pub fn probe_exponents(&self) -> Result<Vec<f64>, String> {
        self.list("probe.exponents")
    }

    pub fn probe_positions_fractional(&self) -> Result<Vec<[f64; 3]>, String> {
        self.triples("probe.positions_fractional")
    }

    pub fn probe_points(&self) -> Result<Vec<Vec<f64>>, String> {
        self.numbers("probe.points")
    }

    pub fn functional(&self) -> Result<&str, String> {
        self.get("xc.functional")
            .and_then(toml::Value::as_str)
            .ok_or_else(|| "input has no string xc.functional".to_owned())
    }

    fn list(&self, key: &str) -> Result<Vec<f64>, String> {
        self.get(key)
            .and_then(toml::Value::as_array)
            .ok_or_else(|| format!("input has no array {key}"))?
            .iter()
            .map(|x| float(x).ok_or_else(|| format!("{key} must hold numbers")))
            .collect()
    }

    pub fn solver(&self) -> Result<Option<&str>, String> {
        self.get("bands.solver").map_or(Ok(None), |v| {
            v.as_str()
                .map(Some)
                .ok_or_else(|| "bands.solver must be a string".to_owned())
        })
    }

    pub fn symmetry(&self) -> Result<Option<bool>, String> {
        self.get("kpoints.symmetry").map_or(Ok(None), |v| {
            v.as_bool()
                .map(Some)
                .ok_or_else(|| "kpoints.symmetry must be true or false".to_owned())
        })
    }

    pub fn threads(&self) -> Result<Option<NonZeroUsize>, String> {
        self.get("parallel.threads").map_or(Ok(None), |v| {
            (v.as_integer())
                .and_then(|n| usize::try_from(n).ok())
                .and_then(NonZeroUsize::new)
                .map(Some)
                .ok_or_else(|| "parallel.threads must be a count".to_owned())
        })
    }

    pub fn bands(&self) -> Result<usize, String> {
        self.get("bands.count")
            .and_then(toml::Value::as_integer)
            .and_then(|b| usize::try_from(b).ok())
            .ok_or_else(|| "input has no integer bands.count".to_owned())
    }
}

impl Output<'_> {
    fn field(&self, key: &str) -> &Value {
        self.reads
            .borrow_mut()
            .insert((self.name.to_owned(), key.to_owned()));
        &self.value[key]
    }

    pub fn basis_sizes(&self) -> Result<Vec<usize>, String> {
        let error = || "output field basis_sizes must be [k] counts".to_owned();
        self.field("basis_sizes")
            .as_array()
            .ok_or_else(error)?
            .iter()
            .map(|v| {
                v.as_u64()
                    .and_then(|n| usize::try_from(n).ok())
                    .ok_or_else(error)
            })
            .collect()
    }

    pub fn ewald_energy_ha(&self) -> Result<f64, String> {
        self.number("ewald_energy_ha")
    }

    pub fn hartree_energy_ha(&self) -> Result<f64, String> {
        self.number("hartree_energy_ha")
    }

    pub fn paw_xc_energy_ha(&self) -> Result<[f64; 2], String> {
        let error = || "output field paw_xc_energy_ha must be [2] numbers".to_owned();
        match self.field("paw_xc_energy_ha").as_array().map(Vec::as_slice) {
            Some([ae, ps]) => Ok([
                ae.as_f64().ok_or_else(error)?,
                ps.as_f64().ok_or_else(error)?,
            ]),
            _ => Err(error()),
        }
    }

    pub fn paw_xc_d_ha(&self) -> Result<Matrices, String> {
        let error = || "output field paw_xc_d_ha must be [spin][i][j] numbers".to_owned();
        fn numbers(v: &Value) -> Result<&Vec<Value>, String> {
            v.as_array()
                .ok_or_else(|| "output field paw_xc_d_ha must be [spin][i][j] numbers".to_owned())
        }
        (numbers(self.field("paw_xc_d_ha"))?.iter())
            .map(|m| {
                (numbers(m)?.iter())
                    .map(|row| {
                        numbers(row)?
                            .iter()
                            .map(|x| x.as_f64().ok_or_else(error))
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }

    pub fn xc_energy_density_ha_bohr3(&self) -> Result<Vec<[f64; 2]>, String> {
        let error =
            || "output field xc_energy_density_ha_bohr3 must be [point][2] numbers".to_owned();
        self.field("xc_energy_density_ha_bohr3")
            .as_array()
            .ok_or_else(error)?
            .iter()
            .map(|pair| match pair.as_array().map(Vec::as_slice) {
                Some([x, c]) => Ok([x.as_f64().ok_or_else(error)?, c.as_f64().ok_or_else(error)?]),
                _ => Err(error()),
            })
            .collect()
    }

    fn number(&self, key: &str) -> Result<f64, String> {
        self.field(key)
            .as_f64()
            .ok_or_else(|| format!("output field {key} must be a number"))
    }

    pub fn energy_ha(&self, key: &str) -> Result<f64, String> {
        if !key.ends_with("_energy_ha") {
            return Err(format!("{key} is not an energy field"));
        }
        self.number(key)
    }

    pub fn energies(&self) -> Result<BTreeMap<String, f64>, String> {
        (self.value.as_object().into_iter().flat_map(|m| m.keys()))
            .filter(|key| key.ends_with("_energy_ha"))
            .map(|key| Ok((key.clone(), self.energy_ha(key)?)))
            .collect()
    }

    pub fn largest_energy_difference(&self, other: &Output, factor: f64) -> Result<f64, String> {
        let (ours, theirs) = (self.energies()?, other.energies()?);
        if !ours.keys().eq(theirs.keys()) {
            return Err(format!(
                "{} and {} print different energy terms",
                self.name, other.name
            ));
        }
        Ok((ours.iter())
            .map(|(key, x)| (theirs[key] - factor * x).abs())
            .fold(0.0, f64::max))
    }

    pub fn fermi_level_ha(&self) -> Result<f64, String> {
        self.number("fermi_level_ha")
    }

    pub fn magnetization_bohr_magneton(&self) -> Result<f64, String> {
        self.number("magnetization_bohr_magneton")
    }

    pub fn forces_ha_bohr(&self) -> Result<Vec<[f64; 3]>, String> {
        self.triples("forces_ha_bohr", "atom")
    }

    pub fn stress_ha_bohr3(&self) -> Result<[[f64; 3]; 3], String> {
        let rows = self.triples("stress_ha_bohr3", "i")?;
        <[[f64; 3]; 3]>::try_from(rows)
            .map_err(|_| "output field stress_ha_bohr3 must be [3][3] numbers".into())
    }

    pub fn discretization(&self) -> Result<Discretization, String> {
        Discretization::of(self.field("discretization")).map_err(|e| format!("{}: {e}", self.name))
    }

    pub fn scf_steps(&self) -> Result<usize, String> {
        self.field("scf_steps")
            .as_u64()
            .and_then(|n| usize::try_from(n).ok())
            .ok_or_else(|| "output field scf_steps must be a count".to_owned())
    }

    pub fn kpoints_fractional(&self) -> Result<Vec<[f64; 3]>, String> {
        self.triples("kpoints_fractional", "k")
    }

    fn triples(&self, key: &str, row: &str) -> Result<Vec<[f64; 3]>, String> {
        let error = || format!("output field {key} must be [{row}][3] numbers");
        self.field(key)
            .as_array()
            .ok_or_else(error)?
            .iter()
            .map(|k| match k.as_array().map(Vec::as_slice) {
                Some([a, b, c]) => {
                    let n = |v: &Value| v.as_f64().ok_or_else(error);
                    Ok([n(a)?, n(b)?, n(c)?])
                }
                _ => Err(error()),
            })
            .collect()
    }

    pub fn kpoint_weights(&self) -> Result<Vec<f64>, String> {
        let error = || "output field kpoint_weights must be [k] numbers".to_owned();
        self.field("kpoint_weights")
            .as_array()
            .ok_or_else(error)?
            .iter()
            .map(|w| w.as_f64().ok_or_else(error))
            .collect()
    }

    pub fn occupations(&self) -> Result<Vec<Vec<Vec<f64>>>, String> {
        self.cube("occupations")
    }

    pub fn eigenvalues_ha(&self) -> Result<Vec<Vec<Vec<f64>>>, String> {
        self.cube("eigenvalues_ha")
    }

    fn cube(&self, key: &str) -> Result<Vec<Vec<Vec<f64>>>, String> {
        let error = || format!("output field {key} must be [spin][k][band] numbers");
        let array = |v: &Value| v.as_array().cloned().ok_or_else(error);
        array(self.field(key))?
            .iter()
            .map(|spin| {
                array(spin)?
                    .iter()
                    .map(|k| {
                        array(k)?
                            .iter()
                            .map(|e| e.as_f64().ok_or_else(error))
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }
}

pub(crate) struct Block<'a> {
    input: &'a str,
    rival: Option<rival::Job>,
    stop_ha: Option<f64>,
    index: usize,
    threads: Option<NonZeroUsize>,
    table: toml::Table,
    text: String,
    datasets: &'a [(String, Vec<u8>)],
    atoms: usize,
}

impl<'a> Block<'a> {
    fn new(
        input: &'a str,
        index: usize,
        threads: Option<NonZeroUsize>,
        mut table: toml::Table,
        datasets: &'a [(String, Vec<u8>)],
        atoms: usize,
    ) -> Result<Self, String> {
        if let Some(p) = threads {
            at_threads(&mut table, p, true)?;
        }
        let text = toml::to_string(&table).map_err(|e| e.to_string())?;
        let stop_ha = (table
            .get("scf")
            .and_then(|scf| scf.get("energy_tolerance_ha")))
        .and_then(toml::Value::as_float);
        Ok(Block {
            input,
            rival: None,
            stop_ha,
            index,
            threads,
            table,
            text,
            datasets,
            atoms,
        })
    }

    fn at(&self) -> String {
        let code = (self.rival.as_ref()).map_or_else(String::new, |_| format!("{}: ", self.code()));
        match self.threads {
            Some(p) => format!("{code}{} at {p} threads", self.input),
            None => format!("{code}{}", self.input),
        }
    }

    fn code(&self) -> speed::Code {
        (self.rival.as_ref()).map_or(speed::Code::Orbitar, |r| speed::Code::Rival(r.code))
    }

    fn stopped(&self, stop_ha: f64) -> Result<String, engine::Failure> {
        self.input_at(stop_ha, false)
    }

    fn stressed(&self, stop_ha: f64) -> Result<String, engine::Failure> {
        self.input_at(stop_ha, true)
    }

    fn input_at(&self, stop_ha: f64, stress: bool) -> Result<String, engine::Failure> {
        let mut table = self.table.clone();
        engine::set(&mut table, "scf", "energy_tolerance_ha", stop_ha.into())
            .map_err(engine::Failure::Harness)?;
        if stress {
            engine::set(&mut table, "outputs", "stress", true.into())
                .map_err(engine::Failure::Harness)?;
        }
        toml::to_string(&table).map_err(|e| engine::Failure::Harness(e.to_string()))
    }

    fn rival(&self, code: rival::Rival, rivals: &rival::Rivals) -> Result<Block<'a>, String> {
        let p = self.threads.map_or(1, NonZeroUsize::get);
        let cell = rival::Cell::parse(&self.table)?;
        Ok(Block {
            rival: Some(rivals.job(code, cell, p, self.datasets)?),
            table: self.table.clone(),
            text: String::new(),
            ..*self
        })
    }

    fn step_cap(&self) -> Option<usize> {
        (self
            .table
            .get("scf")
            .and_then(|scf| scf.get("max_iterations")))
        .and_then(toml::Value::as_integer)
        .and_then(|n| usize::try_from(n).ok())
    }

    fn floor_ha(&self) -> f64 {
        (self.rival.as_ref())
            .and_then(|job| job.claim.as_ref()?.floor_ha)
            .unwrap_or(0.0)
    }
}

fn at_abinit_cutoff(
    table: &toml::Table,
    cutoffs: &rival::ClaimCutoff,
) -> Result<(toml::Table, (f64, f64)), String> {
    let (cutoff, density) = cutoffs.of(&rival::Cell::parse(table)?)?;
    let mut table = table.clone();
    engine::set(&mut table, "basis", "cutoff_ha", cutoff.into())?;
    engine::set(&mut table, "basis", "density_cutoff_ha", density.into())?;
    Ok((table, (cutoff, density)))
}

const GPA_PER_HA_BOHR3: f64 = 29_421.015_697;

fn pressure_gpa(stdout: &str) -> Result<f64, String> {
    let out: Value =
        serde_json::from_str(stdout).map_err(|e| format!("the outcome is not JSON: {e}"))?;
    let sigma: [[f64; 3]; 3] = serde_json::from_value(out["stress_ha_bohr3"].clone())
        .map_err(|_| "the outcome holds no stress_ha_bohr3 [3][3]".to_owned())?;
    Ok(-(sigma[0][0] + sigma[1][1] + sigma[2][2]) / 3.0 * GPA_PER_HA_BOHR3)
}

const STRESS_CASE: &str = "15_stress_derivative";
const SPIN_STRESS_NOTE: &str = "orbitaR's spin stress has no derivative case yet";

fn spin_stress_case(root: &Path) -> bool {
    inputs(root, &root.join("benchmarks").join(STRESS_CASE), None).is_ok_and(|(all, _)| {
        (all.iter()).any(|(_, text)| {
            toml::from_str::<toml::Table>(text).is_ok_and(|t| t.contains_key("spin"))
        })
    })
}

fn compressed(table: &toml::Table, volume_scale: f64) -> Result<toml::Table, String> {
    let s = volume_scale.cbrt();
    let mut table = table.clone();
    let rows = (table.get_mut("structure"))
        .and_then(|t| t.get_mut("cell_bohr"))
        .and_then(toml::Value::as_array_mut)
        .ok_or("the input has no structure.cell_bohr to scale")?;
    for row in rows.iter_mut() {
        for x in row
            .as_array_mut()
            .ok_or("structure.cell_bohr rows must be arrays")?
        {
            let v = (x.as_float())
                .or_else(|| x.as_integer().map(|n| n as f64))
                .ok_or("structure.cell_bohr must hold numbers")?;
            *x = (v * s).into();
        }
    }
    Ok(table)
}

fn borrows(id: &str, b: &Block, rivals: &rival::Rivals) -> bool {
    let abinit = rival::Rival::Abinit;
    let path = rival::input_path(id, b.input);
    (rival::Cell::parse(&b.table))
        .and_then(|cell| rivals.calibrated(abinit, &path, &cell, rivals.cutoff(abinit, &cell)?))
        .is_ok_and(|row| row.is_some_and(|(_, from)| from != path))
}

fn accuracies(
    id: &str,
    blocks: &[Block],
    timed: &mut [speed::Timed],
    rivals: &rival::Rivals,
    pressures: &BTreeMap<(speed::Code, usize), Result<f64, String>>,
    compressed: &BTreeMap<(speed::Code, usize), Result<f64, String>>,
    spin_stress_case: bool,
) -> Vec<String> {
    let abinit = rival::Rival::Abinit;
    let target = rivals.settings.target.energy_difference_ha_per_atom;
    let target_p = rivals.settings.target.pressure_gpa;
    let path = |b: &Block| rival::input_path(id, b.input);
    let calibrated = |code: rival::Rival, b: &Block| {
        rival::Cell::parse(&b.table).and_then(|cell| {
            let cutoff = rivals.cutoff(code, &cell)?;
            rivals.calibrated(code, &path(b), &cell, cutoff)
        })
    };
    let basis = |b: &Block, from: &str| match from == path(b) {
        true => "own".to_owned(),
        false => format!("borrowed from {from}"),
    };
    let pressure_of = |b: &Block| {
        (pressures.get(&(b.code(), b.index)).cloned())
            .unwrap_or_else(|| Err("no reference run with stress".to_owned()))
    };
    let mut failures = Vec::new();
    for (i, b) in blocks.iter().enumerate() {
        let code = b.rival.as_ref().map_or(abinit, |r| r.code);
        let calibration = match calibrated(code, b) {
            Ok(row) => row,
            Err(e) => {
                failures.push(format!("{}: {e}", b.at()));
                None
            }
        };
        let row = calibration.as_ref().map(|(row, _)| *row);
        let from = calibration.as_ref().map(|(_, from)| basis(b, from));
        let own = calibration
            .as_ref()
            .is_some_and(|(_, from)| *from == path(b));
        let (e, e_abs, e_p) = (row.map(|r| r[1]), row.map(|r| r[3]), row.map(|r| r[2]));
        if b.rival.is_some() {
            timed[i].record.accuracy = Some(json!({
                "e_ha_per_atom": e,
                "e_abs_ha_per_atom": e_abs,
                "e_p_gpa": e_p,
                "p_gpa": row.map(|r| r[4]),
                "basis": from,
                "target_energy_difference_ha_per_atom": target,
                "target_pressure_gpa": target_p,
            }));
            continue;
        }
        let theirs = (blocks.iter().zip(timed.iter()))
            .find(|(o, _)| o.input == b.input && o.rival.as_ref().is_some_and(|r| r.code == abinit))
            .and_then(|(_, t)| t.record.e_ref_ha);
        let atoms = b.atoms as f64;
        let ours_e1 = timed[i].record.e_ref_ha;
        let d_e1 = (theirs.zip(ours_e1)).map(|(a, o)| (o - a).abs() / atoms);
        let e2 = |code: speed::Code| {
            (compressed.get(&(code, b.index)).cloned())
                .unwrap_or_else(|| Err("no run at V₂".to_owned()))
        };
        let (ours_e2, theirs_e2) = (e2(speed::Code::Orbitar), e2(speed::Code::Rival(abinit)));
        let d = match (ours_e1, &ours_e2, theirs, &theirs_e2) {
            (Some(o1), Ok(o2), Some(a1), Ok(a2)) => Some(((o1 - o2) - (a1 - a2)).abs() / atoms),
            _ => None,
        };
        let ours = pressure_of(b);
        let abinit_block = (blocks.iter())
            .find(|o| o.input == b.input && o.rival.as_ref().is_some_and(|r| r.code == abinit));
        let theirs_p = match (row, own) {
            (Some(r), true) if !r[4].is_nan() => Ok(r[4]),
            (Some(_), true) => {
                Err("ABINIT's row at its frozen cutoff holds no pressure".to_owned())
            }
            (Some(_), false) => abinit_block
                .ok_or_else(|| "no ABINIT block runs this input".to_owned())
                .and_then(pressure_of)
                .map_err(|e| format!("ABINIT's reference run at c, its row being borrowed: {e}")),
            (None, _) => Err("ABINIT has no calibration row at its frozen cutoff".to_owned()),
        };
        let measured = ours
            .clone()
            .and_then(|o| Ok((o, theirs_p.clone()?, e_p.unwrap_or(f64::NAN))));
        let d_p = measured.as_ref().ok().map(|(o, a, _)| (o - a).abs());
        let holds_p = (d_p.zip(e_p)).map(|(d_p, e_p)| 2.0 * d_p + e_p <= target_p);
        timed[i].record.accuracy = Some(json!({
            "d_ha_per_atom": d,
            "d_e1_ha_per_atom": d_e1,
            "e2_orbitar_ha": ours_e2.as_ref().ok(),
            "e2_abinit_ha": theirs_e2.as_ref().ok(),
            "e2_unmeasured": [ours_e2.as_ref().err(), theirs_e2.as_ref().err()],
            "e_ha_per_atom": e,
            "e_abs_ha_per_atom": e_abs,
            "basis": from,
            "bound": "2d + e",
            "target_energy_difference_ha_per_atom": target,
            "p_orbitar_gpa": ours.as_ref().ok(),
            "p_abinit_gpa": theirs_p.as_ref().ok(),
            "p_abinit_from": row.map(|_| if own { "its calibration row" } else { "its reference run at c" }),
            "e_p_from": calibration.as_ref().map(|(_, from)| format!("{from}'s row")),
            "d_p_gpa": d_p,
            "e_p_gpa": e_p,
            "bound_p": "2 d_P + e_P",
            "target_pressure_gpa": target_p,
            "pressure_holds": holds_p,
            "pressure_unmeasured": measured.as_ref().err(),
            "spin": b.table.contains_key("spin"),
            "note": (b.table.contains_key("spin") && !spin_stress_case).then_some(SPIN_STRESS_NOTE),
        }));
        if first_of(blocks, b) != i {
            continue;
        }
        match (d, e) {
            (Some(d), Some(e)) if 2.0 * d + e <= target => {}
            (Some(d), Some(e)) => failures.push(format!(
                "{}: at ABINIT's frozen cutoff, 2d + e = 2 · {d:e} + {e:e} Ha per atom is past the target {target:e}",
                b.input
            )),
            _ => failures.push(format!(
                "{}: at ABINIT's frozen cutoff, the accuracy is unmeasured (d {d:?}, e {e:?})",
                b.input
            )),
        }
        match measured.as_ref().map(|&(o, a, e_p)| ((o - a).abs(), e_p)) {
            Ok((d_p, e_p)) if 2.0 * d_p + e_p <= target_p => {}
            Ok((d_p, e_p)) => failures.push(format!(
                "{}: at ABINIT's frozen cutoff, 2 d_P + e_P = 2 · {d_p:e} + {e_p:e} GPa is past the target {target_p:e}",
                b.input
            )),
            Err(why) => failures.push(format!(
                "{}: at ABINIT's frozen cutoff, the pressure is unmeasured: {why}",
                b.input
            )),
        }
    }
    failures
}

fn first_of(blocks: &[Block], b: &Block) -> usize {
    (blocks.iter())
        .position(|o| {
            o.index == b.index
                && o.rival.as_ref().map(|r| r.code) == b.rival.as_ref().map(|r| r.code)
        })
        .unwrap_or_default()
}

#[derive(Debug, PartialEq)]
pub enum Outcome {
    Pass,
    Fail(String),
    Planned(String),
    Promote,
    Shard,
    Skipped,
}

struct HeldOut {
    inputs: Vec<(String, String)>,
    held: BTreeSet<String>,
    tuned: Vec<String>,
}

pub(crate) const PILOT: &str = "pilot (ORBITAR_PILOT): one timed run per block prices the claim run; the record proves no claim";

#[derive(Clone)]
pub struct Session {
    root: PathBuf,
    bin: PathBuf,
    engine_sha256: String,
    tier: Tier,
    records: PathBuf,
    seed: u64,
    seed_fixed: bool,
    rivals: Option<PathBuf>,
    filter: Option<Vec<String>>,
    held_out: Option<PathBuf>,
    table: Option<PathBuf>,
    pilot: bool,
}

impl Session {
    fn at_v2(
        &self,
        b: &Block,
        volume_scale: f64,
        rivals: &rival::Rivals,
        timeout: Duration,
    ) -> Result<f64, String> {
        let text = |f: engine::Failure| match f {
            engine::Failure::Harness(e) | engine::Failure::Engine(e) => e,
        };
        let stop = speed::reference_stop(b).map_err(text)?;
        let run = match &b.rival {
            Some(job) => {
                let cell = rival::Cell::parse(&b.table)?.scaled(volume_scale);
                let p = b.threads.map_or(1, NonZeroUsize::get);
                let job = rivals.job(job.code, cell, p, b.datasets)?;
                let modes = (rival::Mode::Energy, rival::Clocks::Warning);
                rivals
                    .run(&job, b.datasets, stop, modes.0, modes.1, timeout)
                    .map_err(text)?
            }
            None => {
                let mut table = compressed(&b.table, volume_scale)?;
                engine::set(&mut table, "scf", "energy_tolerance_ha", stop.into())?;
                let input = toml::to_string(&table).map_err(|e| e.to_string())?;
                engine::once(&self.bin, &input, b.datasets, timeout).map_err(text)?
            }
        };
        (run.steps.last())
            .map(|s| s.free_energy_ha)
            .ok_or_else(|| format!("{}: its run at V₂ printed no step line", b.at()))
    }

    pub fn new(root: &Path, bin: &Path, tier: Tier, records: &Path) -> Session {
        let nanos = unix_ms();
        Session {
            root: root.to_owned(),
            bin: bin.to_owned(),
            engine_sha256: fs::read(bin)
                .map(|b| crate::lock::hex(&Sha256::digest(b)))
                .unwrap_or_else(|e| format!("unreadable: {e}")),
            tier,
            records: records.to_owned(),
            seed: RandomState::new().hash_one(nanos),
            seed_fixed: false,
            rivals: None,
            filter: None,
            held_out: None,
            table: None,
            pilot: false,
        }
    }

    pub fn with_held_out(mut self, dir: &Path) -> Session {
        self.held_out = Some(dir.to_owned());
        self
    }

    pub fn with_filter(mut self, selectors: Vec<String>) -> Session {
        self.filter = Some(selectors);
        self
    }

    pub fn with_table(mut self, path: &Path) -> Session {
        self.table = Some(path.to_owned());
        self
    }

    pub fn with_rivals(mut self, prefix: &Path) -> Session {
        self.rivals = Some(prefix.to_owned());
        self
    }

    pub fn from_vars(
        root: &Path,
        var: impl Fn(&str) -> Option<OsString>,
    ) -> Result<Option<Session>, String> {
        let Some(bin) = var("ORBITAR_BIN") else {
            return Ok(None);
        };
        let text = |key: &str| var(key).map(|v| v.to_string_lossy().into_owned());
        let name = text("ORBITAR_TIER").unwrap_or_else(|| "push".into());
        let tier = Tier::parse(&name).ok_or_else(|| format!("unknown tier {name}"))?;
        let records = var("ORBITAR_RECORD_DIR")
            .map_or_else(|| root.join("target/benchmarks").join(&name), PathBuf::from);
        let mut session = Session::new(root, Path::new(&bin), tier, &records);
        session.rivals = var("ORBITAR_RIVALS").map(PathBuf::from);
        session.held_out = var("ORBITAR_HELDOUT").map(PathBuf::from);
        if let Some(seed) = text("ORBITAR_SEED") {
            session.seed = seed.parse().map_err(|_| "ORBITAR_SEED must be a u64")?;
            session.seed_fixed = true;
        }
        if let Some(pilot) = text("ORBITAR_PILOT") {
            if pilot != "1" || tier != Tier::Claim {
                return Err(format!(
                    "ORBITAR_PILOT must be 1 and runs in the claim tier alone, not `{pilot}` in {name}"
                ));
            }
            session.pilot = true;
        }
        if let Some(filter) = text("ORBITAR_CASE_FILTER") {
            if tier != Tier::Node {
                return Err(filter_refused(tier));
            }
            let selectors: Vec<String> = filter.split(',').map(|s| s.trim().to_owned()).collect();
            if selectors.iter().any(String::is_empty) {
                return Err(format!(
                    "ORBITAR_CASE_FILTER `{filter}` holds an empty selector; separate input names by commas"
                ));
            }
            session.filter = Some(selectors);
        }
        if let Some(table) = var("ORBITAR_DATASET_TABLE") {
            if tier != Tier::Node {
                return Err(format!(
                    "ORBITAR_DATASET_TABLE runs only in the node tier, where the tuning case runs, not in {name}"
                ));
            }
            session.table = Some(PathBuf::from(table));
        }
        Ok(Some(session))
    }

    pub fn evaluate(&self, id: &str, measure: Measure) -> Outcome {
        let case = match Case::load(&self.root, id) {
            Ok(case) => case,
            Err(e) => return Outcome::Fail(e),
        };
        if !self.tier.includes(case.tier()) || case.status() == Status::Retired {
            return Outcome::Skipped;
        }
        let started = unix_ms();
        let mut trace = Trace::default();
        let mut failures = self.attempt(id, &case, measure, &mut trace);
        if self.pilot {
            failures.push(PILOT.to_owned());
        }
        let outcome = match (case.status(), failures.is_empty(), trace.faulted) {
            (_, _, true) | (Status::Active, false, _) => Outcome::Fail(failures.join("; ")),
            (_, true, _) if self.filter.is_some() || self.table.is_some() => Outcome::Shard,
            (Status::Planned, true, _) => Outcome::Promote,
            (Status::Planned, false, _) => Outcome::Planned(failures.join("; ")),
            (_, true, _) => Outcome::Pass,
            (_, false, _) => Outcome::Fail(failures.join("; ")),
        };
        let name = match &outcome {
            Outcome::Pass => "pass",
            Outcome::Promote => "promote",
            Outcome::Planned(_) => "planned",
            Outcome::Shard => "shard",
            _ => "fail",
        };
        let record = json!({
            "schema": record::SCHEMA,
            "case": id,
            "tier": self.tier.name(),
            "case_tier": case.tier().name(),
            "status": format!("{:?}", case.status()).to_lowercase(),
            "outcome": name,
            "failures": failures,
            "metrics": trace.metrics,
            "tolerance": case.tolerance(),
            "why": case.why(),
            "readouts": readouts(&trace.metrics, case.tolerance()),
            "inputs": trace.inputs,
            "filter": self.filter,
            "table": trace.table,
            "repeats": case.repeats(),
            "runs_s": trace.runs_s,
            "peak_rss_bytes": trace.peak_rss_bytes,
            "peak_pss_bytes": trace.peak_pss_bytes,
            "threads": self.tier.policy().speed.map(|s| s.threads),
            "speed": trace.speed,
            "skipped": trace.skipped,
            "versus": trace.versus,
            "held_out": trace.held_out,
            "tuned": trace.tuned,
            "relaxations": trace.relaxations,
            "rotation_s": trace.rotation_s,
            "rotation_max": trace.rotation.as_ref().map(|r| r.max),
            "rotation_used": trace.rotation.as_ref().map(|r| r.used),
            "rotation_field": trace.rotation.as_ref().and_then(|r| r.field.as_ref()).map(|f| &f.0),
            "spent_s": trace.spent_s,
            "started_unix_ms": started,
            "seed": self.seed.to_string(),
            "seed_fixed": self.seed_fixed,
            "pilot": self.pilot,
            "engine_sha256": self.engine_sha256,
            "lock": { "case": crate::lock::digest(&self.root, id), "harness": crate::lock::digest(&self.root, crate::lock::HARNESS) },
            "commit": git(&self.root, &["rev-parse", "HEAD"]),
            "dirty": !git(&self.root, &["status", "--porcelain"]).is_empty(),
            "rustc": rustc(),
            "node": crate::node::block(&self.root, self.rivals.as_deref()),
        });
        if let Err(e) = record::write(&self.records, id, &record) {
            return Outcome::Fail(format!("record: {e}"));
        }
        outcome
    }

    fn attempt(&self, id: &str, case: &Case, measure: Measure, trace: &mut Trace) -> Vec<String> {
        let dir = self.root.join("benchmarks").join(id);
        let (inputs, own) = match inputs(&self.root, &dir, self.table.as_deref()) {
            Ok((inputs, _)) if inputs.is_empty() => {
                return vec!["the case has no input yet".into()];
            }
            Ok(found) => found,
            Err(e) => return vec![trace.failed("inputs", engine::Failure::Harness(e))],
        };
        trace.table = own.as_ref().map(datasets::Table::named);
        let inputs = match (&self.filter, self.tier) {
            (None, _) => inputs,
            (Some(_), tier) if tier != Tier::Node => {
                return vec![
                    trace.failed("filter", engine::Failure::Harness(filter_refused(tier))),
                ];
            }
            (Some(filter), _) => {
                let names: Vec<&str> = inputs.iter().map(|(n, _)| n.as_str()).collect();
                let blind = match own {
                    Some(_) => acwf::Split::read(&self.root)
                        .and_then(|s| s.blind(filter.iter().map(String::as_str))),
                    None => Ok(()),
                };
                let chosen =
                    selected(&names, filter).map_err(|e| format!("ORBITAR_CASE_FILTER {e}"));
                match blind.and(chosen) {
                    Ok(chosen) => (inputs.into_iter())
                        .filter(|(n, _)| chosen.contains(n))
                        .collect(),
                    Err(e) => return vec![trace.failed("filter", engine::Failure::Harness(e))],
                }
            }
        };
        let HeldOut {
            inputs,
            held,
            tuned,
        } = match self.held_out(id, case, inputs, trace) {
            Ok(held_out) => held_out,
            Err(e) => return vec![trace.failed("held-out", engine::Failure::Harness(e))],
        };
        trace.tuned = tuned;
        let timeout = Duration::from_secs(case.timeout_s());
        let policy = self.tier.policy();
        let mut tables = Vec::new();
        let mut files = Vec::new();
        let mut atoms = Vec::new();
        for (name, text) in &inputs {
            trace.inputs.push(name.clone());
            let table = match toml::from_str::<toml::Table>(text) {
                Ok(table) => table,
                Err(e) => return vec![format!("{name}: {e}")],
            };
            match Input(&table).species().and_then(|s| {
                let found = datasets::files(&self.root, own.as_ref(), &Input(&table))?;
                Ok((s.len(), found))
            }) {
                Ok((n, found)) => {
                    atoms.push(n);
                    files.push(found);
                }
                Err(e) => return vec![trace.failed(name, engine::Failure::Harness(e))],
            }
            tables.push(table);
        }
        let mut warm = Vec::new();
        for (name, &kind) in case.relax() {
            let Some(i) = inputs.iter().position(|(n, _)| n == name) else {
                let why = format!("case.toml [relax] names {name}, which is no input");
                return vec![trace.failed("relax", engine::Failure::Harness(why))];
            };
            let clock = Instant::now();
            let relaxed = relax::run(&self.bin, &tables[i], &files[i], kind, timeout);
            trace.spend(clock);
            match relaxed {
                Ok(r) => {
                    trace.relaxations.push(json!({
                        "input": name, "kind": format!("{kind:?}").to_lowercase(), "steps": r.steps,
                        "geometries": r.geometries, "seconds": r.seconds,
                    }));
                    tables[i] = r.endpoint;
                    warm.push((warm_name(name), r.warm));
                }
                Err(failure) => return vec![trace.failed(name, failure)],
            }
        }
        let sweep: Vec<Option<NonZeroUsize>> = match &policy.speed {
            Some(speed) => speed.threads.iter().copied().map(Some).collect(),
            None => vec![None],
        };
        let rivals = match (&policy.speed, case.rivals()) {
            (Some(_), [_, ..]) => {
                let loaded = (self.rivals.as_deref())
                    .ok_or_else(|| "the case times rivals, but ORBITAR_RIVALS is unset".to_owned())
                    .and_then(|prefix| rival::Rivals::load(&self.root, prefix));
                match loaded {
                    Ok(rivals) => Some(rivals),
                    Err(e) => return vec![trace.failed("rivals", engine::Failure::Harness(e))],
                }
            }
            _ => None,
        };
        let mut blocks: Vec<Block> = Vec::new();
        for &threads in &sweep {
            for (i, (name, _)) in inputs.iter().enumerate() {
                let table = match rivals.as_ref().map_or(Ok(tables[i].clone()), |r| {
                    at_abinit_cutoff(&tables[i], &r.claim_cutoff()).map(|(table, _)| table)
                }) {
                    Ok(table) => table,
                    Err(e) => return vec![trace.failed(name, engine::Failure::Harness(e))],
                };
                let ours = match Block::new(name, i, threads, table, &files[i], atoms[i]) {
                    Ok(block) => block,
                    Err(e) => return vec![trace.failed(name, engine::Failure::Harness(e))],
                };
                for (&code, rivals) in case.rivals().iter().zip(std::iter::repeat(&rivals)) {
                    let Some(rivals) = rivals else { break };
                    if let Some(reason) = rivals.settings.skips(code, &rival::input_path(id, name))
                    {
                        if threads == sweep[0] {
                            let code = code.adapter().name();
                            trace
                                .skipped
                                .push(json!({ "code": code, "input": name, "reason": reason }));
                        }
                        continue;
                    }
                    if threads.is_some_and(|p| !rivals.settings.times(p.get())) {
                        continue;
                    }
                    match ours.rival(code, rivals) {
                        Ok(block) => blocks.push(block),
                        Err(e) => return vec![trace.failed(name, engine::Failure::Harness(e))],
                    }
                }
                blocks.push(ours);
            }
        }
        if policy.speed.is_some()
            && let Some(missing) = preflight(&dir, &inputs, &tables, &held, measure)
        {
            return vec![trace.failed("reference", engine::Failure::Harness(missing))];
        }
        let clock = Instant::now();
        let mut reserved = Metrics::new();
        let mut failures = Vec::new();
        let ran = match &policy.speed {
            Some(speed) => {
                let pressures: RefCell<BTreeMap<(speed::Code, usize), Result<f64, String>>> =
                    RefCell::default();
                let stressed = |b: &Block| match (&b.rival, &rivals) {
                    (_, None) => false,
                    (None, Some(_)) => true,
                    (Some(job), Some(rivals)) => {
                        job.code == rival::Rival::Abinit && borrows(id, b, rivals)
                    }
                };
                let run = |b: &Block, stop: f64, pass: speed::Pass| {
                    let stress = pass == speed::Pass::Reference && stressed(b);
                    let ran = match (&b.rival, &rivals) {
                        (Some(job), Some(rivals)) => {
                            let mode = if stress {
                                rival::Mode::Stress
                            } else {
                                rival::Mode::Energy
                            };
                            rivals.run(job, b.datasets, stop, mode, rival::Clocks::Loss, timeout)?
                        }
                        _ => {
                            let input = if stress {
                                b.stressed(stop)?
                            } else {
                                b.stopped(stop)?
                            };
                            engine::once(&self.bin, &input, b.datasets, timeout)?
                        }
                    };
                    if stress {
                        let p = match &b.rival {
                            Some(job) => (job.code.pressure(&ran.stdout))
                                .ok_or_else(|| "its reference run printed no pressure".to_owned()),
                            None => pressure_gpa(&ran.stdout)
                                .map_err(|e| format!("orbitaR's reference run: {e}")),
                        };
                        pressures.borrow_mut().insert((b.code(), b.index), p);
                    }
                    Ok(ran)
                };
                let (repeats, speed) = match self.pilot {
                    true => (1, &speed::never_grown(speed)),
                    false => (case.repeats() as usize, speed),
                };
                let ran = speed::time(&run, &blocks, speed, repeats, self.seed);
                ran.map(|mut timed| {
                    if let Some(rivals) = &rivals {
                        let spin = spin_stress_case(&self.root);
                        let scale = rivals.settings.target.volume_scale;
                        let mut compressed = BTreeMap::new();
                        for b in &blocks {
                            let key = (b.code(), b.index);
                            let wanted = (b.rival.as_ref()).is_none_or(|j| j.code == rival::Rival::Abinit);
                            if wanted && !compressed.contains_key(&key) {
                                compressed.insert(key, self.at_v2(b, scale, rivals, timeout));
                            }
                        }
                        failures.extend(accuracies(
                            id,
                            &blocks,
                            &mut timed,
                            rivals,
                            &pressures.borrow(),
                            &compressed,
                            spin,
                        ));
                        let (versus, judged) = speed::versus(&blocks, &timed, case.milestone());
                        trace.versus = versus;
                        failures.extend(judged);
                    }
                    if case.claims().contains(&Claim::Memory) {
                        reserved = speed::memory(&blocks, &timed);
                    }
                    (blocks.iter().zip(timed))
                        .filter_map(|(b, t)| {
                            if t.record.noisy && case.timed() {
                                let (n, lost) = (t.runs.len(), t.record.losses.len());
                                failures.push(format!(
                                    "{}: T(δ) has CV {} over {n} runs and {lost} losses: under two judged times or no finite CV, so the judge refuses it",
                                    b.at(),
                                    (t.record.timing.as_ref()).and_then(|t| t.cv_t_delta).unwrap_or(f64::NAN),
                                ));
                            }
                            trace.speed.push(json!(t.record));
                            b.rival.is_none().then_some((t.stdout, t.runs))
                        })
                        .collect::<Vec<_>>()
                })
                .map_err(|failure| ("timing", failure))
            }
            None => (blocks.iter())
                .map(|b| {
                    let runs =
                        engine::repeat(&self.bin, &b.text, b.datasets, case.repeats(), timeout);
                    runs.map(|runs| (runs[0].stdout.clone(), runs))
                        .map_err(|failure| (b.input, failure))
                })
                .collect(),
        };
        trace.spend(clock);
        blocks.retain(|b| b.rival.is_none());
        let ran = match ran {
            Ok(ran) => ran,
            Err((name, failure)) => return vec![trace.failed(name, failure)],
        };
        for (_, runs) in &ran {
            trace.runs_s.extend(runs.iter().map(|r| r.seconds));
            trace
                .peak_rss_bytes
                .extend(runs.iter().map(|r| r.peak_rss_bytes));
            trace
                .peak_pss_bytes
                .extend(runs.iter().map(|r| r.peak_pss_bytes));
        }
        let mut own = Vec::new();
        if rivals.is_some() {
            for (i, (name, _)) in inputs
                .iter()
                .enumerate()
                .filter(|(_, (n, _))| !held.contains(n))
            {
                let b = match Block::new(name, i, sweep[0], tables[i].clone(), &files[i], atoms[i])
                {
                    Ok(b) => b,
                    Err(e) => return vec![trace.failed(name, engine::Failure::Harness(e))],
                };
                let clock = Instant::now();
                let run = engine::once(&self.bin, &b.text, b.datasets, timeout);
                trace.spend(clock);
                match run {
                    Ok(run) => own.push((b, run.stdout)),
                    Err(failure) => return vec![trace.failed(name, failure)],
                }
            }
        }
        let judged: Vec<(&Block, &String)> = match own.is_empty() {
            true => blocks
                .iter()
                .zip(ran.iter().map(|(stdout, _)| stdout))
                .filter(|(b, _)| !held.contains(b.input))
                .collect(),
            false => own.iter().map(|(b, stdout)| (b, stdout)).collect(),
        };
        let measured_inputs: Vec<String> = (trace.inputs.iter())
            .filter(|n| !held.contains(*n))
            .cloned()
            .collect();
        let mut worst = engine::Change::default();
        let mut evidences: Vec<(Option<NonZeroUsize>, Evidence)> = Vec::new();
        for (b, stdout) in judged {
            let (turned, rotation) = match engine::rotated(&b.text, self.seed) {
                Ok(turned) => turned,
                Err(e) => return vec![trace.failed(b.input, engine::Failure::Harness(e))],
            };
            let clock = Instant::now();
            let rotated = engine::once(&self.bin, &turned, b.datasets, timeout);
            trace.spend(clock);
            let out = match rotated {
                Ok(run) => {
                    trace.rotation_s.push(run.seconds);
                    run.stdout
                }
                Err(failure) => return vec![trace.failed(b.input, failure)],
            };
            match engine::deviation(&b.text, stdout, &out, rotation) {
                Ok(change) => worst.add(change),
                Err(failure) => return vec![trace.failed(b.input, failure)],
            }
            let output = match serde_json::from_str(stdout) {
                Ok(output) => output,
                Err(e) => return vec![format!("{}: engine stdout is not JSON: {e}", b.input)],
            };
            let at = match evidences.iter().position(|(p, _)| *p == b.threads) {
                Some(at) => at,
                None => {
                    evidences.push((b.threads, Evidence::new(&dir, Vec::new(), Vec::new())));
                    evidences.len() - 1
                }
            };
            evidences[at]
                .1
                .inputs
                .push((b.input.to_owned(), b.table.clone()));
            evidences[at].1.outputs.push((b.input.to_owned(), output));
        }
        for (_, evidence) in &mut evidences {
            evidence.outputs.extend(warm.iter().cloned());
        }
        failures.extend(engine::excess(&worst));
        trace.rotation = Some(worst);
        let (own, measured): (BTreeMap<String, f64>, BTreeMap<String, f64>) =
            (case.tolerance().iter())
                .map(|(k, v)| (k.clone(), *v))
                .partition(|(key, _)| crate::case::is_reserved(key));
        failures.extend(record::judge(&own, &reserved));
        for (threads, evidence) in &evidences {
            let (judged, metrics) = self.judge(&measured, measure, evidence, &measured_inputs);
            trace.metrics = metrics;
            let prefix = threads
                .map(|p| format!("threads {p}: "))
                .unwrap_or_default();
            failures.extend(judged.into_iter().map(|f| format!("{prefix}{f}")));
        }
        trace.metrics.extend(reserved);
        failures
    }

    fn held_out(
        &self,
        id: &str,
        case: &Case,
        mut inputs: Vec<(String, String)>,
        trace: &mut Trace,
    ) -> Result<HeldOut, String> {
        let mut held = BTreeSet::new();
        if self.tier != Tier::Claim || case.rivals().is_empty() {
            return Ok(HeldOut {
                inputs,
                held,
                tuned: Vec::new(),
            });
        }
        let settings = rival::settings(&self.root)?;
        let cells: BTreeSet<&String> = (settings.calibration_cells.iter())
            .chain(&settings.layout_cells)
            .collect();
        let tuned = (inputs.iter())
            .map(|(name, _)| name)
            .filter(|name| cells.contains(&rival::input_path(id, name)))
            .cloned()
            .collect();
        let Some(base) = self.held_out.as_deref() else {
            return Ok(HeldOut {
                inputs,
                held,
                tuned,
            });
        };
        let sha = |text: &str| crate::lock::hex(&Sha256::digest(text));
        let mut known: BTreeMap<String, String> = BTreeMap::new();
        let committed = (crate::walk::files(&self.root, "benchmarks").into_iter())
            .chain(crate::walk::files(&self.root, crate::layout::TUNING))
            .filter(|file| file.ends_with(".toml"));
        let mut parsed: Vec<(toml::Table, String)> = Vec::new();
        for file in cells.into_iter().cloned().chain(committed) {
            let text =
                fs::read_to_string(self.root.join(&file)).map_err(|e| format!("{file}: {e}"))?;
            parsed.extend(toml::from_str(&text).ok().map(|t| (t, file.clone())));
            known.entry(sha(&text)).or_insert(file);
        }
        for (name, text) in &inputs {
            parsed.extend(
                toml::from_str(text)
                    .ok()
                    .map(|t| (t, format!("input {name}"))),
            );
            known.insert(sha(text), format!("input {name}"));
        }
        let dir = base.join(id);
        let named = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
        let (inside, root) = (
            fs::canonicalize(&dir).map_err(|e| named(&dir, e))?,
            fs::canonicalize(&self.root),
        );
        if root.is_ok_and(|root| inside.starts_with(root)) {
            return Err(format!(
                "{}: held-out variants stay outside the repository",
                dir.display()
            ));
        }
        let mut files: Vec<PathBuf> = (fs::read_dir(&dir).map_err(|e| named(&dir, e))?)
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<_, _>>()
            .map_err(|e| named(&dir, e))?;
        files.retain(|p| p.extension().is_some_and(|x| x == "toml"));
        files.sort();
        if files.is_empty() {
            return Err(format!(
                "{}: no held-out <stem>.toml for {id}",
                dir.display()
            ));
        }
        for path in files {
            let stem = path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default();
            let text = fs::read_to_string(&path).map_err(|e| named(&path, e))?;
            let name = format!("held-out-{stem}");
            if inputs.iter().any(|(n, _)| *n == name) {
                return Err(format!(
                    "{}: the case already holds an input {name}",
                    path.display()
                ));
            }
            let hash = sha(&text);
            if let Some(same) = known.get(&hash) {
                return Err(format!(
                    "{}: the same bytes as {same}; a held-out variant is new (CONTRIBUTING.md, rival cases)",
                    path.display()
                ));
            }
            let table = toml::from_str::<toml::Table>(&text).ok();
            if let Some((_, same)) = parsed.iter().find(|(t, _)| Some(t) == table.as_ref()) {
                return Err(format!(
                    "{}: the same input as {same} once parsed; a held-out variant is new (CONTRIBUTING.md, rival \
                     cases)",
                    path.display()
                ));
            }
            trace
                .held_out
                .push(json!({ "input": name, "sha256": hash }));
            held.insert(name.clone());
            inputs.push((name, text));
        }
        Ok(HeldOut {
            inputs,
            held,
            tuned,
        })
    }

    fn judge(
        &self,
        tolerance: &BTreeMap<String, f64>,
        measure: Measure,
        evidence: &Evidence,
        names: &[String],
    ) -> (Vec<String>, Metrics) {
        let mut failures = Vec::new();
        let mut measured = Metrics::new();
        let mut bounds = tolerance.clone();
        match measure(evidence) {
            Ok(metrics) => {
                if self.filter.is_some() {
                    bounds = record::measured(tolerance, &metrics);
                }
                for name in metrics
                    .keys()
                    .filter(|name| crate::case::RESERVED.contains(&name.as_str()))
                {
                    failures.push(format!(
                        "measure: metric `{name}` is reserved; only the harness times the engine and reads its peak RSS"
                    ));
                }
                failures.extend(record::judge(&bounds, &metrics));
                measured = metrics;
            }
            Err(e) => failures.push(format!("measure: {e}")),
        }
        let judged = |metrics: Metrics| record::judge(&bounds, &metrics);
        let reads = evidence.reads.take();
        for name in names {
            if !reads.iter().any(|(input, _)| input == name) {
                failures.push(format!(
                    "measure never read the output of `{name}`; each input must matter"
                ));
            }
        }
        for (input, field) in &reads {
            let random = seeds(self.seed)
                .flat_map(|seed| [false, true].map(|mirror| Move::Random { seed, mirror }));
            let moves: Vec<Move> = [Move::Up, Move::Down].into_iter().chain(random).collect();
            let missed = moves
                .iter()
                .filter(
                    |&&how| match measure(&evidence.perturbed(input, field, how)) {
                        Ok(metrics) => judged(metrics).is_empty(),
                        Err(_) => false,
                    },
                )
                .count();
            if missed > 0 {
                failures.push(format!(
                    "measure passed {missed} of {} perturbed copies of `{input}.{field}`; its tolerance must reject a 1e-3 change",
                    moves.len()
                ));
            }
        }
        (failures, measured)
    }
}

fn readouts(metrics: &Metrics, tolerance: &BTreeMap<String, f64>) -> Vec<String> {
    let bounded: BTreeSet<&str> = (tolerance.keys())
        .filter_map(|key| crate::case::bounded(key).map(|(metric, _)| metric))
        .collect();
    (metrics.keys())
        .filter(|m| !bounded.contains(m.as_str()))
        .cloned()
        .collect()
}

fn preflight(
    dir: &Path,
    inputs: &[(String, String)],
    tables: &[toml::Table],
    held: &BTreeSet<String>,
    measure: Measure,
) -> Option<String> {
    let mut missing = BTreeSet::new();
    for ((name, _), table) in
        (inputs.iter().zip(tables)).filter(|((name, _), _)| !held.contains(name))
    {
        let evidence = Evidence::new(
            dir,
            vec![(name.clone(), table.clone())],
            vec![(name.clone(), Value::Null)],
        );
        let _ = measure(&evidence);
        missing.append(&mut evidence.missing.take());
    }
    (!missing.is_empty()).then(|| {
        let files: Vec<String> = missing.into_iter().collect();
        format!(
            "{} missing; the claim stops before it times anything",
            files.join(", ")
        )
    })
}

fn seeds(base: u64) -> impl Iterator<Item = u64> {
    (1..=RANDOM_SEEDS).map(move |k| base ^ k.wrapping_mul(0x9e37_79b9_7f4a_7c15))
}

fn unix_ms() -> u128 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis())
}

#[derive(Default)]
struct Trace {
    inputs: Vec<String>,
    runs_s: Vec<f64>,
    peak_rss_bytes: Vec<u64>,
    peak_pss_bytes: Vec<u64>,
    rotation_s: Vec<f64>,
    rotation: Option<engine::Change>,
    speed: Vec<Value>,
    skipped: Vec<Value>,
    versus: Vec<Value>,
    held_out: Vec<Value>,
    tuned: Vec<String>,
    relaxations: Vec<Value>,
    table: Option<Value>,
    spent_s: f64,
    metrics: Metrics,
    faulted: bool,
}

impl Trace {
    fn spend(&mut self, clock: Instant) {
        self.spent_s += clock.elapsed().as_secs_f64();
    }

    fn failed(&mut self, input: &str, failure: engine::Failure) -> String {
        match failure {
            engine::Failure::Harness(e) => {
                self.faulted = true;
                format!("{input}: harness: {e}")
            }
            engine::Failure::Engine(e) => format!("{input}: {e}"),
        }
    }
}

pub(crate) struct Job {
    pub(crate) at: String,
    pub(crate) atoms: usize,
    table: toml::Table,
    files: Vec<(String, Vec<u8>)>,
    timeout: Duration,
}

impl Job {
    pub(crate) fn at_claim_cutoff(
        &self,
        cutoffs: &rival::ClaimCutoff,
    ) -> Result<(Job, (f64, f64)), String> {
        let named = |e: String| format!("{}: {e}", self.at);
        let (table, pair) = at_abinit_cutoff(&self.table, cutoffs).map_err(named)?;
        let job = Job {
            at: self.at.clone(),
            atoms: self.atoms,
            table,
            files: self.files.clone(),
            timeout: self.timeout,
        };
        Ok((job, pair))
    }

    pub(crate) fn run(
        &self,
        bin: &Path,
        threads: NonZeroUsize,
        steps: bool,
    ) -> Result<engine::Run, String> {
        let named = |e: String| format!("{}: {e}", self.at);
        let mut table = self.table.clone();
        at_threads(&mut table, threads, steps).map_err(named)?;
        let text = toml::to_string(&table).map_err(|e| named(e.to_string()))?;
        engine::once(bin, &text, &self.files, self.timeout).map_err(|f| match f {
            engine::Failure::Harness(e) => named(format!("harness: {e}")),
            engine::Failure::Engine(e) => named(e),
        })
    }
}

fn at_threads(table: &mut toml::Table, p: NonZeroUsize, steps: bool) -> Result<(), String> {
    engine::set(table, "parallel", "threads", (p.get() as i64).into())?;
    if steps {
        engine::set(table, "outputs", "step_energies", true.into())?;
    }
    Ok(())
}

pub(crate) fn jobs(root: &Path, id: &str) -> Result<Vec<Job>, String> {
    let timeout = Duration::from_secs(Case::load(root, id)?.timeout_s());
    let mut out = Vec::new();
    let (inputs, own) = inputs(root, &root.join("benchmarks").join(id), None)?;
    for (name, text) in inputs {
        let at = format!("{id}/{name}");
        let named = |e: String| format!("{at}: {e}");
        let table: toml::Table = toml::from_str(&text).map_err(|e| named(e.to_string()))?;
        let species = Input(&table).species().map_err(named)?;
        let files = datasets::files(root, own.as_ref(), &Input(&table)).map_err(named)?;
        out.push(Job {
            at,
            atoms: species.len(),
            table,
            files,
            timeout,
        });
    }
    Ok(out)
}

type Named = Vec<(String, String)>;

pub(crate) fn inputs(
    root: &Path,
    dir: &Path,
    candidate: Option<&Path>,
) -> Result<(Named, Option<datasets::Table>), String> {
    let table = datasets::Table::of_case(root, dir, candidate)?;
    let named = |path: &Path, e: std::io::Error| format!("{}: {e}", path.display());
    let mut out = Vec::new();
    let first = dir.join(crate::case::INPUT_FILE);
    match fs::read_to_string(&first) {
        Ok(text) => out.push(("input".to_owned(), text)),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(named(&first, e)),
    }
    let folder = dir.join(crate::case::INPUTS_DIR);
    let mut more: Vec<PathBuf> = match fs::read_dir(&folder) {
        Ok(entries) => entries
            .map(|entry| entry.map(|e| e.path()))
            .collect::<Result<_, _>>()
            .map_err(|e| named(&folder, e))?,
        Err(e) if e.kind() == ErrorKind::NotFound => Vec::new(),
        Err(e) => return Err(named(&folder, e)),
    };
    more.sort();
    for path in more {
        let stem = path.file_stem().map(|s| s.to_string_lossy().into_owned());
        match (stem, path.extension().is_some_and(|x| x == "toml")) {
            (Some(stem), true) if stem == "input" => {
                return Err(format!(
                    "{}: the name `input` belongs to input.toml; rename this file",
                    path.display()
                ));
            }
            (Some(stem), true) if stem.contains('@') => {
                return Err(format!(
                    "{}: `@` marks the generated inputs of crystals.json; rename this file",
                    path.display()
                ));
            }
            (Some(stem), true) => {
                let text = fs::read_to_string(&path).map_err(|e| named(&path, e))?;
                out.push((stem, text));
            }
            _ => {
                return Err(format!(
                    "{}: inputs/ holds only .toml files",
                    path.display()
                ));
            }
        }
    }
    let crystals = dir.join(crate::case::CRYSTALS);
    match fs::read_to_string(&crystals) {
        Ok(text) => out.extend(acwf::expand(root, &text, table.as_ref())?),
        Err(e) if e.kind() == ErrorKind::NotFound => {}
        Err(e) => return Err(named(&crystals, e)),
    }
    if let Some(table) = &table {
        acwf::Split::read(root)?.blind(out.iter().map(|(name, _)| name.as_str()))?;
        for (name, text) in &mut out {
            let parsed: toml::Table = toml::from_str(text).map_err(|e| format!("{name}: {e}"))?;
            let species = Input(&parsed)
                .species()
                .map_err(|e| format!("{name}: {e}"))?;
            *text = table
                .with_files(text, &species)
                .map_err(|e| format!("{name}: {e}"))?;
        }
    }
    Ok((out, table))
}

pub(crate) fn filter_refused(tier: Tier) -> String {
    format!(
        "ORBITAR_CASE_FILTER runs only in the node tier, where case 10's shards run; a {} run covers every input",
        tier.name()
    )
}

pub(crate) fn selected(names: &[&str], selectors: &[String]) -> Result<Vec<String>, String> {
    if selectors.is_empty() {
        return Err("names no selector".into());
    }
    let picks = |name: &str, s: &str| {
        let system = acwf::system_of(name);
        name == s
            || system == Some(s)
            || system
                .and_then(|n| n.split_once('-'))
                .is_some_and(|(e, _)| e == s)
    };
    let unknown: Vec<&str> = (selectors.iter())
        .filter(|s| !names.iter().any(|name| picks(name, s)))
        .map(String::as_str)
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "names no input of this case: {}",
            unknown.join(", ")
        ));
    }
    Ok((names.iter())
        .filter(|name| selectors.iter().any(|s| picks(name, s)))
        .map(|name| (*name).to_owned())
        .collect())
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Discretization {
    pub operations: Vec<DiscreteOperation>,
    pub coarse_grid: [u32; 3],
    pub fine_grid: [u32; 3],
    pub plane_waves: String,
    pub density_spheres: [String; 2],
    pub nyquist: bool,
}

#[derive(Clone, Debug, PartialEq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DiscreteOperation {
    pub rotation: [[i32; 3]; 3],
    pub translation: [f64; 3],
}

impl Discretization {
    pub fn of(value: &Value) -> Result<Self, String> {
        if value.is_null() {
            return Err("no discretization; set outputs.discretization".into());
        }
        let record: Discretization = serde_json::from_value(value.clone())
            .map_err(|e| format!("discretization is not a whole record: {e}"))?;
        let identity = [[1, 0, 0], [0, 1, 0], [0, 0, 1]];
        if !record.operations.is_empty()
            && !record.operations.iter().any(|o| o.rotation == identity)
        {
            return Err("discretization: operations without the identity".into());
        }
        if record.coarse_grid.contains(&0) || record.fine_grid.contains(&0) {
            return Err("discretization: a grid with a zero dimension".into());
        }
        let hex = |h: &String| h.len() == 16 && h.chars().all(|c| c.is_ascii_hexdigit());
        if !hex(&record.plane_waves) || !record.density_spheres.iter().all(hex) {
            return Err("discretization: a signature that is not 16 hex digits".into());
        }
        Ok(record)
    }
}

pub use record::{readout, union, verify};
pub use speed::{G_MIN, G_RESAMPLES, G_SEED, g_interval};

pub fn main(cases: &[(&'static str, Measure)]) -> ! {
    let args = Arguments::from_args();
    let session =
        crate::root().and_then(|root| Session::from_vars(&root, |key: &str| std::env::var_os(key)));
    let trials = match session {
        Ok(Some(session)) => cases
            .iter()
            .map(|&(id, measure)| {
                let session = session.clone();
                Trial::test(id, move || match session.evaluate(id, measure) {
                    Outcome::Pass | Outcome::Skipped => Ok(()),
                    Outcome::Planned(why) => {
                        println!("{id}: planned, still fails: {why}");
                        Ok(())
                    }
                    Outcome::Shard => {
                        println!("{id}: shard passes; `cargo xtask shards` judges the case on every shard");
                        Ok(())
                    }
                    Outcome::Promote => Err(Failed::from(
                        "planned case passes: set status = \"active\" and lock it",
                    )),
                    Outcome::Fail(why) => Err(Failed::from(why)),
                })
            })
            .collect(),
        Ok(None) => {
            eprintln!("benchmarks: ORBITAR_BIN is unset; run `cargo xtask smoke`");
            cases
                .iter()
                .map(|&(id, _)| Trial::test(id, || Ok(())).with_ignored_flag(true))
                .collect()
        }
        Err(e) => vec![Trial::test("session", move || Err(Failed::from(e.clone())))],
    };
    libtest_mimic::run(&args, trials).exit()
}

fn git(root: &Path, args: &[&str]) -> String {
    crate::git(root, args).unwrap_or_else(|_| "unknown".into())
}

fn rustc() -> String {
    Command::new("rustc").arg("--version").output().map_or_else(
        |_| "unknown".into(),
        |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
    )
}
