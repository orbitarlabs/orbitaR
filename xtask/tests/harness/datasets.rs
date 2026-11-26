use super::{CASE, engine, honest, jth};
use crate::{Tree, tree, write};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use xtask::harness::{Outcome, Session, Tier, agree};

const SILICON: &str = "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n";
const READS_DATASET: &str = "f=$(sed -n 's/^Si = \"\\(.*\\)\"$/\\1/p' input.toml)\n\
    e=$(cat \"datasets/${f:-JTH-PBE-v2.0/Si/Si.xml}\")\nprintf '{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1]}' \"$e\"";

fn silicon(name: &str, remote: &str, pinned: &str) -> Tree {
    let input = super::INPUT.replace("[basis]", &format!("{SILICON}\n[basis]"));
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", &input),
        ],
    );
    write(&root, "remote/JTH-PBE-v2.0/Si/Si.xml", remote);
    let table = jth(&remote_url(&root), &[("pbe", "Si", pinned, None)]);
    write(&root, "datasets/jth-2.0.toml", &table);
    root
}

fn remote_url(root: &Tree) -> String {
    format!("file://{}/remote", root.display())
}

fn evaluate(root: &Tree) -> Outcome {
    let bin = engine(root, READS_DATASET);
    Session::new(root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest)
}

#[test]
fn the_sandbox_holds_the_pinned_file_of_each_species() {
    let root = silicon("datasets-pass", "0.5", "0.5");
    assert_eq!(evaluate(&root), Outcome::Pass);
    let cached =
        std::fs::read_to_string(root.join("target/datasets/JTH-PBE-v2.0/Si/Si.xml")).unwrap();
    assert_eq!(cached, "0.5");
}

#[test]
fn every_element_of_a_mixed_structure_has_its_pinned_file_in_the_sandbox() {
    let structure = "species = [\"Si\", \"Al\", \"Si\"]\n\
        positions_fractional = [[0.0, 0.0, 0.0], [0.25, 0.25, 0.25], [0.5, 0.5, 0.5]]\n";
    let input = super::INPUT.replace("[basis]", &format!("{structure}\n[basis]"));
    let root = tree(
        "datasets-two",
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", &input),
        ],
    );
    write(&root, "remote/JTH-PBE-v2.0/Si/Si.xml", "0.5");
    write(&root, "remote/JTH-PBE-v2.0/Al/Al.xml", "0.25");
    let rows = [("pbe", "Al", "0.25", None), ("pbe", "Si", "0.5", None)];
    write(
        &root,
        "datasets/jth-2.0.toml",
        &jth(&remote_url(&root), &rows),
    );
    let reads_both = format!("test -f datasets/JTH-PBE-v2.0/Al/Al.xml || exit 3\n{READS_DATASET}");
    let bin = engine(&root, &reads_both);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert_eq!(outcome, Outcome::Pass);
}

fn paw_reads(evidence: &xtask::harness::Evidence) -> Result<xtask::harness::Metrics, String> {
    let (input, output) = (evidence.input("input")?, evidence.output("input")?);
    let read = (input.species()?, input.probe_rho_ij()?);
    let want = (
        vec!["Si".to_owned()],
        ("Si".to_owned(), vec![vec![vec![1.0, 0.5], vec![0.5, 2.0]]]),
    );
    let fields = (output.paw_xc_energy_ha()?, output.paw_xc_d_ha()?);
    let error = (read != want
        || fields != ([-2.5, -0.75], vec![vec![vec![0.5, 0.25], vec![0.25, 1.0]]]))
        as u8;
    Ok(xtask::harness::Metrics::from([(
        "error".to_owned(),
        f64::from(error),
    )]))
}

#[test]
fn a_paw_probes_element_has_its_pinned_file_in_the_sandbox_and_its_fields_read() {
    let probe = "\n[probe]\nterm = \"paw\"\n\n[probe.rho_ij]\nSi = [[[1.0, 0.5], [0.5, 2]]]\n";
    let root = silicon("datasets-paw", "0.5", "0.5");
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &format!("{}{probe}", super::INPUT),
    );
    let sha = crate::harness::hex(&Sha256::digest("0.5"));
    let paw = format!(
        "test -f datasets/JTH-PBE-v2.0/Si/Si.xml || exit 3\n\
        printf '{{\"paw_xc_energy_ha\": [-2.5, -0.75], \"paw_xc_d_ha\": [[[0.5, 0.25], [0.25, 1]]], \
        \"datasets\": {{\"Si\": {{\"path\": \"JTH-PBE-v2.0/Si/Si.xml\", \"sha256\": \"{sha}\"}}}}}}'"
    );
    let bin = engine(&root, &paw);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", paw_reads);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn a_stale_cached_copy_is_fetched_again() {
    let root = silicon("datasets-stale", "0.5", "0.5");
    write(&root, "target/datasets/JTH-PBE-v2.0/Si/Si.xml", "9.5");
    assert_eq!(evaluate(&root), Outcome::Pass);
}

#[test]
fn a_file_that_differs_from_its_pin_is_a_harness_fault() {
    let root = silicon("datasets-sha", "0.5", "0.6");
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("input: harness: file://") && why.contains("but datasets/jth-2.0.toml pins")),
        "{outcome:?}"
    );
    assert!(!root.join("target/datasets/JTH-PBE-v2.0/Si/Si.xml").exists());
}

#[test]
fn a_species_without_a_pinned_file_is_a_harness_fault() {
    let root = silicon("datasets-missing", "0.5", "0.5");
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &input.replace("[\"Si\"]", "[\"Ge\"]"),
    );
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("input: harness: datasets/jth-2.0.toml holds no file for Ge")),
        "{outcome:?}"
    );
}

#[test]
fn every_run_gets_the_checked_bytes_even_if_the_cache_changes_between_runs() {
    let root = silicon("datasets-cache", "0.5", "0.5");
    let cache = root.join("target/datasets/JTH-PBE-v2.0/Si/Si.xml");
    let body = format!(
        "e=$(cat datasets/JTH-PBE-v2.0/Si/Si.xml)\nprintf 9.9 > {}\nprintf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1]}}' \"$e\"",
        cache.display()
    );
    let bin = engine(&root, &body);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert_eq!(outcome, Outcome::Pass);
}

