use super::{
    hex,
    speed::{both, fake, passed, prelude},
};
use crate::{Tree, tree, write};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::os::unix::fs::PermissionsExt;
use xtask::Env;
use xtask::harness::{Outcome, Session, Tier};

mod abinit;
mod qe;

pub(crate) const RIVALS: &str = "calibration_cells = [\"benchmarks/1_a/input.toml\"]\nlayout_cells = [\"benchmarks/1_a/input.toml\"]\nbudget_s = 1000.0\nthreads = [8]\n\n[target]\nenergy_difference_ha_per_atom = 3.674932217565499e-5\nvolume_scale = 0.94\npressure_gpa = 0.1\n\n[qe]\nladder_ha = [10.0, 12.5, 15.0, 17.5, 20.0]\nconverged_ha = 30.0\ndensity_ratio = 8.0\nmpiexec = [\"--bind-to\", \"none\"]\n\n[abinit]\nladder_ha = [6.0, 8.0, 10.0, 12.0]\nconverged_ha = 20.0\ndensity_ratio = 2.0\n";
pub(crate) const TUNED: &str = "[qe.cutoff_ha]\nAl = 10.0\nSi = 10.0\n\n[qe.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 1, threads = 8 }\n\n[abinit.cutoff_ha]\nAl = 10.0\nSi = 10.0\n\n[abinit]\nvolume_scale = 0.94\n\n[abinit.calibration]\n\"benchmarks/1_a/input.toml\" = [[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]\n\n[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 1, threads = 8 }\n";
pub(crate) const CASE: &str = "[case]\ntier = \"claim\"\nstatus = \"active\"\nclaims = [\"speed\", \"correctness\"]\nreference = \"analytic\"\nrivals = [\"qe\", \"abinit\"]\n\n[tolerance]\nerror_max = 1e-12\n";
pub(crate) const INPUT: &str = "schema = 1\n\n[structure]\ncell_bohr = [[0.0, 5.0, 5.0], [5.0, 0.0, 5.0], [5.0, 5.0, 0.0]]\nspecies = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.25]]\n\n[basis]\ncutoff_ha = 1.0\n\n[kpoints]\nmesh = [2, 2, 2]\nshift = [0.5, 0.5, 0.5]\nsymmetry = false\n\n[occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.01\n\n[scf]\nenergy_tolerance_ha = 0.001\n\n[bands]\ncount = 8\n";
pub(crate) const PW: &str = "t=${t:-0.1}; e=${e:--4.0}\necho '     Program PWSCF v.7.5 starts on 31Oct2026'\nsleep $t\necho \"     total cpu time spent up to now is        $t secs\"\necho \"     total energy              =     $e Ry\"\necho \"!    total energy              =     $e Ry\"\necho \"     PWSCF        :      0.01s CPU      ${t}s WALL\"";
pub(crate) const ABINIT: &str = "t=${t:-0.1}; e=${e:--2.0}\necho ' ABINIT 10.0.3'\nsleep $t\necho \" ETOT  1  $e    -2.000E+00 1.0E-04 1.0E+00\"\necho \"{SCF_istep: 1 , Vnl|psi>: 437 , wall_time: '  $t [s] '} <<< TIME\"\necho \" Proc.   0 individual time (sec): cpu=          $t  wall=          $t\"\necho 'overall_wall_time:          14.2'";

const JTH_SI: &str = "<atom Z=\"14.00\" valence=\"4.00\"/>";
const JTH_AL: &str = "<atom Z=\"13.00\" valence=\"3.00\"/>";
const UPF_SI: &str = "<PP_HEADER z_valence=\"4.000000000000000E+000\"/>";

fn repin(root: &Tree, path: &str, bytes: &str) {
    let old = hex(&Sha256::digest(std::fs::read(root.join(path)).unwrap()));
    let table = ["benchmarks/DATASETS.toml", "datasets/jth-2.0.toml"]
        .into_iter()
        .find(|table| {
            std::fs::read_to_string(root.join(table))
                .unwrap()
                .contains(&old)
        })
        .unwrap_or_else(|| panic!("{path}: no pin"));
    let pins = std::fs::read_to_string(root.join(table)).unwrap();
    let pins = pins.replace(&old, &hex(&Sha256::digest(bytes)));
    write(root, table, &pins);
    write(root, path, bytes);
}

pub(crate) fn script(path: &std::path::Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

pub(crate) fn with_layout_policies(root: &Tree, tuned: &str) -> String {
    let mut table: toml::Table = toml::from_str(tuned).unwrap();
    let Ok(policies) = xtask::harness::rival_layout_policies(root) else {
        return tuned.to_owned();
    };
    let mut frozen_any = false;
    for (code, policy) in policies {
        if let Some(frozen) = table.get_mut(&code).and_then(|f| f.as_table_mut())
            && frozen.contains_key("layout")
        {
            frozen.insert("layout_policy".into(), policy.into());
            frozen_any = true;
        }
    }
    match frozen_any {
        true => toml::to_string(&table).unwrap(),
        false => tuned.to_owned(),
    }
}

pub(crate) fn rivals_tree(name: &str, input: &str, tuned: &str, pw: &str, abinit: &str) -> Tree {
    let root = tree(
        name,
        &[
            ("benchmarks/1_a/case.toml", CASE),
            ("benchmarks/1_a/input.toml", input),
            ("benchmarks/RIVALS.toml", RIVALS),
            ("target/datasets/JTH-PBE-v2.0/Si/Si.xml", JTH_SI),
            ("rivals/SSSP/Si.UPF", UPF_SI),
            ("target/datasets/JTH-PBE-v2.0/Al/Al.xml", JTH_AL),
            ("rivals/SSSP/Al.UPF", "<PP_HEADER z_valence=\"3.0\"/>"),
        ],
    );
    let sha = |text: &str| hex(&Sha256::digest(text));
    let rows = [("pbe", "Al", JTH_AL, None), ("pbe", "Si", JTH_SI, None)];
    write(
        &root,
        "datasets/jth-2.0.toml",
        &super::jth("file:///nowhere", &rows),
    );
    let pins = format!(
        "[qe]\narchive = \"file:///nowhere/sssp.tar.gz\"\narchive_sha256 = \"none\"\ndirectory = \"SSSP\"\n\n[qe.files]\nAl = {{ file = \"Al.UPF\", sha256 = \"{}\" }}\nSi = {{ file = \"Si.UPF\", sha256 = \"{}\" }}\n",
        sha("<PP_HEADER z_valence=\"3.0\"/>"),
        sha(UPF_SI),
    );
    write(&root, "benchmarks/DATASETS.toml", &pins);
    write(
        &root,
        "benchmarks/TUNED.toml",
        &with_layout_policies(&root, tuned),
    );
    script(&root.join("rivals/qe/bin/pw.x"), pw);
    script(&root.join("rivals/abinit/bin/abinit"), abinit);
    let mpiexec = "while [ \"$1\" != -n ]; do shift; done\nshift 2\nexec \"$@\"";
    script(&root.join("rivals/qe/bin/mpiexec"), mpiexec);
    script(&root.join("rivals/abinit/bin/mpiexec"), mpiexec);
    root
}

fn claim(root: &Tree, rivals: bool) -> (Outcome, Value) {
    claim_by(root, rivals, "step -2")
}

fn claim_by(root: &Tree, rivals: bool, body: &str) -> (Outcome, Value) {
    let bin = fake(root, body);
    let mut session = Session::new(root, &bin, Tier::Claim, &root.join("records"));
    if rivals {
        session = session.with_rivals(&root.join("rivals"));
    }
    let outcome = session.evaluate("1_a", both);
    let text = std::fs::read_to_string(root.join("records/1_a.json")).unwrap();
    (outcome, serde_json::from_str(&text).unwrap())
}

fn blocks<'a>(record: &'a Value, code: &str) -> Vec<&'a Value> {
    (record["speed"].as_array().unwrap().iter())
        .filter(|b| b["code"] == code)
        .collect()
}

#[test]
fn each_rival_is_timed_beside_orbitar_at_the_claims_8_threads_against_its_own_reference() {
    let root = rivals_tree("rival-timed", INPUT, TUNED, PW, ABINIT);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for code in ["orbitar", "qe", "abinit"] {
        let seen = blocks(&record, code);
        let threads: Vec<u64> = seen
            .iter()
            .map(|b| b["threads"].as_u64().unwrap())
            .collect();
        assert_eq!(threads, [8], "{code}: {record}");
        for b in seen {
            assert_eq!(b["e_ref_ha"].as_f64(), Some(-2.0), "{b}");
            assert!(
                [5, 10].contains(&b["t_delta_s"].as_array().unwrap().len()),
                "{b}"
            );
            assert!(b["losses"].as_array().unwrap().is_empty(), "{b}");
        }
    }
    for b in record["speed"].as_array().unwrap() {
        assert!(b["median_t_delta_s"].is_f64(), "{b}");
    }
    let sha = |text: &str| hex(&Sha256::digest(text));
    let files = &blocks(&record, "qe")[0]["rival"]["datasets"];
    assert_eq!(
        files["Si.UPF"].as_str(),
        Some(sha(UPF_SI).as_str()),
        "{files}"
    );
    let files = &blocks(&record, "abinit")[0]["rival"]["datasets"];
    assert_eq!(
        files["JTH-PBE-v2.0/Si/Si.xml"].as_str(),
        Some(sha(JTH_SI).as_str()),
        "{files}"
    );
    let qe = blocks(&record, "qe")[0];
    assert_eq!(qe["rival"]["cutoff_ha"].as_f64(), Some(10.0), "{qe}");
    assert_eq!(qe["rival"]["layout"]["threads"].as_u64(), Some(8), "{qe}");
    assert_eq!(qe["rival"]["mpiexec"], json!([]), "{qe}");
}

fn first_inputs(name: &str, input: &str) -> [String; 4] {
    first_inputs_in(name, input, |_| {})
}

