use crate::{has, lever, tree, write};
use std::{path::Path, time::UNIX_EPOCH};
use xtask::harness::{Tier, verify};

const ACTIVE: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nerror_max = 1e-12\n";
const GOOD: &str = "{\"schema\": 4, \"case\": \"1_a\", \"outcome\": \"pass\", \"inputs\": [\"input\"], \"runs_s\": [0.1], \"rotation_s\": [0.1], \"rotation_max\": 0.0, \"rotation_used\": 0.0, \"rotation_field\": null, \"spent_s\": 0.2, \"started_unix_ms\": 1, \"seed_fixed\": false, \"peak_rss_bytes\": [1]}";

fn findings(root: &Path) -> Vec<String> {
    verify(root, Tier::Push, &root.join("records"), UNIX_EPOCH)
}

#[test]
fn a_tier_with_no_active_case_proves_nothing() {
    let root = tree(
        "record-empty",
        &[(
            "benchmarks/1_a/case.toml",
            &ACTIVE.replace("active", "planned"),
        )],
    );
    assert!(has(&findings(&root), "no active case"));
}

#[test]
fn a_missing_failing_or_hollow_record_fails_the_tier() {
    let root = tree("record-bad", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    assert!(has(&findings(&root), "1_a left no readable record"));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"pass\"", "\"fail\""),
    );
    assert!(has(&findings(&root), "1_a fail"));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("[0.1], \"rotation_s\"", "[], \"rotation_s\""),
    );
    assert!(has(&findings(&root), "0 timed runs"));
    let hollow = GOOD
        .replace("\"inputs\": [\"input\"]", "\"inputs\": []")
        .replace("\"runs_s\": [0.1]", "\"runs_s\": []")
        .replace("\"peak_rss_bytes\": [1]", "\"peak_rss_bytes\": []");
    write(&root, "records/1_a.json", &hollow);
    assert!(has(&findings(&root), "0 timed runs for 0 inputs"));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"peak_rss_bytes\": [1]", "\"peak_rss_bytes\": []"),
    );
    let f = findings(&root);
    assert!(
        has(&f, "record has 0 peak RSS values for 1 timed runs") && !has(&f, "timed runs for"),
        "{f:?}"
    );
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"rotation_max\": 0.0", "\"rotation_max\": null"),
    );
    assert!(has(&findings(&root), "no rotated rerun"));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"rotation_used\": 0.0", "\"rotation_used\": null"),
    );
    assert!(has(&findings(&root), "no rotated rerun"));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"rotation_s\": [0.1]", "\"rotation_s\": []"),
    );
    assert!(has(&findings(&root), "no rotated rerun"));
}

#[test]
fn unreviewed_hashes_fail_the_tier() {
    let root = tree(
        "record-lock",
        &[
            ("benchmarks/1_a/case.toml", ACTIVE),
            ("records/1_a.json", GOOD),
        ],
    );
    let f = findings(&root);
    assert!(has(&f, "1_a is not in benchmarks/LOCK"), "{f:?}");
    assert!(has(&f, "the harness is not in benchmarks/LOCK"), "{f:?}");
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "r"], &root).is_empty());
    write(
        &root,
        "benchmarks/1_a/case.toml",
        &ACTIVE.replace("1e-12", "1e-11"),
    );
    let f = findings(&root);
    assert!(has(&f, "1_a changed since its LOCK review"), "{f:?}");
    assert!(!has(&f, "the harness"), "{f:?}");
}

#[test]
fn a_lock_that_does_not_parse_is_named_once() {
    let lock = "# harness reviewer=a reason=b\n# harness reviewer=c reason=d\n";
    let root = tree(
        "record-lock-twice",
        &[
            ("benchmarks/1_a/case.toml", ACTIVE),
            ("benchmarks/2_b/case.toml", ACTIVE),
            ("benchmarks/LOCK", lock),
            ("records/1_a.json", GOOD),
            ("records/2_b.json", &GOOD.replace("1_a", "2_b")),
        ],
    );
    let f = findings(&root);
    let named: Vec<_> = f
        .iter()
        .filter(|x| x.contains("lists harness twice"))
        .collect();
    assert_eq!(named.len(), 1, "{f:?}");
    assert!(!has(&f, "LOCK review"), "{f:?}");
}

#[test]
fn the_tier_budget_is_enforced() {
    let root = tree(
        "record-budget",
        &[
            ("benchmarks/1_a/case.toml", ACTIVE),
            (
                "records/1_a.json",
                &GOOD.replace("\"spent_s\": 0.2", "\"spent_s\": 61.0"),
            ),
        ],
    );
    assert!(has(&findings(&root), "exceeds the 60 s budget"));
}

#[test]
fn a_record_must_come_from_this_run_with_a_random_seed() {
    let root = tree("record-fresh", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    write(&root, "records/1_a.json", GOOD);
    let later = UNIX_EPOCH + std::time::Duration::from_millis(2);
    let f = verify(&root, Tier::Push, &root.join("records"), later);
    assert!(has(&f, "1_a left a stale record"), "{f:?}");
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"seed_fixed\": false", "\"seed_fixed\": true"),
    );
    assert!(has(&findings(&root), "1_a ran with a fixed rotation seed"));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"case\": \"1_a\"", "\"case\": \"2_b\""),
    );
    assert!(has(
        &findings(&root),
        "1_a record has the wrong schema or case"
    ));
}

#[test]
fn a_record_without_engine_seconds_spends_the_whole_budget() {
    let root = tree(
        "record-spent",
        &[
            ("benchmarks/1_a/case.toml", ACTIVE),
            ("records/1_a.json", &GOOD.replace("\"spent_s\": 0.2, ", "")),
        ],
    );
    assert!(has(
        &findings(&root),
        "engine time inf s exceeds the 60 s budget"
    ));
}

#[test]
fn a_planned_case_that_passes_fails_the_tier() {
    let root = tree(
        "record-promote",
        &[
            (
                "benchmarks/1_a/case.toml",
                &ACTIVE.replace("active", "planned"),
            ),
            ("records/1_a.json", &GOOD.replace("\"pass\"", "\"promote\"")),
        ],
    );
    assert!(has(&findings(&root), "planned but passes"));
}

#[test]
fn a_good_record_passes_and_a_retired_case_needs_none() {
    let twice = ACTIVE.replace("analytic\"\n", "analytic\"\nrepeats = 2\n");
    let retired = ACTIVE.replace("active", "retired");
    let record = GOOD
        .replace("\"runs_s\": [0.1]", "\"runs_s\": [0.1, 0.1]")
        .replace("\"peak_rss_bytes\": [1]", "\"peak_rss_bytes\": [1, 1]")
        .replace("\"started_unix_ms\": 1", "\"started_unix_ms\": 0")
        .replace("\"spent_s\": 0.2", "\"spent_s\": 60.0");
    let root = tree(
        "record-good",
        &[
            ("benchmarks/1_a/case.toml", twice.as_str()),
            ("benchmarks/2_b/case.toml", retired.as_str()),
            ("records/1_a.json", record.as_str()),
        ],
    );
    let f = findings(&root);
    for absent in [
        "2_b",
        "no active case",
        "timed runs",
        "no rotated rerun",
        "stale record",
        "fixed rotation seed",
        "wrong schema",
        "exceeds",
    ] {
        assert!(!has(&f, absent), "{absent}: {f:?}");
    }
}

