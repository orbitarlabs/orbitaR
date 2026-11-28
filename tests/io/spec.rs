use crate::{EMPTY_CUBE, ONE_SI, ROCK_SALT_PROBE, readme_block, with_atoms};
use orbitar::io::read_spec;
use orbitar::model::{Draft, Functional, Mixer, Probe, Smearing, Solver};

fn draft(input: &str) -> Result<Draft, String> {
    read_spec(input).map(|spec| spec.draft().clone())
}

#[test]
fn the_input_schema_example_reads_as_written() {
    let d = draft(&readme_block("Input schema")).unwrap();
    assert_eq!(d.atoms, [(14, [0.0; 3]), (14, [0.25; 3])]);
    assert_eq!((d.cutoff_ha, d.bands), (15.0, Some(16)));
    assert_eq!(
        (d.density_cutoff_ha, d.solver, d.dataset_directory.as_str()),
        (Some(30.0), Solver::Lobpcg, "datasets")
    );
    assert_eq!(d.kpoints.points().len(), 512);
    assert_eq!(d.kpoints.points()[0], [0.0625; 3]);
    assert_eq!(
        (d.functional, d.family.as_str()),
        (Some(Functional::Pbe), "jth-2.0")
    );
    assert_eq!(
        (d.smearing, d.width_ha),
        (Some(Smearing::FermiDirac), Some(0.00225))
    );
    assert_eq!(
        (d.energy_tolerance_ha, d.max_iterations, d.mixer),
        (1e-9, 100, Mixer::Pulay)
    );
    assert!(d.forces && d.stress && d.step_energies && d.discretization);
    assert_eq!(d.moments, Some(vec![2.5, -2.5]));
    assert_eq!(d.threads.map(|n| n.get()), Some(4));
    assert_eq!(d.dataset_files, [(14, "own/Si.xml".to_owned())].into());
}

fn defaults(schema: &str) -> Vec<(&str, &str, &str)> {
    schema
        .lines()
        .filter_map(|line| {
            let (key, rest) = line.split_once(" = ")?;
            let (_, default) = rest.split_once("# default ")?;
            let default = default.split([';', ':']).next()?.trim();
            Some((line, key.trim(), default))
        })
        .collect()
}

fn without(schema: &str, key: &str) -> String {
    let mut skipping = false;
    let kept: Vec<&str> = schema
        .lines()
        .filter(|line| {
            skipping =
                line.starts_with(&format!("{key} = ")) || (skipping && line.starts_with(' '));
            !skipping
        })
        .collect();
    kept.join("\n")
}

#[test]
fn every_stated_default_equals_leaving_the_key_out() {
    let schema = readme_block("Input schema");
    let found = defaults(&schema);
    assert_eq!(found.len(), 14, "{found:?}");
    for (line, key, default) in found {
        let (stated, left_out) = if key == "species" {
            (
                with_atoms(EMPTY_CUBE, &format!("species = {default}")),
                EMPTY_CUBE.to_owned(),
            )
        } else {
            (
                schema.replace(line, &format!("{key} = {default}")),
                without(&schema, key),
            )
        };
        assert_eq!(draft(&stated).unwrap(), draft(&left_out).unwrap(), "{key}");
    }
}

#[test]
fn every_listed_option_is_accepted_and_no_other() {
    let schema = readme_block("Input schema");
    let lines: Vec<&str> = schema.lines().collect();
    let mut checked = 0;
    for pair in lines.windows(2) {
        let Some(options) = pair[0].strip_prefix("# options: ") else {
            continue;
        };
        let key = pair[1].split(" = ").next().unwrap();
        for option in options.split(" | ") {
            let base = if option == "\"fixed\"" {
                without(&without(&schema, "width_ha"), "moments")
            } else {
                schema.clone()
            };
            let input = base.replace(pair[1], &format!("{key} = {option}"));
            assert!(draft(&input).is_ok(), "{key} = {option}");
        }
        let listed = options.replace('"', "").replace(" | ", ", ");
        let bogus = schema.replace(pair[1], &format!("{key} = \"bogus\""));
        let error = draft(&bogus).unwrap_err();
        assert!(
            error.ends_with(&format!("is not one of {listed}")),
            "{error}"
        );
        checked += 1;
    }
    assert_eq!(checked, 4);
}

