use orbitar::model::Functional;
use orbitar::xc::{
    DENSITY_FLOOR_PER_BOHR3, Inputs, Point, Rung, TAU_FLOOR_HA_PER_BOHR3, Unpolarized, Value,
    Variable, energy_density, evaluate, partials, prepares, prepares_unpolarized, unpolarized,
};
use serde::Deserialize;
use std::collections::HashMap;
use std::f64::consts::PI;

#[derive(Deserialize)]
pub struct Rows {
    #[serde(default = "floors")]
    pub inputs: String,
    pub points: Vec<Vec<f64>>,
    pub exchange: Vec<[f64; 8]>,
    #[serde(default)]
    pub exchange_scaled: Vec<[f64; 8]>,
    pub correlation: Vec<[f64; 8]>,
    pub exchange_epsilon: Vec<f64>,
    pub correlation_epsilon: Vec<f64>,
    pub exchange_spread: Vec<[f64; 8]>,
    #[serde(default)]
    pub exchange_scaled_spread: Vec<[f64; 8]>,
    pub correlation_spread: Vec<[f64; 8]>,
    pub exchange_epsilon_spread: Vec<f64>,
    pub correlation_epsilon_spread: Vec<f64>,
    pub exchange_kappa: Vec<Option<f64>>,
    pub correlation_kappa: Vec<Option<f64>>,
    #[serde(default)]
    pub exchange_strict: Vec<bool>,
    #[serde(default)]
    pub correlation_strict: Vec<bool>,
    pub unpolarized: Option<Spinless>,
    pub fixtures: Option<Fixtures>,
}

#[derive(Deserialize)]
pub struct Spinless {
    pub points: Vec<[f64; 3]>,
    pub exchange: Vec<[f64; 4]>,
    pub correlation: Vec<[f64; 4]>,
    pub exchange_epsilon: Vec<f64>,
    pub correlation_epsilon: Vec<f64>,
    pub exchange_spread: Vec<[f64; 4]>,
    pub correlation_spread: Vec<[f64; 4]>,
    pub exchange_epsilon_spread: Vec<f64>,
    pub correlation_epsilon_spread: Vec<f64>,
    pub exchange_kappa: Vec<Option<f64>>,
    pub correlation_kappa: Vec<Option<f64>>,
    pub exchange_strict: Vec<bool>,
    pub correlation_strict: Vec<bool>,
}

#[derive(Deserialize)]
pub struct Fixtures {
    pub polarized: FixtureRows,
    pub unpolarized: FixtureRows,
}

#[derive(Deserialize)]
pub struct FixtureRows {
    pub points: Vec<Vec<f64>>,
    pub exchange: Vec<Vec<Option<f64>>>,
    pub correlation: Vec<Vec<Option<f64>>>,
    pub exchange_epsilon: Vec<Option<f64>>,
    pub correlation_epsilon: Vec<Option<f64>>,
    pub exchange_spread: Vec<Vec<f64>>,
    pub correlation_spread: Vec<Vec<f64>>,
    pub exchange_epsilon_spread: Vec<f64>,
    pub correlation_epsilon_spread: Vec<f64>,
    pub exchange_kappa: Vec<Option<f64>>,
    pub correlation_kappa: Vec<Option<f64>>,
}

#[derive(Deserialize, Clone, Copy)]
pub struct Rule {
    pub spreads: f64,
    pub cancellation: f64,
    pub density: f64,
}

fn floors() -> String {
    "floors".to_owned()
}

#[derive(Deserialize)]
struct Table {
    functionals: HashMap<String, Rows>,
    rule: Rule,
}

#[expect(
    clippy::disallowed_macros,
    reason = "the test reads the case's own reference file"
)]
fn libxc() -> Table {
    let text = include_str!("../../benchmarks/4_xc_libxc/reference/table.json");
    serde_json::from_str(text).unwrap()
}

fn rule() -> Rule {
    libxc().rule
}

pub fn rows(functional: Functional) -> Rows {
    let word = functional.row().word;
    libxc()
        .functionals
        .remove(word)
        .unwrap_or_else(|| panic!("the case-4 table has no Libxc rows for {word:?}"))
}

