use orbitar::ks::{Hamiltonian, Local, Nonlocal};
use orbitar::model::Cell;
use orbitar::pw::{Basis, Grid, Mat, c64, kinetic_density};
use orbitar::xc::Rung;
use std::f64::consts::TAU;

fn plain(v: &[f64]) -> Local {
    Local::of(v.to_vec(), None, Rung::Gga).unwrap()
}

fn cell() -> Cell {
    Cell::try_from([[0.0, 5.1, 5.1], [5.1, 0.0, 5.1], [5.1, 5.1, 0.0]]).unwrap()
}

fn stream(seed: u64) -> impl FnMut() -> f64 {
    let mut state = seed;
    move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    }
}

#[test]
fn without_projectors_a_constant_potential_shifts_the_kinetic_energy() {
    let (cell, k) = (cell(), [0.1, 0.2, -0.3]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![-0.7; grid.dims().iter().product()];
    let potential = plain(&potential);
    let h = Hamiltonian::assembled(&basis, &grid, &potential, &[]).unwrap();
    let x = Mat::from_fn(basis.millers().len(), 2, |g, j| {
        c64::new(g as f64 + 1.0, j as f64)
    });
    let (hx, sx) = (h.h(x.as_ref()), h.s(x.as_ref()));
    for g in 0..x.nrows() {
        for j in 0..2 {
            assert!((hx[(g, j)] - x[(g, j)] * (basis.kinetic_ha()[g] - 0.7)).norm() < 1e-12);
            assert!((sx[(g, j)] - x[(g, j)]).norm() < 1e-15);
        }
    }
}

#[test]
fn a_cosine_potential_couples_plane_waves_one_wavevector_apart() {
    let (cell, k) = (cell(), [0.0; 3]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let grid = Grid::coarse(&cell, 3.0);
    let potential: Vec<f64> = grid
        .points()
        .iter()
        .map(|f| 2.0 * 0.3 * (TAU * f[0]).cos())
        .collect();
    let potential = plain(&potential);
    let h = Hamiltonian::assembled(&basis, &grid, &potential, &[]).unwrap();
    let origin = basis
        .millers()
        .iter()
        .position(|m| *m == [0, 0, 0])
        .unwrap();
    let x = Mat::from_fn(basis.millers().len(), 1, |g, _| {
        c64::new(f64::from(u8::from(g == origin)), 0.0)
    });
    let hx = h.h(x.as_ref());
    for (g, m) in basis.millers().iter().enumerate() {
        let want = if *m == [1, 0, 0] || *m == [-1, 0, 0] {
            0.3
        } else {
            0.0
        };
        assert!((hx[(g, 0)] - c64::new(want, 0.0)).norm() < 1e-13, "{m:?}");
    }
}

struct Atom {
    projectors: Mat<c64>,
    d: Mat<f64>,
    q: Mat<f64>,
}

fn borrowed(atoms: &[Atom]) -> Vec<Nonlocal<'_>> {
    (atoms.iter())
        .map(|a| Nonlocal {
            projectors: a.projectors.as_ref(),
            d: a.d.as_ref(),
            q: a.q.as_ref(),
        })
        .collect()
}

fn two_atoms(basis: &Basis, r: &mut impl FnMut() -> f64) -> Vec<Atom> {
    let npw = basis.millers().len();
    [3, 2]
        .into_iter()
        .map(|k| {
            let mut entry = || r();
            let d = Mat::from_fn(k, k, |i, j| 1.0 + (i + j) as f64 + 0.1 * (i * j) as f64);
            let q = Mat::from_fn(k, k, |i, j| if i == j { 0.5 } else { 0.1 });
            Atom {
                projectors: Mat::from_fn(npw, k, |_, _| c64::new(entry(), entry())),
                d,
                q,
            }
        })
        .collect()
}

fn complex(m: &Mat<f64>) -> Mat<c64> {
    Mat::from_fn(m.nrows(), m.ncols(), |i, j| c64::new(m[(i, j)], 0.0))
}

#[test]
fn each_atom_adds_its_projected_d_to_h_and_its_projected_q_to_s() {
    let (cell, k) = (cell(), [0.25, -0.1, 0.4]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let grid = Grid::coarse(&cell, 3.0);
    let mut r = stream(3);
    let potential: Vec<f64> = (0..grid.points().len()).map(|_| r()).collect();
    let atoms = two_atoms(&basis, &mut r);
    let npw = basis.millers().len();
    let x = Mat::from_fn(npw, 2, |_, _| c64::new(r(), r()));
    let potential = plain(&potential);
    let local = Hamiltonian::assembled(&basis, &grid, &potential, &[]).unwrap();
    let h = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed(&atoms)).unwrap();
    let (mut want_h, mut want_s) = (local.h(x.as_ref()), x.clone());
    for a in &atoms {
        let p = &a.projectors;
        want_h += p * (complex(&a.d) * (p.adjoint() * &x));
        want_s += p * (complex(&a.q) * (p.adjoint() * &x));
    }
    let (hx, sx) = (h.h(x.as_ref()), h.s(x.as_ref()));
    for g in 0..npw {
        for j in 0..2 {
            assert!((hx[(g, j)] - want_h[(g, j)]).norm() < 1e-12);
            assert!((sx[(g, j)] - want_s[(g, j)]).norm() < 1e-12);
        }
    }
    let y = Mat::from_fn(npw, 2, |_, _| c64::new(r(), r()));
    let (yhx, xhy) = (y.adjoint() * h.h(x.as_ref()), x.adjoint() * h.h(y.as_ref()));
    assert!((0..2).all(|i| (0..2).all(|j| (yhx[(i, j)] - xhy[(j, i)].conj()).norm() < 1e-12)));
}

#[test]
fn projectors_d_and_q_of_mismatched_sizes_are_refused() {
    let cell = cell();
    let basis = Basis::sphere(&cell, [0.0; 3], 3.0);
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![0.0; grid.points().len()];
    let mut r = stream(5);
    for spoil in 0..3 {
        let mut atoms = two_atoms(&basis, &mut r);
        match spoil {
            0 => atoms[1].d = Mat::zeros(3, 3),
            1 => atoms[0].q = Mat::zeros(3, 2),
            _ => atoms[1].projectors = Mat::zeros(5, 2),
        }
        let potential = plain(&potential);
        let err = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed(&atoms))
            .err()
            .unwrap();
        assert!(
            err.starts_with(&format!("atom {}:", [1, 0, 1][spoil])),
            "{err}"
        );
    }
    let err = Hamiltonian::assembled(&basis, &grid, &plain(&[0.0; 4]), &[])
        .err()
        .unwrap();
    assert!(err.starts_with("the potential has 4 values"), "{err}");
}

