use super::scf::{Datasets, ONE, input, solve, synthetic, synthetic_meta};
use crate::{synthetic_3d_draft, synthetic_draft, synthetic_meta_draft};
use orbitar::io::read_spec;
use orbitar::ks::{Diagnosis, Failure, GridGap, Outcome, ewald_energy_ha, run, run_diagnosed};
use orbitar::model::Cell;
use orbitar::paw::{CoreKinetic, Dataset, DatasetDraft, FormFactors};
use orbitar::pw::{Basis, Grid};
use std::f64::consts::TAU;

fn difference(a: &Outcome, b: &Outcome) -> f64 {
    let flat = |o: &Outcome| -> Vec<f64> {
        let e = &o.scf().unwrap().energies;
        let mut all: Vec<f64> = o
            .eigenvalues_ha()
            .iter()
            .flatten()
            .flatten()
            .copied()
            .collect();
        all.extend([
            e.free_ha(),
            e.kinetic_ha,
            e.hartree_ha,
            e.xc_ha,
            e.local_ha,
            e.one_center_ha,
        ]);
        all.push(o.scf().unwrap().fermi_level_ha);
        all
    };
    let (p, q) = (flat(a), flat(b));
    assert_eq!(p.len(), q.len());
    p.iter()
        .zip(&q)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

#[test]
fn the_ground_state_holds_its_electrons_two_per_band_at_most() {
    let outcome = solve(&input(&ONE, "")).unwrap();
    let occupations = &outcome.scf().unwrap().occupations[0];
    let held: f64 = occupations.iter().flatten().sum::<f64>() / 2.0;
    assert!((held - 4.0).abs() < 1e-12, "{held}");
    assert!(
        occupations
            .iter()
            .flatten()
            .all(|f| (0.0..=2.0).contains(f))
    );
}

#[test]
fn the_free_energy_is_the_sum_of_its_terms() {
    let outcome = solve(&input(&ONE, "")).unwrap();
    let e = &outcome.scf().unwrap().energies;
    let sum = e.kinetic_ha
        + e.hartree_ha
        + e.xc_ha
        + e.ewald_ha
        + e.psp_core_ha
        + e.local_ha
        + e.one_center_ha;
    assert_eq!(e.internal_ha(), sum);
    assert_eq!(e.free_ha(), sum + e.smearing_ha);
    assert!(e.smearing_ha <= 0.0 && e.kinetic_ha > 0.0);
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]).unwrap();
    assert_eq!(
        e.ewald_ha,
        ewald_energy_ha(&cell, &[(4.0, ONE[0])]).unwrap()
    );
    let form = FormFactors::tabulated(&synthetic(6).unwrap(), 1.0);
    assert!((e.psp_core_ha - form.epsatm() * 4.0 / cell.volume()).abs() < 1e-15);
}

#[test]
fn the_scf_takes_steps_to_bands_that_ascend_at_each_k_point() {
    let outcome = solve(&input(&ONE, "")).unwrap();
    for k in outcome.eigenvalues_ha()[0].iter() {
        assert!(k.windows(2).all(|w| w[0] <= w[1]));
    }
    let steps = outcome.scf().unwrap().steps;
    assert!((4..=20).contains(&steps), "{steps} steps");
}

#[test]
fn the_order_of_the_atoms_changes_no_output() {
    let atoms = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.8]];
    let forward = solve(&input(&atoms, "")).unwrap();
    let backward = solve(&input(&[atoms[1], atoms[0]], "")).unwrap();
    assert!(difference(&forward, &backward) < 1e-10);
}

#[test]
fn a_translation_that_maps_both_grids_onto_themselves_changes_no_output() {
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]).unwrap();
    let coarse = Grid::coarse(&cell, 3.0);
    let fine = Grid::fine(6.0, &coarse);
    let gcd = |mut a: usize, mut b: usize| {
        while b != 0 {
            (a, b) = (b, a % b);
        }
        a
    };
    let step: [f64; 3] =
        std::array::from_fn(|i| 1.0 / gcd(coarse.dims()[i], fine.dims()[i]) as f64);
    assert!(
        step.iter().all(|&s| s < 1.0),
        "{:?} {:?}",
        coarse.dims(),
        fine.dims()
    );
    let moved = [std::array::from_fn(|i| ONE[0][i] + step[i])];
    let here = solve(&input(&ONE, "")).unwrap();
    let there = solve(&input(&moved, "")).unwrap();
    assert!(difference(&here, &there) < 1e-10);
}

#[test]
fn a_basis_of_three_plane_waves_per_band_or_fewer_is_solved_densely() {
    let small = input(&ONE, "\n[bands]\ncount = 6\n").replace("cutoff_ha = 3.0", "cutoff_ha = 1.2");
    let default = solve(&small).unwrap();
    let dense = solve(&format!("{small}solver = \"dense\"\n")).unwrap();
    assert_eq!(default.basis_sizes(), [19, 10]);
    assert!(difference(&default, &dense) < 1e-12);
}

#[test]
fn the_dense_solver_gives_the_iterative_ground_state() {
    for k in ["[0.5, 0.0, 0.0]", "[0.3, 0.1, 0.0]"] {
        let at = |extra: &str| input(&ONE, extra).replace("[0.5, 0.0, 0.0]", k);
        let iterative = solve(&at("")).unwrap();
        let dense = solve(&at("\n[bands]\nsolver = \"dense\"\n")).unwrap();
        assert!(difference(&iterative, &dense) < 1e-10, "{k}");
    }
}

