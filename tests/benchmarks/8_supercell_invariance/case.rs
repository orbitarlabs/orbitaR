use xtask::harness::{Evidence, Metrics};

const PAIRS: [(&str, &str, bool); 2] = [("input", "si-double", false), ("fe", "fe-double", true)];

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let (mut term, mut fermi, mut moment) = (0.0_f64, 0.0_f64, 0.0_f64);
    for (cell, supercell, spin) in PAIRS {
        let (a, b) = (
            evidence.input(cell)?.cell_bohr()?,
            evidence.input(supercell)?.cell_bohr()?,
        );
        if b[0] != a[0].map(|x| 2.0 * x) || b[1] != a[1] || b[2] != a[2] {
            return Err(format!(
                "{supercell} must double a1 of {cell} and keep a2, a3"
            ));
        }
        let (one, two) = (evidence.output(cell)?, evidence.output(supercell)?);
        term = term.max(one.largest_energy_difference(&two, 2.0)?);
        fermi = fermi.max((two.fermi_level_ha()? - one.fermi_level_ha()?).abs());
        if spin {
            moment = moment.max(
                (two.magnetization_bohr_magneton()? - 2.0 * one.magnetization_bohr_magneton()?)
                    .abs(),
            );
        }
    }
    Ok(Metrics::from([
        ("term_difference_ha".to_owned(), term),
        ("fermi_level_difference_ha".to_owned(), fermi),
        ("magnetization_difference_bohr_magneton".to_owned(), moment),
    ]))
}
