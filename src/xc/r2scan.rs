use super::functional::{Polarized, Real, Spin, Spinless};
use super::lda::{PW_MOD_A, exact_fz20, pw92, pw92_g, pw92_g_slope, pw92_slope, slater};
use super::pbe::{BETA, gamma, reduced_gradient_squared, t_squared};
use std::f64::consts::PI;

pub(super) fn correlation<T: Real>(point: Polarized<T>) -> T {
    let Polarized {
        n,
        c,
        rs,
        spin,
        sigma: [uu, ud, dd],
        tau,
        uniform,
    } = point;
    let local = Local {
        energy: pw92(PW_MOD_A, exact_fz20(), rs, spin),
        slope: pw92_slope(PW_MOD_A, exact_fz20(), rs, spin),
        phi: spin.phi(),
        d: spin.d(),
        gc: scan_gc(spin),
    };
    correlation_from(n, c, rs, uu + ud * 2.0 + dd, [tau, uniform], local)
}

pub(super) fn unpolarized_exchange<T: Real>(point: Spinless<T>) -> T {
    exchange_from(point.n, point.c, point.sigma, point.tau)
}

pub(super) fn unpolarized_correlation<T: Real>(point: Spinless<T>) -> T {
    let Spinless {
        n,
        c,
        rs,
        sigma,
        tau,
    } = point;
    let sqrt_rs = rs.sqrt();
    let one = T::constant(1.0);
    let local = Local {
        energy: pw92_g(0, PW_MOD_A[0], rs, sqrt_rs),
        slope: pw92_g_slope(0, PW_MOD_A[0], rs, sqrt_rs),
        phi: one,
        d: one,
        gc: one,
    };
    correlation_from(n, c, rs, sigma, [tau, one], local)
}

const ETA: f64 = 0.001;
const DP2: f64 = 0.361;
const X_SWITCH_EXP: [f64; 3] = [0.667, 0.8, 1.24];
const X_SWITCH_HIGH: [f64; 3] = [-0.023185843322, 0.234528941479, -0.887998041597];
const X_SWITCH_MIDDLE: [f64; 3] = [1.45129704449, -0.663086601049, -0.4445555];
const X_SWITCH_LOW: [f64; 2] = [-0.667, 1.0];
const K1: f64 = 0.065;
const H0X: f64 = 1.174;
const A1: f64 = 4.9479;
const C_SWITCH_EXP: [f64; 3] = [0.64, 1.5, 0.7];
const C_SWITCH_HIGH: [f64; 3] = [-0.051848879792, 0.516884468372, -1.915710236206];
const C_SWITCH_MIDDLE: [f64; 3] = [3.061560252175, -1.535685604549, -0.4352];
const C_SWITCH_LOW: [f64; 2] = [-0.64, 1.0];
const LDA0: [f64; 3] = [0.0285764, 0.0889, 0.125541];
const CHI_INFINITY: f64 = 0.12802585262625815;
const G_C: f64 = 2.363;
const BETA_RS: [f64; 2] = [0.1, 0.1778];
const POLYNOMIAL_END: f64 = 2.5;
const FLAT_EXPONENT: f64 = 700.0;

fn exchange_from<T: Real>(n: T, c: T, sigma: T, tau: T) -> T {
    let p = reduced_gradient_squared(n, c, sigma);
    let alpha = alpha(n, c, sigma, tau, T::constant(1.0));
    let c2 = -(1..=8)
        .map(|i| i as f64 * polynomial(X_SWITCH_HIGH, X_SWITCH_MIDDLE, X_SWITCH_LOW)[8 - i])
        .sum::<f64>()
        * (1.0 - H0X);
    let c_eta = 20.0 / 27.0 + ETA * 5.0 / 3.0;
    let x = p * ((-(p * p) / DP2.powi(4)).exp() * (c_eta * c2) + 10.0 / 81.0);
    let h1 = (-(T::constant(K1) / (x + K1)) + 1.0) * K1 + 1.0;
    let f = switch(
        alpha,
        X_SWITCH_EXP,
        polynomial(X_SWITCH_HIGH, X_SWITCH_MIDDLE, X_SWITCH_LOW),
    );
    let root = p.sqrt().sqrt();
    let gx = if flat_gx(A1 / root.value()) {
        T::constant(1.0)
    } else {
        -(-(T::constant(A1) / root)).exp() + 1.0
    };
    slater(n, c) * (h1 + f * (-h1 + H0X)) * gx
}

fn flat_gx(exponent: f64) -> bool {
    exponent > FLAT_EXPONENT
}

fn alpha<T: Real>(n: T, c: T, sigma: T, tau: T, d: T) -> T {
    let tau_w = sigma / (n * 8.0);
    let tau_unif = n * c * c * d * ((3.0 * PI * PI).cbrt().powi(2) * 3.0 / 10.0);
    (tau - tau_w) / (tau_unif + tau_w * ETA)
}

