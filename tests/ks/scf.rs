use crate::{synthetic_3d_draft, synthetic_draft, synthetic_meta_draft};
use orbitar::io::{read_spec, to_json};
use orbitar::ks::{Failure, Outcome, eigensolver_floor, run};
use orbitar::paw::{CoreKinetic, Dataset, DatasetDraft};
use serde::Deserialize;
use std::collections::BTreeMap;

pub(super) fn input(positions: &[[f64; 3]], extra: &str) -> String {
    let rows: Vec<String> = positions.iter().map(|p| format!("{p:?}")).collect();
    format!(
        "schema = 1\n\n[structure]\ncell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]\n\
         species = [{}]\npositions_fractional = [{}]\n\n[basis]\ncutoff_ha = 3.0\n\n\
         [kpoints]\nlist = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]\n\n\
         [occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.002\n{extra}",
        vec!["\"C\""; positions.len()].join(", "),
        rows.join(", ")
    )
}

pub(super) fn synthetic(z: u8) -> Result<Dataset, Failure> {
    assert_eq!(z, 6, "only carbon is asked for");
    Ok(Dataset::try_from(synthetic_draft()).unwrap())
}

pub(super) fn synthetic_meta(z: u8) -> Result<Dataset, Failure> {
    assert_eq!(z, 6, "only carbon is asked for");
    Ok(Dataset::try_from(synthetic_meta_draft()).unwrap())
}

pub(super) fn solve(text: &str) -> Result<Outcome, Failure> {
    run(
        &read_spec(text).map_err(Failure::Invalid)?,
        synthetic,
        |_, _| {},
    )
}

pub(super) const ONE: [[f64; 3]; 1] = [[0.1, 0.2, 0.3]];

#[test]
fn an_unconverged_meta_gga_scf_names_its_kinetic_energy_density_change() {
    let text = input(
        &ONE,
        "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nmax_iterations = 2\n",
    );
    let result = run(&read_spec(&text).unwrap(), synthetic_meta, |_, _| {});
    let named = |m: &str| -> Option<f64> {
        m.split("‖Δτ̃‖ = ").nth(1)?.split(' ').next()?.parse().ok()
    };
    assert!(
        matches!(&result, Err(Failure::Unconverged(m)) if named(m).is_some_and(|c| c > 1e-6)),
        "{result:?}"
    );
}

#[test]
fn a_meta_gga_3d_crystal_is_one_state_with_and_without_symmetry() {
    let solve = |symmetry: &str| {
        let text = format!(
            "schema = 1\n\n[structure]\ncell_bohr = [[-2.7, 2.7, 2.4], [2.7, -2.7, 2.4], [2.7, 2.7, -2.4]]\n\
             species = [\"Fe\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n\n[basis]\ncutoff_ha = 5.0\n\n\
             [kpoints]\nmesh = [2, 2, 2]\nsymmetry = {symmetry}\n\n[spin]\nmoments = [2.0]\n\n[occupations]\n\
             smearing = \"fermi-dirac\"\nwidth_ha = 0.01\n\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\n\
             energy_tolerance_ha = 1e-10\n"
        );
        let datasets = |z: u8| {
            assert_eq!(z, 26, "only the synthetic iron is asked for");
            Ok(Dataset::try_from(synthetic_3d_draft()).unwrap())
        };
        run(&read_spec(&text).unwrap(), datasets, |_, _| {}).unwrap()
    };
    let (reduced, full) = (solve("true"), solve("false"));
    let gamma = |o: &Outcome| {
        (o.kpoints().points().iter())
            .position(|k| k == &[0.0; 3])
            .unwrap()
    };
    let bands = (0..2)
        .flat_map(|s| {
            let at = |o: &Outcome| o.eigenvalues_ha()[s][gamma(o)].clone();
            let (x, y) = (at(&reduced), at(&full));
            x.into_iter()
                .zip(y)
                .map(|(x, y)| (x - y).abs())
                .collect::<Vec<_>>()
        })
        .fold(0.0_f64, f64::max);
    let (a, b) = (reduced.scf().unwrap(), full.scf().unwrap());
    let moment = |s: &orbitar::ks::Scf| s.magnetization_bohr_magneton.unwrap();
    let gaps = [
        (a.energies.free_ha() - b.energies.free_ha()).abs(),
        (a.fermi_level_ha - b.fermi_level_ha).abs(),
        (moment(a) - moment(b)).abs(),
        bands,
    ];
    let bounds = [5e-10, 2e-11, 1e-10, 5e-11];
    assert!(
        gaps.iter().zip(bounds).all(|(g, b)| *g < b),
        "{gaps:?} against {bounds:?}"
    );
}

