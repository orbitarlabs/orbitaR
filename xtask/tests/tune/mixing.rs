use super::rivals;
use crate::harness::rival::{ABINIT, INPUT, PW, RIVALS, TUNED, rivals_tree, three_atoms};
use crate::{Tree, has};

const MIXING_CELLS: &str = "mixing_cells = [\"benchmarks/1_a/input.toml\"]\nbudget_s = 1000.0";
const STUCK_PW: &str = "grep -q 'mixing_beta = 0.3' pw.in || echo '     convergence NOT achieved after 100 iterations: stopping'";

fn mixing_tree(name: &str, pw: &str, abinit: &str) -> Tree {
    let root = rivals_tree(name, INPUT, TUNED, pw, abinit);
    let settings = RIVALS.replace("budget_s = 1000.0", MIXING_CELLS);
    crate::write(&root, "benchmarks/RIVALS.toml", &settings);
    root
}

#[test]
fn mixing_freezes_the_fastest_candidate_that_converges_with_its_reason() {
    let pw = format!("t=0.3; grep -q '^  mixing_beta = 0.3$' pw.in && t=0.02\n{PW}");
    let abinit = format!("t=0.3; grep -qx 'diemix 0.3' run.abi && t=0.02\n{ABINIT}");
    let root = mixing_tree("tune-mixing", &pw, &abinit);
    let (findings, tuned) = rivals(&root, "mixing");
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["mixing"]["input"],
        toml::Value::try_from(["mixing_beta = 0.3"]).unwrap(),
        "{tuned}"
    );
    let mixing = &tuned["abinit"]["mixing"];
    assert_eq!(
        mixing["input"],
        toml::Value::try_from(["diemac 10.0", "diemix 0.3"]).unwrap(),
        "{tuned}"
    );
    let reason = mixing["reason"].as_str().unwrap();
    assert!(
        reason.contains("on the defaults, at their state"),
        "{reason}"
    );
    let searched = mixing["searched"].as_array().unwrap();
    assert_eq!(searched.len(), 2, "{tuned}");
    assert!(
        searched[0].as_str().unwrap().starts_with("[]: "),
        "{searched:?}"
    );
}

#[test]
fn the_mixing_cells_join_the_tuning_set() {
    let root = mixing_tree("tune-mixing-set", PW, ABINIT);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "budget_s = 1000.0",
            "mixing_cells = [\"benchmarks/1_a/inputs/b.toml\"]\nbudget_s = 1000.0",
        ),
    );
    crate::write(&root, "benchmarks/1_a/inputs/b.toml", INPUT);
    let (findings, tuned) = rivals(&root, "mixing");
    assert!(findings.is_empty(), "{findings:?}");
    assert!(
        (tuned["tuning_set"].as_array().unwrap().iter())
            .any(|c| c.as_str() == Some("benchmarks/1_a/inputs/b.toml")),
        "{tuned}"
    );
}

#[test]
fn the_layout_search_runs_on_the_frozen_mixing_and_keeps_it() {
    let pw = format!("{STUCK_PW}\n{PW}");
    let tuned = format!("{TUNED}\n[qe.mixing]\ninput = [\"mixing_beta = 0.3\"]\n");
    let root = rivals_tree("tune-mixing-layout", INPUT, &tuned, &pw, ABINIT);
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["mixing"]["input"],
        toml::Value::try_from(["mixing_beta = 0.3"]).unwrap(),
        "{tuned}"
    );
}

fn damped_at(name: &str, e: &str) -> toml::Value {
    let abinit = format!("t=0.3; grep -qx 'diemix 0.3' run.abi && {{ t=0.02; e={e}; }}\n{ABINIT}");
    let root = rivals_tree(name, &three_atoms(INPUT), TUNED, PW, &abinit);
    let settings = RIVALS.replace("budget_s = 1000.0", MIXING_CELLS);
    crate::write(&root, "benchmarks/RIVALS.toml", &settings);
    let (findings, tuned) = rivals(&root, "mixing");
    assert!(findings.is_empty(), "{findings:?}");
    tuned["abinit"]["mixing"].clone()
}

#[test]
fn a_faster_candidate_that_reaches_another_state_than_the_defaults_loses() {
    let mixing = damped_at("tune-mixing-state", "-2.5");
    assert_eq!(mixing["input"], toml::Value::Array(Vec::new()), "{mixing}");
    let searched = mixing["searched"].as_array().unwrap();
    assert!(
        searched[1].as_str().unwrap().contains(
            "failed: benchmarks/1_a/input.toml: another state than the defaults': 1.6666666666666666e-1 Ha per atom off"
        ),
        "{searched:?}"
    );
}

