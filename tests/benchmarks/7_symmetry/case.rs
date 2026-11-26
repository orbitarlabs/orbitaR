use xtask::harness::{Evidence, Metrics};

const PAIRS: [(&str, &str, bool, usize); 4] = [
    ("input", "si-full", false, 8),
    ("fe", "fe-full", true, 4),
    ("afm", "afm-full", true, 4),
    ("l12", "l12-full", false, 4),
];

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    pairs(evidence, &PAIRS)
}

pub fn pairs(evidence: &Evidence, table: &[(&str, &str, bool, usize)]) -> Result<Metrics, String> {
    let (mut term, mut fermi, mut moment, mut eigenvalue, mut weighted) =
        (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for &(reduced, full, spin, count) in table {
        if evidence.input(reduced)?.symmetry()? == Some(false)
            || evidence.input(full)?.symmetry()? != Some(false)
        {
            return Err(format!(
                "{reduced} must use symmetry and {full} must set kpoints.symmetry = false"
            ));
        }
        let (a, b) = (evidence.output(reduced)?, evidence.output(full)?);
        term = term.max(a.largest_energy_difference(&b, 1.0)?);
        fermi = fermi.max((a.fermi_level_ha()? - b.fermi_level_ha()?).abs());
        if spin {
            moment = moment
                .max((a.magnetization_bohr_magneton()? - b.magnetization_bohr_magneton()?).abs());
        }
        let (points, weights, all) = (
            a.kpoints_fractional()?,
            a.kpoint_weights()?,
            b.kpoints_fractional()?,
        );
        let sum: f64 = weights.iter().sum();
        if points.len() != count || weights.len() != count || (sum - 1.0).abs() > 1e-12 {
            return Err(format!(
                "{reduced}: {} points with weights summing to {sum}, not {count} of the {} on the full mesh",
                points.len(),
                all.len()
            ));
        }
        for (ours, theirs) in a.eigenvalues_ha()?.iter().zip(&b.eigenvalues_ha()?) {
            let mean: f64 = (ours.iter().zip(&weights))
                .map(|(row, w)| w * row.iter().sum::<f64>())
                .sum();
            let whole = theirs.iter().flatten().sum::<f64>() / all.len() as f64;
            weighted = weighted.max((mean - whole).abs());
            for (k, bands) in points.iter().zip(ours) {
                let j = all
                    .iter()
                    .position(|q| q == k)
                    .ok_or_else(|| format!("{full} has no k-point {k:?}"))?;
                for (x, y) in bands.iter().zip(&theirs[j]) {
                    eigenvalue = eigenvalue.max((x - y).abs());
                }
            }
        }
    }
    Ok(Metrics::from([
        ("term_difference_ha".to_owned(), term),
        ("fermi_level_difference_ha".to_owned(), fermi),
        ("magnetization_difference_bohr_magneton".to_owned(), moment),
        ("eigenvalue_difference_ha".to_owned(), eigenvalue),
        ("weighted_eigenvalue_difference_ha".to_owned(), weighted),
    ]))
}