#[test]
fn a_family_path_that_leaves_the_dataset_directory_is_a_harness_fault() {
    for layout in ["../{element}.xml", "/abs/{element}.xml", "a//{element}.xml"] {
        let root = silicon("datasets-pin-path", "0.5", "0.5");
        let table = std::fs::read_to_string(root.join("datasets/jth-2.0.toml")).unwrap();
        let moved = table.replace("JTH-PBE-v2.0/{element}/{element}.xml", layout);
        write(&root, "datasets/jth-2.0.toml", &moved);
        let outcome = evaluate(&root);
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("datasets/jth-2.0.toml: Si: ")
                && why.contains("is not a relative path of plain names")),
            "{layout}: {outcome:?}"
        );
    }
}

#[test]
fn species_that_are_not_symbols_are_a_harness_fault() {
    let root = silicon("datasets-species", "0.5", "0.5");
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &input.replace("[\"Si\"]", "[14]"),
    );
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("structure.species must be a list of element symbols")),
        "{outcome:?}"
    );
}

#[test]
fn jth_2_0_pins_one_pbe_file_for_each_acwf_element_that_jth_covers() {
    let repo = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let read = |path: &str| std::fs::read_to_string(repo.join(path)).unwrap();
    let family: toml::Table = read("datasets/jth-2.0.toml").parse().unwrap();
    assert_eq!(
        family["pbe"]["layout"].as_str(),
        Some("JTH-PBE-v2.0/{element}/{element}.GGA_PBE-JTH.xml")
    );
    let files: Vec<&String> = family["pbe"]["element"]
        .as_table()
        .unwrap()
        .keys()
        .collect();
    assert_eq!(files.len(), 86);
    let acwf: serde_json::Value = serde_json::from_str(&read(
        "benchmarks/10_acwf_subset/reference/acwf-ae-average.json",
    ))
    .unwrap();
    let absent = ["Fr", "Ra", "Ac", "Th", "Pa", "U", "Np", "Pu", "Am", "Cm"];
    let covered: BTreeSet<&str> = (acwf["BM_fit_data"].as_object().unwrap().keys())
        .map(|system| system.split_once('-').unwrap().0)
        .filter(|element| !absent.contains(element))
        .collect();
    let pinned: BTreeSet<&str> = files.iter().map(|element| element.as_str()).collect();
    assert_eq!(pinned, covered);
}

fn tabled(name: &str, table: &str) -> Tree {
    let root = silicon(name, "0.25", "0.25");
    let hinted = jth(&remote_url(&root), &[("pbe", "Si", "0.25", Some(10.0))]);
    write(&root, "datasets/jth-2.0.toml", &hinted);
    write(&root, "benchmarks/1_a/table.toml", table);
    let split = concat!(env!("CARGO_MANIFEST_DIR"), "/../benchmarks/ACWF.toml");
    write(
        &root,
        "benchmarks/ACWF.toml",
        &std::fs::read_to_string(split).unwrap(),
    );
    write(&root, "benchmarks/1_a/si.xml", "0.5");
    write(&root, "candidate/si.xml", "0.5");
    root
}

const SI_FILE: &str =
    "[element]\nSi = { file = \"si.xml\", sha256 = \"SHA\", cutoff_hint_ha = 5.0 }\n";

fn half() -> String {
    crate::harness::hex(&Sha256::digest("0.5"))
}

#[test]
fn a_table_entry_jth_2_0_runs_the_pin_and_a_candidate_runs_its_file_by_datasets_files() {
    let root = tabled("table-jth", "[element]\nSi = \"jth-2.0\"\n");
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("error = 2.5e-1")),
        "{outcome:?}"
    );
    let candidate = root.join("candidate/table.toml");
    write(
        &root,
        "candidate/table.toml",
        &SI_FILE.replace("SHA", &half()),
    );
    let bin = engine(&root, READS_DATASET);
    let session =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).with_table(&candidate);
    assert_eq!(session.evaluate("1_a", honest), Outcome::Shard);
    let record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap())
            .unwrap();
    let sha = crate::harness::hex(&Sha256::digest(SI_FILE.replace("SHA", &half())));
    assert_eq!(
        record["table"],
        serde_json::json!({ "path": candidate.display().to_string(), "sha256": sha })
    );
}

#[test]
fn a_candidate_file_that_differs_from_its_sha256_is_a_harness_fault() {
    let root = tabled("table-sha", &SI_FILE.replace("SHA", "0"));
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("1_a/si.xml: sha256") && why.contains("but") && why.contains("1_a/table.toml gives 0")),
        "{outcome:?}"
    );
}

#[test]
fn a_candidate_table_for_a_case_without_a_table_is_a_harness_fault() {
    let root = silicon("table-none", "0.5", "0.5");
    let bin = engine(&root, READS_DATASET);
    let session = Session::new(&root, &bin, Tier::Push, &root.join("records"))
        .with_table(&root.join("t.toml"));
    let outcome = session.evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("t.toml: the case has no table.toml, so it takes no candidate table")),
        "{outcome:?}"
    );
}

