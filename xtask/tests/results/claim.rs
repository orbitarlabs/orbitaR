use super::{WHAT, cases, commit, read, s11};
use crate::{Tree, has, lever, write};
use serde_json::{Value, json};
use std::path::Path;

const CLAIM: &str = "[case]\ntier = \"claim\"\nstatus = \"active\"\nclaims = [\"speed\", \"memory\", \"correctness\"]\nreference = \"analytic\"\nrivals = [\"qe\", \"abinit\"]\n\n[tolerance]\nf_error_max = 1e-9\n";
pub(super) const RECORD: &str = "target/benchmarks/claim/5_e.json";
const CODES: [&str; 3] = ["orbitar", "qe", "abinit"];
const SKIP: &str = "no dataset converges it";
const HELD: &str = "held-out-e";

#[derive(Clone)]
pub(super) struct Cell {
    pub(super) input: &'static str,
    pub(super) atoms: usize,
    pub(super) runs: [Option<(f64, u64)>; 3],
    pub(super) long: bool,
}

pub(super) fn cells() -> Vec<Cell> {
    vec![
        Cell {
            input: "input",
            atoms: 2,
            runs: [
                Some((0.2, 100_000_000)),
                Some((0.4, 200_000_000)),
                Some((0.6006, 300_000_000)),
            ],
            long: false,
        },
        Cell {
            input: "b",
            atoms: 8,
            runs: [
                Some((8.0, 400_000_000)),
                Some((32.0, 800_000_000)),
                Some((24.024, 900_000_000)),
            ],
            long: true,
        },
        Cell {
            input: "c",
            atoms: 32,
            runs: [
                Some((64.0, 1_600_000_000)),
                None,
                Some((192.192, 2_700_000_000)),
            ],
            long: true,
        },
        Cell {
            input: "d",
            atoms: 128,
            runs: [
                Some((512.0, 6_400_000_000)),
                Some((4111.1552, 12_864_000_000)),
                Some((1537.536, 10_800_000_000)),
            ],
            long: true,
        },
        Cell {
            input: HELD,
            atoms: 4,
            runs: [
                Some((1.0, 200_000_000)),
                Some((4.4, 400_000_000)),
                Some((3.003, 600_000_000)),
            ],
            long: true,
        },
    ]
}

fn path(input: &str) -> String {
    match input {
        "input" => "benchmarks/5_e/input.toml".to_owned(),
        name => format!("benchmarks/5_e/inputs/{name}.toml"),
    }
}

