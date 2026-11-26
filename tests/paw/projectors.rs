use crate::synthetic_draft;
use orbitar::model::Cell;
use orbitar::paw::{Dataset, Projectors};
use orbitar::paw::{gauss_legendre, real_ylm};
use orbitar::pw::{Basis, c64};
use std::f64::consts::PI;

fn setup() -> (Cell, Basis, Projectors, Dataset) {
    let cell = Cell::try_from([[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]]).unwrap();
    let basis = Basis::sphere(&cell, [0.1, -0.2, 0.3], 6.0);
    let dataset = Dataset::try_from(synthetic_draft()).unwrap();
    let projectors = Projectors::tabulated(&dataset, (2.0 * 6.0_f64).sqrt());
    (cell, basis, projectors, dataset)
}

#[test]
fn one_column_per_channel_and_m() {
    let (cell, basis, projectors, _) = setup();
    assert_eq!(projectors.count(), 4);
    let b = projectors.at(&cell, [0.1, -0.2, 0.3], &basis, [0.0; 3]);
    assert_eq!((b.nrows(), b.ncols()), (basis.millers().len(), 4));
}

#[test]
fn the_m_sum_of_each_channel_is_the_addition_theorem_value() {
    let (cell, basis, projectors, dataset) = setup();
    let k = [0.1, -0.2, 0.3];
    let b = projectors.at(&cell, k, &basis, [0.21, 0.4, -0.3]);
    let d = dataset.draft();
    for (row, m) in basis.millers().iter().enumerate().step_by(7) {
        let q = cell.wavevector(std::array::from_fn(|i| k[i] + f64::from(m[i])));
        let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        let mut column = 0;
        for c in &d.channels {
            let f = d.grid.bessel_transform(&c.projector, c.l, norm);
            let want = 16.0 * PI * PI / cell.volume() * f * f * (2 * c.l + 1) as f64 / (4.0 * PI);
            let got: f64 = (column..column + 2 * c.l + 1)
                .map(|j| b[(row, j)].norm_sqr())
                .sum();
            assert!(
                (got - want).abs() < 1e-9 * want.max(1e-12),
                "{m:?}, l {}",
                c.l
            );
            column += 2 * c.l + 1;
        }
    }
}

#[test]
fn moving_the_atom_multiplies_each_row_by_its_plane_wave_phase() {
    let (cell, basis, projectors, _) = setup();
    let k = [0.1, -0.2, 0.3];
    let here = projectors.at(&cell, k, &basis, [0.1, 0.2, 0.3]);
    let n = [1.0, -1.0, 2.0];
    let translated = projectors.at(&cell, k, &basis, [1.1, -0.8, 2.3]);
    let bloch = -2.0 * PI * (0..3).map(|i| k[i] * n[i]).sum::<f64>();
    let bloch = c64::new(bloch.cos(), bloch.sin());
    let delta = [0.05, -0.1, 0.2];
    let moved = projectors.at(&cell, k, &basis, [0.15, 0.1, 0.5]);
    for (row, m) in basis.millers().iter().enumerate() {
        let phase = -2.0
            * PI
            * (0..3)
                .map(|i| (k[i] + f64::from(m[i])) * delta[i])
                .sum::<f64>();
        for j in 0..4 {
            assert!((translated[(row, j)] - here[(row, j)] * bloch).norm() < 1e-12);
            assert!(
                (moved[(row, j)] - here[(row, j)] * c64::new(phase.cos(), phase.sin())).norm()
                    < 1e-12
            );
        }
    }
}

