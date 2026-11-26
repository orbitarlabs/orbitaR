use orbitar::model::Cell;
use std::f64::consts::TAU;

const FCC: [[f64; 3]; 3] = [[0.0, 5.13, 5.13], [5.13, 0.0, 5.13], [5.13, 5.13, 0.0]];
const TILTED: [[f64; 3]; 3] = [[4.0, 0.3, -0.2], [1.1, 5.0, 0.4], [-0.7, 0.9, 6.0]];

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

#[test]
fn reciprocal_vectors_satisfy_a_dot_b_equals_two_pi_delta() {
    for rows in [FCC, TILTED, [TILTED[1], TILTED[0], TILTED[2]]] {
        let b = Cell::try_from(rows).unwrap().reciprocal();
        for (i, a) in rows.iter().enumerate() {
            for (j, b) in b.iter().enumerate() {
                let want = if i == j { TAU } else { 0.0 };
                assert!((dot(*a, *b) - want).abs() < 1e-12, "a{i}·b{j}");
            }
        }
    }
}

#[test]
fn fcc_volume_is_a_cubed_over_four() {
    let a = 2.0 * 5.13;
    assert!((Cell::try_from(FCC).unwrap().volume() - a * a * a / 4.0).abs() < 1e-12);
}

#[test]
fn a_left_handed_cell_has_the_same_positive_volume() {
    let swapped = [TILTED[1], TILTED[0], TILTED[2]];
    let (right, left) = (
        Cell::try_from(TILTED).unwrap(),
        Cell::try_from(swapped).unwrap(),
    );
    assert!(left.volume() > 0.0);
    assert!((left.volume() - right.volume()).abs() < 1e-12);
}

