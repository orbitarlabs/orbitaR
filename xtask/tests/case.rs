use crate::{has, lever, tree};

const ACTIVE: &str = "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n";

fn layout_of(name: &str, case_toml: &str) -> Vec<String> {
    let root = tree(name, &[("benchmarks/1_a/case.toml", case_toml)]);
    lever(&["layout"], &root)
}

#[test]
fn ids_need_a_positive_unpadded_number_and_a_lowercase_slug() {
    for bad in ["01_si", "0_si", "1_Si", "si", "1_", "+5_si", "_si"] {
        let root = tree(
            "case-id",
            &[(&format!("benchmarks/{bad}/case.toml"), ACTIVE)],
        );
        assert!(has(&lever(&["layout"], &root), "must"), "{bad}");
    }
}

#[test]
fn a_number_may_appear_once() {
    let root = tree(
        "case-twice",
        &[
            ("benchmarks/3_demo/case.toml", ACTIVE),
            ("benchmarks/3_other/case.toml", ACTIVE),
        ],
    );
    assert!(has(
        &lever(&["layout"], &root),
        "case number 3 is used twice"
    ));
}

#[test]
fn an_active_case_needs_a_finite_tolerance() {
    assert!(has(
        &layout_of("case-empty", ACTIVE),
        "non-empty [tolerance]"
    ));
    let nan = format!("{ACTIVE}\n[tolerance]\nerror_max = nan\n");
    assert!(has(&layout_of("case-nan", &nan), "must be finite"));
}

#[test]
fn claims_and_repeats_must_be_meaningful() {
    let none = ACTIVE.replace("[\"correctness\"]", "[]") + "[tolerance]\nx_max = 1.0\n";
    assert!(has(&layout_of("case-claims", &none), "claims must name"));
    let zero =
        ACTIVE.replace("analytic\"\n", "analytic\"\nrepeats = 0\n") + "[tolerance]\nx_max = 1.0\n";
    assert!(has(
        &layout_of("case-repeats", &zero),
        "repeats must be at least 1"
    ));
}

const TIMED: [(&str, &str); 2] = [
    ("speed", "speedup_min = 1.25\n"),
    ("memory", "speedup_min = 1.0\n"),
];

#[test]
fn timed_claims_and_measured_vasp_run_only_in_the_claim_tier() {
    for (claim, bound) in TIMED {
        let timed = ACTIVE.replace("correctness", claim) + "[tolerance]\n" + bound;
        assert!(
            has(&layout_of("case-timed", &timed), "claim tier"),
            "{claim}"
        );
    }
    let vasp = ACTIVE.replace("analytic", "vasp-measured") + "[tolerance]\nerror_max = 1.0\n";
    assert!(has(&layout_of("case-vasp", &vasp), "claim tier"));
}

#[test]
fn a_timed_claim_needs_its_own_accuracy_tolerance() {
    let claim = ACTIVE.replace("push", "claim");
    for (kind, bound) in TIMED {
        let alone = claim.replace("correctness", kind) + "[tolerance]\n" + bound + "x_max = 1.0\n";
        assert!(
            has(
                &layout_of("case-alone", &alone),
                "needs an accuracy or correctness claim"
            ),
            "{kind}"
        );
        let with = claim.replace("\"correctness\"", &format!("\"accuracy\", \"{kind}\""));
        let one = with.clone() + "[tolerance]\n" + bound;
        assert!(
            has(
                &layout_of("case-one", &one),
                "needs an accuracy or correctness claim"
            ),
            "{kind}"
        );
        let both = with.clone() + "[tolerance]\n" + bound + "error_max = 1e-3\n";
        let f = layout_of("case-both", &both);
        assert!(
            !has(&f, "needs an accuracy") && !has(&f, "needs a bound"),
            "{kind}: {f:?}"
        );
        let timing = with + "[tolerance]\n" + bound + "wall_s_max = 60.0\n";
        assert!(
            has(
                &layout_of("case-timing", &timing),
                "a tolerance on a measured metric"
            ),
            "{kind}"
        );
    }
}

