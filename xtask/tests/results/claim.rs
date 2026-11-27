use super::{WHAT, cases, commit, read, s11};
use crate::{Tree, has, lever, write};
use serde_json::{Value, json};

const CLAIM: &str = "[case]\ntier = \"claim\"\nstatus = \"active\"\nclaims = [\"speed\", \"correctness\"]\nreference = \"analytic\"\nrivals = [\"qe\", \"abinit\"]\n\n[tolerance]\nf_error_max = 1e-9\n";
pub(super) const RECORD: &str = "target/benchmarks/claim/5_e.json";
const CODES: [&str; 3] = ["orbitar", "qe", "abinit"];
const SKIP: &str = "no dataset converges it";

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
                Some((0.6, 300_000_000)),
            ],
            long: false,
        },
        Cell {
            input: "b",
            atoms: 8,
            runs: [
                Some((8.0, 400_000_000)),
                Some((32.0, 800_000_000)),
                Some((24.0, 900_000_000)),
            ],
            long: true,
        },
        Cell {
            input: "c",
            atoms: 32,
            runs: [
                Some((64.0, 1_600_000_000)),
                None,
                Some((192.0, 2_700_000_000)),
            ],
            long: true,
        },
        Cell {
            input: "d",
            atoms: 128,
            runs: [
                Some((512.0, 6_400_000_000)),
                Some((4096.0, 12_800_000_000)),
                Some((1536.0, 10_800_000_000)),
            ],
            long: true,
        },
    ]
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
    let mut skips = String::new();
    for cell in cells.iter().filter(|c| c.runs[1].is_none()) {
        let path = match cell.input {
            "input" => "benchmarks/5_e/input.toml".to_owned(),
            name => format!("benchmarks/5_e/inputs/{name}.toml"),
        };
        skips.push_str(&format!("{{ cell = \"{path}\", reason = \"{SKIP}\" }}, "));
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

fn tidy(x: f64) -> f64 {
    (x * 1e12).round() / 1e12
}

fn block(code: &str, cell: &Cell, t: f64, pss: u64) -> Value {
    let wall = if cell.long { t + 1.0 } else { t + 0.3 };
    let orbitar = code == "orbitar";
    json!({
        "code": code, "input": cell.input, "threads": 8, "atoms": cell.atoms,
        "e_ref_ha": -2.0, "e_ref_shift_ha": if orbitar { json!(0.0) } else { Value::Null },
        "delta_ha": 1e-4 / 27.211_386_245_988 * cell.atoms as f64, "warmup": null,
        "t_delta_s": vec![t; 5], "wall_s": vec![wall; 5], "peak_rss_bytes": vec![pss; 5], "peak_pss_bytes": vec![pss; 5],
        "losses": [], "noisy": false, "median_t_delta_s": t, "range_t_delta_s": [t, t], "cv_t_delta": 0.0,
        "median_peak_rss_bytes": pss as f64, "median_peak_pss_bytes": pss as f64,
        "rival": if orbitar { Value::Null } else { json!({}) }, "accuracy": null, "step_cap": null,
        "steps": vec![9; 5], "e_ref_steps": [8, 9],
    })
}

fn slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0.ln()).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1.ln()).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0.ln() - mx).powi(2)).sum();
    let sxy: f64 = points
        .iter()
        .map(|p| (p.0.ln() - mx) * (p.1.ln() - my))
        .sum();
    sxy / sxx
}

