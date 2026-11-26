use super::functional::{Polarized, Real, Spin, Spinless};
use std::f64::consts::PI;

pub(super) fn spin_exchange<T: Real>(rho: T, _sigma: T, _tau: T) -> T {
    slater(rho * 2.0, (rho * 2.0).cbrt()) * 0.5
}

pub(super) fn correlation<T: Real>(point: Polarized<T>) -> T {
    point.n * pw92(PW_A, PW_FZ20, point.rs, point.spin)
}

pub(super) fn unpolarized_exchange<T: Real>(point: Spinless<T>) -> T {
    slater(point.n, point.c)
}

pub(super) fn unpolarized_correlation<T: Real>(point: Spinless<T>) -> T {
    let Spinless { n, rs, .. } = point;
    n * pw92_g(0, PW_A[0], rs, rs.sqrt())
}

const PW_ALPHA1: [f64; 3] = [0.21370, 0.20548, 0.11125];
const PW_BETA_PARAMAGNETIC: [f64; 4] = [7.5957, 3.5876, 1.6382, 0.49294];
const PW_BETA_FERROMAGNETIC: [f64; 4] = [14.1189, 6.1977, 3.3662, 0.62517];
const PW_BETA_STIFFNESS: [f64; 4] = [10.357, 3.6231, 0.88026, 0.49671];
const PW_A: [f64; 3] = [0.031091, 0.015545, 0.016887];
const PW_FZ20: f64 = 1.709921;
pub(in crate::xc) const PW_MOD_A: [f64; 3] = [0.0310907, 0.01554535, 0.0168869];

pub(super) fn slater<T: Real>(n: T, c: T) -> T {
    n * c * (-0.75 * (3.0 / PI).cbrt())
}

pub(super) fn pw92<T: Real>(a: [f64; 3], fz20: f64, rs: T, spin: Spin<T>) -> T {
    let sqrt_rs = rs.sqrt();
    let g = |k: usize| pw92_g(k, a[k], rs, sqrt_rs);
    interpolate([g(0), g(1), g(2)], fz20, spin)
}

pub(super) fn pw92_slope<T: Real>(a: [f64; 3], fz20: f64, rs: T, spin: Spin<T>) -> T {
    let sqrt_rs = rs.sqrt();
    let g = |k: usize| pw92_g_slope(k, a[k], rs, sqrt_rs);
    interpolate([g(0), g(1), g(2)], fz20, spin)
}

fn interpolate<T: Real>(
    [paramagnetic, ferromagnetic, minus_stiffness]: [T; 3],
    fz20: f64,
    spin: Spin<T>,
) -> T {
    let f = spin.f();
    paramagnetic + spin.z4 * f * (ferromagnetic - paramagnetic + minus_stiffness / fz20)
        - f * minus_stiffness / fz20
}

const PW_BETA: [[f64; 4]; 3] = [
    PW_BETA_PARAMAGNETIC,
    PW_BETA_FERROMAGNETIC,
    PW_BETA_STIFFNESS,
];

pub(super) fn pw92_g<T: Real>(k: usize, a: f64, rs: T, sqrt_rs: T) -> T {
    let [b1, b2, b3, b4] = PW_BETA[k];
    let s = sqrt_rs * b1 + rs * b2 + rs * sqrt_rs * b3 + rs * rs * b4;
    (rs * PW_ALPHA1[k] + 1.0) * (-2.0 * a) * (T::constant(1.0) / (s * (2.0 * a)) + 1.0).ln()
}

pub(super) fn pw92_g_slope<T: Real>(k: usize, a: f64, rs: T, sqrt_rs: T) -> T {
    let [b1, b2, b3, b4] = PW_BETA[k];
    let q = sqrt_rs * b1 + rs * b2 + rs * sqrt_rs * b3 + rs * rs * b4;
    let slope = T::constant(b1) / (sqrt_rs * 2.0) + b2 + sqrt_rs * (1.5 * b3) + rs * (2.0 * b4);
    (T::constant(1.0) / (q * (2.0 * a)) + 1.0).ln() * (-2.0 * a * PW_ALPHA1[k])
        + (rs * PW_ALPHA1[k] + 1.0) * slope * (2.0 * a) / (q * (q * (2.0 * a) + 1.0))
}

pub(super) fn exact_fz20() -> f64 {
    8.0 / (9.0 * (2.0_f64.powf(4.0 / 3.0) - 2.0))
}
