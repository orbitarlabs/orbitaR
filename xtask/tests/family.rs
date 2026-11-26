use crate::{has, lever, tree, write};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn sha(text: &str) -> String {
    crate::harness::hex(&Sha256::digest(text))
}

const FROM: &str = "datasets/ours-v0/t.toml";

fn tuning(name: &str, al_sha: &str) -> crate::Tree {
    let table = format!(
        "[element]\nSi = \"jth-2.0\"\nAl = {{ file = \"Al.xml\", sha256 = \"{al_sha}\", cutoff_hint_ha = 25.0 }}\n\
         C = {{ file = \"C.xml\", sha256 = \"{}\", cutoff_hint_ha = 15.5 }}\n",
        sha("c")
    );
    tree(
        name,
        &[
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            (FROM, &table),
            ("datasets/ours-v0/Al.xml", "al"),
            ("datasets/ours-v0/C.xml", "c"),
        ],
    )
}

#[test]
fn it_writes_the_family_table_with_the_sha256_of_each_own_file() {
    let root = tuning("family-new", &sha("al"));
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(found.is_empty(), "{found:?}");
    let written = fs::read_to_string(root.join("datasets/ours-v0.toml")).unwrap();
    assert_eq!(
        written,
        format!(
            "# Written by `cargo xtask family ours-v0 --from datasets/ours-v0/t.toml`; a family never changes.\n\
             base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n\
             Al = {{ file = \"ours-v0/Al.xml\", sha256 = \"{}\", cutoff_hint_ha = 25.0 }}\n\
             C = {{ file = \"ours-v0/C.xml\", sha256 = \"{}\", cutoff_hint_ha = 15.5 }}\n",
            sha("al"),
            sha("c")
        )
    );
    let link = root.join("python/orbitar/families/ours-v0.toml");
    assert_eq!(
        fs::read_link(&link).unwrap(),
        Path::new("../../../datasets/ours-v0.toml")
    );
    assert_eq!(fs::read_to_string(&link).unwrap(), written);
}

#[test]
fn every_table_of_the_repository_has_its_link_in_the_package() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let tables: Vec<String> = (fs::read_dir(repo.join("datasets")).unwrap())
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.ends_with(".toml"))
        .collect();
    assert!(!tables.is_empty());
    for name in tables {
        let link = repo.join("python/orbitar/families").join(&name);
        assert_eq!(
            fs::read_link(&link).ok(),
            Some(Path::new("../../../datasets").join(&name)),
            "{name}: `cargo xtask family` writes python/orbitar/families/{name}"
        );
    }
}

#[test]
fn a_name_that_exists_is_refused() {
    let root = tuning("family-exists", &sha("al"));
    for name in ["jth-2.0", "ours-v0"] {
        if name == "ours-v0" {
            write(&root, "datasets/ours-v0.toml", "");
        }
        let found = lever(&["family", name, "--from", FROM], &root);
        assert!(
            has(
                &found,
                &format!("family: {name} exists; a family never changes")
            ),
            "{found:?}"
        );
    }
}

#[test]
fn a_file_off_its_sha256_another_family_or_a_bad_name_is_refused() {
    let root = tuning("family-sha", "0");
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(&found, "Al.xml: sha256 ") && has(&found, "datasets/ours-v0/t.toml gives 0"),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
    let root = tuning("family-other", &sha("al"));
    write(&root, FROM, "[element]\nSi = \"jth-1.1\"\n");
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "Si names \"jth-1.1\""), "{found:?}");
    write(&root, FROM, "[element]\nSi = \"jth-2.0\"\n");
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "no row names a file of its own"), "{found:?}");
    write(
        &root,
        FROM,
        &format!(
            "[element]\nAl = {{ file = \"Al.xml\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n\
             C = {{ file = \"Al.xml\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n",
            sha("al"),
            sha("al")
        ),
    );
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "C: \"Al.xml\" must be C.xml"), "{found:?}");
    for name in ["../x", ".hidden", "a b", ""] {
        let found = lever(&["family", name, "--from", FROM], &root);
        assert!(
            has(&found, "a family name holds letters"),
            "{name}: {found:?}"
        );
    }
}

