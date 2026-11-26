use orbitar::model::{Cell, Draft, Functional, KPoints, Mixer, Probe, Smearing, Spec};

fn cube() -> Draft {
    let cell = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]];
    let kpoints = KPoints::list(vec![[0.0; 3], [0.5, 0.0, 0.0]]).unwrap();
    let mut draft = Draft::required(Cell::try_from(cell).unwrap(), 2.0, kpoints);
    draft.bands = Some(2);
    draft
}

fn error(draft: Draft) -> String {
    Spec::try_from(draft).unwrap_err()
}

#[test]
fn optional_keys_take_the_schema_1_defaults() {
    let draft = cube();
    assert_eq!(
        (draft.energy_tolerance_ha, draft.max_iterations),
        (1e-9, 100)
    );
    assert_eq!(
        (draft.mixer, draft.forces, draft.stress),
        (Mixer::Pulay, false, false)
    );
    assert!(draft.atoms.is_empty() && draft.smearing.is_none());
}

#[test]
fn a_valid_draft_becomes_a_spec_that_keeps_it() {
    assert_eq!(Spec::try_from(cube()).unwrap().draft(), &cube());
}

#[test]
fn an_empty_lattice_needs_at_least_one_band() {
    let mut draft = cube();
    draft.bands = None;
    assert!(error(draft.clone()).contains("bands.count"));
    draft.bands = Some(0);
    assert!(error(draft).contains("bands.count"));
}

#[test]
fn with_atoms_the_band_count_may_be_left_out() {
    let mut draft = cube();
    draft.bands = None;
    draft.atoms = vec![(14, [0.0; 3]), (14, [0.25; 3])];
    assert!(Spec::try_from(draft).is_ok());
}

#[test]
fn an_atom_needs_a_known_element_and_a_finite_position() {
    for (atom, problem) in [
        ((1, [0.0; 3]), None),
        ((118, [0.5; 3]), None),
        ((0, [0.0; 3]), Some("structure.species")),
        ((119, [0.0; 3]), Some("structure.species")),
        (
            (14, [f64::NAN, 0.0, 0.0]),
            Some("structure.positions_fractional"),
        ),
    ] {
        let mut draft = cube();
        draft.atoms = vec![atom];
        let result = Spec::try_from(draft);
        match problem {
            None => assert!(result.is_ok(), "{atom:?}"),
            Some(key) => assert!(result.unwrap_err().contains(key), "{atom:?}"),
        }
    }
}

#[test]
fn the_cutoff_must_be_positive_and_finite() {
    for cutoff in [0.0, -1.0, f64::INFINITY] {
        let mut draft = cube();
        draft.cutoff_ha = cutoff;
        assert!(error(draft).contains("basis.cutoff_ha"));
    }
}

#[test]
fn a_width_goes_with_every_smearing_but_fixed() {
    for (smearing, width, ok) in [
        (Some(Smearing::Fixed), None, true),
        (Some(Smearing::MarzariVanderbilt), Some(0.01), true),
        (Some(Smearing::FermiDirac), None, false),
        (Some(Smearing::Gaussian), Some(0.0), false),
        (Some(Smearing::Fixed), Some(0.01), false),
        (None, Some(0.01), false),
    ] {
        let mut draft = cube();
        (draft.smearing, draft.width_ha) = (smearing, width);
        match Spec::try_from(draft) {
            Ok(_) => assert!(ok, "{smearing:?} {width:?}"),
            Err(e) => assert!(
                !ok && e.contains("occupations."),
                "{smearing:?} {width:?}: {e}"
            ),
        }
    }
}

#[test]
fn the_energy_tolerance_must_be_positive() {
    let mut draft = cube();
    draft.energy_tolerance_ha = 0.0;
    assert!(error(draft).contains("scf.energy_tolerance_ha"));
}

#[test]
fn the_scf_needs_at_least_one_iteration() {
    let mut draft = cube();
    draft.max_iterations = 0;
    assert!(error(draft).contains("scf.max_iterations"));
}

#[test]
fn forces_and_stress_need_atoms() {
    for (forces, stress) in [(true, false), (false, true)] {
        let mut draft = cube();
        (draft.forces, draft.stress) = (forces, stress);
        assert!(error(draft.clone()).starts_with("outputs."));
        draft.atoms = vec![(14, [0.0; 3])];
        assert!(Spec::try_from(draft).is_ok());
    }
}

fn probing(probe: Probe) -> Draft {
    let mut draft = cube();
    (draft.bands, draft.probe) = (None, Some(probe));
    draft
}

#[test]
fn a_probe_has_no_species_and_no_bands() {
    let ewald = Probe::Ewald(vec![(1.0, [0.0; 3])]);
    assert!(Spec::try_from(probing(ewald.clone())).is_ok());
    let mut with_bands = probing(ewald.clone());
    with_bands.bands = Some(2);
    let mut with_atoms = probing(ewald);
    with_atoms.atoms = vec![(14, [0.0; 3])];
    for draft in [with_bands, with_atoms] {
        assert!(error(draft).starts_with("probe:"));
    }
}

