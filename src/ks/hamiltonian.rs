use super::Local;
use crate::pw::{
    Basis, Complex64, Grid, Mat, MatRef, Pairs, SphereFft, accumulate, c64, per_column,
    per_column_pair, per_point, times,
};

const REAL_FUNCTION: f64 = 1e-12;

#[derive(Clone, Copy, Debug)]
pub struct Nonlocal<'a> {
    pub projectors: MatRef<'a, c64>,
    pub d: MatRef<'a, f64>,
    pub q: MatRef<'a, f64>,
}

pub struct Hamiltonian<'a> {
    kinetic: &'a [f64],
    wavevectors: &'a [[f64; 3]],
    fft: SphereFft,
    local: &'a Local,
    atoms: Vec<Projected<'a>>,
}

struct Scratch {
    grid: Vec<Complex64>,
    sums: Vec<Complex64>,
}

struct Projected<'a> {
    projectors: MatRef<'a, c64>,
    d: Mat<c64>,
    q: Mat<c64>,
}

impl<'a> Hamiltonian<'a> {
    pub fn assembled(
        basis: &'a Basis,
        grid: &Grid,
        local: &'a Local,
        atoms: &[Nonlocal<'a>],
    ) -> Result<Self, String> {
        let fft = grid.sphere(basis.millers())?;
        if local.v().len() != grid.dims().iter().product::<usize>() {
            return Err(format!(
                "the potential has {} values for a {:?} grid",
                local.v().len(),
                grid.dims()
            ));
        }
        let complex = |m: MatRef<'_, f64>| {
            Mat::from_fn(m.nrows(), m.ncols(), |i, j| c64::new(m[(i, j)], 0.0))
        };
        let atoms = (atoms.iter().enumerate())
            .map(|(a, n)| {
                let k = n.projectors.ncols();
                let square = |m: MatRef<'_, f64>| m.nrows() == k && m.ncols() == k;
                if n.projectors.nrows() != basis.millers().len() || !square(n.d) || !square(n.q) {
                    return Err(format!(
                        "atom {a}: {}×{} projectors on {} plane waves with a {}×{} D and a {}×{} q",
                        n.projectors.nrows(),
                        k,
                        basis.millers().len(),
                        n.d.nrows(),
                        n.d.ncols(),
                        n.q.nrows(),
                        n.q.ncols()
                    ));
                }
                Ok(Projected {
                    projectors: n.projectors,
                    d: complex(n.d),
                    q: complex(n.q),
                })
            })
            .collect::<Result<_, _>>()?;
        Ok(Self {
            kinetic: basis.kinetic_ha(),
            wavevectors: basis.wavevectors(),
            fft,
            local,
            atoms,
        })
    }

    pub fn h(&self, x: MatRef<'_, c64>) -> Mat<c64> {
        let mut out = self.nonlocal(x, |a| &a.d);
        per_column(
            out.as_mut(),
            || self.scratch(),
            |scratch, j, mut out| {
                self.apply_local(
                    |g| x[(g, j)],
                    scratch,
                    |g, v| out[g] += v + x[(g, j)] * self.kinetic[g],
                );
            },
        );
        out
    }

    fn scratch(&self) -> Scratch {
        Scratch {
            grid: vec![Complex64::default(); self.local.v().len()],
            sums: Vec::new(),
        }
    }

    fn apply_local(
        &self,
        c: impl Fn(usize) -> c64,
        scratch: &mut Scratch,
        mut add: impl FnMut(usize, c64),
    ) {
        let count = self.kinetic.len();
        let grid = &mut scratch.grid;
        self.fft.inverse((0..count).map(&c), grid);
        grid.iter_mut()
            .zip(self.local.v())
            .for_each(|(u, v)| *u *= v);
        let Some(v_tau) = self.local.v_tau() else {
            for (g, v) in self.fft.forward(grid).enumerate() {
                add(g, v);
            }
            return;
        };
        let sums = &mut scratch.sums;
        sums.clear();
        sums.extend(self.fft.forward(grid));
        for axis in 0..3 {
            let k = |g: usize| self.wavevectors[g][axis];
            self.fft.inverse((0..count).map(|g| c(g) * k(g)), grid);
            grid.iter_mut().zip(v_tau).for_each(|(u, v)| *u *= v);
            for (g, w) in self.fft.forward(grid).enumerate() {
                sums[g] += w * (0.5 * k(g));
            }
        }
        for (g, v) in sums.iter().enumerate() {
            add(g, *v);
        }
    }

    pub fn s(&self, x: MatRef<'_, c64>) -> Mat<c64> {
        let mut out = self.nonlocal(x, |a| &a.q);
        per_column(
            out.as_mut(),
            || (),
            |(), j, mut out| {
                for g in 0..x.nrows() {
                    out[g] += x[(g, j)];
                }
            },
        );
        out
    }

