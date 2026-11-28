use super::{Parts, bits, entry};
use orbitar::pw::{
    Applied, Eigenpairs, Guess, Mat, MatRef, Pencil, Tolerance, Workspace, c64, dense, lowest,
    random,
};

fn stream(seed: u64) -> impl FnMut() -> f64 {
    let mut state = seed;
    move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }
}

struct Toy<T> {
    kinetic: Vec<f64>,
    h: Mat<T>,
    s: Mat<T>,
}

fn toy<T: Parts>(n: usize, seed: u64) -> Toy<T> {
    let d = Mat::from_fn(4, 4, |i, j| entry::<T>(if i == j { 2.0 } else { 0.3 }, 0.0));
    let q = Mat::from_fn(4, 4, |i, j| {
        entry::<T>(if i == j { [1.5, 0.8, 0.4, 0.2][i] } else { 0.0 }, 0.0)
    });
    paw_shaped(n, seed, &d, &q)
}

fn semicore<T: Parts>(n: usize, seed: u64) -> Toy<T> {
    let d = Mat::from_fn(8, 8, |i, j| {
        entry::<T>(if i == j { -1.0 } else { 0.1 }, 0.0)
    });
    let q = Mat::from_fn(8, 8, |i, j| entry::<T>(f64::from(u8::from(i == j)), 0.0));
    paw_shaped(n, seed, &d, &q)
}

fn paw_shaped<T: Parts>(n: usize, seed: u64, d: &Mat<T>, q: &Mat<T>) -> Toy<T> {
    let mut r = stream(seed);
    let kinetic: Vec<f64> = (0..n).map(|g| 0.02 * g as f64 + 0.3 * r().abs()).collect();
    let b = Mat::from_fn(n, d.nrows(), |g, _| {
        entry::<T>(r(), r()) * (-0.01 * g as f64).exp()
    });
    let v = Mat::from_fn(n, n, |i, j| {
        let d = i.abs_diff(j) as f64;
        entry::<T>(
            -0.4 * (-0.3 * d).exp() * (0.5 * d).cos(),
            0.1 * (i as f64 - j as f64) * (-0.3 * d).exp(),
        )
    });
    let mut h = &v + &b * d * b.adjoint();
    let mut s = &b * q * b.adjoint();
    for g in 0..n {
        h[(g, g)] += entry::<T>(kinetic[g], 0.0);
        s[(g, g)] += entry::<T>(1.0, 0.0);
    }
    Toy { kinetic, h, s }
}

fn solve<T: Parts>(
    t: &Toy<T>,
    guess: Mat<T>,
    converged: usize,
    tolerance: impl Into<Tolerance>,
    max: usize,
) -> Result<Eigenpairs<T>, Eigenpairs<T>> {
    solve_counting(t, guess, converged, tolerance, max).0
}

fn solve_counting<T: Parts>(
    t: &Toy<T>,
    guess: Mat<T>,
    converged: usize,
    tolerance: impl Into<Tolerance>,
    max: usize,
) -> (Result<Eigenpairs<T>, Eigenpairs<T>>, usize) {
    solve_in(
        t,
        guess,
        converged,
        tolerance,
        max,
        &mut Workspace::default(),
    )
}

fn solve_in<T: Parts>(
    t: &Toy<T>,
    guess: impl Into<Guess<T>>,
    converged: usize,
    tolerance: impl Into<Tolerance>,
    max: usize,
    work: &mut Workspace<T>,
) -> (Result<Eigenpairs<T>, Eigenpairs<T>>, usize) {
    let applied = std::cell::Cell::new(0);
    let h = |x: MatRef<'_, T>| {
        applied.set(applied.get() + x.ncols());
        &t.h * x
    };
    let s = |x: MatRef<'_, T>| &t.s * x;
    let result = lowest(
        &Pencil {
            h: &h,
            s: &s,
            kinetic_ha: &t.kinetic,
        },
        guess,
        converged,
        tolerance,
        max,
        work,
    );
    (result, applied.get())
}