pub(super) fn claim_record(commit: &str, cells: &[Cell]) -> Value {
    let mut speed = Vec::new();
    let mut skipped = Vec::new();
    for cell in cells {
        for (code, runs) in CODES.iter().zip(&cell.runs) {
            match runs {
                Some((t, pss)) => speed.push(block(code, cell, *t, *pss)),
                None => skipped.push(json!({ "code": code, "input": cell.input, "reason": SKIP })),
            }
        }
    }
    let mut versus = Vec::new();
    for (k, code) in CODES.iter().enumerate().skip(1) {
        let (mut pairs, mut t, mut m) = (Vec::new(), Vec::new(), Vec::new());
        for cell in cells {
            let (Some((to, po)), Some((tr, pr))) = (cell.runs[0], cell.runs[k]) else {
                continue;
            };
            t.push(tr / to);
            if cell.long {
                m.push(pr as f64 / po as f64);
            }
            pairs.push(json!({
                "input": cell.input, "threads": 8, "medians_t_delta_s": [tr, to],
                "medians_peak_pss_bytes": [pr as f64, po as f64], "in_r": cell.long, "losses": 0,
            }));
        }
        let mean = |v: &[f64]| tidy((v.iter().map(|x| x.ln()).sum::<f64>() / v.len() as f64).exp());
        let g = mean(&t);
        versus.push(json!({ "code": code, "g": g, "g_ci": [g, g], "r": mean(&m), "pairs": pairs }));
    }
    let mut series = Vec::new();
    for (k, code) in CODES.iter().enumerate() {
        let points: Vec<(&Cell, f64, u64)> = (cells.iter())
            .filter_map(|c| c.runs[k].map(|(t, pss)| (c, t, pss)))
            .collect();
        let fit = |y: fn(f64, u64) -> f64| {
            let xy: Vec<(f64, f64)> = points
                .iter()
                .map(|(c, t, p)| (c.atoms as f64, y(*t, *p)))
                .collect();
            slope(&xy)
        };
        series.push(json!({
            "code": code,
            "t_delta_exponent": fit(|t, _| t),
            "pss_exponent": fit(|_, p| p as f64),
            "points": points.iter().map(|(c, t, p)| json!({
                "input": c.input, "atoms": c.atoms, "median_t_delta_s": t, "median_peak_pss_bytes": *p as f64,
            })).collect::<Vec<_>>(),
        }));
    }
    let inputs: Vec<&str> = cells.iter().map(|c| c.input).collect();
    let runs = 5 * cells.len();
    json!({
        "schema": 4, "case": "5_e", "tier": "claim", "commit": commit, "dirty": false, "pilot": false,
        "engine_sha256": "cd".repeat(32), "outcome": "pass", "failures": [], "filter": null,
        "metrics": { "f_error": 1e-10 }, "tolerance": { "f_error_max": 1e-9 },
        "inputs": inputs, "held_out": [inputs[inputs.len() - 1]], "runs_s": vec![0.1; runs],
        "peak_rss_bytes": vec![1; runs], "rotation_s": vec![0.1; cells.len() - 1], "rotation_max": 0.0,
        "rotation_used": 0.0, "seed_fixed": false, "speed": speed, "skipped": skipped, "versus": versus,
        "series": series,
        "node": {
            "host": { "cpu": "Stand-in CPU", "cores": 8, "core_cpus": ["0", "1"], "memory_bytes": 16_762_847_232_u64 },
            "orbitar": { "build": "cargo build --release", "profile_release": { "lto": "thin" } },
            "qe": { "qe": "7.5 h19104ac_2 sha256:00", "libgfortran-ng": "14.2 h0 sha256:01", "a b": "1 x sha256:02" },
            "abinit": { "abinit": "10.0.3 h2b8cb88_2 sha256:03" },
        },
    })
}

pub(super) fn collected(root: &Tree, record: &Value) -> Vec<String> {
    write(root, RECORD, &record.to_string());
    lever(&["results", "collect"], root)
}

