use compensated_summation::KahanBabuskaNeumaier;
use orbitar::model::{Cell, Functional};
use orbitar::pw::{
    Grid, Kinetic, Spins, Xc, exchange_correlation, grid_defect, grid_energy, grid_gap, grid_points,
};
use orbitar::xc::{Point, evaluate, unpolarized};
use std::f64::consts::TAU;

fn grid() -> Grid {
    let cell = Cell::try_from([[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]]).unwrap();
    Grid::coarse(&cell, 4.0)
}

fn density(grid: &Grid, scale: f64) -> Vec<f64> {
    grid.points()
        .iter()
        .map(|f| {
            scale
                * (0.05
                    + 0.02 * (TAU * (f[0] + f[1])).cos()
                    + 0.015 * (TAU * (2.0 * f[2] - f[0]) + 0.3).sin())
        })
        .collect()
}

#[test]
fn a_uniform_density_gives_one_energy_on_eight_points_and_on_ninety_thousand_to_the_last_bits() {
    let energy = |cutoff_ha: f64| {
        let grid = Grid::coarse(grid().cell(), cutoff_ha);
        let rho = vec![0.0123; grid.points().len()];
        (
            grid.points().len(),
            exchange_correlation(&grid, Functional::Lda, Spins::Unpolarized(&rho), None, None)
                .energy_ha,
        )
    };
    let ((few, coarse), (many, fine)) = (energy(0.01), energy(40.0));
    assert_eq!(few, 8);
    assert!(many > 90_000, "{many} points");
    assert!(
        ((fine - coarse) / coarse).abs() < 8.0 * f64::EPSILON,
        "{fine} against {coarse}"
    );
}

#[test]
fn a_uniform_density_gives_volume_times_the_energy_density_and_a_flat_potential() {
    let grid = grid();
    let n = grid.points().len();
    let (rho, core) = (vec![0.04; n], vec![0.01; n]);
    let xc = exchange_correlation(
        &grid,
        Functional::Pbe,
        Spins::Unpolarized(&rho),
        Some(&core),
        None,
    );
    let [x, c] = evaluate(Functional::Pbe, Point::of(0.025, 0.025, [0.0; 3]));
    assert!((xc.energy_ha - grid.cell().volume() * (x.energy + c.energy)).abs() < 1e-12);
    assert!(
        xc.potential[0]
            .iter()
            .all(|v| (v - (x.vrho[0] + c.vrho[0])).abs() < 1e-12)
    );
}

#[test]
fn the_potential_is_the_derivative_of_the_grid_energy() {
    let grid = grid();
    let volume = grid.cell().volume() / grid.points().len() as f64;
    let (rho, core) = (density(&grid, 1.0), density(&grid, 0.2));
    let delta: Vec<f64> = grid
        .points()
        .iter()
        .map(|f| 0.01 * (1.0 + (TAU * (f[0] + f[1])).cos() + (TAU * (f[1] - 2.0 * f[2])).cos()))
        .collect();
    let shifted = |t: f64| {
        rho.iter()
            .zip(&delta)
            .map(|(r, d)| r + t * d)
            .collect::<Vec<f64>>()
    };
    for functional in [Functional::Lda, Functional::Pbe] {
        let h = 1e-4;
        let energy = |t: f64| {
            exchange_correlation(
                &grid,
                functional,
                Spins::Unpolarized(&shifted(t)),
                Some(&core),
                None,
            )
            .energy_ha
        };
        let numeric = (energy(h) - energy(-h)) / (2.0 * h);
        let v = &exchange_correlation(
            &grid,
            functional,
            Spins::Unpolarized(&rho),
            Some(&core),
            None,
        )
        .potential[0];
        let analytic: f64 = v.iter().zip(&delta).map(|(v, d)| v * d * volume).sum();
        assert!(
            (numeric - analytic).abs() < 1e-8 * analytic.abs(),
            "{functional:?}: {numeric} vs {analytic}"
        );
        let down = density(&grid, 0.6);
        let energy = |t: f64| {
            exchange_correlation(
                &grid,
                functional,
                Spins::Polarized([&shifted(t), &down]),
                Some(&core),
                None,
            )
            .energy_ha
        };
        let numeric = (energy(h) - energy(-h)) / (2.0 * h);
        let v = &exchange_correlation(
            &grid,
            functional,
            Spins::Polarized([&rho, &down]),
            Some(&core),
            None,
        )
        .potential[0];
        let analytic: f64 = v.iter().zip(&delta).map(|(v, d)| v * d * volume).sum();
        assert!(
            (numeric - analytic).abs() < 1e-8 * analytic.abs(),
            "{functional:?} ↑: {numeric} vs {analytic}"
        );
    }
}

