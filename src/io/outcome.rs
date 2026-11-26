use crate::ks::{Outcome, Probed};
use serde_json::{Value, json};

pub fn to_json(outcome: &Outcome) -> String {
    fields(outcome).to_string()
}

pub(super) fn fields(outcome: &Outcome) -> Value {
    let mut fields = json!({
        "basis_sizes": outcome.basis_sizes(),
        "eigenvalues_ha": outcome.eigenvalues_ha(),
        "kpoints_fractional": outcome.kpoints().points(),
        "kpoint_weights": outcome.kpoints().weights(),
    });
    if let (Some(scf), Some(map)) = (outcome.scf(), fields.as_object_mut()) {
        let e = &scf.energies;
        for (key, value) in [
            ("free_energy_ha", json!(e.free_ha())),
            ("internal_energy_ha", json!(e.internal_ha())),
            ("kinetic_energy_ha", json!(e.kinetic_ha)),
            ("hartree_energy_ha", json!(e.hartree_ha)),
            ("xc_energy_ha", json!(e.xc_ha)),
            ("ewald_energy_ha", json!(e.ewald_ha)),
            ("psp_core_energy_ha", json!(e.psp_core_ha)),
            ("local_energy_ha", json!(e.local_ha)),
            ("one_center_energy_ha", json!(e.one_center_ha)),
            ("smearing_energy_ha", json!(e.smearing_ha)),
            ("fermi_level_ha", json!(scf.fermi_level_ha)),
            ("occupations", json!(scf.occupations)),
            ("scf_steps", json!(scf.steps)),
        ] {
            map.insert(key.to_owned(), value);
        }
        if let Some(moment) = scf.magnetization_bohr_magneton {
            map.insert("magnetization_bohr_magneton".to_owned(), json!(moment));
        }
        if let Some(forces) = &scf.forces_ha_bohr {
            map.insert("forces_ha_bohr".to_owned(), json!(forces));
        }
        if let Some(stress) = &scf.stress_ha_bohr3 {
            map.insert("stress_ha_bohr3".to_owned(), json!(stress));
        }
    }
    if let (Some(d), Some(map)) = (outcome.discretization(), fields.as_object_mut()) {
        let operations: Vec<Value> = (d.operations.iter())
            .map(|(rotation, translation)| json!({ "rotation": rotation, "translation": translation }))
            .collect();
        let hex = |h: u64| format!("{h:016x}");
        map.insert(
            "discretization".to_owned(),
            json!({
                "operations": operations,
                "coarse_grid": d.coarse_grid,
                "fine_grid": d.fine_grid,
                "plane_waves": hex(d.plane_waves),
                "density_spheres": d.density_spheres.map(hex),
                "nyquist": d.nyquist,
            }),
        );
    }
    fields
}

pub fn probe_json(probed: &Probed) -> String {
    match probed {
        Probed::EwaldEnergy(e) => json!({ "ewald_energy_ha": e }),
        Probed::HartreeEnergy(e) => json!({ "hartree_energy_ha": e }),
        Probed::XcEnergyDensity(points) => json!({ "xc_energy_density_ha_bohr3": points }),
        Probed::PawXc { xc_ha, d_ha } => json!({ "paw_xc_energy_ha": xc_ha, "paw_xc_d_ha": d_ha }),
    }
    .to_string()
}
