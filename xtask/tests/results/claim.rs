use super::{WHAT, cases, commit, read, s11};
use crate::{Tree, has, lever, write};
use serde_json::{Value, json};
use std::path::Path;

pub(super) const RECORD: &str = "target/benchmarks/claim/5_e.json";
const CODES: [&str; 3] = ["orbitar", "qe", "abinit"];
const SKIP: &str = "no dataset converges it";
const QUIET_NODE: &str = r#"{"quiet": {"load_avg": [8.0, 8.0, 8.0], "others_runnable": 0, "psi_some_avg10": 20.0, "psi_some_share": 0.0}}"#;
const ALONE: &str = r#"{"quiet": {"wall_s": 0.2, "busy_cpu_s": 1.6, "child_cpu_s": 1.5, "harness_cpu_s": 0.05, "others_cpu_s": 0.05, "share": 0.03125}}"#;

#[derive(Clone, Copy)]
pub(super) enum Claims {
    Memory,
    Speed,
    SpeedM4,
}

#[derive(Clone)]
pub(super) struct Cell {
    pub(super) input: &'static str,
    pub(super) base: Option<&'static str>,
    pub(super) atoms: usize,
    pub(super) runs: [Option<(f64, u64)>; 3],
    pub(super) long: bool,
}

fn cell(input: &'static str, atoms: usize, runs: [Option<(f64, u64)>; 3], long: bool) -> Cell {
    Cell {
        input,
        base: None,
        atoms,
        runs,
        long,
    }
}

pub(super) fn cells() -> Vec<Cell> {
    vec![
        cell(
            "input",
            2,
            [
                Some((0.2, 100_000_000)),
                Some((0.4, 200_000_000)),
                Some((0.6006, 300_000_000)),
            ],
            false,
        ),
        cell(
            "b",
            8,
            [
                Some((8.0, 400_000_000)),
                Some((32.0, 800_000_000)),
                Some((24.024, 900_000_000)),
            ],
            true,
        ),
        cell(
            "c",
            32,
            [
                Some((64.0, 1_600_000_000)),
                Some((256.32, 3_200_000_000)),
                Some((192.192, 2_700_000_000)),
            ],
            true,
        ),
        cell(
            "d",
            128,
            [
                Some((512.0, 6_400_000_000)),
                Some((4111.1552, 12_864_000_000)),
                Some((1537.536, 10_800_000_000)),
            ],
            true,
        ),
        Cell {
            base: Some("b"),
            ..cell(
                "held-out-b",
                8,
                [
                    Some((8.5, 420_000_000)),
                    Some((37.4, 840_000_000)),
                    Some((25.5255, 1_260_000_000)),
                ],
                true,
            )
        },
    ]
}

fn path(input: &str) -> String {
    match input {
        "input" => "benchmarks/5_e/input.toml".to_owned(),
        name => format!("benchmarks/5_e/inputs/{name}.toml"),
    }
}

pub(super) fn claim_cases(name: &str, cells: &[Cell], claims: Claims) -> Tree {
    let root = cases(name);
    let (claimed, milestone) = match claims {
        Claims::Memory => ("\"speed\", \"memory\", \"correctness\"", ""),
        Claims::Speed => ("\"speed\", \"correctness\"", ""),
        Claims::SpeedM4 => ("\"speed\", \"correctness\"", "milestone = \"m4\"\n"),
    };
    write(
        &root,
        "benchmarks/5_e/case.toml",
        &format!(
            "[case]\ntier = \"claim\"\nstatus = \"active\"\nclaims = [{claimed}]\nreference = \"analytic\"\nrivals = [\"qe\", \"abinit\"]\n{milestone}\n[tolerance]\nf_error_max = 1e-9\n"
        ),
    );
    write(
        &root,
        "benchmarks/results-what.toml",
        &format!(
            "{WHAT}\n[5]\nwhat = \"F vs reference; speed vs rivals\"\nmetric = [\"f_error\"]\nunit = \"Ha/atom\"\n"
        ),
    );
    for c in cells.iter().filter(|c| c.base.is_none()) {
        write(
            &root,
            &path(c.input),
            &format!("[structure]\nspecies = {:?}\n", vec!["Si"; c.atoms]),
        );
    }
    let mut skips = String::new();
    for c in cells.iter().filter(|c| c.runs[1].is_none()) {
        skips.push_str(&format!(
            "{{ cell = \"{}\", reason = \"{SKIP}\" }}, ",
            path(c.input)
        ));
    }
    let rivals = crate::harness::rival::RIVALS.replacen(
        "density_ratio = 8.0\n",
        &format!("density_ratio = 8.0\nskip_cells = [{skips}]\n"),
        1,
    );
    write(&root, "benchmarks/RIVALS.toml", &rivals);
    commit(&root, "claim case");
    root
}