fn largest_gap(a: impl IntoIterator<Item = f64>, b: impl IntoIterator<Item = f64>) -> f64 {
    (a.into_iter().zip(b))
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

#[test]
fn at_real_k_points_two_bands_per_transform_give_the_dense_complex_state_forces_and_stress() {
    let two = [[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]];
    let rows: [(&str, Datasets); 3] = [
        ("", synthetic),
        ("\n[spin]\nmoments = [1.5, -1.0]\n", synthetic),
        ("\n[xc]\nfunctional = \"r2scan\"\n", synthetic_meta),
    ];
    for (extra, dataset) in rows {
        let at = |solver: &str| {
            let outputs = "\n[outputs]\nforces = true\nstress = true\n";
            let text = input(
                &two,
                &format!("{extra}{outputs}\n[bands]\nsolver = \"{solver}\"\n"),
            );
            run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap()
        };
        let (paired, dense) = (at("lobpcg"), at("dense"));
        let state = difference(&paired, &dense);
        let force = largest_gap(
            forces(&paired).into_iter().flatten(),
            forces(&dense).into_iter().flatten(),
        );
        let sigma = largest_gap(
            stress(&paired).into_iter().flatten(),
            stress(&dense).into_iter().flatten(),
        );
        eprintln!("{extra:?}: state {state:e} forces {force:e} stress {sigma:e}");
        assert!(
            state < 1e-10 && force < 1e-10 && sigma < 1e-11,
            "{extra:?}: state {state:e}, forces {force:e}, stress {sigma:e}"
        );
    }
}

#[test]
fn the_free_energy_is_stationary_at_the_self_consistent_state() {
    let free = |extra: &str| {
        let outcome = solve(&input(&ONE, extra)).unwrap();
        outcome.scf().unwrap().energies.free_ha()
    };
    let loose = free("\n[scf]\nenergy_tolerance_ha = 1e-5\n");
    let tight = free("\n[scf]\nenergy_tolerance_ha = 1e-10\n");
    assert!((loose - tight).abs() < 1e-10, "{loose} {tight}");
}

#[test]
fn the_r2scan_free_energy_is_stationary_at_the_self_consistent_state() {
    for spin in ["", "\n[spin]\nmoments = [1.0]\n"] {
        let free = |extra: &str| {
            let text = input(
                &ONE,
                &format!("\n[xc]\nfunctional = \"r2scan\"\n{spin}{extra}"),
            );
            let outcome = run(&read_spec(&text).unwrap(), synthetic_meta, |_, _| {}).unwrap();
            outcome.scf().unwrap().energies.free_ha()
        };
        let loose = free("\n[scf]\nenergy_tolerance_ha = 1e-5\n");
        let tight = free("\n[scf]\nenergy_tolerance_ha = 1e-10\n");
        assert!((loose - tight).abs() < 1e-10, "{spin:?}: {loose} {tight}");
    }
}

#[test]
fn a_supercell_holds_twice_the_free_energy_of_its_cell() {
    let cell = solve(&input(&ONE, "")).unwrap();
    let atoms = [[0.05, 0.2, 0.3], [0.55, 0.2, 0.3]];
    let doubled = input(&atoms, "")
        .replace("[[6.0, 0.0, 0.0]", "[[12.0, 0.0, 0.0]")
        .replace("[[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]", "[[0.0, 0.0, 0.0]]");
    let doubled = solve(&doubled).unwrap();
    let (one, two) = (cell.scf().unwrap(), doubled.scf().unwrap());
    let (e, d) = (&one.energies, &two.energies);
    for (single, double) in [
        (e.free_ha(), d.free_ha()),
        (e.kinetic_ha, d.kinetic_ha),
        (e.hartree_ha, d.hartree_ha),
        (e.xc_ha, d.xc_ha),
        (e.local_ha, d.local_ha),
        (e.one_center_ha, d.one_center_ha),
        (e.smearing_ha, d.smearing_ha),
    ] {
        assert!((double - 2.0 * single).abs() < 1e-10, "{single} {double}");
    }
    assert!((two.fermi_level_ha - one.fermi_level_ha).abs() < 1e-10);
}

fn lithium(_: u8) -> Result<Dataset, Failure> {
    let mut draft = synthetic_draft();
    (draft.z, draft.valence_electrons) = (3.0, 1.0);
    (draft.channels[0].occupation, draft.channels[1].occupation) = (Some(1.0), Some(0.0));
    Ok(Dataset::try_from(draft).unwrap())
}

#[test]
fn one_electron_polarizes_fully_either_way_and_lowers_the_energy() {
    let text = input(&ONE, "")
        .replace("\"C\"", "\"Li\"")
        .replace("[[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]", "[[0.0, 0.0, 0.0]]");
    let solve = |extra: &str| {
        run(
            &read_spec(&format!("{text}{extra}")).unwrap(),
            lithium,
            |_, _| {},
        )
        .unwrap()
    };
    let plain = solve("");
    let (up, down) = (
        solve("\n[spin]\nmoments = [1.0]\n"),
        solve("\n[spin]\nmoments = [-1.0]\n"),
    );
    let free = |o: &Outcome| o.scf().unwrap().energies.free_ha();
    let moment = |o: &Outcome| o.scf().unwrap().magnetization_bohr_magneton.unwrap();
    assert!((moment(&up) - 1.0).abs() < 1e-9 && (moment(&down) + 1.0).abs() < 1e-9);
    assert!((free(&up) - free(&down)).abs() < 1e-12);
    assert!(free(&up) < free(&plain) - 0.01);
    let [a, b] = up.eigenvalues_ha() else {
        panic!("two spins")
    };
    let [c, d] = down.eigenvalues_ha() else {
        panic!("two spins")
    };
    for (x, y) in a.iter().chain(b).flatten().zip(d.iter().chain(c).flatten()) {
        assert!((x - y).abs() < 1e-10, "{x} {y}");
    }
}

#[test]
fn the_irreducible_points_give_the_full_mesh_state() {
    let cubic = "cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]";
    let fcc = "cell_bohr = [[0.0, 3.4, 3.4], [3.4, 0.0, 3.4], [3.4, 3.4, 0.0]]";
    for (cell, positions, mesh, extra, count) in [
        (cubic, vec![[0.0; 3]], "[3, 3, 3]", "", 4),
        (
            cubic,
            vec![[0.0; 3], [0.5; 3]],
            "[2, 2, 2]",
            "\n[spin]\nmoments = [1.0, -1.0]\n\n[bands]\ncount = 12\n\n[scf]\nenergy_tolerance_ha = 1e-11\n",
            4,
        ),
        (
            cubic,
            vec![[0.1, 0.0, 0.0], [0.9, 0.0, 0.0]],
            "[3, 3, 2]",
            "",
            8,
        ),
        (
            cubic,
            vec![[0.5, 0.0, 0.0], [0.0, 0.5, 0.0], [0.0, 0.0, 0.5]],
            "[2, 2, 2]",
            "",
            4,
        ),
        (fcc, vec![[0.0; 3], [0.25; 3]], "[2, 2, 2]", "", 3),
        (
            cubic,
            vec![[0.1, 0.2, 0.3], [0.9, 0.8, 0.3], [0.0; 3]],
            "[3, 1, 3]",
            "",
            4,
        ),
    ] {
        let text = |symmetry: bool| {
            input(&positions, extra).replace(cubic, cell).replace(
                "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
                &format!("mesh = {mesh}\nsymmetry = {symmetry}"),
            )
        };
        let (full, reduced) = (solve(&text(false)).unwrap(), solve(&text(true)).unwrap());
        assert_eq!(reduced.kpoints().points().len(), count, "{positions:?}");
        assert!((reduced.kpoints().weights().iter().sum::<f64>() - 1.0).abs() < 1e-14);
        let (f, r) = (full.scf().unwrap(), reduced.scf().unwrap());
        let (a, b) = (f.energies, r.energies);
        for (x, y) in [
            (a.kinetic_ha, b.kinetic_ha),
            (a.hartree_ha, b.hartree_ha),
            (a.xc_ha, b.xc_ha),
            (a.local_ha, b.local_ha),
            (a.one_center_ha, b.one_center_ha),
            (a.smearing_ha, b.smearing_ha),
            (f.fermi_level_ha, r.fermi_level_ha),
            (
                f.magnetization_bohr_magneton.unwrap_or(0.0),
                r.magnetization_bohr_magneton.unwrap_or(0.0),
            ),
        ] {
            assert!((x - y).abs() < 1e-10, "{x} {y}");
        }
        for (k, rows) in reduced
            .kpoints()
            .points()
            .iter()
            .zip(&reduced.eigenvalues_ha()[0])
        {
            let i = full.kpoints().points().iter().position(|q| q == k).unwrap();
            for (x, y) in rows.iter().zip(&full.eigenvalues_ha()[0][i]) {
                assert!((x - y).abs() < 1e-10, "{k:?}: {x} {y}");
            }
        }
    }
}

#[test]
fn an_operation_that_moves_a_density_sphere_edge_off_the_sphere_drops_out() {
    let text = |cutoff: f64, symmetry: bool| {
        input(
            &[[1.0 / 3.0, 2.0 / 3.0, 0.25], [2.0 / 3.0, 1.0 / 3.0, 0.75]],
            "",
        )
        .replace(
            "cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]",
            "cell_bohr = [[5.4, 0.0, 0.0], [-2.7, 4.676535, 0.0], [0.0, 0.0, 8.8]]",
        )
        .replace("cutoff_ha = 3.0", &format!("cutoff_ha = {cutoff}"))
        .replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            &format!("mesh = [2, 2, 2]\nsymmetry = {symmetry}"),
        )
    };
    let points = |o: &Outcome| o.kpoints().points().len();
    assert_eq!(points(&solve(&text(3.0503690, true)).unwrap()), 4);
    let (reduced, full) = (
        solve(&text(3.0503709, true)).unwrap(),
        solve(&text(3.0503709, false)).unwrap(),
    );
    assert_eq!((points(&reduced), points(&full)), (6, 8));
    let (a, b) = (reduced.scf().unwrap(), full.scf().unwrap());
    assert!((a.energies.free_ha() - b.energies.free_ha()).abs() < 1e-10);
    assert!((a.fermi_level_ha - b.fermi_level_ha).abs() < 1e-10);
}

#[test]
fn the_magnetization_weighs_each_kpoint() {
    let text = format!(
        "{}\n[spin]\nmoments = [1.0]\n",
        input(&ONE, "").replace("\"C\"", "\"Li\"")
    );
    let outcome = run(&read_spec(&text).unwrap(), lithium, |_, _| {}).unwrap();
    let moment = outcome.scf().unwrap().magnetization_bohr_magneton.unwrap();
    assert!((moment - 1.0).abs() < 1e-9, "{moment}");
}

