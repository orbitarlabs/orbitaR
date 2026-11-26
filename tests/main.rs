use crate::{EMPTY_CUBE, EMPTY_CUBE_OUTCOME, ONE_SI, readme_block, readme_section, with_atoms};
use serde_json::Value;
use std::io::Write;

#[expect(
    clippy::disallowed_macros,
    clippy::disallowed_types,
    reason = "these tests run the built CLI as a user does"
)]
fn orbitar(args: &[&str], input: &str) -> (i32, String, String) {
    use std::process::{Command, Stdio};
    let mut child = Command::new(env!("CARGO_BIN_EXE_orbitar"))
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    let text = |bytes: Vec<u8>| String::from_utf8(bytes).unwrap();
    (
        out.status.code().unwrap_or(-1),
        text(out.stdout),
        text(out.stderr),
    )
}

fn run(input: &str) -> (i32, String, String) {
    orbitar(&["run", "/dev/stdin"], input)
}

#[test]
fn the_empty_cube_prints_its_outcome() {
    let (code, stdout, _) = run(EMPTY_CUBE);
    assert_eq!(code, 0);
    let got: Value = serde_json::from_str(&stdout).unwrap();
    let want: Value = serde_json::from_str(EMPTY_CUBE_OUTCOME).unwrap();
    assert_eq!(got, want);
}

#[test]
fn the_exit_code_tells_invalid_input_from_unsupported_physics_and_no_convergence() {
    let (invalid, unsupported, unconverged) = (2, 3, 4);
    for (input, want, cause) in [
        (
            EMPTY_CUBE.replace("schema = 1", "schema = 2"),
            invalid,
            "schema:",
        ),
        (
            with_atoms(EMPTY_CUBE, ONE_SI) + "\n[scf]\nmixer = \"broyden\"\n",
            unsupported,
            "scf.mixer:",
        ),
    ] {
        let (code, stdout, stderr) = run(&input);
        assert_eq!((code, stdout.as_str()), (want, ""), "{stderr}");
        assert!(stderr.starts_with(cause), "{stderr}");
    }
    let input = "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\
                 species = [\"C\"]\npositions_fractional = [[0.1, 0.2, 0.3]]\n\n[basis]\ncutoff_ha = 3.0\n\n\
                 [kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[scf]\nmax_iterations = 2\n\n[outputs]\nstep_energies = true\n";
    let root = beside("unconverged", input);
    let (code, stdout, stderr) = orbitar(&["run", root.join("input.toml").to_str().unwrap()], "");
    assert_eq!((code, stdout.as_str()), (unconverged, ""), "{stderr}");
    let lines: Vec<&str> = stderr.lines().collect();
    assert_eq!(lines.len(), 3, "{stderr}");
    assert!(lines[2].starts_with("scf.max_iterations:"), "{stderr}");
}

#[test]
fn step_energies_put_each_scf_step_and_its_free_energy_on_stderr_as_the_readme_writes_them() {
    let schema = readme_block("Input schema");
    let (_, form) = schema.split_once("each SCF step on stderr: ").unwrap();
    let form: Vec<&str> = form.lines().next().unwrap().split(' ').collect();
    let input = "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\
                 species = [\"C\"]\npositions_fractional = [[0.1, 0.2, 0.3]]\n\n[basis]\ncutoff_ha = 3.0\n\n\
                 [kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[outputs]\nstep_energies = true\n";
    let root = beside(
        "steps",
        &input.replace("step_energies = true", "step_energies = false"),
    );
    let (code, quiet, stderr) = orbitar(&["run", root.join("input.toml").to_str().unwrap()], "");
    assert_eq!((code, stderr.as_str()), (0, ""));
    let root = beside("steps", input);
    let (code, stdout, stderr) = orbitar(&["run", root.join("input.toml").to_str().unwrap()], "");
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, quiet, "step energies leave stdout byte for byte");
    let outcome: Value = serde_json::from_str(&stdout).unwrap();
    let steps: Vec<(u64, f64)> = stderr
        .lines()
        .map(|line| {
            let words: Vec<&str> = line.split(' ').collect();
            assert_eq!(words.len(), form.len(), "{line}");
            let mut step = (0, f64::NAN);
            for (word, slot) in words.iter().zip(&form) {
                match *slot {
                    "<n>" => step.0 = word.parse().unwrap(),
                    "<F>" => step.1 = word.parse().unwrap(),
                    literal => assert_eq!(*word, literal, "{line}"),
                }
            }
            step
        })
        .collect();
    let numbers: Vec<u64> = steps.iter().map(|s| s.0).collect();
    let count = outcome["scf_steps"].as_u64().unwrap();
    assert_eq!(numbers, (1..=count).collect::<Vec<_>>());
    assert_eq!(
        steps.last().map(|s| s.1),
        outcome["free_energy_ha"].as_f64()
    );
}

