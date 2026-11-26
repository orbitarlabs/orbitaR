use crate::{Tree, env, has, tree, write};
use sha2::{Digest, Sha256};
use std::{
    f64::consts::PI,
    ffi::OsStr,
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
    sync::OnceLock,
};
use xtask::Env;

const URL: &str =
    "https://conda.anaconda.org/conda-forge/linux-64/atompaw-4.2.0.4-h8876d29_0.conda";
const PACKAGE: &str = "6b28f546e012636620d811f08a87a4bdda074a6a75e3819dfdbb1b3959ccf13f";
const LIBXC: &str = "https://conda.anaconda.org/conda-forge/linux-64/libxc-4.3.4-h1234567_0.conda";
const DATE: &str = "11/30/2025, 11:46:02.695";
const CONTACT: &str = "  Contact info: ";
const INPUT: &str = "O 8\nXMLOUT\nEND\n";
const VALENCE: &str = "<state n=\"2\" l=\"0\" e=\"-0.88\"/>";
const LIBRARY: &str = "lib/libx.so.1";
const FAMILY: &str = "orbitaR-PBE-v0";
const FILE: &str = "orbitaR-PBE-v0/O.xml";
const COMMITTED: &str = "datasets/orbitaR-PBE-v0/O.xml";
const RECORD: &str = "target/atompaw/orbitaR-PBE-v0/O.json";
const RADIUS: &str = "<paw_radius rc=\"1.41\" shift=\"0.0\"/>";
const PROJECTOR: &str =
    "<projector_function state=\"O-2s\">1.0E+00 0.0E+00 -2.5E-01</projector_function>";

fn date_program() -> &'static str {
    static PROGRAM: OnceLock<String> = OnceLock::new();
    PROGRAM.get_or_init(|| date_for(&std::env::var_os("PATH").unwrap_or_default()))
}

fn millis(program: &str, path: &OsStr) -> bool {
    Command::new(program)
        .arg("+%3N")
        .env("PATH", path)
        .output()
        .is_ok_and(|out| {
            let text = String::from_utf8_lossy(&out.stdout);
            let text = text.trim_end();
            text.len() == 3 && text.bytes().all(|b| b.is_ascii_digit())
        })
}

const FAKE_PATH: &str = "/usr/bin:/bin";

fn date_for(search: &OsStr) -> String {
    let fixed = OsStr::new(FAKE_PATH);
    if millis("date", fixed) {
        return "date".into();
    }
    let gnu = (std::env::split_paths(search))
        .flat_map(|dir| [dir.join("gdate"), dir.join("date")])
        .filter(|path| path.is_absolute() && path.is_file())
        .filter_map(|path| path.to_str().map(str::to_owned))
        .find(|path| millis(path, fixed));
    gnu.map_or_else(
        || "date".into(),
        |path| format!("'{}'", path.replace('\'', "'\\''")),
    )
}

fn date_line() -> String {
    format!("$({} '+%m/%d/%Y, %H:%M:%S.%3N')", date_program())
}

fn sha(bytes: &[u8]) -> String {
    crate::harness::hex(&Sha256::digest(bytes))
}

fn xml(contact: &str, date: &str, input: &str) -> String {
    format!(
        "<fake_xml>\n<!-- Atompaw 4.2.0.4\n  Energy units=Hartree, length units=bohr\n{contact}  PAW functions generated on {date}\n-->\n<valence_states>\n\
         {VALENCE}\n</valence_states>\n{RADIUS}\n{PROJECTOR}\n<input>{input}</input>\n</fake_xml>\n"
    )
}

fn address() -> String {
    ["author", "example.org"].join("@")
}

fn contact() -> String {
    format!(
        "{CONTACT}A. Author\n  email: {}, web: example.org\n",
        address()
    )
}

fn built() -> String {
    xml("", DATE, INPUT.trim_end())
}

fn rows(energies: impl Iterator<Item = f64>, paw: impl Fn(f64, f64) -> f64) -> String {
    energies
        .map(|e| {
            let ae = -0.3 * (e + 5.0);
            let p = paw(e, ae);
            format!(
                "{e:12.4e}{:12.4e}{:12.4e}{ae:12.4e}{p:12.4e}\n",
                ae.tan(),
                p.tan()
            )
        })
        .collect()
}

fn grid(from: f64, step: f64, count: u32) -> impl Iterator<Item = f64> {
    (0..count).map(move |i| from + step * f64::from(i))
}

fn logderiv(paw: impl Fn(f64, f64) -> f64) -> String {
    rows(grid(-5.0, 0.05, 200), paw)
}

fn good() -> [String; 2] {
    [logderiv(|_, ae| ae + 0.01), logderiv(|_, ae| ae - 0.02)]
}

fn script(root: &Path, body: &str) -> String {
    let prefix = root.join("prefix");
    format!(
        "#!/bin/sh\np=$PATH\nPATH={FAKE_PATH}\ntest \"$p\" = {prefix}/bin || exit 7\n\
         test \"$OPENBLAS_CORETYPE\" = HASWELL || exit 8\ntest \"$OPENBLAS_NUM_THREADS\" = 1 || exit 8\n\
         names=$(env | cut -d= -f1 | grep -vxE 'PATH|PWD|OLDPWD|SHLVL|_' | sort | tr '\\n' ' ')\n\
         test \"$names\" = 'OPENBLAS_CORETYPE OPENBLAS_NUM_THREADS ' || exit 9\n{body}\n\
         cat > O.GGA_X_PBE+GGA_C_PBE-paw.xml <<EOF\n{}EOF\ncp {root}/waves/logderiv.* .\n",
        xml(&contact(), &date_line(), "$(cat)"),
        prefix = prefix.display(),
        root = root.display()
    )
}

