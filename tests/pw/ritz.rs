use super::{Parts, bits, entry};
use orbitar::model::Cell;
use orbitar::pw::{
    Basis, Guess, Mat, MatRef, Pencil, c64, random, ritz_start, ritz_start_paired, times,
};
use rayon::ThreadPoolBuilder;

fn diagonal<T: Parts>(x: MatRef<'_, T>) -> Mat<T> {
    Mat::from_fn(x.nrows(), x.ncols(), |i, j| x[(i, j)] * (i + 1) as f64)
}

fn twice<T: Parts>(x: MatRef<'_, T>) -> Mat<T> {
    Mat::from_fn(x.nrows(), x.ncols(), |i, j| x[(i, j)] * 2.0)
}

fn mixtures<T: Parts>(n: usize, rows_used: &[usize], columns: usize) -> Mat<T> {
    let mixing = random::<T>(rows_used.len(), columns, 7);
    Mat::from_fn(n, columns, |i, j| {
        match rows_used.iter().position(|&r| r == i) {
            Some(k) => mixing[(k, j)],
            None => entry::<T>(0.0, 0.0),
        }
    })
}

fn images<T: Parts>(guess: Guess<T>) -> [Mat<T>; 3] {
    match guess {
        Guess::Images(a) => [a.x(), a.hx(), a.sx()].map(|m| m.to_owned()),
        Guess::Vectors(_) => panic!("a start without images"),
    }
}

fn distance<T: Parts>(a: MatRef<'_, T>, b: MatRef<'_, T>) -> f64 {
    (0..a.ncols())
        .flat_map(|j| (0..a.nrows()).map(move |i| (a[(i, j)] - b[(i, j)]).abs()))
        .fold(0.0, f64::max)
}

#[test]
fn the_guess_holds_the_lowest_ritz_vectors_of_the_span_with_their_images() {
    the_guess_holds_the_lowest_ritz_vectors_of_the_span_with_their_images_in::<c64>();
    the_guess_holds_the_lowest_ritz_vectors_of_the_span_with_their_images_in::<f64>();
}

fn the_guess_holds_the_lowest_ritz_vectors_of_the_span_with_their_images_in<T: Parts>() {
    let kinetic = vec![0.0; 40];
    let pencil = Pencil {
        h: &diagonal::<T>,
        s: &twice::<T>,
        kinetic_ha: &kinetic,
    };
    let block = mixtures::<T>(40, &[9, 3, 1, 6, 4], 5);
    let [x, hx, sx] = images(ritz_start(&pencil, block.as_ref(), 3, 11));
    assert_eq!((x.nrows(), x.ncols()), (40, 3));
    for (j, unit) in [1, 3, 4].into_iter().enumerate() {
        let norm: f64 = x.col(j).iter().map(|z| z.abs().powi(2)).sum();
        assert!(
            (x[(unit, j)].abs().powi(2) / norm - 1.0).abs() < 1e-12,
            "column {j}"
        );
        assert!((2.0 * norm - 1.0).abs() < 1e-12, "S-normalized: 2 ‖x‖² = 1");
    }
    assert!(distance(hx.as_ref(), diagonal(x.as_ref()).as_ref()) < 1e-12);
    assert!(distance(sx.as_ref(), twice(x.as_ref()).as_ref()) < 1e-12);
}

#[test]
fn fewer_orbitals_than_bands_fill_the_rest_with_the_seeded_random_block() {
    fewer_orbitals_than_bands_fill_the_rest_with_the_seeded_random_block_in::<c64>();
    fewer_orbitals_than_bands_fill_the_rest_with_the_seeded_random_block_in::<f64>();
}

fn fewer_orbitals_than_bands_fill_the_rest_with_the_seeded_random_block_in<T: Parts>() {
    let kinetic = vec![0.0; 30];
    let pencil = Pencil {
        h: &diagonal::<T>,
        s: &twice::<T>,
        kinetic_ha: &kinetic,
    };
    let block = mixtures::<T>(30, &[5, 2], 2);
    let [x, hx, sx] = images(ritz_start(&pencil, block.as_ref(), 6, 13));
    for (j, unit) in [2, 5].into_iter().enumerate() {
        let norm: f64 = x.col(j).iter().map(|z| z.abs().powi(2)).sum();
        assert!((x[(unit, j)].abs().powi(2) / norm - 1.0).abs() < 1e-12);
    }
    assert_eq!(
        bits(x.subcols(2, 4)),
        bits(random::<T>(30, 8, 13).subcols(4, 4))
    );
    assert!(distance(hx.as_ref(), diagonal(x.as_ref()).as_ref()) < 1e-12);
    assert!(distance(sx.as_ref(), twice(x.as_ref()).as_ref()) < 1e-12);
}

