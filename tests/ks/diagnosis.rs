use super::scf::{input, synthetic, synthetic_meta};
use orbitar::io::read_spec;
use orbitar::ks::{Diagnosis, Outcome, run_diagnosed};

fn pair() -> (Outcome, Diagnosis) {
    let text = input(
        &[[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]],
        "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n",
    );
    let (outcome, diagnosis) =
        run_diagnosed(&read_spec(&text).unwrap(), synthetic_meta, |_, _| {}).unwrap();
    (
        outcome,
        diagnosis.expect("a crystal with atoms has a diagnosis"),
    )
}

#[test]
fn a_states_sphere_energy_in_its_own_frame_is_its_one_center_term() {
    let (outcome, diagnosis) = pair();
    let one_center = outcome.scf().unwrap().energies.one_center_ha;
    let spheres = diagnosis.sphere_energy_ha(&diagnosis);
    assert!(
        (spheres - one_center).abs() < 1e-11 * one_center.abs(),
        "{spheres} vs {one_center}"
    );
}

#[test]
fn a_path_of_no_length_has_no_grid_gap() {
    let (_, diagnosis) = pair();
    let grid = diagnosis.grid_gap(&diagnosis, &diagnosis).unwrap();
    assert_eq!((grid.bound_ha, grid.predicted_ha), (0.0, 0.0), "{grid:?}");
}

#[test]
fn states_of_other_atoms_or_functionals_are_refused_by_name() {
    let (_, pair) = pair();
    let solved = |positions: &[[f64; 3]], functional: &str| {
        let text = input(
            positions,
            &format!("\n[xc]\nfunctional = \"{functional}\"\n"),
        );
        let dataset = if functional == "r2scan" {
            synthetic_meta
        } else {
            synthetic
        };
        run_diagnosed(&read_spec(&text).unwrap(), dataset, |_, _| {})
            .unwrap()
            .1
            .unwrap()
    };
    let one = solved(&[[0.1, 0.2, 0.3]], "r2scan");
    let refused = pair.sphere_contraction_ha(&one).unwrap_err();
    assert!(
        refused.starts_with("diagnosis: 2 atoms against 1"),
        "{refused}"
    );
    let pbe = solved(&[[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]], "pbe");
    let refused = pair.grid_gap(&pbe, &pair).unwrap_err();
    assert!(
        refused.starts_with("diagnosis: the functionals differ"),
        "{refused}"
    );
}

#[test]
fn states_of_one_element_on_other_datasets_are_refused_by_name() {
    let (_, pair) = pair();
    let reordered = |z: u8| {
        assert_eq!(z, 6, "only carbon is asked for");
        let mut d = crate::synthetic_meta_draft();
        d.channels.reverse();
        let k = d.kinetic_differences.clone();
        d.kinetic_differences = vec![k[3], k[2], k[1], k[0]];
        Ok(orbitar::paw::Dataset::try_from(d).unwrap())
    };
    let text = input(
        &[[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]],
        "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n",
    );
    let (_, other) = run_diagnosed(&read_spec(&text).unwrap(), reordered, |_, _| {}).unwrap();
    let refused = pair.sphere_contraction_ha(&other.unwrap()).unwrap_err();
    assert!(
        refused.starts_with("diagnosis: atom 0 has another dataset (Z 6 with channels l = [0, 1], against Z 6 with [1, 0])"),
        "{refused}"
    );
}
