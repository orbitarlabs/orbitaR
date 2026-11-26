use orbitar::paw::{AngularMesh, Gaunt};
use std::f64::consts::PI;

fn on_sphere(mesh: &AngularMesh, f: impl Fn(usize) -> f64) -> f64 {
    4.0 * PI
        * (mesh.weights().iter().enumerate())
            .map(|(p, w)| w * f(p))
            .sum::<f64>()
}

fn l_of(lm: usize) -> usize {
    (lm as f64).sqrt() as usize
}

#[test]
fn a_product_mesh_takes_nonzero_counts_and_its_weights_sum_to_one() {
    let count = |n| NonZeroUsize::new(n).unwrap();
    for (ntheta, nphi) in [(1, 1), (2, 3), (12, 13)] {
        let mesh = AngularMesh::product(count(ntheta), count(nphi));
        assert_eq!(mesh.points().len(), ntheta * nphi);
        assert!((mesh.weights().iter().sum::<f64>() - 1.0).abs() < 1e-14);
    }
    assert_eq!(
        AngularMesh::product(count(12), count(13)),
        AngularMesh::abinit()
    );
}

#[test]
fn abinits_mesh_has_156_unit_points_from_phi_0_whose_weights_sum_to_1() {
    let mesh = AngularMesh::abinit();
    assert_eq!(mesh.points().len(), 156);
    let sum: f64 = mesh.weights().iter().sum();
    assert!((sum - 1.0).abs() < 1e-14, "{:e}", sum - 1.0);
    for p in mesh.points() {
        assert!((p.iter().map(|x| x * x).sum::<f64>() - 1.0).abs() < 1e-15);
    }
    assert_eq!(mesh.points()[0][1], 0.0);
    assert!(mesh.points()[0][0] > 0.0);
}

#[test]
fn the_harmonics_are_orthonormal_on_the_mesh_to_degree_5() {
    let mesh = AngularMesh::abinit();
    let y = mesh.harmonics(5);
    for a in 0..36 {
        for b in 0..36 {
            let want = if a == b { 1.0 } else { 0.0 };
            let got = on_sphere(&mesh, |p| y[p][a] * y[p][b]);
            assert!((got - want).abs() < 1e-13, "{a} {b} {got}");
        }
    }
}

#[test]
fn surface_gradients_have_norm_l_l_plus_1_and_are_orthogonal() {
    let mesh = AngularMesh::abinit();
    let g = mesh.gradients(4);
    let dot = |p: usize, a: usize, b: usize| (0..3).map(|i| g[p][a][i] * g[p][b][i]).sum::<f64>();
    for a in 0..25 {
        for b in 0..25 {
            let l = l_of(a) as f64;
            let want = if a == b { l * (l + 1.0) } else { 0.0 };
            let got = on_sphere(&mesh, |p| dot(p, a, b));
            assert!((got - want).abs() < 1e-12, "{a} {b} {got}");
        }
    }
}

#[test]
fn the_gradient_gaunt_identity_holds_on_the_mesh() {
    let mesh = AngularMesh::abinit();
    let (y, g) = (mesh.harmonics(4), mesh.gradients(2));
    let gaunt = Gaunt::up_to(2, 4);
    let c = |l: usize| (l * (l + 1)) as f64;
    let triples =
        (0..25).flat_map(|big| (0..9).flat_map(move |a| (0..9).map(move |b| (big, a, b))));
    for (big, a, b) in triples {
        let got = on_sphere(&mesh, |p| {
            y[p][big] * (0..3).map(|i| g[p][a][i] * g[p][b][i]).sum::<f64>()
        });
        let want = (c(l_of(a)) + c(l_of(b)) - c(l_of(big))) / 2.0 * gaunt.get(big, a, b);
        assert!((got - want).abs() < 1e-13, "{big} {a} {b} {got} {want}");
    }
}

use crate::synthetic_draft;
use orbitar::model::{Cell, Functional, operations};
use orbitar::paw::{CoreKinetic, Dataset, Species};
use orbitar::pw::Mat;
use orbitar::xc::unpolarized;
use std::num::NonZeroUsize;

