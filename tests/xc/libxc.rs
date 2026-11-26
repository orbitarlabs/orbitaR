use orbitar::model::Functional;
use orbitar::xc::{
    Inputs, Point, TAU_FLOOR_HA_PER_BOHR3, Unpolarized, Value, evaluate, unpolarized,
};

const EXCHANGE: f64 = 1e-11;
const CORRELATION: f64 = 1e-15;

fn libxc_rows() -> impl Iterator<Item = Functional> {
    (Functional::ALL.into_iter())
        .filter(|row| row.inputs == Inputs::Libxc)
        .map(|row| row.functional)
}

fn numbers(v: Value) -> Vec<f64> {
    [
        [v.energy, v.epsilon].as_slice(),
        &v.vrho,
        &v.vsigma,
        &v.vtau,
    ]
    .concat()
}

fn zero(v: Value) -> bool {
    numbers(v).iter().all(|&x| x == 0.0)
}

fn zero_unpolarized(u: Unpolarized) -> bool {
    [u.energy, u.epsilon, u.vrho, u.vsigma, u.vtau]
        .iter()
        .all(|&x| x == 0.0)
}

#[test]
fn each_part_skips_a_point_whose_density_is_below_its_threshold() {
    for functional in libxc_rows() {
        let at = |n: f64| {
            let p = Point::of(0.6 * n, 0.4 * n, [1e-40, 0.0, 1e-40]).with_tau([1e-25; 2]);
            (
                evaluate(functional, p),
                unpolarized(functional, n, 1e-40, Some(2e-25)),
            )
        };
        let ([x, c], [ux, uc]) = at(0.5 * EXCHANGE);
        assert!(zero(x) && zero_unpolarized(ux), "{functional:?}");
        assert!(c.energy < 0.0 && uc.energy < 0.0, "{functional:?}");
        let ([x, c], [ux, uc]) = at(0.5 * CORRELATION);
        assert!(zero(x) && zero(c), "{functional:?}");
        assert!(
            zero_unpolarized(ux) && zero_unpolarized(uc),
            "{functional:?}"
        );
    }
}

#[test]
fn the_exchange_drops_a_spin_at_or_below_its_threshold() {
    for functional in libxc_rows() {
        let at = |down: f64| {
            let p = Point::of(0.1, down, [0.01, 0.0, 1e-30]).with_tau([0.06, 1e-12]);
            evaluate(functional, p)[0]
        };
        let (dropped, lower) = (at(EXCHANGE), at(0.3 * EXCHANGE));
        assert_eq!(dropped.epsilon, lower.epsilon, "{functional:?}");
        assert_eq!(
            [dropped.vrho[0], dropped.vsigma[0], dropped.vtau[0]],
            [lower.vrho[0], lower.vsigma[0], lower.vtau[0]],
            "{functional:?}"
        );
        assert_eq!(
            [dropped.vrho[1], dropped.vsigma[2], dropped.vtau[1]],
            [0.0; 3],
            "{functional:?}"
        );
        assert_eq!(lower.energy, lower.epsilon * (0.1 + 0.3 * EXCHANGE));
        assert!(at(3.0 * EXCHANGE).vrho[1] < 0.0, "{functional:?}");
        let [x, _] = unpolarized(functional, 1.5 * EXCHANGE, 1e-30, Some(1e-18));
        assert!(zero_unpolarized(x), "{functional:?}");
    }
}

#[test]
fn a_tau_below_the_floor_reads_as_the_floor() {
    for functional in libxc_rows() {
        let at = |tau: f64| {
            let p = Point::of(5e-12, 3e-12, [1e-32, 0.0, 1e-32]).with_tau([tau, 1e-19]);
            evaluate(functional, p)
        };
        assert_eq!(at(0.0), at(TAU_FLOOR_HA_PER_BOHR3), "{functional:?}");
        assert_eq!(at(-1e-9), at(TAU_FLOOR_HA_PER_BOHR3), "{functional:?}");
        assert_ne!(
            at(2.0 * TAU_FLOOR_HA_PER_BOHR3)[1],
            at(TAU_FLOOR_HA_PER_BOHR3)[1],
            "{functional:?}"
        );
    }
}

#[test]
fn each_sigma_past_its_bound_reads_as_the_bound() {
    let (up, down, tau) = (0.1, 0.08, [0.01, 0.05]);
    let [b_up, b_down] = [8.0 * up * tau[0], 8.0 * down * tau[1]];
    let pairs = [
        ([2.0 * b_up, 1e-3, 1e-3], [5.0 * b_up, 1e-3, 1e-3]),
        (
            [2.0 * b_up, 0.0, 3.0 * b_down],
            [4.0 * b_up, 0.0, 9.0 * b_down],
        ),
        (
            [2.0 * b_up, 0.9 * b_up, 1e-3],
            [2.0 * b_up, 7.0 * b_up, 1e-3],
        ),
        (
            [2.0 * b_up, -0.9 * b_up, 1e-3],
            [2.0 * b_up, -7.0 * b_up, 1e-3],
        ),
    ];
    for functional in libxc_rows() {
        let at = |sigma: [f64; 3]| evaluate(functional, Point::of(up, down, sigma).with_tau(tau));
        for (a, b) in pairs {
            assert_eq!(at(a), at(b), "{functional:?} σ {a:?} and {b:?}");
        }
        assert_ne!(at([0.5 * b_up, 0.0, 1e-3]), at([0.6 * b_up, 0.0, 1e-3]));
        let (n, t) = (0.2, 0.03);
        let u = |sigma: f64| unpolarized(functional, n, sigma, Some(t));
        assert_eq!(u(2.0 * 8.0 * n * t), u(3.0 * 8.0 * n * t), "{functional:?}");
    }
}
