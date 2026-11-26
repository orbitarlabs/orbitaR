use crate::n13_forces_derivative as forces;
use crate::n15_stress_derivative::{self as stress, Order};
use xtask::harness::{Evidence, Metrics};

const MOVES: [(&str, &str, &str, bool); 1] =
    [("input", "gaas", "gaas_force_difference_ha_bohr", false)];

const STRAINS: [(&str, &str, &str, Order); 3] = [
    (
        "input",
        "gaas-strain",
        "gaas_stress_difference_ha_bohr3",
        Order::None,
    ),
    (
        "input",
        "gaas-volume",
        "gaas_volume_stress_difference_ha_bohr3",
        Order::None,
    ),
    (
        "fe",
        "fe-strain",
        "fe_stress_difference_ha_bohr3",
        Order::Ferro,
    ),
];

const ON_THRESHOLD: f64 = 1e-6;

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = Metrics::new();
    let moved = forces::triples(evidence, &MOVES, &mut metrics)?;
    let strained = stress::triples(evidence, &STRAINS, &mut metrics)?;
    for &(centre, stem, ..) in &MOVES {
        same_kpoints(evidence, centre, stem)?;
        same_discretization(evidence, centre, stem)?;
    }
    for &(centre, stem, ..) in &STRAINS {
        same_kpoints(evidence, centre, stem)?;
        same_discretization(evidence, centre, stem)?;
        same_sets(evidence, centre, stem)?;
    }
    metrics.insert(
        "moment_spread_bohr_magneton".to_owned(),
        moved.max(strained),
    );
    Ok(metrics)
}

fn same_kpoints(evidence: &Evidence, centre: &str, stem: &str) -> Result<(), String> {
    let at = evidence.output(centre)?;
    let (points, weights) = (at.kpoints_fractional()?, at.kpoint_weights()?);
    for other in [format!("{stem}-plus"), format!("{stem}-minus")] {
        let o = evidence.output(&other)?;
        if o.kpoints_fractional()? != points || o.kpoint_weights()? != weights {
            return Err(format!("{other}: other k-points or weights than {centre}"));
        }
    }
    Ok(())
}

fn same_discretization(evidence: &Evidence, centre: &str, stem: &str) -> Result<(), String> {
    let record = evidence.output(centre)?.discretization()?;
    if record.nyquist {
        return Err(format!(
            "{centre}: the density sphere holds a Nyquist coefficient, where v is not the derivative"
        ));
    }
    for other in [format!("{stem}-plus"), format!("{stem}-minus")] {
        if evidence.output(&other)?.discretization()? != record {
            return Err(format!(
                "{other}: another discretization than {centre} (operations, grids, or plane waves)"
            ));
        }
    }
    Ok(())
}

type Matrix = [[f64; 3]; 3];

fn reciprocal(a: &Matrix) -> Matrix {
    let cross = |u: [f64; 3], v: [f64; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let rows = [cross(a[1], a[2]), cross(a[2], a[0]), cross(a[0], a[1])];
    let volume: f64 = (0..3).map(|i| a[0][i] * rows[0][i]).sum();
    rows.map(|r| r.map(|x| x * std::f64::consts::TAU / volume))
}

fn same_sets(evidence: &Evidence, centre: &str, stem: &str) -> Result<(), String> {
    let input = evidence.input(centre)?;
    let cutoff = input.cutoff_ha()?;
    let density = input
        .number("basis.density_cutoff_ha")
        .unwrap_or(2.0 * cutoff);
    let cells = [
        input.cell_bohr()?,
        evidence.input(&format!("{stem}-plus"))?.cell_bohr()?,
        evidence.input(&format!("{stem}-minus"))?.cell_bohr()?,
    ];
    let bases = cells.map(|a| reciprocal(&a));
    let thresholds = [8.0 * cutoff, 8.0 * density];
    let n = [0, 1, 2].map(|i| {
        let a = cells[0][i].iter().map(|x| x * x).sum::<f64>().sqrt();
        ((8.0 * density).sqrt() * a / std::f64::consts::TAU).ceil() as i32 + 2
    });
    let side = |q2: f64, t: f64| {
        if q2 < t * (1.0 - ON_THRESHOLD) {
            Ok(false)
        } else if q2 > t * (1.0 + ON_THRESHOLD) {
            Ok(true)
        } else {
            Err(())
        }
    };
    let check = |shift: [f64; 3], limits: &[f64]| -> Result<(), String> {
        for m0 in -n[0]..=n[0] {
            for m1 in -n[1]..=n[1] {
                for m2 in -n[2]..=n[2] {
                    let m = [
                        f64::from(m0) + shift[0],
                        f64::from(m1) + shift[1],
                        f64::from(m2) + shift[2],
                    ];
                    let q2 = bases.map(|b| {
                        (0..3)
                            .map(|j| (0..3).map(|i| m[i] * b[i][j]).sum::<f64>().powi(2))
                            .sum::<f64>()
                    });
                    for &t in limits {
                        let sides = q2.map(|q| side(q, t));
                        if sides.iter().any(Result::is_err)
                            || sides[1] != sides[0]
                            || sides[2] != sides[0]
                        {
                            return Err(format!(
                                "{stem}: the vector {m:?} lies on or crosses |q|² = {t} between {centre} and its strained \
                                 cells, so a set or box changes; take another strain"
                            ));
                        }
                    }
                }
            }
        }
        Ok(())
    };
    check([0.0; 3], &thresholds)?;
    for k in evidence.output(centre)?.kpoints_fractional()? {
        check(k, &[2.0 * cutoff])?;
    }
    Ok(())
}