fn lda(positions: &[[f64; 3]], extra: &str, edit: impl Fn(String) -> String) -> Outcome {
    let text = edit(input(
        positions,
        &format!("\n[xc]\nfunctional = \"lda\"\n{extra}"),
    ));
    let dataset = |_| {
        let mut draft = synthetic_draft();
        draft.functional = orbitar::model::Functional::Lda;
        Ok(Dataset::try_from(draft).unwrap())
    };
    run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap()
}

fn forces(outcome: &Outcome) -> Vec<[f64; 3]> {
    outcome.scf().unwrap().forces_ha_bohr.clone().unwrap()
}

#[test]
fn forces_are_minus_the_derivative_of_the_free_energy_with_and_without_spin() {
    let at = [[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]];
    let direction = [[0.3, -0.5, 0.2], [-0.4, 0.1, 0.6]];
    let (h, side) = (1e-4, 6.0);
    for spin in ["", "\n[spin]\nmoments = [1.5, -1.0]\n"] {
        let extra = format!("{spin}\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n");
        let energy = |t: f64| {
            let moved: Vec<[f64; 3]> = (at.iter().zip(&direction))
                .map(|(p, d)| std::array::from_fn(|i| p[i] + t * h * d[i] / side))
                .collect();
            lda(&moved, &extra, |t| t).scf().unwrap().energies.free_ha()
        };
        let with = format!("{extra}\n[outputs]\nforces = true\n");
        let along: f64 = (forces(&lda(&at, &with, |t| t)).iter().zip(&direction))
            .map(|(f, d)| (0..3).map(|i| f[i] * d[i]).sum::<f64>())
            .sum();
        let slope = (energy(1.0) - energy(-1.0)) / (2.0 * h);
        assert!(along.abs() > 0.05, "{along}");
        assert!((along + slope).abs() < 1e-8, "{along} vs {slope}");
    }
}

#[test]
fn forces_on_the_irreducible_k_points_equal_those_of_the_full_mesh() {
    let atoms = [[0.1, 0.2, 0.0], [-0.1, -0.2, 0.0], [0.0, 0.0, 0.4]];
    let on_mesh = |symmetry: bool| {
        let mesh = format!("mesh = [3, 3, 3]\nsymmetry = {symmetry}");
        let outcome = lda(&atoms, "\n[outputs]\nforces = true\n", |t| {
            t.replace("list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]", &mesh)
        });
        (outcome.kpoints().points().len(), forces(&outcome))
    };
    let ((solved, reduced), (all, full)) = (on_mesh(true), on_mesh(false));
    assert!(solved < all && all == 27, "{solved} of {all}");
    for (a, b) in reduced.iter().zip(&full) {
        for i in 0..3 {
            assert!((a[i] - b[i]).abs() < 1e-9, "{reduced:?} vs {full:?}");
        }
    }
    assert!(full[0].iter().all(|f| f.abs() > 1e-3), "{full:?}");
}

#[test]
fn a_cell_too_wide_for_its_closest_pair_is_unsupported() {
    let close = input(&[[0.1, 0.2, 0.3], [0.15, 0.2, 0.3]], "");
    let refused = solve(&close).err();
    assert!(
        matches!(&refused, Some(Failure::Unsupported(m)) if m.starts_with("structure: the Ewald sum")),
        "{refused:?}"
    );
}

#[test]
fn a_start_with_fewer_orbitals_than_bands_reaches_the_dense_ground_state() {
    let extra = "\n[bands]\ncount = 10\n";
    let iterative = solve(&input(&ONE, extra)).unwrap();
    let dense = solve(&input(&ONE, &format!("{extra}solver = \"dense\"\n"))).unwrap();
    assert!(difference(&iterative, &dense) < 1e-10);
}

#[test]
fn a_start_with_more_orbitals_than_bands_reaches_the_dense_ground_state() {
    let four = [
        [0.1, 0.2, 0.3],
        [0.6, 0.7, 0.8],
        [0.35, 0.9, 0.55],
        [0.85, 0.4, 0.05],
    ];
    let iterative = solve(&input(&four, "")).unwrap();
    let dense = solve(&input(&four, "\n[bands]\nsolver = \"dense\"\n")).unwrap();
    assert!(difference(&iterative, &dense) < 1e-10);
}

fn stress(outcome: &Outcome) -> [[f64; 3]; 3] {
    outcome.scf().unwrap().stress_ha_bohr3.unwrap()
}

#[test]
fn stress_is_the_strain_derivative_of_the_free_energy_per_volume_with_and_without_spin() {
    let at = [[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]];
    let e = [[0.3, 0.5, -0.2], [0.5, -0.4, 0.6], [-0.2, 0.6, 0.1]];
    let (h, side) = (1e-5, 6.0);
    let cube = "cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]";
    for spin in ["", "\n[spin]\nmoments = [1.5, -1.0]\n"] {
        let extra = format!("{spin}\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n");
        let strained = |t: f64| {
            let rows: Vec<String> = (0..3)
                .map(|r| {
                    format!(
                        "{:?}",
                        std::array::from_fn::<f64, 3, _>(|c| {
                            side * (if r == c { 1.0 } else { 0.0 } + t * h * e[c][r])
                        })
                    )
                })
                .collect();
            let cell = format!("cell_bohr = [{}]", rows.join(", "));
            lda(&at, &extra, |text| text.replace(cube, &cell))
        };
        let (plus, minus) = (strained(1.0), strained(-1.0));
        assert_eq!(plus.basis_sizes(), minus.basis_sizes());
        let rise = plus.scf().unwrap().energies.free_ha() - minus.scf().unwrap().energies.free_ha();
        let with = format!("{extra}\n[outputs]\nstress = true\n");
        let sigma = stress(&lda(&at, &with, |t| t));
        let along: f64 = (0..3)
            .flat_map(|i| (0..3).map(move |j| (i, j)))
            .map(|(i, j)| sigma[i][j] * e[i][j])
            .sum();
        let volume = side.powi(3);
        let slope = rise / (2.0 * h * volume);
        assert!(along.abs() > 1e-4, "{along}");
        assert!((along - slope).abs() < 1e-9, "{spin}: {along} vs {slope}");
    }
}

#[test]
fn stress_on_the_irreducible_k_points_equals_that_of_the_full_mesh() {
    let atoms = [[0.1, 0.2, 0.0], [-0.1, -0.2, 0.0], [0.0, 0.0, 0.4]];
    let on_mesh = |symmetry: bool| {
        let mesh = format!("mesh = [3, 3, 3]\nsymmetry = {symmetry}");
        let outcome = lda(&atoms, "\n[outputs]\nstress = true\n", |t| {
            t.replace("list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]", &mesh)
        });
        (outcome.kpoints().points().len(), stress(&outcome))
    };
    let ((solved, reduced), (all, full)) = (on_mesh(true), on_mesh(false));
    assert!(solved < all && all == 27, "{solved} of {all}");
    for i in 0..3 {
        for j in 0..3 {
            assert!(
                (reduced[i][j] - full[i][j]).abs() < 1e-10,
                "{reduced:?} vs {full:?}"
            );
        }
    }
    assert!(full[0][1].abs() > 1e-6, "{full:?}");
}

fn first_step(text: &str) -> f64 {
    let mut first = None;
    let text = format!("{text}\n[outputs]\nstep_energies = true\n");
    run(&read_spec(&text).unwrap(), synthetic, |step, free| {
        if step == 1 {
            first = Some(free);
        }
    })
    .unwrap();
    first.unwrap()
}

const PAIR: [[f64; 3]; 2] = [[0.1, 0.2, 0.3], [0.6, 0.7, 0.8]];