fn random_block<T: Parts>(n: usize, m: usize, seed: u64) -> Mat<T> {
    let mut r = stream(seed);
    Mat::from_fn(n, m, |_, _| entry::<T>(r(), r()))
}

fn s_orthonormality_error<T: Parts>(t: &Toy<T>, x: MatRef<'_, T>) -> f64 {
    let g = x.adjoint() * &t.s * x;
    let mut worst: f64 = 0.0;
    for i in 0..g.nrows() {
        for j in 0..g.ncols() {
            worst = worst.max((g[(i, j)] - entry::<T>(f64::from(u8::from(i == j)), 0.0)).abs());
        }
    }
    worst
}

#[test]
fn the_dense_solver_gives_the_closed_form_of_a_two_by_two_pencil() {
    the_dense_solver_gives_the_closed_form_of_a_two_by_two_pencil_in::<c64>();
    the_dense_solver_gives_the_closed_form_of_a_two_by_two_pencil_in::<f64>();
}

fn the_dense_solver_gives_the_closed_form_of_a_two_by_two_pencil_in<T: Parts>() {
    let h = Mat::from_fn(2, 2, |i, j| entry::<T>(if i == j { 2.0 } else { 1.0 }, 0.0));
    let s = Mat::from_fn(2, 2, |i, j| {
        entry::<T>(if i == j { [1.0, 2.0][i] } else { 0.0 }, 0.0)
    });
    let (values, x) = dense(h.as_ref(), s.as_ref()).unwrap();
    let root = 0.75_f64.sqrt();
    assert!((values[0] - (1.5 - root)).abs() < 1e-15 && (values[1] - (1.5 + root)).abs() < 1e-15);
    let g: Mat<T> = x.adjoint() * &s * &x;
    assert!((g[(0, 0)].parts()[0] - 1.0).abs() < 1e-15 && g[(0, 1)].abs() < 1e-15);
}

#[test]
fn the_dense_solver_refuses_an_indefinite_overlap() {
    the_dense_solver_refuses_an_indefinite_overlap_in::<c64>();
    the_dense_solver_refuses_an_indefinite_overlap_in::<f64>();
}

fn the_dense_solver_refuses_an_indefinite_overlap_in<T: Parts>() {
    let h = Mat::<T>::identity(2, 2);
    let s = Mat::from_fn(2, 2, |i, j| entry::<T>(if i == j { 1.0 } else { 2.0 }, 0.0));
    assert!(dense(h.as_ref(), s.as_ref()).is_none());
}

#[test]
fn lobpcg_finds_the_lowest_pairs_of_the_dense_solution() {
    lobpcg_finds_the_lowest_pairs_of_the_dense_solution_in::<c64>();
    lobpcg_finds_the_lowest_pairs_of_the_dense_solution_in::<f64>();
}

fn lobpcg_finds_the_lowest_pairs_of_the_dense_solution_in<T: Parts>() {
    let t = toy::<T>(240, 1);
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let pairs = solve(&t, random_block::<T>(240, 12, 2), 9, 1e-10, 200).unwrap();
    for (i, (got, want)) in pairs.values().iter().zip(&exact).take(9).enumerate() {
        assert!((got - want).abs() < 1e-12, "band {i}: {got} vs {want}");
    }
    assert!(pairs.residuals()[..9].iter().all(|&r| r < 1e-10));
    assert!(s_orthonormality_error(&t, pairs.vectors()) < 1e-12);
    let x = pairs.vectors();
    let r = &t.h * x
        - &t.s
            * x
            * Mat::from_fn(12, 12, |i, j| {
                entry::<T>(if i == j { pairs.values()[i] } else { 0.0 }, 0.0)
            });
    for j in 0..12 {
        assert!((r.col(j).norm_l2() - pairs.residuals()[j]).abs() < 1e-12);
    }
}

#[test]
fn a_restarted_semicore_pencil_reaches_a_hundred_epsilon_in_real_and_complex_arithmetic() {
    a_restarted_semicore_pencil_reaches_a_hundred_epsilon_in::<f64>();
    a_restarted_semicore_pencil_reaches_a_hundred_epsilon_in::<c64>();
}