fn smooth_species(functional: Functional) -> Species {
    smooth_species_with(functional, |_| {})
}

fn smooth_species_with(
    functional: Functional,
    change: impl Fn(&mut orbitar::paw::DatasetDraft),
) -> Species {
    let mut d = synthetic_draft();
    let r = d.grid.radii().to_vec();
    for c in &mut d.channels {
        c.ae = r
            .iter()
            .map(|x| x.powi(c.l as i32) * (-4.0 * x).exp())
            .collect();
        c.pseudo = (r.iter().zip(&c.ae))
            .map(|(x, a)| {
                if *x < 1.5 {
                    a * (1.0 - 0.5 * (1.0 - x / 1.5).powi(4))
                } else {
                    *a
                }
            })
            .collect();
    }
    let thomas_fermi = |n: &[f64]| -> Vec<f64> {
        let c = 0.3 * (3.0 * PI * PI).powf(2.0 / 3.0);
        n.iter().map(|x| c * x.max(0.0).powf(5.0 / 3.0)).collect()
    };
    let kinetic = CoreKinetic::checked(
        &d.grid,
        thomas_fermi(&d.core_density),
        thomas_fermi(&d.pseudo_core_density),
    );
    d.core_kinetic = Some(kinetic.unwrap());
    change(&mut d);
    Species::for_functional(Dataset::try_from(d).unwrap(), functional).unwrap()
}

fn nonspherical() -> Mat<f64> {
    Mat::from_fn(4, 4, |i, j| {
        let (a, b) = (i.min(j) as f64, i.max(j) as f64);
        if i == j {
            0.6 - 0.1 * a
        } else {
            0.3 + 0.1 * a - 0.05 * b
        }
    })
}

fn turn(axis: [f64; 3], angle: f64) -> [[f64; 3]; 3] {
    let n = axis.iter().map(|x| x * x).sum::<f64>().sqrt();
    let [x, y, z] = axis.map(|a| a / n);
    let (c, s) = (angle.cos(), angle.sin());
    let t = 1.0 - c;
    [
        [c + x * x * t, x * y * t - z * s, x * z * t + y * s],
        [y * x * t + z * s, c + y * y * t, y * z * t - x * s],
        [z * x * t - y * s, z * y * t + x * s, c + z * z * t],
    ]
}

#[test]
fn a_spherical_occupancy_gives_the_moment_expansions_terms_on_the_mesh() {
    let pbe = smooth_species(Functional::Pbe);
    let spherical = Mat::from_fn(
        4,
        4,
        |i, j| if i == j { [0.8, 0.3, 0.3, 0.3][i] } else { 0.0 },
    );
    for spins in [
        vec![spherical.clone()],
        vec![&spherical * 0.6, &spherical * 0.4],
    ] {
        let (a, b) = (pbe.one_center(&spins), pbe.one_center_on_mesh(&spins));
        assert!(
            (a.energy_ha - b.energy_ha).abs() < 1e-12,
            "{} {}",
            a.energy_ha,
            b.energy_ha
        );
        for (x, y) in a.d.iter().zip(&b.d) {
            assert!((x - y).norm_max() < 1e-12, "{:e}", (x - y).norm_max());
        }
        for part in 0..2 {
            let (x, y) = (a.xc_ha[part], b.xc_ha[part]);
            assert!((x - y).abs() < 1e-12, "part {part}: {x} vs {y}");
        }
    }
}

