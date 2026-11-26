use xtask::harness::{Evidence, Metrics};

const CELLS: [(&str, &str, &str, bool); 4] = [
    ("input", "gaas", "gaas_force_difference_ha_bohr", false),
    ("fe", "fe", "fe_pbe_force_difference_ha_bohr", true),
    ("fe-lda", "fe-lda", "fe_lda_force_difference_ha_bohr", true),
    ("feo", "feo", "feo_force_difference_ha_bohr", true),
];
const MAGNETIC: f64 = 2.0;

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = Metrics::new();
    let spread = triples(evidence, &CELLS, &mut metrics)?;
    symmetric(evidence, &mut metrics)?;
    metrics.insert("moment_spread_bohr_magneton".to_owned(), spread);
    Ok(metrics)
}

pub fn triples(
    evidence: &Evidence,
    cells: &[(&str, &str, &str, bool)],
    metrics: &mut Metrics,
) -> Result<f64, String> {
    let mut spread: f64 = 0.0;
    for &(name, stem, metric, magnetic) in cells {
        let (plus, minus) = (format!("{stem}-plus"), format!("{stem}-minus"));
        let at = evidence.input(name)?;
        let cell = at.cell_bohr()?;
        let cartesian = |f: [f64; 3]| -> [f64; 3] {
            std::array::from_fn(|i| (0..3).map(|j| f[j] * cell[j][i]).sum())
        };
        for other in [&plus, &minus] {
            let o = evidence.input(other)?;
            if !o.equal_apart_from(&at, &["structure.positions_fractional", "outputs"]) {
                return Err(format!(
                    "{other} must differ from {name} in its positions only"
                ));
            }
        }
        let (centre, ahead, behind) = (
            at.positions_fractional()?,
            evidence.input(&plus)?.positions_fractional()?,
            evidence.input(&minus)?.positions_fractional()?,
        );
        let mut moved = Vec::new();
        for ((c, a), b) in centre.iter().zip(&ahead).zip(&behind) {
            if (0..3).any(|i| (a[i] + b[i] - 2.0 * c[i]).abs() > 1e-12) {
                return Err(format!("{name} must lie midway between {plus} and {minus}"));
            }
            moved.push(cartesian(std::array::from_fn(|i| a[i] - b[i])));
        }
        let step = moved.iter().flatten().map(|x| x * x).sum::<f64>().sqrt();
        let forces = evidence.output(name)?.forces_ha_bohr()?;
        if forces.len() != moved.len() || step == 0.0 {
            return Err(format!("{name}: one force per atom and a nonzero move"));
        }
        let work: f64 = (forces.iter().zip(&moved))
            .map(|(f, r)| (0..3).map(|i| f[i] * r[i]).sum::<f64>())
            .sum();
        let rise = evidence.output(&plus)?.energy_ha("free_energy_ha")?
            - evidence.output(&minus)?.energy_ha("free_energy_ha")?;
        metrics.insert(metric.to_owned(), (rise + work).abs() / step);
        if magnetic {
            let moment = evidence.output(name)?.magnetization_bohr_magneton()?;
            if moment.abs() < MAGNETIC {
                return Err(format!("{name}: a moment of {moment} μB, below {MAGNETIC}"));
            }
            for other in [&plus, &minus] {
                let m = evidence.output(other)?.magnetization_bohr_magneton()?;
                spread = spread.max((m - moment).abs());
            }
        }
    }
    Ok(spread)
}

fn symmetric(evidence: &Evidence, metrics: &mut Metrics) -> Result<(), String> {
    let (reduced, full) = (
        evidence.output("fe-sym")?.forces_ha_bohr()?,
        evidence.output("fe-sym-full")?.forces_ha_bohr()?,
    );
    if evidence.input("fe-sym")?.symmetry()? == Some(false)
        || evidence.input("fe-sym-full")?.symmetry()? != Some(false)
        || reduced.len() != full.len()
    {
        return Err(
            "fe-sym must use symmetry, and fe-sym-full set kpoints.symmetry = false".into(),
        );
    }
    let apart = (reduced.iter().flatten().zip(full.iter().flatten()))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    metrics.insert("symmetry_force_difference_ha_bohr".to_owned(), apart);
    Ok(())
}
