use crate::model::Smearing;
use std::f64::consts::{FRAC_1_SQRT_2, FRAC_2_SQRT_PI};

#[derive(Clone, Debug, PartialEq)]
pub struct Occupied {
    pub fermi_ha: f64,
    pub occupations: Vec<Vec<Vec<f64>>>,
    pub smearing_ha: f64,
}

impl Occupied {
    pub fn counted(&self) -> Vec<Vec<Vec<f64>>> {
        (self.occupations.iter())
            .map(|spin| {
                (spin.iter())
                    .map(|k| {
                        k.iter()
                            .map(|&f| {
                                if f.abs() <= DENSITY_AND_ENERGY_OCCUPATION_THRESHOLD {
                                    0.0
                                } else {
                                    f
                                }
                            })
                            .collect()
                    })
                    .collect()
            })
            .collect()
    }
}

const DENSITY_AND_ENERGY_OCCUPATION_THRESHOLD: f64 = 1e-8;
const FERMI_BRACKET_WIDTHS: [f64; 2] = [6.001, 30.001];
const RELATIVE_ELECTRON_COUNT_TOLERANCE: f64 = 1e-14;
const K_POINT_WEIGHT_SUM_TOLERANCE: f64 = 1e-12;

pub fn occupy(
    smearing: Smearing,
    width_ha: f64,
    bands: &[Vec<Vec<f64>>],
    weights: &[f64],
    electrons: f64,
) -> Result<Occupied, String> {
    check(smearing, width_ha, bands, weights, electrons)?;
    let spins = bands.len() as f64;
    let g = 2.0 / spins;
    let count = bands[0][0].len();
    let fill = (electrons / (g * spins)).ceil() as usize;
    let edge = |n: usize, highest: bool| {
        let values = bands.iter().flatten().map(|k| k[n]);
        if highest {
            values.fold(f64::MIN, f64::max)
        } else {
            values.fold(f64::MAX, f64::min)
        }
    };
    if fill > count {
        return Err(format!(
            "bands: {electrons} electrons need {fill} bands, and {count} are computed"
        ));
    }
    let each = |f: &dyn Fn(&[f64]) -> Vec<f64>| -> Vec<Vec<Vec<f64>>> {
        bands
            .iter()
            .map(|s| s.iter().map(|k| f(k)).collect())
            .collect()
    };
    if smearing == Smearing::Fixed {
        if fill as f64 * g * spins != electrons {
            return Err(format!(
                "occupations.smearing: fixed occupations cannot hold {electrons} electrons"
            ));
        }
        return Ok(Occupied {
            fermi_ha: edge(fill - 1, true),
            occupations: each(&|_| (0..count).map(|n| if n < fill { g } else { 0.0 }).collect()),
            smearing_ha: 0.0,
        });
    }
    let x = |e: f64, mu: f64| (e - mu) / width_ha;
    let total = |mu: f64, part: fn((f64, f64)) -> f64| {
        let per_k = |(k, w): (&Vec<f64>, &f64)| {
            w * k
                .iter()
                .map(|&e| part(shape(smearing, x(e, mu))))
                .sum::<f64>()
        };
        g * bands
            .iter()
            .map(|s| s.iter().zip(weights).map(per_k).sum::<f64>())
            .sum::<f64>()
    };
    let count_at = |mu: f64| total(mu, |t| t.0);
    let converged =
        |mu: f64| (count_at(mu) - electrons).abs() <= RELATIVE_ELECTRON_COUNT_TOLERANCE * electrons;
    let above = if fill < count {
        edge(fill, false)
    } else {
        f64::INFINITY
    };
    let mut fermi = (edge(fill - 1, true) + above) / 2.0;
    if !(fermi.is_finite() && converged(fermi)) {
        let pad = if smearing == Smearing::FermiDirac {
            FERMI_BRACKET_WIDTHS[1]
        } else {
            FERMI_BRACKET_WIDTHS[0]
        } * width_ha;
        let all = || bands.iter().flatten().flatten().copied();
        let (lo, hi) = (
            all().fold(f64::MAX, f64::min) - pad,
            all().fold(f64::MIN, f64::max) + pad,
        );
        if count_at(hi) < electrons * (1.0 - RELATIVE_ELECTRON_COUNT_TOLERANCE) {
            return Err(format!(
                "bands: {electrons} electrons do not fit in {count} bands at width {width_ha} Ha"
            ));
        }
        fermi = bisect(lo, hi, &count_at, electrons, &converged);
    }
    Ok(Occupied {
        fermi_ha: fermi,
        occupations: each(&|k| {
            k.iter()
                .map(|&e| g * shape(smearing, x(e, fermi)).0)
                .collect()
        }),
        smearing_ha: -width_ha * total(fermi, |t| t.1),
    })
}