#[test]
fn the_energy_holds_the_two_sphere_xc_energies_with_hartree_frozen_and_core_terms() {
    let rho = nonspherical();
    let dot = |a: &Mat<f64>, b: &Mat<f64>| -> f64 {
        (0..a.nrows())
            .flat_map(|i| (0..a.ncols()).map(move |j| a[(i, j)] * b[(i, j)]))
            .sum()
    };
    for (s, mesh) in [
        (smooth_species(Functional::Pbe), false),
        (smooth_species(Functional::R2scan), true),
    ] {
        let spins = std::slice::from_ref(&rho);
        let one = if mesh {
            s.one_center_on_mesh(spins)
        } else {
            s.one_center(spins)
        };
        let want = one.xc_ha[0] - one.xc_ha[1] - s.core_xc()
            + 0.5 * dot(&rho, &s.hartree(&rho))
            + dot(&rho, s.frozen_d());
        assert!(
            one.xc_ha[0] != 0.0 && one.xc_ha[1] != 0.0,
            "{:?}",
            one.xc_ha
        );
        assert!(
            (one.energy_ha - want).abs() < 1e-12,
            "mesh {mesh}: {} vs {want}",
            one.energy_ha
        );
    }
}

#[test]
fn a_turned_occupancy_keeps_the_mesh_energy_and_turns_d() {
    for functional in [Functional::Pbe, Functional::R2scan] {
        let s = smooth_species(functional);
        let rho = nonspherical();
        let base = s.one_center_on_mesh(std::slice::from_ref(&rho));
        let turns = [
            ([0.0, 0.0, 1.0], PI / 2.0),
            ([1.0, 1.0, 1.0], 2.0 * PI / 3.0),
            ([0.3, -0.7, 0.5], 1.234),
        ];
        for (axis, angle) in turns {
            let rot = s.rotation(turn(axis, angle));
            let turned = &rot * &rho * rot.transpose();
            let got = s.one_center_on_mesh(std::slice::from_ref(&turned));
            let bound = 1e-10 + 1e-12 * base.energy_ha.abs();
            assert!(
                (got.energy_ha - base.energy_ha).abs() <= bound,
                "{functional:?} {axis:?}"
            );
            let want = &rot * &base.d[0] * rot.transpose();
            let bound = 1e-10 + 1e-12 * base.d[0].norm_max();
            assert!(
                (&got.d[0] - &want).norm_max() <= bound,
                "{functional:?} {axis:?}"
            );
        }
    }
}

fn off_the_clamp() -> Species {
    smooth_species_with(Functional::R2scan, |d| {
        let r = d.grid.radii();
        let c = 0.3 * (3.0 * PI * PI).powf(2.0 / 3.0);
        let above = |n: &[f64], alpha: f64| -> Vec<f64> {
            (n.iter().zip(r))
                .map(|(n, x)| c * n.max(0.0).powf(5.0 / 3.0) + alpha * alpha * x * x * n)
                .collect()
        };
        let (ae, pseudo) = (
            above(&d.core_density, 4.0),
            above(&d.pseudo_core_density, 1.0),
        );
        d.core_kinetic = Some(CoreKinetic::checked(&d.grid, ae, pseudo).unwrap());
    })
}

fn positive() -> Mat<f64> {
    nonspherical() + Mat::<f64>::identity(4, 4) * 0.05
}

#[test]
fn d_on_the_mesh_lies_near_the_gradient_of_the_mesh_energy_on_these_spheres() {
    let s = off_the_clamp();
    let rho = positive();
    let d = s.one_center_on_mesh(std::slice::from_ref(&rho)).d.remove(0);
    let h = 1e-5;
    for (i, j) in [(0, 0), (0, 2), (1, 1), (1, 3), (2, 3)] {
        let step = |sign: f64| {
            let mut r = rho.clone();
            r[(i, j)] += sign * h;
            if i != j {
                r[(j, i)] += sign * h;
            }
            s.one_center_on_mesh(&[r]).energy_ha
        };
        let slope = (step(1.0) - step(-1.0)) / (2.0 * h);
        let want = if i == j { d[(i, j)] } else { 2.0 * d[(i, j)] };
        assert!(
            (slope - want).abs() < 1e-7 * want.abs().max(1.0),
            "({i}, {j}): {slope} vs {want}"
        );
    }
}