#[test]
fn a_memory_claim_needs_a_correctness_bound_and_rivals_or_an_rss_bound() {
    let memory = ACTIVE
        .replace("push", "claim")
        .replace("\"correctness\"", "\"correctness\", \"memory\"");
    let rivals = memory.replace("\"memory\"]", "\"memory\", \"speed\"]\nrivals = [\"qe\"]");
    let planned = memory.replace("\"active\"", "\"planned\"");
    let needs_judge = "an active memory claim needs rivals, whose R judges it, or rss_exponent_max";
    for (case, tolerance, refused) in [
        (&memory, "error_max = 1e-3\n", Some(needs_judge)),
        (&memory, "error_max = 1e-3\nrss_exponent_max = 1.25\n", None),
        (
            &memory,
            "error_max = 1e-3\nrss_exponent_min = 0.5\n",
            Some(needs_judge),
        ),
        (&rivals, "error_max = 1e-3\n", None),
        (&planned, "error_max = 1e-3\n", None),
        (
            &memory,
            "rss_exponent_max = 1.25\n",
            Some("a tolerance on a measured metric"),
        ),
    ] {
        let text = case.clone() + "[tolerance]\n" + tolerance;
        let found = layout_of("case-memory", &text);
        for needle in [needs_judge, "a tolerance on a measured metric"] {
            assert_eq!(
                has(&found, needle),
                refused == Some(needle),
                "{text}: {found:?}"
            );
        }
    }
}

#[test]
fn a_timeout_above_the_tier_ceiling_is_rejected() {
    let text = ACTIVE.replace("analytic\"\n", "analytic\"\ntimeout_s = 99999\n")
        + "[tolerance]\nx_max = 1.0\n";
    assert!(has(&layout_of("case-timeout", &text), "tier ceiling"));
    let at = |t: u32| {
        ACTIVE.replace("analytic\"\n", &format!("analytic\"\ntimeout_s = {t}\n"))
            + "[tolerance]\nx_max = 1.0\n"
    };
    assert!(!has(&layout_of("case-timeout-30", &at(30)), "tier ceiling"));
    assert!(has(&layout_of("case-timeout-31", &at(31)), "tier ceiling"));
    assert!(has(&layout_of("case-timeout-0", &at(0)), "tier ceiling"));
}

#[test]
fn unknown_tables_and_bad_keys_are_rejected() {
    let unknown = format!("{ACTIVE}[tolerance]\nx_max = 1.0\n[override]\nx_max = 9.0\n");
    assert!(has(&layout_of("case-table", &unknown), "unknown field"));
    let suffix = format!("{ACTIVE}[tolerance]\nx = 1.0\n");
    assert!(has(&layout_of("case-suffix", &suffix), "_max or _min"));
}

#[test]
fn a_milestone_names_an_intent_bar_on_g_and_needs_rivals() {
    let claim = |extra: &str| {
        ACTIVE
            .replace("\"push\"", "\"claim\"")
            .replace("[\"correctness\"]", "[\"speed\", \"correctness\"]")
            .replace("analytic\"\n", &format!("analytic\"\n{extra}"))
            + "[tolerance]\nx_max = 1.0\n"
    };
    let bare = "a milestone sets the bar on G, which only rivals give";
    for (n, extra, want) in [
        (0, "rivals = [\"qe\"]\nmilestone = \"m4\"\n", None),
        (1, "rivals = [\"qe\"]\nmilestone = \"m3\"\n", None),
        (2, "milestone = \"m3\"\n", None),
        (3, "milestone = \"m4\"\n", Some(bare)),
        (
            4,
            "rivals = [\"qe\"]\nmilestone = \"m5\"\n",
            Some("unknown variant `m5`"),
        ),
    ] {
        let f = layout_of(&format!("case-milestone-{n}"), &claim(extra));
        match want {
            Some(want) => assert!(has(&f, want), "{extra}: {f:?}"),
            None => assert!(!has(&f, "milestone") && !has(&f, "m5"), "{extra}: {f:?}"),
        }
    }
}

const INPUT_TOML: &str = "benchmarks/1_a/input.toml";

fn why_findings(name: &str, case_toml: &str, input: Option<&str>) -> Vec<String> {
    let mut files = vec![("benchmarks/1_a/case.toml", case_toml)];
    if let Some(path) = input {
        files.push((path, "schema = 1\n"));
    }
    let findings = lever(&["layout"], &tree(name, &files));
    findings
        .into_iter()
        .filter(|f| f.contains("[why]") || f.contains("[relax]"))
        .collect()
}

#[test]
fn each_why_names_a_value_that_the_case_file_sets_and_holds_a_reason() {
    let with = |why: &str| {
        format!("{ACTIVE}[tolerance]\nx_max = 1.0\n\n[relax]\nb = \"ions\"\n\n[why]\n{why}")
    };
    let named = "x_max = \"r\"\nb = \"r\"\nstatus = \"r\"\ninputs = \"r\"\ntolerance = \"r\"\n";
    let f = why_findings("case-why", &with(named), Some(INPUT_TOML));
    assert!(f.is_empty(), "{f:?}");
    let stray = why_findings("case-why-stray", &with("y_max = \"r\"\n"), Some(INPUT_TOML));
    assert!(
        has(&stray, "[why].y_max names no value of the case file"),
        "{stray:?}"
    );
    let empty = why_findings("case-why-empty", &with("x_max = \" \"\n"), Some(INPUT_TOML));
    assert!(has(&empty, "[why].x_max is empty"), "{empty:?}");
}

