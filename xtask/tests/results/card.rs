use super::{WHAT, cases, commit, read, s11};
use crate::{Tree, has, lever, write};
use std::{fs, path::Path};

const CLAIM: &str = "[case]\ntier = \"claim\"\nstatus = \"active\"\nclaims = [\"speed\", \"correctness\"]\nreference = \"analytic\"\nrivals = [\"qe\", \"abinit\"]\n\n[tolerance]\nf_error_max = 1e-9\n";
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
const RECORD: &str = "target/benchmarks/claim/5_e.json";

fn claim_cases(name: &str) -> Tree {
    let root = cases(name);
    write(&root, "benchmarks/5_e/case.toml", CLAIM);
    write(
        &root,
        "benchmarks/results-what.toml",
        &format!(
            "{WHAT}\n[5]\nwhat = \"F vs reference; speed vs rivals\"\nmetric = [\"f_error\"]\nunit = \"Ha/atom\"\n"
        ),
    );
    commit(&root, "claim case");
    root
}

type Cell = (&'static str, f64, f64, f64, f64, bool);

fn versus(code: &str, ci: [f64; 2], pairs: &[Cell]) -> String {
    let n = pairs.len() as f64;
    let g = (pairs.iter()).map(|p| (p.1 / p.2).ln()).sum::<f64>() / n;
    let read: Vec<&Cell> = pairs.iter().filter(|p| p.5).collect();
    let r = (read.iter()).map(|p| (p.3 / p.4).ln()).sum::<f64>() / read.len() as f64;
    let pairs: Vec<String> = (pairs.iter())
        .map(|(input, tr, to, mr, mo, in_r)| {
            format!(
                r#"{{"input":"{input}","threads":8,"medians_t_delta_s":[{tr:?},{to:?}],"medians_peak_pss_bytes":[{mr:?},{mo:?}],"t_ratio":{},"pss_ratio":{},"in_r":{in_r},"losses":0}}"#,
                tr / to,
                mr / mo
            )
        })
        .collect();
    format!(
        r#"{{"code":"{code}","r":{:?},"g":{:?},"g_ci":[{:?},{:?}],"g_min":1.0,"milestone":"m3","g_judged":true,"pairs":[{}]}}"#,
        r.exp(),
        g.exp(),
        ci[0],
        ci[1],
        pairs.join(",")
    )
}

fn series(code: &str, pss_exponent: f64, t_exponent: f64, points: &[(u32, f64, f64)]) -> String {
    let points: Vec<String> = (points.iter())
        .map(|(atoms, t, pss)| {
            format!(
                r#"{{"input":"si{atoms}","atoms":{atoms},"median_t_delta_s":{t:?},"median_peak_pss_bytes":{pss:?}}}"#
            )
        })
        .collect();
    format!(
        r#"{{"code":"{code}","t_delta_exponent":{t_exponent:?},"pss_exponent":{pss_exponent:?},"points":[{}]}}"#,
        points.join(",")
    )
}

const NODE: &str = r#"{"host":{"cpu":"Stand-in CPU","cores":8,"core_cpus":["0","1"],"memory_bytes":16762847232},"orbitar":{"build":"cargo build --release","profile_release":{"lto":"thin"}},"qe":{"qe":"7.5 h19104ac_2 sha256:00","libgfortran-ng":"14.2 h0 sha256:01","a b":"1 x sha256:02"},"abinit":{"abinit":"10.0.3 h2b8cb88_2 sha256:03"}}"#;

fn claim_record(commit: &str, dirty: bool, pilot: bool, outcome: &str) -> String {
    let qe = versus(
        "qe",
        [1.21, 1.8],
        &[
            ("a.toml", 45.0, 30.0, 2.4e9, 1.2e9, true),
            ("b.toml", 20.0, 16.0, 9e8, 6e8, true),
            ("c.toml", 20.0, 10.0, 6e8, 5e8, true),
        ],
    );
    let abinit = versus(
        "abinit",
        [1.55, 2.6],
        &[
            ("a.toml", 60.0, 30.0, 1.8e9, 1.2e9, true),
            ("b.toml", 24.0, 16.0, 6.6e8, 6e8, false),
            ("c.toml", 30.0, 10.0, 7e8, 5e8, true),
        ],
    );
    let series = [
        series(
            "orbitar",
            1.0,
            1.5,
            &[(2, 1.0, 1e8), (8, 8.0, 4e8), (32, 64.0, 1.6e9)],
        ),
        series("qe", 1.0, 2.0, &[(2, 2.0, 2e8), (8, 32.0, 8e8)]),
        series(
            "abinit",
            3f64.ln() / 4f64.ln(),
            1.5,
            &[(2, 3.0, 3e8), (8, 24.0, 9e8), (32, 192.0, 2.7e9)],
        ),
    ];
    format!(
        r#"{{"case":"5_e","tier":"claim","commit":"{commit}","dirty":{dirty},"pilot":{pilot},"engine_sha256":"{}","outcome":"{outcome}","metrics":{{"f_error":1e-10}},"tolerance":{{"f_error_max":1e-9}},"versus":[{qe},{abinit}],"series":[{}],"node":{NODE}}}"#,
        "cd".repeat(32),
        series.join(",")
    )
}

fn cards_of(root: &Path) -> Vec<String> {
    CARDS.iter().map(|p| read(root, p)).collect()
}

fn holds(card: &str, words: &[&str]) {
    for word in words {
        assert!(card.contains(word), "{word}: {card}");
    }
}

#[test]
fn a_passing_claim_record_gives_speed_memory_and_scaling_cards_that_show_its_numbers() {
    let root = claim_cases("cards");
    let head = commit(&root, "head");
    write(&root, RECORD, &claim_record(&head, false, false, "pass"));
    assert_eq!(lever(&["results", "collect"], &root), Vec::<String>::new());
    let results = read(&root, "benchmarks/RESULTS.toml");
    holds(
        &results,
        &[
            "[[case.versus]]\nrival = \"qe\"\ng = ",
            "[[case.versus.pair]]\ninput = \"b.toml\"\nthreads = 8\nt_delta_s_rival = 2.4e1\nt_delta_s_orbitar = 1.6e1\npeak_pss_bytes_rival = 6.6e8\npeak_pss_bytes_orbitar = 6e8\nin_r = false\n",
            "[[case.series]]\ncode = \"qe\"\nt_delta_exponent = 2e0\npss_exponent = 1e0\n",
            "[[case.series.point]]\ninput = \"si32\"\natoms = 32\nt_delta_s = 1.92e2\npeak_pss_bytes = 2.7e9\n",
            "[case.node.host]\ncore_cpus = [\"0\", \"1\"]\ncores = 8\ncpu = \"Stand-in CPU\"\nmemory_bytes = 16762847232\n",
            "[case.node.orbitar.profile_release]\nlto = \"thin\"\n",
            "[case.node.qe]\n\"a b\" = \"1 x sha256:02\"\nlibgfortran-ng = \"14.2 h0 sha256:01\"\n",
        ],
    );
    assert!(lever(&["results", "render"], &root).is_empty());
    let first = cards_of(&root);
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert_eq!(read(&root, "benchmarks/RESULTS.toml"), results);
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
            ">Time to the same accuracy</text>",
            ">≥ 1.21×</text>",
            ">faster than QE</text>",
            ">≥ 1.55×</text>",
            ">faster than ABINIT</text>",
            ">95% CI lower bound</text>",
            ">geometric mean over 3 cells</text>",
            ">one representative cell: a</text>",
            ">orbitaR</text>",
            ">QE 7.5</text>",
            ">ABINIT 10.0.3</text>",
            ">30.0 s</text>",
            ">45.0 s</text>",
            ">60.0 s</text>",
        ],
    );
    holds(
        memory,
        &[
            ">MEMORY</text>",
            ">1.53×</text>",
            ">less peak memory than QE</text>",
            ">geometric mean over 3 cells</text>",
            ">1.44×</text>",
            ">less peak memory than ABINIT</text>",
            ">geometric mean over 2 cells</text>",
            ">one representative cell: c</text>",
            ">500 MB</text>",
            ">600 MB</text>",
            ">700 MB</text>",
        ],
    );
    holds(
        scaling,
        &[
            ">SCALABILITY</text>",
            ">4.00×</text>",
            ">faster than QE at 8 atoms</text>",
            ">3.00×</text>",
            ">faster than ABINIT at 32 atoms</text>",
            ">N<tspan dy=\"-22\" font-size=\"30\">1.50</tspan></text>",
            ">QE α 2.00</text>",
            ">ABINIT α 1.50</text>",
            ">1 s</text>",
            ">1000 s</text>",
            ">32</text>",
        ],
    );
    assert_eq!(scaling.matches("<polyline").count(), 3, "{scaling}");
    assert_eq!(scaling.matches("<circle").count(), 8, "{scaling}");
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
    }
    for card in first.iter().filter(|c| !c.is_empty()) {
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
    write(&root, CARDS[2], &memory.replace("1.53×", "1.60×"));
    fs::remove_file(root.join(CARDS[5])).unwrap();
    let found = s11(&root);
    for path in [CARDS[2], CARDS[5]] {
        assert!(has(&found, &format!("S11 {path} differs")), "{found:?}");
    }
}

