use super::Grid;
use super::per_point;
use crate::model::Functional;
use crate::xc::{
    DENSITY_FLOOR_PER_BOHR3, Inputs, Point, Rung, Variable, energy_density, partials, prepares,
    prepares_unpolarized, unpolarized,
};
use Variable::{RhoDown, RhoUp, SigmaDownDown, SigmaUpDown, SigmaUpUp, TauDown, TauUp};

#[derive(Clone, Debug, PartialEq)]
pub struct Xc {
    pub energy_ha: f64,
    pub potential: Vec<Vec<f64>>,
    pub gradient_strain_ha: [[f64; 3]; 3],
    pub kinetic_potential: Option<Vec<Vec<f64>>>,
}

#[derive(Clone, Copy, Debug)]
pub struct Kinetic<'a> {
    pub valence: Spins<'a>,
    pub core: Option<&'a [f64]>,
}

#[derive(Clone, Copy, Debug)]
pub enum Spins<'a> {
    Unpolarized(&'a [f64]),
    Polarized([&'a [f64]; 2]),
}

pub fn exchange_correlation(
    grid: &Grid,
    functional: Functional,
    spins: Spins<'_>,
    core: Option<&[f64]>,
    kinetic: Option<Kinetic<'_>>,
) -> Xc {
    evaluated(grid, functional, spins, core, kinetic, false).0
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridPoint {
    pub spins: usize,
    pub rho: [f64; 2],
    pub sigma: [f64; 3],
    pub tau: [f64; 2],
    pub v_rho: [f64; 2],
    pub v_sigma: [f64; 3],
    pub v_tau: [f64; 2],
    pub prepared: bool,
}

pub fn grid_points(
    grid: &Grid,
    functional: Functional,
    spins: Spins<'_>,
    core: Option<&[f64]>,
    kinetic: Option<Kinetic<'_>>,
) -> Vec<GridPoint> {
    (evaluated(grid, functional, spins, core, kinetic, true).1)
        .expect("evaluated records the points it is asked to")
}

pub fn grid_energy(functional: Functional, point: &GridPoint) -> f64 {
    let floor = |x: f64| match functional.row().inputs {
        Inputs::Libxc => x.max(DENSITY_FLOOR_PER_BOHR3),
        Inputs::Floors => x,
    };
    let meta = functional.rung().reads(TauUp);
    if point.spins == 1 {
        let tau = meta.then(|| floor(point.tau[0]));
        return unpolarized_summed(functional, floor(point.rho[0]), point.sigma[0], tau).0;
    }
    let mut at = Point::of(floor(point.rho[0]), floor(point.rho[1]), point.sigma);
    if meta {
        at = at.with_tau(point.tau.map(floor));
    }
    let (x, c) = energy_density(functional, at);
    x + c
}

pub fn grid_defect(
    functional: Functional,
    centre: &GridPoint,
    plus: &GridPoint,
    minus: &GridPoint,
) -> f64 {
    let contraction: f64 = (0..2)
        .map(|s| {
            centre.v_rho[s] * (plus.rho[s] - minus.rho[s])
                + centre.v_tau[s] * (plus.tau[s] - minus.tau[s])
        })
        .chain((0..3).map(|k| centre.v_sigma[k] * (plus.sigma[k] - minus.sigma[k])))
        .sum();
    grid_energy(functional, plus) - grid_energy(functional, minus) - contraction
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridGap {
    pub points: usize,
    pub predicted_ha: f64,
    pub bound_ha: f64,
}

pub fn grid_gap(
    functional: Functional,
    element: f64,
    centre: &[GridPoint],
    plus: &[GridPoint],
    minus: &[GridPoint],
) -> Result<GridGap, String> {
    if plus.len() != centre.len() || minus.len() != centre.len() {
        return Err(format!(
            "grid gap: the centre holds {} grid points, the plus state {}, the minus state {}",
            centre.len(),
            plus.len(),
            minus.len()
        ));
    }
    let (mut points, mut predicted, mut bound) = (0, 0.0, 0.0);
    for ((c, p), m) in centre.iter().zip(plus).zip(minus) {
        let defect = element * grid_defect(functional, c, p, m);
        bound += defect.abs();
        if c.prepared || p.prepared || m.prepared {
            points += 1;
            predicted += defect;
        }
    }
    Ok(GridGap {
        points,
        predicted_ha: predicted,
        bound_ha: bound,
    })
}

fn evaluated(
    grid: &Grid,
    functional: Functional,
    spins: Spins<'_>,
    core: Option<&[f64]>,
    kinetic: Option<Kinetic<'_>>,
    record: bool,
) -> (Xc, Option<Vec<GridPoint>>) {
    let rung = functional.rung();
    let kinetic = kinetic.filter(|_| rung.reads(TauUp));
    assert!(
        kinetic.is_some() || !rung.reads(TauUp),
        "pw::exchange_correlation: {} reads τ; give it",
        functional.row().word
    );
    let n = grid.dims().iter().product();
    let (densities, weight): (Vec<&[f64]>, f64) = match spins {
        Spins::Unpolarized(rho) => (vec![rho], 0.5),
        Spins::Polarized([up, down]) => (vec![up, down], 1.0),
    };
    assert!(
        densities
            .iter()
            .chain(core.as_slice())
            .all(|f| f.len() == n),
        "one density value per grid point"
    );
    let channels: Vec<Vec<f64>> = (densities.iter())
        .map(|rho| per_point(n, |r| rho[r] * weight + core.map_or(0.0, |c| c[r] * 0.5)))
        .collect();
    let taus: Option<Vec<Vec<f64>>> = kinetic.map(|k| {
        let valence: Vec<&[f64]> = match (spins, k.valence) {
            (Spins::Unpolarized(_), Spins::Unpolarized(t)) => vec![t],
            (Spins::Polarized(_), Spins::Polarized([up, down])) => vec![up, down],
            _ => panic!("pw::exchange_correlation: τ in another spin layout than ρ"),
        };
        assert!(
            valence
                .iter()
                .chain(k.core.as_slice())
                .all(|f| f.len() == n),
            "one τ value per grid point"
        );
        (valence.iter())
            .map(|t| per_point(n, |r| t[r] * weight + k.core.map_or(0.0, |c| c[r] * 0.5)))
            .collect()
    });
    let gradients: Option<Vec<[Vec<f64>; 3]>> = (rung >= Rung::Gga).then(|| {
        let fields: Vec<&[f64]> = channels.iter().map(Vec::as_slice).collect();
        grid.gradients(&fields)
    });
    let last = channels.len() - 1;
    let floor = |x: f64| match functional.row().inputs {
        Inputs::Libxc => x.max(DENSITY_FLOOR_PER_BOHR3),
        Inputs::Floors => x,
    };
    let g = |s: usize, r: usize| -> [f64; 3] {
        let gradients = gradients.as_ref().expect("a GGA's gradients");
        std::array::from_fn(|a| gradients[s][a][r])
    };
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let tau = |s: usize, r: usize| taus.as_ref().map(|t| t[s][r]);
    let at = |r: usize| {
        let g = |s: usize| g(s, r);
        let tau = |s: usize| tau(s, r);
        let point = |sigma| Point::of(floor(channels[0][r]), floor(channels[last][r]), sigma);
        match (gradients.is_some(), spins) {
            (false, Spins::Unpolarized(_)) => {
                let (e, v, _, _) =
                    unpolarized_summed(functional, floor(2.0 * channels[0][r]), 0.0, None);
                (e, [v, 0.0], [0.0; 2], 0.0, [0.0; 2])
            }
            (false, Spins::Polarized(_)) => {
                let (e, vrho) = summed(functional, point([0.0; 3]), [RhoUp, RhoDown]);
                (e, vrho, [0.0; 2], 0.0, [0.0; 2])
            }
            (true, Spins::Unpolarized(_)) => {
                let sigma = 4.0 * dot(g(0), g(0));
                let whole = tau(0).map(|t| floor(2.0 * t));
                let (e, v, vsigma, vtau) =
                    unpolarized_summed(functional, floor(2.0 * channels[0][r]), sigma, whole);
                (e, [v, 0.0], [2.0 * vsigma, 0.0], 0.0, [vtau, 0.0])
            }
            (true, Spins::Polarized(_)) => {
                let sigma = [dot(g(0), g(0)), dot(g(0), g(1)), dot(g(1), g(1))];
                match (tau(0), tau(last)) {
                    (Some(up), Some(down)) => {
                        let point = point(sigma).with_tau([floor(up), floor(down)]);
                        let wanted = [
                            RhoUp,
                            RhoDown,
                            SigmaUpUp,
                            SigmaUpDown,
                            SigmaDownDown,
                            TauUp,
                            TauDown,
                        ];
                        let (e, [up, down, uu, ud, dd, tu, td]) = summed(functional, point, wanted);
                        (e, [up, down], [uu, dd], ud, [tu, td])
                    }
                    _ => {
                        let wanted = [RhoUp, RhoDown, SigmaUpUp, SigmaUpDown, SigmaDownDown];
                        let (e, [up, down, uu, ud, dd]) = summed(functional, point(sigma), wanted);
                        (e, [up, down], [uu, dd], ud, [0.0; 2])
                    }
                }
            }
        }
    };
    let local = per_point(n, |r| {
        let (energy, vrho, own, mixed, vtau) = at(r);
        let g = |s: usize| g(s, r);
        let flux: [[f64; 3]; 2] = std::array::from_fn(|s| {
            if gradients.is_none() || s > last {
                return [0.0; 3];
            }
            std::array::from_fn(|a| 2.0 * own[s] * g(s)[a] + mixed * g(last - s)[a])
        });
        (energy, vrho, flux, vtau)
    });
    let points = record.then(|| {
        per_point(n, |r| {
            let (_, vrho, own, mixed, vtau) = at(r);
            let tau = |s: usize| tau(s, r);
            let gradient = |s: usize| match &gradients {
                Some(_) => g(s, r),
                None => [0.0; 3],
            };
            match spins {
                Spins::Unpolarized(_) => {
                    let (n, sigma) = (2.0 * channels[0][r], 4.0 * dot(gradient(0), gradient(0)));
                    let whole = tau(0).map(|t| 2.0 * t);
                    let floored = floor(n) != n || whole.is_some_and(|t| floor(t) != t);
                    GridPoint {
                        spins: 1,
                        rho: [n, 0.0],
                        sigma: [sigma, 0.0, 0.0],
                        tau: [whole.unwrap_or(0.0), 0.0],
                        v_rho: [vrho[0], 0.0],
                        v_sigma: [0.5 * own[0], 0.0, 0.0],
                        v_tau: [vtau[0], 0.0],
                        prepared: floored || prepares_unpolarized(functional, n, sigma, whole),
                    }
                }
                Spins::Polarized(_) => {
                    let rho = [channels[0][r], channels[last][r]];
                    let (a, b) = (gradient(0), gradient(last));
                    let sigma = [dot(a, a), dot(a, b), dot(b, b)];
                    let taus = [tau(0), tau(last)];
                    let mut given = Point::of(rho[0], rho[1], sigma);
                    if let [Some(up), Some(down)] = taus {
                        given = given.with_tau([up, down]);
                    }
                    let floored = rho
                        .iter()
                        .chain(taus.iter().flatten())
                        .any(|&x| floor(x) != x);
                    GridPoint {
                        spins: 2,
                        rho,
                        sigma,
                        tau: taus.map(|t| t.unwrap_or(0.0)),
                        v_rho: vrho,
                        v_sigma: [own[0], mixed, own[1]],
                        v_tau: vtau,
                        prepared: floored || prepares(functional, given),
                    }
                }
            }
        })
    });
    let energy = local.iter().fold(0.0, |sum, (e, ..)| sum + e);
    let mut gradient_strain_ha = [[0.0; 3]; 3];
    if let Some(gradients) = &gradients {
        let scale = -grid.cell().volume() / n as f64 / weight;
        for (s, gradient) in gradients.iter().enumerate() {
            let mut sums = [[0.0; 3]; 3];
            for (r, (_, _, h, _)) in local.iter().enumerate() {
                for (i, row) in sums.iter_mut().enumerate() {
                    for (j, sum) in row.iter_mut().enumerate() {
                        *sum += gradient[i][r] * h[s][j];
                    }
                }
            }
            for (entry, sum) in gradient_strain_ha
                .iter_mut()
                .flatten()
                .zip(sums.iter().flatten())
            {
                *entry += scale * sum;
            }
        }
    }
    let potential = match gradients {
        None => (0..channels.len())
            .map(|s| per_point(n, |r| local[r].1[s]))
            .collect(),
        Some(_) => {
            let divergences = grid.divergences(channels.len(), |s, a, r| local[r].2[s][a]);
            (divergences.iter().enumerate())
                .map(|(s, d)| per_point(n, |r| local[r].1[s] - d[r]))
                .collect()
        }
    };
    let kinetic_potential = taus.map(|taus| {
        (0..taus.len())
            .map(|s| per_point(n, |r| local[r].3[s]))
            .collect()
    });
    let xc = Xc {
        energy_ha: energy * grid.cell().volume() / n as f64,
        potential,
        gradient_strain_ha,
        kinetic_potential,
    };
    (xc, points)
}

fn unpolarized_summed(
    functional: Functional,
    n: f64,
    sigma: f64,
    tau: Option<f64>,
) -> (f64, f64, f64, f64) {
    let [x, c] = unpolarized(functional, n, sigma, tau);
    let v = (x.vrho + c.vrho, x.vsigma + c.vsigma, x.vtau + c.vtau);
    (x.energy + c.energy, v.0, v.1, v.2)
}

fn summed<const N: usize>(
    functional: Functional,
    point: Point,
    wanted: [Variable; N],
) -> (f64, [f64; N]) {
    let [(ex, x), (ec, c)] = partials(functional, point, wanted);
    (ex + ec, std::array::from_fn(|k| x[k] + c[k]))
}