#[test]
fn a_plane_wave_outside_the_grid_is_refused() {
    let cell = cell();
    let basis = Basis::sphere(&cell, [0.0; 3], 20.0);
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![0.0; grid.dims().iter().product()];
    let potential = plain(&potential);
    let err = Hamiltonian::assembled(&basis, &grid, &potential, &[])
        .err()
        .unwrap();
    assert!(err.contains("lies outside the"), "{err}");
}

#[test]
fn the_paired_form_gives_the_real_form_of_h_and_s() {
    for k in [[0.0; 3], [0.5, 0.0, 0.5]] {
        let cell = cell();
        let basis = Basis::sphere(&cell, k, 3.0);
        let pairs = basis.pairs(k).unwrap();
        let grid = Grid::coarse(&cell, 3.0);
        let mut r = stream(11);
        let potential: Vec<f64> = (0..grid.dims().iter().product()).map(|_| r()).collect();
        let n = basis.millers().len();
        let mut atoms = two_atoms(&basis, &mut r);
        for atom in &mut atoms {
            let columns = Mat::from_fn(n, atom.projectors.ncols(), |_, _| r());
            atom.projectors = pairs.complex(columns.as_ref());
        }
        let borrowed = borrowed(&atoms);
        let potential = plain(&potential);
        let whole = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed).unwrap();
        let paired = whole.paired(&pairs).unwrap();
        let x = Mat::from_fn(n, 3, |_, _| r());
        let c = pairs.complex(x.as_ref());
        for (name, got, want) in [
            (
                "h",
                paired.h(x.as_ref()),
                pairs.real(whole.h(c.as_ref()).as_ref()),
            ),
            (
                "s",
                paired.s(x.as_ref()),
                pairs.real(whole.s(c.as_ref()).as_ref()),
            ),
        ] {
            let scale = want.norm_max();
            assert!((&got - &want).norm_max() < 1e-12 * scale, "{k:?} {name}");
        }
        let hc = whole.h(c.as_ref());
        let mirrored = pairs.complex(pairs.real(hc.as_ref()).as_ref());
        assert!(
            (&hc - &mirrored).norm_max() < 1e-12 * hc.norm_max(),
            "{k:?}"
        );
    }
}

