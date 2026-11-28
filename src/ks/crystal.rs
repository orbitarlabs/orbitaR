pub(super) use super::density::Density;
use super::density::{Occupied, Sums};
use super::diagnosis::{Diagnosis, SphereState};
pub(super) use super::field::Field;
use super::run::Discretization;
use super::{
    Failure, Hamiltonian, Local, Nonlocal, Weight, ewald_energy_ha, ewald_forces_ha_bohr,
    ewald_stress_ha_bohr3,
};
use crate::model::{KPoints, Solver, Spec, operations};
use crate::paw::{Dataset, FormFactors, Projectors, Sampled, Species, Sphere, total_occupancies};
use crate::pw::{
    Basis, Complex64, Grid, Guess, Hartree, Kinetic, Mat, MatRef, Pairs, Pencil, Pool, Scalar,
    SphereFft, Spins, Workspace, Xc, c64, dense, exchange_correlation, grid_points, hartree,
    kinetic_strain, lowest, per_point, pool_threads, ritz_start, ritz_start_paired,
};
use crate::xc::Variable;
use std::collections::HashMap;
use std::f64::consts::TAU;
use std::sync::{Arc, Mutex, PoisonError};

const ITERATIONS: usize = 100;
const BANDS_PER_THREAD: usize = 4;
const SOLVES_IN_FLIGHT: usize = 2;

const ORTHOGONAL: f64 = 1e-10;

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0100_0000_01b3;

fn fnv(millers: impl Iterator<Item = [i32; 3]>) -> u64 {
    let mut hash = FNV_OFFSET;
    for byte in millers.flatten().flat_map(i32::to_le_bytes) {
        hash = (hash ^ u64::from(byte)).wrapping_mul(FNV_PRIME);
    }
    hash
}

fn phase(m: [i32; 3], position: [f64; 3]) -> f64 {
    -TAU * (0..3).map(|i| f64::from(m[i]) * position[i]).sum::<f64>()
}

fn occupied_sum(weight: f64, f: &[f64], derivative: &Mat<c64>, m: &Mat<c64>) -> f64 {
    let mut sum = 0.0;
    for (band, occupation) in f.iter().enumerate() {
        for i in 0..m.nrows() {
            sum += weight * occupation * 2.0 * (derivative[(i, band)] * m[(i, band)].conj()).re;
        }
    }
    sum
}

fn spin_mean(channels: &[Vec<f64>]) -> Vec<f64> {
    let spins = channels.len() as f64;
    (0..channels[0].len())
        .map(|r| channels.iter().map(|v| v[r]).sum::<f64>() / spins)
        .collect()
}

fn core_tau(form: &FormFactors, q: f64) -> f64 {
    (form.pseudo_core_kinetic(q)).expect("ks::run takes a meta-GGA dataset with its core τ")
}

fn core_tau_slope(form: &FormFactors, q: f64) -> f64 {
    (form.pseudo_core_kinetic_slope(q)).expect("ks::run takes a meta-GGA dataset with its core τ")
}

fn batch_bands() -> usize {
    BANDS_PER_THREAD * pool_threads()
}

struct Kind {
    species: Arc<Species>,
    form: FormFactors,
    projectors: Projectors,
    orbitals: Projectors,
    overlap: Mat<f64>,
    upright: Option<Mat<f64>>,
}

struct Atom {
    kind: usize,
    position: [f64; 3],
    sphere: Sphere,
}

pub(super) struct Point {
    k: [f64; 3],
    basis: Basis,
    pub(super) fft: SphereFft,
    projectors: Vec<Sampled>,
    pairs: Option<Pairs>,
}

pub(super) struct Fine {
    smooth: Vec<Vec<f64>>,
    total: Vec<f64>,
    kinetic: Option<Vec<Vec<f64>>>,
    hartree: Hartree,
    xc: Xc,
    electrostatic: Vec<f64>,
}

struct Symmetry {
    sources: Vec<(usize, c64)>,
    preimages: Vec<usize>,
    orbital_rotations: Vec<Mat<f64>>,
    cartesian: [[f64; 3]; 3],
}

pub(super) enum Start {
    Atoms { columns: usize, seed: u64 },
    Warm(Mat<c64>),
}

#[derive(Clone, Copy)]
pub(super) struct Tolerance {
    pub(super) target: f64,
    pub(super) accepted: f64,
}

pub(super) struct Bands {
    pub(super) values: Vec<f64>,
    pub(super) vectors: Mat<c64>,
    pub(super) converged: bool,
}

pub(super) struct Crystal<'a> {
    spec: &'a Spec,
    coarse: Grid,
    fine: Grid,
    kinds: Vec<Kind>,
    atoms: Vec<Atom>,
    pub(super) points: Vec<Point>,
    pub(super) kpoints: KPoints,
    symmetries: Vec<Symmetry>,
    accepted: Vec<([[i32; 3]; 3], [f64; 3])>,
    images: Vec<Vec<Mat<f64>>>,
    sphere: Vec<(usize, f64)>,
    fine_slots: Vec<usize>,
    local: Vec<f64>,
    core: Vec<f64>,
    core_kinetic: Option<Vec<f64>>,
    workspaces: Mutex<Vec<Workspace<c64>>>,
    paired_workspaces: Mutex<Vec<Workspace<f64>>>,
    pub(super) electrons: f64,
    pub(super) spins: usize,
    pub(super) ewald_ha: f64,
    pub(super) psp_core_ha: f64,
}

