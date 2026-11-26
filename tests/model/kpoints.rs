use orbitar::model::{Cell, KPoints, operations};

fn fcc() -> Cell {
    let h = 10.26 / 2.0;
    Cell::try_from([[0.0, h, h], [h, 0.0, h], [h, h, 0.0]]).unwrap()
}

#[test]
fn a_mesh_lists_gamma_centered_points_with_the_last_axis_fastest() {
    let k = KPoints::mesh([2, 1, 3], [0.0; 3]).unwrap();
    let third = 1.0 / 3.0;
    let want = [
        [0.0, 0.0, 0.0],
        [0.0, 0.0, third],
        [0.0, 0.0, 2.0 * third],
        [0.5, 0.0, 0.0],
        [0.5, 0.0, third],
        [0.5, 0.0, 2.0 * third],
    ];
    assert_eq!(k.points(), want);
}

#[test]
fn a_shift_moves_each_point_by_shift_over_divisions() {
    let k = KPoints::mesh([2, 2, 4], [0.5, 0.0, 0.25]).unwrap();
    assert_eq!(k.points().len(), 16);
    assert_eq!(k.points()[0], [0.25, 0.0, 0.0625]);
    assert_eq!(k.points()[15], [0.75, 0.5, 0.8125]);
}

#[test]
fn weights_are_equal_and_sum_to_one() {
    for k in [
        KPoints::mesh([3, 3, 3], [0.0; 3]).unwrap(),
        KPoints::list(vec![[0.0; 3], [0.5, 0.0, 0.5]]).unwrap(),
    ] {
        let n = k.points().len() as f64;
        assert!(k.weights().iter().all(|&w| w == 1.0 / n));
    }
}

#[test]
fn a_mesh_rejects_zero_divisions_and_a_shift_outside_zero_to_one() {
    for (divisions, shift) in [
        ([0, 1, 1], [0.0; 3]),
        ([1; 3], [1.0, 0.0, 0.0]),
        ([1; 3], [-0.1, 0.0, 0.0]),
    ] {
        assert!(
            KPoints::mesh(divisions, shift)
                .unwrap_err()
                .contains("kpoints.")
        );
    }
}

#[test]
fn a_list_keeps_its_points_in_order() {
    let points = vec![[0.0; 3], [-1.0, 0.5, 1.0]];
    assert_eq!(KPoints::list(points.clone()).unwrap().points(), points);
}

#[test]
fn a_list_rejects_an_empty_list_or_a_point_beyond_one() {
    for bad in [vec![], vec![[1.5, 0.0, 0.0]], vec![[f64::NAN, 0.0, 0.0]]] {
        assert!(KPoints::list(bad).unwrap_err().contains("kpoints.list"));
    }
}

#[test]
fn the_cubic_group_and_time_reversal_leave_8_of_a_4x4x4_mesh() {
    let rotations = operations(&fcc(), &[(14, [0.0; 3]), (14, [0.25; 3])], None)
        .unwrap()
        .iter()
        .map(|o| o.rotation)
        .collect::<Vec<_>>();
    assert_eq!(rotations.len(), 48);
    for (n, count) in [(4, 8), (8, 29)] {
        let mesh = KPoints::mesh([n; 3], [0.0; 3]).unwrap();
        assert!(rotations.iter().all(|r| mesh.keeps(r)));
        let reduced = mesh.irreducible(&rotations);
        assert_eq!(reduced.points().len(), count);
        assert!((reduced.weights().iter().sum::<f64>() - 1.0).abs() < 1e-14);
        assert_eq!(reduced.points()[0], [0.0; 3]);
        assert_eq!(reduced.weights()[0], 1.0 / (n * n * n) as f64);
    }
}

#[test]
fn the_cubic_group_and_time_reversal_leave_4_of_a_3x3x3_bcc_mesh() {
    let h = 5.35 / 2.0;
    let bcc = Cell::try_from([[-h, h, h], [h, -h, h], [h, h, -h]]).unwrap();
    let rotations = operations(&bcc, &[(26, [0.0; 3])], None)
        .unwrap()
        .iter()
        .map(|o| o.rotation)
        .collect::<Vec<_>>();
    assert_eq!(rotations.len(), 48);
    let reduced = KPoints::mesh([3; 3], [0.0; 3])
        .unwrap()
        .irreducible(&rotations);
    assert_eq!(reduced.points().len(), 4);
    assert!((reduced.weights().iter().sum::<f64>() - 1.0).abs() < 1e-14);
}

#[test]
fn a_shifted_mesh_keeps_only_the_rotations_that_map_it_onto_itself() {
    let mesh = KPoints::mesh([2, 2, 2], [0.5, 0.0, 0.0]).unwrap();
    assert!(mesh.keeps(&[[1, 0, 0], [0, 1, 0], [0, 0, 1]]));
    assert!(!mesh.keeps(&[[0, 1, 0], [1, 0, 0], [0, 0, 1]]));
}

#[test]
fn time_reversal_reduces_only_a_mesh_that_holds_each_minus_k() {
    let identity = [[[1, 0, 0], [0, 1, 0], [0, 0, 1]]];
    let centered = KPoints::mesh([4; 3], [0.0; 3])
        .unwrap()
        .irreducible(&identity);
    assert_eq!(centered.points().len(), 8 + (64 - 8) / 2);
    let quarter = KPoints::mesh([4, 1, 1], [0.25, 0.0, 0.0]).unwrap();
    assert_eq!(quarter.irreducible(&identity).points(), quarter.points());
    let half = KPoints::mesh([4, 1, 1], [0.5, 0.0, 0.0])
        .unwrap()
        .irreducible(&identity);
    assert_eq!(half.points(), [[0.125, 0.0, 0.0], [0.375, 0.0, 0.0]]);
    assert_eq!(half.weights(), [0.5, 0.5]);
}

#[test]
fn reduced_weights_keep_their_sum() {
    let mesh = KPoints::mesh([3, 2, 1], [0.0; 3]).unwrap();
    let same = mesh.irreducible(&[]);
    assert_eq!(
        (same.points(), same.weights()),
        (mesh.points(), mesh.weights())
    );
    let twice = KPoints::list(vec![[0.0; 3], [-1e-12, 0.0, 0.0]]).unwrap();
    let folded = twice.irreducible(&[[[1, 0, 0], [0, 1, 0], [0, 0, 1]]]);
    assert_eq!(
        (folded.points(), folded.weights()),
        (&[[0.0; 3]][..], &[1.0][..])
    );
}

#[test]
fn a_mesh_may_be_reduced_and_a_list_is_used_as_given() {
    let mesh = KPoints::mesh([2, 2, 2], [0.0; 3]).unwrap();
    assert!(mesh.reducible() && !mesh.clone().as_given().reducible());
    assert!(!mesh.irreducible(&[]).reducible());
    assert!(!KPoints::list(vec![[0.0; 3]]).unwrap().reducible());
    assert_eq!(mesh.clone().as_given().points(), mesh.points());
}