fn a_restarted_semicore_pencil_reaches_a_hundred_epsilon_in<T: Parts>() {
    let target = 100.0 * f64::EPSILON;
    let t = semicore::<T>(400, 1);
    let cold = solve(&t, random_block::<T>(400, 24, 3), 20, target, 100).unwrap_or_else(|p| p);
    let warm = solve(&t, cold.vectors().to_owned(), 20, target, 100).unwrap_or_else(|p| p);
    let x = warm.vectors().subcols(0, 20);
    let sx = &t.s * x;
    let values = Mat::from_fn(20, 20, |i, j| {
        entry::<T>(if i == j { warm.values()[i] } else { 0.0 }, 0.0)
    });
    let r = &t.h * x - &sx * values;
    let gram = x.adjoint() * &sx;
    for j in 0..20 {
        let residual = r.col(j).norm_l2() / gram[(j, j)].parts()[0].sqrt();
        assert!(residual < target, "band {j}: {residual:e}");
    }
}

fn prescribed<T: Parts>(residuals: [f64; 3]) -> (Toy<T>, Mat<T>) {
    let mut h = Mat::from_fn(16, 16, |i, j| {
        entry::<T>(if i == j { i as f64 } else { 0.0 }, 0.0)
    });
    for (j, r) in residuals.into_iter().enumerate() {
        h[(j, j + 3)] = entry::<T>(r, 0.0);
        h[(j + 3, j)] = entry::<T>(r, 0.0);
    }
    let toy = Toy {
        kinetic: (1..=16).map(f64::from).collect(),
        h,
        s: Mat::identity(16, 16),
    };
    let start = Mat::from_fn(16, 3, |i, j| entry::<T>(f64::from(u8::from(i == j)), 0.0));
    (toy, start)
}

#[test]
fn pairs_are_converged_below_a_bound_only_strictly_and_only_for_the_asked_bands() {
    pairs_are_converged_below_a_bound_only_strictly_and_only_for_the_asked_bands_in::<f64>();
    pairs_are_converged_below_a_bound_only_strictly_and_only_for_the_asked_bands_in::<c64>();
}

fn pairs_are_converged_below_a_bound_only_strictly_and_only_for_the_asked_bands_in<T: Parts>() {
    let (t, start) = prescribed::<T>([5e-14, 1e-13, 2e-13]);
    let Err(start) = solve(&t, start, 2, 100.0 * f64::EPSILON, 0) else {
        panic!("a solve of no iterations converged");
    };
    assert_eq!(start.residuals(), [5e-14, 1e-13, 2e-13]);
    let above = 1e-13_f64.next_up();
    assert!(start.converged_below(1, 1e-13));
    assert!(!start.converged_below(2, 1e-13));
    assert!(start.converged_below(2, above));
    assert!(!start.converged_below(3, above));
}

#[test]
fn a_solve_of_no_iterations_is_unconverged_though_its_start_meets_the_accepted_bound() {
    a_solve_of_no_iterations_is_unconverged_though_its_start_meets_the_accepted_bound_in::<f64>();
    a_solve_of_no_iterations_is_unconverged_though_its_start_meets_the_accepted_bound_in::<c64>();
}

fn a_solve_of_no_iterations_is_unconverged_though_its_start_meets_the_accepted_bound_in<
    T: Parts,
>() {
    let (t, start) = prescribed::<T>([5e-14, 1e-13, 2e-13]);
    let tolerance = Tolerance::aiming(100.0 * f64::EPSILON, 1e-13);
    assert!(solve(&t, start, 1, tolerance, 0).is_err());
}

#[test]
fn a_solve_short_of_its_target_converges_when_its_pairs_meet_the_accepted_bound() {
    a_solve_short_of_its_target_converges_when_its_pairs_meet_the_accepted_bound_in::<f64>();
    a_solve_short_of_its_target_converges_when_its_pairs_meet_the_accepted_bound_in::<c64>();
}

