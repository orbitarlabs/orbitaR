use crate::pw::Mat;
use gauss_quad::legendre::GaussLegendre;
use hyperjet::jets::Jet1;
use std::f64::consts::{PI, TAU};
use std::num::NonZeroUsize;

pub fn real_ylm(lmax: usize, direction: [f64; 3]) -> Vec<f64> {
    let norm = direction.iter().map(|x| x * x).sum::<f64>().sqrt();
    let [x, y, z] = if norm > 0.0 {
        direction.map(|v| v / norm)
    } else {
        [0.0, 0.0, 1.0]
    };
    let mut ylm = vec![0.0; (lmax + 1) * (lmax + 1)];
    let (mut cos_m, mut sin_m) = (1.0, 0.0);
    let mut q_mm = 1.0;
    for m in 0..=lmax {
        let (mut below, mut q) = (0.0, q_mm);
        for l in m..=lmax {
            if l > m {
                let next =
                    ((2 * l - 1) as f64 * z * q - (l + m - 1) as f64 * below) / (l - m) as f64;
                (below, q) = (q, next);
            }
            let ratio: f64 = (l - m + 1..=l + m).map(|k| k as f64).product();
            let n = ((2 * l + 1) as f64 / (4.0 * PI) / ratio).sqrt();
            if m == 0 {
                ylm[l * l + l] = n * q;
            } else {
                ylm[l * l + l + m] = 2f64.sqrt() * n * q * cos_m;
                ylm[l * l + l - m] = 2f64.sqrt() * n * q * sin_m;
            }
        }
        (cos_m, sin_m) = (cos_m * x - sin_m * y, cos_m * y + sin_m * x);
        q_mm *= (2 * m + 1) as f64;
    }
    ylm
}

pub fn solid_harmonics(lmax: usize, r: [f64; 3]) -> Vec<(f64, [f64; 3])> {
    let [x, y, z]: [Jet1<3>; 3] = std::array::from_fn(|axis| Jet1::variable(r[axis], axis));
    let r2 = x * x + y * y + z * z;
    let mut out = vec![(0.0, [0.0; 3]); (lmax + 1) * (lmax + 1)];
    let (mut cos_m, mut sin_m) = (Jet1::constant(1.0), Jet1::constant(0.0));
    let mut r_mm = Jet1::constant(1.0);
    for m in 0..=lmax {
        let (mut below, mut q) = (Jet1::constant(0.0), r_mm);
        for l in m..=lmax {
            if l > m {
                let next = (z * q * (2 * l - 1) as f64 - r2 * below * (l + m - 1) as f64)
                    * (1.0 / (l - m) as f64);
                (below, q) = (q, next);
            }
            let ratio: f64 = (l - m + 1..=l + m).map(|k| k as f64).product();
            let n = ((2 * l + 1) as f64 / (4.0 * PI) / ratio).sqrt();
            if m == 0 {
                out[l * l + l] = parts(q * n);
            } else {
                out[l * l + l + m] = parts(q * cos_m * (2f64.sqrt() * n));
                out[l * l + l - m] = parts(q * sin_m * (2f64.sqrt() * n));
            }
        }
        (cos_m, sin_m) = (cos_m * x - sin_m * y, cos_m * y + sin_m * x);
        r_mm *= (2 * m + 1) as f64;
    }
    out
}

fn parts(jet: Jet1<3>) -> (f64, [f64; 3]) {
    (jet.value, jet.grad)
}

pub fn reduced_jn(l: usize, x: f64) -> f64 {
    debug_assert!(l <= 10, "the 20-term series holds 1e-15 only to l = 10");
    if by_series(l, x) {
        (1..=l).fold(1.0, |acc, k| acc / (2 * k + 1) as f64) * series(l, x)
    } else {
        spherical_jn(l, x) / x.powi(l as i32)
    }
}

fn by_series(l: usize, x: f64) -> bool {
    x < (l as f64).max(1.0)
}

fn series(l: usize, x: f64) -> f64 {
    let (mut term, mut sum) = (1.0, 1.0);
    for k in 1..20 {
        term *= -0.5 * x * x / (k as f64 * (2 * l + 2 * k + 1) as f64);
        sum += term;
    }
    sum
}

pub fn spherical_jn(l: usize, x: f64) -> f64 {
    debug_assert!(l <= 10, "the 20-term series holds 1e-15 only to l = 10");
    if by_series(l, x) {
        (1..=l).fold(1.0, |acc, k| acc * x / (2 * k + 1) as f64) * series(l, x)
    } else {
        let (s, c) = x.sin_cos();
        let (mut below, mut j) = (s / x, s / (x * x) - c / x);
        if l == 0 {
            return below;
        }
        for n in 1..l {
            (below, j) = (j, (2 * n + 1) as f64 / x * j - below);
        }
        j
    }
}

pub fn gauss_legendre(n: usize) -> Vec<(f64, f64)> {
    let n = NonZeroUsize::new(n).expect("a rule of at least one node");
    GaussLegendre::new(n).into_node_weight_pairs().into_vec()
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gaunt {
    small: usize,
    values: Vec<f64>,
}

impl Gaunt {
    pub fn up_to(small: usize, large: usize) -> Self {
        let (na, nl) = ((small + 1).pow(2), (large + 1).pow(2));
        let degree = 2 * small + large;
        let nodes = gauss_legendre((degree + 1).div_ceil(2));
        let azimuths = degree + 1;
        let mut values = vec![0.0; nl * na * na];
        for &(z, weight) in &nodes {
            let s = (1.0 - z * z).sqrt();
            for k in 0..azimuths {
                let phi = TAU * k as f64 / azimuths as f64;
                let y = real_ylm(small.max(large), [s * phi.cos(), s * phi.sin(), z]);
                let w = weight * TAU / azimuths as f64;
                for big in 0..nl {
                    for a in 0..na {
                        for b in a..na {
                            values[(big * na + a) * na + b] += w * y[big] * y[a] * y[b];
                        }
                    }
                }
            }
        }
        for big in 0..nl {
            for a in 0..na {
                for b in 0..a {
                    values[(big * na + a) * na + b] = values[(big * na + b) * na + a];
                }
            }
        }
        for v in &mut values {
            if v.abs() < 1e-14 {
                *v = 0.0;
            }
        }
        Self { small, values }
    }

    pub fn get(&self, big: usize, a: usize, b: usize) -> f64 {
        let na = (self.small + 1).pow(2);
        self.values[(big * na + a) * na + b]
    }
}

pub fn ylm_rotation(lmax: usize, w: [[f64; 3]; 3]) -> Mat<f64> {
    let rotated = |r: [f64; 3]| std::array::from_fn(|i| (0..3).map(|j| w[i][j] * r[j]).sum());
    let azimuths = 2 * lmax + 1;
    let size = (lmax + 1) * (lmax + 1);
    let mut d = Mat::zeros(size, size);
    for (z, weight) in gauss_legendre(lmax + 1) {
        for k in 0..azimuths {
            let phi = TAU * k as f64 / azimuths as f64;
            let s = (1.0 - z * z).sqrt();
            let r = [s * phi.cos(), s * phi.sin(), z];
            let (y, y_rotated) = (real_ylm(lmax, r), real_ylm(lmax, rotated(r)));
            let share = weight * TAU / azimuths as f64;
            for l in 0..=lmax {
                for a in l * l..(l + 1) * (l + 1) {
                    for b in l * l..(l + 1) * (l + 1) {
                        d[(a, b)] += share * y_rotated[a] * y[b];
                    }
                }
            }
        }
    }
    d
}
