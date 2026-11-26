use serde::Deserialize;
use xtask::harness::{Evidence, Matrices, Metrics};

#[derive(Deserialize)]
struct Abinit {
    kinetic_energy_ha: f64,
    hartree_energy_ha: f64,
    xc_energy_ha: f64,
    ewald_energy_ha: f64,
    psp_core_energy_ha: f64,
    local_energy_ha: f64,
    one_center_energy_ha: f64,
    smearing_energy_ha: f64,
    free_energy_ha: f64,
    core_offset_ha: f64,
    fermi_level_ha: f64,
    kpoints_fractional: Vec<[f64; 3]>,
    eigenvalues_ha: Vec<Vec<Vec<f64>>>,
    magnetization_bohr_magneton: Option<f64>,
    forces_ha_bohr: Option<Vec<[f64; 3]>>,
    force_mean_ha_bohr: Option<[f64; 3]>,
}

#[derive(Deserialize)]
struct Dump {
    element: String,
    rho_ij: Matrices,
    paw_xc_energy_ha: [f64; 2],
    paw_xc_d_ha: Matrices,
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let (mut free, mut term, mut eigenvalue, mut fermi, mut moment) =
        (0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64, 0.0_f64);
    let (mut sphere, mut d, mut probed) = (0.0_f64, 0.0_f64, 0);
    let (mut force, mut mean, mut forced) = (0.0_f64, 0.0_f64, 0);
    for name in evidence.names() {
        let text = evidence.reference(&format!("{name}.json"))?;
        let parsed = |e: serde_json::Error| format!("reference/{name}.json: {e}");
        if text.contains("\"rho_ij\"") {
            let dump: Dump = serde_json::from_str(&text).map_err(parsed)?;
            if evidence.input(name)?.probe_rho_ij()? != (dump.element, dump.rho_ij) {
                return Err(format!("{name}: probe.rho_ij must equal the dump's"));
            }
            let output = evidence.output(name)?;
            let energies = output.paw_xc_energy_ha()?;
            for (ours, theirs) in energies.iter().zip(dump.paw_xc_energy_ha) {
                sphere = sphere.max((ours - theirs).abs());
            }
            let ours = output.paw_xc_d_ha()?;
            let shape = |m: &Matrices| {
                m.iter()
                    .map(|s| s.iter().map(Vec::len).collect::<Vec<_>>())
                    .collect::<Vec<_>>()
            };
            if shape(&ours) != shape(&dump.paw_xc_d_ha) {
                return Err(format!(
                    "{name}: paw_xc_d_ha has another shape than ABINIT's dijxc"
                ));
            }
            let pairs = ours
                .iter()
                .flatten()
                .flatten()
                .zip(dump.paw_xc_d_ha.iter().flatten().flatten());
            d = pairs.fold(d, |worst, (ours, theirs)| worst.max((ours - theirs).abs()));
            probed += 1;
            continue;
        }
        if evidence.input(name)?.symmetry()? == Some(false) {
            return Err(format!(
                "{name}: runs with symmetry, as the case's cost assumes"
            ));
        }
        let abinit: Abinit = serde_json::from_str(&text).map_err(parsed)?;
        let output = evidence.output(name)?;
        let terms = [
            ("kinetic_energy_ha", abinit.kinetic_energy_ha),
            ("hartree_energy_ha", abinit.hartree_energy_ha),
            ("xc_energy_ha", abinit.xc_energy_ha),
            ("ewald_energy_ha", abinit.ewald_energy_ha),
            ("psp_core_energy_ha", abinit.psp_core_energy_ha),
            ("local_energy_ha", abinit.local_energy_ha),
            (
                "one_center_energy_ha",
                abinit.one_center_energy_ha + abinit.core_offset_ha,
            ),
            ("smearing_energy_ha", abinit.smearing_energy_ha),
        ];
        for (key, theirs) in terms {
            term = term.max((output.energy_ha(key)? - theirs).abs());
        }
        let theirs = abinit.free_energy_ha + abinit.core_offset_ha;
        free = free.max((output.energy_ha("free_energy_ha")? - theirs).abs());
        fermi = fermi.max((output.fermi_level_ha()? - abinit.fermi_level_ha).abs());
        if let Some(theirs) = abinit.magnetization_bohr_magneton {
            moment = moment.max((output.magnetization_bohr_magneton()? - theirs).abs());
        }
        if let (Some(theirs), Some(drift)) = (&abinit.forces_ha_bohr, abinit.force_mean_ha_bohr) {
            let (relative, ours) =
                upright_forces(evidence.input(name)?.cell_bohr()?, output.forces_ha_bohr()?);
            if relative.len() != theirs.len() {
                return Err(format!(
                    "{name}: {} forces, {} in ABINIT",
                    relative.len(),
                    theirs.len()
                ));
            }
            for (a, b) in relative.iter().flatten().zip(theirs.iter().flatten()) {
                force = force.max((a - b).abs());
            }
            mean = (0..3).fold(mean, |worst, i| worst.max((ours[i] - drift[i]).abs()));
            forced += 1;
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
        if values.iter().any(|v| v.len() != kpoints.len()) || kpoints.is_empty() {
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
    if probed == 0 {
        return Err("no input probes a sphere of ABINIT's dump".into());
    }
    if forced == 0 {
        return Err("no row compares forces".into());
    }
    Ok(Metrics::from([
        ("paw_xc_energy_error_ha".to_owned(), sphere),
        ("paw_xc_d_error_ha".to_owned(), d),
        ("free_energy_error_ha".to_owned(), free),
        ("term_error_ha".to_owned(), term),
        ("eigenvalue_error_ha".to_owned(), eigenvalue),
        ("fermi_level_error_ha".to_owned(), fermi),
        ("magnetization_error_bohr_magneton".to_owned(), moment),
        ("force_error_ha_bohr".to_owned(), force),
        ("force_mean_error_ha_bohr".to_owned(), mean),
    ]))
}

fn upright_forces(a: [[f64; 3]; 3], forces: Vec<[f64; 3]>) -> (Vec<[f64; 3]>, [f64; 3]) {
    let cross = |u: [f64; 3], v: [f64; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let unit = |v: [f64; 3]| {
        let n = v.iter().map(|x| x * x).sum::<f64>().sqrt();
        v.map(|x| x / n)
    };
    let (e1, e3) = (unit(a[0]), unit(cross(a[0], a[1])));
    let frame = [e1, cross(e3, e1), e3];
    let turned: Vec<[f64; 3]> = (forces.iter())
        .map(|f| frame.map(|e| (0..3).map(|i| e[i] * f[i]).sum()))
        .collect();
    let count = turned.len() as f64;
    let mean: [f64; 3] = std::array::from_fn(|i| turned.iter().map(|f| f[i]).sum::<f64>() / count);
    let relative = turned
        .iter()
        .map(|f| std::array::from_fn(|i| f[i] - mean[i]))
        .collect();
    (relative, mean)
}

fn same_kpoint(list: &[[f64; 3]], k: &[f64; 3]) -> Option<usize> {
    list.iter().position(|q| {
        (0..3).all(|i| {
            let d = q[i] - k[i];
            (d - d.round()).abs() < 1e-9
        })
    })
}