#[expect(
    clippy::disallowed_methods,
    reason = "the test lays out an input file and its dataset as a user does"
)]
fn beside(name: &str, input: &str) -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("orbitar-cli-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let dataset = root.join("datasets/synthetic/C.xml");
    std::fs::create_dir_all(dataset.parent().unwrap()).unwrap();
    std::fs::write(&dataset, crate::io::paw_xml::xml(&crate::synthetic_draft())).unwrap();
    let files = "\n[datasets]\nfiles = { C = \"synthetic/C.xml\" }\n";
    std::fs::write(root.join("input.toml"), format!("{input}{files}")).unwrap();
    root
}

fn license_links() -> Vec<(String, String)> {
    (readme_section("License").lines())
        .filter_map(|line| {
            let (text, rest) = line.strip_prefix("- [")?.split_once("](")?;
            let (path, _) = rest.split_once(')')?;
            Some((text.to_owned(), path.to_owned()))
        })
        .collect()
}

#[expect(
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    reason = "the tests read the license files that README links and the container page's runs"
)]
fn repository_file(path: &str) -> String {
    std::fs::read_to_string(std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(path))
        .unwrap_or_else(|e| panic!("{path}: {e}"))
}

#[test]
#[expect(
    clippy::disallowed_macros,
    reason = "the test reads the license that Cargo.toml gives the CLI's package"
)]
fn the_first_readme_license_link_is_the_license_file_of_the_package() {
    let links = license_links();
    let (text, path) = links.first().expect("README ## License links a license");
    assert_eq!(
        (text.as_str(), path.as_str()),
        ("PolyForm Noncommercial 1.0.0", "LICENSE")
    );
    assert_eq!(env!("CARGO_PKG_LICENSE"), "PolyForm-Noncommercial-1.0.0");
    assert!(repository_file(path).starts_with("# PolyForm Noncommercial License 1.0.0\n"));
}

#[test]
fn each_gpl_readme_license_link_is_a_gpl_3_file_and_the_dataset_one_has_its_notice() {
    let gpl: Vec<String> = (license_links().into_iter())
        .filter(|(text, _)| text == "GPL-3.0")
        .map(|(_, path)| path)
        .collect();
    assert!(
        gpl.contains(&"datasets/orbitaR-PBE-v0/LICENSE".to_owned()),
        "{gpl:?}"
    );
    for path in gpl {
        let text = repository_file(&path);
        assert!(
            text.contains("GNU GENERAL PUBLIC LICENSE") && text.contains("Version 3, 29 June 2007"),
            "{path}"
        );
        if path.starts_with("datasets/") {
            let notice = std::path::Path::new(&path).with_file_name("NOTICE");
            let notice = repository_file(notice.to_str().unwrap());
            assert!(notice.contains("are GPL-3.0 too"), "{path}");
        }
    }
}

#[test]
#[expect(
    clippy::disallowed_macros,
    reason = "the test reads the commit that the build of the CLI saw"
)]
fn version_prints_the_cargo_version_and_the_build_commit() {
    let (code, stdout, stderr) = orbitar(&["--version"], "");
    assert_eq!((code, stderr.as_str()), (0, ""));
    let commit = option_env!("ORBITAR_COMMIT").unwrap_or("unknown");
    assert_eq!(stdout, format!("orbitar 0.0.1 (commit {commit})\n"));
}

#[test]
fn anything_but_run_and_one_path_is_a_usage_error() {
    for args in [
        &[][..],
        &["run"],
        &["check", "/dev/stdin"],
        &["run", "a", "b"],
    ] {
        let (code, _, stderr) = orbitar(args, "");
        assert_eq!(code, 2, "{args:?}");
        assert!(stderr.contains("usage"), "{args:?}");
    }
}

fn stand_in_table(files: &[(&str, Vec<u8>)]) -> String {
    use sha2::{Digest, Sha256};
    let hex = |bytes: &[u8]| -> String {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect()
    };
    let mut table = "url = \"https://example.invalid/pseudos\"\n\n[pbe]\n\
                     layout = \"JTH-PBE-v2.0/{element}/{element}.GGA_PBE-JTH.xml\"\n\n[pbe.element]\n"
        .to_owned();
    for ((element, bytes), hint) in files.iter().zip([10.0, 15.0]) {
        table += &format!(
            "{element} = {{ sha256 = \"{}\", cutoff_hint_ha = {hint:?} }}\n",
            hex(bytes)
        );
    }
    table += &format!(
        "Fe = {{ sha256 = \"{}\", cutoff_hint_ha = 12.0 }}\n",
        hex(b"Fe")
    );
    table += &format!("La = {{ sha256 = \"{}\" }}\n", hex(b"La"));
    table +=
        "\n[lda]\nlayout = \"JTH-LDA-v2.0/{element}/{element}.LDA_PW-JTH.xml\"\n\n[lda.element]\n";
    for (element, hint) in [("Si", 30.0), ("C", 40.0)] {
        table += &format!(
            "{element} = {{ sha256 = \"{}\", cutoff_hint_ha = {hint:?} }}\n",
            hex(element.as_bytes())
        );
    }
    table + &format!("Fe = {{ sha256 = \"{}\" }}\n", hex(b"lda Fe"))
}

