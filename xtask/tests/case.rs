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
