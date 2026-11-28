use super::parallel::{per_column, per_point, product, times};
use faer::linalg::triangular_solve::{
    solve_lower_triangular_in_place, solve_upper_triangular_in_place,
};
use faer::traits::ComplexField;
use faer::traits::math_utils::{abs2, conj, real, zero};
use faer::{Mat, MatMut, MatRef, Par, Side, c64};
use rustfft::num_complex::ComplexFloat;
use std::ops::{DivAssign, Mul, MulAssign};

pub trait Scalar:
    ComplexField<Real = f64>
    + ComplexFloat<Real = f64>
    + Mul<f64, Output = Self>
    + MulAssign<f64>
    + DivAssign<f64>
{
    fn draw(next: impl FnMut() -> f64) -> Self;
}

impl Scalar for c64 {
    fn draw(mut next: impl FnMut() -> f64) -> Self {
        c64::new(next(), next())
    }
}

impl Scalar for f64 {
    fn draw(mut next: impl FnMut() -> f64) -> Self {
        next()
    }
}

pub struct Pencil<'a, T> {
    pub h: &'a dyn Fn(MatRef<'_, T>) -> Mat<T>,
    pub s: &'a dyn Fn(MatRef<'_, T>) -> Mat<T>,
    pub kinetic_ha: &'a [f64],
}

#[derive(Clone, Debug)]
pub struct Eigenpairs<T> {
    values: Vec<f64>,
    vectors: Mat<T>,
    residuals: Vec<f64>,
}

impl<T> Eigenpairs<T> {
    pub fn values(&self) -> &[f64] {
        &self.values
    }

    pub fn vectors(&self) -> MatRef<'_, T> {
        self.vectors.as_ref()
    }

    pub fn residuals(&self) -> &[f64] {
        &self.residuals
    }

    pub fn converged_below(&self, bands: usize, residual: f64) -> bool {
        self.residuals[..bands].iter().all(|&r| r < residual)
    }

    pub fn into_parts(self) -> (Vec<f64>, Mat<T>) {
        (self.values, self.vectors)
    }
}

pub struct Workspace<T> {
    y: [Mat<T>; 3],
    ritz: [Mat<T>; 2],
    p: Mat<T>,
}

impl<T: Scalar> Default for Workspace<T> {
    fn default() -> Self {
        Self {
            y: std::array::from_fn(|_| Mat::new()),
            ritz: std::array::from_fn(|_| Mat::new()),
            p: Mat::new(),
        }
    }
}

pub enum Guess<T> {
    Vectors(Mat<T>),
    Images(Applied<T>),
}

impl<T> From<Mat<T>> for Guess<T> {
    fn from(x: Mat<T>) -> Self {
        Self::Vectors(x)
    }
}

pub struct Applied<T> {
    x: Mat<T>,
    hx: Mat<T>,
    sx: Mat<T>,
}

impl<T: Scalar> Applied<T> {
    pub fn of(pencil: &Pencil<T>, x: Mat<T>) -> Self {
        let (hx, sx) = ((pencil.h)(x.as_ref()), (pencil.s)(x.as_ref()));
        Self { x, hx, sx }
    }

    pub fn combined(&self, c: MatRef<'_, T>) -> Self {
        let [x, hx, sx] = [&self.x, &self.hx, &self.sx].map(|image| {
            let mut out = Mat::zeros(image.nrows(), c.ncols());
            product(out.as_mut(), image.as_ref(), c);
            out
        });
        Self { x, hx, sx }
    }

    pub fn beside(self, other: Self) -> Self {
        let join = |a: Mat<T>, b: Mat<T>| {
            let mut out = Mat::zeros(a.nrows(), a.ncols() + b.ncols());
            out.subcols_mut(0, a.ncols()).copy_from(&a);
            out.subcols_mut(a.ncols(), b.ncols()).copy_from(&b);
            out
        };
        Self {
            x: join(self.x, other.x),
            hx: join(self.hx, other.hx),
            sx: join(self.sx, other.sx),
        }
    }