#[test]
fn at_the_stop_the_direct_free_energy_equals_the_double_counting_one() {
    type Datasets = fn(u8) -> Result<Dataset, Failure>;
    let cases: [(&str, Datasets); 2] = [
        ("", synthetic),
        ("\n[xc]\nfunctional = \"r2scan\"\n", synthetic_meta),
    ];
    for (extra, dataset) in cases {
        let outcome = run(&read_spec(&input(&ONE, extra)).unwrap(), dataset, |_, _| {}).unwrap();
        let scf = outcome.scf().unwrap();
        let gap = (scf.energies.free_ha() - scf.free_dc_ha).abs();
        assert!(gap <= 1e-9, "{extra:?}: {gap:e}");
    }
}

#[test]
fn at_a_loose_stop_both_free_energy_forms_lie_within_second_order_of_the_converged_one() {
    type Datasets = fn(u8) -> Result<Dataset, Failure>;
    let cases: [(&str, Datasets); 2] = [
        ("", synthetic),
        ("\n[xc]\nfunctional = \"r2scan\"\n", synthetic_meta),
    ];
    for (extra, dataset) in cases {
        let scf = |tolerance: &str| {
            let text = input(
                &ONE,
                &format!("{extra}\n[scf]\nenergy_tolerance_ha = {tolerance}\n"),
            );
            let outcome = run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
            let scf = outcome.scf().unwrap();
            (scf.energies.free_ha(), scf.free_dc_ha)
        };
        let ((direct, dc), (converged, _)) = (scf("1e-1"), scf("1e-10"));
        for (form, value) in [("direct", direct), ("double counting", dc)] {
            assert!(
                (value - converged).abs() < 1e-6,
                "{extra:?} {form}: {value} vs {converged}"
            );
        }
    }
}

#[derive(Deserialize)]
struct Archived {
    expected: Forms,
    state: State,
}

#[derive(Deserialize)]
struct Forms {
    direct_ha: f64,
    dc_ha: f64,
}

#[derive(Deserialize)]
struct State {
    volume: f64,
    points: usize,
    weights: Vec<f64>,
    eigenvalues: Vec<Vec<Vec<f64>>>,
    occupations: Vec<Vec<Vec<f64>>>,
    smearing_ha: f64,
    ewald_ha: f64,
    psp_core_ha: f64,
    #[serde(rename = "in")]
    input: [f64; 4],
    out: [f64; 4],
    v: Vec<Vec<f64>>,
    v_tau: Option<Vec<Vec<f64>>>,
    n_in: Vec<Vec<f64>>,
    n_out: Vec<Vec<f64>>,
    tau_in: Option<Vec<Vec<f64>>>,
    tau_out: Option<Vec<Vec<f64>>>,
    d: Vec<Vec<Vec<Vec<f64>>>>,
    rho_in: Vec<Vec<Vec<Vec<f64>>>>,
    rho_out: Vec<Vec<Vec<Vec<f64>>>>,
}

impl State {
    fn forms(&self) -> Forms {
        let grid = |v: &[Vec<f64>], n: &[Vec<f64>]| -> f64 {
            let sum: f64 = (v.iter().zip(n))
                .flat_map(|(v, n)| v.iter().zip(n).map(|(v, n)| v * n))
                .sum();
            self.volume / self.points as f64 * sum
        };
        let onsite = |rho: &[Vec<Vec<Vec<f64>>>]| -> f64 {
            (self.d.iter().flatten().flatten().flatten())
                .zip(rho.iter().flatten().flatten().flatten())
                .map(|(d, r)| d * r)
                .sum()
        };
        let kinetic = |tau: &Option<Vec<Vec<f64>>>| match (&self.v_tau, tau) {
            (Some(v), Some(t)) => grid(v, t),
            _ => 0.0,
        };
        let band: f64 = (self.eigenvalues.iter().zip(&self.occupations))
            .flat_map(|(e, f)| self.weights.iter().zip(e.iter().zip(f)))
            .flat_map(|(w, (e, f))| e.iter().zip(f).map(move |(e, f)| w * f * e))
            .sum();
        let fixed = self.ewald_ha + self.psp_core_ha + self.smearing_ha;
        let terms = |t: &[f64; 4]| t.iter().sum::<f64>();
        Forms {
            direct_ha: band
                - grid(&self.v, &self.n_out)
                - kinetic(&self.tau_out)
                - onsite(&self.rho_out)
                + terms(&self.out)
                + fixed,
            dc_ha: band - grid(&self.v, &self.n_in) - kinetic(&self.tau_in) - onsite(&self.rho_in)
                + terms(&self.input)
                + fixed,
        }
    }
}