pub fn point(x: &[f64]) -> Point {
    let p = Point::of(x[0], x[1], [x[2], x[3], x[4]]);
    match x.len() {
        7 => p.with_tau([x[5], x[6]]),
        _ => {
            let uniform = |rho: f64, sigma: f64| match rho {
                0.0 => 0.0,
                _ => {
                    sigma / (8.0 * rho)
                        + 0.3 * (6.0 * PI * PI).powf(2.0 / 3.0) * rho.powf(5.0 / 3.0)
                }
            };
            p.with_tau([uniform(x[0], x[2]), uniform(x[1], x[4])])
        }
    }
}

fn every_point() -> Vec<Point> {
    let mut all: Vec<Point> = Vec::new();
    for rows in libxc().functionals.values() {
        all.extend(rows.points.iter().map(|x| point(x)));
    }
    all
}

fn row(v: Value) -> [f64; 8] {
    let [up, down] = v.vrho;
    let [uu, ud, dd] = v.vsigma;
    let [tu, td] = v.vtau;
    [v.energy, up, down, uu, ud, dd, tu, td]
}

#[test]
fn the_table_spells_each_functional_once() {
    for (k, a) in Functional::ALL.iter().enumerate() {
        assert_eq!(a.functional.row(), *a);
        assert_eq!(
            (a.functional.rung(), a.functional.dataset()),
            (a.rung, a.dataset)
        );
        for b in &Functional::ALL[k + 1..] {
            assert_ne!(a.functional, b.functional);
            assert_ne!(a.word, b.word);
        }
    }
}

#[test]
fn a_dataset_header_names_the_first_functional_that_reads_it() {
    for row in Functional::ALL {
        let (kind, name) = row.dataset;
        let first = Functional::ALL.iter().find(|r| r.dataset == row.dataset);
        assert_eq!(
            Functional::of_dataset(kind, name),
            first.map(|r| r.functional)
        );
    }
    assert_eq!(Functional::of_dataset("GGA", "PW91"), None);
}

#[test]
fn each_rung_reads_a_leading_part_of_the_variables_and_the_next_rung_more() {
    let read = |rung: Rung| Variable::ALL.iter().filter(|&&v| rung.reads(v)).count();
    assert_eq!([Rung::Lda, Rung::Gga, Rung::MetaGga].map(read), [2, 5, 7]);
    assert!(Rung::Lda < Rung::Gga && Rung::Gga < Rung::MetaGga);
    for rung in [Rung::Lda, Rung::Gga, Rung::MetaGga] {
        let n = read(rung);
        assert!(Variable::ALL[..n].iter().all(|&v| rung.reads(v)));
    }
}

#[derive(Clone, Copy)]
struct Terms {
    spread: f64,
    kappa: Option<f64>,
    epsilon: Option<f64>,
}

fn matches(got: f64, want: Option<f64>, terms: Terms, rule: Rule) -> bool {
    let Terms {
        spread,
        kappa,
        epsilon,
    } = terms;
    match want {
        None => got.is_nan(),
        Some(0.0) => got == 0.0,
        Some(want) => {
            let u = f64::EPSILON / 2.0;
            let relative = 1e-12 + rule.cancellation * kappa.unwrap_or(0.0) * u;
            let density = rule.density * u * epsilon.unwrap_or(0.0).abs();
            (got - want).abs() <= relative * want.abs() + rule.spreads * spread + density
        }
    }
}

struct Part<'a> {
    name: &'static str,
    value: Value,
    energy: f64,
    want: Vec<Option<f64>>,
    epsilon: (Option<f64>, f64),
    spread: &'a [f64],
    kappa: Option<f64>,
    strict: bool,
}

fn judge(at: &str, part: Part, rule: Rule) {
    let Part {
        name,
        value,
        energy,
        want,
        epsilon: (zk, zk_spread),
        spread,
        kappa,
        strict,
    } = part;
    let terms = |spread: f64, density: bool| Terms {
        spread: if strict { 0.0 } else { spread },
        kappa,
        epsilon: if density { zk } else { None },
    };
    let check = |what: &str, got: f64, want: Option<f64>, terms: Terms| {
        assert!(
            matches(got, want, terms, rule),
            "{at}: {name} {what}: {got} vs {want:?}"
        );
    };
    check("e", energy, want[0], terms(spread[0], false));
    check("ε", value.epsilon, zk, terms(zk_spread, false));
    for (column, g) in row(value).iter().enumerate() {
        let what = format!("column {column}");
        check(
            &what,
            *g,
            want[column],
            terms(spread[column], (1..3).contains(&column)),
        );
    }
}