pub(super) fn claim_cases(name: &str, cells: &[Cell]) -> Tree {
    let root = cases(name);
    write(&root, "benchmarks/5_e/case.toml", CLAIM);
    write(
        &root,
        "benchmarks/results-what.toml",
        &format!(
            "{WHAT}\n[5]\nwhat = \"F vs reference; speed vs rivals\"\nmetric = [\"f_error\"]\nunit = \"Ha/atom\"\n"
        ),
    );
    for cell in cells.iter().filter(|c| c.input != HELD) {
        write(
            &root,
            &path(cell.input),
            &format!("[structure]\nspecies = {:?}\n", vec!["Si"; cell.atoms]),
        );
    }
    let mut skips = String::new();
    for cell in cells.iter().filter(|c| c.runs[1].is_none()) {
        skips.push_str(&format!(
            "{{ cell = \"{}\", reason = \"{SKIP}\" }}, ",
            path(cell.input)
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
    json!({
        "code": code, "input": input, "threads": 8, "atoms": atoms,
        "e_ref_ha": -2.0, "e_ref_shift_ha": if orbitar { json!(0.0) } else { Value::Null },
        "delta_ha": 1e-4 / 27.211_386_245_988 * atoms as f64, "warmup": null,
        "t_delta_s": vec![t; 5], "wall_s": vec![wall; 5], "peak_rss_bytes": vec![pss; 5],
        "peak_pss_bytes": vec![pss; 5], "losses": [], "noisy": false, "median_t_delta_s": t,
        "range_t_delta_s": [t, t], "cv_t_delta": 0.0, "median_peak_rss_bytes": pss as f64,
        "median_peak_pss_bytes": pss as f64,
        "rival": if orbitar { Value::Null } else { json!({}) }, "accuracy": null, "step_cap": null,
        "steps": vec![9; 5], "e_ref_steps": [8, 9],
    })
}

pub(super) fn measured_record(commit: &str, cells: &[Cell]) -> Value {
    let mut speed = Vec::new();
    for cell in cells {
        for (code, runs) in CODES.iter().zip(&cell.runs) {
            if let Some((t, pss)) = runs {
                speed.push(block(code, cell.input, cell.atoms, *t, *pss, cell.long));
            }
        }
    }
    let inputs: Vec<&str> = cells.iter().map(|c| c.input).collect();
    let runs = 5 * cells.len();
    json!({
        "schema": 4, "case": "5_e", "tier": "claim", "commit": commit, "dirty": false, "pilot": false,
        "engine_sha256": "cd".repeat(32), "outcome": "pass", "failures": [], "filter": null,
        "metrics": { "f_error": 1e-10 }, "tolerance": { "f_error_max": 1e-9 },
        "inputs": inputs, "held_out": [{ "input": HELD, "sha256": "ab".repeat(32) }],
        "runs_s": vec![0.1; runs], "peak_rss_bytes": vec![1; runs],
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
    record["series"] = made.series;
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

#[test]
fn a_passing_claim_record_keeps_the_comparisons_series_and_skips_that_its_samples_give() {
    let root = claim_cases("claim-table", &cells());
    let head = commit(&root, "head");
    let record = claim_record(&root, &head, &cells());
    assert_eq!(collected(&root, &record), Vec::<String>::new());
    let results = read(&root, "benchmarks/RESULTS.toml");
    for text in [
        "[[case.claim.versus]]\nrival = \"qe\"\n",
        "[[case.claim.versus.pair]]\ninput = \"input\"\nthreads = 8\nt_delta_s_rival = 6.006e-1\nt_delta_s_orbitar = 2e-1\npeak_pss_bytes_rival = 3e8\npeak_pss_bytes_orbitar = 1e8\nin_r = false\nlosses = 0\n",
        "input = \"held-out-e\"\nthreads = 8\n",
        "[[case.claim.series]]\ncode = \"qe\"\ncoverage = \"partial\"\n",
        "[[case.claim.series.point]]\ninput = \"c\"\natoms = 32\nstatus = \"skipped: no dataset converges it\"\n\n",
        "[[case.claim.series.point]]\ninput = \"d\"\natoms = 128\nstatus = \"measured\"\nt_delta_s = 1.537536e3\npeak_pss_bytes = 1.08e10\n",
        "[[case.claim.skip]]\ncode = \"qe\"\ninput = \"c\"\natoms = 32\nkind = \"skipped\"\nreason = \"no dataset converges it\"\n",
        "[case.claim.node.host]\ncore_cpus = [\"0\", \"1\"]\ncores = 8\ncpu = \"Stand-in CPU\"\nmemory_bytes = 16762847232\n",
        "[case.claim.node.qe]\n\"a b\" = \"1 x sha256:02\"\nlibgfortran-ng = \"14.2 h0 sha256:01\"\n",
    ] {
        assert!(results.contains(text), "{text}: {results}");
    }
    assert!(
        !results.contains("input = \"held-out-e\"\natoms"),
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
fn a_claim_whose_samples_fail_the_bar_is_refused_whatever_summary_it_submits() {
    let root = claim_cases("claim-bar", &cells());
    let head = commit(&root, "head");
    let good = claim_record(&root, &head, &cells());
    let mut close = cells();
    for cell in &mut close {
        let (t, pss) = cell.runs[0].unwrap();
        cell.runs[2] = Some((t * 0.999_999_999_9, pss * 2));
    }
    let mut record = measured_record(&head, &close);
    record["versus"] = good["versus"].clone();
    record["versus"][1]["g"] = json!(1.0);
    record["versus"][1]["g_ci"] = json!([1.0, 1.0]);
    record["skipped"] = good["skipped"].clone();
    record["series"] = good["series"].clone();
    let found = collected(&root, &record);
    assert!(
        has(&found, "G against abinit = 0.99999999")
            && has(&found, "whose lower bound is not at least 1"),
        "{found:?}"
    );
    assert!(!root.join("benchmarks/RESULTS.toml").exists());
}

#[test]
fn a_submitted_summary_must_equal_bit_for_bit_what_the_samples_give() {
    let root = claim_cases("claim-exact", &cells());
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
            edit(&|r| r["skipped"] = json!([])),
            "the record's skipped holds 0 entries, but its speed blocks give 1",
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
}

#[test]
fn the_pairs_follow_the_case_manifest_of_declared_inputs_and_held_out_names() {
    let root = claim_cases("claim-manifest", &cells());
    let head = commit(&root, "head");
    let good = claim_record(&root, &head, &cells());
    let renamed = {
        let mut r = good.clone();
        r["inputs"][4] = json!("held-out-x");
        r["held_out"][0]["input"] = json!("held-out-x");
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
            "the speed block of orbitar held-out-e at 8 threads is not in the case's manifest",
        ),
        (
            dropped,
            "the record's inputs [\"input\",\"b\",\"d\",\"held-out-e\"] are not the case's inputs and held-out names",
        ),
        (doubled, "the harness's claim proof refuses it"),
    ] {
        let found = collected(&root, &record);
        assert!(has(&found, why), "{why}: {found:?}");
    }
}

#[test]
fn a_block_must_hold_its_declared_cell_and_sum_up_its_own_samples() {
    let root = claim_cases("claim-blocks", &cells());
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
    let root = claim_cases("claim-s11", &cells());
    let head = commit(&root, "head");
    assert!(collected(&root, &claim_record(&root, &head, &cells())).is_empty());
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
