use orbitar::paw::{RadialGrid, extrapolated};

fn grid(size: usize) -> RadialGrid {
    RadialGrid::logarithmic(1e-3, 0.01, size).unwrap()
}

#[test]
fn radii_start_at_zero_and_follow_the_logarithmic_rule() {
    let r = grid(5).radii().to_vec();
    assert_eq!(r[0], 0.0);
    for (i, r) in r.iter().enumerate() {
        assert_eq!(*r, 1e-3 * ((0.01 * i as f64).exp() - 1.0));
    }
}

#[test]
fn a_grid_needs_positive_parameters_finite_radii_and_three_points() {
    for (a, d, size) in [
        (0.0, 0.01, 10),
        (1e-3, 0.0, 10),
        (1e-3, -0.01, 10),
        (1e-3, 0.01, 2),
        (1e-3, 800.0, 10),
        (1e300, 10.0, 10),
    ] {
        let err = RadialGrid::logarithmic(a, d, size).unwrap_err();
        assert!(err.starts_with("radial_grid:"), "{err}");
    }
    assert!(RadialGrid::logarithmic(1e-3, 0.01, 3).is_ok());
}

#[test]
fn integration_is_exact_for_a_quadratic_in_the_index_for_odd_and_even_counts() {
    let g = grid(40);
    let dr: Vec<f64> = (0..40)
        .map(|i| 1e-3 * 0.01 * (0.01 * i as f64).exp())
        .collect();
    for n in [3, 4, 9, 10, 39, 40] {
        let f: Vec<f64> = (0..n)
            .map(|i| (3.0 * (i * i) as f64 - 2.0 * i as f64 + 5.0) / dr[i])
            .collect();
        let m = (n - 1) as f64;
        let exact = m.powi(3) - m * m + 5.0 * m;
        let got = g.integrate(&f);
        assert!(
            (got - exact).abs() < 1e-11 * exact,
            "{n} points: {got} vs {exact}"
        );
    }
}

#[test]
fn the_weights_of_three_and_four_points_are_simpsons_and_the_start_correction() {
    let g = grid(10);
    let dr: Vec<f64> = (0..4)
        .map(|i| 1e-3 * 0.01 * (0.01 * i as f64).exp())
        .collect();
    for (n, weights) in [
        (3, vec![1.0 / 3.0, 4.0 / 3.0, 1.0 / 3.0]),
        (4, vec![5.0 / 12.0, 1.0, 5.0 / 4.0, 1.0 / 3.0]),
    ] {
        for (k, want) in weights.iter().enumerate() {
            let f: Vec<f64> = (0..n)
                .map(|i| if i == k { 1.0 / dr[i] } else { 0.0 })
                .collect();
            let got = g.integrate(&f);
            assert!((got - want).abs() < 1e-14, "{n} points, weight {k}: {got}");
        }
    }
}

#[test]
fn integration_converges_to_a_closed_form_on_a_dataset_like_grid() {
    let g = RadialGrid::logarithmic(4.3309254207421976e-4, 6.063_295_589_039_077e-3, 2001).unwrap();
    let f: Vec<f64> = g.radii().iter().map(|r| r * r * (-r).exp()).collect();
    assert!((g.integrate(&f) - 2.0).abs() < 1e-12);
    assert!(
        (g.integrate(&f[..2000]) - 2.0).abs() < 1e-12,
        "the even-count branch"
    );
}

#[test]
fn a_bessel_transform_of_a_gaussian_takes_its_closed_form() {
    let g = RadialGrid::logarithmic(4.3309254207421976e-4, 6.063_295_589_039_077e-3, 2001).unwrap();
    let alpha = 1.3_f64;
    for l in 0..=3 {
        let f: Vec<f64> = g
            .radii()
            .iter()
            .map(|r| r.powi(l) * (-alpha * r * r).exp())
            .collect();
        for q in [0.0_f64, 0.7, 3.1, 9.0] {
            let want = std::f64::consts::PI.sqrt() * q.powi(l) * (-q * q / (4.0 * alpha)).exp()
                / (2f64.powi(l + 2) * alpha.powf(f64::from(l) + 1.5));
            assert!(
                (g.bessel_transform(&f, l as usize, q) - want).abs() < 1e-14,
                "l {l}, q {q}"
            );
        }
    }
}