fn turned(text: &str) -> String {
    let axis = [1.0, 2.0, 3.0].map(|x: f64| x / 14f64.sqrt());
    let (c, s) = (0.7f64.cos(), 0.7f64.sin());
    let r: [[f64; 3]; 3] = std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let cross = match (i, j) {
                (0, 1) => -axis[2],
                (0, 2) => axis[1],
                (1, 0) => axis[2],
                (1, 2) => -axis[0],
                (2, 0) => -axis[1],
                (2, 1) => axis[0],
                _ => 0.0,
            };
            c * f64::from(u8::from(i == j)) + s * cross + (1.0 - c) * axis[i] * axis[j]
        })
    });
    let rows: Vec<String> = (0..3)
        .map(|a| format!("{:?}", std::array::from_fn::<f64, 3, _>(|i| 6.0 * r[i][a])))
        .collect();
    let turned = text.replace(
        "cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]",
        &format!("cell_bohr = [{}]", rows.join(", ")),
    );
    assert_ne!(text, turned);
    turned
}

#[test]
fn a_rotated_crystal_starts_from_the_same_first_step() {
    let text = input(&PAIR, "");
    let (here, there) = (first_step(&text), first_step(&turned(&text)));
    assert!((here - there).abs() < 1e-10, "{here} vs {there}");
}

#[test]
fn a_rotated_r2scan_crystal_keeps_its_free_energy_and_eigenvalues() {
    let text = input(&PAIR, "\n[xc]\nfunctional = \"r2scan\"\n");
    let solve = |text: &str| run(&read_spec(text).unwrap(), synthetic_meta, |_, _| {}).unwrap();
    let (here, there) = (solve(&text), solve(&turned(&text)));
    let free = |o: &Outcome| o.scf().unwrap().energies.free_ha();
    let bound = 1e-10 + 1e-12 * free(&here).abs();
    assert!(
        (free(&here) - free(&there)).abs() <= bound,
        "{} vs {}",
        free(&here),
        free(&there)
    );
    let rows = |o: &Outcome| {
        o.eigenvalues_ha()
            .iter()
            .flatten()
            .flatten()
            .copied()
            .collect::<Vec<f64>>()
    };
    let (a, b) = (rows(&here), rows(&there));
    let largest = a.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    for (x, y) in a.iter().zip(&b) {
        assert!((x - y).abs() <= 1e-10 + 1e-12 * largest, "{x} vs {y}");
    }
}

#[test]
fn a_translated_crystal_starts_from_the_same_first_step() {
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]).unwrap();
    let coarse = Grid::coarse(&cell, 3.0);
    let fine = Grid::fine(6.0, &coarse);
    let step: [f64; 3] = std::array::from_fn(|i| {
        let (mut a, mut b) = (coarse.dims()[i], fine.dims()[i]);
        while b != 0 {
            (a, b) = (b, a % b);
        }
        1.0 / a as f64
    });
    let moved = PAIR.map(|p| std::array::from_fn(|i| p[i] + step[i]));
    let (here, there) = (
        first_step(&input(&PAIR, "")),
        first_step(&input(&moved, "")),
    );
    assert!((here - there).abs() < 1e-10, "{here} vs {there}");
}

#[test]
fn the_real_form_starts_where_the_complex_form_starts_and_reaches_its_ground_state() {
    let at = |k: &str| {
        input(&PAIR, "").replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            &format!("list = [{k}]"),
        )
    };
    let (real, complex) = (at("[0.0, 0.0, 0.0]"), at("[1e-9, 0.0, 0.0]"));
    let (first, other) = (first_step(&real), first_step(&complex));
    assert!((first - other).abs() < 1e-6, "{first} vs {other}");
    let free = |text: &str| solve(text).unwrap().scf().unwrap().energies.free_ha();
    let (real, complex) = (free(&real), free(&complex));
    assert!((real - complex).abs() < 1e-10, "{real} vs {complex}");
}

fn rodrigues(axis: [f64; 3], angle: f64) -> [[f64; 3]; 3] {
    let n = axis.iter().map(|x| x * x).sum::<f64>().sqrt();
    let u = axis.map(|x| x / n);
    let (c, s) = (angle.cos(), angle.sin());
    std::array::from_fn(|i| {
        std::array::from_fn(|j| {
            let cross = match (i, j) {
                (0, 1) => -u[2],
                (0, 2) => u[1],
                (1, 0) => u[2],
                (1, 2) => -u[0],
                (2, 0) => -u[1],
                (2, 1) => u[0],
                _ => 0.0,
            };
            c * f64::from(u8::from(i == j)) + s * cross + (1.0 - c) * u[i] * u[j]
        })
    })
}

const CUBE: &str = "cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]";

fn strained(text: &str, e: [[f64; 3]; 3], t: f64) -> String {
    let rows: Vec<String> = (0..3)
        .map(|r| {
            let row: [f64; 3] =
                std::array::from_fn(|c| 6.0 * (f64::from(u8::from(r == c)) + t * e[c][r]));
            format!("{row:?}")
        })
        .collect();
    text.replace(CUBE, &format!("cell_bohr = [{}]", rows.join(", ")))
}

fn moved(positions: &[[f64; 3]], direction: &[[f64; 3]], t: f64) -> Vec<[f64; 3]> {
    (positions.iter().zip(direction))
        .map(|(p, d)| std::array::from_fn(|i| p[i] + t * d[i] / 6.0))
        .collect()
}

fn solved(text: &str, dataset: &impl Fn(u8) -> Result<Dataset, Failure>) -> Outcome {
    run(&read_spec(text).unwrap(), dataset, |_, _| {}).unwrap()
}

fn diagnosed(
    text: &str,
    dataset: &impl Fn(u8) -> Result<Dataset, Failure>,
) -> (Outcome, Diagnosis) {
    let text = if text.contains("discretization = true") {
        text.to_owned()
    } else if text.contains("[outputs]") {
        text.replace("[outputs]", "[outputs]\ndiscretization = true")
    } else {
        format!("{text}\n[outputs]\ndiscretization = true\n")
    };
    let (outcome, diagnosis) =
        run_diagnosed(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
    (outcome, diagnosis.unwrap())
}

struct Slopes {
    analytic: f64,
    difference: f64,
    predicted: f64,
    frame: f64,
    grid: GridGap,
}

impl Slopes {
    fn of(runs: &[(Outcome, Diagnosis); 3], h: f64, analytic: f64) -> Self {
        let [(c, centre), (p, plus), (m, minus)] = runs;
        assert!(
            p.discretization() == c.discretization() && m.discretization() == c.discretization(),
            "the three runs of a path share their operations, grids, and plane waves"
        );
        assert!(
            !c.discretization().unwrap().nyquist,
            "no retained Nyquist coefficient"
        );
        let free = |o: &Outcome| o.scf().unwrap().energies.free_ha();
        let frame = centre.sphere_energy_ha(plus) - centre.sphere_energy_ha(minus);
        let spheres = plus.sphere_energy_ha(plus) - minus.sphere_energy_ha(minus) - frame;
        let contraction = centre.sphere_contraction_ha(plus).unwrap()
            - centre.sphere_contraction_ha(minus).unwrap();
        let mut grid = centre.grid_gap(plus, minus).unwrap();
        grid.bound_ha /= 2.0 * h;
        grid.predicted_ha /= 2.0 * h;
        Slopes {
            analytic,
            difference: (free(p) - free(m)) / (2.0 * h),
            predicted: (spheres - contraction) / (2.0 * h),
            frame: frame / (2.0 * h),
            grid,
        }
    }

    fn per(self, scale: f64) -> Self {
        Slopes {
            analytic: self.analytic / scale,
            difference: self.difference / scale,
            predicted: self.predicted / scale,
            frame: self.frame / scale,
            grid: GridGap {
                bound_ha: self.grid.bound_ha / scale,
                predicted_ha: self.grid.predicted_ha / scale,
                ..self.grid
            },
        }
    }

    fn residual(&self) -> f64 {
        self.difference - self.analytic - self.predicted - self.grid.predicted_ha
    }

    fn check(&self, name: &str, spheres: f64, grid: f64, rest: f64) {
        assert!(
            self.predicted.abs() <= spheres,
            "{name}: spheres' gap {}",
            self.predicted
        );
        let g = self.grid;
        assert!(
            g.predicted_ha.abs() <= grid && g.predicted_ha.abs() <= g.bound_ha,
            "{name}: grid {g:?}"
        );
        assert!(
            self.residual().abs() <= rest,
            "{name}: residual {}",
            self.residual()
        );
    }
}

fn force_slopes(
    text_at: impl Fn(&[[f64; 3]]) -> String,
    at: &[[f64; 3]],
    direction: &[[f64; 3]],
    dataset: &impl Fn(u8) -> Result<Dataset, Failure>,
) -> Slopes {
    let h = 1e-4;
    let runs = [
        diagnosed(
            &format!("{}\n[outputs]\nforces = true\n", text_at(at)),
            dataset,
        ),
        diagnosed(&text_at(&moved(at, direction, h)), dataset),
        diagnosed(&text_at(&moved(at, direction, -h)), dataset),
    ];
    let along: f64 = (forces(&runs[0].0).iter().zip(direction))
        .map(|(f, d)| (0..3).map(|i| f[i] * d[i]).sum::<f64>())
        .sum();
    Slopes::of(&runs, h, -along)
}

fn stress_slopes(
    text: &str,
    e: [[f64; 3]; 3],
    dataset: &impl Fn(u8) -> Result<Dataset, Failure>,
) -> Slopes {
    let h = 1e-5;
    let runs = [
        diagnosed(&format!("{text}\n[outputs]\nstress = true\n"), dataset),
        diagnosed(&strained(text, e, h), dataset),
        diagnosed(&strained(text, e, -h), dataset),
    ];
    let sigma = stress(&runs[0].0);
    let along: f64 = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| sigma[i][j] * e[i][j])
        .sum();
    Slopes::of(&runs, h, 216.0 * along).per(216.0)
}