#[test]
fn every_key_of_the_empty_cube_is_required() {
    for line in EMPTY_CUBE.lines().filter(|line| line.contains(" = ")) {
        let key = line.split(" = ").next().unwrap();
        let error = draft(&EMPTY_CUBE.replace(line, "")).unwrap_err();
        assert!(error.contains(key), "{key}: {error}");
    }
}

#[test]
fn an_unknown_table_or_key_is_rejected() {
    let schema = readme_block("Input schema");
    let table = format!("{schema}\n[magnetism]\nspin = 2");
    let key = schema.replace("count = 16", "count = 16\nextra = 1");
    for input in [table, key] {
        assert!(
            draft(&input).unwrap_err().contains("unknown field"),
            "{input}"
        );
    }
}

#[test]
fn left_out_threads_mean_every_core() {
    let schema = without(&readme_block("Input schema"), "threads");
    let cores = std::thread::available_parallelism().unwrap();
    assert_eq!(read_spec(&schema).unwrap().threads(), cores);
}

#[test]
fn zero_threads_are_refused() {
    let schema = readme_block("Input schema").replace("threads = 4", "threads = 0");
    assert_eq!(
        read_spec(&schema).unwrap_err(),
        "parallel.threads must be at least 1"
    );
}

#[test]
fn only_schema_1_is_read() {
    let input = EMPTY_CUBE.replace("schema = 1", "schema = 2");
    assert!(draft(&input).unwrap_err().starts_with("schema:"));
}

#[test]
fn kpoints_take_a_mesh_with_an_optional_shift_or_a_list() {
    let list = "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]";
    let mesh = draft(&EMPTY_CUBE.replace(list, "mesh = [2, 2, 2]")).unwrap();
    assert_eq!(mesh.kpoints.points().len(), 8);
    for bad in [
        format!("{list}\nmesh = [2, 2, 2]"),
        format!("{list}\nshift = [0.5, 0.5, 0.5]"),
        String::new(),
    ] {
        assert!(
            draft(&EMPTY_CUBE.replace(list, &bad))
                .unwrap_err()
                .starts_with("kpoints:")
        );
    }
}

#[test]
fn symmetry_may_reduce_a_mesh_unless_turned_off_and_never_a_list() {
    let list = "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]";
    let mesh = |extra: &str| draft(&EMPTY_CUBE.replace(list, &format!("mesh = [2, 2, 2]{extra}")));
    assert!(mesh("").unwrap().kpoints.reducible());
    assert!(!mesh("\nsymmetry = false").unwrap().kpoints.reducible());
    assert!(!draft(EMPTY_CUBE).unwrap().kpoints.reducible());
    assert_eq!(
        draft(&EMPTY_CUBE.replace(list, &format!("{list}\nsymmetry = true"))).unwrap_err(),
        "kpoints.symmetry: a list is used as given"
    );
}

#[test]
fn each_species_entry_needs_an_element_symbol_and_one_position() {
    let with = |atoms: &str| draft(&with_atoms(EMPTY_CUBE, atoms));
    assert_eq!(with(ONE_SI).unwrap().atoms, [(14, [0.0; 3])]);
    let og = with(&ONE_SI.replace("Si", "Og")).unwrap();
    assert_eq!(og.atoms, [(118, [0.0; 3])]);
    let unknown = with(&ONE_SI.replace("Si", "Xx")).unwrap_err();
    assert!(unknown.starts_with("structure.species"), "{unknown}");
    let unmatched = with(&ONE_SI.replace("[\"Si\"]", "[\"Si\", \"Si\"]")).unwrap_err();
    assert!(
        unmatched.starts_with("structure.positions_fractional"),
        "{unmatched}"
    );
}