fn set_script(root: &Path, text: &str) {
    write(root, "prefix/bin/atompaw", text);
    fs::set_permissions(
        root.join("prefix/bin/atompaw"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let pins = fs::read_to_string(root.join("benchmarks/DATASETS.toml")).unwrap();
    let line = |sha: &str| format!("\"bin/atompaw\" = \"{sha}\"\n");
    let old = pins
        .lines()
        .find(|l| l.starts_with("\"bin/atompaw\""))
        .map(|l| format!("{l}\n"));
    let pins = match old {
        Some(old) => pins.replace(&old, &line(&sha(text.as_bytes()))),
        None => pins,
    };
    write(root, "benchmarks/DATASETS.toml", &pins);
}

fn edit_script(root: &Path, from: &str, to: &str) {
    let text = fs::read_to_string(root.join("prefix/bin/atompaw")).unwrap();
    assert!(text.contains(from), "{from}");
    set_script(root, &text.replace(from, to));
}

fn case(name: &str, pin: &str, body: &str, waves: [&str; 2]) -> (Tree, Env) {
    let pins = format!(
        "[atompaw]\nversion = \"4.2.0.4\"\n\
         build = \"h8876d29_0\"\nsha256 = \"{PACKAGE}\"\npinned_date = \"{DATE}\"\npackages = [\n  \
         {{ url = \"{URL}\", sha256 = \"{PACKAGE}\" }},\n  {{ url = \"{LIBXC}\", sha256 = \"{}\" }},\n]\n\n\
         [atompaw.binaries]\n\"bin/atompaw\" = \"\"\n\"{LIBRARY}\" = \"{}\"\n",
        "a".repeat(64),
        sha(b"library")
    );
    let root = tree(
        name,
        &[
            ("benchmarks/DATASETS.toml", &pins),
            ("datasets/jth-2.0.toml", "url = \"u\"\n"),
            (&format!("datasets/{FAMILY}/O.in"), INPUT),
            (COMMITTED, &built()),
            ("waves/logderiv.0", waves[0]),
            ("waves/logderiv.1", waves[1]),
            ("prefix/lib/libx.so.1.0", "library"),
        ],
    );
    if !pin.is_empty() {
        write(&root, &format!("datasets/{FAMILY}.toml"), &table(pin));
    }
    symlink("libx.so.1.0", root.join("prefix").join(LIBRARY)).unwrap();
    set_script(&root, &script(&root, body));
    meta(&root.join("prefix"), "atompaw", URL, PACKAGE);
    meta(&root.join("prefix"), "libxc", LIBXC, &"a".repeat(64));
    let env = Env {
        atompaw: Some(root.join("prefix")),
        ..env(false)
    };
    (root, env)
}

fn table(pin: &str) -> String {
    format!(
        "base = \"jth-2.0\"\nfunctional = \"pbe\"\n\n[element]\n\
         O = {{ file = \"{FILE}\", sha256 = \"{pin}\", cutoff_hint_ha = 17.5 }}\n"
    )
}

fn meta(prefix: &Path, name: &str, url: &str, sha256: &str) {
    let json = format!("{{\"name\": \"{name}\", \"url\": \"{url}\", \"sha256\": \"{sha256}\"}}");
    write(prefix, &format!("conda-meta/{name}.json"), &json);
}

fn atompaw(root: &Path, env: &Env) -> Vec<String> {
    xtask::run(&["atompaw"], root, env)
}

fn with_wave(name: &str, wave: &str) -> Vec<String> {
    let (root, env) = case(name, &sha(built().as_bytes()), "", [wave, &good()[1]]);
    atompaw(&root, &env)
}

#[test]
fn a_rebuild_of_the_committed_file_passes_with_its_record() {
    let [s, p] = good();
    let (root, env) = case("atompaw-pass", &sha(built().as_bytes()), "", [&s, &p]);
    let findings = atompaw(&root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
    assert!(!root.join("target/datasets").exists());
    let record: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(root.join(RECORD)).unwrap()).unwrap();
    assert_eq!(record["file"], FILE);
    assert_eq!(record["sha256"], sha(built().as_bytes()));
    assert_eq!(record["input_sha256"], sha(INPUT.as_bytes()));
    assert_eq!(record["lmax"], 0);
    assert_eq!(record["lowest_valence_ry"], -1.76);
    assert_eq!(record["window_bound_ry"], serde_json::json!([-5.0, 4.95]));
    assert_eq!(record["rebuild_bound_rel"], 1e-6);
    assert_eq!(record["rebuild_largest_rel"], 0.0);
    let waves = record["logderiv"].as_array().unwrap();
    assert_eq!(waves.len(), 2);
    assert_eq!(waves[0]["window_ry"], serde_json::json!([-5.0, 4.95]));
    assert_eq!(waves[0]["points"], 200);
    assert!((waves[0]["step_ry"].as_f64().unwrap() - 0.05).abs() < 1e-9);
    assert!((waves[0]["gap_rad"].as_f64().unwrap() - 0.01).abs() < 2e-4);
    assert!((waves[1]["gap_rad"].as_f64().unwrap() - 0.02).abs() < 2e-4);
    assert_eq!(
        (
            waves[0]["poles_ae"].as_u64(),
            waves[0]["poles_paw"].as_u64()
        ),
        (Some(1), Some(1))
    );
}

#[test]
fn atompaw_runs_with_the_fixed_blas_kernel_and_nothing_inherited() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-environment",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    assert!(atompaw(&root, &env).is_empty());
    for (from, to, code) in [
        ("= HASWELL", "= SKYLAKEX", 8),
        ("'OPENBLAS_CORETYPE ", "'HOME OPENBLAS_CORETYPE ", 9),
        ("/prefix/bin ||", "/elsewhere/bin ||", 7),
    ] {
        edit_script(&root, from, to);
        let findings = atompaw(&root, &env);
        assert!(
            has(&findings, &format!("atompaw exit status: {code}")),
            "{to}: {findings:?}"
        );
        edit_script(&root, to, from);
    }
}

#[test]
fn two_builds_that_differ_are_a_finding() {
    let [s, p] = good();
    let (root, env) = case("atompaw-unstable", &sha(built().as_bytes()), "", [&s, &p]);
    edit_script(
        &root,
        "</fake_xml>\nEOF",
        &format!("<n>$({} +%N)</n>\n</fake_xml>\nEOF", date_program()),
    );
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            "two runs of the pinned ATOMPAW wrote different bytes"
        ),
        "{findings:?}"
    );
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
}

#[test]
fn two_runs_that_write_other_log_derivatives_are_a_finding() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-unstable-waves",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    write(&root, "waves-2/logderiv.1", &logderiv(|_, ae| ae - 0.021));
    let mark = root.join("ran");
    let copy = format!("cp {}/waves/logderiv.* .\n", root.display());
    let both = format!(
        "{copy}test -f {0} && cp {1}/waves-2/logderiv.1 .\ntouch {0}\n",
        mark.display(),
        root.display()
    );
    edit_script(&root, &copy, &both);
    let findings = atompaw(&root, &env);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        has(
            &findings,
            "two runs of the pinned ATOMPAW wrote different logderiv.1"
        ),
        "{findings:?}"
    );
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
}

#[test]
fn a_committed_file_off_its_pin_is_a_finding() {
    let [s, p] = good();
    let (root, env) = case("atompaw-off-pin", &"0".repeat(64), "", [&s, &p]);
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            &format!("{COMMITTED}: sha256 {}", sha(built().as_bytes()))
        ),
        "{findings:?}"
    );
    assert!(
        has(&findings, &format!("pins {}", "0".repeat(64))),
        "{findings:?}"
    );
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
}

#[test]
fn an_unpinned_file_names_the_write_flag() {
    let [s, p] = good();
    let (root, env) = case("atompaw-unpinned", "", "", [&s, &p]);
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            &format!("{COMMITTED}: datasets/{FAMILY}.toml pins no such file")
        ),
        "{findings:?}"
    );
    assert!(has(&findings, "write it with --write"), "{findings:?}");
}