#[test]
fn a_table_entry_of_another_family_without_a_pin_or_with_no_positive_hint_fails_by_name() {
    for (table, why) in [
        (
            "[element]\nSi = \"jth-1.1\"\n",
            "Si: \"jth-1.1\" is not \"jth-2.0\", the family of the ACWF inputs; give a file",
        ),
        (
            "[element]\nGe = \"jth-2.0\"\n",
            "Ge: datasets/jth-2.0.toml holds no pbe file of it",
        ),
        ("[elements]\nSi = \"jth-2.0\"\n", "unknown field `elements`"),
        (
            "[element]\nSi = { file = \"si.xml\", sha256 = \"0\", cutoff_hint_ha = 0.0 }\n",
            "Si: cutoff_hint_ha 0 is not above 0",
        ),
        (
            "[element]\nSi = { file = \"si.xml\", sha256 = \"0\", cutoff_hint_ha = nan }\n",
            "Si: cutoff_hint_ha NaN is not above 0",
        ),
        (
            "[element]\nSi = { file = \"../si.xml\", sha256 = \"0\", cutoff_hint_ha = 5.0 }\n",
            "Si: file \"../si.xml\" must be a relative path of plain names outside the directory of a family",
        ),
        (
            "[element]\nSi = { file = \"JTH-PBE-v2.0/Si/Si.xml\", sha256 = \"0\", cutoff_hint_ha = 5.0 }\n",
            "Si: file \"JTH-PBE-v2.0/Si/Si.xml\" must be a relative path of plain names outside the directory of a family",
        ),
    ] {
        let outcome = evaluate(&tabled("table-bad", table));
        assert!(
            matches!(&outcome, Outcome::Fail(f) if f.contains(why)),
            "{why}: {outcome:?}"
        );
    }
}

#[test]
fn a_jth_2_0_entry_without_any_hint_fails_by_name() {
    let root = silicon("table-hintless", "0.5", "0.5");
    write(
        &root,
        "benchmarks/1_a/table.toml",
        "[element]\nSi = \"jth-2.0\"\n",
    );
    let outcome = evaluate(&root);
    let why = "Si: datasets/jth-2.0.toml holds no pbe hint";
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains(why)),
        "{outcome:?}"
    );
}

#[test]
fn a_tabled_input_that_sets_its_own_dataset_files_is_a_harness_fault() {
    let root = tabled("table-input-files", &SI_FILE.replace("SHA", &half()));
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    let own = format!("{input}\n[datasets]\nfiles = {{ Si = \"si.xml\" }}\n");
    write(&root, "benchmarks/1_a/input.toml", &own);
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("input: datasets.files: ")
            && f.contains("1_a/table.toml gives each file, so the input sets none")),
        "{outcome:?}"
    );
}

#[test]
fn an_input_names_only_the_table_files_of_its_own_species() {
    let sha = half();
    let table = format!(
        "[element]\nSi = {{ file = \"si.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 5.0 }}\n\
         C = {{ file = \"c.xml\", sha256 = \"{sha}\", cutoff_hint_ha = 5.0 }}\n"
    );
    let root = tabled("table-species", &table);
    write(&root, "benchmarks/1_a/c.xml", "0.5");
    let bin = engine(
        &root,
        &format!("grep -q '^C = ' input.toml && exit 2\n{READS_DATASET}"),
    );
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert_eq!(outcome, Outcome::Pass);
}

fn naming(path: &str, named: &str, sha: &str) -> String {
    format!(
        "e=$(cat datasets/{path})\nprintf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1], \"datasets\": \
         {{\"Si\": {{\"path\": \"{named}\", \"sha256\": \"{sha}\"}}}}}}' \"$e\""
    )
}

#[test]
fn an_outcome_must_name_the_bytes_that_the_harness_placed() {
    let root = silicon("datasets-agree", "0.5", "0.5");
    let at = "JTH-PBE-v2.0/Si/Si.xml";
    let run = |body: &str| {
        let bin = engine(&root, body);
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest)
    };
    assert_eq!(run(&naming(at, at, &half())), Outcome::Pass);
    for (named, sha) in [(at, "0".to_owned()), ("own/Si.xml", half())] {
        let outcome = run(&naming(at, named, &sha));
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: the outcome names Si at ")
                && why.contains("with the sha256 of the bytes that the harness placed")),
            "{named}: {outcome:?}"
        );
    }
}

#[test]
fn an_outcome_that_names_no_file_of_a_species_fails() {
    let root = silicon("datasets-unnamed", "0.5", "0.5");
    let body = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"datasets\": {}}'";
    let bin = engine(&root, body);
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: the outcome names Si at null")),
        "{outcome:?}"
    );
}

fn ours(name: &str) -> Tree {
    let root = silicon(name, "0.25", "0.25");
    let rows = [
        ("pbe", "Si", "0.25", None),
        ("pbe", "Al", "al", None),
        ("lda", "Al", "lda", None),
    ];
    write(
        &root,
        "datasets/jth-2.0.toml",
        &jth(&remote_url(&root), &rows),
    );
    write(&root, "remote/JTH-PBE-v2.0/Al/Al.xml", "al");
    write(&root, "remote/JTH-LDA-v2.0/Al/Al.xml", "lda");
    write(
        &root,
        "datasets/ours-v0.toml",
        &format!(
            "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n\
             Si = {{ file = \"{OURS_SI}\", sha256 = \"{}\", cutoff_hint_ha = 5.0 }}\n",
            half()
        ),
    );
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    let input = input.replace(
        "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]",
        "species = [\"Si\", \"Al\"]\npositions_fractional = [[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]]",
    );
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &format!("{input}\n[datasets]\nfamily = \"ours-v0\"\n"),
    );
    root
}

const OURS_SI: &str = "ours-v0/Si.xml";

#[test]
fn an_input_of_a_family_of_ours_gets_its_own_file_and_the_base_file_for_the_rest() {
    let root = ours("datasets-ours");
    let al = "JTH-PBE-v2.0/Al/Al.xml";
    let reads = format!(
        "test ! -e datasets/JTH-PBE-v2.0/Si/Si.xml && test ! -e datasets/JTH-LDA-v2.0/Al/Al.xml || exit 5\n\
         read -r a < datasets/{al}; [ \"$a\" = al ] || exit 6\n\
         read -r e < datasets/{OURS_SI}\n\
         printf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1], \"datasets\": {{\"Si\": {{\"path\": \"{OURS_SI}\", \
         \"sha256\": \"{}\"}}, \"Al\": {{\"path\": \"{al}\", \"sha256\": \"{}\"}}}}}}' \"$e\"",
        half(),
        crate::harness::hex(&Sha256::digest("al"))
    );
    let bin = bare(&root, &reads);
    let evaluate =
        || Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    let outcome = evaluate();
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("ours-v0/Si.xml: datasets/ours-v0.toml names no source")),
        "{outcome:?}"
    );
    write(&root, &format!("target/datasets/{OURS_SI}"), "0.5");
    assert_eq!(evaluate(), Outcome::Pass);
}