impl<'a> Crystal<'a> {
    pub(super) fn assembled(spec: &'a Spec, datasets: &[Dataset]) -> Result<Self, Failure> {
        let draft = spec.draft();
        let (cell, cutoff, density_cutoff) =
            (&draft.cell, draft.cutoff_ha, spec.density_cutoff_ha());
        let coarse = Grid::coarse(cell, cutoff);
        let fine = Grid::fine(density_cutoff, &coarse);
        let largest_l = (datasets.iter())
            .flat_map(|d| d.draft().channels.iter().map(|c| c.l))
            .max()
            .unwrap_or(0);
        let kinds: Vec<Kind> = per_point(datasets.len(), |kind| {
            let dataset = &datasets[kind];
            let species = Species::for_functional(dataset.clone(), spec.functional())
                .expect("ks::run refuses a meta-GGA run of a dataset without core τ");
            let species = Arc::new(species.with_largest_l(largest_l));
            let meta = spec.functional().rung().reads(Variable::TauUp);
            Kind {
                upright: meta.then(|| species.rotation(cell.frame())),
                overlap: species.overlap(),
                form: FormFactors::tabulated(dataset, (8.0 * density_cutoff).sqrt()),
                projectors: Projectors::tabulated(dataset, (2.0 * cutoff).sqrt()),
                orbitals: Projectors::pseudo_orbitals(dataset, (2.0 * cutoff).sqrt()),
                species,
            }
        });
        let atoms: Vec<Atom> = per_point(draft.atoms.len(), |atom| {
            let (z, position) = draft.atoms[atom];
            let kind = (datasets.iter())
                .position(|d| d.draft().z == f64::from(z))
                .expect("ks::run gives one dataset per element");
            Atom {
                kind,
                position,
                sphere: kinds[kind].species.sphere(&fine, position),
            }
        });
        let sphere = coarse.density_sphere(cutoff);
        let millers = coarse.millers();
        let fine_slots = (sphere.iter())
            .map(|&(slot, _)| {
                fine.slot(millers[slot])
                    .expect("the fine box holds the coarse one")
            })
            .collect();
        let index: HashMap<[i32; 3], usize> = (sphere.iter().enumerate())
            .map(|(i, &(slot, _))| (millers[slot], i))
            .collect();
        let meta = spec.functional().rung().reads(Variable::TauUp);
        let reducible = draft.kpoints.reducible();
        let found = if reducible {
            operations(cell, &draft.atoms, draft.moments.as_deref()).map_err(Failure::Invalid)?
        } else if meta {
            operations(cell, &draft.atoms, draft.moments.as_deref()).unwrap_or_default()
        } else {
            Vec::new()
        };
        let mut skew: f64 = 0.0;
        let mut accepted = Vec::new();
        let (rotations, symmetries): (Vec<_>, Vec<_>) = (found.into_iter())
            .filter(|o| {
                draft.kpoints.keeps(&o.rotation)
                    && o.keeps_grid(coarse.dims())
                    && o.keeps_grid(fine.dims())
            })
            .filter_map(|o| {
                let sources = (sphere.iter())
                    .map(|&(slot, _)| {
                        let g = millers[slot];
                        let source: [i32; 3] =
                            std::array::from_fn(|i| (0..3).map(|j| o.rotation[j][i] * g[j]).sum());
                        let shift: f64 = (0..3).map(|i| f64::from(g[i]) * o.translation[i]).sum();
                        index
                            .get(&source)
                            .map(|&at| (at, c64::from_polar(1.0, -TAU * shift)))
                    })
                    .collect::<Option<_>>()?;
                accepted.push((o.rotation, o.translation));
                let w = o.cartesian;
                let squares: f64 = (0..3)
                    .flat_map(|i| (0..3).map(move |j| (i, j)))
                    .map(|(i, j)| {
                        let product: f64 = (0..3).map(|k| w[k][i] * w[k][j]).sum();
                        (product - f64::from(u8::from(i == j))).powi(2)
                    })
                    .sum();
                skew = skew.max(squares.sqrt());
                let cartesian = if meta { o.isometry() } else { o.cartesian };
                let orbital_rotations = (kinds.iter())
                    .map(|k| k.species.rotation(cartesian))
                    .collect();
                Some((
                    o.rotation,
                    Symmetry {
                        sources,
                        preimages: o.preimages,
                        orbital_rotations,
                        cartesian,
                    },
                ))
            })
            .unzip();
        if meta && draft.stress && skew > ORTHOGONAL {
            return Err(Failure::Unsupported(format!(
                "outputs.stress: an operation of this cell is orthogonal only to ‖WᵀW − 1‖_F = {skew:.1e} (the finder's \
                 tolerance), so the spheres' images by its polar factor would move with the strain; give the cell its \
                 exact symmetry"
            )));
        }
        let (rotations, symmetries, images) = if reducible {
            (rotations, symmetries, Vec::new())
        } else {
            let mut distinct: Vec<usize> = Vec::new();
            for (i, r) in rotations.iter().enumerate() {
                if !distinct.iter().any(|&j| rotations[j] == *r) {
                    distinct.push(i);
                }
            }
            let images = (kinds.iter())
                .map(|k| {
                    (distinct.iter())
                        .map(|&i| k.species.rotation(symmetries[i].cartesian))
                        .collect()
                })
                .collect();
            (Vec::new(), Vec::new(), images)
        };
        let kpoints = draft.kpoints.irreducible(&rotations);
        let points = per_point(kpoints.points().len(), |point| {
            let k = kpoints.points()[point];
            let basis = Basis::sphere(cell, k, cutoff);
            let fft =
                (coarse.sphere(basis.millers())).expect("the coarse box holds every plane wave");
            let projectors = (kinds.iter())
                .map(|kind| kind.projectors.sampled(cell, k, &basis))
                .collect();
            Point {
                k,
                pairs: basis.pairs(k),
                basis,
                fft,
                projectors,
            }
        });
        let mut crystal = Self {
            spec,
            local: Vec::new(),
            core: Vec::new(),
            core_kinetic: None,
            ewald_ha: 0.0,
            psp_core_ha: 0.0,
            workspaces: Mutex::default(),
            paired_workspaces: Mutex::default(),
            electrons: 0.0,
            spins: if draft.moments.is_some() { 2 } else { 1 },
            coarse,
            kinds,
            atoms,
            points,
            kpoints,
            symmetries,
            accepted,
            images,
            sphere,
            fine_slots,
            fine,
        };
        crystal.local = crystal.on_fine(|form, q| {
            if q > 0.0 {
                form.local_potential(q)
            } else {
                0.0
            }
        });
        crystal.core = crystal.on_fine(FormFactors::pseudo_core);
        let pseudo_core = (crystal.kinds.iter()).any(|k| k.species.dataset().has_pseudo_core());
        if crystal.reads_tau() && pseudo_core {
            crystal.core_kinetic = Some(crystal.on_fine(|form, q| {
                (form.pseudo_core_kinetic(q))
                    .expect("ks::run takes a meta-GGA dataset with its core τ")
            }));
        }
        let ions = crystal.ions();
        crystal.electrons = ions.iter().map(|(valence, _)| valence).sum();
        let epsatm: f64 = (crystal
            .atoms
            .iter()
            .map(|a| crystal.kinds[a.kind].form.epsatm()))
        .sum();
        crystal.psp_core_ha = epsatm * crystal.electrons / cell.volume();
        crystal.ewald_ha = ewald_energy_ha(cell, &ions)?;
        Ok(crystal)
    }