#[test]
fn a_phase_gap_above_the_bound_is_a_finding() {
    let s = logderiv(|e, ae| {
        if (e - 2.0).abs() < 1e-9 {
            ae + 0.11
        } else {
            ae
        }
    });
    let (root, env) = case(
        "atompaw-gap",
        &sha(built().as_bytes()),
        "",
        [&good()[0], &s],
    );
    let findings = atompaw(&root, &env);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(
        has(
            &findings,
            "l = 1: the PAW log derivative departs 0.1100 rad"
        ),
        "{findings:?}"
    );
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
    let record = fs::read_to_string(root.join(RECORD)).unwrap();
    assert!(record.contains("departs"), "{record}");
}

#[test]
fn a_gap_at_the_bound_passes() {
    let s: String = (grid(-5.0, 0.05, 200))
        .map(|e| format!("{e:12.4e}  0.0  0.0  0.0  0.1\n"))
        .collect();
    let findings = with_wave("atompaw-at-bound", &s);
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_phase_shift_of_pi_is_the_same_log_derivative() {
    let findings = with_wave("atompaw-pi", &logderiv(|_, ae| ae + PI));
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn an_extra_pole_of_the_paw_log_derivative_is_a_ghost_state() {
    let s = logderiv(|e, ae| {
        let step = ((e - 1.0) / 0.1).clamp(0.0, 1.0);
        ae - PI * step
    });
    let findings = with_wave("atompaw-ghost", &s);
    assert!(
        has(
            &findings,
            "l = 0: the PAW log derivative has 2 poles, the all-electron one 1: a ghost state"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_ghost_between_samples_still_shifts_the_phase() {
    let s = logderiv(|e, ae| if e > 1.02 { ae - PI } else { ae });
    let findings = with_wave("atompaw-narrow", &s);
    assert!(has(&findings, "has 2 poles"), "{findings:?}");
}

#[test]
fn a_window_that_starts_above_the_floor_is_a_finding() {
    let findings = with_wave(
        "atompaw-narrow-window",
        &rows(grid(-1.0, 0.05, 120), |_, ae| ae),
    );
    assert!(
        has(
            &findings,
            "l = 0: the window [-1.0, 4.95] Ry must reach from -5 to 4.95 Ry"
        ),
        "{findings:?}"
    );
}

#[test]
fn a_window_that_ends_below_the_top_is_a_finding() {
    let findings = with_wave(
        "atompaw-short-window",
        &rows(grid(-5.0, 0.05, 180), |_, ae| ae),
    );
    assert!(
        has(&findings, "l = 0: the window [-5.0, 3.95] Ry must reach"),
        "{findings:?}"
    );
}

#[test]
fn a_window_wider_than_the_rule_passes() {
    let findings = with_wave("atompaw-wide", &rows(grid(-8.0, 0.05, 281), |_, ae| ae));
    assert!(findings.is_empty(), "{findings:?}");
}

#[test]
fn a_step_coarser_than_the_rule_is_a_finding() {
    let s = rows(
        (0..100).map(|i| -5.0 + 9.95 * f64::from(i) / 99.0),
        |_, ae| ae,
    );
    let findings = with_wave("atompaw-coarse", &s);
    assert!(has(&findings, "l = 0: a step of 0.1"), "{findings:?}");
    assert!(has(&findings, "above 0.05"), "{findings:?}");
}

#[test]
fn one_row_is_no_step_and_fails() {
    let findings = with_wave("atompaw-one-row", "-5.0 1.0 1.0 0.0 0.0\n");
    assert!(has(&findings, "a step of inf Ry"), "{findings:?}");
}

#[test]
fn a_semicore_state_lowers_the_floor() {
    let [s, _] = good();
    let semicore = "<state n=\"2\" l=\"0\" e=\"-3.0\"/>";
    for (name, wave, finding) in [
        ("atompaw-semicore", s.clone(), true),
        (
            "atompaw-semicore-wide",
            rows(grid(-7.0, 0.05, 240), |_, ae| ae),
            false,
        ),
    ] {
        let text = built().replace(VALENCE, semicore);
        let (root, env) = case(name, &sha(text.as_bytes()), "", [&wave, &wave]);
        write(&root, COMMITTED, &text);
        edit_script(&root, VALENCE, semicore);
        let findings = atompaw(&root, &env);
        let narrow = has(&findings, "must reach from -7 to 4.95 Ry");
        assert_eq!(narrow, finding, "{name}: {findings:?}");
        assert_eq!(findings.is_empty(), !finding, "{name}: {findings:?}");
    }
}

#[test]
fn each_l_up_to_lmax_plus_one_needs_its_logderiv() {
    let [s, p] = good();
    let (root, env) = case("atompaw-l-gap", &sha(built().as_bytes()), "", [&s, &p]);
    fs::remove_file(root.join("waves/logderiv.1")).unwrap();
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            "ATOMPAW wrote no logderiv.1 (the XML's lmax is 0, so l = 0 to 1 each need one)"
        ),
        "{findings:?}"
    );
    let p_state = "<state n=\"2\" l=\"1\" e=\"-0.33\"/>";
    let (root, env) = case("atompaw-l-max", "", "", [&s, &p]);
    edit_script(&root, VALENCE, &format!("{VALENCE}\n{p_state}"));
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            "ATOMPAW wrote no logderiv.2 (the XML's lmax is 1, so l = 0 to 2 each need one)"
        ),
        "{findings:?}"
    );
}

#[test]
fn an_xml_without_valence_states_stops_the_element() {
    let [s, p] = good();
    let (root, env) = case("atompaw-no-valence", "", "", [&s, &p]);
    edit_script(&root, VALENCE, "");
    assert!(has(
        &atompaw(&root, &env),
        "the built XML holds no <valence_states> state"
    ));
    edit_script(
        &root,
        "<valence_states>",
        "<valence_states>\n<state l=\"x\" e=\"1\"/>",
    );
    assert!(has(
        &atompaw(&root, &env),
        "a valence state without a numeric l and e"
    ));
    edit_script(&root, "<fake_xml>", "<fake_xml");
    assert!(has(&atompaw(&root, &env), "the built XML: "));
}

#[test]
fn a_logderiv_row_without_five_numbers_stops_the_element() {
    let findings = with_wave("atompaw-rows", "-5.0 1.0 1.0 0.0\n");
    assert!(
        has(&findings, "logderiv.0: line 1: want 5 finite numbers"),
        "{findings:?}"
    );
    let findings = with_wave("atompaw-word", "-5.0 1.0 1.0 0.0 x\n");
    assert!(has(&findings, "logderiv.0: line 1: "), "{findings:?}");
    let findings = with_wave("atompaw-empty", "\n");
    assert!(has(&findings, "logderiv.0: no rows"), "{findings:?}");
}

#[test]
fn a_logderiv_phase_that_is_no_number_stops_the_element() {
    let [s, _] = good();
    let (root, env) = case(
        "atompaw-nan",
        &sha(built().as_bytes()),
        "",
        [&s, "-5.0 1.0 1.0 0.0 NaN\n"],
    );
    let findings = atompaw(&root, &env);
    assert!(
        has(&findings, "logderiv.1: line 1: want 5 finite numbers"),
        "{findings:?}"
    );
}

#[test]
fn a_failed_run_stops_the_element() {
    let [s, p] = good();
    let (root, env) = case("atompaw-exit", &sha(built().as_bytes()), "exit 3", [&s, &p]);
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            "atompaw: orbitaR-PBE-v0/O: atompaw exit status: 3"
        ),
        "{findings:?}"
    );
}

