mod card;
mod claim;

use crate::{Tree, git, has, lever, tree, write};
use std::{fs, path::Path, process::Command};

const PUSH: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nx_error_max = 1e-12\nshare_min = 0.1\n";
const NODE: &str = "[case]\ntier = \"node\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\ne_error_max = 1.0\n";
const EXACT: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nd_difference_max = 0.0\n";
const PLANNED: &str = "[case]\ntier = \"push\"\nstatus = \"planned\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\n";
const WHAT: &str = "[1]\nwhat = \"x vs analytic\"\nmetric = [\"x_error\", \"share\"]\nunit = \"Ha\"\n\n[2]\nwhat = \"e vs reference\"\nmetric = [\"e_error\"]\nunit = \"eV\"\n\n[4]\nwhat = \"1 vs 4 threads\"\nmetric = [\"d_difference\"]\nunit = \"\"\nexact = \"bit-identical at 1 and 4 threads\"\n";
const PAGE: &str = "benchmarks/README.md";
const LIGHT: &str = "assets/benchmarks/results-light.svg";
const DARK: &str = "assets/benchmarks/results-dark.svg";

fn cases(name: &str) -> Tree {
    let root = tree(
        &format!("results-{name}"),
        &[
            ("benchmarks/1_a/case.toml", PUSH),
            ("benchmarks/2_b/case.toml", NODE),
            ("benchmarks/3_c/case.toml", PLANNED),
            ("benchmarks/4_d/case.toml", EXACT),
            ("benchmarks/results-what.toml", WHAT),
        ],
    );
    git(&root, &["init", "-q"]);
    commit(&root, "cases");
    root
}

fn commit(root: &Path, message: &str) -> String {
    git(root, &["add", "-A"]);
    git(root, &["commit", "-q", "--allow-empty", "-m", message]);
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    String::from_utf8(out.stdout).unwrap().trim().to_owned()
}

fn record(
    case: &str,
    tier: &str,
    commit: &str,
    dirty: bool,
    outcome: &str,
    metrics: &str,
) -> String {
    let tolerance = match case {
        "1_a" => r#"{"share_min":0.1,"x_error_max":1e-12}"#,
        "2_b" => r#"{"e_error_max":1.0}"#,
        _ => r#"{"d_difference_max":0.0}"#,
    };
    format!(
        r#"{{"schema":4,"failures":[],"filter":null,"case":"{case}","tier":"{tier}","commit":"{commit}","dirty":{dirty},"pilot":false,"engine_sha256":"{}","outcome":"{outcome}","metrics":{metrics},"tolerance":{tolerance}}}"#,
        "ab".repeat(32)
    )
}

fn push_records(root: &Path, commit: &str) {
    let one = record(
        "1_a",
        "push",
        commit,
        false,
        "pass",
        r#"{"x_error":5e-13,"share":0.4}"#,
    );
    write(root, "target/benchmarks/push/1_a.json", &one);
    let four = record(
        "4_d",
        "push",
        commit,
        false,
        "pass",
        r#"{"d_difference":0.0}"#,
    );
    write(root, "target/benchmarks/push/4_d.json", &four);
}

fn read(root: &Path, path: &str) -> String {
    fs::read_to_string(root.join(path)).unwrap_or_default()
}

fn s11(root: &Path) -> Vec<String> {
    let found = lever(&["science"], root);
    found.into_iter().filter(|f| f.contains("S11")).collect()
}

#[test]
fn collect_writes_a_row_per_active_case_and_a_node_case_without_a_record_says_so() {
    let root = cases("collect");
    let head = commit(&root, "head");
    push_records(&root, &head);
    assert_eq!(lever(&["results", "collect"], &root), Vec::<String>::new());
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert_eq!(results.matches("[[case]]").count(), 3, "{results}");
    assert!(
        !results.contains("3_c"),
        "a planned case has no row: {results}"
    );
    assert!(
        results.contains(&format!("commit = \"{head}\"")),
        "{results}"
    );
    assert!(results.contains("name = \"x_error\"\nbound = \"max\"\nlimit = 1e-12\nmeasured = 5e-13\nratio = 5e-1\nheadline = true\n"), "{results}");
    assert!(
        results.contains(
            "name = \"share\"\nbound = \"min\"\nlimit = 1e-1\nmeasured = 4e-1\nratio = 2.5e-1\n\n"
        ),
        "{results}"
    );
    assert!(results.contains("id = \"2_b\"\nwhat = \"e vs reference\"\nunit = \"eV\"\ntier = \"node\"\noutcome = \"no record\"\n"), "{results}");
    assert!(
        !results.contains("unix") && !results.contains("started"),
        "{results}"
    );
}

