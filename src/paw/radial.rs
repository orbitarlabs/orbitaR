use super::spherical_jn;

#[derive(Clone, Debug, PartialEq)]
pub struct RadialGrid {
    a: f64,
    d: f64,
    r: Vec<f64>,
    dr: Vec<f64>,
}

impl RadialGrid {
    pub fn logarithmic(a: f64, d: f64, size: usize) -> Result<Self, String> {
        if !(a > 0.0 && d > 0.0 && (a * (d * size as f64).exp()).is_finite()) || size < 3 {
            return Err(format!(
                "radial_grid: a = {a}, d = {d}, {size} points; need a, d > 0, finite radii, and 3 points"
            ));
        }
        let (r, dr) = (0..size)
            .map(|i| {
                let e = (d * i as f64).exp();
                (a * (e - 1.0), a * d * e)
            })
            .unzip();
        Ok(Self { a, d, r, dr })
    }

    pub fn radii(&self) -> &[f64] {
        &self.r
    }

    pub fn integrate(&self, f: &[f64]) -> f64 {
        let n = f.len();
        assert!(
            (3..=self.r.len()).contains(&n),
            "integrate 3 to {} points",
            self.r.len()
        );
        let h: Vec<f64> = f.iter().zip(&self.dr).map(|(f, dr)| f * dr).collect();
        let first = usize::from(n.is_multiple_of(2));
        let (ends, inner) = (h[first] + h[n - 1], &h[first + 1..n - 1]);
        let weighted: f64 = inner
            .iter()
            .enumerate()
            .map(|(i, x)| if i % 2 == 0 { 4.0 * x } else { 2.0 * x })
            .sum();
        let simpson = (ends + weighted) / 3.0;
        let start = if first == 1 {
            (5.0 * h[0] + 8.0 * h[1] - h[2]) / 12.0
        } else {
            0.0
        };
        simpson + start
    }

    pub fn nearest(&self, radius: f64) -> usize {
        let distance = |k: usize| (self.r[k] - radius).abs();
        (0..self.r.len())
            .min_by(|&a, &b| distance(a).total_cmp(&distance(b)))
            .expect("a grid has points")
    }

    pub fn index_below(&self, radius: f64) -> usize {
        ((1.0 + radius / self.a).ln() / self.d + 1e-8) as usize
    }

    pub fn derivative(&self, f: &[f64]) -> Vec<f64> {
        let n = f.len();
        assert!(
            (5..=self.r.len()).contains(&n),
            "differentiate 5 to {} points",
            self.r.len()
        );
        let stencil = |i: usize| -> [f64; 5] {
            match i {
                0 => [-25.0, 48.0, -36.0, 16.0, -3.0],
                1 => [-3.0, -10.0, 18.0, -6.0, 1.0],
                _ if i == n - 2 => [-1.0, 6.0, -18.0, 10.0, 3.0],
                _ if i == n - 1 => [3.0, -16.0, 36.0, -48.0, 25.0],
                _ => [1.0, -8.0, 0.0, 8.0, -1.0],
            }
        };
        (0..n)
            .map(|i| {
                let start = i.saturating_sub(2).min(n - 5);
                let w = stencil(i);
                let sum: f64 = (0..5).map(|k| w[k] * f[start + k]).sum();
                sum / 12.0 / self.dr[i]
            })
            .collect()
    }

    pub fn poisson(&self, rho: &[f64], l: usize) -> Vec<f64> {
        let n = rho.len();
        assert!(
            (4..=self.r.len()).contains(&n),
            "solve on 4 to {} points",
            self.r.len()
        );
        let power = |i: usize| self.r[i].powi(l as i32);
        let mut inner = vec![0.0; n];
        let mut outer = vec![0.0; n];
        for j in 1..n {
            inner[n - 1 - j] = rho[j] * self.dr[j] * power(j);
            outer[n - 1 - j] = rho[j] * self.dr[j] / (self.r[j] * power(j));
        }
        for f in [&mut inner, &mut outer] {
            f[n - 1] = extrapolated([f[n - 2], f[n - 3], f[n - 4]]);
        }
        let first = |c: f64, f: [f64; 3]| {
            c + [5.0, 8.0, -1.0]
                .iter()
                .zip(f)
                .map(|(w, f)| w * f)
                .sum::<f64>()
                / 12.0
        };
        let accumulate = |f: &[f64]| {
            let mut c = vec![0.0; n];
            for i in (2..n).step_by(2) {
                c[i] = c[i - 2] + (f[i - 2] + 4.0 * f[i - 1] + f[i]) / 3.0;
                c[i - 1] = first(c[i - 2], [f[i - 2], f[i - 1], f[i]]);
            }
            if n.is_multiple_of(2) {
                c[n - 1] = first(c[n - 2], [f[n - 3], f[n - 2], f[n - 1]]);
            }
            c
        };
        let (inside, outside) = (accumulate(&inner), accumulate(&outer));
        (0..n)
            .map(|i| {
                let j = n - 1 - i;
                if i == 0 {
                    0.0
                } else {
                    (outside[j] * self.r[i] * power(i) + (inside[n - 1] - inside[j]) / power(i))
                        / (2 * l + 1) as f64
                }
            })
            .collect()
    }

    pub fn bessel_transform(&self, f: &[f64], l: usize, q: f64) -> f64 {
        let g: Vec<f64> = f
            .iter()
            .zip(&self.r)
            .map(|(f, r)| r * r * f * spherical_jn(l, q * r))
            .collect();
        self.integrate(&g)
    }
}

pub fn extrapolated([f1, f2, f3]: [f64; 3]) -> f64 {
    f3 + 3.0 * (f1 - f2)
}