fn a_solve_short_of_its_target_converges_when_its_pairs_meet_the_accepted_bound_in<T: Parts>() {
    let t = semicore::<T>(400, 1);
    let start = || random_block::<T>(400, 24, 3);
    assert!(solve(&t, start(), 20, 1e-16, 100).is_err());
    let Ok(pairs) = solve(&t, start(), 20, Tolerance::aiming(1e-16, 1e-13), 100) else {
        panic!("pairs short of 1e-16 did not meet 1e-13");
    };
    assert!(pairs.converged_below(20, 1e-13));
}

#[test]
fn deep_bands_from_a_cold_start_converge_to_a_tolerance_near_rounding() {
    deep_bands_from_a_cold_start_converge_to_a_tolerance_near_rounding_in::<c64>();
    deep_bands_from_a_cold_start_converge_to_a_tolerance_near_rounding_in::<f64>();
}

fn deep_bands_from_a_cold_start_converge_to_a_tolerance_near_rounding_in<T: Parts>() {
    let t = semicore::<T>(400, 1);
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let result = solve(&t, random_block::<T>(400, 24, 3), 20, 1e-13, 100);
    let pairs = match result {
        Ok(pairs) => pairs,
        Err(pairs) => panic!("residuals {:?}", pairs.residuals()),
    };
    for (i, (got, want)) in pairs.values().iter().zip(&exact).take(20).enumerate() {
        assert!((got - want).abs() < 1e-12, "band {i}: {got} vs {want}");
    }
    assert!(s_orthonormality_error(&t, pairs.vectors()) < 1e-14);
}

#[test]
fn a_nearly_dependent_guess_still_converges() {
    a_nearly_dependent_guess_still_converges_in::<c64>();
    a_nearly_dependent_guess_still_converges_in::<f64>();
}

fn a_nearly_dependent_guess_still_converges_in<T: Parts>() {
    let t = toy::<T>(200, 3);
    let mut guess = random_block::<T>(200, 8, 4);
    let noise = random_block::<T>(200, 1, 5);
    for g in 0..200 {
        guess[(g, 7)] = guess[(g, 6)] + noise[(g, 0)] * 1e-12;
    }
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let pairs = solve(&t, guess, 6, 1e-9, 300).unwrap();
    assert!(
        pairs
            .values()
            .iter()
            .zip(&exact)
            .take(6)
            .all(|(a, b)| (a - b).abs() < 1e-11)
    );
}

fn free_electrons<T: Parts>() -> Toy<T> {
    let mut kinetic = Vec::new();
    for i in -3..=3_i32 {
        for j in -3..=3_i32 {
            for k in -3..=3_i32 {
                kinetic.push(0.5 * f64::from(i * i + j * j + k * k));
            }
        }
    }
    let n = kinetic.len();
    Toy {
        h: Mat::from_fn(n, n, |i, j| {
            entry::<T>(if i == j { kinetic[i] } else { 0.0 }, 0.0)
        }),
        s: Mat::<T>::identity(n, n),
        kinetic,
    }
}

const SHELLS: [f64; 7] = [0.0, 0.5, 0.5, 0.5, 0.5, 0.5, 0.5];

#[test]
fn a_degenerate_shell_at_the_block_edge_converges_with_buffer_bands() {
    a_degenerate_shell_at_the_block_edge_converges_with_buffer_bands_in::<c64>();
    a_degenerate_shell_at_the_block_edge_converges_with_buffer_bands_in::<f64>();
}

fn a_degenerate_shell_at_the_block_edge_converges_with_buffer_bands_in<T: Parts>() {
    let t = free_electrons::<T>();
    let n = t.kinetic.len();
    let (pairs, applied) = solve_counting(&t, random_block::<T>(n, 10, 6), 7, 1e-10, 200);
    let pairs = pairs.unwrap();
    assert!(applied <= 20 * 10, "{applied} H applications for 10 bands");
    let want = SHELLS;
    assert!(
        pairs
            .values()
            .iter()
            .zip(want)
            .all(|(a, b)| (a - b).abs() < 1e-12)
    );
}

#[test]
fn a_reused_workspace_gives_the_bits_of_a_fresh_one() {
    a_reused_workspace_gives_the_bits_of_a_fresh_one_in::<c64>();
    a_reused_workspace_gives_the_bits_of_a_fresh_one_in::<f64>();
}