const NAMES: [&str; 6] = ["input", "b", "c", "d", "e", "f"];

fn block(t: &[f64]) -> String {
    block_of("orbitar", t, 0)
}

fn block_of(code: &str, t: &[f64], losses: usize) -> String {
    let n = t.len();
    format!(
        "{{\"code\": \"{code}\", \"input\": \"input\", \"threads\": 8, \"atoms\": 1, \"e_ref_ha\": -2.0, \
         \"e_ref_shift_ha\": 3.6e-8, \"delta_ha\": {}, \"warmup\": null, \"t_delta_s\": {t:?}, \"wall_s\": {:?}, \
         \"peak_rss_bytes\": {:?}, \"peak_pss_bytes\": {:?}, \"losses\": {:?}, \"median_t_delta_s\": 0.1, \
         \"range_t_delta_s\": [0.1, 0.2], \"cv_t_delta\": 0.0, \"noisy\": false, \
         \"median_peak_rss_bytes\": 1.0, \"median_peak_pss_bytes\": 1.0, \"rival\": null, \"step_cap\": 200, \
         \"steps\": {:?}, \"e_ref_steps\": [20, 24]}}",
        1e-4 / 27.211_386_245_988,
        vec![0.1; n],
        vec![1; n],
        vec![1; n],
        vec!["loss"; losses],
        vec![12; n],
    )
}

fn claim_record(blocks: &[Vec<f64>], edit: impl Fn(String) -> String) -> String {
    let runs: usize = blocks.iter().map(Vec::len).sum();
    let names = &NAMES[..blocks.len()];
    let mut speed: Vec<String> = (blocks.iter().zip(names))
        .map(|(t, name)| {
            block(t).replace("\"input\": \"input\"", &format!("\"input\": \"{name}\""))
        })
        .collect();
    speed[0] = edit(speed[0].clone());
    GOOD.replace("\"inputs\": [\"input\"]", &format!("\"inputs\": {names:?}"))
        .replace(
            "\"runs_s\": [0.1]",
            &format!("\"runs_s\": {:?}", vec![0.1; runs]),
        )
        .replace(
            "\"peak_rss_bytes\": [1]",
            &format!("\"peak_rss_bytes\": {:?}", vec![1; runs]),
        )
        .replace(
            "\"rotation_s\": [0.1]",
            &format!("\"rotation_s\": {:?}", vec![0.1; names.len()]),
        )
        .replace(
            "\"spent_s\": 0.2",
            &format!("\"spent_s\": 1.0, \"speed\": [{}]", speed.join(", ")),
        )
}

fn claim_findings(name: &str, record: &str) -> Vec<String> {
    claim_findings_of(name, "\"speed\", \"correctness\"", record)
}

fn claim_findings_of(name: &str, claims: &str, record: &str) -> Vec<String> {
    let claim = ACTIVE
        .replace("\"push\"", "\"claim\"")
        .replace("\"correctness\"", claims);
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", &claim),
            ("records/1_a.json", record),
        ],
    );
    verify(&root, Tier::Claim, &root.join("records"), UNIX_EPOCH)
}

const QUIET: [f64; 5] = [0.1; 5];

#[test]
fn a_claim_record_times_each_pair_five_times_or_ten_when_its_first_five_vary_past_five_percent() {
    let quiet = |n: usize| vec![0.1; n];
    let noisy = |n: usize| (0..n).map(|i| [0.1, 0.2][i % 2]).collect::<Vec<f64>>();
    let edge = vec![1.147, 1.252, 1.173, 1.214, 1.2991060817549602];
    let edge_ten: Vec<f64> = edge.iter().chain(&edge).copied().collect();
    for (blocks, finding) in [
        (vec![edge.clone(), quiet(5), quiet(5), quiet(5)], None),
        (
            vec![edge_ten, quiet(5), quiet(5), quiet(5)],
            Some("times 10 runs of 8; the CV of its first 5 asks for 5"),
        ),
        (
            vec![quiet(5), noisy(5), quiet(5), quiet(5)],
            Some("times 5 runs of 8; the CV of its first 5 asks for 10"),
        ),
        (
            vec![quiet(10), quiet(5), quiet(5), quiet(5)],
            Some("times 10 runs of 8; the CV of its first 5 asks for 5"),
        ),
    ] {
        let f = claim_findings("record-claim", &claim_record(&blocks, |b| b));
        match finding {
            Some(finding) => assert!(has(&f, finding), "{f:?}"),
            None => assert!(
                !has(&f, "times")
                    && !has(&f, "timed runs for")
                    && !has(&f, "no rotated rerun")
                    && !has(&f, "past the CV limit")
                    && !has(&f, "passes with δ")
                    && !has(&f, "is not below δ / 100")
                    && !has(&f, "RIVALS.toml"),
                "{f:?}"
            ),
        }
    }
    let three = claim_record(&[quiet(5), quiet(5), quiet(5)], |b| b).replace(
        "\"inputs\": [\"input\", \"b\", \"c\"]",
        "\"inputs\": [\"input\", \"b\", \"c\", \"d\"]",
    );
    let f = claim_findings("record-claim-three", &three);
    assert!(
        has(
            &f,
            "times 3 blocks, but its inputs, codes, and thread counts call for 4"
        ),
        "{f:?}"
    );
}