struct Spinless1<'a> {
    value: Unpolarized,
    want: Vec<Option<f64>>,
    epsilon: (Option<f64>, f64),
    spread: &'a [f64],
    kappa: Option<f64>,
    strict: bool,
}

fn judge_unpolarized(at: &str, part: Spinless1, rule: Rule) {
    let Spinless1 {
        value: u,
        want,
        epsilon: (zk, zk_spread),
        spread,
        kappa,
        strict,
    } = part;
    let terms = |spread: f64, density: bool| Terms {
        spread: if strict { 0.0 } else { spread },
        kappa,
        epsilon: if density { zk } else { None },
    };
    let got = [u.energy, u.vrho, u.vsigma, u.vtau];
    for (column, g) in got.iter().enumerate() {
        assert!(
            matches(*g, want[column], terms(spread[column], column == 1), rule),
            "{at}: column {column}: {g} vs {:?}",
            want[column]
        );
    }
    assert!(
        matches(u.epsilon, zk, terms(zk_spread, false), rule),
        "{at}: ε {} vs {zk:?}",
        u.epsilon
    );
}

pub fn matches_libxc(functional: Functional) {
    let (rows, rule) = (rows(functional), rule());
    let flag = |flags: &Vec<bool>, k: usize| flags.get(k).copied().unwrap_or(false);
    for (k, x) in rows.points.iter().enumerate() {
        let p = point(x);
        let at = format!("{functional:?} {x:?}");
        let [vx, vc] = evaluate(functional, p);
        let (ex, ec) = energy_density(functional, p);
        let exchange = (
            Some(rows.exchange_epsilon[k]),
            rows.exchange_epsilon_spread[k],
        );
        let mut parts = vec![
            Part {
                name: "exchange",
                value: vx,
                energy: ex,
                want: rows.exchange[k].map(Some).to_vec(),
                epsilon: exchange,
                spread: &rows.exchange_spread[k],
                kappa: rows.exchange_kappa[k],
                strict: flag(&rows.exchange_strict, k),
            },
            Part {
                name: "correlation",
                value: vc,
                energy: ec,
                want: rows.correlation[k].map(Some).to_vec(),
                epsilon: (
                    Some(rows.correlation_epsilon[k]),
                    rows.correlation_epsilon_spread[k],
                ),
                spread: &rows.correlation_spread[k],
                kappa: rows.correlation_kappa[k],
                strict: flag(&rows.correlation_strict, k),
            },
        ];
        if !rows.exchange_scaled.is_empty() {
            parts.push(Part {
                name: "exchange (scaled)",
                value: vx,
                energy: ex,
                want: rows.exchange_scaled[k].map(Some).to_vec(),
                epsilon: exchange,
                spread: &rows.exchange_scaled_spread[k],
                kappa: rows.exchange_kappa[k],
                strict: false,
            });
        }
        for part in parts {
            judge(&at, part, rule);
        }
    }
    if let Some(spinless) = &rows.unpolarized {
        for (k, &[n, sigma, tau]) in spinless.points.iter().enumerate() {
            let at = format!("{functional:?} unpolarized {:?}", spinless.points[k]);
            let [ux, uc] = unpolarized(functional, n, sigma, Some(tau));
            let x = &spinless;
            judge_unpolarized(
                &at,
                Spinless1 {
                    value: ux,
                    want: x.exchange[k].map(Some).to_vec(),
                    epsilon: (Some(x.exchange_epsilon[k]), x.exchange_epsilon_spread[k]),
                    spread: &x.exchange_spread[k],
                    kappa: x.exchange_kappa[k],
                    strict: x.exchange_strict[k],
                },
                rule,
            );
            judge_unpolarized(
                &at,
                Spinless1 {
                    value: uc,
                    want: x.correlation[k].map(Some).to_vec(),
                    epsilon: (
                        Some(x.correlation_epsilon[k]),
                        x.correlation_epsilon_spread[k],
                    ),
                    spread: &x.correlation_spread[k],
                    kappa: x.correlation_kappa[k],
                    strict: x.correlation_strict[k],
                },
                rule,
            );
        }
    }
    if let Some(fixtures) = &rows.fixtures {
        fixtures_match(functional, fixtures, rule);
    }
}

