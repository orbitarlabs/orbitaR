use crate::synthetic_draft;
use orbitar::model::{Cell, Functional};
use orbitar::paw::{CoreKinetic, Dataset, DatasetDraft, Shape, Species};
use orbitar::pw::{Grid, Mat, Spins, exchange_correlation};
use std::f64::consts::PI;

fn species(change: impl Fn(&mut DatasetDraft)) -> Species {
    let mut draft = synthetic_draft();
    change(&mut draft);
    Species::from(Dataset::try_from(draft).unwrap())
}

fn shape_integral(s: &Species, l: usize, radius: f64) -> f64 {
    let n = 20000;
    let h = radius / n as f64;
    let f = |k: usize| {
        let r = k as f64 * h;
        s.shape(l, r) * r.powi(l as i32 + 2)
    };
    let inner: f64 = (1..n)
        .map(|k| if k % 2 == 1 { 4.0 * f(k) } else { 2.0 * f(k) })
        .sum();
    (f(0) + inner + f(n)) * h / 3.0
}

#[test]
fn orbitals_run_over_channels_then_m_from_minus_l() {
    let s = species(|_| {});
    let got: Vec<(usize, usize, i32, usize)> = s
        .orbitals()
        .iter()
        .map(|o| (o.channel, o.l, o.m, o.lm()))
        .collect();
    assert_eq!(
        got,
        [(0, 0, 0, 0), (1, 1, -1, 1), (1, 1, 0, 2), (1, 1, 1, 3)]
    );
}

#[test]
fn bessel_shapes_hold_unit_moments_and_end_flat_at_r_shape() {
    let s = species(|_| {});
    for l in 0..=2 {
        assert!((shape_integral(&s, l, 1.2) - 1.0).abs() < 1e-10, "l {l}");
        let h = 1e-6;
        assert!(s.shape(l, 1.2).abs() < 1e-12, "g_{l}(r_shape)");
        assert!(
            ((s.shape(l, 1.2) - s.shape(l, 1.2 - h)) / h).abs() < 1e-5,
            "g_{l}'(r_shape)"
        );
    }
}

#[test]
fn sinc_shapes_are_normalized_on_the_dataset_grid() {
    let s = species(|d| d.shape = Shape::Sinc { radius: 1.2 });
    for l in 0..=2 {
        assert!((shape_integral(&s, l, 1.2) - 1.0).abs() < 1e-6, "l {l}");
    }
}

#[test]
fn a_sinc_shape_is_sinc_squared_times_r_to_the_l() {
    let s = species(|d| d.shape = Shape::Sinc { radius: 1.2 });
    for l in 0..=2 {
        let ratio = s.shape(l, 0.6) / s.shape(l, 0.3) / 2f64.powi(l as i32);
        assert!((ratio - 0.5).abs() < 1e-14, "l {l}: {ratio}");
    }
}

#[test]
fn a_sinc_shape_is_normalized_over_the_grid_points_below_the_last_one_inside_r_shape() {
    let s = species(|d| d.shape = Shape::Sinc { radius: 1.2 });
    let grid = synthetic_draft().grid;
    let r = grid.radii();
    let last = r.iter().rposition(|&x| x <= 1.2).unwrap();
    let sinc2 = |x: f64| (PI * x / 1.2).sin().powi(2) / (PI * x / 1.2).powi(2);
    for l in 0..=2 {
        let f: Vec<f64> = (0..=grid.nearest(1.2))
            .map(|k| {
                if k > 0 && k < last {
                    sinc2(r[k]) * r[k].powi(2 * l as i32 + 2)
                } else {
                    0.0
                }
            })
            .collect();
        let want = sinc2(0.6) * 0.6f64.powi(l as i32) / grid.integrate(&f);
        assert!((s.shape(l, 0.6) / want - 1.0).abs() < 1e-13, "l {l}");
    }
}

