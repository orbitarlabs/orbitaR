use super::claim::{Cell, RECORD, cells, claim_cases, claim_record, collected};
use super::{WHAT, cases, commit, read, s11};
use crate::{has, lever, write};
use std::{fs, path::Path};

const CARDS: [&str; 8] = [
    "assets/benchmarks/card-speed-light.svg",
    "assets/benchmarks/card-speed-dark.svg",
    "assets/benchmarks/card-memory-light.svg",
    "assets/benchmarks/card-memory-dark.svg",
    "assets/benchmarks/card-scaling-light.svg",
    "assets/benchmarks/card-scaling-dark.svg",
    "assets/benchmarks/card-verification-light.svg",
    "assets/benchmarks/card-verification-dark.svg",
];

fn cards_of(root: &Path) -> Vec<String> {
    CARDS.iter().map(|p| read(root, p)).collect()
}

fn holds(card: &str, words: &[&str]) {
    for word in words {
        assert!(card.contains(word), "{word}: {card}");
    }
}

fn rendered(name: &str, cells: &[Cell]) -> Vec<String> {
    let root = claim_cases(name, cells);
    let head = commit(&root, "head");
    let found = collected(&root, &claim_record(&head, cells));
    assert_eq!(found, Vec::<String>::new());
    assert!(lever(&["results", "render"], &root).is_empty());
    cards_of(&root)
}

#[test]
fn a_passing_claim_record_gives_speed_memory_and_scaling_cards_that_show_its_numbers() {
    let root = claim_cases("cards", &cells());
    let head = commit(&root, "head");
    assert!(collected(&root, &claim_record(&head, &cells())).is_empty());
    assert!(lever(&["results", "render"], &root).is_empty());
    let first = cards_of(&root);
    assert!(lever(&["results", "render"], &root).is_empty());
    assert_eq!(cards_of(&root), first);
    assert_eq!(s11(&root), Vec::<String>::new());
    let [
        speed,
        speed_dark,
        memory,
        memory_dark,
        scaling,
        scaling_dark,
        verification,
        _,
    ] = &first[..]
    else {
        panic!("eight cards")
    };
    assert!(
        verification.is_empty(),
        "this tree has no verification case"
    );
    holds(
        speed,
        &[
            ">SPEED</text>",
            ">≥ 4.00×</text>",
            ">faster than QE</text>",
            ">≥ 3.00×</text>",
            ">faster than ABINIT</text>",
            ">95% CI lower bound</text>",
            ">geometric mean over 3 of 4 cells</text>",
            ">geometric mean over 4 of 4 cells</text>",
            ">one representative cell: b</text>",
            ">of the cells that every rival ran, the one whose ratios lie nearest each G; ties go to the first by name</text>",
            ">8.00 s</text>",
            ">32.0 s</text>",
            ">24.0 s</text>",
            ">QE 7.5</text>",
            ">ABINIT 10.0.3</text>",
            ">not timed: QE c (32 atoms), skipped: no dataset converges it</text>",
        ],
    );
    holds(
        memory,
        &[
            ">MEMORY</text>",
            ">2.00×</text>",
            ">less peak memory than QE</text>",
            ">geometric mean over 2 of 4 cells</text>",
            ">1.85×</text>",
            ">geometric mean over 3 of 4 cells</text>",
            ">one representative cell: d</text>",
            "nearest each R;",
            ">6.40 GB</text>",
            ">12.80 GB</text>",
            ">10.80 GB</text>",
            ">not in R against QE: input, its runs are too short for a peak Pss reading</text>",
            ">not in R against ABINIT: input, its runs are too short for a peak Pss reading</text>",
        ],
    );
    holds(
        scaling,
        &[
            ">SCALABILITY</text>",
            ">8.00×</text>",
            ">faster than QE at 128 atoms</text>",
            ">3.00×</text>",
            ">faster than ABINIT at 128 atoms</text>",
            ">median T(δ) of d</text>",
            ">N<tspan dy=\"-22\" font-size=\"30\">1.85</tspan></text>",
            ">least squares over 2–128 atoms</text>",
            ">QE α 2.15 over 2–128 atoms</text>",
            ">ABINIT α 1.85 over 2–128 atoms</text>",
            ">0.1 s</text>",
            ">10000 s</text>",
            ">not timed: QE c (32 atoms), skipped: no dataset converges it</text>",
        ],
    );
    assert_eq!(scaling.matches("<polyline").count(), 3, "{scaling}");
    assert_eq!(scaling.matches("<circle").count(), 11, "{scaling}");
    for card in [speed, memory, scaling] {
        holds(
            card,
            &[
                ">8 cores, one node, same accuracy target · Stand-in CPU</text>",
                &format!(
                    ">case 5 · orbitaR at commit {} · QE 7.5 (h19104ac_2) · ABINIT 10.0.3 (h2b8cb88_2)</text>",
                    &head[..7]
                ),
            ],
        );
        assert!(
            card.starts_with("<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"960.0\""),
            "{card}"
        );
        assert!(
            !card.contains("href") && !card.contains("url(") && !card.contains("@import"),
            "{card}"
        );
    }
    for (light, dark) in [
        (speed, speed_dark),
        (memory, memory_dark),
        (scaling, scaling_dark),
    ] {
        holds(light, &["fill=\"#f6f4ef\"", "fill=\"#bf6038\""]);
        holds(dark, &["fill=\"#101112\"", "fill=\"#eb965f\""]);
        assert!(!light.contains("#eb965f") && !dark.contains("#bf6038"));
    }
    write(&root, CARDS[2], &memory.replace("2.00×", "2.10×"));
    fs::remove_file(root.join(CARDS[5])).unwrap();
    let found = s11(&root);
    for path in [CARDS[2], CARDS[5]] {
        assert!(has(&found, &format!("S11 {path} differs")), "{found:?}");
    }
}