fn fixtures_match(functional: Functional, fixtures: &Fixtures, rule: Rule) {
    let polarized = &fixtures.polarized;
    for (k, x) in polarized.points.iter().enumerate() {
        let at = format!("{functional:?} fixture {x:?}");
        let p = point(x);
        let [vx, vc] = evaluate(functional, p);
        let (ex, ec) = energy_density(functional, p);
        let parts = [
            Part {
                name: "exchange",
                value: vx,
                energy: ex,
                want: polarized.exchange[k].clone(),
                epsilon: (
                    polarized.exchange_epsilon[k],
                    polarized.exchange_epsilon_spread[k],
                ),
                spread: &polarized.exchange_spread[k],
                kappa: polarized.exchange_kappa[k],
                strict: true,
            },
            Part {
                name: "correlation",
                value: vc,
                energy: ec,
                want: polarized.correlation[k].clone(),
                epsilon: (
                    polarized.correlation_epsilon[k],
                    polarized.correlation_epsilon_spread[k],
                ),
                spread: &polarized.correlation_spread[k],
                kappa: polarized.correlation_kappa[k],
                strict: true,
            },
        ];
        for part in parts {
            judge(&at, part, rule);
        }
    }
    let x = &fixtures.unpolarized;
    for (k, point) in x.points.iter().enumerate() {
        let at = format!("{functional:?} unpolarized fixture {point:?}");
        let [ux, uc] = unpolarized(functional, point[0], point[1], Some(point[2]));
        judge_unpolarized(
            &at,
            Spinless1 {
                value: ux,
                want: x.exchange[k].clone(),
                epsilon: (x.exchange_epsilon[k], x.exchange_epsilon_spread[k]),
                spread: &x.exchange_spread[k],
                kappa: x.exchange_kappa[k],
                strict: true,
            },
            rule,
        );
        judge_unpolarized(
            &at,
            Spinless1 {
                value: uc,
                want: x.correlation[k].clone(),
                epsilon: (x.correlation_epsilon[k], x.correlation_epsilon_spread[k]),
                spread: &x.correlation_spread[k],
                kappa: x.correlation_kappa[k],
                strict: true,
            },
            rule,
        );
    }
}

#[test]
fn r2scan_forms_the_partials_of_zeta_in_libxcs_order() {
    let rows = rows(Functional::R2scan);
    for x in [
        [2.75, 1e-14, 0.1, 0.0, 0.0, 0.1, 0.5],
        [1e-14, 2.75, 0.0, 0.0, 0.1, 0.5, 0.1],
        [
            195972.6183649697,
            1.0767369063135253e-13,
            0.1,
            0.0,
            0.0,
            0.1,
            1054061.7367287616,
        ],
    ] {
        let k = rows
            .points
            .iter()
            .position(|p| p[..] == x[..])
            .expect("a case-4 point");
        let [_, c] = evaluate(Functional::R2scan, point(&x));
        for (got, want) in c.vrho.iter().zip(&rows.correlation[k][1..3]) {
            assert!(
                (got - want).abs() <= 1e-12 * want.abs(),
                "{x:?}: {got} vs {want}"
            );
        }
    }
}

#[test]
fn every_functional_has_libxc_rows() {
    for functional in Functional::ALL.map(|row| row.functional) {
        let rows = rows(functional);
        assert!(!rows.points.is_empty(), "{functional:?}");
        assert_eq!(rows.exchange.len(), rows.points.len(), "{functional:?}");
        let others = match rows.inputs.as_str() {
            "floors" => [rows.exchange_scaled.len(); 3],
            "shipped" => {
                let u = rows
                    .unpolarized
                    .as_ref()
                    .expect("shipped rows hold unpolarized ones");
                [u.points.len(), u.exchange.len(), u.correlation.len()]
            }
            other => panic!("{functional:?}: inputs {other:?}"),
        };
        assert_eq!(others, [rows.points.len(); 3], "{functional:?}");
        assert_eq!(rows.correlation.len(), rows.points.len(), "{functional:?}");
    }
}

#[test]
fn the_uniform_gas_gives_slater_exchange() {
    for functional in Functional::ALL.map(|row| row.functional) {
        for rho in [1e-4, 0.03, 1.0, 20.0] {
            let p = point(&[rho, 0.5 * rho, 0.0, 0.0, 0.0]);
            let (x, _) = energy_density(functional, p);
            let slater = -0.75
                * (3.0 / PI).cbrt()
                * (rho.powf(4.0 / 3.0) + (0.5 * rho).powf(4.0 / 3.0))
                * 2.0_f64.cbrt();
            assert!(
                (x - slater).abs() <= 1e-11 * slater.abs(),
                "{functional:?} {rho}: {x} vs {slater}"
            );
        }
    }
}