#[test]
fn the_paired_h_of_a_block_is_the_paired_h_of_each_column_alone_with_and_without_v_tau() {
    let cell = cell();
    let mut r = stream(37);
    for k in [[0.0; 3], [0.5, 0.0, 0.5]] {
        let basis = Basis::sphere(&cell, k, 3.0);
        let pairs = basis.pairs(k).unwrap();
        let grid = Grid::coarse(&cell, 3.0);
        let n = grid.points().len();
        let v: Vec<f64> = (0..n).map(|_| r()).collect();
        let v_tau: Vec<f64> = (0..n).map(|_| 0.5 + r()).collect();
        let npw = basis.millers().len();
        let mut atoms = two_atoms(&basis, &mut r);
        for atom in &mut atoms {
            let columns = Mat::from_fn(npw, atom.projectors.ncols(), |_, _| r());
            atom.projectors = pairs.complex(columns.as_ref());
        }
        let borrowed = borrowed(&atoms);
        for local in [plain(&v), with_tau(&v, &v_tau)] {
            let whole = Hamiltonian::assembled(&basis, &grid, &local, &borrowed).unwrap();
            let paired = whole.paired(&pairs).unwrap();
            let x = Mat::from_fn(npw, 5, |_, _| r());
            let block = paired.h(x.as_ref());
            for j in 0..x.ncols() {
                let alone = paired.h(x.subcols(j, 1));
                let gap = (block.col(j) - alone.col(0)).norm_max();
                assert!(
                    gap < 1e-14 * alone.norm_max(),
                    "{k:?} column {j}: {gap:e} of {:e}",
                    alone.norm_max()
                );
            }
        }
    }
}

#[test]
fn each_column_of_a_paired_h_keeps_its_accuracy_beside_a_far_larger_or_a_zero_column() {
    let cell = cell();
    let mut r = stream(41);
    for k in [[0.0; 3], [0.5, 0.0, 0.5]] {
        let basis = Basis::sphere(&cell, k, 3.0);
        let pairs = basis.pairs(k).unwrap();
        let grid = Grid::coarse(&cell, 3.0);
        let n = grid.points().len();
        let v: Vec<f64> = (0..n).map(|_| r()).collect();
        let v_tau: Vec<f64> = (0..n).map(|_| 0.5 + r()).collect();
        let local = with_tau(&v, &v_tau);
        let whole = Hamiltonian::assembled(&basis, &grid, &local, &[]).unwrap();
        let paired = whole.paired(&pairs).unwrap();
        let npw = basis.millers().len();
        let small = Mat::from_fn(npw, 1, |_, _| r());
        let alone = paired.h(small.as_ref());
        for scale in [0.0, 1.0, 2f64.powi(20), 2f64.powi(55)] {
            let other = Mat::from_fn(npw, 1, |_, _| scale * r());
            let other_alone = paired.h(other.as_ref());
            for small_first in [true, false] {
                let (j, columns) = match small_first {
                    true => (0, [small.as_ref(), other.as_ref()]),
                    false => (1, [other.as_ref(), small.as_ref()]),
                };
                let x = Mat::from_fn(npw, 2, |g, c| columns[c][(g, 0)]);
                let block = paired.h(x.as_ref());
                let gap = (block.col(j) - alone.col(0)).norm_max();
                assert!(
                    gap <= 1e-14 * alone.norm_max(),
                    "{k:?} beside {scale:e}, small first {small_first}: {gap:e}"
                );
                let other_gap = (block.col(1 - j) - other_alone.col(0)).norm_max();
                assert!(
                    other_gap <= 1e-14 * other_alone.norm_max(),
                    "{k:?} the {scale:e} column, small first {small_first}: {other_gap:e}"
                );
            }
        }
    }
}

