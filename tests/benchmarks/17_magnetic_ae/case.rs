use nalgebra::Matrix3;
use serde::Deserialize;
use std::collections::BTreeMap;
use xtask::harness::{
    self, Evidence, Input, Metrics,
    acwf::{BOHR_ANGSTROM, HARTREE_EV, SCALES},
    eos::{self, Bm},
};

const ELEMENTS: [&str; 5] = ["fe", "co", "ni", "cr", "mn"];
const WIDTH: &str = "occupations.width_ha";
const MESH: &str = "kpoints.mesh";
const CUTOFF: &str = "basis.cutoff_ha";
const NEIGHBOURS: [&str; 5] = ["half", "k4", "k8", "e10", "e20"];
const SHAPE: [usize; 3] = [0, 3, 6];
const MEV: f64 = 1000.0;
const CIF_VOLUME_SHARE: f64 = 1e-6;

#[derive(Deserialize)]
struct Wien2k {
    elements: BTreeMap<String, Row>,
}

#[derive(Deserialize)]
struct Row {
    v0_ang3: f64,
    b0_gpa: f64,
    b1: f64,
    cif_volume_ang3: f64,
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let wien2k: Wien2k = serde_json::from_str(&evidence.reference("wien2k.json")?)
        .map_err(|e| format!("reference/wien2k.json: {e}"))?;
    let mut metrics = Metrics::new();
    let mut converged = 0.0_f64;
    let names = evidence.names();
    let ran: Vec<&str> = (ELEMENTS.into_iter())
        .filter(|element| names.contains(&format!("{element}-v1.00").as_str()))
        .collect();
    if ran.is_empty() {
        return Err(format!(
            "no element's {{el}}-v1.00 ran; the elements are {ELEMENTS:?}"
        ));
    }
    for element in ran {
        let row = (wien2k.elements.get(element))
            .ok_or_else(|| format!("reference/wien2k.json has no row of {element}"))?;
        let centre = evidence.input(&format!("{element}-v1.00"))?;
        let atoms = centre.species()?.len() as f64;
        let volume = volume_ang3(&centre.cell_bohr()?) / atoms;
        if (volume / row.cif_volume_ang3 - 1.0).abs() > CIF_VOLUME_SHARE {
            return Err(format!(
                "{element}-v1.00 holds {volume} Å³ per atom, not the primCIF's {}",
                row.cif_volume_ang3
            ));
        }
        let mut points = Vec::new();
        for s in SCALES {
            let name = format!("{element}-v{s:.2}");
            let input = evidence.input(&name)?;
            if !input.equal_apart_from(&centre, &["structure.cell_bohr"])
                || !scaled(&input.cell_bohr()?, &centre.cell_bohr()?, s.cbrt())
            {
                return Err(format!(
                    "{name} must be {element}-v1.00 with its cell scaled to {s} V"
                ));
            }
            points.push((
                volume_ang3(&input.cell_bohr()?) / atoms,
                evidence.output(&name)?.energy_ha("free_energy_ha")? * HARTREE_EV / atoms,
            ));
        }
        let fit = eos::fit(&points).map_err(|e| format!("{element}: {e}"))?;
        let reference = Bm {
            v0_ang3: row.v0_ang3,
            b0_ev_ang3: row.b0_gpa * eos::GPA_EV_ANG3,
            b1: row.b1,
        };
        metrics.insert(format!("{element}_delta_mev"), eos::delta(&fit, &reference));
        metrics.insert(format!("{element}_v0_ang3"), fit.v0_ang3);
        metrics.insert(
            format!("{element}_b0_gpa"),
            fit.b0_ev_ang3 / eos::GPA_EV_ANG3,
        );
        metrics.insert(format!("{element}_b1"), fit.b1);
        converged = harness::worst(converged, rule(evidence, element, atoms)?);
    }
    metrics.insert("convergence_mev".to_owned(), converged);
    Ok(metrics)
}

fn rule(evidence: &Evidence, element: &str, atoms: f64) -> Result<f64, String> {
    let point = |s: f64| format!("{element}-v{s:.2}");
    let energy = |name: &str| -> Result<f64, String> {
        Ok(evidence.output(name)?.energy_ha("free_energy_ha")? * HARTREE_EV * MEV / atoms)
    };
    let [low, middle, high] = SHAPE.map(|i| SCALES[i]);
    let mut worst = 0.0_f64;
    for suffix in NEIGHBOURS {
        let mut shifted = [0.0; 3];
        let mut base = [0.0; 3];
        for (k, s) in [low, middle, high].into_iter().enumerate() {
            let at_name = point(s);
            let name = format!("{at_name}-{suffix}");
            let (at, other) = (evidence.input(&at_name)?, evidence.input(&name)?);
            step(&at, &other, suffix).map_err(|key| {
                format!("{name} must be {at_name} with its {suffix} step in {key} only")
            })?;
            base[k] = energy(&at_name)?;
            shifted[k] = energy(&name)?;
        }
        for k in [0, 2] {
            worst = worst.max(((shifted[k] - shifted[1]) - (base[k] - base[1])).abs());
        }
    }
    Ok(worst)
}

fn step(at: &Input, other: &Input, suffix: &str) -> Result<(), &'static str> {
    let number = |input: &Input, key: &'static str| input.number(key).map_err(|_| key);
    let (key, ok) = match suffix {
        "half" => (WIDTH, number(other, WIDTH)? == number(at, WIDTH)? / 2.0),
        "k4" | "k8" => {
            let plus = if suffix == "k4" { 4 } else { 8 };
            let mesh = at.mesh().map_err(|_| MESH)?;
            (
                MESH,
                other.mesh().map_err(|_| MESH)? == mesh.map(|n| n + plus),
            )
        }
        _ => {
            let plus = if suffix == "e10" { 10.0 } else { 20.0 };
            (CUTOFF, number(other, CUTOFF)? == number(at, CUTOFF)? + plus)
        }
    };
    if ok && other.equal_apart_from(at, &[key]) {
        Ok(())
    } else {
        Err(key)
    }
}

fn scaled(cell: &[[f64; 3]; 3], base: &[[f64; 3]; 3], factor: f64) -> bool {
    (cell.iter().flatten().zip(base.iter().flatten())).all(|(c, b)| (c - b * factor).abs() <= 1e-12)
}

fn volume_ang3(cell: &[[f64; 3]; 3]) -> f64 {
    Matrix3::from_fn(|i, j| cell[i][j]).determinant().abs() * BOHR_ANGSTROM.powi(3)
}