const TRACEFUL: [[f64; 3]; 3] = [[0.7, 0.5, -0.2], [0.5, 0.0, 0.6], [-0.2, 0.6, 0.5]];

const GENERAL: [[f64; 3]; 3] = [[0.3, 0.5, -0.2], [0.5, -0.4, 0.6], [-0.2, 0.6, 0.1]];

#[test]
fn r2scan_forces_and_stress_are_the_free_energy_derivatives_with_and_without_spin() {
    let at = [[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]];
    let direction = [[0.3, -0.5, 0.2], [-0.4, 0.1, 0.6]];
    for spin in ["", "\n[spin]\nmoments = [1.5, -1.0]\n"] {
        let extra = format!(
            "\n[xc]\nfunctional = \"r2scan\"\n{spin}\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n"
        );
        let f = force_slopes(|p| input(p, &extra), &at, &direction, &synthetic_meta);
        let s = stress_slopes(&input(&at, &extra), GENERAL, &synthetic_meta);
        assert!(
            f.analytic.abs() > 0.05 && s.analytic.abs() > 1e-4,
            "{} {}",
            f.analytic,
            s.analytic
        );
        f.check("forces", 5e-7, 0.0, 2e-9);
        s.check("stress", 5e-8, 0.0, 5e-10);
    }
}

fn carbon_and_coreless_silicon(
    carbon_core: bool,
    scale: f64,
) -> impl Fn(u8) -> Result<Dataset, Failure> {
    move |z: u8| {
        let mut d = synthetic_meta_draft();
        if !carbon_core {
            d.pseudo_core_density.fill(0.0);
        }
        if z == 6 {
            return Ok(Dataset::try_from(d).unwrap());
        }
        assert_eq!(z, 14, "only carbon and silicon are asked for");
        let mut d = DatasetDraft {
            z: 14.0,
            core_electrons: 10.0,
            core_density: d.core_density.iter().map(|n| 5.0 * n).collect(),
            ..d
        };
        d.pseudo_core_density.fill(0.0);
        let c = 0.3 * (3.0 * std::f64::consts::PI.powi(2)).powf(2.0 / 3.0);
        let ae = (d.core_density.iter().zip(d.grid.radii()))
            .map(|(n, r)| c * n.max(0.0).powf(5.0 / 3.0) + 16.0 * r * r * n)
            .collect();
        let k = d.core_kinetic.take().unwrap();
        let pseudo = k.pseudo().iter().map(|t| t * scale).collect();
        d.core_kinetic = Some(CoreKinetic::checked(&d.grid, ae, pseudo).unwrap());
        Ok(Dataset::try_from(d).unwrap())
    }
}

#[test]
fn r2scan_forces_and_stress_take_the_pseudo_core_tau_only_where_the_grid_holds_it() {
    let at = [[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]];
    let direction = [[0.3, -0.5, 0.2], [-0.4, 0.1, 0.6]];
    let text = |p: &[[f64; 3]]| {
        input(
            p,
            "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n",
        )
        .replace("species = [\"C\", \"C\"]", "species = [\"C\", \"Si\"]")
    };
    assert!(text(&at).contains("\"Si\""));
    for carbon_core in [true, false] {
        let dataset = carbon_and_coreless_silicon(carbon_core, 1.0);
        let f = force_slopes(text, &at, &direction, &dataset);
        let s = stress_slopes(&text(&at), GENERAL, &dataset);
        let outputs = |scale: f64| {
            let with = format!("{}\n[outputs]\nforces = true\nstress = true\n", text(&at));
            let dataset = carbon_and_coreless_silicon(carbon_core, scale);
            let outcome = run(&read_spec(&with).unwrap(), dataset, |_, _| {}).unwrap();
            (forces(&outcome)[1], stress(&outcome))
        };
        let ((f1, s1), (f2, s2)) = (outputs(1.0), outputs(2.0));
        let moved = (0..3).map(|i| (f1[i] - f2[i]).abs()).fold(0.0, f64::max);
        let strained = s1
            .iter()
            .flatten()
            .zip(s2.iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0, f64::max);
        let polarized = text(&at).replace("[scf]", "[spin]\nmoments = [1.5, -1.0]\n\n[scf]");
        let v = stress_slopes(&polarized, TRACEFUL, &dataset);
        f.check("forces", 5e-6, 0.0, 5e-9);
        s.check("stress", 1e-6, 0.0, 2e-9);
        v.check("polarized volume", 1e-6, 0.0, 2e-9);
        if carbon_core {
            assert!(moved > 1e-5 && strained > 1e-7, "{moved} {strained}");
        } else {
            assert!(moved == 0.0 && strained == 0.0, "{moved} {strained}");
        }
    }
}

fn iron_pair(extra: &str) -> String {
    input(
        &[[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]],
        &format!(
            "\n[xc]\nfunctional = \"r2scan\"\n\n[spin]\nmoments = [2.0, 2.0]\n\n[scf]\n\
             energy_tolerance_ha = 1e-12\nmax_iterations = 200\n{extra}"
        ),
    )
    .replace("species = [\"C\", \"C\"]", "species = [\"Fe\", \"Fe\"]")
    .replace("cutoff_ha = 3.0", "cutoff_ha = 4.0")
    .replace("width_ha = 0.002", "width_ha = 0.01")
}

fn synthetic_iron(z: u8) -> Result<Dataset, Failure> {
    assert_eq!(z, 26, "only the synthetic iron is asked for");
    Ok(Dataset::try_from(synthetic_3d_draft()).unwrap())
}

#[test]
fn r2scan_stress_holds_the_turn_of_the_spheres_upright_frame() {
    let diagonal = [[0.6, 0.0, 0.0], [0.0, -0.3, 0.0], [0.0, 0.0, 0.2]];
    let turned = stress_slopes(&iron_pair(""), GENERAL, &synthetic_iron);
    let still = stress_slopes(&iron_pair(""), diagonal, &synthetic_iron);
    turned.check("general", 2e-5, 5e-9, 1e-8);
    still.check("diagonal", 3e-4, 5e-10, 5e-9);
    assert!(
        turned.frame != 0.0 && still.frame == 0.0,
        "{} {}",
        turned.frame,
        still.frame
    );
    let r = rodrigues([1.0, 2.0, 3.0], 0.7);
    let rows: Vec<String> = (0..3)
        .map(|a| format!("{:?}", std::array::from_fn::<f64, 3, _>(|i| 6.0 * r[i][a])))
        .collect();
    let with = iron_pair("\n[outputs]\nstress = true\n");
    let sigma = stress(&solved(&with, &synthetic_iron));
    let turned_sigma = stress(&solved(
        &with.replace(CUBE, &format!("cell_bohr = [{}]", rows.join(", "))),
        &synthetic_iron,
    ));
    for i in 0..3 {
        for j in 0..3 {
            let want: f64 = (0..3)
                .flat_map(|k| (0..3).map(move |l| (k, l)))
                .map(|(k, l)| r[i][k] * sigma[k][l] * r[j][l])
                .sum();
            assert!(
                (turned_sigma[i][j] - want).abs() < 1e-9,
                "[{i}][{j}] {} vs {want}",
                turned_sigma[i][j]
            );
        }
    }
}