#[test]
fn moments_and_the_overlap_are_symmetric_and_couple_only_what_symmetry_allows() {
    let s = species(|_| {});
    let draft = synthetic_draft();
    let r = draft.grid.radii();
    let end = draft.grid.nearest(1.5) + 1;
    let overlap = s.overlap();
    let (ae, ps) = (&draft.channels[0].ae, &draft.channels[0].pseudo);
    let f: Vec<f64> = (0..end)
        .map(|k| (ae[k] * ae[k] - ps[k] * ps[k]) * r[k] * r[k])
        .collect();
    assert!((overlap[(0, 0)] - draft.grid.integrate(&f)).abs() < 1e-14);
    for (i, j) in [(0, 1), (0, 2), (1, 2), (1, 3)] {
        assert_eq!(overlap[(i, j)], 0.0);
    }
    for lm in 0..9 {
        let q = s.moment(lm);
        for i in 0..4 {
            for j in 0..4 {
                assert_eq!(q[(i, j)], q[(j, i)], "q^{lm}");
            }
        }
    }
    for lm in 4..9 {
        let q = s.moment(lm);
        assert!(
            (1..4).any(|i| (1..4).any(|j| q[(i, j)].abs() > 1e-3)),
            "q^{lm}"
        );
    }
    let dipole = s.moment(2)[(0, 2)];
    assert!(dipole.abs() > 1e-3);
    for lm in 1..4 {
        assert_eq!(s.moment(lm)[(0, 0)], 0.0);
        assert!(
            (s.moment(lm)[(0, lm)] - dipole).abs() < 1e-14 * dipole.abs(),
            "q^{lm}"
        );
    }
}

#[test]
fn two_f_orbitals_couple_through_every_moment_up_to_l_6() {
    let s = species(|d| d.channels[1].l = 3);
    let f: Vec<usize> = (0..s.orbitals().len())
        .filter(|&i| s.orbitals()[i].l == 3)
        .collect();
    assert_eq!(f.len(), 7);
    for lm in 36..49 {
        let q = s.moment(lm);
        assert!(
            f.iter().any(|&i| f.iter().any(|&j| q[(i, j)].abs() > 1e-3)),
            "q^{lm}"
        );
    }
}

#[test]
fn the_hartree_kernel_is_symmetric_in_each_pair_and_between_pairs() {
    let s = species(|_| {});
    let n = 4;
    let unit = |i: usize, j: usize| {
        let mut rho = Mat::<f64>::zeros(n, n);
        rho[(i, j)] = 1.0;
        s.hartree(&rho)
    };
    for (i, j) in [(0, 0), (0, 2), (1, 3), (2, 2)] {
        let (a, b) = (unit(i, j), unit(j, i));
        for k in 0..n {
            for l in 0..n {
                assert_eq!(a[(k, l)], b[(k, l)]);
                assert_eq!(a[(k, l)], a[(l, k)]);
                assert_eq!(a[(k, l)], unit(k, l)[(i, j)]);
            }
        }
    }
}

#[test]
fn with_equal_partial_waves_the_frozen_d_is_the_kinetic_difference() {
    let s = species(|d| {
        for c in &mut d.channels {
            c.pseudo = c.ae.clone();
        }
        let r = d.grid.radii().to_vec();
        d.local_potential = r
            .iter()
            .map(|&r| {
                if r > 0.0 {
                    (2.0 * libm::erf(2.0 * r) - 6.0) / r
                } else {
                    0.0
                }
            })
            .collect();
    });
    let d0 = s.frozen_d();
    let t = [[1.0, 0.2], [0.2, 3.0]];
    for (i, a) in s.orbitals().iter().enumerate() {
        for (j, b) in s.orbitals().iter().enumerate() {
            let want = if (a.l, a.m) == (b.l, b.m) {
                t[a.channel][b.channel]
            } else {
                0.0
            };
            assert!(
                (d0[(i, j)] - want).abs() < 1e-9,
                "D0[{i}][{j}] = {} vs {want}",
                d0[(i, j)]
            );
        }
    }
}

#[test]
fn a_constant_added_to_the_local_potential_lowers_the_frozen_d_by_the_partial_wave_overlap() {
    let c = 0.3;
    let base = species(|_| {});
    let shifted = species(|d| d.local_potential.iter_mut().for_each(|v| *v += c));
    let draft = synthetic_draft();
    let r = draft.grid.radii();
    let end = draft.grid.nearest(1.5) + 1;
    for (i, channel) in [(0, 0), (1, 1), (2, 1), (3, 1)] {
        let ae = &draft.channels[channel].ae;
        let f: Vec<f64> = (0..end).map(|k| ae[k] * ae[k] * r[k] * r[k]).collect();
        let want = -c * draft.grid.integrate(&f);
        let got = shifted.frozen_d()[(i, i)] - base.frozen_d()[(i, i)];
        assert!((got - want).abs() < 1e-12, "D0[{i}][{i}]: {got} vs {want}");
    }
}

