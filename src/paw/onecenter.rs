use super::radial::extrapolated;
use super::species::largest_moment;
use super::{Species, real_ylm, solid_harmonics};
use crate::pw::{Grid, Mat, per_point};
use crate::xc::{DENSITY_FLOOR_PER_BOHR3, Point, Rung, evaluate, unpolarized};
use std::f64::consts::PI;
use std::sync::Arc;

const STEP: f64 = 1e-2;
const STENCIL: [f64; 4] = [-2.0, -1.0, 1.0, 2.0];
const RHO_MIN: f64 = 1e-14;
pub(super) const MIN_SQUARED_DENSITY_GRADIENT_PER_BOHR8: f64 = 1e-24;
const SPHERE_RADIUS_SQUARED_MARGIN: f64 = 1.0000001;

#[derive(Clone, Debug)]
pub struct OneCenter {
    pub energy_ha: f64,
    pub d: Vec<Mat<f64>>,
    pub xc_ha: [f64; 2],
}

struct Expansion {
    energy: f64,
    potential: Vec<Vec<Vec<f64>>>,
}

pub fn total_occupancies(rho: &[Mat<f64>]) -> Mat<f64> {
    rho.iter().skip(1).fold(rho[0].clone(), |sum, r| sum + r)
}

impl Species {
    pub fn one_center(&self, rho: &[Mat<f64>]) -> OneCenter {
        if self.functional.rung() == Rung::MetaGga {
            return self.one_center_on_mesh(rho);
        }
        assert!(matches!(rho.len(), 1 | 2), "one ρ_ij, or one per spin");
        let (ae, ps): (Vec<_>, Vec<_>) = rho.iter().map(|r| self.moment_densities(r)).unzip();
        let d = self.dataset.draft();
        let (core, pseudo_core) = (
            &d.core_density[..self.size],
            &d.pseudo_core_density[..self.size],
        );
        let (all, pseudo) = if rho.len() == 1 {
            (
                self.expansion(&ae[0], core),
                self.expansion(&ps[0], pseudo_core),
            )
        } else {
            (
                self.expansion_polarized(&ae, core),
                self.expansion_polarized(&ps, pseudo_core),
            )
        };
        let n = self.orbitals.len();
        let grid = &d.grid;
        let total = total_occupancies(rho);
        let dh = self.hartree(&total);
        let dot = |m: &Mat<f64>| {
            (0..n)
                .flat_map(|i| (0..n).map(move |j| (i, j)))
                .map(|(i, j)| total[(i, j)] * m[(i, j)])
                .sum::<f64>()
        };
        let (hartree, frozen) = (dot(&dh), dot(self.frozen_d()));
        let d = (all.potential.iter().zip(&pseudo.potential))
            .map(|(v, tv)| {
                Mat::from_fn(n, n, |i, j| {
                    let (uu, tt) = self.products(i, j);
                    let xc: f64 = (0..ae[0].len())
                        .filter(|&lm| self.gaunt(lm, i, j) != 0.0)
                        .map(|lm| {
                            let f: Vec<f64> = (0..self.end)
                                .map(|k| v[lm][k] * uu[k] - tv[lm][k] * tt[k])
                                .collect();
                            grid.integrate(&f) * self.gaunt(lm, i, j)
                        })
                        .sum();
                    self.frozen_d()[(i, j)] + dh[(i, j)] + xc
                })
            })
            .collect();
        OneCenter {
            energy_ha: all.energy + 0.5 * hartree + frozen - self.core_xc() - pseudo.energy,
            d,
            xc_ha: [all.energy, pseudo.energy],
        }
    }

    pub(super) fn moment_densities(&self, rho: &Mat<f64>) -> (Vec<Vec<f64>>, Vec<Vec<f64>>) {
        let r = self.dataset.draft().grid.radii();
        let count = self.moments_count();
        let (mut ae, mut ps) = (
            vec![vec![0.0; self.size]; count],
            vec![vec![0.0; self.size]; count],
        );
        let n = self.orbitals.len();
        for i in 0..n {
            for j in 0..n {
                if rho[(i, j)] == 0.0 {
                    continue;
                }
                let (uu, tt) = self.products(i, j);
                for lm in 0..count {
                    let g = self.gaunt(lm, i, j);
                    if g == 0.0 {
                        continue;
                    }
                    for k in 1..self.size {
                        ae[lm][k] += rho[(i, j)] * g * uu[k] / (r[k] * r[k]);
                        ps[lm][k] += rho[(i, j)] * g * tt[k] / (r[k] * r[k]);
                    }
                }
            }
        }
        for f in ae.iter_mut().chain(&mut ps) {
            f[0] = extrapolated([f[1], f[2], f[3]]);
        }
        (ae, ps)
    }