#[test]
fn r2scan_forces_and_stress_on_the_irreducible_k_points_equal_those_of_the_full_mesh() {
    let atoms = [[0.1, 0.2, 0.0], [-0.1, -0.2, 0.0], [0.0, 0.0, 0.4]];
    let on_mesh = |symmetry: bool| {
        let mesh = format!("mesh = [3, 3, 3]\nsymmetry = {symmetry}");
        let text = input(
            &atoms,
            "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n\n\
             [outputs]\nforces = true\nstress = true\n",
        )
        .replace("list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]", &mesh);
        let (outcome, _) = diagnosed(&text, &synthetic_meta);
        let operations = outcome.discretization().unwrap().operations.clone();
        (
            outcome.kpoints().points().len(),
            forces(&outcome),
            stress(&outcome),
            operations,
        )
    };
    let ((solved_points, reduced, reduced_stress, kept), (all, full, full_stress, images)) =
        (on_mesh(true), on_mesh(false));
    assert!(solved_points == 10 && all == 27, "{solved_points} of {all}");
    let c2 = [[-1, 0, 0], [0, -1, 0], [0, 0, 1]];
    assert!(
        kept == images && kept.len() == 2 && kept.iter().any(|(r, _)| *r == c2),
        "{kept:?}"
    );
    let apart = (reduced.iter().flatten().zip(full.iter().flatten()))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    let stress_apart = (reduced_stress
        .iter()
        .flatten()
        .zip(full_stress.iter().flatten()))
    .map(|(a, b)| (a - b).abs())
    .fold(0.0, f64::max);
    assert!(
        apart < 1e-9 && stress_apart < 1e-10,
        "{apart} {stress_apart}"
    );
    assert!(full[0].iter().all(|f| f.abs() > 1e-3) && full_stress[0][1].abs() > 1e-6);
}

#[test]
fn the_discretization_record_tells_apart_operations_and_plane_wave_sets_that_equal_outputs_hide() {
    let lda = |text: &str| {
        let dataset = |_| {
            let mut draft = synthetic_draft();
            draft.functional = orbitar::model::Functional::Lda;
            Ok(Dataset::try_from(draft).unwrap())
        };
        let text =
            format!("{text}\n[xc]\nfunctional = \"lda\"\n\n[outputs]\ndiscretization = true\n")
                .replace(
                    "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
                    "mesh = [1, 1, 1]",
                );
        run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap()
    };
    let (symmetric, moved) = (
        lda(&input(&[[0.1, 0.2, 0.3], [-0.1, -0.2, -0.3]], "")),
        lda(&input(&[[0.1, 0.2, 0.3], [-0.1, -0.2, -0.31]], "")),
    );
    assert_eq!(symmetric.kpoints(), moved.kpoints());
    let operations = |o: &Outcome| o.discretization().unwrap().operations.len();
    assert!(
        operations(&symmetric) > operations(&moved),
        "{} {}",
        operations(&symmetric),
        operations(&moved)
    );
    let stretched = |row: &str| {
        let text = input(&ONE, "").replace("cutoff_ha = 3.0", "cutoff_ha = 2.19");
        lda(&text.replace(CUBE, row))
    };
    let (x, y) = (
        stretched("cell_bohr = [[6.06, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]"),
        stretched("cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.06, 0.0], [0.0, 0.0, 6.0]]"),
    );
    assert_eq!(x.basis_sizes(), y.basis_sizes());
    let waves = |o: &Outcome| o.discretization().unwrap().plane_waves;
    assert_ne!(waves(&x), waves(&y));
}

fn fnv1a(bytes: impl Iterator<Item = u8>) -> u64 {
    bytes.fold(0xcbf2_9ce4_8422_2325, |h, b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    })
}

#[test]
fn the_discretization_signatures_are_fnv_1a_of_the_ordered_miller_indices_and_name_a_kept_nyquist_plane()
 {
    assert_eq!(fnv1a("".bytes()), 0xcbf2_9ce4_8422_2325);
    assert_eq!(fnv1a("a".bytes()), 0xaf63_dc4c_8601_ec8c);
    let lda = |side: f64, cutoff: f64| {
        let dataset = |_| {
            let mut draft = synthetic_draft();
            draft.functional = orbitar::model::Functional::Lda;
            Ok(Dataset::try_from(draft).unwrap())
        };
        let text = input(
            &[[0.1, 0.2, 0.3]],
            "\n[xc]\nfunctional = \"lda\"\n\n[outputs]\ndiscretization = true\n",
        )
        .replace(
            CUBE,
            &format!(
                "cell_bohr = [[{side:?}, 0.0, 0.0], [0.0, {side:?}, 0.0], [0.0, 0.0, {side:?}]]"
            ),
        )
        .replace("cutoff_ha = 3.0", &format!("cutoff_ha = {cutoff:?}"))
        .replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            "list = [[0.0, 0.0, 0.0]]",
        );
        let spec = read_spec(&text).unwrap();
        let outcome = run(&spec, dataset, |_, _| {}).unwrap();
        (spec, outcome)
    };
    let bytes = |m: &[[i32; 3]]| {
        m.iter()
            .flatten()
            .flat_map(|x| x.to_le_bytes())
            .collect::<Vec<u8>>()
    };
    for (side, cutoff, nyquist) in [(TAU, 2.0, true), (6.0, 3.0, false)] {
        let (spec, outcome) = lda(side, cutoff);
        let cell = &spec.draft().cell;
        let record = outcome.discretization().unwrap();
        let waves = Basis::sphere(cell, [0.0; 3], cutoff);
        assert_eq!(
            record.plane_waves,
            fnv1a(bytes(waves.millers()).into_iter())
        );
        let coarse = Grid::coarse(cell, cutoff);
        let fine = Grid::fine(spec.density_cutoff_ha(), &coarse);
        let sphere = |g: &Grid, c: f64| -> Vec<[i32; 3]> {
            let millers = g.millers();
            g.density_sphere(c)
                .iter()
                .map(|&(slot, _)| millers[slot])
                .collect()
        };
        let spheres = [
            sphere(&coarse, cutoff),
            sphere(&fine, spec.density_cutoff_ha()),
        ];
        assert_eq!(
            record.density_spheres,
            spheres.each_ref().map(|s| fnv1a(bytes(s).into_iter()))
        );
        assert_eq!(
            (record.coarse_grid, record.nyquist),
            (coarse.dims(), nyquist),
            "{side}"
        );
    }
}

#[test]
fn a_run_below_the_meta_gga_rung_without_reduction_accepts_no_operation() {
    let lda = |symmetry: bool| {
        let dataset = |_| {
            let mut draft = synthetic_draft();
            draft.functional = orbitar::model::Functional::Lda;
            Ok(Dataset::try_from(draft).unwrap())
        };
        let text = input(
            &[[0.0, 0.0, 0.0]],
            "\n[xc]\nfunctional = \"lda\"\n\n[outputs]\ndiscretization = true\n",
        )
        .replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            &format!("mesh = [2, 2, 2]\nsymmetry = {symmetry}"),
        );
        let outcome = run(&read_spec(&text).unwrap(), dataset, |_, _| {}).unwrap();
        outcome.discretization().unwrap().operations.len()
    };
    assert_eq!((lda(false), lda(true)), (0, 48));
}

