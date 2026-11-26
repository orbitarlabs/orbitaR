use super::{Grid, per_point};
use rustfft::num_complex::Complex64;
use std::f64::consts::PI;

#[derive(Clone, Debug, PartialEq)]
pub struct Hartree {
    pub energy_ha: f64,
    pub potential: Vec<f64>,
}

pub fn hartree(grid: &Grid, density: &[f64], cutoff_ha: f64) -> Hartree {
    let mut rho: Vec<Complex64> = per_point(density.len(), |r| density[r].into());
    grid.forward(&mut rho);
    let mut potential = vec![Complex64::default(); rho.len()];
    let mut sum = 0.0;
    for (slot, g2) in grid.density_sphere(cutoff_ha) {
        if g2 > 0.0 {
            sum += rho[slot].norm_sqr() / g2;
            potential[slot] = rho[slot] * (4.0 * PI / g2);
        }
    }
    grid.inverse(&mut potential);
    let n = rho.len() as f64;
    Hartree {
        energy_ha: 2.0 * PI * grid.cell().volume() * sum / (n * n),
        potential: per_point(potential.len(), |r| potential[r].re),
    }
}
