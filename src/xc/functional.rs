use super::libxc::{self, Part};
use super::{lda, pbe, r2scan};
use crate::model::Functional;
use hyperjet::jets::Jet1;
use std::f64::consts::PI;
use std::ops::{Add, Div, Mul, Neg, Sub};

pub(super) trait Real:
    Copy
    + Add<Output = Self>
    + Sub<Output = Self>
    + Mul<Output = Self>
    + Div<Output = Self>
    + Neg<Output = Self>
    + Add<f64, Output = Self>
    + Sub<f64, Output = Self>
    + Mul<f64, Output = Self>
    + Div<f64, Output = Self>
{
    fn value(self) -> f64;
    fn constant(x: f64) -> Self;
    fn chained(value: f64, along: [(Self, f64); 2]) -> Self;
    fn ln(self) -> Self;
    fn exp(self) -> Self;
    fn sqrt(self) -> Self;
    fn cbrt(self) -> Self;
}

impl Real for f64 {
    fn value(self) -> f64 {
        self
    }
    fn constant(x: f64) -> Self {
        x
    }
    fn chained(value: f64, _: [(Self, f64); 2]) -> Self {
        value
    }
    fn ln(self) -> Self {
        f64::ln(self)
    }
    fn exp(self) -> Self {
        f64::exp(self)
    }
    fn sqrt(self) -> Self {
        f64::sqrt(self)
    }
    fn cbrt(self) -> Self {
        f64::cbrt(self)
    }
}