fn a_reused_workspace_gives_the_bits_of_a_fresh_one_in<T: Parts>() {
    let (large, small) = (toy::<T>(320, 11), toy::<T>(200, 12));
    let mut work = Workspace::default();
    let bits = |pairs: &Eigenpairs<T>| {
        let values: Vec<u64> = pairs.values().iter().map(|x| x.to_bits()).collect();
        (values, bits(pairs.vectors()))
    };
    for (t, n, m, seed) in [(&large, 320, 30, 13), (&small, 200, 9, 14)] {
        let reused = solve_in(
            t,
            random_block::<T>(n, m, seed),
            m - 3,
            1e-9,
            200,
            &mut work,
        )
        .0
        .unwrap();
        let fresh = solve(t, random_block::<T>(n, m, seed), m - 3, 1e-9, 200).unwrap();
        assert_eq!(bits(&reused), bits(&fresh), "{n} plane waves, {m} bands");
    }
}

#[test]
fn too_few_iterations_return_the_unconverged_pairs_as_an_error() {
    too_few_iterations_return_the_unconverged_pairs_as_an_error_in::<c64>();
    too_few_iterations_return_the_unconverged_pairs_as_an_error_in::<f64>();
}

fn too_few_iterations_return_the_unconverged_pairs_as_an_error_in<T: Parts>() {
    let t = toy::<T>(240, 7);
    let partial = solve(&t, random_block::<T>(240, 6, 8), 6, 1e-12, 2).unwrap_err();
    assert_eq!(partial.values().len(), 6);
    assert!(partial.residuals().iter().any(|&r| r >= 1e-12));
}

#[test]
fn the_g0_plane_wave_at_gamma_as_a_guess_still_converges() {
    the_g0_plane_wave_at_gamma_as_a_guess_still_converges_in::<c64>();
    the_g0_plane_wave_at_gamma_as_a_guess_still_converges_in::<f64>();
}

fn the_g0_plane_wave_at_gamma_as_a_guess_still_converges_in<T: Parts>() {
    let t = free_electrons::<T>();
    let n = t.kinetic.len();
    let mut guess = random_block::<T>(n, 10, 9);
    guess.col_mut(0).fill(entry::<T>(0.0, 0.0));
    guess[(171, 0)] = entry::<T>(1.0, 0.0);
    let pairs = solve(&t, guess, 7, 1e-10, 200).unwrap();
    assert!(
        pairs
            .values()
            .iter()
            .zip(SHELLS)
            .all(|(a, b)| (a - b).abs() < 1e-12)
    );
}

fn diagonal<T: Parts>() -> Toy<T> {
    let n = 40;
    let kinetic: Vec<f64> = (0..n).map(|g| 0.1 + 0.05 * g as f64).collect();
    Toy {
        h: Mat::from_fn(n, n, |i, j| {
            entry::<T>(if i == j { kinetic[i] } else { 0.0 }, 0.0)
        }),
        s: Mat::<T>::identity(n, n),
        kinetic,
    }
}

fn unit_columns<T: Parts>() -> Mat<T> {
    Mat::from_fn(40, 4, |g, j| entry::<T>(f64::from(u8::from(g == j)), 0.0))
}

#[test]
fn a_guess_spanning_the_exact_subspace_gives_its_values() {
    a_guess_spanning_the_exact_subspace_gives_its_values_in::<c64>();
    a_guess_spanning_the_exact_subspace_gives_its_values_in::<f64>();
}

fn a_guess_spanning_the_exact_subspace_gives_its_values_in<T: Parts>() {
    let (t, n) = (diagonal::<T>(), 40);
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let guess = Mat::from_fn(n, 4, |g, j| {
        let pair = 2 * (j / 2);
        let sign = if j % 2 == 0 || g == pair { 1.0 } else { -1.0 };
        entry::<T>(
            if g / 2 == j / 2 && g < 4 {
                sign * half
            } else {
                0.0
            },
            0.0,
        )
    });
    let pairs = solve(&t, guess, 4, 1e-10, 50).unwrap();
    for (got, want) in pairs.values().iter().zip([0.10, 0.15, 0.20, 0.25]) {
        assert!((got - want).abs() < 1e-12, "{got} vs {want}");
    }
}