#[test]
fn two_own_files_whose_names_differ_only_in_case_are_refused() {
    let root = tuning("family-case", &sha("al"));
    write(
        &root,
        FROM,
        &format!(
            "[element]\nAl = {{ file = \"Al.xml\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n\
             AL = {{ file = \"AL.xml\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n",
            sha("al"),
            sha("c")
        ),
    );
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(&found, "two own files are named AL.xml and Al.xml"),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn an_own_file_name_outside_ascii_letters_digits_and_dot_dash_underscore_is_refused() {
    for element in ["ál", "a l", ".al"] {
        let root = tuning("family-own-name", &sha("al"));
        let name = format!("{element}.xml");
        write(
            &root,
            FROM,
            &format!(
                "[element]\n\"{element}\" = {{ file = \"{name}\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n",
                sha("al")
            ),
        );
        write(&root, &format!("datasets/ours-v0/{name}"), "al");
        let found = lever(&["family", "ours-v0", "--from", FROM], &root);
        assert!(
            has(&found, "an own file name holds letters"),
            "{name}: {found:?}"
        );
        assert!(!root.join("datasets/ours-v0.toml").exists());
    }
}

#[test]
fn a_name_that_differs_from_a_table_only_in_case_is_refused() {
    let root = tuning("family-name-case", &sha("al"));
    let found = lever(&["family", "JTH-2.0", "--from", FROM], &root);
    assert!(
        has(&found, "family: JTH-2.0 exists; a family never changes"),
        "{found:?}"
    );
}

#[test]
fn a_name_of_a_directory_of_the_base_in_any_case_is_refused() {
    let root = tuning("family-name-directory", &sha("al"));
    let base = format!(
        "url = \"u\"\n\n[pbe]\nlayout = \"JTH-PBE-v2.0/{{element}}/{{element}}.xml\"\n\n[pbe.element]\n\
         Si = {{ sha256 = \"{}\" }}\n",
        sha("si")
    );
    write(&root, "datasets/jth-2.0.toml", &base);
    for name in ["JTH-PBE-v2.0", "jth-pbe-v2.0"] {
        let found = lever(&["family", name, "--from", FROM], &root);
        assert!(
            has(
                &found,
                &format!("family: {name} names the directory JTH-PBE-v2.0 of jth-2.0")
            ),
            "{name}: {found:?}"
        );
    }
    assert!(!root.join("datasets/jth-pbe-v2.0.toml").exists());
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(found.is_empty(), "{found:?}");
}

#[test]
fn a_name_whose_package_link_alone_exists_is_refused() {
    let root = tuning("family-link-only", &sha("al"));
    std::fs::create_dir_all(root.join("python/orbitar/families")).unwrap();
    std::os::unix::fs::symlink(
        "../../../datasets/Ours-V0.toml",
        root.join("python/orbitar/families/Ours-V0.toml"),
    )
    .unwrap();
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(&found, "family: ours-v0 exists; a family never changes"),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn a_package_link_that_cannot_be_written_leaves_no_table_and_a_retry_writes_both() {
    use std::os::unix::fs::PermissionsExt;
    let root = tuning("family-no-link", &sha("al"));
    let packaged = root.join("python/orbitar/families");
    fs::create_dir_all(&packaged).unwrap();
    fs::set_permissions(&packaged, fs::Permissions::from_mode(0o555)).unwrap();
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    fs::set_permissions(&packaged, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        has(&found, "python/orbitar/families/ours-v0.toml: "),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(
        fs::read_to_string(packaged.join("ours-v0.toml")).unwrap(),
        fs::read_to_string(root.join("datasets/ours-v0.toml")).unwrap()
    );
}

#[test]
fn a_copy_of_the_shipped_family_becomes_a_new_family_and_the_shipped_name_is_refused() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let read = |path: &str| fs::read_to_string(repo.join(path)).unwrap();
    let shipped = "datasets/orbitaR-PBE-v0";
    let mut files = vec![
        (
            "datasets/jth-2.0.toml".to_owned(),
            read("datasets/jth-2.0.toml"),
        ),
        (format!("{shipped}.toml"), read(&format!("{shipped}.toml"))),
    ];
    for entry in fs::read_dir(repo.join(shipped)).unwrap() {
        let name = entry.unwrap().file_name().into_string().unwrap();
        files.push((
            format!("{shipped}/{name}"),
            read(&format!("{shipped}/{name}")),
        ));
    }
    let files: Vec<(&str, &str)> = files
        .iter()
        .map(|(p, b)| (p.as_str(), b.as_str()))
        .collect();
    let root = tree("family-readme", &files);
    let packaged = root.join("python/orbitar/families");
    fs::create_dir_all(&packaged).unwrap();
    for name in ["jth-2.0", "orbitaR-PBE-v0"] {
        let target = format!("../../../datasets/{name}.toml");
        std::os::unix::fs::symlink(target, packaged.join(format!("{name}.toml"))).unwrap();
    }
    let refused = lever(
        &[
            "family",
            "orbitaR-PBE-v0",
            "--from",
            "datasets/orbitaR-PBE-v0/tuning.toml",
        ],
        &root,
    );
    assert!(
        has(&refused, "orbitaR-PBE-v0 exists; a family never changes"),
        "{refused:?}"
    );

    fs::create_dir_all(root.join("datasets/mine-v0")).unwrap();
    for entry in fs::read_dir(root.join(shipped)).unwrap() {
        let entry = entry.unwrap();
        fs::copy(
            entry.path(),
            root.join("datasets/mine-v0").join(entry.file_name()),
        )
        .unwrap();
    }
    let args = [
        "family",
        "mine-v0",
        "--from",
        "datasets/mine-v0/tuning.toml",
    ];
    let found = lever(&args, &root);
    assert!(found.is_empty(), "{found:?}");
    assert_eq!(
        fs::read_to_string(root.join("datasets/mine-v0.toml")).unwrap(),
        read("datasets/orbitaR-PBE-v0.toml").replace("orbitaR-PBE-v0", "mine-v0")
    );
    assert_eq!(
        fs::read_link(packaged.join("mine-v0.toml")).unwrap(),
        Path::new("../../../datasets/mine-v0.toml")
    );
}

#[test]
fn a_tuning_table_outside_the_family_directory_or_a_file_in_a_subdirectory_is_refused() {
    let root = tuning("family-place", &sha("al"));
    write(&root, "t/table.toml", "[element]\nSi = \"jth-2.0\"\n");
    let outside = tuning("family-place-src", &sha("al"));
    let away = outside.join("datasets/ours-v0/table.toml");
    for from in [
        "t/table.toml",
        "datasets/table.toml",
        away.to_str().unwrap(),
    ] {
        let found = lever(&["family", "ours-v0", "--from", from], &root);
        assert!(
            has(
                &found,
                "a tuning table of ours-v0 lies in datasets/ours-v0/"
            ),
            "{from}: {found:?}"
        );
    }
    write(
        &root,
        "datasets/ours-v0/table.toml",
        &format!(
            "[element]\nC = {{ file = \"own/c.xml\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n",
            sha("c")
        ),
    );
    let found = lever(
        &["family", "ours-v0", "--from", "datasets/ours-v0/table.toml"],
        &root,
    );
    assert!(has(&found, "C: \"own/c.xml\" must be C.xml"), "{found:?}");
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn an_own_file_is_named_after_its_element_as_the_rebuild_reads_it() {
    let root = tuning("family-canonical", &sha("al"));
    write(
        &root,
        FROM,
        &format!(
            "[element]\nO = {{ file = \"renamed.xml\", sha256 = \"{}\", cutoff_hint_ha = 1.0 }}\n",
            sha("al")
        ),
    );
    write(&root, "datasets/ours-v0/renamed.xml", "al");
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "O: \"renamed.xml\" must be O.xml"), "{found:?}");
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn a_family_directory_a_tuning_table_or_an_own_file_that_is_a_link_is_refused() {
    use std::os::unix::fs::symlink;
    let root = tuning("family-links", &sha("al"));
    let outside = tuning("family-links-outside", &sha("al"));
    fs::remove_dir_all(root.join("datasets/ours-v0")).unwrap();
    symlink(
        outside.join("datasets/ours-v0"),
        root.join("datasets/ours-v0"),
    )
    .unwrap();
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "datasets/ours-v0 is a link"), "{found:?}");
    let root = tuning("family-links-file", &sha("al"));
    fs::remove_file(root.join("datasets/ours-v0/Al.xml")).unwrap();
    symlink(
        outside.join("datasets/ours-v0/Al.xml"),
        root.join("datasets/ours-v0/Al.xml"),
    )
    .unwrap();
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(&found, "datasets/ours-v0/Al.xml is a link"),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn the_lever_writes_no_table_that_the_validating_reader_refuses() {
    for (probe, row, why) in [
        (
            "family-cutoff",
            "Al = { file = \"Al.xml\", sha256 = \"SHA\", cutoff_hint_ha = -1.0 }",
            "Al: cutoff_hint_ha must be positive",
        ),
        (
            "family-symbol",
            "Xx = { file = \"Xx.xml\", sha256 = \"SHA\", cutoff_hint_ha = 1.0 }",
            "\"Xx\" is not an element symbol",
        ),
    ] {
        let root = tuning(probe, &sha("al"));
        write(&root, "datasets/ours-v0/Xx.xml", "al");
        write(
            &root,
            FROM,
            &format!("[element]\n{}\n", row.replace("SHA", &sha("al"))),
        );
        let found = lever(&["family", "ours-v0", "--from", FROM], &root);
        assert!(
            has(&found, &format!("datasets/ours-v0.toml: {why}")),
            "{probe}: {found:?}"
        );
        assert!(!root.join("datasets/ours-v0.toml").exists(), "{probe}");
        assert!(
            !root.join("python/orbitar/families/ours-v0.toml").exists(),
            "{probe}"
        );
    }
}

#[test]
fn a_base_reached_through_a_link_is_refused() {
    use std::os::unix::fs::symlink;
    let root = tuning("family-linked-base", &sha("al"));
    write(&root, "notes/jth.toml", "url = \"u\"\n");
    fs::remove_file(root.join("datasets/jth-2.0.toml")).unwrap();
    symlink("../notes/jth.toml", root.join("datasets/jth-2.0.toml")).unwrap();
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(
            &found,
            "datasets/jth-2.0.toml is a link or no regular file; a table is a regular file"
        ),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn the_lever_writes_no_table_beside_a_faulted_snapshot_or_over_a_base_that_breaks_the_engines_rules()
 {
    let root = tuning("family-faulted", &sha("al"));
    write(&root, "datasets/junk.toml", "= nonsense");
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "datasets/junk.toml: "), "{found:?}");
    assert!(!root.join("datasets/ours-v0.toml").exists());
    let root = tuning("family-pbx-base", &sha("al"));
    write(
        &root,
        "datasets/jth-2.0.toml",
        &format!(
            "url = \"u\"\n\n[pbx]\nlayout = \"X/{{element}}.xml\"\n\n[pbx.element]\nSi = {{ sha256 = \"{}\" }}\n",
            "a".repeat(64)
        ),
    );
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(&found, "\"pbx\" keys no dataset section (lda, pbe)"),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn the_lever_refuses_a_linked_package_directory_and_writes_nothing_outside() {
    use std::os::unix::fs::symlink;
    let root = tuning("family-linked-package", &sha("al"));
    let outside = tuning("family-linked-package-outside", &sha("al"));
    fs::create_dir_all(outside.join("python/orbitar/families")).unwrap();
    fs::create_dir_all(root.join("python")).unwrap();
    symlink(outside.join("python/orbitar"), root.join("python/orbitar")).unwrap();
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(has(&found, "python/orbitar is a link"), "{found:?}");
    assert!(
        !outside
            .join("python/orbitar/families/ours-v0.toml")
            .exists()
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}

#[test]
fn the_lever_names_a_package_directory_below_a_regular_file() {
    let root = tuning("family-package-enotdir", &sha("al"));
    write(&root, "python/orbitar", "a file where a directory is\n");
    let found = lever(&["family", "ours-v0", "--from", FROM], &root);
    assert!(
        has(&found, "python/orbitar/families cannot be listed: "),
        "{found:?}"
    );
    assert!(!root.join("datasets/ours-v0.toml").exists());
}