fn first_inputs_in(name: &str, input: &str, setup: impl Fn(&Tree)) -> [String; 4] {
    let log = std::env::temp_dir().join(format!("xtask-{name}-log-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&log);
    std::fs::create_dir_all(&log).unwrap();
    let keep = |file: &str| {
        format!(
            "[ -e '{0}/{file}' ] || {{ cp {file} '{0}/{file}'; echo \"$OMP_NUM_THREADS $OPENBLAS_NUM_THREADS $GFORTRAN_UNBUFFERED_PRECONNECTED $*\" > '{0}/{file}.env'; }}",
            log.display()
        )
    };
    let pw = format!("{}\n{PW}", keep("pw.in"));
    let abinit = format!("{}\n{ABINIT}", keep("run.abi"));
    let root = rivals_tree(name, input, TUNED, &pw, &abinit);
    setup(&root);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let read = |file: &str| std::fs::read_to_string(log.join(file)).unwrap();
    let files = ["pw.in", "run.abi", "pw.in.env", "run.abi.env"].map(read);
    let _ = std::fs::remove_dir_all(&log);
    files
}

#[test]
fn the_rival_inputs_carry_the_case_cell_mesh_smearing_bands_and_stop_with_the_io_off() {
    let [qe, abi, qe_env, abi_env] = first_inputs("rival-inputs", INPUT);
    let want_qe = "&control\n  calculation = 'scf'\n  disk_io = 'none'\n  pseudo_dir = './'\n  outdir = './'\n  tstress = .false.\n/\n\
        &system\n  ibrav = 0\n  nat = 1\n  ntyp = 1\n  ecutwfc = 20.0\n  ecutrho = 160.0\n  occupations = 'smearing'\n  smearing = 'fd'\n  degauss = 0.02\n  nbnd = 8\n  nosym = .true.\n  noinv = .true.\n/\n\
        &electrons\n  conv_thr = 7.349864435130998e-9\n  electron_maxstep = 100\n  startingpot = 'atomic'\n  startingwfc = 'atomic+random'\n/\n\
        ATOMIC_SPECIES\n  Si 1.0 Si.UPF\nCELL_PARAMETERS bohr\n  0.0 5.0 5.0\n  5.0 0.0 5.0\n  5.0 5.0 0.0\nATOMIC_POSITIONS crystal\n  Si 0.0 0.0 0.25\nK_POINTS automatic\n  2 2 2 1 1 1\n";
    assert_eq!(qe, want_qe);
    let want_abinit = "acell 3*1.0\nrprim\n  0.0 5.0 5.0\n  5.0 0.0 5.0\n  5.0 5.0 0.0\nnatom 1\nntypat 1\nznucl 14\ntypat 1\nxred\n  0.0 0.0 0.25\n\
        pp_dirpath \"./\"\npseudos \"JTH-PBE-v2.0/Si/Si.xml\"\necut 10.0\npawecutdg 20.0\nngkpt 2 2 2\nnshiftk 1\nshiftk 0.5 0.5 0.5\nkptopt 3\nnsym 1\n\
        occopt 3\ntsmear 0.01\nnband 8\ntoldfe 3.674932217565499e-9\nnstep 500\noptstress 0\nchkprim 0\nchksymbreak 0\nprtwf 0\nprtden 0\nprteig 0\nprtgsr 0\nprtebands 0\n";
    assert_eq!(abi, want_abinit);
    assert_eq!(qe_env.trim(), "8 1 y -in pw.in");
    assert_eq!(abi_env.trim(), "8 1 y run.abi");
}

#[test]
fn a_claim_refuses_a_frozen_layout_that_another_candidate_set_or_thread_policy_chose() {
    let legacy = TUNED.replace(
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 1, threads = 8 }",
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 8, threads = 1, input = [\"paral_kgb 1\", \"autoparal 1\", \"istwfk *1\"] }",
    );
    assert_ne!(legacy, TUNED);
    let root = rivals_tree("rival-legacy-layout", INPUT, TUNED, PW, ABINIT);
    write(&root, "benchmarks/TUNED.toml", &legacy);
    let (outcome, _) = claim(&root, true);
    let named = "layouts were searched under another candidate set or thread policy";
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains(named) && f.contains("none frozen); run `cargo xtask rivals layout` on the node")),
        "{outcome:?}"
    );
    let abinit_alone = legacy.replacen(
        "[qe.layout]",
        "[qe]\nlayout_policy = \"<qe>\"\n\n[qe.layout]",
        1,
    );
    let qe = xtask::harness::rival_layout_policies(&root).unwrap()["qe"].clone();
    write(
        &root,
        "benchmarks/TUNED.toml",
        &abinit_alone.replace("<qe>", &qe),
    );
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("the abinit layouts were searched under another candidate set") && f.contains("none frozen")),
        "{outcome:?}"
    );
    let stale =
        with_layout_policies(&root, TUNED).replace("layout_policy = \"", "layout_policy = \"0");
    write(&root, "benchmarks/TUNED.toml", &stale);
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(f) if f.contains("searched under another candidate set or thread policy")),
        "{outcome:?}"
    );
}

#[test]
fn each_declared_rival_links_the_pthreads_openblas_and_no_other_blas() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../benchmarks/NODE.toml");
    let node: toml::Table = toml::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    for code in ["qe", "abinit"] {
        let packages = node[code].as_table().unwrap();
        let pin = |name: &str| {
            packages
                .get(name)
                .and_then(|p| p.as_str())
                .unwrap_or_default()
        };
        assert!(
            pin("libopenblas").contains(" pthreads_"),
            "{code}: {packages:?}"
        );
        for (name, pin) in packages {
            let pin = pin.as_str().unwrap_or_default();
            let blas = ["blas", "mkl", "blis", "lapack"];
            if blas.iter().any(|b| name.contains(b) || pin.contains(b)) {
                assert!(
                    name.contains("openblas") || pin.contains("_openblas"),
                    "{code}.{name} = {pin}"
                );
            }
        }
    }
}

