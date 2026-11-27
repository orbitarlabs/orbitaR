use orbitar::model::Cell;
use orbitar::pw::{Basis, Complex64, Grid, kinetic_density, kinetic_strain};
use std::f64::consts::TAU;

fn scaled(rows: [[f64; 3]; 3], scale: [f64; 3]) -> Cell {
    Cell::try_from(std::array::from_fn(|i| rows[i].map(|x| x * scale[i]))).unwrap()
}

fn fcc(a: f64) -> Cell {
    scaled([[0.0, 0.5, 0.5], [0.5, 0.0, 0.5], [0.5, 0.5, 0.0]], [a; 3])
}

#[test]
fn both_boxes_equal_abinits_for_cubic_hexagonal_and_triclinic_cells() {
    let bcc = scaled(
        [[-0.5, 0.5, 0.5], [0.5, -0.5, 0.5], [0.5, 0.5, -0.5]],
        [5.35; 3],
    );
    let s3 = 0.75_f64.sqrt();
    let hcp = scaled(
        [[1.0, 0.0, 0.0], [-0.5, s3, 0.0], [0.0, 0.0, 1.0]],
        [6.0, 6.0, 9.8],
    );
    let triclinic = scaled(
        [[6.1, 0.0, 0.0], [1.9, 5.7, 0.0], [1.3, 2.2, 6.8]],
        [1.0; 3],
    );
    let table = [
        (&fcc(10.26), 10.0, 20.0, [24; 3], [30; 3]),
        (&fcc(10.26), 15.0, 45.0, [27; 3], [45; 3]),
        (&fcc(10.26), 40.0, 80.0, [45; 3], [60; 3]),
        (&fcc(10.26), 5.0, 7.5, [16; 3], [18; 3]),
        (&bcc, 40.0, 80.0, [30; 3], [40; 3]),
        (&bcc, 15.0, 45.0, [18; 3], [30; 3]),
        (&hcp, 17.0, 34.0, [24, 24, 40], [32, 32, 54]),
        (&hcp, 23.0, 46.0, [27, 27, 45], [40, 40, 60]),
        (&triclinic, 7.0, 14.0, [16, 16, 18], [24, 24, 27]),
        (&triclinic, 11.0, 22.0, [20, 18, 24], [27, 27, 32]),
        (&triclinic, 23.0, 46.0, [30, 27, 32], [40, 40, 48]),
        (&triclinic, 16.0, 16.0, [24, 24, 30], [24, 24, 30]),
    ];
    for (cell, ecut, ecutdg, coarse, fine) in table {
        let c = Grid::coarse(cell, ecut);
        assert_eq!(
            (c.dims(), Grid::fine(ecutdg, &c).dims()),
            (coarse, fine),
            "{ecut} / {ecutdg}"
        );
    }
}

#[test]
fn the_integral_of_a_constant_is_the_constant_times_the_volume_to_the_last_bits() {
    let cell = fcc(10.26);
    let grid = Grid::coarse(&cell, 40.0);
    let value = 0.123_456_789;
    let integral = grid.integral(std::iter::repeat_n(value, grid.points().len()));
    let exact = value * cell.volume();
    assert!(
        ((integral - exact) / exact).abs() < 4.0 * f64::EPSILON,
        "{integral} against {exact}"
    );
}

