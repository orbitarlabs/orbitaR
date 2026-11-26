use xtask::harness::{self, Evidence, Metrics, acwf, eos};

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let ae = acwf::published(&evidence.reference("acwf-ae-average.json")?)?;
    let mut metrics = Metrics::new();
    let mut worst = 0.0;
    for (system, curve) in evidence.curves()? {
        let theirs = ae(&system)?;
        let (epsilon, nu) = match eos::fit(&curve.points) {
            Ok(fit) if curve.points.len() == acwf::SCALES.len() => {
                let ours = fit.per(curve.atoms as f64);
                (eos::epsilon(&ours, &theirs), eos::nu(&ours, &theirs))
            }
            _ => (f64::NAN, f64::NAN),
        };
        worst = harness::worst(worst, epsilon);
        metrics.insert(format!("epsilon/{system}"), epsilon);
        metrics.insert(format!("nu/{system}"), nu);
    }
    metrics.insert("epsilon".to_owned(), worst);
    Ok(metrics)
}
