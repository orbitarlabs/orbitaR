use crate::Findings;
use std::{fs, path::Path};

type Matrix = [[f64; 3]; 3];

pub fn write(
    root: &Path,
    centre: &Path,
    direction: &[f64],
    h: f64,
    stem: Option<&str>,
) -> Findings {
    match triple(root, centre, direction, h, stem) {
        Ok(()) => Vec::new(),
        Err(e) => vec![format!("strain: {e}")],
    }
}

fn triple(
    root: &Path,
    centre: &Path,
    direction: &[f64],
    h: f64,
    stem: Option<&str>,
) -> Result<(), String> {
    let path = root.join(centre);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let k: Matrix = match direction {
        [a, b, c, d, e, f, g, p, q] => [[*a, *b, *c], [*d, *e, *f], [*g, *p, *q]],
        _ => {
            return Err(format!(
                "--direction takes 9 numbers, row-major; got {}",
                direction.len()
            ));
        }
    };
    if (0..3).any(|i| (0..3).any(|j| k[i][j] != k[j][i])) {
        return Err("--direction must be symmetric".into());
    }
    let size = k.iter().flatten().map(|x| x * x).sum::<f64>().sqrt();
    if !(size > 0.0 && size.is_finite() && h > 0.0 && h.is_finite()) {
        return Err("--direction must be nonzero and --h positive".into());
    }
    let e = k.map(|row| row.map(|x| x / size));
    let a = cell(&text, &path)?;
    let stem = match stem {
        Some(s) => s.to_owned(),
        None => (path.file_stem().and_then(|s| s.to_str()))
            .ok_or_else(|| format!("{}: no file stem", path.display()))?
            .to_owned(),
    };
    for (sign, suffix) in [(1.0, "plus"), (-1.0, "minus")] {
        let m: Matrix = std::array::from_fn(|i| {
            std::array::from_fn(|j| f64::from(u8::from(i == j)) + sign * h * e[i][j])
        });
        let b: Matrix = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                (a[i][2]).mul_add(m[2][j], a[i][1].mul_add(m[1][j], a[i][0] * m[0][j]))
            })
        });
        let out = path.with_file_name(format!("{stem}-{suffix}.toml"));
        fs::write(&out, strained(&text, &b)).map_err(|e| format!("{}: {e}", out.display()))?;
    }
    Ok(())
}

fn cell(text: &str, path: &Path) -> Result<Matrix, String> {
    let value: toml::Table = text
        .parse()
        .map_err(|e| format!("{}: {e}", path.display()))?;
    let rows = (value.get("structure").and_then(|s| s.get("cell_bohr")))
        .and_then(toml::Value::as_array)
        .ok_or_else(|| format!("{}: no structure.cell_bohr", path.display()))?;
    let number = |v: &toml::Value| v.as_float().or(v.as_integer().map(|i| i as f64));
    let read: Option<Vec<Vec<f64>>> = (rows.iter())
        .map(|r| r.as_array().and_then(|r| r.iter().map(number).collect()))
        .collect();
    match read.as_deref() {
        Some([a, b, c]) if [a, b, c].iter().all(|r| r.len() == 3) => {
            Ok([a, b, c].map(|r| [r[0], r[1], r[2]]))
        }
        _ => Err(format!(
            "{}: structure.cell_bohr must be 3 rows of 3 numbers",
            path.display()
        )),
    }
}

fn strained(text: &str, b: &Matrix) -> String {
    let row = |r: &[f64; 3]| format!("[{}, {}, {}]", number(r[0]), number(r[1]), number(r[2]));
    let value = format!(
        "cell_bohr = [{},\n             {},\n             {}]",
        row(&b[0]),
        row(&b[1]),
        row(&b[2])
    );
    let (mut out, mut in_cell, mut in_outputs) = (Vec::new(), false, false);
    for line in text.lines() {
        if line.starts_with('[') {
            in_outputs = line.trim() == "[outputs]";
        }
        if line.starts_with("cell_bohr") {
            in_cell = true;
            out.push(value.clone());
        }
        if !in_cell && !in_outputs {
            out.push(line.to_owned());
        }
        if in_cell && line.trim_end().ends_with("]]") {
            in_cell = false;
        }
    }
    while out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out.join("\n") + "\n"
}

fn number(x: f64) -> String {
    let text = format!("{x:?}");
    match text.split_once('e') {
        Some((mantissa, exponent)) => {
            let (sign, digits) = exponent
                .strip_prefix('-')
                .map_or(("", exponent), |d| ("-", d));
            format!("{mantissa}e{sign}{digits:0>2}")
        }
        None => text,
    }
}
