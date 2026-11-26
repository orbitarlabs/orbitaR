use orbitar::ks::{Mixer, Weight};

fn plain(n: usize) -> Vec<Weight> {
    vec![
        Weight {
            damping: 0.7,
            metric: 1.0,
        };
        n
    ]
}

struct Affine {
    a: Vec<Vec<f64>>,
    b: Vec<f64>,
    fixed: Vec<f64>,
}

fn affine(n: usize) -> Affine {
    let a: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            (0..n)
                .map(|j| {
                    let (i, j) = (i as f64, j as f64);
                    0.72 / n as f64 * ((1.3 * i + 0.7 * j).sin() + 0.4 * (i - j))
                })
                .collect()
        })
        .collect();
    let b: Vec<f64> = (0..n).map(|i| 1.0 + 0.5 * i as f64).collect();
    let mut m: Vec<Vec<f64>> = (0..n)
        .map(|i| {
            let mut row: Vec<f64> = (0..n)
                .map(|j| f64::from(u8::from(i == j)) - a[i][j])
                .collect();
            row.push(b[i]);
            row
        })
        .collect();
    for c in 0..n {
        let pivot = (c..n)
            .max_by(|&p, &q| m[p][c].abs().total_cmp(&m[q][c].abs()))
            .unwrap();
        m.swap(c, pivot);
        let row = m[c].clone();
        for (_, other) in m.iter_mut().enumerate().filter(|(r, _)| *r != c) {
            let f = other[c] / row[c];
            other
                .iter_mut()
                .zip(&row)
                .skip(c)
                .for_each(|(x, y)| *x -= f * y);
        }
    }
    let fixed = (0..n).map(|i| m[i][n] / m[i][i]).collect();
    Affine { a, b, fixed }
}

impl Affine {
    fn apply(&self, x: &[f64]) -> Vec<f64> {
        (self.a.iter().zip(&self.b))
            .map(|(row, b)| row.iter().zip(x).map(|(a, x)| a * x).sum::<f64>() + b)
            .collect()
    }
}

fn largest_difference(p: &[f64], q: &[f64]) -> f64 {
    p.iter()
        .zip(q)
        .map(|(p, q)| (p - q).abs())
        .fold(0.0, f64::max)
}

#[test]
fn pulay_reaches_the_fixed_point_of_an_affine_map_once_its_steps_span_the_space() {
    let map = affine(6);
    let mut mixer = Mixer::damped(&plain(6));
    let mut x = vec![0.0; 6];
    let mut errors = Vec::new();
    for _ in 0..7 {
        let out = map.apply(&x);
        x = mixer.next(&x, &out);
        errors.push(largest_difference(&x, &map.fixed));
    }
    assert!(errors[6] < 1e-12, "{errors:?}");
    assert!(errors[3] > 1e-3, "{errors:?}");
}

#[test]
fn the_first_step_is_damped_linear_mixing() {
    let mut mixer = Mixer::damped(&[
        Weight {
            damping: 0.5,
            metric: 1.0,
        },
        Weight {
            damping: 0.25,
            metric: 0.0,
        },
    ]);
    assert_eq!(mixer.next(&[1.0, 2.0], &[3.0, 6.0]), [2.0, 3.0]);
}

#[test]
fn mixing_keeps_a_linear_constraint_that_every_step_meets() {
    let map = affine(6);
    let mut mixer = Mixer::damped(&plain(6));
    let mut x = vec![1.0 / 6.0; 6];
    for _ in 0..10 {
        let y: Vec<f64> = map
            .apply(&x)
            .iter()
            .zip(&map.b)
            .map(|(y, b)| y - b)
            .collect();
        let shift = (1.0 - y.iter().sum::<f64>()) / 6.0;
        let out: Vec<f64> = y.iter().map(|v| v + shift).collect();
        x = mixer.next(&x, &out);
        assert!((x.iter().sum::<f64>() - 1.0).abs() < 1e-14);
    }
}

