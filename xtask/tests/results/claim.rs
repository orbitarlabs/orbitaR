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

#[test]
fn a_cell_that_both_rivals_skip_is_kept_with_orbitars_atom_count() {
    let mut both = skipping();
    both[2].runs[2] = None;
    let root = claim_cases("claim-both-skip", &both, Claims::Speed);
    let rivals = read(&root, "benchmarks/RIVALS.toml");
    let abinit = rivals.replacen(
        "density_ratio = 2.0\n",
        "density_ratio = 2.0\nskip_cells = [{ cell = \"benchmarks/5_e/inputs/c.toml\", reason = \"no dataset converges it\" }]\n",
        1,
    );
    assert_ne!(abinit, rivals);
    write(&root, "benchmarks/RIVALS.toml", &abinit);
    let head = commit(&root, "head");
    assert!(collected(&root, &claim_record(&root, &head, &both)).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    for code in ["qe", "abinit"] {
        assert!(
            results.contains(&format!(
                "[[case.claim.skip]]\ncode = \"{code}\"\ninput = \"c\"\natoms = 32\n"
            )),
            "{results}"
        );
    }
}

fn without_pss_at_b() -> Vec<Cell> {
    let mut zero = cells();
    zero[1].runs = [
        Some((0.2, 400_000_000)),
        Some((0.4, 0)),
        Some((0.3, 900_000_000)),
    ];
    zero[1].long = false;
    zero
}

#[test]
fn a_series_point_whose_runs_hold_no_positive_pss_is_unmeasured() {
    let zero = without_pss_at_b();
    let root = claim_cases("claim-zero-pss", &zero, Claims::Memory);
    let head = commit(&root, "head");
    let made = xtask::harness::claim_summaries(&root, &measured_record(&head, &zero)).unwrap();
    let qe = (made.series.as_array().unwrap().iter())
        .find(|s| s["code"] == "qe")
        .unwrap();
    let b = (qe["points"].as_array().unwrap().iter())
        .find(|p| p["input"] == "b")
        .unwrap();
    assert_eq!(b["status"], "pss unmeasured: no Pss poll landed", "{qe}");
    assert_eq!(b["pss_polled_runs"], json!([0, 5]), "{qe}");
}

#[test]
fn the_claim_proof_names_each_code_whose_blocks_at_an_input_hold_no_positive_pss() {
    let zero = without_pss_at_b();
    let root = claim_cases("claim-proof-pss", &zero, Claims::Memory);
    let head = commit(&root, "head");
    let found = collected(&root, &claim_record(&root, &head, &zero));
    assert!(
        has(
            &found,
            "no qe block at b holds a timed run with a positive Pss"
        ),
        "{found:?}"
    );
    assert!(!has(&found, "no orbitar block at b"), "{found:?}");
}

#[test]
fn a_refused_orbitar_block_pairs_with_no_rival_block() {
    let root = claim_cases("claim-refused-pair", &cells(), Claims::Speed);
    let head = commit(&root, "head");
    let mut record = measured_record(&head, &cells());
    let d = speed_block(&mut record, "orbitar", "d");
    for key in [
        "t_delta_s",
        "wall_s",
        "peak_rss_bytes",
        "peak_pss_bytes",
        "steps",
    ] {
        d[key] = json!([]);
    }
    d["refused"] = json!("d at 8 threads: refused before it runs");
    let block = d.as_object_mut().unwrap();
    for key in [
        "median_t_delta_s",
        "range_t_delta_s",
        "cv_t_delta",
        "median_peak_rss_bytes",
        "median_peak_pss_bytes",
    ] {
        block.remove(key);
    }
    let made = xtask::harness::claim_summaries(&root, &record).unwrap();
    for v in made.versus.as_array().unwrap() {
        let inputs: Vec<&str> = (v["pairs"].as_array().unwrap().iter())
            .map(|p| p["input"].as_str().unwrap())
            .collect();
        assert!(!inputs.contains(&"d") && inputs.len() == 4, "{v}");
    }
}

#[test]
fn the_claim_proof_names_a_held_out_variant_whose_base_is_no_input_of_the_record() {
    let root = claim_cases("claim-held-base", &cells(), Claims::Speed);
    let head = commit(&root, "head");
    let mut record = claim_record(&root, &head, &cells());
    record["held_out"][0]["base"] = json!("zzz");
    let found = collected(&root, &record);
    assert!(
        has(
            &found,
            "record's held-out held-out-b varies zzz, which is no input of the record"
        ),
        "{found:?}"
    );
}

#[test]
fn a_claim_whose_rivals_are_timed_at_no_thread_count_calls_for_no_rival_block() {
    let root = claim_cases("claim-untimed-rivals", &cells(), Claims::Speed);
    let rivals = read(&root, "benchmarks/RIVALS.toml");
    let untimed = rivals.replacen("threads = [8]", "threads = []", 1);
    assert_ne!(untimed, rivals);
    write(&root, "benchmarks/RIVALS.toml", &untimed);
    let head = commit(&root, "head");
    let mut record = measured_record(&head, &cells());
    record["speed"]
        .as_array_mut()
        .unwrap()
        .retain(|b| b["code"] == "orbitar");
    let made = xtask::harness::claim_summaries(&root, &record);
    assert!(made.is_ok(), "{:?}", made.err());
}

const FROZEN_SUMMARIES: &str = r#"{"series":[{"code":"orbitar","coverage":"full","points":[{"atoms":2,"input":"input","median_peak_pss_bytes":100000000.0,"median_t_delta_s":0.2,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":8,"input":"b","median_peak_pss_bytes":400000000.0,"median_t_delta_s":8.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":32,"input":"c","median_peak_pss_bytes":1600000000.0,"median_t_delta_s":64.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":6400000000.0,"median_t_delta_s":512.0,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":0.9999999999999994,"t_delta_exponent":1.8482892142331044},{"code":"qe","coverage":"full","points":[{"atoms":2,"input":"input","median_peak_pss_bytes":200000000.0,"median_t_delta_s":0.4,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":8,"input":"b","median_peak_pss_bytes":800000000.0,"median_t_delta_s":32.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":32,"input":"c","median_peak_pss_bytes":3200000000.0,"median_t_delta_s":256.32,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":12864000000.0,"median_t_delta_s":4111.1552,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":1.001079325210631,"t_delta_exponent":2.1491785444690965},{"code":"abinit","coverage":"full","points":[{"atoms":2,"input":"input","median_peak_pss_bytes":300000000.0,"median_t_delta_s":0.6006,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":8,"input":"b","median_peak_pss_bytes":900000000.0,"median_t_delta_s":24.024,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":32,"input":"c","median_peak_pss_bytes":2700000000.0,"median_t_delta_s":192.192,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":10800000000.0,"median_t_delta_s":1537.536,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":0.8547368752524044,"t_delta_exponent":1.8482892142331044}],"skipped":[],"versus":[{"code":"qe","g":4.081011503966281,"g_ci":[4.081011503966281,4.081011503966281],"g_judged":true,"g_min":1.0,"g_point_min":null,"g_resamples":10000,"g_seed":2316557831640514561,"milestone":"m3","pairs":[{"in_r":false,"input":"input","losses":0,"medians_peak_pss_bytes":[200000000.0,100000000.0],"medians_t_delta_s":[0.4,0.2],"pss_ratio":2.0,"shortest_run_s":0.5,"t_ratio":2.0,"threads":8},{"in_r":true,"input":"b","losses":0,"medians_peak_pss_bytes":[800000000.0,400000000.0],"medians_t_delta_s":[32.0,8.0],"pss_ratio":2.0,"shortest_run_s":9.0,"t_ratio":4.0,"threads":8},{"in_r":true,"input":"c","losses":0,"medians_peak_pss_bytes":[3200000000.0,1600000000.0],"medians_t_delta_s":[256.32,64.0],"pss_ratio":2.0,"shortest_run_s":65.0,"t_ratio":4.005,"threads":8},{"in_r":true,"input":"d","losses":0,"medians_peak_pss_bytes":[12864000000.0,6400000000.0],"medians_t_delta_s":[4111.1552,512.0],"pss_ratio":2.01,"shortest_run_s":513.0,"t_ratio":8.0296,"threads":8},{"in_r":true,"input":"held-out-b","losses":0,"medians_peak_pss_bytes":[840000000.0,420000000.0],"medians_t_delta_s":[37.4,8.5],"pss_ratio":2.0,"shortest_run_s":9.5,"t_ratio":4.3999999999999995,"threads":8}],"r":2.0024953261250533,"r_min":1.0},{"code":"abinit","g":3.0029999999999997,"g_ci":[3.0029999999999997,3.0029999999999997],"g_judged":true,"g_min":1.0,"g_point_min":null,"g_resamples":10000,"g_seed":2316557831640514561,"milestone":"m3","pairs":[{"in_r":false,"input":"input","losses":0,"medians_peak_pss_bytes":[300000000.0,100000000.0],"medians_t_delta_s":[0.6006,0.2],"pss_ratio":3.0,"shortest_run_s":0.5,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"b","losses":0,"medians_peak_pss_bytes":[900000000.0,400000000.0],"medians_t_delta_s":[24.024,8.0],"pss_ratio":2.25,"shortest_run_s":9.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"c","losses":0,"medians_peak_pss_bytes":[2700000000.0,1600000000.0],"medians_t_delta_s":[192.192,64.0],"pss_ratio":1.6875,"shortest_run_s":65.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"d","losses":0,"medians_peak_pss_bytes":[10800000000.0,6400000000.0],"medians_t_delta_s":[1537.536,512.0],"pss_ratio":1.6875,"shortest_run_s":513.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"held-out-b","losses":0,"medians_peak_pss_bytes":[1260000000.0,420000000.0],"medians_t_delta_s":[25.5255,8.5],"pss_ratio":3.0,"shortest_run_s":9.5,"t_ratio":3.003,"threads":8}],"r":2.093860932979724,"r_min":1.0}]}"#;

const FROZEN_CASE_12_SIZES: &str = r#"{"series":[{"code":"orbitar","coverage":"full","points":[{"atoms":2,"input":"input","median_peak_pss_bytes":100000000.0,"median_t_delta_s":0.2,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":16,"input":"b","median_peak_pss_bytes":400000000.0,"median_t_delta_s":8.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":54,"input":"c","median_peak_pss_bytes":1600000000.0,"median_t_delta_s":64.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":6400000000.0,"median_t_delta_s":512.0,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":0.9726813055275533,"t_delta_exponent":1.8521570634359281},{"code":"qe","coverage":"full","points":[{"atoms":2,"input":"input","median_peak_pss_bytes":200000000.0,"median_t_delta_s":0.4,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":16,"input":"b","median_peak_pss_bytes":800000000.0,"median_t_delta_s":32.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":54,"input":"c","median_peak_pss_bytes":3200000000.0,"median_t_delta_s":256.32,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":12864000000.0,"median_t_delta_s":4111.1552,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":0.9735887369433327,"t_delta_exponent":2.1483708012907066},{"code":"abinit","coverage":"full","points":[{"atoms":2,"input":"input","median_peak_pss_bytes":300000000.0,"median_t_delta_s":0.6006,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":16,"input":"b","median_peak_pss_bytes":900000000.0,"median_t_delta_s":24.024,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":54,"input":"c","median_peak_pss_bytes":2700000000.0,"median_t_delta_s":192.192,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":10800000000.0,"median_t_delta_s":1537.536,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":0.8231724646413126,"t_delta_exponent":1.8521570634359281}],"skipped":[],"versus":[{"code":"qe","g":4.081011503966281,"g_ci":[4.081011503966281,4.081011503966281],"g_judged":true,"g_min":1.0,"g_point_min":null,"g_resamples":10000,"g_seed":2316557831640514561,"milestone":"m3","pairs":[{"in_r":false,"input":"input","losses":0,"medians_peak_pss_bytes":[200000000.0,100000000.0],"medians_t_delta_s":[0.4,0.2],"pss_ratio":2.0,"shortest_run_s":0.5,"t_ratio":2.0,"threads":8},{"in_r":true,"input":"b","losses":0,"medians_peak_pss_bytes":[800000000.0,400000000.0],"medians_t_delta_s":[32.0,8.0],"pss_ratio":2.0,"shortest_run_s":9.0,"t_ratio":4.0,"threads":8},{"in_r":true,"input":"c","losses":0,"medians_peak_pss_bytes":[3200000000.0,1600000000.0],"medians_t_delta_s":[256.32,64.0],"pss_ratio":2.0,"shortest_run_s":65.0,"t_ratio":4.005,"threads":8},{"in_r":true,"input":"d","losses":0,"medians_peak_pss_bytes":[12864000000.0,6400000000.0],"medians_t_delta_s":[4111.1552,512.0],"pss_ratio":2.01,"shortest_run_s":513.0,"t_ratio":8.0296,"threads":8},{"in_r":true,"input":"held-out-b","losses":0,"medians_peak_pss_bytes":[840000000.0,420000000.0],"medians_t_delta_s":[37.4,8.5],"pss_ratio":2.0,"shortest_run_s":9.5,"t_ratio":4.3999999999999995,"threads":8}],"r":2.0024953261250533,"r_min":1.0},{"code":"abinit","g":3.0029999999999997,"g_ci":[3.0029999999999997,3.0029999999999997],"g_judged":true,"g_min":1.0,"g_point_min":null,"g_resamples":10000,"g_seed":2316557831640514561,"milestone":"m3","pairs":[{"in_r":false,"input":"input","losses":0,"medians_peak_pss_bytes":[300000000.0,100000000.0],"medians_t_delta_s":[0.6006,0.2],"pss_ratio":3.0,"shortest_run_s":0.5,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"b","losses":0,"medians_peak_pss_bytes":[900000000.0,400000000.0],"medians_t_delta_s":[24.024,8.0],"pss_ratio":2.25,"shortest_run_s":9.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"c","losses":0,"medians_peak_pss_bytes":[2700000000.0,1600000000.0],"medians_t_delta_s":[192.192,64.0],"pss_ratio":1.6875,"shortest_run_s":65.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"d","losses":0,"medians_peak_pss_bytes":[10800000000.0,6400000000.0],"medians_t_delta_s":[1537.536,512.0],"pss_ratio":1.6875,"shortest_run_s":513.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"held-out-b","losses":0,"medians_peak_pss_bytes":[1260000000.0,420000000.0],"medians_t_delta_s":[25.5255,8.5],"pss_ratio":3.0,"shortest_run_s":9.5,"t_ratio":3.003,"threads":8}],"r":2.093860932979724,"r_min":1.0}]}"#;

fn frozen_record_collects(name: &str, sized: &[Cell], frozen: &str) {
    let root = claim_cases(name, sized, Claims::Memory);
    let head = commit(&root, "head");
    let mut record = measured_record(&head, sized);
    let frozen: Value = serde_json::from_str(frozen).unwrap();
    for key in ["versus", "series", "skipped"] {
        record[key] = frozen[key].clone();
    }
    assert_eq!(collected(&root, &record), Vec::<String>::new());
}

#[test]
fn a_claim_record_whose_summaries_the_reviewed_producer_wrote_still_collects_bit_for_bit() {
    frozen_record_collects("claim-frozen", &cells(), FROZEN_SUMMARIES);
    let mut sized = cells();
    for (cell, atoms) in sized.iter_mut().zip([2, 16, 54, 128, 16]) {
        cell.atoms = atoms;
    }
    frozen_record_collects("claim-frozen-12", &sized, FROZEN_CASE_12_SIZES);
}

const FROZEN_ZERO_ATOMS: &str = r#"{"series":[{"code":"orbitar","coverage":"full","points":[{"atoms":0,"input":"input","median_peak_pss_bytes":100000000.0,"median_t_delta_s":0.2,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":8,"input":"b","median_peak_pss_bytes":400000000.0,"median_t_delta_s":8.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":32,"input":"c","median_peak_pss_bytes":1600000000.0,"median_t_delta_s":64.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":6400000000.0,"median_t_delta_s":512.0,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":"NaN","t_delta_exponent":"NaN"},{"code":"qe","coverage":"full","points":[{"atoms":0,"input":"input","median_peak_pss_bytes":200000000.0,"median_t_delta_s":0.4,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":8,"input":"b","median_peak_pss_bytes":800000000.0,"median_t_delta_s":32.0,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":32,"input":"c","median_peak_pss_bytes":3200000000.0,"median_t_delta_s":256.32,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":12864000000.0,"median_t_delta_s":4111.1552,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":"NaN","t_delta_exponent":"NaN"},{"code":"abinit","coverage":"full","points":[{"atoms":0,"input":"input","median_peak_pss_bytes":300000000.0,"median_t_delta_s":0.6006,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":8,"input":"b","median_peak_pss_bytes":900000000.0,"median_t_delta_s":24.024,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":32,"input":"c","median_peak_pss_bytes":2700000000.0,"median_t_delta_s":192.192,"pss_polled_runs":[5,5],"status":"measured"},{"atoms":128,"input":"d","median_peak_pss_bytes":10800000000.0,"median_t_delta_s":1537.536,"pss_polled_runs":[5,5],"status":"measured"}],"pss_exponent":"NaN","t_delta_exponent":"NaN"}],"skipped":[],"versus":[{"code":"qe","g":4.081011503966281,"g_ci":[4.081011503966281,4.081011503966281],"g_judged":true,"g_min":1.0,"g_point_min":null,"g_resamples":10000,"g_seed":2316557831640514561,"milestone":"m3","pairs":[{"in_r":false,"input":"input","losses":0,"medians_peak_pss_bytes":[200000000.0,100000000.0],"medians_t_delta_s":[0.4,0.2],"pss_ratio":2.0,"shortest_run_s":0.5,"t_ratio":2.0,"threads":8},{"in_r":true,"input":"b","losses":0,"medians_peak_pss_bytes":[800000000.0,400000000.0],"medians_t_delta_s":[32.0,8.0],"pss_ratio":2.0,"shortest_run_s":9.0,"t_ratio":4.0,"threads":8},{"in_r":true,"input":"c","losses":0,"medians_peak_pss_bytes":[3200000000.0,1600000000.0],"medians_t_delta_s":[256.32,64.0],"pss_ratio":2.0,"shortest_run_s":65.0,"t_ratio":4.005,"threads":8},{"in_r":true,"input":"d","losses":0,"medians_peak_pss_bytes":[12864000000.0,6400000000.0],"medians_t_delta_s":[4111.1552,512.0],"pss_ratio":2.01,"shortest_run_s":513.0,"t_ratio":8.0296,"threads":8},{"in_r":true,"input":"held-out-b","losses":0,"medians_peak_pss_bytes":[840000000.0,420000000.0],"medians_t_delta_s":[37.4,8.5],"pss_ratio":2.0,"shortest_run_s":9.5,"t_ratio":4.3999999999999995,"threads":8}],"r":2.0024953261250533,"r_min":1.0},{"code":"abinit","g":3.0029999999999997,"g_ci":[3.0029999999999997,3.0029999999999997],"g_judged":true,"g_min":1.0,"g_point_min":null,"g_resamples":10000,"g_seed":2316557831640514561,"milestone":"m3","pairs":[{"in_r":false,"input":"input","losses":0,"medians_peak_pss_bytes":[300000000.0,100000000.0],"medians_t_delta_s":[0.6006,0.2],"pss_ratio":3.0,"shortest_run_s":0.5,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"b","losses":0,"medians_peak_pss_bytes":[900000000.0,400000000.0],"medians_t_delta_s":[24.024,8.0],"pss_ratio":2.25,"shortest_run_s":9.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"c","losses":0,"medians_peak_pss_bytes":[2700000000.0,1600000000.0],"medians_t_delta_s":[192.192,64.0],"pss_ratio":1.6875,"shortest_run_s":65.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"d","losses":0,"medians_peak_pss_bytes":[10800000000.0,6400000000.0],"medians_t_delta_s":[1537.536,512.0],"pss_ratio":1.6875,"shortest_run_s":513.0,"t_ratio":3.003,"threads":8},{"in_r":true,"input":"held-out-b","losses":0,"medians_peak_pss_bytes":[1260000000.0,420000000.0],"medians_t_delta_s":[25.5255,8.5],"pss_ratio":3.0,"shortest_run_s":9.5,"t_ratio":3.003,"threads":8}],"r":2.093860932979724,"r_min":1.0}]}"#;

#[test]
fn a_record_with_a_measured_cell_of_no_atoms_keeps_its_undefined_exponents() {
    let mut zero = cells();
    zero[0].atoms = 0;
    let root = claim_cases("claim-frozen-zero", &zero, Claims::Memory);
    let head = commit(&root, "head");
    let mut record = measured_record(&head, &zero);
    let first = speed_block(&mut record, "orbitar", "input");
    first["e_ref_shift_ha"] = Value::Null;
    first["rival"] = json!({});
    let frozen: Value = serde_json::from_str(FROZEN_ZERO_ATOMS).unwrap();
    for key in ["versus", "series", "skipped"] {
        record[key] = frozen[key].clone();
    }
    assert_eq!(record["series"][0]["t_delta_exponent"], "NaN");
    assert_eq!(collected(&root, &record), Vec::<String>::new());
}

pub(super) fn lost(record: &mut Value, code: &str) {
    for b in (record["speed"].as_array_mut().unwrap().iter_mut()).filter(|b| b["code"] == code) {
        for key in [
            "t_delta_s",
            "wall_s",
            "peak_rss_bytes",
            "peak_pss_bytes",
            "steps",
        ] {
            b[key] = json!([]);
        }
        b["losses"] = json!(vec![format!("{code}: lost"); 5]);
        let block = b.as_object_mut().unwrap();
        for key in [
            "median_t_delta_s",
            "range_t_delta_s",
            "cv_t_delta",
            "median_peak_rss_bytes",
            "median_peak_pss_bytes",
        ] {
            block.remove(key);
        }
    }
}

pub(super) fn solo_memory_case(root: &Tree, head: &str) {
    if read(root, "benchmarks/RIVALS.toml").is_empty() {
        write(
            root,
            "benchmarks/RIVALS.toml",
            crate::harness::rival::RIVALS,
        );
    }
    write(
        root,
        "benchmarks/4_d/case.toml",
        "[case]\ntier = \"claim\"\nstatus = \"active\"\nclaims = [\"speed\", \"memory\", \"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nf_error_max = 1e-9\nrss_exponent_max = 2.0\n",
    );
    let solo: Vec<Cell> = (cells().into_iter())
        .filter(|c| c.base.is_none())
        .map(|c| Cell {
            runs: [c.runs[0], None, None],
            ..c
        })
        .collect();
    for c in &solo {
        let path = match c.input {
            "input" => "benchmarks/4_d/input.toml".to_owned(),
            name => format!("benchmarks/4_d/inputs/{name}.toml"),
        };
        write(
            root,
            &path,
            &format!("[structure]\nspecies = {:?}\n", vec!["Si"; c.atoms]),
        );
    }
    let what = read(root, "benchmarks/results-what.toml");
    let (before, after) = what.split_once("[4]\n").unwrap();
    let rest = after
        .split_once("\n[")
        .map_or(String::new(), |(_, r)| format!("\n[{r}"));
    write(
        root,
        "benchmarks/results-what.toml",
        &format!(
            "{before}[4]\nwhat = \"F vs reference; peak memory alone\"\nmetric = [\"f_error\"]\nunit = \"Ha/atom\"\n{rest}"
        ),
    );
    let mut record = measured_record(head, &solo);
    record["case"] = json!("4_d");
    record["metrics"] = json!({ "f_error": 1e-10, "rss_exponent": 1.0 });
    record["tolerance"] = json!({ "f_error_max": 1e-9, "rss_exponent_max": 2.0 });
    record["held_out"] = json!([]);
    record["rotation_s"] = json!(vec![0.1; solo.len()]);
    write(
        root,
        "target/benchmarks/claim/4_d.json",
        &summed(root, record).to_string(),
    );
}
