use super::onecenter::{MIN_SQUARED_DENSITY_GRADIENT_PER_BOHR8, OneCenter, total_occupancies};
use super::radial::extrapolated;
use super::{Species, gauss_legendre, real_ylm, solid_harmonics};
use crate::pw::{Mat, per_point};
use crate::xc::{Point, Rung, evaluate, unpolarized};
use std::f64::consts::{PI, TAU};
use std::num::NonZeroUsize;

#[derive(Clone, Debug, PartialEq)]
pub struct AngularMesh {
    points: Vec<[f64; 3]>,
    weights: Vec<f64>,
}

impl AngularMesh {
    pub fn product(ntheta: NonZeroUsize, nphi: NonZeroUsize) -> Self {
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        let nphi = nphi.get();
        for (cos_theta, w) in gauss_legendre(ntheta.get()) {
            let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
            for k in 0..nphi {
                let phi = TAU * k as f64 / nphi as f64;
                points.push([sin_theta * phi.cos(), sin_theta * phi.sin(), cos_theta]);
                weights.push(w / (2 * nphi) as f64);
            }
        }
        Self { points, weights }
    }

    pub fn abinit() -> Self {
        let count = |n| NonZeroUsize::new(n).expect("ABINIT's counts are nonzero");
        Self::product(count(12), count(13))
    }

    pub fn points(&self) -> &[[f64; 3]] {
        &self.points
    }

    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    pub fn harmonics(&self, lmax: usize) -> Vec<Vec<f64>> {
        self.points.iter().map(|&p| real_ylm(lmax, p)).collect()
    }

    pub fn gradients(&self, lmax: usize) -> Vec<Vec<[f64; 3]>> {
        (self.points.iter())
            .map(|&p| {
                let solid = solid_harmonics(lmax, p);
                (0..solid.len())
                    .map(|lm| {
                        let (value, gradient) = solid[lm];
                        let l = (lm as f64).sqrt() as usize as f64;
                        std::array::from_fn(|i| gradient[i] - l * value * p[i])
                    })
                    .collect()
            })
            .collect()
    }
}

pub(super) const SPHERE_DENSITY_AND_TAU_FLOOR: f64 = 1e-14;

const FRAME_TURN_RAD: f64 = 1e-3;

fn axis_rotation(axis: usize, angle: f64) -> [[f64; 3]; 3] {
    let (b, c) = ((axis + 1) % 3, (axis + 2) % 3);
    let mut r = [[0.0; 3]; 3];
    r[axis][axis] = 1.0;
    (r[b][b], r[c][c]) = (angle.cos(), angle.cos());
    (r[c][b], r[b][c]) = (angle.sin(), -angle.sin());
    r
}

struct MeshPart {
    energy: f64,
    v: Vec<Vec<Vec<f64>>>,
    v_tau: Vec<Vec<Vec<f64>>>,
}

struct Tables {
    mesh: AngularMesh,
    y: Vec<Vec<f64>>,
    gy: Vec<Vec<[f64; 3]>>,
}

struct Waves {
    u: Vec<Vec<f64>>,
    nabla: Vec<Vec<f64>>,
}

impl Species {
    pub fn one_center_on_mesh(&self, rho: &[Mat<f64>]) -> OneCenter {
        assert!(matches!(rho.len(), 1 | 2), "one ρ_ij, or one per spin");
        let (energies, xc) = self.mesh_xc(rho);
        let n = self.orbitals.len();
        let total = total_occupancies(rho);
        let dh = self.hartree(&total);
        let dot = |m: &Mat<f64>| {
            (0..n)
                .flat_map(|i| (0..n).map(move |j| (i, j)))
                .map(|(i, j)| total[(i, j)] * m[(i, j)])
                .sum::<f64>()
        };
        let (hartree, frozen) = (dot(&dh), dot(self.frozen_d()));
        OneCenter {
            energy_ha: energies[0] - energies[1] + 0.5 * hartree + frozen - self.core_xc(),
            d: xc.iter().map(|x| self.frozen_d() + &dh + x).collect(),
            xc_ha: energies,
        }
    }

