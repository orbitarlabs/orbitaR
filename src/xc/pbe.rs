use super::functional::{Polarized, Real, Spinless};
use super::lda::{PW_MOD_A, exact_fz20, pw92, pw92_g, slater};
use std::f64::consts::PI;

pub(super) fn spin_exchange<T: Real>(rho: T, sigma: T, _tau: T) -> T {
    exchange(rho * 2.0, sigma * 4.0) * 0.5
}

pub(super) fn correlation<T: Real>(point: Polarized<T>) -> T {
    let Polarized {
        n,
        c,
        rs,
        spin,
        sigma: [uu, ud, dd],
        ..
    } = point;
    correlation_from(
        n,
        c,
        rs,
        pw92(PW_MOD_A, exact_fz20(), rs, spin),
        spin.phi(),
        uu + ud * 2.0 + dd,
    )
}

pub(super) fn unpolarized_exchange<T: Real>(point: Spinless<T>) -> T {
    exchange_from(point.n, point.c, point.sigma)
}

pub(super) fn unpolarized_correlation<T: Real>(point: Spinless<T>) -> T {
    let Spinless {
        n, c, rs, sigma, ..
    } = point;
    let sqrt_rs = rs.sqrt();
    correlation_from(
        n,
        c,
        rs,
        pw92_g(0, PW_MOD_A[0], rs, sqrt_rs),
        T::constant(1.0),
        sigma,
    )
}

const KAPPA: f64 = 0.804;
pub(in crate::xc) const BETA: f64 = 0.06672455060314922;

fn exchange<T: Real>(n: T, sigma: T) -> T {
    if n.value() == 0.0 {
        return T::constant(0.0);
    }
    exchange_from(n, n.cbrt(), sigma)
}

fn exchange_from<T: Real>(n: T, c: T, sigma: T) -> T {
    let s2 = reduced_gradient_squared(n, c, sigma);
    let mu = BETA * PI * PI / 3.0;
    slater(n, c) * (-(T::constant(KAPPA) / (s2 * (mu / KAPPA) + 1.0)) + 1.0 + KAPPA)
}

pub(super) fn reduced_gradient_squared<T: Real>(n: T, c: T, sigma: T) -> T {
    let kf = c * (3.0 * PI * PI).cbrt();
    let w = kf * n * 2.0;
    sigma / (w * w)
}

pub(super) fn gamma() -> f64 {
    (1.0 - 2.0_f64.ln()) / (PI * PI)
}

pub(super) fn t_squared<T: Real>(n: T, c: T, rs: T, phi: T, sigma: T) -> T {
    sigma / (n * n * c * c) / (phi * phi * rs * (16.0 * 2.0_f64.cbrt().powi(2)))
}

fn correlation_from<T: Real>(n: T, c: T, rs: T, local: T, phi: T, sigma: T) -> T {
    let gamma = gamma();
    let phi3 = phi * phi * phi;
    let t2 = t_squared(n, c, rs, phi, sigma);
    let a = T::constant(BETA) / (((-local / (phi3 * gamma)).exp() - 1.0) * gamma);
    let f1 = t2 + a * t2 * t2;
    n * (local + phi3 * gamma * (f1 * BETA / ((a * f1 + 1.0) * gamma) + 1.0).ln())
}
