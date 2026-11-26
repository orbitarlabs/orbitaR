use crate::{cargo_stub_read_by_shell, git, has, tree, write};
use sha2::{Digest, Sha256};
use std::path::Path;
use xtask::Env;

fn engine(key: &str) -> String {
    let body = format!(
        "/usr/bin/grep -q step_energies input.toml && exit 9\nf=$(/usr/bin/sed -n 's/^{key} = //p' input.toml)\nt=$(/usr/bin/sed -n 's/^threads = //p' input.toml)\necho \"{{\\\"free_energy_ha\\\": $f, \\\"threads\\\": $t}}\"\n"
    );
    naming(&body)
}

fn naming(body: &str) -> String {
    crate::harness::wrapped(body)
}

const CASE: &str = "[case]\ntier = \"TIER\"\nstatus = \"STATUS\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\nerror_max = 1e-12\n";

fn case(tier: &str, status: &str) -> String {
    CASE.replace("TIER", tier).replace("STATUS", status)
}

fn committed(name: &str) -> (crate::Tree, Env) {
    let push = case("push", "active");
    let pr = case("pr", "active");
    let planned = case("pr", "planned");
    let nightly = case("nightly", "active");
    let root = tree(
        name,
        &[
            ("engine.sh", &engine("a")),
            ("benchmarks/1_a/case.toml", &push),
            (
                "benchmarks/1_a/input.toml",
                "schema = 1\na = 1.5\nb = 1.5\n",
            ),
            ("benchmarks/2_b/case.toml", &pr),
            (
                "benchmarks/2_b/input.toml",
                "schema = 1\na = 2.0\nb = 2.0\n",
            ),
            (
                "benchmarks/2_b/inputs/seeded.toml",
                "schema = 1\na = 1.0\nb = 1.5\n",
            ),
            ("benchmarks/3_c/case.toml", &planned),
            (
                "benchmarks/3_c/input.toml",
                "schema = 1\na = 1.0\nb = 0.0\n",
            ),
            ("benchmarks/4_d/case.toml", &nightly),
            (
                "benchmarks/4_d/input.toml",
                "schema = 1\na = 1.0\nb = 0.0\n",
            ),
        ],
    );
    let env = cargo_stub_read_by_shell(
        &root,
        false,
        &format!(
            "pwd >> {}\nwhile [ \"$1\" != --target-dir ]; do shift; done\nbin=\"$2/release/orbitar\"\n\
             mkdir -p \"$2/release\" && {{ echo '#!/bin/sh'; cat engine.sh; }} > \"$bin\" && chmod +x \"$bin\"",
            root.join("builds.log").display()
        ),
    );
    git(&root, &["init", "-q"]);
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "base"]);
    (root, env)
}

fn cores() -> usize {
    std::thread::available_parallelism().unwrap().get().max(2)
}

fn same(root: &Path, env: &Env) -> Vec<String> {
    xtask::run(&["same", "--base", "HEAD"], root, env)
}

#[test]
fn equal_engines_pass() {
    let (root, env) = committed("same-equal");
    let f = same(&root, &env);
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn the_base_builds_outside_the_work_tree_in_a_directory_that_is_removed_after() {
    let (root, env) = committed("same-outside");
    let f = same(&root, &env);
    assert!(f.is_empty(), "{f:?}");
    let builds = std::fs::read_to_string(root.join("builds.log")).unwrap();
    let dirs: Vec<&Path> = builds.lines().map(Path::new).collect();
    assert_eq!(dirs.len(), 2, "{builds}");
    let here = root.canonicalize().unwrap();
    assert_eq!(dirs[1].canonicalize().unwrap(), here);
    assert!(!dirs[0].starts_with(&here), "{builds}");
    assert!(!dirs[0].exists(), "{builds}");
}

fn differing(input: &str, base: &str, here: &str, gap: &str) -> Vec<String> {
    let sha = |text: String| -> String {
        (Sha256::digest(text).iter())
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let printed = |f: &str, p: usize| format!("{{\"free_energy_ha\": {f}, \"threads\": {p}}}");
    [1, cores()]
        .into_iter()
        .map(|p| {
            format!(
                "same: {input} at {p} threads: stdout sha256 {} at HEAD, {} here; |ΔF| = {gap} Ha",
                sha(printed(base, p)),
                sha(printed(here, p))
            )
        })
        .collect()
}

#[test]
fn each_active_pr_tier_input_whose_stdout_differs_is_named_at_both_thread_counts() {
    let (root, env) = committed("same-seeded");
    write(&root, "engine.sh", &engine("b"));
    let f = same(&root, &env);
    assert_eq!(f, differing("2_b/seeded", "1.0", "1.5", "5e-1"));
}

const FRESH: &str = "schema = 1\na = 3.0\nb = 4.0\n";

#[test]
fn an_input_or_a_case_new_since_the_base_is_not_compared_and_the_shared_inputs_are() {
    let (root, env) = committed("same-new");
    write(&root, "engine.sh", &engine("b"));
    write(&root, "benchmarks/2_b/inputs/fresh.toml", FRESH);
    write(&root, "benchmarks/5_e/case.toml", &case("pr", "active"));
    write(
        &root,
        "benchmarks/5_e/input.toml",
        "schema = 1\na = 5.0\nb = 6.0\n",
    );
    let f = same(&root, &env);
    assert_eq!(f, differing("2_b/seeded", "1.0", "1.5", "5e-1"));
}

fn engine_or(missing: &str) -> String {
    engine("a").replace("\nt=", &format!("\n[ -n \"$f\" ] || f={missing}\nt="))
}

#[test]
fn a_generated_input_at_the_base_compares_and_one_new_since_the_base_does_not() {
    let (root, env) = committed("same-generated");
    let url = format!("file://{}/remote", root.display());
    let table = crate::harness::jth(&url, &[("pbe", "Si", "si", Some(10.0))]);
    write(&root, "datasets/jth-2.0.toml", &table);
    write(&root, "remote/JTH-PBE-v2.0/Si/Si.xml", "si");
    write(&root, "benchmarks/2_b/crystals.json", crate::harness::SI);
    write(&root, "engine.sh", &engine_or("1.0"));
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "generated inputs"]);
    write(&root, "engine.sh", &engine_or("2.0"));
    write(&root, "benchmarks/1_a/crystals.json", crate::harness::SI);
    let f = same(&root, &env);
    assert_eq!(f.len(), 14, "{f:?}");
    for finding in &f {
        assert!(finding.starts_with("same: 2_b/Si-X/Diamond@"), "{f:?}");
        assert!(finding.ends_with("; |ΔF| = 1e0 Ha"), "{f:?}");
    }
}