#[test]
fn a_paired_h_scales_columns_at_both_ends_of_the_exponent_range_exactly() {
    let (cell, k) = (cell(), [0.0; 3]);
    let basis = Basis::sphere(&cell, k, 0.01);
    assert_eq!(basis.millers(), [[0, 0, 0]]);
    let pairs = basis.pairs(k).unwrap();
    let grid = Grid::coarse(&cell, 0.01);
    let quarter = plain(&vec![0.25; grid.points().len()]);
    let whole = Hamiltonian::assembled(&basis, &grid, &quarter, &[]).unwrap();
    let paired = whole.paired(&pairs).unwrap();
    let (largest, tiniest) = (2f64.powi(1023), f64::from_bits(1_u64 << 49));
    for column in [
        [1.0, largest],
        [largest, 1.0],
        [tiniest, 1.0],
        [1.0, tiniest],
    ] {
        let block = paired.h(Mat::from_fn(1, 2, |_, j| column[j]).as_ref());
        for (j, &value) in column.iter().enumerate() {
            let alone = paired.h(Mat::from_fn(1, 1, |_, _| value).as_ref());
            assert_eq!(block[(0, j)], value / 4.0, "{column:?} column {j}");
            assert_eq!(block[(0, j)], alone[(0, 0)], "{column:?} column {j} alone");
        }
    }
}

#[test]
fn projectors_that_are_no_real_functions_have_no_paired_form() {
    let (cell, k) = (cell(), [0.0; 3]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let pairs = basis.pairs(k).unwrap();
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![0.0; grid.dims().iter().product()];
    let atoms = two_atoms(&basis, &mut stream(5));
    let borrowed = borrowed(&atoms);
    let potential = plain(&potential);
    let whole = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed).unwrap();
    let message = whole.paired(&pairs).err().unwrap();
    assert!(
        message.starts_with("atom 0: projector 0 differs from its mirror image"),
        "{message}"
    );
}

#[test]
fn a_projector_that_vanishes_at_every_plane_wave_has_a_paired_form() {
    let (cell, k) = (cell(), [0.0; 3]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let pairs = basis.pairs(k).unwrap();
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![0.0; grid.dims().iter().product()];
    let mut atoms = two_atoms(&basis, &mut stream(7));
    for atom in &mut atoms {
        atom.projectors = Mat::zeros(basis.millers().len(), atom.projectors.ncols());
    }
    let borrowed = borrowed(&atoms);
    let potential = plain(&potential);
    let whole = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed).unwrap();
    assert!(whole.paired(&pairs).is_ok());
}

#[test]
fn a_projector_a_billionth_off_its_mirror_image_has_no_paired_form() {
    let (cell, k) = (cell(), [0.0; 3]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let pairs = basis.pairs(k).unwrap();
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![0.0; grid.dims().iter().product()];
    let mut r = stream(13);
    let mut atoms = two_atoms(&basis, &mut r);
    for atom in &mut atoms {
        let columns = Mat::from_fn(basis.millers().len(), atom.projectors.ncols(), |_, _| r());
        atom.projectors = pairs.complex(columns.as_ref());
    }
    let largest = atoms[1].projectors.col(0).norm_max();
    atoms[1].projectors[(1, 0)] += c64::new(0.0, 1e-9 * largest);
    let borrowed = borrowed(&atoms);
    let potential = plain(&potential);
    let whole = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed).unwrap();
    let message = whole.paired(&pairs).err().unwrap();
    assert!(
        message.starts_with("atom 1: projector 0 differs from its mirror image"),
        "{message}"
    );
}

