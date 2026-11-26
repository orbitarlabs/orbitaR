use xtask::harness::{Evidence, Metrics};

const RUNS: [(&str, &str); 2] = [("input", "jth-2.0"), ("v0", "orbitaR-PBE-v0")];

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let [(jth, jth_family), (v0, v0_family)] = RUNS;
    let (a, b) = (evidence.input(jth)?, evidence.input(v0)?);
    for (input, family, name) in [(&a, jth_family, jth), (&b, v0_family, v0)] {
        if input.family() != family {
            return Err(format!(
                "{name} must name the family {family}, not {}",
                input.family()
            ));
        }
    }
    if !a.equal_apart_from(&b, &["datasets"]) {
        return Err(format!("{v0} must equal {jth} but for [datasets]"));
    }
    if !b.species()?.iter().any(|s| s == "O") {
        return Err(format!(
            "{v0} must hold O, the element of {v0_family}'s own row"
        ));
    }
    let (one, two) = (evidence.output(jth)?, evidence.output(v0)?);
    Ok(Metrics::from([
        (
            "free_energy_difference_ha".to_owned(),
            (two.energy_ha("free_energy_ha")? - one.energy_ha("free_energy_ha")?).abs(),
        ),
        (
            "term_difference_ha".to_owned(),
            one.largest_energy_difference(&two, 1.0)?,
        ),
        (
            "fermi_level_difference_ha".to_owned(),
            (two.fermi_level_ha()? - one.fermi_level_ha()?).abs(),
        ),
    ]))
}
