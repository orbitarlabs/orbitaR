use orbitar::paw::Table;

fn gaussian_transform(l: i32, alpha: f64, q: f64) -> f64 {
    std::f64::consts::PI.sqrt() * q.powi(l) * (-q * q / (4.0 * alpha)).exp()
        / (2f64.powi(l + 2) * alpha.powf(f64::from(l) + 1.5))
}

#[test]
fn the_spline_reproduces_a_smooth_transform_between_samples() {
    for l in 0..=3 {
        let f = |q: f64| gaussian_transform(l, 1.3, q);
        let table = Table::sampled(9.0, 0.01, f);
        let scale = f(0.0)
            .abs()
            .max(gaussian_transform(l, 1.3, (2.6 * f64::from(l)).sqrt()));
        for i in 0..900 {
            let q = 0.01 * f64::from(i) + 0.00737;
            assert!((table.at(q) - f(q)).abs() < 1e-10 * scale, "l {l}, q {q}");
        }
    }
}

#[test]
fn the_spline_passes_through_its_samples() {
    let table = Table::sampled(1.0, 0.125, |q| (3.0 * q).sin());
    for i in 0..=9 {
        let q = 0.125 * f64::from(i);
        assert!((table.at(q) - (3.0 * q).sin()).abs() < 1e-15);
    }
}

#[test]
#[should_panic(expected = "lies outside this table's 0 to")]
fn a_q_past_the_last_sample_is_refused() {
    let table = Table::sampled(1.0, 0.1, |q| (-q * q).exp());
    table.at(1.15);
}

#[test]
fn the_slope_is_the_derivative_of_the_spline_and_of_the_smooth_transform() {
    for l in 0..=3 {
        let f = |q: f64| gaussian_transform(l, 1.3, q);
        let table = Table::sampled(9.0, 0.01, f);
        let h = 1e-4;
        for i in 0..90 {
            let q = 0.1 * f64::from(i) + 0.00537;
            let fd = (8.0 * (table.at(q + h) - table.at(q - h))
                - (table.at(q + 2.0 * h) - table.at(q - 2.0 * h)))
                / (12.0 * h);
            let exact =
                (8.0 * (f(q + h) - f(q - h)) - (f(q + 2.0 * h) - f(q - 2.0 * h))) / (12.0 * h);
            assert!((table.slope(q) - fd).abs() < 1e-9, "l {l}, q {q}");
            assert!((table.slope(q) - exact).abs() < 1e-8, "l {l}, q {q}");
        }
    }
}