#[test]
fn a_why_of_a_case_key_that_the_file_leaves_out_fails() {
    let base = format!("{ACTIVE}[tolerance]\nx_max = 1.0\n");
    for key in ["timeout_s", "repeats", "rivals", "milestone"] {
        let f = why_findings(
            &format!("case-why-unset-{key}"),
            &format!("{base}\n[why]\n{key} = \"r\"\n"),
            Some(INPUT_TOML),
        );
        assert!(
            has(&f, &format!("[why].{key} names no value of the case file")),
            "{key}: {f:?}"
        );
    }
    let set = ACTIVE.replace("analytic\"\n", "analytic\"\ntimeout_s = 10\nrepeats = 1\n")
        + "[tolerance]\nx_max = 1.0\n\n[why]\ntimeout_s = \"r\"\nrepeats = \"r\"\n";
    let f = why_findings("case-why-set", &set, Some(INPUT_TOML));
    assert!(f.is_empty(), "{f:?}");
    let claim = ACTIVE
        .replace("\"push\"", "\"claim\"")
        .replace("[\"correctness\"]", "[\"speed\", \"correctness\"]")
        .replace(
            "analytic\"\n",
            "analytic\"\nrivals = [\"qe\"]\nmilestone = \"m3\"\n",
        )
        + "[tolerance]\nx_max = 1.0\n\n[why]\nrivals = \"r\"\nmilestone = \"r\"\n";
    let f = why_findings("case-why-set-claim", &claim, Some(INPUT_TOML));
    assert!(f.is_empty(), "{f:?}");
}

#[test]
fn the_why_groups_need_their_table_and_their_inputs() {
    let planned = ACTIVE.replace("\"active\"", "\"planned\"") + "\n[why]\ntolerance = \"r\"\n";
    let f = why_findings("case-why-no-tolerance", &planned, Some(INPUT_TOML));
    assert!(
        has(
            &f,
            "[why].tolerance names the whole [tolerance] table, which is empty"
        ),
        "{f:?}"
    );
    let inputs = format!("{ACTIVE}[tolerance]\nx_max = 1.0\n\n[why]\ninputs = \"r\"\n");
    let f = why_findings(
        "case-why-inputs-dir",
        &inputs,
        Some("benchmarks/1_a/inputs/b.toml"),
    );
    assert!(f.is_empty(), "{f:?}");
    let f = why_findings(
        "case-why-crystals",
        &inputs,
        Some("benchmarks/1_a/crystals.json"),
    );
    assert!(f.is_empty(), "{f:?}");
    let f = why_findings("case-why-no-input", &inputs, None);
    assert!(
        has(
            &f,
            "[why].inputs names the case's inputs, but benchmarks/1_a holds no input.toml, inputs/<stem>.toml, or crystals.json"
        ),
        "{f:?}"
    );
    let f = why_findings(
        "case-why-inputs-ghost",
        &inputs,
        Some("benchmarks/1_a/inputs/ghost.toml/a.toml"),
    );
    assert!(
        has(
            &f,
            "[why].inputs names the case's inputs, but benchmarks/1_a holds no"
        ),
        "a directory named ghost.toml holds no input: {f:?}"
    );
    for name in ["inputs", "status"] {
        let relax = format!("{ACTIVE}[tolerance]\nx_max = 1.0\n\n[relax]\n{name} = \"ions\"\n");
        let f = why_findings(&format!("case-why-relax-{name}"), &relax, Some(INPUT_TOML));
        assert!(
            f.is_empty(),
            "a [relax] input without a reason passes: {f:?}"
        );
        let reason = format!("{relax}\n[why]\n{name} = \"r\"\n");
        let f = why_findings(
            &format!("case-why-relax-why-{name}"),
            &reason,
            Some(INPUT_TOML),
        );
        assert!(
            has(
                &f,
                &format!("[why].{name} names both [relax].{name} and another target")
            ),
            "{f:?}"
        );
    }
}

#[test]
fn the_contributing_example_of_why_names_values_of_its_case() {
    let contributing =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/../CONTRIBUTING.md"))
            .unwrap();
    for line in crate::WHY_EXAMPLE {
        assert!(contributing.lines().any(|l| l.trim() == line), "{line}");
    }
    let case = format!(
        "{}\n[why]\n{}\n",
        crate::harness::CASE,
        crate::WHY_EXAMPLE.join("\n")
    );
    let f = why_findings("case-why-contributing", &case, Some(INPUT_TOML));
    assert!(f.is_empty(), "{f:?}");
}