fn check(
    smearing: Smearing,
    width_ha: f64,
    bands: &[Vec<Vec<f64>>],
    weights: &[f64],
    electrons: f64,
) -> Result<(), String> {
    if !(1..=2).contains(&bands.len()) {
        return Err(format!("bands: give one or two spins, not {}", bands.len()));
    }
    if bands.iter().any(|s| s.len() != weights.len()) || weights.is_empty() {
        return Err(format!(
            "bands: each spin needs one row per k-point weight ({})",
            weights.len()
        ));
    }
    let sum: f64 = weights.iter().sum();
    if weights.iter().any(|w| !(w.is_finite() && *w >= 0.0))
        || !(1.0 - K_POINT_WEIGHT_SUM_TOLERANCE..=1.0 + K_POINT_WEIGHT_SUM_TOLERANCE).contains(&sum)
    {
        return Err(format!(
            "weights: k-point weights must be ≥ 0 and sum to 1, not {sum}"
        ));
    }
    let count = bands[0][0].len();
    let ascending =
        |k: &Vec<f64>| k.len() == count && k.iter().all(|e| e.is_finite()) && k.is_sorted();
    if count == 0 || !bands.iter().flatten().all(ascending) {
        return Err(
            "bands: every k-point needs the same number of finite, ascending energies".into(),
        );
    }
    if !(electrons.is_finite() && electrons > 0.0) {
        return Err(format!("electrons: need a positive count, not {electrons}"));
    }
    if smearing != Smearing::Fixed && !(width_ha.is_finite() && width_ha > 0.0) {
        return Err(format!(
            "occupations.width: a smearing needs a positive width, not {width_ha} Ha"
        ));
    }
    Ok(())
}

fn bisect(
    mut lo: f64,
    mut hi: f64,
    count_at: &dyn Fn(f64) -> f64,
    electrons: f64,
    converged: &dyn Fn(f64) -> bool,
) -> f64 {
    loop {
        let mid = 0.5 * (lo + hi);
        if mid <= lo || mid >= hi || converged(mid) {
            return mid;
        }
        if count_at(mid) < electrons {
            lo = mid;
        } else {
            hi = mid;
        }
    }
}

fn shape(smearing: Smearing, x: f64) -> (f64, f64) {
    let gauss = (-x * x).exp() * FRAC_2_SQRT_PI / 2.0;
    match smearing {
        Smearing::Fixed => unreachable!("fixed occupations have no shape"),
        Smearing::FermiDirac => {
            let e = (-x.abs()).exp();
            let theta = if x > 0.0 {
                e / (1.0 + e)
            } else {
                1.0 / (1.0 + e)
            };
            (theta, e.ln_1p() + x.abs() * e / (1.0 + e))
        }
        Smearing::Gaussian => (libm::erfc(x) / 2.0, gauss / 2.0),
        Smearing::MethfesselPaxton => (
            libm::erfc(x) / 2.0 - x * gauss / 2.0,
            (1.0 - 2.0 * x * x) * gauss / 4.0,
        ),
        Smearing::MarzariVanderbilt => {
            let u = x + FRAC_1_SQRT_2;
            let gu = (-u * u).exp() * FRAC_1_SQRT_2 * FRAC_2_SQRT_PI / 2.0;
            (libm::erfc(u) / 2.0 + gu, u * gu)
        }
    }
}