fn fresh_and_seeded() -> Vec<String> {
    let [fresh, seeded] = [
        differing("2_b/fresh", "3.0", "4.0", "1e0"),
        differing("2_b/seeded", "1.0", "1.5", "5e-1"),
    ];
    (fresh.into_iter().zip(seeded))
        .flat_map(|(a, b)| [a, b])
        .collect()
}

#[test]
fn a_case_whose_inputs_the_base_cannot_list_is_a_finding_and_each_of_its_inputs_compares() {
    let (root, env) = committed("same-base-inputs");
    write(&root, "benchmarks/2_b/inputs/notes.txt", "not an input");
    git(&root, &["add", "-A"]);
    git(&root, &["commit", "-q", "-m", "a file that is no input"]);
    std::fs::remove_file(root.join("benchmarks/2_b/inputs/notes.txt")).unwrap();
    write(&root, "engine.sh", &engine("b"));
    write(&root, "benchmarks/2_b/inputs/fresh.toml", FRESH);
    let f = same(&root, &env);
    assert_eq!(f.len(), 5, "{f:?}");
    assert!(f[0].starts_with("same: 2_b: its inputs at HEAD: "), "{f:?}");
    let reason = "inputs/notes.txt: inputs/ holds only .toml files; none counts as new";
    assert!(f[0].ends_with(reason), "{f:?}");
    assert_eq!(f[1..], fresh_and_seeded());
}

#[test]
fn a_base_whose_case_list_cannot_be_read_is_a_finding_and_every_input_compares() {
    let (root, env) = committed("same-base-cases");
    write(&root, "benchmarks/Bad/keep.txt", "no case");
    git(&root, &["add", "-A"]);
    git(
        &root,
        &["commit", "-q", "-m", "a directory that is no case"],
    );
    std::fs::remove_dir_all(root.join("benchmarks/Bad")).unwrap();
    write(&root, "engine.sh", &engine("b"));
    write(&root, "benchmarks/2_b/inputs/fresh.toml", FRESH);
    let f = same(&root, &env);
    let listed =
        "same: the cases at HEAD: benchmarks/Bad: must be <N>_<slug>; no input counts as new";
    assert_eq!(f[0], listed, "{f:?}");
    assert_eq!(f[1..], fresh_and_seeded());
}

fn failing_on_seeded(key: &str) -> String {
    format!(
        "/usr/bin/grep -q '^a = 1.0$' input.toml && exit 3\n{}",
        engine(key)
    )
}

#[test]
fn a_run_that_fails_is_a_finding_and_the_other_inputs_of_its_case_still_compare() {
    let (root, env) = committed("same-fails-here");
    write(&root, "engine.sh", &failing_on_seeded("b"));
    write(
        &root,
        "benchmarks/2_b/input.toml",
        "schema = 1\na = 2.0\nb = 2.5\n",
    );
    let f = same(&root, &env);
    assert_eq!(f.len(), 4, "{f:?}");
    for (pair, p) in f.chunks(2).zip([1, cores()]) {
        let differs = format!("same: 2_b/input at {p} threads: stdout sha256 ");
        assert!(pair[0].starts_with(&differs), "{f:?}");
        assert!(pair[0].ends_with("; |ΔF| = 5e-1 Ha"), "{f:?}");
        let fails = format!(
            "same: 2_b/seeded at {p} threads: the engine here fails: 2_b/seeded: engine exited with exit status: 3"
        );
        assert!(pair[1].starts_with(&fails), "{f:?}");
    }
}

