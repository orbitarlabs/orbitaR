use crate::synthetic_draft;
use orbitar::model::{Cell, Functional};
use orbitar::paw::{
    CoreKinetic, Dataset, DatasetDraft, Gaunt, Shape, Species, gauss_legendre, real_ylm,
    total_occupancies,
};
use orbitar::pw::{Grid, Mat};
use orbitar::xc::{DENSITY_FLOOR_PER_BOHR3, Point, evaluate};
use std::f64::consts::{PI, TAU};
use std::sync::Arc;

fn species(change: impl Fn(&mut DatasetDraft)) -> Arc<Species> {
    let mut draft = synthetic_draft();
    change(&mut draft);
    Arc::new(Species::from(Dataset::try_from(draft).unwrap()))
}

fn occupancies(scale: f64) -> Mat<f64> {
    Mat::from_fn(4, 4, |i, j| {
        let (a, b) = (i.min(j) as f64, i.max(j) as f64);
        if i == j {
            0.6 - 0.1 * a
        } else {
            scale * (0.3 + 0.1 * a - 0.05 * b)
        }
    })
}

fn trace_product(a: &Mat<f64>, b: &Mat<f64>) -> f64 {
    (0..a.nrows())
        .flat_map(|i| (0..a.ncols()).map(move |j| (i, j)))
        .map(|(i, j)| a[(i, j)] * b[(i, j)])
        .sum()
}

fn smooth(functional: Functional) -> Arc<Species> {
    species(|d| {
        d.functional = functional;
        let r = d.grid.radii().to_vec();
        for c in &mut d.channels {
            c.ae = r
                .iter()
                .map(|x| x.powi(c.l as i32) * (-4.0 * x).exp())
                .collect();
            c.pseudo = r
                .iter()
                .zip(&c.ae)
                .map(|(x, a)| {
                    if *x < 1.5 {
                        a * (1.0 - 0.5 * (1.0 - x / 1.5).powi(4))
                    } else {
                        *a
                    }
                })
                .collect();
        }
    })
}

#[test]
fn the_requested_functional_not_the_dataset_label_sets_every_one_center_xc_term() {
    let labelled_pbe = Dataset::try_from(synthetic_draft()).unwrap();
    let requested = Species::for_functional(labelled_pbe, Functional::Lda).unwrap();
    let labelled_lda = species(|d| d.functional = Functional::Lda);
    assert_eq!(requested.core_xc(), labelled_lda.core_xc());
    let rho = occupancies(1.0);
    for spins in [vec![rho.clone()], vec![&rho * 0.6, &rho * 0.4]] {
        let (a, b) = (
            requested.one_center(&spins),
            labelled_lda.one_center(&spins),
        );
        assert_eq!(a.energy_ha, b.energy_ha);
        assert_eq!(a.d, b.d);
    }
}

#[test]
fn d_is_the_gradient_of_the_one_center_energy() {
    for (functional, tolerance) in [(Functional::Lda, 1e-9), (Functional::Pbe, 1e-6)] {
        let s = smooth(functional);
        let rho = occupancies(1.0);
        let d = s.one_center(std::slice::from_ref(&rho)).d.remove(0);
        let h = 1e-5;
        for (i, j) in [(0, 0), (0, 2), (1, 1), (1, 3), (2, 3)] {
            let step = |sign: f64| {
                let mut r = rho.clone();
                r[(i, j)] += sign * h;
                if i != j {
                    r[(j, i)] += sign * h;
                }
                s.one_center(&[r]).energy_ha
            };
            let slope = (step(1.0) - step(-1.0)) / (2.0 * h);
            let want = if i == j { d[(i, j)] } else { 2.0 * d[(i, j)] };
            assert!(
                (slope - want).abs() < tolerance * want.abs().max(1.0),
                "{functional:?} ({i}, {j}): {slope} vs {want}"
            );
        }
    }
}

