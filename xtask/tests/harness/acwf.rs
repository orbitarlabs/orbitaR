use super::{CASE, engine};
use crate::{tree, write};
use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use xtask::harness::{
    Evidence, Measure, Metrics, Outcome, Session, Tier,
    acwf::{self, Part},
};

const CRYSTALS: &str = r#"{
 "Si-X/Diamond": {"cell": [[0.0, 2.7351025696286, 2.7351025696286], [2.7351025696286, 0.0, 2.7351025696286], [2.7351025696286, 2.7351025696286, 0.0]], "sites": [["Si", [0.0, 0.0, 0.0]], ["Si", [1.3675512848143, 1.3675512848143, 1.3675512848143]]], "kinds": {"Si": "Si"}},
 "Na-X2O": {"cell": [[0.0, 2.7960562151102, 2.7960562151102], [2.7960562151102, 0.0, 2.7960562151102], [2.7960562151102, 2.7960562151102, 0.0]], "sites": [["Na", [4.1940843226653, 1.3980281075551, 4.1940843226653]], ["Na", [4.1940843226653, 1.3980281075551, 1.3980281075551]], ["O", [0.0, 0.0, 0.0]]], "kinds": {"Na": "Na", "O": "O"}},
 "Fe-X2O5": {"cell": [[4.8770751342874, 0.0, 0.0], [0.0, 4.8770751342874, 0.0], [0.0, 0.0, 4.8770751342874]], "sites": [["Fe", [3.6578063507156, 3.6578063507156, 3.6578063507156]], ["Fe", [1.2192687835719, 1.2192687835719, 3.6578063507156]], ["Fe", [1.2192687835719, 3.6578063507156, 1.2192687835719]], ["Fe", [3.6578063507156, 1.2192687835719, 1.2192687835719]], ["O", [1.2192687835719, 3.6578063507156, 3.6578063507156]], ["O", [3.6578063507156, 1.2192687835719, 3.6578063507156]], ["O", [3.6578063507156, 3.6578063507156, 1.2192687835719]], ["O", [1.2192687835719, 1.2192687835719, 1.2192687835719]], ["O", [2.4385375671437, 2.4385375671437, 0.0]], ["O", [2.4385375671437, 0.0, 2.4385375671437]], ["O", [0.0, 2.4385375671437, 2.4385375671437]], ["O", [2.4385375671437, 0.0, 0.0]], ["O", [0.0, 2.4385375671437, 0.0]], ["O", [0.0, 0.0, 2.4385375671437]]], "kinds": {"Fe": "Fe", "O": "O"}}
}"#;

const TETRAGONAL: &str = r#"{"X": {"cell": [[3.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 5.0]], "sites": [["a", [1.5, 0.75, 1.0]]], "kinds": {"a": "Si"}}}"#;

const TRICLINIC: &str = r#"{"X": {"cell": [[3.0, 0.2, 0.3], [0.4, 4.0, 0.5], [0.6, 0.7, 5.0]], "sites": [["a", [1.0, 2.0, 3.0]]], "kinds": {"a": "Si"}}}"#;

const BOHR: f64 = 0.52917720859;

fn hints(pairs: &[(&str, f64)]) -> BTreeMap<String, f64> {
    pairs.iter().map(|&(e, h)| (e.to_owned(), h)).collect()
}

fn input(system: &str, crystals: &str, hints: &BTreeMap<String, f64>) -> Vec<toml::Table> {
    (acwf::inputs(crystals, hints).unwrap().into_iter())
        .filter(|(name, _)| name.rsplit_once('@').is_some_and(|(s, _)| s == system))
        .map(|(_, text)| toml::from_str(&text).unwrap())
        .collect()
}

fn floats(value: &toml::Value) -> Vec<f64> {
    let mut out = Vec::new();
    match value {
        toml::Value::Array(items) => items.iter().for_each(|v| out.extend(floats(v))),
        v => out.push(v.as_float().unwrap()),
    }
    out
}

fn det(c: &[f64]) -> f64 {
    c[0] * (c[4] * c[8] - c[5] * c[7]) - c[1] * (c[3] * c[8] - c[5] * c[6])
        + c[2] * (c[3] * c[7] - c[4] * c[6])
}

fn each(
    system: &str,
    crystals: &str,
    hints: &BTreeMap<String, f64>,
    table: &str,
    key: &str,
) -> Vec<toml::Value> {
    input(system, crystals, hints)
        .into_iter()
        .map(|run| run[table][key].clone())
        .collect()
}

fn seven(value: toml::Value) -> Vec<toml::Value> {
    vec![value; 7]
}

#[test]
fn the_mesh_is_the_reciprocal_length_over_the_k_point_spacing() {
    let mesh = |system, crystals| each(system, crystals, &hints(&[("Si", 1.0)]), "kpoints", "mesh");
    let triple = |m: [i64; 3]| seven(toml::Value::try_from(m).unwrap());
    assert_eq!(mesh("Si-X/Diamond", CRYSTALS), triple([34, 34, 34]));
    assert_eq!(mesh("Na-X2O", CRYSTALS), triple([33, 33, 33]));
    assert_eq!(mesh("Fe-X2O5", CRYSTALS), triple([22, 22, 22]));
    assert_eq!(mesh("X", TETRAGONAL), triple([35, 35, 21]));
    assert_eq!(mesh("X", TRICLINIC), triple([36, 28, 22]));
}

