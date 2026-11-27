use super::{CASE, INPUT, engine, honest};
use crate::{Tree, tree, write};
use serde_json::{Value, json};
use xtask::harness::{Evidence, Metrics, Outcome, Session, Tier};

const DELTA_HA: f64 = 3.674_932_217_565_499e-6;
const SWEEP: [u64; 1] = [8];
pub(crate) fn prelude() -> String {
    format!(
        "grep -qx 'step_energies = true' input.toml || exit 7\n\
         step() {{ echo \"{}\" >&2; f=$1; }}\n\
         tight() {{ ! grep -qx 'energy_tolerance_ha = 0.001' input.toml; }}\n\
         t=$(sed -n 's/^threads = //p' input.toml)",
        step_line().replace("<n>", "1").replace("<F>", "$1")
    )
}

pub(crate) fn step_line() -> &'static str {
    let readme = include_str!("../../../README.md");
    let (_, form) = readme.split_once("each SCF step on stderr: ").unwrap();
    form.lines().next().unwrap()
}

const OUTPUT: &str = "if grep -qx 'stress = true' input.toml; then printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"free_energy_ha\": %s, \"stress_ha_bohr3\": [[%s, 0, 0], [0, %s, 0], [0, 0, %s]]}' \"$f\" \"${sig:-0}\" \"${sig:-0}\" \"${sig:-0}\"; \
     else printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"free_energy_ha\": %s}' \"$f\"; fi";
const ONE_SI: &str = "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n";

pub(crate) fn fake(root: &Tree, body: &str) -> std::path::PathBuf {
    engine(root, &format!("{}\n{body}\n{OUTPUT}", prelude()))
}

fn claim(name: &str, body: &str) -> (Outcome, Value, Tree) {
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    claim_of(name, &input, ONE_SI, body)
}

fn claim_of(name: &str, input: &str, structure: &str, body: &str) -> (Outcome, Value, Tree) {
    claim_files(name, &[("input.toml", input)], structure, body)
}

fn claim_files(
    name: &str,
    inputs: &[(&str, &str)],
    structure: &str,
    body: &str,
) -> (Outcome, Value, Tree) {
    claim_case(
        name,
        &CASE.replace("\"push\"", "\"claim\""),
        inputs,
        structure,
        body,
    )
}

fn claim_case(
    name: &str,
    case: &str,
    inputs: &[(&str, &str)],
    structure: &str,
    body: &str,
) -> (Outcome, Value, Tree) {
    let root = claim_tree(name, case, inputs, structure, body);
    let session = Session::new(
        &root,
        &root.join("fake-engine"),
        Tier::Claim,
        &root.join("records"),
    );
    let (outcome, record) = evaluated(&root, &session);
    (outcome, record, root)
}

fn claim_tree(
    name: &str,
    case: &str,
    inputs: &[(&str, &str)],
    structure: &str,
    body: &str,
) -> Tree {
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", case),
            ("target/datasets/JTH-PBE-v2.0/Si/Si.xml", "si"),
        ],
    );
    for (path, input) in inputs {
        let input = input.replace("[basis]", &format!("{structure}\n[basis]"));
        write(&root, &format!("benchmarks/1_a/{path}"), &input);
    }
    let table = super::jth("file:///nowhere", &[("pbe", "Si", "si", None)]);
    write(&root, "datasets/jth-2.0.toml", &table);
    fake(&root, body);
    root
}

fn evaluated(root: &Tree, session: &Session) -> (Outcome, Value) {
    let outcome = session.evaluate("1_a", both);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

pub(crate) fn passed(outcome: &Outcome) -> bool {
    let tolerated = |f: &str| {
        f.contains(": T(δ) varies by CV ")
            || f.contains(" is unmeasured: no pair whose runs all last 20 Pss polls")
    };
    match outcome {
        Outcome::Pass => true,
        Outcome::Fail(why) => why.split("; ").all(tolerated),
        _ => false,
    }
}

pub(crate) fn both(evidence: &Evidence) -> Result<Metrics, String> {
    let mut error: f64 = 0.0;
    for name in evidence.names() {
        let e = evidence.output(name)?.eigenvalues_ha()?[0][0][0];
        error = error.max((e - 0.5).abs());
    }
    Ok(Metrics::from([("error".to_owned(), error)]))
}

fn numbers(value: &Value) -> Vec<f64> {
    value
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_f64().unwrap())
        .collect()
}

fn median(t: &[f64]) -> f64 {
    let mut sorted = t.to_vec();
    sorted.sort_by(f64::total_cmp);
    let n = sorted.len();
    if n % 2 == 1 {
        sorted[n / 2]
    } else {
        0.5 * (sorted[n / 2 - 1] + sorted[n / 2])
    }
}

fn sample_cv(t: &[f64]) -> f64 {
    let n = t.len() as f64;
    let mean = t.iter().sum::<f64>() / n;
    (t.iter().map(|x| (x - mean) * (x - mean)).sum::<f64>() / (n - 1.0)).sqrt() / mean
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * b.abs()
}

fn statistics_hold(record: &Value) {
    for block in record["speed"].as_array().unwrap() {
        let t = numbers(&block["t_delta_s"]);
        let low = t.iter().copied().fold(f64::INFINITY, f64::min);
        let high = t.iter().copied().fold(0.0, f64::max);
        assert!(
            close(block["median_t_delta_s"].as_f64().unwrap(), median(&t)),
            "{block}"
        );
        assert_eq!(numbers(&block["range_t_delta_s"]), [low, high], "{block}");
        let cv = block["cv_t_delta"].as_f64().unwrap();
        assert!((cv - sample_cv(&t)).abs() <= 1e-9 * cv, "{block}");
        assert_eq!(numbers(&block["wall_s"]).len(), t.len());
        assert_eq!(numbers(&block["peak_rss_bytes"]).len(), t.len());
        assert!(block.get("efficiency").is_none(), "{block}");
    }
}

