use crate::io::{paw_xml::xml, run::directory};
use crate::{EMPTY_CUBE, EMPTY_CUBE_OUTCOME, synthetic_draft};
use orbitar::io::{DatasetFiles, probe_json, read_spec, to_json};
use orbitar::ks::{Probed, run};
use orbitar::paw::Dataset;
use serde_json::Value;
use std::collections::BTreeSet;

#[test]
fn the_json_of_the_empty_cube_is_its_outcome_on_one_line() {
    let outcome = run(
        &read_spec(EMPTY_CUBE).unwrap(),
        |_| unreachable!("the empty lattice reads no dataset"),
        |_, _| {},
    )
    .unwrap();
    let want: Value = serde_json::from_str(EMPTY_CUBE_OUTCOME).unwrap();
    assert_eq!(to_json(&outcome), want.to_string());
}

#[test]
fn a_probe_prints_its_one_field() {
    let printed = [
        (Probed::EwaldEnergy(-1.5), r#"{"ewald_energy_ha":-1.5}"#),
        (Probed::HartreeEnergy(0.25), r#"{"hartree_energy_ha":0.25}"#),
        (
            Probed::XcEnergyDensity(vec![(-0.5, -0.0625)]),
            r#"{"xc_energy_density_ha_bohr3":[[-0.5,-0.0625]]}"#,
        ),
        (
            Probed::PawXc {
                xc_ha: [-2.5, -0.75],
                d_ha: vec![vec![vec![0.5]]],
            },
            r#"{"paw_xc_d_ha":[[[0.5]]],"paw_xc_energy_ha":[-2.5,-0.75]}"#,
        ),
    ];
    for (probed, want) in printed {
        assert_eq!(probe_json(&probed), want);
    }
}

const ATOM_FIELDS: [&str; 14] = [
    "free_energy_ha",
    "internal_energy_ha",
    "kinetic_energy_ha",
    "hartree_energy_ha",
    "xc_energy_ha",
    "ewald_energy_ha",
    "psp_core_energy_ha",
    "local_energy_ha",
    "one_center_energy_ha",
    "smearing_energy_ha",
    "fermi_level_ha",
    "occupations",
    "scf_steps",
    "datasets",
];
const PAIR_FORCES: [[f64; 3]; 2] = [[0.0, -0.108, 0.0], [0.0, 0.108, 0.0]];
const PAIR_STRESS: [[f64; 3]; 3] = [[-0.0247, 0.0, 0.0], [0.0, -0.015, 0.0], [0.0, 0.0, -0.0247]];

#[test]
fn an_scf_outcome_prints_the_fields_of_atoms_and_each_asked_output() {
    let input = "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\
                 species = [\"C\"]\npositions_fractional = [[0.1, 0.2, 0.3]]\n\n[basis]\ncutoff_ha = 3.0\n\n\
                 [kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[datasets]\nfiles = { C = \"own/C.xml\" }\n";
    let root = directory(
        "outcome-fields",
        &[("datasets/own/C.xml", xml(&synthetic_draft()))],
    );
    let spec = read_spec(input).unwrap();
    let mut files = DatasetFiles::at(&spec, &root.join("datasets"));
    let outcome = run(&spec, |z| files.read(z), |_, _| {}).unwrap();
    let printed: Value = serde_json::from_str(&files.json(&outcome)).unwrap();
    let keys = |v: &Value| {
        v.as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect::<BTreeSet<_>>()
    };
    let lattice: Value = serde_json::from_str(EMPTY_CUBE_OUTCOME).unwrap();
    let both: BTreeSet<String> = (keys(&lattice).into_iter())
        .chain(ATOM_FIELDS.map(str::to_owned))
        .collect();
    assert_eq!(keys(&printed), both);
    let spin = format!(
        "{input}\n[spin]\nmoments = [1.0]\n\n[occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.002\n"
    );
    let outcome = run(
        &read_spec(&spin).unwrap(),
        |_| Ok(Dataset::try_from(synthetic_draft()).unwrap()),
        |_, _| {},
    )
    .unwrap();
    let printed: Value = serde_json::from_str(&to_json(&outcome)).unwrap();
    let mut added = keys(&printed);
    added.retain(|k| !both.contains(k));
    assert_eq!(
        added,
        BTreeSet::from(["magnetization_bohr_magneton".to_owned()])
    );
    let pair = input
        .replace("[\"C\"]", "[\"C\", \"C\"]")
        .replace("[[0.1, 0.2, 0.3]]", "[[0.1, 0.2, 0.3], [0.6, 0.55, 0.8]]");
    let forces = format!("{pair}\n[outputs]\nforces = true\n");
    let outcome = run(
        &read_spec(&forces).unwrap(),
        |_| Ok(Dataset::try_from(synthetic_draft()).unwrap()),
        |_, _| {},
    )
    .unwrap();
    let printed: Value = serde_json::from_str(&to_json(&outcome)).unwrap();
    let mut added = keys(&printed);
    added.retain(|k| !both.contains(k));
    assert_eq!(added, BTreeSet::from(["forces_ha_bohr".to_owned()]));
    let got: Vec<[f64; 3]> = serde_json::from_value(printed["forces_ha_bohr"].clone()).unwrap();
    assert_eq!(got.len(), PAIR_FORCES.len());
    for (g, r) in got.iter().flatten().zip(PAIR_FORCES.iter().flatten()) {
        assert!((g - r).abs() < 5e-4, "{got:?} vs {PAIR_FORCES:?}");
    }
    let stress = format!("{pair}\n[outputs]\nstress = true\n");
    let outcome = run(
        &read_spec(&stress).unwrap(),
        |_| Ok(Dataset::try_from(synthetic_draft()).unwrap()),
        |_, _| {},
    )
    .unwrap();
    let printed: Value = serde_json::from_str(&to_json(&outcome)).unwrap();
    let mut added = keys(&printed);
    added.retain(|k| !both.contains(k));
    assert_eq!(added, BTreeSet::from(["stress_ha_bohr3".to_owned()]));
    let got: [[f64; 3]; 3] = serde_json::from_value(printed["stress_ha_bohr3"].clone()).unwrap();
    for (g, r) in got.iter().flatten().zip(PAIR_STRESS.iter().flatten()) {
        assert!((g - r).abs() < 5e-5, "{got:?} vs {PAIR_STRESS:?}");
    }
}