#[test]
fn values_do_not_move_with_a_variable_above_the_rung() {
    for functional in Functional::ALL.map(|row| row.functional) {
        let rung = functional.rung();
        for p in every_point() {
            let bits = |q: Point| evaluate(functional, q).map(|v| row(v).map(f64::to_bits));
            let [uu, ud, dd] = p.sigma;
            let [tu, td] = p.tau().unwrap();
            let sigma = if rung >= Rung::Gga {
                [uu, ud, dd]
            } else {
                [3.0 * uu + 1.0, 0.0, 2.0 * dd + 7.0]
            };
            let tau = if rung >= Rung::MetaGga {
                [tu, td]
            } else {
                [2.0 * tu + 0.5, 40.0]
            };
            let moved = Point::of(p.up, p.down, sigma).with_tau(tau);
            assert_eq!(bits(p), bits(moved), "{functional:?} {p:?}");
        }
    }
}

fn libxc_leaves(p: &Point) -> bool {
    let [uu, ud, dd] = p.sigma;
    let [tu, td] = p.tau().unwrap();
    let spins = [(p.up, uu, tu), (p.down, dd, td)];
    spins
        .iter()
        .all(|&(rho, sigma, tau)| rho >= 1e-6 && tau > 0.0 && sigma <= 0.99 * 8.0 * rho * tau)
        && ud.abs() <= 0.99 * 0.5 * (uu + dd)
}

#[test]
fn every_partial_equals_a_central_difference_of_the_energy() {
    for table in Functional::ALL {
        let functional = table.functional;
        for p in every_point() {
            let dilute = [p.up, p.down].iter().any(|&rho| rho > 0.0 && rho < 1e-6);
            if dilute || (table.inputs == Inputs::Libxc && !libxc_leaves(&p)) {
                continue;
            }
            let values = evaluate(functional, p);
            let x = [
                p.up,
                p.down,
                p.sigma[0],
                p.sigma[1],
                p.sigma[2],
                p.tau().unwrap()[0],
                p.tau().unwrap()[1],
            ];
            for k in 0..7 {
                let h = 1e-5 * x[k].abs();
                let at = |shift: f64| {
                    let mut y = x;
                    y[k] += shift;
                    Point::of(y[0], y[1], [y[2], y[3], y[4]]).with_tau([y[5], y[6]])
                };
                let valid = |q: Point| {
                    let [uu, ud, dd] = q.sigma;
                    uu >= 0.0 && dd >= 0.0 && ud * ud <= uu * dd
                };
                if h == 0.0 || !valid(at(h)) || !valid(at(-h)) {
                    continue;
                }
                let (plus, minus) = (
                    energy_density(functional, at(h)),
                    energy_density(functional, at(-h)),
                );
                for (s, value) in values.iter().enumerate() {
                    let e = |pair: (f64, f64)| if s == 0 { pair.0 } else { pair.1 };
                    let difference = (e(plus) - e(minus)) / (2.0 * h);
                    let exact = row(*value)[k + 1];
                    let scale = exact.abs() + value.energy.abs() / x[k].abs();
                    assert!(
                        (difference - exact).abs() <= 1e-6 * scale,
                        "{functional:?} {p:?} variable {k} part {s}: {difference} vs {exact}"
                    );
                }
            }
        }
    }
}

#[test]
fn a_partial_above_the_rung_is_exactly_zero() {
    for functional in Functional::ALL.map(|row| row.functional) {
        let rung = functional.rung();
        for p in every_point() {
            for value in evaluate(functional, p) {
                for (k, v) in Variable::ALL.iter().enumerate() {
                    if !rung.reads(*v) {
                        assert_eq!(row(value)[k + 1], 0.0, "{functional:?} {v:?}");
                    }
                }
            }
            for (_, d) in partials(functional, p, [Variable::TauDown, Variable::SigmaUpDown]) {
                assert!(rung.reads(Variable::TauDown) || d[0] == 0.0);
                assert!(rung.reads(Variable::SigmaUpDown) || d[1] == 0.0);
            }
            let n = p.up + p.down;
            for u in unpolarized(functional, n, p.sigma[0], Some(p.tau().unwrap()[0])) {
                assert!(rung >= Rung::Gga || u.vsigma == 0.0, "{functional:?}");
                assert!(rung >= Rung::MetaGga || u.vtau == 0.0, "{functional:?}");
            }
        }
    }
}