#[test]
fn a_repeat_of_sixty_four_cells_integrates_to_sixty_four_times_one_cell_to_the_last_bits() {
    let cell = fcc(10.26);
    let repeat = scaled(
        [[0.0, 0.5, 0.5], [0.5, 0.0, 0.5], [0.5, 0.5, 0.0]],
        [4.0 * 10.26; 3],
    );
    let (one, many) = (Grid::coarse(&cell, 6.0), Grid::coarse(&repeat, 6.0));
    assert_eq!((one.dims(), many.dims()), ([16; 3], [64; 3]));
    let values: Vec<f64> = (one.points().iter())
        .map(|f| {
            0.05 + 0.02 * (TAU * (f[0] + f[1])).cos()
                + 0.015 * (TAU * (2.0 * f[2] - f[0]) + 0.3).sin()
        })
        .collect();
    let tiled = (0..64 * 64 * 64).map(|i| {
        let (i1, i2, i3) = (i / 4096 % 16, i / 64 % 64 % 16, i % 64 % 16);
        values[(i1 * 16 + i2) * 16 + i3]
    });
    let (cell_integral, repeat_integral) =
        (one.integral(values.iter().copied()), many.integral(tiled));
    let expected = 64.0 * cell_integral;
    assert!(
        ((repeat_integral - expected) / expected).abs() < 8.0 * f64::EPSILON,
        "{repeat_integral} against {expected}"
    );
}

#[test]
fn the_integral_of_values_near_the_largest_float_stays_finite() {
    let cell = scaled(
        [[0.0, 0.5, 0.5], [0.5, 0.0, 0.5], [0.5, 0.5, 0.0]],
        [10.2; 3],
    );
    let grid = Grid::coarse(&cell, 0.01);
    assert_eq!(grid.points().len(), 8);
    let integral = grid.integral(std::iter::repeat_n(1e305, 8));
    let exact = 1e305 * cell.volume();
    assert!(
        ((integral - exact) / exact).abs() < 4.0 * f64::EPSILON,
        "{integral} against {exact}"
    );
}

#[test]
#[should_panic(expected = "one value per grid point")]
fn an_integral_of_another_number_of_values_is_refused() {
    let grid = Grid::coarse(&fcc(10.26), 5.0);
    grid.integral(std::iter::repeat_n(1.0, grid.points().len() - 1));
}

#[test]
fn a_ball_holds_each_grid_point_once_per_lattice_image_within_its_radius() {
    let cell = scaled(
        [[6.1, 0.0, 0.0], [1.9, 5.7, 0.0], [1.3, 2.2, 6.8]],
        [1.0; 3],
    );
    let grid = Grid::coarse(&cell, 7.0);
    let points = grid.points();
    let center = [0.93, -0.21, 0.48];
    let length = |v: [f64; 3]| v.iter().map(|x| x * x).sum::<f64>().sqrt();
    for (radius, wraps) in [(1.0, false), (4.0, true)] {
        let mut want = Vec::new();
        for (index, p) in points.iter().enumerate() {
            for image in 0..7_i32.pow(3) {
                let m = [image / 49 - 3, image / 7 % 7 - 3, image % 7 - 3];
                let r = length(
                    cell.cartesian(std::array::from_fn(|k| p[k] - center[k] + f64::from(m[k]))),
                );
                if r <= radius {
                    want.push((index, r));
                }
            }
        }
        let found = grid.within(center, radius);
        assert!(found.is_sorted_by_key(|&(index, ..)| index));
        assert!(
            found
                .iter()
                .all(|&(_, shift, r)| (length(cell.cartesian(shift)) - r).abs() < 1e-12)
        );
        let mut found: Vec<_> = found.iter().map(|&(index, _, r)| (index, r)).collect();
        for list in [&mut want, &mut found] {
            list.sort_by(|a, b| a.0.cmp(&b.0).then(a.1.total_cmp(&b.1)));
        }
        assert_eq!(found.len(), want.len(), "radius {radius}");
        assert_eq!(found.len() > points.len(), wraps, "radius {radius}");
        for (found, want) in found.iter().zip(&want) {
            assert_eq!(found.0, want.0, "radius {radius}");
            assert!((found.1 - want.1).abs() < 1e-12, "radius {radius}");
        }
    }
}