    pub(super) fn moments_count(&self) -> usize {
        (self.moments_lmax() + 1).pow(2)
    }

    pub(super) fn moments_lmax(&self) -> usize {
        largest_moment(self.orbitals.iter().map(|o| o.l).max().unwrap_or(0))
    }

    fn expansion(&self, moments: &[Vec<f64>], core: &[f64]) -> Expansion {
        let grid = &self.dataset.draft().grid;
        let r = grid.radii();
        let y = (4.0 * PI).sqrt();
        let total: Vec<f64> = moments[0]
            .iter()
            .zip(core)
            .map(|(n, c)| n + y * c)
            .collect();
        let up: Vec<f64> = total
            .iter()
            .map(|t| (t / (2.0 * y)).max(DENSITY_FLOOR_PER_BOHR3))
            .collect();
        let scaled = |s: f64| up.iter().map(|x| x * s).collect::<Vec<f64>>();
        let runs: Vec<Option<f64>> = std::iter::once(None).chain(STENCIL.map(Some)).collect();
        let mut done = per_point(runs.len(), |j| match runs[j] {
            None => self.spherical_xc(&up),
            Some(k) => self.spherical_xc(&scaled(1.0 + k * STEP)),
        });
        let (eps, v0) = std::mem::take(&mut done[0]);
        let [far_below, below, above, far_above] =
            std::array::from_fn(|j| std::mem::take(&mut done[1 + j].1));
        let squares: Vec<f64> = (0..self.size)
            .map(|k| moments[1..].iter().map(|m| m[k].powi(2)).sum())
            .collect();
        let mut potential = vec![vec![0.0; self.size]; moments.len()];
        let mut first = vec![0.0; self.size];
        for k in 0..self.size {
            let inverse = if up[k] > RHO_MIN {
                0.5 / STEP / up[k]
            } else {
                0.0
            };
            let samples = [far_below[k], below[k], v0[k], above[k], far_above[k]];
            let (slope, curvature) = differences(samples, inverse);
            first[k] = slope;
            potential[0][k] = v0[k] * y + squares[k] * curvature / (2.0 * y);
            for lm in 1..moments.len() {
                potential[lm][k] = first[k] * moments[lm][k];
            }
        }
        let energy: Vec<f64> = (0..self.end)
            .map(|k| (total[k] * eps[k] * y + 0.5 * squares[k] * first[k]) * r[k] * r[k])
            .collect();
        Expansion {
            energy: grid.integrate(&energy),
            potential: vec![potential],
        }
    }

    fn expansion_polarized(&self, moments: &[Vec<Vec<f64>>], core: &[f64]) -> Expansion {
        let grid = &self.dataset.draft().grid;
        let r = grid.radii();
        let y = (4.0 * PI).sqrt();
        let spherical: [Vec<f64>; 2] = std::array::from_fn(|s| {
            (moments[s][0].iter().zip(core))
                .map(|(n, c)| ((n + 0.5 * y * c) / y).max(DENSITY_FLOOR_PER_BOHR3))
                .collect()
        });
        let stencil = (0..2).flat_map(|s| STENCIL.map(move |k| Some((s, k))));
        let runs: Vec<Option<(usize, f64)>> = std::iter::once(None).chain(stencil).collect();
        let mut done = per_point(runs.len(), |j| {
            let mut density = spherical.clone();
            if let Some((s, k)) = runs[j] {
                density[s].iter_mut().for_each(|x| *x *= 1.0 + k * STEP);
            }
            self.spherical_xc_polarized(&density)
        });
        let (eps, v0) = std::mem::take(&mut done[0]);
        let mut shifts = done[1..].chunks_mut(STENCIL.len());
        let shifted: [Vec<[Vec<f64>; 2]>; 2] = std::array::from_fn(|_| {
            let spin = shifts.next().expect("one stencil per spin");
            spin.iter_mut()
                .map(|run| std::mem::take(&mut run.1))
                .collect()
        });
        let product = |a: usize, b: usize, k: usize| -> f64 {
            (moments[a][1..].iter().zip(&moments[b][1..]))
                .map(|(x, y)| x[k] * y[k])
                .sum()
        };
        let mut potential = vec![vec![vec![0.0; self.size]; moments[0].len()]; 2];
        let mut energy = vec![0.0; self.end];
        for k in 0..self.size {
            let (mut first, mut second) = ([[0.0; 2]; 2], [[0.0; 2]; 2]);
            for s in 0..2 {
                let inverse = if spherical[s][k] > RHO_MIN {
                    1.0 / (STEP * spherical[s][k])
                } else {
                    0.0
                };
                for sigma in 0..2 {
                    let at = |j: usize| shifted[s][j][sigma][k];
                    let samples = [at(0), at(1), v0[sigma][k], at(2), at(3)];
                    (first[s][sigma], second[s][sigma]) = differences(samples, inverse);
                }
            }
            let (uu, ud, dd) = (product(0, 0, k), product(0, 1, k), product(1, 1, k));
            let (up_up, down_up, down_down) = (first[0][0], first[0][1], first[1][1]);
            potential[0][0][k] = v0[0][k] * y
                + (uu * second[0][0] + 2.0 * ud * second[0][1] + dd * second[1][0]) / (2.0 * y);
            potential[1][0][k] = v0[1][k] * y
                + (uu * second[0][1] + 2.0 * ud * second[1][0] + dd * second[1][1]) / (2.0 * y);
            for lm in 1..moments[0].len() {
                let (m_up, m_down) = (moments[0][lm][k], moments[1][lm][k]);
                potential[0][lm][k] = up_up * m_up + down_up * m_down;
                potential[1][lm][k] = down_up * m_up + down_down * m_down;
            }
            if k < self.end {
                let n0 = moments[0][0][k] + moments[1][0][k] + y * core[k];
                energy[k] = (n0 * eps[k] * y + ud * down_up + 0.5 * (uu * up_up + dd * down_down))
                    * r[k]
                    * r[k];
            }
        }
        Expansion {
            energy: grid.integrate(&energy),
            potential,
        }
    }