#[test]
fn each_spins_d_on_the_mesh_lies_near_the_gradient_of_the_mesh_energy_on_these_spheres() {
    let s = off_the_clamp();
    let spins = [&positive() * 0.65, &positive() * 0.35];
    let d = s.one_center_on_mesh(&spins).d;
    let h = 1e-5;
    for spin in 0..2 {
        for (i, j) in [(0, 0), (0, 2), (1, 3), (3, 3)] {
            let step = |sign: f64| {
                let mut r = spins.clone();
                r[spin][(i, j)] += sign * h;
                if i != j {
                    r[spin][(j, i)] += sign * h;
                }
                s.one_center_on_mesh(&r).energy_ha
            };
            let slope = (step(1.0) - step(-1.0)) / (2.0 * h);
            let want = if i == j {
                d[spin][(i, j)]
            } else {
                2.0 * d[spin][(i, j)]
            };
            assert!(
                (slope - want).abs() < 1e-7 * want.abs().max(1.0),
                "spin {spin} ({i}, {j}): {slope} vs {want}"
            );
        }
    }
}

#[test]
fn equal_spins_on_the_mesh_give_the_unpolarized_energy_and_d() {
    let s = smooth_species(Functional::R2scan);
    let rho = nonspherical();
    let one = s.one_center_on_mesh(std::slice::from_ref(&rho));
    let two = s.one_center_on_mesh(&[&rho * 0.5, &rho * 0.5]);
    assert!(
        (one.energy_ha - two.energy_ha).abs() < 1e-12,
        "{} vs {}",
        one.energy_ha,
        two.energy_ha
    );
    for d in &two.d {
        let gap = (d - &one.d[0]).norm_max();
        assert!(gap < 1e-12, "{gap:e}");
    }
}

#[test]
fn a_spherical_r2scan_occupancy_gives_each_spheres_radial_integral_of_the_functional() {
    let s = smooth_species(Functional::R2scan);
    let rho = Mat::from_fn(
        4,
        4,
        |i, j| if i == j { [0.8, 0.3, 0.3, 0.3][i] } else { 0.0 },
    );
    let one = s.one_center_on_mesh(std::slice::from_ref(&rho));
    let d = s.dataset().draft();
    let r = d.grid.radii();
    let end = d.grid.nearest(d.sphere_radius) + 1;
    let kinetic = d.core_kinetic.as_ref().unwrap();
    let wave = |l: usize, x: f64, pseudo: bool| -> (f64, f64, f64) {
        let e = (-4.0 * x).exp();
        let (a, da, over) = match l {
            0 => (e, -4.0 * e, 0.0),
            _ => (x * e, (1.0 - 4.0 * x) * e, e),
        };
        if pseudo && x < 1.5 {
            let t = 1.0 - x / 1.5;
            let (f, df) = (1.0 - 0.5 * t.powi(4), (4.0 / 3.0) * t.powi(3));
            (a * f, da * f + a * df, over * f)
        } else {
            (a, da, over)
        }
    };
    let core = |x: f64, pseudo: bool| -> (f64, f64) {
        let (scale, a) = if pseudo { (0.1, 1.0) } else { (1.0, 4.0) };
        let n = scale * 2.0 * (4.0 / PI).powf(1.5) * (-a * x * x).exp();
        (n, -2.0 * a * x * n)
    };
    for (part, pseudo) in [(0, false), (1, true)] {
        let f: Vec<f64> = (0..end)
            .map(|k| {
                let x = r[k];
                let ((rs, ds, _), (rp, dp, op)) = (wave(0, x, pseudo), wave(1, x, pseudo));
                let (nc, dnc) = core(x, pseudo);
                let tau_c = if pseudo {
                    kinetic.pseudo()[k]
                } else {
                    kinetic.ae()[k]
                };
                let four_pi = 4.0 * PI;
                let n = (0.8 * rs * rs + 0.9 * rp * rp) / four_pi + nc;
                let slope = (1.6 * rs * ds + 1.8 * rp * dp) / four_pi + dnc;
                let tau =
                    0.5 * (0.8 * ds * ds + 0.3 * (3.0 * dp * dp + 6.0 * op * op)) / four_pi + tau_c;
                let (up, tau_up) = ((n / 2.0).max(1e-14), (tau / 2.0).max(1e-14));
                let [x_part, c_part] = unpolarized(
                    Functional::R2scan,
                    2.0 * up,
                    slope * slope,
                    Some(2.0 * tau_up),
                );
                four_pi * x * x * n * (x_part.epsilon + c_part.epsilon)
            })
            .collect();
        let want = d.grid.integrate(&f);
        let got = one.xc_ha[part];
        assert!(
            (got - want).abs() < 1e-9 * want.abs(),
            "part {part}: {got} vs {want}"
        );
    }
}