#[test]
fn a_family_of_ours_whose_base_is_another_of_ours_is_a_harness_fault() {
    let root = ours("datasets-ours-chain");
    write(
        &root,
        "datasets/ours-v1.toml",
        "base = \"ours-v0\"\nfunctional = \"pbe\"\n\n[element]\n",
    );
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &input.replace("\"ours-v0\"", "\"ours-v1\""),
    );
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets/ours-v1.toml: base \"ours-v0\" is a family of ours")),
        "{outcome:?}"
    );
}

fn bare(root: &Tree, body: &str) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;
    let path = root.join("bare-engine");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

#[test]
fn a_family_table_that_no_input_names_leaves_the_verdict_of_a_tabled_case() {
    let own = "ours-v9/si.xml";
    let root = tabled(
        "table-unnamed-family",
        &SI_FILE.replace("si.xml", own).replace("SHA", &half()),
    );
    write(&root, &format!("benchmarks/1_a/{own}"), "0.5");
    let body = format!(
        "read -r e < datasets/{own}\nprintf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1], \"datasets\": \
         {{\"Si\": {{\"path\": \"{own}\", \"sha256\": \"{}\"}}}}}}' \"$e\"",
        half()
    );
    let bin = bare(&root, &body);
    let evaluate =
        || Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    assert_eq!(evaluate(), Outcome::Pass);
    write(
        &root,
        "datasets/ours-v9.toml",
        &format!(
            "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n\
             Al = {{ file = \"ours-v9/al.xml\", sha256 = \"{}\", cutoff_hint_ha = 5.0 }}\n",
            half()
        ),
    );
    assert_eq!(evaluate(), Outcome::Pass);
    write(&root, "datasets/broken.toml", "[[not toml");
    assert_eq!(evaluate(), Outcome::Pass);
}

fn sections(name: &str, word: &str) -> Tree {
    let root = silicon(name, "0.5", "0.5");
    let rows = [
        ("pbe", "Si", "0.5", None),
        ("pbe", "Al", "al", None),
        ("lda", "Al", "lda", None),
    ];
    write(
        &root,
        "datasets/jth-2.0.toml",
        &jth(&remote_url(&root), &rows),
    );
    write(&root, "remote/JTH-PBE-v2.0/Al/Al.xml", "al");
    write(&root, "remote/JTH-LDA-v2.0/Al/Al.xml", "lda");
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    let input = input.replace(
        "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]",
        "species = [\"Si\", \"Al\"]\npositions_fractional = [[0.0, 0.0, 0.0], [0.5, 0.5, 0.5]]",
    );
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &format!("{input}\n[xc]\nfunctional = \"{word}\"\n"),
    );
    root
}

fn naming_al(root: &Tree, al: &str) -> Outcome {
    let sha = |bytes: &str| crate::harness::hex(&Sha256::digest(bytes));
    let held = if al.contains("LDA") { "lda" } else { "al" };
    let body = format!(
        "printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"datasets\": {{\"Si\": {{\"path\": \
         \"JTH-PBE-v2.0/Si/Si.xml\", \"sha256\": \"{}\"}}, \"Al\": {{\"path\": \"{al}\", \"sha256\": \"{}\"}}}}}}'",
        sha("0.5"),
        sha(held)
    );
    let bin = bare(root, &body);
    Session::new(root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest)
}

const PBE_AL: &str = "JTH-PBE-v2.0/Al/Al.xml";
const LDA_AL: &str = "JTH-LDA-v2.0/Al/Al.xml";

#[test]
fn an_input_whose_functional_keys_a_section_must_name_the_file_of_that_section() {
    let root = sections("datasets-own-section", "pbe");
    assert_eq!(naming_al(&root, PBE_AL), Outcome::Pass);
    let outcome = naming_al(&root, LDA_AL);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: the outcome names Al at ")
            && why.contains(&format!("not one of [\"{PBE_AL}\"] with the sha256 of the bytes that the harness placed"))),
        "{outcome:?}"
    );
}

#[test]
fn an_input_whose_functional_has_no_section_is_refused_whatever_the_outcome_names() {
    let root = sections("datasets-no-section", "r2scan");
    for al in [PBE_AL, LDA_AL] {
        let outcome = naming_al(&root, al);
        assert!(
            matches!(&outcome, Outcome::Fail(why)
                if why.contains("datasets: datasets/jth-2.0.toml has no r2scan section for Al")),
            "{al}: {outcome:?}"
        );
    }
}

fn reporting(
    name: &str,
    word: Option<&str>,
    table: &str,
    remote: &[(&str, &str)],
    datasets: &str,
) -> Outcome {
    let root = silicon(name, "0.5", "0.5");
    if let Some(word) = word {
        let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
        let input = format!("{input}\n[xc]\nfunctional = \"{word}\"\n");
        write(&root, "benchmarks/1_a/input.toml", &input);
    }
    write(
        &root,
        "datasets/jth-2.0.toml",
        &table.replace("URL", &remote_url(&root)),
    );
    for (path, text) in remote {
        write(&root, &format!("remote/{path}"), text);
    }
    let body = format!(
        "printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"datasets\": {datasets}}}'"
    );
    let bin = bare(&root, &body);
    Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest)
}

fn hash(text: &str) -> String {
    crate::harness::hex(&Sha256::digest(text))
}

fn si_at(path: &str, text: &str) -> String {
    format!(
        "{{\"Si\": {{\"path\": \"{path}\", \"sha256\": \"{}\"}}}}",
        hash(text)
    )
}

const LDA_SI: &str = "JTH-LDA-v2.0/Si/Si.xml";

fn lda_and(pbe: &str) -> String {
    format!(
        "url = \"URL\"\n\n[lda]\nlayout = \"JTH-LDA-v2.0/{{element}}/{{element}}.xml\"\n\n[lda.element]\n\
         Si = {{ sha256 = \"{}\" }}\n{pbe}",
        hash("lda")
    )
}