#[test]
fn each_rival_block_names_the_thread_environment_that_its_runs_had() {
    let log =
        std::env::temp_dir().join(format!("xtask-rival-thread-env-log-{}", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let keep = format!(
        "echo \"$OMP_NUM_THREADS $OPENBLAS_NUM_THREADS\" >> '{}'",
        log.display()
    );
    let (pw, abinit) = (format!("{keep}\n{PW}"), format!("{keep}\n{ABINIT}"));
    let root = rivals_tree("rival-thread-env", INPUT, TUNED, &pw, &abinit);
    let (outcome, record) = claim(&root, true);
    let seen = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let lines: std::collections::BTreeSet<&str> = seen.lines().collect();
    assert_eq!(lines, ["8 1"].into(), "{seen}");
    for code in ["qe", "abinit"] {
        let timed = blocks(&record, code);
        assert_eq!(timed.len(), 1, "{record}");
        assert_eq!(
            timed[0]["rival"]["thread_env"],
            json!({ "OMP_NUM_THREADS": "8", "OPENBLAS_NUM_THREADS": "1" }),
            "{record}"
        );
    }
}

#[test]
fn abinit_reads_the_pbe_file_of_an_element_that_jth_holds_in_lda_too() {
    let lda = "<atom Z=\"14.00\" valence=\"4.00\" xc=\"LDA\"/>";
    let [_, abi, ..] = first_inputs_in("rival-lda-pin", INPUT, |root| {
        write(root, "target/datasets/JTH-LDA-v2.0/Si/Si.xml", lda);
        let rows = [
            ("lda", "Si", lda, None),
            ("pbe", "Al", JTH_AL, None),
            ("pbe", "Si", JTH_SI, None),
        ];
        write(
            root,
            "datasets/jth-2.0.toml",
            &super::jth("file:///nowhere", &rows),
        );
    });
    assert!(
        abi.contains("\npseudos \"JTH-PBE-v2.0/Si/Si.xml\"\n"),
        "{abi}"
    );
}

#[test]
fn spin_gives_qe_one_type_per_moment_and_abinit_spinat() {
    let input = INPUT.replace("[\"Si\"]", "[\"Si\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    ) + "\n[spin]\nmoments = [2.0, -2.0]\n";
    let [qe, abi, ..] = first_inputs("rival-spin", &input);
    for line in [
        "  ntyp = 2\n",
        "  nspin = 2\n",
        "  starting_magnetization(1) = 0.5\n",
        "  starting_magnetization(2) = -0.5\n",
        "  Si 1.0 Si.UPF\n  Si1 1.0 Si.UPF\n",
        "  Si 0.0 0.0 0.0\n  Si1 0.25 0.25 0.25\n",
    ] {
        assert!(qe.contains(line), "{line:?} in {qe}");
    }
    for line in [
        "ntypat 1\n",
        "typat 1 1\n",
        "nsppol 2\nspinat\n  0.0 0.0 2.0\n  0.0 0.0 -2.0\n",
    ] {
        assert!(abi.contains(line), "{line:?} in {abi}");
    }
}

#[test]
fn the_input_step_cap_is_every_code_step_cap() {
    let capped = INPUT.replace(
        "energy_tolerance_ha = 0.001\n",
        "energy_tolerance_ha = 0.001\nmax_iterations = 7\n",
    );
    let [qe, abi, ..] = first_inputs("rival-cap", &capped);
    assert!(qe.contains("  electron_maxstep = 7\n"), "{qe}");
    assert!(abi.contains("\nnstep 7\n"), "{abi}");
}

pub(crate) fn three_atoms(input: &str) -> String {
    input
        .replace("[\"Si\"]", "[\"Al\", \"Si\", \"Al\"]")
        .replace(
            "[[0.0, 0.0, 0.25]]",
            "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25], [0.5, 0.5, 0.5]]",
        )
}

#[test]
fn two_elements_get_one_type_each_in_the_order_they_first_appear() {
    let [qe, abi, ..] = first_inputs("rival-two", &three_atoms(INPUT));
    for line in [
        "  nat = 3\n  ntyp = 2\n",
        "  Al 1.0 Al.UPF\n  Si 1.0 Si.UPF\n",
        "  Al 0.0 0.0 0.0\n  Si 0.25 0.25 0.25\n  Al 0.5 0.5 0.5\n",
    ] {
        assert!(qe.contains(line), "{line:?} in {qe}");
    }
    for line in [
        "natom 3\nntypat 2\nznucl 13 14\ntypat 1 2 1\n",
        "pseudos \"JTH-PBE-v2.0/Al/Al.xml, JTH-PBE-v2.0/Si/Si.xml\"\n",
    ] {
        assert!(abi.contains(line), "{line:?} in {abi}");
    }
}

#[test]
fn fixed_occupations_symmetry_and_an_unshifted_mesh_reach_both_codes() {
    let input = INPUT
        .replace("shift = [0.5, 0.5, 0.5]\nsymmetry = false\n", "")
        .replace(
            "[occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.01\n",
            "",
        );
    let [qe, abi, ..] = first_inputs("rival-fixed", &input);
    assert!(
        qe.contains("  occupations = 'fixed'\n  nbnd = 8\n/\n") && qe.ends_with("  2 2 2 0 0 0\n"),
        "{qe}"
    );
    assert!(
        abi.contains("shiftk 0.0 0.0 0.0\nkptopt 1\noccopt 1\nnband 8\n"),
        "{abi}"
    );
}

#[test]
fn qe_runs_an_unshifted_one_point_mesh_as_gamma_and_any_other_mesh_as_automatic() {
    let unshifted = INPUT.replace("shift = [0.5, 0.5, 0.5]\n", "");
    for (name, input, want) in [
        (
            "rival-gamma",
            unshifted.replace("[2, 2, 2]", "[1, 1, 1]"),
            "K_POINTS gamma\n",
        ),
        (
            "rival-shifted-one",
            INPUT.replace("[2, 2, 2]", "[1, 1, 1]"),
            "K_POINTS automatic\n  1 1 1 1 1 1\n",
        ),
        (
            "rival-one-axis",
            unshifted.replace("[2, 2, 2]", "[1, 1, 2]"),
            "K_POINTS automatic\n  1 1 2 0 0 0\n",
        ),
    ] {
        let [qe, ..] = first_inputs(name, &input);
        assert!(
            qe.ends_with(&format!(
                "ATOMIC_POSITIONS crystal\n  Si 0.0 0.0 0.25\n{want}"
            )),
            "{qe}"
        );
    }
}

#[test]
fn each_smearing_maps_to_its_qe_name_and_abinit_occopt() {
    for (smearing, qe_name, occopt) in [
        ("gaussian", "gaussian", 7),
        ("methfessel-paxton", "mp", 6),
        ("marzari-vanderbilt", "mv", 4),
    ] {
        let input = INPUT.replace("\"fermi-dirac\"", &format!("\"{smearing}\""));
        let [qe, abi, ..] = first_inputs(&format!("rival-{qe_name}"), &input);
        assert!(
            qe.contains(&format!("  smearing = '{qe_name}'\n  degauss = 0.02\n")),
            "{qe}"
        );
        assert!(
            abi.contains(&format!("occopt {occopt}\ntsmear 0.01\n")),
            "{abi}"
        );
    }
}

#[test]
fn an_input_that_the_rivals_cannot_read_stops_the_claim_by_name() {
    for (from, to, why) in [
        (
            "shift = [0.5, 0.5, 0.5]",
            "shift = [0.25, 0.5, 0.5]",
            "rivals: kpoints.shift 0.25 is neither 0 nor 0.5 of a division",
        ),
        (
            "[scf]",
            "[xc]\nfunctional = \"lda\"\n\n[scf]",
            "rivals: their datasets are PBE",
        ),
        (
            "mesh = [2, 2, 2]",
            "list = [[0.0, 0.0, 0.0]]",
            "rivals: the input needs a kpoints.mesh",
        ),
        (
            "\"fermi-dirac\"",
            "\"cold\"",
            "rivals: no rival smearing for cold",
        ),
        (
            "[[0.0, 0.0, 0.25]]",
            "[[0.0, 0.0, 0.25], [0.5, 0.5, 0.5]]",
            "rivals: 2 positions for 1 species entries",
        ),
        (
            "count = 8",
            "count = -8",
            "rivals: bands.count -8 is not a positive count",
        ),
        (
            "count = 8",
            "count = 0",
            "rivals: bands.count 0 is not a positive count",
        ),
        (
            "energy_tolerance_ha = 0.001",
            "energy_tolerance_ha = 0.001\nmax_iterations = 0",
            "rivals: scf.max_iterations 0 is not a positive count",
        ),
    ] {
        let root = rivals_tree("rival-unread", &INPUT.replace(from, to), TUNED, PW, ABINIT);
        let (outcome, _) = claim(&root, true);
        assert!(
            matches!(&outcome, Outcome::Fail(w) if w.contains(why)),
            "{outcome:?}"
        );
    }
}

#[test]
fn a_rival_that_crashes_loses_each_run_and_orbitar_is_still_judged() {
    let root = rivals_tree(
        "rival-crash",
        INPUT,
        TUNED,
        "echo 'Error in routine cdiaghg' >&2\nexit 1",
        ABINIT,
    );
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for b in blocks(&record, "qe") {
        let losses = b["losses"].as_array().unwrap();
        assert_eq!(losses.len(), 5, "{b}");
        assert!(
            losses[0]
                .as_str()
                .unwrap()
                .contains("Error in routine cdiaghg"),
            "{b}"
        );
        assert!(b["t_delta_s"].as_array().unwrap().is_empty(), "{b}");
        assert!(b["warmup"]["loss"].is_string(), "{b}");
        for key in ["median_t_delta_s", "range_t_delta_s", "cv_t_delta"] {
            assert!(b.get(key).is_none(), "{key}: {b}");
        }
    }
    assert!(
        blocks(&record, "abinit")
            .iter()
            .all(|b| b["losses"].as_array().unwrap().is_empty())
    );
}

const NI_UPF_V1: &str = "<PP_INFO>
Generated using Vanderbilt code, version   7  3  6                              
Author: kfg        Generation date:    1    4   15                              
Automatically converted from original format                                    
    1        The Pseudo was generated with a Scalar-Relativistic Calculation
  1.50000000000E+00    Local Potential cutoff radius
</PP_INFO>


<PP_HEADER>
   0                   Version Number
  Ni                   Element
   US                  Ultrasoft pseudopotential
    T                  Nonlinear Core Correction
 SLA  PW   PBX  PBC    PBE  Exchange-Correlation functional
   18.00000000000      Z valence
  -340.79676105500      Total energy
    0.00000    0.00000 Suggested cutoff for wfc and rho
    2                  Max angular momentum component
</PP_HEADER>
";

const O_UPF_V2: &str = "<UPF version=\"2.0.1\">
  <PP_HEADER
            element=\"O\"
            z_valence=\"6.000000000000000E+000\"
  />
</UPF>
";

fn passed_unmeasured(outcome: &Outcome) -> bool {
    match outcome {
        Outcome::Fail(why) => why.split("; ").all(|f| {
            passed(&Outcome::Fail(f.to_owned()))
                || f == "input: at ABINIT's frozen cutoff, the accuracy is unmeasured (d None, e Some(1e-6))"
        }),
        _ => passed(outcome),
    }
}

fn first_loss(name: &str, pw: &str, abinit: &str, code: &str) -> String {
    let root = rivals_tree(name, INPUT, TUNED, pw, abinit);
    let (outcome, record) = claim(&root, true);
    assert!(passed_unmeasured(&outcome), "{outcome:?}: {record}");
    let b = blocks(&record, code)[0];
    b["losses"][0].as_str().unwrap_or_default().to_owned()
}

#[test]
fn a_short_run_whose_lines_all_arrive_at_its_end_loses_by_its_own_clock() {
    let burst = "sleep 0.25\necho ' ABINIT 10.0.3'\necho ' ETOT  1  -2.0    -2.000E+00 1.0E-04 1.0E+00'\necho ' Proc.   0 individual time (sec): cpu=          0.25  wall=          0.25'";
    let loss = first_loss("rival-burst", PW, burst, "abinit");
    assert!(
        loss.contains("abinit: its lines span 0.0")
            && loss.contains("its own clock reads 0.250 s: its output buffers"),
        "{loss}"
    );
}

#[test]
fn step_lines_that_arrive_in_one_block_lose_by_each_steps_own_clock() {
    let block = "echo ' ABINIT 10.0.3'\nsleep 0.9\n\
        for k in 1 2 3; do echo \" ETOT  $k  -2.0    -2.000E+00 1.0E-04 1.0E+00\"; echo \"{SCF_istep: $k , Vnl|psi>: 437 , wall_time: '  0.30 [s] '} <<< TIME\"; done\n\
        echo ' Proc.   0 individual time (sec): cpu=          0.9  wall=          0.9'";
    let loss = first_loss("rival-block", PW, block, "abinit");
    assert!(
        loss.contains("abinit: its step 2 arrived 0.0")
            && loss.contains("but its own clock gives 0.300 s: its output buffers"),
        "{loss}"
    );
    let pw = "echo '     Program PWSCF v.7.5 starts'\nsleep 0.6\n\
        for c in 0.3 0.6; do echo \"     total cpu time spent up to now is        $c secs\"; echo '     total energy              =     -4.0 Ry'; done\n\
        echo '     PWSCF        :      0.60s CPU      0.60s WALL'";
    let loss = first_loss("rival-block-qe", pw, ABINIT, "qe");
    assert!(
        loss.contains("qe: its step 2 arrived 0.0") && loss.contains("own clock gives 0.300 s"),
        "{loss}"
    );
}

#[test]
fn a_step_that_arrives_later_than_its_own_clock_is_no_buffering() {
    let abinit = "echo ' ABINIT 10.0.3'\n\
        for k in 1 2 3; do sleep 0.5; echo \" ETOT  $k  -2.0    -2.000E+00 1.0E-04 1.0E+00\"; echo \"{SCF_istep: $k , Vnl|psi>: 437 , wall_time: '  0.30 [s] '} <<< TIME\"; done\n\
        echo ' Proc.   0 individual time (sec): cpu=          1.5  wall=          1.5'";
    assert_eq!(first_loss("rival-late", PW, abinit, "abinit"), "");
}

#[test]
fn a_step_that_arrives_within_its_share_of_its_own_clock_is_no_buffering() {
    let abinit = "echo ' ABINIT 10.0.3'\nsleep 0.3\n\
        echo ' ETOT  1  -2.0    -2.000E+00 1.0E-04 1.0E+00'\necho \"{SCF_istep: 1 , Vnl|psi>: 437 , wall_time: '  0.30 [s] '} <<< TIME\"\n\
        sleep 0.469\necho ' ETOT  2  -2.0    -2.000E+00 1.0E-04 1.0E+00'\necho \"{SCF_istep: 2 , Vnl|psi>: 437 , wall_time: '  0.58 [s] '} <<< TIME\"\n\
        sleep 0.5\necho ' ETOT  3  -2.0    -2.000E+00 1.0E-04 1.0E+00'\necho \"{SCF_istep: 3 , Vnl|psi>: 437 , wall_time: '  1.00 [s] '} <<< TIME\"\n\
        echo ' Proc.   0 individual time (sec): cpu=          1.27  wall=          1.27'";
    assert_eq!(first_loss("rival-share", PW, abinit, "abinit"), "");
}

#[test]
fn a_step_that_arrives_sooner_than_its_share_of_its_own_clock_loses() {
    let abinit = "echo ' ABINIT 10.0.3'\n\
        for k in 1 2; do sleep 0.2; echo \" ETOT  $k  -2.0    -2.000E+00 1.0E-04 1.0E+00\"; echo \"{SCF_istep: $k , Vnl|psi>: 437 , wall_time: '  0.60 [s] '} <<< TIME\"; done\n\
        echo ' Proc.   0 individual time (sec): cpu=          0.4  wall=          0.4'";
    let loss = first_loss("rival-soon", PW, abinit, "abinit");
    assert!(
        loss.contains("abinit: its step 2 arrived 0.2")
            && loss.contains("but its own clock gives 0.600 s"),
        "{loss}"
    );
}

#[test]
fn steps_that_arrive_as_their_own_clock_says_pass() {
    let abinit = "echo ' ABINIT 10.0.3'\n\
        for k in 1 2 3; do sleep 0.3; echo \" ETOT  $k  -2.0    -2.000E+00 1.0E-04 1.0E+00\"; echo \"{SCF_istep: $k , Vnl|psi>: 437 , wall_time: '  0.30 [s] '} <<< TIME\"; echo \"{GS_kpoint: 1 , wall_time: '  9.99 [s] '} <<< TIME\"; done\n\
        echo ' Proc.   0 individual time (sec): cpu=          0.9  wall=          0.9'";
    assert_eq!(first_loss("rival-steps", PW, abinit, "abinit"), "");
}

#[test]
fn a_rival_whose_scf_stops_unconverged_loses_though_it_exits_zero() {
    let pw = PW.replace(
        "!    total energy",
        "     convergence NOT achieved after 100 iterations: stopping\\n!    total energy",
    );
    let pw = format!("{pw}\nexit 0");
    let loss = first_loss("rival-unconverged", &pw, ABINIT, "qe");
    assert!(
        loss.contains(
            "qe: its SCF stopped unconverged: convergence NOT achieved after 100 iterations"
        ),
        "{loss}"
    );
    let abinit = ABINIT.replace(
        "echo \" Proc.",
        "for e in -2.5 -2.4 -2.45 -2.43 -2.44; do echo \" ETOT  2  $e    -2.000E+00 1.0E-04 1.0E+00\"; done\n\
         echo ' nstep=100 was not enough SCF cycles to converge'\necho \" Proc.",
    );
    let loss = first_loss("rival-unconverged-abinit", PW, &abinit, "abinit");
    assert!(
        loss.contains(
            "abinit: its SCF stopped unconverged: nstep=100 was not enough SCF cycles to converge (6 steps; the last \
             energy changes 1.0e-1, -5.0e-2, 2.0e-2, -1.0e-2 Ha)"
        ),
        "{loss}"
    );
}

fn capped() -> String {
    INPUT.replace(
        "energy_tolerance_ha = 0.001\n",
        "energy_tolerance_ha = 0.001\nmax_iterations = 6\n",
    )
}

#[test]
fn a_rival_that_the_input_step_cap_stops_unconverged_stops_the_claim_and_is_no_loss() {
    let abinit = ABINIT.replace(
        "echo \" Proc.",
        "echo ' nstep=6 was not enough SCF cycles to converge'\necho \" Proc.",
    );
    let root = rivals_tree("rival-capped", &capped(), TUNED, PW, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why)
            if why.contains("abinit: its SCF stopped unconverged: nstep=6 was not enough SCF cycles to converge")
            && why.contains("at the input's step cap scf.max_iterations = 6, which every code shares")),
        "{outcome:?}"
    );
    assert!(
        record["speed"].as_array().is_none_or(Vec::is_empty),
        "{record}"
    );
}

#[test]
fn each_block_records_its_step_cap_and_the_scf_steps_of_every_run_and_of_e_ref() {
    let root = rivals_tree("rival-steps-record", &capped(), TUNED, PW, ABINIT);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for (code, steps) in [("orbitar", 1), ("qe", 2), ("abinit", 1)] {
        for b in blocks(&record, code) {
            let runs = b["wall_s"].as_array().unwrap().len();
            assert_eq!(b["step_cap"], 6, "{b}");
            assert_eq!(b["steps"], json!(vec![steps; runs]), "{b}");
            assert_eq!(b["e_ref_steps"], json!([steps, steps]), "{b}");
            assert_eq!(b["warmup"]["steps"], steps, "{b}");
        }
    }
}

#[test]
fn a_claim_with_rivals_needs_their_installs_and_their_frozen_tuning() {
    let root = rivals_tree("rival-unset", INPUT, TUNED, PW, ABINIT);
    let (outcome, _) = claim(&root, false);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("rivals: harness: the case times rivals, but ORBITAR_RIVALS is unset")),
        "{outcome:?}"
    );
    let untuned = TUNED.replace("8 = { ranks = 1, threads = 8 }\n", "");
    let root = rivals_tree("rival-untuned", INPUT, &untuned, PW, ABINIT);
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("benchmarks/TUNED.toml has no qe layout at 8 threads; run `cargo xtask rivals layout` on the node")),
        "{outcome:?}"
    );
    let uneven = TUNED.replace(
        "8 = { ranks = 1, threads = 8 }",
        "8 = { ranks = 2, threads = 1 }",
    );
    let root = rivals_tree("rival-uneven", INPUT, &uneven, PW, ABINIT);
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("the qe layout at 8 threads runs 2 ranks × 1 threads")),
        "{outcome:?}"
    );
}