#[test]
fn each_input_scales_the_volume_by_its_scale() {
    for (crystals, volume) in [(TETRAGONAL, 45.0), (TRICLINIC, 57.974)] {
        let runs = input("X", crystals, &hints(&[("Si", 10.0)]));
        assert_eq!(runs.len(), 7);
        for (run, scale) in runs.iter().zip(acwf::SCALES) {
            let got = det(&floats(&run["structure"]["cell_bohr"])) * BOHR.powi(3);
            assert!((got - volume * scale).abs() < 1e-11, "{got} at {scale}");
        }
    }
}

#[test]
fn each_input_keeps_the_fractional_sites() {
    let cases = [
        (TETRAGONAL, [0.5, 0.25, 0.2]),
        (
            TRICLINIC,
            [0.17059371442370716, 0.3951771483768587, 0.5502466622968917],
        ),
    ];
    for (crystals, expected) in cases {
        for site in each(
            "X",
            crystals,
            &hints(&[("Si", 10.0)]),
            "structure",
            "positions_fractional",
        ) {
            let site = floats(&site);
            assert!(
                (site.iter().zip(expected)).all(|(x, y)| (x - y).abs() < 1e-15),
                "{site:?}"
            );
        }
    }
}

#[test]
fn the_cutoff_is_the_highest_hint_of_the_elements_plus_20_ha() {
    let table = hints(&[("Na", 17.5), ("O", 13.0), ("Ni", 25.0)]);
    let cutoff = each("Na-X2O", CRYSTALS, &table, "basis", "cutoff_ha");
    assert_eq!(cutoff, seven(toml::Value::Float(37.5)));
}

#[test]
fn an_element_without_a_hint_takes_the_highest_hint() {
    let table = hints(&[("Na", 17.5), ("Ni", 25.0)]);
    let cutoff = each("Si-X/Diamond", CRYSTALS, &table, "basis", "cutoff_ha");
    assert_eq!(cutoff, seven(toml::Value::Float(45.0)));
}

#[test]
fn a_crystal_without_hints_fails() {
    let error = acwf::inputs(CRYSTALS, &BTreeMap::new()).unwrap_err();
    assert!(error.contains("no cutoff hints"), "{error}");
}

#[test]
fn the_density_cutoff_is_twice_the_cutoff() {
    let density = each(
        "X",
        TETRAGONAL,
        &hints(&[("Si", 10.0)]),
        "basis",
        "density_cutoff_ha",
    );
    assert_eq!(density, seven(toml::Value::Float(60.0)));
}

#[test]
fn the_species_follow_the_site_order() {
    let species = each(
        "Na-X2O",
        CRYSTALS,
        &hints(&[("Na", 17.5), ("O", 13.0)]),
        "structure",
        "species",
    );
    assert_eq!(
        species,
        seven(toml::Value::try_from(["Na", "Na", "O"]).unwrap())
    );
}

#[test]
fn each_input_holds_pbe_fermi_dirac_and_the_scf_stop() {
    for run in input("Na-X2O", CRYSTALS, &hints(&[("Na", 17.5), ("O", 13.0)])) {
        assert_eq!(run["xc"]["functional"].as_str(), Some("pbe"));
        assert_eq!(run["occupations"]["smearing"].as_str(), Some("fermi-dirac"));
        assert_eq!(run["occupations"]["width_ha"].as_float(), Some(0.00225));
        assert_eq!(run["scf"]["energy_tolerance_ha"].as_float(), Some(1e-9));
        assert_eq!(run["scf"]["max_iterations"].as_integer(), Some(500));
    }
}

#[test]
fn each_input_leaves_bands_and_symmetry_to_the_engine() {
    for run in input("Na-X2O", CRYSTALS, &hints(&[("Na", 17.5), ("O", 13.0)])) {
        assert!(run.get("bands").is_none() && run["kpoints"].get("symmetry").is_none());
    }
}

#[test]
fn every_generated_key_appears_in_the_readme_input_schema() {
    let readme =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../README.md")).unwrap();
    let schema = readme
        .split("## Input schema")
        .nth(1)
        .unwrap()
        .split("```")
        .nth(1)
        .unwrap();
    let (mut table, mut keys) = (String::new(), Vec::new());
    for line in schema.lines() {
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.split(']').next()) {
            table = name.to_owned();
        } else if let Some((key, _)) = line.split_once(" = ").filter(|_| !line.starts_with('#')) {
            keys.push(format!("{table}.{}", key.trim()));
        }
    }
    for run in input("Na-X2O", CRYSTALS, &hints(&[("Na", 17.5), ("O", 13.0)])) {
        for (name, value) in &run {
            let inner: Vec<String> = match value.as_table() {
                Some(t) => t.keys().map(|k| format!("{name}.{k}")).collect(),
                None => vec![format!(".{name}")],
            };
            for key in inner {
                assert!(
                    keys.contains(&key),
                    "{key} is not in README's input schema: {keys:?}"
                );
            }
        }
    }
}

#[test]
fn a_site_kind_without_an_element_fails() {
    let text = r#"{"X": {"cell": [[3.0, 0.0, 0.0], [0.0, 3.0, 0.0], [0.0, 0.0, 3.0]], "sites": [["b", [0.0, 0.0, 0.0]]], "kinds": {"a": "Si"}}}"#;
    let error = acwf::inputs(text, &hints(&[("Si", 1.0)])).unwrap_err();
    assert!(error.contains("site kind b"), "{error}");
}

