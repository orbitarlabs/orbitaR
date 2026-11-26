use super::run::checked;
use super::{Failure, ewald_energy_ha};
use crate::model::{Cell, Probe, Spec};
use crate::paw::{Dataset, Species, total_occupancies};
use crate::pw::{Grid, Mat, hartree};
use crate::xc::{Point, energy_density};
use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub enum Probed {
    EwaldEnergy(f64),
    HartreeEnergy(f64),
    XcEnergyDensity(Vec<(f64, f64)>),
    PawXc {
        xc_ha: [f64; 2],
        d_ha: Vec<Vec<Vec<f64>>>,
    },
}

pub fn probe(spec: &Spec) -> Result<Option<Probed>, Failure> {
    let draft = spec.draft();
    let Some(probe) = draft.probe.as_ref() else {
        return Ok(None);
    };
    Ok(Some(match probe {
        Probe::Ewald(charges) => Probed::EwaldEnergy(ewald_energy_ha(&draft.cell, charges)?),
        Probe::Hartree(gaussians) => {
            let grid = Grid::coarse(&draft.cell, draft.cutoff_ha);
            let density = gaussian_density(&draft.cell, &grid, gaussians);
            Probed::HartreeEnergy(hartree(&grid, &density, draft.cutoff_ha).energy_ha)
        }
        Probe::Xc { points, tau } => {
            let functional = draft
                .functional
                .expect("Spec::try_from requires xc.functional for an xc probe");
            let at = |k: usize| {
                let [up, down, uu, ud, dd] = points[k];
                let point = Point::of(up, down, [uu, ud, dd]);
                tau.as_ref().map_or(point, |tau| point.with_tau(tau[k]))
            };
            Probed::XcEnergyDensity(
                (0..points.len())
                    .map(|k| energy_density(functional, at(k)))
                    .collect(),
            )
        }
        Probe::Paw { .. } => {
            return Err(Failure::Invalid(
                "probe: a paw probe reads its element's dataset; ks::paw_probe evaluates it".into(),
            ));
        }
    }))
}

pub fn paw_probe(
    spec: &Spec,
    mut dataset: impl FnMut(u8) -> Result<Dataset, Failure>,
) -> Result<Option<Probed>, Failure> {
    let Some(Probe::Paw { z, rho }) = spec.draft().probe.as_ref() else {
        return Ok(None);
    };
    let given = checked(spec, *z, dataset(*z)?)?;
    let species = Species::for_functional(given, spec.functional())
        .expect("checked refuses a meta-GGA dataset without core τ");
    let n = species.orbitals().len();
    if rho[0].len() != n {
        return Err(Failure::Invalid(format!(
            "probe.rho_ij: the dataset for Z = {z} has {n} orbitals; give {n} × {n} matrices"
        )));
    }
    let rho: Vec<Mat<f64>> = (rho.iter())
        .map(|r| Mat::from_fn(n, n, |i, j| r[i][j]))
        .collect();
    let one = species.one_center(&rho);
    let hartree = species.hartree(&total_occupancies(&rho));
    let d_ha = (one.d.iter())
        .map(|d| {
            let xc = d - species.frozen_d() - &hartree;
            (0..n)
                .map(|i| (0..n).map(|j| xc[(i, j)]).collect())
                .collect()
        })
        .collect();
    Ok(Some(Probed::PawXc {
        xc_ha: one.xc_ha,
        d_ha,
    }))
}

fn gaussian_density(cell: &Cell, grid: &Grid, gaussians: &[(f64, f64, [f64; 3])]) -> Vec<f64> {
    let points = grid.points();
    let mut density = vec![0.0; points.len()];
    for &(q, alpha, center) in gaussians {
        let radius = (46.0 / alpha).sqrt();
        let norm = q * (alpha / PI).powf(1.5);
        for (value, f) in density.iter_mut().zip(&points) {
            let offset = std::array::from_fn(|k| f[k] - center[k]);
            *value += cell
                .translates_within(offset, radius)
                .into_iter()
                .map(|(_, d)| norm * (-alpha * d * d).exp())
                .sum::<f64>();
        }
    }
    density
}