#[test]
fn the_history_holds_the_newest_eight_steps() {
    let map = affine(12);
    let steps: Vec<(Vec<f64>, Vec<f64>)> = (0..9_u32)
        .map(|s| {
            let x: Vec<f64> = (0..12_u32)
                .map(|i| (0.37 * f64::from((s * 12 + i).pow(2))).sin())
                .collect();
            let out = map.apply(&x);
            (x, out)
        })
        .collect();
    let run = |from: usize, to: usize| {
        let mut mixer = Mixer::damped(&plain(12));
        steps[from..to]
            .iter()
            .map(|(x, out)| mixer.next(x, out))
            .last()
            .unwrap()
    };
    assert!(largest_difference(&run(0, 9), &run(1, 9)) < 1e-12);
    assert!(largest_difference(&run(0, 8), &run(1, 8)) > 1e-6);
}

#[test]
fn components_outside_the_metric_do_not_steer_the_coefficients() {
    let weights = [plain(1)[0], Weight::onsite()];
    let (mut first, mut second) = (Mixer::damped(&weights), Mixer::damped(&weights));
    let inputs = [[0.0, 0.0], [1.0, 0.5], [0.8, 0.3]];
    let outputs = [[2.0, 1.0], [0.6, 0.9], [0.9, 0.1]];
    let (mut p, mut q) = (Vec::new(), Vec::new());
    for (x, out) in inputs.iter().zip(&outputs) {
        p = first.next(x, out);
        q = second.next(x, &[out[0], out[1] + 7.0]);
    }
    assert!((p[0] - q[0]).abs() < 1e-15);
    assert!((p[1] - q[1]).abs() > 1.0);
}

#[test]
fn a_zero_residual_in_the_metric_falls_back_to_the_newest_step() {
    let mut mixer = Mixer::damped(&[plain(1)[0], Weight::onsite()]);
    mixer.next(&[1.0, 0.0], &[1.0, 1.0]);
    let x = mixer.next(&[1.0, 1.0], &[1.0, 2.0]);
    assert!(x.iter().all(|v| v.is_finite()));
    assert!((x[1] - 1.7).abs() < 1e-15);
}

#[test]
fn a_step_that_repeats_an_older_one_to_rounding_adds_nothing_at_any_scale() {
    let scale = 1e8;
    let first = ([0.0, 0.0], [1.0, 2.0]);
    let second = ([1.0, 0.5], [0.4, 1.7]);
    let repeat = ([1.0, 0.5 + 1e-15], [0.4, 1.7]);
    let run = |steps: &[([f64; 2], [f64; 2])]| {
        let mut mixer = Mixer::damped(&plain(2));
        (steps.iter())
            .map(|(x, out)| mixer.next(&x.map(|v| v * scale), &out.map(|v| v * scale)))
            .last()
            .unwrap()
    };
    let (with, without) = (run(&[first, second, repeat]), run(&[first, repeat]));
    assert!(
        largest_difference(&with, &without) < 1e-6,
        "{with:?} {without:?}"
    );
}

#[test]
fn kerker_damps_long_waves_and_keeps_the_charge() {
    assert_eq!(
        Weight::onsite(),
        Weight {
            damping: 0.7,
            metric: 0.0
        }
    );
    let damping = |g2: f64| Weight::density(g2, 1.0).damping;
    assert_eq!(
        Weight::density(0.0, 5.0),
        Weight {
            damping: 0.7,
            metric: 5.0
        }
    );
    let k0 = 1.0 / 1.0774841;
    assert!((damping(k0 * k0) - 0.35).abs() < 1e-15);
    assert!((damping(1e-6) - 0.7 * 1e-6 * 1.0774841_f64.powi(2)).abs() < 1e-12);
    assert!((damping(1e8) - 0.7).abs() < 1e-8);
}

fn mixed_on_a_diagonal_map(stiffness: &[f64], inputs: &[&[f64]]) -> Vec<f64> {
    let weight = Weight {
        damping: 0.5,
        metric: 1.0,
    };
    let mut mixer = Mixer::damped(&vec![weight; stiffness.len()]);
    (inputs.iter())
        .map(|x| {
            let out: Vec<f64> = (x.iter().zip(stiffness)).map(|(x, e)| x - e * x).collect();
            mixer.next(x, &out)
        })
        .last()
        .unwrap()
}

fn mixed_along_one_stiff_axis(a: &[f64]) -> Vec<f64> {
    let inputs: Vec<[f64; 2]> = a.iter().map(|&a| [a, 1.0]).collect();
    let inputs: Vec<&[f64]> = inputs.iter().map(<[f64; 2]>::as_slice).collect();
    mixed_on_a_diagonal_map(&[10.0, 1.0], &inputs)
}

