use orbitar::ks::{Failure, ewald_energy_ha, ewald_forces_ha_bohr, ewald_stress_ha_bohr3};
use orbitar::model::Cell;

#[expect(
    clippy::disallowed_macros,
    reason = "the test reads the case's own reference file"
)]
pub(super) fn madelung() -> [f64; 3] {
    let text = include_str!("../../benchmarks/2_ewald_madelung/reference/madelung.json");
    let table: std::collections::HashMap<String, f64> = serde_json::from_str(text).unwrap();
    ["NaCl", "CsCl", "ZnS_zincblende"].map(|key| table[key])
}

const SIMPLE_CUBIC_ALPHA: f64 = 2.8373;

fn pair(rows: [[f64; 3]; 3], second: [f64; 3], d: f64, s: f64, q: f64) -> (f64, f64) {
    let cell = Cell::try_from(rows.map(|row| row.map(|x| x * s))).unwrap();
    (
        ewald_energy_ha(&cell, &[(q, [0.0; 3]), (-q, second)]).unwrap(),
        d * s,
    )
}

const FCC: [[f64; 3]; 3] = [[0.0, 1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 0.0]];
const CUBE: [[f64; 3]; 3] = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];

#[test]
fn rock_salt_cesium_chloride_and_zincblende_give_their_madelung_constants_at_any_size() {
    let half_diagonal = 3.0_f64.sqrt() / 2.0;
    let crystals = [
        (FCC, [0.5; 3], 1.0),
        (CUBE, [0.5; 3], half_diagonal),
        (FCC, [0.25; 3], half_diagonal),
    ];
    for ((rows, second, d), want) in crystals.into_iter().zip(madelung()) {
        for s in [0.3, 1.0, 3.0] {
            let (energy, d) = pair(rows, second, d, s, 1.0);
            assert!(
                (-energy * d - want).abs() < 5e-13,
                "{rows:?} × {s}: {}",
                -energy * d
            );
        }
    }
}

#[test]
fn a_skewed_basis_of_the_same_crystal_gives_the_same_energy() {
    let skewed = [FCC[0], [1.0, 2.0, 3.0], [1.0, 4.0, 3.0]];
    let (energy, _) = pair(skewed, [-2.0, 0.5, 0.5], 1.0, 1.0, 1.0);
    assert!((-energy - madelung()[0]).abs() < 5e-13, "{}", -energy);
}

#[test]
fn moving_every_charge_by_the_same_offset_changes_nothing() {
    let cell = Cell::try_from(FCC).unwrap();
    let at = |offset: [f64; 3]| {
        let shifted = |f: [f64; 3]| std::array::from_fn(|k| f[k] + offset[k]);
        ewald_energy_ha(
            &cell,
            &[(1.0, shifted([0.0; 3])), (-1.0, shifted([0.5; 3]))],
        )
        .unwrap()
    };
    assert!((at([0.0; 3]) - at([0.3, -0.7, 1.9])).abs() < 1e-12);
}

const OFF_ORIGIN_PAIR: f64 = -0.8394214335408896;

#[test]
fn a_pair_away_from_the_origin_has_the_independent_ewald_energy() {
    let cell = Cell::try_from(CUBE.map(|r| r.map(|x| 8.0 * x))).unwrap();
    let energy = ewald_energy_ha(&cell, &[(1.0, [0.3; 3]), (-1.0, [0.45, 0.3, 0.3])]).unwrap();
    assert!((energy - OFF_ORIGIN_PAIR).abs() < 1e-10, "{energy}");
}

#[test]
fn the_energy_scales_as_the_charge_squared() {
    let (one, _) = pair(FCC, [0.5; 3], 1.0, 1.0, 1.0);
    let (two, _) = pair(FCC, [0.5; 3], 1.0, 1.0, 2.0);
    assert!((two - 4.0 * one).abs() < 1e-12);
}

#[test]
fn a_charge_on_a_uniform_background_has_the_makov_payne_energy() {
    for a in [2.0, 5.0] {
        let cell = Cell::try_from(CUBE.map(|row| row.map(|x| x * a))).unwrap();
        let energy = ewald_energy_ha(&cell, &[(1.0, [0.0; 3])]).unwrap();
        assert!(
            (energy * 2.0 * a + SIMPLE_CUBIC_ALPHA).abs() < 1e-4,
            "{a}: {energy}"
        );
    }
}

#[test]
fn any_basis_of_one_lattice_gives_one_energy() {
    let energy = |rows: [[f64; 3]; 3]| {
        ewald_energy_ha(&Cell::try_from(rows).unwrap(), &[(1.0, [0.0; 3])]).unwrap()
    };
    let pairs = [
        (
            [[1.0, 0.0, 0.0], [-0.2, 0.4, 0.0], [0.0, 0.0, 1.0]],
            [[1.0, 0.0, 0.0], [0.8, 0.4, 0.0], [0.0, 0.0, 1.0]],
        ),
        (FCC, [[1.0, 1.0, 2.0], [2.0, 1.0, 1.0], [2.0, 2.0, 2.0]]),
    ];
    for (reduced, other) in pairs {
        assert!((energy(reduced) - energy(other)).abs() < 1e-13, "{other:?}");
    }
}