pub(super) fn block(code: &str, input: &str, atoms: usize, t: f64, pss: u64, long: bool) -> Value {
    let wall = if long { t + 1.0 } else { t + 0.3 };
    let orbitar = code == "orbitar";
    let quiet: Value = serde_json::from_str(QUIET_NODE).unwrap();
    let alone: Value = serde_json::from_str(ALONE).unwrap();
    json!({
        "code": code, "input": input, "threads": 8, "atoms": atoms,
        "e_ref_ha": -2.0, "e_ref_shift_ha": if orbitar { json!(0.0) } else { Value::Null },
        "delta_ha": 1e-4 / 27.211_386_245_988 * atoms as f64, "warmup": null,
        "t_delta_s": vec![t; 5], "wall_s": vec![wall; 5], "peak_rss_bytes": vec![pss; 5],
        "peak_pss_bytes": vec![pss; 5], "losses": [], "noisy": false, "median_t_delta_s": t,
        "range_t_delta_s": [t, t], "cv_t_delta": 0.0, "median_peak_rss_bytes": pss as f64,
        "median_peak_pss_bytes": pss as f64, "load": vec![quiet; 6], "during": vec![alone; 6],
        "rival": if orbitar { Value::Null } else { json!({}) }, "accuracy": null, "step_cap": null,
        "steps": vec![9; 5], "e_ref_steps": [8, 9],
    })
}

pub(super) fn measured_record(commit: &str, cells: &[Cell]) -> Value {
    let mut speed = Vec::new();
    for c in cells {
        for (code, runs) in CODES.iter().zip(&c.runs) {
            if let Some((t, pss)) = runs {
                speed.push(block(code, c.input, c.atoms, *t, *pss, c.long));
            }
        }
    }
    let inputs: Vec<&str> = cells.iter().map(|c| c.input).collect();
    let held: Vec<Value> = (cells.iter())
        .filter_map(|c| {
            Some(json!({ "input": c.input, "base": c.base?, "sha256": "ab".repeat(32) }))
        })
        .collect();
    let runs = 5 * cells.len();
    json!({
        "schema": 4, "case": "5_e", "tier": "claim", "commit": commit, "dirty": false, "pilot": false,
        "engine_sha256": "cd".repeat(32), "outcome": "pass", "failures": [], "filter": null,
        "metrics": { "f_error": 1e-10 }, "tolerance": { "f_error_max": 1e-9 },
        "inputs": inputs, "held_out": held, "runs_s": vec![0.1; runs], "peak_rss_bytes": vec![1; runs],
        "rotation_s": vec![0.1; cells.len() - 1], "rotation_max": 0.0, "rotation_used": 0.0,
        "seed_fixed": false, "speed": speed,
        "node": {
            "host": { "cpu": "Stand-in CPU", "cores": 8, "core_cpus": ["0", "1"], "memory_bytes": 16_762_847_232_u64 },
            "orbitar": { "build": "cargo build --release", "profile_release": { "lto": "thin" } },
            "qe": { "qe": "7.5 h19104ac_2 sha256:00", "libgfortran-ng": "14.2 h0 sha256:01", "a b": "1 x sha256:02" },
            "abinit": { "abinit": "10.0.3 h2b8cb88_2 sha256:03" },
        },
    })
}

