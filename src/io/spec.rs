use crate::model::{Cell, Draft, Functional, KPoints, Mixer, Probe, Smearing, Solver, Spec};
use crate::xc::Rung;
use serde::Deserialize;
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

const ELEMENTS: &str = "H He Li Be B C N O F Ne Na Mg Al Si P S Cl Ar K Ca Sc Ti V Cr Mn Fe Co Ni \
    Cu Zn Ga Ge As Se Br Kr Rb Sr Y Zr Nb Mo Tc Ru Rh Pd Ag Cd In Sn Sb Te I Xe Cs Ba La Ce Pr Nd \
    Pm Sm Eu Gd Tb Dy Ho Er Tm Yb Lu Hf Ta W Re Os Ir Pt Au Hg Tl Pb Bi Po At Rn Fr Ra Ac Th Pa U \
    Np Pu Am Cm Bk Cf Es Fm Md No Lr Rf Db Sg Bh Hs Mt Ds Rg Cn Nh Fl Mc Lv Ts Og";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    schema: u32,
    structure: Structure,
    basis: BasisKeys,
    kpoints: KPointKeys,
    #[serde(default)]
    bands: BandKeys,
    #[serde(default)]
    xc: XcKeys,
    #[serde(default)]
    datasets: DatasetKeys,
    #[serde(default)]
    spin: SpinKeys,
    #[serde(default)]
    occupations: OccupationKeys,
    #[serde(default)]
    scf: ScfKeys,
    #[serde(default)]
    outputs: OutputKeys,
    #[serde(default)]
    parallel: ParallelKeys,
    probe: Option<ProbeKeys>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Structure {
    cell_bohr: [[f64; 3]; 3],
    #[serde(default)]
    species: Vec<String>,
    #[serde(default)]
    positions_fractional: Vec<[f64; 3]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BasisKeys {
    cutoff_ha: f64,
    density_cutoff_ha: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct KPointKeys {
    mesh: Option<[usize; 3]>,
    shift: Option<[f64; 3]>,
    list: Option<Vec<[f64; 3]>>,
    symmetry: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct BandKeys {
    count: Option<usize>,
    solver: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct XcKeys {
    functional: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct DatasetKeys {
    family: Option<String>,
    directory: Option<String>,
    #[serde(default)]
    files: BTreeMap<String, String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct SpinKeys {
    moments: Option<Vec<f64>>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct OccupationKeys {
    smearing: Option<String>,
    width_ha: Option<f64>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ScfKeys {
    energy_tolerance_ha: Option<f64>,
    max_iterations: Option<usize>,
    mixer: Option<String>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct OutputKeys {
    forces: Option<bool>,
    stress: Option<bool>,
    step_energies: Option<bool>,
    discretization: Option<bool>,
}

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ParallelKeys {
    threads: Option<usize>,
}

#[derive(Deserialize)]
#[serde(tag = "term", rename_all = "lowercase", deny_unknown_fields)]
enum ProbeKeys {
    Ewald {
        charges: Vec<f64>,
        positions_fractional: Vec<[f64; 3]>,
    },
    Hartree {
        charges: Vec<f64>,
        exponents: Vec<f64>,
        positions_fractional: Vec<[f64; 3]>,
    },
    Xc {
        points: Vec<Vec<f64>>,
    },
    Paw {
        rho_ij: BTreeMap<String, Vec<Vec<Vec<f64>>>>,
    },
}

pub fn read_spec(text: &str) -> Result<Spec, String> {
    let input: Input = toml::from_str(text).map_err(|e| e.to_string())?;
    if input.schema != 1 {
        return Err(format!(
            "schema: {} is unknown; this engine reads schema 1",
            input.schema
        ));
    }
    let structure = input.structure;
    if structure.species.len() != structure.positions_fractional.len() {
        return Err("structure.positions_fractional: give one row per species entry".into());
    }
    let atoms = structure
        .species
        .iter()
        .zip(structure.positions_fractional)
        .map(|(symbol, position)| Ok((atomic_number("structure.species", symbol)?, position)))
        .collect::<Result<_, String>>()?;
    let k = input.kpoints;
    let kpoints = match (k.mesh, k.shift, k.list) {
        (Some(mesh), shift, None) => KPoints::mesh(mesh, shift.unwrap_or_default())?,
        (None, None, Some(list)) => KPoints::list(list)?,
        _ => return Err("kpoints: give mesh, with an optional shift, or list".into()),
    };
    let kpoints = match k.symmetry {
        Some(false) => kpoints.as_given(),
        Some(true) if !kpoints.reducible() => {
            return Err("kpoints.symmetry: a list is used as given".into());
        }
        _ => kpoints,
    };
    let cell = Cell::try_from(structure.cell_bohr)?;
    let mut draft = Draft::required(cell, input.basis.cutoff_ha, kpoints);
    let (xc, occupations, scf, outputs) = (input.xc, input.occupations, input.scf, input.outputs);
    draft.atoms = atoms;
    draft.density_cutoff_ha = input.basis.density_cutoff_ha;
    draft.bands = input.bands.count;
    let solvers = [("lobpcg", Solver::Lobpcg), ("dense", Solver::Dense)];
    draft.solver = pick("bands.solver", input.bands.solver, &solvers)?.unwrap_or(draft.solver);
    let functionals = Functional::ALL.map(|row| (row.word, row.functional));
    draft.functional = pick("xc.functional", xc.functional, &functionals)?;
    if let Some(family) = input.datasets.family {
        super::family::plain_name(&family).map_err(|e| format!("datasets.family: {e}"))?;
        draft.family = family;
    }
    draft.dataset_directory = input.datasets.directory.unwrap_or(draft.dataset_directory);
    draft.dataset_files = (input.datasets.files.into_iter())
        .map(|(symbol, path)| Ok((atomic_number("datasets.files", &symbol)?, path)))
        .collect::<Result<_, String>>()?;
    draft.smearing = pick(
        "occupations.smearing",
        occupations.smearing,
        &[
            ("fixed", Smearing::Fixed),
            ("fermi-dirac", Smearing::FermiDirac),
            ("gaussian", Smearing::Gaussian),
            ("methfessel-paxton", Smearing::MethfesselPaxton),
            ("marzari-vanderbilt", Smearing::MarzariVanderbilt),
        ],
    )?;
    draft.moments = input.spin.moments;
    draft.width_ha = occupations.width_ha;
    draft.energy_tolerance_ha = scf.energy_tolerance_ha.unwrap_or(draft.energy_tolerance_ha);
    draft.max_iterations = scf.max_iterations.unwrap_or(draft.max_iterations);
    let mixers = [
        ("linear", Mixer::Linear),
        ("pulay", Mixer::Pulay),
        ("broyden", Mixer::Broyden),
    ];
    draft.mixer = pick("scf.mixer", scf.mixer, &mixers)?.unwrap_or(draft.mixer);
    draft.forces = outputs.forces.unwrap_or(draft.forces);
    draft.stress = outputs.stress.unwrap_or(draft.stress);
    draft.step_energies = outputs.step_energies.unwrap_or(draft.step_energies);
    draft.discretization = outputs.discretization.unwrap_or(draft.discretization);
    draft.probe = input.probe.map(probe).transpose()?;
    if let (Some(Probe::Xc { tau: None, .. }), Some(functional)) = (&draft.probe, draft.functional)
        && functional.rung() == Rung::MetaGga
    {
        return Err(format!(
            "probe.points: {} reads τ; give 7 values per point: ρ↑, ρ↓, σ↑↑, σ↑↓, σ↓↓, τ↑, τ↓",
            functional.row().word
        ));
    }
    draft.threads = (input.parallel.threads)
        .map(|n| NonZeroUsize::new(n).ok_or("parallel.threads must be at least 1"))
        .transpose()?;
    Spec::try_from(draft)
}

fn probe(keys: ProbeKeys) -> Result<Probe, String> {
    let per_charge = |key: &str, n: usize, len: usize| {
        (n == len)
            .then_some(())
            .ok_or_else(|| format!("probe.{key}: give one entry per charge"))
    };
    Ok(match keys {
        ProbeKeys::Ewald {
            charges,
            positions_fractional: at,
        } => {
            per_charge("positions_fractional", charges.len(), at.len())?;
            Probe::Ewald(charges.into_iter().zip(at).collect())
        }
        ProbeKeys::Hartree {
            charges,
            exponents,
            positions_fractional: at,
        } => {
            per_charge("positions_fractional", charges.len(), at.len())?;
            per_charge("exponents", charges.len(), exponents.len())?;
            let gaussians = charges.into_iter().zip(exponents).zip(at);
            Probe::Hartree(gaussians.map(|((q, a), at)| (q, a, at)).collect())
        }
        ProbeKeys::Xc { points } => xc_points(&points)?,
        ProbeKeys::Paw { rho_ij } => {
            let mut elements = rho_ij.into_iter();
            let (Some((symbol, rho)), None) = (elements.next(), elements.next()) else {
                return Err("probe.rho_ij: give the occupancies of one element".into());
            };
            Probe::Paw {
                z: atomic_number("probe.rho_ij", &symbol)?,
                rho,
            }
        }
    })
}

fn xc_points(rows: &[Vec<f64>]) -> Result<Probe, String> {
    let width = rows.first().map_or(5, Vec::len);
    if !matches!(width, 5 | 7) || rows.iter().any(|row| row.len() != width) {
        return Err("probe.points: give 5 values per point (ρ↑, ρ↓, σ↑↑, σ↑↓, σ↓↓) or 7 (then τ↑, τ↓), the same at every point".into());
    }
    let points = rows
        .iter()
        .map(|row| std::array::from_fn(|k| row[k]))
        .collect();
    let tau = (width == 7).then(|| rows.iter().map(|row| [row[5], row[6]]).collect());
    Ok(Probe::Xc { points, tau })
}

pub(super) fn symbol(z: u8) -> &'static str {
    ELEMENTS
        .split_whitespace()
        .nth(usize::from(z) - 1)
        .expect("Spec::try_from keeps atomic numbers in 1..=118")
}

pub(super) fn atomic_number(key: &str, symbol: &str) -> Result<u8, String> {
    ELEMENTS
        .split_whitespace()
        .position(|element| element == symbol)
        .map(|i| i as u8 + 1)
        .ok_or_else(|| format!("{key}: {symbol:?} is not an element symbol"))
}

fn pick<T: Copy>(
    key: &str,
    value: Option<String>,
    options: &[(&str, T)],
) -> Result<Option<T>, String> {
    let Some(value) = value else {
        return Ok(None);
    };
    match options.iter().find(|(name, _)| *name == value) {
        Some(&(_, option)) => Ok(Some(option)),
        None => {
            let names: Vec<&str> = options.iter().map(|(name, _)| *name).collect();
            Err(format!(
                "{key}: {value:?} is not one of {}",
                names.join(", ")
            ))
        }
    }
}