#[test]
fn the_free_atom_spreads_each_shell_over_its_m() {
    let rho = species(|_| {}).initial_occupancies();
    let diagonal: Vec<f64> = (0..4).map(|i| rho[(i, i)]).collect();
    assert_eq!(diagonal, [2.0, 2.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0]);
    assert_eq!(rho[(0, 2)], 0.0);
}

#[test]
fn the_core_xc_energy_equals_that_of_the_same_density_on_a_plane_wave_grid() {
    let alpha = 0.5_f64;
    let gaussian = move |r: f64| 2.0 * (alpha / PI).powf(1.5) * (-alpha * r * r).exp();
    let s = species(|d| d.core_density = d.grid.radii().iter().map(|&r| gaussian(r)).collect());
    let cell = Cell::try_from([[16.0, 0.0, 0.0], [0.0, 16.0, 0.0], [0.0, 0.0, 16.0]]).unwrap();
    let grid = Grid::coarse(&cell, 4.0);
    let core: Vec<f64> = grid
        .points()
        .iter()
        .map(|f| {
            let d = f.map(|x| 16.0 * (x - 0.5));
            gaussian((d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt())
        })
        .collect();
    let zero = vec![0.0; core.len()];
    let box_energy = exchange_correlation(
        &grid,
        Functional::Pbe,
        Spins::Unpolarized(&zero),
        Some(&core),
        None,
    )
    .energy_ha;
    assert!(
        (s.core_xc() - box_energy).abs() < 1e-7,
        "{} vs {box_energy}",
        s.core_xc()
    );
}

#[test]
fn an_orbital_rotation_turns_each_channel_by_its_l_and_keeps_the_others_apart() {
    let carbon = species(|_| {});
    let n = carbon.orbitals().len();
    let identity = carbon.rotation([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]);
    let inversion = carbon.rotation([[-1.0, 0.0, 0.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]]);
    for i in 0..n {
        for j in 0..n {
            let parity = if carbon.orbitals()[i].l % 2 == 0 {
                1.0
            } else {
                -1.0
            };
            let want = if i == j { 1.0 } else { 0.0 };
            assert!((identity[(i, j)] - want).abs() < 1e-14);
            assert!((inversion[(i, j)] - parity * want).abs() < 1e-14);
        }
    }
}

#[test]
fn a_quarter_turn_about_z_takes_p_y_to_p_x_and_p_x_to_minus_p_y() {
    let carbon = species(|_| {});
    let d = carbon.rotation([[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]);
    let p: Vec<usize> = (0..carbon.orbitals().len())
        .filter(|&i| carbon.orbitals()[i].l == 1)
        .take(3)
        .collect();
    let want = [[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [-1.0, 0.0, 0.0]];
    for (row, &i) in want.iter().zip(&p) {
        for (value, &j) in row.iter().zip(&p) {
            assert!((d[(i, j)] - value).abs() < 1e-14, "{i} {j}");
        }
    }
}

#[test]
fn a_meta_gga_core_xc_equals_libxc_by_abinits_radial_recipe() {
    let mut draft = synthetic_draft();
    let r = draft.grid.radii().to_vec();
    let c = 0.3 * (3.0 * std::f64::consts::PI.powi(2)).powf(2.0 / 3.0);
    let tau: Vec<f64> = (draft.core_density.iter().zip(&r))
        .map(|(n, x)| c * n.powf(5.0 / 3.0) + 16.0 * x * x * n)
        .collect();
    draft.core_kinetic = Some(CoreKinetic::checked(&draft.grid, tau.clone(), tau).unwrap());
    let species =
        Species::for_functional(Dataset::try_from(draft).unwrap(), Functional::R2scan).unwrap();
    let got = species.core_xc();
    assert!((got - -1.395417779923).abs() < 1e-9, "{got}");
}

#[test]
fn a_meta_gga_species_of_a_dataset_without_core_tau_is_refused_by_name() {
    let dataset = Dataset::try_from(synthetic_draft()).unwrap();
    let refused = Species::for_functional(dataset.clone(), Functional::R2scan).map(|_| ());
    assert_eq!(
        refused,
        Err("datasets: the dataset for Z = 6 has no core kinetic energy densities, which r2scan reads".into())
    );
    assert!(Species::for_functional(dataset, Functional::Pbe).is_ok());
}
