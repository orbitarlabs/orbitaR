use super::rivals;
use crate::harness::rival::{ABINIT, INPUT, PW, RIVALS, TUNED, rivals_tree, three_atoms};
use crate::has;

#[test]
fn each_thread_count_gets_a_share_of_the_budget_by_its_count_of_layouts() {
    let pw = format!("t=0.01\n{PW}");
    let abinit = format!("t=0.01\n{ABINIT}");
    let root = rivals_tree("tune-shares", INPUT, TUNED, &pw, &abinit);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace("budget_s = 1000.0", "budget_s = 5.0"),
    );
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(
        tuned["qe"]["searched"]["8"].as_array().unwrap().len(),
        13,
        "{tuned}"
    );
}

#[test]
fn layout_freezes_the_fastest_layout_per_thread_count_and_logs_every_one_tried() {
    let pw =
        format!("t=0.15; case \"$OMP_NUM_THREADS $* \" in \"4 \"*\"-nk 2 \"*) t=0.02;; esac\n{PW}");
    let abinit = format!(
        "t=0.15; [ \"$OMP_NUM_THREADS\" = 4 ] && grep -q 'autoparal 1' run.abi && t=0.02\n{ABINIT}"
    );
    let root = rivals_tree("tune-layout", INPUT, TUNED, &pw, &abinit);
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    let layout = |code: &str, p: &str| tuned[code]["layout"][p].clone();
    let want: toml::Value = toml::from_str::<toml::Table>(
        "x = { ranks = 2, threads = 4, args = [\"-nk\", \"2\"], input = [] }",
    )
    .unwrap()["x"]
        .clone();
    assert_eq!(layout("qe", "8"), want, "{tuned}");
    let want: toml::Value = toml::from_str::<toml::Table>(
        "x = { ranks = 2, threads = 4, args = [], input = [\"paral_kgb 1\", \"autoparal 1\", \"istwfk *1\"] }",
    )
    .unwrap()["x"]
        .clone();
    assert_eq!(layout("abinit", "8"), want, "{tuned}");
    assert_eq!(
        tuned["qe"]["searched"]["8"].as_array().unwrap().len(),
        13,
        "{tuned}"
    );
    let abinit: Vec<&str> = (tuned["abinit"]["searched"]["8"].as_array().unwrap().iter())
        .map(|l| l.as_str().unwrap())
        .collect();
    assert_eq!(abinit.len(), 10, "{tuned}");
    for (ranks, threads) in [(1, 8), (2, 4), (4, 2), (8, 1)] {
        let layout =
            format!("Layout {{ ranks: {ranks}, threads: {threads}, args: [], input: [] }}");
        assert!(
            abinit.iter().any(|l| l.starts_with(&layout)),
            "{layout}: {abinit:?}"
        );
    }
    assert_eq!(
        tuned["qe"]["cutoff_ha"]["Si"].as_float(),
        Some(10.0),
        "{tuned}"
    );
}

#[test]
fn abinits_band_split_keeps_the_default_istwfk_and_rounds_abinits_band_count_up_to_the_split() {
    let input = INPUT.replace("\n[bands]\ncount = 8\n", "");
    let abinit = format!(
        "t=0.15; grep -q 'npfft 1' run.abi && grep -qx 'npband 4' run.abi && grep -qx 'nband 4' run.abi && ! grep -q istwfk run.abi && t=0.02\n{ABINIT}"
    );
    let root = rivals_tree("tune-bands", &input, TUNED, PW, &abinit);
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    let want: toml::Value = toml::from_str::<toml::Table>(
        "x = { ranks = 4, threads = 2, args = [], input = [\"paral_kgb 1\", \"np_spkpt 1\", \"npband 4\", \"npfft 1\"] }",
    )
    .unwrap()["x"]
        .clone();
    assert_eq!(tuned["abinit"]["layout"]["8"], want, "{tuned}");
}

