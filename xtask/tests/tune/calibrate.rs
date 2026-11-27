use super::rivals;
use crate::harness::rival::{ABINIT, INPUT, PW, RIVALS, TUNED, rivals_tree};
use crate::{Tree, has};
use xtask::Env;

const V1_PW: &str = "v1() { grep -qx '  0.0 5.0 5.0' pw.in; }";
const V1_ABINIT: &str = "v1() { grep -qx '  0.0 5.0 5.0' run.abi; }";

#[test]
fn calibrate_freezes_the_lowest_cutoff_from_which_every_higher_rung_meets_the_target() {
    let pw = format!(
        "{V1_PW}\nif v1; then grep -q 'tstress = .true.' pw.in || exit 9; else grep -q 'tstress = .false.' pw.in || exit 8; fi\n\
         c=$(sed -n 's/^  ecutwfc = //p' pw.in)\n\
         p=10; case $c in 20.0) v1 && e=-3.9;; 30.0) p=12;; esac\n{PW}\necho \"          total   stress  (Ry/bohr**3)                   (kbar)     P=       $p\""
    );
    let abinit = format!(
        "{V1_ABINIT}\nif v1; then grep -q 'optstress 1' run.abi || exit 9; else grep -q 'optstress 0' run.abi || exit 8; fi\n\
         case $(sed -n 's/^ecut //p' run.abi) in 6.0) v1 && e=-1.9;; 8.0) exit 1;; esac\n{ABINIT}\n\
         echo '-Cartesian components of stress tensor (GPa)         [Pressure=  1.0E+00 GPa]'"
    );
    let rivals_toml = RIVALS.replace("converged_ha = 30.0", "converged_ha = 25.0");
    let two = INPUT.replace("[\"Si\"]", "[\"Si\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    let root = rivals_tree("tune-calibrate", &two, "", &pw, &abinit);
    std::fs::remove_file(root.join("benchmarks/TUNED.toml")).unwrap();
    crate::write(&root, "benchmarks/RIVALS.toml", &rivals_toml);
    let (findings, tuned) = rivals(&root, "calibrate");
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(17.5),
        "{tuned}"
    );
    assert_eq!(
        tuned["abinit"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
    let crashed = &tuned["abinit"]["calibration"]["benchmarks/1_a/input.toml"][1];
    let crashed: Vec<f64> = crashed
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x.as_float().unwrap())
        .collect();
    assert!(
        crashed[0] == 8.0 && crashed[1].is_nan() && crashed[2].is_nan(),
        "{tuned}"
    );
    let rows = tuned["qe"]["calibration"]["benchmarks/1_a/input.toml"]
        .as_array()
        .unwrap();
    let row = |i: usize| -> Vec<f64> {
        rows[i]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_float().unwrap())
            .collect()
    };
    assert_eq!(rows.len(), 5);
    assert_eq!(row(0)[0], 10.0, "{tuned}");
    assert!(
        (row(0)[1] - 0.025).abs() < 1e-12 && row(0)[2] == 0.0,
        "{tuned}"
    );
    assert!(
        row(2)[1] == 0.0 && (row(2)[2] - 0.2).abs() < 1e-12,
        "{tuned}"
    );
}