#[test]
fn a_residual_above_the_oldest_bounds_the_step_by_the_largest_eigenvalue_of_the_damped_dielectric()
{
    let x = mixed_along_one_stiff_axis(&[1.0, 3.0]);
    assert!(largest_difference(&x, &[0.0, 0.9]) < 1e-14, "{x:?}");
}

#[test]
fn the_bound_does_not_depend_on_the_scale_of_the_metric() {
    let weight = Weight {
        damping: 0.5,
        metric: 4.0,
    };
    let mut mixer = Mixer::damped(&[weight, weight]);
    let mut x = Vec::new();
    for input in [[1.0, 1.0], [3.0, 1.0]] {
        let out = [input[0] - 10.0 * input[0], 0.0];
        x = mixer.next(&input, &out);
    }
    assert!(largest_difference(&x, &[0.0, 0.9]) < 1e-14, "{x:?}");
}

#[test]
fn a_residual_that_shrank_leaves_the_plain_pulay_step() {
    let x = mixed_along_one_stiff_axis(&[3.0, 1.0]);
    assert!(largest_difference(&x, &[0.0, 0.5]) < 1e-14, "{x:?}");
}

#[test]
fn a_residual_above_the_step_before_but_below_the_oldest_leaves_the_plain_pulay_step() {
    let x = mixed_along_one_stiff_axis(&[3.0, 1.0, 2.0]);
    assert!(largest_difference(&x, &[0.0, 0.5]) < 1e-14, "{x:?}");
}

#[test]
fn a_residual_equal_to_the_oldest_leaves_the_plain_pulay_step() {
    let x = mixed_on_a_diagonal_map(&[4.0, 4.0], &[&[1.0, 0.0], &[0.0, 1.0]]);
    assert!(largest_difference(&x, &[-0.5, -0.5]) < 1e-14, "{x:?}");
}

#[test]
fn the_bound_holds_while_the_stalled_step_is_in_the_history() {
    let x = mixed_along_one_stiff_axis(&[1.0, 3.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3]);
    assert!(largest_difference(&x, &[0.0, 0.9]) < 1e-12, "{x:?}");
}

#[test]
fn a_stall_compares_with_the_oldest_step_left_after_the_history_drops_one() {
    let x = mixed_along_one_stiff_axis(&[10.0, 1.0, 9.0, 0.5, 0.5, 0.5, 0.5, 0.5, 2.0]);
    assert!(largest_difference(&x, &[0.0, 0.9]) < 1e-12, "{x:?}");
}

#[test]
fn the_bound_ends_when_the_stalled_step_leaves_the_history() {
    let x = mixed_along_one_stiff_axis(&[1.0, 3.0, 0.9, 0.8, 0.7, 0.6, 0.5, 0.4, 0.3, 0.2]);
    assert!(largest_difference(&x, &[0.0, 0.5]) < 1e-12, "{x:?}");
}

#[test]
fn a_stall_without_an_eigenvalue_above_one_leaves_the_plain_pulay_step() {
    let x = mixed_on_a_diagonal_map(&[-3.0, 1.0], &[&[1.0, 1.0], &[3.0, 1.0]]);
    assert!(largest_difference(&x, &[0.0, 0.5]) < 1e-14, "{x:?}");
}

#[test]
fn a_component_outside_the_metric_does_not_stall_a_step() {
    let (inside, outside) = (
        Weight {
            damping: 0.5,
            metric: 1.0,
        },
        Weight {
            damping: 0.5,
            metric: 0.0,
        },
    );
    let mut mixer = Mixer::damped(&[inside, inside, outside]);
    let stiffness = [10.0, 1.0, 1.0];
    let mut x = Vec::new();
    for input in [[1.5, 1.0, 0.0], [1.0, 1.0, 20.0]] {
        let out: Vec<f64> = (input.iter().zip(&stiffness))
            .map(|(x, e)| x - e * x)
            .collect();
        x = mixer.next(&input, &out);
    }
    assert!(largest_difference(&x, &[0.0, 0.5, 30.0]) < 1e-12, "{x:?}");
}
