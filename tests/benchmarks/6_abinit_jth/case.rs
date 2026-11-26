use serde::Deserialize;
use std::collections::BTreeMap;
use xtask::harness::{Evidence, Metrics};

#[derive(Deserialize)]
struct Abinit {
    free_energy_ha: f64,
    fermi_level_ha: f64,
    kpoints_fractional: Vec<[f64; 3]>,
    eigenvalues_ha: Vec<Vec<Vec<f64>>>,
    magnetization_bohr_magneton: Option<f64>,
    #[serde(flatten)]
    terms: BTreeMap<String, f64>,
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let (mut free, mut term, mut eigenvalue, mut fermi, mut moment) =
        (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    for name in evidence.names() {
        let abinit: Abinit = evidence.full_mesh_reference(name)?;
        let output = evidence.output(name)?;
        for (key, theirs) in &abinit.terms {
            term = term.max((output.energy_ha(key)? - theirs).abs());
        }
        free = free.max((output.energy_ha("free_energy_ha")? - abinit.free_energy_ha).abs());
        fermi = fermi.max((output.fermi_level_ha()? - abinit.fermi_level_ha).abs());
        if let Some(theirs) = abinit.magnetization_bohr_magneton {
            moment = moment.max((output.magnetization_bohr_magneton()? - theirs).abs());
        }
        let values = output.eigenvalues_ha()?;
        if values.len() != abinit.eigenvalues_ha.len() {
            return Err(format!(
                "{name}: {} spins, {} in ABINIT",
                values.len(),
                abinit.eigenvalues_ha.len()
            ));
        }
        let kpoints = output.kpoints_fractional()?;
        if values.iter().any(|v| v.len() != kpoints.len())
            || kpoints.len() != abinit.kpoints_fractional.len()
        {
            return Err(format!(
                "{name}: {} k-points, {:?} eigenvalue rows per spin, {} in ABINIT",
                kpoints.len(),
                values.iter().map(Vec::len).collect::<Vec<_>>(),
                abinit.kpoints_fractional.len()
            ));
        }
        for (values, theirs) in values.iter().zip(&abinit.eigenvalues_ha) {
            for (k, bands) in kpoints.iter().zip(values) {
                let j = same_kpoint(&abinit.kpoints_fractional, k)
                    .ok_or_else(|| format!("{name}: ABINIT has no k-point {k:?}"))?;
                if bands.is_empty() || bands.len() < theirs[j].len() {
                    return Err(format!(
                        "{name}: {} bands at {k:?}, {} in ABINIT",
                        bands.len(),
                        theirs[j].len()
                    ));
                }
                for (ours, abinit) in bands.iter().zip(&theirs[j]) {
                    eigenvalue = eigenvalue.max((ours - abinit).abs());
                }
            }
        }
    }
    Ok(Metrics::from([
        ("free_energy_error_ha".to_owned(), free),
        ("term_error_ha".to_owned(), term),
        ("eigenvalue_error_ha".to_owned(), eigenvalue),
        ("fermi_level_error_ha".to_owned(), fermi),
        ("magnetization_error_bohr_magneton".to_owned(), moment),
    ]))
}

fn same_kpoint(list: &[[f64; 3]], k: &[f64; 3]) -> Option<usize> {
    list.iter().position(|q| {
        (0..3).all(|i| {
            let d = q[i] - k[i];
            (d - d.round()).abs() < 1e-9
        })
    })
}
