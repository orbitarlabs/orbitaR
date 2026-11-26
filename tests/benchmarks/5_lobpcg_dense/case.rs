use xtask::harness::{Evidence, Metrics};

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let (iterative, dense) = (evidence.input("input")?, evidence.input("dense")?);
    if iterative.solver()?.is_some_and(|s| s != "lobpcg") || dense.solver()? != Some("dense") {
        return Err("input.toml must solve by lobpcg and inputs/dense.toml by dense".into());
    }
    let (a, b) = (evidence.output("input")?, evidence.output("dense")?);
    let (p, q) = (a.eigenvalues_ha()?, b.eigenvalues_ha()?);
    let sizes = a.basis_sizes()?;
    if sizes != b.basis_sizes()? {
        return Err("the two inputs must solve in the same plane waves".into());
    }
    let bands = p.iter().flatten().map(Vec::len).max().unwrap_or(0);
    let smallest = sizes.into_iter().min().unwrap_or(0);
    if smallest <= 3 * (bands + 3) {
        return Err(format!(
            "input.toml: {smallest} plane waves at a k-point for {bands} bands take the dense path, not LOBPCG"
        ));
    }
    let flat = |e: &Vec<Vec<Vec<f64>>>| e.iter().flatten().flatten().copied().collect::<Vec<f64>>();
    let (p, q) = (flat(&p), flat(&q));
    if p.len() != q.len() || p.is_empty() {
        return Err(format!("{} eigenvalues against {}", p.len(), q.len()));
    }
    let eigenvalue = p
        .iter()
        .zip(&q)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max);
    let free = (a.energy_ha("free_energy_ha")? - b.energy_ha("free_energy_ha")?).abs();
    Ok(Metrics::from([
        ("eigenvalue_difference_ha".to_owned(), eigenvalue),
        ("free_energy_difference_ha".to_owned(), free),
    ]))
}