#[test]
fn a_claim_record_with_a_rival_counts_its_blocks_and_its_lost_runs_but_only_orbitar_runs_as_timed()
{
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let theirs = |losses: usize| block_of("qe", &[0.2; 3], losses);
    for (n, (losses, blocks, finding)) in [
        (2, 1, None),
        (
            1,
            1,
            Some("times 4 runs of 8; the CV of its first 5 asks for 5"),
        ),
        (
            2,
            2,
            Some("times 3 blocks, but its inputs, codes, and thread counts call for 2"),
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let speed: Vec<String> = std::iter::once(block(&QUIET))
            .chain(std::iter::repeat_n(theirs(losses), blocks))
            .collect();
        let f = rival_findings(&format!("record-rival-{n}"), &claim, &speed);
        match finding {
            Some(finding) => assert!(has(&f, finding), "{f:?}"),
            None => assert!(!has(&f, "times") && !has(&f, "timed runs for"), "{f:?}"),
        }
    }
}

#[test]
fn a_rival_s_lost_runs_are_its_losses_and_a_spread_is_kept_but_too_few_runs_refuse_a_wall_time_claim()
 {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let refused = "passes with T(δ) at 8 threads under 5 runs, judged or lost, under two judged times, or with no finite CV";
    let spread = vec![0.1, 0.2, 0.1, 0.2];
    for (n, (t, losses, want)) in [
        (vec![0.2; 5], 0, false),
        (vec![0.2; 4], 1, false),
        (vec![0.2], 4, false),
        (vec![], 5, false),
        (vec![0.2; 4], 0, true),
        (vec![0.2], 0, true),
        (spread, 1, false),
    ]
    .into_iter()
    .enumerate()
    {
        let speed = [block(&QUIET), block_of("qe", &t, losses)];
        let f = rival_findings(&format!("record-few-{n}"), &claim, &speed);
        assert_eq!(has(&f, refused), want, "{t:?} + {losses} lost: {f:?}");
    }
}

#[test]
fn a_rival_that_lost_every_run_and_its_reference_holds_only_losses_but_orbitar_must_hold_its_e_ref()
{
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"abinit\"]",
    );
    let lost =
        |code: &str| {
            block_of(code, &[], 5)
            .replace("\"e_ref_ha\": -2.0", "\"e_ref_ha\": null")
            .replace("\"e_ref_shift_ha\": 3.6e-8", "\"e_ref_shift_ha\": null")
            .replace(
                "\"median_t_delta_s\": 0.1, \"range_t_delta_s\": [0.1, 0.2], \"cv_t_delta\": 0.0, ",
                "",
            )
            .replace("\"median_peak_rss_bytes\": 1.0, \"median_peak_pss_bytes\": 1.0, ", "")
        };
    let rival = lost("abinit").replace("\"rival\": null", "\"rival\": {\"cutoff_ha\": 20.0}");
    let speed = vec![block(&QUIET), rival];
    let f = rival_findings("record-lost", &claim, &speed);
    assert!(
        !has(&f, "times") && !has(&f, "does not parse") && !has(&f, "E_ref"),
        "{f:?}"
    );
    let mut speed = speed;
    speed[0] = lost("orbitar").replace(
        "\"losses\": [\"loss\", \"loss\", \"loss\", \"loss\", \"loss\"]",
        "\"losses\": []",
    );
    let f = rival_findings("record-lost-ours", &claim, &speed);
    assert!(
        has(&f, "passes with an E_ref at 8 threads whose shift None"),
        "{f:?}"
    );
}

fn rival_findings(name: &str, claim: &str, speed: &[String]) -> Vec<String> {
    rival_findings_with(name, claim, speed, crate::harness::rival::RIVALS)
}

fn rival_findings_with(name: &str, claim: &str, speed: &[String], rivals: &str) -> Vec<String> {
    let runs = 5 * speed
        .iter()
        .filter(|b| b.contains("\"code\": \"orbitar\""))
        .count();
    let record = GOOD
        .replace(
            "\"runs_s\": [0.1]",
            &format!("\"runs_s\": {:?}", vec![0.1; runs]),
        )
        .replace(
            "\"peak_rss_bytes\": [1]",
            &format!("\"peak_rss_bytes\": {:?}", vec![1; runs]),
        )
        .replace(
            "\"spent_s\": 0.2",
            &format!("\"spent_s\": 1.0, \"speed\": [{}]", speed.join(", ")),
        );
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", claim),
            ("benchmarks/RIVALS.toml", rivals),
            ("records/1_a.json", &record),
        ],
    );
    verify(&root, Tier::Claim, &root.join("records"), UNIX_EPOCH)
}

#[test]
fn a_claim_record_times_rivals_only_at_the_thread_counts_that_rivals_toml_names() {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let at_8 = crate::harness::rival::RIVALS;
    let ours = [block(&QUIET)];
    let qe = |p: usize| {
        block_of("qe", &[0.2; 5], 0).replace("\"threads\": 8,", &format!("\"threads\": {p},"))
    };
    let times = |f: &[String]| has(f, "times") || has(f, "timed runs for") || has(f, "RIVALS.toml");
    let one: Vec<String> = ours.iter().cloned().chain([qe(8)]).collect();
    let f = rival_findings_with("record-threads", &claim, &one, at_8);
    assert!(!times(&f), "{f:?}");
    let four: Vec<String> = ours.iter().cloned().chain([qe(4)]).collect();
    let f = rival_findings_with("record-threads-4", &claim, &four, at_8);
    assert!(
        has(
            &f,
            "a qe block at 4 threads, which benchmarks/RIVALS.toml does not time"
        ),
        "{f:?}"
    );
    let all: Vec<String> = ours.iter().cloned().chain([1, 2, 4, 8].map(qe)).collect();
    let f = rival_findings_with("record-threads-all", &claim, &all, at_8);
    assert!(
        has(
            &f,
            "times 5 blocks, but its inputs, codes, and thread counts call for 2"
        ),
        "{f:?}"
    );
}

#[test]
fn a_claim_record_holds_no_block_of_a_rival_that_skips_the_input() {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let skips = crate::harness::rival::RIVALS.replace(
        "density_ratio = 8.0\n",
        "density_ratio = 8.0\nskip_cells = [{ cell = \"benchmarks/1_a/input.toml\", reason = \"no dataset\" }]\n",
    );
    let ours = vec![block(&QUIET)];
    let theirs = vec![block(&QUIET), block_of("qe", &[0.2; 5], 0)];
    let times = |f: &[String]| has(f, "times") || has(f, "timed runs for");
    let f = rival_findings_with("record-skip", &claim, &ours, &skips);
    assert!(!times(&f), "{f:?}");
    let f = rival_findings_with("record-skip-extra", &claim, &theirs, &skips);
    assert!(
        has(
            &f,
            "times 2 blocks, but its inputs, codes, and thread counts call for 1"
        ),
        "{f:?}"
    );
    let f = rival_findings("record-noskip", &claim, &ours);
    assert!(
        has(
            &f,
            "times 1 blocks, but its inputs, codes, and thread counts call for 2"
        ),
        "{f:?}"
    );
}

#[test]
fn a_passing_claim_record_holds_no_refused_block_by_its_flag_or_its_times_and_a_cv_past_five_percent_is_none()
 {
    let noisy_ten: Vec<f64> = (0..10).map(|i| [0.1, 0.2][i % 2]).collect();
    let quiet = QUIET.to_vec();
    let noisy = "passes with T(δ) at 8 threads under 5 runs, judged or lost, under two judged times, or with no finite CV";
    for (n, (first, edit, refused)) in [
        (noisy_ten.clone(), "", false),
        (vec![0.0; 5], "", true),
        (quiet.clone(), "\"noisy\": true", true),
        (quiet.clone(), "", false),
    ]
    .into_iter()
    .enumerate()
    {
        let blocks = [first, quiet.clone(), quiet.clone(), quiet.clone()];
        let record = claim_record(&blocks, |b| match edit {
            "" => b,
            flag => b.replace("\"noisy\": false", flag),
        });
        let f = claim_findings(&format!("record-noisy-{n}"), &record);
        assert_eq!(has(&f, noisy), refused, "{edit}: {f:?}");
    }
}

