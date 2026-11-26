use super::scf::{synthetic, synthetic_meta};
use crate::{ROCK_SALT_PROBE, synthetic_meta_draft};
use orbitar::io::read_spec;
use orbitar::ks::{Failure, Probed, paw_probe, probe, run};
use orbitar::model::Functional;
use orbitar::paw::{Dataset, Species, total_occupancies};
use orbitar::pw::Mat;
use orbitar::xc::{Point, energy_density};

const ROCK_SALT_PAIR: f64 = 0.0001756160161455844;

const DIFFUSE_PAIR: f64 = 0.02436335225926142;

fn probed(input: &str) -> Probed {
    probe(&read_spec(input).unwrap()).unwrap().unwrap()
}

fn variant(term: &str, uncomment: &str) -> String {
    ROCK_SALT_PROBE
        .replace("term = \"ewald\"", &format!("term = \"{term}\""))
        .replace(&format!("# {uncomment} = "), &format!("{uncomment} = "))
}

#[test]
fn an_ewald_probe_gives_the_rock_salt_madelung_constant() {
    let Probed::EwaldEnergy(energy) = probed(ROCK_SALT_PROBE) else {
        panic!("an Ewald energy");
    };
    let rock_salt = super::ewald::madelung()[0];
    assert!((energy + rock_salt).abs() < 5e-13, "{energy}");
}

#[test]
fn a_hartree_probe_gives_the_energy_of_its_gaussians() {
    let Probed::HartreeEnergy(energy) = probed(&variant("hartree", "exponents")) else {
        panic!("a Hartree energy");
    };
    assert!(
        (energy - ROCK_SALT_PAIR).abs() < 1e-10 * ROCK_SALT_PAIR,
        "{energy}"
    );
}

#[test]
fn a_diffuse_and_a_sharp_gaussian_off_the_center_give_their_energy() {
    let input = variant("hartree", "exponents")
        .replace(
            "[[0.0, 1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 0.0]]",
            "[[0.0, 3.0, 3.0], [3.0, 0.0, 3.0], [3.0, 3.0, 0.0]]",
        )
        .replace("cutoff_ha = 10.0", "cutoff_ha = 20.0")
        .replace("charges = [1.0, -1.0]", "charges = [1.0, -0.4]")
        .replace("exponents = [1.5, 1.5]", "exponents = [0.3, 1.3]")
        .replace("[[0.0, 0.0, 0.0],", "[[0.1, 0.2, 0.3],")
        .replace("[0.5, 0.5, 0.5]]", "[0.7, 0.45, 0.85]]");
    let Probed::HartreeEnergy(energy) = probed(&input) else {
        panic!("a Hartree energy");
    };
    assert!(
        (energy - DIFFUSE_PAIR).abs() < 1e-10 * DIFFUSE_PAIR,
        "{energy}"
    );
}

#[test]
fn an_xc_probe_gives_the_functional_at_each_point() {
    let input = variant("xc", "points")
        .replace("charges = [1.0, -1.0]", "")
        .replace("positions_fractional = [[0.0, 0.0, 0.0],", "")
        .replace("[0.5, 0.5, 0.5]]", "")
        + "\n[xc]\nfunctional = \"lda\"\n";
    let Probed::XcEnergyDensity(values) = probed(&input) else {
        panic!("xc energy densities");
    };
    let point = Point::of(0.5, 0.5, [0.0; 3]);
    assert_eq!(values, [energy_density(Functional::Lda, point)]);
}

#[test]
fn an_xc_probe_with_tau_gives_r2scan_at_each_point() {
    let input = variant("xc", "points")
        .replace("charges = [1.0, -1.0]", "")
        .replace("positions_fractional = [[0.0, 0.0, 0.0],", "")
        .replace("[0.5, 0.5, 0.5]]", "")
        .replace("0.0, 0.0, 0.0]]", "0.01, 0.0, 0.0, 0.6, 0.5]]")
        + "\n[xc]\nfunctional = \"r2scan\"\n";
    let Probed::XcEnergyDensity(values) = probed(&input) else {
        panic!("xc energy densities");
    };
    let point = Point::of(0.5, 0.5, [0.01, 0.0, 0.0]).with_tau([0.6, 0.5]);
    assert_eq!(values, [energy_density(Functional::R2scan, point)]);
}