#[test]
fn every_coefficient_sits_at_the_slot_of_its_miller_index() {
    let grid = Grid::coarse(
        &scaled(
            [[6.1, 0.0, 0.0], [1.9, 5.7, 0.0], [1.3, 2.2, 6.8]],
            [1.0; 3],
        ),
        7.0,
    );
    let [n1, n2, n3] = grid.dims();
    for (i, m) in grid.millers().into_iter().enumerate() {
        assert_eq!(grid.slot(m), Some(i));
        assert!(
            m.iter()
                .zip(grid.dims())
                .all(|(&m, n)| -(n as i32 - 1) / 2 <= m && m <= n as i32 / 2)
        );
    }
    assert_eq!((n1, n2, n3), (16, 16, 18));
    assert_eq!(grid.slot([-8, 0, 0]), None);
    assert_eq!(grid.slot([0, 0, 10]), None);
    assert_eq!(grid.slot([8, -7, 9]), Some((8 * 16 + 9) * 18 + 9));
}

#[test]
fn a_plane_wave_transforms_to_one_coefficient_at_its_wavevector_and_back() {
    let cell = Cell::try_from([[4.0, 0.3, -0.2], [1.1, 5.0, 0.4], [-0.7, 0.9, 6.0]]).unwrap();
    let grid = Grid::coarse(&cell, 2.0);
    let mut values: Vec<Complex64> = grid
        .points()
        .iter()
        .map(|f| Complex64::from_polar(1.0, TAU * (f[0] - 2.0 * f[2])))
        .collect();
    let original = values.clone();
    grid.forward(&mut values);
    let n = values.len() as f64;
    let want = cell.wavevector([1.0, 0.0, -2.0]);
    for (v, g) in values.iter().zip(grid.wavevectors()) {
        let at_peak = g.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-12);
        assert!(
            (v - if at_peak { n } else { 0.0 }).norm() < 1e-9,
            "{g:?}: {v}"
        );
    }
    grid.inverse(&mut values);
    assert!(
        values
            .iter()
            .zip(&original)
            .all(|(a, b)| (a - b).norm() < 1e-14)
    );
}

fn waves(f: &[f64; 3]) -> f64 {
    1.5 + (TAU * (f[0] + 2.0 * f[1])).cos() - 0.7 * (TAU * (3.0 * f[2] - f[0]) + 0.4).cos()
        + 0.2 * (TAU * (-4.0 * f[1] + 2.0 * f[2])).sin()
}

#[test]
fn transfer_between_boxes_is_exact_for_coefficients_both_hold() {
    let cell = fcc(10.26);
    let coarse = Grid::coarse(&cell, 3.0);
    let fine = Grid::fine(9.0, &coarse);
    assert!(fine.dims().iter().zip(coarse.dims()).all(|(f, c)| *f > c));
    let on = |grid: &Grid| grid.points().iter().map(waves).collect::<Vec<f64>>();
    let up = coarse.transfer(&on(&coarse), &fine);
    assert_eq!(up.len(), on(&fine).len());
    assert!(up.iter().zip(on(&fine)).all(|(a, b)| (a - b).abs() < 1e-13));
    let down = fine.transfer(&on(&fine), &coarse);
    assert_eq!(down.len(), on(&coarse).len());
    assert!(
        down.iter()
            .zip(on(&coarse))
            .all(|(a, b)| (a - b).abs() < 1e-13)
    );
}

#[test]
fn transfer_drops_the_nyquist_plane_of_an_even_box() {
    let cell = fcc(10.26);
    let coarse = Grid::coarse(&cell, 3.0);
    let n = coarse.dims()[0];
    assert!(n.is_multiple_of(2));
    let fine = Grid::fine(9.0, &coarse);
    let nyquist: Vec<f64> = coarse
        .points()
        .iter()
        .map(|f| (TAU * (n / 2) as f64 * f[0]).cos())
        .collect();
    assert!(
        coarse
            .transfer(&nyquist, &fine)
            .iter()
            .all(|x| x.abs() < 1e-14)
    );
    let wave: Vec<f64> = fine
        .points()
        .iter()
        .map(|f| (TAU * (n / 2) as f64 * f[0]).cos())
        .collect();
    assert!(
        fine.transfer(&wave, &coarse)
            .iter()
            .all(|x| x.abs() < 1e-13)
    );
}