#[expect(
    clippy::disallowed_macros,
    reason = "the test reads its own archived state"
)]
fn archived() -> BTreeMap<String, Archived> {
    serde_json::from_str(include_str!("scf-loose-stop.json")).unwrap()
}

#[test]
fn at_a_loose_stop_each_free_energy_form_equals_its_own_value_from_the_archived_state() {
    const BOUND: f64 = 1e-10;
    type Datasets = fn(u8) -> Result<Dataset, Failure>;
    let archived = archived();
    let cases: [(&str, &str, Datasets); 2] = [
        ("pbe", "", synthetic),
        (
            "r2scan",
            "\n[xc]\nfunctional = \"r2scan\"\n",
            synthetic_meta,
        ),
    ];
    for (name, extra, dataset) in cases {
        let Archived { expected, state } = &archived[name];
        let again = state.forms();
        assert!(
            (again.direct_ha - expected.direct_ha).abs() < 1e-13
                && (again.dc_ha - expected.dc_ha).abs() < 1e-13,
            "{name}: the archived values are the state's own"
        );
        let gap = (expected.direct_ha - expected.dc_ha).abs();
        assert!(gap > 100.0 * BOUND, "{name}: the forms part by {gap:e}");
        let text = input(
            &ONE,
            &format!("{extra}\n[scf]\nenergy_tolerance_ha = 1e-1\n"),
        );
        let outcome = run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
        let scf = outcome.scf().unwrap();
        let near = |a: &[Vec<Vec<f64>>], b: &[Vec<Vec<f64>>]| {
            (a.iter().flatten().flatten().count() == b.iter().flatten().flatten().count())
                && (a.iter().flatten().flatten())
                    .zip(b.iter().flatten().flatten())
                    .all(|(a, b)| (a - b).abs() < BOUND)
        };
        assert!(
            near(outcome.eigenvalues_ha(), &state.eigenvalues)
                && near(&scf.occupations, &state.occupations),
            "{name}: the run's bands are the archived step's"
        );
        let direct = scf.energies.free_ha();
        assert!(
            (direct - expected.direct_ha).abs() < BOUND,
            "{name} direct: {direct} vs {}",
            expected.direct_ha
        );
        assert!(
            (scf.free_dc_ha - expected.dc_ha).abs() < BOUND,
            "{name} double counting: {} vs {}",
            scf.free_dc_ha,
            expected.dc_ha
        );
    }
}

#[test]
fn a_run_without_any_pseudo_core_takes_no_pseudo_core_tau_on_the_grid() {
    let free = |no_core: bool, scale: f64| {
        let dataset = |z: u8| {
            assert_eq!(z, 6);
            let mut d = synthetic_meta_draft();
            if no_core {
                d.pseudo_core_density.fill(0.0);
            }
            let k = d.core_kinetic.take().unwrap();
            let pseudo = k.pseudo().iter().map(|t| t * scale).collect();
            d.core_kinetic = Some(CoreKinetic::checked(&d.grid, k.ae().to_vec(), pseudo).unwrap());
            Ok(Dataset::try_from(d).unwrap())
        };
        let text = input(&ONE, "\n[xc]\nfunctional = \"r2scan\"\n");
        let outcome = run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
        outcome.scf().unwrap().energies.free_ha()
    };
    assert_eq!(free(true, 1.0), free(true, 2.0));
    assert!((free(false, 1.0) - free(false, 2.0)).abs() > 1e-6);
}

#[test]
fn a_run_with_a_pseudo_core_takes_every_species_pseudo_core_tau_on_the_grid() {
    let free = |scale: f64| {
        let dataset = |z: u8| {
            let carbon = synthetic_meta_draft();
            let mut d = match z {
                6 => return Ok(Dataset::try_from(carbon).unwrap()),
                14 => DatasetDraft {
                    z: 14.0,
                    core_electrons: 10.0,
                    core_density: carbon.core_density.iter().map(|n| 5.0 * n).collect(),
                    ..carbon
                },
                _ => panic!("only carbon and silicon are asked for"),
            };
            d.pseudo_core_density.fill(0.0);
            let k = d.core_kinetic.take().unwrap();
            let pseudo = k.pseudo().iter().map(|t| t * scale).collect();
            d.core_kinetic = Some(CoreKinetic::checked(&d.grid, k.ae().to_vec(), pseudo).unwrap());
            Ok(Dataset::try_from(d).unwrap())
        };
        let pair = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8]];
        let text = input(&pair, "\n[xc]\nfunctional = \"r2scan\"\n")
            .replace("species = [\"C\", \"C\"]", "species = [\"C\", \"Si\"]");
        assert!(text.contains("\"Si\""));
        let outcome = run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
        outcome.scf().unwrap().energies.free_ha()
    };
    assert!((free(1.0) - free(2.0)).abs() > 1e-6);
}