#[test]
fn each_probe_term_takes_its_own_keys() {
    let ewald = ROCK_SALT_PROBE;
    let hartree = ewald.replace("term = \"ewald\"", "term = \"hartree\"");
    let wrong = [
        (
            ewald.replace("# exponents = ", "exponents = "),
            "unknown field `exponents`",
        ),
        (hartree.clone(), "missing field `exponents`"),
        (
            ewald.replace("term = \"ewald\"", "term = \"xc\""),
            "unknown field `charges`",
        ),
        (
            ewald.replace("term = \"ewald\"", "term = \"bogus\""),
            "unknown variant `bogus`",
        ),
        (
            ewald.replace("charges = [1.0, -1.0]", "charges = [1.0]"),
            "probe.positions_fractional:",
        ),
        (
            hartree.replace("# exponents = [1.5, 1.5]", "exponents = [1.5]"),
            "probe.exponents:",
        ),
    ];
    for (input, message) in wrong {
        let error = draft(&input).unwrap_err();
        assert!(error.contains(message), "{error}");
    }
}

fn xc_probe(functional: &str, points: &str) -> String {
    let probe = ROCK_SALT_PROBE;
    let start = probe.find("\n[probe]").unwrap() + 1;
    format!(
        "{}[xc]\nfunctional = \"{functional}\"\n\n[probe]\nterm = \"xc\"\npoints = {points}\n",
        &probe[..start]
    )
}

#[test]
fn an_xc_probe_reads_five_or_seven_values_per_point() {
    let five = draft(&xc_probe("pbe", "[[0.5, 0.4, 0.01, 0.0, 0.02]]")).unwrap();
    assert_eq!(
        five.probe,
        Some(Probe::Xc {
            points: vec![[0.5, 0.4, 0.01, 0.0, 0.02]],
            tau: None
        })
    );
    let seven = draft(&xc_probe(
        "r2scan",
        "[[0.5, 0.4, 0.01, 0.0, 0.02, 0.7, 0.6]]",
    ))
    .unwrap();
    assert_eq!(
        seven.probe,
        Some(Probe::Xc {
            points: vec![[0.5, 0.4, 0.01, 0.0, 0.02]],
            tau: Some(vec![[0.7, 0.6]])
        })
    );
    assert_eq!(seven.functional, Some(Functional::R2scan));
    for points in [
        "[[0.5, 0.4, 0.01, 0.0]]",
        "[[0.5, 0.4, 0.01, 0.0, 0.02, 0.7]]",
        "[[0.5, 0.4, 0.01, 0.0, 0.02], [0.5, 0.4, 0.01, 0.0, 0.02, 0.7, 0.6]]",
    ] {
        let error = draft(&xc_probe("pbe", points)).unwrap_err();
        assert!(error.starts_with("probe.points: give 5 values"), "{error}");
    }
}

fn paw_probe(functional: &str, rho_ij: &str) -> String {
    let probe = ROCK_SALT_PROBE;
    let start = probe.find("\n[probe]").unwrap() + 1;
    format!(
        "{}[xc]\nfunctional = \"{functional}\"\n\n[probe]\nterm = \"paw\"\nrho_ij = {rho_ij}\n",
        &probe[..start]
    )
}

#[test]
fn a_paw_probe_reads_one_elements_occupancies_per_spin() {
    let readme = ROCK_SALT_PROBE;
    let line = (readme.lines())
        .find_map(|l| l.strip_prefix("# rho_ij = "))
        .unwrap();
    let one = draft(&paw_probe("pbe", line.split(" #").next().unwrap().trim())).unwrap();
    assert_eq!(
        one.probe,
        Some(Probe::Paw {
            z: 6,
            rho: vec![vec![vec![1.0]]]
        })
    );
    let both = "{ Fe = [[[1.0, 0.5], [0.5, 2.0]], [[0.5, 0.0], [0.0, 0.0]]] }";
    assert_eq!(
        draft(&paw_probe("r2scan", both)).unwrap().probe,
        Some(Probe::Paw {
            z: 26,
            rho: vec![
                vec![vec![1.0, 0.5], vec![0.5, 2.0]],
                vec![vec![0.5, 0.0], vec![0.0, 0.0]]
            ]
        })
    );
}