fn seven_runs(evidence: &Evidence) -> Result<Metrics, String> {
    let expected: Vec<String> = ["0.94", "0.96", "0.98", "1.00", "1.02", "1.04", "1.06"]
        .map(|s| format!("Si-X/Diamond@{s}"))
        .into();
    if evidence.names() != expected {
        return Err(format!("names {:?}", evidence.names()));
    }
    let mut error: f64 = 0.0;
    for name in &expected {
        if evidence.input(name)?.cutoff_ha()? != 30.0 {
            return Err(format!("{name}: cutoff"));
        }
        error = error.max((evidence.output(name)?.energy_ha("free_energy_ha")? + 1.0).abs());
    }
    Ok(Metrics::from([("error".to_owned(), error)]))
}

#[test]
fn a_case_with_crystals_json_runs_its_seven_inputs_in_order_at_the_pbe_hint() {
    let si = r#"{"Si-X/Diamond": {"cell": [[0.0, 2.7351025696286, 2.7351025696286], [2.7351025696286, 0.0, 2.7351025696286], [2.7351025696286, 2.7351025696286, 0.0]], "sites": [["Si", [0.0, 0.0, 0.0]], ["Si", [1.3675512848143, 1.3675512848143, 1.3675512848143]]], "kinds": {"Si": "Si"}}}"#;
    let root = tree(
        "acwf-case",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/crystals.json", si),
            ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
            ("remote/JTH-LDA-v2.0/Si/Si.xml", "lda"),
        ],
    );
    let url = format!("file://{}/remote", root.display());
    let rows = [
        ("pbe", "Si", "si", Some(10.0)),
        ("lda", "Si", "lda", Some(50.0)),
    ];
    write(&root, "datasets/jth-2.0.toml", &super::jth(&url, &rows));
    let bin = engine(&root, "printf '{\"free_energy_ha\": -1.0}'");
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    assert_eq!(session.evaluate("1_a", seven_runs), Outcome::Pass);
}

fn si_curve(evidence: &Evidence) -> Result<Metrics, String> {
    let curves = evidence.curves()?;
    let curve = &curves["Si-X/Diamond"];
    let shape = curves.len() == 1 && curve.atoms == 2 && curve.points.len() == acwf::SCALES.len();
    let mut error: f64 = if shape { 0.0 } else { 1.0 };
    for (&(v, e), scale) in curve.points.iter().zip(acwf::SCALES) {
        error = error
            .max((v / (40.92143438604461 * scale) - 1.0).abs())
            .max((e + 27.211386245988).abs());
    }
    Ok(Metrics::from([("error".to_owned(), error)]))
}

fn si_cutoff_and_mesh(evidence: &Evidence) -> Result<Metrics, String> {
    let mut error: f64 = 0.0;
    for name in evidence.names() {
        let input = evidence.input(name)?;
        if input.cutoff_ha()? != 30.0 || input.mesh()? != [34, 34, 34] {
            error = 1.0;
        }
        error = error.max((evidence.output(name)?.energy_ha("free_energy_ha")? + 1.0).abs());
    }
    Ok(Metrics::from([("error".to_owned(), error)]))
}

fn si_outcome(name: &str, measure: Measure) -> Outcome {
    let si = r#"{"Si-X/Diamond": {"cell": [[0.0, 2.7351025696286, 2.7351025696286], [2.7351025696286, 0.0, 2.7351025696286], [2.7351025696286, 2.7351025696286, 0.0]], "sites": [["Si", [0.0, 0.0, 0.0]], ["Si", [1.3675512848143, 1.3675512848143, 1.3675512848143]]], "kinds": {"Si": "Si"}}}"#;
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/crystals.json", si),
            ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
        ],
    );
    let url = format!("file://{}/remote", root.display());
    let table = super::jth(&url, &[("pbe", "Si", "si", Some(10.0))]);
    write(&root, "datasets/jth-2.0.toml", &table);
    let bin = engine(&root, "printf '{\"free_energy_ha\": -1.0}'");
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    session.evaluate("1_a", measure)
}

#[test]
fn the_curves_hold_each_crystal_s_atoms_volumes_and_energies_in_ev() {
    assert_eq!(si_outcome("acwf-curves", si_curve), Outcome::Pass);
}

#[test]
fn each_input_holds_the_cutoff_and_k_mesh_of_the_abinit_run() {
    assert_eq!(si_outcome("acwf-mesh", si_cutoff_and_mesh), Outcome::Pass);
}

const BIRCH_MURNAGHAN: &str = r#"awk '/^cell_bohr/ { gsub(/[][,=]/, " "); \
  v = ($2 * ($6 * $10 - $7 * $9) - $3 * ($5 * $10 - $7 * $8) + $4 * ($5 * $9 - $6 * $8)) * 0.52917720859 ^ 3; \
  if (v < 0) v = -v; r = (40.9 / v) ^ (2 / 3); \
  e = 9 / 16 * 0.55 * 40.9 * ((r - 1) ^ 3 * 4.3 + (r - 1) ^ 2 * (6 - 4 * r)) - 300; \
  printf "{\"free_energy_ha\": %.17g}", e / 27.211386245988 }' input.toml"#;