    fn ions(&self) -> Vec<(f64, [f64; 3])> {
        (self.atoms.iter())
            .map(|a| {
                let valence = self.kinds[a.kind]
                    .species
                    .dataset()
                    .draft()
                    .valence_electrons;
                (valence, a.position)
            })
            .collect()
    }

    fn structure_sum(&self, m: [i32; 3], g2: f64, f: &impl Fn(usize, f64) -> f64) -> c64 {
        (self.atoms.iter().enumerate())
            .map(|(index, a)| c64::from_polar(f(index, g2.sqrt()), phase(m, a.position)))
            .sum()
    }

    fn form(&self, atom: usize) -> &FormFactors {
        &self.kinds[self.atoms[atom].kind].form
    }

    fn on_fine(&self, f: impl Fn(&FormFactors, f64) -> f64 + Sync + Send) -> Vec<f64> {
        let n = self.fine.points().len();
        let scale = n as f64 / self.spec.draft().cell.volume();
        let millers = self.fine.millers();
        let sphere = self.fine.density_sphere(self.spec.density_cutoff_ha());
        let sums = per_point(sphere.len(), |i| {
            let (slot, g2) = sphere[i];
            scale * self.structure_sum(millers[slot], g2, &|a, q| f(self.form(a), q))
        });
        let mut values = vec![Complex64::default(); n];
        for (&(slot, _), sum) in sphere.iter().zip(sums) {
            values[slot] = sum;
        }
        self.fine.inverse(&mut values);
        values.iter().map(|z| z.re).collect()
    }

    pub(super) fn guess(&self) -> Density {
        let draft = self.spec.draft();
        let (volume, millers) = (draft.cell.volume(), self.coarse.millers());
        let share = |spin: usize, atom: usize| match &draft.moments {
            None => 1.0,
            Some(moments) => {
                let dataset = self.kinds[self.atoms[atom].kind].species.dataset().draft();
                let sign = if spin == 0 { 1.0 } else { -1.0 };
                0.5 * (1.0 + sign * moments[atom] / dataset.valence_electrons)
            }
        };
        let smooth = (0..self.spins)
            .map(|spin| {
                per_point(self.sphere.len(), |i| {
                    let (slot, g2) = self.sphere[i];
                    let f = |a: usize, q: f64| share(spin, a) * self.form(a).pseudo_valence(q);
                    self.structure_sum(millers[slot], g2, &f) / volume
                })
            })
            .collect();
        let onsite = (self.atoms.iter().enumerate())
            .map(|(index, a)| {
                let free = self.kinds[a.kind].species.initial_occupancies();
                (0..self.spins).map(|c| &free * share(c, index)).collect()
            })
            .collect();
        let kinetic =
            (self.reads_tau()).then(|| vec![vec![c64::default(); self.sphere.len()]; self.spins]);
        Density {
            smooth,
            onsite,
            kinetic,
        }
    }

    fn reads_tau(&self) -> bool {
        self.spec.functional().rung().reads(Variable::TauUp)
    }

    fn core_kinetic_potential(&self, fine: &Fine) -> Option<Vec<f64>> {
        self.core_kinetic.as_ref()?;
        let v_tau = (fine.xc.kinetic_potential.as_deref()).expect("v_τ beside the grid's τ̃_c");
        Some(spin_mean(v_tau))
    }

    fn on_coarse(&self, values: &[f64]) -> Vec<f64> {
        let coefficients = (self.fine).coefficients(values, self.fine_slots.iter().copied());
        (self.coarse).synthesized(
            self.sphere.iter().map(|&(slot, _)| slot),
            coefficients.into_iter(),
        )
    }

    pub(super) fn field(&self, density: &Density, fine: &Fine) -> Field {
        let draft = self.spec.draft();
        let n = self.fine.dims().iter().product::<usize>();
        let Fine {
            total,
            hartree: h,
            xc,
            electrostatic,
            ..
        } = fine;
        let volume_element = draft.cell.volume() / n as f64;
        let local_ha = volume_element
            * self
                .local
                .iter()
                .zip(total)
                .map(|(v, n)| v * n)
                .sum::<f64>();
        let v = &xc.potential;
        let v_tau: Vec<Option<Vec<f64>>> = match &xc.kinetic_potential {
            Some(v_tau) => v_tau.iter().map(|v| Some(self.on_coarse(v))).collect(),
            None => vec![None; v.len()],
        };
        let rung = self.spec.functional().rung();
        let local = (self.fine)
            .transfers(v.len(), |s, r| electrostatic[r] + v[s][r], &self.coarse)
            .into_iter()
            .zip(v_tau)
            .map(|(v, v_tau)| Local::of(v, v_tau, rung).expect("v_τ at the meta-GGA rung alone"))
            .collect();
        let atoms = per_point(self.atoms.len(), |a| {
            let (atom, rho) = (&self.atoms[a], &density.onsite[a]);
            let kind = &self.kinds[atom.kind];
            let images = self.images.get(atom.kind).map_or(&[][..], Vec::as_slice);
            let (energy_ha, d) = kind
                .species
                .sphere_terms(kind.upright.as_ref(), images, rho);
            let hat = atom.sphere.potential_d(electrostatic);
            (energy_ha, d.iter().map(|d| d + &hat).collect::<Vec<_>>())
        });
        let one_center_ha = atoms.iter().fold(0.0, |sum, (e, _)| sum + e);
        let d: Vec<Vec<Mat<f64>>> = atoms.into_iter().map(|(_, d)| d).collect();
        let d = match (self.reads_tau(), !self.symmetries.is_empty()) {
            (true, true) => {
                let n = self.symmetries.len() as f64;
                (0..self.atoms.len())
                    .map(|a| {
                        (0..d[a].len())
                            .map(|spin| {
                                let mut sum =
                                    Mat::<f64>::zeros(d[a][spin].nrows(), d[a][spin].ncols());
                                for s in &self.symmetries {
                                    let r = &s.orbital_rotations[self.atoms[a].kind];
                                    sum += r * &d[s.preimages[a]][spin] * r.transpose();
                                }
                                sum / n
                            })
                            .collect()
                    })
                    .collect()
            }
            _ => d,
        };
        Field {
            local,
            d,
            hartree_ha: h.energy_ha,
            xc_ha: xc.energy_ha,
            local_ha,
            one_center_ha,
        }
    }