#[test]
fn a_rust_dataset_whose_pseudo_core_stays_below_the_threshold_runs_as_one_without() {
    let free = |peak: f64| {
        let dataset = |z: u8| {
            let carbon = synthetic_meta_draft();
            let mut d = match z {
                6 => return Ok(Dataset::try_from(carbon).unwrap()),
                14 => DatasetDraft {
                    z: 14.0,
                    core_electrons: 10.0,
                    core_density: carbon.core_density.iter().map(|n| 5.0 * n).collect(),
                    ..carbon
                },
                _ => panic!("only carbon and silicon are asked for"),
            };
            let largest = d
                .pseudo_core_density
                .iter()
                .fold(0.0_f64, |m, x| m.max(x.abs()));
            d.pseudo_core_density
                .iter_mut()
                .for_each(|x| *x *= peak / largest);
            Ok(Dataset::try_from(d).unwrap())
        };
        let pair = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8]];
        let text = input(&pair, "\n[xc]\nfunctional = \"r2scan\"\n")
            .replace("species = [\"C\", \"C\"]", "species = [\"C\", \"Si\"]");
        let outcome = run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
        outcome.scf().unwrap().energies.free_ha()
    };
    assert_eq!(free(9.9e-7).to_bits(), free(0.0).to_bits());
}

#[test]
fn an_r2scan_output_holds_the_band_kinetic_energy_in_its_tau() {
    for spin in ["", "\n[spin]\nmoments = [1.0]\n"] {
        let text = input(&ONE, &format!("\n[xc]\nfunctional = \"r2scan\"\n{spin}"));
        let outcome = run(&read_spec(&text).unwrap(), synthetic_meta, |_, _| {}).unwrap();
        let scf = outcome.scf().unwrap();
        let tau = scf.kinetic_density_integral_ha.unwrap();
        let kinetic = scf.energies.kinetic_ha;
        assert!(
            (tau - kinetic).abs() <= 1e-12 * kinetic.abs(),
            "{spin:?}: {tau} vs {kinetic}"
        );
    }
    let pbe = solve(&input(&ONE, "")).unwrap();
    assert_eq!(pbe.scf().unwrap().kinetic_density_integral_ha, None);
}

#[test]
fn two_runs_print_the_same_bytes() {
    let text = input(&ONE, "");
    assert_eq!(
        to_json(&solve(&text).unwrap()),
        to_json(&solve(&text).unwrap())
    );
}

#[test]
fn the_output_does_not_depend_on_the_thread_count() {
    let two = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8]];
    let at = |threads: usize| {
        let extra = format!("\n[spin]\nmoments = [1.5, -1.0]\n\n[parallel]\nthreads = {threads}\n");
        to_json(&solve(&input(&two, &extra)).unwrap())
    };
    assert_eq!(at(1), at(3));
}

#[test]
fn three_atoms_give_the_output_of_one_thread_on_three() {
    let three = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8], [0.35, 0.85, 0.55]];
    let at = |threads: usize| {
        let extra = format!("\n[parallel]\nthreads = {threads}\n");
        to_json(&solve(&input(&three, &extra)).unwrap())
    };
    assert_eq!(at(1), at(3));
}

#[test]
fn more_threads_than_bands_per_thread_give_the_output_of_one() {
    let two = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8]];
    let at = |threads: usize| {
        let extra = format!("\n[parallel]\nthreads = {threads}\n");
        to_json(&solve(&input(&two, &extra)).unwrap())
    };
    assert_eq!(at(1), at(5));
}

#[test]
fn bands_that_cannot_hold_the_electrons_or_leave_the_top_one_occupied_are_refused() {
    let one = solve(&input(&ONE, "\n[bands]\ncount = 1\n"));
    assert!(
        matches!(one, Err(Failure::Invalid(m)) if m.starts_with("bands: 4 electrons need 2 bands"))
    );
    let two = solve(&input(&ONE, "\n[bands]\ncount = 2\n"));
    assert!(
        matches!(two, Err(Failure::Invalid(m)) if m.starts_with("bands: 4 electrons do not fit"))
    );
    let wide = input(&ONE, "\n[bands]\ncount = 3\n").replace("width_ha = 0.002", "width_ha = 0.02");
    let three = solve(&wide);
    assert!(
        matches!(three, Err(Failure::Invalid(m)) if m.starts_with("bands.count: the top of 3 bands holds"))
    );
}