#[test]
fn t_delta_is_the_arrival_of_the_step_from_which_the_energy_stays_within_delta_of_the_tight_run() {
    let body = "if tight; then step -2; else\n\
        step -2; sleep 0.3; step -1.9; sleep 0.3; step -2.000001; sleep 1; step -2.000002\nfi";
    let (outcome, record, _) = claim("speed-t-delta", body);
    assert!(passed(&outcome), "{record}");
    let blocks = record["speed"].as_array().unwrap();
    let threads: Vec<u64> = blocks
        .iter()
        .map(|b| b["threads"].as_u64().unwrap())
        .collect();
    assert_eq!(threads, SWEEP);
    assert_eq!(record["threads"], serde_json::json!(SWEEP));
    for block in blocks {
        assert_eq!(block["e_ref_ha"].as_f64(), Some(-2.0), "{block}");
        assert_eq!(block["e_ref_shift_ha"].as_f64(), Some(0.0), "{block}");
        assert_eq!(block["delta_ha"].as_f64(), Some(DELTA_HA), "{block}");
        let t = numbers(&block["t_delta_s"]);
        let want = if sample_cv(&t[..5]) > 0.05 { 10 } else { 5 };
        assert_eq!(t.len(), want, "{block}");
        assert!(t.iter().all(|&t| (0.6..1.3).contains(&t)), "{block}");
    }
    statistics_hold(&record);
}

#[test]
fn a_timed_run_that_ends_outside_delta_fails_the_claim() {
    let body = "if tight; then step -2; else step -2; step -1.9999; fi";
    let (outcome, ..) = claim("speed-outside", body);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("timing: input at ") && why.contains(" threads: the run never held |E − E_ref| < δ")),
        "{outcome:?}"
    );
}

#[test]
fn the_last_step_line_must_give_the_printed_free_energy() {
    let (outcome, ..) = claim("speed-agree", "step -2; f=-2.5");
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("the last step line gives F = -2, but stdout's free_energy_ha is Some(-2.5)")),
        "{outcome:?}"
    );
}

#[test]
fn e_ref_must_hold_at_a_ten_times_tighter_stop_and_the_record_keeps_its_shift() {
    for (moved, passes) in [("-2.00000002", true), ("-2.00000005", false)] {
        let body = format!(
            "s=$(sed -n 's/^energy_tolerance_ha = //p' input.toml)\n\
             if tight && awk \"BEGIN {{ exit !($s < 1e-9) }}\"; then step {moved}; else step -2; fi"
        );
        let (outcome, record, _) = claim(&format!("speed-shift-{passes}"), &body);
        if passes {
            assert!(passed(&outcome), "{record}");
            let shift = record["speed"][0]["e_ref_shift_ha"].as_f64().unwrap();
            assert!((shift + 2e-8).abs() < 1e-15, "{shift}");
        } else {
            assert!(
                matches!(&outcome, Outcome::Fail(why) if why.contains("E_ref moves by -5") && why.contains("e-8 Ha at a 10× tighter stop")),
                "{outcome:?}"
            );
        }
    }
}

#[test]
fn the_reference_runs_stop_at_a_thousandth_of_delta_per_atom_whatever_the_timed_stop_and_tighter_again()
 {
    let two =
        "species = [\"Si\", \"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]]\n";
    for (stop, structure, atoms, want) in [
        (1e-7, ONE_SI, 1.0, 3.674_932_217_565_499e-9),
        (1e-5, ONE_SI, 1.0, 3.674_932_217_565_499e-9),
        (1e-9, two, 2.0, 7.349_864_435_130_998e-9),
        (1e-5, two, 2.0, 7.349_864_435_130_998e-9),
    ] {
        let log = std::env::temp_dir().join(format!(
            "xtask-speed-stops-{stop}-{atoms}-{}.log",
            std::process::id()
        ));
        let body = format!(
            "sed -n 's/^energy_tolerance_ha = //p' input.toml >> '{}'\nstep -2",
            log.display()
        );
        let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = {stop:e}\n");
        let name = format!("speed-stop-{stop}-{atoms}");
        let (outcome, record, _) = claim_of(&name, &input, structure, &body);
        assert!(passed(&outcome), "{record}");
        assert_eq!(
            record["speed"][0]["delta_ha"].as_f64(),
            Some(atoms * DELTA_HA)
        );
        let text = std::fs::read_to_string(&log).unwrap();
        let _ = std::fs::remove_file(&log);
        let stops: Vec<f64> = text.lines().map(|l| l.parse().unwrap()).collect();
        assert!(
            close(stops[0], want) && close(stops[1], want / 10.0),
            "{text}"
        );
        assert!(stops[2..].iter().all(|&s| s == stop), "{text}");
    }
}

#[test]
fn a_timed_input_must_state_its_stop_and_is_refused_before_any_run() {
    let log = std::env::temp_dir().join(format!("xtask-speed-no-stop-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let body = format!("echo run >> '{}'\nstep -2", log.display());
    let (outcome, ..) = claim_of("speed-no-stop", INPUT, ONE_SI, &body);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("harness: a timed input needs a number scf.energy_tolerance_ha")),
        "{outcome:?}"
    );
    assert!(
        !log.exists(),
        "{}",
        std::fs::read_to_string(&log).unwrap_or_default()
    );
}

#[test]
fn an_engine_without_step_lines_cannot_claim() {
    let (outcome, ..) = claim("speed-silent", "f=-2");
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("no SCF step lines on stderr")),
        "{outcome:?}"
    );
}

