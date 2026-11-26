use crate::harness::rival::{ABINIT, INPUT, PW, RIVALS, TUNED, rivals_tree};
use crate::{Tree, has};
use xtask::Env;

mod calibrate;
mod layout;
mod mixing;

pub(crate) fn rivals(root: &Tree, job: &str) -> (Vec<String>, toml::Table) {
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", job], root, &env);
    let text = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    (findings, text.parse().unwrap())
}

#[test]
fn rivals_need_their_installs_and_a_known_job() {
    let root = rivals_tree("tune-unset", INPUT, TUNED, PW, ABINIT);
    assert!(has(
        &crate::lever(&["rivals", "layout"], &root),
        "rivals: set ORBITAR_RIVALS"
    ));
    let (findings, _) = rivals(&root, "tune");
    assert!(
        has(
            &findings,
            "error: invalid value 'tune' for '<JOB>' (one of calibrate, mixing, layout)"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_tuned_file_that_cannot_be_read_is_a_finding_not_an_empty_one() {
    let root = rivals_tree("tune-unreadable", INPUT, "", PW, ABINIT);
    std::fs::remove_file(root.join("benchmarks/TUNED.toml")).unwrap();
    std::fs::create_dir(root.join("benchmarks/TUNED.toml")).unwrap();
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", "layout"], &root, &env);
    assert!(
        has(&findings, "rivals: benchmarks/TUNED.toml: Is a directory"),
        "{findings:?}"
    );
}

#[test]
fn calibrate_tunes_only_the_codes_named_and_keeps_the_others() {
    let pw = format!("{PW}\necho 'total   stress  (kbar)  P=  1'");
    let abinit = format!("{ABINIT}\necho ' Pressure=  1.0 GPa'");
    let root = rivals_tree("tune-one", INPUT, TUNED, &pw, &abinit);
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    assert!(xtask::run(&["rivals", "calibrate", "abinit"], &root, &env).is_empty());
    let tuned: toml::Table = std::fs::read_to_string(root.join("benchmarks/TUNED.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        tuned["abinit"]["cutoff_ha"]["Si"].as_float(),
        Some(6.0),
        "{tuned}"
    );
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Al"].as_float(),
        Some(10.0),
        "{tuned}"
    );
    assert!(
        tuned["qe"]
            .get("calibration")
            .is_none_or(|c| c.as_table().is_some_and(toml::Table::is_empty)),
        "{tuned}"
    );
    let f = xtask::run(&["rivals", "calibrate", "siesta"], &root, &env);
    assert!(
        has(
            &f,
            "error: invalid value 'siesta' for '[CODES]...' (one of qe, abinit)"
        ),
        "{f:?}"
    );
}

#[test]
fn calibrate_keeps_the_section_of_a_code_that_succeeded_when_another_fails() {
    let pw = format!("{PW}\necho 'total   stress  (kbar)  P=  1'");
    let crash = "echo 'Error in pawpsp_17in' >&2\nexit 1";
    let root = rivals_tree("tune-half", INPUT, "", &pw, crash);
    let (findings, tuned) = rivals(&root, "calibrate");
    assert!(
        has(
            &findings,
            "rivals: abinit: engine exited with exit status: 1: Error in pawpsp_17in"
        ),
        "{findings:?}"
    );
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
    assert!(tuned.get("abinit").is_none(), "{tuned}");
}

#[test]
fn a_code_neither_calibrates_nor_lays_out_a_cell_that_it_skips() {
    let pw =
        format!("grep -q '^  Al ' pw.in && exit 7\n{PW}\necho 'total   stress  (kbar)  P=  1'");
    let abinit = format!("t=0.01\n{ABINIT}\necho ' Pressure=  1.0 GPa'");
    let root = rivals_tree("tune-skip", INPUT, TUNED, &format!("t=0.01\n{pw}"), &abinit);
    let alsi = INPUT.replace("[\"Si\"]", "[\"Al\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    crate::write(&root, "benchmarks/1_a/inputs/alsi.toml", &alsi);
    let cells = "[\"benchmarks/1_a/input.toml\", \"benchmarks/1_a/inputs/alsi.toml\"]";
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS
            .replace(
                "calibration_cells = [\"benchmarks/1_a/input.toml\"]",
                &format!("calibration_cells = {cells}"),
            )
            .replace(
                "layout_cells = [\"benchmarks/1_a/input.toml\"]",
                &format!("layout_cells = {cells}"),
            )
            .replace(
                "density_ratio = 8.0\n",
                "density_ratio = 8.0\nskip_cells = [{ cell = \"benchmarks/1_a/inputs/alsi.toml\", reason = \"no dataset\" }]\n",
            ),
    );
    let (findings, tuned) = rivals(&root, "calibrate");
    assert!(findings.is_empty(), "{findings:?}");
    let keys = |code: &str, table: &str| -> Vec<String> {
        tuned[code][table]
            .as_table()
            .unwrap()
            .keys()
            .cloned()
            .collect()
    };
    assert_eq!(keys("qe", "cutoff_ha"), ["Si"], "{tuned}");
    assert_eq!(
        keys("qe", "calibration"),
        ["benchmarks/1_a/input.toml"],
        "{tuned}"
    );
    assert_eq!(keys("abinit", "cutoff_ha"), ["Al", "Si"], "{tuned}");
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    let lines = |code: &str| {
        tuned[code]["searched"]["8"].as_array().unwrap()[0]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert!(!lines("qe").contains("alsi.toml"), "{tuned}");
    assert!(lines("abinit").contains("alsi.toml"), "{tuned}");
}

#[test]
fn calibrate_reads_the_calibration_cells_and_layout_the_layout_cells() {
    let pw = format!("{PW}\necho 'total   stress  (kbar)  P=  1'");
    let abinit = format!("{ABINIT}\necho ' Pressure=  1.0 GPa'");
    let real = "[\"benchmarks/1_a/input.toml\"]";
    let none = "[\"benchmarks/1_a/none.toml\"]";
    for (job, read, other) in [
        ("calibrate", "calibration_cells", "layout_cells"),
        ("layout", "layout_cells", "calibration_cells"),
    ] {
        let root = rivals_tree(&format!("tune-cells-{job}"), INPUT, TUNED, &pw, &abinit);
        let only =
            |list: &str| RIVALS.replace(&format!("{list} = {real}"), &format!("{list} = {none}"));
        crate::write(&root, "benchmarks/RIVALS.toml", &only(other));
        let (findings, _) = rivals(&root, job);
        assert!(findings.is_empty(), "{job}: {findings:?}");
        crate::write(&root, "benchmarks/RIVALS.toml", &only(read));
        let (findings, _) = rivals(&root, job);
        assert!(
            has(&findings, "benchmarks/1_a/none.toml"),
            "{job}: {findings:?}"
        );
    }
}

#[test]
fn rivals_refuse_a_ladder_that_does_not_ascend_below_the_converged_cutoff() {
    for (from, to) in [
        ("[10.0, 12.5, 15.0", "[12.5, 10.0, 15.0"),
        ("[10.0, 12.5, 15.0", "[10.0, 10.0, 15.0"),
        ("converged_ha = 30.0", "converged_ha = 20.0"),
    ] {
        let root = rivals_tree("tune-ladder", INPUT, TUNED, PW, ABINIT);
        crate::write(&root, "benchmarks/RIVALS.toml", &RIVALS.replace(from, to));
        let (findings, _) = rivals(&root, "calibrate");
        assert!(
            has(
                &findings,
                "rivals: benchmarks/RIVALS.toml: [qe] ladder_ha must ascend and stay below converged_ha"
            ),
            "{to}: {findings:?}"
        );
    }
}
