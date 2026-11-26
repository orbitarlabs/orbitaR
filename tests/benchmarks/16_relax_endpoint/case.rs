use xtask::harness::{Evidence, Metrics, relax};

const RUNS_APART: f64 = 2.0 * 0.54 / 100.0;

fn largest_gap<const N: usize>(a: &[[f64; N]], b: &[[f64; N]]) -> f64 {
    (a.iter().flatten().zip(b.iter().flatten()))
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, xtask::harness::worst)
}

fn diagonal(cell: &[[f64; 3]; 3]) -> f64 {
    [[1.0, 1.0], [1.0, -1.0], [-1.0, 1.0], [-1.0, -1.0]]
        .iter()
        .map(|[s, t]| {
            (0..3)
                .map(|k| (s * cell[0][k] + t * cell[1][k] + cell[2][k]).powi(2))
                .sum::<f64>()
                .sqrt()
        })
        .fold(0.0, f64::max)
}

fn volume(a: &[[f64; 3]; 3]) -> f64 {
    (a[0][0] * (a[1][1] * a[2][2] - a[1][2] * a[2][1])
        - a[0][1] * (a[1][0] * a[2][2] - a[1][2] * a[2][0])
        + a[0][2] * (a[1][0] * a[2][1] - a[1][1] * a[2][0]))
        .abs()
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let mut metrics = Metrics::new();
    for name in evidence.names() {
        let input = evidence.input(name)?;
        let (cold, warm) = (evidence.output(name)?, evidence.warm(name)?);
        let free = RUNS_APART * input.number("scf.energy_tolerance_ha")?;
        let g_max = (2.0 * input.number("basis.density_cutoff_ha")?).sqrt();
        let cell = input.cell_bohr()?;
        let bounds = [
            free,
            g_max * free,
            g_max * diagonal(&cell) * free / volume(&cell),
        ];
        let forces = cold.forces_ha_bohr()?;
        let gaps = [
            (cold.energy_ha("free_energy_ha")? - warm.energy_ha("free_energy_ha")?).abs(),
            largest_gap(&forces, &warm.forces_ha_bohr()?),
            largest_gap(&cold.stress_ha_bohr3()?, &warm.stress_ha_bohr3()?),
        ];
        let stem = name.replace('-', "_");
        let units = ["free_energy_ha", "force_ha_bohr", "stress_ha_bohr3"];
        for ((unit, gap), bound) in units.iter().zip(gaps).zip(bounds) {
            metrics.insert(format!("{stem}_{unit}_difference"), gap);
            metrics.insert(format!("{stem}_{unit}_share"), gap / bound);
        }
        let fmax = (forces.iter())
            .map(|f| f.iter().map(|x| x * x).sum::<f64>().sqrt())
            .fold(0.0, xtask::harness::worst);
        metrics.insert(format!("{stem}_fmax_share"), fmax / relax::FMAX_HA_BOHR);
    }
    Ok(metrics)
}