fn counters(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("xtask-{name}-counts-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn the_first_run_of_each_thread_count_is_a_dropped_warm_up() {
    let dir = counters("speed-warm");
    let body = format!(
        "if ! tight; then c='{}'/$t; if [ -e \"$c\" ]; then sleep 0.1; else : > \"$c\"; sleep 2; fi; fi\nstep -2",
        dir.display()
    );
    let (outcome, record, _) = claim("speed-warm", &body);
    let _ = std::fs::remove_dir_all(&dir);
    assert!(passed(&outcome), "{record}");
    for block in record["speed"].as_array().unwrap() {
        assert!(
            block["warmup"]["t_delta_s"].as_f64().unwrap() >= 2.0,
            "{block}"
        );
        assert!(
            numbers(&block["t_delta_s"]).iter().all(|&t| t < 1.5),
            "{block}"
        );
    }
}

#[test]
fn a_thread_count_runs_ten_times_exactly_when_its_first_five_vary_past_five_percent() {
    let dir = counters("speed-cv");
    let body = format!(
        "if ! tight; then c='{}'/$t; n=$(cat \"$c\" 2>/dev/null || echo 0); echo $((n + 1)) > \"$c\"\n\
         sleep 0.2; [ \"$t\" = 8 ] && [ $((n % 2)) = 1 ] && sleep 1; fi\nstep -2",
        dir.display()
    );
    let (_, record, _) = claim("speed-cv", &body);
    let _ = std::fs::remove_dir_all(&dir);
    let blocks = record["speed"].as_array().unwrap();
    for block in blocks {
        let t = numbers(&block["t_delta_s"]);
        let want = if sample_cv(&t[..5]) > 0.05 { 10 } else { 5 };
        assert_eq!(t.len(), want, "{block}");
        assert_eq!(block["noisy"], false, "{block}");
    }
    assert_eq!(numbers(&blocks[0]["t_delta_s"]).len(), 10, "{record}");
    statistics_hold(&record);
}

fn order(name: &str, seed: Option<&str>) -> Vec<u64> {
    let log = std::env::temp_dir().join(format!("xtask-{name}-{}.order", std::process::id()));
    let body = format!(
        "if ! tight; then sed -n 's/^cutoff_ha = \\([0-9]\\).*/\\1/p' input.toml >> '{}'; sleep 0.2; fi\nstep -2",
        log.display()
    );
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let (b, c) = (
        input.replace("cutoff_ha = 1.0", "cutoff_ha = 2.0"),
        input.replace("cutoff_ha = 1.0", "cutoff_ha = 3.0"),
    );
    let files = [
        ("input.toml", input.as_str()),
        ("inputs/b.toml", b.as_str()),
        ("inputs/c.toml", c.as_str()),
    ];
    let (_, _, root) = claim_files(name, &files, ONE_SI, &body);
    if let Some(seed) = seed {
        let _ = std::fs::remove_file(&log);
        let bin = fake(&root, &body);
        let var = |key: &str| match key {
            "ORBITAR_BIN" => Some(bin.clone().into_os_string()),
            "ORBITAR_TIER" => Some("claim".into()),
            "ORBITAR_SEED" => Some(seed.into()),
            "ORBITAR_RECORD_DIR" => Some(root.join("records").into_os_string()),
            _ => None,
        };
        Session::from_vars(&root, var)
            .unwrap()
            .unwrap()
            .with_load(&procfs(&root, 1))
            .evaluate("1_a", honest);
    }
    let text = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_file(&log);
    text.lines().map(|l| l.parse().unwrap()).collect()
}

#[test]
fn a_warm_up_round_and_five_timed_rounds_each_run_every_block_once_in_a_random_order() {
    let runs = order("speed-order", None);
    let rounds: Vec<&[u64]> = runs[..18].chunks(3).collect();
    for round in &rounds {
        let mut sorted = round.to_vec();
        sorted.sort_unstable();
        assert_eq!(sorted, [1, 2, 3], "{runs:?}");
    }
    assert!(
        rounds.iter().any(|r| *r != rounds[0]),
        "the order changes between rounds: {runs:?}"
    );
    assert_eq!(
        runs[runs.len() - 3..],
        [1, 2, 3],
        "the rotated runs follow in input order: {runs:?}"
    );
}

#[test]
fn a_fixed_seed_replays_the_run_order() {
    let (a, b) = (
        order("speed-seed-a", Some("7")),
        order("speed-seed-b", Some("7")),
    );
    assert_eq!(a[..18], b[..18]);
    let seven = [2, 3, 1, 2, 3, 1, 2, 1, 3, 3, 2, 1, 1, 3, 2, 1, 3, 2];
    assert_eq!(a[..18], seven, "{a:?}");
}

fn not_at_eight(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = honest(evidence)?;
    if evidence.input("input")?.threads()?.map(|n| n.get()) == Some(8) {
        *metrics.entry("error".to_owned()).or_default() += 1.0;
    }
    Ok(metrics)
}

#[test]
fn each_thread_count_is_judged_on_the_input_as_the_engine_ran_it() {
    let (_, _, root) = claim("speed-evidence", "step -2");
    let bin = fake(&root, "step -2");
    let outcome =
        Session::new(&root, &bin, Tier::Claim, &root.join("records")).evaluate("1_a", not_at_eight);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("threads 8: error = 1e0 > 1e-12")),
        "{outcome:?}"
    );
}

#[test]
fn every_input_runs_at_every_thread_count_and_is_compared_with_itself() {
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let wider = input.replace("cutoff_ha = 1.0", "cutoff_ha = 2.0");
    let files = [
        ("input.toml", input.as_str()),
        ("inputs/b.toml", wider.as_str()),
    ];
    let (outcome, record, _) = claim_files("speed-two", &files, ONE_SI, "step -2");
    assert!(passed(&outcome), "{record}");
    statistics_hold(&record);
    let blocks: Vec<(String, u64)> = (record["speed"].as_array().unwrap().iter())
        .map(|b| {
            (
                b["input"].as_str().unwrap().to_owned(),
                b["threads"].as_u64().unwrap(),
            )
        })
        .collect();
    let want: Vec<(String, u64)> = SWEEP
        .iter()
        .flat_map(|&p| [("input".to_owned(), p), ("b".to_owned(), p)])
        .collect();
    assert_eq!(blocks, want);
}

#[test]
fn a_t_delta_cv_past_five_percent_refuses_no_block_of_a_speed_claim() {
    let dir = counters("speed-noisy");
    let body = format!(
        "if ! tight; then c='{}'/$t; n=$(cat \"$c\" 2>/dev/null || echo 0); echo $((n + 1)) > \"$c\"\n\
         sleep 0.2; [ \"$t\" = 8 ] && [ $((n % 2)) = 1 ] && sleep 1; fi\nstep -2",
        dir.display()
    );
    let case = CASE
        .replace("\"push\"", "\"claim\"")
        .replace("\"correctness\"", "\"speed\", \"correctness\"");
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let (outcome, record, _) = claim_case(
        "speed-noisy",
        &case,
        &[("input.toml", &input)],
        ONE_SI,
        &body,
    );
    let _ = std::fs::remove_dir_all(&dir);
    let block = &record["speed"][0];
    assert!(block["cv_t_delta"].as_f64().unwrap() > 0.05, "{block}");
    assert_eq!(numbers(&block["t_delta_s"]).len(), 10, "{block}");
    assert_eq!(block["noisy"], false, "{block}");
    assert!(
        !matches!(&outcome, Outcome::Fail(why) if why.contains("the judge refuses")),
        "{outcome:?}"
    );
}