fn against_reference(evidence: &Evidence) -> Result<Metrics, String> {
    let comparison = &evidence.acwf("ae.json")?["Si-X/Diamond"];
    Ok(Metrics::from([
        (
            "epsilon_error".to_owned(),
            (comparison.epsilon - 0.4652227405459203).abs(),
        ),
        (
            "nu_error".to_owned(),
            (comparison.nu - 0.7363704187366482).abs(),
        ),
        (
            "v0_error_ang3".to_owned(),
            (comparison.fit.v0_ang3 - 40.9).abs(),
        ),
    ]))
}

fn measured(name: &str, reference: &str) -> Outcome {
    let case = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"accuracy\"]\nreference = \"all-electron\"\n\n\
        [tolerance]\nepsilon_error_max = 1e-7\nnu_error_max = 1e-7\nv0_error_ang3_max = 1e-10\n";
    let si = r#"{"Si-X/Diamond": {"cell": [[0.0, 2.7351025696286, 2.7351025696286], [2.7351025696286, 0.0, 2.7351025696286], [2.7351025696286, 2.7351025696286, 0.0]], "sites": [["Si", [0.0, 0.0, 0.0]], ["Si", [1.3675512848143, 1.3675512848143, 1.3675512848143]]], "kinds": {"Si": "Si"}}}"#;
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", case),
            ("benchmarks/1_a/crystals.json", si),
            ("benchmarks/1_a/reference/ae.json", reference),
            ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
        ],
    );
    let url = format!("file://{}/remote", root.display());
    let table = super::jth(&url, &[("pbe", "Si", "si", Some(10.0))]);
    write(&root, "datasets/jth-2.0.toml", &table);
    let bin = engine(&root, BIRCH_MURNAGHAN);
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    session.evaluate("1_a", against_reference)
}

#[test]
fn the_acwf_measure_puts_orbitar_first_and_compares_per_atom() {
    let reference = r#"{"BM_fit_data": {"Si-X/Diamond": {"min_volume": 82.4, "bulk_modulus_ev_ang3": 0.56, "bulk_deriv": 4.4, "E0": 0}}, "num_atoms_in_sim_cell": {"Si-X/Diamond": 4}}"#;
    assert_eq!(measured("acwf-measure", reference), Outcome::Pass);
}

#[test]
fn a_reference_without_the_atom_count_names_it() {
    let reference = r#"{"BM_fit_data": {"Si-X/Diamond": {"min_volume": 82.4, "bulk_modulus_ev_ang3": 0.56, "bulk_deriv": 4.4}}, "num_atoms_in_sim_cell": {}}"#;
    let outcome = measured("acwf-atoms", reference);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("has no num_atoms_in_sim_cell of Si-X/Diamond")),
        "{outcome:?}"
    );
}

#[test]
fn an_unreadable_crystals_json_is_a_harness_fault() {
    let root = tree(
        "acwf-unreadable",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/crystals.json/x", ""),
        ],
    );
    let bin = engine(&root, "printf '{}'");
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    let outcome = session.evaluate("1_a", seven_runs);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("crystals.json")),
        "{outcome:?}"
    );
}

#[test]
fn an_inputs_stem_with_an_at_sign_is_refused() {
    let root = tree(
        "acwf-at",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/inputs/Si@1.00.toml", super::INPUT),
        ],
    );
    let bin = engine(&root, "printf '{}'");
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    let outcome = session.evaluate("1_a", seven_runs);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("`@` marks the generated inputs")),
        "{outcome:?}"
    );
}

#[test]
fn a_crystals_json_without_hints_of_its_family_is_a_harness_fault() {
    let root = tree(
        "acwf-nohints",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/crystals.json", TETRAGONAL),
        ],
    );
    let table = super::jth("file:///nowhere", &[("lda", "Si", "lda", Some(10.0))]);
    write(&root, "datasets/jth-2.0.toml", &table);
    let bin = engine(&root, "printf '{}'");
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    let outcome = session.evaluate("1_a", seven_runs);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets/jth-2.0.toml holds no pbe hint")),
        "{outcome:?}"
    );
}

#[test]
fn a_singular_cell_fails() {
    let text = r#"{"X": {"cell": [[3.0, 0.0, 0.0], [6.0, 0.0, 0.0], [0.0, 0.0, 3.0]], "sites": [["a", [0.0, 0.0, 0.0]]], "kinds": {"a": "Si"}}}"#;
    let error = acwf::inputs(text, &hints(&[("Si", 1.0)])).unwrap_err();
    assert!(error.contains("the cell is singular"), "{error}");
}

pub(crate) fn split() -> acwf::Split {
    acwf::Split::read(concat!(env!("CARGO_MANIFEST_DIR"), "/..").as_ref()).unwrap()
}

fn repository(path: &str) -> String {
    std::fs::read_to_string(format!("{}/../{path}", env!("CARGO_MANIFEST_DIR"))).unwrap()
}

pub(crate) fn jth() -> BTreeSet<String> {
    let family: toml::Table = repository("datasets/jth-2.0.toml").parse().unwrap();
    (family["pbe"]["element"].as_table().unwrap().keys())
        .cloned()
        .collect()
}