#[test]
fn an_xml_without_one_date_line_stops_the_element() {
    let [s, p] = good();
    let (root, env) = case("atompaw-date", &sha(built().as_bytes()), "", [&s, &p]);
    edit_script(&root, "  PAW functions generated on", "  generated on");
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            "O.GGA_X_PBE+GGA_C_PBE-paw.xml: 0 lines start with"
        ),
        "{findings:?}"
    );
}

#[test]
fn only_a_stamp_on_the_date_line_is_set_to_the_pin() {
    for stamp in [
        "11/21/2026, 02:23:53.367 x",
        "11/21/2026 02:23:53.367",
        "1a/21/2026, 02:23:53.367",
        "11-21-2026, 02:23:53.367",
    ] {
        let [s, p] = good();
        let (root, env) = case("atompaw-stamp", &sha(built().as_bytes()), "", [&s, &p]);
        edit_script(&root, &date_line(), stamp);
        let findings = atompaw(&root, &env);
        assert!(
            has(
                &findings,
                &format!("the date line holds {stamp:?}, not a stamp")
            ),
            "{stamp}: {findings:?}"
        );
        assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
    }
    let [s, p] = good();
    let (root, env) = case("atompaw-stamp-ns", &sha(built().as_bytes()), "", [&s, &p]);
    edit_script(&root, "%S.%3N", "%S.%N");
    assert!(has(&atompaw(&root, &env), "not a stamp"));
}

#[test]
fn a_build_keeps_every_line_but_the_two_contact_lines() {
    let [s, p] = good();
    let (root, env) = case("atompaw-contact-write", "", "", [&s, &p]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    let raw = xml(&contact(), DATE, INPUT.trim_end());
    assert_eq!(
        fs::read_to_string(root.join(COMMITTED)).unwrap(),
        raw.replace(&contact(), "")
    );
}

#[test]
fn a_raw_rebuild_matches_the_committed_file_without_its_contact_lines() {
    let [s, p] = good();
    let (root, env) = case("atompaw-contact", &sha(built().as_bytes()), "", [&s, &p]);
    assert!(atompaw(&root, &env).is_empty());
    let raw = xml(&contact(), DATE, INPUT.trim_end());
    let (root, env) = case("atompaw-contact-kept", &sha(raw.as_bytes()), "", [&s, &p]);
    write(&root, COMMITTED, &raw);
    let findings = atompaw(&root, &env);
    assert!(has(&findings, "the rebuild differs from"), "{findings:?}");
}

#[test]
fn a_build_without_one_pair_of_contact_lines_stops_the_element() {
    let cases = [
        (
            CONTACT,
            "  Contact: ",
            "0 lines start with \"  Contact info: \", not 1",
        ),
        (
            "  email: ",
            "  mail: ",
            "the line after \"  Contact info: \" does not start with \"  email: \"",
        ),
        (
            "  PAW functions",
            "  Contact info: B. Author\n  email: x\n  PAW functions",
            "2 lines start with \"  Contact info: \", not 1",
        ),
    ];
    for (from, to, finding) in cases {
        let [s, p] = good();
        let (root, env) = case("atompaw-no-contact", &sha(built().as_bytes()), "", [&s, &p]);
        edit_script(&root, from, to);
        let findings = atompaw(&root, &env);
        assert!(has(&findings, finding), "{to}: {findings:?}");
    }
}

#[test]
fn an_address_left_in_a_build_is_a_finding_and_is_never_written() {
    let [s, p] = good();
    let (root, env) = case("atompaw-address", "", "", [&s, &p]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    let line = format!("  Atompaw 4.2.0.4\n  web: {}\n", address());
    edit_script(&root, "  PAW functions", &format!("{line}  PAW functions"));
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    let named = format!("`{}` names a person", address());
    assert!(has(&findings, &named), "{findings:?}");
    assert!(!root.join(COMMITTED).exists());
}

#[test]
fn a_pinned_date_that_is_no_stamp_stops_the_element() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-pinned-date",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    let pins = fs::read_to_string(root.join("benchmarks/DATASETS.toml")).unwrap();
    write(
        &root,
        "benchmarks/DATASETS.toml",
        &pins.replace(
            &format!("pinned_date = \"{DATE}\""),
            "pinned_date = \"PENDING\"",
        ),
    );
    assert!(has(
        &atompaw(&root, &env),
        "pinned_date holds \"PENDING\", not a stamp"
    ));
}

#[test]
fn a_binary_off_its_pin_is_a_finding() {
    let [s, p] = good();
    let (root, env) = case("atompaw-wrapper", &sha(built().as_bytes()), "", [&s, &p]);
    let pinned = sha(&fs::read(root.join("prefix/bin/atompaw")).unwrap());
    write(&root, "prefix/bin/atompaw", "#!/bin/sh\nexec true\n");
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            &format!(
                "bin/atompaw has sha256 {}, but benchmarks/DATASETS.toml pins {pinned}",
                sha(b"#!/bin/sh\nexec true\n")
            )
        ),
        "{findings:?}"
    );
    assert!(!root.join("target/atompaw/orbitaR-PBE-v0/O").exists());
}

#[test]
fn a_library_off_its_pin_is_a_finding() {
    let [s, p] = good();
    let (root, env) = case("atompaw-library", &sha(built().as_bytes()), "", [&s, &p]);
    write(&root, "prefix/lib/libx.so.1.0", "library!");
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            &format!("{LIBRARY} has sha256 {}", sha(b"library!"))
        ),
        "{findings:?}"
    );
    fs::remove_file(root.join("prefix/lib/libx.so.1.0")).unwrap();
    assert!(has(&atompaw(&root, &env), &format!("{LIBRARY}: ")));
}

#[test]
fn the_pins_must_name_bin_atompaw() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-no-binary-pin",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    let pins = fs::read_to_string(root.join("benchmarks/DATASETS.toml")).unwrap();
    let line = pins
        .lines()
        .find(|l| l.starts_with("\"bin/atompaw\""))
        .unwrap();
    write(
        &root,
        "benchmarks/DATASETS.toml",
        &pins.replace(&format!("{line}\n"), ""),
    );
    assert!(has(
        &atompaw(&root, &env),
        "[atompaw.binaries] pins no bin/atompaw"
    ));
}