#[test]
fn equal_spin_channels_reproduce_the_unpolarized_result() {
    let grid = grid();
    let (rho, core) = (density(&grid, 1.0), density(&grid, 0.2));
    let half: Vec<f64> = rho.iter().map(|r| r / 2.0).collect();
    let paired = exchange_correlation(
        &grid,
        Functional::Pbe,
        Spins::Unpolarized(&rho),
        Some(&core),
        None,
    );
    let polarized = exchange_correlation(
        &grid,
        Functional::Pbe,
        Spins::Polarized([&half, &half]),
        Some(&core),
        None,
    );
    assert!((paired.energy_ha - polarized.energy_ha).abs() < 1e-12);
    for channel in &polarized.potential {
        assert!(
            channel
                .iter()
                .zip(&paired.potential[0])
                .all(|(a, b)| (a - b).abs() < 1e-12)
        );
    }
}

#[test]
fn the_gradient_of_a_plane_wave_is_i_g_times_it_and_the_divergence_is_its_negative_transpose() {
    let grid = grid();
    let f: Vec<f64> = grid
        .points()
        .iter()
        .map(|p| (TAU * (p[0] - 2.0 * p[1] + p[2])).cos())
        .collect();
    let g = grid.cell().wavevector([1.0, -2.0, 1.0]);
    let grad = grid.gradient(&f);
    for (i, p) in grid.points().iter().enumerate() {
        let s = -(TAU * (p[0] - 2.0 * p[1] + p[2])).sin();
        for a in 0..3 {
            assert!((grad[a][i] - g[a] * s).abs() < 1e-12);
        }
    }
    let h: [Vec<f64>; 3] = std::array::from_fn(|a| density(&grid, 1.0 + a as f64));
    let lhs: f64 = (0..3)
        .map(|a| h[a].iter().zip(&grad[a]).map(|(x, y)| x * y).sum::<f64>())
        .sum();
    let divergence = grid.divergence(&h);
    assert_eq!(divergence.len(), f.len());
    let rhs: f64 = divergence.iter().zip(&f).map(|(x, y)| x * y).sum();
    assert!((lhs + rhs).abs() < 1e-10 * lhs.abs().max(1.0));
}

#[test]
fn no_core_is_a_zero_core() {
    let grid = grid();
    let rho = density(&grid, 1.0);
    let zero = vec![0.0; rho.len()];
    let without =
        exchange_correlation(&grid, Functional::Pbe, Spins::Unpolarized(&rho), None, None);
    assert_eq!(
        without,
        exchange_correlation(
            &grid,
            Functional::Pbe,
            Spins::Unpolarized(&rho),
            Some(&zero),
            None
        )
    );
}

#[test]
#[should_panic(expected = "one density value per grid point")]
fn a_density_of_another_length_is_refused() {
    let grid = grid();
    exchange_correlation(
        &grid,
        Functional::Lda,
        Spins::Unpolarized(&[0.1; 7]),
        None,
        None,
    );
}

#[test]
fn inside_a_thread_pool_every_bit_is_the_same() {
    let grid = grid();
    let (up, down) = (density(&grid, 0.6), density(&grid, 0.4));
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    for functional in [Functional::Lda, Functional::Pbe] {
        for spins in [Spins::Unpolarized(&up), Spins::Polarized([&up, &down])] {
            let run = || exchange_correlation(&grid, functional, spins, Some(&down), None);
            let bits = |xc: orbitar::pw::Xc| {
                let v: Vec<u64> = xc.potential.iter().flatten().map(|v| v.to_bits()).collect();
                (xc.energy_ha.to_bits(), v)
            };
            assert_eq!(bits(run()), bits(pool.install(run)));
        }
    }
}

