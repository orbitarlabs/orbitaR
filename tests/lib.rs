#![expect(special_module_name, reason = "tests/main.rs mirrors src/main.rs")]

mod io;
mod ks;
mod main;
mod model;
mod paw;
mod pw;
mod xc;

use orbitar::model::Functional;
use orbitar::paw::{Channel, CoreKinetic, DatasetDraft, RadialGrid, Shape};

const ONE_SI: &str = "species = [\"Si\"]\npositions_fractional = [[0.0, 0.0, 0.0]]";

#[expect(
    clippy::disallowed_macros,
    reason = "the tests place the repository's family table beside their datasets"
)]
fn jth_table() -> &'static str {
    include_str!("../datasets/jth-2.0.toml")
}

fn with_atoms(input: &str, atoms: &str) -> String {
    input.replace("[basis]", &format!("{atoms}\n\n[basis]"))
}

#[expect(
    clippy::disallowed_macros,
    reason = "the tests prove the README examples, so they read README.md"
)]
fn readme_block(heading: &str) -> String {
    let readme = include_str!("../README.md");
    let section = readme
        .split(&format!("\n## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("README.md has no `## {heading}`"));
    let example = section.split("```").nth(1).unwrap_or_default();
    example.lines().skip(1).collect::<Vec<_>>().join("\n")
}

#[expect(
    clippy::disallowed_macros,
    reason = "the tests prove the README's links, so they read README.md"
)]
fn readme_section(heading: &str) -> String {
    let readme = include_str!("../README.md");
    let section = readme
        .split(&format!("\n## {heading}\n"))
        .nth(1)
        .unwrap_or_else(|| panic!("README.md has no `## {heading}`"));
    section.split("\n## ").next().unwrap_or_default().to_owned()
}

const EMPTY_CUBE: &str = "schema = 1

[structure]
cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]

[basis]
cutoff_ha = 2.0

[kpoints]
list = [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]

[bands]
count = 2
";

const EMPTY_CUBE_OUTCOME: &str = r#"{
  "basis_sizes": [27, 28],
  "eigenvalues_ha": [[[0.0, 0.5483113556160754], [0.13707783890401884, 0.13707783890401884]]],
  "kpoint_weights": [0.5, 0.5],
  "kpoints_fractional": [[0.0, 0.0, 0.0], [0.5, 0.0, 0.0]]
}"#;

const ROCK_SALT_PROBE: &str = "schema = 1

[structure]
cell_bohr = [[0.0, 1.0, 1.0], [1.0, 0.0, 1.0], [1.0, 1.0, 0.0]]

[basis]
cutoff_ha = 10.0

[kpoints]
list = [[0.0, 0.0, 0.0]]

[probe]
term = \"ewald\"
charges = [1.0, -1.0]
positions_fractional = [[0.0, 0.0, 0.0],
                        [0.5, 0.5, 0.5]]
# exponents = [1.5, 1.5]
# points = [[0.5, 0.5, 0.0, 0.0, 0.0]]
# rho_ij = { C = [[[1.0]]] }
";

fn synthetic_draft() -> DatasetDraft {
    synthetic_draft_on(RadialGrid::logarithmic(1e-3, 0.01, 1001).unwrap())
}

fn synthetic_meta_draft() -> DatasetDraft {
    let mut d = synthetic_draft();
    let r = d.grid.radii().to_vec();
    let c = 0.3 * (3.0 * std::f64::consts::PI.powi(2)).powf(2.0 / 3.0);
    let above = |n: &[f64], alpha: f64| -> Vec<f64> {
        (n.iter().zip(&r))
            .map(|(n, x)| c * n.max(0.0).powf(5.0 / 3.0) + alpha * alpha * x * x * n)
            .collect()
    };
    let (ae, pseudo) = (
        above(&d.core_density, 4.0),
        above(&d.pseudo_core_density, 1.0),
    );
    d.core_kinetic = Some(CoreKinetic::checked(&d.grid, ae, pseudo).unwrap());
    d
}