#[test]
fn each_projector_is_integrated_to_the_first_zero_after_its_last_nonzero_value() {
    let mut draft = synthetic_draft();
    let cut = 600;
    let r = draft.grid.radii().to_vec();
    draft.channels[0].projector = (0..r.len())
        .map(|k| if k <= cut { (-r[k]).exp() } else { 0.0 })
        .collect();
    draft.channels[0].projector[cut + 1] = 1.5e-56;
    let d = Dataset::try_from(draft.clone()).unwrap();
    let projectors = Projectors::tabulated(&d, 4.0);
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]).unwrap();
    let basis = Basis::sphere(&cell, [0.0; 3], 2.0);
    let b = projectors.at(&cell, [0.0; 3], &basis, [0.0; 3]);
    let prefactor = 4.0 * PI / cell.volume().sqrt() / (4.0 * PI).sqrt();
    let sphere = r.partition_point(|&x| x <= 1.5) + 1;
    assert_eq!((cut + 2, sphere % 2), (602, 1));
    for (row, m) in basis.millers().iter().enumerate() {
        let q = cell.wavevector(m.map(f64::from));
        let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        let want = prefactor
            * draft
                .grid
                .bessel_transform(&draft.channels[0].projector[..cut + 2], 0, norm);
        let old = prefactor
            * draft
                .grid
                .bessel_transform(&draft.channels[0].projector[..sphere], 0, norm);
        assert!((b[(row, 0)].re - want).abs() < 1e-9 * want.abs(), "{m:?}");
        if norm > 0.0 {
            assert!(
                (want - old).abs() > 1e-8 * want.abs(),
                "{m:?}: the two ends must differ here"
            );
        }
    }
}

#[test]
fn columns_equal_a_direct_quadrature_of_each_projector_against_the_plane_wave() {
    let (cell, basis, projectors, dataset) = setup();
    let k = [0.1, -0.2, 0.3];
    let tau = [0.21, 0.4, -0.3];
    let b = projectors.at(&cell, k, &basis, tau);
    let d = dataset.draft();
    let r = d.grid.radii();
    let end = r.partition_point(|&x| x <= d.sphere_radius) + 1;
    let nodes = gauss_legendre(16);
    let azimuths = 32;
    for (row, m) in basis.millers().iter().enumerate().step_by(29) {
        let fractional: [f64; 3] = std::array::from_fn(|i| k[i] + f64::from(m[i]));
        let q = cell.wavevector(fractional);
        let shift = -2.0 * PI * (0..3).map(|i| fractional[i] * tau[i]).sum::<f64>();
        let mut column = 0;
        for c in &d.channels {
            for lm in c.l * c.l..(c.l + 1) * (c.l + 1) {
                let mut sum = c64::new(0.0, 0.0);
                for &(z, w) in &nodes {
                    let s = (1.0 - z * z).sqrt();
                    for a in 0..azimuths {
                        let phi = 2.0 * PI * a as f64 / azimuths as f64;
                        let dir = [s * phi.cos(), s * phi.sin(), z];
                        let y = real_ylm(3, dir)[lm];
                        let qr = q[0] * dir[0] + q[1] * dir[1] + q[2] * dir[2];
                        let radial: Vec<c64> = (0..end)
                            .map(|i| {
                                let x = -qr * r[i];
                                c64::new(x.cos(), x.sin()) * r[i] * r[i] * c.projector[i]
                            })
                            .collect();
                        let re: Vec<f64> = radial.iter().map(|z| z.re).collect();
                        let im: Vec<f64> = radial.iter().map(|z| z.im).collect();
                        let angular = w * 2.0 * PI / azimuths as f64 * y;
                        sum += c64::new(d.grid.integrate(&re), d.grid.integrate(&im)) * angular;
                    }
                }
                let want = sum * c64::new(shift.cos(), shift.sin()) / cell.volume().sqrt();
                assert!(
                    (b[(row, column)] - want).norm() < 1e-9,
                    "{m:?}, lm {lm}: {} vs {want}",
                    b[(row, column)]
                );
                column += 1;
            }
        }
    }
}