#[test]
fn a_paw_probe_may_name_the_file_of_its_own_element_and_of_no_other() {
    let probe = paw_probe("r2scan", "{ Fe = [[[1.0]]] }");
    let with = |files: &str| draft(&format!("{probe}\n[datasets]\nfiles = {files}\n"));
    let own = with("{ Fe = \"JTH-PBE-v2.0/Fe/Fe.GGA_PBE-JTH.xml\" }").unwrap();
    assert_eq!(
        own.dataset_files.get(&26).map(String::as_str),
        Some("JTH-PBE-v2.0/Fe/Fe.GGA_PBE-JTH.xml")
    );
    for files in [
        "{ Si = \"Si.xml\" }",
        "{ Fe = \"Fe.xml\", Si = \"Si.xml\" }",
    ] {
        assert_eq!(
            with(files).unwrap_err(),
            "datasets.files: each element must be a species of the structure or the paw probe's element",
            "{files}"
        );
    }
}

#[test]
fn a_paw_probe_refuses_occupancies_that_are_not_one_elements_symmetric_matrices() {
    let shape = "probe.rho_ij: give one or two finite symmetric matrices (ρ↑, ρ↓) of one size";
    for (rho_ij, message) in [
        ("{}", "probe.rho_ij: give the occupancies of one element"),
        (
            "{ C = [[[1.0]]], Si = [[[1.0]]] }",
            "probe.rho_ij: give the occupancies of one element",
        ),
        (
            "{ Q = [[[1.0]]] }",
            "probe.rho_ij: \"Q\" is not an element symbol",
        ),
        ("{ C = [] }", shape),
        ("{ C = [[]] }", shape),
        ("{ C = [[[1.0, 0.0]]] }", shape),
        ("{ C = [[[1.0, 0.5], [0.4, 1.0]]] }", shape),
        ("{ C = [[[1.0]], [[1.0]], [[1.0]]] }", shape),
        ("{ C = [[[1.0]], [[1.0, 0.0], [0.0, 1.0]]] }", shape),
        ("{ C = [[[inf]]] }", shape),
    ] {
        assert_eq!(
            draft(&paw_probe("pbe", rho_ij)).unwrap_err(),
            message,
            "{rho_ij}"
        );
    }
    let without = paw_probe("pbe", "{ C = [[[1.0]]] }").replace("[xc]\nfunctional = \"pbe\"\n", "");
    assert_eq!(
        draft(&without).unwrap_err(),
        "xc.functional: a paw probe needs a functional"
    );
    let ewald = ROCK_SALT_PROBE.replace("# rho_ij = ", "rho_ij = ");
    assert!(
        draft(&ewald)
            .unwrap_err()
            .contains("unknown field `rho_ij`")
    );
}

#[test]
fn an_r2scan_probe_without_tau_is_refused_by_key() {
    let error = draft(&xc_probe("r2scan", "[[0.5, 0.4, 0.01, 0.0, 0.02]]")).unwrap_err();
    assert!(error.starts_with("probe.points: r2scan reads τ"), "{error}");
}

