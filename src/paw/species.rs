use super::mesh::SPHERE_DENSITY_AND_TAU_FLOOR;
use super::radial::extrapolated;
use super::{
    CoreKinetic, Dataset, Gaunt, RadialGrid, Shape, reduced_jn, spherical_jn, ylm_rotation,
};
use crate::model::Functional;
use crate::pw::Mat;
use crate::xc::{DENSITY_FLOOR_PER_BOHR3, Point, Rung, evaluate, unpolarized};
use std::f64::consts::PI;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Orbital {
    pub channel: usize,
    pub l: usize,
    pub m: i32,
}

impl Orbital {
    pub fn lm(&self) -> usize {
        ((self.l * self.l + self.l) as i32 + self.m) as usize
    }
}

pub struct Species {
    pub(super) dataset: Dataset,
    pub(super) orbitals: Vec<Orbital>,
    pub(super) end: usize,
    pub(super) size: usize,
    gaunt: Gaunt,
    pub(super) compensation: Compensation,
    shapes: Vec<Vec<f64>>,
    moments: Vec<Mat<f64>>,
    d0: Mat<f64>,
    hartree: Mat<f64>,
    core_xc: f64,
    pub(super) functional: Functional,
    pub(super) largest_l: usize,
}

impl From<Dataset> for Species {
    fn from(dataset: Dataset) -> Self {
        let functional = dataset.draft().functional;
        Self::for_functional(dataset, functional).expect(
            "Dataset::try_from holds the core τ of a dataset whose own functional is a meta-GGA",
        )
    }
}

pub(crate) fn core_kinetic(
    dataset: &Dataset,
    functional: Functional,
) -> Result<Option<&CoreKinetic>, String> {
    let d = dataset.draft();
    match (functional.rung(), d.core_kinetic.as_ref()) {
        (Rung::MetaGga, None) => Err(format!(
            "datasets: the dataset for Z = {} has no core kinetic energy densities, which {} reads",
            d.z,
            functional.row().word
        )),
        (Rung::MetaGga, kinetic) => Ok(kinetic),
        _ => Ok(None),
    }
}

impl Species {
    pub fn for_functional(dataset: Dataset, functional: Functional) -> Result<Self, String> {
        let kinetic = core_kinetic(&dataset, functional)?;
        let d = dataset.draft();
        let grid = &d.grid;
        let orbitals: Vec<Orbital> = (d.channels.iter().enumerate())
            .flat_map(|(channel, c)| {
                (-(c.l as i32)..=c.l as i32).map(move |m| Orbital { channel, l: c.l, m })
            })
            .collect();
        let lmax = d.channels.iter().map(|c| c.l).max().unwrap_or(0);
        let end = grid.nearest(d.sphere_radius) + 1;
        let size = mesh_size(grid, d.sphere_radius);
        let compensation = Compensation::of(grid, d.shape, largest_moment(lmax));
        let inside = grid.index_below(d.shape.radius());
        let r = grid.radii();
        let shapes = (0..=largest_moment(lmax))
            .map(|l| {
                (0..end)
                    .map(|k| {
                        if k < inside {
                            compensation.value(l, r[k])
                        } else {
                            0.0
                        }
                    })
                    .collect()
            })
            .collect();
        let mut species = Self {
            compensation,
            gaunt: Gaunt::up_to(lmax, largest_moment(lmax)),
            moments: Vec::new(),
            d0: Mat::zeros(orbitals.len(), orbitals.len()),
            hartree: Mat::zeros(orbitals.len().pow(2), orbitals.len().pow(2)),
            core_xc: match kinetic {
                Some(kinetic) => core_xc_kinetic(grid, functional, &d.core_density, kinetic.ae()),
                None => core_xc(grid, functional, &d.core_density),
            },
            functional,
            largest_l: lmax,
            orbitals,
            end,
            size,
            shapes,
            dataset,
        };
        species.moments = species.compensation_moments();
        species.d0 = species.compute_frozen_d();
        species.hartree = species.hartree_kernel();
        Ok(species)
    }

    pub fn with_largest_l(self, largest_l: usize) -> Self {
        Self { largest_l, ..self }
    }

    pub fn dataset(&self) -> &Dataset {
        &self.dataset
    }

    pub fn orbitals(&self) -> &[Orbital] {
        &self.orbitals
    }

    pub fn overlap(&self) -> Mat<f64> {
        let q = &self.moments[0];
        Mat::from_fn(q.nrows(), q.ncols(), |i, j| q[(i, j)] * (4.0 * PI).sqrt())
    }

    pub fn moment(&self, lm: usize) -> &Mat<f64> {
        &self.moments[lm]
    }

    pub fn frozen_d(&self) -> &Mat<f64> {
        &self.d0
    }

