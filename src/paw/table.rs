use crate::pw::per_point;

#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    step: f64,
    values: Vec<f64>,
    curvature: Vec<f64>,
}

impl Table {
    pub fn sampled(q_max: f64, step: f64, f: impl Fn(f64) -> f64 + Sync + Send) -> Self {
        let n = ((q_max / step).ceil() as usize + 2).max(8);
        let values = per_point(n, |i| f(i as f64 * step));
        let slope = |v: [f64; 5]| {
            (-25.0 * v[0] + 48.0 * v[1] - 36.0 * v[2] + 16.0 * v[3] - 3.0 * v[4]) / (12.0 * step)
        };
        let first = slope(std::array::from_fn(|i| values[i]));
        let last = -slope(std::array::from_fn(|i| values[n - 1 - i]));
        Self {
            step,
            curvature: clamped(&values, step, first, last),
            values,
        }
    }

    pub fn at(&self, q: f64) -> f64 {
        let (i, a, b) = self.piece(q);
        let h2 = self.step * self.step / 6.0;
        a * self.values[i]
            + b * self.values[i + 1]
            + ((a * a * a - a) * self.curvature[i] + (b * b * b - b) * self.curvature[i + 1]) * h2
    }

    pub fn slope(&self, q: f64) -> f64 {
        let (i, a, b) = self.piece(q);
        let h = self.step;
        (self.values[i + 1] - self.values[i]) / h
            + ((1.0 - 3.0 * a * a) * self.curvature[i]
                + (3.0 * b * b - 1.0) * self.curvature[i + 1])
                * (h / 6.0)
    }

    fn piece(&self, q: f64) -> (usize, f64, f64) {
        let last = self.step * (self.values.len() - 1) as f64;
        assert!(
            (0.0..=last).contains(&q),
            "q = {q} Bohr⁻¹ lies outside this table's 0 to {last}"
        );
        let x = q / self.step;
        let i = (x.floor() as usize).min(self.values.len() - 2);
        let b = x - i as f64;
        (i, 1.0 - b, b)
    }
}

fn clamped(y: &[f64], h: f64, first: f64, last: f64) -> Vec<f64> {
    let n = y.len();
    let (mut diagonal, mut rhs) = (vec![4.0; n], vec![0.0; n]);
    (diagonal[0], diagonal[n - 1]) = (2.0, 2.0);
    let scale = 6.0 / (h * h);
    rhs[0] = scale * (y[1] - y[0] - h * first);
    rhs[n - 1] = scale * (h * last - (y[n - 1] - y[n - 2]));
    for i in 1..n - 1 {
        rhs[i] = scale * (y[i + 1] - 2.0 * y[i] + y[i - 1]);
    }
    for i in 1..n {
        let w = 1.0 / diagonal[i - 1];
        diagonal[i] -= w;
        rhs[i] -= w * rhs[i - 1];
    }
    let mut m = vec![0.0; n];
    m[n - 1] = rhs[n - 1] / diagonal[n - 1];
    for i in (0..n - 1).rev() {
        m[i] = (rhs[i] - m[i + 1]) / diagonal[i];
    }
    m
}
