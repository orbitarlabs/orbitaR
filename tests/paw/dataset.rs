use crate::io::paw_xml::xml;
use crate::{synthetic_draft, synthetic_meta_draft};
use orbitar::io::read_dataset;
use orbitar::model::Functional;
use orbitar::paw::{CoreKinetic, Dataset, DatasetDraft, RadialGrid, Shape, Species};

fn refused(change: impl Fn(&mut DatasetDraft)) -> String {
    let mut draft = synthetic_draft();
    change(&mut draft);
    Dataset::try_from(draft).unwrap_err()
}

#[test]
fn core_kinetic_energy_densities_need_one_finite_value_per_grid_point() {
    let grid = synthetic_draft().grid;
    let n = grid.radii().len();
    let ones = || vec![1.0; n];
    let mut nan = ones();
    nan[3] = f64::NAN;
    assert_eq!(
        CoreKinetic::checked(&grid, vec![1.0; n - 1], ones()).unwrap_err(),
        "ae_core_kinetic_energy_density: needs one finite value per grid point"
    );
    assert_eq!(
        CoreKinetic::checked(&grid, ones(), nan).unwrap_err(),
        "pseudo_core_kinetic_energy_density: needs one finite value per grid point"
    );
    assert!(CoreKinetic::checked(&grid, ones(), ones()).is_ok());
}

#[test]
fn a_core_kinetic_energy_density_must_fit_the_dataset_grid() {
    let other = RadialGrid::logarithmic(1e-3, 0.01, 501).unwrap();
    let kinetic = CoreKinetic::checked(&other, vec![1.0; 501], vec![1.0; 501]).unwrap();
    assert_eq!(
        refused(|d| d.core_kinetic = Some(kinetic.clone())),
        "core kinetic energy densities: need one value per point of the dataset grid"
    );
}

#[test]
fn a_dataset_whose_own_functional_is_a_meta_gga_needs_its_core_kinetic_energy_densities() {
    assert_eq!(
        refused(|d| d.functional = Functional::R2scan),
        "core kinetic energy densities: a dataset whose own functional is r2scan, a meta-GGA, needs them"
    );
    let mut draft = synthetic_meta_draft();
    draft.functional = Functional::R2scan;
    let species = Species::from(Dataset::try_from(draft).unwrap());
    assert!(species.core_xc().is_finite());
}

#[test]
fn a_consistent_dataset_passes_unchanged() {
    let draft = synthetic_draft();
    assert_eq!(Dataset::try_from(draft.clone()).unwrap().draft(), &draft);
}

#[test]
fn the_atomic_number_must_equal_core_plus_valence_with_valence_electrons() {
    assert!(refused(|d| d.z = 6.5).starts_with("atom:"));
    let no_valence = |d: &mut DatasetDraft| {
        d.z = 2.0;
        d.valence_electrons = 0.0;
    };
    assert!(refused(no_valence).starts_with("atom:"));
}

#[test]
fn every_radial_function_needs_one_finite_value_per_grid_point() {
    let message = "radial parts: each needs one finite value per grid point";
    assert_eq!(
        refused(|d| d.core_density.pop().map(drop).unwrap()),
        message
    );
    assert_eq!(refused(|d| d.local_potential[3] = f64::NAN), message);
    assert_eq!(refused(|d| d.channels[1].projector.push(0.0)), message);
    assert_eq!(
        refused(|d| d.pseudo_valence_density[0] = f64::INFINITY),
        message
    );
}

#[test]
fn the_shape_radius_lies_within_the_sphere_inside_the_grid() {
    let message = "paw_radius, shape_function: need 0 < r_shape ≤ r_PAW < the last grid radius";
    assert_eq!(
        refused(|d| d.shape = Shape::Sinc {
            radius: 1.5 * (1.0 + 1e-11)
        }),
        message
    );
    assert_eq!(refused(|d| d.shape = Shape::Sinc { radius: 0.0 }), message);
    assert_eq!(refused(|d| d.sphere_radius = 30.0), message);
    let last = *synthetic_draft().grid.radii().last().unwrap();
    assert_eq!(refused(|d| d.sphere_radius = last), message);
    assert_eq!(refused(|d| d.sphere_radius = 0.0), message);
    let mut draft = synthetic_draft();
    draft.shape = Shape::Sinc {
        radius: 1.5 * (1.0 + 1e-13),
    };
    assert!(Dataset::try_from(draft).is_ok());
}

#[test]
fn channels_exist_and_reach_at_most_l3() {
    let message = "valence_states: give at least one channel, each with l ≤ 3";
    assert_eq!(refused(|d| d.channels[1].l = 4), message);
    assert_eq!(
        refused(|d| {
            d.channels.clear();
            d.kinetic_differences.clear();
        }),
        message
    );
    let mut draft = synthetic_draft();
    draft.channels[1].l = 3;
    assert!(Dataset::try_from(draft).is_ok());
}