#[test]
fn a_rival_that_skips_an_input_times_none_of_it() {
    let root = rivals_tree("rival-skip", INPUT, TUNED, "exit 7", ABINIT);
    write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "density_ratio = 8.0\n",
            "density_ratio = 8.0\nskip_cells = [{ cell = \"benchmarks/1_a/input.toml\", reason = \"no dataset\" }]\n",
        ),
    );
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    assert!(blocks(&record, "qe").is_empty(), "{record}");
    assert_eq!(
        record["skipped"],
        json!([{ "code": "qe", "input": "input", "reason": "no dataset" }]),
        "{record}"
    );
    assert_eq!(blocks(&record, "abinit").len(), 1, "{record}");
    assert_eq!(blocks(&record, "orbitar").len(), 1, "{record}");
}

#[test]
fn rivals_are_timed_only_at_the_thread_counts_that_rivals_toml_names() {
    let tuned = TUNED.replace(
        "1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n",
        "",
    );
    let root = rivals_tree("rival-threads", INPUT, &tuned, PW, ABINIT);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for code in ["qe", "abinit"] {
        let at: Vec<&Value> = blocks(&record, code)
            .iter()
            .map(|b| &b["threads"])
            .collect();
        assert_eq!(at, [&json!(8)], "{record}");
    }
    assert_eq!(blocks(&record, "orbitar").len(), 1, "{record}");
}

#[test]
fn rivals_toml_names_only_thread_counts_of_the_claim_sweep() {
    let root = rivals_tree("rival-threads-3", INPUT, TUNED, PW, ABINIT);
    write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace("threads = [8]", "threads = [3, 8]"),
    );
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("RIVALS.toml: threads holds 3, which the claim sweep [8] does not")),
        "{outcome:?}"
    );
}

#[test]
fn rivals_toml_holds_a_table_for_every_rival_and_for_no_other_code() {
    let without = RIVALS.split("[abinit]").next().unwrap();
    let siesta =
        format!("{RIVALS}\n[siesta]\nladder_ha = [1.0]\nconverged_ha = 2.0\ndensity_ratio = 2.0\n");
    for (name, rivals, why) in [
        (
            "rival-table",
            without.to_owned(),
            "benchmarks/RIVALS.toml: no [abinit] table",
        ),
        ("rival-siesta", siesta, "unknown variant `siesta`"),
    ] {
        let root = rivals_tree(name, INPUT, TUNED, PW, ABINIT);
        write(&root, "benchmarks/RIVALS.toml", &rivals);
        let (outcome, _) = claim(&root, true);
        assert!(
            matches!(&outcome, Outcome::Fail(w) if w.contains(why)),
            "{outcome:?}"
        );
    }
}

#[test]
fn a_layout_of_one_rank_runs_the_code_without_mpiexec() {
    let log = std::env::temp_dir().join(format!("xtask-rival-no-mpi-{}.log", std::process::id()));
    let root = rivals_tree("rival-no-mpi", INPUT, TUNED, PW, ABINIT);
    let mpiexec = format!("echo \"mpiexec $*\" >> '{}'\nexit 1", log.display());
    script(&root.join("rivals/qe/bin/mpiexec"), &mpiexec);
    script(&root.join("rivals/abinit/bin/mpiexec"), &mpiexec);
    let (outcome, record) = claim(&root, true);
    let text = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    assert_eq!(text, "", "mpiexec ran for a layout of one rank");
}

#[test]
fn a_layout_with_ranks_runs_the_code_under_mpiexec_with_its_flags() {
    let log = std::env::temp_dir().join(format!("xtask-rival-mpi-{}.log", std::process::id()));
    let pw = format!("echo \"$*\" >> '{}'\n{PW}", log.display());
    let tuned = TUNED.replace(
        "8 = { ranks = 1, threads = 8 }\n\n[abinit",
        "8 = { ranks = 2, threads = 4, args = [\"-nk\", \"2\"] }\n\n[abinit",
    );
    let root = rivals_tree("rival-mpi", INPUT, &tuned, &pw, ABINIT);
    let mpiexec = format!(
        "echo \"mpiexec $*\" >> '{}'\nwhile [ \"$1\" != -n ]; do shift; done\nshift 2\nexec \"$@\"",
        log.display()
    );
    script(&root.join("rivals/qe/bin/mpiexec"), &mpiexec);
    let (outcome, record) = claim(&root, true);
    let text = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_file(&log);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let pw_x = root.join("rivals/qe/bin/pw.x");
    assert!(
        text.contains(&format!(
            "mpiexec --bind-to none -n 2 {} -nk 2 -in pw.in\n-nk 2 -in pw.in\n",
            pw_x.display()
        )),
        "{text}"
    );
    let launched: Vec<&str> = text.lines().filter(|l| l.starts_with("mpiexec")).collect();
    assert!(launched.iter().all(|l| l.contains(" -n 2 ")), "{text}");
    assert!(text.lines().all(|l| l != "-in pw.in"), "{text}");
    let qe = blocks(&record, "qe");
    assert_eq!(
        qe[0]["rival"]["mpiexec"],
        json!(["--bind-to", "none"]),
        "{}",
        qe[0]
    );
}