fn real_noise_on(grid: &Grid, seed: u64) -> Vec<f64> {
    (0..grid.points().len())
        .map(|i| noise(i, seed).re)
        .collect()
}

#[test]
fn a_density_sphere_synthesized_on_the_fine_grid_and_its_adjoint_keep_one_mode_set_nyquist_plane_included()
 {
    let unit = Cell::try_from([[TAU, 0.0, 0.0], [0.0, TAU, 0.0], [0.0, 0.0, TAU]]).unwrap();
    for (cell, cutoff, edge) in [(unit, 2.0, true), (fcc(10.26), 3.0, false)] {
        let coarse = Grid::coarse(&cell, cutoff);
        let fine = Grid::fine(2.5 * cutoff, &coarse);
        let millers = coarse.millers();
        let sphere: Vec<usize> = (coarse.density_sphere(cutoff).iter())
            .map(|&(slot, _)| slot)
            .collect();
        let fine_slots: Vec<usize> = (sphere.iter())
            .map(|&s| fine.slot(millers[s]).unwrap())
            .collect();
        let d = coarse.dims();
        let nyquist =
            |m: [i32; 3]| (0..3).any(|k| d[k].is_multiple_of(2) && m[k] == d[k] as i32 / 2);
        assert_eq!(sphere.iter().any(|&s| nyquist(millers[s])), edge, "{d:?}");
        let (v, t) = (real_noise_on(&fine, 5), real_noise_on(&coarse, 6));
        let inner = |grid: &Grid, a: &[f64], b: &[f64]| {
            let n = grid.points().len() as f64;
            grid.cell().volume() / n * a.iter().zip(b).map(|(a, b)| a * b).sum::<f64>()
        };
        let padded = fine.synthesized(
            fine_slots.iter().copied(),
            coarse.coefficients(&t, sphere.iter().copied()).into_iter(),
        );
        let back = coarse.synthesized(
            sphere.iter().copied(),
            fine.coefficients(&v, fine_slots.iter().copied())
                .into_iter(),
        );
        let (on_fine, on_coarse) = (inner(&fine, &v, &padded), inner(&coarse, &back, &t));
        assert!(
            (on_fine - on_coarse).abs() < 1e-13 * on_fine.abs().max(1.0),
            "{:?}: {on_fine} vs {on_coarse}",
            coarse.dims()
        );
    }
}

#[test]
fn coefficients_on_a_sphere_synthesize_their_field_and_come_back() {
    let grid = Grid::coarse(&fcc(10.26), 3.0);
    let f: Vec<f64> = (grid.points().iter())
        .map(|f| 1.5 + (TAU * (f[0] + 2.0 * f[1])).cos())
        .collect();
    let slots: Vec<usize> = [[0, 0, 0], [1, 2, 0], [-1, -2, 0], [3, -1, 2]]
        .map(|m| grid.slot(m).unwrap())
        .to_vec();
    let c = grid.coefficients(&f, slots.iter().copied());
    let want = [1.5, 0.5, 0.5, 0.0];
    assert!(
        c.iter()
            .zip(want)
            .all(|(c, w)| (c - Complex64::new(w, 0.0)).norm() < 1e-14),
        "{c:?}"
    );
    let back = grid.synthesized(slots.into_iter(), c.into_iter());
    assert!(back.iter().zip(&f).all(|(a, b)| (a - b).abs() < 1e-14));
}

#[test]
#[should_panic(expected = "one value per grid point")]
fn a_buffer_of_another_length_is_refused() {
    let grid = Grid::coarse(&fcc(10.26), 2.0);
    grid.forward(&mut [Complex64::default(); 10]);
}