#[test]
fn a_dirty_record_or_one_of_no_ancestor_commit_is_refused_by_name_and_nothing_is_written() {
    let root = cases("refused");
    let side = commit(&root, "side");
    git(&root, &["checkout", "-q", "--orphan", "other"]);
    let head = commit(&root, "other");
    let dirty = record(
        "1_a",
        "push",
        &head,
        true,
        "pass",
        r#"{"x_error":0.0,"share":1.0}"#,
    );
    write(&root, "target/benchmarks/push/1_a.json", &dirty);
    let foreign = record(
        "4_d",
        "push",
        &side,
        false,
        "pass",
        r#"{"d_difference":0.0}"#,
    );
    write(&root, "target/benchmarks/push/4_d.json", &foreign);
    let found = lever(&["results", "collect"], &root);
    assert!(
        has(&found, "1_a.json: 1_a ran on a dirty tree"),
        "{found:?}"
    );
    assert!(
        has(
            &found,
            &format!("4_d.json: 4_d ran at {side}, which is no ancestor of HEAD")
        ),
        "{found:?}"
    );
    assert!(!root.join("benchmarks/RESULTS.toml").exists());
    let shard = record(
        "4_d",
        "push",
        &head,
        false,
        "shard",
        r#"{"d_difference":0.0}"#,
    );
    write(&root, "target/benchmarks/push/4_d.json", &shard);
    assert!(has(
        &lever(&["results", "collect"], &root),
        "4_d has outcome Some(\"shard\")"
    ));
    assert!(has(
        &lever(&["results", "collect", "--tier", "fast"], &root),
        "results: unknown tier fast"
    ));
}

#[test]
fn a_missing_phrase_or_a_metric_that_the_case_does_not_bound_is_refused_by_name() {
    let root = cases("what");
    let head = commit(&root, "head");
    push_records(&root, &head);
    write(
        &root,
        "benchmarks/results-what.toml",
        &WHAT
            .replace("[2]", "[5]")
            .replace("\"d_difference\"", "\"d_error\""),
    );
    let found = lever(&["results", "collect"], &root);
    assert!(has(&found, "has no [2] for 2_b"), "{found:?}");
    assert!(
        has(
            &found,
            "[4] metric d_error is no bound of 4_d's [tolerance]"
        ),
        "{found:?}"
    );
    write(
        &root,
        "benchmarks/results-what.toml",
        &WHAT.replace("x vs analytic", "x <b>"),
    );
    assert!(has(
        &lever(&["results", "collect"], &root),
        "[1] what, unit and exact are plain words"
    ));
    assert!(!root.join("benchmarks/RESULTS.toml").exists());
}