#[test]
fn a_claim_runs_abinits_band_split_at_the_band_count_its_record_names() {
    let input = INPUT.replace("\n[bands]\ncount = 8\n", "");
    let tuned = TUNED.replace(
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 1, threads = 8 }",
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 8, threads = 1, input = [\"paral_kgb 1\", \"np_spkpt 1\", \"npband 8\", \"npfft 1\"] }",
    );
    assert_ne!(tuned, TUNED);
    let log = std::env::temp_dir().join(format!("xtask-rival-bands-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let abinit = format!(
        "grep -q 'npband 8' run.abi && {{ grep -qx 'nband 8' run.abi || exit 3; }}\n\
         echo \"$(grep -c 'npband 8' run.abi) $(sed -n 's/^toldfe //p' run.abi)\" >> '{}'\n{ABINIT}",
        log.display()
    );
    let root = rivals_tree("rival-bands", &input, &tuned, PW, &abinit);
    script(
        &root.join("rivals/abinit/bin/mpiexec"),
        "while [ \"$1\" != -n ]; do shift; done\nshift 2\nexec \"$@\"",
    );
    let (outcome, record) = claim(&root, true);
    let calls = std::fs::read_to_string(&log).unwrap_or_default();
    let _ = std::fs::remove_file(&log);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let timed: f64 = 3.674932217565499e-6;
    let e_ref: Vec<(&str, f64)> = (calls.lines())
        .filter_map(|l| {
            let (split, stop) = l.split_once(' ')?;
            Some((split, stop.parse::<f64>().ok()?))
        })
        .filter(|&(_, stop)| stop < timed)
        .collect();
    let count = |split: &str| e_ref.iter().filter(|(s, _)| *s == split).count();
    assert_eq!((count("0"), count("1")), (0, 3), "{calls}");
    let abinit = blocks(&record, "abinit");
    assert_eq!(
        abinit[0]["rival"]["layout"]["input"],
        json!([
            "paral_kgb 1",
            "np_spkpt 1",
            "npband 8",
            "npfft 1",
            "nband 8"
        ]),
        "{}",
        abinit[0]
    );
    assert_eq!(abinit.len(), 1, "{record}");
}

#[test]
fn a_claim_runs_each_rival_on_its_frozen_mixing_and_its_record_names_it() {
    let tuned = format!(
        "{TUNED}\n[qe.mixing]\ninput = [\"mixing_beta = 0.3\"]\nreason = \"r\"\n\n[abinit.mixing]\ninput = [\"diemac 10.0\", \"diemix 0.3\"]\nreason = \"r\"\n"
    );
    let pw = format!(
        "sed -n '/^&electrons/,/^\\//p' pw.in | grep -qx '  mixing_beta = 0.3' || exit 3\n{PW}"
    );
    let abinit = format!(
        "grep -qx 'diemac 10.0' run.abi && grep -qx 'diemix 0.3' run.abi || exit 3\n{ABINIT}"
    );
    let root = rivals_tree("rival-mixing", INPUT, &tuned, &pw, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for (code, want) in [
        ("qe", json!(["mixing_beta = 0.3"])),
        ("abinit", json!(["diemac 10.0", "diemix 0.3"])),
    ] {
        for b in blocks(&record, code) {
            assert_eq!(b["rival"]["mixing"], want, "{b}");
        }
    }
}

#[test]
fn the_record_names_each_rivals_stop_floor_and_busy_cores() {
    let tuned = TUNED.replace(
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 1, threads = 8 }",
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 2, threads = 4 }",
    );
    let root = rivals_tree("rival-busy", INPUT, &tuned, PW, ABINIT);
    write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "density_ratio = 2.0\n",
            "density_ratio = 2.0\nstop_floor_ha = 1e-10\nopenmp = false\n",
        ),
    );
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let busy = |code: &str| -> Vec<u64> {
        (blocks(&record, code).iter())
            .map(|b| b["rival"]["busy_cores"].as_u64().unwrap())
            .collect()
    };
    assert_eq!(busy("abinit"), [2], "{record}");
    assert_eq!(busy("qe"), [8], "{record}");
    for b in blocks(&record, "abinit") {
        assert_eq!(b["rival"]["stop_floor_ha"].as_f64(), Some(1e-10), "{b}");
    }
    for b in blocks(&record, "qe") {
        assert!(b["rival"]["stop_floor_ha"].is_null(), "{b}");
    }
}

#[test]
fn orbitar_runs_beside_the_rivals_at_abinits_frozen_cutoff_and_density_ratio() {
    let log = std::env::temp_dir().join(format!("xtask-rival-ours-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let root = rivals_tree(
        "rival-ours",
        INPUT,
        &TUNED.replace(
            "[abinit.cutoff_ha]\nAl = 10.0\nSi = 10.0",
            "[abinit.cutoff_ha]\nAl = 10.0\nSi = 13.0",
        ),
        PW,
        ABINIT,
    );
    let bin = fake(
        &root,
        &format!("grep cutoff_ha input.toml >> '{}'\nstep -2", log.display()),
    );
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    let outcome = session.evaluate("1_a", both);
    assert!(passed(&outcome), "{outcome:?}");
    let seen = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_file(&log);
    let lines: Vec<&str> = seen.lines().collect();
    let own = lines.iter().filter(|l| **l == "cutoff_ha = 1.0").count();
    assert_eq!(own, 2, "{seen}");
    assert!(
        (lines.iter())
            .filter(|l| **l != "cutoff_ha = 1.0")
            .all(|l| *l == "cutoff_ha = 13.0" || *l == "density_cutoff_ha = 26.0"),
        "{seen}"
    );
    assert!(seen.contains("density_cutoff_ha = 26.0"), "{seen}");
}

#[test]
fn orbitar_fails_where_its_timed_cutoff_misses_the_target_that_abinit_meets_there() {
    let abinit = format!("grep -qx '  0.0 5.0 5.0' run.abi || e=-2.0001\n{ABINIT}");
    let root = rivals_tree("rival-accuracy", INPUT, TUNED, PW, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(
            "input: at ABINIT's frozen cutoff, 2d + e = 2 · 1.00000000000"
        ) && why.contains("e-4 + 1e-6 Ha per atom is past the target 3.674932217565499e-5")),
        "{outcome:?}"
    );
    let Outcome::Fail(why) = &outcome else {
        unreachable!()
    };
    assert_eq!(why.matches("at ABINIT's frozen cutoff").count(), 1, "{why}");
    let ours = blocks(&record, "orbitar");
    assert_eq!(ours.len(), 1, "{record}");
    for b in ours {
        assert_eq!(b["accuracy"]["e_ha_per_atom"].as_f64(), Some(1e-6), "{b}");
        assert_eq!(b["accuracy"]["basis"], "own", "{b}");
        assert_eq!(b["accuracy"]["bound"], "2d + e", "{b}");
        assert_eq!(b["accuracy"]["bound_p"], "2 d_P + e_P", "{b}");
    }
    for b in blocks(&record, "abinit") {
        assert_eq!(b["accuracy"]["e_ha_per_atom"].as_f64(), Some(1e-6), "{b}");
        assert_eq!(b["accuracy"]["basis"], "own", "{b}");
    }
    for b in blocks(&record, "qe") {
        assert!(b["accuracy"]["e_ha_per_atom"].is_null(), "{b}");
        assert!(
            b["accuracy"]["target_energy_difference_ha_per_atom"].is_f64(),
            "{b}"
        );
    }
    let root = rivals_tree("rival-unmeasured", INPUT, TUNED, PW, "exit 3");
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(
            "input: at ABINIT's frozen cutoff, the accuracy is unmeasured (d None, e Some(1e-6))"
        )),
        "{outcome:?}"
    );
    let untuned = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]",
        "[[13.0, 1e-6, 0.0, 2e-6, 0.0]]",
    );
    let root = rivals_tree("rival-uncalibrated", INPUT, &untuned, PW, ABINIT);
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(
            "input: at ABINIT's frozen cutoff, the accuracy is unmeasured (d Some(0.0), e None)"
        )),
        "{outcome:?}"
    );
}

#[test]
fn d_is_per_atom_and_2d_plus_e_meets_the_target() {
    let two = INPUT.replace("[\"Si\"]", "[\"Si\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    let abinit = format!("e=-2.00002; grep -qx '  0.0 5.0 5.0' run.abi || e=-2.00005\n{ABINIT}");
    let tuned = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0]",
        "[[10.0, 1e-5, 0.0, 2e-6, 0.0]",
    );
    let root = rivals_tree("rival-d-plus-e", &two, &tuned, PW, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("input: at ABINIT's frozen cutoff, 2d + e = 2 · ")
            && why.contains(" + 1e-5 Ha per atom is past the target")),
        "{outcome:?}"
    );
    let d = blocks(&record, "orbitar")[0]["accuracy"]["d_ha_per_atom"]
        .as_f64()
        .unwrap();
    assert!((d - 1.5e-5).abs() < 1e-12, "{record}");
    let d_e1 = blocks(&record, "orbitar")[0]["accuracy"]["d_e1_ha_per_atom"]
        .as_f64()
        .unwrap();
    assert!((d_e1 - 1e-5).abs() < 1e-12, "{record}");
    let root = rivals_tree("rival-d-plus-e-meets", &two, TUNED, PW, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
}

#[test]
fn the_bound_judges_the_energy_difference_error_and_the_record_keeps_the_absolute_one() {
    let tuned = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0]",
        "[[10.0, 1e-6, 0.0, 5e-5, 0.0]",
    );
    let root = rivals_tree("rival-e-abs", INPUT, &tuned, PW, ABINIT);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for b in blocks(&record, "orbitar")
        .into_iter()
        .chain(blocks(&record, "abinit"))
    {
        assert_eq!(b["accuracy"]["e_ha_per_atom"].as_f64(), Some(1e-6), "{b}");
        assert_eq!(
            b["accuracy"]["e_abs_ha_per_atom"].as_f64(),
            Some(5e-5),
            "{b}"
        );
    }
}

fn pressure_claim(name: &str, e_p: &str, body: &str) -> (Outcome, Value) {
    let tuned = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0]",
        &format!("[[10.0, 1e-6, {e_p}, 2e-6, 0.25421015697]"),
    );
    let root = rivals_tree(name, INPUT, &tuned, PW, ABINIT);
    claim_by(&root, true, body)
}