#[test]
fn a_passing_claim_record_keeps_its_comparisons_its_series_and_its_skips_in_a_claim_table() {
    let root = claim_cases("claim-table", &cells());
    let head = commit(&root, "head");
    assert_eq!(
        collected(&root, &claim_record(&head, &cells())),
        Vec::<String>::new()
    );
    let results = read(&root, "benchmarks/RESULTS.toml");
    for text in [
        "[[case.claim.versus]]\nrival = \"qe\"\ng = 4e0\ng_ci = [4e0, 4e0]\nr = 2e0\n",
        "[[case.claim.versus.pair]]\ninput = \"input\"\nthreads = 8\nt_delta_s_rival = 6e-1\nt_delta_s_orbitar = 2e-1\npeak_pss_bytes_rival = 3e8\npeak_pss_bytes_orbitar = 1e8\nin_r = false\nlosses = 0\n",
        "[[case.claim.series]]\ncode = \"qe\"\nt_delta_exponent = 2.1531325849839087e0\n",
        "[[case.claim.series.point]]\ninput = \"d\"\natoms = 128\nt_delta_s = 1.536e3\npeak_pss_bytes = 1.08e10\n",
        "[[case.claim.skip]]\ncode = \"qe\"\ninput = \"c\"\natoms = 32\nkind = \"skipped\"\nreason = \"no dataset converges it\"\n",
        "[case.claim.node.host]\ncore_cpus = [\"0\", \"1\"]\ncores = 8\ncpu = \"Stand-in CPU\"\nmemory_bytes = 16762847232\n",
        "[case.claim.node.qe]\n\"a b\" = \"1 x sha256:02\"\nlibgfortran-ng = \"14.2 h0 sha256:01\"\n",
    ] {
        assert!(results.contains(text), "{text}: {results}");
    }
    let pinned = read(&root, "benchmarks/RECORDS.jsonl");
    assert_eq!(pinned, format!("{}\n", read(&root, RECORD)));
    assert!(lever(&["results", "render"], &root).is_empty());
    assert_eq!(s11(&root), Vec::<String>::new());
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert_eq!(read(&root, "benchmarks/RESULTS.toml"), results);
}

#[test]
fn a_claim_record_that_the_harness_proof_or_its_own_speed_blocks_refute_is_refused_by_name() {
    let root = claim_cases("claim-refused", &cells());
    let head = commit(&root, "head");
    let good = claim_record(&head, &cells());
    let edit = |f: &dyn Fn(&mut Value)| {
        let mut record = good.clone();
        f(&mut record);
        record
    };
    let cases: Vec<(Value, &str)> = vec![
        (
            edit(&|r| r["failures"] = json!(["a block failed"])),
            "5_e says pass, but names failures",
        ),
        (
            edit(&|r| r["filter"] = json!(["b"])),
            "5_e says pass, but names failures [] or a filter [\"b\"]",
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
            edit(&|r| r["versus"][0]["g"] = json!(4.5)),
            "versus qe: g is 4.5e0, but its speed blocks give",
        ),
        (
            edit(&|r| r["versus"][0]["pairs"][1]["medians_t_delta_s"][0] = json!(33.0)),
            "versus qe: medians_t_delta_s is 3.3e1, but its speed blocks give 3.2e1",
        ),
        (
            edit(&|r| r["versus"][0]["pairs"][0]["in_r"] = json!(true)),
            "versus qe: the pair \"input\" differs from its speed blocks in input, threads, in_r or losses",
        ),
        (
            edit(&|r| {
                let copy = r["speed"][1].clone();
                r["speed"].as_array_mut().unwrap().push(copy);
            }),
            "the harness's claim proof refuses it",
        ),
        (
            edit(&|r| r["skipped"] = json!([])),
            "qe c: neither a speed block nor a recorded skip",
        ),
        (
            edit(&|r| {
                r["series"].as_array_mut().unwrap().remove(2);
            }),
            "series abinit: input has neither a point nor a recorded skip",
        ),
        (
            edit(&|r| r["series"][1]["points"][1]["median_t_delta_s"] = json!(30.0)),
            "series qe: the point of b differs from its speed block's atoms and medians",
        ),
        (
            edit(&|r| r["series"][0]["t_delta_exponent"] = json!(1.9)),
            "series orbitar: the T(δ) exponent is 1.9e0, but its points give",
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
fn science_s11_derives_each_claim_row_again_from_its_pinned_record() {
    let root = claim_cases("claim-s11", &cells());
    let head = commit(&root, "head");
    assert!(collected(&root, &claim_record(&head, &cells())).is_empty());
    assert!(lever(&["results", "render"], &root).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    let changed = "differs from what its record in benchmarks/RECORDS.jsonl gives";
    for (from, to) in [
        ("g = 4e0\ng_ci = [4e0, 4e0]", "g = 5e0\ng_ci = [5e0, 5e0]"),
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
