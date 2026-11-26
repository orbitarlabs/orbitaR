use gauss_quad::legendre::GaussLegendre;
use nalgebra::{DMatrix, DVector};
use std::num::NonZeroUsize;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bm {
    pub v0_ang3: f64,
    pub b0_ev_ang3: f64,
    pub b1: f64,
}

const NODES: NonZeroUsize = NonZeroUsize::new(16).unwrap();

impl Bm {
    pub fn energy_ev(&self, volume_ang3: f64) -> f64 {
        let r = (self.v0_ang3 / volume_ang3).powf(2.0 / 3.0);
        9.0 / 16.0
            * self.b0_ev_ang3
            * self.v0_ang3
            * ((r - 1.0).powi(3) * self.b1 + (r - 1.0).powi(2) * (6.0 - 4.0 * r))
    }

    pub fn per(&self, factor: f64) -> Bm {
        Bm {
            v0_ang3: self.v0_ang3 / factor,
            ..*self
        }
    }
}

pub fn fit(points: &[(f64, f64)]) -> Result<Bm, String> {
    let mut volumes: Vec<f64> = points.iter().map(|&(v, _)| v).collect();
    volumes.sort_by(f64::total_cmp);
    volumes.dedup();
    if volumes.len() < 4 {
        return Err(format!(
            "a cubic fit needs 4 or more distinct volumes; got {}",
            volumes.len()
        ));
    }
    let n = points.len();
    let x: Vec<f64> = points.iter().map(|&(v, _)| v.powf(-2.0 / 3.0)).collect();
    let mid = x.iter().sum::<f64>() / n as f64;
    let e_mid = points.iter().map(|&(_, e)| e).sum::<f64>() / n as f64;
    let powers = DMatrix::from_fn(n, 4, |i, j| (x[i] - mid).powi(j as i32));
    let energies = DVector::from_iterator(n, points.iter().map(|&(_, e)| e - e_mid));
    let qr = powers.qr();
    let c = (qr.r())
        .solve_upper_triangular(&(qr.q().transpose() * energies))
        .ok_or("the volumes do not determine a cubic")?;
    let (c1, c2, c3) = (c[1], c[2], c[3]);
    let (a, b, c) = (3.0 * c3, 2.0 * c2, c1);
    let root = (b * b - 4.0 * a * c).sqrt();
    let t0 = -2.0 * c / (b + root);
    if !t0.is_finite() {
        return Err("the fitted E(V) has no minimum".into());
    }
    let v0 = (mid + t0).powf(-1.5);
    if v0.is_nan() {
        return Err(format!("V0 is not a positive volume: x0 = {}", mid + t0));
    }
    let (e2, e3) = (root, 6.0 * c3);
    let (dx, ddx) = (
        -2.0 / 3.0 * v0.powf(-5.0 / 3.0),
        10.0 / 9.0 * v0.powf(-8.0 / 3.0),
    );
    let b0 = v0 * e2 * dx * dx;
    let db = e2 * dx * dx + v0 * (e3 * dx.powi(3) + 3.0 * e2 * dx * ddx);
    let bm = Bm {
        v0_ang3: v0,
        b0_ev_ang3: b0,
        b1: -v0 * db / b0,
    };
    if [bm.v0_ang3, bm.b0_ev_ang3, bm.b1]
        .iter()
        .all(|p| p.is_finite())
    {
        Ok(bm)
    } else {
        Err(format!("the fit is not finite: {bm:?}"))
    }
}

pub const GPA_EV_ANG3: f64 = 1e-21 / 1.602_176_565e-19;

struct Window {
    low: f64,
    high: f64,
    rule: GaussLegendre,
}

impl Window {
    fn new(a: &Bm, b: &Bm) -> Self {
        let mid = (a.v0_ang3 + b.v0_ang3) / 2.0;
        let (low, high) = (0.94 * mid, 1.06 * mid);
        Self {
            low,
            high,
            rule: GaussLegendre::new(NODES),
        }
    }

    fn integral(&self, g: impl Fn(f64) -> f64) -> f64 {
        self.rule.integrate(self.low, self.high, g)
    }
}

pub fn delta(code: &Bm, reference: &Bm) -> f64 {
    let w = Window::new(code, reference);
    let difference = w.integral(|v| (code.energy_ev(v) - reference.energy_ev(v)).powi(2));
    1000.0 * (difference / (w.high - w.low)).sqrt()
}

pub fn epsilon(code: &Bm, reference: &Bm) -> f64 {
    let w = Window::new(code, reference);
    let mean = w.integral(|v| code.energy_ev(v)) / (w.high - w.low);
    let difference = w.integral(|v| (code.energy_ev(v) - reference.energy_ev(v)).powi(2));
    let spread_code = w.integral(|v| (code.energy_ev(v) - mean).powi(2));
    let spread_reference = w.integral(|v| (reference.energy_ev(v) - mean).powi(2));
    (difference / (spread_code * spread_reference).sqrt()).sqrt()
}

pub fn nu(code: &Bm, reference: &Bm) -> f64 {
    let relative = |a: f64, b: f64| 2.0 * (a - b) / (a + b);
    100.0
        * (relative(code.v0_ang3, reference.v0_ang3).powi(2)
            + (relative(code.b0_ev_ang3, reference.b0_ev_ang3) / 20.0).powi(2)
            + (relative(code.b1, reference.b1) / 400.0).powi(2))
        .sqrt()
}