#[test]
fn the_conventional_rock_salt_cell_holds_four_primitive_energies() {
    let primitive = pair(FCC, [0.5; 3], 1.0, 1.0, 1.0).0;
    let corners = [
        [0.0, 0.0, 0.0],
        [0.0, 0.5, 0.5],
        [0.5, 0.0, 0.5],
        [0.5, 0.5, 0.0],
    ];
    let charges: Vec<(f64, [f64; 3])> = corners
        .iter()
        .flat_map(|&c| [(1.0, c), (-1.0, c.map(|x| x + 0.5))])
        .collect();
    let conventional = ewald_energy_ha(
        &Cell::try_from(CUBE.map(|r| r.map(|x| 2.0 * x))).unwrap(),
        &charges,
    )
    .unwrap();
    assert!(
        (conventional - 4.0 * primitive).abs() < 1e-12,
        "{conventional} vs {primitive}"
    );
}

#[test]
#[should_panic(expected = "two charges share a site")]
fn two_charges_on_one_site_are_refused() {
    let cell = Cell::try_from(CUBE).unwrap();
    ewald_energy_ha(&cell, &[(1.0, [0.0; 3]), (-1.0, [1.0, 0.0, 0.0])]).unwrap();
}

#[test]
fn a_cell_fourteen_times_the_closest_pair_is_unsupported_before_its_g_list() {
    let cell = Cell::try_from(CUBE.map(|row| row.map(|x| 2.0 * x))).unwrap();
    let pair = [(1.0, [0.0; 3]), (-1.0, [1.0 / 14.0, 0.0, 0.0])];
    for refused in [
        ewald_energy_ha(&cell, &pair).err(),
        ewald_forces_ha_bohr(&cell, &pair).err(),
    ] {
        assert!(
            matches!(&refused, Some(Failure::Unsupported(m)) if m.contains("2.6e7 G vectors")),
            "{refused:?}"
        );
    }
}

const ELEVENTH_PAIRS: [(f64, f64); 2] = [(2.0, -5.508760842685888), (3.0, -3.672507228457259)];

#[test]
fn a_cell_eleven_times_the_closest_pair_has_the_independent_ewald_energy() {
    let pair = [(1.0, [0.0; 3]), (-1.0, [1.0 / 11.0, 0.0, 0.0])];
    for (side, want) in ELEVENTH_PAIRS {
        let cell = Cell::try_from(CUBE.map(|row| row.map(|x| side * x))).unwrap();
        let energy = ewald_energy_ha(&cell, &pair).unwrap();
        assert!((energy - want).abs() < 1e-13, "L = {side}: {energy}");
    }
}

#[test]
fn ewald_forces_are_minus_the_gradient_of_the_energy() {
    let cell = Cell::try_from([[5.0, 0.3, 0.0], [0.4, 6.0, 0.2], [0.0, -0.5, 5.5]]).unwrap();
    let apart = [
        (3.0, [0.1, 0.2, 0.3]),
        (5.0, [0.55, 0.45, 0.7]),
        (1.0, [0.8, 0.9, 0.05]),
    ];
    let close = [
        (3.0, [0.1, 0.2, 0.3]),
        (-2.0, [0.3, 0.2, 0.3]),
        (1.0, [0.8, 0.9, 0.05]),
    ];
    for charges in [apart, close] {
        let forces = ewald_forces_ha_bohr(&cell, &charges).unwrap();
        let b = cell.reciprocal();
        let h = 1e-3;
        for (a, force) in forces.iter().enumerate() {
            for axis in 0..3 {
                let e = |t: f64| {
                    let mut moved = charges;
                    for (x, row) in moved[a].1.iter_mut().zip(&b) {
                        *x += t * h * row[axis] / std::f64::consts::TAU;
                    }
                    ewald_energy_ha(&cell, &moved).unwrap()
                };
                let fd = -(8.0 * (e(1.0) - e(-1.0)) - (e(2.0) - e(-2.0))) / (12.0 * h);
                assert!(
                    (force[axis] - fd).abs() < 5e-9 * (1.0 + fd.abs()),
                    "{a} {axis}: {} vs {fd}",
                    force[axis]
                );
            }
        }
    }
}

#[test]
fn the_ewald_stress_is_the_strain_derivative_of_the_energy_per_volume() {
    let rows = [[5.0, 0.3, 0.0], [0.4, 6.0, 0.2], [0.0, -0.5, 5.5]];
    let charges = [
        (3.0, [0.1, 0.2, 0.3]),
        (5.0, [0.55, 0.45, 0.7]),
        (-1.0, [0.8, 0.9, 0.05]),
    ];
    let e = [[0.3, 0.5, -0.2], [0.5, -0.4, 0.6], [-0.2, 0.6, 0.1]];
    let cell = |t: f64| {
        let moved: [[f64; 3]; 3] = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                rows[r][c] + t * (0..3).map(|k| e[c][k] * rows[r][k]).sum::<f64>()
            })
        });
        Cell::try_from(moved).unwrap()
    };
    let stress = ewald_stress_ha_bohr3(&cell(0.0), &charges).unwrap();
    let energy = |t: f64| ewald_energy_ha(&cell(t), &charges).unwrap();
    let h = 1e-4;
    let fd = (8.0 * (energy(h) - energy(-h)) - (energy(2.0 * h) - energy(-2.0 * h))) / (12.0 * h);
    let along: f64 = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| stress[i][j] * e[i][j])
        .sum::<f64>()
        * cell(0.0).volume();
    assert!(along.abs() > 0.1, "{along}");
    assert!(
        (along - fd).abs() < 1e-9 * (1.0 + fd.abs()),
        "{along} vs {fd}"
    );
    for i in [0, 1, 2] {
        for j in [0, 1, 2] {
            assert_eq!(stress[i][j], stress[j][i]);
        }
    }
}