    pub(super) fn first_order_ha(&self, field: &Field, a: &Density, b: &Density) -> f64 {
        let contraction = |v: &[f64], a: &[c64], b: &[c64]| {
            let slots = self.sphere.iter().map(|&(slot, _)| slot);
            let change = a.iter().zip(b).map(|(a, b)| b - a);
            let change = self.coarse.synthesized(slots, change);
            change.iter().zip(v).map(|(d, v)| d * v).sum::<f64>()
        };
        let smooth: f64 = (field.local.iter())
            .zip(a.smooth.iter().zip(&b.smooth))
            .map(|(local, (a, b))| contraction(local.v(), a, b))
            .sum();
        let smooth = match (&a.kinetic, &b.kinetic) {
            (Some(ta), Some(tb)) => {
                let tau: f64 = (field.local.iter())
                    .zip(ta.iter().zip(tb))
                    .map(|(local, (a, b))| contraction(local.v_tau().expect("v_τ beside τ̃"), a, b))
                    .sum();
                smooth + tau
            }
            _ => smooth,
        };
        let onsite: f64 = (field.d.iter())
            .zip(a.onsite.iter().zip(&b.onsite))
            .flat_map(|(d, (a, b))| d.iter().zip(a.iter().zip(b)))
            .map(|(d, (a, b))| {
                (0..d.nrows())
                    .flat_map(|i| (0..d.ncols()).map(move |j| (i, j)))
                    .map(|at| d[at] * (b[at] - a[at]))
                    .sum::<f64>()
            })
            .sum();
        let n = self.coarse.dims().iter().product::<usize>();
        self.spec.draft().cell.volume() / n as f64 * smooth + onsite
    }

    pub(super) fn fine(&self, density: &Density) -> Fine {
        let densities: Vec<Vec<f64>> = (density.smooth.iter())
            .map(|smooth| {
                (self.fine).synthesized(self.fine_slots.iter().copied(), smooth.iter().copied())
            })
            .collect();
        let mut total = densities[0].clone();
        for spin in &densities[1..] {
            total.iter_mut().zip(spin).for_each(|(t, c)| *t += c);
        }
        for (atom, rho) in self.atoms.iter().zip(&density.onsite) {
            atom.sphere
                .add_compensation(&total_occupancies(rho), &mut total);
        }
        let h = hartree(&self.fine, &total, self.spec.density_cutoff_ha());
        let padded = |fields: &[Vec<c64>]| -> Vec<Vec<f64>> {
            (fields.iter())
                .map(|f| {
                    (self.fine).synthesized(self.fine_slots.iter().copied(), f.iter().copied())
                })
                .collect()
        };
        let kinetic = density.kinetic.as_deref().map(padded);
        let xc = exchange_correlation(
            &self.fine,
            self.spec.functional(),
            spins(&densities),
            Some(&self.core),
            kinetic.as_deref().map(|tau| Kinetic {
                valence: spins(tau),
                core: self.core_kinetic.as_deref(),
            }),
        );
        let electrostatic = per_point(total.len(), |r| h.potential[r] + self.local[r]);
        Fine {
            smooth: densities,
            kinetic,
            total,
            hartree: h,
            xc,
            electrostatic,
        }
    }

    pub(super) fn bands(
        &self,
        pool: Option<&Pool>,
        field: &Field,
        starts: Vec<Vec<Start>>,
        converged: usize,
        tolerance: Tolerance,
    ) -> Vec<Vec<Bands>> {
        let solve =
            |spin, point, start| self.solve(field, spin, point, start, converged, tolerance);
        let Some(pool) = pool else {
            return (starts.into_iter().enumerate())
                .map(|(spin, starts)| {
                    (self.points.iter().zip(starts))
                        .map(|(point, start)| solve(spin, point, start))
                        .collect()
                })
                .collect();
        };
        let spins = starts.len();
        let queue = Mutex::new(starts.into_iter().flatten().enumerate());
        let in_flight = || {
            let mut done = Vec::new();
            loop {
                let next = queue.lock().unwrap_or_else(PoisonError::into_inner).next();
                let Some((task, guess)) = next else {
                    return done;
                };
                let (spin, k) = (task / self.points.len(), task % self.points.len());
                done.push((task, solve(spin, &self.points[k], guess)));
            }
        };
        let results: Vec<Vec<(usize, Bands)>> =
            pool.run(|| per_point(SOLVES_IN_FLIGHT, |_| in_flight()));
        let mut solved: Vec<(usize, Bands)> = results.into_iter().flatten().collect();
        solved.sort_by_key(|&(task, _)| task);
        let mut solved = solved.into_iter().map(|(_, bands)| bands);
        (0..spins)
            .map(|_| solved.by_ref().take(self.points.len()).collect())
            .collect()
    }

    pub(super) fn forces(
        &self,
        density: &Density,
        bands: &[Vec<Bands>],
        occupations: &[Vec<Vec<f64>>],
    ) -> Result<Vec<[f64; 3]>, Failure> {
        let fine = self.fine(density);
        let field = self.field(density, &fine);
        let mut forces = ewald_forces_ha_bohr(&self.spec.draft().cell, &self.ions())?;
        self.form_factor_forces(&fine, &mut forces);
        self.compensation_forces(density, &fine, &mut forces);
        self.nonlocal_forces(&field, bands, occupations, &mut forces);
        Ok(self.symmetrized_forces(forces))
    }

    fn form_factor_forces(&self, fine: &Fine, forces: &mut [[f64; 3]]) {
        let n = self.fine.points().len();
        let transformed = |values: &[f64]| {
            let mut t: Vec<Complex64> = values.iter().map(|&x| x.into()).collect();
            self.fine.forward(&mut t);
            t
        };
        let total = transformed(&fine.total);
        let xc = transformed(&spin_mean(&fine.xc.potential));
        let kinetic = self.core_kinetic_potential(fine).map(|v| transformed(&v));
        let sphere = self.fine.density_sphere(self.spec.density_cutoff_ha());
        let (millers, wavevectors) = (self.fine.millers(), self.fine.wavevectors());
        let added = per_point(self.atoms.len(), |a| {
            let (atom, form) = (&self.atoms[a], self.form(a));
            let mut force = forces[a];
            for &(slot, g2) in sphere.iter().filter(|&&(_, g2)| g2 > 0.0) {
                let q = g2.sqrt();
                let structure = c64::from_polar(1.0, phase(millers[slot], atom.position));
                let mut c = form.local_potential(q) * structure * total[slot].conj()
                    + form.pseudo_core(q) * structure * xc[slot].conj();
                if let Some(v_tau) = &kinetic {
                    c += core_tau(form, q) * structure * v_tau[slot].conj();
                }
                let weight = c.im / n as f64;
                for axis in 0..3 {
                    force[axis] -= wavevectors[slot][axis] * weight;
                }
            }
            force
        });
        forces.copy_from_slice(&added);
    }