#[test]
fn a_pilot_dirty_failing_or_rivalless_claim_record_makes_no_card() {
    let root = claim_cases("no-card");
    let head = commit(&root, "head");
    let good = claim_record(&head, false, false, "pass");
    let abinit = good.find(r#",{"code":"abinit","r""#).unwrap();
    let series = good.find(r#"],"series""#).unwrap();
    let without_abinit = format!("{}{}", &good[..abinit], &good[series..]);
    for (record, why) in [
        (
            claim_record(&head, true, false, "pass"),
            "5_e ran on a dirty tree",
        ),
        (
            claim_record(&head, false, true, "pass"),
            "5_e is a pilot record",
        ),
        (
            without_abinit,
            "a passing claim record holds no versus entry for abinit, which the case names",
        ),
        (
            good.replace(r#""code":"abinit","r""#, r#""code":"vasp","r""#),
            "versus vasp: the case names rivals [\"qe\", \"abinit\"], each once",
        ),
        (
            good.replace(r#""code":"qe","t_delta"#, r#""code":"elk","t_delta"#),
            "series elk: the case names rivals",
        ),
    ] {
        write(&root, RECORD, &record);
        assert!(has(&lever(&["results", "collect"], &root), why), "{why}");
        assert!(!root.join("benchmarks/RESULTS.toml").exists());
    }
    write(&root, RECORD, &claim_record(&head, false, false, "fail"));
    assert!(lever(&["results", "collect"], &root).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    assert!(
        results.contains("id = \"5_e\"")
            && !results.contains("versus")
            && !results.contains("series")
            && !results.contains("[case.node"),
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
    write(&root, CARDS[7], "<svg/>");
    assert!(has(
        &s11(&root),
        &format!("S11 {} exists without benchmarks/RESULTS.toml", CARDS[7])
    ));
}

#[test]
fn a_versus_or_series_entry_that_its_own_numbers_or_its_node_contradict_is_refused() {
    let root = claim_cases("versus");
    let head = commit(&root, "head");
    let good = claim_record(&head, false, false, "pass");
    for (record, why) in [
        (
            good.replace(r#""qe":"7.5 h19104ac_2 sha256:00","#, ""),
            "names no version and build of qe",
        ),
        (
            good.replace(r#""cpu":"Stand-in CPU","#, ""),
            "the node block names no CPU: [host].cpu",
        ),
        (
            good.replacen("[45.0,30.0]", "[0.0,30.0]", 1),
            "versus qe: each median lies above 0",
        ),
    ] {
        write(&root, RECORD, &record);
        assert!(has(&lever(&["results", "collect"], &root), why), "{why}");
    }
    write(&root, RECORD, &good);
    assert!(lever(&["results", "collect"], &root).is_empty());
    let results = read(&root, "benchmarks/RESULTS.toml");
    let g = (results.lines())
        .find(|l| l.starts_with("g = "))
        .unwrap()
        .to_owned();
    for (from, to, why) in [
        (
            g.as_str(),
            "g = 1.4e0",
            "versus qe: G is 1.4e0, but its pairs give",
        ),
        (
            "g_ci = [1.21e0, 1.8e0]",
            "g_ci = [1.6e0, 1.8e0]",
            "versus qe: G 1.55",
        ),
        (
            "in_r = true\n\n[[case.versus]]",
            "in_r = false\n\n[[case.versus]]",
            "versus qe: R is",
        ),
        (
            "t_delta_exponent = 2e0",
            "t_delta_exponent = 2.1e0",
            "series qe: the T(δ) exponent is 2.1e0, but its points give",
        ),
        (
            "atoms = 8\nt_delta_s = 3.2e1",
            "atoms = 1\nt_delta_s = 3.2e1",
            "series qe: points go by atoms, then input, once each",
        ),
        (
            "code = \"abinit\"\nt_delta",
            "code = \"qe\"\nt_delta",
            "a series names each code once",
        ),
        ("\n[case.node]\n", "\n[case.other]\n", "unknown field"),
        (
            "outcome = \"pass\"\ncommit",
            "outcome = \"fail\"\ncommit",
            "only a passing claim record gives versus and series entries",
        ),
    ] {
        assert!(results.contains(from), "{from}");
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

const CHECKS: [(&str, &str, &str, &str); 6] = [
    (
        "6_f",
        "free_energy_error_ha_max = 5e-10",
        "free_energy_error",
        "1.7e-10",
    ),
    (
        "9_i",
        "term_difference_ha_max = 0.0\nfermi_level_difference_ha_max = 0.0",
        "",
        "0.0",
    ),
    (
        "13_m",
        "force_difference_ha_bohr_max = 1e-6",
        "force_difference_ha_bohr",
        "2.4e-7",
    ),
    (
        "15_o",
        "stress_difference_ha_bohr3_max = 1e-8",
        "stress_difference_ha_bohr3",
        "3.1e-9",
    ),
    (
        "20_t",
        "free_energy_error_ha_max = 5e-10",
        "free_energy_error",
        "2.0e-10",
    ),
    (
        "22_v",
        "stress_difference_ha_bohr3_max = 2e-9",
        "stress_difference_ha_bohr3",
        "1e-10",
    ),
];

fn check_record(case: &str, commit: &str, measured: &str, outcome: &str) -> String {
    let (_, tolerance, _, _) = CHECKS.iter().find(|c| c.0 == case).unwrap();
    let pairs: Vec<(&str, &str)> = (tolerance.lines())
        .map(|l| l.split_once(" = ").unwrap())
        .collect();
    let tolerance: Vec<String> = pairs.iter().map(|(k, v)| format!("\"{k}\":{v}")).collect();
    let metrics: Vec<String> = (pairs.iter())
        .map(|(k, _)| format!("\"{}\":{measured}", k.strip_suffix("_max").unwrap()))
        .collect();
    format!(
        r#"{{"case":"{case}","tier":"push","commit":"{commit}","dirty":false,"pilot":false,"engine_sha256":"{}","outcome":"{outcome}","metrics":{{{}}},"tolerance":{{{}}}}}"#,
        "ef".repeat(32),
        metrics.join(","),
        tolerance.join(",")
    )
}

#[test]
fn the_verification_card_shows_each_active_check_against_its_bound_and_only_when_every_one_passes()
{
    let root = cases("verification");
    let mut what = WHAT.to_owned();
    for (id, tolerance, _, _) in CHECKS {
        write(
            &root,
            &format!("benchmarks/{id}/case.toml"),
            &format!(
                "[case]\ntier = \"push\"\nstatus = \"active\"\nclaims = [\"correctness\"]\nreference = \"analytic\"\n\n[tolerance]\n{tolerance}\n"
            ),
        );
        let (number, _) = id.split_once('_').unwrap();
        let metrics: Vec<String> = (tolerance.lines())
            .map(|l| format!("\"{}\"", l.split_once("_max").unwrap().0))
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
    for (id, _, _, measured) in CHECKS {
        write(
            &root,
            &format!("target/benchmarks/push/{id}.json"),
            &check_record(id, &head, measured, "pass"),
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
            ">worst 31% of bound</text>",
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
        &check_record("15_o", &head, "2e-8", "fail"),
    );
    assert!(lever(&["results", "collect"], &root).is_empty());
    assert!(has(
        &s11(&root),
        "card-verification-light.svg exists, but no row"
    ));
    assert!(lever(&["results", "render"], &root).is_empty());
    assert!(cards_of(&root).iter().all(String::is_empty));
}