#[test]
fn a_dataset_file_needs_a_species_and_a_plain_relative_path() {
    let schema = readme_block("Input schema");
    let with = |files: &str| draft(&schema.replace("{ Si = \"own/Si.xml\" }", files));
    assert!(with("{ Si = \"Si.xml\" }").is_ok());
    let refused = [
        (
            "{ Xx = \"Si.xml\" }",
            "datasets.files: \"Xx\" is not an element symbol",
        ),
        (
            "{ Al = \"Al.xml\" }",
            "datasets.files: each element must be a species of the structure or the paw probe's element",
        ),
    ];
    for (files, want) in refused {
        assert_eq!(with(files).unwrap_err(), want, "{files}");
    }
    for path in [
        "",
        "/abs/Si.xml",
        "../Si.xml",
        "own/../Si.xml",
        "./Si.xml",
        "own/./Si.xml",
        "own//Si.xml",
        "own/",
    ] {
        let error = with(&format!("{{ Si = {path:?} }}")).unwrap_err();
        assert_eq!(
            error,
            format!(
                "datasets.files: {path:?} must be relative to datasets.directory, with no `.` or `..`"
            )
        );
    }
}

#[test]
fn a_family_is_a_file_name_in_the_dataset_directory() {
    let first = with_atoms(EMPTY_CUBE, ONE_SI);
    let family = |name: &str| draft(&format!("{first}\n[datasets]\nfamily = {name:?}\n"));
    assert_eq!(family("orbitaR-PBE-v0").unwrap().family, "orbitaR-PBE-v0");
    for name in ["../x", "a/b", "", "."] {
        assert_eq!(
            family(name).unwrap_err(),
            format!(
                "datasets.family: {name:?} must be a file name: datasets.directory holds <family>.toml"
            )
        );
    }
}

const TUNING_CELL: &str = "schema = 1\n\n[structure]\ncell_bohr = [[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]]\nspecies = [\"Si\", \"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]\n\n[basis]\ncutoff_ha = 10.0\n\n[kpoints]\nmesh = [1, 1, 1]\n";

#[expect(
    clippy::disallowed_methods,
    reason = "the test reads each tuning input that a tuning directory holds"
)]
fn tuning_inputs_the_engine_refuses(tuning: &std::path::Path) -> Vec<String> {
    let listed = |dir: &std::path::Path| {
        let mut paths: Vec<std::path::PathBuf> = (std::fs::read_dir(dir).unwrap())
            .map(|entry| entry.unwrap().path())
            .collect();
        paths.sort();
        paths
    };
    let mut refused = Vec::new();
    for setting in listed(tuning) {
        let inputs = listed(&setting).into_iter();
        let toml = |p: &std::path::PathBuf| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().ends_with(".toml"))
        };
        for file in inputs.filter(toml) {
            if let Err(e) = read_spec(&std::fs::read_to_string(&file).unwrap()) {
                let name = file.strip_prefix(tuning).unwrap().display().to_string();
                refused.push(format!("{name}: {e}"));
            }
        }
    }
    refused
}

#[test]
fn every_tuning_input_in_the_repository_is_an_input_that_the_engine_reads() {
    let refused = tuning_inputs_the_engine_refuses(std::path::Path::new("tuning"));
    assert!(refused.is_empty(), "{refused:?}");
}

#[test]
fn a_tuning_input_with_a_two_number_lattice_row_or_no_basis_is_one_that_the_engine_refuses() {
    let short_row = TUNING_CELL.replace("[[0.0, 5.1, 5.1],", "[[5.1, 5.1],");
    let no_basis = TUNING_CELL.replace("\n[basis]\ncutoff_ha = 10.0\n", "");
    let root = super::run::directory(
        "tuning-refused",
        &[
            ("mixer/.toml", no_basis.clone()),
            ("mixer/cell.toml", TUNING_CELL.to_owned()),
            ("mixer/no_basis.toml", no_basis),
            ("mixer/short_row.toml", short_row),
            ("mixer/slab.py", "print()\n".to_owned()),
        ],
    );
    let refused = tuning_inputs_the_engine_refuses(&root);
    let names: Vec<&str> = refused
        .iter()
        .map(|r| r.split(':').next().unwrap())
        .collect();
    assert_eq!(
        names,
        ["mixer/.toml", "mixer/no_basis.toml", "mixer/short_row.toml"],
        "{refused:?}"
    );
}
