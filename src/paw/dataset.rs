use super::RadialGrid;
use crate::model::{Functional, spec::rule};
use crate::xc::Rung;
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Bessel { radius: f64 },
    Sinc { radius: f64 },
}

impl Shape {
    pub fn radius(&self) -> f64 {
        match *self {
            Shape::Bessel { radius } | Shape::Sinc { radius } => radius,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Channel {
    pub l: usize,
    pub occupation: Option<f64>,
    pub ae: Vec<f64>,
    pub pseudo: Vec<f64>,
    pub projector: Vec<f64>,
}

impl Channel {
    pub fn electrons(&self) -> f64 {
        self.occupation.unwrap_or(0.0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct CoreKinetic {
    ae: Vec<f64>,
    pseudo: Vec<f64>,
}

impl CoreKinetic {
    pub fn checked(grid: &RadialGrid, ae: Vec<f64>, pseudo: Vec<f64>) -> Result<Self, String> {
        let size = grid.radii().len();
        for (tag, f) in [
            ("ae_core_kinetic_energy_density", &ae),
            ("pseudo_core_kinetic_energy_density", &pseudo),
        ] {
            rule(
                f.len() == size && f.iter().all(|x| x.is_finite()),
                &format!("{tag}: needs one finite value per grid point"),
            )?;
        }
        Ok(Self { ae, pseudo })
    }

    pub fn ae(&self) -> &[f64] {
        &self.ae
    }

    pub fn pseudo(&self) -> &[f64] {
        &self.pseudo
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DatasetDraft {
    pub z: f64,
    pub core_electrons: f64,
    pub valence_electrons: f64,
    pub functional: Functional,
    pub grid: RadialGrid,
    pub sphere_radius: f64,
    pub shape: Shape,
    pub channels: Vec<Channel>,
    pub kinetic_differences: Vec<f64>,
    pub core_density: Vec<f64>,
    pub pseudo_core_density: Vec<f64>,
    pub pseudo_valence_density: Vec<f64>,
    pub local_potential: Vec<f64>,
    pub core_kinetic: Option<CoreKinetic>,
}

#[derive(Clone, Debug)]
pub struct Dataset(DatasetDraft);

const RADIUS_DIGITS: f64 = 1e-12;

const CHARGE_RELATIVE: f64 = 1e-9;

const PSEUDO_CORE_DENSITY_FLOOR_PER_BOHR3: f64 = 1e-6;

impl TryFrom<DatasetDraft> for Dataset {
    type Error = String;

    fn try_from(mut draft: DatasetDraft) -> Result<Self, String> {
        let r = draft.grid.radii();
        let size = r.len();
        let n = draft.channels.len();
        rule(
            draft.z == draft.core_electrons + draft.valence_electrons
                && draft.valence_electrons > 0.0,
            "atom: Z must equal core + valence, with valence electrons",
        )?;
        rule(
            [
                &draft.core_density,
                &draft.pseudo_core_density,
                &draft.pseudo_valence_density,
                &draft.local_potential,
            ]
            .into_iter()
            .chain(
                draft
                    .channels
                    .iter()
                    .flat_map(|c| [&c.ae, &c.pseudo, &c.projector]),
            )
            .all(|f| f.len() == size && f.iter().all(|x| x.is_finite())),
            "radial parts: each needs one finite value per grid point",
        )?;
        rule(
            (draft.core_kinetic.iter()).all(|k| k.ae.len() == size),
            "core kinetic energy densities: need one value per point of the dataset grid",
        )?;
        rule(
            !matches!(draft.functional.rung(), Rung::MetaGga) || draft.core_kinetic.is_some(),
            &format!(
                "core kinetic energy densities: a dataset whose own functional is {}, a meta-GGA, needs them",
                draft.functional.row().word
            ),
        )?;
        let radius = draft.shape.radius();
        rule(
            draft.sphere_radius < r[size - 1]
                && radius > 0.0
                && radius <= draft.sphere_radius * (1.0 + RADIUS_DIGITS),
            "paw_radius, shape_function: need 0 < r_shape ≤ r_PAW < the last grid radius",
        )?;
        rule(
            n > 0 && draft.channels.iter().all(|c| c.l <= 3),
            "valence_states: give at least one channel, each with l ≤ 3",
        )?;
        rule(
            draft
                .channels
                .iter()
                .all(|c| (0.0..=(4 * c.l + 2) as f64).contains(&c.electrons()))
                && draft.channels.iter().map(Channel::electrons).sum::<f64>()
                    == draft.valence_electrons,
            "valence_states: each f must lie in [0, 2(2l + 1)], and they must sum to the valence electrons",
        )?;
        rule(
            draft.kinetic_differences.len() == n * n
                && (0..n).all(|i| {
                    (0..n).all(|j| {
                        let a = draft.kinetic_differences[i * n + j];
                        a.is_finite() && a == draft.kinetic_differences[j * n + i]
                    })
                }),
            "kinetic_energy_differences: give a symmetric n × n matrix for n channels",
        )?;
        let outside = r.partition_point(|&x| x <= draft.sphere_radius);
        rule(
            draft
                .channels
                .iter()
                .all(|c| (outside..size).all(|i| c.ae[i] == c.pseudo[i] && c.projector[i] == 0.0)),
            "partial waves: beyond r_PAW, φ must equal φ̃ and p̃ must be 0",
        )?;
        let charge = |f: &[f64]| {
            let weighted: Vec<f64> = f.iter().zip(r).map(|(f, r)| 4.0 * PI * r * r * f).collect();
            draft.grid.integrate(&weighted)
        };
        rule(
            (charge(&draft.core_density) - draft.core_electrons).abs()
                <= CHARGE_RELATIVE * draft.core_electrons.max(1.0),
            "ae_core_density: ∫ 4π r² n_c dr must equal the core electron count",
        )?;
        if draft
            .pseudo_core_density
            .iter()
            .all(|x| x.abs() < PSEUDO_CORE_DENSITY_FLOOR_PER_BOHR3)
        {
            draft.pseudo_core_density.fill(0.0);
        }
        Ok(Self(draft))
    }
}

impl Dataset {
    pub fn draft(&self) -> &DatasetDraft {
        &self.0
    }

    pub fn has_pseudo_core(&self) -> bool {
        self.0.pseudo_core_density.iter().any(|&x| x != 0.0)
    }
}