pub(super) fn summed(root: &Path, mut record: Value) -> Value {
    let made = xtask::harness::claim_summaries(root, &record).unwrap();
    record["versus"] = made.versus;
    if !made.series.is_null() {
        record["series"] = made.series;
    }
    record["skipped"] = made.skipped;
    record
}

pub(super) fn claim_record(root: &Path, commit: &str, cells: &[Cell]) -> Value {
    summed(root, measured_record(commit, cells))
}

pub(super) fn collected(root: &Tree, record: &Value) -> Vec<String> {
    write(root, RECORD, &record.to_string());
    lever(&["results", "collect"], root)
}

fn speed_block<'a>(record: &'a mut Value, code: &str, input: &str) -> &'a mut Value {
    (record["speed"].as_array_mut().unwrap().iter_mut())
        .find(|b| b["code"] == code && b["input"] == input)
        .unwrap()
}

pub(super) fn skipping() -> Vec<Cell> {
    let mut skipping = cells();
    skipping[2].runs[1] = None;
    skipping
}

fn steady(ratio: [f64; 5], held: f64) -> Vec<Cell> {
    let mut steady: Vec<Cell> = ["input", "b", "c", "d", "e"]
        .iter()
        .zip(ratio)
        .map(|(&input, ratio)| {
            cell(
                input,
                2,
                [
                    Some((8.0, 100_000_000)),
                    Some((8.0 * ratio, 200_000_000)),
                    Some((16.0, 200_000_000)),
                ],
                true,
            )
        })
        .collect();
    steady.push(Cell {
        base: Some("b"),
        ..cell(
            "held-out-b",
            2,
            [
                Some((8.0, 100_000_000)),
                Some((8.0 * held, 200_000_000)),
                Some((16.0, 200_000_000)),
            ],
            true,
        )
    });
    steady
}

#[test]
fn a_passing_claim_record_keeps_the_comparisons_series_and_skips_that_its_samples_give() {
    let root = claim_cases("claim-table", &cells(), Claims::Memory);
    let head = commit(&root, "head");
    let record = claim_record(&root, &head, &cells());
    assert_eq!(record["series"][0]["coverage"], "full", "{record}");
    assert_eq!(collected(&root, &record), Vec::<String>::new());
    let results = read(&root, "benchmarks/RESULTS.toml");
    for text in [
        "[[case.claim.versus]]\nrival = \"qe\"\n",
        "[[case.claim.versus.pair]]\ninput = \"input\"\nthreads = 8\nt_delta_s_rival = 6.006e-1\nt_delta_s_orbitar = 2e-1\npeak_pss_bytes_rival = 3e8\npeak_pss_bytes_orbitar = 1e8\nin_r = false\nlosses = 0\n",
        "input = \"held-out-b\"\nthreads = 8\n",
        "[[case.claim.series]]\ncode = \"qe\"\ncoverage = \"full\"\n",
        "[[case.claim.series.point]]\ninput = \"d\"\natoms = 128\nstatus = \"measured\"\nt_delta_s = 1.537536e3\npeak_pss_bytes = 1.08e10\n",
        "[case.claim.node.host]\ncore_cpus = [\"0\", \"1\"]\ncores = 8\ncpu = \"Stand-in CPU\"\nmemory_bytes = 16762847232\n",
        "[case.claim.node.qe]\n\"a b\" = \"1 x sha256:02\"\nlibgfortran-ng = \"14.2 h0 sha256:01\"\n",
    ] {
        assert!(results.contains(text), "{text}: {results}");
    }
    assert!(
        !results.contains("input = \"held-out-b\"\natoms"),
        "a held-out cell is no series point: {results}"
    );
    let pinned = read(&root, "benchmarks/RECORDS.jsonl");
    assert_eq!(pinned, format!("{}\n", read(&root, RECORD)));
    assert!(lever(&["results", "render"], &root).is_empty());
    assert_eq!(s11(&root), Vec::<String>::new());
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert_eq!(read(&root, "benchmarks/RESULTS.toml"), results);
}

