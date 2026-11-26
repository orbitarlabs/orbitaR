use orbitar::paw::{
    gauss_legendre, real_ylm, reduced_jn, solid_harmonics, spherical_jn, ylm_rotation,
};
use std::f64::consts::PI;

#[test]
fn real_harmonics_up_to_l2_take_the_closed_forms_without_the_condon_shortley_phase() {
    let [x, y, z] = [0.3_f64, -0.5, 0.8];
    let n = (x * x + y * y + z * z).sqrt();
    let [x, y, z] = [x / n, y / n, z / n];
    let c = |k: f64| (k / (4.0 * PI)).sqrt();
    let expected = [
        c(1.0),
        c(3.0) * y,
        c(3.0) * z,
        c(3.0) * x,
        c(15.0) * x * y,
        c(15.0) * y * z,
        c(5.0 / 4.0) * (3.0 * z * z - 1.0),
        c(15.0) * x * z,
        c(15.0 / 4.0) * (x * x - y * y),
    ];
    let ylm = real_ylm(2, [0.6, -1.0, 1.6]);
    assert_eq!(ylm.len(), 9);
    for (k, (got, want)) in ylm.iter().zip(expected).enumerate() {
        assert!((got - want).abs() < 1e-15, "index {k}: {got} vs {want}");
    }
}

#[test]
fn real_harmonics_up_to_l6_are_orthonormal_on_the_sphere() {
    let mut gram = vec![0.0; 49 * 49];
    for (t, w) in gauss_legendre(8) {
        let s = (1.0 - t * t).sqrt();
        for k in 0..16 {
            let phi = PI * k as f64 / 8.0;
            let ylm = real_ylm(6, [s * phi.cos(), s * phi.sin(), t]);
            for a in 0..49 {
                for b in 0..49 {
                    gram[a * 49 + b] += w * PI / 8.0 * ylm[a] * ylm[b];
                }
            }
        }
    }
    for a in 0..49 {
        for b in 0..49 {
            let want = if a == b { 1.0 } else { 0.0 };
            assert!(
                (gram[a * 49 + b] - want).abs() < 1e-13,
                "⟨{a}|{b}⟩ = {}",
                gram[a * 49 + b]
            );
        }
    }
}

#[test]
fn real_harmonics_at_l3_take_the_closed_forms_with_cos_for_positive_m() {
    let [x, y, z] = [0.48_f64, 0.6, 0.64];
    let c = |k: f64| (k / PI).sqrt();
    let expected = [
        c(35.0 / 32.0) * y * (3.0 * x * x - y * y),
        c(105.0 / 4.0) * x * y * z,
        c(21.0 / 32.0) * y * (5.0 * z * z - 1.0),
        c(7.0 / 16.0) * z * (5.0 * z * z - 3.0),
        c(21.0 / 32.0) * x * (5.0 * z * z - 1.0),
        c(105.0 / 16.0) * z * (x * x - y * y),
        c(35.0 / 32.0) * x * (x * x - 3.0 * y * y),
    ];
    let ylm = real_ylm(3, [x, y, z]);
    for (m, want) in expected.iter().enumerate() {
        assert!((ylm[9 + m] - want).abs() < 1e-15, "m = {}", m as i32 - 3);
    }
}

#[test]
fn a_zero_direction_reads_as_the_z_axis() {
    let ylm = real_ylm(3, [0.0; 3]);
    for l in 0..=3_i32 {
        for m in -l..=l {
            let want = if m == 0 {
                f64::from(2 * l + 1).sqrt() / (4.0 * PI).sqrt()
            } else {
                0.0
            };
            assert!((ylm[(l * l + l + m) as usize] - want).abs() < 1e-15);
        }
    }
}

#[test]
fn spherical_bessel_functions_match_scipy_on_both_sides_of_the_series_switch() {
    let xs = [1e-6, 0.3, 1.0, 2.9, 6.0, 25.0];
    let table: [[f64; 6]; 7] = [
        [
            0.9999999999998334,
            0.9850673555377986,
            0.8414709848078965,
            0.0824997686944767,
            -0.04656924969982098,
            -0.005294070003910922,
        ],
        [
            3.3333333333330117e-07,
            0.09910288804064196,
            0.3011686789397571,
            0.36326135649795427,
            -0.16778992272503115,
            -0.03985987527469538,
        ],
        [
            6.6666666666662e-14,
            0.00596152486862022,
            0.062035052011373916,
            0.29328784147582077,
            -0.0373257116626946,
            0.0005108849709474757,
        ],
        [
            9.523809523808996e-21,
            0.0002558597696950819,
            0.009006581117112524,
            0.14240733570173686,
            0.136685163006119,
            0.03996205226888488,
        ],
        [
            1.0582010582010156e-27,
            8.536424265025156e-06,
            0.0010110158084137538,
            0.05045400332147502,
            0.19679173516983342,
            0.01067848966434029,
        ],
        [
            9.620009620009354e-35,
            2.3295825567290316e-07,
            9.256115861125825e-05,
            0.014174053916633757,
            0.15850243974863115,
            -0.036117795989722375,
        ],
        [
            7.400007400007188e-42,
            5.378442981277006e-09,
            7.156936310087096e-06,
            0.003309649465756503,
            0.09379607103599083,
            -0.026570319899818135,
        ],
    ];
    for l in 0..=6 {
        assert_eq!(spherical_jn(l, 0.0), if l == 0 { 1.0 } else { 0.0 });
    }
    for (l, row) in table.iter().enumerate() {
        for (&x, &want) in xs.iter().zip(row) {
            let got = spherical_jn(l, x);
            assert!(
                (got - want).abs() <= 2e-14 * want.abs(),
                "j_{l}({x}) = {got}, scipy {want}"
            );
        }
    }
}

