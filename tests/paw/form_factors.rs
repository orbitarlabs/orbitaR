use crate::{synthetic_draft, synthetic_draft_on};
use orbitar::paw::RadialGrid;
use orbitar::paw::{CoreKinetic, Dataset, FormFactors};
use std::f64::consts::PI;

fn screened() -> FormFactors {
    screened_by(0.5)
}

fn screened_by(a: f64) -> FormFactors {
    let mut draft = synthetic_draft();
    draft.local_potential = draft
        .grid
        .radii()
        .iter()
        .map(|&r| {
            if r > 0.0 {
                -4.0 * (1.0 - (-r / a).exp()) / r
            } else {
                -4.0 / a
            }
        })
        .collect();
    FormFactors::tabulated(&Dataset::try_from(draft).unwrap(), 12.0)
}

#[test]
fn the_local_potential_keeps_the_coulomb_tail_of_the_valence_charge() {
    let f = screened();
    for q in [0.3_f64, 1.0, 4.7, 11.0] {
        let want = 16.0 * PI * (1.0 / (q * q + 4.0) - 1.0 / (q * q));
        assert!(
            (f.local_potential(q) - want).abs() < 1e-10 * want.abs(),
            "q {q}"
        );
    }
}

#[test]
fn epsatm_is_the_g0_limit_of_the_screened_part() {
    for (a, want) in [(0.5, 4.0 * PI), (0.25, PI)] {
        let got = screened_by(a).epsatm();
        assert!((got - want).abs() < 1e-10, "a {a}: {got} vs {want}");
    }
}

#[test]
fn the_pseudo_core_kinetic_energy_density_transforms_like_the_pseudo_core() {
    let mut draft = synthetic_draft();
    let tau: Vec<f64> = draft.pseudo_core_density.iter().map(|x| 3.0 * x).collect();
    let ae = draft.core_density.clone();
    draft.core_kinetic = Some(CoreKinetic::checked(&draft.grid, ae, tau).unwrap());
    let f = FormFactors::tabulated(&Dataset::try_from(draft).unwrap(), 12.0);
    for q in [0.0_f64, 0.9, 3.0, 8.0] {
        let got = f.pseudo_core_kinetic(q).unwrap();
        assert!((got - 4.8 * (-q * q / 4.0).exp()).abs() < 1e-10, "q {q}");
    }
    let plain = FormFactors::tabulated(&Dataset::try_from(synthetic_draft()).unwrap(), 12.0);
    assert_eq!(plain.pseudo_core_kinetic(1.0), None);
}

#[test]
fn the_densities_transform_to_their_closed_forms_and_charges() {
    let f = FormFactors::tabulated(&Dataset::try_from(synthetic_draft()).unwrap(), 12.0);
    for q in [0.0_f64, 0.9, 3.0, 8.0] {
        assert!(
            (f.pseudo_core(q) - 1.6 * (-q * q / 4.0).exp()).abs() < 1e-10,
            "q {q}"
        );
    }
    for q in [0.0_f64, 0.5, 2.0] {
        let want = 2.4 * PI / (1.0 + q * q).powi(2);
        assert!((f.pseudo_valence(q) - want).abs() < 1e-6 * want, "q {q}");
    }
}

#[test]
fn the_local_potential_ends_at_the_last_point_at_or_below_20_bohr() {
    let grid = RadialGrid::logarithmic(1e-3, 0.0101, 1001).unwrap();
    let below = grid.radii().partition_point(|&r| r <= 20.0) - 1;
    let r = grid.radii().to_vec();
    assert!(r[below + 1] - 20.0 < 20.0 - r[below]);
    let epsatm = |k: usize| {
        let mut draft = synthetic_draft_on(grid.clone());
        draft.local_potential[k] += 1e-3;
        FormFactors::tabulated(&Dataset::try_from(draft).unwrap(), 4.0).epsatm()
    };
    let base = FormFactors::tabulated(
        &Dataset::try_from(synthetic_draft_on(grid.clone())).unwrap(),
        4.0,
    )
    .epsatm();
    assert_eq!(epsatm(below + 1), base);
    assert_ne!(epsatm(below), base);
}

#[test]
fn the_slopes_are_the_derivatives_of_the_closed_forms() {
    let f = FormFactors::tabulated(&Dataset::try_from(synthetic_draft()).unwrap(), 12.0);
    let s = screened();
    for q in [0.3_f64, 1.0, 4.7, 11.0] {
        let local = 16.0 * PI * (-2.0 * q / (q * q + 4.0).powi(2) + 2.0 / q.powi(3));
        assert!(
            (s.local_potential_slope(q) - local).abs() < 1e-8 * local.abs().max(1.0),
            "q {q}: {} vs {local}",
            s.local_potential_slope(q)
        );
        let core = -0.8 * q * (-q * q / 4.0).exp();
        assert!((f.pseudo_core_slope(q) - core).abs() < 1e-8, "q {q}");
    }
}