#[test]
fn an_lda_grid_is_the_point_entry_at_each_point_bit_for_bit() {
    let grid = grid();
    let n = grid.points().len();
    let (up, down, core) = (
        density(&grid, 0.6),
        density(&grid, 0.4),
        density(&grid, 0.1),
    );
    for polarized in [false, true] {
        let (spins, channels) = if polarized {
            let channel = |rho: &[f64]| -> Vec<f64> {
                (0..n).map(|r| rho[r] * 1.0 + core[r] * 0.5).collect()
            };
            (
                Spins::Polarized([&up, &down]),
                vec![channel(&up), channel(&down)],
            )
        } else {
            let whole: Vec<f64> = (0..n).map(|r| up[r] * 0.5 + core[r] * 0.5).collect();
            (Spins::Unpolarized(&up), vec![whole])
        };
        let xc = exchange_correlation(&grid, Functional::Lda, spins, Some(&core), None);
        let last = channels.len() - 1;
        let mut energy = KahanBabuskaNeumaier::<f64>::new();
        for r in 0..n {
            let (e, vrho) = if polarized {
                let p = Point::of(channels[0][r], channels[last][r], [0.0; 3]);
                let [x, c] = evaluate(Functional::Lda, p);
                (
                    x.energy + c.energy,
                    vec![x.vrho[0] + c.vrho[0], x.vrho[1] + c.vrho[1]],
                )
            } else {
                let [x, c] = unpolarized(Functional::Lda, 2.0 * channels[0][r], 0.0, None);
                (x.energy + c.energy, vec![x.vrho + c.vrho])
            };
            energy += e;
            for (s, v) in xc.potential.iter().enumerate() {
                assert_eq!(v[r].to_bits(), vrho[s].to_bits(), "point {r}");
            }
        }
        let want = energy.total() * grid.cell().volume() / n as f64;
        assert_eq!(xc.energy_ha.to_bits(), want.to_bits());
    }
}

fn strained(t: f64) -> (Grid, [[f64; 3]; 3]) {
    let e = [[0.3, 0.5, -0.2], [0.5, -0.4, 0.6], [-0.2, 0.6, 0.1]];
    let a = [[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]];
    let rows: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| a[r][c] + t * (0..3).map(|k| a[r][k] * e[k][c]).sum::<f64>())
    });
    (Grid::coarse(&Cell::try_from(rows).unwrap(), 4.0), e)
}

#[test]
fn the_gradient_strain_is_the_derivative_of_the_energy_per_volume_at_fixed_grid_values() {
    let (grid, e) = strained(0.0);
    let (rho, down, core) = (
        density(&grid, 1.0),
        density(&grid, 0.6),
        density(&grid, 0.2),
    );
    for functional in [Functional::Lda, Functional::Pbe] {
        for polarized in [false, true] {
            let at = |grid: &Grid| {
                let spins = if polarized {
                    Spins::Polarized([&rho, &down])
                } else {
                    Spins::Unpolarized(&rho)
                };
                exchange_correlation(grid, functional, spins, Some(&core), None)
            };
            let per_volume = |t: f64| {
                let (moved, _) = strained(t);
                assert_eq!(moved.dims(), grid.dims());
                at(&moved).energy_ha / moved.cell().volume()
            };
            let h = 1e-3;
            let slope = grid.cell().volume()
                * (8.0 * (per_volume(h) - per_volume(-h))
                    - (per_volume(2.0 * h) - per_volume(-2.0 * h)))
                / (12.0 * h);
            let term = at(&grid).gradient_strain_ha;
            let along: f64 = (0..3)
                .flat_map(|i| (0..3).map(move |j| (i, j)))
                .map(|(i, j)| term[i][j] * e[i][j])
                .sum();
            assert!(
                (along - slope).abs() < 1e-10 * (1.0 + slope.abs()),
                "{functional:?} {polarized}: {along} vs {slope}"
            );
            if functional == Functional::Pbe {
                assert!(along.abs() > 1e-4, "{along}");
            }
        }
    }
}

fn tau(grid: &Grid, scale: f64) -> Vec<f64> {
    (grid.points().iter())
        .map(|f| scale * (0.03 + 0.01 * (TAU * (f[1] - f[2]) + 0.7).cos()))
        .collect()
}

fn v_tau(xc: &Xc, s: usize) -> &[f64] {
    &xc.kinetic_potential
        .as_ref()
        .expect("v_τ at the meta-GGA rung")[s]
}

