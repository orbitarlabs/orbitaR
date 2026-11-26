use super::scf::{ONE, input, solve, synthetic, synthetic_meta};
use crate::{EMPTY_CUBE, ONE_SI, with_atoms};
use orbitar::io::{read_spec, to_json};
use orbitar::ks::{Failure, Session, run};
use orbitar::model::Spec;
use orbitar::paw::Dataset;
use std::cell::Cell as Counter;

#[test]
fn the_rust_api_reads_an_input_runs_it_and_prints_the_outcome_line() -> Result<(), Failure> {
    use orbitar::io::DatasetFiles;
    let spec = read_spec(EMPTY_CUBE).map_err(Failure::Invalid)?;
    let mut datasets = DatasetFiles::at(&spec, "datasets".as_ref());
    let outcome = run(&spec, |z| datasets.read(z), |_, _| {})?;
    let rows = outcome.eigenvalues_ha();
    assert_eq!(rows[0].len(), outcome.kpoints().points().len());
    assert!(outcome.scf().is_none());
    let json = datasets.json(&outcome);
    assert!(json.starts_with('{'));
    Ok(())
}

#[test]
fn the_bands_are_the_lowest_kinetic_energies_at_each_k_point() {
    let outcome = run(&read_spec(EMPTY_CUBE).unwrap(), no_dataset, |_, _| {}).unwrap();
    let step = std::f64::consts::TAU / 6.0;
    let want = [vec![0.0, 0.5 * step * step], vec![0.125 * step * step; 2]];
    let [channel] = outcome.eigenvalues_ha() else {
        panic!("one spin channel");
    };
    assert_eq!(channel.len(), want.len());
    for (got, want) in channel.iter().zip(want) {
        assert_eq!(got.len(), want.len());
        assert!(
            got.iter().zip(&want).all(|(g, w)| (g - w).abs() < 1e-15),
            "{got:?}"
        );
    }
    assert_eq!(outcome.basis_sizes(), [27, 28]);
}

#[test]
fn the_bands_may_reach_but_not_pass_the_plane_wave_count() {
    let with = |count: &str| {
        let input = EMPTY_CUBE.replace("count = 2", count);
        run(&read_spec(&input).unwrap(), no_dataset, |_, _| {})
    };
    assert!(with("count = 27").is_ok());
    let error = with("count = 28").unwrap_err();
    assert!(matches!(error, Failure::Invalid(m) if m.contains("bands.count")));
}

fn no_dataset(z: u8) -> Result<Dataset, Failure> {
    panic!("no dataset for Z = {z} was expected")
}

#[test]
fn an_r2scan_scf_reads_a_pbe_dataset_with_its_core_kinetic_energy_densities() {
    let r2scan = run(
        &read_spec(&input(&ONE, "\n[xc]\nfunctional = \"r2scan\"\n")).unwrap(),
        synthetic_meta,
        |_, _| {},
    )
    .unwrap();
    let pbe = solve(&input(&ONE, "")).unwrap();
    let free = |o: &orbitar::ks::Outcome| o.scf().unwrap().energies.free_ha();
    assert!(free(&r2scan).is_finite() && (free(&r2scan) - free(&pbe)).abs() > 1e-3);
}

#[test]
fn a_meta_gga_scf_refuses_a_dataset_without_core_kinetic_energy_densities() {
    let text = input(&ONE, "\n[xc]\nfunctional = \"r2scan\"\n");
    let result = run(&read_spec(&text).unwrap(), synthetic, |_, _| {});
    assert!(
        matches!(&result, Err(Failure::Invalid(m)) if m == "datasets: the dataset for Z = 6 has no core kinetic energy densities, which r2scan reads"),
        "{result:?}"
    );
}

#[test]
fn other_mixers_are_unsupported_before_any_dataset_is_read() {
    let atoms = with_atoms(EMPTY_CUBE, ONE_SI);
    for (extra, key) in [
        ("[scf]\nmixer = \"broyden\"", "scf.mixer"),
        ("[scf]\nmixer = \"linear\"", "scf.mixer"),
    ] {
        let result = run(
            &read_spec(&format!("{atoms}\n{extra}\n")).unwrap(),
            no_dataset,
            |_, _| {},
        );
        assert!(
            matches!(result, Err(Failure::Unsupported(m)) if m.starts_with(key)),
            "{key}"
        );
    }
}

#[test]
fn each_element_is_asked_for_once() {
    let asked = Counter::new(0);
    let spec = read_spec(&input(&[[0.1, 0.2, 0.3], [0.6, 0.55, 0.8]], "")).unwrap();
    run(
        &spec,
        |z| {
            asked.set(asked.get() + 1);
            synthetic(z)
        },
        |_, _| {},
    )
    .unwrap();
    assert_eq!(asked.get(), 1);
}

#[test]
fn a_dataset_of_another_element_or_functional_is_refused() {
    let nitrogen = input(&ONE, "").replace("\"C\"", "\"N\"");
    let result = run(&read_spec(&nitrogen).unwrap(), |_| synthetic(6), |_, _| {});
    assert!(
        matches!(result, Err(Failure::Invalid(m)) if m == "datasets: the dataset given for Z = 7 holds Z = 6")
    );
    let lda = solve(&input(&ONE, "\n[xc]\nfunctional = \"lda\"\n"));
    assert!(matches!(lda, Err(Failure::Invalid(m)) if m.starts_with("xc.functional: Lda, but")));
}