    fn compute_frozen_d(&self) -> Mat<f64> {
        let d = self.dataset.draft();
        let (r, n) = (d.grid.radii(), d.channels.len());
        let rho: Vec<f64> = (d.core_density.iter().zip(r))
            .map(|(n, r)| 4.0 * PI * r * r * n)
            .collect();
        let rv = d.grid.poisson(&rho, 0);
        let mut nuclear: Vec<f64> = (rv.iter().zip(r)).map(|(rv, r)| (rv - d.z) / r).collect();
        nuclear[0] = extrapolated([nuclear[1], nuclear[2], nuclear[3]]);
        let below = d.grid.nearest(d.shape.radius()) + 1;
        let mut shape = self.shapes[0].clone();
        let r2: Vec<f64> = (shape.iter().zip(r))
            .map(|(g, r)| g * r * r)
            .take(below)
            .collect();
        let norm = d.grid.integrate(&r2);
        shape.iter_mut().for_each(|g| *g /= norm);
        let local =
            |f: &dyn Fn(usize) -> f64| d.grid.integrate(&(0..self.end).map(f).collect::<Vec<_>>());
        let shaped_local = local(&|k| d.local_potential[k] * shape[k] * r[k] * r[k]);
        Mat::from_fn(self.orbitals.len(), self.orbitals.len(), |i, j| {
            let (a, b) = (self.orbitals[i], self.orbitals[j]);
            if (a.l, a.m) != (b.l, b.m) {
                return 0.0;
            }
            let (ci, cj) = (&d.channels[a.channel], &d.channels[b.channel]);
            let ae = |k: usize| ci.ae[k] * cj.ae[k] * r[k] * r[k];
            let ps = |k: usize| ci.pseudo[k] * cj.pseudo[k] * r[k] * r[k];
            d.kinetic_differences[a.channel * n + b.channel] + local(&|k| ae(k) * nuclear[k])
                - local(&|k| ps(k) * d.local_potential[k])
                - shaped_local * local(&|k| ae(k) - ps(k))
        })
    }

    pub fn hartree(&self, rho: &Mat<f64>) -> Mat<f64> {
        let n = self.orbitals.len();
        Mat::from_fn(n, n, |i, j| {
            (0..n * n)
                .map(|kl| self.hartree[(i * n + j, kl)] * rho[(kl / n, kl % n)])
                .sum()
        })
    }

    pub fn core_xc(&self) -> f64 {
        self.core_xc
    }

    pub fn rotation(&self, w: [[f64; 3]; 3]) -> Mat<f64> {
        let lmax = self.orbitals.iter().map(|o| o.l).max().unwrap_or(0);
        let d = ylm_rotation(lmax, w);
        let n = self.orbitals.len();
        Mat::from_fn(n, n, |i, j| {
            let (a, b) = (&self.orbitals[i], &self.orbitals[j]);
            if a.channel == b.channel {
                d[(a.lm(), b.lm())]
            } else {
                0.0
            }
        })
    }

    pub fn initial_occupancies(&self) -> Mat<f64> {
        let channels = &self.dataset.draft().channels;
        Mat::from_fn(self.orbitals.len(), self.orbitals.len(), |i, j| {
            let o = self.orbitals[i];
            if i == j {
                channels[o.channel].electrons() / (2 * o.l + 1) as f64
            } else {
                0.0
            }
        })
    }

    pub fn shape(&self, l: usize, r: f64) -> f64 {
        self.compensation.value(l, r)
    }

    pub(super) fn gaunt(&self, lm: usize, i: usize, j: usize) -> f64 {
        self.gaunt
            .get(lm, self.orbitals[i].lm(), self.orbitals[j].lm())
    }

    pub(super) fn products(&self, i: usize, j: usize) -> (Vec<f64>, Vec<f64>) {
        let d = self.dataset.draft();
        let r = d.grid.radii();
        let (a, b) = (
            &d.channels[self.orbitals[i].channel],
            &d.channels[self.orbitals[j].channel],
        );
        (0..self.size)
            .map(|k| {
                (
                    a.ae[k] * b.ae[k] * r[k] * r[k],
                    a.pseudo[k] * b.pseudo[k] * r[k] * r[k],
                )
            })
            .unzip()
    }

    fn compensation_moments(&self) -> Vec<Mat<f64>> {
        let grid = &self.dataset.draft().grid;
        let r = grid.radii();
        let n = self.orbitals.len();
        let mut moments = vec![Mat::zeros(n, n); self.shapes.len().pow(2)];
        for i in 0..n {
            for j in 0..n {
                let (ae, ps) = self.products(i, j);
                let (li, lj) = (self.orbitals[i].l, self.orbitals[j].l);
                for l in (li.abs_diff(lj)..=li + lj).step_by(2) {
                    let f: Vec<f64> = (0..self.end)
                        .map(|k| (ae[k] - ps[k]) * r[k].powi(l as i32))
                        .collect();
                    let radial = grid.integrate(&f);
                    for (lm, q) in moments
                        .iter_mut()
                        .enumerate()
                        .take((l + 1) * (l + 1))
                        .skip(l * l)
                    {
                        q[(i, j)] = radial * self.gaunt(lm, i, j);
                    }
                }
            }
        }
        moments
    }