#[test]
fn a_projector_off_its_mirror_image_by_rounding_has_a_paired_form() {
    let (cell, k) = (cell(), [0.0; 3]);
    let basis = Basis::sphere(&cell, k, 3.0);
    let pairs = basis.pairs(k).unwrap();
    let grid = Grid::coarse(&cell, 3.0);
    let potential = vec![0.0; grid.dims().iter().product()];
    let mut r = stream(17);
    let mut atoms = two_atoms(&basis, &mut r);
    for atom in &mut atoms {
        let columns = Mat::from_fn(basis.millers().len(), atom.projectors.ncols(), |_, _| {
            1e6 * r()
        });
        atom.projectors = pairs.complex(columns.as_ref());
    }
    let largest = atoms[1].projectors.col(0).norm_max();
    atoms[1].projectors[(1, 0)] += c64::new(0.0, 1e-14 * largest);
    let borrowed = borrowed(&atoms);
    let potential = plain(&potential);
    let whole = Hamiltonian::assembled(&basis, &grid, &potential, &borrowed).unwrap();
    assert!(whole.paired(&pairs).is_ok());
}

fn with_tau(v: &[f64], v_tau: &[f64]) -> Local {
    Local::of(v.to_vec(), Some(v_tau.to_vec()), Rung::MetaGga).unwrap()
}

fn wavevector(cell: &Cell, k: [f64; 3], m: [i32; 3]) -> [f64; 3] {
    let b = cell.reciprocal();
    std::array::from_fn(|a| (0..3).map(|i| (f64::from(m[i]) + k[i]) * b[i][a]).sum())
}

#[test]
fn a_cosine_v_tau_couples_plane_waves_one_wavevector_apart_by_a_quarter_of_their_dot_product() {
    let cell = cell();
    for k in [[0.1, 0.2, -0.3], [0.5, 0.0, 0.5]] {
        let basis = Basis::sphere(&cell, k, 3.0);
        let grid = Grid::coarse(&cell, 3.0);
        let n = grid.points().len();
        let v_tau: Vec<f64> = (grid.points().iter()).map(|f| (TAU * f[0]).cos()).collect();
        let local = with_tau(&vec![0.0; n], &v_tau);
        let h = Hamiltonian::assembled(&basis, &grid, &local, &[]).unwrap();
        let millers = basis.millers();
        let columns: Vec<usize> = (0..millers.len()).step_by(7).collect();
        let x = Mat::from_fn(millers.len(), columns.len(), |g, j| {
            c64::new(f64::from(u8::from(g == columns[j])), 0.0)
        });
        let hx = h.h(x.as_ref());
        for (j, &column) in columns.iter().enumerate() {
            let q = wavevector(&cell, k, millers[column]);
            for (g, m) in millers.iter().enumerate() {
                let d: [i32; 3] = std::array::from_fn(|i| m[i] - millers[column][i]);
                let p = wavevector(&cell, k, *m);
                let dot: f64 = (0..3).map(|a| p[a] * q[a]).sum();
                let want = match (g == column, d == [1, 0, 0] || d == [-1, 0, 0]) {
                    (true, _) => basis.kinetic_ha()[g],
                    (false, true) => 0.25 * dot,
                    (false, false) => 0.0,
                };
                assert!(
                    (hx[(g, j)] - c64::new(want, 0.0)).norm() < 1e-12,
                    "{k:?} {m:?} from {:?}: {} vs {want}",
                    millers[column],
                    hx[(g, j)]
                );
            }
        }
        if let Some(pairs) = basis.pairs(k) {
            let paired = h.paired(&pairs).unwrap();
            let mut r = stream(19);
            let real = Mat::from_fn(millers.len(), 2, |_, _| r());
            let c = pairs.complex(real.as_ref());
            let want = pairs.real(h.h(c.as_ref()).as_ref());
            let got = paired.h(real.as_ref());
            assert!((&got - &want).norm_max() < 1e-12 * want.norm_max(), "{k:?}");
        }
    }
}