    pub fn x(&self) -> MatRef<'_, T> {
        self.x.as_ref()
    }

    pub fn hx(&self) -> MatRef<'_, T> {
        self.hx.as_ref()
    }

    pub fn sx(&self) -> MatRef<'_, T> {
        self.sx.as_ref()
    }
}

pub fn lowest<T: Scalar>(
    pencil: &Pencil<T>,
    guess: impl Into<Guess<T>>,
    converged: usize,
    tolerance: f64,
    max_iterations: usize,
    work: &mut Workspace<T>,
) -> Result<Eigenpairs<T>, Eigenpairs<T>> {
    let guess = guess.into();
    let (n, m) = match &guess {
        Guess::Vectors(x) => (x.nrows(), x.ncols()),
        Guess::Images(a) => (a.x.nrows(), a.x.ncols()),
    };
    assert!(n > 3 * m, "LOBPCG needs more than 3 plane waves per band");
    assert!(converged <= m, "converge at most the guessed bands");
    assert_eq!(
        pencil.kinetic_ha.len(),
        n,
        "one kinetic energy per plane wave"
    );
    for block in work.y.iter_mut() {
        block.resize_with(n, 3 * m, |_, _| zero::<T>());
    }
    for block in work.ritz.iter_mut().chain([&mut work.p]) {
        block.resize_with(n, m, |_, _| zero::<T>());
    }
    let Workspace { y, ritz, p } = work;
    let [y0, y1, y2] = y;
    let vectors = match guess {
        Guess::Vectors(x) => Some(x),
        Guess::Images(Applied { x, hx, sx }) => match cholesky(times(x.adjoint(), sx.as_ref())) {
            Some(l) => {
                let inverse = lower_inverse(l.as_ref()).adjoint().to_owned();
                for (target, image) in [(&mut *y0, &x), (&mut *y1, &hx), (&mut *y2, &sx)] {
                    product(target.subcols_mut(0, m), image.as_ref(), inverse.as_ref());
                }
                None
            }
            None => Some(x),
        },
    };
    if let Some(mut x) = vectors {
        ortho(&mut x);
        let sx = (pencil.s)(x.as_ref());
        s_normalize(
            x.as_ref(),
            sx.as_ref(),
            y0.subcols_mut(0, m),
            y2.subcols_mut(0, m),
        );
        drop((x, sx));
        copy(y1.subcols_mut(0, m), (pencil.h)(y0.subcols(0, m)).as_ref());
    }
    let mut values: Vec<f64> = (0..m).map(|j| quotient(y, j)).collect();
    let mut residuals = vec![f64::INFINITY; m];
    let (mut locked, mut iteration) = (0, 0);
    let mut parts = 1;
    loop {
        let active = m - locked;
        let searched = parts > 1;
        let ritz_pairs = searched.then(|| {
            let basis = |k: usize| y[k].subcols(locked, parts * active);
            let (theta, c) = rayleigh_ritz(basis(0), basis(1), active);
            for (k, target) in [1, 2].into_iter().zip(ritz.iter_mut()) {
                product(
                    target.subcols_mut(0, active),
                    basis(k),
                    c.subcols(0, active),
                );
            }
            (theta, c)
        });
        let theta = match &ritz_pairs {
            Some((theta, _)) => theta.clone(),
            None => values[locked..].to_vec(),
        };
        let [hx, sx] = match ritz_pairs {
            Some(_) => [ritz[0].subcols(0, active), ritz[1].subcols(0, active)],
            None => [y[1].subcols(locked, active), y[2].subcols(locked, active)],
        };
        let mut residual = Mat::zeros(n, active);
        per_column(
            residual.as_mut(),
            || (),
            |(), j, mut r| {
                for i in 0..n {
                    r[i] = hx[(i, j)] - sx[(i, j)] * theta[j];
                }
            },
        );
        let norms = per_point(active, |j| residual.col(j).norm_l2());
        residuals[locked..].copy_from_slice(&norms);
        let before = locked;
        if iteration > 0 {
            locked += residuals[locked..]
                .iter()
                .take_while(|&&r| r < tolerance)
                .count();
        }
        let done = locked >= converged || iteration >= max_iterations;
        let newly = locked - before;
        let next = active - newly;
        if let Some((_, c)) = &ritz_pairs {
            let coefficients =
                (!done).then(|| duersch(c.subcols(active, c.ncols() - active), newly, active));
            for (k, slot) in [(1, 0), (2, 1), (0, 0)] {
                let basis = y[k].subcols(before, parts * active);
                if k == 0 {
                    product(ritz[0].subcols_mut(0, active), basis, c.subcols(0, active));
                }
                if let Some(coefficients) = &coefficients {
                    product(p.subcols_mut(0, next), basis, coefficients.as_ref());
                }
                copy(
                    y[k].subcols_mut(before, active),
                    ritz[slot].subcols(0, active),
                );
                if coefficients.is_some() {
                    copy(y[k].subcols_mut(m, next), p.subcols(0, next));
                }
            }
        }
        values[before..].copy_from_slice(&theta);
        if searched {
            s_reorthonormalize(y, before, m);
        }
        if done {
            let pairs = sorted(values, y[0].subcols(0, m), residuals);
            return if locked >= converged {
                Ok(pairs)
            } else {
                Err(pairs)
            };
        }
        precondition(
            pencil.kinetic_ha,
            y[0].subcols(before, active),
            &mut residual,
        );
        let r = if newly == 0 {
            residual
        } else {
            residual.subcols(newly, next).to_owned()
        };
        let z = m + if searched { next } else { 0 };
        let r = ortho_against(r, y[0].subcols(0, z), y[2].subcols(0, z));
        let sr = (pencil.s)(r.as_ref());
        if searched {
            for block in y.iter_mut() {
                let (from, to) = block.subcols_mut(m, 2 * next).split_at_col_mut(next);
                copy(to, from.as_ref());
            }
        }
        let [y0, y1, y2] = y;
        s_normalize(
            r.as_ref(),
            sr.as_ref(),
            y0.subcols_mut(m, next),
            y2.subcols_mut(m, next),
        );
        drop((r, sr));
        copy(
            y1.subcols_mut(m, next),
            (pencil.h)(y0.subcols(m, next)).as_ref(),
        );
        parts = if searched { 3 } else { 2 };
        iteration += 1;
    }
}