#[test]
fn the_hartree_kernel_is_the_hartree_energy_of_the_moment_densities() {
    let s = species(|_| {});
    let rho = occupancies(1.0);
    let draft = synthetic_draft();
    let grid = &draft.grid;
    let end = grid.nearest(draft.sphere_radius) + 1;
    let r = &grid.radii()[..end];
    let gaunt = Gaunt::up_to(1, 2);
    let orbitals = s.orbitals();
    let moment = |pseudo: bool, lm: usize| -> Vec<f64> {
        (0..end)
            .map(|k| {
                let mut sum = 0.0;
                for (i, a) in orbitals.iter().enumerate() {
                    for (j, b) in orbitals.iter().enumerate() {
                        let (ca, cb) = (&draft.channels[a.channel], &draft.channels[b.channel]);
                        let f = if pseudo {
                            ca.pseudo[k] * cb.pseudo[k]
                        } else {
                            ca.ae[k] * cb.ae[k]
                        };
                        sum += rho[(i, j)] * gaunt.get(lm, a.lm(), b.lm()) * f;
                    }
                }
                sum
            })
            .collect()
    };
    let hat = |lm: usize| -> Vec<f64> {
        let l = (lm as f64).sqrt() as usize;
        let q = trace_product(&rho, s.moment(lm));
        let inside = grid.index_below(draft.shape.radius());
        (0..end)
            .map(|k| {
                if k < inside {
                    q * s.shape(l, r[k])
                } else {
                    0.0
                }
            })
            .collect()
    };
    let mutual = |a: &[f64], b: &[f64], l: usize| {
        let rho_b: Vec<f64> = b.iter().zip(r).map(|(b, r)| b * r * r).collect();
        let rv = grid.poisson(&rho_b, l);
        let f: Vec<f64> = (0..end).map(|k| a[k] * rv[k] * r[k]).collect();
        2.0 * PI * grid.integrate(&f)
    };
    let mut energy = 0.0;
    for lm in 0..9 {
        let l = (lm as f64).sqrt() as usize;
        let (ae, ps, nh) = (moment(false, lm), moment(true, lm), hat(lm));
        energy += mutual(&ae, &ae, l)
            - mutual(&ps, &ps, l)
            - 2.0 * mutual(&ps, &nh, l)
            - mutual(&nh, &nh, l);
    }
    let kernel = 0.5 * trace_product(&rho, &s.hartree(&rho));
    assert!(
        (kernel - energy).abs() < 1e-8 * energy.abs(),
        "{kernel} vs {energy}"
    );
}

fn sphere_grid() -> Grid {
    let cell = Cell::try_from([[0.0, 3.5, 3.5], [3.5, 0.0, 3.5], [3.5, 3.5, 0.0]]).unwrap();
    Grid::coarse(&cell, 30.0)
}

#[test]
fn the_compensation_density_and_d_hat_are_adjoint() {
    let s = species(|_| {});
    let grid = sphere_grid();
    let sphere = s.sphere(&grid, [0.1, 0.2, 0.3]);
    let rho = occupancies(1.0);
    let v: Vec<f64> = grid
        .points()
        .iter()
        .map(|f| 0.3 + 0.2 * (TAU * (f[0] + f[1])).cos())
        .collect();
    let mut nhat = vec![0.0; v.len()];
    sphere.add_compensation(&rho, &mut nhat);
    let volume = grid.cell().volume() / v.len() as f64;
    let grid_sum: f64 = v.iter().zip(&nhat).map(|(v, n)| v * n * volume).sum();
    let projected = trace_product(&rho, &sphere.potential_d(&v));
    assert!(
        (grid_sum - projected).abs() < 1e-12 * grid_sum.abs(),
        "{grid_sum} vs {projected}"
    );
}

#[test]
fn a_potential_near_the_largest_float_keeps_d_hat_finite_and_linear() {
    let s = species(|_| {});
    let grid = sphere_grid();
    let sphere = s.sphere(&grid, [0.1, 0.2, 0.3]);
    let points = grid.points().len();
    let (two, huge) = (
        sphere.potential_d(&vec![2.0; points]),
        sphere.potential_d(&vec![1e306; points]),
    );
    let scale = 0.5e306 * two[(0, 0)].abs();
    for i in 0..4 {
        for j in 0..4 {
            let want = 0.5e306 * two[(i, j)];
            assert!(huge[(i, j)].is_finite(), "({i}, {j}): {}", huge[(i, j)]);
            assert!(
                (huge[(i, j)] - want).abs() <= 1e-12 * scale,
                "({i}, {j}): {} against {want}",
                huge[(i, j)]
            );
        }
    }
}