#[test]
fn below_the_meta_gga_rung_a_given_tau_changes_no_bit() {
    let grid = grid();
    let (up, down, core) = (
        density(&grid, 0.6),
        density(&grid, 0.4),
        density(&grid, 0.1),
    );
    let (t_up, t_down) = (tau(&grid, 0.6), tau(&grid, 0.4));
    for functional in [Functional::Lda, Functional::Pbe] {
        for (spins, kinetic) in [
            (Spins::Unpolarized(&up), Spins::Unpolarized(&t_up)),
            (
                Spins::Polarized([&up, &down]),
                Spins::Polarized([&t_up, &t_down]),
            ),
        ] {
            let given = Kinetic {
                valence: kinetic,
                core: Some(&core),
            };
            let with = exchange_correlation(&grid, functional, spins, Some(&core), Some(given));
            let without = exchange_correlation(&grid, functional, spins, Some(&core), None);
            assert_eq!(with.kinetic_potential, None);
            let bits = |xc: &Xc| {
                let v: Vec<u64> = xc.potential.iter().flatten().map(|v| v.to_bits()).collect();
                let strain = xc.gradient_strain_ha.map(|row| row.map(f64::to_bits));
                (
                    xc.energy_ha.to_bits(),
                    v,
                    strain,
                    xc.kinetic_potential.is_some(),
                )
            };
            assert_eq!(bits(&with), bits(&without), "{functional:?}");
        }
    }
}

#[test]
fn a_uniform_density_and_tau_give_volume_times_the_energy_density_and_flat_potentials() {
    let grid = grid();
    let n = grid.points().len();
    let (rho, core, t, t_core) = (vec![0.04; n], vec![0.01; n], vec![0.03; n], vec![0.02; n]);
    let kinetic = Kinetic {
        valence: Spins::Unpolarized(&t),
        core: Some(&t_core),
    };
    let xc = exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&rho),
        Some(&core),
        Some(kinetic),
    );
    let [x, c] = unpolarized(Functional::R2scan, 0.05, 0.0, Some(0.05));
    assert!((xc.energy_ha - grid.cell().volume() * (x.energy + c.energy)).abs() < 1e-12);
    assert!(
        xc.potential[0]
            .iter()
            .all(|v| (v - (x.vrho + c.vrho)).abs() < 1e-12)
    );
    assert_eq!(xc.kinetic_potential.as_ref().map(Vec::len), Some(1));
    assert!(
        v_tau(&xc, 0)
            .iter()
            .all(|v| (v - (x.vtau + c.vtau)).abs() < 1e-12)
    );
    assert!(x.vtau + c.vtau != 0.0);
}

#[test]
fn the_meta_gga_potentials_are_the_derivatives_of_the_grid_energy() {
    let grid = grid();
    let volume = grid.cell().volume() / grid.points().len() as f64;
    let (rho, down, core) = (
        density(&grid, 1.0),
        density(&grid, 0.6),
        density(&grid, 0.2),
    );
    let (t, t_down, t_core) = (tau(&grid, 1.0), tau(&grid, 0.6), tau(&grid, 0.3));
    let delta: Vec<f64> = (grid.points().iter())
        .map(|f| 0.004 * (1.0 + (TAU * (f[0] + f[1])).cos() + (TAU * (f[1] - 2.0 * f[2])).cos()))
        .collect();
    let shifted = |field: &[f64], s: f64| -> Vec<f64> {
        field.iter().zip(&delta).map(|(x, d)| x + s * d).collect()
    };
    let h = 1e-4;
    for polarized in [false, true] {
        let run = |rho: &[f64], t: &[f64]| {
            let (spins, valence) = match polarized {
                false => (Spins::Unpolarized(rho), Spins::Unpolarized(t)),
                true => (
                    Spins::Polarized([rho, &down]),
                    Spins::Polarized([t, &t_down]),
                ),
            };
            let kinetic = Kinetic {
                valence,
                core: Some(&t_core),
            };
            exchange_correlation(&grid, Functional::R2scan, spins, Some(&core), Some(kinetic))
        };
        let at = run(&rho, &t);
        for (name, numeric, potential) in [
            (
                "v",
                (run(&shifted(&rho, h), &t).energy_ha - run(&shifted(&rho, -h), &t).energy_ha)
                    / (2.0 * h),
                at.potential[0].as_slice(),
            ),
            (
                "v_τ",
                (run(&rho, &shifted(&t, h)).energy_ha - run(&rho, &shifted(&t, -h)).energy_ha)
                    / (2.0 * h),
                v_tau(&at, 0),
            ),
        ] {
            let analytic: f64 = potential
                .iter()
                .zip(&delta)
                .map(|(v, d)| v * d * volume)
                .sum();
            assert!(
                (numeric - analytic).abs() < 1e-8 * analytic.abs(),
                "{name}, polarized {polarized}: {numeric} vs {analytic}"
            );
        }
    }
}