fn s_reorthonormalize<T: Scalar>(y: &mut [Mat<T>; 3], from: usize, to: usize) {
    let width = to - from;
    let overlap = times(y[0].subcols(0, from).adjoint(), y[2].subcols(from, width));
    let blocks: [Mat<T>; 3] = std::array::from_fn(|k| {
        y[k].subcols(from, width) - times(y[k].subcols(0, from), overlap.as_ref())
    });
    let l = cholesky(times(blocks[0].adjoint(), blocks[2].as_ref()))
        .expect("an S-orthonormal block up to rounding keeps xᴴ S x positive definite");
    let inverse = lower_inverse(l.as_ref()).adjoint().to_owned();
    for (k, block) in blocks.iter().enumerate() {
        product(
            y[k].subcols_mut(from, width),
            block.as_ref(),
            inverse.as_ref(),
        );
    }
}

pub fn dense<T: Scalar>(h: MatRef<'_, T>, s: MatRef<'_, T>) -> Option<(Vec<f64>, Mat<T>)> {
    let llt = s.llt(Side::Lower).ok()?;
    let l = llt.L();
    let mut m = h.to_owned();
    solve_lower_triangular_in_place(l, m.as_mut(), Par::Seq);
    let mut m = m.adjoint().to_owned();
    solve_lower_triangular_in_place(l, m.as_mut(), Par::Seq);
    let evd = m.self_adjoint_eigen(Side::Lower).ok()?;
    let mut x = evd.U().to_owned();
    solve_upper_triangular_in_place(l.adjoint(), x.as_mut(), Par::Seq);
    Some(((0..h.nrows()).map(|i| real(&evd.S()[i])).collect(), x))
}