#[test]
fn abinits_pressure_alone_can_hold_its_cutoff_up() {
    let abinit = format!(
        "p=1.0; case $(sed -n 's/^ecut //p' run.abi) in 6.0|8.0) p=1.5;; esac\n{ABINIT}\n\
         echo \"-Cartesian components of stress tensor (GPa)         [Pressure=  $p GPa]\""
    );
    let root = rivals_tree("tune-abinit-pressure", INPUT, TUNED, PW, &abinit);
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", "calibrate", "abinit"], &root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    let text = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    let tuned: toml::Table = text.parse().unwrap();
    assert_eq!(
        tuned["abinit"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
}

fn pw_from(rule: &str) -> String {
    format!(
        "{V1_PW}\nc=$(sed -n 's/^  ecutwfc = //p' pw.in); c=${{c%.*}}\n{rule}\n[ \"$c\" -lt \"$low\" ] && v1 && e=-3.9\n{PW}\n\
         echo 'total   stress  (kbar)  P=  1'"
    )
}

fn calibrate_qe(root: &Tree) -> (Vec<String>, toml::Table) {
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", "calibrate", "qe"], root, &env);
    let text = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    (findings, text.parse().unwrap())
}

#[test]
fn an_element_takes_the_least_frozen_cutoff_of_the_cells_that_hold_it() {
    let pw = pw_from("low=25; grep -q '^  Al ' pw.in && low=35");
    let root = rivals_tree("tune-least", INPUT, TUNED, &pw, ABINIT);
    let alsi = INPUT.replace("[\"Si\"]", "[\"Al\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    crate::write(&root, "benchmarks/1_a/inputs/alsi.toml", &alsi);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "calibration_cells = [\"benchmarks/1_a/input.toml\"]",
            "calibration_cells = [\"benchmarks/1_a/inputs/alsi.toml\", \"benchmarks/1_a/input.toml\"]",
        ),
    );
    let (findings, tuned) = calibrate_qe(&root);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(12.5),
        "{tuned}"
    );
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Al"].as_float(),
        Some(17.5),
        "{tuned}"
    );
    let set: Vec<&str> = (tuned["tuning_set"].as_array().unwrap().iter())
        .map(|c| c.as_str().unwrap())
        .collect();
    assert_eq!(
        set,
        [
            "benchmarks/1_a/input.toml",
            "benchmarks/1_a/inputs/alsi.toml"
        ],
        "{tuned}"
    );
}

#[test]
fn a_cell_that_fails_hides_no_cell_after_it_and_the_cells_that_froze_are_written() {
    let pw = pw_from("low=25; grep -q '^  Al ' pw.in && low=60");
    let root = rivals_tree("tune-each-cell", INPUT, TUNED, &pw, ABINIT);
    let alsi = INPUT.replace("[\"Si\"]", "[\"Al\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    crate::write(&root, "benchmarks/1_a/inputs/alsi.toml", &alsi);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "calibration_cells = [\"benchmarks/1_a/input.toml\"]",
            "calibration_cells = [\"benchmarks/1_a/inputs/alsi.toml\", \"benchmarks/1_a/input.toml\"]",
        ),
    );
    let (findings, tuned) = calibrate_qe(&root);
    assert_eq!(
        findings,
        [
            "rivals: qe: benchmarks/1_a/inputs/alsi.toml: no rung of the ladder meets the target with a rung above it that meets it too"
        ],
        "{tuned}"
    );
    let cutoffs = tuned["qe"]["cutoff_ha"].as_table().unwrap();
    assert_eq!(cutoffs.len(), 1, "{tuned}");
    assert_eq!(cutoffs["Si"].as_float(), Some(12.5), "{tuned}");
    assert_eq!(
        tuned["qe"]["calibration"].as_table().unwrap().len(),
        2,
        "{tuned}"
    );
}

#[test]
fn a_cell_that_its_elements_cannot_give_its_own_cutoff_back_fails_the_calibration() {
    let pw = pw_from("low=25; grep -q '^  nat = 2' pw.in && low=35");
    let root = rivals_tree("tune-least-fails", INPUT, TUNED, &pw, ABINIT);
    let two = INPUT.replace("[\"Si\"]", "[\"Si\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    crate::write(&root, "benchmarks/1_a/inputs/si2.toml", &two);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "calibration_cells = [\"benchmarks/1_a/input.toml\"]",
            "calibration_cells = [\"benchmarks/1_a/input.toml\", \"benchmarks/1_a/inputs/si2.toml\"]",
        ),
    );
    let (findings, tuned) = calibrate_qe(&root);
    assert!(
        has(
            &findings,
            "rivals: qe: benchmarks/1_a/inputs/si2.toml froze at 17.5 Ha, but its elements give it back 12.5 Ha"
        ),
        "{findings:?}"
    );
    assert_eq!(
        tuned,
        toml::from_str::<toml::Table>(&crate::harness::rival::with_layout_policies(&root, TUNED))
            .unwrap(),
        "TUNED.toml stays as it was"
    );
}