#[test]
fn the_split_tunes_on_fcc_bcc_xo_xo3_x2o5_and_examines_sc_diamond_x2o_x2o3_xo2() {
    let parts = [
        ("X/FCC", Part::Tune),
        ("X/BCC", Part::Tune),
        ("XO", Part::Tune),
        ("XO3", Part::Tune),
        ("X2O5", Part::Tune),
        ("X/SC", Part::Exam),
        ("X/Diamond", Part::Exam),
        ("X2O", Part::Exam),
        ("X2O3", Part::Exam),
        ("XO2", Part::Exam),
    ];
    let split = split();
    for (structure, part) in parts {
        assert_eq!(
            split.part(&format!("Fe-{structure}")),
            Ok(part),
            "{structure}"
        );
    }
}

#[test]
fn a_system_outside_the_split_fails_by_name() {
    let split = split();
    let error = split.part("Fe-X/HCP").unwrap_err();
    assert!(
        error.contains("Fe-X/HCP: structure type X/HCP is in no part"),
        "{error}"
    );
    for name in ["X/FCC", "-X2O"] {
        let error = split.part(name).unwrap_err();
        assert!(
            error.contains(&format!("{name} is not <element>-<structure type>")),
            "{error}"
        );
    }
}

#[test]
fn the_exam_part_holds_480_crystals_of_96_elements_and_430_of_the_86_with_a_jth_file() {
    let ae = repository("benchmarks/10_acwf_subset/reference/acwf-ae-average.json");
    let (split, jth) = (split(), jth());
    let (mut exam, mut tune) = (Vec::new(), 0);
    for system in acwf::systems(&ae).unwrap() {
        match split.part(&system).unwrap() {
            Part::Exam => exam.push(system.split_once('-').unwrap().0.to_owned()),
            Part::Tune => tune += 1,
        }
    }
    let elements: BTreeSet<&str> = exam.iter().map(String::as_str).collect();
    let on_jth = exam.iter().filter(|e| jth.contains(*e)).count();
    assert_eq!(
        (tune, exam.len(), elements.len(), on_jth, jth.len()),
        (480, 480, 96, 430, 86)
    );
}

#[test]
fn the_flagged_systems_are_the_nine_crystals_of_case_10_in_the_exam_part() {
    let crystals: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&repository("benchmarks/10_acwf_subset/crystals.json")).unwrap();
    let split = split();
    let exam: BTreeSet<String> = (crystals.into_keys())
        .filter(|system| split.part(system).unwrap() == Part::Exam)
        .collect();
    assert_eq!((split.flagged(), exam.len()), (&exam, 9));
}

#[test]
fn a_flagged_system_outside_the_exam_part_fails_by_name() {
    let text = "flagged = [\"Fe-X2O5\"]\n\n[part]\n\"X2O5\" = \"tune\"\n";
    let root = tree("acwf-flagged", &[("benchmarks/ACWF.toml", text)]);
    let error = acwf::Split::read(&root).unwrap_err();
    assert!(
        error.contains("flagged Fe-X2O5 is not in the exam part"),
        "{error}"
    );
}

#[test]
fn a_part_other_than_tune_or_exam_fails() {
    let text = "flagged = []\n\n[part]\n\"X2O5\" = \"train\"\n";
    let root = tree("acwf-part", &[("benchmarks/ACWF.toml", text)]);
    let error = acwf::Split::read(&root).unwrap_err();
    assert!(
        error.contains("benchmarks/ACWF.toml") && error.contains("`train`"),
        "{error}"
    );
}

#[test]
fn the_tuning_case_holds_the_tune_part_of_the_86_jth_elements_at_jth_2_0() {
    let crystals: BTreeMap<String, serde_json::Value> =
        serde_json::from_str(&repository("benchmarks/18_acwf_tune/crystals.json")).unwrap();
    let (split, jth) = (split(), jth());
    let tune = ["X/FCC", "X/BCC", "XO", "XO3", "X2O5"];
    let expected: BTreeSet<String> = (jth.iter())
        .flat_map(|e| tune.map(|t| format!("{e}-{t}")))
        .collect();
    let systems: BTreeSet<String> = crystals.into_keys().collect();
    assert_eq!((systems.len(), &systems), (430, &expected));
    assert!(systems.iter().all(|s| split.part(s) == Ok(Part::Tune)));
    let table: toml::Table = repository("benchmarks/18_acwf_tune/table.toml")
        .parse()
        .unwrap();
    let elements = table["element"].as_table().unwrap();
    assert_eq!(elements.keys().cloned().collect::<BTreeSet<_>>(), jth);
    assert!(elements.values().all(|v| v.as_str() == Some("jth-2.0")));
    for file in ["acwf-ae-average.json", "acwf-vasp.json"] {
        let text = repository(&format!("benchmarks/18_acwf_tune/reference/{file}"));
        let held: BTreeSet<String> = acwf::systems(&text).unwrap().into_iter().collect();
        assert_eq!(held, systems, "{file}");
    }
}

#[test]
fn blind_refuses_each_exam_system_by_name_and_passes_tune_systems_and_elements() {
    let split = split();
    assert_eq!(
        split.blind(["Al", "input", "Al-X/FCC", "Al-XO3@0.94"]),
        Ok(())
    );
    let error = split
        .blind(["Al-X/FCC", "Si-XO2@0.96", "Si-XO2@1.00", "Al-X/SC"])
        .unwrap_err();
    assert_eq!(
        error,
        "Al-X/SC, Si-XO2 in the ACWF exam part, blind until M5 judges it; the tuning tooling refuses it"
    );
    assert!(
        split
            .blind(["Al-X/HCP"])
            .unwrap_err()
            .contains("in no part")
    );
}