#[test]
fn r2scan_stress_is_refused_where_an_operation_holds_only_to_the_finders_tolerance() {
    let text = |delta: f64, turn: bool, symmetry: bool, outputs: &str| {
        let r = if turn {
            rodrigues([1.0, 2.0, 3.0], 5.0)
        } else {
            rodrigues([0.0, 0.0, 1.0], 0.0)
        };
        let a = [
            [6.0, 0.0, 0.0],
            [0.0, 6.0 * (1.0 + delta), 0.0],
            [0.0, 0.0, 6.0],
        ];
        let rows: Vec<String> = (a.iter())
            .map(|v| {
                format!(
                    "{:?}",
                    std::array::from_fn::<f64, 3, _>(|i| (0..3).map(|k| r[i][k] * v[k]).sum())
                )
            })
            .collect();
        input(
            &[[0.0, 0.0, 0.0]],
            &format!(
                "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nmax_iterations = 1\n\n[outputs]\n{outputs}\n"
            ),
        )
        .replace(CUBE, &format!("cell_bohr = [{}]", rows.join(", ")))
        .replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            &format!("mesh = [1, 1, 1]\nsymmetry = {symmetry}"),
        )
    };
    let result = |delta, turn, symmetry, outputs| {
        run(
            &read_spec(&text(delta, turn, symmetry, outputs)).unwrap(),
            synthetic_meta,
            |_, _| {},
        )
    };
    for (turn, symmetry) in [(false, true), (true, true), (false, false), (true, false)] {
        let refused = result(5e-11, turn, symmetry, "stress = true");
        assert!(
            matches!(&refused, Err(Failure::Unsupported(m)) if m.starts_with("outputs.stress: an operation")),
            "{turn} {symmetry}: {refused:?}"
        );
        for (delta, outputs) in [(2.5e-11, "stress = true"), (5e-11, "forces = true")] {
            let ran = result(delta, turn, symmetry, outputs);
            assert!(
                matches!(ran, Err(Failure::Unconverged(_))),
                "{delta} {turn} {symmetry} {outputs}: {ran:?}"
            );
        }
    }
}

fn clamped_carbon(z: u8) -> Result<Dataset, Failure> {
    assert_eq!(z, 6, "only carbon is asked for");
    let mut d = synthetic_meta_draft();
    let c = 0.3 * (3.0 * std::f64::consts::PI.powi(2)).powf(2.0 / 3.0);
    let k = d.core_kinetic.take().unwrap();
    let pseudo = d
        .pseudo_core_density
        .iter()
        .map(|n| c * n.max(0.0).powf(5.0 / 3.0))
        .collect();
    d.core_kinetic = Some(CoreKinetic::checked(&d.grid, k.ae().to_vec(), pseudo).unwrap());
    Ok(Dataset::try_from(d).unwrap())
}

#[test]
fn where_the_grids_cap_binds_the_force_gap_stays_within_its_bound() {
    let at = [[0.1, 0.2, 0.3], [0.62, 0.55, 0.83]];
    let direction = [[0.3, -0.5, 0.2], [-0.4, 0.1, 0.6]];
    let extra = "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 200\n";
    let f = force_slopes(|p| input(p, extra), &at, &direction, &clamped_carbon);
    assert!(f.grid.points > 0 && f.grid.bound_ha > 0.0, "{:?}", f.grid);
    f.check("clamped forces", 5e-7, 5e-5, 3e-9);
}

struct Row {
    stem: &'static str,
    centre: &'static str,
    cell: bool,
}

#[test]
#[ignore = "hours of CPU; reads JTH files from $ORBITAR_DATASETS"]
#[expect(
    clippy::disallowed_methods,
    clippy::disallowed_macros,
    reason = "the measurement reads case 22's locked inputs and the JTH files the harness fetched"
)]
fn case_22_rows_meet_their_bounds_after_their_signed_gaps() {
    let root =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("benchmarks/22_r2scan_derivatives");
    let data =
        std::path::PathBuf::from(std::env::var("ORBITAR_DATASETS").expect("ORBITAR_DATASETS"));
    let dataset = |z: u8| {
        let name = match z {
            31 => "Ga",
            33 => "As",
            26 => "Fe",
            _ => panic!("case 22 holds Ga, As, Fe; not Z = {z}"),
        };
        let file = data.join(format!("JTH-PBE-v2.0/{name}/{name}.GGA_PBE-JTH.xml"));
        let text = std::fs::read_to_string(file).unwrap();
        Ok(orbitar::io::read_dataset_for(&text, orbitar::model::Functional::R2scan).unwrap())
    };
    let read = |name: &str| std::fs::read_to_string(root.join(format!("{name}.toml"))).unwrap();
    let bounds: toml::Value = toml::from_str(&read("case")).unwrap();
    let rows = [
        (
            Row {
                stem: "gaas",
                centre: "input",
                cell: false,
            },
            "gaas_force_difference_ha_bohr_max",
        ),
        (
            Row {
                stem: "gaas-strain",
                centre: "input",
                cell: true,
            },
            "gaas_stress_difference_ha_bohr3_max",
        ),
        (
            Row {
                stem: "gaas-volume",
                centre: "input",
                cell: true,
            },
            "gaas_volume_stress_difference_ha_bohr3_max",
        ),
        (
            Row {
                stem: "fe-strain",
                centre: "inputs/fe",
                cell: true,
            },
            "fe_stress_difference_ha_bohr3_max",
        ),
    ];
    for (row, bound) in rows {
        let bound = bounds["tolerance"][bound].as_float().unwrap();
        for (setting, stop, half) in [
            ("case", "1e-10", false),
            ("stop 1e-12", "1e-12", false),
            ("half step", "1e-10", true),
        ] {
            let tune = |t: String| {
                t.replace(
                    "energy_tolerance_ha = 1e-10",
                    &format!("energy_tolerance_ha = {stop}"),
                )
            };
            let centre = tune(read(row.centre));
            let end = |side: &str| {
                let end = tune(read(&format!("inputs/{}-{side}", row.stem)));
                if half {
                    midway(&centre, &end, row.cell)
                } else {
                    end
                }
            };
            let runs = [centre.clone(), end("plus"), end("minus")].map(|t| diagnosed(&t, &dataset));
            let specs = [&centre, &end("plus"), &end("minus")].map(|t| read_spec(t).unwrap());
            let slopes = if row.cell {
                let rows_of = |s: &orbitar::model::Spec| -> [[f64; 3]; 3] {
                    std::array::from_fn(|r| {
                        s.draft()
                            .cell
                            .cartesian(std::array::from_fn(|i| f64::from(u8::from(i == r))))
                    })
                };
                let (a, p, m) = (rows_of(&specs[0]), rows_of(&specs[1]), rows_of(&specs[2]));
                let inverse = invert(a);
                let delta: [[f64; 3]; 3] = std::array::from_fn(|i| {
                    std::array::from_fn(|j| {
                        (0..3).map(|k| (p[k][i] - m[k][i]) * inverse[j][k]).sum()
                    })
                });
                let h = delta.iter().flatten().map(|x| x * x).sum::<f64>().sqrt() / 2.0;
                let sigma = stress(&runs[0].0);
                let volume = specs[0].draft().cell.volume();
                let work: f64 = (0..3)
                    .flat_map(|i| (0..3).map(move |j| (i, j)))
                    .map(|(i, j)| sigma[i][j] * delta[i][j])
                    .sum::<f64>()
                    * volume;
                Slopes::of(&runs, h, work / (2.0 * h)).per(volume)
            } else {
                let cell = specs[0].draft().cell;
                let moves: Vec<[f64; 3]> =
                    (specs[1].draft().atoms.iter().zip(&specs[2].draft().atoms))
                        .map(|(a, b)| cell.cartesian(std::array::from_fn(|i| a.1[i] - b.1[i])))
                        .collect();
                let h = moves.iter().flatten().map(|x| x * x).sum::<f64>().sqrt() / 2.0;
                let work: f64 = (forces(&runs[0].0).iter().zip(&moves))
                    .map(|(f, r)| (0..3).map(|i| f[i] * r[i]).sum::<f64>())
                    .sum();
                Slopes::of(&runs, h, -work / (2.0 * h))
            };
            let free: Vec<f64> = runs
                .iter()
                .map(|(o, _)| o.scf().unwrap().energies.free_ha())
                .collect();
            let moments: Vec<Option<f64>> = runs
                .iter()
                .map(|(o, _)| o.scf().unwrap().magnetization_bohr_magneton)
                .collect();
            eprintln!(
                "case 22 {} ({setting}): FD − analytic {:.3e}, spheres {:.3e}, frame {:.3e}, grid {:?}, left {:.3e}; \
                 F {free:?}; moments {moments:?}",
                row.stem,
                slopes.difference - slopes.analytic,
                slopes.predicted,
                slopes.frame,
                slopes.grid,
                slopes.residual(),
            );
            if setting == "case" {
                assert!(
                    (slopes.difference - slopes.analytic).abs() <= bound,
                    "{}",
                    row.stem
                );
                assert!(slopes.residual().abs() <= bound, "{}", row.stem);
            }
        }
    }
}