#[test]
fn an_scf_out_of_steps_is_unconverged() {
    let result = solve(&input(&ONE, "\n[scf]\nmax_iterations = 2\n"));
    assert!(
        matches!(&result, Err(Failure::Unconverged(m)) if m.starts_with("scf.max_iterations")
            && m.contains("(limit 1e-11)") && m.contains("(limit 1e-10)") && m.contains("; not met: ")),
        "{result:?}"
    );
}

fn residuals(message: &str) -> (Vec<(String, f64, f64)>, Vec<String>) {
    let (body, unmet) = message.split_once("; not met: ").expect("a not-met list");
    let body = body.split_once(" bands, leave ").expect("the residuals").1;
    let body = body.replace(", and ", ", ").replace(") and ", "), ");
    let each = (body.split(", "))
        .map(|part| {
            let (name, rest) = part.split_once(" = ").unwrap();
            let (value, limit) = rest.split_once(" (limit ").unwrap();
            (
                name.to_owned(),
                value.parse().unwrap(),
                limit.trim_end_matches(')').parse().unwrap(),
            )
        })
        .collect();
    (each, unmet.split(", ").map(str::to_owned).collect())
}

#[test]
fn an_unconverged_message_names_each_residual_with_its_limit_and_only_the_unmet_ones() {
    type Datasets = fn(u8) -> Result<Dataset, Failure>;
    let cases: [(&str, Datasets); 4] = [
        ("\n[scf]\nmax_iterations = 2\n", synthetic),
        (
            "\n[scf]\nmax_iterations = 1\nenergy_tolerance_ha = 24.0\n",
            synthetic,
        ),
        (
            "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nmax_iterations = 2\n",
            synthetic_meta,
        ),
        (
            "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nmax_iterations = 3\nenergy_tolerance_ha = 3.0\n",
            synthetic_meta,
        ),
    ];
    for (extra, dataset) in cases {
        let Err(Failure::Unconverged(message)) =
            run(&read_spec(&input(&ONE, extra)).unwrap(), dataset, |_, _| {})
        else {
            panic!("{extra:?}: unconverged")
        };
        let (each, unmet) = residuals(&message);
        let above: Vec<String> = (each.iter())
            .filter(|(_, v, l)| v >= l)
            .map(|(n, _, _)| n.clone())
            .collect();
        let named: Vec<String> = unmet
            .iter()
            .filter(|n| !n.starts_with("the band"))
            .cloned()
            .collect();
        assert_eq!(named, above, "{message}");
        assert_eq!(
            each.len(),
            if extra.contains("r2scan") { 3 } else { 2 },
            "{message}"
        );
    }
}

#[test]
fn the_scf_stops_only_when_both_changes_lie_below_their_bounds() {
    let result = solve(&input(
        &ONE,
        "\n[scf]\nmax_iterations = 1\nenergy_tolerance_ha = 24.0\n",
    ));
    assert!(
        matches!(&result, Err(Failure::Unconverged(m)) if m.ends_with("(limit 2e0); not met: ‖Δñ‖")),
        "{result:?}"
    );
}

fn top_share(outcome: &Outcome) -> f64 {
    let scf = outcome.scf().unwrap();
    let full = 2.0 / scf.occupations.len() as f64;
    (scf.occupations.iter().flatten())
        .map(|k| k.last().unwrap() / full)
        .fold(0.0, f64::max)
}

const TWO: [[f64; 3]; 2] = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8]];

#[test]
fn a_smeared_default_count_starts_at_half_a_band_per_atom() {
    let spun = solve(&input(&TWO, "\n[spin]\nmoments = [-1.5, -1.0]\n")).unwrap();
    assert_eq!(spun.eigenvalues_ha()[0][0].len(), 4 + 1 + 2);
    assert!(top_share(&spun) <= 1e-14);
}

#[test]
fn a_smeared_default_count_grows_until_its_top_band_is_empty() {
    let wide = input(&TWO, "").replace("width_ha = 0.002", "width_ha = 0.02");
    let grown = solve(&wide).unwrap();
    assert!(grown.eigenvalues_ha()[0][0].len() > 12);
    assert!(top_share(&grown) <= 1e-14);
    let twelve = solve(&format!("{wide}\n[bands]\ncount = 12\n"));
    assert!(
        matches!(twelve, Err(Failure::Invalid(m)) if m.starts_with("bands.count: the top of 12 bands holds"))
    );
}