#[test]
fn gauss_legendre_with_n_points_integrates_degree_2n_minus_1() {
    let rule = orbitar::paw::gauss_legendre(5);
    assert!((rule.iter().map(|(_, w)| w).sum::<f64>() - 2.0).abs() < 1e-15);
    for k in 0..10 {
        let got: f64 = rule.iter().map(|(x, w)| w * x.powi(k)).sum();
        let want = if k % 2 == 0 {
            2.0 / (k + 1) as f64
        } else {
            0.0
        };
        assert!((got - want).abs() < 1e-15, "x^{k}: {got} vs {want}");
    }
}

#[test]
fn gaunt_coefficients_expand_a_product_of_two_harmonics() {
    let gaunt = orbitar::paw::Gaunt::up_to(3, 6);
    for direction in [[0.3, -0.5, 0.8], [-0.9, 0.1, 0.2], [0.0, 0.0, -1.0]] {
        let y = real_ylm(6, direction);
        for a in 0..16 {
            for b in 0..16 {
                let sum: f64 = (0..49).map(|big| gaunt.get(big, a, b) * y[big]).sum();
                assert!(
                    (sum - y[a] * y[b]).abs() < 1e-14,
                    "{a} {b}: {sum} vs {}",
                    y[a] * y[b]
                );
            }
        }
    }
    let y00 = 1.0 / (4.0 * PI).sqrt();
    for a in 0..16 {
        for b in 0..16 {
            let want = if a == b { y00 } else { 0.0 };
            assert!((gaunt.get(0, a, b) - want).abs() < 1e-15);
        }
    }
}

#[test]
fn each_gaunt_coefficient_is_zero_or_larger_than_1e_3() {
    let gaunt = orbitar::paw::Gaunt::up_to(3, 6);
    for big in 0..49 {
        for a in 0..16 {
            for b in 0..16 {
                let g = gaunt.get(big, a, b).abs();
                assert!(g == 0.0 || g > 1e-3, "G({big}, {a}, {b}) = {g}");
            }
        }
    }
}

fn rotation(axis: [f64; 3], angle: f64, improper: bool) -> [[f64; 3]; 3] {
    let (s, c) = angle.sin_cos();
    let sign = if improper { -1.0 } else { 1.0 };
    let cross = [
        [0.0, -axis[2], axis[1]],
        [axis[2], 0.0, -axis[0]],
        [-axis[1], axis[0], 0.0],
    ];
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let identity = if i == j { 1.0 } else { 0.0 };
            sign * (c * identity + s * cross[i][j] + (1.0 - c) * axis[i] * axis[j])
        })
    })
}

#[test]
fn the_rotation_matrix_maps_each_harmonic_onto_its_rotated_self_within_its_l() {
    let lmax = 3;
    let norm = (1.0f64 + 4.0 + 9.0).sqrt();
    for w in [
        rotation([1.0 / norm, 2.0 / norm, 3.0 / norm], 0.7, false),
        rotation([0.0, 0.0, 1.0], PI / 2.0, true),
    ] {
        let d = ylm_rotation(lmax, w);
        let size = (lmax + 1) * (lmax + 1);
        for r in [[0.3, -0.4, 0.5], [-1.0, 0.2, 0.1]] {
            let turned: [f64; 3] = std::array::from_fn(|i| (0..3).map(|j| w[i][j] * r[j]).sum());
            let (y, want) = (real_ylm(lmax, r), real_ylm(lmax, turned));
            for a in 0..size {
                let got: f64 = (0..size).map(|b| d[(a, b)] * y[b]).sum();
                assert!((got - want[a]).abs() < 1e-13, "lm {a}");
            }
        }
        let product = &d * d.transpose();
        let l = |a: usize| (a as f64).sqrt() as usize;
        for a in 0..size {
            for b in 0..size {
                assert!((product[(a, b)] - f64::from(u8::from(a == b))).abs() < 1e-13);
                assert!(l(a) == l(b) || d[(a, b)] == 0.0);
            }
        }
    }
}