#[test]
fn a_candidate_within_the_target_per_atom_of_the_defaults_is_their_state() {
    let mixing = damped_at("tune-mixing-same", "-2.00006");
    assert_eq!(
        mixing["input"],
        toml::Value::try_from(["diemac 10.0", "diemix 0.3"]).unwrap(),
        "{mixing}"
    );
}

#[test]
fn the_defaults_stay_where_no_candidate_beats_them() {
    let abinit = format!("t=0.02; grep -qx 'diemix 0.3' run.abi && t=0.3\n{ABINIT}");
    let root = mixing_tree("tune-mixing-defaults", PW, &abinit);
    let (findings, tuned) = rivals(&root, "mixing");
    assert!(findings.is_empty(), "{findings:?}");
    let mixing = &tuned["abinit"]["mixing"];
    assert_eq!(mixing["input"], toml::Value::Array(Vec::new()), "{tuned}");
    let reason = mixing["reason"].as_str().unwrap();
    assert!(reason.starts_with("the defaults were fastest"), "{reason}");
}

#[test]
fn mixing_without_mixing_cells_freezes_nothing() {
    let root = rivals_tree("tune-mixing-none", INPUT, TUNED, PW, ABINIT);
    let (findings, tuned) = rivals(&root, "mixing");
    assert!(
        has(&findings, "rivals: qe: no mixing cells"),
        "{findings:?}"
    );
    assert_eq!(tuned, toml::from_str::<toml::Table>(TUNED).unwrap());
}

fn two_cells(name: &str, stuck: &str, e: &str) -> (Vec<String>, toml::Table) {
    let abinit = format!(
        "c=a; grep -q ' 0.3$' run.abi && c=b\n\
         if grep -qx 'diemix 0.3' run.abi; then t=0.02; e={e}; else t=0.3; case '{stuck}' in *$c*) echo ' There was not enough SCF cycles to converge';; esac; fi\n{ABINIT}"
    );
    let root = rivals_tree(name, INPUT, TUNED, PW, &abinit);
    let cells = "mixing_cells = [\"benchmarks/1_a/input.toml\", \"benchmarks/1_a/inputs/b.toml\"]\nbudget_s = 1000.0";
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace("budget_s = 1000.0", cells),
    );
    crate::write(
        &root,
        "benchmarks/1_a/inputs/b.toml",
        &INPUT.replace("[[0.0, 0.0, 0.25]]", "[[0.0, 0.0, 0.3]]"),
    );
    rivals(&root, "mixing")
}

#[test]
fn where_the_defaults_fail_on_one_cell_a_candidate_must_hold_their_state_on_the_others() {
    let (findings, tuned) = two_cells("tune-mixing-half-off", "a", "-2.5");
    assert!(
        has(
            &findings,
            "rivals: abinit: every mixing failed; the defaults: []: failed: benchmarks/1_a/input.toml:"
        ),
        "{findings:?}"
    );
    assert!(tuned["abinit"].get("mixing").is_none(), "{tuned}");
}

#[test]
fn where_the_defaults_fail_on_one_cell_a_candidate_at_their_state_on_the_others_freezes() {
    let (findings, tuned) = two_cells("tune-mixing-half-on", "a", "-2.0");
    assert!(findings.is_empty(), "{findings:?}");
    let mixing = &tuned["abinit"]["mixing"];
    assert_eq!(
        mixing["input"],
        toml::Value::try_from(["diemac 10.0", "diemix 0.3"]).unwrap(),
        "{tuned}"
    );
    let reason = mixing["reason"].as_str().unwrap();
    assert!(
        reason.contains(
            "at the defaults' state where they converged, unchecked where they failed: benchmarks/1_a/input.toml:"
        ),
        "{reason}"
    );
}

#[test]
fn where_the_defaults_converge_on_no_cell_nothing_freezes() {
    let (findings, tuned) = two_cells("tune-mixing-none-on", "ab", "-2.0");
    assert!(
        has(
            &findings,
            "rivals: abinit: the defaults converged on no mixing cell, so no state can check a candidate"
        ),
        "{findings:?}"
    );
    assert!(tuned["abinit"].get("mixing").is_none(), "{tuned}");
    assert!(tuned["qe"].get("mixing").is_some(), "{tuned}");
}
