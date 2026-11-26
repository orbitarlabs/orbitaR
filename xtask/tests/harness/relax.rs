use crate::tree;
use serde_json::json;
use std::{os::unix::fs::PermissionsExt, path::PathBuf};
use xtask::harness::{Evidence, Metrics, Outcome, Session, Tier, relax};

const CASE: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\ngap_max = 1e-12\n\n[relax]\ninput = \"ions\"\n";
const INPUT: &str = "schema = 1\n\n[structure]\ncell_bohr = [[14.0, 0.0, 0.0], [0.0, 14.0, 0.0], [0.0, 0.0, 14.0]]\nspecies = [\"Si\", \"Si\"]\npositions_fractional = [[0.1, 0.1, 0.1], [0.45, 0.11, 0.09]]\n\n[basis]\ncutoff_ha = 1.0\n";

const FAKE: &str = r#"
import glob, hashlib, json, os, sys, tomllib
import numpy as np
from ase import Atoms
from ase.calculators.lj import LennardJones
from ase.calculators.socketio import SocketClient
from ase.units import Bohr, Hartree

input_path = sys.argv[-1]
with open(input_path, "rb") as f:
    s = tomllib.load(f)["structure"]
atoms = Atoms(s["species"], cell=np.array(s["cell_bohr"]) * Bohr, scaled_positions=s["positions_fractional"], pbc=True)
atoms.calc = LennardJones(sigma=2.0, epsilon=0.1, rc=4.5)

def files():
    """The file of each species that orbitaR would name: the first placed file in the element's directory."""
    root = os.path.join(os.path.dirname(os.path.abspath(input_path)), "datasets")
    named = {}
    for element in dict.fromkeys(s["species"]):
        for path in sorted(glob.glob(os.path.join(root, "*", element, "*")))[:1]:
            with open(path, "rb") as f:
                named[element] = {"path": os.path.relpath(path, root), "sha256": hashlib.sha256(f.read()).hexdigest()}
    return named

def outcome(shift):
    return json.dumps({
        "datasets": files(),
        "free_energy_ha": atoms.get_potential_energy() / Hartree + shift,
        "forces_ha_bohr": (atoms.get_forces() / (Hartree / Bohr)).tolist(),
        "stress_ha_bohr3": (atoms.get_stress(voigt=False) / (Hartree / Bohr**3)).tolist(),
    })

if sys.argv[1] == "run":
    print(outcome(0.0))
else:
    client = SocketClient(unixsocket=sys.argv[3].removeprefix("/tmp/ipi_"))
    for _ in client.irun(atoms, use_stress=True):
        print(outcome(IPI_SHIFT), flush=True)
"#;

fn python() -> PathBuf {
    std::env::split_paths(&std::env::var_os("PATH").unwrap())
        .map(|dir| dir.join("python3"))
        .find(|p| p.is_file())
        .expect("python3 with ASE 3.29.0 on PATH; CI's check job installs it")
}

fn relaxed(
    name: &str,
    case: &str,
    shift: &str,
    measure: fn(&Evidence) -> Result<Metrics, String>,
) -> (Outcome, serde_json::Value) {
    relaxed_with(name, case, &[], shift, measure)
}

fn relaxed_with(
    name: &str,
    case: &str,
    more: &[(&str, &str)],
    shift: &str,
    measure: fn(&Evidence) -> Result<Metrics, String>,
) -> (Outcome, serde_json::Value) {
    let paths: Vec<String> = more
        .iter()
        .map(|(p, _)| format!("benchmarks/1_a/{p}"))
        .collect();
    let mut files = vec![
        ("benchmarks/1_a/case.toml", case),
        ("benchmarks/1_a/input.toml", INPUT),
        ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
    ];
    files.extend(
        paths
            .iter()
            .zip(more)
            .map(|(p, (_, body))| (p.as_str(), *body)),
    );
    let root = tree(name, &files);
    let url = format!("file://{}/remote", root.display());
    let table = super::jth(&url, &[("pbe", "Si", "si", None)]);
    crate::write(&root, "datasets/jth-2.0.toml", &table);
    let bin = root.join("fake-engine");
    let body = FAKE.replace("IPI_SHIFT", shift);
    std::fs::write(&bin, format!("#!{}\n{body}", python().display())).unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755)).unwrap();
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    let outcome = session.evaluate("1_a", measure);
    let record = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&record).unwrap())
}

fn gap(evidence: &Evidence) -> Result<Metrics, String> {
    let cold = evidence.output("input")?.energy_ha("free_energy_ha")?;
    let warm = evidence.warm("input")?.energy_ha("free_energy_ha")?;
    Ok(Metrics::from([("gap".to_owned(), (cold - warm).abs())]))
}

