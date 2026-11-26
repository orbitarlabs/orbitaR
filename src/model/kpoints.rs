use std::collections::HashMap;

#[derive(Clone, Debug, PartialEq)]
pub struct KPoints {
    points: Vec<[f64; 3]>,
    weights: Vec<f64>,
    reducible: bool,
}

const SAME: f64 = 1e-9;

impl KPoints {
    pub fn mesh(divisions: [usize; 3], shift: [f64; 3]) -> Result<Self, String> {
        if divisions.contains(&0) {
            return Err("kpoints.mesh: each division must be at least 1".into());
        }
        if !shift.iter().all(|s| (0.0..1.0).contains(s)) {
            return Err("kpoints.shift: each entry must lie in [0, 1)".into());
        }
        let [n1, n2, n3] = divisions;
        let points = (0..n1)
            .flat_map(|i| (0..n2).flat_map(move |j| (0..n3).map(move |l| [i, j, l])))
            .map(|n| std::array::from_fn(|a| (n[a] as f64 + shift[a]) / divisions[a] as f64))
            .collect();
        Ok(Self::equal(points, true))
    }

    pub fn list(points: Vec<[f64; 3]>) -> Result<Self, String> {
        if points.is_empty() || !points.iter().flatten().all(|k| (-1.0..=1.0).contains(k)) {
            return Err("kpoints.list: give at least one point, each entry in [-1, 1]".into());
        }
        Ok(Self::equal(points, false))
    }

    fn equal(points: Vec<[f64; 3]>, reducible: bool) -> Self {
        let weights = vec![1.0 / points.len() as f64; points.len()];
        Self {
            points,
            weights,
            reducible,
        }
    }

    pub fn as_given(self) -> Self {
        Self {
            reducible: false,
            ..self
        }
    }

    pub fn reducible(&self) -> bool {
        self.reducible
    }

    pub fn points(&self) -> &[[f64; 3]] {
        &self.points
    }

    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    pub fn keeps(&self, rotation: &[[i32; 3]; 3]) -> bool {
        let index = self.index();
        (self.points.iter()).all(|&k| index.contains_key(&key(turned(rotation, k))))
    }

    pub fn irreducible(&self, rotations: &[[[i32; 3]; 3]]) -> Self {
        let index = self.index();
        let mut seen = vec![false; self.points.len()];
        let (mut points, mut weights) = (Vec::new(), Vec::new());
        for (i, &k) in self.points.iter().enumerate() {
            if seen[i] {
                continue;
            }
            seen[i] = true;
            let mut weight = self.weights[i];
            for r in rotations {
                let q = turned(r, k);
                for image in [q, q.map(|x| -x)] {
                    if let Some(&j) = index.get(&key(image))
                        && !seen[j]
                    {
                        seen[j] = true;
                        weight += self.weights[j];
                    }
                }
            }
            points.push(k);
            weights.push(weight);
        }
        Self {
            points,
            weights,
            reducible: false,
        }
    }

    fn index(&self) -> HashMap<[i64; 3], usize> {
        (self.points.iter().enumerate())
            .map(|(i, &k)| (key(k), i))
            .collect()
    }
}

fn turned(r: &[[i32; 3]; 3], k: [f64; 3]) -> [f64; 3] {
    std::array::from_fn(|i| (0..3).map(|j| f64::from(r[j][i]) * k[j]).sum())
}

fn key(k: [f64; 3]) -> [i64; 3] {
    k.map(|x| {
        let reduced = x - x.floor();
        let steps = (reduced / SAME).round() as i64;
        steps % (1.0 / SAME).round() as i64
    })
}