#[test]
fn on_a_strained_cell_both_on_site_symmetry_paths_agree_through_the_operations_isometries() {
    let cell = Cell::try_from([
        [6.0, 0.0, 0.0],
        [0.0, 6.0, 0.0],
        [0.0, 0.0, 6.0 * (1.0 + 4e-7)],
    ])
    .unwrap();
    let ops = operations(&cell, &[(6, [0.0; 3])], None).unwrap();
    assert_eq!(ops.len(), 48);
    let defect = |w: [[f64; 3]; 3]| {
        (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| {
                ((0..3).map(|k| w[k][i] * w[k][j]).sum::<f64>() - f64::from(u8::from(i == j))).abs()
            })
            .fold(0.0, f64::max)
    };
    assert!(ops.iter().any(|o| defect(o.cartesian) > 1e-7));
    assert!(ops.iter().all(|o| defect(o.isometry()) < 1e-14));
    let s = smooth_species(Functional::R2scan);
    let paths = |rotations: &[Mat<f64>]| -> (f64, f64) {
        let n = rotations.len() as f64;
        let average = |m: &Mat<f64>| {
            rotations
                .iter()
                .fold(m * 0.0, |sum, r| sum + r * m * r.transpose())
                * (1.0 / n)
        };
        let rho = average(&nonspherical());
        let one = s.one_center_on_mesh(std::slice::from_ref(&rho));
        let reduced = average(&one.d[0]);
        let (mut energy, mut full) = (0.0, &rho * 0.0);
        for r in rotations {
            let image = s.one_center_on_mesh(&[r.transpose() * &rho * r]);
            energy += image.energy_ha / n;
            full += r * &image.d[0] * r.transpose() * (1.0 / n);
        }
        (
            (one.energy_ha - energy).abs(),
            (&reduced - &full).norm_max(),
        )
    };
    let isometries: Vec<Mat<f64>> = ops.iter().map(|o| s.rotation(o.isometry())).collect();
    let (e, d) = paths(&isometries);
    assert!(e <= 1e-13 && d <= 1e-13, "{e:e} {d:e}");
    let raw: Vec<Mat<f64>> = ops.iter().map(|o| s.rotation(o.cartesian)).collect();
    let (e_raw, d_raw) = paths(&raw);
    assert!(
        e_raw > 1e-13 && d_raw > 1e-13,
        "W itself: {e_raw:e} {d_raw:e}"
    );
}