#[test]
fn kinetic_differences_form_a_symmetric_square_matrix() {
    let message = "kinetic_energy_differences: give a symmetric n × n matrix for n channels";
    assert_eq!(refused(|d| d.kinetic_differences[1] = 0.25), message);
    assert_eq!(refused(|d| d.kinetic_differences.push(0.0)), message);
    assert_eq!(refused(|d| d.kinetic_differences[3] = f64::NAN), message);
    let mut draft = synthetic_draft();
    draft.channels.truncate(1);
    (draft.z, draft.valence_electrons) = (4.0, 2.0);
    draft.kinetic_differences = vec![1.0, 1.0];
    assert_eq!(Dataset::try_from(draft.clone()).unwrap_err(), message);
    draft.kinetic_differences.truncate(1);
    assert!(Dataset::try_from(draft).is_ok());
}

#[test]
fn beyond_the_sphere_partial_waves_agree_and_projectors_vanish() {
    let message = "partial waves: beyond r_PAW, φ must equal φ̃ and p̃ must be 0";
    let last = synthetic_draft().grid.radii().len() - 1;
    let past = synthetic_draft()
        .grid
        .radii()
        .partition_point(|&r| r <= 1.5);
    assert_eq!(refused(|d| d.channels[0].pseudo[past] += 1e-9), message);
    assert_eq!(refused(|d| d.channels[1].projector[last] = 1e-9), message);
    let mut draft = synthetic_draft();
    draft.channels[0].projector[past - 1] = 7.0;
    draft.channels[0].pseudo[past - 1] = 7.0;
    assert!(Dataset::try_from(draft).is_ok());
}

#[test]
fn the_core_density_holds_the_core_electrons() {
    let message = "ae_core_density: ∫ 4π r² n_c dr must equal the core electron count";
    let scaled =
        |s: f64| move |d: &mut DatasetDraft| d.core_density.iter_mut().for_each(|x| *x *= s);
    assert_eq!(refused(scaled(1.0 + 2e-9)), message);
    assert_eq!(refused(scaled(1.0 - 2e-9)), message);
    let mut draft = synthetic_draft();
    scaled(1.0 + 0.7e-9)(&mut draft);
    assert!(Dataset::try_from(draft).is_ok());
}

#[test]
fn occupations_fit_their_shells_and_sum_to_the_valence_count() {
    let message = "valence_states: each f must lie in [0, 2(2l + 1)], and they must sum to the valence electrons";
    let moved = |s: f64, p: f64| {
        move |d: &mut DatasetDraft| {
            d.channels[0].occupation = Some(s);
            d.channels[1].occupation = Some(p);
        }
    };
    assert_eq!(refused(moved(3.0, 1.0)), message);
    assert_eq!(refused(moved(-1.0, 5.0)), message);
    assert_eq!(refused(moved(2.0, 2.5)), message);
    for (s, p) in [(0.0, 4.0), (2.0, 2.0)] {
        let mut draft = synthetic_draft();
        moved(s, p)(&mut draft);
        assert!(Dataset::try_from(draft).is_ok());
    }
    let mut draft = synthetic_draft();
    moved(2.0, 6.0)(&mut draft);
    (draft.z, draft.valence_electrons) = (10.0, 8.0);
    assert!(Dataset::try_from(draft).is_ok());
}

pub(crate) fn with_pseudo_core_peak(peak: f64) -> (DatasetDraft, usize) {
    let mut draft = synthetic_draft();
    let core = &mut draft.pseudo_core_density;
    let (top, largest) = (core.iter().enumerate()).fold((0, 0.0_f64), |(i, m), (j, x)| {
        if x.abs() > m { (j, x.abs()) } else { (i, m) }
    });
    core.iter_mut().for_each(|x| *x *= peak / largest);
    core[top] = peak;
    (draft, top)
}

#[test]
fn a_pseudo_core_below_1e_minus_6_everywhere_counts_as_none_on_both_entry_paths() {
    for (peak, kept) in [(9.9e-7, false), (1e-6, true)] {
        let (draft, top) = with_pseudo_core_peak(peak);
        let file = read_dataset(&xml(&draft)).unwrap();
        let rust = Dataset::try_from(draft.clone()).unwrap();
        assert_eq!(
            (rust.has_pseudo_core(), file.has_pseudo_core()),
            (kept, kept),
            "{peak}"
        );
        let core = &rust.draft().pseudo_core_density;
        match kept {
            true => assert_eq!(core, &draft.pseudo_core_density, "kept as given"),
            false => {
                assert!(core.iter().all(|&x| x == 0.0) && draft.pseudo_core_density[top] == peak)
            }
        }
    }
}