#[test]
fn orbitars_pressure_must_meet_twice_its_gap_to_abinit_plus_abinits_own_error() {
    let (outcome, record) = pressure_claim("rival-pressure-meets", "0.019", "sig=-1e-5; step -2");
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let a = &blocks(&record, "orbitar")[0]["accuracy"];
    assert!(
        (a["p_orbitar_gpa"].as_f64().unwrap() - 0.29421015697).abs() < 1e-12,
        "{a}"
    );
    assert_eq!(a["p_abinit_gpa"].as_f64(), Some(0.25421015697), "{a}");
    assert!((a["d_p_gpa"].as_f64().unwrap() - 0.04).abs() < 1e-12, "{a}");
    assert_eq!(a["e_p_gpa"].as_f64(), Some(0.019), "{a}");
    assert_eq!(a["pressure_holds"], true, "{a}");
    let (outcome, record) = pressure_claim("rival-pressure-past", "0.021", "sig=-1e-5; step -2");
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("input: at ABINIT's frozen cutoff, 2 d_P + e_P = 2 · 4.0000")
            && why.contains(" + 2.1e-2 GPa is past the target 1e-1")),
        "{outcome:?}"
    );
    assert_eq!(
        blocks(&record, "orbitar")[0]["accuracy"]["pressure_holds"],
        false,
        "{record}"
    );
}

#[test]
fn the_energy_rule_judges_the_gap_in_e_v1_minus_e_v2_not_at_v1_alone() {
    let abinit = format!("grep -qx '  0.0 5.0 5.0' run.abi || e=-2.00005512398\n{ABINIT}");
    let log = std::env::temp_dir().join(format!("xtask-rival-qe-v2-{}", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let pw = format!(
        "grep -q '4\\.8979' pw.in && echo v2 >> '{}'\n{PW}",
        log.display()
    );
    let root = rivals_tree("rival-delta-e", INPUT, TUNED, &pw, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("input: at ABINIT's frozen cutoff, 2d + e = 2 · 5.512398")),
        "{outcome:?}"
    );
    let a = &blocks(&record, "orbitar")[0]["accuracy"];
    assert_eq!(a["d_e1_ha_per_atom"].as_f64(), Some(0.0), "{a}");
    assert_eq!(a["e2_orbitar_ha"].as_f64(), Some(-2.0), "{a}");
    assert_eq!(a["e2_abinit_ha"].as_f64(), Some(-2.00005512398), "{a}");
    assert!(!log.exists(), "QE ran at V₂");
    let root = rivals_tree("rival-delta-e-alike", INPUT, TUNED, PW, &abinit);
    let ours = "if grep -q '4\\.8979' input.toml; then step -2.00005512398; else step -2; fi";
    let (outcome, record) = claim_by(&root, true, ours);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let a = &blocks(&record, "orbitar")[0]["accuracy"];
    assert!(a["d_ha_per_atom"].as_f64().unwrap() < 1e-12, "{a}");
}

#[test]
fn only_e_refs_reference_run_carries_stress() {
    let log = std::env::temp_dir().join(format!("xtask-rival-stress-runs-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&log);
    std::fs::create_dir_all(&log).unwrap();
    let ours = format!(
        "s=plain; grep -qx 'stress = true' input.toml && s=stress\n\
         echo \"$s $(sed -n 's/^energy_tolerance_ha = //p' input.toml)\" >> '{0}/orbitar'\nstep -2",
        log.display()
    );
    let abinit = format!(
        "awk '/^optstress/ {{ print $2 }}' run.abi >> '{}/abinit'\n{ABINIT}",
        log.display()
    );
    let root = rivals_tree("rival-stress-runs", INPUT, TUNED, PW, &abinit);
    let (outcome, record) = claim_by(&root, true, &ours);
    let read = |name: &str| std::fs::read_to_string(log.join(name)).unwrap();
    let (runs, theirs) = (read("orbitar"), read("abinit"));
    let _ = std::fs::remove_dir_all(&log);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let stressed: Vec<f64> = (runs.lines())
        .filter_map(|l| l.strip_prefix("stress "))
        .map(|stop| stop.parse().unwrap())
        .collect();
    assert_eq!(stressed, [3.674932217565499e-9], "{runs}");
    assert!(theirs.lines().all(|l| l == "0"), "{theirs}");
}

#[test]
fn an_own_row_whose_pressure_failed_leaves_the_pressure_unmeasured_and_fails() {
    let tuned = TUNED.replace(
        "[[10.0, 1e-6, 0.0, 2e-6, 0.0]",
        "[[10.0, 1e-6, 0.0, 2e-6, nan]",
    );
    let root = rivals_tree("rival-pressure-nan", INPUT, &tuned, PW, ABINIT);
    let (outcome, record) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(
            "input: at ABINIT's frozen cutoff, the pressure is unmeasured: ABINIT's row at its frozen cutoff holds no pressure"
        )),
        "{outcome:?}"
    );
    assert!(
        blocks(&record, "orbitar")[0]["accuracy"]["pressure_holds"].is_null(),
        "{record}"
    );
}

#[test]
fn an_orbitar_outcome_without_stress_leaves_the_pressure_unmeasured_and_fails() {
    let (outcome, record) = pressure_claim(
        "rival-no-stress",
        "0.019",
        "sed -i 's/^stress = true$//' input.toml; step -2",
    );
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains(
            "input: at ABINIT's frozen cutoff, the pressure is unmeasured: orbitaR's reference run: the outcome holds no stress_ha_bohr3 [3][3]"
        )),
        "{outcome:?}"
    );
    let a = &blocks(&record, "orbitar")[0]["accuracy"];
    assert!(
        a["pressure_holds"].is_null() && a["p_orbitar_gpa"].is_null(),
        "{a}"
    );
}

#[test]
fn a_borrowed_pressure_error_is_nan_when_one_eligible_cells_is() {
    let rows = "\"benchmarks/1_a/input.toml\" = [[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]";
    let tuned = TUNED.replace(
        rows,
        &format!(
            "{rows}\n\"benchmarks/9_c/c.toml\" = [[10.0, 2e-6, 0.0, 2e-6, 0.0]]\n\
             \"benchmarks/9_c/g.toml\" = [[10.0, 1e-6, nan, 2e-6, 0.0]]"
        ),
    );
    let abinit = format!(
        "{ABINIT}\nif grep -q 'optstress 1' run.abi; then echo '-Cartesian components of stress tensor (GPa)         [Pressure=  0.0E+00 GPa]'; fi"
    );
    let root = rivals_tree("rival-borrowed-nan", INPUT, &tuned, PW, &abinit);
    write(&root, "benchmarks/9_c/c.toml", INPUT);
    write(&root, "benchmarks/9_c/g.toml", INPUT);
    write(
        &root,
        "benchmarks/1_a/inputs/b.toml",
        &INPUT.replace("0.25]]", "0.3]]"),
    );
    let (outcome, record) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(why) if why.contains("b: at ABINIT's frozen cutoff, 2 d_P + e_P = ")),
        "{outcome:?}"
    );
    for b in blocks(&record, "orbitar")
        .into_iter()
        .filter(|b| b["input"] == "b")
    {
        assert!(b["accuracy"]["e_p_gpa"].is_null(), "{b}");
        assert_eq!(b["accuracy"]["pressure_holds"], false, "{b}");
    }
}

#[test]
fn a_cell_without_its_own_calibration_row_borrows_the_largest_row_of_cells_with_its_elements() {
    let rows = "\"benchmarks/1_a/input.toml\" = [[10.0, 1e-6, 0.0, 2e-6, 0.0], [13.0, 1e-6, 0.0, 2e-6, 0.0]]";
    let tuned = TUNED.replace(
        rows,
        &format!(
            "{rows}\n\"benchmarks/9_c/c.toml\" = [[10.0, 2e-6, 0.0, 2e-6, 0.0]]\n\"benchmarks/9_c/d.toml\" = [[10.0, 2e-6, 0.0, 2e-6, 0.0]]\n\
             \"benchmarks/9_c/alsi.toml\" = [[10.0, 5e-6, 0.0, 2e-6, 0.0]]\n\"benchmarks/9_c/e.toml\" = [[10.0, 1e-6, 0.05, 2e-6, 0.0]]\n\"benchmarks/9_c/f.toml\" = [[10.0, nan, 0.09, 2e-6, 0.0]]"
        ),
    );
    let abinit = format!(
        "{ABINIT}\nif grep -q 'optstress 1' run.abi; then echo '-Cartesian components of stress tensor (GPa)         [Pressure=  0.0E+00 GPa]'; fi"
    );
    let root = rivals_tree("rival-borrowed", INPUT, &tuned, PW, &abinit);
    let alsi = INPUT.replace("[\"Si\"]", "[\"Al\", \"Si\"]").replace(
        "[[0.0, 0.0, 0.25]]",
        "[[0.0, 0.0, 0.0], [0.25, 0.25, 0.25]]",
    );
    write(&root, "benchmarks/9_c/c.toml", INPUT);
    write(&root, "benchmarks/9_c/d.toml", INPUT);
    write(&root, "benchmarks/9_c/e.toml", INPUT);
    write(&root, "benchmarks/9_c/f.toml", INPUT);
    write(&root, "benchmarks/9_c/alsi.toml", &alsi);
    write(
        &root,
        "benchmarks/1_a/inputs/b.toml",
        &INPUT.replace("0.25]]", "0.3]]"),
    );
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for code in ["orbitar", "abinit"] {
        for b in blocks(&record, code) {
            let (e, basis) = match b["input"].as_str() {
                Some("b") => (2e-6, "borrowed from benchmarks/9_c/c.toml"),
                _ => (1e-6, "own"),
            };
            assert_eq!(b["accuracy"]["e_ha_per_atom"].as_f64(), Some(e), "{b}");
            assert_eq!(b["accuracy"]["basis"], basis, "{b}");
        }
    }
    for b in blocks(&record, "orbitar") {
        let from = match b["input"].as_str() {
            Some("b") => "its reference run at c",
            _ => "its calibration row",
        };
        assert_eq!(b["accuracy"]["p_abinit_from"], from, "{b}");
        let e_p = if b["input"] == "b" { 0.09 } else { 0.0 };
        assert_eq!(b["accuracy"]["e_p_gpa"].as_f64(), Some(e_p), "{b}");
        assert_eq!(b["accuracy"]["pressure_holds"], true, "{b}");
    }
    for b in blocks(&record, "abinit") {
        assert_eq!(b["e_ref_ha"].as_f64(), Some(-2.0), "{b}");
    }
}