#[test]
fn an_r2scan_input_whose_outcome_names_the_lda_file_is_refused() {
    let pbe = jth("URL", &[("pbe", "Si", "0.5", None)]);
    let pbe = pbe
        .split_once("\n[pbe]")
        .map(|(_, rest)| format!("\n[pbe]{rest}"))
        .unwrap();
    let outcome = reporting(
        "datasets-r2scan-only-lda",
        Some("r2scan"),
        &lda_and(&pbe),
        &[(LDA_SI, "lda")],
        &si_at(LDA_SI, "lda"),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: datasets/jth-2.0.toml has no r2scan section for Si")),
        "{outcome:?}"
    );
}

#[test]
fn an_own_file_of_the_checkout_is_placed_at_its_sha256_and_other_bytes_are_a_harness_fault() {
    let root = ours("datasets-ours-checkout");
    let reads = format!(
        "read -r e < datasets/{OURS_SI}\n\
         printf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1], \"datasets\": {{\"Si\": {{\"path\": \"{OURS_SI}\", \
         \"sha256\": \"{}\"}}, \"Al\": {{\"path\": \"JTH-PBE-v2.0/Al/Al.xml\", \"sha256\": \"{}\"}}}}}}' \"$e\"",
        half(),
        crate::harness::hex(&Sha256::digest("al"))
    );
    let bin = bare(&root, &reads);
    let evaluate =
        || Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    write(&root, &format!("datasets/{OURS_SI}"), "0.4");
    let outcome = evaluate();
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(&format!(
            "ours-v0/Si.xml: datasets/ours-v0/Si.xml has sha256 {}, but datasets/ours-v0.toml pins {}",
            crate::harness::hex(&Sha256::digest("0.4")),
            half()
        ))),
        "{outcome:?}"
    );
    assert!(!root.join(format!("target/datasets/{OURS_SI}")).exists());
    write(&root, &format!("datasets/{OURS_SI}"), "0.5");
    assert_eq!(evaluate(), Outcome::Pass);
}

#[test]
fn a_pbe_input_against_a_family_without_a_pbe_section_is_refused() {
    let outcome = reporting(
        "datasets-pbe-absent",
        None,
        &lda_and(""),
        &[(LDA_SI, "lda")],
        &si_at(LDA_SI, "lda"),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: datasets/jth-2.0.toml has no pbe section for Si")),
        "{outcome:?}"
    );
}

#[test]
fn a_pbe_input_against_an_empty_pbe_section_is_refused() {
    let empty = "\n[pbe]\nlayout = \"JTH-PBE-v2.0/{element}/{element}.xml\"\n\n[pbe.element]\n";
    let outcome = reporting(
        "datasets-pbe-empty",
        None,
        &lda_and(empty),
        &[(LDA_SI, "lda")],
        &si_at(LDA_SI, "lda"),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: datasets/jth-2.0.toml has no pbe section for Si")),
        "{outcome:?}"
    );
}

#[test]
fn one_path_with_two_pins_is_a_harness_fault() {
    let table = format!(
        "url = \"URL\"\n{}{}",
        section("lda", "shared/{element}.xml", "lda"),
        section("pbe", "shared/{element}.xml", "pbe")
    );
    let (root, outcome) = staged(
        "datasets-pbe-conflicting-pin",
        "[datasets]\nfamily = \"Si\"\n",
        &[("Si.toml", table)],
        &[("shared/Si.xml", "pbe")],
        &[("shared/Si.xml", "lda")],
        &printing(&si_at("shared/Si.xml", "lda")),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(&twice("datasets/shared/Si.xml"))),
        "{outcome:?}"
    );
    assert!(!root.join("ran").exists(), "the engine ran");
}

#[test]
fn an_outcome_that_names_a_file_of_no_species_of_the_input_fails() {
    let pbe = "JTH-PBE-v2.0/Si/Si.xml";
    let table = jth("URL", &[("pbe", "Si", "0.5", None)]);
    let extra = format!(
        "{{\"Si\": {{\"path\": \"{pbe}\", \"sha256\": \"{}\"}}, \"Al\": {{\"path\": \"unplaced/Al.xml\", \
         \"sha256\": \"{}\"}}}}",
        hash("0.5"),
        hash("al")
    );
    let outcome = reporting("datasets-pbe-extra-entry", None, &table, &[], &extra);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: the outcome names Al, which is no species of the input")),
        "{outcome:?}"
    );
    let root_pass = reporting(
        "datasets-pbe-exact-entry",
        None,
        &table,
        &[],
        &si_at(pbe, "0.5"),
    );
    assert_eq!(root_pass, Outcome::Pass);
}

#[test]
fn an_input_of_ours_whose_outcome_names_the_base_file_of_an_own_row_fails_before_its_measure() {
    let root = ours("datasets-ours-wrong-file");
    let jth_si = "JTH-PBE-v2.0/Si/Si.xml";
    let reads = format!(
        "printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"datasets\": {{\"Si\": {{\"path\": \"{jth_si}\", \
         \"sha256\": \"{}\"}}, \"Al\": {{\"path\": \"JTH-PBE-v2.0/Al/Al.xml\", \"sha256\": \"{}\"}}}}}}'",
        crate::harness::hex(&Sha256::digest("0.25")),
        crate::harness::hex(&Sha256::digest("al"))
    );
    let bin = bare(&root, &reads);
    write(&root, &format!("datasets/{OURS_SI}"), "0.5");
    let measured = |_: &xtask::harness::Evidence| -> Result<xtask::harness::Metrics, String> {
        Err("measured".into())
    };
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", measured);
    assert!(
        matches!(&outcome, Outcome::Fail(why)
            if why.contains("the outcome names Si at") && why.contains("[\"ours-v0/Si.xml\"]")
                && !why.contains("measured")),
        "{outcome:?}"
    );
}