fn unit<T: Copy + std::ops::Div<f64, Output = T>>(
    count: usize,
    draw: impl FnMut() -> T,
    norm: impl Fn(&[T]) -> f64,
) -> Vec<T> {
    let v: Vec<T> = std::iter::repeat_with(draw).take(count).collect();
    let length = norm(&v);
    v.into_iter().map(|x| x / length).collect()
}

#[test]
fn with_v_tau_h_is_hermitian_on_normalized_random_vectors_complex_and_paired() {
    let cell = cell();
    let mut r = stream(23);
    for k in [[0.1, 0.2, -0.3], [0.5, 0.0, 0.5]] {
        let basis = Basis::sphere(&cell, k, 3.0);
        let grid = Grid::coarse(&cell, 3.0);
        let n = grid.points().len();
        let v: Vec<f64> = (0..n).map(|_| r()).collect();
        let v_tau: Vec<f64> = (0..n).map(|_| 0.5 + r()).collect();
        let local = with_tau(&v, &v_tau);
        let h = Hamiltonian::assembled(&basis, &grid, &local, &[]).unwrap();
        let npw = basis.millers().len();
        let dense = h.h(Mat::<c64>::identity(npw, npw).as_ref());
        let norm = (0..npw).map(|j| dense.col(j).norm_l2()).fold(0.0, f64::max);
        let length = |v: &[c64]| v.iter().map(|z| z.norm_sqr()).sum::<f64>().sqrt();
        for _ in 0..3 {
            let [x, y] = [0, 1].map(|_| {
                let column = unit(npw, || c64::new(r(), r()), length);
                Mat::from_fn(npw, 1, |g, _| column[g])
            });
            let yhx = (y.adjoint() * h.h(x.as_ref()))[(0, 0)];
            let xhy = (x.adjoint() * h.h(y.as_ref()))[(0, 0)];
            assert!(
                (yhx - xhy.conj()).norm() <= 1e-13 * norm,
                "{k:?}: {yhx} vs {xhy}"
            );
        }
        if let Some(pairs) = basis.pairs(k) {
            let paired = h.paired(&pairs).unwrap();
            let length = |v: &[f64]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
            for _ in 0..3 {
                let [x, y] = [0, 1].map(|_| {
                    let column = unit(npw, &mut r, length);
                    Mat::from_fn(npw, 1, |g, _| column[g])
                });
                let yhx = (y.transpose() * paired.h(x.as_ref()))[(0, 0)];
                let xhy = (x.transpose() * paired.h(y.as_ref()))[(0, 0)];
                assert!(
                    (yhx - xhy).abs() <= 1e-13 * norm,
                    "{k:?} paired: {yhx} vs {xhy}"
                );
            }
        }
    }
}