    fn hartree_kernel(&self) -> Mat<f64> {
        let grid = &self.dataset.draft().grid;
        let r = &grid.radii()[..self.end];
        let n = self.orbitals.len();
        let integrate =
            |f: &dyn Fn(usize) -> f64| grid.integrate(&(0..self.end).map(f).collect::<Vec<_>>());
        let shape_potential: Vec<Vec<f64>> = (self.shapes.iter().enumerate())
            .map(|(l, g)| {
                let rho: Vec<f64> = (g.iter().zip(r)).map(|(g, r)| g * r * r).collect();
                let rv = grid.poisson(&rho, l);
                (rv.iter().zip(r))
                    .map(|(v, r)| if *r > 0.0 { 2.0 * v / r } else { 0.0 })
                    .collect()
            })
            .collect();
        let shape_self: Vec<f64> = (shape_potential.iter().zip(&self.shapes))
            .map(|(v, g)| integrate(&|k| v[k] * g[k] * r[k] * r[k]))
            .collect();
        let pairs: Vec<(usize, usize)> =
            (0..n).flat_map(|j| (0..=j).map(move |i| (i, j))).collect();
        let products: Vec<(Vec<f64>, Vec<f64>)> =
            pairs.iter().map(|&(i, j)| self.products(i, j)).collect();
        let shape_pseudo: Vec<Vec<f64>> = (products.iter())
            .map(|(_, ps)| {
                shape_potential
                    .iter()
                    .map(|v| integrate(&|k| v[k] * ps[k]))
                    .collect()
            })
            .collect();
        let mut packed = Mat::<f64>::zeros(pairs.len(), pairs.len());
        for (p, &(i, j)) in pairs.iter().enumerate() {
            let (li, lj) = (self.orbitals[i].l, self.orbitals[j].l);
            for l in (li.abs_diff(lj)..=li + lj).step_by(2) {
                let ae_rv = grid.poisson(&products[p].0[..self.end], l);
                let ps_rv = grid.poisson(&products[p].1[..self.end], l);
                for (p1, &(i1, j1)) in pairs.iter().enumerate().skip(p) {
                    let (l1, l2) = (self.orbitals[i1].l, self.orbitals[j1].l);
                    let radial = if (l1.abs_diff(l2)..=l1 + l2).contains(&l) {
                        let (ae1, ps1) = &products[p1];
                        integrate(&|k| {
                            if k > 0 {
                                (ae1[k] * ae_rv[k] - ps1[k] * ps_rv[k]) * 2.0 / r[k]
                            } else {
                                0.0
                            }
                        })
                    } else {
                        0.0
                    };
                    let mut sum = 0.0;
                    for lm in l * l..(l + 1) * (l + 1) {
                        let (g, g1) = (self.gaunt(lm, i, j), self.gaunt(lm, i1, j1));
                        let (q, q1) = (self.moments[lm][(i, j)], self.moments[lm][(i1, j1)]);
                        sum += (radial * g * g1
                            - shape_pseudo[p][l] * g * q1
                            - shape_pseudo[p1][l] * g1 * q
                            - shape_self[l] * q * q1)
                            * 2.0
                            * PI;
                    }
                    packed[(p, p1)] += sum;
                }
            }
        }
        let mut e = Mat::zeros(n * n, n * n);
        for (p, &(i, j)) in pairs.iter().enumerate() {
            for (p1, &(i1, j1)) in pairs.iter().enumerate().skip(p) {
                for (a, b) in [(i, j), (j, i)] {
                    for (c, d) in [(i1, j1), (j1, i1)] {
                        e[(a * n + b, c * n + d)] = packed[(p, p1)];
                        e[(c * n + d, a * n + b)] = packed[(p, p1)];
                    }
                }
            }
        }
        e
    }
}

pub(super) struct Compensation {
    shape: Shape,
    bessel: Vec<([f64; 2], [f64; 2])>,
    norms: Vec<f64>,
}

impl Compensation {
    fn of(grid: &RadialGrid, shape: Shape, lmax: usize) -> Self {
        let mut c = Self {
            shape,
            bessel: Vec::new(),
            norms: vec![1.0; lmax + 1],
        };
        match shape {
            Shape::Bessel { radius } => {
                c.bessel = (0..=lmax).map(|l| bessel_shape(l, radius)).collect()
            }
            Shape::Sinc { radius } => {
                let r = grid.radii();
                let inside = grid.index_below(radius);
                for l in 0..=lmax {
                    let f: Vec<f64> = (0..=grid.nearest(radius))
                        .map(|k| {
                            if (1..inside).contains(&k) {
                                c.value(l, r[k]) * r[k].powi(l as i32 + 2)
                            } else {
                                0.0
                            }
                        })
                        .collect();
                    c.norms[l] = grid.integrate(&f);
                }
            }
        }
        c
    }