#[test]
fn the_kinetic_moments_of_closed_form_partial_waves() {
    let s = smooth_species(Functional::R2scan);
    let rho = nonspherical();
    let [ae, _] = s.kinetic_moments(&rho);
    let grid = &s.dataset().draft().grid;
    let r = grid.radii();
    let gaunt = Gaunt::up_to(1, 2);
    let (lm_of, l_of) = ([0, 1, 2, 3], [0, 1, 1, 1]);
    let radial = |l: usize, x: f64| {
        if l == 0 {
            (-4.0 * x).exp()
        } else {
            x * (-4.0 * x).exp()
        }
    };
    let slope = |l: usize, x: f64| {
        if l == 0 {
            -4.0 * (-4.0 * x).exp()
        } else {
            (1.0 - 4.0 * x) * (-4.0 * x).exp()
        }
    };
    let c = |l: usize| (l * (l + 1)) as f64;
    let exact = |big: usize, x: f64| {
        let lb = (big as f64).sqrt() as usize;
        let mut sum = 0.0;
        for i in 0..4 {
            for j in 0..4 {
                let g = gaunt.get(big, lm_of[i], lm_of[j]);
                let g_nabla = (c(l_of[i]) + c(l_of[j]) - c(lb)) / 2.0 * g;
                sum += 0.5
                    * rho[(i, j)]
                    * (g * slope(l_of[i], x) * slope(l_of[j], x)
                        + g_nabla * radial(l_of[i], x) * radial(l_of[j], x) / (x * x));
            }
        }
        sum
    };
    let largest = ae.iter().flatten().fold(0.0_f64, |m, x| m.max(x.abs()));
    let n = ae[0].len() - 3;
    let weighted = |f: &dyn Fn(usize) -> f64| -> f64 {
        let g: Vec<f64> = (0..n)
            .map(|k| if k == 0 { 0.0 } else { f(k) * r[k] * r[k] })
            .collect();
        grid.integrate(&g)
    };
    let scale = (0..9)
        .map(|big| weighted(&|k| exact(big, r[k]).abs()))
        .fold(0.0, f64::max);
    for (big, moment) in ae.iter().enumerate().take(9) {
        for k in (1..n).filter(|&k| r[k] >= 0.01) {
            let want = exact(big, r[k]);
            assert!(
                (moment[k] - want).abs() < 1e-7 * largest,
                "L{big} r {}: {} vs {want}",
                r[k],
                moment[k]
            );
        }
        let gap = (weighted(&|k| moment[k]) - weighted(&|k| exact(big, r[k]))).abs();
        assert!(
            gap <= 1e-9 * scale,
            "L{big}: ∫τr² off by {gap:e} of {scale:e}"
        );
    }
}

#[test]
fn the_flux_runs_to_degree_2_l_plus_1_of_the_runs_largest_l() {
    assert_eq!(smooth_species(Functional::R2scan).flux_degree(), 3);
    assert_eq!(
        smooth_species(Functional::R2scan)
            .with_largest_l(2)
            .flux_degree(),
        5
    );
    assert_eq!(
        smooth_species(Functional::R2scan)
            .with_largest_l(3)
            .flux_degree(),
        7
    );
}

#[test]
fn a_pseudo_core_counted_as_none_drops_its_tau_from_the_pseudo_sphere() {
    let rho = nonspherical();
    let pseudo = |no_core: bool, tau_scale: f64| {
        let s = smooth_species_with(Functional::R2scan, |d| {
            if no_core {
                d.pseudo_core_density.fill(0.0);
            }
            let k = d.core_kinetic.take().unwrap();
            let scaled = k.pseudo().iter().map(|t| t * tau_scale).collect();
            d.core_kinetic = Some(CoreKinetic::checked(&d.grid, k.ae().to_vec(), scaled).unwrap());
        });
        let one = s.one_center_on_mesh(std::slice::from_ref(&rho));
        (one.xc_ha[1], one.d[0].clone())
    };
    let (a, da) = pseudo(true, 1.0);
    let (b, db) = pseudo(true, 2.0);
    assert_eq!(a, b);
    assert_eq!(da, db);
    let (c, dc) = pseudo(false, 1.0);
    let (e, de) = pseudo(false, 2.0);
    assert!((c - e).abs() > 1e-6, "{c} {e}");
    assert!((&dc - &de).norm_max() > 1e-6);
}

fn anisotropic_3d() -> (Species, Vec<Mat<f64>>) {
    let s = Species::for_functional(
        Dataset::try_from(crate::synthetic_3d_draft()).unwrap(),
        Functional::R2scan,
    )
    .unwrap();
    let n = s.orbitals().len();
    let base = s.initial_occupancies();
    let rho = [(0.6, 3), (0.4, 7)]
        .map(|(share, seed)| {
            let w: Vec<f64> = (0..n)
                .map(|i| ((i * seed + 5) % 11) as f64 / 11.0 - 0.4)
                .collect();
            Mat::from_fn(n, n, |i, j| {
                0.1 * share * base[(i, j)] + 2.0 * share * w[i] * w[j]
            })
        })
        .to_vec();
    (s, rho)
}

