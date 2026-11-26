use xtask::harness::{Evidence, Metrics};

#[derive(Clone, Copy, PartialEq)]
pub enum Order {
    None,
    Ferro,
    Antiferro { ferro: &'static str },
}

const TRIPLES: [(&str, &str, &str, Order); 6] = [
    (
        "input",
        "gaas",
        "gaas_stress_difference_ha_bohr3",
        Order::None,
    ),
    (
        "input",
        "gaas-volume",
        "gaas_volume_stress_difference_ha_bohr3",
        Order::None,
    ),
    (
        "fe",
        "fe",
        "fe_pbe_stress_difference_ha_bohr3",
        Order::Ferro,
    ),
    ("feo", "feo", "feo_stress_difference_ha_bohr3", Order::Ferro),
    (
        "fe-lda",
        "fe-lda",
        "fe_lda_stress_difference_ha_bohr3",
        Order::Ferro,
    ),
    (
        "afm",
        "afm",
        "afm_stress_difference_ha_bohr3",
        Order::Antiferro { ferro: "fe" },
    ),
];
const MIN_FERROMAGNETIC_MOMENT: f64 = 2.0;
const MAX_ANTIFERROMAGNETIC_NET_MOMENT: f64 = 0.1;

type Matrix = [[f64; 3]; 3];

fn solve(a: &Matrix, b: &Matrix) -> Matrix {
    let minor = |r: usize, c: usize| {
        let (r0, r1) = ((r + 1) % 3, (r + 2) % 3);
        let (c0, c1) = ((c + 1) % 3, (c + 2) % 3);
        a[r0][c0] * a[r1][c1] - a[r0][c1] * a[r1][c0]
    };
    let det: f64 = (0..3).map(|c| a[0][c] * minor(0, c)).sum();
    std::array::from_fn(|i| {
        std::array::from_fn(|j| (0..3).map(|k| minor(k, i) / det * b[k][j]).sum())
    })
}

fn volume(a: &Matrix) -> f64 {
    (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
        .abs()
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = Metrics::new();
    let max_moment_change = triples(evidence, &TRIPLES, &mut metrics)?;
    symmetric(evidence, &mut metrics)?;
    metrics.insert("moment_spread_bohr_magneton".to_owned(), max_moment_change);
    Ok(metrics)
}

pub fn triples(
    evidence: &Evidence,
    table: &[(&str, &str, &str, Order)],
    metrics: &mut Metrics,
) -> Result<f64, String> {
    let mut max_moment_change: f64 = 0.0;
    for &(name, stem, metric, order) in table {
        let (plus, minus) = (format!("{stem}-plus"), format!("{stem}-minus"));
        let at = evidence.input(name)?;
        for other in [&plus, &minus] {
            if !evidence
                .input(other)?
                .equal_apart_from(&at, &["structure.cell_bohr", "outputs"])
            {
                return Err(format!("{other} must differ from {name} in its cell only"));
            }
        }
        let a = at.cell_bohr()?;
        let strain = |other: &str| -> Result<Matrix, String> {
            let s = solve(&a, &evidence.input(other)?.cell_bohr()?);
            Ok(std::array::from_fn(|i| {
                std::array::from_fn(|j| s[i][j] - if i == j { 1.0 } else { 0.0 })
            }))
        };
        let (ahead, behind) = (strain(&plus)?, strain(&minus)?);
        for i in 0..3 {
            for j in 0..3 {
                if (ahead[i][j] + behind[i][j]).abs() > 1e-12 {
                    return Err(format!("{name} must lie midway between {plus} and {minus}"));
                }
                if (ahead[i][j] - ahead[j][i]).abs() > 1e-12 {
                    return Err(format!(
                        "{plus} must strain {name} along a symmetric direction"
                    ));
                }
            }
        }
        let sizes = evidence.output(name)?.basis_sizes()?;
        for other in [&plus, &minus] {
            if evidence.output(other)?.basis_sizes()? != sizes {
                return Err(format!(
                    "{other}: a plane wave crossed the cutoff, so F jumps there; take a smaller strain"
                ));
            }
        }
        let delta: Matrix =
            std::array::from_fn(|i| std::array::from_fn(|j| ahead[i][j] - behind[i][j]));
        let size = delta.iter().flatten().map(|x| x * x).sum::<f64>().sqrt();
        let sigma = evidence.output(name)?.stress_ha_bohr3()?;
        let omega = volume(&a);
        let work: f64 = (sigma.iter().flatten().zip(delta.iter().flatten()))
            .map(|(s, d)| s * d)
            .sum::<f64>()
            * omega;
        let rise = evidence.output(&plus)?.energy_ha("free_energy_ha")?
            - evidence.output(&minus)?.energy_ha("free_energy_ha")?;
        if size == 0.0 {
            return Err(format!("{plus}: a nonzero strain"));
        }
        metrics.insert(metric.to_owned(), (rise - work).abs() / (omega * size));
        if order != Order::None {
            let moment = evidence.output(name)?.magnetization_bohr_magneton()?;
            if order == Order::Ferro && moment.abs() < MIN_FERROMAGNETIC_MOMENT {
                return Err(format!(
                    "{name}: a moment of {moment} μB, below {MIN_FERROMAGNETIC_MOMENT}"
                ));
            }
            if let Order::Antiferro { ferro } = order {
                if moment.abs() > MAX_ANTIFERROMAGNETIC_NET_MOMENT {
                    return Err(format!(
                        "{name}: a net moment of {moment} μB, above {MAX_ANTIFERROMAGNETIC_NET_MOMENT}"
                    ));
                }
                let nm = format!("{name}-nm");
                for (other, keys) in [
                    (nm.as_str(), ["spin", "outputs"]),
                    (ferro, ["spin.moments", "outputs"]),
                ] {
                    if !evidence.input(other)?.equal_apart_from(&at, &keys) {
                        return Err(format!("{other} must differ from {name} in its spin only"));
                    }
                }
                let free = |input: &str| evidence.output(input)?.energy_ha("free_energy_ha");
                let (nonmagnetic_energy_ha, ferromagnetic_energy_ha) = (free(&nm)?, free(ferro)?);
                metrics.insert(
                    "afm_order_share".to_owned(),
                    (nonmagnetic_energy_ha - free(name)?)
                        / (nonmagnetic_energy_ha - ferromagnetic_energy_ha),
                );
            }
            for other in [&plus, &minus] {
                let m = evidence.output(other)?.magnetization_bohr_magneton()?;
                max_moment_change = max_moment_change.max((m - moment).abs());
            }
        }
    }
    Ok(max_moment_change)
}

fn symmetric(evidence: &Evidence, metrics: &mut Metrics) -> Result<(), String> {
    let (reduced, full) = (
        evidence.output("fe-sym")?.stress_ha_bohr3()?,
        evidence.output("fe-sym-full")?.stress_ha_bohr3()?,
    );
    if evidence.input("fe-sym")?.symmetry()? == Some(false)
        || evidence.input("fe-sym-full")?.symmetry()? != Some(false)
    {
        return Err(
            "fe-sym must use symmetry, and fe-sym-full set kpoints.symmetry = false".into(),
        );
    }
    let apart = (reduced.iter().flatten().zip(full.iter().flatten()))
        .map(|(a, b)| (a - b).abs())
        .fold(0.0, f64::max);
    metrics.insert("symmetry_stress_difference_ha_bohr3".to_owned(), apart);
    Ok(())
}
