use super::{Mat, MatRef, c64};
use crate::model::Cell;
use std::f64::consts::SQRT_2;

const SHELL_MARGIN: f64 = 1e-12;

#[derive(Clone, Debug, PartialEq)]
pub struct Basis {
    millers: Vec<[i32; 3]>,
    kinetic_ha: Vec<f64>,
    wavevectors: Vec<[f64; 3]>,
}

impl Basis {
    pub fn sphere(cell: &Cell, k: [f64; 3], cutoff_ha: f64) -> Self {
        let limit = cutoff_ha * (1.0 + SHELL_MARGIN);
        let found = cell.wavevectors_within(k, (2.0 * limit).sqrt());
        let millers = (found.iter())
            .map(|(p, _)| std::array::from_fn(|i| (p[i] - k[i]).round() as i32))
            .collect();
        let kinetic_ha = found.iter().map(|(_, q)| 0.5 * q * q).collect();
        let wavevectors = found.iter().map(|(p, _)| cell.wavevector(*p)).collect();
        Self {
            millers,
            kinetic_ha,
            wavevectors,
        }
    }

    pub fn millers(&self) -> &[[i32; 3]] {
        &self.millers
    }

    pub fn kinetic_ha(&self) -> &[f64] {
        &self.kinetic_ha
    }

    pub fn wavevectors(&self) -> &[[f64; 3]] {
        &self.wavevectors
    }

    pub fn pairs(&self, k: [f64; 3]) -> Option<Pairs> {
        if k.iter().any(|x| (2.0 * x).fract() != 0.0) {
            return None;
        }
        let partner = (self.millers.iter())
            .map(|m| {
                let other = std::array::from_fn(|i| -m[i] - (2.0 * k[i]) as i32);
                self.millers.binary_search(&other).ok()
            })
            .collect::<Option<Vec<usize>>>()?;
        Some(Pairs { partner })
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Pairs {
    partner: Vec<usize>,
}

impl Pairs {
    pub fn real(&self, c: MatRef<'_, c64>) -> Mat<f64> {
        Mat::from_fn(c.nrows(), c.ncols(), |row, column| {
            let other = self.partner[row];
            match row.cmp(&other) {
                std::cmp::Ordering::Less => SQRT_2 * c[(row, column)].re,
                std::cmp::Ordering::Greater => SQRT_2 * c[(other, column)].im,
                std::cmp::Ordering::Equal => c[(row, column)].re,
            }
        })
    }

    pub fn complex(&self, x: MatRef<'_, f64>) -> Mat<c64> {
        Mat::from_fn(x.nrows(), x.ncols(), |row, column| {
            self.coefficient(|r| x[(r, column)], row)
        })
    }

    pub fn coefficient(&self, x: impl Fn(usize) -> f64, row: usize) -> c64 {
        let other = self.partner[row];
        match row.cmp(&other) {
            std::cmp::Ordering::Less => c64::new(x(row), x(other)) / SQRT_2,
            std::cmp::Ordering::Greater => c64::new(x(other), -x(row)) / SQRT_2,
            std::cmp::Ordering::Equal => c64::new(x(row), 0.0),
        }
    }

    pub fn packed(&self, a: impl Fn(usize) -> f64, b: impl Fn(usize) -> f64, row: usize) -> c64 {
        let (a, b) = (self.coefficient(a, row), self.coefficient(b, row));
        c64::new(a.re - b.im, a.im + b.re)
    }

    pub fn halves(&self, packed: &[c64], row: usize) -> [c64; 2] {
        let (here, mirror) = (packed[row], packed[self.partner[row]].conj());
        [(here + mirror) * 0.5, (here - mirror) * c64::new(0.0, -0.5)]
    }

    pub fn entry(&self, row: usize, v: c64) -> f64 {
        match row.cmp(&self.partner[row]) {
            std::cmp::Ordering::Less => SQRT_2 * v.re,
            std::cmp::Ordering::Greater => -SQRT_2 * v.im,
            std::cmp::Ordering::Equal => v.re,
        }
    }
}
