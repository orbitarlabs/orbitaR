use serde::Deserialize;
use std::collections::BTreeMap;
use xtask::harness::{Evidence, Metrics};

#[derive(Deserialize)]
struct Table {
    functionals: BTreeMap<String, Rows>,
}

#[derive(Deserialize)]
struct Rows {
    points: Vec<Vec<f64>>,
    exchange: Vec<Vec<f64>>,
    correlation: Vec<Vec<f64>>,
    exchange_case: Vec<bool>,
    correlation_case: Vec<bool>,
}

fn error(got: f64, want: f64) -> f64 {
    match (want, got) {
        (0.0, 0.0) => 0.0,
        (0.0, _) => f64::INFINITY,
        _ => ((got - want) / want).abs(),
    }
}

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let table: Table = serde_json::from_str(&evidence.reference("table.json")?)
        .map_err(|e| format!("reference/table.json: {e}"))?;
    let mut worst: f64 = 0.0;
    let mut probed = Vec::new();
    for name in evidence.names() {
        let input = evidence.input(name)?;
        let word = input.functional()?;
        let rows = (table.functionals.get(word))
            .ok_or_else(|| format!("{name}: table.json has no Libxc rows for {word:?}"))?;
        if input.probe_points()? != rows.points {
            return Err(format!(
                "{name}: probe.points must equal the table's {word} points"
            ));
        }
        let got = evidence.output(name)?.xc_energy_density_ha_bohr3()?;
        let lengths = [
            got.len(),
            rows.exchange.len(),
            rows.correlation.len(),
            rows.exchange_case.len(),
            rows.correlation_case.len(),
        ];
        if lengths != [rows.points.len(); 5] {
            return Err(format!(
                "{name}: {} values for {} points",
                got.len(),
                rows.points.len()
            ));
        }
        let parts = [
            (&rows.exchange, &rows.exchange_case),
            (&rows.correlation, &rows.correlation_case),
        ];
        for (part, (table, measured)) in parts.into_iter().enumerate() {
            for ((values, row), &measured) in got.iter().zip(table).zip(measured) {
                if measured {
                    worst = worst.max(error(values[part], row[0]));
                }
            }
        }
        probed.push(word.to_owned());
    }
    if let Some(word) = (table.functionals.keys()).find(|word| !probed.contains(word)) {
        return Err(format!("no input probes the table's {word:?} rows"));
    }
    Ok(Metrics::from([("xc_relative_error".to_owned(), worst)]))
}
