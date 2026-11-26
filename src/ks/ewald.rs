use super::Failure;
use crate::model::Cell;
use crate::pw::{Complex64, per_point};
use compensated_summation::KahanBabuskaNeumaier;
use std::f64::consts::{PI, TAU};

const SPLIT: f64 = 6.5;

const MOST_VECTORS: f64 = 2e7;

pub fn ewald_energy_ha(cell: &Cell, charges: &[(f64, [f64; 3])]) -> Result<f64, Failure> {
    let Split { eta, vectors } = split(cell, charges)?;
    let terms = per_point(vectors.len(), |i| {
        let (m, g) = vectors[i];
        let structure: Complex64 = charges
            .iter()
            .map(|(q, f)| {
                Complex64::from_polar(*q, TAU * (m[0] * f[0] + m[1] * f[1] + m[2] * f[2]))
            })
            .sum();
        (-g * g / (4.0 * eta * eta)).exp() / (g * g) * structure.norm_sqr()
    });
    let reciprocal = terms.into_iter().sum::<KahanBabuskaNeumaier<f64>>().total();
    let volume = cell.volume();
    let total: f64 = charges.iter().map(|(q, _)| q).sum();
    let squares: f64 = charges.iter().map(|(q, _)| q * q).sum();
    Ok(TAU / volume * reciprocal
        - eta / PI.sqrt() * squares
        - PI * total * total / (2.0 * volume * eta * eta))
}

pub fn ewald_forces_ha_bohr(
    cell: &Cell,
    charges: &[(f64, [f64; 3])],
) -> Result<Vec<[f64; 3]>, Failure> {
    let Split { eta, vectors } = split(cell, charges)?;
    let scale = TAU / cell.volume();
    let mut forces = vec![[0.0; 3]; charges.len()];
    for (m, g) in vectors {
        let phases: Vec<Complex64> = (charges.iter())
            .map(|(q, f)| {
                Complex64::from_polar(*q, TAU * (m[0] * f[0] + m[1] * f[1] + m[2] * f[2]))
            })
            .collect();
        let structure: Complex64 = phases.iter().sum();
        let weight = scale * (-g * g / (4.0 * eta * eta)).exp() / (g * g);
        let cartesian = cell.wavevector(m);
        for (force, phase) in forces.iter_mut().zip(&phases) {
            let along = 2.0 * (structure.conj() * phase).im * weight;
            for axis in 0..3 {
                force[axis] += along * cartesian[axis];
            }
        }
    }
    Ok(forces)
}

pub fn ewald_stress_ha_bohr3(
    cell: &Cell,
    charges: &[(f64, [f64; 3])],
) -> Result<[[f64; 3]; 3], Failure> {
    let Split { eta, vectors } = split(cell, charges)?;
    let volume = cell.volume();
    let mut stress = [[0.0; 3]; 3];
    for (m, g) in vectors {
        let structure: Complex64 = (charges.iter())
            .map(|(q, f)| {
                Complex64::from_polar(*q, TAU * (m[0] * f[0] + m[1] * f[1] + m[2] * f[2]))
            })
            .sum();
        let weight = TAU / (volume * volume) * (-g * g / (4.0 * eta * eta)).exp() / (g * g)
            * structure.norm_sqr();
        let along = 2.0 * (1.0 / (4.0 * eta * eta) + 1.0 / (g * g));
        let cartesian = cell.wavevector(m);
        for (i, row) in stress.iter_mut().enumerate() {
            for (j, entry) in row.iter_mut().enumerate() {
                let diagonal = if i == j { 1.0 } else { 0.0 };
                *entry += weight * (along * (cartesian[i] * cartesian[j]) - diagonal);
            }
        }
    }
    let total: f64 = charges.iter().map(|(q, _)| q).sum();
    for (i, row) in stress.iter_mut().enumerate() {
        row[i] += PI * total * total / (2.0 * volume * volume * eta * eta);
    }
    Ok(stress)
}

struct Split {
    eta: f64,
    vectors: Vec<([f64; 3], f64)>,
}

fn split(cell: &Cell, charges: &[(f64, [f64; 3])]) -> Result<Split, Failure> {
    let r_min = closest(cell, charges);
    assert!(r_min > 0.0, "ewald: two charges share a site");
    let eta = SPLIT / r_min;
    let g_max = 2.0 * eta * 40.0_f64.sqrt();
    let count = cell.volume() * g_max.powi(3) / (6.0 * PI * PI);
    if too_many(count) {
        return Err(Failure::Unsupported(format!(
            "structure: the Ewald sum needs {count:.1e} G vectors, above {MOST_VECTORS:.0e}: the cell is too wide for \
             its closest pair of {r_min:.3} Bohr"
        )));
    }
    let vectors = (cell.wavevectors_within([0.0; 3], g_max).into_iter())
        .filter(|&(_, g)| g > 0.0)
        .collect();
    Ok(Split { eta, vectors })
}

fn too_many(count: f64) -> bool {
    count > MOST_VECTORS
}

fn closest(cell: &Cell, charges: &[(f64, [f64; 3])]) -> f64 {
    let radius = cell.lengths().into_iter().fold(f64::INFINITY, f64::min);
    let mut best = radius;
    for (i, (_, a)) in charges.iter().enumerate() {
        for (j, (_, b)) in charges.iter().enumerate().skip(i) {
            let d: [f64; 3] = std::array::from_fn(|k| b[k] - a[k]);
            for (t, r) in cell.translates_within(d, radius) {
                if i != j || t != [0.0; 3] {
                    best = best.min(r);
                }
            }
        }
    }
    best
}