#[test]
fn the_frame_torque_is_the_turn_derivative_of_the_sphere_energy_about_any_axis() {
    let (s, rho) = anisotropic_3d();
    let t = s.frame_torque(&rho);
    let axis = [1.0, 2.0, 3.0].map(|x| x / 14f64.sqrt());
    let energy = |angle: f64| {
        let d = s.rotation(turn(axis, angle));
        let turned: Vec<Mat<f64>> = rho.iter().map(|p| &d * p * d.transpose()).collect();
        s.one_center(&turned).energy_ha
    };
    let h = 2e-3;
    let slope = (energy(h) - energy(-h)) / (2.0 * h);
    let along: f64 = (0..3).map(|k| t[k] * axis[k]).sum();
    let size = t.iter().map(|x| x.abs()).fold(f64::INFINITY, f64::min);
    assert!(size > 1e-7, "{t:?}");
    assert!(
        (along - slope).abs() < 1e-3 * size,
        "{along} vs {slope} ({t:?})"
    );
    for (k, axis) in [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
        .into_iter()
        .enumerate()
    {
        let about = |angle: f64| {
            let d = s.rotation(turn(axis, angle));
            let turned: Vec<Mat<f64>> = rho.iter().map(|p| &d * p * d.transpose()).collect();
            s.one_center(&turned).energy_ha
        };
        let central = |h: f64| (about(h) - about(-h)) / (2.0 * h);
        let reference = (4.0 * central(2e-3) - central(4e-3)) / 3.0;
        assert!(
            (t[k] - reference).abs() < 2e-4 * reference.abs(),
            "{k}: {} vs {reference}",
            t[k]
        );
    }
    let n = s.orbitals().len();
    let spherical: Vec<Mat<f64>> = (0..2)
        .map(|_| {
            Mat::from_fn(n, n, |i, j| {
                f64::from(u8::from(i == j && s.orbitals()[i].lm() == 0))
            })
        })
        .collect();
    let still = s.frame_torque(&spherical);
    assert!(still.iter().all(|x| x.abs() < 1e-10), "{still:?}");
}

fn strained_frame(a: [[f64; 3]; 3], e: [[f64; 3]; 3], t: f64) -> [[f64; 3]; 3] {
    let rows: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3)
                .map(|m| (f64::from(u8::from(c == m)) + t * e[c][m]) * a[r][m])
                .sum()
        })
    });
    Cell::try_from(rows).unwrap().frame()
}

#[test]
fn the_frame_strain_is_the_strain_derivative_of_the_sphere_energy_in_the_turning_upright_frame() {
    let (s, rho) = anisotropic_3d();
    let a = [[5.0, 1.0, 2.0], [0.5, 6.0, -1.0], [1.0, -0.7, 5.5]];
    let e = [[0.3, 0.5, -0.2], [0.5, -0.4, 0.6], [-0.2, 0.6, 0.1]];
    let energy = |frame: [[f64; 3]; 3]| s.sphere_terms(Some(&s.rotation(frame)), &[], &rho).0;
    let h = 1e-4;
    let slope = (energy(strained_frame(a, e, h)) - energy(strained_frame(a, e, -h))) / (2.0 * h);
    let x = s.frame_strain(Cell::try_from(a).unwrap().frame(), &[], &rho);
    let along: f64 = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| x[i][j] * e[i][j])
        .sum();
    assert!(along.abs() > 1e-6, "{along}");
    assert!(
        (along - slope).abs() < 1e-4 * along.abs(),
        "{along} vs {slope}"
    );
    let upright = [[6.1, 0.0, 0.0], [1.9, 5.7, 0.0], [1.3, 2.2, 6.8]];
    let diagonal = [[0.6, 0.0, 0.0], [0.0, -0.3, 0.0], [0.0, 0.0, 0.2]];
    let x = s.frame_strain(Cell::try_from(upright).unwrap().frame(), &[], &rho);
    let still: f64 = (0..3).map(|i| x[i][i] * diagonal[i][i]).sum();
    assert!(still.abs() < 1e-12, "{still}");
}
