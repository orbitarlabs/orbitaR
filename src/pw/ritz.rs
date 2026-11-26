use super::basis::Pairs;
use super::eigensolver::{Applied, Guess, Pencil, Scalar, dense, hermitian_part, random};
use super::parallel::times;
use faer::{Mat, MatRef, c64};
use std::f64::consts::TAU;

const ATOMIC_ORBITAL_NOISE_AMPLITUDE: f64 = 0.05;

pub fn ritz_start<T: Scalar>(
    pencil: &Pencil<T>,
    block: MatRef<'_, T>,
    columns: usize,
    seed: u64,
) -> Guess<T> {
    let (rows, orbitals) = (block.nrows(), block.ncols());
    let fresh = random::<T>(rows, orbitals + columns, seed);
    let block = randomized(block, fresh.subcols(0, orbitals));
    span(pencil, block, fresh.subcols(orbitals, columns))
}

pub fn ritz_start_paired(
    pencil: &Pencil<f64>,
    block: MatRef<'_, c64>,
    pairs: &Pairs,
    columns: usize,
    seed: u64,
) -> Guess<f64> {
    let (rows, orbitals) = (block.nrows(), block.ncols());
    let block = randomized(block, random::<c64>(rows, orbitals, seed).as_ref());
    let fresh = random::<f64>(rows, orbitals + columns, seed);
    span(
        pencil,
        pairs.real(block.as_ref()),
        fresh.subcols(orbitals, columns),
    )
}

fn randomized<T: Scalar>(block: MatRef<'_, T>, fresh: MatRef<'_, T>) -> Mat<T> {
    Mat::from_fn(block.nrows(), block.ncols(), |g, j| {
        let z = fresh[(g, j)];
        let (radius, phase) = (
            ATOMIC_ORBITAL_NOISE_AMPLITUDE * (z.re() + 0.5),
            TAU * (z.im() + 0.5),
        );
        let mut parts = [1.0 + radius * phase.cos(), radius * phase.sin()].into_iter();
        block[(g, j)] * T::draw(|| parts.next().expect("two parts"))
    })
}

fn span<T: Scalar>(pencil: &Pencil<T>, block: Mat<T>, fill: MatRef<'_, T>) -> Guess<T> {
    let (orbitals, columns) = (block.ncols(), fill.ncols());
    let fallback = || Guess::Vectors(fill.to_owned());
    if orbitals == 0 {
        return fallback();
    }
    let block = Applied::of(pencil, block);
    let small = |image: MatRef<'_, T>| hermitian_part(times(block.x().adjoint(), image));
    let Some((_, c)) = dense(small(block.hx()).as_ref(), small(block.sx()).as_ref()) else {
        return fallback();
    };
    let kept = columns.min(orbitals);
    let fill = Applied::of(pencil, fill.subcols(kept, columns - kept).to_owned());
    Guess::Images(block.combined(c.subcols(0, kept)).beside(fill))
}