#[test]
fn an_install_off_the_package_pins_is_a_finding() {
    let [s, p] = good();
    let (root, env) = case("atompaw-packages", &sha(built().as_bytes()), "", [&s, &p]);
    let prefix = root.join("prefix");
    meta(&prefix, "libxc", LIBXC, &"c".repeat(64));
    let other = "https://conda.anaconda.org/conda-forge/linux-64/libxc-5.2.3-h0000000_0.conda";
    meta(&prefix, "extra", other, &"d".repeat(64));
    meta(&prefix, "atompaw-again", URL, PACKAGE);
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            &format!("{LIBXC} has sha256 \"{}\"", "c".repeat(64))
        ),
        "{findings:?}"
    );
    assert!(
        has(
            &findings,
            &format!("holds {other}, which benchmarks/DATASETS.toml does not pin")
        ),
        "{findings:?}"
    );
    assert!(
        has(&findings, &format!("two conda-meta records name {URL}")),
        "{findings:?}"
    );
    fs::remove_file(prefix.join("conda-meta/libxc.json")).unwrap();
    let findings = atompaw(&root, &env);
    assert!(has(&findings, &format!("lacks {LIBXC}")), "{findings:?}");
}

#[test]
fn a_build_needs_the_install() {
    let [s, p] = good();
    let (root, _) = case("atompaw-no-prefix", &sha(built().as_bytes()), "", [&s, &p]);
    let findings = atompaw(&root, &env(false));
    assert!(has(&findings, "set ORBITAR_ATOMPAW"), "{findings:?}");
}

#[test]
fn a_relative_prefix_is_refused() {
    let [s, p] = good();
    let (root, env) = case("atompaw-relative", &sha(built().as_bytes()), "", [&s, &p]);
    let env = Env {
        atompaw: Some("prefix".into()),
        ..env
    };
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            "the install's prefix prefix must be an absolute path"
        ),
        "{findings:?}"
    );
    let findings = xtask::run(&["atompaw", "--install", "new"], &root, &env);
    assert!(
        has(
            &findings,
            "the install's prefix new must be an absolute path"
        ),
        "{findings:?}"
    );
}

fn micromamba(name: &str, code: u8) -> (Tree, Env) {
    let [s, p] = good();
    let (root, env) = case(name, &sha(built().as_bytes()), "", [&s, &p]);
    let body = format!(
        "#!/bin/sh\nPATH=/usr/bin:/bin\n\
         test \"$1 $2 $3 $4 $6\" = 'create --yes --quiet --prefix --file' || exit 5\n\
         head -1 \"$7\" | grep -qx '@EXPLICIT' || exit 6\n\
         grep -qx '{URL}#{PACKAGE}' \"$7\" || exit 6\n\
         grep -qx '{LIBXC}#{}' \"$7\" || exit 6\n\
         mkdir -p \"$5\" && cp -R {}/prefix/. \"$5\"\nexit {code}\n",
        "a".repeat(64),
        root.display()
    );
    write(&root, "mm/micromamba", &body);
    fs::set_permissions(
        root.join("mm/micromamba"),
        fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let env = Env {
        atompaw: None,
        path: Some(root.join("mm").into_os_string()),
        ..env
    };
    (root, env)
}

#[test]
fn an_install_runs_micromamba_on_the_given_path_and_checks_the_result() {
    let (root, env) = micromamba("atompaw-install", 0);
    let dir = root.join("new-prefix");
    let findings = xtask::run(
        &["atompaw", "--install", dir.to_str().unwrap()],
        &root,
        &env,
    );
    assert!(findings.is_empty(), "{findings:?}");
    assert!(dir.join("conda-meta/atompaw.json").is_file());
    assert!(!root.join("new-prefix.explicit.txt").exists());
}

#[test]
fn a_failed_install_is_a_finding() {
    let (root, env) = micromamba("atompaw-install-fails", 1);
    let dir = root.join("new-prefix");
    let findings = xtask::run(
        &["atompaw", "--install", dir.to_str().unwrap()],
        &root,
        &env,
    );
    assert_eq!(
        findings,
        [format!(
            "atompaw: micromamba create {}: exit status: 1",
            dir.display()
        )]
    );
    assert!(!root.join("new-prefix.explicit.txt").exists());
}

#[test]
fn an_install_without_micromamba_names_it() {
    let (root, env) = micromamba("atompaw-install-missing", 0);
    let env = Env {
        path: Some(root.join("nowhere").into_os_string()),
        ..env
    };
    let dir = root.join("new-prefix");
    let findings = xtask::run(
        &["atompaw", "--install", dir.to_str().unwrap()],
        &root,
        &env,
    );
    assert!(has(&findings, "atompaw: micromamba: "), "{findings:?}");
}

#[test]
fn the_pins_hold_the_atompaw_package_once_at_its_sha256() {
    let [s, p] = good();
    let (root, env) = case("atompaw-pin", &sha(built().as_bytes()), "", [&s, &p]);
    let pins = fs::read_to_string(root.join("benchmarks/DATASETS.toml")).unwrap();
    write(
        &root,
        "benchmarks/DATASETS.toml",
        &pins.replacen(
            &format!("url = \"{URL}\", sha256 = \"{PACKAGE}\""),
            &format!("url = \"{URL}\", sha256 = \"{}\"", "e".repeat(64)),
            1,
        ),
    );
    assert!(has(
        &atompaw(&root, &env),
        "has another sha256 than [atompaw] sha256"
    ));
    write(&root, "benchmarks/DATASETS.toml", &pins.replace(URL, LIBXC));
    assert!(has(
        &atompaw(&root, &env),
        "packages must hold /atompaw-4.2.0.4-h8876d29_0.conda once"
    ));
    write(&root, "benchmarks/DATASETS.toml", "");
    assert!(has(&atompaw(&root, &env), "has no [atompaw] table"));
}

#[test]
fn the_repository_pins_atompaw_4_2_0_4_and_an_input_for_each_file_of_ours() {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let pins: toml::Table = fs::read_to_string(repo.join("benchmarks/DATASETS.toml"))
        .unwrap()
        .parse()
        .unwrap();
    let atompaw = pins["atompaw"].as_table().unwrap();
    assert_eq!(atompaw["version"].as_str(), Some("4.2.0.4"));
    assert_eq!(atompaw["build"].as_str(), Some("h8876d29_0"));
    assert_eq!(atompaw["sha256"].as_str(), Some(PACKAGE));
    assert!(
        atompaw["binaries"]
            .as_table()
            .unwrap()
            .contains_key("bin/atompaw")
    );
    let mut own = 0;
    for entry in fs::read_dir(repo.join("datasets")).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_none_or(|e| e != "toml") {
            continue;
        }
        let table: toml::Table = fs::read_to_string(&path).unwrap().parse().unwrap();
        if !table.contains_key("base") {
            continue;
        }
        let family = path.file_stem().unwrap().to_str().unwrap();
        for (element, row) in table["element"].as_table().unwrap() {
            own += 1;
            assert_eq!(
                row["file"].as_str(),
                Some(format!("{family}/{element}.xml").as_str())
            );
            let dir = repo.join("datasets").join(family);
            assert!(
                dir.join(format!("{element}.in")).is_file(),
                "{family}/{element}"
            );
            for file in ["LICENSE", "NOTICE"] {
                assert!(dir.join(file).is_file(), "{family}/{file}");
            }
        }
    }
    assert!(own > 0);
}