#[test]
fn a_claim_block_without_a_field_that_speed_rs_writes_does_not_parse() {
    let quiet = QUIET.to_vec();
    let blocks = [quiet.clone(), quiet.clone(), quiet.clone(), quiet];
    for field in ["\"atoms\": 1, ", "\"noisy\": false, "] {
        let record = claim_record(&blocks, |b| b.replace(field, ""));
        let f = claim_findings("record-field", &record);
        assert!(
            has(&f, "1_a record holds a speed block that does not parse"),
            "{field}: {f:?}"
        );
    }
}

#[test]
fn a_passing_claim_record_holds_delta_for_its_own_atom_count() {
    let quiet = QUIET.to_vec();
    let blocks = [quiet.clone(), quiet.clone(), quiet.clone(), quiet];
    for (atoms, refused) in [("1", false), ("2", true)] {
        let record = claim_record(&blocks, |b| {
            b.replace("\"atoms\": 1", &format!("\"atoms\": {atoms}"))
        });
        let f = claim_findings(&format!("record-delta-{atoms}"), &record);
        assert_eq!(has(&f, "passes with δ "), refused, "{atoms}: {f:?}");
    }
}

#[test]
fn a_noisy_block_refuses_a_wall_time_claim_but_not_a_memory_claim() {
    let noisy: Vec<f64> = (0..10).map(|i| [0.1, 0.2][i % 2]).collect();
    let quiet = QUIET.to_vec();
    let record = claim_record(&[noisy, quiet.clone(), quiet.clone(), quiet], |b| {
        b.replace("\"noisy\": false", "\"noisy\": true")
    });
    let past = "the judge refuses the block";
    assert!(has(&claim_findings("record-noisy-speed", &record), past));
    let optimization = claim_findings_of(
        "record-noisy-optimization",
        "\"optimization\", \"correctness\"",
        &record,
    );
    assert!(has(&optimization, past), "{optimization:?}");
    let memory = claim_findings_of(
        "record-noisy-memory",
        "\"memory\", \"correctness\"",
        &record,
    );
    assert!(
        !has(&memory, past) && !has(&memory, "needs a bound"),
        "{memory:?}"
    );
}

#[test]
fn a_passing_claim_record_keeps_its_e_ref_within_a_hundredth_of_delta_times_atoms() {
    let quiet = QUIET.to_vec();
    let breaks = "passes with an E_ref at 8 threads whose shift";
    let edge = format!("{}", 1e-4 / 27.211_386_245_988 * 1e-2);
    for (n, (from, to, refused)) in [
        ("", "", false),
        ("3.6e-8", "-3.6e-8", false),
        ("3.6e-8", "3.7e-8", true),
        ("3.6e-8", edge.as_str(), true),
        ("3.6e-8", "-3.7e-8", true),
    ]
    .into_iter()
    .enumerate()
    {
        let blocks = [quiet.clone(), quiet.clone(), quiet.clone(), quiet.clone()];
        let record = claim_record(&blocks, |b| b.replace(from, to));
        let f = claim_findings(&format!("record-shift-{n}"), &record);
        assert_eq!(has(&f, breaks), refused, "{to}: {f:?}");
    }
}

fn sharded(name: &str, case: &str, shards: &[&[&str]]) -> crate::Tree {
    sharded_by(name, case, shards, crate::harness::minus_one)
}

fn sharded_by(
    name: &str,
    case: &str,
    shards: &[&[&str]],
    measure: xtask::harness::Measure,
) -> crate::Tree {
    use crate::harness::si_tree;
    use xtask::harness::{Outcome, Session};
    let (root, bin) = si_tree(name, &case.replace("\"push\"", "\"node\""));
    write(
        &root,
        ".gitignore",
        "fake-engine\nrecords/\nshards/\ntarget/\n",
    );
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "e2e"], &root).is_empty());
    crate::git(&root, &["init", "-q"]);
    crate::git(&root, &["add", "-A"]);
    crate::git(&root, &["commit", "-qm", "case"]);
    for (k, selectors) in shards.iter().enumerate() {
        let records = root.join("records");
        let selectors = selectors.iter().map(|s| (*s).to_owned()).collect();
        let session = Session::new(&root, &bin, Tier::Node, &records).with_filter(selectors);
        assert_eq!(session.evaluate("1_a", measure), Outcome::Shard);
        let shard = std::fs::read_to_string(records.join("1_a.json")).unwrap();
        write(&root, &format!("shards/{k}.json"), &shard);
    }
    root
}

const FIRST: &[&str] = &[
    "b",
    "Si-X/Diamond@0.94",
    "Si-X/Diamond@0.96",
    "Si-X/Diamond@0.98",
];
const SECOND: &[&str] = &[
    "Si-X/Diamond@1.00",
    "Si-X/Diamond@1.02",
    "Si-X/Diamond@1.04",
    "Si-X/Diamond@1.06",
];

fn shards(root: &Path) -> Vec<String> {
    xtask::harness::union(root, "1_a", &root.join("shards"))
}

#[test]
fn shards_that_cover_every_input_once_prove_the_case() {
    let root = sharded("shards-cover", ACTIVE, &[FIRST, SECOND]);
    assert_eq!(shards(&root), Vec::<String>::new());
    assert_eq!(
        verify(&root, Tier::Node, &root.join("records"), UNIX_EPOCH),
        Vec::<String>::new()
    );
}

#[test]
fn shards_that_miss_or_repeat_an_input_fail_by_name() {
    let root = sharded("shards-missing", ACTIVE, &[FIRST]);
    assert!(has(
        &shards(&root),
        "no record runs 4 of its 8 inputs: Si-X/Diamond@1.00, Si-X/Diamond@1.02, Si-X/Diamond@1.04, Si-X/Diamond@1.06"
    ));
    let root = sharded(
        "shards-twice",
        ACTIVE,
        &[FIRST, SECOND, &["Si-X/Diamond@0.98"]],
    );
    assert!(has(&shards(&root), "Si-X/Diamond@0.98 runs in both"));
}

#[test]
fn a_shard_of_other_files_or_commits_fails_the_union_but_another_cpu_does_not() {
    let root = sharded("shards-mixed", ACTIVE, &[FIRST, SECOND]);
    let path = root.join("shards/1.json");
    let text = std::fs::read_to_string(&path).unwrap();
    let mut record: serde_json::Value = serde_json::from_str(&text).unwrap();
    record["lock"]["case"] = "0".repeat(64).into();
    record["commit"] = "1".repeat(40).into();
    record["node"]["host"]["cpu"] = "Other CPU".into();
    std::fs::write(&path, record.to_string()).unwrap();
    let found = shards(&root);
    for needle in [
        "ran other case files than this tree's reviewed LOCK hashes",
        "the records ran more than one commit",
    ] {
        assert!(has(&found, needle), "{needle}: {found:?}");
    }
    assert!(!has(&found, "CPU"), "{found:?}");
}

fn edit(root: &Path, k: usize, edit: impl Fn(&mut serde_json::Value)) {
    let path = root.join(format!("shards/{k}.json"));
    let mut record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    edit(&mut record);
    std::fs::write(&path, record.to_string()).unwrap();
}