#[test]
fn each_probe_term_needs_meaningful_values() {
    let point = [0.5, 0.5, 0.0, 0.0, 0.0];
    let pair = |a: [f64; 3], b: [f64; 3]| Probe::Ewald(vec![(1.0, a), (-1.0, b)]);
    for (probe, key) in [
        (Probe::Ewald(vec![]), "probe.charges"),
        (Probe::Ewald(vec![(f64::NAN, [0.0; 3])]), "probe.charges"),
        (
            pair([0.25, 0.0, 0.0], [1.25, 0.0, -2.0]),
            "probe.positions_fractional",
        ),
        (
            pair([0.0; 3], [1e-12, 0.0, 0.0]),
            "probe.positions_fractional",
        ),
        (Probe::Hartree(vec![]), "probe.charges"),
        (
            Probe::Hartree(vec![(1.0, 1.0, [f64::NAN; 3])]),
            "probe.charges",
        ),
        (
            Probe::Hartree(vec![(1.0, 0.0, [0.0; 3])]),
            "probe.exponents",
        ),
        (
            Probe::Xc {
                points: vec![point],
                tau: None,
            },
            "xc.functional",
        ),
    ] {
        assert!(error(probing(probe.clone())).starts_with(key), "{probe:?}");
    }
    let xc = |points: Vec<[f64; 5]>| {
        let mut draft = probing(Probe::Xc { points, tau: None });
        draft.functional = Some(Functional::Pbe);
        Spec::try_from(draft)
    };
    let bad = [
        [-0.1, 0.5, 0.0, 0.0, 0.0],
        [0.5, -0.1, 0.0, 0.0, 0.0],
        [0.0; 5],
        [0.5, 0.5, -0.1, 0.0, 0.0],
        [0.5, 0.5, 0.0, 0.0, -0.1],
        [0.5, 0.5, 0.01, 0.02, 0.01],
    ];
    for p in bad {
        assert!(
            xc(vec![p]).unwrap_err().starts_with("probe.points"),
            "{p:?}"
        );
    }
    assert!(xc(vec![]).unwrap_err().starts_with("probe.points"));
    let (polarized, aligned) = ([0.5, 0.0, 0.1, 0.0, 0.0], [0.3, 0.1, 0.02, 0.01, 0.005]);
    assert!(xc(vec![point, polarized, aligned]).is_ok());
    let with_tau = |tau: Vec<[f64; 2]>| {
        let mut draft = probing(Probe::Xc {
            points: vec![point, aligned],
            tau: Some(tau),
        });
        draft.functional = Some(Functional::R2scan);
        Spec::try_from(draft)
    };
    assert!(with_tau(vec![[0.0, 0.0], [0.2, 0.1]]).is_ok());
    for tau in [
        vec![[0.1, 0.1]],
        vec![[0.1, -0.1], [0.2, 0.1]],
        vec![[f64::NAN, 0.1], [0.2, 0.1]],
    ] {
        let error = with_tau(tau.clone()).unwrap_err();
        assert!(error.starts_with("probe.points: give τ↑, τ↓"), "{tau:?}");
    }
}

#[test]
fn the_density_cutoff_is_at_least_the_cutoff_and_twice_it_when_left_out() {
    let spec = Spec::try_from(cube()).unwrap();
    assert_eq!(spec.density_cutoff_ha(), 4.0);
    let mut draft = cube();
    draft.density_cutoff_ha = Some(2.0);
    assert_eq!(
        Spec::try_from(draft.clone()).unwrap().density_cutoff_ha(),
        2.0
    );
    for bad in [1.5, f64::INFINITY, f64::NAN] {
        draft.density_cutoff_ha = Some(bad);
        assert!(
            error(draft.clone()).starts_with("basis.density_cutoff_ha"),
            "{bad}"
        );
    }
}

#[test]
fn the_functional_is_pbe_unless_the_input_names_one() {
    assert_eq!(
        Spec::try_from(cube()).unwrap().functional(),
        Functional::Pbe
    );
    let mut draft = cube();
    draft.functional = Some(Functional::Lda);
    assert_eq!(Spec::try_from(draft).unwrap().functional(), Functional::Lda);
}

#[test]
fn spin_takes_atoms_one_finite_moment_each_and_a_smearing() {
    let with = |moments: Vec<f64>, smearing: Option<Smearing>| {
        let mut draft = cube();
        draft.atoms = vec![(26, [0.0; 3]), (26, [0.5; 3])];
        draft.bands = None;
        (draft.smearing, draft.width_ha) = (smearing, smearing.map(|_| 0.01));
        draft.moments = Some(moments);
        Spec::try_from(draft)
    };
    let fd = Some(Smearing::FermiDirac);
    assert!(with(vec![2.5, -2.5], fd).is_ok());
    for (moments, smearing) in [
        (vec![2.5], fd),
        (vec![2.5, f64::NAN], fd),
        (vec![2.5, -2.5], None),
    ] {
        let error = with(moments, smearing).unwrap_err();
        assert!(error.starts_with("spin.moments"), "{error}");
    }
    let mut empty = cube();
    (empty.smearing, empty.width_ha, empty.moments) = (fd, Some(0.01), Some(vec![]));
    assert_eq!(
        Spec::try_from(empty).unwrap_err(),
        "spin.moments: a spin-polarized run needs atoms"
    );
    let mut draft = cube();
    draft.atoms = vec![(26, [0.0; 3])];
    (draft.smearing, draft.width_ha, draft.moments) =
        (Some(Smearing::Fixed), None, Some(vec![2.0]));
    assert!(
        Spec::try_from(draft)
            .unwrap_err()
            .starts_with("spin.moments")
    );
}

#[test]
fn two_atoms_on_one_site_or_its_image_are_refused() {
    let mut draft = cube();
    draft.bands = None;
    draft.atoms = vec![(6, [0.1, 0.2, 0.3]), (8, [1.1, 0.2, -0.7])];
    assert!(
        error(draft.clone()).contains("structure.positions_fractional: two atoms share a site")
    );
    draft.atoms[1].1 = [0.6, 0.2, 0.3];
    assert!(Spec::try_from(draft).is_ok());
}