#[test]
fn a_guess_whose_first_residuals_coincide_still_converges() {
    a_guess_whose_first_residuals_coincide_still_converges_in::<c64>();
    a_guess_whose_first_residuals_coincide_still_converges_in::<f64>();
}

fn a_guess_whose_first_residuals_coincide_still_converges_in<T: Parts>() {
    let n = 40;
    let kinetic: Vec<f64> = (0..n).map(|g| 0.1 + 0.05 * (g / 2) as f64).collect();
    let mut r = stream(77);
    let w: Vec<[f64; 3]> = (0..n).map(|_| [r(), r(), r()]).collect();
    let couple = |g: usize, j: usize| 0.05 * w[g][[0, 0, 1, 2][j]];
    let h = Mat::from_fn(n, n, |i, j| {
        let v = match (i, j) {
            _ if i == j => kinetic[i],
            _ if i >= 4 && j < 4 => couple(i, j),
            _ if j >= 4 && i < 4 => couple(j, i),
            _ => 0.0,
        };
        entry::<T>(v, 0.0)
    });
    let t = Toy {
        kinetic,
        h,
        s: Mat::<T>::identity(n, n),
    };
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let pairs = solve(&t, unit_columns::<T>(), 4, 1e-10, 100).unwrap();
    for (got, want) in pairs.values().iter().zip(&exact) {
        assert!((got - want).abs() < 1e-11, "{got} vs {want}");
    }
}

#[test]
fn zero_iterations_return_the_rayleigh_quotients_of_the_guess() {
    zero_iterations_return_the_rayleigh_quotients_of_the_guess_in::<c64>();
    zero_iterations_return_the_rayleigh_quotients_of_the_guess_in::<f64>();
}

fn zero_iterations_return_the_rayleigh_quotients_of_the_guess_in<T: Parts>() {
    let t = diagonal::<T>();
    let pairs = solve(&t, unit_columns::<T>(), 4, 1e-10, 0).unwrap_err();
    assert_eq!(pairs.values(), &t.kinetic[..4]);
    assert_eq!(pairs.residuals(), [0.0; 4]);
}

#[test]
fn zero_iterations_give_exact_quotients_under_an_ill_conditioned_s() {
    zero_iterations_give_exact_quotients_under_an_ill_conditioned_s_in::<c64>();
    zero_iterations_give_exact_quotients_under_an_ill_conditioned_s_in::<f64>();
}

fn zero_iterations_give_exact_quotients_under_an_ill_conditioned_s_in<T: Parts>() {
    let n = 40;
    let mut s = Mat::<T>::identity(n, n);
    s[(1, 1)] = entry::<T>(1e-10, 0.0);
    let h = Mat::from_fn(n, n, |i, j| s[(i, j)] * 2.0);
    let t = Toy {
        kinetic: (0..n).map(|g| 0.1 + 0.05 * g as f64).collect(),
        h,
        s,
    };
    let half = std::f64::consts::FRAC_1_SQRT_2;
    let guess = Mat::from_fn(n, 4, |g, j| match (g, j) {
        (0, 0 | 1) | (1, 0) => entry::<T>(half, 0.0),
        (1, 1) => entry::<T>(-half, 0.0),
        (g, j) if j > 1 && g == j => entry::<T>(1.0, 0.0),
        _ => entry::<T>(0.0, 0.0),
    });
    let pairs = solve(&t, guess, 4, 1e-10, 0).unwrap_err();
    for v in pairs.values() {
        assert!((v - 2.0).abs() < 1e-14, "{v}");
    }
}

#[test]
fn a_residual_equal_to_the_tolerance_does_not_converge() {
    a_residual_equal_to_the_tolerance_does_not_converge_in::<c64>();
    a_residual_equal_to_the_tolerance_does_not_converge_in::<f64>();
}