#[test]
fn the_density_sphere_holds_g_squared_up_to_eight_cutoffs_with_abinits_margin() {
    let cell = Cell::try_from([[TAU, 0.0, 0.0], [0.0, TAU, 0.0], [0.0, 0.0, TAU]]).unwrap();
    let grid = Grid::coarse(&cell, 2.0);
    let at = |cutoff: f64, m: [i32; 3]| {
        let slot = grid.slot(m).unwrap();
        (grid.density_sphere(cutoff).iter())
            .any(|&(s, g2)| s == slot && g2 == f64::from(m[0] * m[0] + m[1] * m[1] + m[2] * m[2]))
    };
    assert!(at(1.125, [3, 0, 0]) && at(1.125, [2, 2, 1]) && !at(1.125, [2, 2, 2]));
    assert!(at(9.0 / (8.0 * (1.0 + 5e-8)), [3, 0, 0]));
    assert!(!at(9.0 / (8.0 * (1.0 + 2e-7)), [3, 0, 0]));
    let count = grid.density_sphere(1.125).len();
    let mut want = 0;
    for i in -3..=3_i32 {
        for j in -3..=3_i32 {
            for k in -3..=3_i32 {
                want += usize::from(i * i + j * j + k * k <= 9);
            }
        }
    }
    assert_eq!(count, want);
}

fn spheres() -> Vec<(Grid, Basis)> {
    let s3 = 0.75_f64.sqrt();
    let hcp = scaled(
        [[1.0, 0.0, 0.0], [-0.5, s3, 0.0], [0.0, 0.0, 1.0]],
        [4.0, 4.0, 6.5],
    );
    let triclinic = Cell::try_from([[6.1, 0.0, 0.0], [1.9, 5.7, 0.0], [1.3, 2.2, 6.8]]).unwrap();
    let boxes = [
        (fcc(10.26), [0.1, -0.2, 0.3], 5.0),
        (hcp, [0.0; 3], 6.0),
        (triclinic, [0.25, 0.0, 0.5], 7.0),
        (fcc(10.26), [0.0, 0.25, 0.0], 15.0),
    ];
    let spheres: Vec<_> = (boxes.into_iter())
        .map(|(cell, k, cutoff)| (Grid::coarse(&cell, cutoff), Basis::sphere(&cell, k, cutoff)))
        .collect();
    assert!(
        (spheres.iter()).any(|(grid, _)| grid.dims().iter().any(|n| n % 2 == 1)),
        "an odd axis"
    );
    spheres
}

fn noise(i: usize, seed: u64) -> Complex64 {
    let mut state = (i as u64 + 1).wrapping_mul(0x9E37_79B9_7F4A_7C15) ^ seed;
    let mut next = || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64 - 0.5
    };
    Complex64::new(next(), next())
}

#[test]
fn the_sphere_inverse_is_the_full_inverse_bitwise_over_any_old_values() {
    for (grid, basis) in spheres() {
        let sphere = grid.sphere(basis.millers()).unwrap();
        let n = grid.points().len();
        let coefficients: Vec<Complex64> =
            (0..basis.millers().len()).map(|g| noise(g, 1)).collect();
        let mut want = vec![Complex64::default(); n];
        for (m, c) in basis.millers().iter().zip(&coefficients) {
            want[grid.slot(*m).unwrap()] = *c;
        }
        grid.inverse(&mut want);
        let mut values: Vec<Complex64> = (0..n).map(|i| noise(i, 2)).collect();
        sphere.inverse(coefficients.iter().copied(), &mut values);
        assert!(
            values.iter().zip(&want).all(|(a, b)| a == b),
            "{:?}",
            grid.dims()
        );
    }
}

#[test]
fn the_sphere_forward_is_the_full_forward_bitwise_at_each_plane_wave() {
    for (grid, basis) in spheres() {
        let sphere = grid.sphere(basis.millers()).unwrap();
        let mut values: Vec<Complex64> = (0..grid.points().len()).map(|i| noise(i, 3)).collect();
        let mut full = values.clone();
        grid.forward(&mut full);
        let want: Vec<Complex64> = (basis.millers().iter())
            .map(|m| full[grid.slot(*m).unwrap()])
            .collect();
        let got: Vec<Complex64> = sphere.forward(&mut values).collect();
        assert!(
            got.iter().zip(&want).all(|(a, b)| a == b),
            "{:?}",
            grid.dims()
        );
        assert_eq!(got.len(), want.len());
    }
}

