use std::collections::BTreeMap;
use xtask::harness::{Evidence, Metrics};

const CRYSTALS: [(&str, &str); 3] = [
    ("input", "NaCl"),
    ("cesium_chloride", "CsCl"),
    ("zincblende", "ZnS_zincblende"),
];

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let table: BTreeMap<String, f64> = serde_json::from_str(&evidence.reference("madelung.json")?)
        .map_err(|e| format!("reference/madelung.json: {e}"))?;
    let mut worst: f64 = 0.0;
    for name in evidence.names() {
        let key = CRYSTALS
            .iter()
            .find(|(input, _)| *input == name)
            .map(|(_, key)| *key)
            .ok_or_else(|| format!("{name}: no crystal for this input"))?;
        let want = table
            .get(key)
            .ok_or_else(|| format!("madelung.json has no {key}"))?;
        let input = evidence.input(name)?;
        let (cell, sites) = (input.cell_bohr()?, input.probe_positions_fractional()?);
        if input.probe_charges()? != [1.0, -1.0] || sites.len() != 2 {
            return Err(format!(
                "{name}: the convention is one +1 and one −1 charge per cell"
            ));
        }
        let energy = evidence.output(name)?.ewald_energy_ha()?;
        let madelung = -energy * nearest(cell, sites[0], sites[1]);
        worst = worst.max((madelung - want).abs());
    }
    Ok(Metrics::from([("madelung_error".to_owned(), worst)]))
}

fn nearest(cell: [[f64; 3]; 3], a: [f64; 3], b: [f64; 3]) -> f64 {
    let mut best = f64::INFINITY;
    for i in -3..=3 {
        for j in -3..=3 {
            for l in -3..=3 {
                let f = [
                    b[0] - a[0] + f64::from(i),
                    b[1] - a[1] + f64::from(j),
                    b[2] - a[2] + f64::from(l),
                ];
                let r: [f64; 3] = std::array::from_fn(|c| (0..3).map(|k| f[k] * cell[k][c]).sum());
                best = best.min(r.iter().map(|x| x * x).sum::<f64>().sqrt());
            }
        }
    }
    best
}