#[test]
fn a_constant_potential_gives_d_hat_of_the_overlap_and_the_charge_of_the_moments() {
    let s = species(|_| {});
    let grid = sphere_grid();
    let sphere = s.sphere(&grid, [0.1, 0.2, 0.3]);
    let points = grid.points().len();
    let d_hat = sphere.potential_d(&vec![2.0; points]);
    let overlap = s.overlap();
    for i in 0..4 {
        for j in 0..4 {
            assert!(
                (d_hat[(i, j)] - 2.0 * overlap[(i, j)]).abs() < 2e-3 * overlap[(0, 0)].abs(),
                "({i}, {j})"
            );
        }
    }
    let rho = occupancies(1.0);
    let mut nhat = vec![0.0; points];
    sphere.add_compensation(&rho, &mut nhat);
    let charge: f64 = nhat.iter().sum::<f64>() * grid.cell().volume() / points as f64;
    let want = trace_product(&rho, &overlap);
    assert!(
        (charge - want).abs() < 1e-3 * want.abs(),
        "{charge} vs {want}"
    );
}

#[test]
fn a_grid_point_within_abinits_margin_past_r_shape_joins_the_sphere() {
    let (s, grid) = (species(|_| {}), sphere_grid());
    let origin_density = |d: f64| {
        let sphere = s.sphere(&grid, [-d / 7.0, d / 7.0, d / 7.0]);
        let mut nhat = vec![0.0; grid.points().len()];
        sphere.add_compensation(&occupancies(1.0), &mut nhat);
        nhat[0]
    };
    assert_ne!(origin_density(1.2 * (1.0 + 2e-8)), 0.0);
    assert_eq!(origin_density(1.2 * (1.0 + 1e-7)), 0.0);
}

#[test]
fn an_s_only_dataset_puts_the_charge_of_its_moments_in_l_0() {
    let s = species(|d| {
        d.channels.truncate(1);
        d.kinetic_differences = vec![1.0];
        (d.z, d.valence_electrons) = (4.0, 2.0);
    });
    let grid = sphere_grid();
    let sphere = s.sphere(&grid, [0.1, 0.2, 0.3]);
    let rho = Mat::from_fn(1, 1, |_, _| 0.6);
    let mut nhat = vec![0.0; grid.points().len()];
    sphere.add_compensation(&rho, &mut nhat);
    let charge: f64 = nhat.iter().sum::<f64>() * grid.cell().volume() / nhat.len() as f64;
    let want = 0.6 * s.overlap()[(0, 0)];
    assert!(
        (charge - want).abs() < 1e-3 * want.abs(),
        "{charge} vs {want}"
    );
}

fn polarized() -> [Mat<f64>; 2] {
    let rho = occupancies(1.0);
    [
        Mat::from_fn(4, 4, |i, j| 0.7 * rho[(i, j)]),
        Mat::from_fn(4, 4, |i, j| {
            0.3 * rho[(i, j)] + if i == j { 0.02 } else { 0.0 }
        }),
    ]
}

fn largest_difference(a: &Mat<f64>, b: &Mat<f64>) -> f64 {
    (0..a.nrows())
        .flat_map(|i| (0..a.ncols()).map(move |j| (i, j)))
        .map(|(i, j)| (a[(i, j)] - b[(i, j)]).abs())
        .fold(0.0, f64::max)
}

#[test]
fn equal_spin_channels_give_the_unpolarized_terms() {
    for functional in [Functional::Lda, Functional::Pbe] {
        let s = species(|d| d.functional = functional);
        let rho = occupancies(1.0);
        let half = Mat::from_fn(4, 4, |i, j| 0.5 * rho[(i, j)]);
        let one = s.one_center(std::slice::from_ref(&rho));
        let two = s.one_center(&[half.clone(), half]);
        assert!(
            (two.energy_ha - one.energy_ha).abs() < 1e-10,
            "{functional:?}"
        );
        let bound = match functional {
            Functional::Lda => 1e-10,
            _ => 1e-3,
        };
        for d in &two.d {
            assert!(largest_difference(d, &one.d[0]) < bound, "{functional:?}");
        }
    }
}