fn a_residual_equal_to_the_tolerance_does_not_converge_in<T: Parts>() {
    let t = toy::<T>(60, 3);
    let guess = random_block::<T>(60, 3, 5);
    let tolerance = solve(&t, guess.clone(), 1, 0.0, 1).unwrap_err().residuals()[0];
    let pairs = solve(&t, guess, 1, tolerance, 200).unwrap();
    assert!(
        pairs.residuals()[0] < tolerance,
        "{:?} vs {tolerance}",
        pairs.residuals()
    );
}

#[test]
fn an_exact_guess_still_converges() {
    an_exact_guess_still_converges_in::<c64>();
    an_exact_guess_still_converges_in::<f64>();
}

fn an_exact_guess_still_converges_in<T: Parts>() {
    let t = diagonal::<T>();
    let pairs = solve(&t, unit_columns::<T>(), 4, 1e-10, 50).unwrap();
    for (got, want) in pairs.values().iter().zip(&t.kinetic) {
        assert!((got - want).abs() < 1e-14, "{got} vs {want}");
    }
}

#[test]
fn a_guess_with_a_zero_column_still_converges() {
    a_guess_with_a_zero_column_still_converges_in::<c64>();
    a_guess_with_a_zero_column_still_converges_in::<f64>();
}

fn a_guess_with_a_zero_column_still_converges_in<T: Parts>() {
    let t = toy::<T>(200, 11);
    let mut guess = random_block::<T>(200, 8, 12);
    guess.col_mut(5).fill(entry::<T>(0.0, 0.0));
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let pairs = solve(&t, guess, 6, 1e-9, 300).unwrap();
    for (got, want) in pairs.values().iter().zip(&exact).take(6) {
        assert!((got - want).abs() < 1e-11, "{got} vs {want}");
    }
}

#[test]
fn free_electrons_scaled_by_four_take_the_same_steps() {
    free_electrons_scaled_by_four_take_the_same_steps_in::<c64>();
    free_electrons_scaled_by_four_take_the_same_steps_in::<f64>();
}

fn free_electrons_scaled_by_four_take_the_same_steps_in<T: Parts>() {
    let runs = [1.0, 4.0].map(|scale| {
        let t = free_electrons::<T>();
        let t = Toy {
            kinetic: t.kinetic.iter().map(|k| scale * k).collect(),
            h: Mat::from_fn(t.h.nrows(), t.h.ncols(), |i, j| t.h[(i, j)] * scale),
            s: t.s.clone(),
        };
        let n = t.kinetic.len();
        let (pairs, applied) =
            solve_counting(&t, random_block::<T>(n, 10, 6), 7, 1e-10 * scale, 200);
        (pairs.unwrap(), applied)
    });
    let [(one, applied), (four, applied_four)] = runs;
    assert_eq!(applied_four, applied);
    for (a, b) in four.values().iter().zip(one.values()).take(7) {
        assert!((a - 4.0 * b).abs() < 4e-12, "{a} vs 4 × {b}");
    }
}

#[test]
fn random_entries_lie_in_the_half_open_square_about_zero() {
    random_entries_lie_in_the_half_open_square_about_zero_in::<c64>();
    random_entries_lie_in_the_half_open_square_about_zero_in::<f64>();
}

fn random_entries_lie_in_the_half_open_square_about_zero_in<T: Parts>() {
    let block = random::<T>(500, 2, 3);
    let mut parts = Vec::new();
    for j in 0..2 {
        for i in 0..500 {
            parts.extend(block[(i, j)].parts());
        }
    }
    assert!(parts.iter().all(|x| (-0.5..0.5).contains(x)));
    let (low, high) = parts
        .iter()
        .fold((0.0_f64, 0.0_f64), |(l, h), &x| (l.min(x), h.max(x)));
    assert!(low < -0.49 && high > 0.49, "{low} … {high}");
}

fn with_images<T: Parts>(t: &Toy<T>, x: Mat<T>) -> Guess<T> {
    let (h, s) = (|x: MatRef<'_, T>| &t.h * x, |x: MatRef<'_, T>| &t.s * x);
    let pencil = Pencil {
        h: &h,
        s: &s,
        kinetic_ha: &t.kinetic,
    };
    Guess::Images(Applied::of(&pencil, x))
}