    fn spherical_xc(&self, up: &[f64]) -> (Vec<f64>, Vec<f64>) {
        let d = self.dataset.draft();
        let r = d.grid.radii();
        let slope = d.grid.derivative(up);
        let points = per_point(up.len(), |k| {
            let [x, c] = unpolarized(
                self.functional,
                2.0 * up[k],
                4.0 * slope[k] * slope[k],
                None,
            );
            let vsigma = x.vsigma + c.vsigma;
            let h = if flat(slope[k]) {
                0.0
            } else {
                2.0 * vsigma * slope[k]
            };
            ((x.energy + c.energy) / (2.0 * up[k]), x.vrho + c.vrho, h)
        });
        let eps = points.iter().map(|p| p.0).collect();
        let vrho: Vec<f64> = points.iter().map(|p| p.1).collect();
        let h: Vec<f64> = points.iter().map(|p| p.2).collect();
        let dh = d.grid.derivative(&h);
        let mut v: Vec<f64> = (0..up.len())
            .map(|k| vrho[k] - 2.0 * (dh[k] + 2.0 * h[k] / r[k]))
            .collect();
        v[0] = extrapolated([v[1], v[2], v[3]]);
        (eps, v)
    }

    fn spherical_xc_polarized(&self, density: &[Vec<f64>; 2]) -> (Vec<f64>, [Vec<f64>; 2]) {
        let d = self.dataset.draft();
        let r = d.grid.radii();
        let slope = density.each_ref().map(|n| d.grid.derivative(n));
        let count = density[0].len();
        let points = per_point(count, |k| {
            let (up, down) = (slope[0][k], slope[1][k]);
            let sigma = [up * up, up * down, down * down];
            let [x, c] = evaluate(
                self.functional,
                Point::of(density[0][k], density[1][k], sigma),
            );
            let vsigma: [f64; 3] = std::array::from_fn(|i| x.vsigma[i] + c.vsigma[i]);
            let both = up + down;
            let mixed = if flat(both) { 0.0 } else { vsigma[1] * both };
            let pairs = [(up, vsigma[0]), (down, vsigma[2])];
            let vrho: [f64; 2] = std::array::from_fn(|s| x.vrho[s] + c.vrho[s]);
            let h = pairs.map(|(own, pure)| {
                let own_part = if flat(own) {
                    0.0
                } else {
                    (2.0 * pure - vsigma[1]) * own
                };
                own_part + mixed
            });
            (
                (x.energy + c.energy) / (density[0][k] + density[1][k]),
                vrho,
                h,
            )
        });
        let eps = points.iter().map(|p| p.0).collect();
        let vrho: [Vec<f64>; 2] = std::array::from_fn(|s| points.iter().map(|p| p.1[s]).collect());
        let h: [Vec<f64>; 2] = std::array::from_fn(|s| points.iter().map(|p| p.2[s]).collect());
        let v = std::array::from_fn(|s| {
            let dh = d.grid.derivative(&h[s]);
            let mut v: Vec<f64> = (0..count)
                .map(|k| vrho[s][k] - (dh[k] + 2.0 * h[s][k] / r[k]))
                .collect();
            v[0] = extrapolated([v[1], v[2], v[3]]);
            v
        });
        (eps, v)
    }