fn faint(functional: Functional, level: f64, tilt: f64) -> Arc<Species> {
    species(|d| {
        d.functional = functional;
        let r = d.grid.radii().to_vec();
        d.core_density = (r.iter())
            .map(|x| {
                if *x < 1.8 {
                    level * (1.0 + tilt * x)
                } else {
                    0.0
                }
            })
            .collect();
        d.core_electrons = 0.0;
        d.z = d.valence_electrons;
        d.pseudo_core_density = vec![0.0; r.len()];
        let c = 0.3 * (3.0 * PI * PI).powf(2.0 / 3.0);
        let tau = d
            .core_density
            .iter()
            .map(|n| c * n.powf(5.0 / 3.0))
            .collect();
        d.core_kinetic = Some(CoreKinetic::checked(&d.grid, tau, vec![0.0; r.len()]).unwrap());
    })
}

#[test]
fn a_slope_below_abinits_flat_gradient_keeps_no_gradient_term_of_v() {
    let empty = Mat::from_fn(4, 4, |_, _| 0.0);
    for (functional, level, tilt, spins, bound) in [
        (Functional::Pbe, 1e-13, 1e-5, 1, 1e-8),
        (Functional::Pbe, 1e-13, 1e-5, 2, 1e-8),
        (Functional::R2scan, 1e-13, 1e-5, 1, 1e-8),
        (Functional::R2scan, 1e-13, 1e-5, 2, 1e-8),
        (Functional::R2scan, 3e-11, 2e-15 / 3e-11, 2, 1e-7),
    ] {
        let (tilted, flat) = (
            faint(functional, level, tilt),
            faint(functional, level, 0.0),
        );
        let rho = vec![empty.clone(); spins];
        let (a, b) = (tilted.one_center(&rho), flat.one_center(&rho));
        for (x, y) in a.d.iter().zip(&b.d) {
            let gap = largest_difference(x, y);
            assert!(
                gap < bound,
                "{functional:?} at {level:e}, {spins} spins: {gap:e}"
            );
        }
    }
}

fn spike(far: f64) -> Arc<Species> {
    species(|d| {
        let size = d.grid.radii().len();
        d.core_density = vec![0.0; size];
        d.core_density[688] = 4e-9;
        d.core_density[690] = far;
        d.core_electrons = 0.0;
        d.z = d.valence_electrons;
        d.pseudo_core_density = vec![0.0; size];
    })
}

fn spike_slope_squared(far: f64) -> f64 {
    let draft = synthetic_draft();
    let y = (4.0 * PI).sqrt();
    let mut core = vec![0.0; draft.grid.radii().len()];
    core[688] = 4e-9;
    core[690] = far;
    let up: Vec<f64> = core
        .iter()
        .map(|n| (y * n / (2.0 * y)).max(DENSITY_FLOOR_PER_BOHR3))
        .collect();
    draft.grid.derivative(&up)[688].powi(2)
}

#[test]
fn a_squared_slope_at_abinits_flat_gradient_keeps_the_gradient_term_of_v() {
    let start = 2.5343032635005216e-13_f64;
    let at = (0..64)
        .flat_map(|k| [k, -k])
        .map(|k: i32| {
            let mut far = start;
            for _ in 0..k.unsigned_abs() {
                far = if k > 0 {
                    far.next_up()
                } else {
                    far.next_down()
                };
            }
            far
        })
        .find(|&far| spike_slope_squared(far) == 1e-24)
        .expect("a far sample whose squared slope is 1e-24");
    let below = at.next_down();
    assert!(spike_slope_squared(below) < 1e-24);
    let empty = [Mat::from_fn(4, 4, |_, _| 0.0)];
    let d = |far: f64| spike(far).one_center(&empty).d.remove(0);
    let (kept, dropped, further) = (d(at), d(below), d(below.next_down()));
    let jump = largest_difference(&kept, &dropped);
    assert!(jump > 1e-9, "{jump:e}");
    assert!(largest_difference(&dropped, &further) < 1e-12);
}

#[test]
fn swapping_the_spins_of_an_lda_atom_swaps_its_d() {
    let s = species(|d| d.functional = Functional::Lda);
    let [up, down] = polarized();
    let (a, b) = (
        s.one_center(&[up.clone(), down.clone()]),
        s.one_center(&[down, up]),
    );
    assert!((a.energy_ha - b.energy_ha).abs() < 1e-11);
    assert!(largest_difference(&a.d[0], &b.d[1]) < 1e-11);
    assert!(largest_difference(&a.d[0], &a.d[1]) > 1e-3);
}