#[test]
fn a_shard_of_an_unknown_commit_or_engine_another_engine_or_a_dirty_tree_fails_the_union() {
    let root = sharded("shards-provenance", ACTIVE, &[FIRST, SECOND]);
    edit(&root, 0, |r| {
        r["commit"] = "unknown".into();
        r["engine_sha256"] = "".into();
    });
    edit(&root, 1, |r| {
        r["engine_sha256"] = "2".repeat(64).into();
        r["dirty"] = true.into();
    });
    let found = shards(&root);
    for needle in [
        "0.json: no known commit (\"unknown\")",
        "0.json: no known engine_sha256 (\"\")",
        "the records ran more than one engine",
        "1.json ran a dirty tree",
    ] {
        assert!(has(&found, needle), "{needle}: {found:?}");
    }
}

fn union_after(name: &str, change: impl Fn(&mut serde_json::Value)) -> Vec<String> {
    let root = sharded(name, ACTIVE, &[FIRST, SECOND]);
    edit(&root, 0, change);
    shards(&root)
}

#[test]
fn a_shard_that_lists_a_failure_fails_the_union() {
    let found = union_after("shards-failures", |r| {
        r["failures"] = serde_json::json!(["a failure"])
    });
    assert!(
        has(&found, "0.json: it failed: [\"a failure\"]"),
        "{found:?}"
    );
}

#[test]
fn a_shard_whose_rotation_was_not_rigid_fails_the_union() {
    for (value, name) in [
        (serde_json::json!(1.01), "shards-rotation"),
        (serde_json::Value::Null, "shards-rotation-null"),
    ] {
        let found = union_after(name, |r| {
            r["rotation_used"] = value.clone();
            r["rotation_field"] = "free_energy_ha".into();
        });
        assert!(
            has(
                &found,
                &format!("0.json: rotation_used {value} of `\"free_energy_ha\"`'s bound is past 1")
            ),
            "{value}: {found:?}"
        );
    }
    let found = union_after("shards-rotation-small", |r| {
        r["rotation_max"] = 1e-9.into();
        r["rotation_used"] = 1.0.into();
    });
    assert!(!has(&found, "rotation_used"), "{found:?}");
}

#[test]
fn a_shard_of_another_status_or_tier_than_its_case_fails_the_union() {
    let found = union_after("shards-status", |r| {
        r["status"] = "retired".into();
        r["case_tier"] = "push".into();
    });
    assert!(
        has(&found, "0.json: status \"retired\", but the case is active"),
        "{found:?}"
    );
    assert!(
        has(&found, "0.json: case_tier \"push\", but the case is node"),
        "{found:?}"
    );
}

#[test]
fn a_shard_whose_seed_is_no_drawn_u64_fails_the_union() {
    let found = union_after("shards-seed", |r| r["seed"] = "x".into());
    assert!(
        has(&found, "0.json: seed \"x\" is no drawn u64"),
        "{found:?}"
    );
}

#[test]
fn a_shard_whose_times_or_rss_are_not_positive_and_finite_fails_the_union() {
    let found = union_after("shards-runs", |r| {
        r["runs_s"][0] = 0.0.into();
        r["rotation_s"][0] = serde_json::Value::Null;
        r["peak_rss_bytes"][0] = 0.into();
    });
    for key in ["runs_s", "rotation_s", "peak_rss_bytes"] {
        let needle = format!("0.json: {key} holds a value that is not positive and finite");
        assert!(has(&found, &needle), "{needle}: {found:?}");
    }
}

#[test]
fn a_shard_must_run_exactly_what_its_filter_picks_and_meet_the_tolerance() {
    let root = sharded("shards-edited", ACTIVE, &[FIRST, SECOND]);
    edit(&root, 0, |r| {
        r["inputs"].as_array_mut().unwrap().truncate(3)
    });
    edit(&root, 1, |r| {
        let first = r["inputs"][0].clone();
        r["inputs"].as_array_mut().unwrap().push(first);
        r["metrics"]["error"] = 1.0.into();
    });
    let found = shards(&root);
    for needle in [
        "0.json: its filter [\"b\",\"Si-X/Diamond@0.94\",\"Si-X/Diamond@0.96\",\"Si-X/Diamond@0.98\"] picks",
        "1.json: its filter",
        "Si-X/Diamond@1.00 runs in both",
        "1.json: by [tolerance]: error = 1e0 > 1e-12",
    ] {
        assert!(has(&found, needle), "{needle}: {found:?}");
    }
}

const PARTS: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nb_max = 1e-12\nsi_max = 1e-12\n";

fn parts(evidence: &xtask::harness::Evidence) -> Result<xtask::harness::Metrics, String> {
    let mut metrics = xtask::harness::Metrics::new();
    for name in evidence.names() {
        let error = (evidence.output(name)?.energy_ha("free_energy_ha")? + 1.0).abs();
        let metric = if name == "b" { "b" } else { "si" };
        let worst = metrics.entry(metric.to_owned()).or_insert(0.0);
        *worst = worst.max(error);
    }
    Ok(metrics)
}

fn no_b(evidence: &xtask::harness::Evidence) -> Result<xtask::harness::Metrics, String> {
    let mut metrics = parts(evidence)?;
    if let Some(b) = metrics.remove("b") {
        let si = metrics.entry("si".to_owned()).or_insert(0.0);
        *si = si.max(b);
    }
    Ok(metrics)
}

#[test]
fn a_shard_judges_the_bounds_it_measures_and_the_union_judges_every_bound() {
    use xtask::harness::{Outcome, Session};
    let root = sharded_by("shards-parts", PARTS, &[FIRST, SECOND], parts);
    assert_eq!(shards(&root), Vec::<String>::new());
    let root = sharded_by("shards-no-b", PARTS, &[FIRST, SECOND], no_b);
    assert_eq!(
        shards(&root),
        vec!["shards: 1_a: no record judges [tolerance].b_max".to_owned()]
    );
    let records = root.join("records");
    let session = Session::new(&root, &root.join("fake-engine"), Tier::Node, &records);
    assert!(matches!(
        session.evaluate("1_a", no_b),
        Outcome::Fail(why) if why.contains("metric b missing for [tolerance].b_max")
    ));
    let root = sharded_by("shards-parts-past", PARTS, &[FIRST, SECOND], parts);
    edit(&root, 1, |r| r["metrics"]["si"] = 1.0.into());
    assert!(has(
        &shards(&root),
        "1.json: by [tolerance]: si = 1e0 > 1e-12"
    ));
}

#[test]
fn a_failing_shard_of_a_planned_case_covers_nothing_and_never_promotes_it() {
    use xtask::harness::{Evidence, Metrics, Outcome, Session};
    fn wrong(_: &Evidence) -> Result<Metrics, String> {
        Err("no code yet".into())
    }
    let planned = ACTIVE.replace("active", "planned");
    let root = sharded("shards-failing", &planned, &[FIRST, SECOND]);
    let bin = root.join("fake-engine");
    let records = root.join("records");
    let selectors = SECOND.iter().map(|s| (*s).to_owned()).collect();
    let session = Session::new(&root, &bin, Tier::Node, &records).with_filter(selectors);
    assert!(matches!(
        session.evaluate("1_a", wrong),
        Outcome::Planned(_)
    ));
    write(
        &root,
        "shards/1.json",
        &std::fs::read_to_string(records.join("1_a.json")).unwrap(),
    );
    let found = shards(&root);
    assert!(
        has(
            &found,
            "1.json: outcome planned, not pass or shard; its inputs count for nothing"
        ),
        "{found:?}"
    );
    assert!(has(&found, "no record runs 4 of its 8 inputs"), "{found:?}");
    assert!(!has(&found, "planned but its shards pass"), "{found:?}");
}