fn python(name: &str, script: &str, element: &str) -> (String, std::path::PathBuf) {
    python_with(name, script, element, true)
}

#[expect(
    clippy::disallowed_macros,
    clippy::disallowed_methods,
    clippy::disallowed_types,
    reason = "these tests run README's Python examples, which start the built CLI, as a user does"
)]
fn python_with(
    name: &str,
    script: &str,
    element: &str,
    stand_in: bool,
) -> (String, std::path::PathBuf) {
    use std::process::Command;
    let root = std::env::temp_dir().join(format!("orbitar-python-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let xml = |draft| crate::io::paw_xml::xml(&draft).into_bytes();
    let files = [
        ("Si", xml(crate::synthetic_silicon())),
        ("C", xml(crate::synthetic_draft())),
    ];
    let (_, bytes) = files.iter().find(|(e, _)| *e == element).unwrap();
    let dataset = root.join(format!("datasets/synthetic/{element}.xml"));
    std::fs::create_dir_all(dataset.parent().unwrap()).unwrap();
    std::fs::write(&dataset, bytes).unwrap();
    let site = root.join("site");
    let package = site.join("stand_in");
    for (element, bytes) in &files {
        let path = package.join(format!("JTH-PBE-v2.0/{element}/{element}.GGA_PBE-JTH.xml"));
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, bytes).unwrap();
    }
    std::fs::write(package.join("jth-2.0.toml"), stand_in_table(&files)).unwrap();
    std::fs::write(package.join("__init__.py"), "").unwrap();
    let info = site.join("stand_in-0.dist-info");
    std::fs::create_dir_all(&info).unwrap();
    std::fs::write(
        info.join("METADATA"),
        "Metadata-Version: 2.1\nName: stand-in\nVersion: 0\n",
    )
    .unwrap();
    std::fs::write(
        info.join("entry_points.txt"),
        "[orbitar.datasets]\njth-2.0 = stand_in\n",
    )
    .unwrap();
    let wheel = std::env::var_os("ORBITAR_PYTHON").map(std::path::PathBuf::from);
    let (python, bin, source) = match &wheel {
        Some(python) => (python.clone(), python.parent().unwrap().to_owned(), None),
        None => (
            "python3".into(),
            std::path::Path::new(env!("CARGO_BIN_EXE_orbitar"))
                .parent()
                .unwrap()
                .to_owned(),
            Some(concat!(env!("CARGO_MANIFEST_DIR"), "/python")),
        ),
    };
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").unwrap_or_default()
    );
    let prelude = source.map_or_else(
        || "import sys; sys.dont_write_bytecode = True\n".to_owned(),
        |dir| {
            let mut paths = vec![dir.to_owned()];
            if stand_in {
                paths.push(site.display().to_string());
            }
            format!("import sys; sys.path[:0] = {paths:?}; sys.dont_write_bytecode = True\n")
        },
    );
    let out = Command::new(&python)
        .args(["-I", "-c", &(prelude + script)])
        .current_dir(&root)
        .env("PATH", path)
        .output()
        .expect("python3 runs README's Python examples; CI's check job installs it");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "{stderr}");
    (String::from_utf8(out.stdout).unwrap(), root)
}

#[expect(
    clippy::disallowed_methods,
    reason = "the venv of `cargo xtask wheel` names the installed data package"
)]
fn package_directory(root: &std::path::Path) -> String {
    match std::env::var_os("ORBITAR_PYTHON") {
        Some(python) => {
            let venv = std::path::Path::new(&python)
                .parent()
                .unwrap()
                .parent()
                .unwrap();
            venv.join("lib/python3.12/site-packages/orbitar_jth")
                .display()
                .to_string()
        }
        None => root.join("site/stand_in").display().to_string(),
    }
}

const READ: &str = "\nimport json, pathlib, tomllib\n\
    written = tomllib.loads(pathlib.Path('input.toml').read_text())\n\
    print(json.dumps({'directory': written.get('datasets', {}).get('directory'), \
    'cutoff_ha': written['basis']['cutoff_ha'], \
    'read': json.loads(pathlib.Path('outcome.json').read_text().splitlines()[-1])['datasets']}))";

