use super::diagnosis::Diagnosis;
use super::scf::{Energies, Warm, ground_state};
use crate::model::{KPoints, Mixer, Spec};
use crate::paw::Dataset;
use crate::pw::Basis;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    Invalid(String),
    Unsupported(String),
    Unconverged(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    kpoints: KPoints,
    basis_sizes: Vec<usize>,
    eigenvalues_ha: Vec<Vec<Vec<f64>>>,
    scf: Option<Scf>,
    discretization: Option<Discretization>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Discretization {
    pub operations: Vec<([[i32; 3]; 3], [f64; 3])>,
    pub coarse_grid: [usize; 3],
    pub fine_grid: [usize; 3],
    pub plane_waves: u64,
    pub density_spheres: [u64; 2],
    pub nyquist: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Scf {
    pub energies: Energies,
    pub fermi_level_ha: f64,
    pub occupations: Vec<Vec<Vec<f64>>>,
    pub magnetization_bohr_magneton: Option<f64>,
    pub forces_ha_bohr: Option<Vec<[f64; 3]>>,
    pub stress_ha_bohr3: Option<[[f64; 3]; 3]>,
    pub steps: usize,
    pub free_dc_ha: f64,
    pub kinetic_density_integral_ha: Option<f64>,
}

impl Outcome {
    pub fn kpoints(&self) -> &KPoints {
        &self.kpoints
    }

    pub fn basis_sizes(&self) -> &[usize] {
        &self.basis_sizes
    }

    pub fn eigenvalues_ha(&self) -> &[Vec<Vec<f64>>] {
        &self.eigenvalues_ha
    }

    pub fn scf(&self) -> Option<&Scf> {
        self.scf.as_ref()
    }

    pub fn discretization(&self) -> Option<&Discretization> {
        self.discretization.as_ref()
    }
}

pub fn run(
    spec: &Spec,
    dataset: impl FnMut(u8) -> Result<Dataset, Failure>,
    on_step: impl FnMut(usize, f64),
) -> Result<Outcome, Failure> {
    Session::default().run(spec, dataset, on_step)
}

#[derive(Default)]
pub struct Session {
    warm: Option<Warm>,
}

impl Session {
    pub fn run(
        &mut self,
        spec: &Spec,
        dataset: impl FnMut(u8) -> Result<Dataset, Failure>,
        on_step: impl FnMut(usize, f64),
    ) -> Result<Outcome, Failure> {
        let warm = self.warm.take();
        let (outcome, next, _) = solve(spec, dataset, warm.as_ref(), false, on_step)?;
        self.warm = next;
        Ok(outcome)
    }
}

pub fn run_diagnosed(
    spec: &Spec,
    dataset: impl FnMut(u8) -> Result<Dataset, Failure>,
    on_step: impl FnMut(usize, f64),
) -> Result<(Outcome, Option<Diagnosis>), Failure> {
    let (outcome, _, diagnosis) = solve(spec, dataset, None, true, on_step)?;
    Ok((outcome, diagnosis))
}

pub(super) fn checked(spec: &Spec, z: u8, given: Dataset) -> Result<Dataset, Failure> {
    let (held, functional) = (given.draft().z, given.draft().functional);
    if held != f64::from(z) {
        return Err(Failure::Invalid(format!(
            "datasets: the dataset given for Z = {z} holds Z = {held}"
        )));
    }
    if functional.dataset() != spec.functional().dataset() {
        return Err(Failure::Invalid(format!(
            "xc.functional: {:?}, but the dataset for Z = {z} is {functional:?}",
            spec.functional()
        )));
    }
    crate::paw::core_kinetic(&given, spec.functional()).map_err(Failure::Invalid)?;
    Ok(given)
}

fn solve(
    spec: &Spec,
    mut dataset: impl FnMut(u8) -> Result<Dataset, Failure>,
    warm: Option<&Warm>,
    diagnose: bool,
    on_step: impl FnMut(usize, f64),
) -> Result<(Outcome, Option<Warm>, Option<Diagnosis>), Failure> {
    let draft = spec.draft();
    if draft.probe.is_some() {
        return Err(Failure::Invalid(
            "probe: a probe input has no bands; ks::probe evaluates it".into(),
        ));
    }
    if draft.atoms.is_empty() {
        let bands = draft
            .bands
            .expect("Spec::try_from requires a band count without atoms");
        return empty_lattice(spec, bands).map(|outcome| (outcome, None, None));
    }
    if draft.mixer != Mixer::Pulay {
        return Err(Failure::Unsupported(
            "scf.mixer: only \"pulay\" is implemented".into(),
        ));
    }
    let mut elements: Vec<u8> = Vec::new();
    let mut datasets = Vec::new();
    for &(z, _) in &draft.atoms {
        if !elements.contains(&z) {
            let given = checked(spec, z, dataset(z)?)?;
            elements.push(z);
            datasets.push(given);
        }
    }
    for (i, (&(z, _), m)) in draft
        .atoms
        .iter()
        .zip(draft.moments.iter().flatten())
        .enumerate()
    {
        let kind = elements
            .iter()
            .position(|&e| e == z)
            .expect("each element has its dataset");
        let valence = datasets[kind].draft().valence_electrons;
        if m.abs() > valence {
            return Err(Failure::Invalid(format!(
                "spin.moments: atom {i} starts at {m} μB, beyond its {valence} valence electrons"
            )));
        }
    }
    let (ground, next) = ground_state(spec, &datasets, warm, diagnose, on_step)?;
    let outcome = Outcome {
        kpoints: ground.kpoints,
        basis_sizes: ground.basis_sizes,
        eigenvalues_ha: ground.eigenvalues,
        scf: Some(ground.scf),
        discretization: ground.discretization,
    };
    Ok((outcome, Some(next), ground.diagnosis))
}

fn empty_lattice(spec: &Spec, bands: usize) -> Result<Outcome, Failure> {
    let draft = spec.draft();
    let (mut basis_sizes, mut channel) = (Vec::new(), Vec::new());
    for (i, &k) in draft.kpoints.points().iter().enumerate() {
        let basis = Basis::sphere(&draft.cell, k, draft.cutoff_ha);
        let mut energies = basis.kinetic_ha().to_vec();
        energies.sort_by(f64::total_cmp);
        if energies.len() < bands {
            return Err(Failure::Invalid(format!(
                "bands.count: {bands} bands, but k-point {i} has only {} plane waves",
                energies.len()
            )));
        }
        basis_sizes.push(energies.len());
        channel.push(energies[..bands].to_vec());
    }
    Ok(Outcome {
        kpoints: draft.kpoints.clone(),
        basis_sizes,
        eigenvalues_ha: vec![channel],
        scf: None,
        discretization: None,
    })
}