fn staged(
    name: &str,
    extra: &str,
    tables: &[(&str, String)],
    remote: &[(&str, &str)],
    cached: &[(&str, &str)],
    body: &str,
) -> (Tree, Outcome) {
    let root = silicon(name, "0.5", "0.5");
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &format!("{input}\n{extra}"),
    );
    for (file, text) in tables {
        let text = text.replace("URL", &remote_url(&root));
        write(&root, &format!("datasets/{file}"), &text);
    }
    for (path, text) in remote {
        write(&root, &format!("remote/{path}"), text);
    }
    for (path, text) in cached {
        write(&root, &format!("target/datasets/{path}"), text);
    }
    let ran = root.join("ran");
    let bin = bare(&root, &format!(": > '{}'\n{body}", ran.display()));
    let outcome =
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest);
    (root, outcome)
}

fn printing(datasets: &str) -> String {
    format!(
        "printf '{{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1], \"datasets\": {datasets}}}'"
    )
}

fn section(section: &str, layout: &str, pinned: &str) -> String {
    format!(
        "\n[{section}]\nlayout = \"{layout}\"\n\n[{section}.element]\nSi = {{ sha256 = \"{}\" }}\n",
        hash(pinned)
    )
}

fn twice(path: &str) -> String {
    format!("{path}: File exists (os error 17); the sandbox stages each file where nothing is")
}

#[test]
fn a_dataset_file_at_the_path_of_a_family_table_is_refused_before_the_engine_runs() {
    let fake = format!(
        "url = \"URL\"\n{}",
        section("pbe", "alternate/{element}.xml", "lda")
    );
    let root = silicon("datasets-shadow-probe", "0.5", "0.5");
    let fake_text = fake.replace("URL", &remote_url(&root));
    let table = format!(
        "url = \"URL\"\n{}{}",
        section("lda", "alternate/{element}.xml", "lda"),
        section("pbe", "{element}.toml", &fake_text)
    );
    let (root, outcome) = staged(
        "datasets-shadow-probe",
        "[datasets]\nfamily = \"Si\"\n",
        &[("Si.toml", table)],
        &[("Si.toml", &fake_text), ("alternate/Si.xml", "lda")],
        &[],
        &printing(&si_at("alternate/Si.xml", "lda")),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(&twice("datasets/Si.toml"))),
        "{outcome:?}"
    );
    assert!(!root.join("ran").exists(), "the engine ran");
}

#[test]
fn a_shadowed_table_whose_row_pins_other_bytes_is_refused_before_the_engine_runs() {
    let fake = format!(
        "url = \"URL\"\n{}",
        section("pbe", "{element}.toml", "other")
    );
    let root = silicon("datasets-shadow-pin", "0.5", "0.5");
    let fake_text = fake.replace("URL", &remote_url(&root));
    let table = format!(
        "url = \"URL\"\n{}",
        section("pbe", "{element}.toml", &fake_text)
    );
    let (root, outcome) = staged(
        "datasets-shadow-pin",
        "[datasets]\nfamily = \"Si\"\n",
        &[("Si.toml", table)],
        &[("Si.toml", &fake_text)],
        &[],
        &printing(&si_at("Si.toml", &fake_text)),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(&twice("datasets/Si.toml"))),
        "{outcome:?}"
    );
    assert!(!root.join("ran").exists(), "the engine ran");
}

fn folds(root: &Tree) -> bool {
    write(root, "case-probe", "");
    root.join("CASE-PROBE").exists()
}

#[test]
fn two_paths_that_one_file_holds_are_refused_where_the_filesystem_folds_case() {
    let table = format!(
        "url = \"URL\"\n{}{}",
        section("lda", "Shared/{element}.xml", "lda"),
        section("pbe", "shared/{element}.xml", "pbe")
    );
    let (root, outcome) = staged(
        "datasets-casefold",
        "[xc]\nfunctional = \"lda\"\n[datasets]\nfamily = \"Si\"\n",
        &[("Si.toml", table)],
        &[("shared/Si.xml", "pbe")],
        &[("Shared/Si.xml", "lda")],
        &format!(
            "read -r a < datasets/Shared/Si.xml; read -r b < datasets/shared/Si.xml; [ \"$a\" != \"$b\" ] || exit 7\n{}",
            printing(&si_at("Shared/Si.xml", "lda"))
        ),
    );
    if folds(&root) {
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains(&twice("datasets/shared/Si.xml"))),
            "{outcome:?}"
        );
        assert!(!root.join("ran").exists(), "the engine ran");
    } else {
        assert_eq!(outcome, Outcome::Pass);
    }
}

#[test]
fn an_input_without_atoms_passes_without_a_map_or_with_an_empty_one_and_fails_on_any_species() {
    let run = |name: &str, body: &str| {
        let root = tree(
            name,
            &[
                ("benchmarks/1_a/case.toml", CASE),
                ("benchmarks/1_a/input.toml", super::INPUT),
            ],
        );
        let bin = bare(&root, body);
        Session::new(&root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest)
    };
    let none = "printf '{\"eigenvalues_ha\": [[[0.5]]], \"basis_sizes\": [1]}'";
    assert_eq!(run("datasets-atomless-none", none), Outcome::Pass);
    assert_eq!(
        run("datasets-atomless-empty", &printing("{}")),
        Outcome::Pass
    );
    let outcome = run(
        "datasets-atomless-extra",
        &printing(&si_at("unplaced/Si.xml", "si")),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets: the outcome names Si, which is no species of the input")),
        "{outcome:?}"
    );
}

fn si_input(extra: &str) -> String {
    format!(
        "{}{extra}",
        super::INPUT.replace("[basis]", &format!("{SILICON}\n[basis]"))
    )
}

fn naming_si(path: &str, text: &str) -> String {
    format!(
        "{{\"eigenvalues_ha\": [[[0.5]]], \"datasets\": {}}}",
        si_at(path, text)
    )
}

const PBE_SI: &str = "JTH-PBE-v2.0/Si/Si.xml";

fn checked_vector(pinned: &str, staged: &str) -> Vec<(String, Vec<u8>)> {
    let table = jth("u", &[("pbe", "Si", pinned, None)]);
    vec![
        (PBE_SI.to_owned(), staged.as_bytes().to_vec()),
        ("jth-2.0.toml".to_owned(), table.into_bytes()),
    ]
}

