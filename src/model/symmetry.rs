use super::Cell;
use moyo::MoyoDataset;
use moyo::base::{Cell as Structure, Lattice, Position};
use moyo::utils::to_3x3_slice;

const TOLERANCE_BOHR: f64 = 1e-5;
const WHOLE: f64 = 1e-6;

#[derive(Clone, Debug, PartialEq)]
pub struct Operation {
    pub rotation: [[i32; 3]; 3],
    pub translation: [f64; 3],
    pub cartesian: [[f64; 3]; 3],
    pub preimages: Vec<usize>,
}

pub fn operations(
    cell: &Cell,
    atoms: &[(u8, [f64; 3])],
    moments: Option<&[f64]>,
) -> Result<Vec<Operation>, String> {
    let lattice = Lattice::from_basis(
        [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]].map(|e| cell.cartesian(e)),
    );
    let kind = |i: usize| (atoms[i].0, moments.map_or(0.0, |m| m[i]));
    let labels = (0..atoms.len())
        .map(|i| (0..i).find(|&j| kind(j) == kind(i)).unwrap_or(i) as i32)
        .collect();
    let positions = atoms
        .iter()
        .map(|&(_, [a, b, c])| Position::new(a, b, c))
        .collect();
    let structure = Structure::new(lattice.clone(), positions, labels);
    let refused = |cause: String| {
        format!(
            "structure: no space group within {TOLERANCE_BOHR} Bohr ({cause}); kpoints.symmetry = false skips it"
        )
    };
    let dataset = MoyoDataset::with_default(&structure, TOLERANCE_BOHR)
        .map_err(|e| refused(e.to_string()))?;
    if dataset.symprec > TOLERANCE_BOHR {
        return Err(refused(format!("moyo needed {} Bohr", dataset.symprec)));
    }
    let operations = (dataset.operations.iter())
        .map(|o| {
            let (rotation, translation) = (o.rotation_as_array(), o.translation_as_array());
            let image = |x: [f64; 3]| -> [f64; 3] {
                std::array::from_fn(|i| {
                    (0..3)
                        .map(|j| f64::from(rotation[i][j]) * x[j])
                        .sum::<f64>()
                        + translation[i]
                })
            };
            let distance = |a: [f64; 3], b: [f64; 3]| {
                let d = cell.cartesian(std::array::from_fn(|i| {
                    (a[i] - b[i]) - (a[i] - b[i]).round()
                }));
                d.iter().map(|x| x * x).sum::<f64>()
            };
            let preimages = (atoms.iter())
                .map(|&(_, a)| {
                    (0..atoms.len())
                        .min_by(|&p, &q| {
                            distance(image(atoms[p].1), a)
                                .total_cmp(&distance(image(atoms[q].1), a))
                        })
                        .expect("one candidate per atom")
                })
                .collect();
            Operation {
                rotation,
                translation,
                cartesian: to_3x3_slice(&o.cartesian_rotation(&lattice)),
                preimages,
            }
        })
        .collect();
    Ok(operations)
}

impl Operation {
    pub fn isometry(&self) -> [[f64; 3]; 3] {
        let mut x = self.cartesian;
        for _ in 0..8 {
            let inverse = inverse_transpose(x);
            let next: [[f64; 3]; 3] =
                std::array::from_fn(|i| std::array::from_fn(|j| 0.5 * (x[i][j] + inverse[i][j])));
            let change = (0..3)
                .flat_map(|i| (0..3).map(move |j| (i, j)))
                .map(|(i, j)| (next[i][j] - x[i][j]).abs())
                .fold(0.0, f64::max);
            x = next;
            if change <= 1e-15 {
                break;
            }
        }
        x
    }

    pub fn keeps_grid(&self, dims: [usize; 3]) -> bool {
        let rotation = (0..3).all(|i| {
            (0..3).all(|j| (i64::from(self.rotation[i][j]) * dims[i] as i64) % dims[j] as i64 == 0)
        });
        let translation = (0..3).all(|i| {
            let steps = self.translation[i] * dims[i] as f64;
            (steps - steps.round()).abs() < WHOLE
        });
        rotation && translation
    }
}

fn inverse_transpose(m: [[f64; 3]; 3]) -> [[f64; 3]; 3] {
    let cofactor = |i: usize, j: usize| {
        let (r0, r1) = ((i + 1) % 3, (i + 2) % 3);
        let (c0, c1) = ((j + 1) % 3, (j + 2) % 3);
        m[r0][c0] * m[r1][c1] - m[r0][c1] * m[r1][c0]
    };
    let det = (0..3).map(|j| m[0][j] * cofactor(0, j)).sum::<f64>();
    std::array::from_fn(|i| std::array::from_fn(|j| cofactor(i, j) / det))
}
