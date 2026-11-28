use orbitar::model::Cell;
use orbitar::pw::{Basis, Mat, c64};
use std::f64::consts::TAU;

fn unit_cube() -> Cell {
    Cell::try_from([[TAU, 0.0, 0.0], [0.0, TAU, 0.0], [0.0, 0.0, TAU]]).unwrap()
}

#[test]
fn the_gamma_sphere_holds_the_lattice_points_within_the_cutoff() {
    for (cutoff, size) in [(0.5, 7), (1.0, 19), (1.5, 27)] {
        assert_eq!(
            Basis::sphere(&unit_cube(), [0.0; 3], cutoff)
                .kinetic_ha()
                .len(),
            size
        );
    }
}

#[test]
fn plane_waves_come_in_lexicographic_miller_order_with_their_kinetic_energies() {
    let basis = Basis::sphere(&unit_cube(), [0.25, 0.0, 0.0], 0.6);
    let millers = basis.millers();
    assert!(millers.windows(2).all(|w| w[0] < w[1]));
    for (m, t) in millers.iter().zip(basis.kinetic_ha()) {
        let want = 0.5 * ((f64::from(m[0]) + 0.25).powi(2) + f64::from(m[1] * m[1] + m[2] * m[2]));
        assert!((t - want).abs() < 1e-15);
    }
    assert_eq!(
        millers,
        [
            [-1, 0, 0],
            [0, -1, 0],
            [0, 0, -1],
            [0, 0, 0],
            [0, 0, 1],
            [0, 1, 0]
        ]
    );
}

#[test]
fn each_wavevector_is_k_plus_g_in_cartesian_bohr_and_its_half_square_is_the_kinetic_energy() {
    let cell = Cell::try_from([[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]]).unwrap();
    let k = [0.1, -0.2, 0.35];
    let basis = Basis::sphere(&cell, k, 4.0);
    let b = TAU / 10.2;
    let rows = [[-b, b, b], [b, -b, b], [b, b, -b]];
    assert_eq!(basis.wavevectors().len(), basis.millers().len());
    for ((m, q), t) in basis
        .millers()
        .iter()
        .zip(basis.wavevectors())
        .zip(basis.kinetic_ha())
    {
        let want: [f64; 3] =
            std::array::from_fn(|a| (0..3).map(|i| (f64::from(m[i]) + k[i]) * rows[i][a]).sum());
        assert!(
            (0..3).all(|a| (q[a] - want[a]).abs() < 1e-14),
            "{m:?}: {q:?} vs {want:?}"
        );
        assert!((0.5 * q.iter().map(|x| x * x).sum::<f64>() - t).abs() < 1e-13);
    }
}

#[test]
fn a_plane_wave_on_the_sphere_is_inside_and_one_just_beyond_is_not() {
    let on = Basis::sphere(&unit_cube(), [0.0; 3], 0.5);
    assert_eq!(on.kinetic_ha(), [0.5, 0.5, 0.5, 0.0, 0.5, 0.5, 0.5]);
    let below = Basis::sphere(&unit_cube(), [0.0; 3], 0.5 - 1e-9);
    assert_eq!(below.kinetic_ha(), [0.0]);
}

#[test]
fn the_sphere_is_centered_on_k() {
    let half = Basis::sphere(&unit_cube(), [0.5, 0.0, 0.0], 0.125);
    assert_eq!(half.kinetic_ha(), [0.125, 0.125]);
    assert_eq!(half.millers(), [[-1, 0, 0], [0, 0, 0]]);
    for k in [0.9, -0.9] {
        let near = Basis::sphere(&unit_cube(), [k, 0.0, 0.0], 0.02);
        assert_eq!(near.kinetic_ha().len(), 1, "{k}");
        assert!((near.kinetic_ha()[0] - 0.005).abs() < 1e-15, "{k}");
    }
}

#[test]
fn a_large_sphere_holds_every_lattice_point_within_it() {
    let want = (-20..=20_i32)
        .flat_map(|i| (-20..=20).flat_map(move |j| (-20..=20).map(move |l| i * i + j * j + l * l)))
        .filter(|&norm| norm <= 100)
        .count();
    assert_eq!(
        Basis::sphere(&unit_cube(), [0.0; 3], 50.0)
            .kinetic_ha()
            .len(),
        want
    );
}

#[test]
fn a_long_lattice_vector_still_finds_every_plane_wave() {
    let long = Cell::try_from([[4.0 * TAU, 0.0, 0.0], [0.0, TAU, 0.0], [0.0, 0.0, TAU]]).unwrap();
    assert_eq!(Basis::sphere(&long, [0.0; 3], 0.5).kinetic_ha().len(), 13);
}

