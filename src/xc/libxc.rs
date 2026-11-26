use super::functional::{Polarized, Real, Spin, Spinless, spinless_point, wigner_seitz};
use super::lda::slater;

pub(super) const ZETA_THRESHOLD: f64 = f64::EPSILON;
pub const TAU_FLOOR_HA_PER_BOHR3: f64 = 1e-20;

pub(super) const EXCHANGE_DENSITY_THRESHOLD_PER_BOHR3: f64 = 1e-11;
pub(super) const CORRELATION_DENSITY_THRESHOLD_PER_BOHR3: f64 = 1e-15;

const OTHER_BELOW: f64 = 2.0_f64.next_down();

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Part {
    Exchange,
    Correlation,
}

impl Part {
    fn density(self) -> f64 {
        match self {
            Part::Exchange => EXCHANGE_DENSITY_THRESHOLD_PER_BOHR3,
            Part::Correlation => CORRELATION_DENSITY_THRESHOLD_PER_BOHR3,
        }
    }

    fn sigma(self) -> f64 {
        let s = self.density().powf(4.0 / 3.0);
        s * s
    }

    pub(super) fn polarized(self, [up, down, uu, ud, dd, tu, td]: [f64; 7]) -> Option<[f64; 7]> {
        if up + down < self.density() {
            return None;
        }
        let [(up, uu, tu), (down, dd, td)] =
            [(up, uu, tu), (down, dd, td)].map(|(rho, sigma, tau)| {
                let (rho, tau) = (rho.max(self.density()), tau.max(TAU_FLOOR_HA_PER_BOHR3));
                (rho, sigma.max(self.sigma()).min(8.0 * rho * tau), tau)
            });
        let average = 0.5 * (uu + dd);
        let ud = if ud >= -average { ud } else { -average };
        let ud = if ud <= average { ud } else { average };
        Some([up, down, uu, ud, dd, tu, td])
    }

    pub(super) fn unpolarized(self, [n, sigma, tau]: [f64; 3]) -> Option<[f64; 3]> {
        if n < self.density() {
            return None;
        }
        let (n, tau) = (n.max(self.density()), tau.max(TAU_FLOOR_HA_PER_BOHR3));
        Some([n, sigma.max(self.sigma()).min(8.0 * n * tau), tau])
    }

    pub(super) fn drops(self, rho: f64) -> bool {
        self == Part::Exchange && rho <= self.density()
    }
}

fn zeta<T: Real>(up: T, down: T) -> (T, T) {
    let n = up + down;
    let (d, r) = ((up - down).value(), 1.0 / n.value());
    let square = d * (1.0 / (n.value() * n.value()));
    (
        T::chained(d * r, [(up, r - square), (down, -r - square)]),
        n,
    )
}

fn powers<T: Real>(up: T, down: T) -> [T; 2] {
    let (d, n) = ((up - down).value(), (up + down).value());
    let (d2, n2) = (d * d, n * n);
    let (d4, n4) = (d2 * d2, n2 * n2);
    let (d8, n8) = (d4 * d4, n4 * n4);
    let fourth = {
        let (a, b) = (d2 * d * (1.0 / n4), d4 * (1.0 / (n4 * n)));
        T::chained(
            d4 * (1.0 / n4),
            [(up, 4.0 * a - 4.0 * b), (down, -4.0 * a - 4.0 * b)],
        )
    };
    let twelfth = {
        let (a, b) = (
            d8 * (d2 * d) * (1.0 / (n8 * n4)),
            d8 * d4 * (1.0 / (n8 * (n4 * n))),
        );
        T::chained(
            d8 * d4 * (1.0 / (n8 * n4)),
            [(up, 12.0 * a - 12.0 * b), (down, -12.0 * a - 12.0 * b)],
        )
    };
    [fourth, twelfth]
}

fn floored<T: Real>(x: T) -> T {
    match x.value() <= ZETA_THRESHOLD {
        true => T::constant(ZETA_THRESHOLD),
        false => x,
    }
}

fn five_thirds<T: Real>(x: T) -> T {
    match x.value() == 0.0 {
        true => T::constant(0.0),
        false => {
            let c = x.cbrt();
            x * c * c
        }
    }
}

pub(super) fn exchange<T: Real>(v: [T; 7], unpolarized: fn(Spinless<T>) -> T) -> T {
    let (z, n) = zeta(v[0], v[1]);
    let small = |s: usize| (v[s] * 2.0 * (T::constant(1.0) / n)).value() <= ZETA_THRESHOLD;
    let spin = |s: usize| {
        if Part::Exchange.drops(v[s].value()) {
            return T::constant(0.0);
        }
        let other = 1 - s;
        let share = match (small(s), small(other)) {
            (true, _) => T::constant(ZETA_THRESHOLD),
            (false, true) => T::constant(OTHER_BELOW),
            (false, false) if s == 0 => z + 1.0,
            (false, false) => -z + 1.0,
        };
        let m = floored(share) * n;
        let own = spinless_point([v[s] * 2.0, v[2 + 2 * s] * 4.0, v[5 + s] * 2.0]);
        let enhancement = unpolarized(own) / slater(own.n, own.c);
        slater(m, m.cbrt()) * enhancement * 0.5
    };
    spin(0) + spin(1)
}

pub(super) fn polarized<T: Real>([up, down, uu, ud, dd, tu, td]: [T; 7]) -> Polarized<T> {
    let (z, n) = zeta(up, down);
    let (plus, minus) = (z + 1.0, -z + 1.0);
    let spin = Spin::of(powers(up, down), floored(plus), floored(minus));
    let (rs, c) = wigner_seitz(n);
    let shares = [plus * 0.5, minus * 0.5].map(five_thirds);
    let reduced = |tau: T, rho: T| tau / five_thirds(rho);
    Polarized {
        n,
        c,
        rs,
        spin,
        sigma: [uu, ud, dd],
        tau: n * c * c * (reduced(tu, up) * shares[0] + reduced(td, down) * shares[1]),
        uniform: (shares[0] + shares[1]) * 2.0_f64.cbrt().powi(2),
    }
}