fn tau(grid: &Grid, basis: &Basis, c: &[Complex64], scale: f64) -> Vec<f64> {
    let sphere = grid.sphere(basis.millers()).unwrap();
    let n = grid.points().len();
    let (mut buffer, mut tau) = (vec![noise(0, 9); n], vec![f64::NAN; n]);
    kinetic_density(&sphere, basis, |g| c[g], scale, &mut buffer, &mut tau);
    tau
}

#[test]
fn one_plane_wave_has_tau_half_its_squared_wavevector_over_the_volume_everywhere() {
    for (grid, basis) in spheres() {
        let volume = grid.cell().volume();
        let count = basis.millers().len();
        for g in [0, count / 3, count - 1] {
            let c: Vec<Complex64> = (0..count)
                .map(|h| Complex64::new(f64::from(u8::from(h == g)), 0.0))
                .collect();
            let want = basis.kinetic_ha()[g] / volume;
            let got = tau(&grid, &basis, &c, 1.0 / volume);
            assert!(
                got.iter().all(|t| (t - want).abs() < 1e-13 * want),
                "{:?} plane wave {g}",
                grid.dims()
            );
        }
    }
}

#[test]
fn the_integral_of_tau_is_the_band_kinetic_energy() {
    for (grid, basis) in spheres() {
        let (volume, n) = (grid.cell().volume(), grid.points().len());
        let c: Vec<Complex64> = (0..basis.millers().len()).map(|g| noise(g, 4)).collect();
        let want: f64 = (basis.kinetic_ha().iter().zip(&c))
            .map(|(t, c)| t * c.norm_sqr())
            .sum();
        let integral = volume / n as f64 * tau(&grid, &basis, &c, 1.0 / volume).iter().sum::<f64>();
        assert!(
            (integral - want).abs() < 1e-12 * want,
            "{:?}: {integral} vs {want}",
            grid.dims()
        );
    }
}

#[test]
fn a_sphere_with_a_plane_wave_outside_the_box_is_refused() {
    let grid = Grid::coarse(&fcc(10.26), 2.0);
    let refused = grid.sphere(&[[0, 0, 0], [40, 0, 0]]).err().unwrap();
    assert!(refused.contains("[40, 0, 0]"), "{refused}");
}

#[test]
#[should_panic(expected = "one coefficient per plane wave")]
fn a_sphere_inverse_with_another_number_of_coefficients_is_refused() {
    let cell = fcc(10.26);
    let grid = Grid::coarse(&cell, 2.0);
    let basis = Basis::sphere(&cell, [0.0; 3], 2.0);
    let sphere = grid.sphere(basis.millers()).unwrap();
    let mut values = vec![Complex64::default(); grid.points().len()];
    let one_short = basis.millers().len() - 1;
    sphere.inverse(
        vec![Complex64::default(); one_short].into_iter(),
        &mut values,
    );
}

#[test]
fn inside_a_thread_pool_a_transform_gives_the_same_bits() {
    let grid = Grid::coarse(&fcc(10.26), 10.0);
    let values: Vec<Complex64> = (0..grid.points().len())
        .map(|i| Complex64::new((i as f64 * 0.37).sin(), (i as f64 * 0.11).cos()))
        .collect();
    let transformed = || {
        let mut v = values.clone();
        grid.forward(&mut v);
        grid.inverse(&mut v);
        v.iter()
            .flat_map(|z| [z.re.to_bits(), z.im.to_bits()])
            .collect::<Vec<u64>>()
    };
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(4)
        .build()
        .unwrap();
    assert_eq!(transformed(), pool.install(transformed));
}

fn mixed() -> Grid {
    let cell = Cell::try_from([[7.0, 0.4, -0.3], [1.2, 8.1, 0.5], [-0.6, 0.8, 9.3]]).unwrap();
    let mixed =
        |dims: [usize; 3]| dims.iter().any(|n| n % 2 == 0) && dims.iter().any(|n| n % 2 == 1);
    (1..40)
        .map(|k| Grid::coarse(&cell, 0.5 * k as f64))
        .find(|grid| mixed(grid.dims()))
        .expect("a cutoff gives a box with an even and an odd axis")
}