#[test]
fn an_input_that_the_base_engine_cannot_run_is_a_finding_that_names_the_base() {
    let (root, env) = committed("same-fails-base");
    write(&root, "engine.sh", &failing_on_seeded("a"));
    git(&root, &["commit", "-q", "-am", "fails on seeded"]);
    write(&root, "engine.sh", &engine("a"));
    let f = same(&root, &env);
    assert_eq!(f.len(), 2, "{f:?}");
    for (finding, p) in f.iter().zip([1, cores()]) {
        let fails = format!(
            "same: 2_b/seeded at {p} threads: the engine at HEAD fails: 2_b/seeded: engine exited with exit status: 3"
        );
        assert!(finding.starts_with(&fails), "{f:?}");
    }
}

#[test]
fn a_base_that_git_cannot_check_out_is_a_finding() {
    let (root, env) = committed("same-no-base");
    let f = xtask::run(&["same", "--base", "no-such-rev"], &root, &env);
    assert!(has(&f, "same: cannot check out no-such-rev"), "{f:?}");
}

fn failing(name: &str) -> (crate::Tree, Env, std::path::PathBuf) {
    let (root, _) = committed(name);
    let builds = root.join("builds.log");
    let env = cargo_stub_read_by_shell(
        &root,
        false,
        &format!("pwd >> {}\nexit 101", builds.display()),
    );
    (root, env, builds)
}

#[test]
fn a_failed_build_is_a_finding() {
    let (root, env, _) = failing("same-no-build");
    let f = same(&root, &env);
    assert!(has(&f, "exited with"), "{f:?}");
}

#[test]
fn a_failed_build_leaves_no_base_checkout() {
    let (root, env, builds) = failing("same-no-build-checkout");
    let _ = same(&root, &env);
    let log = std::fs::read_to_string(&builds).unwrap();
    let base = Path::new(log.lines().next().unwrap());
    assert!(!base.starts_with(root.canonicalize().unwrap()), "{log}");
    assert!(!base.exists(), "{log}");
}

#[test]
fn no_active_push_or_pr_input_is_a_finding() {
    let (root, env) = committed("same-none");
    for id in ["1_a", "2_b"] {
        let path = format!("benchmarks/{id}/case.toml");
        let text = std::fs::read_to_string(root.join(&path)).unwrap();
        write(&root, &path, &text.replace("\"active\"", "\"planned\""));
    }
    let f = same(&root, &env);
    assert_eq!(
        f,
        ["same: no active push or pr input; nothing was compared"]
    );
}

fn timed(name: &str, sleep_s: [&str; 2], step_ms: u32, tags: [&str; 2]) -> (crate::Tree, Env) {
    let (root, env) = committed(name);
    let count = root.join("count").display().to_string();
    let script = |s: &str, tag: &str| {
        format!(
            "n=$(/usr/bin/cat {count} 2>/dev/null || echo 0)\necho $((n + 1)) > {count}\n\
             k=$((n % 12 - 2)); [ $k -lt 0 ] && k=0; ms=$((k * {step_ms}))\n/usr/bin/sleep {s} $((ms / 1000)).$(printf '%03d' $((ms % 1000)))\n\
             echo '{{\"engine\": \"{tag}\"}}'\n"
        )
    };
    write(&root, "engine.sh", &script(sleep_s[0], tags[0]));
    git(&root, &["commit", "-q", "-am", "timed"]);
    write(&root, "engine.sh", &script(sleep_s[1], tags[1]));
    (root, env)
}

fn time(root: &Path, env: &Env, names: &[&str]) -> Vec<String> {
    let mut args = vec!["same", "--base", "HEAD"];
    for name in names {
        args.extend(["--time", name]);
    }
    xtask::run(&args, root, env)
}

fn numbers(part: &str) -> Vec<f64> {
    part.replace(['(', ')', '–', ','], " ")
        .split_whitespace()
        .filter_map(|w| w.parse().ok())
        .collect()
}

const STEP: f64 = 0.25;

struct Logged<'a> {
    engine: &'a str,
    seconds: f64,
    t_delta: Option<f64>,
    step: Option<usize>,
    sha: &'a str,
}