#[test]
fn a_skip_that_rivals_toml_configures_is_kept_with_its_reason() {
    let root = claim_cases("claim-skips", &skipping(), Claims::Speed);
    let head = commit(&root, "head");
    assert!(collected(&root, &claim_record(&root, &head, &skipping())).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert!(
        results.contains("[[case.claim.skip]]\ncode = \"qe\"\ninput = \"c\"\natoms = 32\nkind = \"skipped\"\nreason = \"no dataset converges it\"\n")
            && !results.contains("[[case.claim.series]]"),
        "{results}"
    );
}

#[test]
fn five_pairs_of_ten_seconds_against_eight_meet_the_m4_bar_exactly() {
    let ratios = steady([1.25; 5], 1.25);
    let root = claim_cases("claim-m4", &ratios, Claims::SpeedM4);
    let head = commit(&root, "head");
    let record = claim_record(&root, &head, &ratios);
    let qe = &record["versus"][0];
    assert_eq!(
        (&qe["code"], &qe["g"], &qe["g_ci"]),
        (&json!("qe"), &json!(1.25), &json!([1.25, 1.25])),
        "{qe}"
    );
    assert_eq!(collected(&root, &record), Vec::<String>::new());
    assert!(lever(&["results", "render"], &root).is_empty());
    let speed = read(&root, "assets/benchmarks/card-speed-light.svg");
    assert!(speed.contains(">≥ 1.25×</text>"), "{speed}");
}

#[test]
fn a_point_estimate_just_below_one_fails_m3_whatever_summary_is_submitted() {
    let good = steady([1.5; 5], 1.5);
    let root = claim_cases("claim-m3", &good, Claims::Speed);
    let head = commit(&root, "head");
    let submitted = claim_record(&root, &head, &good);
    let below = steady(
        [0.057, 17.543_859_649_122_8, 1.0, 1.0, 1.0],
        0.999_999_999_999_999,
    );
    let mut record = measured_record(&head, &below);
    for key in ["versus", "skipped"] {
        record[key] = submitted[key].clone();
    }
    record["versus"][0]["g"] = json!(1.0);
    record["versus"][0]["g_ci"] = json!([1.0, 1.0]);
    let found = collected(&root, &record);
    assert!(
        has(&found, "G against qe = 0.99999999")
            && has(&found, "whose lower bound is not at least 1"),
        "{found:?}"
    );
    assert!(!root.join("benchmarks/RESULTS.toml").exists());
}

#[test]
fn a_submitted_summary_must_equal_bit_for_bit_what_the_samples_give() {
    let root = claim_cases("claim-exact", &cells(), Claims::Memory);
    let head = commit(&root, "head");
    let good = claim_record(&root, &head, &cells());
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut record = good.clone();
        f(&mut record);
        record
    };
    let cases: Vec<(Value, &str)> = vec![
        (
            edit(&|r| {
                let g = r["versus"][0]["g"].as_f64().unwrap();
                r["versus"][0]["g"] = json!(f64::from_bits(g.to_bits() + 1));
            }),
            "the record's versus for qe is not what its speed blocks give: g is",
        ),
        (
            edit(&|r| r["versus"][0]["pairs"][0]["in_r"] = json!(true)),
            "the record's versus for qe is not what its speed blocks give: pairs is",
        ),
        (
            edit(&|r| r["series"][1]["points"][1]["median_t_delta_s"] = json!(30.0)),
            "the record's series for qe is not what its speed blocks give: points is",
        ),
        (
            edit(&|r| r["series"][0]["t_delta_exponent"] = json!(1.9)),
            "the record's series for orbitar is not what its speed blocks give: t_delta_exponent is 1.9",
        ),
        (
            edit(&|r| r["failures"] = json!(["a block failed"])),
            "5_e says pass, but names failures",
        ),
        (
            edit(&|r| r["pilot"] = json!("true")),
            "5_e names no pilot flag false",
        ),
        (
            edit(&|r| r["speed"] = json!([])),
            "the harness's claim proof refuses it",
        ),
        (
            edit(&|r| r["versus"][0]["g_ci"][0] = json!(0.8)),
            "the harness's claim proof refuses it",
        ),
        (
            edit(&|r| r["node"]["qe"]["qe"] = json!("7.5")),
            "the node block names no version and build of qe",
        ),
        (
            edit(&|r| {
                r["node"]["host"].as_object_mut().unwrap().remove("cpu");
            }),
            "the node block names no CPU: [host].cpu",
        ),
    ];
    for (record, why) in cases {
        let found = collected(&root, &record);
        assert!(has(&found, why), "{why}: {found:?}");
        assert!(!root.join("benchmarks/RESULTS.toml").exists());
    }
    let root = claim_cases("claim-exact-skips", &skipping(), Claims::Speed);
    let head = commit(&root, "head");
    let mut record = claim_record(&root, &head, &skipping());
    record["skipped"] = json!([]);
    let found = collected(&root, &record);
    assert!(
        has(
            &found,
            "the record's skipped holds 0 entries, but its speed blocks give 1"
        ),
        "{found:?}"
    );
}