#[test]
fn displaced_projections_are_the_derivative_of_the_projections_in_the_atom_position() {
    let (cell, basis, projectors, _) = setup();
    let k = [0.1, -0.2, 0.3];
    let at = [0.21, 0.4, -0.3];
    let x = orbitar::pw::Mat::from_fn(basis.millers().len(), 2, |g, band| {
        c64::new(
            (g as f64 * 0.37 + band as f64).sin(),
            (g as f64 * 0.11).cos() / (1.0 + g as f64),
        )
    });
    let p = projectors.at(&cell, k, &basis, at);
    let b = cell.reciprocal();
    let h = 1e-3;
    for (axis, column) in (0..3).map(|axis| b.map(|row| row[axis])).enumerate() {
        let derivative = Projectors::displaced(&p, &cell, k, &basis, x.as_ref(), axis);
        let projected = |t: f64| {
            let moved: [f64; 3] =
                std::array::from_fn(|i| at[i] + t * h * column[i] / std::f64::consts::TAU);
            projectors.at(&cell, k, &basis, moved).adjoint() * &x
        };
        let (a, z, y, w) = (
            projected(1.0),
            projected(-1.0),
            projected(2.0),
            projected(-2.0),
        );
        for i in 0..p.ncols() {
            for band in 0..2 {
                let fd = ((a[(i, band)] - z[(i, band)]) * 8.0 - (y[(i, band)] - w[(i, band)]))
                    / (12.0 * h);
                assert!(
                    (derivative[(i, band)] - fd).norm() < 1e-9 * (1.0 + fd.norm()),
                    "{axis} {i} {band}: {} vs {fd}",
                    derivative[(i, band)]
                );
            }
        }
    }
}

fn hydrogenic(p_bound: bool) -> Dataset {
    let mut draft = synthetic_draft();
    let r = draft.grid.radii().to_vec();
    let sphere = draft.sphere_radius;
    for (l, c) in draft.channels.iter_mut().enumerate() {
        c.pseudo = r.iter().map(|&r| r.powi(l as i32) * (-r).exp()).collect();
        c.ae = (r.iter().zip(&c.pseudo))
            .map(|(&r, &f)| if r <= sphere { 2.0 * f } else { f })
            .collect();
    }
    if !p_bound {
        (draft.z, draft.valence_electrons) = (4.0, 2.0);
        draft.channels[1].occupation = None;
    }
    Dataset::try_from(draft).unwrap()
}