fn split_bands(name: &str, input: &str) -> String {
    let log = std::env::temp_dir().join(format!("xtask-{name}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let abinit = format!(
        "grep -qx 'npband 2' run.abi && grep '^nband' run.abi >> '{}'\n{ABINIT}",
        log.display()
    );
    let input = three_atoms(&input.replace("\n[bands]\ncount = 8\n", ""));
    let root = rivals_tree(name, &input, TUNED, PW, &abinit);
    let (findings, _) = rivals(&root, "layout");
    let seen = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    assert!(findings.is_empty(), "{findings:?}");
    let lines: std::collections::BTreeSet<&str> = seen.lines().collect();
    assert_eq!(lines.len(), 1, "{seen}");
    lines.into_iter().next().unwrap().to_owned()
}

#[test]
fn abinits_band_count_follows_its_own_rule_for_smearing_fixed_occupations_and_spin() {
    let fixed = INPUT.replace(
        "[occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.01\n",
        "",
    );
    let spin = format!("{INPUT}\n[spin]\nmoments = [-4.0, 0.0, 0.0]\n");
    for (name, input, want) in [
        ("tune-bands-smeared", INPUT.to_owned(), "nband 8"),
        ("tune-bands-fixed", fixed, "nband 6"),
        ("tune-bands-spin", spin, "nband 10"),
    ] {
        assert_eq!(split_bands(name, &input), want, "{name}");
    }
}

#[test]
fn a_case_band_count_that_the_split_does_not_divide_fails_that_layout_by_name() {
    let input = INPUT.replace("count = 8", "count = 6");
    let root = rivals_tree("tune-bands-case", &input, TUNED, PW, ABINIT);
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    let searched: Vec<&str> = (tuned["abinit"]["searched"]["8"].as_array().unwrap().iter())
        .map(|l| l.as_str().unwrap())
        .collect();
    assert!(
        searched.iter().any(|l| l.contains("\"npband 4\"")
            && l.contains("failed: benchmarks/1_a/input.toml: rivals: the case's 6 bands do not split over npband 4")),
        "{searched:?}"
    );
    assert!(
        searched
            .iter()
            .any(|l| l.contains("\"npband 2\"") && !l.contains("failed")),
        "{searched:?}"
    );
}

#[test]
fn a_failed_band_split_names_the_band_count_fitted_to_the_cell() {
    let input = INPUT.replace("\n[bands]\ncount = 8\n", "");
    let abinit =
        format!("grep -qx 'npband 8' run.abi && {{ echo '--- !BUG'; exit 13; }}\n{ABINIT}");
    let root = rivals_tree("tune-bands-failed", &input, TUNED, PW, &abinit);
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    let searched: Vec<&str> = (tuned["abinit"]["searched"]["8"].as_array().unwrap().iter())
        .map(|l| l.as_str().unwrap())
        .collect();
    let failed = (searched.iter())
        .find(|l| l.contains("\"npband 8\""))
        .unwrap_or_else(|| panic!("{searched:?}"));
    assert!(
        failed.contains("failed: benchmarks/1_a/input.toml: with nband 8: ")
            && failed.contains("13"),
        "{failed}"
    );
    assert!(
        !(searched.iter()).any(|l| l.contains("with nband") && !l.contains("npband")),
        "{searched:?}"
    );
}

#[test]
fn layout_searches_only_the_thread_counts_that_rivals_toml_names() {
    let pw = format!("t=0.01\n{PW}");
    let abinit = format!("t=0.01\n{ABINIT}");
    let root = rivals_tree("tune-threads", INPUT, TUNED, &pw, &abinit);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS
            .replace("threads = [1, 2, 4, 8]", "threads = [8]")
            .replace("budget_s = 1000.0", "budget_s = 3.0"),
    );
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    for code in ["qe", "abinit"] {
        let keys: Vec<&String> = tuned[code]["layout"].as_table().unwrap().keys().collect();
        assert_eq!(keys, ["8"], "{tuned}");
        let keys: Vec<&String> = tuned[code]["searched"].as_table().unwrap().keys().collect();
        assert_eq!(keys, ["8"], "{tuned}");
    }
    assert_eq!(
        tuned["qe"]["searched"]["8"].as_array().unwrap().len(),
        13,
        "{tuned}"
    );
}