#[test]
fn a_grown_count_solves_its_input_again_and_then_converges_as_usual() {
    let grown = solve(&input(&TWO, "")).unwrap();
    assert_eq!(grown.eigenvalues_ha()[0][0].len(), 8);
    assert_eq!(grown.scf().unwrap().steps, 20);
}

#[test]
fn a_grown_count_does_not_depend_on_the_thread_count() {
    let at = |threads: usize| {
        let extra = format!("\n[spin]\nmoments = [1.5, -1.0]\n\n[parallel]\nthreads = {threads}\n");
        let text = input(&TWO, &extra).replace("width_ha = 0.002", "width_ha = 0.02");
        solve(&text).unwrap()
    };
    let one = at(1);
    assert!(one.eigenvalues_ha()[0][0].len() > 6);
    assert_eq!(to_json(&one), to_json(&at(3)));
}

#[test]
fn every_k_point_holds_the_computed_bands_or_the_count_is_refused() {
    let small = input(&ONE, "").replace("cutoff_ha = 3.0", "cutoff_ha = 1.2");
    assert!(solve(&format!("{small}\n[bands]\ncount = 7\n")).is_ok());
    let more = solve(&format!("{small}\n[bands]\ncount = 8\n"));
    assert!(
        matches!(more, Err(Failure::Invalid(m)) if m == "bands.count: 8 bands and 3 above them, but k-point 1 has 10 plane waves")
    );
}

#[test]
fn the_top_band_bound_counts_a_state_as_two_electrons_without_spin_and_one_with_it() {
    let at = |extra: &str| {
        let text = input(&ONE, &format!("\n[bands]\ncount = 3\n{extra}"));
        solve(&text.replace("width_ha = 0.002", "width_ha = 0.0021"))
    };
    let plain = at("");
    assert!(
        matches!(&plain, Err(Failure::Invalid(m)) if m.starts_with("bands.count: the top of 3 bands holds 1.8e-14"))
    );
    assert_eq!(plain, at("\n[spin]\nmoments = [0.0]\n"));
}

#[test]
fn the_first_step_starts_from_the_atoms_moments() {
    let change = |extra: &str| {
        let text = input(&ONE, &format!("{extra}\n[scf]\nmax_iterations = 1\n"));
        let Err(Failure::Unconverged(message)) = solve(&text) else {
            panic!("one step cannot meet the stop rule")
        };
        let value = message
            .split("‖Δñ‖ = ")
            .nth(1)
            .and_then(|s| s.split(' ').next());
        value
            .expect("the message names ‖Δñ‖")
            .parse::<f64>()
            .unwrap()
    };
    let (spin, plain) = (change("\n[spin]\nmoments = [2.0]\n"), change(""));
    assert!(spin > 1.2 * plain && spin < 2.0 * plain, "{spin} {plain}");
}

#[test]
fn fixed_occupations_fill_whole_bands_with_no_smearing_term() {
    let fixed = input(&ONE, "").replace(
        "smearing = \"fermi-dirac\"\nwidth_ha = 0.002\n",
        "smearing = \"fixed\"\n",
    );
    let outcome = solve(&fixed).unwrap();
    let scf = outcome.scf().unwrap();
    assert!(scf.occupations[0].iter().all(|k| k == &[2.0, 2.0]));
    assert_eq!(scf.energies.smearing_ha, 0.0);
    assert_eq!(
        scf.fermi_level_ha,
        outcome.eigenvalues_ha()[0]
            .iter()
            .map(|k| k[1])
            .fold(f64::MIN, f64::max)
    );
}

#[test]
fn a_grown_count_stops_where_a_k_point_holds_no_more_plane_waves() {
    let small = input(&ONE, "")
        .replace("cutoff_ha = 3.0", "cutoff_ha = 1.2")
        .replace("width_ha = 0.002", "width_ha = 0.2");
    let capped = solve(&small);
    assert!(
        matches!(&capped, Err(Failure::Invalid(m)) if m.starts_with("smearing: the top of 7 bands holds")
            && m.ends_with("k-point 1 has 10 plane waves, so no more bands fit; raise cutoff_ha")),
        "{capped:?}"
    );
}

#[test]
fn a_short_count_grows_even_when_the_first_step_already_meets_the_stop_rule() {
    let loose = input(&TWO, "\n[scf]\nenergy_tolerance_ha = 1000.0\n");
    let grown = solve(&loose).unwrap();
    assert!(grown.eigenvalues_ha()[0][0].len() > 5);
    assert!(top_share(&grown) <= 1e-14);
}

