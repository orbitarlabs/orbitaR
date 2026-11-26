use xtask::harness::{self, Evidence, Metrics};

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let cell = evidence.input("input")?.cell_bohr()?;
    let one = evidence.output("input")?;
    let (mut term, mut fermi) = (0.0_f64, 0.0_f64);
    for name in evidence.names().into_iter().filter(|&n| n != "input") {
        let rows = evidence.input(name)?.cell_bohr()?;
        let mut f = 1.0;
        for (row, base) in rows.iter().zip(&cell) {
            let dot = |a: &[f64; 3], b: &[f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let n = (dot(row, base) / dot(base, base)).round();
            if n < 1.0 || *row != base.map(|x| n * x) {
                return Err(format!(
                    "{name}: each lattice vector must repeat input's a whole number of times"
                ));
            }
            f *= n;
        }
        let repeat = evidence.output(name)?;
        term = harness::worst(term, one.largest_energy_difference(&repeat, f)? / f);
        fermi = harness::worst(
            fermi,
            (repeat.fermi_level_ha()? - one.fermi_level_ha()?).abs(),
        );
    }
    Ok(Metrics::from([
        ("term_difference_ha_per_cell".to_owned(), term),
        ("fermi_level_difference_ha".to_owned(), fermi),
    ]))
}