#[test]
fn pseudo_orbitals_transform_e_minus_r_to_its_analytic_form_factor() {
    let (cell, basis, _, _) = setup();
    let k = [0.1, -0.2, 0.3];
    let orbitals = Projectors::pseudo_orbitals(&hydrogenic(true), (2.0 * 6.0_f64).sqrt());
    assert_eq!(orbitals.count(), 4);
    let b = orbitals.at(&cell, k, &basis, [0.3, -0.1, 0.25]);
    for (row, m) in basis.millers().iter().enumerate() {
        let q = cell.wavevector(std::array::from_fn(|i| k[i] + f64::from(m[i])));
        let q = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        let analytic = [2.0 / (1.0 + q * q).powi(2), 8.0 * q / (1.0 + q * q).powi(3)];
        for (l, f) in analytic.into_iter().enumerate() {
            let want = 16.0 * PI * PI / cell.volume() * f * f * (2 * l + 1) as f64 / (4.0 * PI);
            let got: f64 = (l * l..(l + 1) * (l + 1))
                .map(|j| b[(row, j)].norm_sqr())
                .sum();
            assert!(
                (got - want).abs() < 1e-5 * want + 1e-14,
                "{m:?}, l {l}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn pseudo_orbitals_leave_out_an_unbound_state() {
    let orbitals = Projectors::pseudo_orbitals(&hydrogenic(false), 3.0);
    assert_eq!(orbitals.count(), 1);
    assert_eq!(Projectors::tabulated(&hydrogenic(false), 3.0).count(), 4);
}

#[test]
fn the_pseudo_orbital_block_has_the_norm_of_the_orbital_in_a_large_basis() {
    let cell = Cell::try_from([[24.0, 0.0, 0.0], [0.0, 24.0, 0.0], [0.0, 0.0, 24.0]]).unwrap();
    let basis = Basis::sphere(&cell, [0.0; 3], 32.0);
    let orbitals = Projectors::pseudo_orbitals(&hydrogenic(true), 8.0);
    let b = orbitals.at(&cell, [0.0; 3], &basis, [0.2, 0.5, 0.7]);
    let gram = b.adjoint() * &b;
    let want = [0.25, 0.75, 0.75, 0.75];
    for i in 0..4 {
        for j in 0..4 {
            let w = if i == j { want[i] } else { 0.0 };
            assert!(
                (gram[(i, j)] - c64::new(w, 0.0)).norm() < 1e-4,
                "{i} {j}: {}",
                gram[(i, j)]
            );
        }
    }
}

#[test]
fn strained_projections_are_the_derivative_in_the_strain_at_fixed_plane_waves() {
    let (cell, basis, projectors, _) = setup();
    let (k, at) = ([0.1, -0.2, 0.3], [0.21, 0.4, -0.3]);
    let rows = [[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]];
    let strained = projectors.strained(&cell, k, &basis, at);
    let h = 1e-4;
    for i in [0, 1, 2] {
        for j in 0..3 {
            let on = |t: f64| {
                let moved: [[f64; 3]; 3] = std::array::from_fn(|r| {
                    std::array::from_fn(|c| {
                        rows[r][c] + if c == i { t * h * rows[r][j] } else { 0.0 }
                    })
                });
                projectors.at(&Cell::try_from(moved).unwrap(), k, &basis, at)
            };
            let (a, z, y, w) = (on(1.0), on(-1.0), on(2.0), on(-2.0));
            let mut largest: f64 = 0.0;
            for g in 0..basis.millers().len() {
                for c in 0..projectors.count() {
                    let fd = ((a[(g, c)] - z[(g, c)]) * 8.0 - (y[(g, c)] - w[(g, c)])) / (12.0 * h);
                    let got = strained[i][j][(g, c)];
                    assert!(
                        (got - fd).norm() < 1e-8,
                        "({i}, {j}) row {g} column {c}: {got} vs {fd}"
                    );
                    largest = largest.max(got.norm());
                }
            }
            assert!(largest > 1e-2, "({i}, {j}): {largest}");
        }
    }
}

#[test]
fn a_p_orbital_column_is_minus_i_y_times_its_form_factor_with_the_structure_phase() {
    let (cell, basis, _, _) = setup();
    let (k, tau) = ([0.1, -0.2, 0.3], [0.3, -0.1, 0.25]);
    let b = Projectors::pseudo_orbitals(&hydrogenic(true), (2.0 * 6.0_f64).sqrt())
        .at(&cell, k, &basis, tau);
    for (row, m) in basis.millers().iter().enumerate() {
        let fractional: [f64; 3] = std::array::from_fn(|i| k[i] + f64::from(m[i]));
        let q = cell.wavevector(fractional);
        let norm = q.iter().map(|x| x * x).sum::<f64>().sqrt();
        let phase = -2.0 * PI * (0..3).map(|i| fractional[i] * tau[i]).sum::<f64>();
        let f1 = 8.0 * norm / (1.0 + norm * norm).powi(3);
        let want = c64::new(phase.cos(), phase.sin())
            * c64::new(0.0, -1.0)
            * real_ylm(1, q)[2]
            * f1
            * (4.0 * PI / cell.volume().sqrt());
        assert!(
            (b[(row, 2)] - want).norm() < 1e-5 * want.norm() + 1e-12,
            "{m:?}: {} vs {want}",
            b[(row, 2)]
        );
    }
}

#[test]
fn at_gamma_an_orbital_column_holds_c_minus_g_equal_to_the_conjugate_of_c_g() {
    let cell = Cell::try_from([[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]]).unwrap();
    let basis = Basis::sphere(&cell, [0.0; 3], 6.0);
    let b = Projectors::pseudo_orbitals(&hydrogenic(true), (2.0 * 6.0_f64).sqrt()).at(
        &cell,
        [0.0; 3],
        &basis,
        [0.3, -0.1, 0.25],
    );
    let millers = basis.millers();
    for (row, m) in millers.iter().enumerate() {
        let minus = millers
            .iter()
            .position(|g| *g == m.map(|x| -x))
            .expect("a Γ sphere holds −G");
        for column in 0..b.ncols() {
            let (c, d) = (b[(row, column)], b[(minus, column)]);
            assert!(
                (d - c.conj()).norm() <= 1e-14 * c.norm(),
                "{m:?}, column {column}: {c} vs {d}"
            );
        }
    }
}

#[test]
#[should_panic(expected = "the basis that `sampled` took")]
fn a_sampling_refuses_another_basis() {
    let (cell, basis, projectors, _) = setup();
    let sampled = projectors.sampled(&cell, [0.1, -0.2, 0.3], &basis);
    let other = Basis::sphere(&cell, [0.0; 3], 3.0);
    sampled.at(&other, [0.0; 3]);
}