#[test]
fn an_r2scan_step_energy_lies_within_the_second_order_of_the_step_from_its_outputs_own_energy() {
    let stepped = |text: &str| {
        let mut steps = Vec::new();
        let outcome = run(&read_spec(text).unwrap(), synthetic_meta, |_, f| {
            steps.push(f)
        })
        .unwrap();
        (steps, outcome.scf().unwrap().energies.free_ha())
    };
    let pair = [[0.1, 0.2, 0.3], [0.55, 0.6, 0.7]];
    for (atoms, spin) in [
        (&ONE[..], ""),
        (&pair[..], "\n[spin]\nmoments = [2.0, 1.0]\n"),
    ] {
        let extra =
            format!("\n[xc]\nfunctional = \"r2scan\"\n{spin}\n[outputs]\nstep_energies = true\n");
        let (tight, _) = stepped(&input(atoms, &extra));
        for tolerance in [1e-1, 1e-2, 1e-3] {
            let scf = format!("\n[scf]\nenergy_tolerance_ha = {tolerance:e}\n");
            let (loose, own) = stepped(&input(atoms, &(extra.clone() + &scf)));
            let k = loose.len();
            assert!(k < tight.len(), "the looser run stops first");
            assert_eq!(loose[..k - 1], tight[..k - 1], "the same steps");
            let bound = 10.0 * (tolerance / 100.0_f64).powi(2);
            let gap = (tight[k - 1] - own).abs();
            assert!(
                gap <= bound,
                "{spin:?} tolerance {tolerance}: {gap:e} above {bound:e}"
            );
        }
    }
}

#[test]
fn a_step_energy_lies_within_the_second_order_of_the_step_from_its_outputs_own_energy() {
    let lda = |z: u8| {
        assert_eq!(z, 6, "only carbon is asked for");
        let mut draft = synthetic_draft();
        draft.functional = orbitar::model::Functional::Lda;
        Ok(Dataset::try_from(draft).unwrap())
    };
    let stepped = |text: &str| {
        let mut steps = Vec::new();
        let outcome = run(&read_spec(text).unwrap(), lda, |_, f| steps.push(f)).unwrap();
        (steps, outcome.scf().unwrap().energies.free_ha())
    };
    let pair = [[0.1, 0.2, 0.3], [0.55, 0.6, 0.7]];
    for (atoms, spin) in [
        (&ONE[..], ""),
        (&pair[..], "\n[spin]\nmoments = [2.0, 1.0]\n"),
    ] {
        let extra =
            format!("\n[xc]\nfunctional = \"lda\"\n{spin}\n[outputs]\nstep_energies = true\n");
        let (tight, _) = stepped(&input(atoms, &extra));
        for tolerance in [1e-1, 1e-2, 1e-3] {
            let scf = format!("\n[scf]\nenergy_tolerance_ha = {tolerance:e}\n");
            let (loose, own) = stepped(&input(atoms, &(extra.clone() + &scf)));
            let k = loose.len();
            assert!(k < tight.len(), "the looser run stops first");
            assert_eq!(loose[..k - 1], tight[..k - 1], "the same steps");
            let bound = 2.0 * (tolerance / 100.0_f64).powi(2);
            let gap = (tight[k - 1] - own).abs();
            assert!(
                gap <= bound,
                "tolerance {tolerance}: {gap:e} above {bound:e}"
            );
        }
    }
}

type Datasets = fn(u8) -> Result<Dataset, Failure>;

const PAIR: [[f64; 3]; 2] = [[0.1, 0.2, 0.3], [0.55, 0.6, 0.7]];

fn stop_rows() -> [(&'static [[f64; 3]], &'static str, Datasets); 4] {
    [
        (&ONE, "", synthetic),
        (&PAIR, "\n[spin]\nmoments = [2.0, 1.0]\n", synthetic),
        (&ONE, "\n[xc]\nfunctional = \"r2scan\"\n", synthetic_meta),
        (
            &PAIR,
            "\n[xc]\nfunctional = \"r2scan\"\n\n[spin]\nmoments = [2.0, 1.0]\n",
            synthetic_meta,
        ),
    ]
}

struct Stopped {
    eigenvalues: Vec<f64>,
    fermi_level: f64,
    free: f64,
    terms: [f64; 6],
    steps: usize,
}