    pub fn sphere(self: &Arc<Self>, grid: &Grid, position: [f64; 3]) -> Sphere {
        let cell = grid.cell();
        let radius = self.dataset.draft().shape.radius() * SPHERE_RADIUS_SQUARED_MARGIN.sqrt();
        let (count, lmax) = (self.moments_count(), self.moments_lmax());
        let mut sphere = Sphere {
            species: Arc::clone(self),
            points: Vec::new(),
            offsets: Vec::new(),
            values: Vec::new(),
            volume: cell.volume() / grid.dims().iter().product::<usize>() as f64,
        };
        for (index, shift, distance) in grid.within(position, radius) {
            let d = cell.cartesian(shift);
            let ylm = real_ylm(lmax, d);
            let values = (0..count)
                .map(|lm| self.compensation.value(lm.isqrt(), distance) * ylm[lm])
                .collect();
            sphere.points.push(index);
            sphere.offsets.push(d);
            sphere.values.push(values);
        }
        sphere
    }
}

pub struct Sphere {
    species: Arc<Species>,
    points: Vec<usize>,
    offsets: Vec<[f64; 3]>,
    values: Vec<Vec<f64>>,
    volume: f64,
}

impl Sphere {
    pub fn add_compensation(&self, rho: &Mat<f64>, density: &mut [f64]) {
        let weights = moments_of(&self.species, rho);
        for (&index, values) in self.points.iter().zip(&self.values) {
            density[index] += values.iter().zip(&weights).map(|(v, w)| v * w).sum::<f64>();
        }
    }

    pub fn potential_d(&self, v: &[f64]) -> Mat<f64> {
        let species = &self.species;
        let count = self.values.first().map_or(0, Vec::len);
        let mut projected = vec![0.0; count];
        for (&index, values) in self.points.iter().zip(&self.values) {
            for (p, g) in projected.iter_mut().zip(values) {
                *p += v[index] * g * self.volume;
            }
        }
        let n = species.orbitals().len();
        Mat::from_fn(n, n, |i, j| {
            (0..count)
                .map(|lm| species.moment(lm)[(i, j)] * projected[lm])
                .sum()
        })
    }

    pub fn compensation_gradient(&self, rho: &Mat<f64>, v: &[f64]) -> [f64; 3] {
        let mut sum = [0.0; 3];
        for (index, _, gradient) in self.gradients(rho) {
            for axis in 0..3 {
                sum[axis] -= v[index] * gradient[axis] * self.volume;
            }
        }
        sum
    }

    pub fn compensation_strain(&self, rho: &Mat<f64>, v: &[f64]) -> [[f64; 3]; 3] {
        let mut sum = [[0.0; 3]; 3];
        for (index, d, gradient) in self.gradients(rho) {
            for (i, row) in sum.iter_mut().enumerate() {
                for (j, entry) in row.iter_mut().enumerate() {
                    *entry += v[index] * gradient[i] * d[j] * self.volume;
                }
            }
        }
        sum
    }

    fn gradients(&self, rho: &Mat<f64>) -> impl Iterator<Item = (usize, [f64; 3], [f64; 3])> {
        let species = &self.species;
        let weights = moments_of(species, rho);
        let lmax = species.moments_lmax();
        (self.points.iter().zip(&self.offsets)).map(move |(&index, d)| {
            let r = d.iter().map(|x| x * x).sum::<f64>().sqrt();
            let solid = solid_harmonics(lmax, *d);
            let mut gradient = [0.0; 3];
            for (lm, (w, (s, ds))) in weights.iter().zip(&solid).enumerate() {
                let (h, slope) = species.compensation.reduced(lm.isqrt(), r);
                let radial = if r > 0.0 { slope * s / r } else { 0.0 };
                for axis in 0..3 {
                    gradient[axis] += w * (radial * d[axis] + h * ds[axis]);
                }
            }
            (index, *d, gradient)
        })
    }
}

fn moments_of(species: &Species, rho: &Mat<f64>) -> Vec<f64> {
    let n = species.orbitals().len();
    (0..species.moments_count())
        .map(|lm| {
            (0..n)
                .flat_map(|i| (0..n).map(move |j| (i, j)))
                .map(|(i, j)| rho[(i, j)] * species.moment(lm)[(i, j)])
                .sum()
        })
        .collect()
}

fn flat(slope: f64) -> bool {
    slope * slope < MIN_SQUARED_DENSITY_GRADIENT_PER_BOHR8
}

fn differences([far_below, below, at, above, far_above]: [f64; 5], inverse: f64) -> (f64, f64) {
    (
        (8.0 * (above - below) - (far_above - far_below)) / 12.0 * inverse,
        (16.0 * (above + below) - (far_above + far_below) - 30.0 * at) / 12.0 * inverse * inverse,
    )
}