fn rayleigh_ritz<T: Scalar>(
    basis: MatRef<'_, T>,
    h: MatRef<'_, T>,
    active: usize,
) -> (Vec<f64>, Mat<T>) {
    let evd = hermitian_part(times(basis.adjoint(), h))
        .self_adjoint_eigen(Side::Lower)
        .expect("a Hermitian eigensolver converges");
    let theta = (0..active).map(|i| real(&evd.S()[i])).collect();
    (theta, evd.U().to_owned())
}

fn duersch<T: Scalar>(perp: MatRef<'_, T>, newly: usize, active: usize) -> Mat<T> {
    let mut overlap = Mat::from_fn(perp.ncols(), active - newly, |i, j| {
        -conj(&perp[(newly + j, i)])
    });
    ortho(&mut overlap);
    times(perp, overlap.as_ref())
}

pub(super) fn hermitian_part<T: Scalar>(a: Mat<T>) -> Mat<T> {
    Mat::from_fn(a.nrows(), a.ncols(), |i, j| {
        (a[(i, j)] + conj(&a[(j, i)])) * 0.5
    })
}

fn quotient<T: Scalar>(y: &[Mat<T>; 3], j: usize) -> f64 {
    let dot = |a: &Mat<T>| {
        (0..y[0].nrows())
            .map(|i| real(&(conj(&y[0][(i, j)]) * a[(i, j)])))
            .sum::<f64>()
    };
    dot(&y[1]) / dot(&y[2])
}

fn sorted<T: Scalar>(values: Vec<f64>, x: MatRef<'_, T>, residuals: Vec<f64>) -> Eigenpairs<T> {
    let mut order: Vec<usize> = (0..values.len()).collect();
    order.sort_by(|&a, &b| values[a].total_cmp(&values[b]));
    Eigenpairs {
        values: order.iter().map(|&j| values[j]).collect(),
        vectors: Mat::from_fn(x.nrows(), order.len(), |i, j| x[(i, order[j])]),
        residuals: order.iter().map(|&j| residuals[j]).collect(),
    }
}

fn precondition<T: Scalar>(kinetic: &[f64], x: MatRef<'_, T>, r: &mut Mat<T>) {
    per_column(
        r.as_mut(),
        || (),
        |(), j, mut r| {
            let (t, norm) = (0..x.nrows()).fold((0.0, 0.0), |(t, n), i| {
                let w = abs2(&x[(i, j)]);
                (t + kinetic[i] * w, n + w)
            });
            let ek = 1.5 * t / norm;
            for (i, &k) in kinetic.iter().enumerate() {
                let y = k / ek;
                let p = 27.0 + y * (18.0 + y * (12.0 + 8.0 * y));
                r[i] *= p / (p + 16.0 * y.powi(4));
            }
        },
    );
}

fn ortho<T: Scalar>(x: &mut Mat<T>) -> f64 {
    let mut growth = 1.0;
    for _ in 0..10 {
        let Some(l) = cholesky(times(x.adjoint(), x.as_ref())) else {
            break;
        };
        let inverse = lower_inverse(l.as_ref());
        *x = times(x.as_ref(), inverse.adjoint());
        growth *= normest(inverse.as_ref());
        if orthonormal(l.as_ref(), inverse.as_ref()) {
            return growth;
        }
    }
    let svd = x.thin_svd().expect("a thin SVD converges");
    *x = svd.U() * svd.V().adjoint();
    f64::INFINITY
}

fn orthonormal<T: Scalar>(l: MatRef<'_, T>, inverse: MatRef<'_, T>) -> bool {
    normest(l) * normest(inverse) < std::f64::consts::SQRT_2
}