#[test]
fn a_narrow_jet_gives_the_partials_of_evaluate_bit_for_bit_in_the_order_asked() {
    use Variable::{RhoUp, SigmaUpDown, SigmaUpUp, TauDown};
    for functional in Functional::ALL.map(|row| row.functional) {
        for p in every_point() {
            let narrow = partials(functional, p, [SigmaUpDown, TauDown, RhoUp, SigmaUpUp]);
            for (v, (e, d)) in evaluate(functional, p).iter().zip(narrow) {
                let want = [v.energy, v.vsigma[1], v.vtau[1], v.vrho[0], v.vsigma[0]];
                assert_eq!(
                    want.map(f64::to_bits),
                    [e, d[0], d[1], d[2], d[3]].map(f64::to_bits),
                    "{functional:?} {p:?}"
                );
            }
        }
    }
}

#[test]
fn exchange_obeys_spin_scaling() {
    for row in Functional::ALL {
        let functional = row.functional;
        for p in every_point() {
            if row.inputs == Inputs::Libxc && !libxc_leaves(&p) {
                continue;
            }
            let [uu, _, dd] = p.sigma;
            let [tu, td] = p.tau().unwrap();
            let half = |rho: f64, sigma: f64, tau: f64| {
                energy_density(
                    functional,
                    Point::of(rho, rho, [sigma; 3]).with_tau([tau; 2]),
                )
                .0 / 2.0
            };
            let (exchange, _) = energy_density(functional, p);
            let scaled = half(p.up, uu, tu) + half(p.down, dd, td);
            assert!(
                (exchange - scaled).abs() <= 1e-14 * exchange.abs(),
                "{functional:?} {p:?}: {exchange} vs {scaled}"
            );
        }
    }
}

#[test]
fn a_fully_polarized_exchange_is_half_the_unpolarized_exchange_at_twice_the_density() {
    for row in Functional::ALL
        .iter()
        .filter(|row| row.inputs == Inputs::Floors)
    {
        let functional = row.functional;
        for (rho, sigma, tau) in [(0.1, 0.01, 0.06), (1e-3, 2e-6, 3e-4), (2.0, 0.7, 58.0)] {
            let [x, _] = unpolarized(functional, 2.0 * rho, 4.0 * sigma, Some(2.0 * tau));
            for p in [
                Point::of(rho, 0.0, [sigma, 0.0, 0.0]).with_tau([tau, 0.0]),
                Point::of(0.0, rho, [0.0, 0.0, sigma]).with_tau([0.0, tau]),
            ] {
                let (exchange, _) = energy_density(functional, p);
                let half = x.energy / 2.0;
                assert!(
                    (exchange - half).abs() <= 1e-14 * half.abs(),
                    "{functional:?} {p:?}: {exchange} vs {half}"
                );
            }
        }
    }
}

fn sweep() -> Vec<Point> {
    let mut state: u64 = 761;
    let mut next = move || {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        (state >> 11) as f64 / (1u64 << 53) as f64
    };
    (0..10_000)
        .map(|k| {
            let mut spin = |empty: bool| {
                let rho = if empty {
                    0.0
                } else {
                    10f64.powf(-12.0 + 14.0 * next())
                };
                let w = 2.0 * (6.0 * PI * PI * rho).cbrt() * rho;
                let sigma = 10.0 * next() * w * w;
                let uniform = 0.3 * (6.0 * PI * PI).powf(2.0 / 3.0) * rho.powf(5.0 / 3.0);
                let tau = if empty {
                    0.0
                } else {
                    sigma / (8.0 * rho) + 5.0 * next() * uniform
                };
                (rho, sigma, tau)
            };
            let (up, uu, tu) = spin(false);
            let (down, dd, td) = spin(k % 10 == 0);
            let ud = (2.0 * next() - 1.0) * (uu * dd).sqrt();
            Point::of(up, down, [uu, ud, dd]).with_tau([tu, td])
        })
        .collect()
}