#[test]
fn a_planned_case_whose_shards_cover_it_must_be_promoted() {
    let root = sharded(
        "shards-planned",
        &ACTIVE.replace("active", "planned"),
        &[FIRST, SECOND],
    );
    assert!(has(&shards(&root), "1_a is planned but its shards pass"));
}

#[test]
fn a_pass_record_with_a_filter_or_a_shard_without_one_fails_verify() {
    let root = tree(
        "record-shard-label",
        &[("benchmarks/1_a/case.toml", ACTIVE)],
    );
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"seed_fixed\"", "\"filter\": [\"input\"], \"seed_fixed\""),
    );
    assert!(has(
        &findings(&root),
        "only a filtered run or a candidate-table run is a shard"
    ));
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"pass\"", "\"shard\""),
    );
    assert!(has(
        &findings(&root),
        "only a filtered run or a candidate-table run is a shard"
    ));
}

#[test]
fn every_rival_the_case_declares_needs_its_r_even_without_a_timed_block() {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\", \"abinit\"]",
    );
    let mut speed = vec![block(&QUIET), block_of("qe", &[0.2; 5], 0)];
    let last = speed.pop().unwrap();
    speed.push(format!(
        "{last}], \"versus\": [{{\"code\": \"qe\", \"r\": 1.2}}], \"held_out\": [{{\"input\": \"held-out-x\", \"sha256\": \"0\"}}], \"x\": [0"
    ));
    let f = rival_findings("record-declared-rival", &claim, &speed);
    assert!(has(&f, "its R against abinit is None"), "{f:?}");
    assert!(!has(&f, "its R against qe"), "{f:?}");
}

#[test]
fn a_passing_claim_beside_a_rival_needs_g_s_ci_lower_bound_of_at_least_one() {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let one = "[{\"input\": \"held-out-x\", \"sha256\": \"0\"}]";
    let refused = "the 95% CI lower bound of its G against qe is";
    for (n, (ci, want)) in [
        ("[1.0000001, 1.3]", None),
        ("[1.0, 1.3]", None),
        ("[\"inf\", \"inf\"]", None),
        ("[0.9999999, 1.3]", Some("Some(0.9999999), not at least 1")),
        ("[\"NaN\", 1.3]", Some("Some(NaN)")),
        ("null", Some("None")),
    ]
    .into_iter()
    .enumerate()
    {
        let mut speed = vec![block(&QUIET), block_of("qe", &[0.2; 5], 0)];
        let last = speed.pop().unwrap();
        speed.push(format!(
            "{last}], \"versus\": [{{\"code\": \"qe\", \"r\": 1.2, \"g_ci\": {ci}}}], \"held_out\": {one}, \"x\": [0"
        ));
        let f = rival_findings(&format!("record-g-{n}"), &claim, &speed);
        assert_eq!(has(&f, refused), want.is_some(), "{ci}: {f:?}");
        if let Some(want) = want {
            assert!(has(&f, &format!("{refused} {want}")), "{ci}: {f:?}");
        }
        assert!(!has(&f, "its R against"), "{ci}: {f:?}");
    }
}

#[test]
fn an_m4_claim_needs_g_of_at_least_one_and_a_quarter_and_a_ci_lower_bound_above_one_against_each_rival()
 {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]\nmilestone = \"m4\"",
    );
    let one = "[{\"input\": \"held-out-x\", \"sha256\": \"0\"}]";
    let refused = "its G against qe is";
    for (n, (g, ci, want)) in [
        ("1.25", "[1.0000001, 1.5]", None),
        ("\"inf\"", "[\"inf\", \"inf\"]", None),
        (
            "1.2499999",
            "[1.1, 1.5]",
            Some("Some(1.2499999) with a 95% CI lower bound Some(1.1)"),
        ),
        (
            "1.5",
            "[1.0, 2.0]",
            Some("Some(1.5) with a 95% CI lower bound Some(1.0)"),
        ),
        ("\"NaN\"", "[1.1, 1.5]", Some("Some(NaN)")),
        ("null", "[1.1, 1.5]", Some("None with")),
        ("1.5", "null", Some("lower bound None")),
    ]
    .into_iter()
    .enumerate()
    {
        let mut speed = vec![block(&QUIET), block_of("qe", &[0.2; 5], 0)];
        let last = speed.pop().unwrap();
        speed.push(format!(
            "{last}], \"versus\": [{{\"code\": \"qe\", \"r\": 1.2, \"g\": {g}, \"g_ci\": {ci}}}], \"held_out\": {one}, \"x\": [0"
        ));
        let f = rival_findings(&format!("record-m4-{n}"), &claim, &speed);
        assert_eq!(has(&f, refused), want.is_some(), "{g} {ci}: {f:?}");
        if let Some(want) = want {
            assert!(has(&f, want), "{g} {ci}: {f:?}");
            assert!(has(&f, "M4 asks G ≥ 1.25 and a bound > 1"), "{f:?}");
        }
        let m3 = rival_findings(
            &format!("record-m3-{n}"),
            &claim.replace("milestone = \"m4\"\n", ""),
            &speed,
        );
        let low_ok = ci.starts_with("[1.") || ci.starts_with("[\"inf");
        assert_eq!(
            has(&m3, "the 95% CI lower bound of its G"),
            !low_ok,
            "{g} {ci}: {m3:?}"
        );
    }
}

#[test]
fn a_claim_record_holds_one_scf_step_count_per_timed_run() {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let one = "[{\"input\": \"held-out-x\", \"sha256\": \"0\"}]";
    let rival = |steps: &str| {
        block_of("qe", &[0.2; 5], 0).replace("\"steps\": [12, 12, 12, 12, 12]", steps)
            + &format!(
                "], \"versus\": [{{\"code\": \"qe\", \"r\": 1.2, \"g_ci\": [1.1, 1.2]}}], \"held_out\": {one}, \"x\": [0"
            )
    };
    let missing = "record holds 4 step counts for the 5 timed runs of qe at 8 threads";
    for (n, steps, want) in [
        (0, "\"steps\": [12, 12, 12, 12, 12]", false),
        (1, "\"steps\": [12, 12, 12, 12]", true),
    ] {
        let f = rival_findings(
            &format!("record-steps-{n}"),
            &claim,
            &[block(&QUIET), rival(steps)],
        );
        assert_eq!(has(&f, missing), want, "{f:?}");
    }
    let f = rival_findings(
        "record-steps-none",
        &claim,
        &[block(&QUIET), rival("\"x_steps\": []")],
    );
    assert!(
        has(&f, "record holds 0 step counts for the 5 timed runs of qe"),
        "{f:?}"
    );
}