#[test]
fn a_step_just_above_the_slack_is_a_finding() {
    let s: String = (0..200)
        .map(|i| -5.0 + 0.050_000_01 * f64::from(i))
        .map(|e| format!("{e} 0 0 0 0\n"))
        .collect();
    let findings = with_wave("atompaw-slack", &s);
    assert_eq!(findings.len(), 1, "{findings:?}");
    assert!(has(&findings, "l = 0: a step of 0.0500000"), "{findings:?}");
}

#[test]
fn a_step_exactly_at_the_slack_passes() {
    let edge = 0.05 + 1e-9;
    let s: String = (-100..100)
        .map(|i| match i {
            ..=0 => 0.05 * f64::from(i),
            _ => edge + 0.05 * f64::from(i - 1),
        })
        .map(|e| format!("{e} 0 0 0 0\n"))
        .collect();
    let findings = with_wave("atompaw-edge", &s);
    assert!(findings.is_empty(), "{findings:?}");
}

fn committed_as(name: &str, from: &str, to: &str) -> Vec<String> {
    let text = built();
    assert!(text.contains(from), "{from}");
    let text = text.replace(from, to);
    let [s, p] = good();
    let (root, env) = case(name, &sha(text.as_bytes()), "", [&s, &p]);
    write(&root, COMMITTED, &text);
    atompaw(&root, &env)
}

#[test]
fn a_number_within_the_rebuild_bound_passes_and_one_above_it_is_a_finding() {
    for (to, pass) in [
        ("0.0E+00 -2.5E-01<", false),
        ("1.0E-06 -2.5E-01<", true),
        ("1.1E-06 -2.5E-01<", false),
    ] {
        let findings = committed_as("atompaw-rebuild-array", "0.0E+00 -2.5E-01<", to);
        match pass {
            true => assert!(findings.is_empty(), "{to}: {findings:?}"),
            false if to.starts_with("0.0") => assert!(findings.is_empty(), "{findings:?}"),
            false => {
                assert_eq!(findings.len(), 1, "{findings:?}");
                assert!(
                    has(
                        &findings,
                        &format!(
                            "a number of the rebuild departs 1.10e-6 from {COMMITTED}, above 1e-6"
                        )
                    ),
                    "{findings:?}"
                );
            }
        }
    }
}

#[test]
fn an_attribute_is_measured_against_its_own_value() {
    assert!(committed_as("atompaw-rebuild-rc", "rc=\"1.41\"", "rc=\"1.4100014\"").is_empty());
    let findings = committed_as("atompaw-rebuild-rc-far", "rc=\"1.41\"", "rc=\"1.4100141\"");
    assert!(
        has(&findings, "a number of the rebuild departs 1.00e-5"),
        "{findings:?}"
    );
}

#[test]
fn a_committed_zero_needs_a_rebuilt_zero() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-rebuild-zero",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    edit_script(&root, "shift=\"0.0\"", "shift=\"1.0E-30\"");
    let findings = atompaw(&root, &env);
    assert!(
        has(
            &findings,
            &format!(
                "the rebuild differs from {COMMITTED}: <paw_radius>: shift=\"0.0\", the rebuild's \"1.0E-30\""
            )
        ),
        "{findings:?}"
    );
}

#[test]
fn a_rebuild_with_another_count_attribute_word_or_element_is_a_finding() {
    for (from, to, why) in [
        (
            "0.0E+00 -2.5E-01<",
            "0.0E+00<",
            "<projector_function state=\"O-2s\"> text: 2 numbers in 2 words, the rebuild's 3 in 3",
        ),
        (
            "state=\"O-2s\"",
            "state=\"O-2p\"",
            "<projector_function state=\"O-2p\">: state=\"O-2p\", the rebuild's \"O-2s\"",
        ),
        (
            "rc=\"1.41\" shift",
            "shift",
            "<paw_radius>: attributes [\"shift\"], the rebuild's [\"rc\", \"shift\"]",
        ),
        (
            "O 8\nXMLOUT",
            "O 8\nXMLIN",
            "<input> text: \"XMLIN\" in the text, the rebuild's \"XMLOUT\"",
        ),
        (
            "XMLOUT\nEND<",
            "XMLOUT<",
            "<input> text: 1 numbers in 3 words, the rebuild's 1 in 4",
        ),
        (
            "1.0E+00 0.0E+00",
            "1.0E+00 zero",
            "<projector_function state=\"O-2s\"> text: 2 numbers in 3 words, the rebuild's 3 in 3",
        ),
        (
            "Atompaw 4.2.0.4",
            "Atompaw 4.2.0.5",
            "<fake_xml> comment: \"4.2.0.5\" in the text, the rebuild's \"4.2.0.4\"",
        ),
        (
            &format!("{RADIUS}\n"),
            "",
            "<projector_function state=\"O-2s\"> in the committed file, <paw_radius> in the rebuild",
        ),
        (
            "</fake_xml>",
            "<more/></fake_xml>",
            "the committed file has 19 nodes, the rebuild 18",
        ),
        ("<fake_xml>", "<fake_xml", "the committed XML: "),
    ] {
        let findings = committed_as("atompaw-rebuild-shape", from, to);
        assert_eq!(findings.len(), 1, "{to}: {findings:?}");
        assert!(has(&findings, why), "{why}: {findings:?}");
    }
}

#[test]
fn a_committed_file_that_ends_early_is_a_finding() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-rebuild-longer",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    edit_script(&root, "</fake_xml>\nEOF", "</fake_xml>\n<!-- tail -->\nEOF");
    let findings = atompaw(&root, &env);
    assert!(
        has(&findings, "the committed file has 18 nodes, the rebuild 19"),
        "{findings:?}"
    );
}