fn ortho_against<T: Scalar>(mut x: Mat<T>, y: MatRef<'_, T>, by: MatRef<'_, T>) -> Mat<T> {
    let tolerance = 2.0 * f64::EPSILON;
    let mut fresh = None;
    let mut refill = |x: &mut Mat<T>, j: usize| {
        let fresh = fresh.get_or_insert_with(|| random::<T>(x.nrows(), x.ncols(), 0));
        let column = fresh.subcols(j, 1);
        x.col_mut(j)
            .copy_from((column - y * (by.adjoint() * column)).col(0));
    };
    let norms = per_point(x.ncols(), |j| x.col(j).norm_l2());
    let unit = |n: f64| n.is_finite() && n > 0.0;
    per_column(
        x.as_mut(),
        || (),
        |(), j, column| {
            if unit(norms[j]) {
                column.iter_mut().for_each(|v| *v /= norms[j]);
            }
        },
    );
    for (j, &n) in norms.iter().enumerate() {
        if !unit(n) {
            refill(&mut x, j);
        }
    }
    for round in 1..=10 {
        let overlap = times(by.adjoint(), x.as_ref());
        let mut projected = times(y, overlap.as_ref());
        per_column(
            projected.as_mut(),
            || (),
            |(), j, mut column| {
                for i in 0..column.nrows() {
                    column[i] = x[(i, j)] - column[i];
                }
            },
        );
        x = projected;
        let norms = per_point(x.ncols(), |j| x.col(j).norm_l2());
        for (j, n) in norms.into_iter().enumerate() {
            if n <= tolerance {
                refill(&mut x, j);
            }
        }
        if unchanged(round, overlap.norm_l2(), tolerance) {
            break;
        }
        if ortho(&mut x) < 2.0 {
            break;
        }
    }
    x
}

fn unchanged(round: usize, overlap: f64, tolerance: f64) -> bool {
    round > 1 && overlap < tolerance
}

const LCG: [u64; 2] = [6_364_136_223_846_793_005, 1_442_695_040_888_963_407];

pub fn random<T: Scalar>(rows: usize, columns: usize, seed: u64) -> Mat<T> {
    let [multiplier, increment] = LCG;
    let mut state = seed;
    let mut next = || {
        state = state.wrapping_mul(multiplier).wrapping_add(increment);
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    let mut block = Mat::zeros(rows, columns);
    for j in 0..columns {
        for i in 0..rows {
            block[(i, j)] = T::draw(&mut next);
        }
    }
    block
}

fn s_normalize<T: Scalar>(
    x: MatRef<'_, T>,
    sx: MatRef<'_, T>,
    to_x: MatMut<'_, T>,
    to_sx: MatMut<'_, T>,
) {
    let gram = times(x.adjoint(), sx);
    let l = cholesky(gram).expect("an orthonormal block keeps xᴴ S x positive definite");
    let inverse = lower_inverse(l.as_ref()).adjoint().to_owned();
    product(to_x, x, inverse.as_ref());
    product(to_sx, sx, inverse.as_ref());
}

fn cholesky<T: Scalar>(gram: Mat<T>) -> Option<Mat<T>> {
    let llt = hermitian_part(gram).llt(Side::Lower).ok()?;
    Some(llt.L().to_owned())
}

fn lower_inverse<T: Scalar>(l: MatRef<'_, T>) -> Mat<T> {
    let mut inverse = Mat::<T>::identity(l.nrows(), l.nrows());
    solve_lower_triangular_in_place(l, inverse.as_mut(), Par::Seq);
    inverse
}

fn normest<T: Scalar>(m: MatRef<'_, T>) -> f64 {
    let n = m.nrows();
    let diagonal = (0..n)
        .map(|i| ComplexFloat::abs(m[(i, i)]))
        .fold(0.0, f64::max);
    let off: f64 = (0..n)
        .flat_map(|i| (0..n).map(move |j| (i, j)))
        .filter(|(i, j)| i != j)
        .map(|(i, j)| abs2(&m[(i, j)]))
        .sum();
    diagonal + off.sqrt()
}

fn copy<T: Scalar>(target: MatMut<'_, T>, source: MatRef<'_, T>) {
    per_column(
        target,
        || (),
        |(), j, mut column| column.copy_from(source.col(j)),
    );
}
