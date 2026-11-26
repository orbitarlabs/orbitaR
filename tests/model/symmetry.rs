use orbitar::model::{Cell, Operation, operations};

fn cell(rows: [[f64; 3]; 3]) -> Cell {
    Cell::try_from(rows).unwrap()
}

fn fcc() -> Cell {
    let h = 10.26 / 2.0;
    cell([[0.0, h, h], [h, 0.0, h], [h, h, 0.0]])
}

const DIAMOND: [(u8, [f64; 3]); 2] = [(14, [0.0; 3]), (14, [0.25; 3])];

#[test]
fn opposite_moments_halve_the_operations_of_bcc() {
    let cubic = cell([[5.35, 0.0, 0.0], [0.0, 5.35, 0.0], [0.0, 0.0, 5.35]]);
    let pair = [(26, [0.0; 3]), (26, [0.5; 3])];
    let bcc = operations(&cubic, &pair, None).unwrap();
    assert_eq!(bcc.len(), 96);
    let afm = operations(&cubic, &pair, Some(&[2.5, -2.5])).unwrap();
    assert_eq!(afm.len(), 48);
    assert!(afm.iter().all(|o| o.translation == [0.0; 3]));
}

#[test]
fn each_operation_names_the_atom_it_maps_onto_each_atom() {
    let cubic = cell([[5.35, 0.0, 0.0], [0.0, 5.35, 0.0], [0.0, 0.0, 5.35]]);
    for (cell, atoms) in [
        (cubic, vec![(6, [0.1, 0.2, 0.3]), (6, [0.9, 0.8, 0.7])]),
        (fcc(), DIAMOND.to_vec()),
    ] {
        for o in operations(&cell, &atoms, None).unwrap() {
            for (a, &b) in o.preimages.iter().enumerate() {
                let x = atoms[b].1;
                let image: [f64; 3] = std::array::from_fn(|i| {
                    (0..3)
                        .map(|j| f64::from(o.rotation[i][j]) * x[j])
                        .sum::<f64>()
                        + o.translation[i]
                });
                let d = (0..3).map(|i| image[i] - atoms[a].1[i]);
                assert!(d.into_iter().all(|d| (d - d.round()).abs() < 1e-9), "{o:?}");
            }
        }
    }
}

#[test]
fn an_atom_just_off_its_site_keeps_the_operations_moyo_finds_at_a_narrower_tolerance() {
    let cubic = cell([[5.35, 0.0, 0.0], [0.0, 5.35, 0.0], [0.0, 0.0, 5.35]]);
    let pair = [(26, [0.0; 3]), (26, [0.5 + 1.1e-5 / 5.35, 0.5, 0.5])];
    assert_eq!(operations(&cubic, &pair, None).unwrap().len(), 16);
}

#[test]
fn atoms_on_one_site_have_no_space_group() {
    let err = operations(&fcc(), &[(14, [0.0; 3]), (14, [0.0; 3])], None).unwrap_err();
    assert!(err.starts_with("structure: no space group"), "{err}");
}

#[test]
fn a_grid_keeps_the_operations_whose_translation_and_rotation_land_on_its_points() {
    let diamond = operations(&fcc(), &DIAMOND, None).unwrap();
    assert_eq!(diamond.len(), 48);
    let kept = |dims| diamond.iter().filter(|o| o.keeps_grid(dims)).count();
    assert_eq!((kept([24; 3]), kept([18; 3])), (48, 24));
    let tetragonal = cell([[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [0.0, 0.0, 7.0]]);
    let square = operations(&tetragonal, &[(6, [0.0; 3])], None).unwrap();
    let kept = |dims| square.iter().filter(|o| o.keeps_grid(dims)).count();
    assert_eq!((kept([12, 12, 15]), kept([12, 15, 15])), (16, 8));
}

#[test]
fn a_translation_less_than_a_millionth_of_a_step_off_a_grid_point_lands_on_it() {
    let shifted = |t: f64| Operation {
        rotation: [[1, 0, 0], [0, 1, 0], [0, 0, 1]],
        translation: [t, 0.0, 0.0],
        cartesian: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
        preimages: vec![0],
    };
    assert!(shifted(0.999e-6).keeps_grid([1; 3]));
    assert!(!shifted(1e-6).keeps_grid([1; 3]));
}

#[test]
fn the_cartesian_rotation_of_the_3_fold_axis_of_fcc_cycles_x_y_z() {
    let diamond = operations(&fcc(), &DIAMOND, None).unwrap();
    let cycle = (diamond.iter())
        .find(|o| o.rotation == [[0, 0, 1], [1, 0, 0], [0, 1, 0]] && o.translation == [0.0; 3])
        .unwrap();
    let want = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]];
    for (row, expected) in cycle.cartesian.iter().zip(&want) {
        for (x, y) in row.iter().zip(expected) {
            assert!((x - y).abs() < 1e-12, "{:?}", cycle.cartesian);
        }
    }
}

#[test]
fn an_isometry_is_the_operations_rotation_made_orthogonal() {
    let gap = |a: [[f64; 3]; 3], b: [[f64; 3]; 3]| {
        (0..3)
            .flat_map(|i| (0..3).map(move |j| (a[i][j] - b[i][j]).abs()))
            .fold(0.0, f64::max)
    };
    let gram = |w: [[f64; 3]; 3]| -> [[f64; 3]; 3] {
        std::array::from_fn(|i| std::array::from_fn(|j| (0..3).map(|k| w[k][i] * w[k][j]).sum()))
    };
    let unit = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for o in operations(&fcc(), &DIAMOND, None).unwrap() {
        assert!(gap(o.isometry(), o.cartesian) < 1e-15);
    }
    let strained = cell([
        [6.0, 0.0, 0.0],
        [0.0, 6.0, 0.0],
        [0.0, 0.0, 6.0 * (1.0 + 4e-7)],
    ]);
    let ops: Vec<Operation> = operations(&strained, &[(6, [0.0; 3])], None).unwrap();
    assert_eq!(ops.len(), 48);
    assert!(ops.iter().any(|o| gap(gram(o.cartesian), unit) > 5e-7));
    for o in &ops {
        assert!(gap(gram(o.isometry()), unit) < 1e-15);
        assert!(gap(o.isometry(), o.cartesian) < 1e-6);
    }
}