fn stopped(atoms: &[[f64; 3]], extra: &str, dataset: Datasets) -> Stopped {
    let outcome = run(
        &read_spec(&input(atoms, extra)).unwrap(),
        dataset,
        |_, _| {},
    )
    .unwrap();
    let scf = outcome.scf().unwrap();
    let e = &scf.energies;
    Stopped {
        eigenvalues: (outcome.eigenvalues_ha().iter().flatten().flatten())
            .copied()
            .collect(),
        fermi_level: scf.fermi_level_ha,
        free: e.free_ha(),
        terms: [
            e.kinetic_ha,
            e.hartree_ha,
            e.xc_ha,
            e.local_ha,
            e.one_center_ha,
            e.smearing_ha,
        ],
        steps: scf.steps,
    }
}

fn largest_gap(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    (a.iter().zip(b))
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

#[test]
fn at_the_stop_eigenvalues_and_e_f_lie_within_0_54_terms_within_1_density_limit_of_a_tight_stop() {
    let scf = |tolerance: f64| {
        format!("\n[scf]\nenergy_tolerance_ha = {tolerance:e}\nmax_iterations = 300\n")
    };
    for (atoms, extra, dataset) in stop_rows() {
        let tight = stopped(atoms, &format!("{extra}{}", scf(1e-12)), dataset);
        for tolerance in [1e-6, 1e-8] {
            let limit = tolerance / 100.0;
            let loose = stopped(atoms, &format!("{extra}{}", scf(tolerance)), dataset);
            let shares = [
                (
                    "eigenvalues",
                    largest_gap(&loose.eigenvalues, &tight.eigenvalues),
                    0.54,
                ),
                ("E_F", (loose.fermi_level - tight.fermi_level).abs(), 0.54),
                ("F", (loose.free - tight.free).abs(), 0.01),
                ("terms", largest_gap(&loose.terms, &tight.terms), 1.0),
            ];
            for (what, gap, share) in shares {
                assert!(
                    gap <= share * limit,
                    "{extra:?} at {tolerance:e}: {what} {gap:e} above {share} × {limit:e}"
                );
            }
        }
    }
}

#[test]
fn the_eigensolver_tolerance_that_follows_the_density_change_costs_at_most_one_scf_step_over_the_dense_solver()
 {
    for (atoms, extra, dataset) in stop_rows() {
        let iterative = stopped(atoms, extra, dataset);
        let dense = stopped(
            atoms,
            &format!("{extra}\n[bands]\nsolver = \"dense\"\n"),
            dataset,
        );
        eprintln!("{extra:?}: {} {}", iterative.steps, dense.steps);
        assert!(
            iterative.steps <= dense.steps + 1,
            "{extra:?}: {} against {}",
            iterative.steps,
            dense.steps
        );
    }
}

#[test]
fn the_eigensolver_floor_is_a_tenth_of_the_density_stop_between_a_hundred_epsilon_and_1e_13() {
    let hundred_epsilon = 100.0 * f64::EPSILON;
    for (stop, want) in [(5e-13, 5e-14), (3e-13, 3e-14), (2.5e-13, 2.5e-14)] {
        let got = eigensolver_floor(stop);
        assert!(
            (got - want).abs() <= 2.0 * f64::EPSILON * want,
            "{stop:e}: {got:e}"
        );
    }
    assert_eq!(eigensolver_floor(1e-13), hundred_epsilon);
    assert_eq!(eigensolver_floor(1e-14), hundred_epsilon);
    assert_eq!(eigensolver_floor(1e-12), 1e-13);
    assert_eq!(eigensolver_floor(1e-11), 1e-13);
}

#[test]
fn a_tight_stop_at_a_complex_point_converges_within_a_step_of_the_old_floor() {
    let text = "schema = 1\n\n[structure]\n\
        cell_bohr = [[0.0, 3.8172467884289993, 3.8172467884289993], [3.8172467884289993, 0.0, 3.8172467884289993], \
        [3.8172467884289993, 3.8172467884289993, 0.0]]\n\
        species = [\"Fe\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n\n[basis]\ncutoff_ha = 12.0\n\n\
        [kpoints]\nlist = [[0.23, 0.12, 0.1]]\n\n[occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.00225\n\n\
        [scf]\nenergy_tolerance_ha = 1e-11\n\n[bands]\ncount = 45\n";
    let iron = |z: u8| {
        assert_eq!(z, 26, "only the synthetic iron is asked for");
        Ok(Dataset::try_from(synthetic_3d_draft()).unwrap())
    };
    let outcome = run(&read_spec(text).unwrap(), iron, |_, _| {}).unwrap();
    assert_eq!(outcome.basis_sizes(), [215]);
    let steps = outcome.scf().unwrap().steps;
    assert!(
        (19..=21).contains(&steps),
        "{steps} steps, where a fixed floor of 1e-13 takes 20"
    );
}