#[test]
fn energy_density_is_the_energy_of_evaluate_to_floating_point_precision() {
    let mut points = every_point();
    points.extend(sweep());
    for functional in Functional::ALL.map(|row| row.functional) {
        for &p in &points {
            let (x, c) = energy_density(functional, p);
            let [vx, vc] = evaluate(functional, p);
            for (a, b) in [(x, vx.energy), (c, vc.energy)] {
                assert!(
                    a == b || ((a - b) / b).abs() <= 2e-13,
                    "{functional:?} {p:?}: {a} vs {b}"
                );
            }
        }
    }
}

#[test]
fn swapping_the_spins_changes_nothing() {
    for functional in Functional::ALL.map(|row| row.functional) {
        for p in every_point() {
            let [uu, ud, dd] = p.sigma;
            let [tu, td] = p.tau().unwrap();
            let swapped = Point::of(p.down, p.up, [dd, ud, uu]).with_tau([td, tu]);
            assert_eq!(
                energy_density(functional, p),
                energy_density(functional, swapped)
            );
        }
    }
}

#[test]
#[should_panic(expected = "wanted twice")]
fn a_variable_wanted_twice_is_refused() {
    let p = Point::of(0.1, 0.1, [0.01; 3]);
    let _ = partials(
        Functional::ALL[0].functional,
        p,
        [Variable::RhoUp, Variable::SigmaUpUp, Variable::RhoUp],
    );
}

#[test]
fn a_meta_gga_without_tau_is_refused_by_name() {
    for row in Functional::ALL
        .iter()
        .filter(|row| row.rung == Rung::MetaGga)
    {
        let p = Point::of(0.1, 0.1, [0.01; 3]);
        let refused = std::panic::catch_unwind(|| evaluate(row.functional, p));
        let message = refused.unwrap_err();
        let text = message.downcast_ref::<String>().unwrap();
        assert!(
            text.contains(row.word) && text.contains("with_tau"),
            "{text}"
        );
        assert!(std::panic::catch_unwind(|| unpolarized(row.functional, 0.2, 0.01, None)).is_err());
    }
}

#[test]
fn values_stay_finite_at_zero_density_and_zero_tau() {
    for functional in Functional::ALL.map(|row| row.functional) {
        for point in [
            Point::of(0.2, 0.0, [0.01, 0.0, 0.0]),
            Point::of(0.0, 0.0, [0.0; 3]),
            Point::of(0.3, -1e-6, [0.02, 0.0, 0.0]),
            Point::of(0.2, 0.1, [0.01, 0.0, 0.0]),
            Point::of(1e-20, 1e-20, [1e-30; 3]),
        ] {
            for tau in [[0.0; 2], [0.1, 0.0], [-1e-9, 1e-30]] {
                for value in evaluate(functional, point.with_tau(tau)) {
                    let finite = row(value).iter().all(|v| v.is_finite());
                    assert!(finite, "{functional:?} {point:?} τ {tau:?}: {value:?}");
                }
            }
        }
        for (n, sigma, tau) in [
            (0.0, 0.0, 0.0),
            (0.1, 0.0, 0.0),
            (0.1, 1e-3, 0.0),
            (-1e-3, 0.0, -1.0),
        ] {
            for u in unpolarized(functional, n, sigma, Some(tau)) {
                let all = [u.energy, u.vrho, u.vsigma, u.vtau];
                assert!(
                    all.iter().all(|v| v.is_finite()),
                    "{functional:?} n {n} τ {tau}: {u:?}"
                );
            }
        }
    }
}

fn same_as_polarized(functional: Functional, n: f64, sigma: f64, tau: f64, at: &str) {
    let close = |got: f64, want: f64| (got - want).abs() <= 1e-13 * want.abs();
    let point = Point::of(n / 2.0, n / 2.0, [sigma / 4.0; 3]).with_tau([tau / 2.0; 2]);
    let both = evaluate(functional, point);
    for (got, want) in unpolarized(functional, n, sigma, Some(tau))
        .iter()
        .zip(&both)
    {
        let vrho = (want.vrho[0] + want.vrho[1]) / 2.0;
        let vsigma = want.vsigma.iter().sum::<f64>() / 4.0;
        let vtau = (want.vtau[0] + want.vtau[1]) / 2.0;
        let at = format!("{functional:?} {at}");
        assert!(close(got.energy, want.energy), "{at}: e {got:?} {want:?}");
        assert!(close(got.vrho, vrho), "{at}: v {} {vrho}", got.vrho);
        assert!(
            close(got.vsigma, vsigma),
            "{at}: vσ {} {vsigma}",
            got.vsigma
        );
        assert!(close(got.vtau, vtau), "{at}: vτ {} {vtau}", got.vtau);
    }
}