#[test]
fn each_probe_has_its_own_entry() {
    let paw = read_spec(&paw_input("pbe", &occupancies())).unwrap();
    assert_eq!(
        probe(&paw),
        Err(Failure::Invalid(
            "probe: a paw probe reads its element's dataset; ks::paw_probe evaluates it".into()
        ))
    );
    let ewald = read_spec(ROCK_SALT_PROBE).unwrap();
    let no_dataset = |_| unreachable!("an Ewald probe reads no dataset");
    assert_eq!(paw_probe(&ewald, no_dataset), Ok(None));
}

#[test]
fn a_probe_input_has_no_bands_to_run() {
    let spec = read_spec(ROCK_SALT_PROBE).unwrap();
    let result = run(
        &spec,
        |_| unreachable!("a probe reads no dataset"),
        |_, _| {},
    );
    assert!(matches!(result, Err(Failure::Invalid(m)) if m.starts_with("probe:")));
}

fn paw_input(functional: &str, rho: &[[[f64; 4]; 4]]) -> String {
    let readme = ROCK_SALT_PROBE;
    let start = readme.find("\n[probe]").unwrap() + 1;
    format!(
        "{}[xc]\nfunctional = \"{functional}\"\n\n[probe]\nterm = \"paw\"\nrho_ij = {{ C = {rho:?} }}\n",
        &readme[..start]
    )
}

fn occupancies() -> [[[f64; 4]; 4]; 2] {
    let spin = |scale: f64| {
        let m = Mat::from_fn(4, 4, |i, j| {
            let k = (i * 4 + j).min(j * 4 + i) as f64;
            if i == j {
                scale * (1.0 - 0.1 * k / 4.0)
            } else {
                scale * 0.05 * (1.0 + k).sin()
            }
        });
        std::array::from_fn(|i| std::array::from_fn(|j| m[(i, j)]))
    };
    [spin(0.6), spin(0.4)]
}

#[test]
fn a_paw_probe_gives_the_spheres_xc_energies_and_d_of_its_occupancies() {
    let rho = occupancies();
    let mut asked = Vec::new();
    let datasets = |z: u8| {
        asked.push(z);
        synthetic_meta(z)
    };
    let Ok(Some(Probed::PawXc { xc_ha, d_ha })) =
        paw_probe(&read_spec(&paw_input("r2scan", &rho)).unwrap(), datasets)
    else {
        panic!("a paw probe's terms");
    };
    assert_eq!(asked, [6]);
    let dataset = Dataset::try_from(synthetic_meta_draft()).unwrap();
    let species = Species::for_functional(dataset, Functional::R2scan).unwrap();
    let rho: Vec<Mat<f64>> = (rho.iter())
        .map(|r| Mat::from_fn(4, 4, |i, j| r[i][j]))
        .collect();
    let one = species.one_center(&rho);
    assert_eq!(xc_ha, one.xc_ha);
    let hartree = species.hartree(&total_occupancies(&rho));
    let xc: Vec<Vec<Vec<f64>>> = (one.d.iter())
        .map(|d| {
            let xc = d - species.frozen_d() - &hartree;
            (0..4)
                .map(|i| (0..4).map(|j| xc[(i, j)]).collect())
                .collect()
        })
        .collect();
    assert_eq!(d_ha, xc);
}

#[test]
fn a_paw_probe_refuses_occupancies_or_a_dataset_that_cannot_hold_them() {
    let rho = occupancies();
    let three: String = {
        let rows: Vec<Vec<f64>> = (0..3)
            .map(|i| (0..3).map(|j| rho[0][i][j]).collect())
            .collect();
        let readme = ROCK_SALT_PROBE;
        let start = readme.find("\n[probe]").unwrap() + 1;
        format!(
            "{}[xc]\nfunctional = \"pbe\"\n\n[probe]\nterm = \"paw\"\nrho_ij = {{ C = [{rows:?}] }}\n",
            &readme[..start]
        )
    };
    let refused = paw_probe(&read_spec(&three).unwrap(), synthetic);
    assert_eq!(
        refused,
        Err(Failure::Invalid(
            "probe.rho_ij: the dataset for Z = 6 has 4 orbitals; give 4 × 4 matrices".into()
        ))
    );
    let pbe = paw_probe(&read_spec(&paw_input("pbe", &rho)).unwrap(), synthetic);
    assert!(matches!(pbe, Ok(Some(Probed::PawXc { .. }))), "{pbe:?}");
    let refused = paw_probe(&read_spec(&paw_input("r2scan", &rho)).unwrap(), synthetic);
    assert_eq!(
        refused,
        Err(Failure::Invalid(
            "datasets: the dataset for Z = 6 has no core kinetic energy densities, which r2scan reads".into()
        ))
    );
}