#[test]
fn solid_harmonics_are_r_to_the_l_times_the_real_harmonics_with_their_gradients() {
    let v = [0.7, -0.4, 1.1];
    let r = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    let solid = solid_harmonics(6, v);
    let y = real_ylm(6, v);
    let h = 1e-5;
    for (lm, (value, gradient)) in solid.iter().enumerate() {
        let l = lm.isqrt() as i32;
        assert!((value - r.powi(l) * y[lm]).abs() < 1e-13, "{lm}");
        for axis in 0..3 {
            let at = |t: f64| {
                let mut w = v;
                w[axis] += t * h;
                solid_harmonics(6, w)[lm].0
            };
            let fd = (8.0 * (at(1.0) - at(-1.0)) - (at(2.0) - at(-2.0))) / (12.0 * h);
            assert!(
                (gradient[axis] - fd).abs() < 1e-9,
                "{lm} {axis}: {} vs {fd}",
                gradient[axis]
            );
        }
    }
    let origin = solid_harmonics(2, [0.0; 3]);
    let c = (3.0 / (4.0 * PI)).sqrt();
    for (lm, axis) in [(1, 1), (2, 2), (3, 0)] {
        assert!((origin[lm].1[axis] - c).abs() < 1e-15);
    }
    assert!((origin[0].0 - 1.0 / (4.0 * PI).sqrt()).abs() < 1e-15);
    assert!(origin[1..].iter().all(|(value, _)| *value == 0.0));
}

#[test]
fn the_reduced_bessel_function_is_j_l_over_x_to_the_l_and_finite_at_zero() {
    for l in 0..=6 {
        let double_factorial: f64 = (1..=l).map(|k| (2 * k + 1) as f64).product();
        assert!((reduced_jn(l, 0.0) - 1.0 / double_factorial).abs() < 1e-16);
        for x in [0.3, 2.0, 7.5] {
            let want = spherical_jn(l, x) / x.powi(l as i32);
            assert!(
                (reduced_jn(l, x) - want).abs() < 1e-14 * want.abs().max(1e-3),
                "{l} {x}"
            );
        }
    }
}

#[test]
fn both_bessel_branches_agree_across_their_switch() {
    let ulps = |a: f64, b: f64| (a - b).abs() / (f64::EPSILON * a.abs());
    for l in 0..=10 {
        let above = (l as f64).max(1.0);
        let below = above.next_down();
        assert!(
            ulps(spherical_jn(l, above), spherical_jn(l, below)) < 20.0,
            "{l}"
        );
        assert!(
            ulps(reduced_jn(l, above), reduced_jn(l, below)) < 14.0,
            "{l}"
        );
        for x in [above, below] {
            assert!(
                ulps(spherical_jn(l, x), reduced_jn(l, x) * x.powi(l as i32)) < 4.0,
                "{l} {x}"
            );
        }
    }
}

#[test]
fn gaunt_coefficients_of_s_p_and_d_products_equal_sympys_exact_values() {
    let pi = std::f64::consts::PI;
    type Row = ((usize, i64), (usize, i64), (usize, i64), f64);
    let rows: [Row; 11] = [
        ((0, 0), (0, 0), (0, 0), 1.0 / (2.0 * pi.sqrt())),
        ((1, 0), (1, 0), (0, 0), 1.0 / (2.0 * pi.sqrt())),
        ((1, 0), (1, 0), (2, 0), 5f64.sqrt() / (5.0 * pi.sqrt())),
        ((1, 1), (1, 1), (2, 0), -(5f64.sqrt()) / (10.0 * pi.sqrt())),
        ((1, 1), (1, 1), (2, 2), 15f64.sqrt() / (10.0 * pi.sqrt())),
        ((2, 0), (2, 0), (2, 0), 5f64.sqrt() / (7.0 * pi.sqrt())),
        ((2, 0), (2, 0), (4, 0), 3.0 / (7.0 * pi.sqrt())),
        ((2, 2), (2, 2), (4, 0), 1.0 / (14.0 * pi.sqrt())),
        ((2, 1), (2, 1), (2, 0), 5f64.sqrt() / (14.0 * pi.sqrt())),
        ((2, 1), (2, 1), (4, 2), 5f64.sqrt() / (7.0 * pi.sqrt())),
        ((1, 0), (2, 0), (1, 0), 5f64.sqrt() / (5.0 * pi.sqrt())),
    ];
    let gaunt = orbitar::paw::Gaunt::up_to(2, 4);
    let index = |(l, m): (usize, i64)| (l * l + l) as i64 + m;
    for (a, b, big, want) in rows {
        let got = gaunt.get(index(big) as usize, index(a) as usize, index(b) as usize);
        assert!(
            (got - want).abs() < 1e-14,
            "{a:?} {b:?} {big:?}: {got} vs {want}"
        );
    }
}
