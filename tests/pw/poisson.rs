use orbitar::model::Cell;
use orbitar::pw::{Grid, hartree};
use std::f64::consts::{PI, TAU};

const TILTED: [[f64; 3]; 3] = [[4.0, 0.3, -0.2], [1.1, 5.0, 0.4], [-0.7, 0.9, 6.0]];

fn cosines(grid: &Grid, waves: &[([f64; 3], f64)]) -> Vec<f64> {
    let phase = |m: [f64; 3], f: &[f64; 3]| TAU * (m[0] * f[0] + m[1] * f[1] + m[2] * f[2]);
    grid.points()
        .iter()
        .map(|f| waves.iter().map(|&(m, c)| c * phase(m, f).cos()).sum())
        .collect()
}

fn g2(cell: &Cell, m: [f64; 3]) -> f64 {
    cell.wavevector(m).iter().map(|x| x * x).sum()
}

#[test]
fn a_sum_of_cosines_has_the_hartree_energy_pi_v_c_squared_over_g_squared() {
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [2.0, 8.0, 0.0], [1.0, -1.5, 11.0]]).unwrap();
    let grid = Grid::coarse(&cell, 3.0);
    let waves = [
        ([1.0, 0.0, 0.0], 0.7),
        ([0.0, 2.0, -1.0], 0.4),
        ([2.0, -1.0, 3.0], 0.2),
        ([-3.0, 2.0, 1.0], 0.1),
    ];
    let h = hartree(&grid, &cosines(&grid, &waves), 3.0);
    let want: f64 = waves
        .iter()
        .map(|&(m, c)| PI * cell.volume() * c * c / g2(&cell, m))
        .sum();
    assert!((h.energy_ha - want).abs() < 1e-12 * want);
    let scaled: Vec<_> = waves
        .iter()
        .map(|&(m, c)| (m, 4.0 * PI * c / g2(&cell, m)))
        .collect();
    for (got, want) in h.potential.iter().zip(cosines(&grid, &scaled)) {
        assert!((got - want).abs() < 1e-12);
    }
}

#[test]
fn a_uniform_density_has_no_hartree_energy() {
    let grid = Grid::coarse(&Cell::try_from(TILTED).unwrap(), 2.0);
    let density = vec![0.3; grid.points().len()];
    let h = hartree(&grid, &density, 2.0);
    assert!(h.energy_ha.abs() < 1e-14);
    assert!(h.potential.iter().all(|v| v.abs() < 1e-14));
}

#[test]
fn waves_outside_the_density_sphere_carry_no_hartree_energy() {
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]).unwrap();
    let grid = Grid::coarse(&cell, 3.0);
    let inside = ([1.0, 1.0, 0.0], 0.5);
    let corner = ([4.0, 4.0, 3.0], 0.3);
    assert!(g2(&cell, inside.0) < 24.0 && g2(&cell, corner.0) > 24.0);
    assert!(grid.dims().iter().all(|&n| n as f64 / 2.0 > 4.0));
    let both = hartree(&grid, &cosines(&grid, &[inside, corner]), 3.0);
    let alone = hartree(&grid, &cosines(&grid, &[inside]), 3.0);
    assert!((both.energy_ha - alone.energy_ha).abs() < 1e-14);
    assert!((both.energy_ha - PI * cell.volume() * 0.25 / g2(&cell, inside.0)).abs() < 1e-12);
}
