use std::f64::consts::TAU;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cell([[f64; 3]; 3]);

impl TryFrom<[[f64; 3]; 3]> for Cell {
    type Error = String;

    fn try_from(rows: [[f64; 3]; 3]) -> Result<Self, String> {
        if flat(rows) || !rows.iter().flatten().all(|x| x.is_finite()) {
            return Err(
                "structure.cell_bohr: the three lattice vectors must be finite and span a volume"
                    .into(),
            );
        }
        Ok(Self(rows))
    }
}

impl Cell {
    pub fn lengths(&self) -> [f64; 3] {
        self.0.map(length)
    }

    pub fn cartesian(&self, f: [f64; 3]) -> [f64; 3] {
        combine(f, self.0)
    }

    pub fn wavevector(&self, m: [f64; 3]) -> [f64; 3] {
        Self::wavevector_in(self.reciprocal(), m)
    }

    pub fn wavevector_in(reciprocal: [[f64; 3]; 3], m: [f64; 3]) -> [f64; 3] {
        combine(m, reciprocal)
    }

    pub fn translates_within(&self, f: [f64; 3], radius: f64) -> Vec<([f64; 3], f64)> {
        ball(f, radius, self.0, self.reciprocal())
    }

    pub fn wavevectors_within(&self, k: [f64; 3], radius: f64) -> Vec<([f64; 3], f64)> {
        ball(k, radius, self.reciprocal(), self.0)
    }

    pub fn reach(&self, radius: f64) -> [f64; 3] {
        reach(radius, self.reciprocal())
    }

    pub fn volume(&self) -> f64 {
        signed_volume(self.0).abs()
    }

    pub fn upright(&self) -> Cell {
        let [a1, a2, a3] = self.0;
        let e1 = a1.map(|x| x / length(a1));
        let along = dot(a2, e1);
        let normal = [0, 1, 2].map(|i| a2[i] - along * e1[i]);
        let e2 = normal.map(|x| x / length(normal));
        let e3 = cross(e1, e2);
        Self([
            [length(a1), 0.0, 0.0],
            [along, length(normal), 0.0],
            [dot(a3, e1), dot(a3, e2), dot(a3, e3)],
        ])
    }

    pub fn frame(&self) -> [[f64; 3]; 3] {
        let [a1, a2, _] = self.0;
        let e1 = a1.map(|x| x / length(a1));
        let normal = cross(a1, a2);
        let e3 = normal.map(|x| x / length(normal));
        [e1, cross(e3, e1), e3]
    }

    pub fn reciprocal(&self) -> [[f64; 3]; 3] {
        let [a1, a2, a3] = self.0;
        let scale = TAU / signed_volume(self.0);
        [cross(a2, a3), cross(a3, a1), cross(a1, a2)].map(|v| v.map(|x| x * scale))
    }
}

fn flat(rows: [[f64; 3]; 3]) -> bool {
    let lengths: f64 = rows.map(length).iter().product();
    signed_volume(rows).abs() <= 1e-9 * lengths
}

fn reach(radius: f64, duals: [[f64; 3]; 3]) -> [f64; 3] {
    duals.map(|d| radius * length(d) / TAU)
}

fn ball(
    c: [f64; 3],
    radius: f64,
    rows: [[f64; 3]; 3],
    duals: [[f64; 3]; 3],
) -> Vec<([f64; 3], f64)> {
    let reach = reach(radius, duals);
    let range = |i: usize| (-c[i] - reach[i]).floor() as i64..=(-c[i] + reach[i]).ceil() as i64;
    range(0)
        .flat_map(|n1| range(1).flat_map(move |n2| range(2).map(move |n3| [n1, n2, n3])))
        .map(|n| {
            let p: [f64; 3] = std::array::from_fn(|i| c[i] + n[i] as f64);
            (p, length(combine(p, rows)))
        })
        .filter(|&(_, r)| r <= radius)
        .collect()
}

fn combine(x: [f64; 3], rows: [[f64; 3]; 3]) -> [f64; 3] {
    std::array::from_fn(|c| (0..3).map(|i| x[i] * rows[i][c]).sum())
}

fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn signed_volume([a1, a2, a3]: [[f64; 3]; 3]) -> f64 {
    dot(a1, cross(a2, a3))
}