fn memory_case(name: &str, tolerance: &str, bytes: &str) -> (Outcome, Value, Tree) {
    memory_of(name, &(memory_claim() + tolerance), &si(4, "1.0"), bytes)
}

fn memory_claim() -> String {
    CASE.replace("\"push\"", "\"claim\"")
        .replace("\"active\"", "\"planned\"")
        .replace("\"correctness\"", "\"correctness\", \"memory\"")
}

fn si(atoms: usize, cutoff: &str) -> String {
    let rows = [
        "[0.0, 0.0, 0.0]",
        "[0.5, 0.5, 0.5]",
        "[0.25, 0.25, 0.25]",
        "[0.75, 0.75, 0.75]",
    ];
    let species = vec!["\"Si\""; atoms].join(", ");
    let at = format!(
        "species = [{species}]\npositions_fractional = [{}]\n[basis]",
        rows[..atoms].join(", ")
    );
    format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n")
        .replace("[basis]", &at)
        .replace("cutoff_ha = 1.0", &format!("cutoff_ha = {cutoff}"))
}

fn memory_of(name: &str, case: &str, third: &str, bytes: &str) -> (Outcome, Value, Tree) {
    let (one, two) = (si(1, "1.0"), si(2, "1.0"));
    let files = [
        ("input.toml", one.as_str()),
        ("inputs/b.toml", two.as_str()),
        ("inputs/c.toml", third),
    ];
    let body = format!(
        "a=$(grep -o '\"Si\"' input.toml | wc -l); w=0; grep -qx 'cutoff_ha = 2.0' input.toml && w=1\n\
         m=$(({bytes}))\ndd if=/dev/zero of=/dev/null bs=$m count=1 2>/dev/null\nstep -2"
    );
    claim_case(name, case, &files, "", &body)
}

#[test]
fn rss_exponent_sees_an_atoms_squared_term_that_a_fit_over_every_size_would_hide() {
    let bytes = "40000000 + 16000000 * a * a";
    let (outcome, record, _) = memory_case("speed-memory-growth", "", bytes);
    let exponent = record["metrics"]["rss_exponent"].as_f64().unwrap();
    assert!((1.35..=1.55).contains(&exponent), "{record}");
    assert_eq!(outcome, Outcome::Promote, "{record}");
    assert_eq!(
        record["readouts"],
        serde_json::json!(["rss_exponent"]),
        "{record}"
    );
    let active = memory_claim().replace("\"planned\"", "\"active\"") + "rss_exponent_max = 1.25\n";
    let (outcome, record, _) = memory_of("speed-memory-bound", &active, &si(4, "1.0"), bytes);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rss_exponent = 1.") && why.contains(" > 1.25e0")),
        "{outcome:?}"
    );
    assert_eq!(record["readouts"], serde_json::json!([]), "{record}");
}

#[test]
fn equal_atom_counts_give_rss_exponent_its_worst_input() {
    let bytes = "a == 1 ? 30000000 : (w == 0 ? 90000000 : 120000000)";
    let (_, record, _) = memory_of("speed-memory-ties", &memory_claim(), &si(2, "2.0"), bytes);
    let exponent = record["metrics"]["rss_exponent"].as_f64().unwrap();
    assert!((1.65..=2.0).contains(&exponent), "{record}");
}

#[test]
fn at_eight_threads_rss_exponent_reads_the_working_sets_in_flight() {
    let bytes = "20000000 * a + (t < 16 / a ? t : 16 / a) * 8000000 * a * a";
    let (_, record, _) = memory_case("speed-memory-sets", "", bytes);
    let exponent = record["metrics"]["rss_exponent"].as_f64().unwrap();
    assert!((0.9..=1.05).contains(&exponent), "{record}");
}

#[test]
fn a_claim_without_speed_keeps_a_block_whose_cv_passes_five_percent() {
    let dir = counters("speed-noisy-untimed");
    let body = format!(
        "if ! tight; then c='{}'/$t; n=$(cat \"$c\" 2>/dev/null || echo 0); echo $((n + 1)) > \"$c\"\n\
         [ \"$t\" = 8 ] && [ $((n % 2)) = 1 ] && sleep 1; fi\nstep -2",
        dir.display()
    );
    let (outcome, record, _) = claim("speed-noisy-untimed", &body);
    let _ = std::fs::remove_dir_all(&dir);
    let block = &record["speed"][0];
    assert!(block["cv_t_delta"].as_f64().unwrap() > 0.05, "{record}");
    assert_eq!(block["noisy"], false, "{record}");
    assert!(
        !matches!(&outcome, Outcome::Fail(why) if why.contains("the judge refuses")),
        "{outcome:?}"
    );
}

#[test]
fn memory_readouts_need_a_memory_claim() {
    let (_, record, _) = claim("speed-no-memory", "step -2");
    assert!(record["metrics"].get("rss_exponent").is_none(), "{record}");
}

#[test]
fn rss_exponent_is_nan_below_two_atom_counts() {
    let case = memory_claim();
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let (outcome, record, _) = claim_case(
        "speed-one-size",
        &case,
        &[("input.toml", &input)],
        ONE_SI,
        "step -2",
    );
    assert_eq!(outcome, Outcome::Promote, "{record}");
    assert!(record["metrics"]["rss_exponent"].is_null(), "{record}");
}

fn hold(mb: u64) -> String {
    let bytes = mb * 1_000_000;
    format!("head -c {bytes} /dev/zero | tail -c {bytes} | sleep 1.1")
}

fn versus(name: &str, ours: u64, theirs: u64) -> (Outcome, Value) {
    versus_with(name, "sleep 1.1", ours, theirs)
}