#[test]
fn no_orbitals_or_a_dependent_block_give_the_random_vectors() {
    let kinetic = vec![0.0; 30];
    let pencil = Pencil {
        h: &diagonal::<c64>,
        s: &twice::<c64>,
        kinetic_ha: &kinetic,
    };
    let vectors = |guess: Guess<c64>| match guess {
        Guess::Vectors(x) => bits(x.as_ref()),
        Guess::Images(_) => panic!("images from a block that spans nothing"),
    };
    let empty = Mat::<c64>::zeros(30, 0);
    let want = bits(random::<c64>(30, 4, 17).as_ref());
    assert_eq!(vectors(ritz_start(&pencil, empty.as_ref(), 4, 17)), want);
    let mut dependent = mixtures::<c64>(30, &[1, 2, 3], 3);
    dependent.col_mut(1).fill(c64::ZERO);
    let want = bits(random::<c64>(30, 7, 17).subcols(3, 4));
    assert_eq!(
        vectors(ritz_start(&pencil, dependent.as_ref(), 4, 17)),
        want
    );
}

#[test]
fn the_guess_does_not_depend_on_the_thread_count() {
    let n = 600;
    let a = random::<c64>(n, n, 3);
    let h = Mat::from_fn(n, n, |i, j| (a[(i, j)] + a[(j, i)].conj()) * 0.5);
    let b = random::<c64>(n, 4, 5);
    let s = Mat::<c64>::identity(n, n) + times(b.as_ref(), b.adjoint());
    let kinetic = vec![0.0; n];
    let block = random::<c64>(n, 24, 9);
    let guess = || {
        let (apply_h, apply_s) = (
            |x: MatRef<'_, c64>| times(h.as_ref(), x),
            |x: MatRef<'_, c64>| times(s.as_ref(), x),
        );
        let pencil = Pencil {
            h: &apply_h,
            s: &apply_s,
            kinetic_ha: &kinetic,
        };
        images(ritz_start(&pencil, block.as_ref(), 30, 21)).map(|m| bits(m.as_ref()))
    };
    let pool = ThreadPoolBuilder::new().num_threads(4).build().unwrap();
    assert_eq!(guess(), pool.install(guess));
}

#[test]
fn each_orbital_entry_is_randomized_as_qe_atomic_random() {
    let kinetic = vec![0.0; 50];
    let pencil = Pencil {
        h: &diagonal::<c64>,
        s: &twice::<c64>,
        kinetic_ha: &kinetic,
    };
    let block = random::<c64>(50, 1, 99);
    let [x, _, _] = images(ritz_start(&pencil, block.as_ref(), 1, 23));
    let z = random::<c64>(50, 2, 23);
    let randomized = |g: usize| {
        let (r, phi) = (
            z[(g, 0)].re + 0.5,
            std::f64::consts::TAU * (z[(g, 0)].im + 0.5),
        );
        block[(g, 0)] * (1.0 + c64::new(0.05 * r * phi.cos(), 0.05 * r * phi.sin()))
    };
    let scale = x[(0, 0)] / randomized(0);
    for g in 0..50 {
        assert!((x[(g, 0)] - scale * randomized(g)).norm() < 1e-14, "{g}");
    }
}

#[test]
fn a_real_orbital_is_randomized_by_a_real_factor() {
    let kinetic = vec![0.0; 50];
    let pencil = Pencil {
        h: &diagonal::<f64>,
        s: &twice::<f64>,
        kinetic_ha: &kinetic,
    };
    let block = random::<f64>(50, 1, 99);
    let [x, _, _] = images(ritz_start(&pencil, block.as_ref(), 1, 23));
    let z = random::<f64>(50, 2, 23);
    let randomized = |g: usize| block[(g, 0)] * (1.0 - 0.05 * (z[(g, 0)] + 0.5));
    let scale = x[(0, 0)] / randomized(0);
    for g in 0..50 {
        assert!((x[(g, 0)] - scale * randomized(g)).abs() < 1e-14, "{g}");
    }
}

#[test]
fn no_orbitals_give_the_real_random_vectors_on_the_real_form() {
    let cell = Cell::try_from([[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]).unwrap();
    let basis = Basis::sphere(&cell, [0.0; 3], 2.0);
    let pairs = basis.pairs([0.0; 3]).unwrap();
    let rows = basis.millers().len();
    let kinetic = vec![0.0; rows];
    let pencil = Pencil {
        h: &diagonal::<f64>,
        s: &twice::<f64>,
        kinetic_ha: &kinetic,
    };
    let empty = Mat::<c64>::zeros(rows, 0);
    let Guess::Vectors(x) = ritz_start_paired(&pencil, empty.as_ref(), &pairs, 4, 17) else {
        panic!("images from a block that spans nothing");
    };
    assert_eq!(bits(x.as_ref()), bits(random::<f64>(rows, 4, 17).as_ref()));
}