#[test]
fn equal_spin_channels_reproduce_the_unpolarized_meta_gga_result() {
    let grid = grid();
    let (rho, core, t, t_core) = (
        density(&grid, 1.0),
        density(&grid, 0.2),
        tau(&grid, 1.0),
        tau(&grid, 0.2),
    );
    let half = |f: &[f64]| -> Vec<f64> { f.iter().map(|x| x / 2.0).collect() };
    let (rho_half, t_half) = (half(&rho), half(&t));
    let paired = exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&rho),
        Some(&core),
        Some(Kinetic {
            valence: Spins::Unpolarized(&t),
            core: Some(&t_core),
        }),
    );
    let polarized = exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Polarized([&rho_half, &rho_half]),
        Some(&core),
        Some(Kinetic {
            valence: Spins::Polarized([&t_half, &t_half]),
            core: Some(&t_core),
        }),
    );
    let close = |a: &[f64], b: &[f64]| a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-12);
    assert!((paired.energy_ha - polarized.energy_ha).abs() < 1e-12);
    for s in 0..2 {
        assert!(close(&polarized.potential[s], &paired.potential[0]));
        assert!(close(v_tau(&polarized, s), v_tau(&paired, 0)));
    }
}

#[test]
fn no_core_tau_is_a_zero_core_tau() {
    let grid = grid();
    let (rho, t) = (density(&grid, 1.0), tau(&grid, 1.0));
    let zero = vec![0.0; rho.len()];
    let run = |core: Option<&[f64]>| {
        let kinetic = Kinetic {
            valence: Spins::Unpolarized(&t),
            core,
        };
        exchange_correlation(
            &grid,
            Functional::R2scan,
            Spins::Unpolarized(&rho),
            None,
            Some(kinetic),
        )
    };
    assert_eq!(run(None), run(Some(&zero)));
}

#[test]
#[should_panic(expected = "pw::exchange_correlation: r2scan reads τ; give it")]
fn a_meta_gga_without_tau_is_refused() {
    let grid = grid();
    let rho = density(&grid, 1.0);
    exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&rho),
        None,
        None,
    );
}

#[test]
#[should_panic(expected = "pw::exchange_correlation: τ in another spin layout than ρ")]
fn tau_in_another_spin_layout_than_the_density_is_refused() {
    let grid = grid();
    let (rho, t) = (density(&grid, 1.0), tau(&grid, 1.0));
    let kinetic = Kinetic {
        valence: Spins::Polarized([&t, &t]),
        core: None,
    };
    exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&rho),
        None,
        Some(kinetic),
    );
}

#[test]
#[should_panic(expected = "one τ value per grid point")]
fn a_tau_of_another_length_is_refused() {
    let grid = grid();
    let rho = density(&grid, 1.0);
    let kinetic = Kinetic {
        valence: Spins::Unpolarized(&[0.1; 7]),
        core: None,
    };
    exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&rho),
        None,
        Some(kinetic),
    );
}

#[test]
fn inside_a_thread_pool_every_meta_gga_bit_is_the_same() {
    let grid = grid();
    let (up, down, t_up, t_down) = (
        density(&grid, 0.6),
        density(&grid, 0.4),
        tau(&grid, 0.6),
        tau(&grid, 0.4),
    );
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    for (spins, valence) in [
        (Spins::Unpolarized(&up), Spins::Unpolarized(&t_up)),
        (
            Spins::Polarized([&up, &down]),
            Spins::Polarized([&t_up, &t_down]),
        ),
    ] {
        let kinetic = Kinetic {
            valence,
            core: Some(&t_down),
        };
        let run =
            || exchange_correlation(&grid, Functional::R2scan, spins, Some(&down), Some(kinetic));
        let bits = |xc: Xc| {
            let fields = xc
                .potential
                .iter()
                .chain(xc.kinetic_potential.iter().flatten());
            let v: Vec<u64> = fields.flatten().map(|v| v.to_bits()).collect();
            (xc.energy_ha.to_bits(), v)
        };
        assert_eq!(bits(run()), bits(pool.install(run)));
    }
}