fn versus_with(name: &str, sleep: &str, ours: u64, theirs: u64) -> (Outcome, Value) {
    use super::rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let hold = |mb: u64| hold(mb).replace("sleep 1.1", sleep);
    let (pw, abinit) = (
        format!("{}\n{PW}", hold(theirs)),
        format!("{}\n{ABINIT}", hold(theirs)),
    );
    let root = rivals_tree(name, INPUT, TUNED, &pw, &abinit);
    let bin = fake(&root, &format!("{}\nstep -2", hold(ours)));
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    let outcome = session.evaluate("1_a", both);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

#[test]
fn r_per_rival_compares_peak_pss_over_the_timed_runs_and_g_is_judged_beside_it() {
    let (outcome, record) = versus("speed-versus-win", 20, 80);
    let versus = record["versus"].as_array().unwrap();
    let codes: Vec<&str> = versus.iter().map(|v| v["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["qe", "abinit"], "{record}");
    for v in versus {
        let r = v["r"].as_f64().unwrap();
        assert!((2.5..5.0).contains(&r), "{v}");
        assert!(v["g"].as_f64().unwrap() > 0.0, "{v}");
        let ci = numbers(&v["g_ci"]);
        assert!(
            ci[0] <= v["g"].as_f64().unwrap() && v["g"].as_f64().unwrap() <= ci[1],
            "{v}"
        );
        assert_eq!(
            (v["r_min"].as_f64(), v["g_judged"].as_bool()),
            (Some(1.0), Some(true)),
            "{v}"
        );
        assert_eq!(v["pairs"].as_array().unwrap().len(), 1, "{v}");
        assert_eq!(v["pairs"][0]["threads"], 8, "{v}");
    }
    assert!(
        !matches!(&outcome, Outcome::Fail(why) if why.contains("R against")),
        "{outcome:?}"
    );
}

#[test]
fn r_below_one_against_a_rival_fails_the_claim_by_its_name() {
    let (outcome, record) = versus("speed-versus-loss", 80, 20);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("R against qe = 0.") && why.contains(" is below 1: orbitaR holds more memory")),
        "{outcome:?}: {}",
        record["versus"]
    );
}

#[test]
fn r_reads_no_pair_whose_runs_last_less_than_twenty_pss_polls() {
    let (outcome, record) = versus_with("speed-versus-short", "sleep 0.3", 20, 80);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("R against qe is unmeasured: no pair whose runs all last 20 Pss polls (1 s)")),
        "{outcome:?}"
    );
    let qe = &record["versus"][0];
    assert_eq!(qe["pairs"][0]["in_r"], false, "{qe}");
    assert!(qe["g"].as_f64().is_some(), "{qe}");
}

#[test]
fn g_and_r_are_geomeans_over_the_pairs_of_the_ratios_of_the_medians_each_pair_records() {
    use super::rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let count = std::env::temp_dir().join(format!("xtask-speed-geomean-{}", std::process::id()));
    let pw = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); echo $((n + 1)) > '{0}'\n\
         [ $n -ge 6 ] && [ $((n % 3)) = 0 ] && {{ echo 'Error in routine cdiaghg' >&2; exit 1; }}\n{1}\n{PW}",
        count.display(),
        hold(60)
    );
    let abinit = format!("{}\n{ABINIT}", hold(60));
    let root = rivals_tree("speed-versus-geomean", INPUT, TUNED, &pw, &abinit);
    write(
        &root,
        "benchmarks/1_a/inputs/b.toml",
        &INPUT.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5"),
    );
    let bin = fake(&root, &format!("{}\nstep -2", hold(20)));
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    let _ = session.evaluate("1_a", both);
    let _ = std::fs::remove_file(&count);
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap())
            .unwrap();
    let number = |v: &Value| {
        v.as_f64()
            .unwrap_or_else(|| v.as_str().unwrap().parse().unwrap())
    };
    let median = |code: &str, input: &str, key: &str| {
        let b = (record["speed"].as_array().unwrap().iter())
            .find(|b| b["code"] == code && b["input"] == input)
            .unwrap();
        let mut runs = numbers(&b[key]);
        runs.extend(std::iter::repeat_n(
            f64::INFINITY,
            b["losses"].as_array().unwrap().len(),
        ));
        runs.sort_by(f64::total_cmp);
        let k = runs.len() / 2;
        match runs.len() % 2 {
            1 => runs[k],
            _ => (runs[k - 1] + runs[k]) / 2.0,
        }
    };
    let close = |a: f64, b: f64| a == b || (a - b).abs() <= 1e-12 * b.abs();
    let mut lost = 0;
    for v in record["versus"].as_array().unwrap() {
        let code = v["code"].as_str().unwrap();
        let pairs = v["pairs"].as_array().unwrap();
        assert_eq!(pairs.len(), 2, "{v}");
        let (mut ln_t, mut ln_m) = (0.0, 0.0);
        for p in pairs {
            let input = p["input"].as_str().unwrap();
            let (t, m) = (&p["medians_t_delta_s"], &p["medians_peak_pss_bytes"]);
            assert!(
                close(number(&t[0]), median(code, input, "t_delta_s")),
                "{p}"
            );
            assert!(
                close(number(&t[1]), median("orbitar", input, "t_delta_s")),
                "{p}"
            );
            assert!(
                close(number(&m[0]), median(code, input, "peak_pss_bytes")),
                "{p}"
            );
            assert!(
                close(number(&m[1]), median("orbitar", input, "peak_pss_bytes")),
                "{p}"
            );
            let (t, m) = (number(&t[0]) / number(&t[1]), number(&m[0]) / number(&m[1]));
            assert!(
                close(number(&p["t_ratio"]), t) && (t - 1.0).abs() > 1e-3,
                "{p}"
            );
            assert!(close(number(&p["pss_ratio"]), m), "{p}");
            assert_eq!(p["in_r"], true, "{p}");
            lost += p["losses"].as_u64().unwrap();
            (ln_t, ln_m) = (ln_t + t.ln(), ln_m + m.ln());
        }
        assert!(close(number(&v["g"]), (ln_t / 2.0).exp()), "{v}");
        assert!(close(number(&v["r"]), (ln_m / 2.0).exp()), "{v}");
    }
    assert!(lost > 0, "no QE pair lost a run: {record}");
}

fn needs_a_reference(evidence: &Evidence) -> Result<Metrics, String> {
    evidence.reference("missing.json")?;
    both(evidence)
}

#[test]
fn a_claim_whose_measure_lacks_a_reference_stops_before_it_times_anything() {
    let log =
        std::env::temp_dir().join(format!("xtask-speed-preflight-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let body = format!("echo run >> '{}'\nstep -2", log.display());
    let (_, _, root) = claim("speed-preflight", "step -2");
    let bin = fake(&root, &body);
    let outcome = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .evaluate("1_a", needs_a_reference);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("reference: harness: reference/missing.json missing; the claim stops before it times anything")),
        "{outcome:?}"
    );
    assert!(!log.exists(), "the engine ran");
}

fn reference_then_output(evidence: &Evidence) -> Result<Metrics, String> {
    for name in evidence.names() {
        evidence.reference(&format!("{name}.json"))?;
        evidence.output(name)?.energy_ha("free_energy_ha")?;
    }
    both(evidence)
}