#[test]
fn a_later_tier_keeps_each_row_with_its_own_record_commit() {
    let root = cases("keep");
    let old = commit(&root, "old");
    let node = record("2_b", "node", &old, false, "fail", r#"{"e_error":3.0}"#);
    write(&root, "elsewhere/node/2_b.json", &node);
    let head = commit(&root, "head");
    push_records(&root, &head);
    assert!(lever(&["results", "collect", "--tier", "push"], &root).is_empty());
    let records = root.join("elsewhere");
    let found = lever(
        &[
            "results",
            "collect",
            "--tier",
            "node",
            "--records",
            records.to_str().unwrap(),
        ],
        &root,
    );
    assert!(found.is_empty(), "{found:?}");
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert!(
        results.contains(&format!("commit = \"{head}\"")),
        "{results}"
    );
    assert!(
        results.contains(&format!(
            "run = \"node\"\noutcome = \"fail\"\ncommit = \"{old}\""
        )),
        "{results}"
    );
    assert!(lever(&["results", "render"], &root).is_empty());
    let chart = read(&root, LIGHT);
    assert!(
        chart.contains(">✕ fail</text>") && chart.contains(">3.0 ≤ 1 eV</text>"),
        "{chart}"
    );
}

#[test]
fn render_writes_the_same_bytes_every_time_and_science_s11_holds_the_committed_files_to_them() {
    let root = cases("render");
    assert!(has(
        &lever(&["results", "render"], &root),
        "RESULTS.toml is missing"
    ));
    let head = commit(&root, "head");
    push_records(&root, &head);
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert!(lever(&["results", "render"], &root).is_empty());
    let first: Vec<String> = [PAGE, LIGHT, DARK].iter().map(|p| read(&root, p)).collect();
    assert!(lever(&["results", "render"], &root).is_empty());
    let second: Vec<String> = [PAGE, LIGHT, DARK].iter().map(|p| read(&root, p)).collect();
    assert_eq!(first, second);
    assert_eq!(s11(&root), Vec::<String>::new());
    let (light, dark) = (&first[1], &first[2]);
    for chart in [light, dark] {
        assert!(
            chart.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960.0\""),
            "{chart}"
        );
        assert!(
            chart.contains("font-family=\"system-ui, sans-serif\""),
            "{chart}"
        );
        assert!(
            chart.contains(">✓ pass</text>") && chart.contains(">no record</text>"),
            "{chart}"
        );
        assert!(chart.contains(">5.0e-13 ≤ 1e-12 Ha</text>"), "{chart}");
        assert!(
            chart.contains(">bit-identical at 1 and 4 threads</text>") && !chart.contains(">0 ≤ 0"),
            "{chart}"
        );
        assert!(chart.contains("no speed or memory claim yet"), "{chart}");
    }
    assert!(light.contains("fill=\"#ffffff\"") && dark.contains("fill=\"#191919\""));
    let page = &first[0];
    assert!(
        page.contains("| 1 `a` | x vs analytic | `x_error` | 5.0e-13 | ≤ 1e-12 | Ha | pass |"),
        "{page}"
    );
    assert!(page.contains("## node tier"), "{page}");
    write(&root, LIGHT, &light.replace("5.0e-13", "5.0e-14"));
    assert_eq!(
        s11(&root),
        [format!(
            "science: S11 {LIGHT} differs from what `cargo xtask results render` makes of benchmarks/RESULTS.toml"
        )]
    );
    let results = read(&root, "benchmarks/RESULTS.toml");
    write(
        &root,
        "benchmarks/RESULTS.toml",
        &results.replace("what = \"x vs analytic\"", "what = \"x vs y\""),
    );
    let stale = s11(&root);
    assert!(
        [PAGE, DARK]
            .iter()
            .all(|p| has(&stale, &format!("S11 {p} differs"))),
        "{stale:?}"
    );
    fs::remove_file(root.join("benchmarks/RESULTS.toml")).unwrap();
    assert!(has(
        &s11(&root),
        &format!("S11 {PAGE} exists without benchmarks/RESULTS.toml")
    ));
}

