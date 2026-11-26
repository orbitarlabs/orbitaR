use xtask::harness::{Evidence, Metrics};

const BOX: i32 = 8;
const SHELL_MARGIN: f64 = 1e-12;

pub fn measure(evidence: &Evidence) -> Result<Metrics, String> {
    let (mut eigenvalue_error, mut size_error): (f64, f64) = (0.0, 0.0);
    for name in evidence.names() {
        let (eigenvalue, size) = errors(evidence, name)?;
        eigenvalue_error = eigenvalue_error.max(eigenvalue);
        size_error = size_error.max(size);
    }
    Ok(Metrics::from([
        ("eigenvalue_error_ha".to_owned(), eigenvalue_error),
        ("basis_size_error".to_owned(), size_error),
    ]))
}

fn errors(evidence: &Evidence, name: &str) -> Result<(f64, f64), String> {
    let input = evidence.input(name)?;
    let (cell, kpoints) = (input.cell_bohr()?, input.kpoints()?);
    let (bands, cutoff) = (input.bands()?, input.cutoff_ha()?);
    let output = evidence.output(name)?;
    let spins = output.eigenvalues_ha()?;
    let sizes = output.basis_sizes()?;
    let [channel] = spins.as_slice() else {
        return Err(format!("expected one spin channel, got {}", spins.len()));
    };
    if channel.len() != kpoints.len() || sizes.len() != kpoints.len() {
        return Err(format!(
            "{} k-points in, {} eigenvalue rows and {} basis sizes out",
            kpoints.len(),
            channel.len(),
            sizes.len()
        ));
    }
    let metric = metric_tensor(cell)?;
    let (mut eigenvalue_error, mut size_error): (f64, f64) = (0.0, 0.0);
    for ((values, &size), &k) in channel.iter().zip(&sizes).zip(&kpoints) {
        let energies = kinetic_energies(&metric, k, cutoff)?;
        let inside = energies
            .iter()
            .filter(|&&e| e <= cutoff * (1.0 + SHELL_MARGIN))
            .count();
        size_error = size_error.max((inside as f64 - size as f64).abs());
        if values.len() != bands {
            return Err(format!("{} bands out, {bands} expected", values.len()));
        }
        for (got, want) in values.iter().zip(&energies) {
            let error = (got - want).abs();
            if !error.is_finite() {
                return Err(format!("eigenvalue {got} is not finite"));
            }
            eigenvalue_error = eigenvalue_error.max(error);
        }
    }
    Ok((eigenvalue_error, size_error))
}

fn kinetic_energies(metric: &[[f64; 3]; 3], k: [f64; 3], cutoff: f64) -> Result<Vec<f64>, String> {
    let mut energies = Vec::new();
    let mut edge = f64::INFINITY;
    for i in -BOX..=BOX {
        for j in -BOX..=BOX {
            for l in -BOX..=BOX {
                let q = [
                    f64::from(i) + k[0],
                    f64::from(j) + k[1],
                    f64::from(l) + k[2],
                ];
                let e: f64 = (0..3)
                    .flat_map(|r| (0..3).map(move |c| (r, c)))
                    .map(|(r, c)| 0.5 * q[r] * metric[r][c] * q[c])
                    .sum();
                if [i, j, l].iter().any(|n| n.abs() == BOX) {
                    edge = edge.min(e);
                }
                energies.push(e);
            }
        }
    }
    if edge <= cutoff * (1.0 + SHELL_MARGIN) {
        return Err(format!(
            "the Miller box ±{BOX} is too small for cutoff {cutoff} Ha"
        ));
    }
    energies.sort_by(f64::total_cmp);
    Ok(energies)
}

fn metric_tensor(a: [[f64; 3]; 3]) -> Result<[[f64; 3]; 3], String> {
    let gram: [[f64; 3]; 3] =
        std::array::from_fn(|r| std::array::from_fn(|c| (0..3).map(|d| a[r][d] * a[c][d]).sum()));
    let mut m: [[f64; 6]; 3] = std::array::from_fn(|r| {
        std::array::from_fn(|c| {
            if c < 3 {
                gram[r][c]
            } else {
                f64::from(u8::from(c - 3 == r))
            }
        })
    });
    for col in 0..3 {
        let pivot = (col..3)
            .max_by(|&x, &y| m[x][col].abs().total_cmp(&m[y][col].abs()))
            .ok_or("empty matrix")?;
        m.swap(col, pivot);
        let p = m[col][col];
        if p.abs() < 1e-300 {
            return Err("singular cell".into());
        }
        m[col].iter_mut().for_each(|x| *x /= p);
        for row in (0..3).filter(|&r| r != col) {
            let f = m[row][col];
            let pivot_row = m[col];
            m[row]
                .iter_mut()
                .zip(pivot_row)
                .for_each(|(x, y)| *x -= f * y);
        }
    }
    let tau2 = std::f64::consts::TAU * std::f64::consts::TAU;
    Ok(std::array::from_fn(|r| {
        std::array::from_fn(|c| tau2 * m[r][c + 3])
    }))
}