#[test]
fn a_sheared_cell_finds_every_plane_wave_at_any_k() {
    let cell = Cell::try_from([[4.0, 0.0, 0.0], [3.0, 3.0, 0.0], [3.0, 3.0, 3.0]]).unwrap();
    let b = cell.reciprocal();
    for k in [[0.0; 3], [0.75, -0.9, 0.95]] {
        let want = (-12..=12_i32)
            .flat_map(|i| (-12..=12).flat_map(move |j| (-12..=12).map(move |l| [i, j, l])))
            .filter(|n| {
                let q: [f64; 3] = std::array::from_fn(|c| {
                    (0..3).map(|i| (f64::from(n[i]) + k[i]) * b[i][c]).sum()
                });
                0.5 * q.iter().map(|x| x * x).sum::<f64>() <= 20.0
            })
            .count();
        assert_eq!(
            Basis::sphere(&cell, k, 20.0).kinetic_ha().len(),
            want,
            "{k:?}"
        );
    }
}

fn sheared() -> Cell {
    Cell::try_from([[4.0, 0.3, -0.2], [1.1, 5.0, 0.4], [-0.7, 0.9, 6.0]]).unwrap()
}

#[test]
fn plane_waves_pair_up_as_q_and_minus_q_where_twice_k_is_a_lattice_vector() {
    let cell = sheared();
    for k in [[0.0; 3], [0.5, 0.0, 0.5], [-0.5, 1.5, 1.0]] {
        let basis = Basis::sphere(&cell, k, 6.0);
        let pairs = basis.pairs(k).expect("2k is a reciprocal lattice vector");
        let q = |row: usize| {
            let m = basis.millers()[row];
            cell.wavevector(std::array::from_fn(|i| k[i] + f64::from(m[i])))
        };
        let (mut firsts, mut seconds) = (0, 0);
        for row in 0..basis.millers().len() {
            let one = Mat::from_fn(basis.millers().len(), 1, |g, _| {
                c64::new(f64::from(u8::from(g == row)), 0.0)
            });
            let both = pairs.complex(pairs.real(one.as_ref()).as_ref());
            let Some(partner) = (0..both.nrows()).rfind(|&g| both[(g, 0)].norm() > 0.0) else {
                seconds += 1;
                continue;
            };
            let sum: f64 = (0..3).map(|i| (q(row)[i] + q(partner)[i]).abs()).sum();
            assert!(partner >= row && sum < 1e-12, "{k:?} row {row}: {sum}");
            firsts += usize::from(partner > row);
        }
        assert_eq!(
            firsts, seconds,
            "{k:?}: each pair has one second plane wave"
        );
        assert!(firsts > 0);
    }
    for k in [[0.1, 0.2, -0.3], [0.25, 0.0, 0.0], [0.5, 0.5, 0.3]] {
        assert_eq!(Basis::sphere(&cell, k, 6.0).pairs(k), None, "{k:?}");
    }
}

#[test]
fn two_real_bands_packed_as_one_complex_column_split_back_into_each_band() {
    for k in [[0.0; 3], [0.5, 0.0, 0.5]] {
        let basis = Basis::sphere(&sheared(), k, 6.0);
        let pairs = basis.pairs(k).unwrap();
        let n = basis.millers().len();
        let x = Mat::from_fn(n, 2, |g, j| ((5 * g + 2 * j) % 13) as f64 - 6.1);
        let c = pairs.complex(x.as_ref());
        let packed: Vec<c64> = (0..n)
            .map(|g| pairs.packed(|row| x[(row, 0)], |row| x[(row, 1)], g))
            .collect();
        for g in 0..n {
            let want = c[(g, 0)] + c64::new(0.0, 1.0) * c[(g, 1)];
            assert!((packed[g] - want).norm() < 1e-14, "{k:?} row {g}");
            let [a, b] = pairs.halves(&packed, g);
            assert!(
                (a - c[(g, 0)]).norm() < 1e-14 && (b - c[(g, 1)]).norm() < 1e-14,
                "{k:?} row {g}"
            );
        }
    }
}

#[test]
fn the_real_form_keeps_each_inner_product_and_gives_its_columns_back() {
    let (cell, k) = (sheared(), [0.5, 0.0, 0.5]);
    let basis = Basis::sphere(&cell, k, 6.0);
    let pairs = basis.pairs(k).unwrap();
    let n = basis.millers().len();
    let x = Mat::from_fn(n, 2, |g, j| {
        ((7 * g + 3 * j) % 11) as f64 - 4.6 + 0.1 * j as f64
    });
    let c = pairs.complex(x.as_ref());
    let back = pairs.real(c.as_ref());
    for j in 0..2 {
        for g in 0..n {
            assert!((back[(g, j)] - x[(g, j)]).abs() < 1e-14, "{g} {j}");
        }
    }
    for (a, b) in [(0, 0), (0, 1), (1, 1)] {
        let complex: c64 = (0..n).map(|g| c[(g, a)].conj() * c[(g, b)]).sum();
        let real: f64 = (0..n).map(|g| x[(g, a)] * x[(g, b)]).sum();
        assert!((complex - c64::new(real, 0.0)).norm() < 1e-12 * real.abs().max(1.0));
    }
}