#[test]
fn an_initial_moment_may_reach_but_not_pass_the_valence() {
    let two = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.8]];
    let spin = |moments: &str| solve(&input(&two, &format!("\n[spin]\nmoments = {moments}\n")));
    assert!(
        matches!(spin("[0.0, -4.5]"), Err(Failure::Invalid(m)) if m == "spin.moments: atom 1 starts at -4.5 μB, beyond its 4 valence electrons")
    );
    assert!(spin("[4.0, 4.0]").is_ok());
}

const START: [[f64; 3]; 2] = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.5]];
const MOVED: [[f64; 3]; 2] = [[0.1, 0.2, 0.3], [0.61, 0.55, 0.5]];

#[test]
fn a_meta_gga_session_starts_a_moved_geometry_from_its_last_state_and_meets_the_same_stop_rule() {
    let r2scan = |positions: &[[f64; 3]]| {
        read_spec(&input(positions, "\n[xc]\nfunctional = \"r2scan\"\n")).unwrap()
    };
    let mut session = Session::default();
    session
        .run(&r2scan(&START), synthetic_meta, |_, _| {})
        .unwrap();
    let warm = session
        .run(&r2scan(&MOVED), synthetic_meta, |_, _| {})
        .unwrap();
    let cold = run(&r2scan(&MOVED), synthetic_meta, |_, _| {}).unwrap();
    let (warm, cold) = (warm.scf().unwrap(), cold.scf().unwrap());
    assert!(
        warm.steps < cold.steps,
        "warm {} cold {}",
        warm.steps,
        cold.steps
    );
    let gap = (warm.energies.free_ha() - cold.energies.free_ha()).abs();
    assert!(gap < 1.1e-11, "{gap:e}");
}

fn relaxing(positions: &[[f64; 3]]) -> Spec {
    read_spec(&input(positions, "\n[outputs]\nforces = true\n")).unwrap()
}

#[test]
fn a_session_starts_a_moved_geometry_from_the_last_state_and_meets_the_same_stop_rule() {
    let mut session = Session::default();
    session
        .run(&relaxing(&START), synthetic, |_, _| {})
        .unwrap();
    let warm = session
        .run(&relaxing(&MOVED), synthetic, |_, _| {})
        .unwrap();
    let cold = run(&relaxing(&MOVED), synthetic, |_, _| {}).unwrap();
    let (warm, cold) = (warm.scf().unwrap(), cold.scf().unwrap());
    assert!(
        warm.steps < cold.steps,
        "warm {} cold {}",
        warm.steps,
        cold.steps
    );
    let gap = (warm.energies.free_ha() - cold.energies.free_ha()).abs();
    assert!(gap < 1.1e-11, "{gap:e}");
    let force_gap = (warm.forces_ha_bohr.iter().flatten().flatten())
        .zip(cold.forces_ha_bohr.iter().flatten().flatten())
        .map(|(w, c)| (w - c).abs())
        .fold(0.0, f64::max);
    assert!(force_gap < 1e-9, "{force_gap:e}");
}

#[test]
fn a_session_starts_cold_when_any_setting_but_the_positions_changes() {
    let start = input(&START, "");
    let changed = [
        start.replace(
            "[6.0, 0.0, 0.0], [0.0, 6.0, 0.0]",
            "[6.2, 0.0, 0.0], [0.0, 6.0, 0.0]",
        ),
        start.replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            "list = [[0.0, 0.0, 0.0]]",
        ),
        input(&START[..1], ""),
        start.replace("cutoff_ha = 3.0", "cutoff_ha = 3.5"),
        start.replace("width_ha = 0.002", "width_ha = 0.003"),
        input(&START, "\n[spin]\nmoments = [1.0, 0.0]\n"),
    ];
    for text in changed {
        let mut session = Session::default();
        session
            .run(&read_spec(&start).unwrap(), synthetic, |_, _| {})
            .unwrap();
        let after = session
            .run(&read_spec(&text).unwrap(), synthetic, |_, _| {})
            .unwrap();
        assert_eq!(to_json(&after), to_json(&solve(&text).unwrap()), "{text}");
    }
}

#[test]
fn a_failed_run_leaves_the_session_cold() {
    let mut session = Session::default();
    session
        .run(&relaxing(&START), synthetic, |_, _| {})
        .unwrap();
    let one_step = input(&MOVED, "\n[scf]\nmax_iterations = 1\n");
    let failed = session.run(&read_spec(&one_step).unwrap(), synthetic, |_, _| {});
    assert!(matches!(failed, Err(Failure::Unconverged(_))), "{failed:?}");
    let after = session
        .run(&relaxing(&MOVED), synthetic, |_, _| {})
        .unwrap();
    assert_eq!(
        to_json(&after),
        to_json(&run(&relaxing(&MOVED), synthetic, |_, _| {}).unwrap())
    );
}

#[test]
fn a_session_rerun_of_the_same_geometry_starts_from_its_own_answer() {
    let mut session = Session::default();
    let first = session
        .run(&relaxing(&START), synthetic, |_, _| {})
        .unwrap();
    let again = session
        .run(&relaxing(&START), synthetic, |_, _| {})
        .unwrap();
    let (first, again) = (first.scf().unwrap().steps, again.scf().unwrap().steps);
    assert_eq!(
        (first > 1, again),
        (true, 1),
        "first {first}, again {again}"
    );
}