#[test]
fn the_readme_ase_example_reads_the_family_files_of_the_installed_data_package() {
    let (stdout, root) = python("ase-package", &(readme_block("ASE") + READ), "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(got["directory"], package_directory(&root), "{got}");
    assert_eq!(
        got["read"]["Si"]["path"], "JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml",
        "{got}"
    );
}

#[test]
fn a_dataset_directory_that_the_caller_gives_wins_over_a_run_directory_table_and_the_data_package()
{
    let script = "import os, shutil\nfrom ase.build import bulk\nfrom orbitar.ase import Orbitar\n\
        shutil.copytree('site/stand_in', 'mine')\n\
        shutil.copytree('site/stand_in', 'datasets', dirs_exist_ok=True)\n\
        atoms = bulk('Si', 'diamond', a=5.43)\n\
        atoms.calc = Orbitar(datasets=os.path.abspath('mine'), kpoints={'mesh': [1, 1, 1]})\n\
        atoms.get_potential_energy()"
        .to_owned()
        + READ;
    let (stdout, root) = python("ase-given", &script, "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    let mine = root.file_name().unwrap().to_string_lossy().into_owned() + "/mine";
    assert!(got["directory"].as_str().unwrap().ends_with(&mine), "{got}");
    assert_eq!(
        got["read"]["Si"]["path"], "JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml",
        "{got}"
    );
}

#[test]
fn a_dataset_directory_that_the_caller_gives_is_read_without_probing_the_run_directory() {
    let script = "import os, shutil\nfrom ase.build import bulk\nfrom orbitar.ase import Orbitar\n\
        shutil.copytree('site/stand_in', 'mine')\n\
        os.chmod('datasets', 0)\n\
        try:\n    atoms = bulk('Si', 'diamond', a=5.43)\n    \
        atoms.calc = Orbitar(datasets=os.path.abspath('mine'), kpoints={'mesh': [1, 1, 1]})\n    \
        atoms.get_potential_energy()\nfinally:\n    os.chmod('datasets', 0o755)"
        .to_owned()
        + READ;
    let (stdout, root) = python("ase-given-unreadable", &script, "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    let mine = root.file_name().unwrap().to_string_lossy().into_owned() + "/mine";
    assert!(got["directory"].as_str().unwrap().ends_with(&mine), "{got}");
}

#[test]
fn the_maker_rule_of_orbitar_ase_picks_for_each_functional_the_row_that_family_resolve_picks() {
    let words: Vec<&str> = (orbitar::model::Functional::ALL.iter())
        .map(|row| row.word)
        .collect();
    let script = format!(
        "import json, pathlib\nfrom orbitar.ase import family_rows\nhere = pathlib.Path('.')\n\
         print(json.dumps({{xc: family_rows({{'xc': xc, 'datasets': 'site/stand_in'}}, here, ['orbitar'])['Si']['path'] \
         for xc in {words:?}}}))"
    );
    let (stdout, root) = python("maker", &script, "Si");
    let got: Value = serde_json::from_str(stdout.trim()).unwrap();
    let family = orbitar::io::Family::read(&root.join("site/stand_in"), "jth-2.0").unwrap();
    for row in orbitar::model::Functional::ALL {
        let resolved = family.resolve(row.functional, 14).unwrap();
        assert_eq!(got[row.word], resolved.path.as_str(), "{}: {got}", row.word);
    }
    assert_ne!(got["lda"], got["pbe"], "{got}");
}

#[test]
fn a_family_table_in_the_run_directory_wins_over_the_data_package() {
    let script = "import shutil\nfrom ase.build import bulk\nfrom orbitar.ase import Orbitar\n\
        shutil.copytree('site/stand_in', 'datasets', dirs_exist_ok=True)\n\
        atoms = bulk('Si', 'diamond', a=5.43)\n\
        atoms.calc = Orbitar(kpoints={'mesh': [1, 1, 1]})\n\
        atoms.get_potential_energy()"
        .to_owned()
        + READ;
    let (stdout, _) = python("ase-run-directory", &script, "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert_eq!(got["directory"], Value::Null, "{got}");
    assert_eq!(
        got["read"]["Si"]["path"], "JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml",
        "{got}"
    );
}

fn writing(atoms: &str, parameters: &str) -> String {
    format!(
        "import json, pathlib, tomllib\nfrom ase import Atoms\nfrom ase.build import bulk\n\
         from orbitar.ase import Orbitar\natoms = {atoms}\natoms.calc = Orbitar({parameters})\n\
         try:\n    atoms.calc.write_inputfiles(atoms, ['energy'])\n\
         except ValueError as refusal:\n    print(json.dumps(str(refusal)))\nelse:\n    \
         print(json.dumps(tomllib.loads(pathlib.Path('input.toml').read_text())))"
    )
}

fn written(name: &str, atoms: &str, parameters: &str) -> Value {
    let (stdout, _) = python(name, &writing(atoms, parameters), "Si");
    serde_json::from_str(stdout.trim()).unwrap()
}

const SI: &str = "bulk('Si', 'diamond', a=5.43)";
const SIC: &str = "bulk('SiC', 'zincblende', a=4.36)";
const FE: &str = "bulk('Fe', 'bcc', a=2.87)";

#[test]
fn without_cutoff_ha_a_pbe_calculator_takes_the_largest_pbe_hint_of_the_elements() {
    assert_eq!(written("cutoff-sic", SIC, "")["basis"]["cutoff_ha"], 15.0);
    assert_eq!(written("cutoff-si", SI, "")["basis"]["cutoff_ha"], 10.0);
}

#[test]
fn without_cutoff_ha_an_lda_calculator_takes_the_hints_of_the_lda_rows() {
    let input = written("cutoff-lda-sic", SIC, "xc='lda'");
    assert_eq!(input["basis"]["cutoff_ha"], 40.0);
}

#[test]
fn without_cutoff_ha_an_lda_element_whose_lda_row_has_no_hint_is_refused_though_its_pbe_row_has_one()
 {
    assert_eq!(
        written("cutoff-lda-fe", FE, "xc='lda'"),
        "Orbitar: the family jth-2.0 gives no lda cutoff hint (xc lda) for ['Fe']; pass cutoff_ha"
    );
    assert_eq!(written("cutoff-pbe-fe", FE, "")["basis"]["cutoff_ha"], 12.0);
}

#[test]
fn a_cutoff_ha_that_the_caller_gives_wins_over_the_hint() {
    let input = written("cutoff-given", SIC, "cutoff_ha=12.5");
    assert_eq!(input["basis"]["cutoff_ha"], 12.5);
}

#[test]
fn without_cutoff_ha_an_element_with_no_hint_is_refused_by_name() {
    let la = "Atoms('LaSi', cell=[4.0, 4.0, 4.0], scaled_positions=[[0, 0, 0], [0.5, 0.5, 0.5]], pbc=True)";
    assert_eq!(
        written("cutoff-la", la, ""),
        "Orbitar: the family jth-2.0 gives no pbe cutoff hint (xc pbe) for ['La']; pass cutoff_ha"
    );
}

#[test]
fn without_cutoff_ha_an_element_that_dataset_files_names_is_refused_by_name() {
    assert_eq!(
        written("cutoff-own", SI, "dataset_files={'Si': 'synthetic/Si.xml'}"),
        "Orbitar: dataset_files names ['Si'], so the family gives no cutoff hint; pass cutoff_ha"
    );
}

#[test]
fn without_cutoff_ha_a_cell_with_no_atoms_is_refused_before_any_table_is_read() {
    let (stdout, _) = python_with(
        "cutoff-empty",
        &writing("Atoms(cell=[4.0, 4.0, 4.0], pbc=True)", "bands=4"),
        "Si",
        false,
    );
    assert_eq!(
        stdout.trim(),
        "\"Orbitar: the cell holds no atoms, so no element gives a cutoff hint; pass cutoff_ha\""
    );
}

#[test]
fn an_empty_dataset_files_keeps_the_data_package() {
    let input = written("files-empty", SI, "dataset_files={}");
    assert!(input["datasets"].get("files").is_none(), "{input}");
    assert!(
        input["datasets"]["directory"]
            .as_str()
            .unwrap()
            .ends_with("site/stand_in"),
        "{input}"
    );
}

#[test]
fn a_run_with_no_dataset_directory_names_the_missing_provider_and_the_remedies() {
    let (stdout, _) = python_with("no-provider", &writing(SI, "cutoff_ha=10.0"), "Si", false);
    assert_eq!(
        stdout.trim(),
        "\"Orbitar: no dataset directory for family jth-2.0: no datasets= given, no datasets/jth-2.0.toml beside \
         the input, and no installed data package (entry point group orbitar.datasets); pip install orbitar-jth, \
         pass datasets=<directory>, or run `python -m orbitar.datasets fetch datasets` beside the input\""
    );
}

#[test]
fn without_kpoints_the_calculator_takes_the_gamma_centered_mesh_of_ase_at_its_default_density() {
    let si = written("kpoints-si", SI, "");
    assert_eq!(si["kpoints"], serde_json::json!({"mesh": [8, 8, 8]}));
    let slab = "Atoms('Si2', cell=[4.0, 4.0, 12.0], scaled_positions=[[0, 0, 0], [0.5, 0.5, 0.5]], pbc=True)";
    let long = written("kpoints-long", slab, "");
    assert_eq!(long["kpoints"], serde_json::json!({"mesh": [6, 6, 2]}));
}

#[test]
fn kpoints_that_the_caller_gives_win_over_the_default_mesh() {
    let input = written(
        "kpoints-given",
        SI,
        "kpoints={'mesh': [3, 3, 3], 'shift': [0.5, 0.5, 0.5]}",
    );
    assert_eq!(
        input["kpoints"],
        serde_json::json!({"mesh": [3, 3, 3], "shift": [0.5, 0.5, 0.5]})
    );
}

#[test]
fn an_r2scan_calculator_with_no_other_parameter_takes_the_pbe_hint_the_mesh_and_the_data_package() {
    let input = written("r2scan-defaults", SI, "xc='r2scan'");
    assert_eq!(input["xc"]["functional"], "r2scan");
    assert_eq!(input["basis"]["cutoff_ha"], 10.0);
    assert_eq!(input["kpoints"]["mesh"], serde_json::json!([8, 8, 8]));
    assert!(
        input["datasets"]["directory"]
            .as_str()
            .unwrap()
            .ends_with("site/stand_in"),
        "{input}"
    );
}

#[test]
fn the_ase_calculator_writes_dataset_files_and_the_outcome_names_that_file() {
    let script = "import json, os, pathlib\nfrom ase.build import bulk\nfrom orbitar.ase import Orbitar\n\
        os.renames('datasets/synthetic/Si.xml', 'datasets/own/Si.xml')\n\
        atoms = bulk('Si', 'diamond', a=5.43)\n\
        atoms.calc = Orbitar(cutoff_ha=10.0, kpoints={'mesh': [1, 1, 1]}, dataset_files={'Si': 'own/Si.xml'})\n\
        atoms.get_potential_energy()\n\
        print(json.dumps(json.loads(pathlib.Path('outcome.json').read_text())['datasets']))";
    let (stdout, root) = python("ase-files", script, "Si");
    let got: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(got["Si"]["path"], "own/Si.xml");
    assert!(read(&root.join("input.toml")).contains("files = {\"Si\" = \"own/Si.xml\"}"));
}

#[test]
fn the_ase_profile_version_is_the_version_line_of_the_orbitar_on_path() {
    let script = "from orbitar.ase import OrbitarProfile\nprint(OrbitarProfile().version())";
    let (stdout, _) = python("ase-version", script, "Si");
    let (code, line, stderr) = orbitar(&["--version"], "");
    assert_eq!(code, 0, "{stderr}");
    assert_eq!(stdout, line.strip_prefix("orbitar ").unwrap());
}

#[test]
fn the_readme_ase_example_gives_f_the_forces_and_the_stress_of_orbitar_run_in_ev_and_angstrom() {
    let check = "\natoms.rattle(stdev=0.02, seed=1)\n\
                 atoms.calc = Orbitar(**{**atoms.calc.parameters, 'smearing': ('fermi-dirac', 0.005)})\n\
                 energy = atoms.get_potential_energy()\n\
                 forces = atoms.get_forces()\nstress = atoms.get_stress()\n\
                 import json\nfrom ase import units\nprint(json.dumps({'energy': energy, 'forces': forces.tolist(), \
                 'stress': stress.tolist(), 'ha': units.Hartree, 'bohr': units.Bohr}))";
    let (stdout, root) = python("ase", &(readme_block("ASE") + check), "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    let numbers = |v: &Value| -> Vec<f64> {
        let flat = |v: &Value| {
            v.as_array()
                .unwrap()
                .iter()
                .map(|x| x.as_f64().unwrap())
                .collect::<Vec<_>>()
        };
        v.as_array()
            .unwrap()
            .iter()
            .flat_map(|row| {
                row.as_array()
                    .map_or_else(|| vec![row.as_f64().unwrap()], |_| flat(row))
            })
            .collect()
    };
    let input = read(&root.join("input.toml"));
    assert!(input.contains("[outputs]\nstress = true\n"), "{input}");
    let input = input.replace("[outputs]\n", "[outputs]\nforces = true\n");
    let (code, json, stderr) = run(&input);
    assert_eq!(code, 0, "{stderr}");
    let outcome: Value = serde_json::from_str(&json).unwrap();
    let (ha, bohr) = (got["ha"].as_f64().unwrap(), got["bohr"].as_f64().unwrap());
    assert_eq!(
        got["energy"].as_f64().unwrap(),
        outcome["free_energy_ha"].as_f64().unwrap() * ha
    );
    let forces: Vec<f64> = numbers(&outcome["forces_ha_bohr"])
        .iter()
        .map(|f| f * (ha / bohr))
        .collect();
    assert_eq!(numbers(&got["forces"]), forces);
    let sigma = numbers(&outcome["stress_ha_bohr3"]);
    let voigt: Vec<f64> = [0, 4, 8, 5, 2, 1]
        .iter()
        .map(|&i| sigma[i] * ha / bohr.powi(3))
        .collect();
    let stress = numbers(&got["stress"]);
    assert_eq!(stress.len(), 6);
    for (s, v) in stress.iter().zip(&voigt) {
        assert!((s - v).abs() <= 1e-14 * v.abs(), "{stress:?} {voigt:?}");
    }
    assert!(
        voigt[3..].iter().all(|s| s.abs() > 1e-6),
        "the shear terms are tested: {voigt:?}"
    );
}

#[test]
fn the_file_and_the_socket_calculators_give_one_stress_at_one_geometry() {
    let check = format!(
        "\nimport json\nwith calc.socketio(unixsocket=\"orbitar-stress-{}\") as socket:\n    \
         socket_stress = socket.get_stress(atoms)\n\
         print(json.dumps({{'file': stress.tolist(), 'socket': socket_stress.tolist()}}))",
        std::process::id()
    );
    let script = readme_block("ASE") + "\ncalc = atoms.calc" + &check;
    let (stdout, _) = python("stress", &script, "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    let (file, socket) = (
        got["file"].as_array().unwrap(),
        got["socket"].as_array().unwrap(),
    );
    assert_eq!(file.len(), 6);
    for (f, s) in file.iter().zip(socket) {
        let (f, s) = (f.as_f64().unwrap(), s.as_f64().unwrap());
        assert!((f - s).abs() <= 1e-12 * f.abs().max(1e-3), "{got}");
    }
}

const RELAXATION: &str = r#"
from ase import Atoms
from ase.optimize import BFGS
from ase.units import Bohr
from orbitar.ase import Orbitar

cell = [[0.0, 5.13 * Bohr, 5.13 * Bohr], [5.13 * Bohr, 0.0, 5.13 * Bohr], [5.13 * Bohr, 5.13 * Bohr, 0.0]]
atoms = Atoms("Si2", cell=cell, scaled_positions=[[0.01, -0.01, 0.0], [0.25, 0.26, 0.25]], pbc=True)
calc = Orbitar(cutoff_ha=10.0, kpoints={"mesh": [2, 2, 2]}, smearing=("fermi-dirac", 0.005))
with calc.socketio(unixsocket="{socket}") as socket:
    atoms.calc = socket
    BFGS(atoms).run(fmax=0.01)
"#;

#[test]
fn a_bfgs_relaxation_reaches_fmax_over_the_socket() {
    let check = "\nimport json\nprint(json.dumps({'fmax': float(abs(atoms.get_forces()).max())}))";
    let socket = format!("orbitar-test-{}", std::process::id());
    let script = RELAXATION.replace("{socket}", &socket) + check;
    let (stdout, root) = python("relaxation", &script, "Si");
    let got: Value = serde_json::from_str(stdout.lines().last().unwrap()).unwrap();
    assert!(got["fmax"].as_f64().unwrap() < 0.01, "{got}");
    let outcomes = read(&root.join("outcome.json"));
    assert!(outcomes.lines().count() > 2, "{outcomes}");
    assert!(
        outcomes.lines().all(|l| l.contains("\"scf_steps\":")),
        "{outcomes}"
    );
}

#[expect(
    clippy::disallowed_methods,
    reason = "the tests read what the calculator wrote"
)]
fn read(path: &std::path::Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn a_four_word_command_other_than_ipi_unix_prints_the_usage() {
    for args in [
        ["ipi", "--tcp", "s", "input.toml"],
        ["run", "--unix", "s", "input.toml"],
    ] {
        let (code, stdout, stderr) = orbitar(&args, "");
        assert_eq!(code, 2, "{args:?}: {stderr}");
        assert!(
            stdout.is_empty() && stderr.starts_with("usage: orbitar run"),
            "{stderr}"
        );
    }
}

#[test]
fn the_dataset_fetch_downloads_what_orbitar_datasets_names_and_keeps_only_files_that_match() {
    let args = ["fetch", "datasets", "Si", "O"];
    let script = format!(
        r##"
import contextlib, hashlib, io, json, os, pathlib
from orbitar import datasets
args = {args:?}
src = pathlib.Path("source")
for path, text in [("JTH-PBE-v2.0/Si/Si.xml", b"si"), ("JTH-PBE-v2.0/O/O.xml", b"o")]:
    (src / path).parent.mkdir(parents=True)
    (src / path).write_bytes(text)
row = lambda el, path, text: {{"element": el, "functional": "pbe", "path": path, "url": (src / path).resolve().as_uri(),
                               "sha256": hashlib.sha256(text).hexdigest()}}
fake = pathlib.Path("fake-orbitar").resolve()
fake.write_text("#!/bin/sh\ncase \"$*\" in *--family*) echo \"$*\" >&2; exit 2;; \"datasets --directory \"*) exec cat rows.json;; esac\nexit 3\n")
fake.chmod(0o755)
pathlib.Path("rows.json").write_text(json.dumps([row("Si", "JTH-PBE-v2.0/Si/Si.xml", b"si")]))
out = io.StringIO()
with contextlib.redirect_stdout(out):
    datasets.main(args[:3] + ["--orbitar", str(fake)])
pathlib.Path("rows.json").write_text(json.dumps([row("Si", "JTH-PBE-v2.0/Si/Si.xml", b"si"),
                                                 row("O", "JTH-PBE-v2.0/O/O.xml", b"x")]))
def refusal(argv):
    try:
        with contextlib.redirect_stdout(io.StringIO()):
            datasets.main(argv + ["--orbitar", str(fake)])
    except SystemExit as e:
        return str(e)
print(json.dumps({{"log": out.getvalue(), "got": sorted(str(p) for p in pathlib.Path("datasets").rglob("*.xml") if p.parent.name in args),
                  "refused": refusal(["fetch", "bad", "O"]), "bad": pathlib.Path("bad/JTH-PBE-v2.0/O/O.xml").exists(),
                  "mode": oct(pathlib.Path("datasets/JTH-PBE-v2.0/Si/Si.xml").stat().st_mode & 0o777),
                  "umask": oct(datasets.UMASK),
                  "tables": sorted(p.name for p in pathlib.Path("datasets").glob("*.toml")),
                  "unlisted": refusal(["fetch", "datasets", "Q"]),
                  "family": refusal(["fetch", "--family", "nope", "none", "Q"])}}))
"##
    );
    let (stdout, _) = python("datasets", &script, "C");
    let got: Value = serde_json::from_str(stdout.trim()).unwrap();
    let log = got["log"].as_str().unwrap();
    assert!(
        log.starts_with(
            "This command downloads each pinned file that `orbitar datasets --directory <directory>` names, from \
             its source; each file keeps its source's license.\n\
             The data package orbitar-jth distributes the packaged jth-2.0 family separately (GPL-3.0).\n"
        ),
        "{log}"
    );
    assert!(log.ends_with("fetched  JTH-PBE-v2.0/Si/Si.xml\n"), "{log}");
    assert_eq!(
        got["got"],
        serde_json::json!(["datasets/JTH-PBE-v2.0/Si/Si.xml"])
    );
    assert!(
        got["refused"].as_str().unwrap().contains("but the pin is"),
        "{got}"
    );
    assert_eq!(got["bad"], false, "a file off its pin is never written");
    let umask =
        u32::from_str_radix(got["umask"].as_str().unwrap().trim_start_matches("0o"), 8).unwrap();
    assert_eq!(got["mode"], format!("0o{:o}", 0o666 & !umask), "{got}");
    assert!(
        got["unlisted"]
            .as_str()
            .unwrap()
            .contains("the family holds no file for ['Q']"),
        "{got}"
    );
    assert_eq!(
        got["family"], "orbitar.datasets: datasets --directory none --family nope",
        "{got}"
    );
    assert!(
        (got["tables"].as_array().unwrap()).contains(&serde_json::json!("jth-2.0.toml")),
        "{got}"
    );
}

#[test]
fn orbitar_datasets_prints_the_rows_that_a_run_resolves_and_names_a_missing_table() {
    let (code, stdout, stderr) = orbitar(&["datasets", "--directory", "datasets"], "");
    assert_eq!(code, 0, "{stderr}");
    let rows: Value = serde_json::from_str(&stdout).unwrap();
    let si = (rows.as_array().unwrap().iter())
        .find(|row| row["element"] == "Si" && row["functional"] == "pbe")
        .unwrap();
    assert_eq!(si["path"], "JTH-PBE-v2.0/Si/Si.GGA_PBE-JTH.xml");
    assert_eq!(
        si["sha256"],
        "79075fcce1418817b2dce0ebe90d6a970776fc0832375a2ef99af0ac9dbe863a"
    );
    let named = orbitar(
        &["datasets", "--directory", "datasets", "--family", "jth-2.0"],
        "",
    );
    assert_eq!(named.1, stdout);
    let (code, stdout, stderr) = orbitar(
        &["datasets", "--directory", "datasets", "--family", "nope"],
        "",
    );
    assert_eq!((code, stdout.as_str()), (2, ""));
    assert!(
        stderr.starts_with("datasets.family: datasets/nope.toml: ")
            && stderr.contains("put the table nope.toml there"),
        "{stderr}"
    );
    let (code, _, stderr) = orbitar(&["datasets"], "");
    assert_eq!(code, 2);
    assert!(stderr.starts_with("usage: "), "{stderr}");
}

#[test]
fn the_container_page_runs_keep_the_image_read_only_and_bind_only_a_work_directory() {
    let page = repository_file("container/README.md");
    let runs: Vec<&str> = (page.lines())
        .filter(|l| {
            ["docker run", "apptainer exec", "apptainer run"]
                .iter()
                .any(|r| l.starts_with(r))
        })
        .collect();
    assert!(runs.len() >= 4, "{page}");
    for run in runs {
        assert!(
            run.starts_with("apptainer") || run.contains("--read-only"),
            "{run}"
        );
        let words: Vec<&str> = run.split_whitespace().collect();
        for pair in words.windows(2) {
            if ["--volume", "--bind"].contains(&pair[0]) {
                assert!(pair[1].trim_matches('"').ends_with(":/work"), "{run}");
            }
        }
    }
    assert!(!page.contains("docker push"), "{page}");
}

#[test]
fn orbitar_datasets_takes_its_two_flags_by_their_exact_names() {
    for args in [
        &["datasets", "--dir", "datasets"][..],
        &["datasets", "--directory", "datasets", "--fam", "jth-2.0"][..],
    ] {
        let (code, stdout, stderr) = orbitar(args, "");
        assert_eq!((code, stdout.as_str()), (2, ""), "{args:?}: {stderr}");
        assert!(stderr.contains("usage: orbitar"), "{args:?}: {stderr}");
    }
}