#[test]
fn a_spin_inputs_pressure_carries_a_note_until_the_stress_case_holds_a_spin_input() {
    let spin = format!("{INPUT}\n[spin]\nmoments = [1.0]\n");
    let root = rivals_tree("rival-spin-note", &spin, TUNED, PW, ABINIT);
    let (outcome, record) = claim(&root, true);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let a = &blocks(&record, "orbitar")[0]["accuracy"];
    assert_eq!(a["spin"], true, "{a}");
    assert_eq!(
        a["note"], "orbitaR's spin stress has no derivative case yet",
        "{a}"
    );
    assert_eq!(a["pressure_holds"], true, "{a}");
    write(
        &root,
        "benchmarks/15_stress_derivative/inputs/fe.toml",
        &spin,
    );
    let (_, record) = claim(&root, true);
    assert!(
        blocks(&record, "orbitar")[0]["accuracy"]["note"].is_null(),
        "{record}"
    );
}

#[test]
fn the_claim_judges_orbitar_at_the_inputs_own_cutoff_and_times_it_at_abinits() {
    let log = std::env::temp_dir().join(format!("xtask-rival-judged-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let (pw, abinit) = (format!("t=0.1\n{PW}"), format!("t=0.1\n{ABINIT}"));
    let root = rivals_tree("rival-judged", INPUT, TUNED, &pw, &abinit);
    let body = format!(
        "{}\nstep -2\ngrep -x 'cutoff_ha = .*' input.toml >> '{}'\n\
         e=0.6; grep -qx 'cutoff_ha = 1.0' input.toml && e=0.5 && sleep 2\n\
         st=''; grep -qx 'stress = true' input.toml && st=', \"stress_ha_bohr3\": [[0, 0, 0], [0, 0, 0], [0, 0, 0]]'\n\
         printf '{{\"eigenvalues_ha\": [[[%s]]], \"basis_sizes\": [1], \"free_energy_ha\": %s%s}}' \"$e\" \"$f\" \"$st\"",
        prelude(),
        log.display()
    );
    let bin = crate::harness::engine(&root, &body);
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"));
    let outcome = session.evaluate("1_a", both);
    let seen = std::fs::read_to_string(&log).unwrap();
    let _ = std::fs::remove_file(&log);
    assert!(passed(&outcome), "{outcome:?}");
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap())
            .unwrap();
    assert_eq!(record["metrics"]["error"].as_f64(), Some(0.0), "{record}");
    let sum = |key: &str| -> f64 {
        record[key]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x.as_f64().unwrap())
            .sum()
    };
    let spent = record["spent_s"].as_f64().unwrap();
    assert!(spent > sum("runs_s") + sum("rotation_s") + 1.9, "{record}");
    let at = |c: &str| seen.lines().filter(|l| *l == c).count();
    assert_eq!(at("cutoff_ha = 1.0"), 2, "{seen}");
    assert!(at("cutoff_ha = 10.0") > 4, "{seen}");
}

#[test]
fn an_input_key_without_a_rival_meaning_stops_the_claim_by_name() {
    let input = INPUT.replace("width_ha = 0.01\n", "width_ha = 0.01\ntemperature = 1\n");
    let root = rivals_tree("rival-key", &input, TUNED, PW, ABINIT);
    let (outcome, _) = claim(&root, true);
    assert!(
        matches!(&outcome, Outcome::Fail(w) if w.contains("rivals: the input's occupations.temperature has no rival meaning; add it to rival.rs READ or UNUSED")),
        "{outcome:?}"
    );
}

#[test]
fn a_rival_reference_never_asks_for_a_stop_below_its_floor() {
    let log = std::env::temp_dir().join(format!("xtask-rival-floor-{}.log", std::process::id()));
    let abinit = format!(
        "s=$(awk '/^toldfe/ {{ print $2 }}' run.abi); echo \"$s\" >> '{}'\n\
         awk -v s=\"$s\" 'BEGIN {{ exit !(s < 1e-9) }}' && echo ' nstep 100 was not enough SCF cycles to converge'\n{ABINIT}",
        log.display()
    );
    let lost = |b: &&Value| b.get("median_t_delta_s").is_none();
    let root = rivals_tree("rival-floorless", INPUT, TUNED, PW, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(passed_unmeasured(&outcome), "{outcome:?}: {record}");
    assert!(blocks(&record, "abinit").iter().all(lost), "{record}");
    let _ = std::fs::remove_file(&log);
    let root = rivals_tree("rival-floor", INPUT, TUNED, PW, &abinit);
    write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "density_ratio = 2.0\n",
            "density_ratio = 2.0\nstop_floor_ha = 1e-9\n",
        ),
    );
    let (outcome, record) = claim(&root, true);
    let stops: Vec<f64> = (std::fs::read_to_string(&log).unwrap().lines())
        .map(|l| l.parse().unwrap())
        .collect();
    let _ = std::fs::remove_file(&log);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    for b in blocks(&record, "abinit") {
        assert!(!lost(&b), "{b}");
        assert_eq!(b["e_ref_ha"].as_f64(), Some(-2.0), "{b}");
    }
    let least = stops.iter().copied().fold(f64::INFINITY, f64::min);
    assert_eq!(least, 1e-9, "{stops:?}");
    assert!(stops.contains(&1e-8), "{stops:?}");
    assert!(stops.contains(&0.001), "{stops:?}");
    assert!(blocks(&record, "qe").iter().all(|b| !lost(b)), "{record}");
}

#[test]
fn a_stop_floor_whose_tenfold_passes_the_e_ref_bound_stops_the_claim_by_name() {
    let log =
        std::env::temp_dir().join(format!("xtask-rival-floor-runs-{}.log", std::process::id()));
    let logged = |fake: &str| format!("echo run >> '{}'\n{fake}", log.display());
    for (floor, refused, stop) in [
        ("1e-8", true, "1e-7"),
        ("3.674932217565499e-9", false, ""),
        ("3.6749322175654997e-9", true, "3.6749322175654996e-8"),
    ] {
        let _ = std::fs::remove_file(&log);
        let (pw, abinit) = (logged(PW), logged(ABINIT));
        let root = rivals_tree(&format!("rival-floor-{floor}"), INPUT, TUNED, &pw, &abinit);
        write(
            &root,
            "benchmarks/RIVALS.toml",
            &RIVALS.replace(
                "density_ratio = 2.0\n",
                &format!("density_ratio = 2.0\nstop_floor_ha = {floor}\n"),
            ),
        );
        let (outcome, record) = claim(&root, true);
        let named = matches!(&outcome, Outcome::Fail(why) if why.contains(&format!(
            "abinit: input at 8 threads: its stop floor {floor} Ha puts E_ref's stop at TIGHTER × floor = {stop} Ha, \
             past the bound 3.674932217565499e-8 Ha"
        )));
        assert_eq!(named, refused, "{outcome:?}: {record}");
        assert_eq!(
            log.exists(),
            !refused,
            "{floor}: a rival ran before the refusal"
        );
        if !refused {
            assert!(passed(&outcome), "{outcome:?}: {record}");
        }
    }
    let _ = std::fs::remove_file(&log);
}