fn midway(centre: &str, end: &str, cell: bool) -> String {
    let key = if cell {
        "cell_bohr = ["
    } else {
        "positions_fractional = ["
    };
    let block = |t: &str| {
        let i = t.find(key).unwrap();
        (i, i + t[i..].find("]]").unwrap() + 2)
    };
    let numbers = |t: &str| -> Vec<f64> {
        let (i, j) = block(t);
        (t[i + key.len()..j]
            .replace(['[', ']', ','], " ")
            .split_whitespace())
        .map(|x| x.parse().unwrap())
        .collect()
    };
    let mid: Vec<String> = (numbers(centre).iter().zip(numbers(end)))
        .map(|(a, b)| format!("{:?}", 0.5 * (a + b)))
        .collect();
    let rows: Vec<String> = mid
        .chunks(3)
        .map(|r| format!("[{}]", r.join(", ")))
        .collect();
    let (i, j) = block(end);
    format!("{}{key}{}]{}", &end[..i], rows.join(", "), &end[j..])
}

fn invert(a: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let c = |r: usize, k: usize| {
        let (r0, r1, k0, k1) = ((r + 1) % 3, (r + 2) % 3, (k + 1) % 3, (k + 2) % 3);
        a[r0][k0] * a[r1][k1] - a[r0][k1] * a[r1][k0]
    };
    let det: f64 = (0..3).map(|k| a[0][k] * c(0, k)).sum();
    std::array::from_fn(|i| std::array::from_fn(|j| c(j, i) / det))
}

fn strained_rows(text: &str, rows: [[f64; 3]; 3], e: [[f64; 3]; 3], t: f64) -> String {
    let line = |a: [[f64; 3]; 3]| {
        let rows: Vec<String> = a.iter().map(|r| format!("{r:?}")).collect();
        format!("cell_bohr = [{}]", rows.join(", "))
    };
    let turned: [[f64; 3]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            (0..3)
                .map(|m| (f64::from(u8::from(c == m)) + t * e[c][m]) * rows[r][m])
                .sum()
        })
    });
    assert!(text.contains(&line(rows)), "the text holds the rows");
    text.replace(&line(rows), &line(turned))
}

#[test]
fn r2scan_stress_holds_the_frame_turn_of_one_anisotropic_sphere() {
    let rows = [[5.0, 0.3, 0.2], [0.4, 5.5, -0.3], [0.2, -0.5, 6.5]];
    let cell = format!(
        "cell_bohr = [{}]",
        rows.iter()
            .map(|r| format!("{r:?}"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let text = format!(
        "schema = 1\n\n[structure]\n{cell}\nspecies = [\"Fe\"]\npositions_fractional = [[0.0, 0.0, 0.0]]\n\n\
         [basis]\ncutoff_ha = 4.0\n\n[kpoints]\nlist = [[0.0, 0.0, 0.0]]\n\n[spin]\nmoments = [2.0]\n\n\
         [occupations]\nsmearing = \"fermi-dirac\"\nwidth_ha = 0.005\n\n[xc]\nfunctional = \"r2scan\"\n\n\
         [scf]\nenergy_tolerance_ha = 1e-12\nmax_iterations = 300\n"
    );
    let h = 2.5e-6;
    let runs = [
        diagnosed(
            &format!("{text}\n[outputs]\nstress = true\n"),
            &synthetic_iron,
        ),
        diagnosed(&strained_rows(&text, rows, GENERAL, h), &synthetic_iron),
        diagnosed(&strained_rows(&text, rows, GENERAL, -h), &synthetic_iron),
    ];
    let sigma = stress(&runs[0].0);
    let volume = read_spec(&text).unwrap().draft().cell.volume();
    let along: f64 = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| sigma[i][j] * GENERAL[i][j])
        .sum();
    let s = Slopes::of(&runs, h, volume * along).per(volume);
    assert!(s.frame.abs() > 3e-9, "{}", s.frame);
    s.check("one anisotropic sphere", 3e-4, 1e-10, 1e-9);
}

fn skew(rows: [[f64; 3]; 3]) -> f64 {
    let cell = Cell::try_from(rows).unwrap();
    let operations = orbitar::model::operations(&cell, &[(6, [0.0; 3])], None).unwrap();
    assert_eq!(operations.len(), 48);
    (operations.iter())
        .map(|o| {
            let w = o.cartesian;
            let squares: f64 = (0..3)
                .flat_map(|i| (0..3).map(move |j| (i, j)))
                .map(|(i, j)| {
                    let product: f64 = (0..3).map(|k| w[k][i] * w[k][j]).sum();
                    (product - f64::from(u8::from(i == j))).powi(2)
                })
                .sum();
            squares.sqrt()
        })
        .fold(0.0, f64::max)
}

#[test]
fn r2scan_stress_runs_at_a_skew_equal_to_the_bound_and_is_refused_just_above_it() {
    const DRAWS: usize = 64;
    let bound = 1e-10_f64;
    let rows = |e: f64, f: f64| [[6.0, 0.0, 0.0], [e, 6.0, 0.0], [0.0, f, 6.0]];
    let ulps = |e: f64, f: f64| skew(rows(e, f)).to_bits() as i64 - bound.to_bits() as i64;
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut uniform = || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state >> 11) as f64 / (1_u64 << 53) as f64
    };
    let found = (0..DRAWS).find_map(|_| {
        let f = 1e-10 * uniform();
        let (mut lo, mut hi) = (0.0_f64, 1e-8_f64);
        for _ in 0..200 {
            let mid = 0.5 * (lo + hi);
            if mid == lo || mid == hi {
                break;
            }
            if ulps(mid, f) > 0 { hi = mid } else { lo = mid }
        }
        let shears: Vec<f64> = (lo.to_bits() - 8..=lo.to_bits() + 8)
            .map(f64::from_bits)
            .collect();
        let at = shears.iter().position(|&e| ulps(e, f) == 0)?;
        let above = shears[at..].iter().find(|&&e| ulps(e, f) > 0)?;
        Some((f, shears[at], *above))
    });
    let (f, at, above) = found.unwrap_or_else(|| {
        panic!("no sheared cube in {DRAWS} draws has a computed skew of exactly {bound:e} on this platform")
    });
    assert!(ulps(above, f) <= 4, "{} ulps over", ulps(above, f));
    let result = |e: f64, symmetry: bool| {
        let r = rows(e, f);
        let text = input(
            &[[0.0, 0.0, 0.0]],
            "\n[xc]\nfunctional = \"r2scan\"\n\n[scf]\nmax_iterations = 1\n\n[outputs]\nstress = true\n",
        )
        .replace(
            CUBE,
            &format!("cell_bohr = [{:?}, {:?}, {:?}]", r[0], r[1], r[2]),
        )
        .replace(
            "list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]",
            &format!("mesh = [1, 1, 1]\nsymmetry = {symmetry}"),
        );
        run(&read_spec(&text).unwrap(), synthetic_meta, |_, _| {})
    };
    for symmetry in [true, false] {
        let ran = result(at, symmetry);
        assert!(
            matches!(ran, Err(Failure::Unconverged(_))),
            "{symmetry}: {ran:?}"
        );
        let refused = result(above, symmetry);
        assert!(
            matches!(&refused, Err(Failure::Unsupported(m)) if m.starts_with("outputs.stress: an operation")),
            "{symmetry}: {refused:?}"
        );
    }
}
