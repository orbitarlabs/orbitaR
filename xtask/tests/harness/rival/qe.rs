use super::*;

#[test]
fn the_valence_comes_from_a_upf_v1_header_line_or_a_upf_v2_attribute() {
    for (n, (upf, moment)) in [(NI_UPF_V1, "9.0"), (O_UPF_V2, "3.0")]
        .into_iter()
        .enumerate()
    {
        let input = format!("{INPUT}\n[spin]\nmoments = [{moment}]\n");
        let log =
            std::env::temp_dir().join(format!("xtask-rival-upf-{n}-{}.in", std::process::id()));
        let _ = std::fs::remove_file(&log);
        let pw = format!("[ -e '{0}' ] || cp pw.in '{0}'\n{PW}", log.display());
        let root = rivals_tree(&format!("rival-upf-{n}"), &input, TUNED, &pw, ABINIT);
        repin(&root, "rivals/SSSP/Si.UPF", upf);
        let jth = format!(
            "<atom Z=\"14.00\" valence=\"{}\"/>",
            2.0 * moment.parse::<f64>().unwrap()
        );
        repin(&root, "target/datasets/JTH-PBE-v2.0/Si/Si.xml", &jth);
        let (outcome, record) = claim(&root, true);
        let seen = std::fs::read_to_string(&log).unwrap_or_default();
        let _ = std::fs::remove_file(&log);
        assert!(passed(&outcome), "{outcome:?}: {record}");
        assert!(
            seen.contains("  starting_magnetization(1) = 0.5\n"),
            "{seen}"
        );
    }
}

#[test]
fn a_qe_dataset_with_other_valence_electrons_than_jths_loses_each_run() {
    let root = rivals_tree("rival-valence", INPUT, TUNED, PW, ABINIT);
    repin(
        &root,
        "rivals/SSSP/Si.UPF",
        "<PP_HEADER z_valence=\"2.0\"/>",
    );
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for b in blocks(&record, "qe") {
        assert!(b.get("median_t_delta_s").is_none(), "{b}");
        let loss = b["warmup"]["loss"].as_str().unwrap();
        assert!(
            loss.contains(
                "the cell holds 2 valence electrons in qe's datasets and 4 in JTH's: \
                 Si 2 in Si.UPF, 4 in JTH-PBE-v2.0/Si/Si.xml"
            ),
            "{loss}"
        );
    }
    assert!(
        blocks(&record, "abinit")
            .iter()
            .all(|b| b["median_t_delta_s"].is_f64()),
        "{record}"
    );
}

#[test]
fn qe_clocks_of_minutes_and_hours_read_in_seconds() {
    for (clock, agrees) in [("0.10s", true), (" 1m 5.00s", false), (" 1h 2m", false)] {
        let pw = PW.replace("${t}s WALL", &format!("{clock} WALL"));
        let root = rivals_tree("rival-clock", INPUT, TUNED, &pw, ABINIT);
        let (_, record) = claim(&root, true);
        let b = blocks(&record, "qe")[0];
        let losses = b["losses"].as_array().unwrap();
        assert_eq!(losses.is_empty(), agrees, "{clock}: {b}");
        if !agrees {
            let read = if clock.contains('h') {
                "3720.000"
            } else {
                "65.000"
            };
            assert!(
                losses[0]
                    .as_str()
                    .unwrap()
                    .contains(&format!("clock reads {read} s")),
                "{b}"
            );
        }
    }
}

#[test]
fn a_qe_file_that_differs_from_its_pin_stops_the_claim() {
    let root = rivals_tree("rival-pin", INPUT, TUNED, PW, ABINIT);
    write(&root, "rivals/SSSP/Si.UPF", "changed");
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("harness: ") && why.contains("Si.UPF: sha256 ") && why.contains(", but benchmarks/DATASETS.toml pins ")),
        "{outcome:?}"
    );
}

#[test]
fn a_dataset_that_gives_no_valence_stops_the_claim_by_its_file() {
    for (path, bytes, why) in [
        (
            "rivals/SSSP/Si.UPF",
            "<PP_HEADER element=\"Si\"/>",
            "rivals: Si.UPF: qe reads no valence in it",
        ),
        (
            "target/datasets/JTH-PBE-v2.0/Si/Si.xml",
            "<atom Z=\"14.00\"/>",
            "rivals: JTH-PBE-v2.0/Si/Si.xml: no <atom valence>",
        ),
    ] {
        let root = rivals_tree("rival-no-valence", INPUT, TUNED, PW, ABINIT);
        repin(&root, path, bytes);
        let (outcome, _) = claim(&root, true);
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(why)),
            "{why}: {outcome:?}"
        );
    }
}
