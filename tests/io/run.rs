use crate::io::paw_xml::xml;
use crate::{jth_table, synthetic_draft, synthetic_silicon};
use orbitar::io::{DatasetFiles, read_spec, run_file};
use orbitar::ks::Failure;
use std::path::{Path, PathBuf};

#[test]
fn a_benchmark_input_file_becomes_one_line_of_json() {
    let json = run_file("benchmarks/1_free_electron/input.toml", |_, _| {}).unwrap();
    assert!(
        json.starts_with("{\"basis_sizes\":[") && !json.contains('\n'),
        "{json}"
    );
}

#[test]
fn a_missing_file_is_invalid_input_named_by_its_path() {
    let error = run_file("benchmarks/no_such_input.toml", |_, _| {}).unwrap_err();
    assert!(
        matches!(error, Failure::Invalid(m) if m.starts_with("benchmarks/no_such_input.toml:"))
    );
}

pub(super) fn carbon(extra: &str) -> String {
    format!(
        "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\
         species = [\"C\"]\npositions_fractional = [[0.1, 0.2, 0.3]]\n\n[basis]\ncutoff_ha = 3.0\n\n\
         [kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.002\n{extra}"
    )
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test lays out an input file and its datasets as a user does"
)]
pub(super) fn directory(name: &str, files: &[(&str, String)]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("orbitar-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    for (path, text) in files {
        let file = root.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }
    root
}

const OWN_CARBON: &str = "\n[datasets]\nfiles = { C = \"own/C.xml\" }\n";

#[test]
fn datasets_come_from_the_directory_beside_the_input_file() {
    let root = directory(
        "datasets",
        &[
            ("case/input.toml", carbon(OWN_CARBON)),
            ("case/datasets/own/C.xml", xml(&synthetic_draft())),
        ],
    );
    let json = run_file(root.join("case/input.toml").to_str().unwrap(), |_, _| {}).unwrap();
    assert!(
        json.contains("\"free_energy_ha\":") && json.contains("\"scf_steps\":"),
        "{json}"
    );
    let moved = directory(
        "moved",
        &[
            (
                "input.toml",
                carbon("\n[datasets]\ndirectory = \"../store\"\nfiles = { C = \"own/C.xml\" }\n"),
            ),
            ("../store/own/C.xml", xml(&synthetic_draft())),
        ],
    );
    assert_eq!(
        run_file(moved.join("input.toml").to_str().unwrap(), |_, _| {}).unwrap(),
        json
    );
}

#[test]
fn each_functional_reads_the_file_of_its_family_row() {
    let root = directory(
        "families",
        &[("datasets/jth-2.0.toml", jth_table().to_owned())],
    );
    let read = |extra: &str| {
        let spec = read_spec(&carbon(extra)).unwrap();
        DatasetFiles::at(&spec, &root.join("datasets")).read(6)
    };
    let missing = read("").unwrap_err();
    let want = Path::new("JTH-PBE-v2.0/C/C.GGA_PBE-JTH.xml");
    assert!(
        matches!(&missing, Failure::Invalid(m) if m.starts_with("datasets.directory: ")
            && m.contains(want.to_str().unwrap())
            && m.ends_with("; put the file that jth-2.0 pins for C there (sha256 \
                13f129128c908f9e6d8160a52f89491ee6f94306345f1de573847c9c5d37b9c8), or name your own file in \
                [datasets] files")),
        "{missing:?}"
    );
    let lda = read("\n[xc]\nfunctional = \"lda\"\n").unwrap_err();
    assert_eq!(
        lda,
        Failure::Invalid(
            "datasets.family: jth-2.0 holds no lda dataset for C; name a file in [datasets] files"
                .into()
        )
    );
}

fn sha256(text: &str) -> String {
    use sha2::{Digest, Sha256};
    (Sha256::digest(text.as_bytes()).iter())
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[test]
fn a_family_file_off_its_pin_fails_by_key_path_both_hashes_and_the_fix() {
    let synthetic = xml(&synthetic_draft());
    let root = directory(
        "off-pin",
        &[
            ("input.toml", carbon("")),
            ("datasets/jth-2.0.toml", jth_table().to_owned()),
            (
                "datasets/JTH-PBE-v2.0/C/C.GGA_PBE-JTH.xml",
                synthetic.clone(),
            ),
        ],
    );
    let error = run_file(root.join("input.toml").to_str().unwrap(), |_, _| {}).unwrap_err();
    let pin = "13f129128c908f9e6d8160a52f89491ee6f94306345f1de573847c9c5d37b9c8";
    let datasets = root.join("datasets");
    let want = format!(
        "datasets.family: {}: sha256 {}, but jth-2.0 pins {pin} for C; put the pinned file there, or name your own \
         file in [datasets] files",
        datasets.join("JTH-PBE-v2.0/C/C.GGA_PBE-JTH.xml").display(),
        sha256(&synthetic),
    );
    assert_eq!(error, Failure::Invalid(want));
    let bare = directory("no-table", &[("input.toml", carbon(""))]);
    let error = run_file(bare.join("input.toml").to_str().unwrap(), |_, _| {}).unwrap_err();
    assert!(
        matches!(&error, Failure::Invalid(m) if m.starts_with(&format!(
            "datasets.family: {}: ", bare.join("datasets/jth-2.0.toml").display()))
            && m.ends_with("; put the table jth-2.0.toml there: datasets/jth-2.0.toml of the orbitaR repository")),
        "{error:?}"
    );
}

#[test]
fn the_outcome_names_each_file_read_by_its_path_and_sha256() {
    let own = xml(&synthetic_draft());
    let root = directory(
        "files",
        &[
            (
                "own/input.toml",
                carbon("\n[datasets]\nfiles = { C = \"own/C-1.xml\" }\n"),
            ),
            ("own/datasets/own/C-1.xml", own.clone()),
        ],
    );
    let json = run_file(root.join("own/input.toml").to_str().unwrap(), |_, _| {});
    let outcome: serde_json::Value = serde_json::from_str(&json.unwrap()).unwrap();
    assert_eq!(
        outcome["datasets"],
        serde_json::json!({"C": {"path": "own/C-1.xml", "sha256": sha256(&own)}})
    );
}

#[test]
fn a_file_that_holds_another_element_or_none_fails_by_its_path() {
    let silicon = xml(&synthetic_silicon());
    let root = directory("other-element", &[("datasets/own/C.xml", silicon)]);
    let read = |path: &str| {
        let extra = format!("\n[datasets]\nfiles = {{ C = {path:?} }}\n");
        let spec = read_spec(&carbon(&extra)).unwrap();
        DatasetFiles::at(&spec, &root.join("datasets")).read(6)
    };
    let file = root.join("datasets/own/C.xml");
    let wrong = read("own/C.xml").unwrap_err();
    assert!(
        matches!(&wrong, Failure::Invalid(m)
            if *m == format!("datasets.files: {}: holds Z = 14, not C", file.display())),
        "{wrong:?}"
    );
    let missing = read("own/none.xml").unwrap_err();
    let want = format!(
        "datasets.files: {}:",
        root.join("datasets/own/none.xml").display()
    );
    assert!(
        matches!(&missing, Failure::Invalid(m) if m.starts_with(&want)),
        "{missing:?}"
    );
}
