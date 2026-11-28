use faer::dyn_stack::{MemBuffer, MemStack};
use faer::linalg::svd;
use faer::{Col, Mat, Par};
use std::collections::VecDeque;
use std::f64::consts::PI;

const OLDER_RESIDUAL_STEPS: usize = 7;
const MIXING_DAMPING: f64 = 0.7;
const KERKER_SCREENING_LENGTH_BOHR: f64 = 1.0774841;
const RELATIVE_SINGULAR_VALUE_CUTOFF: f64 = 1e-14;
const HARTREE_KERNEL_NUMERATOR: f64 = 4.0 * PI;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Weight {
    pub damping: f64,
    pub metric: f64,
}

impl Weight {
    pub fn density(g2: f64, volume: f64) -> Self {
        if g2 == 0.0 {
            return Self {
                damping: MIXING_DAMPING,
                metric: volume,
            };
        }
        Self {
            damping: MIXING_DAMPING * g2
                / (g2 + 1.0 / (KERKER_SCREENING_LENGTH_BOHR * KERKER_SCREENING_LENGTH_BOHR)),
            metric: volume * HARTREE_KERNEL_NUMERATOR / g2,
        }
    }

    pub fn magnetization(volume: f64) -> Self {
        Self {
            damping: MIXING_DAMPING,
            metric: volume,
        }
    }

    pub fn onsite() -> Self {
        Self {
            damping: MIXING_DAMPING,
            metric: 0.0,
        }
    }

    pub fn kinetic() -> Self {
        Self {
            damping: MIXING_DAMPING,
            metric: 0.0,
        }
    }
}

struct ResidualWeight {
    damping: f64,
    metric_sqrt: f64,
}

struct MixingStep {
    input: Vec<f64>,
    damped_residual: Vec<f64>,
    stalled: bool,
}

pub struct Mixer {
    weights: Vec<ResidualWeight>,
    history: VecDeque<MixingStep>,
}

impl Mixer {
    pub fn damped(weights: &[Weight]) -> Self {
        Self {
            weights: weights
                .iter()
                .map(|weight| ResidualWeight {
                    damping: weight.damping,
                    metric_sqrt: weight.metric.sqrt(),
                })
                .collect(),
            history: VecDeque::with_capacity(OLDER_RESIDUAL_STEPS + 1),
        }
    }

    pub fn next(&mut self, input: &[f64], output: &[f64]) -> Vec<f64> {
        let n = self.weights.len();
        assert!(
            input.len() == n && output.len() == n,
            "one input and one output value per component"
        );
        let damped_residual: Vec<f64> = (0..n)
            .map(|c| self.weights[c].damping * (output[c] - input[c]))
            .collect();
        if self.history.len() > OLDER_RESIDUAL_STEPS {
            self.history.pop_front();
        }
        let stalled = self.history.front().is_some_and(|oldest| {
            self.weighted_residual_norm(&damped_residual)
                > self.weighted_residual_norm(&oldest.damped_residual)
        });
        self.history.push_back(MixingStep {
            input: input.to_vec(),
            damped_residual,
            stalled,
        });
        let MixingStep {
            input: x,
            damped_residual: p,
            ..
        } = self.history.back().expect("the newest step");
        let base: Vec<f64> = (0..n).map(|c| x[c] + p[c]).collect();
        let k = self.history.len();
        let d = Mat::from_fn(n, k, |c, i| {
            self.weights[c].metric_sqrt * (self.history[i].damped_residual[c] - p[c])
        });
        let pinv = pseudoinverse(d.as_ref());
        let gamma = -(pinv * Col::from_fn(n, |c| self.weights[c].metric_sqrt * p[c]));
        let mut next = base.clone();
        for (step, g) in self.history.iter().zip(gamma.iter()) {
            for c in 0..n {
                next[c] += g * (step.input[c] + step.damped_residual[c] - base[c]);
            }
        }
        let cut = 1.0 - self.residual_step_factor();
        for c in 0..n {
            let mixed: f64 = p[c]
                + (self.history.iter().zip(gamma.iter()))
                    .map(|(step, g)| g * (step.damped_residual[c] - p[c]))
                    .sum::<f64>();
            next[c] -= cut * mixed;
        }
        next
    }

    fn residual_step_factor(&self) -> f64 {
        if self.history.iter().any(|step| step.stalled) {
            1.0 / self.largest_real_dielectric_eigenvalue().max(1.0)
        } else {
            1.0
        }
    }

    fn weighted_residual_norm(&self, p: &[f64]) -> f64 {
        (self.weights.iter().zip(p))
            .map(|(weight, p)| (weight.metric_sqrt * p).powi(2))
            .sum::<f64>()
            .sqrt()
    }

    fn largest_real_dielectric_eigenvalue(&self) -> f64 {
        let n = self.weights.len();
        let k = self.history.len().saturating_sub(1);
        let MixingStep {
            input: x,
            damped_residual: p,
            ..
        } = self.history.back().expect("the newest step");
        let w = |c: usize| self.weights[c].metric_sqrt;
        let dx = Mat::from_fn(n, k, |c, i| w(c) * (self.history[i].input[c] - x[c]));
        let dp = Mat::from_fn(n, k, |c, i| {
            -w(c) * (self.history[i].damped_residual[c] - p[c])
        });
        let model = pseudoinverse(dx.as_ref()) * dp;
        let values = model
            .eigenvalues()
            .expect("the eigenvalues of a small finite matrix converge");
        values
            .iter()
            .map(|z| z.re)
            .fold(f64::NEG_INFINITY, f64::max)
    }
}

fn pseudoinverse(a: faer::MatRef<'_, f64>) -> Mat<f64> {
    let (n, k) = (a.nrows(), a.ncols());
    let factors = a
        .thin_svd()
        .expect("a thin SVD of finite numbers converges");
    let mut pinv = Mat::zeros(k, n);
    let mut buffer = MemBuffer::new(svd::pseudoinverse_from_svd_scratch::<f64>(n, k, Par::Seq));
    svd::pseudoinverse_from_svd_with_tolerance(
        pinv.as_mut(),
        factors.S(),
        factors.U(),
        factors.V(),
        0.0,
        RELATIVE_SINGULAR_VALUE_CUTOFF,
        Par::Seq,
        MemStack::new(&mut buffer),
    );
    pinv
}