#[test]
fn the_d_of_each_spin_is_the_gradient_of_the_one_center_energy() {
    for (functional, tolerance) in [(Functional::Lda, 1e-9), (Functional::Pbe, 1e-6)] {
        let s = smooth(functional);
        let rho = polarized();
        let d = s.one_center(&rho).d;
        let h = 1e-5;
        for (spin, (i, j)) in [
            (0, (0, 0)),
            (1, (0, 2)),
            (0, (1, 3)),
            (1, (2, 3)),
            (1, (1, 1)),
        ] {
            let step = |sign: f64| {
                let mut r = rho.clone();
                r[spin][(i, j)] += sign * h;
                if i != j {
                    r[spin][(j, i)] += sign * h;
                }
                s.one_center(&r).energy_ha
            };
            let slope = (step(1.0) - step(-1.0)) / (2.0 * h);
            let want = if i == j {
                d[spin][(i, j)]
            } else {
                2.0 * d[spin][(i, j)]
            };
            let bound = if i == j {
                tolerance * want.abs().max(1.0)
            } else {
                1e-9
            };
            assert!(
                (slope - want).abs() < bound,
                "{functional:?} {spin} ({i}, {j}): {slope} vs {want}"
            );
        }
    }
}

fn angular_xc(species: &Species, rho: &[Mat<f64>], pseudo: bool) -> f64 {
    let d = species.dataset().draft();
    let r = d.grid.radii();
    let end = d.grid.nearest(d.sphere_radius) + 1;
    let core = if pseudo {
        &d.pseudo_core_density
    } else {
        &d.core_density
    };
    let orbitals = species.orbitals();
    let wave = |i: usize| {
        let channel = &d.channels[orbitals[i].channel];
        if pseudo { &channel.pseudo } else { &channel.ae }
    };
    let lmax = orbitals.iter().map(|o| o.l).max().unwrap();
    let azimuths = 48;
    let mut integrand = vec![0.0; end];
    for (z, weight) in gauss_legendre(24) {
        for p in 0..azimuths {
            let phi = TAU * p as f64 / azimuths as f64;
            let s = (1.0 - z * z).sqrt();
            let y = real_ylm(lmax, [s * phi.cos(), s * phi.sin(), z]);
            for k in 1..end {
                let spin = |m: &Mat<f64>| -> f64 {
                    let mut n = core[k] / 2.0;
                    for (i, a) in orbitals.iter().enumerate() {
                        for (j, b) in orbitals.iter().enumerate() {
                            n += m[(i, j)] * wave(i)[k] * wave(j)[k] * y[a.lm()] * y[b.lm()];
                        }
                    }
                    n
                };
                let (up, down) = match rho {
                    [whole] => {
                        let half = whole * 0.5;
                        (spin(&half), spin(&half))
                    }
                    [up, down] => (spin(up), spin(down)),
                    _ => unreachable!("one or two spins"),
                };
                let [x, c] = evaluate(Functional::Lda, Point::of(up, down, [0.0; 3]));
                integrand[k] +=
                    weight * TAU / azimuths as f64 * (x.energy + c.energy) * r[k] * r[k];
            }
        }
    }
    d.grid.integrate(&integrand)
}

#[test]
fn the_expanded_xc_meets_its_angular_integral_to_third_order_in_the_moments() {
    let lda = smooth(Functional::Lda);
    let free = lda.initial_occupancies();
    let coupling = occupancies(1.0) - occupancies(0.0);
    let expanded = |rho: &[Mat<f64>]| {
        let total = total_occupancies(rho);
        let quadratic = trace_product(&total, lda.frozen_d())
            + 0.5 * trace_product(&total, &lda.hartree(&total))
            - lda.core_xc();
        lda.one_center(rho).energy_ha - quadratic
    };
    for share in [1.0, 0.7] {
        let errors: Vec<f64> = [0.8, 0.4, 0.2]
            .iter()
            .map(|&scale| {
                let rho = &free + &coupling * scale;
                let spins = if share == 1.0 {
                    vec![rho]
                } else {
                    vec![&rho * share, &rho * (1.0 - share)]
                };
                (expanded(&spins) - angular_xc(&lda, &spins, false)
                    + angular_xc(&lda, &spins, true))
                .abs()
            })
            .collect();
        assert!(
            errors[0] > 6.0 * errors[1] && errors[1] > 6.0 * errors[2],
            "{errors:?}"
        );
    }
}