#[test]
fn a_results_file_that_its_own_numbers_contradict_is_refused() {
    let root = cases("contradict");
    let head = commit(&root, "head");
    push_records(&root, &head);
    assert!(lever(&["results", "collect"], &root).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    for (from, to, why) in [
        (
            "ratio = 5e-1",
            "ratio = 1e-1",
            "x_error has ratio 1e-1, but its numbers give 5e-1",
        ),
        (
            "measured = 5e-13\nratio = 5e-1",
            "measured = 2e-12\nratio = 2e0",
            "a pass with a metric past its bound",
        ),
        (
            "headline = true\n",
            "",
            "a recorded row has one headline metric",
        ),
        ("number = 4\n", "number = 3\n", "number 3 is not the case's"),
    ] {
        write(
            &root,
            "benchmarks/RESULTS.toml",
            &results.replacen(from, to, 1),
        );
        let found = lever(&["results", "render"], &root);
        assert!(has(&found, why), "{why}: {found:?}");
        assert!(has(&s11(&root), why), "{why}");
    }
}

#[test]
fn science_s11_derives_each_row_again_from_its_pinned_record_and_names_any_other_row() {
    let root = cases("pinned");
    let head = commit(&root, "head");
    push_records(&root, &head);
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert!(lever(&["results", "render"], &root).is_empty());
    assert_eq!(s11(&root), Vec::<String>::new());
    let (results, pinned) = (
        read(&root, "benchmarks/RESULTS.toml"),
        read(&root, "benchmarks/RECORDS.jsonl"),
    );
    assert_eq!(pinned.lines().count(), 2, "{pinned}");
    let row = "1_a: its row in benchmarks/RESULTS.toml differs from what its record in benchmarks/RECORDS.jsonl gives";
    write(
        &root,
        "benchmarks/RESULTS.toml",
        &results.replace(
            "measured = 5e-13\nratio = 5e-1",
            "measured = 2.5e-13\nratio = 2.5e-1",
        ),
    );
    assert!(has(&s11(&root), row), "{:?}", s11(&root));
    write(&root, "benchmarks/RESULTS.toml", &results);
    write(
        &root,
        "benchmarks/RECORDS.jsonl",
        &pinned.replace("5e-13", "4e-13"),
    );
    assert!(has(
        &s11(&root),
        "1_a: record_sha256 is not the sha256 of its record in benchmarks/RECORDS.jsonl"
    ));
    let extra = record("2_b", "node", &head, false, "pass", r#"{"e_error":0.5}"#);
    write(
        &root,
        "benchmarks/RECORDS.jsonl",
        &format!("{pinned}{extra}\n"),
    );
    assert!(has(
        &s11(&root),
        "benchmarks/RECORDS.jsonl holds a record of 2_b, which no row uses"
    ));
    write(&root, "benchmarks/RECORDS.jsonl", &pinned);
    write(
        &root,
        "benchmarks/4_d/case.toml",
        &EXACT.replace("\"active\"", "\"planned\""),
    );
    assert!(has(
        &s11(&root),
        "but the active cases are [\"1_a\", \"2_b\"]"
    ));
}

#[test]
fn a_record_that_names_no_pilot_flag_false_or_an_unread_schema_is_refused() {
    let root = cases("schema");
    let head = commit(&root, "head");
    let one = record(
        "1_a",
        "push",
        &head,
        false,
        "pass",
        r#"{"x_error":5e-13,"share":0.4}"#,
    );
    for (record, why) in [
        (
            one.replace("\"pilot\":false", "\"pilot\":\"false\""),
            "1_a names no pilot flag false",
        ),
        (
            one.replace("\"schema\":4", "\"schema\":3"),
            "1_a has record schema 3; results reads schema [4]",
        ),
        (
            one.replace("\"share_min\":0.1", "\"share_min\":0.2"),
            "1_a: the record bounds share_min at 2e-1, which its case's [tolerance] does not",
        ),
    ] {
        write(&root, "target/benchmarks/push/1_a.json", &record);
        assert!(has(&lever(&["results", "collect"], &root), why), "{why}");
    }
}

#[test]
fn a_passing_record_that_measures_fewer_bounds_than_its_case_is_refused_and_a_failing_one_is_kept()
{
    let root = cases("missing-bound");
    let head = commit(&root, "records");
    for (outcome, refused) in [("pass", true), ("fail", false)] {
        let one = record("1_a", "push", &head, false, outcome, r#"{"x_error":5e-13}"#)
            .replace(r#""share_min":0.1,"#, "");
        write(&root, "target/benchmarks/push/1_a.json", &one);
        let found = lever(&["results", "collect"], &root);
        let named = "1_a passes without a measure of each bound of its case's [tolerance]";
        assert_eq!(has(&found, named), refused, "{outcome}: {found:?}");
    }
}

#[test]
fn science_s11_names_a_kept_row_whose_record_its_case_now_refuses() {
    let root = cases("refused-row");
    let head = commit(&root, "records");
    push_records(&root, &head);
    assert!(lever(&["results", "collect"], &root).is_empty());
    write(
        &root,
        "benchmarks/1_a/case.toml",
        &PUSH.replace("x_error_max = 1e-12", "x_error_max = 1e-11"),
    );
    let found = s11(&root);
    assert!(
        has(
            &found,
            "1_a: its record in benchmarks/RECORDS.jsonl is refused: "
        ),
        "{found:?}"
    );
}

#[test]
fn a_control_character_in_a_results_string_is_written_as_its_escape() {
    let root = cases("control");
    write(
        &root,
        "benchmarks/results-what.toml",
        &WHAT.replace("what = \"x vs analytic\"", "what = \"x vs\\u0007analytic\""),
    );
    let head = commit(&root, "records");
    push_records(&root, &head);
    assert!(lever(&["results", "collect"], &root).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert!(
        results.contains("what = \"x vs\\u0007analytic\""),
        "{results}"
    );
    assert!(!results.contains('\u{7}'), "{results}");
}
