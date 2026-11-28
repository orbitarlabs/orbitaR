use super::{Dataset, Table, real_ylm, solid_harmonics};
use crate::model::Cell;
use crate::pw::{Basis, Mat, MatRef, c64};
use std::f64::consts::{PI, TAU};

const STEP: f64 = 0.01;
const PROJECTOR_ZERO: f64 = 1e-56;

pub struct Projectors {
    channels: Vec<(usize, Table)>,
}

impl Projectors {
    pub fn tabulated(dataset: &Dataset, q_max: f64) -> Self {
        let d = dataset.draft();
        Self::of(
            dataset,
            q_max,
            d.channels.iter().map(|c| (c.l, &c.projector[..])),
        )
    }

    pub fn pseudo_orbitals(dataset: &Dataset, q_max: f64) -> Self {
        let d = dataset.draft();
        let bound = (d.channels.iter()).filter(|c| c.occupation.is_some());
        Self::of(dataset, q_max, bound.map(|c| (c.l, &c.pseudo[..])))
    }

    fn of<'a>(
        dataset: &Dataset,
        q_max: f64,
        radials: impl Iterator<Item = (usize, &'a [f64])>,
    ) -> Self {
        let grid = &dataset.draft().grid;
        let channels = radials
            .map(|(l, f)| {
                let last = (f.iter().zip(grid.radii()))
                    .rposition(|(f, r)| (f * r).abs() >= PROJECTOR_ZERO)
                    .unwrap_or(0);
                let f = &f[..(last + 2).clamp(3, f.len())];
                (
                    l,
                    Table::sampled(q_max, STEP, |q| grid.bessel_transform(f, l, q)),
                )
            })
            .collect();
        Self { channels }
    }

    pub fn count(&self) -> usize {
        self.channels.iter().map(|(l, _)| 2 * l + 1).sum()
    }

    pub fn at(&self, cell: &Cell, k: [f64; 3], basis: &Basis, position: [f64; 3]) -> Mat<c64> {
        self.sampled(cell, k, basis).at(basis, position)
    }

    pub fn sampled(&self, cell: &Cell, k: [f64; 3], basis: &Basis) -> Sampled {
        let lmax = self.channels.iter().map(|&(l, _)| l).max().unwrap_or(0);
        let (mut radial, mut ylm) = (Vec::new(), Vec::new());
        for m in basis.millers() {
            let q = cell.wavevector(std::array::from_fn(|i| k[i] + f64::from(m[i])));
            let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
            radial.extend(self.channels.iter().map(|(_, table)| table.at(norm)));
            ylm.extend(real_ylm(lmax, q));
        }
        Sampled {
            k,
            prefactor: 4.0 * PI / cell.volume().sqrt(),
            ls: self.channels.iter().map(|&(l, _)| l).collect(),
            harmonics: (lmax + 1).pow(2),
            radial,
            ylm,
        }
    }

    pub fn strained(
        &self,
        cell: &Cell,
        k: [f64; 3],
        basis: &Basis,
        position: [f64; 3],
    ) -> [[Mat<c64>; 3]; 3] {
        let lmax = self.channels.iter().map(|&(l, _)| l).max().unwrap_or(0);
        let prefactor = 4.0 * PI / cell.volume().sqrt();
        let rows = basis.millers().len();
        let mut out: [[Mat<c64>; 3]; 3] =
            std::array::from_fn(|_| std::array::from_fn(|_| Mat::zeros(rows, self.count())));
        for (row, m) in basis.millers().iter().enumerate() {
            let fractional: [f64; 3] = std::array::from_fn(|i| k[i] + f64::from(m[i]));
            let q = cell.wavevector(fractional);
            let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
            let (ylm, solid) = (real_ylm(lmax, q), solid_harmonics(lmax, q));
            let phase = -TAU * (0..3).map(|i| fractional[i] * position[i]).sum::<f64>();
            let structure = c64::new(phase.cos(), phase.sin()) * prefactor;
            let mut column = 0;
            for (l, table) in &self.channels {
                let i_l = c64::new(0.0, -1.0).powi(*l as i32);
                let (f, slope) = (table.at(norm), table.slope(norm));
                for lm in l * l..(l + 1) * (l + 1) {
                    let value = structure * i_l * f * ylm[lm];
                    let gradient: [f64; 3] = if norm > 0.0 {
                        let radial = ylm[lm] * (slope - *l as f64 * f / norm) / norm;
                        let scale = f / norm.powi(*l as i32);
                        std::array::from_fn(|j| scale * solid[lm].1[j] + q[j] * radial)
                    } else {
                        [0.0; 3]
                    };
                    for i in 0..3 {
                        for j in 0..3 {
                            let mut d = structure * i_l * (-q[i] * gradient[j]);
                            if i == j {
                                d -= value * 0.5;
                            }
                            out[i][j][(row, column)] = d;
                        }
                    }
                    column += 1;
                }
            }
        }
        out
    }

    pub fn displaced(
        projectors: &Mat<c64>,
        cell: &Cell,
        k: [f64; 3],
        basis: &Basis,
        x: MatRef<'_, c64>,
        axis: usize,
    ) -> Mat<c64> {
        let q = wavevector_components(cell, k, basis, axis);
        let moved = Mat::from_fn(x.nrows(), x.ncols(), |g, band| {
            x[(g, band)] * c64::new(0.0, q[g])
        });
        projectors.adjoint() * &moved
    }

    pub fn displacement(
        projectors: &Mat<c64>,
        cell: &Cell,
        k: [f64; 3],
        basis: &Basis,
        axis: usize,
    ) -> Mat<c64> {
        let q = wavevector_components(cell, k, basis, axis);
        Mat::from_fn(projectors.nrows(), projectors.ncols(), |g, j| {
            projectors[(g, j)] * c64::new(0.0, -q[g])
        })
    }
}

fn wavevector_components(cell: &Cell, k: [f64; 3], basis: &Basis, axis: usize) -> Vec<f64> {
    (basis.millers().iter())
        .map(|m| cell.wavevector(std::array::from_fn(|i| k[i] + f64::from(m[i])))[axis])
        .collect()
}

pub struct Sampled {
    k: [f64; 3],
    prefactor: f64,
    ls: Vec<usize>,
    harmonics: usize,
    radial: Vec<f64>,
    ylm: Vec<f64>,
}

impl Sampled {
    pub fn at(&self, basis: &Basis, position: [f64; 3]) -> Mat<c64> {
        let rows = basis.millers().len();
        assert_eq!(
            rows * self.ls.len(),
            self.radial.len(),
            "the basis that `sampled` took"
        );
        let count = self.ls.iter().map(|l| 2 * l + 1).sum();
        let mut out = Mat::zeros(rows, count);
        for (row, m) in basis.millers().iter().enumerate() {
            let fractional: [f64; 3] = std::array::from_fn(|i| self.k[i] + f64::from(m[i]));
            let phase = -TAU * (0..3).map(|i| fractional[i] * position[i]).sum::<f64>();
            let structure = c64::new(phase.cos(), phase.sin()) * self.prefactor;
            let radial = &self.radial[row * self.ls.len()..][..self.ls.len()];
            let ylm = &self.ylm[row * self.harmonics..][..self.harmonics];
            let mut column = 0;
            for (&l, &f) in self.ls.iter().zip(radial) {
                let i_l = c64::new(0.0, -1.0).powi(l as i32);
                let radial = structure * i_l * f;
                for y in &ylm[l * l..(l + 1) * (l + 1)] {
                    out[(row, column)] = radial * y;
                    column += 1;
                }
            }
        }
        out
    }
}
