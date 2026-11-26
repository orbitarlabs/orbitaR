use crate::io::run::directory;
use crate::jth_table;
use orbitar::io::{Family, Resolved};
use orbitar::model::Functional;
use std::path::Path;

const JTH: &str = "https://raw.githubusercontent.com/abinit/paw_jth_datasets/a03e3f0b7ac8395550cca88880d8cb7dcd478bb3/pseudos";
const SI_PIN: &str = "79075fcce1418817b2dce0ebe90d6a970776fc0832375a2ef99af0ac9dbe863a";
const OWN_SI: &str = "1111111111111111111111111111111111111111111111111111111111111111";

fn ours(rows: &str) -> String {
    format!("base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n{rows}")
}

fn si_row() -> String {
    format!(
        "Si = {{ file = \"orbitaR-PBE-v0/Si.xml\", sha256 = \"{OWN_SI}\", cutoff_hint_ha = 14.0 }}\n"
    )
}

fn tables(name: &str, tables: &[(&str, String)]) -> std::path::PathBuf {
    let mut files = vec![("jth-2.0.toml".to_owned(), jth_table().to_owned())];
    files.extend(
        tables
            .iter()
            .map(|(n, text)| (format!("{n}.toml"), text.clone())),
    );
    let files: Vec<(&str, String)> = files.iter().map(|(p, t)| (p.as_str(), t.clone())).collect();
    directory(name, &files)
}

#[test]
#[expect(
    clippy::disallowed_methods,
    reason = "the test lists the family tables that the repository holds"
)]
fn every_table_under_datasets_parses_and_resolves_each_of_its_rows() {
    let names: Vec<String> = (std::fs::read_dir("datasets").unwrap())
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|x| x == "toml"))
        .map(|path| path.file_stem().unwrap().to_string_lossy().into_owned())
        .collect();
    assert!(names.contains(&"jth-2.0".to_owned()), "{names:?}");
    for name in names {
        let family = Family::read(Path::new("datasets"), &name).unwrap();
        assert!(!family.rows().is_empty(), "{name}");
        for row in family.rows() {
            let resolves = (1..=118u8).any(|z| family.resolve(row.functional, z) == Ok(row));
            assert!(resolves, "{name}: {}", row.element);
        }
    }
}

#[test]
fn jth_resolves_each_functional_by_its_layout_with_its_pin_source_and_hint() {
    let jth = Family::read(Path::new("datasets"), "jth-2.0").unwrap();
    assert_eq!(
        jth.resolve(Functional::Pbe, 14).unwrap(),
        &Resolved {
            element: "Si",
            functional: Functional::Pbe,
            path: "JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml".into(),
            sha256: SI_PIN.into(),
            url: Some(format!("{JTH}/JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml")),
            cutoff_hint_ha: Some(10.0),
        }
    );
    let lda = jth.resolve(Functional::Lda, 26).unwrap();
    assert_eq!(
        (lda.path.as_str(), lda.cutoff_hint_ha),
        ("JTH-LDA-v2.0/Fe/Fe.LDA_PW-JTH.xml", None)
    );
    assert_eq!(jth.rows().len(), 87);
}

#[test]
fn a_family_of_ours_lists_its_own_files_and_reads_the_base_beside_it_for_the_rest() {
    let dir = tables("family-ours", &[("orbitaR-PBE-v0", ours(&si_row()))]);
    let family = Family::read(&dir, "orbitaR-PBE-v0").unwrap();
    assert_eq!(
        family.resolve(Functional::Pbe, 14).unwrap(),
        &Resolved {
            element: "Si",
            functional: Functional::Pbe,
            path: "orbitaR-PBE-v0/Si.xml".into(),
            sha256: OWN_SI.into(),
            url: None,
            cutoff_hint_ha: Some(14.0),
        }
    );
    let jth = Family::read(&dir, "jth-2.0").unwrap();
    assert_eq!(
        family.resolve(Functional::Pbe, 13),
        jth.resolve(Functional::Pbe, 13)
    );
    assert_eq!(family.rows().len(), 86);
    assert_eq!(
        family.resolve(Functional::Lda, 26).unwrap_err(),
        "datasets.family: orbitaR-PBE-v0 holds no lda dataset for Fe; name a file in [datasets] files"
    );
}

#[test]
fn a_missing_table_names_its_path_and_the_fix() {
    let dir = tables(
        "family-missing",
        &[("ours", ours(&si_row()).replace("jth-2.0", "jth-9"))],
    );
    let error = Family::read(&dir, "nope").unwrap_err();
    let path = dir.join("nope.toml");
    assert!(
        error.starts_with(&format!("datasets.family: {}: ", path.display()))
            && error.ends_with(
                "; put the table nope.toml there: datasets/nope.toml of the orbitaR repository"
            ),
        "{error}"
    );
    let error = Family::read(&dir, "ours").unwrap_err();
    assert!(
        error.starts_with(&format!(
            "datasets.family: {}: {}: ",
            dir.join("ours.toml").display(),
            dir.join("jth-9.toml").display()
        )),
        "{error}"
    );
}