#[test]
fn agree_takes_the_selected_row_s_path_pin_and_staged_digest_and_breaks_on_each_alone() {
    let input = si_input("");
    let agrees = |stdout: &str, given: &[(String, Vec<u8>)]| agree(stdout, &input, given);
    let refused = |result: Result<(), String>| {
        assert!(
            result
                .as_ref()
                .is_err_and(|e| e.starts_with("datasets: the outcome names Si at ")),
            "{result:?}"
        );
    };
    let sound = checked_vector("pinned", "pinned");
    assert_eq!(agrees(&naming_si(PBE_SI, "pinned"), &sound), Ok(()));
    refused(agrees(
        &naming_si(PBE_SI, "staged"),
        &checked_vector("pinned", "staged"),
    ));
    refused(agrees(
        &naming_si(PBE_SI, "pinned"),
        &checked_vector("pinned", "staged"),
    ));
    let mut twin = sound.clone();
    twin.push(("twin/Si.xml".to_owned(), b"pinned".to_vec()));
    refused(agrees(&naming_si("twin/Si.xml", "pinned"), &twin));
}

#[test]
fn agree_selects_the_override_of_datasets_files_before_any_family_row() {
    let input = si_input("\n[datasets]\nfiles = { Si = \"own/Si.xml\" }\n");
    let mut given = checked_vector("pinned", "pinned");
    given.push(("own/Si.xml".to_owned(), b"own".to_vec()));
    assert_eq!(
        agree(&naming_si("own/Si.xml", "own"), &input, &given),
        Ok(())
    );
    let family = agree(&naming_si(PBE_SI, "pinned"), &input, &given);
    assert!(
        family
            .as_ref()
            .is_err_and(|e| e.contains("not one of [\"own/Si.xml\"]")),
        "{family:?}"
    );
}

#[test]
fn a_family_or_a_base_name_with_a_slash_is_a_harness_fault() {
    let root = ours("datasets-ours-slash");
    let jth = std::fs::read_to_string(root.join("datasets/jth-2.0.toml")).unwrap();
    let own = std::fs::read_to_string(root.join("datasets/ours-v0.toml")).unwrap();
    write(&root, "datasets/sub/jth-2.0.toml", &jth);
    write(&root, "datasets/sub/ours-v0.toml", &own);
    let input = std::fs::read_to_string(root.join("benchmarks/1_a/input.toml")).unwrap();
    write(
        &root,
        "benchmarks/1_a/input.toml",
        &input.replace("\"ours-v0\"", "\"sub/ours-v0\""),
    );
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("family \"sub/ours-v0\" must be a file name")),
        "{outcome:?}"
    );
    write(&root, "benchmarks/1_a/input.toml", &input);
    write(
        &root,
        "datasets/ours-v0.toml",
        &own.replace("base = \"jth-2.0\"", "base = \"sub/jth-2.0\""),
    );
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("family \"sub/jth-2.0\" must be a file name")),
        "{outcome:?}"
    );
}

#[test]
fn an_input_that_sets_datasets_directory_is_refused_before_the_engine_runs() {
    let (root, outcome) = staged(
        "datasets-directory",
        "[datasets]\ndirectory = \"elsewhere\"\n",
        &[],
        &[],
        &[],
        &printing(&si_at(PBE_SI, "0.5")),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(
            "datasets.directory: the harness stages each dataset at datasets/; a case input leaves this key out"
        )),
        "{outcome:?}"
    );
    assert!(!root.join("ran").exists(), "the engine ran");
}

fn si_table() -> String {
    format!(
        "url = \"URL\"\n{}",
        section("pbe", "JTH-PBE-v2.0/{element}/{element}.xml", "0.5")
    )
}

#[test]
fn a_family_name_that_is_no_file_name_is_refused() {
    let ours = |base: &str| format!("base = \"{base}\"\nfunctional = \"pbe\"\n\n[element]\n");
    for (n, (family, tables)) in [
        ("../evil", vec![("../evil.toml", si_table())]),
        ("sub/t", vec![("sub/t.toml", si_table())]),
        ("", vec![(".toml", si_table())]),
        (
            "ours-x",
            vec![
                ("ours-x.toml", ours("../evil")),
                ("../evil.toml", si_table()),
            ],
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let (root, outcome) = staged(
            &format!("datasets-family-name-{n}"),
            &format!("[datasets]\nfamily = \"{family}\"\n"),
            &tables,
            &[],
            &[],
            &printing(&si_at(PBE_SI, "0.5")),
        );
        let named = if family == "ours-x" {
            "../evil"
        } else {
            family
        };
        assert!(
            matches!(&outcome, Outcome::Fail(why) if why.contains(&format!(
                "family {named:?} must be a file name: no / or \\, not empty, not . or .."
            ))),
            "{family:?}: {outcome:?}"
        );
        assert!(!root.join("ran").exists(), "{family:?}: the engine ran");
    }
}

#[test]
fn a_cycle_of_family_tables_is_refused_without_reading_past_the_base() {
    let ours = |base: &str| format!("base = \"{base}\"\nfunctional = \"pbe\"\n\n[element]\n");
    let (root, outcome) = staged(
        "datasets-family-cycle",
        "[datasets]\nfamily = \"ours-a\"\n",
        &[
            ("ours-a.toml", ours("ours-b")),
            ("ours-b.toml", ours("ours-a")),
        ],
        &[],
        &[],
        &printing(&si_at(PBE_SI, "0.5")),
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets/ours-a.toml: base \"ours-b\" is a family of ours")),
        "{outcome:?}"
    );
    assert!(!root.join("ran").exists(), "the engine ran");
}