fn switch<T: Real>(alpha: T, [c1, c2, d]: [f64; 3], polynomial: [f64; 8]) -> T {
    let a = alpha.value();
    if a <= 0.0 {
        (alpha * -c1 / (-alpha + 1.0)).exp()
    } else if a <= POLYNOMIAL_END {
        let [value, slope] = compensated_horner(polynomial, a);
        T::chained(value, [(alpha, slope), (alpha, 0.0)])
    } else {
        (T::constant(c2) / (-alpha + 1.0)).exp() * -d
    }
}

fn compensated_horner(polynomial: [f64; 8], x: f64) -> [f64; 2] {
    let mut value = Compensated::of(polynomial[0]);
    let mut slope = Compensated::of(0.0);
    for &k in &polynomial[1..] {
        slope = slope.times(x).plus(value);
        value = value.times(x).plus(Compensated::of(k));
    }
    [value.rounded(), slope.rounded()]
}

#[derive(Clone, Copy)]
struct Compensated {
    sum: f64,
    error: f64,
}

impl Compensated {
    fn of(x: f64) -> Self {
        Self { sum: x, error: 0.0 }
    }

    fn times(self, x: f64) -> Self {
        let product = self.sum * x;
        let product_error = self.sum.mul_add(x, -product);
        Self {
            sum: product,
            error: self.error.mul_add(x, product_error),
        }
    }

    fn plus(self, other: Self) -> Self {
        let sum = self.sum + other.sum;
        Self {
            sum,
            error: two_sum_error(other.sum, self.sum, sum) + self.error + other.error,
        }
    }

    fn rounded(self) -> f64 {
        self.sum + self.error
    }
}

fn two_sum_error(a: f64, b: f64, sum: f64) -> f64 {
    let shifted = sum - a;
    (a - (sum - shifted)) + (b - shifted)
}

fn polynomial(high: [f64; 3], middle: [f64; 3], low: [f64; 2]) -> [f64; 8] {
    let mut all = [0.0; 8];
    all[..3].copy_from_slice(&high);
    all[3..6].copy_from_slice(&middle);
    all[6..].copy_from_slice(&low);
    all
}

fn scan_gc<T: Real>(spin: Spin<T>) -> T {
    (-(spin.f() * (G_C * (2.0_f64.cbrt() - 1.0))) + 1.0) * (-spin.z12 + 1.0)
}

#[derive(Clone, Copy)]
struct Local<T> {
    energy: T,
    slope: T,
    phi: T,
    d: T,
    gc: T,
}

fn correlation_from<T: Real>(n: T, c: T, rs: T, sigma: T, [tau, d]: [T; 2], local: Local<T>) -> T {
    let gamma = gamma();
    let phi3 = local.phi * local.phi * local.phi;
    let alpha = alpha(n, c, sigma, tau, d);
    let s2 = reduced_gradient_squared(n, c, sigma);
    let [b1c, b2c, b3c] = LDA0;
    let sqrt_rs = rs.sqrt();
    let denominator = sqrt_rs * b2c + rs * b3c + 1.0;
    let lda0 = -(T::constant(b1c) / denominator);
    let lda0_slope = (T::constant(b2c) / (sqrt_rs * 2.0) + b3c) * b1c / (denominator * denominator);
    let g_infinity = T::constant(1.0) / (s2 * (4.0 * CHI_INFINITY) + 1.0).sqrt().sqrt();
    let h0 = ((((-lda0 / b1c).exp() - 1.0) * (-g_infinity + 1.0)) + 1.0).ln() * b1c;
    let ec0 = (lda0 + h0) * local.gc;
    let w1 = (-local.energy / (phi3 * gamma)).exp() - 1.0;
    let t2 = t_squared(n, c, rs, local.phi, sigma);
    let beta = (rs * BETA_RS[0] + 1.0) * BETA / (rs * BETA_RS[1] + 1.0);
    let y = beta * t2 / (w1 * gamma);
    let dfc2 = (1..=7)
        .map(|i| i as f64 * polynomial(C_SWITCH_HIGH, C_SWITCH_MIDDLE, C_SWITCH_LOW)[7 - i])
        .sum::<f64>();
    let lsda0 = lda0 * local.gc;
    let lsda0_slope = lda0_slope * local.gc;
    let dy = T::constant(dfc2) / (local.d * phi3 * w1 * (27.0 * gamma))
        * ((lsda0_slope - local.slope) * rs * 20.0 - (lsda0 - local.energy) * (45.0 * ETA))
        * s2
        * (-(s2 * s2) / DP2.powi(4)).exp();
    let g = T::constant(1.0) / ((y - dy) * 4.0 + 1.0).sqrt().sqrt();
    let ec1 = local.energy + phi3 * gamma * (w1 * (-g + 1.0) + 1.0).ln();
    let fc = switch(
        alpha,
        C_SWITCH_EXP,
        polynomial(C_SWITCH_HIGH, C_SWITCH_MIDDLE, C_SWITCH_LOW),
    );
    n * (ec1 + fc * (ec0 - ec1))
}