    fn compensation_forces(&self, density: &Density, fine: &Fine, forces: &mut [[f64; 3]]) {
        let added = per_point(self.atoms.len(), |a| {
            let rho = total_occupancies(&density.onsite[a]);
            let hat = (self.atoms[a].sphere).compensation_gradient(&rho, &fine.electrostatic);
            std::array::from_fn(|axis| forces[a][axis] - hat[axis])
        });
        forces.copy_from_slice(&added);
    }

    fn nonlocal_forces(
        &self,
        field: &Field,
        bands: &[Vec<Bands>],
        occupations: &[Vec<Vec<f64>>],
        forces: &mut [[f64; 3]],
    ) {
        let cell = &self.spec.draft().cell;
        let weights = self.kpoints.weights();
        for (spin, (bands, occupations)) in bands.iter().zip(occupations).enumerate() {
            let solved = (self.points.iter().zip(self.kpoints.points()))
                .zip(bands.iter().zip(occupations))
                .zip(weights);
            for (((point, &k), (b, f)), &weight) in solved {
                let x = b.vectors.subcols(0, f.len());
                for (a, projectors) in self.projectors(point).iter().enumerate() {
                    let m = self.residual_projections(field, spin, a, projectors, x, &b.values);
                    for (axis, force) in forces[a].iter_mut().enumerate() {
                        let derivative =
                            Projectors::displaced(projectors, cell, k, &point.basis, x, axis);
                        *force -= occupied_sum(weight, f, &derivative, &m);
                    }
                }
            }
        }
    }

    fn residual_projections(
        &self,
        field: &Field,
        spin: usize,
        a: usize,
        projectors: &Mat<c64>,
        x: MatRef<'_, c64>,
        values: &[f64],
    ) -> Mat<c64> {
        let kind = &self.kinds[self.atoms[a].kind];
        let p = projectors.adjoint() * x;
        let d = &field.d[a][spin];
        let d_p = Mat::from_fn(d.nrows(), d.ncols(), |i, j| c64::new(d[(i, j)], 0.0)) * &p;
        let q_p = Mat::from_fn(d.nrows(), d.ncols(), |i, j| {
            c64::new(kind.overlap[(i, j)], 0.0)
        }) * &p;
        Mat::from_fn(p.nrows(), x.ncols(), |i, band| {
            d_p[(i, band)] - q_p[(i, band)] * values[band]
        })
    }

    pub(super) fn stress(
        &self,
        density: &Density,
        bands: &[Vec<Bands>],
        occupations: &[Vec<Vec<f64>>],
    ) -> Result<[[f64; 3]; 3], Failure> {
        let cell = &self.spec.draft().cell;
        let volume = cell.volume();
        let fine = self.fine(density);
        let field = self.field(density, &fine);
        let mut strain =
            ewald_stress_ha_bohr3(cell, &self.ions())?.map(|row| row.map(|x| x * volume));
        let parts = [
            self.density_strain(density, &fine),
            self.band_strain(&field, bands, occupations),
            self.frame_strain(density),
        ];
        for part in parts {
            for (row, add) in strain.iter_mut().zip(part) {
                row.iter_mut().zip(add).for_each(|(x, y)| *x += y);
            }
        }
        let averaged = self.symmetrized_tensor(strain);
        Ok(std::array::from_fn(|i| {
            std::array::from_fn(|j| (averaged[i][j] + averaged[j][i]) / (2.0 * volume))
        }))
    }

    fn density_strain(&self, density: &Density, fine: &Fine) -> [[f64; 3]; 3] {
        let n = self.fine.points().len();
        let volume = self.spec.draft().cell.volume();
        let element = volume / n as f64;
        let integral =
            |a: &[f64], b: &[f64]| element * a.iter().zip(b).map(|(x, y)| x * y).sum::<f64>();
        let mean = spin_mean(&fine.xc.potential);
        let smooth: Vec<f64> = (0..n)
            .map(|r| fine.smooth.iter().map(|d| d[r]).sum())
            .collect();
        let held: f64 = (fine.xc.potential.iter().zip(&fine.smooth))
            .map(|(v, d)| integral(v, d))
            .sum();
        let held_kinetic: f64 = match (&fine.xc.kinetic_potential, &fine.kinetic) {
            (Some(v_tau), Some(tau)) => v_tau.iter().zip(tau).map(|(v, t)| integral(v, t)).sum(),
            _ => 0.0,
        };
        let core_kinetic = self.core_kinetic_potential(fine);
        let held_core_kinetic = match (&core_kinetic, &self.core_kinetic) {
            (Some(v), Some(tau)) => integral(v, tau),
            _ => 0.0,
        };
        let diagonal = fine.hartree.energy_ha + fine.xc.energy_ha
            - integral(&fine.electrostatic, &smooth)
            - held
            - held_kinetic
            - integral(&mean, &self.core)
            - held_core_kinetic
            - self.psp_core_ha;
        let mut strain = fine.xc.gradient_strain_ha;
        for (i, row) in strain.iter_mut().enumerate() {
            row[i] += diagonal;
        }
        let transformed = |values: &[f64]| {
            let mut t: Vec<Complex64> = values.iter().map(|&x| x.into()).collect();
            self.fine.forward(&mut t);
            t
        };
        let (total, xc) = (transformed(&fine.total), transformed(&mean));
        let kinetic = core_kinetic.map(|v| transformed(&v));
        let sphere = self.fine.density_sphere(self.spec.density_cutoff_ha());
        let (millers, wavevectors) = (self.fine.millers(), self.fine.wavevectors());
        for &(slot, g2) in sphere.iter().filter(|&&(_, g2)| g2 > 0.0) {
            let q = g2.sqrt();
            let g = wavevectors[slot];
            let mut along = 4.0 * std::f64::consts::PI * volume * total[slot].norm_sqr()
                / (g2 * g2 * (n * n) as f64);
            for (a, atom) in self.atoms.iter().enumerate() {
                let form = self.form(a);
                let structure = c64::from_polar(1.0, phase(millers[slot], atom.position));
                let mut weight = form.local_potential_slope(q) * structure * total[slot].conj()
                    + form.pseudo_core_slope(q) * structure * xc[slot].conj();
                if let Some(v_tau) = &kinetic {
                    weight += core_tau_slope(form, q) * structure * v_tau[slot].conj();
                }
                along -= weight.re / (n as f64 * q);
            }
            for (i, row) in strain.iter_mut().enumerate() {
                for (j, entry) in row.iter_mut().enumerate() {
                    *entry += along * (g[i] * g[j]);
                }
            }
        }
        for (a, atom) in self.atoms.iter().enumerate() {
            let rho = total_occupancies(&density.onsite[a]);
            let hat = atom.sphere.compensation_strain(&rho, &fine.electrostatic);
            for (row, add) in strain.iter_mut().zip(hat) {
                row.iter_mut().zip(add).for_each(|(x, y)| *x += y);
            }
        }
        strain
    }