impl<const N: usize> Real for Jet1<N> {
    fn value(self) -> f64 {
        self.value
    }
    fn constant(x: f64) -> Self {
        Jet1::constant(x)
    }
    fn chained(value: f64, [(a, p), (b, q)]: [(Self, f64); 2]) -> Self {
        Jet1 {
            value,
            grad: std::array::from_fn(|k| p * a.grad[k] + q * b.grad[k]),
        }
    }
    fn ln(self) -> Self {
        Jet1::ln(self)
    }
    fn exp(self) -> Self {
        Jet1::exp(self)
    }
    fn sqrt(self) -> Self {
        Jet1::sqrt(self)
    }
    fn cbrt(self) -> Self {
        let c = self.value.cbrt();
        Jet1 {
            value: c,
            grad: self.grad.map(|g| g * c / (3.0 * self.value)),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Rung {
    Lda,
    Gga,
    MetaGga,
}

impl Rung {
    pub fn reads(self, variable: Variable) -> bool {
        Variable::ALL[..self.count()].contains(&variable)
    }

    fn count(self) -> usize {
        match self {
            Rung::Lda => 2,
            Rung::Gga => 5,
            Rung::MetaGga => 7,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Inputs {
    Floors,
    Libxc,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    pub functional: Functional,
    pub word: &'static str,
    pub rung: Rung,
    pub inputs: Inputs,
    pub dataset: (&'static str, &'static str),
}

impl Functional {
    pub const ALL: [Row; 3] = [
        Row {
            functional: Functional::Lda,
            word: "lda",
            rung: Rung::Lda,
            inputs: Inputs::Floors,
            dataset: ("LDA", "PW"),
        },
        Row {
            functional: Functional::Pbe,
            word: "pbe",
            rung: Rung::Gga,
            inputs: Inputs::Floors,
            dataset: ("GGA", "PBE"),
        },
        Row {
            functional: Functional::R2scan,
            word: "r2scan",
            rung: Rung::MetaGga,
            inputs: Inputs::Libxc,
            dataset: ("GGA", "PBE"),
        },
    ];

    pub fn row(self) -> Row {
        *Functional::ALL
            .iter()
            .find(|row| row.functional == self)
            .expect("Functional::ALL holds every functional")
    }

    pub fn rung(self) -> Rung {
        self.row().rung
    }

    pub fn dataset(self) -> (&'static str, &'static str) {
        self.row().dataset
    }

    pub fn of_dataset(kind: &str, name: &str) -> Option<Functional> {
        (Functional::ALL.iter())
            .find(|row| row.dataset == (kind, name))
            .map(|row| row.functional)
    }
}

pub const DENSITY_FLOOR_PER_BOHR3: f64 = 1e-14;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub up: f64,
    pub down: f64,
    pub sigma: [f64; 3],
    tau: Option<[f64; 2]>,
}

impl Point {
    pub fn of(up: f64, down: f64, sigma: [f64; 3]) -> Self {
        Point {
            up,
            down,
            sigma,
            tau: None,
        }
    }

    pub fn with_tau(self, tau: [f64; 2]) -> Self {
        Point {
            tau: Some(tau),
            ..self
        }
    }

    pub fn tau(&self) -> Option<[f64; 2]> {
        self.tau
    }

    fn values(self, functional: Functional) -> [f64; 7] {
        let [tu, td] = match (functional.rung(), self.tau) {
            (_, Some(tau)) => tau,
            (Rung::MetaGga, None) => panic!(
                "xc: {} reads τ; give it with Point::with_tau",
                functional.row().word
            ),
            (_, None) => [0.0; 2],
        };
        let [uu, ud, dd] = self.sigma;
        [self.up, self.down, uu, ud, dd, tu, td]
    }
}

pub fn energy_density(functional: Functional, point: Point) -> (f64, f64) {
    let values = point.values(functional);
    let inputs = functional.row().inputs;
    let [x, c] = parts(functional, values, |v| v)
        .map(|part| weighted(inputs, part, values[0] + values[1]).0);
    (x, c)
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Value {
    pub energy: f64,
    pub epsilon: f64,
    pub vrho: [f64; 2],
    pub vsigma: [f64; 3],
    pub vtau: [f64; 2],
}

pub fn evaluate(functional: Functional, point: Point) -> [Value; 2] {
    match functional.rung() {
        Rung::Lda => leading::<2>(functional, point),
        Rung::Gga => leading::<5>(functional, point),
        Rung::MetaGga => leading::<7>(functional, point),
    }
}

fn leading<const N: usize>(functional: Functional, point: Point) -> [Value; 2] {
    let wanted: [Variable; N] = std::array::from_fn(|k| Variable::ALL[k]);
    jets(functional, point, wanted).map(|(energy, epsilon, d)| {
        let at = |k: usize| if k < N { d[k] } else { 0.0 };
        Value {
            energy,
            epsilon,
            vrho: [at(0), at(1)],
            vsigma: [at(2), at(3), at(4)],
            vtau: [at(5), at(6)],
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Unpolarized {
    pub energy: f64,
    pub epsilon: f64,
    pub vrho: f64,
    pub vsigma: f64,
    pub vtau: f64,
}

pub fn unpolarized(
    functional: Functional,
    n: f64,
    sigma: f64,
    tau: Option<f64>,
) -> [Unpolarized; 2] {
    let tau = match (functional.rung(), tau) {
        (_, Some(tau)) => tau,
        (Rung::MetaGga, None) => panic!(
            "xc: {} reads τ; give it to xc::unpolarized",
            functional.row().word
        ),
        (_, None) => 0.0,
    };
    match functional.rung() {
        Rung::Lda => spinless::<1>(functional, [n, sigma, tau]),
        Rung::Gga => spinless::<2>(functional, [n, sigma, tau]),
        Rung::MetaGga => spinless::<3>(functional, [n, sigma, tau]),
    }
}

fn spinless<const N: usize>(functional: Functional, x: [f64; 3]) -> [Unpolarized; 2] {
    let seed = |x: [f64; 3]| -> [Jet1<N>; 3] {
        std::array::from_fn(|k| match k < N {
            true => Jet1::variable(x[k], k),
            false => Jet1::constant(x[k]),
        })
    };
    let inputs = functional.row().inputs;
    let f = formulas::<Jet1<N>>(functional);
    let prepared: [Option<(Jet1<N>, f64)>; 2] = match inputs {
        Inputs::Floors => {
            let x = [x[0].max(2.0 * DENSITY_FLOOR_PER_BOHR3), x[1], x[2]];
            let point = spinless_point(seed(x));
            f.unpolarized.map(|part| Some((part(point), x[0])))
        }
        Inputs::Libxc => [
            (Part::Exchange, f.unpolarized[0]),
            (Part::Correlation, f.unpolarized[1]),
        ]
        .map(|(part, formula)| {
            let y = part.unpolarized(x)?;
            (!part.drops(y[0] / 2.0)).then(|| (formula(spinless_point(seed(y))), y[0]))
        }),
    };
    prepared.map(|part| {
        let (energy, epsilon) = weighted(inputs, part.map(|(e, n)| (e.value, n)), x[0]);
        let at = |k: usize| match (part, k < N) {
            (Some((e, _)), true) => e.grad[k],
            _ => 0.0,
        };
        Unpolarized {
            energy,
            epsilon,
            vrho: at(0),
            vsigma: at(1),
            vtau: at(2),
        }
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Variable {
    RhoUp,
    RhoDown,
    SigmaUpUp,
    SigmaUpDown,
    SigmaDownDown,
    TauUp,
    TauDown,
}

impl Variable {
    pub const ALL: [Variable; 7] = [
        Variable::RhoUp,
        Variable::RhoDown,
        Variable::SigmaUpUp,
        Variable::SigmaUpDown,
        Variable::SigmaDownDown,
        Variable::TauUp,
        Variable::TauDown,
    ];
}

pub fn prepares(functional: Functional, point: Point) -> bool {
    let values = point.values(functional);
    match functional.row().inputs {
        Inputs::Floors => {
            values[0] < DENSITY_FLOOR_PER_BOHR3 || values[1] < DENSITY_FLOOR_PER_BOHR3
        }
        Inputs::Libxc => [Part::Exchange, Part::Correlation].into_iter().any(|part| {
            match part.polarized(values) {
                None => true,
                Some(p) => p != values || part.drops(p[0]) || part.drops(p[1]),
            }
        }),
    }
}

pub fn prepares_unpolarized(functional: Functional, n: f64, sigma: f64, tau: Option<f64>) -> bool {
    let given = [n, sigma, tau.unwrap_or(0.0)];
    match functional.row().inputs {
        Inputs::Floors => n < 2.0 * DENSITY_FLOOR_PER_BOHR3,
        Inputs::Libxc => [Part::Exchange, Part::Correlation].into_iter().any(|part| {
            match part.unpolarized(given) {
                None => true,
                Some(p) => p != given || part.drops(p[0] / 2.0),
            }
        }),
    }
}

pub fn partials<const N: usize>(
    functional: Functional,
    point: Point,
    wanted: [Variable; N],
) -> [(f64, [f64; N]); 2] {
    jets(functional, point, wanted).map(|(energy, _, d)| (energy, d))
}

fn jets<const N: usize>(
    functional: Functional,
    point: Point,
    wanted: [Variable; N],
) -> [(f64, f64, [f64; N]); 2] {
    for (k, v) in wanted.iter().enumerate() {
        assert!(
            !wanted[..k].contains(v),
            "xc::partials: {v:?} is wanted twice"
        );
    }
    let rung = functional.rung();
    let seed = |values: [f64; 7]| -> [Jet1<N>; 7] {
        std::array::from_fn(|k| {
            let v = Variable::ALL[k];
            match wanted.iter().position(|&w| w == v) {
                Some(slot) if rung.reads(v) => Jet1::variable(values[k], slot),
                _ => Jet1::constant(values[k]),
            }
        })
    };
    let read = wanted.map(|v| rung.reads(v));
    let values = point.values(functional);
    let inputs = functional.row().inputs;
    parts(functional, values, seed).map(|part| {
        let (energy, epsilon) = weighted(
            inputs,
            part.map(|(e, n)| (e.value, n)),
            values[0] + values[1],
        );
        let grad = std::array::from_fn(|slot| match part {
            Some((e, _)) if read[slot] => e.grad[slot],
            _ => 0.0,
        });
        (energy, epsilon, grad)
    })
}

fn weighted(inputs: Inputs, part: Option<(f64, f64)>, given: f64) -> (f64, f64) {
    match (inputs, part) {
        (_, None) => (0.0, 0.0),
        (Inputs::Floors, Some((e, n))) => (e, e / n),
        (Inputs::Libxc, Some((e, n))) => {
            let epsilon = e / n;
            (epsilon * given, epsilon)
        }
    }
}

fn parts<T: Real>(
    functional: Functional,
    values: [f64; 7],
    seed: impl Fn([f64; 7]) -> [T; 7],
) -> [Option<(T, f64)>; 2] {
    let f = formulas::<T>(functional);
    match functional.row().inputs {
        Inputs::Floors => {
            let mut floored = values;
            floored[0] = values[0].max(DENSITY_FLOOR_PER_BOHR3);
            floored[1] = values[1].max(DENSITY_FLOOR_PER_BOHR3);
            let v = seed(floored);
            let n = floored[0] + floored[1];
            let spin = f
                .spin_exchange
                .expect("xc: a floors row names its spin exchange");
            [
                Some((spin(v[0], v[2], v[5]) + spin(v[1], v[4], v[6]), n)),
                Some(((f.correlation)(polarized(v)), n)),
            ]
        }
        Inputs::Libxc => [Part::Exchange, Part::Correlation].map(|part| {
            let p = part.polarized(values)?;
            let v = seed(p);
            let e = match part {
                Part::Exchange => libxc::exchange(v, f.unpolarized[0]),
                Part::Correlation => (f.correlation)(libxc::polarized(v)),
            };
            Some((e, p[0] + p[1]))
        }),
    }
}

#[derive(Clone, Copy)]
pub(super) struct Polarized<T> {
    pub n: T,
    pub c: T,
    pub rs: T,
    pub spin: Spin<T>,
    pub sigma: [T; 3],
    pub tau: T,
    pub uniform: T,
}

#[derive(Clone, Copy)]
pub(super) struct Spinless<T> {
    pub n: T,
    pub c: T,
    pub rs: T,
    pub sigma: T,
    pub tau: T,
}

fn polarized<T: Real>([up, down, uu, ud, dd, ..]: [T; 7]) -> Polarized<T> {
    let n = up + down;
    let (plus, minus) = (up * 2.0 / n, down * 2.0 / n);
    let z = (up - down) / n;
    let unread = T::constant(f64::NAN);
    let spin = Spin::of([z * z * (z * z), unread], plus, minus);
    let (rs, c) = wigner_seitz(n);
    Polarized {
        n,
        c,
        rs,
        spin,
        sigma: [uu, ud, dd],
        tau: unread,
        uniform: unread,
    }
}

pub(super) fn spinless_point<T: Real>([n, sigma, tau]: [T; 3]) -> Spinless<T> {
    let (rs, c) = wigner_seitz(n);
    Spinless {
        n,
        c,
        rs,
        sigma,
        tau,
    }
}

pub(super) struct Formulas<T> {
    spin_exchange: Option<fn(T, T, T) -> T>,
    correlation: fn(Polarized<T>) -> T,
    unpolarized: [fn(Spinless<T>) -> T; 2],
}

fn formulas<T: Real>(functional: Functional) -> Formulas<T> {
    match functional {
        Functional::Lda => Formulas {
            spin_exchange: Some(lda::spin_exchange),
            correlation: lda::correlation,
            unpolarized: [lda::unpolarized_exchange, lda::unpolarized_correlation],
        },
        Functional::Pbe => Formulas {
            spin_exchange: Some(pbe::spin_exchange),
            correlation: pbe::correlation,
            unpolarized: [pbe::unpolarized_exchange, pbe::unpolarized_correlation],
        },
        Functional::R2scan => Formulas {
            spin_exchange: None,
            correlation: r2scan::correlation,
            unpolarized: [
                r2scan::unpolarized_exchange,
                r2scan::unpolarized_correlation,
            ],
        },
    }
}

pub(super) fn wigner_seitz<T: Real>(n: T) -> (T, T) {
    let rs = (T::constant(3.0 / (4.0 * PI)) / n).cbrt();
    (rs, T::constant((3.0 / (4.0 * PI)).cbrt()) / rs)
}

#[derive(Clone, Copy)]
pub(super) struct Spin<T> {
    pub z4: T,
    pub z12: T,
    pub plus: T,
    pub minus: T,
    pub cbrt_plus: T,
    pub cbrt_minus: T,
}

impl<T: Real> Spin<T> {
    pub fn of([z4, z12]: [T; 2], plus: T, minus: T) -> Self {
        Spin {
            z4,
            z12,
            plus,
            minus,
            cbrt_plus: plus.cbrt(),
            cbrt_minus: minus.cbrt(),
        }
    }

    pub fn f(self) -> T {
        (self.plus * self.cbrt_plus + self.minus * self.cbrt_minus - 2.0)
            / (2.0_f64.powf(4.0 / 3.0) - 2.0)
    }

    pub fn phi(self) -> T {
        (self.cbrt_plus * self.cbrt_plus + self.cbrt_minus * self.cbrt_minus) / 2.0
    }

    pub fn d(self) -> T {
        (self.plus * self.cbrt_plus * self.cbrt_plus
            + self.minus * self.cbrt_minus * self.cbrt_minus)
            / 2.0
    }
}