fn tuning_crystals(systems: &[&str]) -> String {
    let cell = SI_CELL;
    let entries: Vec<String> = systems
        .iter()
        .map(|s| {
            let kind = s.split_once('-').unwrap().0;
            format!(
                "\"{s}\": {}",
                cell.replace("\"Si\"", &format!("\"{kind}\""))
            )
        })
        .collect();
    format!("{{{}}}", entries.join(", "))
}

const SI_CELL: &str = r#"{"cell": [[0.0, 2.7351025696286, 2.7351025696286], [2.7351025696286, 0.0, 2.7351025696286], [2.7351025696286, 2.7351025696286, 0.0]], "sites": [["Si", [0.0, 0.0, 0.0]], ["Si", [1.3675512848143, 1.3675512848143, 1.3675512848143]]], "kinds": {"Si": "Si"}}"#;

fn results(systems: &[&str], failed: &[&str]) -> String {
    let fits: Vec<String> = (systems.iter())
        .map(|s| match failed.contains(s) {
            true => format!("\"{s}\": null"),
            false => format!(
                "\"{s}\": {{\"min_volume\": 40.9, \"bulk_modulus_ev_ang3\": 0.55, \"bulk_deriv\": 4.3}}"
            ),
        })
        .collect();
    let atoms: Vec<String> = systems.iter().map(|s| format!("\"{s}\": 2")).collect();
    format!(
        "{{\"BM_fit_data\": {{{}}}, \"num_atoms_in_sim_cell\": {{{}}}}}",
        fits.join(", "),
        atoms.join(", ")
    )
}

pub(crate) fn tuning(name: &str, systems: &[&str]) -> (crate::Tree, std::path::PathBuf) {
    let case = CASE.replace("\"push\"", "\"node\"");
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", &case),
            ("benchmarks/1_a/crystals.json", &tuning_crystals(systems)),
            (
                "benchmarks/1_a/table.toml",
                "[element]\nSi = \"jth-2.0\"\nAl = \"jth-2.0\"\n",
            ),
            (
                "benchmarks/1_a/reference/acwf-ae-average.json",
                &results(systems, &[]),
            ),
            (
                "benchmarks/1_a/reference/acwf-vasp.json",
                &results(systems, &["Al-X/FCC"]),
            ),
            ("benchmarks/ACWF.toml", &repository("benchmarks/ACWF.toml")),
            ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
            ("remote/JTH-PBE-v2.0/Al/Al.xml", "al"),
        ],
    );
    let url = format!("file://{}/remote", root.display());
    let rows = [
        ("pbe", "Si", "si", Some(10.0)),
        ("pbe", "Al", "al", Some(12.0)),
    ];
    write(&root, "datasets/jth-2.0.toml", &super::jth(&url, &rows));
    let bin = engine(&root, super::MINUS_ONE);
    (root, bin)
}

#[test]
fn a_tuning_case_with_an_exam_crystal_is_a_harness_fault_that_names_it() {
    let (root, bin) = tuning("tune-exam-crystal", &["Si-X/FCC", "Si-X/Diamond"]);
    let session = Session::new(&root, &bin, Tier::Node, &root.join("records"));
    let outcome = session.evaluate("1_a", super::minus_one);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("Si-X/Diamond in the ACWF exam part")),
        "{outcome:?}"
    );
}

#[test]
fn a_tuning_filter_that_names_an_exam_system_fails_by_name() {
    let (root, bin) = tuning("tune-exam-filter", &["Si-X/FCC"]);
    let session = Session::new(&root, &bin, Tier::Node, &root.join("records"))
        .with_filter(vec!["Si-X/FCC".into(), "Si-X/SC@1.00".into()]);
    let outcome = session.evaluate("1_a", super::minus_one);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("Si-X/SC in the ACWF exam part")),
        "{outcome:?}"
    );
}

#[test]
fn a_tuning_case_runs_each_crystal_at_its_table_hint_plus_20_ha() {
    let (root, bin) = tuning("tune-hint", &["Si-X/FCC", "Al-X/FCC"]);
    let session =
        Session::new(&root, &bin, Tier::Node, &root.join("records")).with_filter(vec!["Al".into()]);
    assert_eq!(session.evaluate("1_a", cutoff_32), Outcome::Shard);
}

fn cutoff_error(evidence: &Evidence, ha: f64) -> Result<Metrics, String> {
    let mut error: f64 = 0.0;
    for name in evidence.names() {
        error = error.max((evidence.input(name)?.cutoff_ha()? - ha).abs());
        error = error.max((evidence.output(name)?.energy_ha("free_energy_ha")? + 1.0).abs());
    }
    Ok(Metrics::from([("error".to_owned(), error)]))
}

fn cutoff_32(evidence: &Evidence) -> Result<Metrics, String> {
    cutoff_error(evidence, 32.0)
}

fn cutoff_30(evidence: &Evidence) -> Result<Metrics, String> {
    cutoff_error(evidence, 30.0)
}