#[test]
fn calibration_runs_take_every_core_one_thread_per_rank() {
    let log = std::env::temp_dir().join(format!("xtask-tune-cores-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&log);
    std::fs::create_dir_all(&log).unwrap();
    let pw = format!("{PW}\necho 'total   stress  (kbar)  P=  1'");
    let abinit = format!("{ABINIT}\necho ' Pressure=  1.0 GPa'");
    let root = rivals_tree("tune-cores", INPUT, TUNED, &pw, &abinit);
    for code in ["qe", "abinit"] {
        crate::harness::rival::script(
            &root.join(format!("rivals/{code}/bin/mpiexec")),
            &format!(
                "echo \"$OMP_NUM_THREADS $*\" >> '{}/{code}'\nwhile [ \"$1\" != -n ]; do shift; done\nshift 2\nexec \"$@\"",
                log.display()
            ),
        );
    }
    let (findings, _) = rivals(&root, "calibrate");
    let seen = |code: &str| std::fs::read_to_string(log.join(code)).unwrap_or_default();
    let (qe, abinit) = (seen("qe"), seen("abinit"));
    let _ = std::fs::remove_dir_all(&log);
    assert!(findings.is_empty(), "{findings:?}");
    let cores = std::thread::available_parallelism().unwrap().get();
    let pools = (1..=cores)
        .filter(|&n| cores.is_multiple_of(n) && 8_usize.is_multiple_of(n))
        .max()
        .unwrap();
    let nk = format!(" -nk {pools}");
    let qe_ok = |l: &str| {
        l.starts_with(&format!("1 --bind-to none -n {cores} "))
            && l.ends_with(&format!("/pw.x{nk} -in pw.in"))
    };
    assert!(
        !qe.is_empty() && qe.lines().all(qe_ok),
        "{cores} cores: {qe}"
    );
    let abinit_ok =
        |l: &str| l.starts_with(&format!("1 -n {} ", cores.min(8))) && l.contains("/abinit");
    assert!(
        !abinit.is_empty() && abinit.lines().all(abinit_ok),
        "{cores} cores: {abinit}"
    );
}

#[test]
fn a_spin_cell_spreads_each_k_point_and_spin_over_its_own_rank() {
    let log = std::env::temp_dir().join(format!("xtask-tune-spin-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let abinit = format!("m=1.0\n{ABINIT}\necho ' Pressure=  1.0 GPa'\n{ABINIT_MOMENT}");
    let spin =
        INPUT.replace("mesh = [2, 2, 2]", "mesh = [1, 1, 1]") + "\n[spin]\nmoments = [1.0]\n";
    let pw = format!("{PW}\necho 'total   stress  (kbar)  P=  1'");
    let root = rivals_tree("tune-spin", &spin, TUNED, &pw, &abinit);
    for code in ["qe", "abinit"] {
        crate::harness::rival::script(
            &root.join(format!("rivals/{code}/bin/mpiexec")),
            &format!(
                "echo \"$*\" >> '{}'\nwhile [ \"$1\" != -n ]; do shift; done\nshift 2\nexec \"$@\"",
                log.display()
            ),
        );
    }
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", "calibrate", "abinit"], &root, &env);
    let seen = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    assert!(findings.is_empty(), "{findings:?}");
    assert!(
        !seen.is_empty() && seen.lines().all(|l| l.starts_with("-n 2 ")),
        "{seen}"
    );
}

#[test]
fn a_qe_pool_count_divides_the_k_points_as_well_as_the_cores() {
    let log = std::env::temp_dir().join(format!("xtask-tune-odd-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let odd = INPUT
        .replace("mesh = [2, 2, 2]", "mesh = [3, 1, 1]")
        .replace("shift = [0.5, 0.5, 0.5]\n", "");
    let pw = format!("{PW}\necho 'total   stress  (kbar)  P=  1'");
    let abinit = format!("{ABINIT}\necho ' Pressure=  1.0 GPa'");
    let root = rivals_tree("tune-odd", &odd, TUNED, &pw, &abinit);
    for code in ["qe", "abinit"] {
        crate::harness::rival::script(
            &root.join(format!("rivals/{code}/bin/mpiexec")),
            &format!(
                "echo \"$*\" >> '{}'\nwhile [ \"$1\" != -n ]; do shift; done\nshift 2\nexec \"$@\"",
                log.display()
            ),
        );
    }
    let (findings, _) = rivals(&root, "calibrate");
    let seen = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    assert!(findings.is_empty(), "{findings:?}");
    let cores = std::thread::available_parallelism().unwrap().get();
    let pools = if cores.is_multiple_of(3) { 3 } else { 1 };
    let (qe, abinit): (Vec<&str>, Vec<&str>) = seen.lines().partition(|l| l.contains("/pw.x"));
    let nk = format!("/pw.x -nk {pools} -in pw.in");
    assert!(
        !qe.is_empty() && qe.iter().all(|l| l.ends_with(&nk)),
        "{cores} cores: {seen}"
    );
    let ranks = format!("-n {} ", cores.min(3));
    assert!(
        !abinit.is_empty() && abinit.iter().all(|l| l.starts_with(&ranks)),
        "{cores} cores: {seen}"
    );
}

#[test]
fn a_calibration_run_whose_lines_disagree_with_its_clock_still_calibrates() {
    let burst = "sleep 0.25\necho ' ABINIT 10.0.3'\necho ' ETOT  1  -2.0    -2.000E+00 1.0E-04 1.0E+00'\n\
        echo ' Proc.   0 individual time (sec): cpu=          0.25  wall=          0.25'\necho ' Pressure=  1.0 GPa'";
    let root = rivals_tree("tune-burst", INPUT, TUNED, PW, burst);
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", "calibrate", "abinit"], &root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    let tuned: toml::Table = std::fs::read_to_string(root.join("benchmarks/TUNED.toml"))
        .unwrap()
        .parse()
        .unwrap();
    assert_eq!(
        tuned["abinit"]["cutoff_ha"]["Si"].as_float(),
        Some(6.0),
        "{tuned}"
    );
}

#[test]
fn calibrate_names_a_cell_that_no_rung_brings_to_the_target() {
    let pw = format!(
        "{V1_PW}\nc=$(sed -n 's/^  ecutwfc = //p' pw.in)\ncase $c in 50.0|40.0) ;; *) v1 && e=-3.9;; esac\n{PW}\necho 'total   stress  (kbar)  P=  1'"
    );
    let crash = "echo 'Error in pawpsp_17in' >&2\nexit 1";
    let root = rivals_tree("tune-never", INPUT, "", &pw, crash);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace("converged_ha = 30.0", "converged_ha = 25.0"),
    );
    let (findings, tuned) = rivals(&root, "calibrate");
    assert!(tuned.is_empty(), "{tuned}");
    assert!(
        has(
            &findings,
            "rivals: qe: benchmarks/1_a/input.toml: no rung of the ladder meets the target with a rung above it that meets it too"
        ),
        "{findings:?}"
    );
    assert!(
        has(
            &findings,
            "rivals: abinit: engine exited with exit status: 1: Error in pawpsp_17in"
        ),
        "{findings:?}"
    );
}

fn qe_rows(tuned: &toml::Table) -> Vec<Vec<f64>> {
    (tuned["qe"]["calibration"]["benchmarks/1_a/input.toml"]
        .as_array()
        .unwrap()
        .iter())
    .map(|r| {
        (r.as_array().unwrap().iter())
            .map(|x| x.as_float().unwrap())
            .collect()
    })
    .collect()
}

fn qe_only(name: &str, body: &str) -> (Vec<String>, toml::Table) {
    let pw = format!(
        "{V1_PW}\nc=$(sed -n 's/^  ecutwfc = //p' pw.in); c=${{c%.*}}\np=10\n{body}\n{PW}\n\
         echo \"total   stress  (kbar)  P=  $p\""
    );
    let root = rivals_tree(name, INPUT, TUNED, &pw, ABINIT);
    calibrate_qe(&root)
}

#[test]
fn an_absolute_error_that_cancels_in_the_energy_difference_freezes_lower_than_the_absolute_rule() {
    let (findings, tuned) = qe_only("tune-difference-meets", "[ \"$c\" -lt 35 ] && e=-3.9");
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
    let first = &qe_rows(&tuned)[0];
    assert!(
        first[1] == 0.0 && (first[3] - 0.05).abs() < 1e-12,
        "{tuned}"
    );
}

#[test]
fn an_energy_difference_error_holds_the_cutoff_up_though_the_absolute_energy_meets() {
    let (findings, tuned) = qe_only(
        "tune-difference-fails",
        "[ \"$c\" -lt 35 ] && ! v1 && e=-3.9",
    );
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(17.5),
        "{tuned}"
    );
    let first = &qe_rows(&tuned)[0];
    assert!(
        (first[1] - 0.05).abs() < 1e-12 && first[3] == 0.0,
        "{tuned}"
    );
}

#[test]
fn a_failed_compressed_run_fails_its_rung_with_nan_in_the_energy_difference_alone() {
    let (findings, tuned) = qe_only("tune-v2-crash", "[ \"$c\" = 30 ] && ! v1 && exit 1");
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(17.5),
        "{tuned}"
    );
    let row = &qe_rows(&tuned)[2];
    assert!(
        row[0] == 15.0 && row[1].is_nan() && row[2] == 0.0 && row[3] == 0.0,
        "{tuned}"
    );
}

#[test]
fn a_row_holds_the_cutoff_the_three_errors_and_the_pressure_itself() {
    let pw = format!(
        "{V1_PW}\nc=$(sed -n 's/^  ecutwfc = //p' pw.in); c=${{c%.*}}\np=10\n\
         if v1; then e=-4.0; [ \"$c\" = 20 ] && {{ e=-3.9; p=12; }}; else e=-4.02; [ \"$c\" = 20 ] && e=-3.95; fi\n\
         {PW}\necho \"total   stress  (kbar)  P=  $p\""
    );
    let two = INPUT.replace("[\"Si\"]", "[\"Si\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    let root = rivals_tree("tune-row", &two, TUNED, &pw, ABINIT);
    let (findings, tuned) = calibrate_qe(&root);
    assert!(findings.is_empty(), "{findings:?}");
    let row = &qe_rows(&tuned)[0];
    let want = [10.0, 0.0075, 0.2, 0.025, 1.2];
    assert_eq!(row.len(), want.len(), "{row:?}");
    for (got, want) in row.iter().zip(want) {
        assert!((got - want).abs() < 1e-12, "{row:?}");
    }
    assert_eq!(
        tuned["qe"]["volume_scale"].as_float(),
        Some(0.94),
        "{tuned}"
    );
}

#[test]
fn the_compressed_cell_holds_the_volume_scale_and_the_same_fractional_positions() {
    let log = std::env::temp_dir().join(format!("xtask-tune-v2-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&log);
    std::fs::create_dir_all(&log).unwrap();
    let pw = format!(
        "{V1_PW}\nif v1; then cp pw.in '{0}/v1.in'; else cp pw.in '{0}/v2.in'; fi\n{PW}\necho 'total   stress  (kbar)  P=  1'",
        log.display()
    );
    let root = rivals_tree("tune-v2-cell", INPUT, TUNED, &pw, ABINIT);
    let (findings, _) = calibrate_qe(&root);
    assert!(findings.is_empty(), "{findings:?}");
    let read = |name: &str| std::fs::read_to_string(log.join(name)).unwrap();
    let (v1, v2) = (read("v1.in"), read("v2.in"));
    let _ = std::fs::remove_dir_all(&log);
    let block = |text: &str, head: &str| -> Vec<String> {
        (text.lines().skip_while(|l| !l.starts_with(head)).skip(1))
            .take_while(|l| l.starts_with("  "))
            .map(str::to_owned)
            .collect()
    };
    let det = |text: &str| {
        let m: Vec<Vec<f64>> = (block(text, "CELL_PARAMETERS").iter())
            .map(|l| l.split_whitespace().map(|x| x.parse().unwrap()).collect())
            .collect();
        m[0][0] * (m[1][1] * m[2][2] - m[1][2] * m[2][1])
            - m[0][1] * (m[1][0] * m[2][2] - m[1][2] * m[2][0])
            + m[0][2] * (m[1][0] * m[2][1] - m[1][1] * m[2][0])
    };
    assert!((det(&v2) / det(&v1) - 0.94).abs() < 1e-15, "{v1}\n{v2}");
    assert_eq!(
        block(&v1, "ATOMIC_POSITIONS"),
        block(&v2, "ATOMIC_POSITIONS")
    );
    assert_eq!(block(&v1, "K_POINTS"), block(&v2, "K_POINTS"));
}

#[test]
fn a_tuned_file_of_another_rule_or_volume_scale_is_refused_by_name() {
    let three = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]",
        "[[10.0, 1e-6, 0.0], [13.0, 1e-6, 0.0]]",
    );
    let root = rivals_tree("tune-three-columns", INPUT, &three, PW, ABINIT);
    let (findings, _) = rivals(&root, "calibrate");
    assert!(
        has(&findings, "a calibration row of 3 columns, not 5") && has(&findings, "recalibrate"),
        "{findings:?}"
    );
    for (scale, said) in [("volume_scale = 0.9\n", "Some(0.9)"), ("", "None")] {
        let other = TUNED.replace("volume_scale = 0.94\n", scale);
        let root = rivals_tree("tune-other-scale", INPUT, &other, PW, ABINIT);
        let (findings, _) = rivals(&root, "calibrate");
        assert!(
            has(
                &findings,
                &format!(
                    "the abinit calibration ran at volume_scale {said}, but benchmarks/RIVALS.toml asks for 0.94; recalibrate"
                )
            ),
            "{findings:?}"
        );
    }
    let root = rivals_tree("tune-old-key", INPUT, TUNED, PW, ABINIT);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace("energy_difference_ha_per_atom", "energy_ha_per_atom"),
    );
    let (findings, _) = rivals(&root, "calibrate");
    assert!(
        has(&findings, "unknown field `energy_ha_per_atom`"),
        "{findings:?}"
    );
}

fn alsi_tree(name: &str, tuned: &str, abinit: &str) -> Tree {
    let root = rivals_tree(name, INPUT, tuned, PW, abinit);
    let alsi = INPUT.replace("[\"Si\"]", "[\"Al\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    crate::write(&root, "benchmarks/1_a/inputs/alsi.toml", &alsi);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "calibration_cells = [\"benchmarks/1_a/input.toml\"]",
            "calibration_cells = [\"benchmarks/1_a/input.toml\", \"benchmarks/1_a/inputs/alsi.toml\"]",
        ),
    );
    root
}

fn calibrate_abinit(root: &Tree, cells: &[&str]) -> (Vec<String>, toml::Table) {
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let mut args = vec!["rivals", "calibrate", "abinit"];
    for cell in cells {
        args.extend(["--cell", cell]);
    }
    let findings = xtask::run(&args, root, &env);
    let text = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    (findings, text.parse().unwrap())
}

const PRESSURE: &str =
    "echo '-Cartesian components of stress tensor (GPa)         [Pressure=  1.0E+00 GPa]'";

#[test]
fn calibrate_with_cell_runs_that_cell_alone_and_keeps_the_rows_of_the_others() {
    let abinit = format!(
        "{V1_ABINIT}\ngrep -q '^natom 2' run.abi || exit 5\n\
         case $(sed -n 's/^ecut //p' run.abi) in 6.0|8.0) v1 && e=-2.1;; esac\n{ABINIT}\n{PRESSURE}"
    );
    let kept = "[[6.0, 1.0, 0.0, 1.0, 1.0], [8.0, 0.0, 0.0, 0.0, 1.0], [10.0, 0.0, 0.0, 0.0, 1.0], [12.0, 0.0, 0.0, 0.0, 1.0]]";
    let tuned = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]",
        kept,
    );
    let root = alsi_tree("tune-one-cell", &tuned, &abinit);
    let (findings, tuned) = calibrate_abinit(&root, &["benchmarks/1_a/inputs/alsi.toml"]);
    assert!(findings.is_empty(), "{findings:?}");
    let cutoff = |e: &str| tuned["abinit"]["cutoff_ha"][e].as_float();
    assert_eq!(
        (cutoff("Si"), cutoff("Al")),
        (Some(8.0), Some(10.0)),
        "{tuned}"
    );
    let calibration = &tuned["abinit"]["calibration"];
    assert_eq!(
        calibration["benchmarks/1_a/input.toml"],
        toml::from_str::<toml::Table>(&format!("r = {kept}")).unwrap()["r"],
        "{tuned}"
    );
    assert_eq!(
        calibration["benchmarks/1_a/inputs/alsi.toml"]
            .as_array()
            .unwrap()
            .len(),
        4,
        "{tuned}"
    );
}

#[test]
fn calibrate_with_cell_keeps_no_rows_of_another_ladder_or_of_a_cell_never_calibrated() {
    let abinit = format!("{ABINIT}\n{PRESSURE}");
    for (only, finding) in [
        (
            "benchmarks/1_a/inputs/alsi.toml",
            "rivals: abinit: benchmarks/1_a/input.toml: its benchmarks/TUNED.toml rows hold rungs [10.0, 13.0], not today's ladder; calibrate it",
        ),
        (
            "benchmarks/1_a/input.toml",
            "rivals: abinit: benchmarks/1_a/inputs/alsi.toml: benchmarks/TUNED.toml holds no rows of it; calibrate it",
        ),
    ] {
        let root = alsi_tree("tune-one-cell-stale", TUNED, &abinit);
        let (findings, _) = calibrate_abinit(&root, &[only]);
        assert_eq!(findings, [finding], "{only}");
        let after = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
        assert_eq!(
            after,
            crate::harness::rival::with_layout_policies(&root, TUNED),
            "{only}"
        );
    }
}

#[test]
fn cell_scopes_calibrate_alone_and_names_only_calibration_cells() {
    let root = rivals_tree("tune-cell-refused", INPUT, TUNED, PW, ABINIT);
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let cell = "benchmarks/1_a/input.toml";
    for job in ["mixing", "layout"] {
        let f = xtask::run(&["rivals", job, "--cell", cell], &root, &env);
        assert!(
            has(&f, "rivals: --cell scopes calibrate alone"),
            "{job}: {f:?}"
        );
    }
    let f = xtask::run(
        &["rivals", "calibrate", "--cell", "benchmarks/1_a/none.toml"],
        &root,
        &env,
    );
    assert!(
        has(
            &f,
            "rivals: --cell benchmarks/1_a/none.toml: not one of benchmarks/RIVALS.toml's calibration_cells"
        ),
        "{f:?}"
    );
}

const ABINIT_MOMENT: &str = "echo ' Integrated electronic and magnetization densities in atomic spheres:'\n\
     echo ' ---------------------------------------------------------------------'\n\
     echo ' Radius=ratsph(iatom), smearing ratsm=  0.0000. Diff(up-dn)=approximate z local magnetic moment.'\n\
     echo ' Atom    Radius    up_density   dn_density  Total(up+dn)  Diff(up-dn)'\n\
     echo \"    1   2.11532     2.500000     1.500000      4.000000     $m\"\n\
     echo ' ---------------------------------------------------------------------'";
const PW_MOMENT: &str = "echo '     Magnetic moment per site  (integrated on atomic sphere of radius R)'\n\
     echo \"     atom   1 (R=0.413)  charge=  4.0000  magn=  $m\"\n\
     echo ''";

fn spin_si() -> String {
    format!("{INPUT}\n[spin]\nmoments = [1.0]\n")
}

#[test]
fn a_rung_in_another_magnetic_state_runs_once_more_on_the_other_mixing() {
    let pw = format!(
        "{V1_PW}\nm=2.0\nif ! v1 && grep -q '^  ecutwfc = 25.0' pw.in && ! grep -qx '  mixing_beta = 0.3' pw.in; \
         then m=1.0; e=-2.0; fi\n{PW}\necho 'total   stress  (kbar)  P=  1'\n{PW_MOMENT}"
    );
    let abinit = format!(
        "{V1_ABINIT}\nm=2.0\nif ! v1 && grep -qx 'ecut 8.0' run.abi && ! grep -qx 'diemac 10.0' run.abi; \
         then m=1.0; e=-1.0; fi\n{ABINIT}\n{PRESSURE}\n{ABINIT_MOMENT}"
    );
    let root = rivals_tree("tune-state-again", &spin_si(), TUNED, &pw, &abinit);
    let (findings, tuned) = calibrate_qe(&root);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
    let (findings, tuned) = calibrate_abinit(&root, &[]);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["abinit"]["cutoff_ha"]["Si"].as_float(),
        Some(6.0),
        "{tuned}"
    );
}