#[test]
fn qe_pools_divide_the_k_points_of_every_layout_cell_and_each_line_gives_each_cells_seconds() {
    let pw = format!("t=0.01\n{PW}");
    let abinit = format!("t=0.01\n{ABINIT}");
    let root = rivals_tree("tune-pools", INPUT, TUNED, &pw, &abinit);
    let two = INPUT
        .replace("mesh = [2, 2, 2]", "mesh = [2, 1, 1]")
        .replace("shift = [0.5, 0.5, 0.5]\n", "");
    crate::write(&root, "benchmarks/1_a/inputs/b.toml", &two);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "layout_cells = [\"benchmarks/1_a/input.toml\"]",
            "layout_cells = [\"benchmarks/1_a/input.toml\", \"benchmarks/1_a/inputs/b.toml\"]",
        ),
    );
    let (findings, tuned) = rivals(&root, "layout");
    assert!(findings.is_empty(), "{findings:?}");
    let lines: Vec<&str> = (tuned["qe"]["searched"]["8"].as_array().unwrap().iter())
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(lines.len(), 10, "{lines:?}");
    assert!(
        lines.iter().any(|l| l.contains("\"-nk\", \"2\"")),
        "{lines:?}"
    );
    assert!(
        !(lines.iter()).any(|l| l.contains("\"-nk\", \"4\"") || l.contains("\"-nk\", \"8\"")),
        "{lines:?}"
    );
    assert!(
        (lines.iter()).all(|l| {
            let at = |cell: &str| {
                l.find(&format!("benchmarks/1_a/{cell} 0."))
                    .unwrap_or(usize::MAX)
            };
            at("input.toml") < at("inputs/b.toml") && at("inputs/b.toml") < usize::MAX
        }),
        "{lines:?}"
    );
}

#[test]
fn a_layout_left_untried_by_its_share_of_the_budget_fails_the_job_and_freezes_nothing() {
    let root = rivals_tree("tune-budget", INPUT, TUNED, PW, ABINIT);
    crate::write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace("budget_s = 1000.0", "budget_s = 0.1"),
    );
    let (findings, tuned) = rivals(&root, "layout");
    assert!(
        has(
            &findings,
            "rivals: qe: 12 layouts stayed untried, the first at 8 threads; raise budget_s in benchmarks/RIVALS.toml"
        ),
        "{findings:?}"
    );
    assert!(
        has(&findings, "rivals: abinit: 9 layouts stayed untried"),
        "{findings:?}"
    );
    assert_eq!(
        tuned,
        toml::from_str::<toml::Table>(TUNED).unwrap(),
        "TUNED.toml stays as it was"
    );
}

#[test]
fn a_thread_count_where_every_layout_fails_fails_the_job_and_keeps_the_old_layout() {
    let root = rivals_tree(
        "tune-all-fail",
        INPUT,
        TUNED,
        &format!("exit 3\n{PW}"),
        ABINIT,
    );
    let (findings, tuned) = rivals(&root, "layout");
    assert!(
        has(
            &findings,
            "rivals: qe: every layout failed at 8 threads; the first: Layout { ranks: 1"
        ),
        "{findings:?}"
    );
    let old = toml::from_str::<toml::Table>(TUNED).unwrap();
    for (p, layout) in old["qe"]["layout"].as_table().unwrap() {
        for key in ["ranks", "threads"] {
            assert_eq!(tuned["qe"]["layout"][p][key], layout[key], "{p}: {tuned}");
        }
        assert_eq!(
            tuned["qe"]["layout"][p]["args"].as_array().map(Vec::len),
            Some(0),
            "{tuned}"
        );
    }
    assert_eq!(
        tuned["qe"]["searched"].as_table().map(|t| t.len()),
        Some(0),
        "{tuned}"
    );
}