#[test]
fn write_puts_run_one_beside_the_input_without_a_pin_or_a_committed_file() {
    let [s, p] = good();
    let (root, env) = case("atompaw-write", "", "", [&s, &p]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
    let record = fs::read_to_string(root.join(RECORD)).unwrap();
    assert!(record.contains("\"rebuild_largest_rel\": null"), "{record}");
}

#[test]
fn write_with_a_finding_and_a_check_without_write_leave_datasets_alone() {
    let [s, _] = good();
    let ghost = logderiv(|e, ae| if e > 2.0 { ae + PI } else { ae });
    let (root, env) = case("atompaw-write-ghost", "", "", [&s, &ghost]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(has(&findings, "a ghost state"), "{findings:?}");
    assert!(!root.join(COMMITTED).exists());
    let findings = atompaw(&root, &env);
    assert!(has(&findings, "pins no such file"), "{findings:?}");
    assert!(!root.join(COMMITTED).exists());
}

#[test]
fn each_family_builds_its_own_inputs_and_an_argument_picks_a_family_or_one_input() {
    let [s, p] = good();
    let (root, env) = case("atompaw-families", &sha(built().as_bytes()), "", [&s, &p]);
    write(&root, "datasets/ours-v1/O.in", INPUT);
    write(&root, "datasets/ours-v1/O.xml", &built());
    write(
        &root,
        "datasets/ours-v1.toml",
        &table(&sha(built().as_bytes())).replace(FILE, "ours-v1/O.xml"),
    );
    let findings = atompaw(&root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    for (family, record) in [
        (FAMILY, RECORD),
        ("ours-v1", "target/atompaw/ours-v1/O.json"),
    ] {
        let record: serde_json::Value =
            serde_json::from_str(&fs::read_to_string(root.join(record)).unwrap()).unwrap();
        assert_eq!(record["file"], format!("{family}/O.xml"));
        assert_eq!(record["input"], format!("datasets/{family}/O.in"));
    }
    fs::remove_dir_all(root.join("target/atompaw")).unwrap();
    for word in ["ours-v1", "ours-v1/O"] {
        let findings = xtask::run(&["atompaw", word], &root, &env);
        assert!(findings.is_empty(), "{word}: {findings:?}");
        assert!(
            root.join("target/atompaw/ours-v1/O.json").is_file(),
            "{word}"
        );
        assert!(!root.join(RECORD).exists(), "{word}");
    }
    for word in ["O", "ours-v2", "ours-v1/Mg"] {
        let findings = xtask::run(&["atompaw", word], &root, &env);
        assert!(
            has(
                &findings,
                &format!("atompaw: {word}: no datasets/<family>/<element>.in")
            ),
            "{word}: {findings:?}"
        );
    }
}

#[test]
fn a_second_table_never_supplies_the_pin_of_another_familys_file() {
    let [s, p] = good();
    let (root, env) = case("atompaw-alias", &"0".repeat(64), "", [&s, &p]);
    for alias in ["aa-alias", "zz-alias"] {
        write(
            &root,
            &format!("datasets/{alias}.toml"),
            &table(&sha(built().as_bytes())),
        );
    }
    let findings = xtask::run(&["atompaw", "orbitaR-PBE-v0/O"], &root, &env);
    assert!(
        has(
            &findings,
            &format!(
                "{COMMITTED}: sha256 {}, but datasets/{FAMILY}.toml pins {}",
                sha(built().as_bytes()),
                "0".repeat(64)
            )
        ),
        "{findings:?}"
    );
}

#[test]
fn a_family_directory_or_an_input_that_is_a_link_is_no_input() {
    let [s, p] = good();
    let (root, env) = case("atompaw-linked-dir", "", "", [&s, &p]);
    let outside = tree("atompaw-linked-dir-outside", &[("O.in", INPUT)]);
    symlink(&*outside, root.join("datasets/linked-v1")).unwrap();
    symlink(
        root.join(format!("datasets/{FAMILY}/O.in")),
        root.join(format!("datasets/{FAMILY}/N.in")),
    )
    .unwrap();
    for word in ["linked-v1/O", "orbitaR-PBE-v0/N"] {
        let findings = xtask::run(&["atompaw", word, "--write"], &root, &env);
        assert!(
            has(
                &findings,
                &format!("atompaw: {word}: no datasets/<family>/<element>.in")
            ),
            "{word}: {findings:?}"
        );
    }
    assert!(!outside.join("O.xml").exists());
    assert!(!root.join(format!("datasets/{FAMILY}/N.xml")).exists());
}

#[test]
fn write_never_follows_a_link_at_the_output() {
    let [s, p] = good();
    let (root, env) = case("atompaw-linked-out", "", "", [&s, &p]);
    let outside = tree("atompaw-linked-out-outside", &[]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    symlink(outside.join("O.xml"), root.join(COMMITTED)).unwrap();
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(
        has(
            &findings,
            &format!("{COMMITTED} is a link; our files are regular files")
        ),
        "{findings:?}"
    );
    assert!(!outside.join("O.xml").exists());
}

#[test]
fn write_over_a_pinned_file_writes_only_the_bytes_at_its_pin() {
    let [s, p] = good();
    let (root, env) = case("atompaw-write-pinned", &"0".repeat(64), "", [&s, &p]);
    write(&root, COMMITTED, "old");
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(
        has(
            &findings,
            &format!(
                "{COMMITTED}: the build has sha256 {}, but datasets/{FAMILY}.toml pins {}",
                sha(built().as_bytes()),
                "0".repeat(64)
            )
        ),
        "{findings:?}"
    );
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), "old");
    let (root, env) = case(
        "atompaw-write-repair",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    write(&root, COMMITTED, "old");
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(findings.is_empty(), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), built());
}

#[test]
fn a_datasets_directory_that_is_a_link_holds_no_input() {
    let [s, p] = good();
    let (root, env) = case("atompaw-linked-datasets", "", "", [&s, &p]);
    let outside = tree(
        "atompaw-linked-datasets-outside",
        &[("orbitaR-PBE-v0/O.in", INPUT)],
    );
    fs::remove_dir_all(root.join("datasets")).unwrap();
    symlink(&*outside, root.join("datasets")).unwrap();
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(
        has(
            &findings,
            "atompaw: datasets holds no <family>/<element>.in"
        ),
        "{findings:?}"
    );
    assert!(!outside.join("orbitaR-PBE-v0/O.xml").exists());
}

#[test]
fn write_into_a_family_whose_table_pins_no_such_file_or_nothing_is_refused() {
    let [s, p] = good();
    let (root, env) = case("atompaw-write-no-row", "", "", [&s, &p]);
    write(&root, COMMITTED, "old");
    let n_only = table(&"0".repeat(64))
        .replace("O = {", "N = {")
        .replace(FILE, "orbitaR-PBE-v0/N.xml");
    write(&root, &format!("datasets/{FAMILY}.toml"), &n_only);
    write(&root, &format!("datasets/{FAMILY}/N.xml"), "n");
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(
        has(
            &findings,
            &format!(
                "{COMMITTED}: datasets/{FAMILY}.toml pins no such file; a family never changes: build a new one"
            )
        ),
        "{findings:?}"
    );
    write(
        &root,
        &format!("datasets/{FAMILY}.toml"),
        &table(&sha(built().as_bytes())).replace("jth-2.0", "nope"),
    );
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(has(&findings, "such a table pins nothing"), "{findings:?}");
    assert!(!has(&findings, "build a new one"), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), "old");
}

#[test]
fn the_date_program_is_probed_on_the_fakes_own_path() {
    let gnubin = tree("atompaw-gnubin", &[("date", "#!/bin/sh\necho 123\n")]);
    fs::set_permissions(gnubin.join("date"), fs::Permissions::from_mode(0o755)).unwrap();
    let fixed = millis("date", OsStr::new("/usr/bin:/bin"));
    let got = date_for(gnubin.as_os_str());
    match fixed {
        true => assert_eq!(got, "date"),
        false => assert_eq!(got, format!("'{}'", gnubin.join("date").display())),
    }
}

#[test]
fn a_base_that_is_a_link_or_a_cycle_of_ours_authorizes_no_check_and_crashes_nothing() {
    let [s, p] = good();
    let (root, env) = case(
        "atompaw-linked-base",
        &sha(built().as_bytes()),
        "",
        [&s, &p],
    );
    let base = fs::read_to_string(root.join("datasets/jth-2.0.toml")).unwrap();
    write(&root, "notes/jth.toml", &base);
    fs::remove_file(root.join("datasets/jth-2.0.toml")).unwrap();
    symlink("../notes/jth.toml", root.join("datasets/jth-2.0.toml")).unwrap();
    let findings = xtask::run(&["atompaw", "orbitaR-PBE-v0/O"], &root, &env);
    assert!(
        has(
            &findings,
            &format!(
                "{COMMITTED}: datasets/{FAMILY}.toml: base \"jth-2.0\" is no external table of datasets/"
            )
        ),
        "{findings:?}"
    );
    let (root, env) = case("atompaw-self-base", &sha(built().as_bytes()), "", [&s, &p]);
    let table = table(&sha(built().as_bytes())).replace("\"jth-2.0\"", &format!("\"{FAMILY}\""));
    write(&root, &format!("datasets/{FAMILY}.toml"), &table);
    let findings = xtask::run(&["atompaw", "orbitaR-PBE-v0/O"], &root, &env);
    assert!(
        has(
            &findings,
            &format!("base \"{FAMILY}\" is no external table of datasets/")
        ),
        "{findings:?}"
    );
}

#[test]
fn a_failed_write_removes_nothing_that_it_did_not_create() {
    let [s, p] = good();
    let (root, env) = case("atompaw-sentinel", "", "", [&s, &p]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    let part = format!("datasets/{FAMILY}/.O.xml.{}.part", std::process::id());
    write(&root, &part, "sentinel");
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(!findings.is_empty(), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(&part)).unwrap(), "sentinel");
    assert!(!root.join(COMMITTED).exists());
}

#[test]
fn a_record_that_cannot_be_written_stops_the_write_before_the_dataset_is_published() {
    let [s, p] = good();
    let (root, env) = case("atompaw-record-dir", "", "", [&s, &p]);
    fs::remove_file(root.join(COMMITTED)).unwrap();
    fs::create_dir_all(root.join(RECORD)).unwrap();
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(has(&findings, RECORD), "{findings:?}");
    assert!(!root.join(COMMITTED).exists());
}

#[test]
fn a_new_family_needs_a_snapshot_without_a_fault_and_no_table_in_any_spelling() {
    let [s, p] = good();
    let (root, env) = case("atompaw-bootstrap-toml-case", "", "", [&s, &p]);
    write(&root, COMMITTED, "old");
    write(
        &root,
        &format!("datasets/{FAMILY}.ToML"),
        &table(&"0".repeat(64)),
    );
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(!findings.is_empty(), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), "old");
    let (root, env) = case("atompaw-bootstrap-fault", "", "", [&s, &p]);
    write(&root, COMMITTED, "old");
    write(&root, "datasets/junk.toml", "= nonsense");
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(has(&findings, "datasets/junk.toml: "), "{findings:?}");
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), "old");
}