#[test]
fn the_compensation_gradient_is_the_derivative_of_its_grid_sum() {
    let grid = sphere_grid();
    let v: Vec<f64> = (grid.points().iter())
        .map(|f| 0.3 + 0.2 * (TAU * (f[0] + 2.0 * f[1])).cos() - 0.1 * (TAU * f[2]).sin())
        .collect();
    let volume = grid.cell().volume() / v.len() as f64;
    let rho = occupancies(1.0);
    let sinc = |d: &mut DatasetDraft| d.shape = Shape::Sinc { radius: 1.2 };
    let shapes = [(species(|_| {}), 1e-11), (species(sinc), 1e-8)];
    for ((s, bound), at) in shapes
        .iter()
        .flat_map(|s| [[0.1, 0.2, 0.3], [0.0; 3]].map(|at| (s, at)))
    {
        let sum = |position: [f64; 3]| {
            let mut nhat = vec![0.0; v.len()];
            s.sphere(&grid, position).add_compensation(&rho, &mut nhat);
            v.iter()
                .zip(&nhat)
                .map(|(v, n)| v * n * volume)
                .sum::<f64>()
        };
        let gradient = s.sphere(&grid, at).compensation_gradient(&rho, &v);
        let h = 1e-4;
        for axis in 0..3 {
            let b = grid.cell().reciprocal();
            let step =
                |t: f64| -> [f64; 3] { std::array::from_fn(|i| at[i] + t * h * b[i][axis] / TAU) };
            let fd = (8.0 * (sum(step(1.0)) - sum(step(-1.0)))
                - (sum(step(2.0)) - sum(step(-2.0))))
                / (12.0 * h);
            assert!(gradient[axis].abs() > 1e-4, "{gradient:?}");
            assert!(
                (gradient[axis] - fd).abs() < *bound,
                "{axis}: {} vs {fd}",
                gradient[axis]
            );
        }
    }
}

#[test]
fn the_compensation_strain_is_the_derivative_of_its_grid_sum() {
    let rows = [[0.0, 3.5, 3.5], [3.5, 0.0, 3.5], [3.5, 3.5, 0.0]];
    let grid = sphere_grid();
    let v: Vec<f64> = (grid.points().iter())
        .map(|f| 0.3 + 0.2 * (TAU * (f[0] + 2.0 * f[1])).cos() - 0.1 * (TAU * f[2]).sin())
        .collect();
    let volume = grid.cell().volume() / v.len() as f64;
    let rho = occupancies(1.0);
    let at = [0.1, 0.2, 0.3];
    let sinc = |d: &mut DatasetDraft| d.shape = Shape::Sinc { radius: 1.2 };
    for (s, bound) in [(species(|_| {}), 1e-10), (species(sinc), 1e-8)] {
        let strain = s.sphere(&grid, at).compensation_strain(&rho, &v);
        let h = 1e-4;
        for i in [0, 1, 2] {
            for j in 0..3 {
                let sum = |t: f64| {
                    let moved: [[f64; 3]; 3] = std::array::from_fn(|r| {
                        std::array::from_fn(|c| {
                            rows[r][c] + if c == i { t * h * rows[r][j] } else { 0.0 }
                        })
                    });
                    let on = Grid::coarse(&Cell::try_from(moved).unwrap(), 30.0);
                    assert_eq!(on.dims(), grid.dims());
                    let mut nhat = vec![0.0; v.len()];
                    s.sphere(&on, at).add_compensation(&rho, &mut nhat);
                    v.iter()
                        .zip(&nhat)
                        .map(|(v, n)| v * n * volume)
                        .sum::<f64>()
                };
                let fd = (8.0 * (sum(1.0) - sum(-1.0)) - (sum(2.0) - sum(-2.0))) / (12.0 * h);
                assert!(
                    (strain[i][j] - fd).abs() < bound,
                    "({i}, {j}): {} vs {fd}",
                    strain[i][j]
                );
            }
        }
        assert!(
            strain.iter().flatten().any(|x| x.abs() > 1e-2),
            "{strain:?}"
        );
    }
}