fn real_noise(grid: &Grid, seed: u64) -> Vec<f64> {
    (0..grid.points().len())
        .map(|i| noise(i, seed).re)
        .collect()
}

fn reference(grid: &Grid, fields: &[&[f64]], terms: &[(usize, usize)]) -> Vec<f64> {
    let g = grid.wavevectors();
    let mut sum = vec![Complex64::default(); g.len()];
    for &(field, axis) in terms {
        let mut t: Vec<Complex64> = fields[field].iter().map(|&x| x.into()).collect();
        grid.forward(&mut t);
        for ((s, t), g) in sum.iter_mut().zip(&t).zip(&g) {
            *s += t * Complex64::new(0.0, g[axis]);
        }
    }
    grid.inverse(&mut sum);
    sum.iter().map(|z| z.re).collect()
}

fn close(a: &[f64], b: &[f64]) {
    assert_eq!(a.len(), b.len());
    let scale = b.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
    let worst = a
        .iter()
        .zip(b)
        .fold(0.0_f64, |m, (x, y)| m.max((x - y).abs()));
    assert!(worst <= 1e-12 * scale, "{worst:e} against {scale:e}");
}

#[test]
fn two_fields_per_transform_give_the_gradient_and_divergence_of_one_field_per_transform() {
    let grid = mixed();
    let fields: Vec<Vec<f64>> = (1..=6).map(|seed| real_noise(&grid, seed)).collect();
    let [f, g, ..] = [
        &fields[0][..],
        &fields[1][..],
        &fields[2][..],
        &fields[3][..],
    ];
    for (one, gradient) in [f, g].iter().zip(grid.gradients(&[f, g])) {
        for (axis, d) in gradient.iter().enumerate() {
            close(d, &reference(&grid, &[one], &[(0, axis)]));
        }
    }
    for (axis, d) in grid.gradient(f).iter().enumerate() {
        close(d, &reference(&grid, &[f], &[(0, axis)]));
    }
    let h: Vec<&[f64]> = fields.iter().map(Vec::as_slice).collect();
    let divergences = grid.divergences(2, |s, axis, r| h[3 * s + axis][r]);
    for (s, d) in divergences.iter().enumerate() {
        close(
            d,
            &reference(&grid, &h[3 * s..3 * s + 3], &[(0, 0), (1, 1), (2, 2)]),
        );
    }
    let single: [Vec<f64>; 3] = std::array::from_fn(|axis| fields[axis].clone());
    close(&grid.divergence(&single), &divergences[0]);
}

fn transferred(from: &Grid, values: &[f64], to: &Grid) -> Vec<f64> {
    let mut f: Vec<Complex64> = values.iter().map(|&x| x.into()).collect();
    from.forward(&mut f);
    let mut g = vec![Complex64::default(); to.points().len()];
    let nyquist = |m: [i32; 3], dims: [usize; 3]| {
        (0..3).any(|k| dims[k].is_multiple_of(2) && m[k] == dims[k] as i32 / 2)
    };
    let scale = g.len() as f64 / f.len() as f64;
    for (m, value) in from.millers().into_iter().zip(&f) {
        if let (false, false, Some(slot)) =
            (nyquist(m, from.dims()), nyquist(m, to.dims()), to.slot(m))
        {
            g[slot] = value * scale;
        }
    }
    to.inverse(&mut g);
    g.iter().map(|z| z.re).collect()
}

#[test]
fn two_fields_per_transform_transfer_as_one_field_per_transform_both_ways() {
    let coarse = mixed();
    let fine = Grid::fine(5.0, &coarse);
    for (from, to) in [(&coarse, &fine), (&fine, &coarse)] {
        let fields = [
            real_noise(from, 7),
            real_noise(from, 8),
            real_noise(from, 9),
        ];
        let both = from.transfers(3, |k, r| fields[k][r], to);
        assert_eq!(both.len(), 3);
        for (field, got) in fields.iter().zip(&both) {
            close(got, &transferred(from, field, to));
        }
    }
}

