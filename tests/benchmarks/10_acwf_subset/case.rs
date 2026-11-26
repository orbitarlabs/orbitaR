use serde::Deserialize;
use xtask::harness::{self, Evidence, Metrics, acwf, eos};

#[derive(Deserialize)]
struct Abinit {
    system: String,
    natoms: usize,
    ecut_ha: f64,
    kmesh: [u64; 3],
    volumes: Vec<f64>,
    energies_ev: Vec<f64>,
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let ours = evidence.curves()?;
    let text = evidence.reference("abinit.jsonl")?;
    let (mut error, mut epsilon, mut systems, mut whole) = (0.0_f64, 0.0_f64, 0, 0);
    let ae = acwf::published(&evidence.reference("acwf-ae-average.json")?)?;
    let qe = acwf::published(&evidence.reference("acwf-qe-sssp-1.3.json")?)?;
    let mut readouts = Metrics::new();
    for line in text.lines() {
        let abinit: Abinit =
            serde_json::from_str(line).map_err(|e| format!("reference/abinit.jsonl: {e}"))?;
        let system = &abinit.system;
        if abinit.volumes.len() != abinit.energies_ev.len() {
            return Err(format!(
                "{system}: reference/abinit.jsonl holds {} volumes and {} energies",
                abinit.volumes.len(),
                abinit.energies_ev.len()
            ));
        }
        let theirs: Vec<(f64, f64)> = (abinit.volumes.iter().copied())
            .zip(abinit.energies_ev.iter().copied())
            .collect();
        let atoms = abinit.natoms as f64;
        let fit = |points: &[(f64, f64)]| eos::fit(points).map_err(|e| format!("{system}: {e}"));
        let (abinit_fit, ae_fit) = (fit(&theirs)?.per(atoms), ae(system)?);
        readouts.insert(
            format!("epsilon_ae/abinit_jth_2.0/{system}"),
            eos::epsilon(&abinit_fit, &ae_fit),
        );
        readouts.insert(
            format!("epsilon_ae/qe_sssp_1.3/{system}"),
            eos::epsilon(&qe(system)?, &ae_fit),
        );
        let Some(curve) = ours.get(system) else {
            continue;
        };
        for name in
            (evidence.names().into_iter()).filter(|name| acwf::system_of(name) == Some(system))
        {
            let input = evidence.input(name)?;
            let (cutoff, mesh) = (input.cutoff_ha()?, input.mesh()?);
            if cutoff != abinit.ecut_ha || mesh != abinit.kmesh {
                return Err(format!(
                    "{name}: cutoff {cutoff} Ha and mesh {mesh:?}; ABINIT ran {} Ha and {:?}",
                    abinit.ecut_ha, abinit.kmesh
                ));
            }
        }
        if curve.atoms != abinit.natoms || curve.points.len() > theirs.len() {
            return Err(format!(
                "{system}: {} atoms at {} volumes, ABINIT {} at {}",
                curve.atoms,
                curve.points.len(),
                abinit.natoms,
                theirs.len()
            ));
        }
        for &(volume, energy) in &curve.points {
            let (_, e) = (theirs.iter())
                .find(|(v, _)| (v - volume).abs() <= 1e-9 * v)
                .ok_or_else(|| format!("{system}: ABINIT has no volume {volume} Å³"))?;
            error = harness::worst(error, (energy - e).abs());
        }
        systems += 1;
        if curve.points.len() < theirs.len() {
            continue;
        }
        let ours_fit = fit(&curve.points)?.per(atoms);
        epsilon = harness::worst(epsilon, eos::epsilon(&ours_fit, &abinit_fit));
        readouts.insert(
            format!("epsilon_ae/orbitar/{system}"),
            eos::epsilon(&ours_fit, &ae_fit),
        );
        whole += 1;
    }
    if systems != ours.len() {
        return Err(format!(
            "orbitaR ran {} crystals, of which reference/abinit.jsonl holds {systems}",
            ours.len()
        ));
    }
    let mut metrics = Metrics::from([("free_energy_error_ev".to_owned(), error)]);
    if whole > 0 {
        metrics.insert("epsilon".to_owned(), epsilon);
    }
    metrics.extend(readouts);
    Ok(metrics)
}