#[test]
fn a_long_run_may_differ_from_its_own_clock_by_its_share() {
    let pw = PW.replace("${t}s WALL", "3.185s WALL");
    let one = INPUT
        .replace("mesh = [2, 2, 2]", "mesh = [1, 1, 1]")
        .replace("shift = [0.5, 0.5, 0.5]\n", "");
    let root = rivals_tree("tune-share", &one, TUNED, &format!("t=3\n{pw}"), ABINIT);
    let env = Env {
        rivals: Some(root.join("rivals")),
        ..crate::env(false)
    };
    let findings = xtask::run(&["rivals", "layout", "qe"], &root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    let tuned: toml::Table = std::fs::read_to_string(root.join("benchmarks/TUNED.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let lines: Vec<&str> = (tuned["qe"]["searched"].as_table().unwrap().values())
        .flat_map(|v| v.as_array().unwrap().iter().map(|l| l.as_str().unwrap()))
        .collect();
    assert_eq!(lines.len(), 6, "{lines:?}");
    assert!(lines.iter().all(|l| !l.contains("failed")), "{lines:?}");
}

#[test]
fn the_energy_target_is_ten_times_the_delta_of_t_delta() {
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap();
    let rivals: toml::Table = std::fs::read_to_string(root.join("benchmarks/RIVALS.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let delta = 1e-4 / 27.211_386_245_988;
    assert_eq!(
        rivals["target"]["energy_difference_ha_per_atom"].as_float(),
        Some(10.0 * delta)
    );
}

#[test]
fn a_rival_block_that_lost_every_run_is_its_loss_and_never_refuses_the_claim() {
    let abinit = format!("echo ' nstep 100 was not enough SCF cycles to converge'\n{ABINIT}");
    let root = rivals_tree("rival-lost-runs", INPUT, TUNED, PW, &abinit);
    let (outcome, record) = claim(&root, true);
    assert!(passed_unmeasured(&outcome), "{outcome:?}: {record}");
    for b in blocks(&record, "abinit") {
        assert_eq!(b["losses"].as_array().map(Vec::len), Some(5), "{b}");
        assert_eq!(b["noisy"], false, "{b}");
    }
}

#[test]
fn a_rival_block_that_lost_some_runs_counts_them_as_its_losses_not_as_a_refusal() {
    let count = std::env::temp_dir().join(format!("xtask-rival-some-{}", std::process::id()));
    let pw = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); echo $((n + 1)) > '{0}'\n\
         [ $n -ge 3 ] && [ $((n % 3)) = 0 ] && {{ echo 'Error in routine cdiaghg' >&2; exit 1; }}\n{PW}",
        count.display()
    );
    let root = rivals_tree("rival-some-runs", INPUT, TUNED, &pw, ABINIT);
    let (outcome, record) = claim(&root, true);
    let _ = std::fs::remove_file(&count);
    assert!(passed(&outcome), "{outcome:?}: {record}");
    let mut partial = 0;
    for b in blocks(&record, "qe") {
        let (n, lost) = (
            b["t_delta_s"].as_array().unwrap().len(),
            b["losses"].as_array().unwrap().len(),
        );
        assert_eq!(n + lost, 5, "{b}");
        if lost > 0 && n >= 2 {
            partial += 1;
        }
    }
    assert!(
        partial > 0,
        "no QE block lost some but not all of its runs: {record}"
    );
}

#[test]
fn a_rival_block_noisy_over_its_first_runs_that_lost_the_rest_keeps_its_cv_and_its_losses_in_g() {
    let dir = std::env::temp_dir().join(format!("xtask-rival-cv-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pw = format!(
        "if [ \"$OMP_NUM_THREADS\" = 8 ]; then c='{0}/n'; k=$(cat \"$c\" 2>/dev/null || echo 0); echo $((k + 1)) > \"$c\"\n\
         [ $k -ge 8 ] && {{ echo 'Error in routine cdiaghg' >&2; exit 1; }}\n\
         [ $((k % 2)) = 0 ] && t=0.4; fi\n{PW}",
        dir.display()
    );
    let root = rivals_tree("rival-cv-five", INPUT, TUNED, &pw, ABINIT);
    let (outcome, record) = claim(&root, true);
    let _ = std::fs::remove_dir_all(&dir);
    let b = *blocks(&record, "qe")
        .iter()
        .find(|b| b["threads"] == 8)
        .unwrap();
    assert_eq!(b["t_delta_s"].as_array().unwrap().len(), 5, "{b}");
    assert_eq!(b["losses"].as_array().unwrap().len(), 5, "{b}");
    assert!(b["cv_t_delta"].as_f64().unwrap() > 0.05, "{b}");
    assert_eq!(b["noisy"], false, "{b}");
    let qe = (record["versus"].as_array().unwrap().iter())
        .find(|v| v["code"] == "qe")
        .unwrap();
    assert_eq!(qe["pairs"][0]["losses"], 5, "{qe}");
    assert!(
        !matches!(&outcome, Outcome::Fail(why) if why.contains("the judge refuses")),
        "{outcome:?}"
    );
}

#[test]
fn the_series_holds_each_codes_declared_sizes_alone_with_a_status_for_each_and_keeps_a_failed_runs_peak()
 {
    let pw = format!("dd if=/dev/zero of=/dev/null bs=30000000 count=1 2>/dev/null\n{PW}\nexit 7");
    let root = rivals_tree("rival-series", INPUT, TUNED, &pw, ABINIT);
    let case = CASE
        .replace("\"active\"", "\"planned\"")
        .replace("\"correctness\"", "\"correctness\", \"memory\"");
    write(&root, "benchmarks/1_a/case.toml", &case);
    let two = INPUT
        .replace("species = [\"Si\"]", "species = [\"Si\", \"Si\"]")
        .replace(
            "[[0.0, 0.0, 0.25]]",
            "[[0.0, 0.0, 0.25], [0.25, 0.25, 0.5]]",
        );
    write(&root, "benchmarks/1_a/inputs/b.toml", &two);
    write(
        &root,
        "benchmarks/RIVALS.toml",
        &RIVALS.replace(
            "density_ratio = 8.0\n",
            "density_ratio = 8.0\nskip_cells = [{ cell = \"benchmarks/1_a/inputs/b.toml\", reason = \"no dataset\" }]\n",
        ),
    );
    let held = std::env::temp_dir().join(format!("xtask-series-variants-{}", std::process::id()));
    write(
        &held,
        "1_a/b.toml",
        &two.replace("cutoff_ha = 1.0", "cutoff_ha = 1.5"),
    );
    let bin = fake(&root, "step -2");
    let session = Session::new(&root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"))
        .with_held_out(&held);
    let _ = session.evaluate("1_a", both);
    let _ = std::fs::remove_dir_all(&held);
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap())
            .unwrap();
    let series = record["series"].as_array().unwrap();
    let codes: Vec<&str> = series.iter().map(|s| s["code"].as_str().unwrap()).collect();
    assert_eq!(codes, ["orbitar", "qe", "abinit"], "{record}");
    for entry in series {
        let inputs: Vec<&str> = (entry["points"].as_array().unwrap().iter())
            .map(|p| p["input"].as_str().unwrap())
            .collect();
        assert_eq!(inputs, ["input", "b"], "{record}");
    }
    let qe = &series[1];
    assert_eq!(qe["coverage"], "partial", "{record}");
    assert!(
        qe["points"][0]["status"]
            .as_str()
            .unwrap()
            .starts_with("lost"),
        "{record}"
    );
    assert_eq!(qe["points"][1]["status"], "skipped: no dataset", "{record}");
    assert!(qe["t_delta_exponent"].is_string(), "{record}");
    let failures = record["failures"].to_string();
    assert!(
        failures.contains("series: a full-series claim needs every code at every declared size")
            && failures.contains("qe b: skipped: no dataset"),
        "{record}"
    );
    let lost = (record["speed"].as_array().unwrap().iter())
        .find(|b| b["code"] == "qe" && b["input"] == "input")
        .unwrap();
    let kept = numbers_of(&lost["failed_peak_bytes"]);
    assert!(
        !kept.is_empty() && kept.iter().all(|&b| b > 20_000_000),
        "{lost}"
    );
}

fn numbers_of(value: &Value) -> Vec<u64> {
    (value.as_array().into_iter().flatten())
        .filter_map(Value::as_u64)
        .collect()
}

fn memory_rivals(name: &str, input: &str, tuned: &str, pw: &str, abinit: &str) -> Tree {
    let root = rivals_tree(name, input, tuned, pw, abinit);
    let case = CASE
        .replace("\"active\"", "\"planned\"")
        .replace("\"correctness\"", "\"correctness\", \"memory\"");
    write(&root, "benchmarks/1_a/case.toml", &case);
    root
}

fn guarded(root: &Tree, body: &str, guard: f64) -> Value {
    let bin = fake(root, body);
    let _ = Session::new(root, &bin, Tier::Claim, &root.join("records"))
        .with_rivals(&root.join("rivals"))
        .with_memory_guard(guard)
        .evaluate("1_a", both);
    serde_json::from_str(&std::fs::read_to_string(root.join("records/1_a.json")).unwrap()).unwrap()
}

#[test]
fn a_rival_run_that_misses_t_delta_is_a_loss_that_keeps_its_peak() {
    let count =
        std::env::temp_dir().join(format!("xtask-rival-drift-{}.count", std::process::id()));
    let _ = std::fs::remove_file(&count);
    let pw = format!(
        "n=$(cat '{0}' 2>/dev/null || echo 0); echo $((n + 1)) > '{0}'\n\
         dd if=/dev/zero of=/dev/null bs=30000000 count=1 2>/dev/null\n\
         [ \"$n\" -ge 2 ] && e=-3.0\n{PW}",
        count.display()
    );
    let root = memory_rivals("rival-drift", INPUT, TUNED, &pw, ABINIT);
    let record = guarded(&root, "step -2", 14e9);
    let _ = std::fs::remove_file(&count);
    let qe = blocks(&record, "qe");
    assert!(!qe[0]["losses"].as_array().unwrap().is_empty(), "{record}");
    let kept = numbers_of(&qe[0]["failed_peak_bytes"]);
    assert!(
        !kept.is_empty() && kept.iter().all(|&b| b > 20_000_000),
        "{}",
        qe[0]
    );
}

fn own_cutoff_runs(name: &str, heavy: &str) -> (Value, usize) {
    let log = std::env::temp_dir().join(format!("xtask-{name}-{}.log", std::process::id()));
    let _ = std::fs::remove_file(&log);
    let root = memory_rivals(name, INPUT, TUNED, PW, ABINIT);
    let body = format!(
        "grep -qx 'cutoff_ha = 1.0' input.toml && echo own >> '{}'\n\
         if {heavy}; then dd if=/dev/zero of=/dev/null bs=100000000 count=1 2>/dev/null; fi\nstep -2",
        log.display()
    );
    let record = guarded(&root, &body, 50e6);
    let own = std::fs::read_to_string(&log)
        .unwrap_or_default()
        .lines()
        .count();
    let _ = std::fs::remove_file(&log);
    (record, own)
}

#[test]
fn a_measured_peak_past_the_guard_at_v2_or_correctness_stops_the_later_orbitar_launches() {
    let (record, own) = own_cutoff_runs("rival-v2-over", "! grep -q '5.0' input.toml");
    let ours = blocks(&record, "orbitar");
    assert!(
        ours[0]["refused"]
            .as_str()
            .unwrap_or_default()
            .contains("refused after its V₂ run"),
        "{record}"
    );
    assert_eq!(own, 0, "{record}");
    let (record, own) = own_cutoff_runs("rival-own-over", "grep -qx 'cutoff_ha = 1.0' input.toml");
    let ours = blocks(&record, "orbitar");
    assert!(
        ours[0]["refused"]
            .as_str()
            .unwrap_or_default()
            .contains("refused after its correctness run"),
        "{record}"
    );
    assert_eq!(own, 1, "{record}");
    assert!(
        record["failures"]
            .to_string()
            .contains("refused after its correctness run"),
        "{record}"
    );
}

#[test]
fn a_fitted_abinit_layout_keeps_its_smaller_sizes_history_and_records_its_fitted_lines() {
    let input = INPUT.replace("\n[bands]\ncount = 8\n", "");
    let tuned = TUNED.replace(
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 1, threads = 8 }",
        "[abinit.layout]\n1 = { ranks = 1, threads = 1 }\n2 = { ranks = 1, threads = 2 }\n4 = { ranks = 1, threads = 4 }\n8 = { ranks = 8, threads = 1, input = [\"paral_kgb 1\", \"np_spkpt 1\", \"npband 8\", \"npfft 1\"] }",
    );
    let abinit = format!(
        "grep -qx 'nband 8' run.abi && dd if=/dev/zero of=/dev/null bs=40000000 count=1 2>/dev/null\n{ABINIT}"
    );
    let root = memory_rivals("rival-fitted", &input, &tuned, PW, &abinit);
    let four = input
        .replace(
            "species = [\"Si\"]",
            "species = [\"Si\", \"Si\", \"Si\", \"Si\"]",
        )
        .replace(
            "[[0.0, 0.0, 0.25]]",
            "[[0.0, 0.0, 0.25], [0.25, 0.25, 0.5], [0.5, 0.5, 0.75], [0.75, 0.75, 0.0]]",
        );
    write(&root, "benchmarks/1_a/inputs/b.toml", &four);
    let record = guarded(&root, "step -2", 300e6);
    let abinit = blocks(&record, "abinit");
    let of = |input: &str| abinit.iter().find(|b| b["input"] == input).unwrap();
    assert_eq!(
        of("input")["rival"]["fitted"],
        json!(["nband 8"]),
        "{record}"
    );
    assert_eq!(of("b")["rival"]["fitted"], json!(["nband 16"]), "{record}");
    assert!(
        of("b")["refused"]
            .as_str()
            .unwrap_or_default()
            .contains("abinit: b at 8 threads: refused before it runs"),
        "{record}"
    );
}