    pub fn frame_torque(&self, rho: &[Mat<f64>]) -> [f64; 3] {
        let energy = |axis: usize, angle: f64| {
            let d = self.rotation(axis_rotation(axis, angle));
            let turned: Vec<Mat<f64>> = rho.iter().map(|p| &d * p * d.transpose()).collect();
            self.one_center(&turned).energy_ha
        };
        std::array::from_fn(|axis| {
            (energy(axis, FRAME_TURN_RAD) - energy(axis, -FRAME_TURN_RAD)) / (2.0 * FRAME_TURN_RAD)
        })
    }

    pub fn sphere_terms(
        &self,
        turn: Option<&Mat<f64>>,
        images: &[Mat<f64>],
        rho: &[Mat<f64>],
    ) -> (f64, Vec<Mat<f64>>) {
        let at = |rho: &[Mat<f64>]| match turn {
            None => self.one_center(rho),
            Some(turn) => {
                let turned: Vec<Mat<f64>> =
                    rho.iter().map(|r| turn * r * turn.transpose()).collect();
                let mut one = self.one_center(&turned);
                for d in &mut one.d {
                    *d = turn.transpose() * &*d * turn;
                }
                one
            }
        };
        if images.is_empty() {
            let one = at(rho);
            return (one.energy_ha, one.d);
        }
        let mut energy_ha = 0.0;
        let mut d = vec![Mat::zeros(rho[0].nrows(), rho[0].ncols()); rho.len()];
        let ones = per_point(images.len(), |i| {
            let r = &images[i];
            at(&rho
                .iter()
                .map(|p| r.transpose() * p * r)
                .collect::<Vec<_>>())
        });
        for (r, one) in images.iter().zip(ones) {
            energy_ha += one.energy_ha;
            for (total, d) in d.iter_mut().zip(&one.d) {
                *total += r * d * r.transpose();
            }
        }
        let n = images.len() as f64;
        (energy_ha / n, d.into_iter().map(|d| d / n).collect())
    }