#[test]
fn a_rung_in_another_magnetic_state_on_both_mixings_fails() {
    let abinit = format!(
        "{V1_ABINIT}\nm=1.0\nif ! v1 && grep -qx 'ecut 8.0' run.abi; then m=-1.0; fi\n{ABINIT}\n{PRESSURE}\n\
         {ABINIT_MOMENT}"
    );
    let root = rivals_tree("tune-state-lost", &spin_si(), TUNED, PW, &abinit);
    let (findings, tuned) = calibrate_abinit(&root, &[]);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["abinit"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
    let lost = &tuned["abinit"]["calibration"]["benchmarks/1_a/input.toml"][1];
    assert!(
        lost[0].as_float() == Some(8.0)
            && lost[1].as_float().unwrap().is_nan()
            && lost[2].as_float() == Some(0.0),
        "{tuned}"
    );
}

#[test]
fn a_spin_cell_whose_converged_run_prints_no_moment_fails_by_name() {
    let abinit = format!("{ABINIT}\n{PRESSURE}");
    let root = rivals_tree("tune-state-none", &spin_si(), TUNED, PW, &abinit);
    let (findings, _) = calibrate_abinit(&root, &[]);
    assert_eq!(
        findings,
        [
            "rivals: abinit: benchmarks/1_a/input.toml at 20 Ha: moments None μB, no moment on an atom that the cell \
             starts magnetic"
        ],
        "{findings:?}"
    );
}