#[test]
fn the_pairs_follow_the_case_manifest_of_declared_inputs_and_held_out_names() {
    let root = claim_cases("claim-manifest", &cells(), Claims::Speed);
    let head = commit(&root, "head");
    let good = claim_record(&root, &head, &cells());
    let renamed = {
        let mut r = good.clone();
        r["inputs"][4] = json!("held-out-c");
        r["held_out"][0]["input"] = json!("held-out-c");
        r
    };
    let dropped = {
        let mut r = good.clone();
        r["inputs"].as_array_mut().unwrap().remove(2);
        r["rotation_s"].as_array_mut().unwrap().pop();
        r["speed"]
            .as_array_mut()
            .unwrap()
            .retain(|b| b["input"] != "c");
        r["runs_s"] = json!(vec![0.1; 20]);
        r["peak_rss_bytes"] = json!(vec![1; 20]);
        r
    };
    let doubled = {
        let mut r = good.clone();
        let copy = speed_block(&mut r, "abinit", "b").clone();
        r["speed"].as_array_mut().unwrap().push(copy);
        r
    };
    for (record, why) in [
        (
            renamed,
            "the speed block of orbitar held-out-b at 8 threads is not in the case's manifest",
        ),
        (
            dropped,
            "the record's inputs [\"input\",\"b\",\"d\",\"held-out-b\"] are not the case's inputs and held-out names",
        ),
        (doubled, "the harness's claim proof refuses it"),
    ] {
        let found = collected(&root, &record);
        assert!(has(&found, why), "{why}: {found:?}");
    }
}

#[test]
fn a_block_must_hold_its_declared_cell_and_sum_up_its_own_samples() {
    let root = claim_cases("claim-blocks", &cells(), Claims::Memory);
    let head = commit(&root, "head");
    let good = claim_record(&root, &head, &cells());
    let summary = {
        let mut r = good.clone();
        let b = speed_block(&mut r, "qe", "d");
        b["median_t_delta_s"] = json!(256.0);
        b["range_t_delta_s"] = json!([256.0, 256.0]);
        r
    };
    let larger = {
        let mut r = good.clone();
        *speed_block(&mut r, "qe", "d") = block("qe", "d", 256, 4111.1552, 12_864_000_000, true);
        r
    };
    for (record, why) in [
        (
            summary,
            "the speed block of qe d at 8 threads sums up other numbers than its samples give",
        ),
        (
            larger,
            "the speed block of qe d at 8 threads holds 256 atoms, but its cell holds 128",
        ),
    ] {
        let found = collected(&root, &record);
        assert!(has(&found, why), "{why}: {found:?}");
    }
}