#[test]
fn a_passing_claim_beside_a_rival_needs_r_of_at_least_one_and_a_held_out_variant() {
    let claim = ACTIVE.replace("\"push\"", "\"claim\"").replace(
        "[\"correctness\"]",
        "[\"speed\", \"correctness\"]\nrivals = [\"qe\"]",
    );
    let speed = [block(&QUIET), block_of("qe", &[0.2; 5], 0)];
    let with = |versus: &str, held: &str| {
        let mut speed = speed.to_vec();
        let last = speed.pop().unwrap();
        speed.push(format!(
            "{last}], \"versus\": {versus}, \"held_out\": {held}, \"x\": [0"
        ));
        speed
    };
    let one = "[{\"input\": \"held-out-x\", \"sha256\": \"0\"}]";
    for (versus, held, findings) in [
        ("[{\"code\": \"qe\", \"r\": 1.2}]", one, vec![]),
        ("[{\"code\": \"qe\", \"r\": \"inf\"}]", one, vec![]),
        ("[{\"code\": \"qe\", \"r\": 1.0}]", one, vec![]),
        (
            "[{\"code\": \"qe\", \"r\": 0.9}]",
            one,
            vec!["its R against qe is Some(0.9), not at least 1"],
        ),
        (
            "[{\"code\": \"qe\", \"r\": \"NaN\"}]",
            one,
            vec!["its R against qe is Some(NaN)"],
        ),
        ("[]", one, vec!["its R against qe is None"]),
        (
            "[{\"code\": \"qe\", \"r\": 1.2}]",
            "[]",
            vec!["passes beside rivals with no held-out variant"],
        ),
    ] {
        let f = rival_findings("record-versus", &claim, &with(versus, held));
        for needle in ["its R against", "no held-out variant"] {
            let want = findings.iter().any(|w| w.contains(needle));
            assert_eq!(has(&f, needle), want, "{versus} {held}: {f:?}");
        }
        for want in &findings {
            assert!(has(&f, want), "{want}: {f:?}");
        }
    }
}

#[test]
fn a_planned_case_proves_the_tier_run_by_a_passing_shard_alone() {
    let planned = ACTIVE.replace("active", "planned");
    let root = tree("record-proved", &[("benchmarks/1_a/case.toml", &planned)]);
    write(
        &root,
        "records/1_a.json",
        &GOOD.replace("\"pass\"", "\"planned\""),
    );
    assert!(has(&findings(&root), "no active case"));
    let shard = GOOD
        .replace("\"pass\"", "\"shard\"")
        .replace("\"seed_fixed\"", "\"filter\": [\"input\"], \"seed_fixed\"");
    write(&root, "records/1_a.json", &shard);
    let f = findings(&root);
    assert!(!has(&f, "no active case") && !has(&f, "1_a shard"), "{f:?}");
}

const CLAIM: &str = r#"{
  "schema": 4, "case": "11_x", "tier": "claim", "outcome": "fail", "commit": "abc", "dirty": false,
  "engine_sha256": "e5", "seed": "7", "seed_fixed": false,
  "held_out": [], "tuned": "t1", "skipped": [{"code": "qe", "input": "nio"}], "spent_s": 12.5,
  "failures": ["R against abinit is unmeasured", "b: at ABINIT's frozen cutoff, the accuracy is unmeasured"],
  "versus": [
    {"code": "abinit", "g": 1.25, "r": "NaN", "r_min": 1.0, "g_judged": false, "pairs": [
      {"input": "input", "threads": 8, "t_ratio": "inf", "pss_ratio": 0.75, "in_r": false,
       "medians_t_delta_s": ["inf", 2.5], "medians_peak_pss_bytes": [250.0, 300.0], "losses": 5},
      {"input": "b", "threads": 8, "t_ratio": 1.5, "pss_ratio": 1.75, "in_r": true,
       "medians_t_delta_s": [6.0, 4.0], "medians_peak_pss_bytes": [700.0, 400.0], "losses": 0}]},
    {"code": "qe", "g": 0.875, "r": 1.125, "r_min": 1.0, "g_judged": true, "g_ci": [0.75, 1.0625], "g_min": 1.0,
     "milestone": "m3", "g_point_min": null, "g_resamples": 10000, "g_seed": 2316, "pairs": [
      {"input": "input", "threads": 8, "t_ratio": 0.5, "pss_ratio": 2.25, "in_r": true,
       "medians_t_delta_s": [1.25, 2.5], "medians_peak_pss_bytes": [675.0, 300.0], "losses": 1},
      {"input": "b", "threads": 8, "t_ratio": 1.0625, "pss_ratio": 0.625, "in_r": true,
       "medians_t_delta_s": [4.25, 4.0], "medians_peak_pss_bytes": [250.0, 400.0], "losses": 2}]}],
  "speed": [
    {"code": "orbitar", "input": "input", "threads": 8, "atoms": 2, "e_ref_ha": -7.5, "e_ref_shift_ha": 1e-12,
     "delta_ha": 7.3e-6, "warmup": null, "t_delta_s": [3.0, 1.0, 2.0, 2.75], "wall_s": [9.0, 2.0, 4.0, 5.0],
     "peak_rss_bytes": [1, 2, 3, 4], "peak_pss_bytes": [100, 300, 500, 350], "losses": [], "noisy": false,
     "median_t_delta_s": 2.375, "range_t_delta_s": [1.0, 3.0], "cv_t_delta": 0.5, "median_peak_rss_bytes": 2.5,
     "median_peak_pss_bytes": 325.0, "rival": null, "accuracy": {"d_ha_per_atom": 0.5},
     "step_cap": 200, "steps": [12, 11, 12, 13], "e_ref_steps": [20, 24]},
    {"code": "abinit", "input": "input", "threads": 8, "atoms": 2, "e_ref_ha": null, "e_ref_shift_ha": null,
     "delta_ha": 7.3e-6, "warmup": {"loss": "crash"}, "t_delta_s": [], "wall_s": [], "peak_rss_bytes": [],
     "peak_pss_bytes": [], "losses": ["crash", "crash", "crash", "crash", "crash"], "noisy": true,
     "rival": null, "accuracy": null}],
  "metrics": {"t_input": 2.0}
}"#;