#[test]
fn a_vacuum_point_reaches_a_libxc_functional_at_abinits_grid_floors() {
    let grid = grid();
    let n = grid.points().len();
    let zero = vec![0.0; n];
    let volume = grid.cell().volume();
    let [x, c] = unpolarized(Functional::R2scan, 1e-14, 0.0, Some(1e-14));
    assert_eq!(x.energy, 0.0);
    assert!(c.energy != 0.0);
    let whole = exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&zero),
        None,
        Some(Kinetic {
            valence: Spins::Unpolarized(&zero),
            core: None,
        }),
    );
    assert!((whole.energy_ha - volume * c.energy).abs() <= 1e-12 * (volume * c.energy).abs());
    let [x, c] = evaluate(
        Functional::R2scan,
        Point::of(1e-14, 1e-14, [0.0; 3]).with_tau([1e-14; 2]),
    );
    let spins = exchange_correlation(
        &grid,
        Functional::R2scan,
        Spins::Polarized([&zero, &zero]),
        None,
        Some(Kinetic {
            valence: Spins::Polarized([&zero, &zero]),
            core: None,
        }),
    );
    let want = volume * (x.energy + c.energy);
    assert!(
        (spins.energy_ha - want).abs() <= 1e-12 * want.abs(),
        "{} vs {want}",
        spins.energy_ha
    );
}

fn near_thresholds(grid: &Grid, shift: f64) -> [Vec<f64>; 4] {
    let at = |f: fn(&[f64; 3]) -> f64| grid.points().iter().map(f).collect::<Vec<f64>>();
    let up: Vec<f64> = (grid.points().iter())
        .map(|f| 1e-11 * (1.0 + 0.9 * (TAU * f[0]).sin() + shift * (TAU * f[2]).sin()))
        .collect();
    let down = at(|f| 1e-11 * (1.0 + 0.5 * (TAU * f[2]).cos()));
    let tau = at(|f| 1e-13 * (1.0 + 0.5 * (TAU * f[1]).cos()));
    [up, down, tau.clone(), tau]
}

#[test]
fn the_grid_defect_is_the_energy_secant_less_the_recipe_on_every_branch() {
    let grid = grid();
    let element = grid.cell().volume() / grid.points().len() as f64;
    let functional = Functional::R2scan;
    let points = |shift: f64| {
        let [up, down, tu, td] = near_thresholds(&grid, shift);
        let kinetic = Kinetic {
            valence: Spins::Polarized([&tu, &td]),
            core: None,
        };
        let xc = exchange_correlation(
            &grid,
            functional,
            Spins::Polarized([&up, &down]),
            None,
            Some(kinetic),
        );
        (
            xc.energy_ha,
            grid_points(
                &grid,
                functional,
                Spins::Polarized([&up, &down]),
                None,
                Some(kinetic),
            ),
        )
    };
    let ((_, centre), (plus_e, plus), (minus_e, minus)) = (points(0.0), points(0.3), points(-0.3));
    for (energy, at) in [(plus_e, &plus), (minus_e, &minus)] {
        let sum: f64 = at
            .iter()
            .map(|p| element * grid_energy(functional, p))
            .sum();
        assert!(
            (sum - energy).abs() <= 1e-12 * energy.abs(),
            "{sum} vs {energy}"
        );
    }
    let flagged = centre.iter().filter(|p| p.prepared).count();
    assert!(flagged > 0 && flagged < centre.len(), "{flagged}");
    let defect: f64 = (centre.iter().zip(&plus).zip(&minus))
        .map(|((c, p), m)| element * grid_defect(functional, c, p, m))
        .sum();
    let contraction: f64 = (centre.iter().zip(&plus).zip(&minus))
        .map(|((c, p), m)| {
            element
                * ((0..2)
                    .map(|s| {
                        c.v_rho[s] * (p.rho[s] - m.rho[s]) + c.v_tau[s] * (p.tau[s] - m.tau[s])
                    })
                    .sum::<f64>()
                    + (0..3)
                        .map(|k| c.v_sigma[k] * (p.sigma[k] - m.sigma[k]))
                        .sum::<f64>())
        })
        .sum();
    let want = plus_e - minus_e - contraction;
    assert!(
        (defect - want).abs() <= 1e-12 * (plus_e.abs() + minus_e.abs()),
        "{defect} vs {want}"
    );
}