    pub(super) fn value(&self, l: usize, r: f64) -> f64 {
        match self.shape {
            Shape::Bessel { .. } => {
                let ([a1, a2], [q1, q2]) = self.bessel[l];
                a1 * spherical_jn(l, q1 * r) + a2 * spherical_jn(l, q2 * r)
            }
            Shape::Sinc { radius } => {
                let x = PI * r / radius;
                let s = if x > 0.0 { x.sin() / x } else { 1.0 };
                s * s * r.powi(l as i32) / self.norms[l]
            }
        }
    }

    pub(super) fn reduced(&self, l: usize, r: f64) -> (f64, f64) {
        match self.shape {
            Shape::Bessel { .. } => {
                let (a, q) = self.bessel[l];
                (0..2).fold((0.0, 0.0), |(h, slope), i| {
                    let x = q[i] * r;
                    (
                        h + a[i] * q[i].powi(l as i32) * reduced_jn(l, x),
                        slope - a[i] * q[i].powi(l as i32 + 2) * r * reduced_jn(l + 1, x),
                    )
                })
            }
            Shape::Sinc { radius } => {
                let x = PI * r / radius;
                let s = reduced_jn(0, x);
                (
                    s * s / self.norms[l],
                    -2.0 * s * x * reduced_jn(1, x) * PI / radius / self.norms[l],
                )
            }
        }
    }
}

pub(super) fn largest_moment(lmax: usize) -> usize {
    2 * lmax
}

fn mesh_size(grid: &RadialGrid, radius: f64) -> usize {
    (grid.index_below(radius) + 6).min(grid.radii().len())
}

fn bessel_shape(l: usize, radius: f64) -> ([f64; 2], [f64; 2]) {
    let j = |x: f64| spherical_jn(l, x);
    let opposite = |a: f64, b: f64| j(a).is_sign_negative() != j(b).is_sign_negative();
    let mut roots = Vec::with_capacity(2);
    let mut x = 0.1;
    while roots.len() != 2 {
        if opposite(x, x + 0.1) {
            let (mut lo, mut hi) = (x, x + 0.1);
            while hi - lo > 1e-15 * hi {
                let mid = 0.5 * (lo + hi);
                if opposite(lo, mid) {
                    hi = mid
                } else {
                    lo = mid
                }
            }
            roots.push(0.5 * (lo + hi));
        }
        x += 0.1;
    }
    let q = [roots[0] / radius, roots[1] / radius];
    let minus_slope = q.map(|q| spherical_jn(l + 1, q * radius) * q);
    let integral = q.map(|q| spherical_jn(l + 1, q * radius) * radius.powi(l as i32 + 2) / q);
    let det = minus_slope[0] * integral[1] - minus_slope[1] * integral[0];
    ([-minus_slope[1] / det, minus_slope[0] / det], q)
}

fn core_xc_kinetic(grid: &RadialGrid, functional: Functional, density: &[f64], tau: &[f64]) -> f64 {
    let r = grid.radii();
    let slope = grid.derivative(density);
    let f: Vec<f64> = (0..r.len())
        .map(|k| {
            let up = (density[k] / 2.0).max(SPHERE_DENSITY_AND_TAU_FLOOR);
            let tau_up = (tau[k] / 2.0).max(SPHERE_DENSITY_AND_TAU_FLOOR);
            let [x, c] = unpolarized(
                functional,
                2.0 * up,
                slope[k] * slope[k],
                Some(2.0 * tau_up),
            );
            4.0 * PI * r[k] * r[k] * density[k] * (x.epsilon + c.epsilon)
        })
        .collect();
    grid.integrate(&f)
}

fn core_xc(grid: &RadialGrid, functional: Functional, density: &[f64]) -> f64 {
    let r = grid.radii();
    let floored: Vec<f64> = density
        .iter()
        .map(|n| n.max(2.0 * DENSITY_FLOOR_PER_BOHR3))
        .collect();
    let slope = grid.derivative(&floored);
    let f: Vec<f64> = (0..r.len())
        .map(|k| {
            let half = floored[k] / 2.0;
            let sigma = slope[k] * slope[k] / 4.0;
            let [x, c] = evaluate(functional, Point::of(half, half, [sigma; 3]));
            4.0 * PI * r[k] * r[k] * density[k] * (x.energy + c.energy) / floored[k]
        })
        .collect();
    grid.integrate(&f)
}