#[test]
fn the_judge_reads_the_staged_bytes_not_the_sandbox_after_the_run() {
    let evil = "JTH-PBE-v2.0/Si/evil.xml";
    let table = format!(
        "url = \"u\"\n{}",
        section("pbe", "JTH-PBE-v2.0/{element}/evil.xml", "evil")
    );
    let body = format!(
        "printf '%s' '{table}' > datasets/jth-2.0.toml\nprintf 'evil' > datasets/{evil}\n{}",
        printing(&si_at(evil, "evil"))
    );
    let (_, outcome) = staged("datasets-judge-reads-staged", "", &[], &[], &[], &body);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("not one of [\"{PBE_SI}\"]"))),
        "{outcome:?}"
    );
}

#[test]
fn a_fifo_at_an_own_file_a_case_table_or_a_case_family_table_is_a_harness_fault_without_blocking() {
    let root = ours("datasets-fifo-own");
    crate::fifo(&root, &format!("datasets/{OURS_SI}"));
    let path = root.to_path_buf();
    let outcome = crate::within(5, move || evaluate_at(&path));
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(&format!("datasets/{OURS_SI} is no regular file"))),
        "{outcome:?}"
    );
    let root = ours("datasets-fifo-family");
    crate::fifo(&root, "datasets/ours-v0.toml");
    let path = root.to_path_buf();
    let outcome = crate::within(5, move || evaluate_at(&path));
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets/ours-v0.toml is no regular file")),
        "{outcome:?}"
    );
    let root = tabled("datasets-fifo-table", &SI_FILE.replace("SHA", &half()));
    crate::fifo(&root, "benchmarks/1_a/table.toml");
    let path = root.to_path_buf();
    let outcome = crate::within(5, move || evaluate_at(&path));
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("benchmarks/1_a/table.toml is no regular file")),
        "{outcome:?}"
    );
}

fn evaluate_at(root: &std::path::Path) -> Outcome {
    let bin = engine(root, READS_DATASET);
    Session::new(root, &bin, Tier::Push, &root.join("records")).evaluate("1_a", honest)
}

#[test]
fn an_external_base_that_breaks_the_engines_rules_in_a_row_no_input_reads_is_a_harness_fault() {
    let sha = crate::harness::hex(&Sha256::digest("lda"));
    let lda = "layout = \"JTH-LDA-v2.0/{element}/{element}.xml\"\n\n[lda.element]\n";
    let faulty = |table: &str, layout: &str, row: &str, extra: &str| {
        let fault = format!("layout = \"{layout}\"\n\n[lda.element]\n{row}");
        format!("{}{extra}", table.replace(lda, &fault))
    };
    let al = format!("Al = {{ sha256 = \"{sha}\" }}\n");
    let usual = "JTH-LDA-v2.0/{element}/{element}.xml";
    for (probe, layout, row, extra, why) in [
        (
            "word",
            usual,
            String::new(),
            format!("\n[PBE]\nlayout = \"{usual}\"\n\n[PBE.element]\n{al}"),
            "\"PBE\" keys no dataset section".to_owned(),
        ),
        (
            "layout",
            "JTH-LDA-v2.0/Al.xml",
            al.clone(),
            String::new(),
            "lda.layout: \"JTH-LDA-v2.0/Al.xml\" has no {element}".to_owned(),
        ),
        (
            "element",
            usual,
            format!("al = {{ sha256 = \"{sha}\" }}\n"),
            String::new(),
            "\"al\" is not an element symbol".to_owned(),
        ),
        (
            "pin",
            usual,
            "Al = { sha256 = \"XYZ\" }\n".to_owned(),
            String::new(),
            "Al: sha256 \"XYZ\" is not 64 lowercase hex digits".to_owned(),
        ),
        (
            "hint",
            usual,
            format!("Al = {{ sha256 = \"{sha}\", cutoff_hint_ha = -1.0 }}\n"),
            String::new(),
            "Al: cutoff_hint_ha must be positive".to_owned(),
        ),
        (
            "path",
            "../{element}.xml",
            al.clone(),
            String::new(),
            "Al: \"../Al.xml\" is not a relative path of plain names".to_owned(),
        ),
    ] {
        let root = silicon(&format!("datasets-engine-rules-{probe}"), "0.5", "0.5");
        let table = std::fs::read_to_string(root.join("datasets/jth-2.0.toml")).unwrap();
        assert!(table.contains(lda), "{table}");
        write(
            &root,
            "datasets/jth-2.0.toml",
            &faulty(&table, layout, &row, &extra),
        );
        let outcome = evaluate(&root);
        assert!(
            matches!(&outcome, Outcome::Fail(found) if found.contains("datasets/jth-2.0.toml: ") && found.contains(why.as_str())),
            "{probe}: {outcome:?}"
        );
    }
    let root = silicon("datasets-engine-rules-encoding", "0.5", "0.5");
    let mut table = std::fs::read(root.join("datasets/jth-2.0.toml")).unwrap();
    table.extend_from_slice(b"\n# \xff\n");
    std::fs::write(root.join("datasets/jth-2.0.toml"), table).unwrap();
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("datasets/jth-2.0.toml: invalid utf-8")),
        "encoding: {outcome:?}"
    );
    assert_eq!(
        evaluate(&silicon("datasets-engine-rules-control", "0.5", "0.5")),
        Outcome::Pass
    );
}

#[test]
fn an_own_file_whose_metadata_cannot_be_read_is_named_not_taken_for_absent() {
    use std::os::unix::fs::PermissionsExt;
    if crate::root_user() {
        return;
    }
    let root = ours("datasets-own-unreadable");
    write(&root, &format!("datasets/{OURS_SI}"), "0.5");
    let dir = root.join("datasets/ours-v0");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o600)).unwrap();
    let outcome = evaluate(&root);
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("ours-v0/Si.xml: ") && why.contains("ermission denied")),
        "{outcome:?}"
    );
}

#[test]
fn a_cache_below_a_regular_file_is_a_harness_fault_not_a_miss() {
    let root = ours("datasets-cache-enotdir");
    write(&root, &format!("datasets/{OURS_SI}"), "0.5");
    write(
        &root,
        "target/datasets/ours-v0",
        "a file where a directory is\n",
    );
    let outcome = evaluate(&root);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("target/datasets/ours-v0/Si.xml: ") && why.contains("ot a directory")),
        "{outcome:?}"
    );
}
