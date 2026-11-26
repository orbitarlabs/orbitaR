use xtask::harness::{Evidence, Metrics};

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut worst: f64 = 0.0;
    for name in evidence.names() {
        let input = evidence.input(name)?;
        let cell = input.cell_bohr()?;
        let (charges, exponents, sites) = (
            input.probe_charges()?,
            input.probe_exponents()?,
            input.probe_positions_fractional()?,
        );
        let want = analytic(cell, &charges, &exponents, &sites);
        let got = evidence.output(name)?.hartree_energy_ha()?;
        worst = worst.max((got - want).abs() / want.abs());
    }
    Ok(Metrics::from([(
        "hartree_relative_error".to_owned(),
        worst,
    )]))
}

fn analytic(a: [[f64; 3]; 3], charges: &[f64], exponents: &[f64], sites: &[[f64; 3]]) -> f64 {
    let tau = 2.0 * std::f64::consts::PI;
    let cross = |u: [f64; 3], v: [f64; 3]| {
        [
            u[1] * v[2] - u[2] * v[1],
            u[2] * v[0] - u[0] * v[2],
            u[0] * v[1] - u[1] * v[0],
        ]
    };
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let volume = dot(a[0], cross(a[1], a[2])).abs();
    let signed = dot(a[0], cross(a[1], a[2]));
    let b = [cross(a[1], a[2]), cross(a[2], a[0]), cross(a[0], a[1])]
        .map(|v| v.map(|x| x * tau / signed));
    let g_max = (160.0 * exponents.iter().fold(0.0_f64, |m, &x| m.max(x))).sqrt();
    let reach = a.map(|row| (g_max * dot(row, row).sqrt() / tau).ceil() as i32 + 1);
    let mut sum = 0.0;
    for i in -reach[0]..=reach[0] {
        for j in -reach[1]..=reach[1] {
            for l in -reach[2]..=reach[2] {
                let m = [i, j, l].map(f64::from);
                let g: [f64; 3] = std::array::from_fn(|c| (0..3).map(|k| m[k] * b[k][c]).sum());
                let g2 = dot(g, g);
                if g2 == 0.0 || g2 > g_max * g_max {
                    continue;
                }
                let (mut re, mut im) = (0.0, 0.0);
                for ((q, alpha), f) in charges.iter().zip(exponents).zip(sites) {
                    let (phase, form) = (tau * dot(m, *f), q * (-g2 / (4.0 * alpha)).exp());
                    (re, im) = (re + form * phase.cos(), im - form * phase.sin());
                }
                sum += (re * re + im * im) / g2;
            }
        }
    }
    tau / volume * sum
}