fn logged_at<'a>(log: &'a str, input: &str, threads: usize) -> Vec<Logged<'a>> {
    let threads = threads.to_string();
    (log.lines())
        .take_while(|l| *l != "# summary")
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split(' ').collect::<Vec<_>>())
        .filter(|row| row[0] == input && row[1] == threads)
        .map(|row| Logged {
            engine: row[2],
            seconds: row[3].parse().unwrap(),
            t_delta: (row[4] != "none").then(|| row[4].parse().unwrap()),
            step: (row[5] != "none").then(|| row[5].parse().unwrap()),
            sha: row[7],
        })
        .collect()
}

fn logged(log: &str, threads: usize) -> Vec<Logged<'_>> {
    logged_at(log, "1_a/input", threads)
}

fn part(text: &str, runs: &[Logged], pick: fn(&Logged) -> Option<f64>, log: &str) {
    let spread = |engine: &str| {
        let mut s: Vec<f64> = (runs.iter())
            .filter(|r| r.engine == engine)
            .map(|r| pick(r).unwrap())
            .collect();
        s.sort_by(f64::total_cmp);
        assert_eq!(s.len(), 5, "{log}");
        [s[2], s[0], s[4]]
    };
    let [base, here] = [spread("base"), spread("here")];
    let got = numbers(text);
    assert_eq!(got.len(), 7, "{text}");
    let half_ms = 5e-4 + 1e-6;
    for (g, w) in got[..6].iter().zip(base.iter().chain(&here)) {
        assert!((g - w).abs() <= half_ms, "{text}\n{log}");
    }
    let ratio = here[0] / base[0];
    let ratio_off = half_ms + ratio * 5e-7 * (1.0 / base[0] + 1.0 / here[0]);
    assert!((got[6] - ratio).abs() <= ratio_off, "{text}\n{log}");
    let tie = 1e-6 + 1e-9;
    if (base[2] - here[1]).abs() <= tie || (here[2] - base[1]).abs() <= tie {
        assert!(
            text.ends_with("ranges apart") || text.ends_with("ranges overlap"),
            "{text}\n{log}"
        );
        return;
    }
    let apart = base[2] < here[1] || here[2] < base[1];
    let ranges = if apart {
        "ranges apart"
    } else {
        "ranges overlap"
    };
    assert!(text.ends_with(ranges), "{text}\n{log}");
}

fn summarized_at(log: &str, input: &str, threads: usize) -> String {
    let head = format!("{input} at {threads} threads: wall HEAD ");
    let line = (log.lines())
        .find(|l| l.starts_with(&head))
        .unwrap_or_else(|| panic!("{log}"));
    let runs = logged_at(log, input, threads);
    assert_eq!(runs.len(), 10, "{log}");
    let parts: Vec<&str> = line[head.len() - "HEAD ".len()..].split("; ").collect();
    assert_eq!(parts.len(), 3, "{line}");
    part(parts[0], &runs, |r| Some(r.seconds), log);
    stdout_part(parts[2], &runs, line, log);
    let t_delta = parts[1]
        .strip_prefix("T(δ) ")
        .unwrap_or_else(|| panic!("{line}"));
    let lacking = |engine: &str| {
        (runs.iter())
            .filter(|r| r.engine == engine && r.t_delta.is_none())
            .count()
    };
    for r in &runs {
        assert_eq!(r.t_delta.is_some(), r.step.is_some(), "{log}");
    }
    if lacking("base") + lacking("here") == 0 {
        let (clock, steps) = t_delta
            .split_once(", steps ")
            .unwrap_or_else(|| panic!("{line}"));
        part(clock, &runs, |r| r.t_delta, log);
        let range = |engine: &str| {
            let n: Vec<usize> = (runs.iter())
                .filter(|r| r.engine == engine)
                .map(|r| r.step.unwrap())
                .collect();
            format!("{}–{}", n.iter().min().unwrap(), n.iter().max().unwrap())
        };
        assert_eq!(
            steps,
            format!("HEAD {}, here {}", range("base"), range("here")),
            "{line}"
        );
        return clock.to_owned();
    } else {
        assert_eq!(
            t_delta,
            format!(
                "HEAD {} of 5 runs, here {} of 5 runs have none: no step line, no free_energy_ha, or no atoms",
                lacking("base"),
                lacking("here")
            ),
            "{log}"
        );
    }
    t_delta.to_owned()
}

fn stdout_part(text: &str, runs: &[Logged], line: &str, log: &str) {
    let sha = |engine: &str| runs.iter().find(|r| r.engine == engine).map(|r| r.sha);
    if sha("base") == sha("here") {
        assert_eq!(text, "stdout equal", "{line}\n{log}");
    } else {
        assert!(text.starts_with("stdout differs"), "{line}\n{log}");
    }
}

fn summarized(log: &str, threads: usize) -> String {
    summarized_at(log, "1_a/input", threads);
    let head = format!("1_a/input at {threads} threads: wall HEAD ");
    (log.lines())
        .find(|l| l.starts_with(&head))
        .unwrap()
        .to_owned()
}