#[test]
fn coplanar_or_non_finite_lattice_vectors_are_rejected() {
    let flat = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]];
    let nan = [[f64::NAN, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for rows in [flat, nan, [[0.0; 3]; 3]] {
        assert!(
            Cell::try_from(rows)
                .unwrap_err()
                .contains("structure.cell_bohr")
        );
    }
}

#[test]
fn flatness_depends_on_the_angles_and_not_on_the_lengths() {
    let scaled = |rows: [[f64; 3]; 3], s: f64| rows.map(|a| a.map(|x| x * s));
    let nearly_coplanar = [[100.0, 0.0, 0.0], [0.0, 100.0, 0.0], [100.0, 100.0, 1e-8]];
    let one_short_axis = [[100.0, 0.0, 0.0], [0.0, 100.0, 0.0], [0.0, 0.0, 1e-6]];
    for s in [1.0, 1e-3] {
        assert!(Cell::try_from(scaled(nearly_coplanar, s)).is_err());
        assert!(Cell::try_from(scaled(one_short_axis, s)).is_ok());
    }
}

#[test]
fn a_fractional_point_maps_to_its_cartesian_position() {
    let cartesian = Cell::try_from(FCC).unwrap().cartesian([0.5; 3]);
    assert!(
        cartesian.iter().all(|x| (x - 5.13).abs() < 1e-12),
        "{cartesian:?}"
    );
    let tilted = Cell::try_from(TILTED).unwrap().cartesian([0.0, 1.0, 0.0]);
    assert_eq!(tilted, TILTED[1]);
}

#[test]
fn the_reach_of_a_radius_is_the_radius_over_the_plane_spacing_of_each_axis() {
    let cell = Cell::try_from([[2.0, 0.0, 0.0], [1.0, 2.0, 0.0], [0.0, 0.0, 3.0]]).unwrap();
    let want = [3.0 * 45.0_f64.sqrt() / 12.0, 1.5, 1.0];
    for (reach, want) in cell.reach(3.0).iter().zip(want) {
        assert!((reach - want).abs() < 1e-14, "{reach} for {want}");
    }
}

#[test]
fn the_searches_find_every_point_within_the_radius_and_no_other() {
    let cell = Cell::try_from([[4.0, 0.0, 0.0], [3.0, 3.0, 0.0], [3.0, 3.0, 3.0]]).unwrap();
    let box_points = || {
        (-15..=15_i32).flat_map(|i| {
            (-15..=15).flat_map(move |j| (-15..=15).map(move |l| [i, j, l].map(f64::from)))
        })
    };
    let length = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let (f, k) = ([0.3, -0.7, 0.2], [0.75, -0.9, 0.95]);
    let shifted =
        |c: [f64; 3]| move |n: [f64; 3]| std::array::from_fn::<f64, 3, _>(|i| n[i] + c[i]);
    let real = box_points()
        .map(shifted(f))
        .filter(|p| length(cell.cartesian(*p)) <= 9.0)
        .count();
    let reciprocal = box_points()
        .map(shifted(k))
        .filter(|q| length(cell.wavevector(*q)) <= 6.0)
        .count();
    let translates = cell.translates_within(f, 9.0);
    let wavevectors = cell.wavevectors_within(k, 6.0);
    assert_eq!((translates.len(), wavevectors.len()), (real, reciprocal));
    assert!(
        translates
            .iter()
            .all(|&(p, r)| (length(cell.cartesian(p)) - r).abs() < 1e-12 && r <= 9.0)
    );
    assert!(
        wavevectors
            .iter()
            .all(|&(q, g)| (length(cell.wavevector(q)) - g).abs() < 1e-12 && g <= 6.0)
    );
}

#[test]
fn an_upright_cell_keeps_every_length_and_angle_and_ignores_a_rotation() {
    let (axis, angle) = ([1.0 / 3.0, 2.0 / 3.0, 2.0 / 3.0], 0.7_f64);
    let (c, s) = (angle.cos(), angle.sin());
    let turn = |v: [f64; 3]| -> [f64; 3] {
        let k = dot(axis, v);
        let x = [
            axis[1] * v[2] - axis[2] * v[1],
            axis[2] * v[0] - axis[0] * v[2],
            axis[0] * v[1] - axis[1] * v[0],
        ];
        std::array::from_fn(|i| v[i] * c + x[i] * s + axis[i] * k * (1.0 - c))
    };
    let cell = Cell::try_from(TILTED).unwrap();
    let up = cell.upright();
    let turned = Cell::try_from(TILTED.map(turn)).unwrap().upright();
    let rows = |c: &Cell| {
        [0, 1, 2].map(|i| c.cartesian(std::array::from_fn(|j| f64::from(u8::from(i == j)))))
    };
    let (a, b, t) = (rows(&cell), rows(&up), rows(&turned));
    assert!(b[0][1] == 0.0 && b[0][2] == 0.0 && b[0][0] > 0.0 && b[1][2] == 0.0 && b[1][1] > 0.0);
    for i in 0..3 {
        for j in 0..3 {
            assert!((dot(a[i], a[j]) - dot(b[i], b[j])).abs() < 1e-12, "{i} {j}");
            assert!((b[i][j] - t[i][j]).abs() < 1e-13, "{i} {j}");
        }
    }
    assert!((up.volume() - cell.volume()).abs() < 1e-12);
}

#[test]
fn the_frame_is_the_proper_rotation_onto_the_upright_cell() {
    let cell = Cell::try_from(TILTED).unwrap();
    let frame = cell.frame();
    let up = cell.upright();
    let unit = |i: usize| -> [f64; 3] { std::array::from_fn(|j| f64::from(u8::from(i == j))) };
    for i in 0..3 {
        for j in 0..3 {
            let want = f64::from(u8::from(i == j));
            assert!((dot(frame[i], frame[j]) - want).abs() < 1e-15, "{i} {j}");
        }
        let a = cell.cartesian(unit(i));
        let turned: [f64; 3] = std::array::from_fn(|j| dot(frame[j], a));
        let row = up.cartesian(unit(i));
        assert!(
            (0..3).all(|j| (turned[j] - row[j]).abs() < 1e-14),
            "{i}: {turned:?} vs {row:?}"
        );
    }
    let [e1, e2, e3] = frame;
    let cross = [
        e1[1] * e2[2] - e1[2] * e2[1],
        e1[2] * e2[0] - e1[0] * e2[2],
        e1[0] * e2[1] - e1[1] * e2[0],
    ];
    assert!((0..3).all(|j| (cross[j] - e3[j]).abs() < 1e-15));
}

#[test]
fn a_wavevector_from_held_reciprocal_rows_is_the_wavevector_bit_for_bit() {
    let cell = Cell::try_from(TILTED).unwrap();
    for m in [[1.0, -2.0, 3.0], [0.0, 0.0, 0.0], [-4.0, 5.0, 0.5]] {
        assert_eq!(
            Cell::wavevector_in(cell.reciprocal(), m),
            cell.wavevector(m)
        );
    }
    assert_ne!(
        Cell::wavevector_in(cell.reciprocal(), [1.0, 0.0, 0.0]),
        [0.0; 3]
    );
}

#[test]
fn the_frame_turns_with_a_rigid_rotation_of_a_cell_within_its_conditioning() {
    let rows = [[1e5, 1.0, 0.0], [1e5, 2.0, 0.0], [0.0, 0.0, 1.0]];
    let frame = Cell::try_from(rows).unwrap().frame();
    let mut worst = 0.0_f64;
    for t in 0..64 {
        let t = f64::from(t);
        let axis = [
            (1.3 * t).sin(),
            (0.7 * t + 0.4).cos(),
            0.5 + (0.9 * t).sin(),
        ];
        let norm = dot(axis, axis).sqrt();
        let axis = axis.map(|x| x / norm);
        let (c, s) = ((0.37 * t + 0.11).cos(), (0.37 * t + 0.11).sin());
        let turn = |v: [f64; 3]| -> [f64; 3] {
            let k = dot(axis, v);
            let x = [
                axis[1] * v[2] - axis[2] * v[1],
                axis[2] * v[0] - axis[0] * v[2],
                axis[0] * v[1] - axis[1] * v[0],
            ];
            std::array::from_fn(|i| v[i] * c + x[i] * s + axis[i] * k * (1.0 - c))
        };
        let turned = Cell::try_from(rows.map(turn)).unwrap().frame();
        for (e, f) in frame.iter().zip(&turned) {
            let want = turn(*e);
            worst = worst.max((0..3).map(|i| (want[i] - f[i]).abs()).fold(0.0, f64::max));
        }
    }
    assert!(worst <= 2.5e-11, "{worst:e}");
}