#[test]
fn the_preflight_finds_a_missing_reference_behind_another_input_s_output() {
    let log = std::env::temp_dir().join(format!(
        "xtask-speed-preflight-b-{}.log",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&log);
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let b = input.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5");
    let (_, _, root) = claim_files(
        "speed-preflight-b",
        &[("input.toml", &input), ("inputs/b.toml", &b)],
        ONE_SI,
        "step -2",
    );
    write(&root, "benchmarks/1_a/reference/input.json", "{}");
    let bin = fake(&root, &format!("echo run >> '{}'\nstep -2", log.display()));
    let outcome = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .evaluate("1_a", reference_then_output);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("reference/b.json missing; the claim stops before it times anything")),
        "{outcome:?}"
    );
    assert!(!log.exists(), "the engine ran");
}

#[test]
fn rss_exponent_reads_orbitar_runs_only_never_a_rivals() {
    use super::rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    let hold = |bytes: &str| {
        format!("m=$(({bytes}))\ndd if=/dev/zero of=/dev/null bs=$m count=1 2>/dev/null")
    };
    let (pw, abinit) = (
        format!("{}\n{PW}", hold("200000000")),
        format!("{}\n{ABINIT}", hold("200000000")),
    );
    let tuned = TUNED.replace(
        "\"benchmarks/1_a/input.toml\" = [[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]",
        "\"benchmarks/1_a/input.toml\" = [[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]\n\
         \"benchmarks/1_a/inputs/b.toml\" = [[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]",
    );
    let root = rivals_tree("speed-memory-rivals", INPUT, &tuned, &pw, &abinit);
    let case = std::fs::read_to_string(root.join("benchmarks/1_a/case.toml")).unwrap();
    write(
        &root,
        "benchmarks/1_a/case.toml",
        &case.replace("\"correctness\"", "\"correctness\", \"memory\""),
    );
    let two = INPUT
        .replace("species = [\"Si\"]", "species = [\"Si\", \"Si\"]")
        .replace(
            "[[0.0, 0.0, 0.25]]",
            "[[0.0, 0.0, 0.25], [0.25, 0.25, 0.5]]",
        );
    write(&root, "benchmarks/1_a/inputs/b.toml", &two);
    let bin = fake(
        &root,
        &format!(
            "a=$(grep -o '\"Si\"' input.toml | wc -l)\n{}\nstep -2",
            hold("20000000 * a")
        ),
    );
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    let _ = session.evaluate("1_a", both);
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap())
            .unwrap();
    assert_eq!(record["speed"].as_array().unwrap().len(), 6, "{record}");
    let exponent = record["metrics"]["rss_exponent"].as_f64().unwrap();
    assert!((0.7..=1.05).contains(&exponent), "{record}");
}

fn noisy_pairs() -> Vec<[Vec<f64>; 2]> {
    vec![
        [
            vec![3.4, 4.0, 4.6, 3.7, 4.3, 4.2, 3.8, 4.5, 3.5, 4.05],
            vec![0.88, 1.0, 1.12, 0.93, 1.07, 1.03, 0.97, 1.15, 0.85, 1.0],
        ],
        [
            vec![8.0, 6.8, 9.2, 7.4, 8.6, 8.3, 7.7, 8.9, 7.1, 8.07],
            vec![2.0, 2.25, 1.75, 2.15, 1.85, 2.3, 1.7, 2.04, 1.96, 2.0],
        ],
    ]
}

fn interval(pairs: &[[Vec<f64>; 2]]) -> [f64; 2] {
    xtask::harness::g_interval(pairs, xtask::harness::G_RESAMPLES, xtask::harness::G_SEED)
}

#[test]
fn g_s_interval_is_the_percentile_bootstrap_that_an_independent_rebuild_gives() {
    let [low, high] = interval(&noisy_pairs());
    assert!(close(low, 3.700_482_461_982_002_5), "{low}");
    assert!(close(high, 4.329_178_365_823_084), "{high}");
}

#[test]
fn g_s_interval_on_a_two_valued_resample_set_is_its_two_values() {
    let pairs = [[vec![4.0; 5], vec![1.0, 1.0, 1.0, 1.0, 2.0]]];
    let [low, high] = interval(&pairs);
    assert!(close(low, 2.0) && close(high, 4.0), "{low} {high}");
}

#[test]
fn equal_runs_collapse_g_s_interval_onto_g() {
    let pairs = [[vec![4.0; 5], vec![2.0; 5]], [vec![9.0; 10], vec![1.0; 10]]];
    let [low, high] = interval(&pairs);
    assert_eq!(low, high);
    assert!(close(low, 18f64.sqrt()), "{low}");
}

#[test]
fn a_rival_s_lost_run_pushes_g_s_interval_up() {
    let pairs = noisy_pairs();
    let mut lost = pairs.clone();
    lost[0][0][8] = f64::INFINITY;
    let ([low, high], [lost_low, lost_high]) = (interval(&pairs), interval(&lost));
    assert!(
        lost_low > low && lost_high > high,
        "{low} {high} {lost_low} {lost_high}"
    );
}

#[test]
fn the_same_seed_rebuilds_g_s_interval_and_another_seed_draws_another() {
    use xtask::harness::{G_RESAMPLES, G_SEED, g_interval};
    let pairs = noisy_pairs();
    let again = g_interval(&pairs, G_RESAMPLES, G_SEED);
    assert_eq!(interval(&pairs), again);
    assert_ne!(g_interval(&pairs, G_RESAMPLES, G_SEED + 1), again);
}

#[test]
fn g_s_interval_is_nan_without_runs_and_its_lower_bound_is_nan_under_forty_resamples() {
    use xtask::harness::{G_SEED, g_interval};
    let [low, high] = interval(&[[vec![4.0; 5], vec![]]]);
    assert!(low.is_nan() && high.is_nan(), "{low} {high}");
    let pairs = noisy_pairs();
    let [low, high] = g_interval(&pairs, 39, G_SEED);
    assert!(low.is_nan() && high.is_finite(), "{low} {high}");
    let [low, high] = g_interval(&pairs, 40, G_SEED);
    assert!(low.is_finite() && low < high, "{low} {high}");
}

