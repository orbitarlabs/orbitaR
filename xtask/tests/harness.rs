#[path = "../../tests/benchmarks/6_abinit_jth/case.rs"]
mod abinit_jth;
mod acwf;
mod datasets;
mod engine;
mod eos;
mod load;
mod record;
mod relax;
pub mod rival;
pub(crate) mod speed;

use crate::{Tree, lever, tree};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    ffi::OsString,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    sync::Mutex,
    time::SystemTime,
};
use xtask::harness::{Evidence, Measure, Metrics, Outcome, Session, Tier};

pub const CASE: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nerror_max = 1e-12\n";
pub const INPUT: &str = "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\n[basis]\ncutoff_ha = 1.0\n\n[kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[bands]\ncount = 1\n";
pub const GOOD: &str = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1]}'";

pub const NAMES_FILES: &str = r#"__names() {
__s=$?
exec 1>&3 3>&-
__out=$(cat .stdout)
case "$__out" in
  *'"datasets"'*) ;;
  '{'*'}')
    __map=""; __seen=""
    __u=$(sed -n 's/^functional = "\(.*\)"$/\1/p' input.toml | tr a-z A-Z)
    for __e in $(sed -n 's/^species = \[\(.*\)\]$/\1/p' input.toml | tr -d '",'); do
      case " $__seen " in *" $__e "*) continue;; esac
      __seen="$__seen $__e"
      __f=$(sed -n "s/^$__e = \"\(.*\)\"$/\1/p" input.toml)
      if [ -z "$__f" ]; then
        for __p in datasets/*-${__u:-PBE}-*/$__e/* datasets/*/$__e/*; do [ -f "$__p" ] && __f=${__p#datasets/} && break; done
      fi
      [ -n "$__f" ] && [ -f "datasets/$__f" ] || continue
      __h=$({ sha256sum "datasets/$__f" 2>/dev/null || shasum -a 256 "datasets/$__f"; } | cut -c1-64)
      __map="$__map${__map:+, }\"$__e\": {\"path\": \"$__f\", \"sha256\": \"$__h\"}"
    done
    if [ -n "$__map" ]; then
      case "$__out" in
        '{}') __out="{\"datasets\": {$__map}}" ;;
        *) __out="${__out%\}}, \"datasets\": {$__map}}" ;;
      esac
    fi ;;
esac
printf '%s' "$__out"
exit $__s
}
exec 3>&1 1>.stdout
trap __names EXIT"#;

pub fn wrapped(body: &str) -> String {
    format!("{NAMES_FILES}\n{body}\n")
}

pub fn engine(root: &Path, body: &str) -> PathBuf {
    let path = root.join("fake-engine");
    std::fs::write(&path, format!("#!/bin/sh\n{}", wrapped(body))).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

pub fn honest(evidence: &Evidence) -> Result<Metrics, String> {
    let e = evidence.output("input")?.eigenvalues_ha()?[0][0][0];
    Ok(Metrics::from([("error".to_owned(), (e - 0.5).abs())]))
}

pub fn run_case(
    name: &str,
    case: &str,
    input: &str,
    body: &str,
    measure: fn(&Evidence) -> Result<Metrics, String>,
) -> (Outcome, Tree) {
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", case),
            ("benchmarks/1_a/input.toml", input),
        ],
    );
    let bin = engine(&root, body);
    let records = root.join("records");
    let session = Session::new(&root, &bin, Tier::Push, &records);
    (session.evaluate("1_a", measure), root)
}

#[test]
fn an_honest_case_passes_and_leaves_a_record() {
    let (outcome, root) = run_case("harness-pass", CASE, INPUT, GOOD, honest);
    assert_eq!(outcome, Outcome::Pass);
    let record = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    for field in [
        "\"runs_s\":[",
        "\"rotation_max\":0.0",
        "\"rotation_used\":0.0",
        "\"rotation_field\":null",
        "\"seed\":",
        "\"outcome\":\"pass\"",
    ] {
        assert!(record.contains(field), "{field} in {record}");
    }
}

#[test]
fn a_record_carries_the_why_of_its_case() {
    let case = format!("{CASE}\n[why]\n{}\n", crate::WHY_EXAMPLE.join("\n"));
    let (outcome, root) = run_case("harness-why", &case, INPUT, GOOD, honest);
    assert_eq!(outcome, Outcome::Pass);
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap())
            .unwrap();
    assert_eq!(
        record["why"],
        serde_json::json!({
            "error_max": "the worst seen is 4e-13; the margin is for another libm",
            "status": "active: its bound passed in the run that promoted it",
        }),
        "{record}"
    );
}

#[test]
fn the_case_20_reducer_prints_its_usage_before_it_reads_a_missing_operand() {
    let script = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../benchmarks/20_abinit_r2scan/reference/reduce.py"
    );
    let short: [&[&str]; 5] = [
        &[],
        &["--abo"],
        &["--abo", "run.abo"],
        &["--abo", "run.abo", "reference.json", "4"],
        &["reference.json", "4"],
    ];
    for args in short {
        let out = std::process::Command::new("python3")
            .arg("-I")
            .arg(script)
            .args(args)
            .output()
            .unwrap();
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert_eq!(out.status.code(), Some(1), "{args:?}: {stderr}");
        assert!(
            stderr.starts_with("usage: python3 -I reduce.py"),
            "{args:?}: {stderr}"
        );
    }
}

#[test]
fn a_measure_that_ignores_the_output_fails() {
    fn fixed(_: &Evidence) -> Result<Metrics, String> {
        Ok(Metrics::from([("error".to_owned(), 0.0)]))
    }
    let (outcome, _) = run_case("harness-fixed", CASE, INPUT, GOOD, fixed);
    assert!(
        matches!(outcome, Outcome::Fail(why) if why.contains("measure never read the output of `input`"))
    );
}

#[test]
fn a_measure_that_clamps_its_metric_fails() {
    fn clamped(evidence: &Evidence) -> Result<Metrics, String> {
        let e = evidence.output("input")?.eigenvalues_ha()?[0][0][0];
        Ok(Metrics::from([(
            "error".to_owned(),
            (e - 0.5).abs().min(1e-13),
        )]))
    }
    let (outcome, _) = run_case("harness-clamp", CASE, INPUT, GOOD, clamped);
    assert!(
        matches!(outcome, Outcome::Fail(why) if why.contains("passed 8 of 8 perturbed copies of `input.eigenvalues_ha`"))
    );
}

#[test]
fn a_measure_must_read_every_input() {
    let root = tree(
        "harness-inputs",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", INPUT),
            ("benchmarks/1_a/inputs/b.toml", INPUT),
        ],
    );
    let bin = engine(&root, GOOD);
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    let outcome = session.evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("never read the output of `b`") && !why.contains("of `input`")),
        "{outcome:?}"
    );
}

fn evaluate(
    name: &str,
    files: &[(&str, &str)],
    measure: fn(&Evidence) -> Result<Metrics, String>,
) -> Outcome {
    let mut all = vec![("benchmarks/1_a/case.toml", CASE)];
    all.extend_from_slice(files);
    let root = tree(name, &all);
    let bin = engine(&root, GOOD);
    Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", measure)
}

#[test]
fn names_follow_run_order_and_input_is_reserved() {
    fn names(evidence: &Evidence) -> Result<Metrics, String> {
        Err(evidence.names().join(","))
    }
    let order = evaluate(
        "harness-order",
        &[
            ("benchmarks/1_a/input.toml", INPUT),
            ("benchmarks/1_a/inputs/a.toml", INPUT),
        ],
        names,
    );
    assert!(
        matches!(&order, Outcome::Fail(why) if why.contains("measure: input,a")),
        "{order:?}"
    );
    let clash = evaluate(
        "harness-clash",
        &[
            ("benchmarks/1_a/input.toml", INPUT),
            ("benchmarks/1_a/inputs/input.toml", INPUT),
        ],
        honest,
    );
    assert!(
        matches!(&clash, Outcome::Fail(why) if why.contains("the name `input` belongs to input.toml")),
        "{clash:?}"
    );
}

#[test]
fn an_unreadable_input_is_a_harness_fault_not_an_absent_file() {
    let root = tree("harness-bytes", &[("benchmarks/1_a/case.toml", CASE)]);
    let mut bytes = INPUT.as_bytes().to_vec();
    bytes.extend_from_slice(b"# caf\xe9\n");
    std::fs::write(root.join("benchmarks/1_a/input.toml"), bytes).unwrap();
    let bin = engine(&root, GOOD);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("harness:") && why.contains("input.toml")),
        "{outcome:?}"
    );
}

#[test]
fn inputs_hold_only_toml_files() {
    let root = tree(
        "harness-inputs-kind",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", INPUT),
            ("benchmarks/1_a/inputs/notes.txt", "x\n"),
        ],
    );
    let bin = engine(&root, GOOD);
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"));
    let outcome = session.evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("inputs/ holds only .toml files")),
        "{outcome:?}"
    );
}

#[test]
fn a_tolerance_just_above_a_wrong_metric_fails_every_run() {
    let loose = CASE.replace("1e-12", "0.0301");
    for run in 0..5 {
        let (outcome, _) = run_case(
            &format!("harness-edge-{run}"),
            &loose,
            INPUT,
            &GOOD.replace("0.5", "0.53"),
            honest,
        );
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("passed 4 of 8 perturbed copies of `input.eigenvalues_ha`")),
            "{outcome:?}"
        );
    }
}

#[test]
fn one_field_cannot_mask_a_shift_in_another() {
    fn both(evidence: &Evidence) -> Result<Metrics, String> {
        let output = evidence.output("input")?;
        let e = output.eigenvalues_ha()?[0][0][0];
        let size = output.basis_sizes()?[0] as f64;
        Ok(Metrics::from([
            ("error".to_owned(), (e - 0.5).abs()),
            ("size_error".to_owned(), (size - 1.0).abs()),
        ]))
    }
    let case = CASE.replace("1e-12", "0.0101") + "size_error_max = 0.0\n";
    let (outcome, _) = run_case(
        "harness-mask",
        &case,
        INPUT,
        &GOOD.replace("0.5", "0.51"),
        both,
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("perturbed copies of `input.eigenvalues_ha`") && !why.contains("input.basis_sizes")),
        "{outcome:?}"
    );
}

#[test]
fn a_measure_may_not_report_a_timing_metric() {
    fn timed(evidence: &Evidence) -> Result<Metrics, String> {
        let mut metrics = honest(evidence)?;
        metrics.insert("speedup".to_owned(), 9.0);
        Ok(metrics)
    }
    let (outcome, _) = run_case("harness-timing", CASE, INPUT, GOOD, timed);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("metric `speedup` is reserved")),
        "{outcome:?}"
    );
}

#[test]
fn a_measure_may_not_report_a_series_exponent_of_a_code() {
    fn with(evidence: &Evidence, name: &str) -> Result<Metrics, String> {
        let mut metrics = honest(evidence)?;
        metrics.insert(name.to_owned(), 1.0);
        Ok(metrics)
    }
    fn t_delta(evidence: &Evidence) -> Result<Metrics, String> {
        with(evidence, "t_delta_exponent_abinit")
    }
    fn pss(evidence: &Evidence) -> Result<Metrics, String> {
        with(evidence, "pss_exponent_orbitar")
    }
    fn lookalike(evidence: &Evidence) -> Result<Metrics, String> {
        with(evidence, "pss_exponentiation")
    }
    let measures: [(&str, Measure, bool); 3] = [
        ("t_delta_exponent_abinit", t_delta, true),
        ("pss_exponent_orbitar", pss, true),
        ("pss_exponentiation", lookalike, false),
    ];
    for (name, measure, reserved) in measures {
        let (outcome, _) = run_case("harness-series-name", CASE, INPUT, GOOD, measure);
        let refused = matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("metric `{name}` is reserved")));
        assert_eq!(refused, reserved, "{name}: {outcome:?}");
    }
}

#[test]
fn the_harness_judges_the_tolerance_not_the_case() {
    let (outcome, _) = run_case(
        "harness-judge",
        CASE,
        INPUT,
        &GOOD.replace("0.5", "0.6"),
        honest,
    );
    assert!(matches!(outcome, Outcome::Fail(why) if why.contains("> 1e-12")));
}

#[test]
fn the_judge_fails_a_low_minimum_a_nan_and_a_missing_metric() {
    fn slow(evidence: &Evidence) -> Result<Metrics, String> {
        let mut metrics = honest(evidence)?;
        metrics.insert("rate".to_owned(), 1.0);
        Ok(metrics)
    }
    fn nan(evidence: &Evidence) -> Result<Metrics, String> {
        honest(evidence)?;
        Ok(Metrics::from([("error".to_owned(), f64::NAN)]))
    }
    let case = CASE.replace("error_max = 1e-12\n", "error_max = 1e-12\nrate_min = 2.0\n");
    let (outcome, _) = run_case("judge-min", &case, INPUT, GOOD, slow);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rate = 1e0 < 2e0")),
        "{outcome:?}"
    );
    let (outcome, _) = run_case("judge-nan", CASE, INPUT, GOOD, nan);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("error = NaN")),
        "{outcome:?}"
    );
    let (outcome, _) = run_case("judge-missing", &case, INPUT, GOOD, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("metric rate missing for [tolerance].rate_min")),
        "{outcome:?}"
    );
}

#[test]
fn a_metric_exactly_at_its_limit_passes() {
    fn at_limits(evidence: &Evidence) -> Result<Metrics, String> {
        let mut metrics = honest(evidence)?;
        metrics.insert("rate".to_owned(), 2.0);
        metrics.insert("size".to_owned(), 3.0);
        Ok(metrics)
    }
    let case = CASE.replace(
        "error_max = 1e-12\n",
        "error_max = 1e-12\nrate_min = 2.0\nsize_max = 3.0\n",
    );
    let (outcome, _) = run_case("judge-limits", &case, INPUT, GOOD, at_limits);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn a_planned_case_that_passes_must_be_promoted() {
    let planned = CASE.replace("active", "planned");
    let (outcome, _) = run_case("harness-promote", &planned, INPUT, GOOD, honest);
    assert_eq!(outcome, Outcome::Promote);
    let (outcome, _) = run_case("harness-planned", &planned, INPUT, "exit 3", honest);
    assert!(matches!(outcome, Outcome::Planned(why) if why.contains("exit")));
}

#[test]
fn a_run_that_stops_at_its_step_cap_fails_with_the_engine_cause_line() {
    let body = "echo 'scf_step 1 free_energy_ha -1.0' >&2\n\
                echo 'scf.max_iterations: 200 steps, at 8 bands, leave 2.0e-11 above 1e-11' >&2\nexit 4";
    let (outcome, _) = run_case("harness-capped", CASE, INPUT, body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("exit status: 4")
            && why.contains("scf.max_iterations: 200 steps") && !why.contains("scf_step")),
        "{outcome:?}"
    );
}

#[test]
fn an_output_field_without_an_invariance_rule_fails() {
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"energy_ev\": 1.0}'";
    let (outcome, _) = run_case("harness-field", CASE, INPUT, body, honest);
    assert!(
        matches!(outcome, Outcome::Fail(why) if why.contains("no rule for output field `energy_ev`"))
    );
}

#[test]
fn an_output_field_without_a_rule_fails_even_a_planned_case() {
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"energy_ev\": 1.0}'";
    let planned = CASE.replace("active", "planned");
    let (outcome, _) = run_case("harness-field-planned", &planned, INPUT, body, honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("harness: invariance: no rule for output field `energy_ev`")),
        "{outcome:?}"
    );
}

#[test]
fn an_input_without_a_cell_cannot_skip_the_rotation() {
    let (outcome, _) = run_case("harness-cell", CASE, "[bands]\ncount = 1\n", GOOD, honest);
    assert!(matches!(outcome, Outcome::Fail(why) if why.contains("no structure.cell_bohr")));
}

#[test]
fn a_nan_tolerance_never_reaches_a_run() {
    let nan = CASE.replace("1e-12", "nan");
    let (outcome, _) = run_case("harness-nan", &nan, INPUT, GOOD, honest);
    assert!(matches!(outcome, Outcome::Fail(why) if why.contains("must be finite")));
}

#[test]
fn a_harness_fault_fails_even_a_planned_case() {
    let planned = CASE.replace("active", "planned");
    let root = tree(
        "harness-fault",
        &[
            ("benchmarks/1_a/case.toml", &planned),
            ("benchmarks/1_a/input.toml", INPUT),
        ],
    );
    let session = Session::new(
        &root,
        &root.join("no-such-engine"),
        Tier::Push,
        &root.join("records"),
    );
    assert!(
        matches!(session.evaluate("1_a", honest), Outcome::Fail(why) if why.contains("harness:"))
    );
}

#[test]
fn a_reference_path_cannot_leave_reference() {
    fn escape(evidence: &Evidence) -> Result<Metrics, String> {
        let golden: f64 = evidence
            .reference("../../../golden.txt")?
            .trim()
            .parse()
            .map_err(|_| "unreadable".to_owned())?;
        let e = evidence.output("input")?.eigenvalues_ha()?[0][0][0];
        Ok(Metrics::from([("error".to_owned(), (e - golden).abs())]))
    }
    let files = [
        ("benchmarks/1_a/input.toml", INPUT),
        ("golden.txt", "0.5\n"),
    ];
    let outcome = evaluate("harness-reference", &files, escape);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("name a file inside reference/")),
        "{outcome:?}"
    );
}

const WIDE: &str = "printf '{\"eigenvalues_ha\": [[[0.5, 0.75, 1.0, 1.25], [0.5, 0.75, 1.0, 1.25]]], \"basis_sizes\": [27, 28]}'";
const OTHER: &str = "schema = 1\n\n[structure]\ncell_bohr = [[7.0, 0.0, 0.0], [0.0, 8.0, 0.0], [0.0, 0.0, 9.0]]\n\n[basis]\ncutoff_ha = 2\n\n[kpoints]\nlist = [[0.5, 0.0, 0.0], [0.0, 0.25, 0.0]]\n\n[bands]\ncount = 3\n";

#[test]
fn a_measure_reads_the_input_and_output_that_it_names() {
    fn reads(evidence: &Evidence) -> Result<Metrics, String> {
        let (b, out) = (evidence.input("b")?, evidence.output("b")?);
        Err(format!(
            "cell {:?} k {:?} cutoff {} bands {} number {} ({}) sizes {:?} ref {}",
            b.cell_bohr()?,
            b.kpoints()?,
            b.cutoff_ha()?,
            b.bands()?,
            b.number("bands.count")?,
            b.number("scf.energy_tolerance_ha").unwrap_err(),
            out.basis_sizes()?,
            evidence.reference("m.json")?.trim()
        ))
    }
    let root = tree(
        "harness-reads",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", INPUT),
            ("benchmarks/1_a/inputs/b.toml", OTHER),
            ("benchmarks/1_a/reference/m.json", "{\"m\": 2}\n"),
        ],
    );
    let bin = engine(&root, WIDE);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", reads);
    let want = "cell [[7.0, 0.0, 0.0], [0.0, 8.0, 0.0], [0.0, 0.0, 9.0]] k [[0.5, 0.0, 0.0], [0.0, 0.25, 0.0]] cutoff 2 bands 3 number 3 (input has no number scf.energy_tolerance_ha) sizes [27, 28] ref {\"m\": 2}";
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(want)),
        "{outcome:?}"
    );
}

fn full_mesh(evidence: &Evidence) -> Result<Metrics, String> {
    #[derive(serde::Deserialize)]
    struct M {
        m: f64,
    }
    let read: M = evidence.full_mesh_reference("input")?;
    Err(format!("m = {}", read.m))
}

#[test]
fn a_full_mesh_reference_needs_an_input_without_symmetry() {
    for (n, (kpoints, json, want)) in [
        ("symmetry = false\n", "{\"m\": 2.5}", "m = 2.5"),
        (
            "",
            "{\"m\": 2.5}",
            "input: ABINIT runs the full k mesh: set kpoints.symmetry = false",
        ),
        (
            "symmetry = true\n",
            "{\"m\": 2.5}",
            "set kpoints.symmetry = false",
        ),
        (
            "symmetry = false\n",
            "{\"n\": 1}",
            "reference/input.json: missing field `m`",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let input = INPUT.replace("[kpoints]\n", &format!("[kpoints]\n{kpoints}"));
        let root = tree(
            &format!("harness-full-mesh-{n}"),
            &[
                ("benchmarks/1_a/case.toml", CASE),
                ("benchmarks/1_a/input.toml", &input),
                ("benchmarks/1_a/reference/input.json", json),
            ],
        );
        let bin = engine(&root, GOOD);
        let outcome =
            Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", full_mesh);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains(want)),
            "{n}: {outcome:?}"
        );
    }
}

#[test]
fn every_perturbed_copy_differs() {
    let band = |b: usize| 0.5 + 0.25 * b as f64;
    let row = format!(
        "[{}]",
        (0..8)
            .map(|b| band(b).to_string())
            .collect::<Vec<_>>()
            .join(", ")
    );
    let rows = vec![row; 8].join(", ");
    let wide = format!("printf '{{\"eigenvalues_ha\": [[{rows}]], \"basis_sizes\": [1]}}'");
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    fn spy(evidence: &Evidence) -> Result<Metrics, String> {
        let values = evidence.output("input")?.eigenvalues_ha()?;
        SEEN.lock().unwrap().push(format!("{values:?}"));
        let error = values[0]
            .iter()
            .flat_map(|k| {
                k.iter()
                    .enumerate()
                    .map(|(b, got)| (got - (0.5 + 0.25 * b as f64)).abs())
            })
            .fold(0.0, f64::max);
        Ok(Metrics::from([("error".to_owned(), error)]))
    }
    let (outcome, _root) = run_case("harness-copies", CASE, INPUT, &wide, spy);
    assert_eq!(outcome, Outcome::Pass);
    let seen = SEEN.lock().unwrap();
    let distinct: BTreeSet<&String> = seen.iter().collect();
    assert_eq!((seen.len(), distinct.len()), (9, 9), "{seen:#?}");
}

#[test]
fn the_runner_reads_its_session_from_the_environment() {
    let root = tree(
        "harness-vars",
        &[
            (
                "benchmarks/1_a/case.toml",
                &CASE.replace("\"push\"", "\"pr\""),
            ),
            ("benchmarks/1_a/input.toml", INPUT),
        ],
    );
    let bin = engine(&root, GOOD).into_os_string();
    let records = root.join("pr-records");
    let vars = |pairs: Vec<(&'static str, OsString)>| {
        move |key: &str| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        }
    };
    assert!(Session::from_vars(&root, vars(vec![])).unwrap().is_none());
    let bad = vars(vec![
        ("ORBITAR_BIN", bin.clone()),
        ("ORBITAR_TIER", "lunch".into()),
    ]);
    assert_eq!(
        Session::from_vars(&root, bad).err(),
        Some("unknown tier lunch".into())
    );
    let seed = vars(vec![
        ("ORBITAR_BIN", bin.clone()),
        ("ORBITAR_SEED", "x".into()),
    ]);
    assert_eq!(
        Session::from_vars(&root, seed).err(),
        Some("ORBITAR_SEED must be a u64".into())
    );
    let push = Session::from_vars(&root, vars(vec![("ORBITAR_BIN", bin.clone())]))
        .unwrap()
        .unwrap();
    assert_eq!(
        push.evaluate("1_a", honest),
        Outcome::Skipped,
        "a pr case is not in push"
    );
    let pr = vars(vec![
        ("ORBITAR_BIN", bin),
        ("ORBITAR_TIER", "pr".into()),
        ("ORBITAR_RECORD_DIR", records.clone().into()),
        ("ORBITAR_SEED", "7".into()),
    ]);
    let session = Session::from_vars(&root, pr).unwrap().unwrap();
    assert_eq!(session.evaluate("1_a", honest), Outcome::Pass);
    let record = std::fs::read_to_string(records.join("1_a.json")).unwrap();
    assert!(
        record.contains("\"seed\":\"7\"") && record.contains("\"seed_fixed\":true"),
        "{record}"
    );
}

#[test]
fn a_record_that_the_harness_writes_passes_verify() {
    let root = tree(
        "harness-verify",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", INPUT),
            ("xtask/src/harness.rs", "// judge\n"),
        ],
    );
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "e2e"], &root).is_empty());
    let bin = engine(&root, &format!("sleep 0.05\n{GOOD}"));
    let records = root.join("records");
    let since = SystemTime::now();
    let outcome = Session::new(&root, &bin, Tier::Push, &records).evaluate("1_a", honest);
    assert_eq!(outcome, Outcome::Pass);
    let findings = xtask::harness::verify(&root, Tier::Push, &records, since, None);
    assert!(findings.is_empty(), "{findings:?}");
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(records.join("1_a.json")).unwrap()).unwrap();
    let line = |path: &str| {
        let bytes = std::fs::read(root.join(path)).unwrap();
        format!("{}  {path}", hex(&Sha256::digest(bytes)))
    };
    let lines = [
        line("benchmarks/1_a/case.toml"),
        line("benchmarks/1_a/input.toml"),
    ];
    let case = hex(&Sha256::digest(lines.join("\n")));
    assert_eq!(
        record["lock"]["case"].as_str(),
        Some(case.as_str()),
        "{record}"
    );
    let spent = record["spent_s"].as_f64().unwrap();
    assert!((0.1..60.0).contains(&spent), "{record}");
}

#[test]
fn skips_and_outcome_names_follow_the_tier_and_status() {
    let retired = CASE.replace("\"active\"", "\"retired\"");
    let (outcome, _root) = run_case("harness-retired", &retired, INPUT, GOOD, honest);
    assert_eq!(outcome, Outcome::Skipped);
    let planned = CASE.replace("\"active\"", "\"planned\"");
    let (outcome, root) = run_case("harness-promote", &planned, INPUT, GOOD, honest);
    assert_eq!(outcome, Outcome::Promote);
    let record = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    assert!(record.contains("\"outcome\":\"promote\""), "{record}");
    fn wrong(_: &Evidence) -> Result<Metrics, String> {
        Err("no code yet".into())
    }
    let (outcome, root) = run_case("harness-planned", &planned, INPUT, GOOD, wrong);
    assert!(matches!(outcome, Outcome::Planned(_)), "{outcome:?}");
    let record = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    assert!(record.contains("\"outcome\":\"planned\""), "{record}");
    let root = tree("harness-no-input", &[("benchmarks/1_a/case.toml", CASE)]);
    let bin = engine(&root, GOOD);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("the case has no input yet")),
        "{outcome:?}"
    );
}

#[test]
fn a_record_names_the_commit_the_toolchain_and_the_host() {
    let root = tree(
        "harness-provenance",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", INPUT),
        ],
    );
    crate::git(&root, &["init", "-q"]);
    crate::git(&root, &["add", "-A"]);
    crate::git(&root, &["commit", "-qm", "case"]);
    let bin = engine(&root, GOOD);
    let records = root.join("records");
    assert_eq!(
        Session::new(&root, &bin, Tier::Push, &records).evaluate("1_a", honest),
        Outcome::Pass
    );
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(records.join("1_a.json")).unwrap()).unwrap();
    let commit = record["commit"].as_str().unwrap();
    assert!(
        commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit()),
        "{record}"
    );
    assert_eq!(
        record["dirty"],
        serde_json::Value::Bool(true),
        "the fake engine is untracked: {record}"
    );
    assert!(
        record["rustc"].as_str().unwrap().starts_with("rustc 1."),
        "{record}"
    );
    assert!(
        record["node"]["host"]["logical_cpus"].as_u64().unwrap() >= 1,
        "{record}"
    );
    assert!(
        !record["node"]["host"]["cpu"].as_str().unwrap().is_empty(),
        "{record}"
    );
}

#[test]
fn an_unreadable_inputs_directory_is_a_harness_fault() {
    let outcome = evaluate(
        "harness-inputs-file",
        &[
            ("benchmarks/1_a/input.toml", INPUT),
            ("benchmarks/1_a/inputs", "not a directory\n"),
        ],
        honest,
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("1_a/inputs: ")),
        "{outcome:?}"
    );
}

#[test]
fn push_is_inside_pr_is_inside_nightly_and_claim_and_node_each_stand_alone() {
    let tiers = [Tier::Push, Tier::Pr, Tier::Nightly, Tier::Claim, Tier::Node];
    let names = ["push", "pr", "nightly", "claim", "node"];
    for (s, &session_tier) in tiers.iter().enumerate() {
        for (c, case_tier) in names.iter().enumerate() {
            let claims = if *case_tier == "claim" {
                "[\"accuracy\", \"speed\"]"
            } else {
                "[\"correctness\"]"
            };
            let case = CASE
                .replace("tier = \"push\"", &format!("tier = \"{case_tier}\""))
                .replace("[\"correctness\"]", claims);
            let root = tree(
                &format!("harness-tiers-{s}-{c}"),
                &[
                    ("benchmarks/1_a/case.toml", &case),
                    ("benchmarks/1_a/input.toml", INPUT),
                ],
            );
            let bin = engine(&root, GOOD);
            let outcome = Session::new(&root, &bin, session_tier, &root.join("records"))
                .evaluate("1_a", honest);
            let included = match session_tier {
                Tier::Claim | Tier::Node => c == s,
                _ => c < 3 && c <= s,
            };
            assert_eq!(
                outcome != Outcome::Skipped,
                included,
                "{case_tier} case in a {:?} run: {outcome:?}",
                names[s]
            );
        }
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

pub fn jth(url: &str, rows: &[(&str, &str, &str, Option<f64>)]) -> String {
    let mut text = format!("url = \"{url}\"\n");
    for functional in ["lda", "pbe"] {
        let upper = functional.to_uppercase();
        text += &format!(
            "\n[{functional}]\nlayout = \"JTH-{upper}-v2.0/{{element}}/{{element}}.xml\"\n\n[{functional}.element]\n"
        );
        for (_, element, pinned, hint) in rows.iter().filter(|row| row.0 == functional) {
            let sha = hex(&Sha256::digest(pinned));
            let hint = hint.map_or(String::new(), |h| format!(", cutoff_hint_ha = {h:?}"));
            text += &format!("{element} = {{ sha256 = \"{sha}\"{hint} }}\n");
        }
    }
    text
}

const PROBE_INPUT: &str = "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\n[basis]\ncutoff_ha = 1.0\n\n[kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[xc]\nfunctional = \"pbe\"\n\n[probe]\nterm = \"ewald\"\ncharges = [1.0, -1]\nexponents = [1.5, 2]\npositions_fractional = [[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]]\npoints = [[0.5, 0.5, 0.0, 0.0, 0.0]]\n";
const PROBE_ENGINE: &str = "printf '{\"ewald_energy_ha\": -1.5, \"hartree_energy_ha\": 0.25, \"xc_energy_density_ha_bohr3\": [[-0.5, -0.0625]]}'";

fn probe_reads(evidence: &Evidence) -> Result<Metrics, String> {
    let (input, output) = (evidence.input("input")?, evidence.output("input")?);
    let read = (
        input.probe_charges()?,
        input.probe_exponents()?,
        input.probe_positions_fractional()?,
        input.probe_points()?,
        input.functional()?.to_owned(),
    );
    let want = (
        vec![1.0, -1.0],
        vec![1.5, 2.0],
        vec![[0.0; 3], [0.5; 3]],
        vec![vec![0.5, 0.5, 0.0, 0.0, 0.0]],
        "pbe".to_owned(),
    );
    let outputs = (
        output.ewald_energy_ha()?,
        output.hartree_energy_ha()?,
        output.xc_energy_density_ha_bohr3()?,
    );
    let error = if read == want && outputs == (-1.5, 0.25, vec![[-0.5, -0.0625]]) {
        0.0
    } else {
        1.0
    };
    Ok(Metrics::from([("error".to_owned(), error)]))
}

#[test]
fn the_probe_accessors_read_the_input_and_the_three_probe_fields() {
    let (outcome, _root) = run_case(
        "harness-probe",
        CASE,
        PROBE_INPUT,
        PROBE_ENGINE,
        probe_reads,
    );
    assert_eq!(outcome, Outcome::Pass);
}

const SCF_ENGINE: &str = "printf '{\"basis_sizes\": [30, 30], \"eigenvalues_ha\": [[[-0.5, 0.25], [-0.75, 0.5]]], \"kpoints_fractional\": [[0.5, 0.0, 0.25], [0.0, 0.0, 0.0]], \"kpoint_weights\": [0.75, 0.25], \"free_energy_ha\": -7.5, \"internal_energy_ha\": -7.25, \"kinetic_energy_ha\": 3.0, \"hartree_energy_ha\": 0.5, \"xc_energy_ha\": -4.5, \"ewald_energy_ha\": -8.25, \"psp_core_energy_ha\": 0.25, \"local_energy_ha\": -2.5, \"one_center_energy_ha\": 4.25, \"smearing_energy_ha\": -0.25, \"fermi_level_ha\": 0.125, \"occupations\": [[[2.0, 0.0], [2.0, 0.0]]], \"magnetization_bohr_magneton\": 1.5, \"scf_steps\": 12}'";

fn scf_reads(evidence: &Evidence) -> Result<Metrics, String> {
    let output = evidence.output("input")?;
    let names = [
        "free",
        "internal",
        "kinetic",
        "hartree",
        "xc",
        "ewald",
        "psp_core",
        "local",
        "one_center",
        "smearing",
    ];
    let energies: Vec<f64> = names
        .iter()
        .map(|n| output.energy_ha(&format!("{n}_energy_ha")))
        .collect::<Result<_, _>>()?;
    let read = (
        energies,
        output.energies()?,
        output.fermi_level_ha()?,
        output.magnetization_bohr_magneton()?,
        output.scf_steps()?,
        output.kpoints_fractional()?,
        output.kpoint_weights()?,
        output.occupations()?,
        output.eigenvalues_ha()?,
        output.basis_sizes()?,
    );
    let values = vec![-7.5, -7.25, 3.0, 0.5, -4.5, -8.25, 0.25, -2.5, 4.25, -0.25];
    let by_name: std::collections::BTreeMap<String, f64> = (names.iter().zip(&values))
        .map(|(n, &v)| (format!("{n}_energy_ha"), v))
        .collect();
    let want = (
        values,
        by_name,
        0.125,
        1.5,
        12,
        vec![[0.5, 0.0, 0.25], [0.0, 0.0, 0.0]],
        vec![0.75, 0.25],
        vec![vec![vec![2.0, 0.0], vec![2.0, 0.0]]],
        vec![vec![vec![-0.5, 0.25], vec![-0.75, 0.5]]],
        vec![30, 30],
    );
    let differences = (
        output.largest_energy_difference(&output, 1.0)?,
        output.largest_energy_difference(&output, 2.0)?,
    );
    let refused = output.energy_ha("fermi_level_ha").is_err();
    let input = evidence.input("input")?;
    let (solver, symmetry, threads) = (input.solver()?, input.symmetry()?, input.threads()?);
    let error = if read == want
        && differences == (0.0, 8.25)
        && refused
        && solver.is_none()
        && symmetry.is_none()
        && threads.is_none()
    {
        0.0
    } else {
        1.0
    };
    Ok(Metrics::from([("error".to_owned(), error)]))
}

#[test]
fn the_scf_accessors_read_every_scf_field() {
    let (outcome, _root) = run_case("harness-scf", CASE, PROBE_INPUT, SCF_ENGINE, scf_reads);
    assert_eq!(outcome, Outcome::Pass);
}

fn solver_reads(evidence: &Evidence) -> Result<Metrics, String> {
    let input = evidence.input("input")?;
    let steps = evidence.output("input")?.scf_steps()?;
    let error = if input.solver()? == Some("dense")
        && input.symmetry()? == Some(false)
        && input.threads()?.map(|n| n.get()) == Some(2)
        && steps == 12
    {
        0.0
    } else {
        1.0
    };
    Ok(Metrics::from([("error".to_owned(), error)]))
}

#[test]
fn the_solver_symmetry_and_threads_accessors_read_their_keys() {
    let input = format!("{PROBE_INPUT}\n[bands]\nsolver = \"dense\"\n\n[parallel]\nthreads = 2\n")
        .replace("[kpoints]\n", "[kpoints]\nsymmetry = false\n");
    let (outcome, _root) = run_case("harness-solver", CASE, &input, SCF_ENGINE, solver_reads);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn a_malformed_xc_field_fails_with_its_shape() {
    let body = "printf '{\"ewald_energy_ha\": -1.5, \"hartree_energy_ha\": 0.25, \"xc_energy_density_ha_bohr3\": [[-0.5]]}'";
    let (outcome, _root) = run_case("harness-probe-shape", CASE, PROBE_INPUT, body, probe_reads);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("[point][2] numbers")),
        "{outcome:?}"
    );
}

#[test]
fn the_worst_of_two_metrics_keeps_a_nan() {
    assert_eq!(xtask::harness::worst(1.0, 2.0), 2.0);
    assert_eq!(xtask::harness::worst(2.0, 1.0), 2.0);
    assert!(xtask::harness::worst(f64::NAN, 1.0).is_nan());
    assert!(xtask::harness::worst(1.0, f64::NAN).is_nan());
}

pub const SI: &str = r#"{"Si-X/Diamond": {"cell": [[0.0, 2.7351025696286, 2.7351025696286], [2.7351025696286, 0.0, 2.7351025696286], [2.7351025696286, 2.7351025696286, 0.0]], "sites": [["Si", [0.0, 0.0, 0.0]], ["Si", [1.3675512848143, 1.3675512848143, 1.3675512848143]]], "kinds": {"Si": "Si"}}}"#;
pub const MINUS_ONE: &str = "printf '{\"free_energy_ha\": -1.0}'";

pub fn si_tree(name: &str, case: &str) -> (Tree, PathBuf) {
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", case),
            ("benchmarks/1_a/crystals.json", SI),
            ("benchmarks/1_a/inputs/b.toml", INPUT),
            ("remote/JTH-PBE-v2.0/Si/Si.xml", "si"),
        ],
    );
    let url = format!("file://{}/remote", root.display());
    let table = jth(&url, &[("pbe", "Si", "si", Some(10.0))]);
    crate::write(&root, "datasets/jth-2.0.toml", &table);
    let bin = engine(&root, MINUS_ONE);
    (root, bin)
}

pub fn minus_one(evidence: &Evidence) -> Result<Metrics, String> {
    let mut error: f64 = 0.0;
    for name in evidence.names() {
        error = error.max((evidence.output(name)?.energy_ha("free_energy_ha")? + 1.0).abs());
    }
    Ok(Metrics::from([("error".to_owned(), error)]))
}

fn filtered(name: &str, selectors: &[&str]) -> (Vec<String>, Outcome, serde_json::Value) {
    static SEEN: Mutex<Vec<String>> = Mutex::new(Vec::new());
    static ONE: Mutex<()> = Mutex::new(());
    let _one = ONE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    fn spy(evidence: &Evidence) -> Result<Metrics, String> {
        *SEEN.lock().unwrap() = evidence.names().into_iter().map(str::to_owned).collect();
        minus_one(evidence)
    }
    let (root, bin) = si_tree(name, &CASE.replace("\"push\"", "\"node\""));
    let selectors = selectors.iter().map(|s| (*s).to_owned()).collect();
    let session =
        Session::new(&root, &bin, Tier::Node, &root.join("records")).with_filter(selectors);
    let mut guard = SEEN.lock().unwrap();
    guard.clear();
    drop(guard);
    let outcome = session.evaluate("1_a", spy);
    let record = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (
        SEEN.lock().unwrap().clone(),
        outcome,
        serde_json::from_str(&record).unwrap(),
    )
}

#[test]
fn a_filter_runs_exactly_the_inputs_that_it_names_and_the_record_lists_them() {
    let (names, outcome, record) = filtered("filter-volume", &["Si-X/Diamond@0.96", "b"]);
    assert_eq!(names, ["b", "Si-X/Diamond@0.96"]);
    assert_eq!(outcome, Outcome::Shard);
    assert_eq!(record["outcome"], "shard", "{record}");
    assert_eq!(
        record["inputs"],
        serde_json::json!(["b", "Si-X/Diamond@0.96"])
    );
    assert_eq!(
        record["filter"],
        serde_json::json!(["Si-X/Diamond@0.96", "b"])
    );
    let (names, outcome, _) = filtered("filter-system", &["Si-X/Diamond"]);
    let seven: Vec<String> = ["0.94", "0.96", "0.98", "1.00", "1.02", "1.04", "1.06"]
        .map(|s| format!("Si-X/Diamond@{s}"))
        .into();
    assert_eq!((names, outcome), (seven, Outcome::Shard));
}

#[test]
fn an_element_selector_picks_each_crystal_of_that_element() {
    let (names, outcome, _) = filtered("filter-element", &["Si"]);
    assert_eq!(names.len(), 7, "{names:?}");
    assert!(names.iter().all(|n| n.starts_with("Si-X/Diamond@")));
    assert_eq!(outcome, Outcome::Shard);
}

#[test]
fn a_candidate_table_runs_only_in_the_node_tier() {
    let root = tree("harness-table-var", &[]);
    let vars = |tier: &'static str| {
        move |key: &str| match key {
            "ORBITAR_BIN" => Some(OsString::from("engine")),
            "ORBITAR_TIER" => Some(OsString::from(tier)),
            "ORBITAR_DATASET_TABLE" => Some(OsString::from("t.toml")),
            _ => None,
        }
    };
    assert_eq!(
        Session::from_vars(&root, vars("pr")).err(),
        Some("ORBITAR_DATASET_TABLE runs only in the node tier, where the tuning case runs, not in pr".into())
    );
    assert!(Session::from_vars(&root, vars("node")).is_ok_and(|s| s.is_some()));
}

#[test]
fn a_filter_that_names_no_input_fails_by_name() {
    let (_, outcome, _) = filtered("filter-unknown", &["Si-X/Diamond@0.95", "b", "Ge-X/FCC"]);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("names no input of this case: Si-X/Diamond@0.95, Ge-X/FCC")),
        "{outcome:?}"
    );
}

#[test]
fn a_filtered_run_never_promotes_or_passes_the_case() {
    let planned = CASE
        .replace("\"active\"", "\"planned\"")
        .replace("\"push\"", "\"node\"");
    let (root, bin) = si_tree("filter-planned", &planned);
    let session = Session::new(&root, &bin, Tier::Node, &root.join("records"))
        .with_filter(vec!["b".into(), "Si-X/Diamond".into()]);
    assert_eq!(session.evaluate("1_a", minus_one), Outcome::Shard);
}

#[test]
fn a_filter_runs_only_in_the_node_tier() {
    for tier in [Tier::Push, Tier::Pr, Tier::Nightly, Tier::Claim] {
        let name = format!("{tier:?}").to_lowercase();
        let case = CASE.replace("\"push\"", &format!("\"{name}\""));
        let (root, bin) = si_tree(&format!("filter-tier-{name}"), &case);
        let session =
            Session::new(&root, &bin, tier, &root.join("records")).with_filter(vec!["b".into()]);
        let outcome = session.evaluate("1_a", minus_one);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("ORBITAR_CASE_FILTER runs only in the node tier, where case 10's shards run; a {name} run covers every input"))),
            "{outcome:?}"
        );
    }
}

#[test]
fn an_empty_filter_names_no_selector() {
    let (_, outcome, _) = filtered("filter-empty", &[]);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("ORBITAR_CASE_FILTER names no selector")),
        "{outcome:?}"
    );
}

#[test]
fn the_filter_comes_from_comma_separated_selectors() {
    let root = tree("filter-vars", &[("benchmarks/1_a/case.toml", CASE)]);
    let vars = |filter: &'static str| {
        move |key: &str| match key {
            "ORBITAR_BIN" => Some(OsString::from("/bin/true")),
            "ORBITAR_CASE_FILTER" => Some(OsString::from(filter)),
            "ORBITAR_TIER" => Some(OsString::from("node")),
            _ => None,
        }
    };
    assert_eq!(
        Session::from_vars(&root, vars("a,,b")).err(),
        Some(
            "ORBITAR_CASE_FILTER `a,,b` holds an empty selector; separate input names by commas"
                .into()
        )
    );
    assert!(Session::from_vars(&root, vars("a, b")).unwrap().is_some());
    let push = |key: &str| match key {
        "ORBITAR_TIER" => Some(OsString::from("push")),
        key => vars("a")(key),
    };
    assert_eq!(
        Session::from_vars(&root, push).err(),
        Some("ORBITAR_CASE_FILTER runs only in the node tier, where case 10's shards run; a push run covers every input".into())
    );
}

fn held_out(name: &str, held: Option<&Path>) -> (Outcome, serde_json::Value) {
    use rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let root = rivals_tree(name, INPUT, TUNED, PW, ABINIT);
    crate::write(&root, "benchmarks/1_a/reference/input.json", "{}");
    let bin = speed::fake(&root, "step -2");
    let mut session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    if let Some(held) = held {
        let held = if held.is_relative() {
            root.join(held)
        } else {
            held.to_owned()
        };
        session = session.with_held_out(&held);
    }
    let outcome = session.evaluate("1_a", no_held_out);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

fn no_held_out(evidence: &Evidence) -> Result<Metrics, String> {
    for name in evidence.names() {
        evidence.reference(&format!("{name}.json"))?;
    }
    if let Some(name) = evidence
        .names()
        .into_iter()
        .find(|n| n.starts_with("held-out-"))
    {
        return Err(format!("the measure saw {name}"));
    }
    speed::both(evidence)
}

#[test]
fn a_claim_beside_rivals_runs_each_held_out_variant_and_names_its_hash_and_the_tuned_inputs() {
    let held = std::env::temp_dir().join(format!("xtask-variants-{}", std::process::id()));
    let variant = rival::INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5");
    crate::write(&held, "1_a/input.toml", &variant);
    let (_, record) = held_out("held-out-runs", Some(&held));
    let _ = std::fs::remove_dir_all(&held);
    assert_eq!(
        record["inputs"],
        serde_json::json!(["input", "held-out-input"]),
        "{record}"
    );
    let timed: Vec<&str> = (record["speed"].as_array().unwrap().iter())
        .filter(|b| b["code"] == "orbitar")
        .map(|b| b["input"].as_str().unwrap())
        .collect();
    assert_eq!(timed, ["input", "held-out-input"], "{record}");
    assert_eq!(
        record["rotation_s"].as_array().unwrap().len(),
        1,
        "{record}"
    );
    let failures = record["failures"].to_string();
    assert!(
        !failures.contains("the measure saw") && !failures.contains("missing; the claim stops"),
        "{record}"
    );
    let sha = hex(&Sha256::digest(&variant));
    assert_eq!(
        record["held_out"],
        serde_json::json!([{
            "input": "held-out-input",
            "base": "input",
            "geometry": { "cell_strain": 0.0, "largest_shift_bohr": 0.0 },
            "sha256": sha
        }])
    );
    assert_eq!(record["tuned"], serde_json::json!(["input"]), "{record}");
    let (_, record) = held_out("held-out-unset", None);
    assert_eq!(record["held_out"], serde_json::json!([]), "{record}");
}

#[test]
fn a_held_out_variant_inherits_each_rival_skip_of_the_input_whose_stem_it_holds() {
    use rival::{ABINIT, INPUT, RIVALS, TUNED, rivals_tree};
    let root = rivals_tree("held-out-skip", INPUT, TUNED, "exit 7", ABINIT);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "density_ratio = 8.0\n",
            "density_ratio = 8.0\nskip_cells = [{ cell = \"benchmarks/1_a/input.toml\", reason = \"no dataset\" }]\n",
        ),
    );
    crate::write(&root, "benchmarks/1_a/reference/input.json", "{}");
    let held = std::env::temp_dir().join(format!("xtask-variants-skip-{}", std::process::id()));
    crate::write(
        &held,
        "1_a/input.toml",
        &INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5"),
    );
    let bin = speed::fake(&root, "step -2");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"))
        .with_held_out(&held);
    let outcome = session.evaluate("1_a", no_held_out);
    let _ = std::fs::remove_dir_all(&held);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        !record["failures"].to_string().contains("qe"),
        "{outcome:?}: {record}"
    );
    let timed = |code: &str| -> Vec<&str> {
        (record["speed"].as_array().unwrap().iter())
            .filter(|b| b["code"] == code)
            .map(|b| b["input"].as_str().unwrap())
            .collect()
    };
    assert_eq!(timed("qe"), Vec::<&str>::new(), "{record}");
    assert_eq!(timed("abinit"), ["input", "held-out-input"], "{record}");
    assert_eq!(timed("orbitar"), ["input", "held-out-input"], "{record}");
    assert_eq!(
        record["skipped"],
        serde_json::json!([
            { "code": "qe", "input": "input", "reason": "no dataset" },
            { "code": "qe", "input": "held-out-input", "reason": "no dataset" }
        ]),
        "{record}"
    );
}

fn skipping_qe_for(root: &Tree, cells: &[&str]) {
    let skips: Vec<String> = (cells.iter())
        .map(|c| format!("{{ cell = \"{c}\", reason = \"no dataset\" }}"))
        .collect();
    crate::write(
        root,
        "benchmarks/RIVALS.toml",
        &rival::RIVALS.replace(
            "density_ratio = 8.0\n",
            &format!("density_ratio = 8.0\nskip_cells = [{}]\n", skips.join(", ")),
        ),
    );
    let tuned = std::fs::read_to_string(root.join("benchmarks/TUNED.toml")).unwrap();
    crate::write(
        root,
        "benchmarks/TUNED.toml",
        &rival::with_layout_policies(root, &tuned),
    );
}

fn timed_by<'a>(record: &'a serde_json::Value, code: &str) -> Vec<&'a str> {
    (record["speed"].as_array().into_iter().flatten())
        .filter(|b| b["code"] == code)
        .map(|b| b["input"].as_str().unwrap())
        .collect()
}

fn claim_with_variants(root: &Tree, id: &str, held: &Path) -> (Outcome, serde_json::Value) {
    let bin = speed::fake(root, "step -2");
    let session = Session::new(root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"))
        .with_held_out(held);
    let outcome = session.evaluate(id, no_held_out);
    let text = std::fs::read_to_string(root.join(format!("records/{id}.json"))).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

fn within_the_generators(name: &str, input: &str, variant: &str) -> bool {
    use rival::{ABINIT, PW, TUNED, rivals_tree};
    let root = rivals_tree(name, input, TUNED, PW, ABINIT);
    crate::write(&root, "benchmarks/1_a/reference/input.json", "{}");
    let held = std::env::temp_dir().join(format!("xtask-{name}-variants-{}", std::process::id()));
    crate::write(&held, "1_a/input.toml", variant);
    let (_, record) = claim_with_variants(&root, "1_a", &held);
    let _ = std::fs::remove_dir_all(&held);
    let entry = &record["held_out"][0];
    assert_eq!(entry["input"], "held-out-input", "{record}");
    match &entry["base"] {
        serde_json::Value::Null => {
            assert!(
                entry["geometry"]["beyond_the_generators"].is_string(),
                "{entry}"
            );
            false
        }
        base => {
            assert_eq!(base, "input", "{entry}");
            true
        }
    }
}

#[test]
fn a_held_out_variant_may_strain_its_cell_or_move_an_atom_up_to_each_bound_and_no_further() {
    use rival::INPUT;
    let cell = "cell_bohr = [[0.0, 5.0, 5.0], [5.0, 0.0, 5.0], [5.0, 5.0, 0.0]]";
    let site = "[[0.0, 0.0, 0.25]]";
    for (n, variant, within) in [
        (
            0,
            INPUT.replace(
                cell,
                "cell_bohr = [[4e-9, 5.0, 5.0], [5.0, 0.0, 5.0], [5.0, 5.0, 0.0]]",
            ),
            true,
        ),
        (
            1,
            INPUT.replace(
                cell,
                "cell_bohr = [[5e-9, 5.0, 5.0], [5.0, 0.0, 5.0], [5.0, 5.0, 0.0]]",
            ),
            true,
        ),
        (
            2,
            INPUT.replace(
                cell,
                "cell_bohr = [[6e-9, 5.0, 5.0], [5.0, 0.0, 5.0], [5.0, 5.0, 0.0]]",
            ),
            false,
        ),
        (
            3,
            INPUT.replace(site, "[[0.018897261328856257, 0.0, 0.2688972613288566]]"),
            true,
        ),
        (4, INPUT.replace(site, "[[0.019, 0.0, 0.269]]"), false),
        (5, INPUT.replace(site, "[[0.0, 0.0, 1.25]]"), true),
    ] {
        assert_ne!(variant, INPUT, "{n}");
        assert_eq!(
            within_the_generators(&format!("held-out-bound-{n}"), INPUT, &variant),
            within,
            "{n}"
        );
    }
}

#[test]
fn each_atom_of_a_held_out_variant_pairs_with_its_own_base_site() {
    use rival::INPUT;
    let two = INPUT
        .replace("species = [\"Si\"]", "species = [\"Si\", \"Si\"]")
        .replace("[[0.0, 0.0, 0.25]]", "[[0.0, 0.0, 0.25], [0.5, 0.5, 0.75]]");
    let crowded = two.replace("[0.5, 0.5, 0.75]]", "[0.0, 0.0, 0.2501]]");
    assert_ne!(crowded, two);
    assert!(!within_the_generators("held-out-pairs", &two, &crowded));
}

#[test]
fn a_held_out_variant_must_name_an_input_of_its_case_and_hold_its_species_mesh_spin_and_occupations()
 {
    use rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let other = INPUT.replace("mesh = [2, 2, 2]", "mesh = [3, 3, 3]");
    assert_ne!(other, INPUT);
    for (n, (file, variant, why)) in [
        (
            "nio.toml",
            INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5"),
            "1_a has no input nio",
        ),
        (
            "input.toml",
            other,
            "not a variant of 1_a's input input: its kpoints differs",
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let root = rivals_tree(&format!("held-out-misnamed-{n}"), INPUT, TUNED, PW, ABINIT);
        crate::write(&root, "benchmarks/1_a/reference/input.json", "{}");
        let held = std::env::temp_dir().join(format!(
            "xtask-variants-misnamed-{n}-{}",
            std::process::id()
        ));
        crate::write(&held, &format!("1_a/{file}"), &variant);
        let (outcome, _) = claim_with_variants(&root, "1_a", &held);
        let _ = std::fs::remove_dir_all(&held);
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(why)),
            "{n}: {outcome:?}"
        );
    }
}

fn largest_generator_rattle_angstrom() -> f64 {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
    let mut largest = 0.0_f64;
    for case in ["11_speed_scaling", "14_set_w"] {
        let path = root.join(format!("benchmarks/{case}/reference/cells.py"));
        let text = std::fs::read_to_string(path).unwrap();
        for call in text.split("rattled(").skip(1) {
            let line = call.lines().next().unwrap_or_default();
            if line.starts_with("atoms, amplitude") {
                continue;
            }
            let at = line.find(", 0.").unwrap();
            let rest = &line[at + 2..];
            let amplitude: f64 = rest[..rest.find(',').unwrap()].parse().unwrap();
            largest = largest.max(amplitude);
        }
    }
    largest
}

fn moved_along_x(bohr: f64, scale: f64) -> String {
    let f = bohr / 10.0;
    rival::INPUT
        .replace(
            "positions_fractional = [[0.0, 0.0, 0.25]]",
            &format!("positions_fractional = [[{}, {f}, {}]]", -f, 0.25 + f),
        )
        .replace(
            "cell_bohr = [[0.0, 5.0, 5.0], [5.0, 0.0, 5.0], [5.0, 5.0, 0.0]]",
            &format!(
                "cell_bohr = [[0.0, {0}, {0}], [{0}, 0.0, {0}], [{0}, {0}, 0.0]]",
                5.0 * scale
            ),
        )
}

#[test]
fn a_variant_inherits_its_bases_skip_and_calibration_only_within_the_generators_rattle_and_no_strain()
 {
    use rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let bound_bohr = 2.0 * largest_generator_rattle_angstrom() / 0.529_177_208_59;
    for (n, (variant, inherits)) in [
        (moved_along_x(0.9 * bound_bohr, 1.0), true),
        (moved_along_x(1.1 * bound_bohr, 1.0), false),
        (moved_along_x(0.0, 1.0 + 1e-6), false),
        (moved_along_x(0.0, 10.0), false),
    ]
    .into_iter()
    .enumerate()
    {
        assert_ne!(variant, INPUT, "{n}");
        let root = rivals_tree(&format!("held-out-bound-{n}"), INPUT, TUNED, PW, ABINIT);
        skipping_qe_for(&root, &["benchmarks/1_a/input.toml"]);
        crate::write(&root, "benchmarks/1_a/reference/input.json", "{}");
        let held =
            std::env::temp_dir().join(format!("xtask-variants-bound-{n}-{}", std::process::id()));
        crate::write(&held, "1_a/input.toml", &variant);
        let (outcome, record) = claim_with_variants(&root, "1_a", &held);
        let _ = std::fs::remove_dir_all(&held);
        let entry = &record["held_out"][0];
        let abinit = (record["speed"].as_array().into_iter().flatten())
            .find(|b| b["code"] == "abinit" && b["input"] == "held-out-input")
            .map(|b| b["accuracy"].clone())
            .unwrap_or_default();
        let own = "held-out held-out-input: no calibration of its own; run `cargo xtask rivals calibrate --cell benchmarks/1_a/inputs/held-out-input.toml`";
        let refused = matches!(&outcome, Outcome::Fail(f) if f.contains(own));
        match inherits {
            true => {
                assert_eq!(entry["base"], "input", "{n}: {record}");
                assert_eq!(timed_by(&record, "qe"), Vec::<&str>::new(), "{n}: {record}");
                assert_eq!(
                    abinit["basis"], "borrowed from benchmarks/1_a/input.toml",
                    "{n}: {record}"
                );
                assert_eq!(abinit["e_ha_per_atom"], 1e-6, "{n}: {record}");
                assert!(!refused, "{n}: {outcome:?}");
            }
            false => {
                assert!(entry["base"].is_null(), "{n}: {record}");
                assert!(
                    entry["geometry"]["beyond_the_generators"].is_string(),
                    "{n}: {record}"
                );
                assert_eq!(timed_by(&record, "qe"), ["held-out-input"], "{n}: {record}");
                assert!(
                    abinit["basis"].is_null() && abinit["e_ha_per_atom"].is_null(),
                    "{n}: {record}"
                );
                assert!(refused, "{n}: {outcome:?}");
            }
        }
    }
}

#[test]
fn a_committed_input_whose_name_starts_like_a_variants_inherits_no_skip() {
    use rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let root = rivals_tree("held-out-committed", INPUT, TUNED, PW, ABINIT);
    skipping_qe_for(&root, &["benchmarks/1_a/input.toml"]);
    let committed = INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5");
    crate::write(
        &root,
        "benchmarks/1_a/inputs/held-out-input.toml",
        &committed,
    );
    for name in ["input", "held-out-input"] {
        crate::write(
            &root,
            &format!("benchmarks/1_a/reference/{name}.json"),
            "{}",
        );
    }
    let bin = speed::fake(&root, "step -2");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    session.evaluate("1_a", speed::both);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    let record: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(timed_by(&record, "qe"), ["held-out-input"], "{record}");
}

#[test]
fn variants_of_the_same_stem_in_two_cases_each_inherit_their_own_cases_skip() {
    use rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let root = rivals_tree("held-out-two-cases", INPUT, TUNED, PW, ABINIT);
    skipping_qe_for(&root, &["benchmarks/1_a/input.toml"]);
    let case = std::fs::read_to_string(root.join("benchmarks/1_a/case.toml")).unwrap();
    crate::write(&root, "benchmarks/2_b/case.toml", &case);
    crate::write(&root, "benchmarks/2_b/input.toml", INPUT);
    let held = std::env::temp_dir().join(format!("xtask-variants-two-{}", std::process::id()));
    let variant = INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5");
    for id in ["1_a", "2_b"] {
        crate::write(
            &root,
            &format!("benchmarks/{id}/reference/input.json"),
            "{}",
        );
        crate::write(&held, &format!("{id}/input.toml"), &variant);
    }
    let (_, first) = claim_with_variants(&root, "1_a", &held);
    let (_, second) = claim_with_variants(&root, "2_b", &held);
    let _ = std::fs::remove_dir_all(&held);
    assert_eq!(timed_by(&first, "qe"), Vec::<&str>::new(), "{first}");
    assert_eq!(
        timed_by(&second, "qe"),
        ["input", "held-out-input"],
        "{second}"
    );
}

#[test]
fn a_held_out_variant_with_the_bytes_of_a_case_input_or_a_tuning_cell_is_refused() {
    use rival::{ABINIT, INPUT, PW, RIVALS, TUNED, rivals_tree};
    let cell = INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 2.5");
    for (variant, same) in [
        (INPUT.to_owned(), "input input"),
        (cell.clone(), "benchmarks/9_x/cell.toml"),
    ] {
        let root = rivals_tree("held-out-copy", INPUT, TUNED, PW, ABINIT);
        crate::write(&root, "benchmarks/9_x/cell.toml", &cell);
        crate::write(
            &root,
            "benchmarks/RIVALS.toml",
            &RIVALS.replace(
                "layout_cells = [\"benchmarks/1_a/input.toml\"]",
                "layout_cells = [\"benchmarks/1_a/input.toml\", \"benchmarks/9_x/cell.toml\"]",
            ),
        );
        let held = std::env::temp_dir().join(format!("xtask-variants-copy-{}", std::process::id()));
        crate::write(&held, "1_a/x.toml", &variant);
        let bin = speed::fake(&root, "step -2");
        let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
            .with_rivals(&root.join("rivals"))
            .with_held_out(&held);
        let outcome = session.evaluate("1_a", speed::both);
        let _ = std::fs::remove_dir_all(&held);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("x.toml: the same bytes as {same}; a held-out variant is new"))),
            "{same}: {outcome:?}"
        );
    }
}

#[test]
fn held_out_variants_stay_outside_the_repository_and_hold_at_least_one_input() {
    let (inside, _) = held_out("held-out-inside", Some(Path::new("benchmarks")));
    assert!(
        matches!(&inside, Outcome::Fail(why) if why.contains("held-out variants stay outside the repository")),
        "{inside:?}"
    );
    let held = std::env::temp_dir().join(format!("xtask-variants-empty-{}", std::process::id()));
    std::fs::create_dir_all(held.join("1_a")).unwrap();
    let (empty, _) = held_out("held-out-empty", Some(&held));
    let _ = std::fs::remove_dir_all(&held);
    assert!(
        matches!(&empty, Outcome::Fail(why) if why.contains("no held-out <stem>.toml for 1_a")),
        "{empty:?}"
    );
}

fn force_reads(evidence: &Evidence) -> Result<Metrics, String> {
    let positions = evidence.input("input")?.positions_fractional()?;
    let forces = evidence.output("input")?.forces_ha_bohr()?;
    let error = if positions == [[0.0, 0.0, 0.0], [0.25, 0.5, 0.75]] && forces == [[0.0; 3]; 2] {
        0.0
    } else {
        1.0
    };
    Ok(Metrics::from([("error".to_owned(), error)]))
}

#[test]
fn the_positions_and_forces_accessors_read_their_fields() {
    let input = PROBE_INPUT.replace(
        "[basis]",
        "positions_fractional = [[0.0, 0.0, 0.0], [0.25, 0.5, 0.75]]\n\n[basis]",
    );
    let engine = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"forces_ha_bohr\": [[0.0, 0.0, 0.0], [0.0, 0.0, 0.0]]}'";
    let (outcome, _root) = run_case("harness-forces", CASE, &input, engine, force_reads);
    assert_eq!(outcome, Outcome::Pass);
}

fn stress_reads(evidence: &Evidence) -> Result<Metrics, String> {
    let stress = evidence.output("input")?.stress_ha_bohr3()?;
    let pressure = [[0.1, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]];
    let error = if stress == pressure { 0.0 } else { 1.0 };
    Ok(Metrics::from([("error".to_owned(), error)]))
}

#[test]
fn the_stress_accessor_reads_its_field() {
    let engine = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"stress_ha_bohr3\": [[0.1, 0.0, 0.0], [0.0, 0.1, 0.0], [0.0, 0.0, 0.1]]}'";
    let (outcome, _root) = run_case("harness-stress", CASE, PROBE_INPUT, engine, stress_reads);
    assert_eq!(outcome, Outcome::Pass);
}

fn apart(evidence: &Evidence) -> Result<Metrics, String> {
    let keys = ["structure.positions_fractional", "outputs"];
    let at = evidence.input("input")?;
    Err(format!(
        "moved {} changed {}",
        evidence.input("moved")?.equal_apart_from(&at, &keys),
        evidence.input("changed")?.equal_apart_from(&at, &keys),
    ))
}

#[test]
fn equal_apart_from_ignores_only_the_named_keys() {
    let centre = format!("{INPUT}\n[outputs]\nforces = true\n").replace(
        "[basis]",
        "positions_fractional = [[0.0, 0.0, 0.0]]\n\n[basis]",
    );
    let moved = INPUT.replace(
        "[basis]",
        "positions_fractional = [[0.1, 0.0, 0.0]]\n\n[basis]",
    );
    let changed = moved.replace("cutoff_ha = 1.0", "cutoff_ha = 2.0");
    let root = tree(
        "harness-apart",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", &centre),
            ("benchmarks/1_a/inputs/moved.toml", &moved),
            ("benchmarks/1_a/inputs/changed.toml", &changed),
        ],
    );
    let bin = engine(&root, GOOD);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", apart);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("moved true changed false")),
        "{outcome:?}"
    );
}

#[test]
fn case_6_refuses_an_output_with_fewer_bands_than_abinit() {
    let input = INPUT.replace(
        "list = [[0.0, 0.0, 0.0]]",
        "list = [[0.0, 0.0, 0.0]]\nsymmetry = false",
    );
    let reference = "{\"free_energy_ha\": -1.0, \"fermi_level_ha\": 0.1, \"kpoints_fractional\": [[0.0, 0.0, 0.0]], \
                     \"eigenvalues_ha\": [[[0.5, 0.7]]]}";
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"free_energy_ha\": -1.0, \
                  \"fermi_level_ha\": 0.1, \"kpoints_fractional\": [[0.0, 0.0, 0.0]]}'";
    let root = tree(
        "harness-case6-bands",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", &input),
            ("benchmarks/1_a/reference/input.json", reference),
        ],
    );
    let bin = engine(&root, body);
    let outcome = Session::new(&root, &bin, Tier::Push, &root.join("records"))
        .evaluate("1_a", abinit_jth::measure);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("input: 1 bands at [0.0, 0.0, 0.0], 2 in ABINIT")),
        "{outcome:?}"
    );
}

#[test]
fn a_discretization_record_with_one_zero_grid_or_one_malformed_signature_is_refused() {
    let record = |fine: &str, waves: &str| {
        let text = format!(
            r#"{{"operations": [], "coarse_grid": [8, 8, 8], "fine_grid": {fine}, "plane_waves": "{waves}",
                "density_spheres": ["0123456789abcdef", "fedcba9876543210"], "nyquist": false}}"#
        );
        xtask::harness::Discretization::of(&serde_json::from_str(&text).unwrap())
    };
    assert!(record("[12, 12, 12]", "0123456789abcdef").is_ok());
    let refused = |fine: &str, waves: &str, why: &str| {
        let e = record(fine, waves).unwrap_err();
        assert!(e.contains(why), "{fine} {waves}: {e}");
    };
    refused(
        "[12, 0, 12]",
        "0123456789abcdef",
        "a grid with a zero dimension",
    );
    refused("[12, 12, 12]", "0123456789abcdeg", "not 16 hex digits");
    refused("[12, 12, 12]", "0123456789abcde", "not 16 hex digits");
    let operations = |rotations: &str| {
        let text = format!(
            r#"{{"operations": {rotations}, "coarse_grid": [8, 8, 8], "fine_grid": [12, 12, 12],
                "plane_waves": "0123456789abcdef", "density_spheres": ["0123456789abcdef", "fedcba9876543210"],
                "nyquist": false}}"#
        );
        xtask::harness::Discretization::of(&serde_json::from_str(&text).unwrap())
    };
    let one = |r: &str| format!(r#"[{{"rotation": {r}, "translation": [0.0, 0.0, 0.0]}}]"#);
    assert!(operations(&one("[[1, 0, 0], [0, 1, 0], [0, 0, 1]]")).is_ok());
    let e = operations(&one("[[-1, 0, 0], [0, -1, 0], [0, 0, 1]]")).unwrap_err();
    assert!(e.contains("operations without the identity"), "{e}");
}