    fn band_strain(
        &self,
        field: &Field,
        bands: &[Vec<Bands>],
        occupations: &[Vec<Vec<f64>>],
    ) -> [[f64; 3]; 3] {
        let cell = &self.spec.draft().cell;
        let volume = cell.volume();
        let weights = self.kpoints.weights();
        let mut strain = [[0.0; 3]; 3];
        let mut buffers = Vec::new();
        for (spin, (bands, occupations)) in bands.iter().zip(occupations).enumerate() {
            let solved = (self.points.iter().zip(self.kpoints.points()))
                .zip(bands.iter().zip(occupations))
                .zip(weights);
            for (((point, &k), (b, f)), &weight) in solved {
                let x = b.vectors.subcols(0, f.len());
                for (g, m) in point.basis.millers().iter().enumerate() {
                    let q = cell.wavevector(std::array::from_fn(|i| k[i] + f64::from(m[i])));
                    let held: f64 = (f.iter().enumerate())
                        .map(|(band, occupation)| weight * occupation * x[(g, band)].norm_sqr())
                        .sum();
                    for (i, row) in strain.iter_mut().enumerate() {
                        for (j, entry) in row.iter_mut().enumerate() {
                            *entry -= held * (q[i] * q[j]);
                        }
                    }
                }
                if let Some(v_tau) = field.local[spin].v_tau() {
                    let n = v_tau.len();
                    buffers.resize(3 * n, Complex64::default());
                    for (band, &occupation) in f.iter().enumerate().filter(|&(_, &f)| f != 0.0) {
                        let scale = weight * occupation / volume;
                        let c = |g: usize| x[(g, band)];
                        let shares =
                            kinetic_strain(&point.fft, &point.basis, c, scale, v_tau, &mut buffers);
                        for (row, add) in strain.iter_mut().zip(shares) {
                            row.iter_mut()
                                .zip(add)
                                .for_each(|(s, a)| *s -= volume / n as f64 * a);
                        }
                    }
                }
                let projectors = self.projectors(point);
                for (a, (atom, projectors)) in self.atoms.iter().zip(&projectors).enumerate() {
                    let m = self.residual_projections(field, spin, a, projectors, x, &b.values);
                    let strained = (self.kinds[atom.kind].projectors).strained(
                        cell,
                        k,
                        &point.basis,
                        atom.position,
                    );
                    for (row, derivatives) in strain.iter_mut().zip(&strained) {
                        for (entry, derivative) in row.iter_mut().zip(derivatives) {
                            *entry += occupied_sum(weight, f, &(derivative.adjoint() * x), &m);
                        }
                    }
                }
            }
        }
        strain
    }

    fn frame_strain(&self, density: &Density) -> [[f64; 3]; 3] {
        if !self.reads_tau() {
            return [[0.0; 3]; 3];
        }
        let frame = self.spec.draft().cell.frame();
        let parts = per_point(self.atoms.len(), |a| {
            let kind = self.atoms[a].kind;
            let images = self.images.get(kind).map_or(&[][..], Vec::as_slice);
            (self.kinds[kind].species).frame_strain(frame, images, &density.onsite[a])
        });
        parts.iter().fold([[0.0; 3]; 3], |sum, t| {
            std::array::from_fn(|i| std::array::from_fn(|j| sum[i][j] + t[i][j]))
        })
    }

    pub(super) fn discretization(&self) -> Discretization {
        let coarse = self.coarse.millers();
        let plane_waves = fnv(self
            .points
            .iter()
            .flat_map(|p| p.basis.millers().iter().copied()));
        let fine = self.fine.millers();
        let fine_sphere = self.fine.density_sphere(self.spec.density_cutoff_ha());
        let dims = self.coarse.dims();
        let nyquist = (self.sphere.iter()).any(|&(slot, _)| {
            (0..3).any(|k| {
                dims[k].is_multiple_of(2) && coarse[slot][k].unsigned_abs() as usize == dims[k] / 2
            })
        });
        Discretization {
            operations: self.accepted.clone(),
            coarse_grid: dims,
            fine_grid: self.fine.dims(),
            plane_waves,
            density_spheres: [
                fnv(self.sphere.iter().map(|&(slot, _)| coarse[slot])),
                fnv(fine_sphere.iter().map(|&(slot, _)| fine[slot])),
            ],
            nyquist,
        }
    }

    pub(super) fn diagnosis(&self, density: &Density) -> Diagnosis {
        let fine = self.fine(density);
        let grid = grid_points(
            &self.fine,
            self.spec.functional(),
            spins(&fine.smooth),
            Some(&self.core),
            fine.kinetic.as_deref().map(|tau| Kinetic {
                valence: spins(tau),
                core: self.core_kinetic.as_deref(),
            }),
        );
        let cell = &self.spec.draft().cell;
        let spheres = (self.atoms.iter().zip(&density.onsite))
            .map(|(atom, rho)| SphereState {
                species: Arc::clone(&self.kinds[atom.kind].species),
                images: self.images.get(atom.kind).cloned().unwrap_or_default(),
                rho: rho.clone(),
            })
            .collect();
        Diagnosis {
            functional: self.spec.functional(),
            frame: cell.frame(),
            element: cell.volume() / self.fine.points().len() as f64,
            dims: self.fine.dims(),
            spheres,
            grid,
        }
    }

    fn symmetrized_tensor(&self, t: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
        if self.symmetries.is_empty() {
            return t;
        }
        let n = self.symmetries.len() as f64;
        let mut sum = [[0.0; 3]; 3];
        for s in &self.symmetries {
            let w = s.cartesian;
            for (i, row) in sum.iter_mut().enumerate() {
                for (j, entry) in row.iter_mut().enumerate() {
                    *entry += (0..3)
                        .flat_map(|k| (0..3).map(move |l| (k, l)))
                        .map(|(k, l)| w[i][k] * t[k][l] * w[j][l])
                        .sum::<f64>();
                }
            }
        }
        sum.map(|row| row.map(|x| x / n))
    }