#[test]
fn a_constant_kinetic_strain_weight_gives_the_kinetic_metric_derivative() {
    for (grid, basis) in spheres() {
        let n = grid.points().len();
        let sphere = grid.sphere(basis.millers()).unwrap();
        let c: Vec<Complex64> = (0..basis.millers().len()).map(|g| noise(g, 5)).collect();
        let (weight, scale) = (0.7, 0.3);
        let mut buffers = vec![noise(1, 9); 3 * n];
        let got = kinetic_strain(
            &sphere,
            &basis,
            |g| c[g],
            scale,
            &vec![weight; n],
            &mut buffers,
        );
        for i in 0..3 {
            for j in 0..3 {
                let want: f64 = (basis.wavevectors().iter().zip(&c))
                    .map(|(q, c)| weight * scale * n as f64 * q[i] * q[j] * c.norm_sqr())
                    .sum();
                let size: f64 = (basis.kinetic_ha().iter().zip(&c))
                    .map(|(t, c)| t * c.norm_sqr())
                    .sum();
                assert!(
                    (got[i][j] - want).abs() < 1e-12 * weight * scale * n as f64 * size,
                    "{:?} [{i}][{j}]: {} vs {want}",
                    grid.dims(),
                    got[i][j]
                );
            }
        }
    }
}

#[test]
fn the_kinetic_strain_is_minus_the_strain_derivative_of_a_weighted_tau() {
    let e = [[0.3, 0.5, -0.2], [0.5, -0.4, 0.6], [-0.2, 0.6, 0.1]];
    let rows = [[6.1, 0.0, 0.0], [1.9, 5.7, 0.0], [1.3, 2.2, 6.8]];
    let (k, cutoff, h) = ([0.25, 0.0, 0.5], 7.0, 1e-5);
    let at = |t: f64| {
        let strained: [[f64; 3]; 3] = std::array::from_fn(|r| {
            std::array::from_fn(|c| {
                (0..3)
                    .map(|m| (f64::from(u8::from(c == m)) + t * e[c][m]) * rows[r][m])
                    .sum()
            })
        });
        let cell = Cell::try_from(strained).unwrap();
        (Grid::coarse(&cell, cutoff), Basis::sphere(&cell, k, cutoff))
    };
    let (grid, basis) = at(0.0);
    let n = grid.points().len();
    let v: Vec<f64> = (0..n).map(|r| 1.0 + noise(r, 7).re).collect();
    let c: Vec<Complex64> = (0..basis.millers().len()).map(|g| noise(g, 6)).collect();
    let weighted = |t: f64| {
        let (g, b) = at(t);
        assert_eq!((g.dims(), b.millers()), (grid.dims(), basis.millers()));
        tau(&g, &b, &c, 1.0)
            .iter()
            .zip(&v)
            .map(|(t, v)| t * v)
            .sum::<f64>()
    };
    let sphere = grid.sphere(basis.millers()).unwrap();
    let mut buffers = vec![Complex64::default(); 3 * n];
    let k_ij = kinetic_strain(&sphere, &basis, |g| c[g], 1.0, &v, &mut buffers);
    let along: f64 = (0..3)
        .flat_map(|i| (0..3).map(move |j| (i, j)))
        .map(|(i, j)| e[i][j] * k_ij[i][j])
        .sum();
    let slope = (weighted(h) - weighted(-h)) / (2.0 * h);
    assert!(
        (along + slope).abs() < 1e-8 * along.abs(),
        "{along} vs {slope}"
    );
    let trace = k_ij[0][0] + k_ij[1][1] + k_ij[2][2];
    assert!(
        (trace - 2.0 * weighted(0.0)).abs() < 1e-12 * trace,
        "{trace}"
    );
}