#[test]
fn a_new_family_takes_a_portable_name_that_is_no_directory_of_the_base() {
    let [s, p] = good();
    let (root, env) = case("atompaw-bootstrap-names", "", "", [&s, &p]);
    write(
        &root,
        "datasets/jth-2.0.toml",
        &format!(
            "url = \"u\"\n\n[pbe]\nlayout = \"JTH-PBE-v2.0/{{element}}/{{element}}.xml\"\n\n[pbe.element]\nO = {{ sha256 = \"{}\" }}\n",
            "a".repeat(64)
        ),
    );
    for family in ["bad name", "jth-pbe-v2.0"] {
        write(&root, &format!("datasets/{family}/O.in"), INPUT);
        let findings = xtask::run(&["atompaw", &format!("{family}/O"), "--write"], &root, &env);
        assert!(!findings.is_empty(), "{family}: {findings:?}");
        assert!(
            !root.join(format!("datasets/{family}/O.xml")).exists(),
            "{family}"
        );
    }
}

#[test]
fn a_family_directory_that_cannot_be_listed_is_a_finding() {
    if crate::root_user() {
        return;
    }
    let [s, p] = good();
    let (root, env) = case("atompaw-unlistable", &sha(built().as_bytes()), "", [&s, &p]);
    write(&root, "datasets/ours-v1/O.in", INPUT);
    let dir = root.join("datasets/ours-v1");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o300)).unwrap();
    let findings = atompaw(&root, &env);
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(
        has(&findings, "datasets/ours-v1 cannot be listed: "),
        "{findings:?}"
    );
}

#[test]
fn a_table_spelled_in_another_case_than_its_family_directory_is_named_as_it_is_spelled() {
    let [s, p] = good();
    let (root, env) = case("atompaw-other-case", "", "", [&s, &p]);
    write(&root, COMMITTED, "old");
    write(
        &root,
        "datasets/ORBITAR-PBE-V0.toml",
        &table(&"0".repeat(64)).replace(FILE, "ORBITAR-PBE-V0/O.xml"),
    );
    let findings = xtask::run(&["atompaw", "--write"], &root, &env);
    assert!(
        has(&findings, "datasets/ORBITAR-PBE-V0.toml pins no such file"),
        "{findings:?}"
    );
    assert_eq!(fs::read_to_string(root.join(COMMITTED)).unwrap(), "old");
}

#[test]
fn the_builder_names_a_pin_manifest_it_cannot_read_and_a_linked_datasets_root() {
    let [s, p] = good();
    let (root, env) = case("atompaw-manifest-fifo", "", "", [&s, &p]);
    crate::fifo(&root, "benchmarks/DATASETS.toml");
    let path = root.to_path_buf();
    let findings = crate::within(5, move || xtask::run(&["atompaw"], &path, &env));
    assert!(
        has(&findings, "benchmarks/DATASETS.toml is no regular file"),
        "{findings:?}"
    );
    let (root, env) = case("atompaw-linked-root", "", "", [&s, &p]);
    let outside = tree(
        "atompaw-linked-root-outside",
        &[("orbitaR-PBE-v0/O.in", INPUT)],
    );
    fs::remove_dir_all(root.join("datasets")).unwrap();
    symlink(&*outside, root.join("datasets")).unwrap();
    let findings = atompaw(&root, &env);
    assert!(
        has(&findings, "datasets is a link or no directory"),
        "{findings:?}"
    );
}