#[test]
fn a_jth_2_0_element_without_a_hint_takes_the_highest_jth_hint() {
    let (root, bin) = tuning("tune-hintless", &["Si-X/FCC", "Al-X/FCC"]);
    let table = std::fs::read_to_string(root.join("datasets/jth-2.0.toml")).unwrap();
    write(
        &root,
        "datasets/jth-2.0.toml",
        &table.replace(", cutoff_hint_ha = 12.0", ""),
    );
    let session =
        Session::new(&root, &bin, Tier::Node, &root.join("records")).with_filter(vec!["Al".into()]);
    assert_eq!(session.evaluate("1_a", cutoff_30), Outcome::Shard);
}

#[test]
fn a_tuning_crystal_of_an_element_outside_the_table_fails_by_name() {
    let (root, bin) = tuning("tune-outside", &["Si-X/FCC", "Al-X/FCC"]);
    write(
        &root,
        "benchmarks/1_a/table.toml",
        "[element]\nSi = \"jth-2.0\"\n",
    );
    let outcome =
        Session::new(&root, &bin, Tier::Node, &root.join("records")).evaluate("1_a", cutoff_30);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("Al-X/FCC: the dataset table has no Al")),
        "{outcome:?}"
    );
}

fn record(inputs: &[&str], metrics: &str, outcome: &str) -> String {
    let inputs: Vec<String> = (inputs.iter())
        .flat_map(|name| match name.contains('@') {
            true => vec![(*name).to_owned()],
            false => acwf::SCALES.map(|s| format!("{name}@{s:.2}")).to_vec(),
        })
        .collect();
    format!(
        "{{\"case\": \"1_a\", \"commit\": \"c\", \"engine_sha256\": \"e\", \"table\": null, \"outcome\": \"{outcome}\", \
         \"failures\": [\"why\"], \"inputs\": {inputs:?}, \"metrics\": {{{metrics}}}}}"
    )
}

const THREE: [&str; 3] = ["Si-X/FCC", "Si-X/BCC", "Al-X/FCC"];

fn readout(name: &str, selectors: &[String], baseline: bool) -> Result<String, String> {
    let (root, _) = tuning(name, &THREE);
    let si = "\"epsilon/Si-X/FCC\": 0.06, \"nu/Si-X/FCC\": 0.2, \"epsilon/Si-X/BCC\": 0.5, \"nu/Si-X/BCC\": 0.3";
    let ran = record(&["Si-X/FCC", "Si-X/BCC"], si, "planned");
    write(&root, "records/a.json", &ran);
    let failed = record(&["Al-X/FCC"], "", "fail");
    write(&root, "records/b.json", &failed);
    write(&root, "records/other.json", "{\"case\": \"2_b\"}");
    let base = "\"epsilon/Si-X/FCC\": 1.0, \"nu/Si-X/FCC\": 0.2";
    write(
        &root,
        "baseline/a.json",
        &record(&["Si-X/FCC"], base, "shard"),
    );
    let baseline = baseline.then(|| root.join("baseline"));
    let records = root.join("records");
    acwf::readout(&root, "1_a", &records, baseline.as_deref(), selectors)
}

#[test]
fn the_epsilon_readout_counts_a_lost_crystal_as_infinite_per_element_and_for_the_set() {
    let text = readout("tune-readout", &[], true).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert_eq!(lines[0].split(';').next(), Some("epsilon: 1_a, 3 crystals"));
    let none = "at table none";
    assert!(
        lines[0].ends_with(none) && lines[1].ends_with(none),
        "{text}"
    );
    assert_eq!(lines[3], "Al-X/FCC  inf  NaN  inf  inf");
    assert!(
        lines[4].starts_with("  lost Al-X/FCC: ")
            && lines[4].ends_with("holds no ε (outcome \"fail\": why)")
    );
    assert_eq!(lines[5], "Si-X/BCC  0.5000  0.3000  0.0000  inf");
    assert_eq!(lines[6], "Si-X/FCC  0.0600  0.2000  0.0000  1.0000");
    let tail: Vec<&str> = lines.iter().rev().take(3).rev().copied().collect();
    assert_eq!(
        tail,
        [
            "Al (1): orbitar median inf share 0.00 above_1 1 · vasp median inf share 0.00 above_1 1 · baseline median inf share 0.00 above_1 1",
            "Si (2): orbitar median 0.2800 share 0.50 above_1 0 · vasp median 0.0000 share 1.00 above_1 0 · baseline median inf share 0.00 above_1 1",
            "set (3): orbitar median 0.5000 share 0.33 above_1 1 · vasp median 0.0000 share 0.67 above_1 1 · baseline median inf share 0.00 above_1 2",
        ]
    );
}

#[test]
fn the_epsilon_readout_takes_the_crystals_that_its_selectors_pick() {
    let text = readout("tune-readout-pick", &["Si-X/FCC".into()], false).unwrap();
    let set = "set (1): orbitar median 0.0600 share 1.00 above_1 0 · vasp median 0.0000 share 1.00 above_1 0";
    assert_eq!(text.lines().last(), Some(set));
}

