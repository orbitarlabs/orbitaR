use super::{Cell, KPoints};
use std::collections::BTreeMap;
use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Functional {
    Lda,
    Pbe,
    R2scan,
}

pub const DEFAULT_FAMILY: &str = "jth-2.0";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Smearing {
    Fixed,
    FermiDirac,
    Gaussian,
    MethfesselPaxton,
    MarzariVanderbilt,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Solver {
    Lobpcg,
    Dense,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mixer {
    Linear,
    Pulay,
    Broyden,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Probe {
    Ewald(Vec<(f64, [f64; 3])>),
    Hartree(Vec<(f64, f64, [f64; 3])>),
    Xc {
        points: Vec<[f64; 5]>,
        tau: Option<Vec<[f64; 2]>>,
    },
    Paw {
        z: u8,
        rho: Vec<Vec<Vec<f64>>>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Draft {
    pub cell: Cell,
    pub atoms: Vec<(u8, [f64; 3])>,
    pub cutoff_ha: f64,
    pub density_cutoff_ha: Option<f64>,
    pub kpoints: KPoints,
    pub bands: Option<usize>,
    pub solver: Solver,
    pub functional: Option<Functional>,
    pub family: String,
    pub dataset_directory: String,
    pub dataset_files: BTreeMap<u8, String>,
    pub moments: Option<Vec<f64>>,
    pub smearing: Option<Smearing>,
    pub width_ha: Option<f64>,
    pub energy_tolerance_ha: f64,
    pub max_iterations: usize,
    pub mixer: Mixer,
    pub forces: bool,
    pub stress: bool,
    pub step_energies: bool,
    pub discretization: bool,
    pub probe: Option<Probe>,
    pub threads: Option<NonZeroUsize>,
}

impl Draft {
    pub fn required(cell: Cell, cutoff_ha: f64, kpoints: KPoints) -> Self {
        Self {
            cell,
            atoms: Vec::new(),
            cutoff_ha,
            density_cutoff_ha: None,
            kpoints,
            bands: None,
            solver: Solver::Lobpcg,
            functional: None,
            family: DEFAULT_FAMILY.to_owned(),
            dataset_directory: "datasets".to_owned(),
            dataset_files: BTreeMap::new(),
            moments: None,
            smearing: None,
            width_ha: None,
            energy_tolerance_ha: 1e-9,
            max_iterations: 100,
            mixer: Mixer::Pulay,
            forces: false,
            stress: false,
            step_energies: false,
            discretization: false,
            probe: None,
            threads: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Spec(Draft);

impl TryFrom<Draft> for Spec {
    type Error = String;

    fn try_from(draft: Draft) -> Result<Self, String> {
        rule(
            draft.atoms.iter().all(|(z, _)| (1..=118).contains(z)),
            "structure.species: every atomic number must lie in 1..=118",
        )?;
        rule(
            draft
                .atoms
                .iter()
                .all(|(_, p)| p.iter().all(|x| x.is_finite())),
            "structure.positions_fractional: every entry must be finite",
        )?;
        let sites: Vec<[f64; 3]> = draft.atoms.iter().map(|&(_, at)| at).collect();
        rule(
            distinct(&sites),
            "structure.positions_fractional: two atoms share a site",
        )?;
        let probed = match &draft.probe {
            Some(Probe::Paw { z, .. }) => Some(*z),
            _ => None,
        };
        rule(
            (draft.dataset_files.keys())
                .all(|z| Some(*z) == probed || draft.atoms.iter().any(|(atom, _)| atom == z)),
            "datasets.files: each element must be a species of the structure or the paw probe's element",
        )?;
        if let Some(path) = draft.dataset_files.values().find(|path| !inside(path)) {
            return Err(format!(
                "datasets.files: {path:?} must be relative to datasets.directory, with no `.` or `..`"
            ));
        }
        rule(
            positive(draft.cutoff_ha),
            "basis.cutoff_ha must be positive",
        )?;
        rule(
            draft
                .density_cutoff_ha
                .is_none_or(|e| e.is_finite() && e >= draft.cutoff_ha),
            "basis.density_cutoff_ha must be finite and at least basis.cutoff_ha",
        )?;
        rule(
            draft.probe.is_some() || draft.bands.map_or(!draft.atoms.is_empty(), |n| n > 0),
            "bands.count: at least 1, and required without atoms",
        )?;
        if let Some(probe) = &draft.probe {
            rule(
                draft.atoms.is_empty() && draft.bands.is_none(),
                "probe: a probe input has no species and no bands",
            )?;
            probe_rules(probe, draft.functional.is_some())?;
        }
        rule(
            !draft.atoms.is_empty() || !(draft.forces || draft.stress),
            "outputs.forces, outputs.stress: forces and stress need atoms",
        )?;
        if let Some(moments) = &draft.moments {
            rule(
                !draft.atoms.is_empty(),
                "spin.moments: a spin-polarized run needs atoms",
            )?;
            rule(
                moments.len() == draft.atoms.len() && moments.iter().all(|m| m.is_finite()),
                "spin.moments: give one finite moment per atom",
            )?;
            rule(
                draft.smearing.is_some_and(|s| s != Smearing::Fixed),
                "spin.moments: a spin-polarized run needs a smearing other than fixed",
            )?;
        }
        match (draft.smearing, draft.width_ha) {
            (None | Some(Smearing::Fixed), None) => Ok(()),
            (None, Some(_)) => Err("occupations.width_ha needs occupations.smearing".to_owned()),
            (Some(Smearing::Fixed), Some(_)) => {
                Err("occupations.width_ha: fixed occupations take no width".to_owned())
            }
            (Some(_), width) => rule(
                width.is_some_and(positive),
                "occupations.width_ha: every smearing but fixed needs a positive width",
            ),
        }?;
        rule(
            positive(draft.energy_tolerance_ha),
            "scf.energy_tolerance_ha must be positive",
        )?;
        rule(
            draft.max_iterations > 0,
            "scf.max_iterations must be at least 1",
        )?;
        Ok(Self(draft))
    }
}

fn probe_rules(probe: &Probe, functional: bool) -> Result<(), String> {
    let finite = |q: f64, at: &[f64; 3]| q.is_finite() && at.iter().all(|x| x.is_finite());
    let charged = |ok: bool| {
        rule(
            ok,
            "probe.charges: give at least one finite charge at a finite position",
        )
    };
    match probe {
        Probe::Ewald(charges) => {
            charged(!charges.is_empty() && charges.iter().all(|(q, at)| finite(*q, at)))?;
            let sites: Vec<[f64; 3]> = charges.iter().map(|&(_, at)| at).collect();
            rule(
                distinct(&sites),
                "probe.positions_fractional: two charges share a site",
            )
        }
        Probe::Hartree(gaussians) => {
            charged(!gaussians.is_empty() && gaussians.iter().all(|(q, _, at)| finite(*q, at)))?;
            rule(
                gaussians.iter().all(|&(_, a, _)| positive(a)),
                "probe.exponents: every exponent must be positive",
            )
        }
        Probe::Xc { points, tau } => {
            rule(functional, "xc.functional: an xc probe needs a functional")?;
            if let Some(tau) = tau {
                rule(
                    tau.len() == points.len()
                        && tau.iter().flatten().all(|t| t.is_finite() && *t >= 0.0),
                    "probe.points: give τ↑, τ↓ ≥ 0 at every point or at none",
                )?;
            }
            let density = |&[up, down, uu, ud, dd]: &[f64; 5]| {
                [up, down, uu, ud, dd].iter().all(|x| x.is_finite())
                    && up >= 0.0
                    && down >= 0.0
                    && up + down > 0.0
                    && uu >= 0.0
                    && dd >= 0.0
                    && ud * ud <= uu * dd
            };
            rule(
                !points.is_empty() && points.iter().all(density),
                "probe.points: give ρ↑, ρ↓ ≥ 0 with a positive sum, σ↑↑, σ↓↓ ≥ 0, and σ↑↓² ≤ σ↑↑ σ↓↓",
            )
        }
        Probe::Paw { rho, .. } => {
            rule(functional, "xc.functional: a paw probe needs a functional")?;
            let n = rho.first().map_or(0, Vec::len);
            let square = |m: &Vec<Vec<f64>>| {
                m.len() == n
                    && (m.iter()).all(|row| row.len() == n && row.iter().all(|x| x.is_finite()))
                    && (0..n).all(|i| (0..i).all(|j| m[i][j] == m[j][i]))
            };
            rule(
                matches!(rho.len(), 1 | 2) && n > 0 && rho.iter().all(square),
                "probe.rho_ij: give one or two finite symmetric matrices (ρ↑, ρ↓) of one size",
            )
        }
    }
}

fn distinct(sites: &[[f64; 3]]) -> bool {
    sites.iter().enumerate().all(|(i, a)| {
        sites[i + 1..].iter().all(|b| {
            (0..3).any(|k| {
                let d = a[k] - b[k];
                (d - d.round()).abs() > 1e-12
            })
        })
    })
}

pub(crate) fn inside(path: &str) -> bool {
    (path.split('/')).all(|name| !matches!(name, "" | "." | ".."))
}

pub(crate) fn rule(ok: bool, message: &str) -> Result<(), String> {
    if ok { Ok(()) } else { Err(message.to_owned()) }
}

fn positive(x: f64) -> bool {
    x > 0.0 && x.is_finite()
}

impl Spec {
    pub fn draft(&self) -> &Draft {
        &self.0
    }

    pub fn functional(&self) -> Functional {
        self.0.functional.unwrap_or(Functional::Pbe)
    }

    pub fn density_cutoff_ha(&self) -> f64 {
        self.0.density_cutoff_ha.unwrap_or(2.0 * self.0.cutoff_ha)
    }

    pub fn threads(&self) -> NonZeroUsize {
        (self.0.threads)
            .unwrap_or_else(|| std::thread::available_parallelism().unwrap_or(NonZeroUsize::MIN))
    }
}