    fn symmetrized_forces(&self, forces: Vec<[f64; 3]>) -> Vec<[f64; 3]> {
        if self.symmetries.is_empty() {
            return forces;
        }
        let n = self.symmetries.len() as f64;
        (0..forces.len())
            .map(|a| {
                let mut sum = [0.0; 3];
                for s in &self.symmetries {
                    let f = forces[s.preimages[a]];
                    for (i, row) in s.cartesian.iter().enumerate() {
                        sum[i] += (0..3).map(|j| row[j] * f[j]).sum::<f64>();
                    }
                }
                sum.map(|x| x / n)
            })
            .collect()
    }

    fn lowest<T: Scalar>(
        free_workspaces: &Mutex<Vec<Workspace<T>>>,
        pencil: &Pencil<T>,
        guess: Guess<T>,
        converged: usize,
        tolerance: Tolerance,
    ) -> (Vec<f64>, Mat<T>, bool) {
        let lock_workspace_list = || {
            free_workspaces
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
        };
        let mut work = lock_workspace_list().pop().unwrap_or_default();
        let Tolerance { target, accepted } = tolerance;
        let solved = lowest(pencil, guess, converged, target, ITERATIONS, &mut work);
        lock_workspace_list().push(work);
        let converged = match &solved {
            Ok(_) => true,
            Err(pairs) => pairs.converged_below(converged, accepted),
        };
        let (values, vectors) = solved.unwrap_or_else(|pairs| pairs).into_parts();
        (values, vectors, converged)
    }

    fn solve(
        &self,
        field: &Field,
        spin: usize,
        point: &Point,
        start: Start,
        converged: usize,
        tolerance: Tolerance,
    ) -> Bands {
        let projectors = self.projectors(point);
        let atoms = (self.atoms.iter().zip(&projectors).zip(&field.d))
            .map(|((atom, projectors), d)| Nonlocal {
                projectors: projectors.as_ref(),
                d: d[spin].as_ref(),
                q: self.kinds[atom.kind].overlap.as_ref(),
            })
            .collect::<Vec<_>>();
        let h = Hamiltonian::assembled(&point.basis, &self.coarse, &field.local[spin], &atoms)
            .expect("each k-point's plane waves, potential, and projectors agree");
        let m = match &start {
            Start::Atoms { columns, .. } => *columns,
            Start::Warm(guess) => guess.ncols(),
        };
        let rows = point.basis.millers().len();
        if self.spec.draft().solver == Solver::Dense || rows <= 3 * m {
            let identity = Mat::<c64>::identity(rows, rows);
            let (values, vectors) = dense(
                h.h(identity.as_ref()).as_ref(),
                h.s(identity.as_ref()).as_ref(),
            )
            .expect("S = 1 + Σ P q Pᴴ is positive definite");
            return Bands {
                values: values[..m].to_vec(),
                vectors: vectors.subcols(0, m).to_owned(),
                converged: true,
            };
        }
        let kinetic_ha = point.basis.kinetic_ha();
        if let Some(pairs) = &point.pairs {
            let paired = (h.paired(pairs)).expect("a dataset's projectors are real functions");
            let (apply_h, apply_s) = (
                |x: MatRef<'_, f64>| paired.h(x),
                |x: MatRef<'_, f64>| paired.s(x),
            );
            let pencil = Pencil {
                h: &apply_h,
                s: &apply_s,
                kinetic_ha,
            };
            let guess = match start {
                Start::Warm(guess) => Guess::Vectors(pairs.real(guess.as_ref())),
                Start::Atoms { columns, seed } => {
                    let block = self.orbitals(point);
                    ritz_start_paired(&pencil, block.as_ref(), pairs, columns, seed)
                }
            };
            let (values, vectors, converged) = Self::lowest(
                &self.paired_workspaces,
                &pencil,
                guess,
                converged,
                tolerance,
            );
            return Bands {
                values,
                vectors: pairs.complex(vectors.as_ref()),
                converged,
            };
        }
        let (apply_h, apply_s) = (|x: MatRef<'_, c64>| h.h(x), |x: MatRef<'_, c64>| h.s(x));
        let pencil = Pencil {
            h: &apply_h,
            s: &apply_s,
            kinetic_ha,
        };
        let guess = match start {
            Start::Warm(guess) => Guess::Vectors(guess),
            Start::Atoms { columns, seed } => {
                ritz_start(&pencil, self.orbitals(point).as_ref(), columns, seed)
            }
        };
        let (values, vectors, converged) =
            Self::lowest(&self.workspaces, &pencil, guess, converged, tolerance);
        Bands {
            values,
            vectors,
            converged,
        }
    }

    fn projectors(&self, point: &Point) -> Vec<Mat<c64>> {
        per_point(self.atoms.len(), |a| {
            let atom = &self.atoms[a];
            point.projectors[atom.kind].at(&point.basis, atom.position)
        })
    }

    fn orbitals(&self, point: &Point) -> Mat<c64> {
        let cell = &self.spec.draft().cell.upright();
        let blocks: Vec<Mat<c64>> = (self.atoms.iter())
            .map(|a| (self.kinds[a.kind].orbitals).at(cell, point.k, &point.basis, a.position))
            .collect();
        let columns: Vec<_> = blocks.iter().flat_map(|b| b.col_iter()).collect();
        Mat::from_fn(point.basis.millers().len(), columns.len(), |g, j| {
            columns[j][g]
        })
    }