#[test]
fn readout_prints_r_g_each_pair_and_each_blocks_medians_from_a_record() {
    let root = tree("record-readout", &[("records/11_x.json", CLAIM)]);
    let dir = root.join("records");
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["readout", "11_x", "--dir", &dir.to_string_lossy()])
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{text}");
    let pair = |code_input: &str, ratios: &str, medians: &str, losses: u32| {
        format!(
            "  pair {code_input} p=8: {ratios} medians with losses as +inf (rival, orbitar): {medians}; rival losses \
             {losses}\n"
        )
    };
    let want = [
        format!("readout: {}/11_x.json\n", dir.display()),
        "tier claim schema 4 outcome fail commit abc dirty false engine e5 seed 7 seed_fixed false\n".into(),
        "held_out [] tuned t1 pilot null\n".into(),
        "skipped [{\"code\":\"qe\",\"input\":\"nio\"}] spent_s 12.5\n".into(),
        "failure: R against abinit is unmeasured\n".into(),
        "failure: b: at ABINIT's frozen cutoff, the accuracy is unmeasured\n".into(),
        "versus abinit: R NaN (judged, R_min 1.0) G 1.25 95% CI null by null resamples, seed null (g_judged false, G_min \
         null, milestone null, G point min null)\n"
            .into(),
        pair("input", "t_ratio inf pss_ratio 0.75 in_r false", "T(δ) [\"inf\",2.5] s, Pss [250.0,300.0] bytes", 5),
        pair("b", "t_ratio 1.5 pss_ratio 1.75 in_r true", "T(δ) [6.0,4.0] s, Pss [700.0,400.0] bytes", 0),
        "versus qe: R 1.125 (judged, R_min 1.0) G 0.875 95% CI [0.75,1.0625] by 10000 resamples, seed 2316 (g_judged \
         true, G_min 1.0, milestone m3, G point min null)\n"
            .into(),
        pair("input", "t_ratio 0.5 pss_ratio 2.25 in_r true", "T(δ) [1.25,2.5] s, Pss [675.0,300.0] bytes", 1),
        pair("b", "t_ratio 1.0625 pss_ratio 0.625 in_r true", "T(δ) [4.25,4.0] s, Pss [250.0,400.0] bytes", 2),
        "block orbitar input p=8: timed runs 4 losses 0; medians of the runs alone: T(δ) 2.375 s, wall 4.5 s, peak Pss \
         325 bytes; noisy false E_ref -7.5 accuracy {\"d_ha_per_atom\":0.5}; SCF steps [12, 11, 12, 13], E_ref's \
         Some([20, 24]), cap Some(200)\n"
            .into(),
        "block abinit input p=8: timed runs 0 losses 5; medians of the runs alone: T(δ) none s, wall none s, peak Pss \
         none bytes; noisy true E_ref none accuracy none; SCF steps [], E_ref's None, cap None\n"
            .into(),
        "rival losses, each counted as +inf in R and G: crash\n".into(),
        "read each rival block's E_ref (none: its reference run was lost, \"E_ref lost\" in the tier log) before its \
         losses count as a win for orbitaR\n"
            .into(),
        "metrics {\"t_input\":2.0}\n".into(),
        "record outcome fail; the PASS below says only that the record is readable\n".into(),
        "xtask readout: PASS\n".into(),
    ]
    .concat();
    assert_eq!(text, want);
}

#[test]
fn readout_of_a_record_it_cannot_read_as_this_harness_writes_it_is_a_finding() {
    let root = tree("record-readout-bad", &[("records/12_y.json", CLAIM)]);
    let at = |id: &str| format!("readout: {}/{id}.json: ", root.join("records").display());
    let readout = |id: &str| {
        crate::lever(
            &[
                "readout",
                id,
                "--dir",
                &root.join("records").to_string_lossy(),
            ],
            &root,
        )
    };
    let f = readout("11_x");
    assert!(f.len() == 1 && f[0].starts_with(&at("11_x")), "{f:?}");
    assert_eq!(
        readout("12_y"),
        [format!("{}case \"11_x\", not 12_y", at("12_y"))]
    );
    for (id, record, why) in [
        (
            "13_s",
            CLAIM
                .replace("\"schema\": 4", "\"schema\": 2")
                .replace("11_x", "13_s"),
            "schema 2, not 4: another harness wrote it",
        ),
        (
            "14_p",
            CLAIM
                .replace("\"code\": \"orbitar\"", "\"code\": 7")
                .replace("11_x", "14_p"),
            "speed: ",
        ),
        (
            "15_t",
            CLAIM
                .replace("\"median_t_delta_s\": 2.375,", "")
                .replace("11_x", "15_t"),
            "block orbitar input p=8 holds 4 runs but no timing",
        ),
    ] {
        crate::write(&root, &format!("records/{id}.json"), &record);
        let f = readout(id);
        assert!(
            f.len() == 1 && f[0].starts_with(&format!("{}{why}", at(id))),
            "{f:?}"
        );
    }
}

#[test]
fn a_pilot_record_proves_nothing() {
    let root = tree("record-pilot", &[("benchmarks/1_a/case.toml", ACTIVE)]);
    write(&root, "records/1_a.json", GOOD);
    assert!(!has(&findings(&root), "pilot"));
    let pilot = GOOD.replace("\"seed_fixed\"", "\"pilot\": true, \"seed_fixed\"");
    write(&root, "records/1_a.json", &pilot);
    assert!(has(
        &findings(&root),
        "1_a record is a pilot (ORBITAR_PILOT): it prices a claim run and proves nothing"
    ));
}

#[test]
fn a_shard_of_a_candidate_dataset_table_never_proves_the_case() {
    use xtask::harness::{Outcome, Session};
    let (root, bin) = crate::harness::acwf::tuning("shards-candidate", &["Si-X/FCC"]);
    write(
        &root,
        ".gitignore",
        "fake-engine\nrecords/\nshards/\ntarget/\ncandidate/\n",
    );
    let candidate = root.join("candidate/table.toml");
    write(
        &root,
        "candidate/table.toml",
        "[element]\nSi = \"jth-2.0\"\nAl = \"jth-2.0\"\n# ours\n",
    );
    assert!(lever(&["lock", "--reviewer", "owner", "--reason", "e2e"], &root).is_empty());
    crate::git(&root, &["init", "-q"]);
    crate::git(&root, &["add", "-A"]);
    crate::git(&root, &["commit", "-qm", "case"]);
    let records = root.join("records");
    let union = |session: Session| {
        assert_eq!(
            session.evaluate("1_a", crate::harness::minus_one),
            Outcome::Shard
        );
        let shard = std::fs::read_to_string(records.join("1_a.json")).unwrap();
        write(&root, "shards/0.json", &shard);
        shards(&root)
    };
    let tracked = Session::new(&root, &bin, Tier::Node, &records).with_filter(vec!["Si".into()]);
    assert!(!has(&union(tracked.clone()), "dataset table"));
    let found = union(tracked.with_table(&candidate));
    assert!(has(&found, "candidate/table.toml"), "{found:?}");
    assert!(
        has(&found, "not the case's own; it proves nothing"),
        "{found:?}"
    );
    let whole = Session::new(&root, &bin, Tier::Node, &records).with_table(&candidate);
    assert_eq!(
        whole.evaluate("1_a", crate::harness::minus_one),
        Outcome::Shard
    );
    let found = verify(&root, Tier::Node, &records, UNIX_EPOCH);
    assert!(
        has(&found, "not the case's own; it proves nothing"),
        "{found:?}"
    );
    assert!(
        !has(
            &found,
            "only a filtered run or a candidate-table run is a shard"
        ),
        "{found:?}"
    );
}