#[test]
fn a_rival_without_a_point_at_the_largest_size_has_no_ratio_there_and_the_card_says_why() {
    let mut skipped = cells();
    skipped[3].runs[1] = None;
    let cards = rendered("cards-withheld", &skipped);
    holds(
        &cards[4],
        &[
            ">none</text>",
            ">no QE ratio at 128 atoms</text>",
            ">d skipped</text>",
            ">faster than ABINIT at 128 atoms</text>",
            ">not timed: QE d (128 atoms), skipped: no dataset converges it</text>",
        ],
    );
    assert!(!cards[4].contains("faster than QE at"), "{}", cards[4]);
}

#[test]
fn a_ratio_that_shows_no_advantage_is_worded_as_a_plain_quotient() {
    let mut slower = cells();
    slower[3].runs[1] = Some((256.0, 12_800_000_000));
    let cards = rendered("cards-neutral", &slower);
    holds(
        &cards[4],
        &[
            ">0.50×</text>",
            ">QE time ÷ orbitaR time at 128 atoms</text>",
        ],
    );
    assert!(!cards[4].contains("faster than QE"), "{}", cards[4]);
}

#[test]
fn a_bound_just_below_a_hundredth_is_cut_and_never_rounded_up() {
    let mut close = cells();
    for cell in &mut close {
        let (t, pss) = cell.runs[0].unwrap();
        cell.runs[2] = Some((t * 1.229_999_999_9, pss * 2));
    }
    let cards = rendered("cards-cut", &close);
    holds(
        &cards[0],
        &[">≥ 1.22×</text>", ">faster than ABINIT</text>"],
    );
    assert!(!cards[0].contains("1.23×"), "{}", cards[0]);
}