#[test]
fn nearest_takes_the_closer_point_and_the_first_on_a_tie() {
    let g = grid(100);
    let r = g.radii().to_vec();
    assert_eq!(g.nearest(r[37]), 37);
    assert_eq!(g.nearest(0.49 * r[37] + 0.51 * r[38]), 38);
    assert_eq!(g.nearest(0.51 * r[37] + 0.49 * r[38]), 37);
    assert_eq!(g.nearest(-1.0), 0);
    assert_eq!(g.nearest(1e9), 99);
}

fn dataset_grid() -> RadialGrid {
    RadialGrid::logarithmic(4.3309254207421976e-4, 6.063_295_589_039_077e-3, 2001).unwrap()
}

#[test]
fn index_below_inverts_the_grid_rule() {
    let g = grid(100);
    let r = g.radii();
    assert_eq!(g.index_below(r[37]), 37);
    assert_eq!(g.index_below(0.5 * (r[37] + r[38])), 37);
    assert_eq!(g.index_below(0.999 * r[38]), 37);
    assert_eq!(g.index_below(0.0), 0);
}

#[test]
fn five_point_derivatives_follow_a_smooth_function_to_its_ends() {
    let g = dataset_grid();
    let r = &g.radii()[..1500];
    let f: Vec<f64> = r.iter().map(|r| (1.0 + r * r) * (-r).exp()).collect();
    for (got, r) in g.derivative(&f).iter().zip(r) {
        let want = (2.0 * r - 1.0 - r * r) * (-r).exp();
        assert!((got - want).abs() < 1e-8, "r = {r}: {got} vs {want}");
    }
}

#[test]
fn poisson_gives_the_potentials_of_gaussian_multipoles() {
    let g = dataset_grid();
    let alpha = 1.3_f64;
    let r = g.radii();
    let tail = |r: f64| (-alpha * r * r).exp() / (2.0 * alpha);
    let erf = |r: f64| libm::erf(alpha.sqrt() * r);
    let sqrt_pi = std::f64::consts::PI.sqrt();
    let inner = [
        |r: f64, alpha: f64, erf: f64, sqrt_pi: f64| {
            sqrt_pi * erf / (4.0 * alpha.powf(1.5)) - r * (-alpha * r * r).exp() / (2.0 * alpha)
        },
        |r: f64, alpha: f64, erf: f64, sqrt_pi: f64| {
            3.0 * sqrt_pi * erf / (8.0 * alpha.powf(2.5))
                - (-alpha * r * r).exp()
                    * (3.0 * r / (4.0 * alpha * alpha) + r.powi(3) / (2.0 * alpha))
        },
    ];
    for (l, inner) in inner.iter().enumerate() {
        let rho: Vec<f64> = r
            .iter()
            .map(|r| r * r * r.powi(l as i32) * (-alpha * r * r).exp())
            .collect();
        for count in [r.len(), r.len() - 1] {
            let rv = g.poisson(&rho[..count], l);
            for (i, &x) in r[..count].iter().enumerate().skip(1).step_by(50) {
                let want = (x.powi(-(l as i32) - 1) * inner(x, alpha, erf(x), sqrt_pi)
                    + x.powi(l as i32) * tail(x))
                    / (2 * l + 1) as f64;
                assert!(
                    (rv[i] / x - want).abs() < 1e-9,
                    "l {l}, r {x}: {} vs {want}",
                    rv[i] / x
                );
            }
            assert_eq!(rv[0], 0.0);
        }
    }
}

#[test]
fn the_value_at_the_origin_follows_the_parabola_through_the_next_three() {
    assert_eq!(extrapolated([6.0, 17.0, 34.0]), 1.0);
}