    pub fn frame_strain(
        &self,
        frame: [[f64; 3]; 3],
        images: &[Mat<f64>],
        rho: &[Mat<f64>],
    ) -> [[f64; 3]; 3] {
        let turn = self.rotation(frame);
        let identity = [Mat::identity(rho[0].nrows(), rho[0].ncols())];
        let images = if images.is_empty() {
            &identity[..]
        } else {
            images
        };
        let mut t = [0.0; 3];
        for r in images {
            let upright: Vec<Mat<f64>> = (rho.iter())
                .map(|p| &turn * (r.transpose() * p * r) * turn.transpose())
                .collect();
            let torque = self.frame_torque(&upright);
            t.iter_mut()
                .zip(torque)
                .for_each(|(sum, x)| *sum += x / images.len() as f64);
        }
        let u = frame;
        std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                -(t[0] * u[2][i] * u[1][j] - t[1] * u[2][i] * u[0][j] + t[2] * u[1][i] * u[0][j])
            })
        })
    }

    pub fn flux_degree(&self) -> usize {
        2 * self.largest_l + 1
    }

    pub fn kinetic_moments(&self, rho: &Mat<f64>) -> [Vec<Vec<f64>>; 2] {
        let r = self.dataset.draft().grid.radii();
        let count = self.moments_count();
        let (ae, ps) = (self.waves(false), self.waves(true));
        let mut out = [
            vec![vec![0.0; self.size]; count],
            vec![vec![0.0; self.size]; count],
        ];
        let casimir = |l: usize| (l * (l + 1)) as f64;
        let n = self.orbitals.len();
        for i in 0..n {
            for j in 0..n {
                if rho[(i, j)] == 0.0 {
                    continue;
                }
                let (a, b) = (&self.orbitals[i], &self.orbitals[j]);
                for lm in 0..count {
                    let g = self.gaunt(lm, i, j);
                    if g == 0.0 {
                        continue;
                    }
                    let big = (lm as f64).sqrt() as usize;
                    let g_nabla = (casimir(a.l) + casimir(b.l) - casimir(big)) / 2.0 * g;
                    let half = 0.5 * rho[(i, j)];
                    for (part, w) in out.iter_mut().zip([&ae, &ps]) {
                        let (ua, ub) = (&w.u[a.channel], &w.u[b.channel]);
                        let (na, nb) = (&w.nabla[a.channel], &w.nabla[b.channel]);
                        for k in 1..self.size {
                            let r2 = r[k] * r[k];
                            part[lm][k] += half
                                * (g * na[k] * nb[k] / r2 + g_nabla * ua[k] * ub[k] / (r2 * r2));
                        }
                    }
                }
            }
        }
        for f in out.iter_mut().flatten() {
            f[0] = extrapolated([f[1], f[2], f[3]]);
        }
        out
    }

    fn waves(&self, pseudo: bool) -> Waves {
        let d = self.dataset.draft();
        let r = d.grid.radii();
        let u: Vec<Vec<f64>> = (d.channels.iter())
            .map(|c| {
                let f = if pseudo { &c.pseudo } else { &c.ae };
                (0..self.size).map(|k| f[k] * r[k]).collect()
            })
            .collect();
        let nabla = (u.iter())
            .map(|u| {
                let du = d.grid.derivative(u);
                let mut g: Vec<f64> = (0..self.size)
                    .map(|k| if k == 0 { 0.0 } else { du[k] - u[k] / r[k] })
                    .collect();
                g[0] = extrapolated([g[1], g[2], g[3]]);
                g
            })
            .collect();
        Waves { u, nabla }
    }

    fn mesh_xc(&self, rho: &[Mat<f64>]) -> ([f64; 2], Vec<Mat<f64>>) {
        let d = self.dataset.draft();
        let meta = self.functional.rung() == Rung::MetaGga;
        let mesh = AngularMesh::abinit();
        let flux_lmax = self.flux_degree();
        let lmax = flux_lmax.max(self.moments_lmax());
        let tables = Tables {
            y: mesh.harmonics(lmax),
            gy: mesh.gradients(lmax),
            mesh,
        };
        let (ae, ps): (Vec<_>, Vec<_>) = rho.iter().map(|r| self.moment_densities(r)).unzip();
        let (tau_ae, tau_ps): (Vec<_>, Vec<_>) = match meta {
            true => rho
                .iter()
                .map(|r| {
                    let [a, p] = self.kinetic_moments(r);
                    (a, p)
                })
                .unzip(),
            false => (Vec::new(), Vec::new()),
        };
        let kinetic = meta.then(|| {
            (d.core_kinetic.as_ref())
                .expect("a meta-GGA species has the dataset's core kinetic energy densities")
        });
        let tcore = self.dataset().has_pseudo_core();
        let zeros = vec![0.0; self.size];
        let pseudo_tau_core = match (kinetic, tcore) {
            (Some(k), true) => &k.pseudo()[..self.size],
            _ => &zeros[..],
        };
        let all = self.mesh_part(
            &ae,
            meta.then_some(&tau_ae[..]),
            &d.core_density[..self.size],
            kinetic.map(|k| &k.ae()[..self.size]),
            &tables,
            flux_lmax,
        );
        let pseudo = self.mesh_part(
            &ps,
            meta.then_some(&tau_ps[..]),
            &d.pseudo_core_density[..self.size],
            meta.then_some(pseudo_tau_core),
            &tables,
            flux_lmax,
        );
        let xc = (0..rho.len())
            .map(|s| self.mesh_d(&all, &pseudo, s, &tables))
            .collect();
        ([all.energy, pseudo.energy], xc)
    }

    fn mesh_part(
        &self,
        moments: &[Vec<Vec<f64>>],
        tau: Option<&[Vec<Vec<f64>>]>,
        core: &[f64],
        core_tau: Option<&[f64]>,
        tables: &Tables,
        flux_lmax: usize,
    ) -> MeshPart {
        let (mesh, y, gy) = (&tables.mesh, &tables.y, &tables.gy);
        let grid = &self.dataset.draft().grid;
        let r = grid.radii();
        let (spins, size) = (moments.len(), self.size);
        let share = if spins == 1 { 1.0 } else { 0.5 };
        let count = moments[0].len();
        let radial = |f: &[f64]| -> (Vec<f64>, Vec<f64>) {
            let df = grid.derivative(f);
            let mut over: Vec<f64> = (0..size)
                .map(|k| if k == 0 { 0.0 } else { f[k] / r[k] })
                .collect();
            over[0] = extrapolated([over[1], over[2], over[3]]);
            (df, over)
        };
        let parts: Vec<Vec<(Vec<f64>, Vec<f64>)>> = (moments.iter())
            .map(|m| m.iter().map(|f| radial(f)).collect())
            .collect();
        let core_slope = grid.derivative(core);
        let points = mesh.points().len();
        type AtPoint = (Vec<f64>, Vec<Vec<f64>>, Vec<Vec<[f64; 3]>>, Vec<Vec<f64>>);
        let at: Vec<AtPoint> = per_point(points, |p| {
            let hat = mesh.points()[p];
            let mut energy = vec![0.0; size];
            let mut v = vec![vec![0.0; size]; spins];
            let mut flux = vec![vec![[0.0; 3]; size]; spins];
            let mut v_tau = vec![vec![0.0; size]; spins];
            for k in 0..size {
                let mut rho = [0.0; 2];
                let mut grad = [[0.0; 3]; 2];
                let mut kin = [0.0; 2];
                for s in 0..spins {
                    let mut value = share * core[k];
                    let mut g: [f64; 3] = std::array::from_fn(|i| share * core_slope[k] * hat[i]);
                    for lm in 0..count {
                        let m = moments[s][lm][k];
                        let (df, over) = (&parts[s][lm].0, &parts[s][lm].1);
                        value += m * y[p][lm];
                        for i in 0..3 {
                            g[i] += df[k] * hat[i] * y[p][lm] + over[k] * gy[p][lm][i];
                        }
                    }
                    rho[s] = value;
                    grad[s] = g;
                    if let (Some(tau), Some(core_tau)) = (tau, core_tau) {
                        kin[s] = share * core_tau[k]
                            + (0..tau[s].len())
                                .map(|lm| tau[s][lm][k] * y[p][lm])
                                .sum::<f64>();
                    }
                }
                let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
                let meta = tau.is_some();
                if spins == 1 {
                    let up = (rho[0] / 2.0).max(SPHERE_DENSITY_AND_TAU_FLOOR);
                    let tau_up = (kin[0] / 2.0).max(SPHERE_DENSITY_AND_TAU_FLOOR);
                    let sigma = dot(grad[0], grad[0]);
                    let [x, c] = unpolarized(
                        self.functional,
                        2.0 * up,
                        sigma,
                        meta.then_some(2.0 * tau_up),
                    );
                    energy[k] = r[k] * r[k] * rho[0] * (x.epsilon + c.epsilon);
                    v[0][k] = x.vrho + c.vrho;
                    if sigma >= 4.0 * MIN_SQUARED_DENSITY_GRADIENT_PER_BOHR8 {
                        let vsigma = x.vsigma + c.vsigma;
                        flux[0][k] = grad[0].map(|g| 2.0 * vsigma * g);
                    }
                    v_tau[0][k] = x.vtau + c.vtau;
                } else {
                    let floored = rho.map(|x| x.max(SPHERE_DENSITY_AND_TAU_FLOOR));
                    let sigma = [
                        dot(grad[0], grad[0]),
                        dot(grad[0], grad[1]),
                        dot(grad[1], grad[1]),
                    ];
                    let mut point = Point::of(floored[0], floored[1], sigma);
                    if meta {
                        point = point.with_tau(kin.map(|t| t.max(SPHERE_DENSITY_AND_TAU_FLOOR)));
                    }
                    let [x, c] = evaluate(self.functional, point);
                    energy[k] = r[k] * r[k] * (rho[0] + rho[1]) * (x.epsilon + c.epsilon);
                    let vs: [f64; 3] = std::array::from_fn(|i| x.vsigma[i] + c.vsigma[i]);
                    let both: [f64; 3] = std::array::from_fn(|i| grad[0][i] + grad[1][i]);
                    for s in 0..2 {
                        v[s][k] = x.vrho[s] + c.vrho[s];
                        v_tau[s][k] = x.vtau[s] + c.vtau[s];
                        let own = if dot(grad[s], grad[s]) >= MIN_SQUARED_DENSITY_GRADIENT_PER_BOHR8
                        {
                            2.0 * vs[2 * s] - vs[1]
                        } else {
                            0.0
                        };
                        let mixed = if dot(both, both) >= MIN_SQUARED_DENSITY_GRADIENT_PER_BOHR8 {
                            vs[1]
                        } else {
                            0.0
                        };
                        flux[s][k] = std::array::from_fn(|i| own * grad[s][i] + mixed * both[i]);
                    }
                }
            }
            (energy, v, flux, v_tau)
        });
        let integrated: f64 = (0..points)
            .map(|p| mesh.weights()[p] * grid.integrate(&at[p].0[..self.end]))
            .sum();
        let mut v: Vec<Vec<Vec<f64>>> = (0..spins)
            .map(|s| (0..points).map(|p| at[p].1[s].clone()).collect())
            .collect();
        let v_tau = (0..spins)
            .map(|s| (0..points).map(|p| at[p].3[s].clone()).collect())
            .collect();
        if self.functional.rung() >= Rung::Gga {
            let flux_count = (flux_lmax + 1).pow(2);
            for (s, v) in v.iter_mut().enumerate() {
                for lm in 0..flux_count {
                    for i in 0..3 {
                        let g: Vec<f64> = (0..size)
                            .map(|k| {
                                4.0 * PI
                                    * (0..points)
                                        .map(|p| mesh.weights()[p] * at[p].2[s][k][i] * y[p][lm])
                                        .sum::<f64>()
                            })
                            .collect();
                        let (dg, over) = radial(&g);
                        for (p, v) in v.iter_mut().enumerate() {
                            let hat = mesh.points()[p];
                            for k in 0..size {
                                v[k] -= dg[k] * hat[i] * y[p][lm] + over[k] * gy[p][lm][i];
                            }
                        }
                    }
                }
            }
        }
        MeshPart {
            energy: 4.0 * PI * integrated,
            v,
            v_tau,
        }
    }

    fn mesh_d(&self, all: &MeshPart, pseudo: &MeshPart, s: usize, tables: &Tables) -> Mat<f64> {
        let (mesh, y, gy) = (&tables.mesh, &tables.y, &tables.gy);
        let d = self.dataset.draft();
        let (grid, r) = (&d.grid, d.grid.radii());
        let end = self.end;
        let meta = self.functional.rung() == Rung::MetaGga;
        let (ae, ps) = (self.waves(false), self.waves(true));
        let channels = d.channels.len();
        let points = mesh.points().len();
        let radial: Vec<Vec<[f64; 3]>> = per_point(points, |p| {
            let (v, tv) = (&all.v[s][p], &pseudo.v[s][p]);
            let mut out = vec![[0.0; 3]; channels * channels];
            for a in 0..channels {
                for b in a..channels {
                    let product = |k: usize| (ae.u[a][k] * ae.u[b][k], ps.u[a][k] * ps.u[b][k]);
                    let f: Vec<f64> = (0..end)
                        .map(|k| {
                            let (uu, tt) = product(k);
                            v[k] * uu - tv[k] * tt
                        })
                        .collect();
                    let mut terms = [grid.integrate(&f), 0.0, 0.0];
                    if meta {
                        let (vt, tvt) = (&all.v_tau[s][p], &pseudo.v_tau[s][p]);
                        let f: Vec<f64> = (0..end)
                            .map(|k| {
                                vt[k] * ae.nabla[a][k] * ae.nabla[b][k]
                                    - tvt[k] * ps.nabla[a][k] * ps.nabla[b][k]
                            })
                            .collect();
                        terms[1] = 0.5 * grid.integrate(&f);
                        let mut f: Vec<f64> = (0..end)
                            .map(|k| {
                                if k == 0 {
                                    return 0.0;
                                }
                                let (uu, tt) = product(k);
                                (vt[k] * uu - tvt[k] * tt) / (r[k] * r[k])
                            })
                            .collect();
                        f[0] = extrapolated([f[1], f[2], f[3]]);
                        terms[2] = grid.integrate(&f);
                    }
                    out[a * channels + b] = terms;
                    out[b * channels + a] = terms;
                }
            }
            out
        });
        let n = self.orbitals.len();
        Mat::from_fn(n, n, |i, j| {
            let (oi, oj) = (&self.orbitals[i], &self.orbitals[j]);
            let pair = oi.channel * channels + oj.channel;
            (0..points)
                .map(|p| {
                    let [vv, tau_radial, tau_angular] = radial[p][pair];
                    let yy = y[p][oi.lm()] * y[p][oj.lm()];
                    let gg: f64 = (0..3).map(|x| gy[p][oi.lm()][x] * gy[p][oj.lm()][x]).sum();
                    4.0 * PI * mesh.weights()[p] * (yy * (vv + tau_radial) + 0.5 * gg * tau_angular)
                })
                .sum()
        })
    }
}