    pub fn paired(&'a self, pairs: &'a Pairs) -> Result<Paired<'a>, String> {
        let (mut atoms, mut first) = (Vec::with_capacity(self.atoms.len()), 0);
        let columns: usize = self.atoms.iter().map(|a| a.projectors.ncols()).sum();
        let mut projectors = Mat::zeros(self.kinetic.len(), columns);
        for (a, atom) in self.atoms.iter().enumerate() {
            let p = atom.projectors;
            let mirrored = pairs.complex(pairs.real(p).as_ref());
            for j in 0..p.ncols() {
                let largest = p.col(j).norm_max();
                let gap = (p.col(j) - mirrored.col(j)).norm_max();
                if gap > REAL_FUNCTION * largest {
                    return Err(format!(
                        "atom {a}: projector {j} differs from its mirror image by {gap:e} of {largest:e}"
                    ));
                }
            }
            projectors
                .subcols_mut(first, p.ncols())
                .copy_from(pairs.real(p));
            let real = |m: &Mat<c64>| Mat::from_fn(m.nrows(), m.ncols(), |i, j| m[(i, j)].re);
            atoms.push((first, real(&atom.d), real(&atom.q)));
            first += p.ncols();
        }
        Ok(Paired {
            whole: self,
            pairs,
            projectors,
            atoms,
        })
    }

    fn nonlocal(
        &self,
        x: MatRef<'_, c64>,
        pick: impl for<'b> Fn(&'b Projected<'a>) -> &'b Mat<c64> + Sync,
    ) -> Mat<c64> {
        let coefficients = per_point(self.atoms.len(), |a| {
            let atom = &self.atoms[a];
            pick(atom) * (atom.projectors.adjoint() * x)
        });
        let terms: Vec<_> = (self.atoms.iter().zip(&coefficients))
            .map(|(atom, c)| (atom.projectors, c.as_ref()))
            .collect();
        let mut out = Mat::zeros(x.nrows(), x.ncols());
        accumulate(out.as_mut(), &terms);
        out
    }
}

pub struct Paired<'a> {
    whole: &'a Hamiltonian<'a>,
    pairs: &'a Pairs,
    projectors: Mat<f64>,
    atoms: Vec<(usize, Mat<f64>, Mat<f64>)>,
}

impl Paired<'_> {
    pub fn h(&self, x: MatRef<'_, f64>) -> Mat<f64> {
        let (whole, pairs) = (self.whole, self.pairs);
        let mut out = self.nonlocal(x, |(_, d, _)| d);
        let column = |j: usize| move |row: usize| x[(row, j)];
        let scaled =
            |j: usize, exponent: i32| move |row: usize| libm::scalbn(x[(row, j)], -exponent);
        per_column_pair(
            out.as_mut(),
            || (whole.scratch(), Vec::new()),
            |(scratch, applied), first, mut columns| {
                let kinetic = |g: usize, j: usize| x[(g, first + j)] * whole.kinetic[g];
                let peaks: Vec<f64> = (0..columns.ncols())
                    .map(|j| x.col(first + j).norm_max())
                    .collect();
                if columns.ncols() == 1 || peaks.contains(&0.0) {
                    for j in 0..columns.ncols() {
                        whole.apply_local(
                            |g| pairs.coefficient(column(first + j), g),
                            scratch,
                            |g, v| columns[(g, j)] += pairs.entry(g, v) + kinetic(g, j),
                        );
                    }
                    return;
                }
                let [ea, eb] = [peaks[0], peaks[1]].map(|peak| libm::frexp(peak).1);
                applied.clear();
                whole.apply_local(
                    |g| pairs.packed(scaled(first, ea), scaled(first + 1, eb), g),
                    scratch,
                    |_, v| applied.push(v),
                );
                for g in 0..applied.len() {
                    let [a, b] = pairs.halves(applied, g);
                    columns[(g, 0)] += libm::scalbn(pairs.entry(g, a), ea) + kinetic(g, 0);
                    columns[(g, 1)] += libm::scalbn(pairs.entry(g, b), eb) + kinetic(g, 1);
                }
            },
        );
        out
    }

    pub fn s(&self, x: MatRef<'_, f64>) -> Mat<f64> {
        let mut out = self.nonlocal(x, |(_, _, q)| q);
        per_column(
            out.as_mut(),
            || (),
            |(), j, mut out| {
                for g in 0..x.nrows() {
                    out[g] += x[(g, j)];
                }
            },
        );
        out
    }

    fn nonlocal(
        &self,
        x: MatRef<'_, f64>,
        pick: impl Fn(&(usize, Mat<f64>, Mat<f64>)) -> &Mat<f64>,
    ) -> Mat<f64> {
        let overlaps: Mat<f64> = times(self.projectors.transpose(), x);
        let mut weighted = Mat::zeros(overlaps.nrows(), overlaps.ncols());
        for atom in &self.atoms {
            let m = pick(atom);
            weighted
                .subrows_mut(atom.0, m.nrows())
                .copy_from(m * overlaps.subrows(atom.0, m.nrows()));
        }
        times(self.projectors.as_ref(), weighted.as_ref())
    }
}