#[test]
fn a_timed_input_runs_five_times_per_engine_in_abba_order_with_each_run_logged() {
    let (root, env) = timed("same-time", ["0", "0"], 0, ["a", "a"]);
    let f = time(&root, &env, &["1_a/input"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    let cores = cores();
    let runs: Vec<Vec<&str>> = (log.lines())
        .take_while(|l| *l != "# summary")
        .filter(|l| !l.starts_with('#'))
        .map(|l| l.split(' ').collect())
        .collect();
    let abba = [
        "base", "here", "here", "base", "base", "here", "here", "base", "base", "here",
    ];
    for (block, threads) in [1, cores].into_iter().enumerate() {
        let rows = &runs[10 * block..10 * block + 10];
        for (row, engine) in rows.iter().zip(abba) {
            assert_eq!(row[1..3], [threads.to_string().as_str(), engine], "{log}");
        }
    }
    assert_eq!(runs.len(), 20, "{log}");
}

#[test]
fn a_timed_input_reports_each_engines_median_and_range_and_whether_the_ranges_are_apart() {
    let (root, env) = timed("same-overlap", ["0", "0"], 250, ["a", "a"]);
    let f = time(&root, &env, &["1_a/input"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    let cores = cores();
    for threads in [1, cores] {
        summarized(&log, threads);
        for (k, run) in logged(&log, threads).iter().enumerate() {
            assert!(run.seconds >= k as f64 * STEP - SLACK, "run {k}\n{log}");
        }
    }
}

fn summary_lines(name: &str, sleep_s: [&str; 2], tags: [&str; 2]) -> Vec<String> {
    let (root, env) = timed(name, sleep_s, 0, tags);
    let f = time(&root, &env, &["1_a/input"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    let cores = cores();
    let sleep = |engine: &str| {
        sleep_s[usize::from(engine == "here")]
            .parse::<f64>()
            .unwrap()
    };
    [1, cores]
        .map(|threads| {
            for run in logged(&log, threads) {
                assert!(run.seconds >= sleep(run.engine) - SLACK, "{log}");
            }
            summarized(&log, threads)
        })
        .to_vec()
}

const SLOW: &str = "1.0";

#[test]
fn a_slower_base_is_reported_apart() {
    for line in summary_lines("same-apart-base", [SLOW, "0"], ["a", "a"]) {
        assert!(
            line.split("; ").next().unwrap().ends_with("ranges apart"),
            "{line}"
        );
    }
}

#[test]
fn a_slower_here_is_reported_apart() {
    for line in summary_lines("same-apart-here", ["0", SLOW], ["a", "a"]) {
        assert!(
            line.split("; ").next().unwrap().ends_with("ranges apart"),
            "{line}"
        );
    }
}

#[test]
fn engines_that_print_different_stdouts_are_reported_as_different() {
    for line in summary_lines("same-differs", ["0", "0"], ["a", "b"]) {
        assert!(line.ends_with("; stdout differs"), "{line}");
    }
}

#[test]
fn the_log_names_both_revisions_and_whether_the_work_tree_is_dirty() {
    let (root, env) = timed("same-revisions", ["0", "0"], 0, ["a", "a"]);
    let f = time(&root, &env, &["1_a/input"]);
    assert!(f.is_empty(), "{f:?}");
    let head = std::process::Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(&root)
        .output()
        .unwrap();
    let head = String::from_utf8(head.stdout).unwrap().trim().to_owned();
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    let mut lines = log.lines();
    assert_eq!(
        lines.next(),
        Some(format!("# base HEAD {head}").as_str()),
        "{log}"
    );
    assert_eq!(
        lines.next(),
        Some(format!("# here {head} with a dirty work tree").as_str()),
        "{log}"
    );
}

#[test]
fn an_engine_whose_runs_print_different_stdouts_is_a_finding() {
    let (root, env) = committed("same-noisy");
    let count = root.join("count").display().to_string();
    write(
        &root,
        "engine.sh",
        &format!(
            "n=$(/usr/bin/cat {count} 2>/dev/null || echo 0)\necho $((n + 1)) > {count}\necho $n\n"
        ),
    );
    git(&root, &["commit", "-q", "-am", "counting"]);
    let f = time(&root, &env, &["1_a/input"]);
    let cores = cores();
    let want: Vec<String> = [1, cores]
        .iter()
        .map(|p| {
            format!(
                "same: 1_a/input at {p} threads: the here engine's runs print different stdouts"
            )
        })
        .collect();
    assert_eq!(f, want);
}

#[test]
fn a_timed_name_that_is_not_an_input_is_a_finding() {
    let (root, env) = committed("same-time-names");
    let f = time(&root, &env, &["1_a", "1_a/nope", "9_z/input"]);
    assert_eq!(f[0], "same: --time 1_a: name an input as <case>/<input>");
    assert_eq!(f[1], "same: --time 1_a/nope: case 1_a has no input nope");
    assert!(has(&f[2..], "9_z"), "{f:?}");
    assert_eq!(f.len(), 3, "{f:?}");
}

#[test]
fn an_engine_without_step_lines_reports_that_no_run_meets_the_t_delta_rule() {
    let (root, env) = timed("same-no-steps", ["0", "0"], 0, ["a", "a"]);
    let f = time(&root, &env, &["1_a/input"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    for threads in [1, cores()] {
        assert_eq!(
            summarized_at(&log, "1_a/input", threads),
            "HEAD 5 of 5 runs, here 5 of 5 runs have none: no step line, no free_energy_ha, or no atoms"
        );
    }
}

const GAP: f64 = 0.5;

type Steps<'a> = &'a [(bool, &'a str)];

const SI2: &str = "schema = 1\n\n[structure]\nspecies = [\"Si\", \"Si\"]\n";

fn script(steps: Steps) -> String {
    let mut text = String::from("/usr/bin/grep -qx 'step_energies = true' input.toml || exit 7\n");
    for (n, (gap, f)) in steps.iter().enumerate() {
        if *gap {
            text.push_str(&format!("/usr/bin/sleep {GAP}\n"));
        }
        let line = (crate::harness::speed::step_line())
            .replace("<n>", &(n + 1).to_string())
            .replace("<F>", f);
        text.push_str(&format!("echo '{line}' >&2\n"));
    }
    let end = steps.last().map_or("-10.0", |s| s.1);
    naming(&(text + &format!("echo '{{\"free_energy_ha\": {end}}}'\n")))
}

fn stepping(name: &str, steps: [Steps; 2]) -> (crate::Tree, Env) {
    let (root, env) = committed(name);
    write(&root, "benchmarks/1_a/inputs/si.toml", SI2);
    write(&root, "remote/JTH-PBE-v2.0/Si/Si.xml", "si");
    let url = format!("file://{}/remote", root.display());
    let table = crate::harness::jth(&url, &[("pbe", "Si", "si", None)]);
    write(&root, "datasets/jth-2.0.toml", &table);
    write(&root, "engine.sh", &script(steps[0]));
    git(&root, &["commit", "-q", "-am", "stepping"]);
    write(&root, "engine.sh", &script(steps[1]));
    (root, env)
}

type Stepped = (f64, Option<f64>, Option<usize>, String);

fn stepped(name: &str, steps: [Steps; 2]) -> (Vec<Stepped>, Vec<String>) {
    let (root, env) = stepping(name, steps);
    let f = time(&root, &env, &["1_a/si"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    let mut runs = Vec::new();
    let mut parts = Vec::new();
    for threads in [1, cores()] {
        parts.push(summarized_at(&log, "1_a/si", threads));
        for r in logged_at(&log, "1_a/si", threads) {
            runs.push((r.seconds, r.t_delta, r.step, r.engine.to_owned()));
        }
    }
    (runs, parts)
}

fn arrives_between(runs: &[Stepped], engine: &str, step: usize, before: f64, after: f64) {
    for (wall, t, n, e) in runs.iter().filter(|r| r.3 == engine) {
        let t = t.unwrap_or_else(|| panic!("{e}: no T(δ)"));
        assert_eq!(*n, Some(step), "{e}: T(δ) {t}, wall {wall}");
        assert!(t >= before * GAP - SLACK, "{e}: T(δ) {t}, wall {wall}");
        assert!(
            wall - t >= after * GAP - SLACK,
            "{e}: T(δ) {t}, wall {wall}"
        );
    }
}

const SLACK: f64 = 0.1;

const SETTLES: Steps = &[
    (false, "-9.0"),
    (true, "-10.000005"),
    (true, "-10.000005"),
    (false, "-10.0"),
];

#[test]
fn t_delta_is_the_arrival_of_the_first_step_line_from_which_every_step_stays_within_delta_of_the_end()
 {
    let (runs, _) = stepped("same-t-delta", [SETTLES, SETTLES]);
    arrives_between(&runs, "base", 2, 1.0, 1.0);
    arrives_between(&runs, "here", 2, 1.0, 1.0);
}

#[test]
fn a_late_step_past_delta_moves_t_delta_to_the_step_after_it() {
    let (runs, _) = stepped("same-t-delta-late", [SETTLES, EXCURSION]);
    arrives_between(&runs, "base", 2, 1.0, 1.0);
    arrives_between(&runs, "here", 3, 2.0, 1.0);
}

const EXCURSION: Steps = &[
    (false, "-10.000005"),
    (true, "-10.00001"),
    (true, "-10.000005"),
    (true, "-10.0"),
];

const EARLY: Steps = &[(false, "-10.0"), (true, "-10.0"), (true, "-10.0")];
const LATE: Steps = &[(false, "-9.0"), (true, "-9.5"), (true, "-10.0")];

#[test]
fn a_base_that_settles_later_has_its_t_delta_range_apart() {
    let (_, parts) = stepped("same-t-delta-base", [LATE, EARLY]);
    for part in parts {
        assert!(part.ends_with("ranges apart"), "{part}");
    }
}

#[test]
fn a_here_that_settles_later_has_its_t_delta_range_apart() {
    let (_, parts) = stepped("same-t-delta-here", [EARLY, LATE]);
    for part in parts {
        assert!(part.ends_with("ranges apart"), "{part}");
    }
}

#[test]
fn runs_without_t_delta_are_counted_per_engine() {
    let (_, parts) = stepped("same-t-delta-none", [&[], SETTLES]);
    for part in parts {
        assert_eq!(
            part,
            "HEAD 5 of 5 runs, here 0 of 5 runs have none: no step line, no free_energy_ha, or no atoms"
        );
    }
}

#[test]
fn engines_that_end_at_different_energies_name_the_gap_against_delta_atoms() {
    let other: Steps = &[(false, "-9.0"), (true, "-10.00001")];
    let (root, env) = stepping("same-t-delta-gap", [SETTLES, other]);
    let f = time(&root, &env, &["1_a/si"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    for threads in [1, cores()] {
        summarized_at(&log, "1_a/si", threads);
        let line = (log.lines())
            .find(|l| l.starts_with(&format!("1_a/si at {threads} threads: ")))
            .unwrap();
        let (_, gap) = line
            .split_once("; stdout differs, |ΔF| ")
            .unwrap_or_else(|| panic!("{line}"));
        let (gap, delta) = gap.split_once(" Ha against δ·atoms ").unwrap();
        assert!((gap.parse::<f64>().unwrap() - 1e-5).abs() < 1e-12, "{line}");
        assert_eq!(delta, "7.349864435130998e-6 Ha", "{line}");
    }
}

#[test]
fn the_summary_names_each_engines_range_of_step_lines_that_meet_t_delta() {
    let (root, env) = stepping("same-t-delta-range", [SETTLES, SETTLES]);
    let count = root.join("count").display().to_string();
    let alternating = format!(
        "n=$(/usr/bin/cat {count} 2>/dev/null || echo 0)\necho $((n + 1)) > {count}\n\
         if [ $((n % 2)) = 0 ]; then\n{}else\n{}fi\n",
        script(SETTLES),
        script(EXCURSION)
    );
    write(&root, "engine.sh", &alternating);
    let f = time(&root, &env, &["1_a/si"]);
    assert!(f.is_empty(), "{f:?}");
    let log = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    for threads in [1, cores()] {
        summarized_at(&log, "1_a/si", threads);
        let line = (log.lines())
            .find(|l| l.starts_with(&format!("1_a/si at {threads} threads: ")))
            .unwrap();
        assert!(line.contains(", steps HEAD 2–2, here 2–3; "), "{line}");
    }
}

fn claim_tree(name: &str, tuned: &str) -> (crate::Tree, Env, std::path::PathBuf) {
    let (root, env) = stepping(name, [&[(false, "-10.0")], &[(false, "-10.0")]]);
    let input = format!(
        "{}\n[parallel]\nthreads = 3\n\n[outputs]\nstep_energies = false\n",
        crate::harness::rival::INPUT
    );
    write(&root, "benchmarks/1_a/inputs/si.toml", &input);
    write(
        &root,
        "benchmarks/RIVALS.toml",
        crate::harness::rival::RIVALS,
    );
    write(&root, "benchmarks/TUNED.toml", tuned);
    let log = root.join("cutoffs.log");
    let engine = format!(
        "/usr/bin/grep -E '^(cutoff_ha|density_cutoff_ha) = ' input.toml >> '{}'\n{}",
        log.display(),
        script(&[(false, "-10.0")])
    );
    write(&root, "engine.sh", &engine);
    git(&root, &["commit", "-q", "-am", "logs its cutoffs"]);
    (root, env, log)
}

const FROZEN: &str = "[abinit.cutoff_ha]\nSi = 12.5\n\n[abinit.calibration]\n\"benchmarks/1_a/input.toml\" = [[12.5, 1e-6, 0.0]]\n";

#[test]
fn claim_cutoff_times_both_engines_at_abinits_frozen_cutoff_and_its_density_ratio() {
    let (root, env, log) = claim_tree("same-claim-cutoff", FROZEN);
    let f = xtask::run(
        &[
            "same",
            "--base",
            "HEAD",
            "--time",
            "1_a/si",
            "--claim-cutoff",
        ],
        &root,
        &env,
    );
    assert!(f.is_empty(), "{f:?}");
    let seen = std::fs::read_to_string(&log).unwrap();
    let lines: Vec<&str> = seen.lines().collect();
    assert_eq!(lines.len(), 2 * 2 * 12, "{seen}");
    for pair in lines.chunks(2) {
        assert_eq!(
            pair,
            ["cutoff_ha = 12.5", "density_cutoff_ha = 25.0"],
            "{seen}"
        );
    }
    let time = std::fs::read_to_string(root.join("target/same/time.log")).unwrap();
    let sha = crate::harness::hex(&Sha256::digest(FROZEN));
    assert!(
        time.contains(&format!(
            "# cutoffs: the claim's, ABINIT's frozen ones in benchmarks/TUNED.toml sha256 {sha}\n\
             # 1_a/si: cutoff 12.5 Ha, density 25 Ha\n"
        )),
        "{time}"
    );
    for threads in [1, cores()] {
        let head = format!("1_a/si at 12.5 Ha (density 25 Ha) at {threads} threads: wall HEAD ");
        assert!(time.lines().any(|l| l.starts_with(&head)), "{time}");
    }
}

#[test]
fn claim_cutoff_refuses_an_input_whose_element_abinit_froze_no_cutoff_for() {
    let (root, env, log) = claim_tree("same-claim-cutoff-none", "[abinit.cutoff_ha]\nAl = 12.5\n");
    let f = xtask::run(
        &[
            "same",
            "--base",
            "HEAD",
            "--time",
            "1_a/si",
            "--claim-cutoff",
        ],
        &root,
        &env,
    );
    assert_eq!(
        f,
        [
            "same: --time 1_a/si: benchmarks/TUNED.toml has no abinit cutoff for Si; run `cargo xtask rivals calibrate` on the node"
        ]
    );
    assert!(!log.exists(), "{}", std::fs::read_to_string(&log).unwrap());
    assert!(!root.join("builds.log").exists());
}

#[test]
fn claim_cutoff_refuses_an_unreadable_tuned_file_before_any_build() {
    let (root, env, _) = claim_tree("same-claim-cutoff-unreadable", "[abinit.cutoff_ha\n");
    let f = xtask::run(
        &[
            "same",
            "--base",
            "HEAD",
            "--time",
            "1_a/si",
            "--claim-cutoff",
        ],
        &root,
        &env,
    );
    assert!(
        f.len() == 1 && f[0].starts_with("same: --claim-cutoff: benchmarks/TUNED.toml: "),
        "{f:?}"
    );
    assert!(!root.join("builds.log").exists());
}

#[test]
fn claim_cutoff_reads_a_five_column_tuned_file_and_takes_the_largest_cutoff_of_a_cells_elements() {
    let tuned = "[abinit]\nvolume_scale = 0.94\n\n[abinit.cutoff_ha]\nAl = 15.0\nSi = 12.5\n\n[abinit.calibration]\n\
                 \"benchmarks/1_a/input.toml\" = [[12.5, 1e-6, 0.0, 2e-6, 0.5]]\n";
    let (root, env, log) = claim_tree("same-claim-cutoff-five", tuned);
    let alsi = std::fs::read_to_string(root.join("benchmarks/1_a/inputs/si.toml"))
        .unwrap()
        .replace("[\"Si\"]", "[\"Al\", \"Si\"]")
        .replace(
            "[[0.0, 0.0, 0.25]]",
            "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
        );
    write(&root, "benchmarks/1_a/inputs/si.toml", &alsi);
    write(&root, "remote/JTH-PBE-v2.0/Al/Al.xml", "al");
    let url = format!("file://{}/remote", root.display());
    let rows = [("pbe", "Al", "al", None), ("pbe", "Si", "si", None)];
    write(
        &root,
        "datasets/jth-2.0.toml",
        &crate::harness::jth(&url, &rows),
    );
    let f = xtask::run(
        &[
            "same",
            "--base",
            "HEAD",
            "--time",
            "1_a/si",
            "--claim-cutoff",
        ],
        &root,
        &env,
    );
    assert!(f.is_empty(), "{f:?}");
    let seen = std::fs::read_to_string(&log).unwrap();
    assert!(
        (seen.lines()).all(|l| l == "cutoff_ha = 15.0" || l == "density_cutoff_ha = 30.0"),
        "{seen}"
    );
    write(
        &root,
        "benchmarks/TUNED.toml",
        &tuned.replace("Al = 15.0\n", ""),
    );
    let f = xtask::run(
        &[
            "same",
            "--base",
            "HEAD",
            "--time",
            "1_a/si",
            "--claim-cutoff",
        ],
        &root,
        &env,
    );
    assert!(
        has(&f, "benchmarks/TUNED.toml has no abinit cutoff for Al"),
        "{f:?}"
    );
}

#[test]
fn claim_cutoff_needs_time() {
    let (root, env) = committed("same-claim-cutoff-alone");
    let f = xtask::run(&["same", "--base", "HEAD", "--claim-cutoff"], &root, &env);
    assert!(has(&f, "required arguments were not provided"), "{f:?}");
    assert!(!root.join("builds.log").exists());
}