#[test]
fn the_epsilon_readout_refuses_an_exam_selector_case_or_record_before_any_number() {
    let error = readout("tune-readout-exam", &["Si-X/Diamond".into()], true).unwrap_err();
    assert!(
        error.contains("Si-X/Diamond in the ACWF exam part"),
        "{error}"
    );
    let (root, _) = tuning("tune-readout-record", &THREE);
    write(
        &root,
        "records/a.json",
        &record(&["Si-XO2@1.00"], "", "fail"),
    );
    let error = acwf::readout(&root, "1_a", &root.join("records"), None, &[]).unwrap_err();
    assert!(
        error.contains("a.json: Si-XO2 in the ACWF exam part"),
        "{error}"
    );
    let repo = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/.."));
    let error =
        acwf::readout(repo, "10_acwf_subset", &root.join("records"), None, &[]).unwrap_err();
    assert!(error.contains("Ag-X/Diamond, Ag-X2O3"), "{error}");
}

#[test]
fn the_epsilon_readout_refuses_records_of_two_commits_or_one_crystal_twice() {
    let (root, _) = tuning("tune-readout-two", &THREE);
    write(
        &root,
        "records/a.json",
        &record(&["Si-X/FCC@1.00"], "", "fail"),
    );
    write(
        &root,
        "records/b.json",
        &record(&["Si-X/FCC@0.94"], "", "fail"),
    );
    let error = acwf::readout(&root, "1_a", &root.join("records"), None, &[]).unwrap_err();
    assert!(
        error.contains("Si-X/FCC runs in more than one record"),
        "{error}"
    );
    let other = record(&["Al-X/FCC@1.00"], "", "fail").replace("\"c\"", "\"d\"");
    write(&root, "records/b.json", &other);
    let error = acwf::readout(&root, "1_a", &root.join("records"), None, &[]).unwrap_err();
    assert!(
        error.contains("differ in commit, engine or dataset table"),
        "{error}"
    );
}

#[test]
fn cargo_xtask_epsilon_reads_the_shards_beside_a_baseline_and_names_its_refusal() {
    let (root, _) = tuning("tune-lever", &THREE);
    let al = record(
        &["Al-X/FCC"],
        "\"epsilon/Al-X/FCC\": 0.01, \"nu/Al-X/FCC\": 0.1",
        "shard",
    );
    write(&root, "shards/a.json", &al);
    write(&root, "jth/a.json", &al);
    let (shards, jth) = (root.join("shards"), root.join("jth"));
    let (shards, jth) = (shards.to_str().unwrap(), jth.to_str().unwrap());
    let args = ["epsilon", "1_a", shards, "--baseline", jth, "Al"];
    assert_eq!(crate::lever(&args, &root), Vec::<String>::new());
    let exam = ["epsilon", "1_a", shards, "Al-X/SC"];
    assert_eq!(
        crate::lever(&exam, &root),
        [
            "epsilon: Al-X/SC in the ACWF exam part, blind until M5 judges it; the tuning tooling refuses it"
        ]
    );
}

#[test]
fn the_epsilon_readout_counts_a_crystal_of_fewer_than_seven_volumes_as_lost() {
    let (root, _) = tuning("tune-readout-partial", &THREE);
    let four = [
        "Al-X/FCC@0.94",
        "Al-X/FCC@0.96",
        "Al-X/FCC@0.98",
        "Al-X/FCC@1.00",
    ];
    let metrics = "\"epsilon/Al-X/FCC\": 0.01, \"nu/Al-X/FCC\": 0.1";
    write(&root, "records/a.json", &record(&four, metrics, "shard"));
    let text = acwf::readout(&root, "1_a", &root.join("records"), None, &["Al".into()]).unwrap();
    assert!(
        text.contains("a.json holds 4 of 7 volumes of Al-X/FCC"),
        "{text}"
    );
    assert!(text.contains("Al-X/FCC  inf  NaN"), "{text}");
}

#[test]
fn the_epsilon_readout_names_the_table_of_its_records_and_whether_it_is_the_case_s_own() {
    let (root, _) = tuning("tune-readout-table", &THREE);
    let own = crate::harness::hex(&Sha256::digest(
        "[element]\nSi = \"jth-2.0\"\nAl = \"jth-2.0\"\n",
    ));
    let at = |sha: &str| {
        record(&["Al-X/FCC"], "", "fail").replace(
            "\"table\": null",
            &format!("\"table\": {{\"sha256\": \"{sha}\"}}"),
        )
    };
    write(&root, "records/a.json", &at(&own));
    write(&root, "baseline/a.json", &at("0"));
    let baseline = root.join("baseline");
    let text = acwf::readout(&root, "1_a", &root.join("records"), Some(&baseline), &[]).unwrap();
    let lines: Vec<&str> = text.lines().collect();
    assert!(
        lines[0].ends_with(&format!("at table \"{own}\" (the case's own)")),
        "{text}"
    );
    assert!(
        lines[1].ends_with("at table \"0\" (a candidate, not the case's own)"),
        "{text}"
    );
}

#[test]
fn the_epsilon_readout_names_an_unknown_selector_as_its_own() {
    let error = readout("tune-readout-unknown", &["Ge".into()], false).unwrap_err();
    assert_eq!(error, "the selector list names no input of this case: Ge");
}

#[test]
fn a_hand_written_exam_input_of_a_tuning_case_is_a_harness_fault_that_names_it() {
    let (root, bin) = tuning("tune-exam-input", &["Si-X/FCC"]);
    write(&root, "benchmarks/1_a/inputs/Si-XO2.toml", super::INPUT);
    let outcome = Session::new(&root, &bin, Tier::Node, &root.join("records"))
        .evaluate("1_a", super::minus_one);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("Si-XO2 in the ACWF exam part")),
        "{outcome:?}"
    );
}