#[test]
fn a_family_name_that_leaves_the_directory_is_refused_before_any_read() {
    for name in ["../x", "/etc/x", "", "a/b"] {
        assert_eq!(
            Family::read(Path::new("datasets"), name).unwrap_err(),
            format!(
                "datasets.family: {name:?} must be a file name: datasets.directory holds <family>.toml"
            )
        );
    }
}

#[test]
fn a_bad_table_fails_by_its_key() {
    let dir = tables(
        "family-bad",
        &[
            ("ours-a", ours(&si_row())),
            ("ours-b", ours(&si_row()).replace("jth-2.0", "ours-a")),
        ],
    );
    let error = Family::read(&dir, "ours-b").unwrap_err();
    assert_eq!(
        error,
        format!(
            "datasets.family: {}: {}: a base is an external family, not one of ours",
            dir.join("ours-b.toml").display(),
            dir.join("ours-a.toml").display()
        )
    );
    let base = |name: &str| Family::read(Path::new("datasets"), name);
    let hex = OWN_SI;
    for (text, want) in [
        (
            ours("").replace("\"pbe\"", "\"pw91\""),
            "\"pw91\" is not a functional of xc.functional",
        ),
        (
            ours(&format!(
                "Si = {{ file = \"../Si.xml\", sha256 = \"{hex}\", cutoff_hint_ha = 14.0 }}\n"
            )),
            "pbe.element.Si: \"../Si.xml\" must be a relative path of plain names",
        ),
        (
            ours("Si = { file = \"Si.xml\", sha256 = \"AB\", cutoff_hint_ha = 14.0 }\n"),
            "pbe.element.Si: sha256 \"AB\" is not 64 lowercase hex digits",
        ),
        (
            ours("Si = { file = \"Si.xml\", sha256 = \"ab\", cutoff_hint_ha = 14.0 }\n"),
            "pbe.element.Si: sha256 \"ab\" is not 64 lowercase hex digits",
        ),
        (
            ours(&format!(
                "Si = {{ file = \"Si.xml\", sha256 = \"{hex}\", cutoff_hint_ha = 0.0 }}\n"
            )),
            "pbe.element.Si: cutoff_hint_ha must be positive",
        ),
        (
            ours(&format!(
                "Xx = {{ file = \"Xx.xml\", sha256 = \"{hex}\", cutoff_hint_ha = 1.0 }}\n"
            )),
            "pbe.element.Xx: \"Xx\" is not an element symbol",
        ),
        (
            format!(
                "url = \"u\"\n[pbe]\nlayout = \"Si.xml\"\n[pbe.element]\nSi = {{ sha256 = \"{hex}\" }}\n"
            ),
            "pbe.layout: \"Si.xml\" has no {element}",
        ),
    ] {
        assert_eq!(Family::parse("x", &text, base).unwrap_err(), want, "{text}");
    }
}

#[test]
fn the_json_rows_carry_a_url_only_for_a_file_with_a_source() {
    let dir = tables("family-json", &[("orbitaR-PBE-v0", ours(&si_row()))]);
    let family = Family::read(&dir, "orbitaR-PBE-v0").unwrap();
    let rows: serde_json::Value = serde_json::from_str(&family.json()).unwrap();
    let row = |element: &str| {
        (rows.as_array().unwrap().iter())
            .find(|row| row["element"] == element)
            .unwrap()
            .clone()
    };
    assert_eq!(
        row("Si"),
        serde_json::json!({"element": "Si", "functional": "pbe", "path": "orbitaR-PBE-v0/Si.xml",
                            "sha256": OWN_SI, "cutoff_hint_ha": 14.0})
    );
    assert_eq!(
        row("Al")["url"],
        format!("{JTH}/JTH-PBE-v2.0/Al/Al.GGA_PBE-JTH.xml")
    );
}

#[test]
fn each_functional_reads_the_section_of_its_dataset() {
    let jth = Family::read(Path::new("datasets"), "jth-2.0").unwrap();
    for row in Functional::ALL {
        let maker = Functional::of_dataset(row.dataset.0, row.dataset.1).unwrap();
        assert_eq!(
            jth.resolve(row.functional, 14),
            jth.resolve(maker, 14),
            "{}",
            row.word
        );
    }
    let si = jth.resolve(Functional::R2scan, 14).unwrap();
    assert_eq!(
        (si.functional, si.path.as_str()),
        (Functional::Pbe, "JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml")
    );
}

#[test]
fn a_section_keyed_by_a_functional_that_reads_another_functionals_datasets_is_refused() {
    let external = "url = \"https://example.org\"\n\n[r2scan]\nlayout = \"x/{element}.xml\"\n\n[r2scan.element]\n\
                    Si = { sha256 = \"1111111111111111111111111111111111111111111111111111111111111111\" }\n";
    let e = Family::parse("x", external, |_| unreachable!()).unwrap_err();
    assert_eq!(
        e,
        "\"r2scan\" reads the datasets of pbe: key its files by \"pbe\""
    );
    let own = ours(&si_row()).replace("\"pbe\"", "\"r2scan\"");
    let e = Family::parse("ours", &own, |name| {
        Family::read(Path::new("datasets"), name)
    })
    .unwrap_err();
    assert_eq!(
        e,
        "\"r2scan\" reads the datasets of pbe: key its files by \"pbe\""
    );
}