#[test]
fn a_whole_tau_below_the_grid_floor_flags_every_point() {
    let grid = grid();
    let n: Vec<f64> = (grid.points().iter())
        .map(|f| 0.05 + 1e-10 * (TAU * (f[0] + f[1])).cos())
        .collect();
    let tau = vec![1e-15; n.len()];
    let kinetic = Kinetic {
        valence: Spins::Unpolarized(&tau),
        core: None,
    };
    let points = grid_points(
        &grid,
        Functional::R2scan,
        Spins::Unpolarized(&n),
        None,
        Some(kinetic),
    );
    assert!(
        points.iter().all(|p| p.prepared),
        "{}",
        points.iter().filter(|p| !p.prepared).count()
    );
}

#[test]
fn the_grid_gap_sums_every_point_flagged_in_any_of_the_three_states() {
    let grid = grid();
    let element = grid.cell().volume() / grid.points().len() as f64;
    let functional = Functional::R2scan;
    let points = |shift: f64| {
        let [up, down, tu, td] = near_thresholds(&grid, shift);
        let kinetic = Kinetic {
            valence: Spins::Polarized([&tu, &td]),
            core: None,
        };
        grid_points(
            &grid,
            functional,
            Spins::Polarized([&up, &down]),
            None,
            Some(kinetic),
        )
    };
    let (centre, plus, minus) = (points(0.0), points(0.3), points(-0.3));
    let flags: Vec<[bool; 3]> = (centre.iter().zip(&plus).zip(&minus))
        .map(|((c, p), m)| [c.prepared, p.prepared, m.prepared])
        .collect();
    let alone = |k: usize| {
        flags
            .iter()
            .filter(|f| f[k] && f.iter().filter(|&&x| x).count() == 1)
            .count()
    };
    assert!(
        alone(1) > 0 && alone(2) > 0,
        "points flagged at one end alone: {} {}",
        alone(1),
        alone(2)
    );
    let gap = grid_gap(functional, element, &centre, &plus, &minus).unwrap();
    let defects: Vec<f64> = (centre.iter().zip(&plus).zip(&minus))
        .map(|((c, p), m)| element * grid_defect(functional, c, p, m))
        .collect();
    let any = |f: &[bool; 3]| f.iter().any(|&x| x);
    let want: f64 = defects
        .iter()
        .zip(&flags)
        .filter(|(_, f)| any(f))
        .map(|(d, _)| d)
        .sum();
    assert_eq!(gap.points, flags.iter().filter(|f| any(f)).count());
    assert!(
        (gap.predicted_ha - want).abs() <= 1e-15 * want.abs().max(1e-30),
        "{} vs {want}",
        gap.predicted_ha
    );
    let bound: f64 = defects.iter().map(|d| d.abs()).sum();
    assert!(
        (gap.bound_ha - bound).abs() <= 1e-12 * bound && gap.bound_ha >= gap.predicted_ha.abs()
    );
}

#[test]
fn a_grid_gap_over_states_of_unequal_point_counts_is_refused_by_name() {
    let grid = grid();
    let n = vec![0.05; grid.points().len()];
    let points = grid_points(&grid, Functional::Lda, Spins::Unpolarized(&n), None, None);
    let short = &points[1..];
    let gap = |plus: &[_], minus: &[_]| grid_gap(Functional::Lda, 1.0, &points, plus, minus);
    let m = points.len();
    for (plus, minus, counts) in [
        (short, &points[..], [m, m - 1, m]),
        (&points[..], short, [m, m, m - 1]),
    ] {
        let e = gap(plus, minus).unwrap_err();
        let want = format!(
            "grid gap: the centre holds {} grid points, the plus state {}, the minus state {}",
            counts[0], counts[1], counts[2]
        );
        assert_eq!(e, want);
    }
    assert!(gap(&points, &points).is_ok());
}
