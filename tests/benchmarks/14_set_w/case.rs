use serde::Deserialize;
use xtask::harness::{Evidence, Metrics};

const ELECTRONS: f64 = 1e-6;

#[derive(Deserialize)]
struct Abinit {
    fermi_level_ha: f64,
    eigenvalues_ha: Vec<Vec<Vec<f64>>>,
}

fn tsmear_ha(abi: &str) -> Result<f64, String> {
    let width: f64 = (abi.lines())
        .find_map(|line| line.strip_prefix("tsmear "))
        .ok_or("no tsmear line")?
        .trim()
        .parse()
        .map_err(|e| format!("tsmear: {e}"))?;
    if width.is_finite() && width > 0.0 {
        Ok(width)
    } else {
        Err(format!("tsmear {width} is not a finite positive width"))
    }
}

fn reference_electrons(abinit: &Abinit, width: f64) -> f64 {
    let full = 2.0 / abinit.eigenvalues_ha.len() as f64;
    (abinit.eigenvalues_ha.iter().flatten())
        .map(|k| {
            let fill =
                |e: &f64| full / (1.0 + ((e - abinit.fermi_level_ha) / width).min(700.0).exp());
            k.iter().map(fill).sum::<f64>() / abinit.eigenvalues_ha[0].len() as f64
        })
        .sum()
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    for name in evidence.names() {
        let abinit: Abinit = evidence.full_mesh_reference(name)?;
        let output = evidence.output(name)?;
        let weights = output.kpoint_weights()?;
        let orbitar: f64 = (output.occupations()?.iter())
            .map(|spin| {
                spin.iter()
                    .zip(&weights)
                    .map(|(k, w)| w * k.iter().sum::<f64>())
                    .sum::<f64>()
            })
            .sum();
        let width = tsmear_ha(&evidence.reference(&format!("{name}.abi"))?)
            .map_err(|e| format!("reference/{name}.abi: {e}"))?;
        let reference = reference_electrons(&abinit, width);
        let gap = (orbitar - reference).abs();
        if gap.is_nan() || gap > ELECTRONS {
            return Err(format!(
                "{name}: orbitaR holds {orbitar} electrons, its reference {reference}"
            ));
        }
    }
    super::n11_speed_scaling::measure(evidence)
}