fn g_claim(name: &str, ours: &str, theirs: &str) -> (Outcome, Value) {
    g_claim_at(name, ours, theirs, "")
}

fn g_claim_at(name: &str, ours: &str, theirs: &str, milestone: &str) -> (Outcome, Value) {
    use super::rival::{ABINIT, CASE, INPUT, PW, TUNED, rivals_tree};
    let (pw, abinit) = (format!("t={theirs}\n{PW}"), format!("t={theirs}\n{ABINIT}"));
    let root = rivals_tree(name, INPUT, TUNED, &pw, &abinit);
    let case = CASE.replace("\n\n[tolerance]", &format!("\n{milestone}\n[tolerance]"));
    write(&root, "benchmarks/1_a/case.toml", &case);
    let bin = fake(&root, &format!("sleep {ours}\nstep -2"));
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    let outcome = session.evaluate("1_a", both);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

#[test]
fn g_s_interval_lower_bound_gates_the_claim_against_each_rival() {
    use xtask::harness::{G_RESAMPLES, G_SEED};
    for (name, ours, theirs, passes) in [
        ("speed-versus-g-win", "0", "0.5", true),
        ("speed-versus-g-loss", "0.6", "0.1", false),
    ] {
        let (outcome, record) = g_claim(name, ours, theirs);
        let versus = record["versus"].as_array().unwrap();
        assert_eq!(versus.len(), 2, "{record}");
        for v in versus {
            let (g, ci) = (v["g"].as_f64().unwrap(), numbers(&v["g_ci"]));
            assert!(ci[0] <= g && g <= ci[1], "{v}");
            assert_eq!(ci[0] >= 1.0, passes, "{v}");
            assert_eq!(v["g_judged"], true, "{v}");
            assert_eq!(
                (
                    v["g_min"].as_f64(),
                    v["g_resamples"].as_u64(),
                    v["g_seed"].as_u64()
                ),
                (Some(1.0), Some(G_RESAMPLES as u64), Some(G_SEED)),
                "{v}"
            );
        }
        let refused = |code: &str| {
            matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("G against {code} = "))
                && why.contains("whose lower bound is not at least 1: orbitaR is not shown faster"))
        };
        assert_eq!(
            (refused("qe"), refused("abinit")),
            (!passes, !passes),
            "{outcome:?}"
        );
    }
}

#[test]
fn an_m4_claim_refuses_a_g_whose_bound_passes_one_but_whose_value_misses_one_and_a_quarter() {
    for (name, milestone, refused) in [
        ("speed-versus-m3", "", false),
        ("speed-versus-m4", "milestone = \"m4\"", true),
    ] {
        let (outcome, record) = g_claim_at(name, "0.4", "0.48", milestone);
        let versus = record["versus"].as_array().unwrap();
        assert_eq!(versus.len(), 2, "{record}");
        for v in versus {
            let (g, ci) = (v["g"].as_f64().unwrap(), numbers(&v["g_ci"]));
            assert!(ci[0] > 1.0 && g < 1.25, "{v}");
            let bar = match refused {
                true => (json!("m4"), json!(1.25)),
                false => (json!("m3"), Value::Null),
            };
            assert_eq!(
                (v["milestone"].clone(), v["g_point_min"].clone()),
                bar,
                "{v}"
            );
        }
        for code in ["qe", "abinit"] {
            let missed = matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("G against {code} = "))
                && why.contains("misses M4 (README Benchmark status): G ≥ 1.25 and a lower bound > 1"));
            let below = matches!(&outcome, Outcome::Fail(why) if why.contains("whose lower bound is not at least 1"));
            assert_eq!((missed, below), (refused, false), "{code}: {outcome:?}");
        }
    }
}

#[test]
fn a_pilot_times_each_block_once_after_its_warm_up_and_e_ref_and_never_passes() {
    use super::rival::{ABINIT, INPUT, PW, TUNED, rivals_tree};
    use std::ffi::OsString;
    let root = rivals_tree("speed-pilot", INPUT, TUNED, PW, ABINIT);
    let bin = fake(&root, "step -2").into_os_string();
    let records = root.join("records");
    let vars = |tier: &str, pilot: &str| {
        let pairs: Vec<(&str, OsString)> = vec![
            ("ORBITAR_BIN", bin.clone()),
            ("ORBITAR_TIER", tier.into()),
            ("ORBITAR_RECORD_DIR", records.clone().into()),
            ("ORBITAR_RIVALS", root.join("rivals").into()),
            ("ORBITAR_PILOT", pilot.into()),
        ];
        move |key: &str| {
            (pairs.iter())
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.clone())
        }
    };
    for (tier, pilot) in [("pr", "1"), ("claim", "yes")] {
        let refused = Session::from_vars(&root, vars(tier, pilot)).err().unwrap();
        assert!(
            refused.contains("ORBITAR_PILOT must be 1 and runs in the claim tier alone"),
            "{refused}"
        );
    }
    let session = Session::from_vars(&root, vars("claim", "1"))
        .unwrap()
        .unwrap();
    let outcome = session.evaluate("1_a", both);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("pilot (ORBITAR_PILOT): one timed run per block")),
        "{outcome:?}"
    );
    let text = std::fs::read_to_string(records.join("1_a.json")).unwrap();
    let record: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(record["pilot"], true, "{record}");
    let blocks = record["speed"].as_array().unwrap();
    assert_eq!(blocks.len(), 3, "{record}");
    for b in blocks {
        assert_eq!(numbers(&b["wall_s"]).len(), 1, "{b}");
        assert_eq!(b["steps"].as_array().unwrap().len(), 1, "{b}");
        assert!(
            b["warmup"]["wall_s"].is_number() && b["e_ref_steps"].is_array(),
            "{b}"
        );
        let load = b["load"].as_array().unwrap();
        assert_eq!(load.len(), 2, "{b}");
        assert!(load.iter().all(|r| r["unread"].is_string()), "{b}");
    }
}

fn one_si_tree(name: &str) -> Tree {
    let input = format!("{INPUT}\n[scf]\nenergy_tolerance_ha = 0.001\n");
    let case = CASE.replace("\"push\"", "\"claim\"");
    claim_tree(name, &case, &[("input.toml", &input)], ONE_SI, "step -2")
}