#[test]
fn a_zero_v_tau_gives_the_gga_h_and_a_nonzero_one_moves_it() {
    let cell = cell();
    let mut r = stream(29);
    for k in [[0.1, 0.2, -0.3], [0.0; 3]] {
        let basis = Basis::sphere(&cell, k, 3.0);
        let grid = Grid::coarse(&cell, 3.0);
        let n = grid.points().len();
        let v: Vec<f64> = (0..n).map(|_| r()).collect();
        let atoms = two_atoms(&basis, &mut r);
        let borrowed = borrowed(&atoms);
        let x = Mat::from_fn(basis.millers().len(), 3, |_, _| c64::new(r(), r()));
        let apply = |local: &Local| {
            let h = Hamiltonian::assembled(&basis, &grid, local, &borrowed).unwrap();
            h.h(x.as_ref())
        };
        let gga = apply(&plain(&v));
        let zero = apply(&with_tau(&v, &vec![0.0; n]));
        assert!((&zero - &gga).norm_max() <= 1e-14 * gga.norm_max(), "{k:?}");
        let moved = apply(&with_tau(&v, &vec![0.1; n]));
        assert!((&moved - &gga).norm_max() > 1e-3 * gga.norm_max(), "{k:?}");
        if let Some(pairs) = basis.pairs(k) {
            let y = Mat::from_fn(basis.millers().len(), 2, |_, _| r());
            let apply = |local: &Local| {
                let h = Hamiltonian::assembled(&basis, &grid, local, &[]).unwrap();
                h.paired(&pairs).unwrap().h(y.as_ref())
            };
            let gga = apply(&plain(&v));
            let zero = apply(&with_tau(&v, &vec![0.0; n]));
            assert!(
                (&zero - &gga).norm_max() <= 1e-14 * gga.norm_max(),
                "{k:?} paired"
            );
        }
    }
}

#[test]
fn the_v_tau_term_is_the_integral_of_v_tau_times_the_band_tau_across_the_two_grids() {
    let unit_cube = Cell::try_from([[TAU, 0.0, 0.0], [0.0, TAU, 0.0], [0.0, 0.0, TAU]]).unwrap();
    let mut r = stream(31);
    for (cell, k, cutoff) in [(unit_cube, [0.0; 3], 2.0), (cell(), [0.1, 0.2, -0.3], 3.0)] {
        let basis = Basis::sphere(&cell, k, cutoff);
        let coarse = Grid::coarse(&cell, cutoff);
        let fine = Grid::fine(2.5 * cutoff, &coarse);
        let millers = coarse.millers();
        let sphere: Vec<usize> = (coarse.density_sphere(cutoff).iter())
            .map(|&(slot, _)| slot)
            .collect();
        let fine_slots: Vec<usize> = (sphere.iter())
            .map(|&s| fine.slot(millers[s]).unwrap())
            .collect();
        let (n, nf) = (coarse.points().len(), fine.points().len());
        let v_fine: Vec<f64> = (0..nf).map(|_| 1.0 + r()).collect();
        let v_tau = coarse.synthesized(
            sphere.iter().copied(),
            fine.coefficients(&v_fine, fine_slots.iter().copied())
                .into_iter(),
        );
        let local = with_tau(&vec![0.0; n], &v_tau);
        let h = Hamiltonian::assembled(&basis, &coarse, &local, &[]).unwrap();
        let npw = basis.millers().len();
        let c: Vec<c64> = (0..npw).map(|_| c64::new(r(), r())).collect();
        let x = Mat::from_fn(npw, 1, |g, _| c[g]);
        let kinetic: f64 = (basis.kinetic_ha().iter().zip(&c))
            .map(|(t, c)| t * c.norm_sqr())
            .sum();
        let expectation = (x.adjoint() * h.h(x.as_ref()))[(0, 0)].re - kinetic;
        let volume = cell.volume();
        let sphere_fft = coarse.sphere(basis.millers()).unwrap();
        let (mut buffer, mut tau) = (vec![c64::default(); n], vec![0.0; n]);
        kinetic_density(
            &sphere_fft,
            &basis,
            |g| c[g],
            1.0 / volume,
            &mut buffer,
            &mut tau,
        );
        let padded = fine.synthesized(
            fine_slots.iter().copied(),
            coarse
                .coefficients(&tau, sphere.iter().copied())
                .into_iter(),
        );
        let integral =
            volume / nf as f64 * v_fine.iter().zip(&padded).map(|(v, t)| v * t).sum::<f64>();
        assert!(
            (expectation - integral).abs() < 1e-12 * integral.abs(),
            "{:?}: {expectation} vs {integral}",
            coarse.dims()
        );
    }
}