fn synthetic_3d_draft() -> DatasetDraft {
    let mut d = synthetic_draft_on(RadialGrid::logarithmic(1e-3, 0.03, 334).unwrap());
    let r = d.grid.radii().to_vec();
    let on = |f: &dyn Fn(f64) -> f64| r.iter().map(|&x| f(x)).collect::<Vec<f64>>();
    let rc = 2.2_f64;
    d.channels = [(0_i32, 1.0_f64, 1.0), (1, 1.5, 0.0), (2, 1.5, 7.0)]
        .into_iter()
        .map(|(l, a, occupation)| {
            let (value, slope) = ((-a * rc).exp(), -a * (-a * rc).exp());
            let (b, c) = (slope / (2.0 * rc), value - slope * rc / 2.0);
            Channel {
                l: l as usize,
                occupation: Some(occupation),
                ae: on(&|x| x.powi(l) * (-a * x).exp()),
                pseudo: on(&|x| {
                    x.powi(l)
                        * if x < rc {
                            c + b * x * x
                        } else {
                            (-a * x).exp()
                        }
                }),
                projector: on(&|x| {
                    if x <= rc {
                        x.powi(l) * (rc - x).powi(2)
                    } else {
                        0.0
                    }
                }),
            }
        })
        .collect();
    d.kinetic_differences = (0..9).map(|k| if k % 4 == 0 { 1.0 } else { 0.2 }).collect();
    d.sphere_radius = rc;
    d.shape = Shape::Bessel { radius: 0.8 * rc };
    d.z = 26.0;
    d.core_electrons = 18.0;
    d.valence_electrons = 8.0;
    d.core_density.iter_mut().for_each(|n| *n *= 9.0);
    let c = 0.3 * (3.0 * std::f64::consts::PI.powi(2)).powf(2.0 / 3.0);
    let above = |n: &[f64], alpha: f64| -> Vec<f64> {
        (n.iter().zip(&r))
            .map(|(n, x)| c * n.max(0.0).powf(5.0 / 3.0) + alpha * alpha * x * x * n)
            .collect()
    };
    let (ae, pseudo) = (
        above(&d.core_density, 4.0),
        above(&d.pseudo_core_density, 1.0),
    );
    d.core_kinetic = Some(CoreKinetic::checked(&d.grid, ae, pseudo).unwrap());
    d
}

fn synthetic_silicon() -> DatasetDraft {
    let carbon = synthetic_draft();
    DatasetDraft {
        z: 14.0,
        core_electrons: 10.0,
        core_density: carbon.core_density.iter().map(|n| 5.0 * n).collect(),
        ..carbon
    }
}

fn synthetic_draft_on(grid: RadialGrid) -> DatasetDraft {
    let r = grid.radii().to_vec();
    let sphere_radius = 1.5;
    let on = |f: &dyn Fn(f64) -> f64| r.iter().map(|&r| f(r)).collect::<Vec<f64>>();
    let channel = |l: i32| {
        let ae = on(&|r| r.powi(l) * (-r).exp());
        let inside = |r: f64| r <= sphere_radius;
        Channel {
            l: l as usize,
            occupation: Some(2.0),
            pseudo: on(&|r| r.powi(l) * (-r).exp() * if inside(r) { 0.5 } else { 1.0 }),
            projector: on(&|r| {
                if inside(r) {
                    r.powi(l) * (sphere_radius - r).powi(2)
                } else {
                    0.0
                }
            }),
            ae,
        }
    };
    let alpha = 4.0_f64;
    let gaussian = |r: f64| 2.0 * (alpha / std::f64::consts::PI).powf(1.5) * (-alpha * r * r).exp();
    DatasetDraft {
        z: 6.0,
        core_electrons: 2.0,
        valence_electrons: 4.0,
        functional: Functional::Pbe,
        sphere_radius,
        shape: Shape::Bessel { radius: 1.2 },
        channels: vec![channel(0), channel(1)],
        kinetic_differences: vec![1.0, 0.2, 0.2, 3.0],
        core_density: on(&gaussian),
        pseudo_core_density: on(&|r| 0.1 * gaussian(r / 2.0)),
        pseudo_valence_density: on(&|r| 0.3 * (-r).exp()),
        local_potential: on(&|r| -4.0 / (r * r + 1.0).sqrt()),
        core_kinetic: None,
        grid,
    }
}
