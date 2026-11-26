use serde::Deserialize;
use xtask::harness::{self, Evidence, Metrics};

#[derive(Deserialize)]
struct Abinit {
    free_energy_ha: f64,
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut error: f64 = 0.0;
    for name in evidence.names() {
        let abinit: Abinit = evidence.full_mesh_reference(name)?;
        let input = evidence.input(name)?;
        let atoms = input.species()?.len() as f64;
        let free = evidence.output(name)?.energy_ha("free_energy_ha")?;
        error = harness::worst(error, (free - abinit.free_energy_ha).abs() / atoms);
    }
    Ok(Metrics::from([(
        "free_energy_error_ha_per_atom".to_owned(),
        error,
    )]))
}