    pub(super) fn output(
        &self,
        bands: &[Vec<Bands>],
        occupations: &[Vec<Vec<f64>>],
    ) -> (Density, f64) {
        let draft = self.spec.draft();
        let n = self.coarse.points().len();
        let mut kinetic_ha = 0.0;
        let mut slab = vec![Complex64::default(); batch_bands() * n];
        let sizes: Vec<usize> = (self.atoms.iter())
            .map(|a| self.kinds[a.kind].overlap.nrows())
            .collect();
        let (mut smooth, mut kinetic) = (Vec::new(), Vec::new());
        let mut onsite: Vec<Vec<Mat<f64>>> = vec![Vec::new(); self.atoms.len()];
        let slots = || self.sphere.iter().map(|&(slot, _)| slot);
        for (bands, occupations) in bands.iter().zip(occupations) {
            let mut sums = Sums::zero(n, &sizes, self.reads_tau(), kinetic_ha);
            let weights = self.kpoints.weights();
            for (((point, b), f), &weight) in
                self.points.iter().zip(bands).zip(occupations).zip(weights)
            {
                let projectors = self.projectors(point);
                let k = Occupied {
                    fft: &point.fft,
                    basis: &point.basis,
                    vectors: b.vectors.as_ref(),
                    occupations: f,
                    weight,
                    projectors: &projectors,
                };
                sums.add(&k, draft.cell.volume(), &mut slab);
            }
            kinetic_ha = sums.kinetic_ha;
            smooth.push(self.coarse.coefficients(&sums.density, slots()));
            if let Some(tau) = &sums.kinetic {
                kinetic.push(self.coarse.coefficients(tau, slots()));
            }
            for (atom, rho) in onsite.iter_mut().zip(sums.occupancies) {
                atom.push(rho);
            }
        }
        let kinetic = self.reads_tau().then_some(kinetic);
        let density = Density {
            smooth,
            onsite,
            kinetic,
        };
        (self.symmetrized(density), kinetic_ha)
    }

    fn symmetrized(&self, density: Density) -> Density {
        if self.symmetries.len() < 2 {
            return density;
        }
        let n = self.symmetries.len() as f64;
        let scalar = |fields: &[Vec<c64>]| -> Vec<Vec<c64>> {
            (fields.iter())
                .map(|values| {
                    per_point(values.len(), |g| {
                        let sum: c64 = (self.symmetries.iter())
                            .map(|s| {
                                let (source, phase) = s.sources[g];
                                values[source] * phase
                            })
                            .sum();
                        sum / n
                    })
                })
                .collect()
        };
        let onsite = (self.atoms.iter().enumerate())
            .map(|(a, atom)| {
                (0..self.spins)
                    .map(|spin| {
                        let size = self.kinds[atom.kind].overlap.nrows();
                        let mut sum = Mat::<f64>::zeros(size, size);
                        for s in &self.symmetries {
                            let d = &s.orbital_rotations[atom.kind];
                            sum += d * &density.onsite[s.preimages[a]][spin] * d.transpose();
                        }
                        sum / n
                    })
                    .collect()
            })
            .collect();
        Density {
            smooth: scalar(&density.smooth),
            onsite,
            kinetic: density.kinetic.as_deref().map(scalar),
        }
    }

    pub(super) fn change(&self, a: &Density, b: &Density) -> (f64, f64, f64) {
        let volume = self.spec.draft().cell.volume();
        let (a, b) = (a.turned(1.0), b.turned(1.0));
        let squares = |a: &[Vec<c64>], b: &[Vec<c64>]| {
            (a.iter().zip(b))
                .flat_map(|(x, y)| x.iter().zip(y).map(|(x, y)| (x - y).norm_sqr()))
                .sum::<f64>()
        };
        let smooth = squares(&a.smooth, &b.smooth);
        let kinetic = match (&a.kinetic, &b.kinetic) {
            (Some(x), Some(y)) => (volume * squares(x, y)).sqrt(),
            _ => 0.0,
        };
        let onsite = (a.onsite.iter().flatten().zip(b.onsite.iter().flatten()))
            .flat_map(|(x, y)| {
                (0..x.nrows())
                    .flat_map(move |i| (0..x.ncols()).map(move |j| (x[(i, j)] - y[(i, j)]).abs()))
            })
            .fold(0.0, f64::max);
        ((volume * smooth).sqrt(), onsite, kinetic)
    }

    pub(super) fn kinetic_integral_ha(&self, density: &Density) -> Option<f64> {
        let millers = self.coarse.millers();
        let zero = (self.sphere.iter())
            .position(|&(slot, _)| millers[slot] == [0, 0, 0])
            .expect("the density sphere holds G = 0");
        let volume = self.spec.draft().cell.volume();
        (density.kinetic.as_ref()).map(|t| volume * t.iter().map(|spin| spin[zero].re).sum::<f64>())
    }

    pub(super) fn components(&self, density: &Density) -> Vec<(f64, Weight)> {
        let volume = self.spec.draft().cell.volume();
        let density = density.turned(1.0);
        let smooth = (density.smooth.iter().enumerate()).flat_map(|(part, values)| {
            (self.sphere.iter().zip(values)).flat_map(move |(&(_, g2), c)| {
                let weight = if part == 0 {
                    Weight::density(g2, volume)
                } else {
                    Weight::magnetization(volume)
                };
                [(c.re, weight), (c.im, weight)]
            })
        });
        let onsite = (density.onsite.iter().flatten()).flat_map(|rho| {
            (0..rho.nrows())
                .flat_map(move |i| (0..rho.ncols()).map(move |j| (rho[(i, j)], Weight::onsite())))
        });
        let kinetic = (density.kinetic.iter().flatten().flatten())
            .flat_map(|c| [(c.re, Weight::kinetic()), (c.im, Weight::kinetic())]);
        smooth.chain(onsite).chain(kinetic).collect()
    }

    pub(super) fn density(&self, values: &[f64]) -> Density {
        let count = self.sphere.len();
        let mut next = 0;
        let smooth = (0..self.spins)
            .map(|_| {
                let part = (0..count)
                    .map(|g| c64::new(values[next + 2 * g], values[next + 2 * g + 1]))
                    .collect();
                next += 2 * count;
                part
            })
            .collect();
        let onsite = (self.atoms.iter())
            .map(|a| {
                let size = self.kinds[a.kind].overlap.nrows();
                (0..self.spins)
                    .map(|_| {
                        let rho = Mat::from_fn(size, size, |i, j| values[next + i * size + j]);
                        next += size * size;
                        rho
                    })
                    .collect()
            })
            .collect();
        let kinetic = self.reads_tau().then(|| {
            (0..self.spins)
                .map(|_| {
                    let part = (0..count)
                        .map(|g| c64::new(values[next + 2 * g], values[next + 2 * g + 1]))
                        .collect();
                    next += 2 * count;
                    part
                })
                .collect()
        });
        Density {
            smooth,
            onsite,
            kinetic,
        }
        .turned(0.5)
    }
}

fn spins(fields: &[Vec<f64>]) -> Spins<'_> {
    match fields {
        [whole] => Spins::Unpolarized(whole),
        [up, down] => Spins::Polarized([up, down]),
        _ => unreachable!("one or two spins"),
    }
}