#[test]
fn a_relaxed_input_runs_cold_at_the_endpoint_beside_its_warm_outcome() {
    fn moved(evidence: &Evidence) -> Result<Metrics, String> {
        let at = evidence.input("input")?.positions_fractional()?;
        let apart = (0..3)
            .map(|k| ((at[1][k] - at[0][k]) * 14.0).powi(2))
            .sum::<f64>()
            .sqrt();
        let forces = evidence.output("input")?.forces_ha_bohr()?;
        let largest = forces.iter().flatten().fold(0.0_f64, |m, f| m.max(f.abs()));
        if (apart - 4.243).abs() > 0.02 || largest > 0.01 / 51.422 {
            return Err(format!(
                "not relaxed: {apart} Bohr apart, max |f| {largest}"
            ));
        }
        gap(evidence)
    }
    let (outcome, record) = relaxed("relax-pass", CASE, "0.0", moved);
    assert_eq!(outcome, Outcome::Pass);
    let run = &record["relaxations"][0];
    assert_eq!(
        (&run["input"], &run["kind"]),
        (&json!("input"), &json!("ions"))
    );
    let (steps, geometries) = (
        run["steps"].as_u64().unwrap(),
        run["geometries"].as_u64().unwrap(),
    );
    assert!(steps > 2 && geometries == steps + 1, "{run}");
    assert!(
        record["spent_s"].as_f64() > run["seconds"].as_f64(),
        "{record}"
    );
}

#[test]
fn a_warm_outcome_off_the_cold_one_fails_the_case() {
    let (outcome, _) = relaxed("relax-shift", CASE, "1e-9", gap);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("gap")),
        "{outcome:?}"
    );
}

#[test]
fn a_cell_relaxation_moves_the_cell() {
    fn grown(evidence: &Evidence) -> Result<Metrics, String> {
        let cell = evidence.input("input")?.cell_bohr()?;
        if cell[0][0] == 14.0 {
            return Err("the cell did not move".into());
        }
        gap(evidence)
    }
    let case = CASE.replace("\"ions\"", "\"cell\"");
    let (outcome, record) = relaxed("relax-cell", &case, "0.0", grown);
    assert_eq!(outcome, Outcome::Pass);
    assert_eq!(record["relaxations"][0]["kind"], json!("cell"));
}

#[test]
fn an_engine_that_dies_in_the_relaxation_fails_the_input() {
    let (outcome, _) = relaxed("relax-dies", CASE, "1 / 0", gap);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.starts_with("input: relax (ions): engine exited")),
        "{outcome:?}"
    );
}

#[test]
fn each_relaxed_input_keeps_its_own_warm_outcome() {
    fn each(evidence: &Evidence) -> Result<Metrics, String> {
        let mut worst: f64 = 0.0;
        for name in evidence.names() {
            let cold = evidence.output(name)?.energy_ha("free_energy_ha")?;
            worst = worst.max((cold - evidence.warm(name)?.energy_ha("free_energy_ha")?).abs());
        }
        Ok(Metrics::from([("gap".to_owned(), worst)]))
    }
    let three = INPUT
        .replace("[\"Si\", \"Si\"]", "[\"Si\", \"Si\", \"Si\"]")
        .replace(
            "[0.45, 0.11, 0.09]]",
            "[0.45, 0.11, 0.09], [0.27, 0.42, 0.1]]",
        );
    let case = CASE.replace("input = \"ions\"\n", "input = \"ions\"\nb = \"ions\"\n");
    let (outcome, _) = relaxed_with(
        "relax-two",
        &case,
        &[("inputs/b.toml", &three)],
        "0.0",
        each,
    );
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn a_relax_entry_must_name_an_input() {
    let case = CASE.replace("input = \"ions\"", "other = \"ions\"");
    let (outcome, _) = relaxed("relax-name", &case, "0.0", gap);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("[relax] names other, which is no input")),
        "{outcome:?}"
    );
}

#[test]
fn only_a_relaxed_input_has_a_warm_outcome() {
    let case = CASE.replace("\n[relax]\ninput = \"ions\"\n", "");
    let (outcome, _) = relaxed("relax-none", &case, "0.0", gap);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("no warm outcome for input: name it in case.toml [relax]")),
        "{outcome:?}"
    );
}

#[test]
fn the_endpoint_takes_the_last_geometry_and_asks_for_forces_and_stress() {
    let input: toml::Table = toml::from_str(INPUT).unwrap();
    let printed = json!({
        "cell_bohr": [[9.0, 0.0, 0.0], [0.0, 9.5, 0.0], [0.0, 0.1, 10.0]],
        "positions_fractional": [[0.0, 0.0, 0.0], [0.4, 0.1, 0.1]],
    });
    let end = relax::endpoint(&input, &printed).unwrap();
    assert_eq!(end["structure"]["cell_bohr"][2][1].as_float(), Some(0.1));
    assert_eq!(
        end["structure"]["positions_fractional"][1][0].as_float(),
        Some(0.4)
    );
    assert_eq!(end["structure"]["species"], input["structure"]["species"]);
    assert_eq!(end["basis"], input["basis"]);
    for key in ["forces", "stress"] {
        assert_eq!(end["outputs"][key].as_bool(), Some(true), "{key}");
    }
    for bad in [
        json!({"cell_bohr": [[9.0, 0.0], [0.0, 9.5, 0.0], [0.0, 0.0, 10.0]], "positions_fractional": []}),
        json!({"cell_bohr": [[9.0, 0.0, "x"]], "positions_fractional": []}),
        json!({"positions_fractional": []}),
    ] {
        assert_eq!(
            relax::endpoint(&input, &bad).unwrap_err(),
            "relax: cell_bohr must be rows of 3 numbers"
        );
    }
}

#[test]
fn fmax_is_a_hundredth_of_an_ev_per_angstrom_in_hartree_per_bohr() {
    assert!((relax::FMAX_HA_BOHR - 1.944_690_38e-4).abs() < 1e-12);
}