#[test]
fn a_start_with_images_reaches_the_dense_solution() {
    a_start_with_images_reaches_the_dense_solution_in::<c64>();
    a_start_with_images_reaches_the_dense_solution_in::<f64>();
}

fn a_start_with_images_reaches_the_dense_solution_in<T: Parts>() {
    let t = toy::<T>(240, 1);
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let start = with_images(&t, random_block(240, 12, 2));
    let pairs = solve_in(&t, start, 9, 1e-10, 200, &mut Workspace::default())
        .0
        .unwrap();
    for (i, (got, want)) in pairs.values().iter().zip(&exact).take(9).enumerate() {
        assert!((got - want).abs() < 1e-12, "band {i}: {got} vs {want}");
    }
    assert!(s_orthonormality_error(&t, pairs.vectors()) < 1e-12);
}

#[test]
fn a_start_with_images_applies_h_to_none_of_its_columns_before_the_first_iteration() {
    a_start_with_images_applies_h_to_none_of_its_columns_before_the_first_iteration_in::<c64>();
    a_start_with_images_applies_h_to_none_of_its_columns_before_the_first_iteration_in::<f64>();
}

fn a_start_with_images_applies_h_to_none_of_its_columns_before_the_first_iteration_in<T: Parts>() {
    let t = toy::<T>(240, 1);
    let x = random_block::<T>(240, 12, 2);
    let work = &mut Workspace::default();
    let (images, applied) = solve_in(&t, with_images(&t, x.clone()), 12, 0.0, 0, work);
    assert_eq!(applied, 0);
    let (vectors, applied) = solve_in(&t, x, 12, 0.0, 0, work);
    assert_eq!(applied, 12);
    let trace = |p: &Eigenpairs<T>| p.values().iter().sum::<f64>();
    let (images, vectors) = (images.unwrap_err(), vectors.unwrap_err());
    assert!((trace(&images) - trace(&vectors)).abs() < 1e-10);
    assert!(s_orthonormality_error(&t, images.vectors()) < 1e-12);
}

#[test]
fn a_rank_deficient_start_with_images_falls_back_to_its_vectors() {
    a_rank_deficient_start_with_images_falls_back_to_its_vectors_in::<c64>();
    a_rank_deficient_start_with_images_falls_back_to_its_vectors_in::<f64>();
}

fn a_rank_deficient_start_with_images_falls_back_to_its_vectors_in<T: Parts>() {
    let t = toy::<T>(240, 1);
    let (exact, _) = dense(t.h.as_ref(), t.s.as_ref()).unwrap();
    let mut x = random_block::<T>(240, 12, 2);
    x.col_mut(3).fill(entry::<T>(0.0, 0.0));
    let (zero, applied) = solve_in(
        &t,
        with_images(&t, x.clone()),
        9,
        1e-10,
        0,
        &mut Workspace::default(),
    );
    assert!(zero.is_err());
    assert_eq!(applied, 12);
    let pairs = solve_in(
        &t,
        with_images(&t, x),
        9,
        1e-10,
        200,
        &mut Workspace::default(),
    )
    .0
    .unwrap();
    for (got, want) in pairs.values().iter().zip(&exact).take(9) {
        assert!((got - want).abs() < 1e-12);
    }
}

#[test]
fn random_takes_one_draw_per_real_entry_and_two_per_complex_one() {
    let mut r = stream(21);
    let draws: Vec<f64> = (0..6).map(|_| r()).collect();
    let real = random::<f64>(3, 2, 21);
    let complex = random::<c64>(3, 1, 21);
    for (k, (i, j)) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)]
        .into_iter()
        .enumerate()
    {
        assert_eq!(real[(i, j)].to_bits(), draws[k].to_bits());
    }
    for i in 0..3 {
        assert_eq!(
            complex[(i, 0)].parts().map(f64::to_bits),
            [draws[2 * i], draws[2 * i + 1]].map(f64::to_bits)
        );
    }
}