fn procfs(root: &Tree, runnable: u32) -> std::path::PathBuf {
    write(
        root,
        "benchmarks/NODE.toml",
        "[host]\ncpus_allowed = \"0-7\"\n",
    );
    write(
        root,
        "proc/loadavg",
        &format!("8.00 8.00 8.00 {runnable}/300 4242\n"),
    );
    write(
        root,
        "proc/pressure/cpu",
        "some avg10=20.00 avg60=10.00 avg300=5.00 total=1000\nfull avg10=0.00 avg60=0.00 avg300=0.00 total=0\n",
    );
    root.join("proc")
}

fn claim_at_load(name: &str, runnable: Option<u32>, pilot: bool) -> (Outcome, Value) {
    use std::ffi::OsString;
    let root = one_si_tree(name);
    let proc = match runnable {
        Some(n) => procfs(&root, n),
        None => {
            procfs(&root, 1);
            root.join("no-proc")
        }
    };
    let bin: OsString = root.join("fake-engine").into();
    let pairs: Vec<(&str, OsString)> = vec![
        ("ORBITAR_BIN", bin),
        ("ORBITAR_TIER", "claim".into()),
        ("ORBITAR_RECORD_DIR", root.join("records").into()),
    ]
    .into_iter()
    .chain(pilot.then(|| ("ORBITAR_PILOT", "1".into())))
    .collect();
    let vars = move |key: &str| {
        (pairs.iter())
            .find(|(k, _)| *k == key)
            .map(|(_, v)| v.clone())
    };
    let session = Session::from_vars(&root, vars)
        .unwrap()
        .unwrap()
        .with_load(&proc);
    evaluated(&root, &session)
}

fn readings(record: &Value) -> Vec<Vec<&str>> {
    (record["speed"].as_array().into_iter().flatten())
        .map(|b| {
            (b["load"].as_array().into_iter().flatten())
                .filter_map(|r| {
                    r.as_object()
                        .and_then(|o| o.keys().next())
                        .map(String::as_str)
                })
                .collect()
        })
        .collect()
}

#[test]
fn a_node_whose_only_runnable_task_is_the_reader_is_quiet_whatever_load_averages_the_last_run_left()
{
    let (outcome, record) = claim_at_load("load-quiet", Some(1), false);
    assert!(passed(&outcome), "{outcome:?}");
    let runs = record["speed"][0]["wall_s"].as_array().unwrap().len();
    assert_eq!(readings(&record), [vec!["quiet"; 1 + runs]], "{record}");
    let sample = &record["speed"][0]["load"][0]["quiet"];
    assert_eq!(sample["load_avg"], json!([8.0, 8.0, 8.0]), "{sample}");
    assert_eq!(sample["psi_some_avg10"], json!(20.0), "{sample}");
    assert_eq!(sample["psi_some_share"], json!(0.0), "{sample}");
}

#[test]
fn one_other_runnable_task_holds_an_eighth_of_the_nodes_8_cpus_past_the_5_percent_cv_and_stops_a_claim()
 {
    for (runnable, held) in [
        (2, "1 other runnable tasks hold 0.125"),
        (9, "8 other runnable tasks hold 1"),
    ] {
        let (outcome, record) =
            claim_at_load(&format!("load-busy-{runnable}"), Some(runnable), false);
        let why = format!(
            "the node is not quiet before a timed run: {held} of the node's 8 CPUs, not under 0.05"
        );
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(&why) && f.contains("harness")),
            "{outcome:?}"
        );
        assert!(
            record["speed"].as_array().is_none_or(Vec::is_empty),
            "{record}"
        );
    }
}

#[test]
fn a_pilot_records_a_loaded_or_unread_node_before_each_run_and_runs_on_so_its_readout_shows_it() {
    for (n, (runnable, kind)) in [(Some(2), "loaded"), (None, "unread")]
        .into_iter()
        .enumerate()
    {
        let (outcome, record) = claim_at_load(&format!("load-pilot-{n}"), runnable, true);
        assert_eq!(readings(&record), [vec![kind; 2]], "{record}");
        let named = "input at 8 threads: 2 of its 2 runs, the warm-up first, began on a node that was not quiet";
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(named)),
            "{outcome:?}"
        );
    }
}

#[test]
fn a_claim_stops_on_a_node_that_shows_no_load_or_declares_no_cpus() {
    let (outcome, _) = claim_at_load("load-unread", None, false);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet before a timed run: ") && f.contains("no-proc/loadavg")),
        "{outcome:?}"
    );
    let root = one_si_tree("load-no-node");
    let proc = procfs(&root, 1);
    std::fs::remove_file(root.join("benchmarks/NODE.toml")).unwrap();
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, _) = evaluated(&root, &session);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("the node is not quiet before a timed run: claim: benchmarks/NODE.toml is missing")),
        "{outcome:?}"
    );
}

fn claim_with_stalls(name: &str, tick_us: u64) -> Outcome {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };
    let root = one_si_tree(name);
    let proc = procfs(&root, 1);
    let done = Arc::new(AtomicBool::new(false));
    let stalls = {
        let (done, proc) = (done.clone(), proc.clone());
        std::thread::spawn(move || {
            let mut total: u64 = 1000;
            while !done.load(Ordering::Relaxed) {
                total += tick_us;
                let next = proc.join("pressure/cpu.next");
                let line = format!("some avg10=0.00 avg60=0.00 avg300=0.00 total={total}\n");
                std::fs::write(&next, line).unwrap();
                std::fs::rename(&next, proc.join("pressure/cpu")).unwrap();
                std::thread::sleep(std::time::Duration::from_millis(10));
            }
        })
    };
    let bin = root.join("fake-engine");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records")).with_load(&proc);
    let (outcome, _) = evaluated(&root, &session);
    done.store(true, Ordering::Relaxed);
    stalls.join().unwrap();
    outcome
}

#[test]
fn a_task_that_waits_for_a_cpu_through_the_window_stops_a_claim_and_one_that_waits_a_hundredth_of_it_does_not()
 {
    let waited =
        |o: &Outcome| matches!(o, Outcome::Fail(f) if f.contains("a task waited for a CPU "));
    let outcome = claim_with_stalls("load-stall", 10_000);
    assert!(
        waited(&outcome)
            && matches!(&outcome, Outcome::Fail(f) if f.contains(" of the window, not under 0.05")),
        "{outcome:?}"
    );
    let outcome = claim_with_stalls("load-stall-brief", 100);
    assert!(!waited(&outcome) && passed(&outcome), "{outcome:?}");
}