#[test]
fn a_failing_claim_record_makes_no_card_and_render_removes_a_stray_one() {
    let root = claim_cases("no-card", &cells());
    let head = commit(&root, "head");
    let mut failing = claim_record(&head, &cells());
    failing["outcome"] = "fail".into();
    failing["failures"] = serde_json::json!(["G against qe is below its bar"]);
    assert!(collected(&root, &failing).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert!(
        results.contains("id = \"5_e\"") && !results.contains("[case.claim"),
        "{results}"
    );
    assert!(lever(&["results", "render"], &root).is_empty());
    assert!(CARDS.iter().all(|p| !root.join(p).exists()));
    write(&root, CARDS[0], "<svg/>");
    assert_eq!(
        s11(&root),
        [format!(
            "science: S11 {} exists, but no row of benchmarks/RESULTS.toml backs it; `cargo xtask results render` removes it",
            CARDS[0]
        )]
    );
    assert!(lever(&["results", "render"], &root).is_empty());
    assert!(!root.join(CARDS[0]).exists());
    fs::remove_file(root.join("benchmarks/RESULTS.toml")).unwrap();
    fs::remove_file(root.join(RECORD)).unwrap();
    write(&root, CARDS[7], "<svg/>");
    assert!(has(
        &s11(&root),
        &format!("S11 {} exists without benchmarks/RESULTS.toml", CARDS[7])
    ));
}

const CHECKS: [(&str, &str, &str); 6] = [
    (
        "6_f",
        "free_energy_error_ha_max = 5e-10",
        r#""free_energy_error_ha":1.7e-10"#,
    ),
    (
        "9_i",
        "term_difference_ha_max = 0.0\nfermi_level_difference_ha_max = 0.0",
        r#""term_difference_ha":0.0,"fermi_level_difference_ha":0.0"#,
    ),
    (
        "13_m",
        "gaas_force_difference_ha_bohr_max = 1e-6",
        r#""gaas_force_difference_ha_bohr":2.4e-7"#,
    ),
    (
        "15_o",
        "gaas_stress_difference_ha_bohr3_max = 1.0\nafm_order_share_min = 0.5",
        r#""gaas_stress_difference_ha_bohr3":0.375,"afm_order_share":0.6"#,
    ),
    (
        "20_t",
        "free_energy_error_ha_max = 5e-10",
        r#""free_energy_error_ha":2.0e-10"#,
    ),
    (
        "22_v",
        "fe_stress_difference_ha_bohr3_max = 2e-9",
        r#""fe_stress_difference_ha_bohr3":1e-10"#,
    ),
];

fn check_record(id: &str, commit: &str, metrics: &str, outcome: &str) -> String {
    let (_, tolerance, _) = CHECKS.iter().find(|c| c.0 == id).unwrap();
    let tolerance: Vec<String> = (tolerance.lines())
        .map(|l| l.split_once(" = ").unwrap())
        .map(|(k, v)| format!("\"{k}\":{v}"))
        .collect();
    format!(
        r#"{{"schema":4,"failures":[],"filter":null,"case":"{id}","tier":"push","commit":"{commit}","dirty":false,"pilot":false,"engine_sha256":"{}","outcome":"{outcome}","metrics":{{{metrics}}},"tolerance":{{{}}}}}"#,
        "ef".repeat(32),
        tolerance.join(",")
    )
}

#[test]
fn the_verification_card_shows_each_active_check_against_its_bound_and_only_when_every_one_passes()
{
    let root = cases("verification");
    let mut what = WHAT.to_owned();
    for (id, tolerance, _) in CHECKS {
        write(
            &root,
            &format!("benchmarks/{id}/case.toml"),
            &format!(
                "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\n{tolerance}\n"
            ),
        );
        let (number, _) = id.split_once('_').unwrap();
        let metrics: Vec<String> = (tolerance.lines())
            .map(|l| l.split_once(" = ").unwrap().0)
            .map(|k| format!("\"{}\"", &k[..k.len() - 4]))
            .collect();
        let (phrase, unit) = match number {
            "9" => ("1 vs 4 threads, bit for bit", ""),
            "13" => ("forces vs finite differences", "Ha/Bohr"),
            "15" | "22" => ("stress vs finite differences", "Ha/Bohr³"),
            _ => ("F vs ABINIT", "Ha"),
        };
        what.push_str(&format!(
            "\n[{number}]\nwhat = \"{phrase}\"\nmetric = [{}]\nunit = \"{unit}\"\n",
            metrics.join(", ")
        ));
    }
    write(&root, "benchmarks/results-what.toml", &what);
    let head = commit(&root, "checks");
    for (id, _, metrics) in CHECKS {
        write(
            &root,
            &format!("target/benchmarks/push/{id}.json"),
            &check_record(id, &head, metrics, "pass"),
        );
    }
    assert_eq!(lever(&["results", "collect"], &root), Vec::<String>::new());
    assert!(lever(&["results", "render"], &root).is_empty());
    let cards = cards_of(&root);
    assert!(
        cards[..6].iter().all(String::is_empty),
        "no claim row, no claim card"
    );
    let (light, dark) = (&cards[6], &cards[7]);
    holds(
        light,
        &[
            ">VERIFICATION</text>",
            ">1.7 × 10⁻¹⁰ Ha</text>",
            ">case 6</text>",
            ">bound 5.0 × 10⁻¹⁰</text>",
            ">2.0 × 10⁻¹⁰ Ha</text>",
            ">case 20</text>",
            ">free energy vs ABINIT</text>",
            ">r2SCAN, same datasets</text>",
            ">dF/dx ✓</text>",
            ">cases 13, 15, 22</text>",
            ">worst ≤ 38% of its bound</text>",
            ">0 bits</text>",
            ">1 vs 4 threads</text>",
            ">bit for bit</text>",
            ">case 9</text>",
            ">2 quantities, each 0</text>",
            &format!(
                ">orbitaR at commit {} · code verification, separate from accuracy</text>",
                &head[..7]
            ),
        ],
    );
    holds(dark, &["fill=\"#101112\"", ">dF/dx ✓</text>"]);
    assert_eq!(s11(&root), Vec::<String>::new());
    write(
        &root,
        "target/benchmarks/push/15_o.json",
        &check_record(
            "15_o",
            &head,
            r#""gaas_stress_difference_ha_bohr3":2.0,"afm_order_share":0.6"#,
            "fail",
        ),
    );
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert!(has(
        &s11(&root),
        "card-verification-light.svg exists, but no row"
    ));
    assert!(lever(&["results", "render"], &root).is_empty());
    assert!(cards_of(&root).iter().all(String::is_empty));
}