#[test]
fn science_s11_derives_each_claim_row_again_from_its_pinned_record() {
    let root = claim_cases("claim-s11", &skipping(), Claims::Speed);
    let head = commit(&root, "head");
    assert!(collected(&root, &claim_record(&root, &head, &skipping())).is_empty());
    assert!(lever(&["results", "render"], &root).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    let changed = "differs from what its record in benchmarks/RECORDS.jsonl gives";
    let g = (results.lines())
        .find(|l| l.starts_with("g = "))
        .unwrap()
        .to_owned();
    for (from, to) in [
        (g.as_str(), "g = 5e0"),
        ("in_r = false\nlosses = 0\n", "in_r = true\nlosses = 0\n"),
        (
            "reason = \"no dataset converges it\"",
            "reason = \"another reason\"",
        ),
    ] {
        assert!(results.contains(from), "{from}");
        write(
            &root,
            "benchmarks/RESULTS.toml",
            &results.replacen(from, to, 1),
        );
        let found = s11(&root);
        assert!(
            has(
                &found,
                &format!("5_e: its row in benchmarks/RESULTS.toml {changed}")
            ),
            "{from}: {found:?}"
        );
    }
}

const REUSED: &str = "benchmarks/REUSED.jsonl";
const STARTED: u64 = 1_764_000_000_000;

fn reuse_key() -> Value {
    let layout = json!({ "ranks": 1, "threads": 8 });
    json!({
        "schema": 4, "pilot": false, "repeats": 5, "timeout_s": 86_400,
        "protocol": { "measuring_code_sha256": "aa".repeat(32), "containment": null, "memory_guard_bytes": null },
        "decks": { "timed": "ab".repeat(32) }, "identity": { "input_sha256": "ac".repeat(32) },
        "frozen": layout, "fitted": layout, "layout_policy": "cd".repeat(32), "cutoff_ha": 10.0, "mixing": [],
        "thread_env": { "OMP_NUM_THREADS": "8", "OPENBLAS_NUM_THREADS": "1" }, "mpiexec": [],
        "invocation_sha256": "ad".repeat(32), "runtime": { "<rivals>/qe/bin/pw.x": "ae".repeat(32) },
        "datasets": { "Si.UPF": "ef".repeat(32) }, "stop_floor_ha": null, "density_ratio": 8.0,
        "volume_scale": 0.94, "package": { "qe": "7.5 h19104ac_2 sha256:00" }, "node": {}, "declared_host": {},
    })
}

fn declared_node(root: &Path) {
    let node: toml::Table =
        serde_json::from_value(measured_record("", &cells())["node"].clone()).unwrap();
    write(
        root,
        "benchmarks/NODE.toml",
        &toml::to_string(&node).unwrap(),
    );
}

fn drift_entry(record: &Value, code: &str) -> Value {
    let b = (record["speed"].as_array().unwrap().iter())
        .find(|b| b["code"] == code && b["input"] == "input")
        .unwrap();
    let quiet: Value = serde_json::from_str(QUIET_NODE).unwrap();
    let alone: Value = serde_json::from_str(ALONE).unwrap();
    let run = json!({ "t_delta_s": b["median_t_delta_s"], "peak_pss_bytes": b["peak_pss_bytes"][0], "loss": null });
    json!({
        "code": code, "input": "input", "threads": 8, "atoms": b["atoms"], "source": b["reused"],
        "runs": [run, run, run], "load": [quiet, quiet, quiet], "during": [alone, alone, alone],
        "share": 0.1, "held": true, "why": null,
    })
}

struct Reusing {
    source: String,
    record: Value,
}

fn reusing(root: &Path, head: &str, codes: &[&str], edit: impl Fn(&mut Value)) -> Reusing {
    let mut source = measured_record(head, &cells());
    source["started_unix_ms"] = json!(STARTED);
    for b in source["speed"].as_array_mut().unwrap() {
        if b["code"] != "orbitar" {
            b["reuse_key"] = reuse_key();
        }
    }
    let mut source = summed(root, source);
    edit(&mut source);
    let text = source.to_string();
    let sha = crate::harness::hex(&<sha2::Sha256 as sha2::Digest>::digest(&text));
    let mut record = source.clone();
    record["started_unix_ms"] = json!(STARTED + 60_000);
    record["dirty"] = json!(false);
    record["commit"] = json!(head);
    record["node"] = measured_record(head, &cells())["node"].clone();
    let named = json!({ "commit": source["commit"], "record_sha256": sha, "started_unix_ms": source["started_unix_ms"] });
    let mut count = 0;
    for b in record["speed"].as_array_mut().unwrap() {
        if codes.contains(&b["code"].as_str().unwrap()) {
            b["reused"] = named.clone();
            count += 1;
        }
    }
    let drift: Vec<Value> = codes
        .iter()
        .map(|code| drift_entry(&record, code))
        .collect();
    record["rival_reuse"] = json!({
        "mode": "claim", "protocol": reuse_key()["protocol"], "reused": count, "drift": drift,
    });
    Reusing {
        source: text,
        record,
    }
}

fn earlier(name: &str, source: &str) -> Tree {
    let earlier =
        Tree(std::env::temp_dir().join(format!("xtask-{name}-earlier-{}", std::process::id())));
    let _ = std::fs::remove_dir_all(&*earlier);
    write(&earlier, "5_e.json", source);
    earlier
}

fn collected_beside(root: &Tree, record: &Value, earlier: &Path) -> Vec<String> {
    write(root, RECORD, &record.to_string());
    let env = xtask::Env {
        rival_records: Some(earlier.to_owned()),
        ..crate::env(false)
    };
    xtask::run(&["results", "collect"], root, &env)
}

fn reused_cases(name: &str) -> (Tree, String) {
    let root = claim_cases(name, &cells(), Claims::Speed);
    declared_node(&root);
    let head = commit(&root, "head");
    (root, head)
}

#[test]
fn a_reused_rival_block_counts_beside_its_pinned_source_and_science_replays_it() {
    let (root, head) = reused_cases("claim-reused");
    let r = reusing(&root, &head, &["qe", "abinit"], |_| {});
    let earlier = earlier("claim-reused", &r.source);
    let missing = collected(&root, &r.record);
    assert!(has(&missing, "is not available"), "{missing:?}");
    assert_eq!(
        collected_beside(&root, &r.record, &earlier),
        Vec::<String>::new()
    );
    assert_eq!(read(&root, REUSED), format!("{}\n", r.source));
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert!(results.contains("[[case.claim.versus.pair]]"), "{results}");
    assert_eq!(
        results.matches("[[case.claim.reused]]").count(),
        10,
        "{results}"
    );
    assert!(lever(&["results", "render"], &root).is_empty());
    assert_eq!(s11(&root), Vec::<String>::new());
    write(
        &root,
        REUSED,
        &format!("{}\n{{\"case\": \"5_e\"}}\n", r.source),
    );
    let found = s11(&root);
    assert!(
        has(
            &found,
            "whose rival blocks no record in benchmarks/RECORDS.jsonl reuses"
        ),
        "{found:?}"
    );
    std::fs::remove_file(root.join(REUSED)).unwrap();
    let found = s11(&root);
    assert!(has(&found, "is not available"), "{found:?}");
}

#[test]
fn each_card_names_the_rival_cells_that_reuse_an_earlier_record_and_no_fresh_one() {
    let (root, head) = reused_cases("claim-reused-cards");
    let r = reusing(&root, &head, &["qe"], |_| {});
    let earlier = earlier("claim-reused-cards", &r.source);
    assert_eq!(
        collected_beside(&root, &r.record, &earlier),
        Vec::<String>::new()
    );
    assert!(lever(&["results", "render"], &root).is_empty());
    let sha = &crate::harness::hex(&<sha2::Sha256 as sha2::Digest>::digest(&r.source))[..12];
    let line = format!(
        "reused: 5 QE cell(s) from the record {sha} of commit {}, pinned in benchmarks/REUSED.jsonl",
        &head[..7]
    );
    let mut seen = 0;
    for card in ["speed", "memory", "scaling"] {
        let path = format!("assets/benchmarks/card-{card}-light.svg");
        if std::fs::metadata(root.join(&path)).is_err() {
            continue;
        }
        seen += 1;
        let svg = read(&root, &path);
        assert!(svg.contains(&line), "{card}: {svg}");
        assert!(!svg.contains("ABINIT cell(s)"), "{card}");
    }
    assert!(seen >= 2, "no card rendered");
}

#[test]
fn a_source_that_is_dirty_unnamed_later_or_off_the_declared_node_is_refused_at_collect() {
    let (root, head) = reused_cases("claim-reused-probes");
    type Edit = fn(&mut Value);
    let probes: [(&str, Edit, &str); 4] = [
        ("dirty", |s| s["dirty"] = json!(true), "its dirty is true"),
        (
            "commit",
            |s| s["commit"] = json!("abc"),
            "it names no full commit hash",
        ),
        (
            "later",
            |s| s["started_unix_ms"] = json!(STARTED + 120_000),
            "not before the reusing run's",
        ),
        (
            "kernel",
            |s| s["node"]["host"]["kernel"] = json!("6.9-other"),
            "its node is not NODE.toml's",
        ),
    ];
    for (name, edit, why) in probes {
        let r = reusing(&root, &head, &["qe", "abinit"], edit);
        let earlier = earlier(&format!("claim-reused-{name}"), &r.source);
        let found = collected_beside(&root, &r.record, &earlier);
        assert!(has(&found, why), "{name}: {found:?}");
    }
    let mut r = reusing(&root, &head, &["qe", "abinit"], |s| {
        s["node"]["host"]["kernel"] = json!("6.9-other")
    });
    r.record["node"]["host"]["kernel"] = json!("6.9-other");
    let earlier = earlier("claim-reused-both", &r.source);
    let found = collected_beside(&root, &r.record, &earlier);
    assert!(
        has(&found, "on a node that is not NODE.toml's: host.kernel"),
        "{found:?}"
    );
}

#[test]
fn a_reused_block_must_equal_its_source_and_its_drift_runs_must_hold_again() {
    let (root, head) = reused_cases("claim-reused-tampered");
    let r = reusing(&root, &head, &["qe", "abinit"], |_| {});
    let earlier = earlier("claim-reused-tampered", &r.source);
    let mut tampered = r.record.clone();
    speed_block(&mut tampered, "qe", "b")["t_delta_s"][0] = json!(7.0);
    let found = collected_beside(&root, &tampered, &earlier);
    assert!(
        has(
            &found,
            "its samples, key or E_ref differ from its source's block"
        ),
        "{found:?}"
    );
    let mut bare = r.record.clone();
    bare["rival_reuse"]["drift"] = json!([{ "code": "qe", "held": true }]);
    let found = collected_beside(&root, &bare, &earlier);
    assert!(
        has(&found, "reuses qe blocks without a drift check that holds"),
        "{found:?}"
    );
    let mut heavier = r.record.clone();
    for run in heavier["rival_reuse"]["drift"][0]["runs"]
        .as_array_mut()
        .unwrap()
    {
        run["peak_pss_bytes"] = json!(400_000_000);
    }
    let found = collected_beside(&root, &heavier, &earlier);
    assert!(has(&found, "median peak Pss"), "{found:?}");
    let mut miscounted = r.record.clone();
    miscounted["rival_reuse"]["reused"] = json!(3);
    let found = collected_beside(&root, &miscounted, &earlier);
    assert!(has(&found, "but its rival_reuse counts 3"), "{found:?}");
}