#[test]
fn the_unpolarized_entry_is_the_polarized_formula_at_equal_spins() {
    for row in Functional::ALL {
        let functional = row.functional;
        for decade in -10..=2 {
            for s2 in [0.0, 0.01, 0.3, 1.0, 10.0] {
                for alpha in [0.0, 0.5, 1.0, 3.0] {
                    let n = 3.7 * 10f64.powi(decade);
                    let w = 2.0 * (3.0 * PI.powi(2) * n).cbrt() * n;
                    let sigma = s2 * w * w;
                    let uniform = 0.3 * (3.0 * PI * PI).powf(2.0 / 3.0) * n.powf(5.0 / 3.0);
                    let tau = sigma / (8.0 * n) + alpha * uniform;
                    if row.inputs == Inputs::Libxc && (sigma == 0.0 || tau == 0.0) {
                        continue;
                    }
                    same_as_polarized(
                        functional,
                        n,
                        sigma,
                        tau,
                        &format!("n {n:e} s² {s2} α {alpha}"),
                    );
                }
            }
        }
    }
}

#[test]
fn an_unpolarized_density_at_or_below_the_floors_is_the_polarized_formula_at_floored_spins() {
    for row in Functional::ALL
        .iter()
        .filter(|row| row.inputs == Inputs::Floors)
    {
        let functional = row.functional;
        for n in [
            0.0,
            -1e-3,
            1e-15,
            1.5 * DENSITY_FLOOR_PER_BOHR3,
            2.0 * DENSITY_FLOOR_PER_BOHR3,
        ] {
            for sigma in [0.0, 1e-35, 1e-30] {
                for tau in [
                    0.0,
                    TAU_FLOOR_HA_PER_BOHR3,
                    2.0 * TAU_FLOOR_HA_PER_BOHR3,
                    1e-18,
                ] {
                    let point =
                        Point::of(n / 2.0, n / 2.0, [sigma / 4.0; 3]).with_tau([tau / 2.0; 2]);
                    let close = |got: f64, want: f64| (got - want).abs() <= 1e-13 * want.abs();
                    for (got, want) in unpolarized(functional, n, sigma, Some(tau))
                        .iter()
                        .zip(evaluate(functional, point))
                    {
                        let at = format!("{functional:?} n {n:e} σ {sigma:e} τ {tau:e}");
                        assert!(close(got.energy, want.energy), "{at}: e {got:?} {want:?}");
                        assert!(close(got.vrho, (want.vrho[0] + want.vrho[1]) / 2.0), "{at}");
                    }
                }
            }
        }
    }
}

#[test]
fn a_point_counts_as_prepared_exactly_where_its_row_changes_it() {
    let pbe = Functional::Pbe;
    assert_eq!(pbe.row().inputs, Inputs::Floors);
    let polarized = |up: f64, down: f64| prepares(pbe, Point::of(up, down, [0.01; 3]));
    let at = DENSITY_FLOOR_PER_BOHR3;
    assert_eq!(
        [
            polarized(at, at),
            polarized(at / 2.0, 0.1),
            polarized(0.1, at / 2.0),
            polarized(0.1, 0.1)
        ],
        [false, true, true, false]
    );
    assert_eq!(
        [
            prepares_unpolarized(pbe, 2.0 * at, 0.01, None),
            prepares_unpolarized(pbe, at, 0.01, None),
            prepares_unpolarized(pbe, 0.1, 0.01, None),
        ],
        [false, true, false]
    );
    let r2scan = Functional::R2scan;
    assert_eq!(r2scan.row().inputs, Inputs::Libxc);
    let whole = |n: f64| prepares_unpolarized(r2scan, n, 1e-29, Some(1e-10));
    assert_eq!([whole(1.5e-11), whole(3e-11)], [true, false]);
    let spins = |down: f64, uu: f64| {
        prepares(
            r2scan,
            Point::of(0.1, down, [uu, 0.0, 1e-29]).with_tau([1e-10, 1e-10]),
        )
    };
    assert_eq!(
        [spins(0.1, 1e-9), spins(1e-11, 1e-29), spins(0.1, 1e-29)],
        [true, true, false]
    );
}
