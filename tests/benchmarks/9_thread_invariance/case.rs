use xtask::harness::{Evidence, Metrics};

const PAIRS: [(&str, &str, bool); 2] = [("input", "al-4", false), ("afm-1", "afm-4", true)];

fn largest(a: &[f64], b: &[f64], field: &str) -> Result<f64, String> {
    if a.len() != b.len() {
        return Err(format!("{field}: {} values against {}", a.len(), b.len()));
    }
    Ok(a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max))
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = Metrics::new();
    let mut worst = |name: &str, value: f64| {
        let slot = metrics.entry(name.to_owned()).or_insert(0.0);
        *slot = slot.max(value);
    };
    for (one, four, spin) in PAIRS {
        if evidence.input(one)?.threads()?.map(|n| n.get()) != Some(1)
            || evidence.input(four)?.threads()?.map(|n| n.get()) != Some(4)
        {
            return Err(format!("{one} must set parallel.threads = 1 and {four} 4"));
        }
        let (a, b) = (evidence.output(one)?, evidence.output(four)?);
        let flat = |cube: Vec<Vec<Vec<f64>>>| -> Vec<f64> {
            cube.into_iter().flatten().flatten().collect()
        };
        let counts = |v: Vec<usize>| -> Vec<f64> { v.into_iter().map(|n| n as f64).collect() };
        worst("term_difference_ha", a.largest_energy_difference(&b, 1.0)?);
        worst(
            "fermi_level_difference_ha",
            (a.fermi_level_ha()? - b.fermi_level_ha()?).abs(),
        );
        if spin {
            let m = a.magnetization_bohr_magneton()? - b.magnetization_bohr_magneton()?;
            worst("magnetization_difference_bohr_magneton", m.abs());
        }
        worst(
            "eigenvalue_difference_ha",
            largest(
                &flat(a.eigenvalues_ha()?),
                &flat(b.eigenvalues_ha()?),
                "eigenvalues_ha",
            )?,
        );
        worst(
            "occupation_difference",
            largest(
                &flat(a.occupations()?),
                &flat(b.occupations()?),
                "occupations",
            )?,
        );
        let points = |o: &xtask::harness::Output| -> Result<Vec<f64>, String> {
            Ok(o.kpoints_fractional()?.into_iter().flatten().collect())
        };
        worst(
            "kpoint_difference",
            largest(&points(&a)?, &points(&b)?, "kpoints_fractional")?,
        );
        worst(
            "weight_difference",
            largest(&a.kpoint_weights()?, &b.kpoint_weights()?, "kpoint_weights")?,
        );
        worst(
            "basis_size_difference",
            largest(
                &counts(a.basis_sizes()?),
                &counts(b.basis_sizes()?),
                "basis_sizes",
            )?,
        );
        worst(
            "scf_step_difference",
            (a.scf_steps()? as f64 - b.scf_steps()? as f64).abs(),
        );
    }
    Ok(metrics)
}
