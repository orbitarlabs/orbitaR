use super::crystal::{Crystal, Density, Field, Start};
use super::diagnosis::Diagnosis;
use super::{Discretization, Failure, Mixer, Scf, Weight, occupy};
use crate::model::{Draft, KPoints, Smearing, Spec};
use crate::paw::Dataset;
use crate::pw::{Mat, Pool, Tolerance, c64, random};
use std::num::NonZeroUsize;

const FIRST_EIGENSOLVER_RESIDUAL_TOLERANCE: f64 = 2.5e-2;
const EIGENSOLVER_TOLERANCE_PER_DENSITY_CHANGE: f64 = 0.02;
const EIGENSOLVER_TOLERANCE_CEILING: f64 = 5e-3;
const EIGENSOLVER_FLOOR_PER_DENSITY_STOP: f64 = 0.1;
const LOWEST_EIGENSOLVER_FLOOR: f64 = 100.0 * f64::EPSILON;
const ACCEPTED_EIGENSOLVER_RESIDUAL: f64 = 1e-13;
const EXTRA_BANDS_AT_REPORTED_EDGE: usize = 3;
const ATOMS_PER_BAND: usize = 2;
const MOMENT_PER_EXTRA_BAND: f64 = 2.0;
const BAND_GROWTH_DIVISOR: usize = 5;
const EMPTY: f64 = 1e-14;
const DENSITY_RESIDUAL_TOLERANCE_DIVISOR: f64 = 100.0;
const ONSITE_RESIDUAL_TOLERANCE_DIVISOR: f64 = 10.0;
const KINETIC_RESIDUAL_TOLERANCE_DIVISOR: f64 = 100.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Energies {
    pub kinetic_ha: f64,
    pub hartree_ha: f64,
    pub xc_ha: f64,
    pub ewald_ha: f64,
    pub psp_core_ha: f64,
    pub local_ha: f64,
    pub one_center_ha: f64,
    pub smearing_ha: f64,
}

impl Energies {
    pub fn internal_ha(&self) -> f64 {
        self.kinetic_ha
            + self.hartree_ha
            + self.xc_ha
            + self.ewald_ha
            + self.psp_core_ha
            + self.local_ha
            + self.one_center_ha
    }

    pub fn free_ha(&self) -> f64 {
        self.internal_ha() + self.smearing_ha
    }
}

pub(super) struct Ground {
    pub(super) kpoints: KPoints,
    pub(super) basis_sizes: Vec<usize>,
    pub(super) eigenvalues: Vec<Vec<Vec<f64>>>,
    pub(super) scf: Scf,
    pub(super) diagnosis: Option<Diagnosis>,
    pub(super) discretization: Option<Discretization>,
}

pub(super) struct Warm {
    settings: Draft,
    kpoints: KPoints,
    reported: usize,
    vectors: Vec<Vec<Mat<c64>>>,
    output: Density,
    free: Density,
}

pub(super) fn ground_state(
    spec: &Spec,
    datasets: &[Dataset],
    warm: Option<&Warm>,
    diagnose: bool,
    mut on_step: impl FnMut(usize, f64),
) -> Result<(Ground, Warm), Failure> {
    let draft = spec.draft();
    let pool = pool(spec.threads()).map_err(Failure::Unsupported)?;
    let crystal = on(pool.as_ref(), || Crystal::assembled(spec, datasets))?;
    let smeared = draft.smearing.is_some_and(|s| s != Smearing::Fixed);
    let adaptive = draft.bands.is_none() && smeared;
    let mut reported = match draft.bands {
        Some(n) => n,
        None if !smeared => (crystal.electrons / 2.0).ceil() as usize,
        None => {
            let net_spin_moment: f64 = draft.moments.iter().flatten().sum();
            (crystal.electrons.ceil() as usize).div_ceil(2)
                + draft.atoms.len().div_ceil(ATOMS_PER_BAND)
                + (net_spin_moment.abs() / MOMENT_PER_EXTRA_BAND).ceil() as usize
        }
    };
    let (thinnest, fewest) = (crystal.points.iter().map(|p| p.fft.len()).enumerate())
        .min_by_key(|&(_, n)| n)
        .unwrap_or((0, 0));
    if fewest < reported + EXTRA_BANDS_AT_REPORTED_EDGE {
        return Err(Failure::Invalid(format!(
            "bands.count: {reported} bands and {EXTRA_BANDS_AT_REPORTED_EDGE} above them, but k-point {thinnest} has {fewest} plane waves"
        )));
    }
    let most = fewest - EXTRA_BANDS_AT_REPORTED_EDGE;
    let weights = crystal.kpoints.weights();
    let (smearing, width) = (
        draft.smearing.unwrap_or(Smearing::Fixed),
        draft.width_ha.unwrap_or(0.0),
    );
    let free = on(pool.as_ref(), || crystal.guess());
    let settings = Draft {
        atoms: draft.atoms.iter().map(|&(z, _)| (z, [0.0; 3])).collect(),
        ..draft.clone()
    };
    let fits = |w: &&Warm| w.settings == settings && w.kpoints == crystal.kpoints;
    let mut seeds = 1..;
    let (mut guesses, mut input): (Vec<Vec<Start>>, _) = match warm.filter(fits) {
        Some(w) => {
            reported = w.reported;
            let vectors = (w.vectors.iter())
                .map(|spin| spin.iter().map(|v| Start::Warm(v.clone())).collect())
                .collect();
            (vectors, w.output.moved(&w.free, &free))
        }
        None => {
            let atomic = (0..crystal.spins)
                .map(|_| {
                    (crystal.points.iter().zip(seeds.by_ref()))
                        .map(|(_, seed)| Start::Atoms {
                            columns: reported + EXTRA_BANDS_AT_REPORTED_EDGE,
                            seed,
                        })
                        .collect()
                })
                .collect();
            (atomic, free.clone())
        }
    };
    let (_, mixing): (Vec<f64>, Vec<Weight>) = crystal.components(&input).into_iter().unzip();
    let mut mixer = Mixer::damped(&mixing);
    let values = |d: &Density| -> Vec<f64> {
        (crystal.components(d).into_iter())
            .map(|(v, _)| v)
            .collect()
    };
    let (mut target, mut smallest) = (FIRST_EIGENSOLVER_RESIDUAL_TOLERANCE, f64::INFINITY);
    let (density_stop, onsite_stop, kinetic_stop) = (
        draft.energy_tolerance_ha / DENSITY_RESIDUAL_TOLERANCE_DIVISOR,
        draft.energy_tolerance_ha / ONSITE_RESIDUAL_TOLERANCE_DIVISOR,
        draft.energy_tolerance_ha / KINETIC_RESIDUAL_TOLERANCE_DIVISOR,
    );
    let mut change = (f64::INFINITY, f64::INFINITY, f64::INFINITY);
    let (mut bands_converged, mut still_growing) = (false, false);
    for step in 1..=draft.max_iterations {
        let field = on(pool.as_ref(), || {
            crystal.field(&input, &crystal.fine(&input))
        });
        let tolerance = Tolerance::aiming(target, target.max(ACCEPTED_EIGENSOLVER_RESIDUAL));
        let bands = crystal.bands(pool.as_ref(), &field, guesses, reported, tolerance);
        let eigenvalues: Vec<Vec<Vec<f64>>> = (bands.iter())
            .map(|spin| {
                (spin.iter())
                    .map(|b| b.values[..reported].to_vec())
                    .collect()
            })
            .collect();
        let occupied = occupy(smearing, width, &eigenvalues, weights, crystal.electrons)
            .map_err(Failure::Invalid)?;
        let top = top_share(&occupied.occupations, crystal.spins);
        let counted = occupied.counted();
        let short = smeared && holds_charge(top);
        let grown = match short && adaptive && step > 1 {
            true => (reported
                + EXTRA_BANDS_AT_REPORTED_EDGE.max(reported.div_ceil(BAND_GROWTH_DIVISOR)))
            .min(most),
            false => reported,
        };
        let (output, kinetic_ha) = on(pool.as_ref(), || crystal.output(&bands, &counted));
        change = crystal.change(&input, &output);
        let converged = bands.iter().flatten().all(|b| b.converged);
        let growing = adaptive && short && reported < most;
        (bands_converged, still_growing) = (converged, growing);
        let done =
            below(change, (density_stop, onsite_stop, kinetic_stop)) && converged && !growing;
        let terms = |of: &Field| Energies {
            kinetic_ha,
            hartree_ha: of.hartree_ha,
            xc_ha: of.xc_ha,
            ewald_ha: crystal.ewald_ha,
            psp_core_ha: crystal.psp_core_ha,
            local_ha: of.local_ha,
            one_center_ha: of.one_center_ha,
            smearing_ha: occupied.smearing_ha,
        };
        let energies = done.then(|| {
            let produced = on(pool.as_ref(), || {
                crystal.field(&output, &crystal.fine(&output))
            });
            terms(&produced)
        });
        if draft.step_energies {
            let free_ha = match &energies {
                Some(energies) => energies.free_ha(),
                None => terms(&field).free_ha() + crystal.first_order_ha(&field, &input, &output),
            };
            on_step(step, free_ha);
        }
        if let Some(energies) = energies {
            let free_dc_ha =
                terms(&field).free_ha() + crystal.first_order_ha(&field, &input, &output);
            if short && adaptive {
                return Err(Failure::Invalid(format!(
                    "smearing: the top of {reported} bands holds {top:.1e} of a state, above {EMPTY:.0e}; k-point \
                     {thinnest} has {fewest} plane waves, so no more bands fit; raise cutoff_ha"
                )));
            }
            if short {
                return Err(Failure::Invalid(format!(
                    "bands.count: the top of {reported} bands holds {top:.1e} of a state, above {EMPTY:.0e}; compute \
                     more bands"
                )));
            }
            let held = |spin: &[Vec<f64>]| -> f64 {
                (spin.iter().zip(weights))
                    .map(|(k, w)| w * k.iter().sum::<f64>())
                    .sum()
            };
            let magnetization_bohr_magneton = match occupied.occupations.as_slice() {
                [up, down] => Some(held(up) - held(down)),
                _ => None,
            };
            let forces_ha_bohr = (draft.forces)
                .then(|| on(pool.as_ref(), || crystal.forces(&output, &bands, &counted)))
                .transpose()?;
            let stress_ha_bohr3 = (draft.stress)
                .then(|| on(pool.as_ref(), || crystal.stress(&output, &bands, &counted)))
                .transpose()?;
            let ground = Ground {
                kpoints: crystal.kpoints.clone(),
                basis_sizes: crystal.points.iter().map(|p| p.fft.len()).collect(),
                eigenvalues,
                scf: Scf {
                    energies,
                    fermi_level_ha: occupied.fermi_ha,
                    occupations: occupied.occupations,
                    magnetization_bohr_magneton,
                    forces_ha_bohr,
                    stress_ha_bohr3,
                    steps: step,
                    free_dc_ha,
                    kinetic_density_integral_ha: crystal.kinetic_integral_ha(&output),
                },
                diagnosis: diagnose.then(|| on(pool.as_ref(), || crystal.diagnosis(&output))),
                discretization: draft.discretization.then(|| crystal.discretization()),
            };
            let next = Warm {
                settings,
                kpoints: crystal.kpoints.clone(),
                reported,
                vectors: (bands.into_iter())
                    .map(|spin| spin.into_iter().map(|b| b.vectors).collect())
                    .collect(),
                output,
                free,
            };
            return Ok((ground, next));
        }
        let added = grown - reported;
        if added == 0 {
            smallest = smallest.min(change.0);
            target = (EIGENSOLVER_TOLERANCE_PER_DENSITY_CHANGE * smallest).clamp(
                eigensolver_floor(density_stop),
                EIGENSOLVER_TOLERANCE_CEILING,
            );
        }
        guesses = (bands.into_iter())
            .map(|spin| {
                (spin.into_iter().zip(&crystal.points).zip(seeds.by_ref()))
                    .map(|((b, p), seed)| {
                        let kept = b.vectors.ncols();
                        let mut guess = Mat::zeros(p.fft.len(), kept + added);
                        guess.subcols_mut(0, kept).copy_from(&b.vectors);
                        guess.subcols_mut(kept, added).copy_from(random::<c64>(
                            p.fft.len(),
                            added,
                            seed,
                        ));
                        Start::Warm(guess)
                    })
                    .collect()
            })
            .collect();
        reported = grown;
        if added > 0 {
            continue;
        }
        input = on(pool.as_ref(), || {
            crystal.density(&mixer.next(&values(&input), &values(&output)))
        });
    }
    let changes = [
        ("‖Δñ‖", change.0, density_stop),
        ("max |Δρ_ij|", change.1, onsite_stop),
        ("‖Δτ̃‖", change.2, kinetic_stop),
    ];
    let changes = &changes[..if input.kinetic.is_some() { 3 } else { 2 }];
    let each: Vec<String> = (changes.iter())
        .map(|(name, value, limit)| format!("{name} = {value:.1e} (limit {limit:.0e})"))
        .collect();
    let mut unmet: Vec<&str> = (changes.iter())
        .filter(|&&(_, value, limit)| !below_limit(value, limit))
        .map(|&(name, _, _)| name)
        .collect();
    if !bands_converged {
        unmet.push("the bands' convergence");
    }
    if still_growing {
        unmet.push("the band count, still growing");
    }
    let last = each.len() - 1;
    let message = format!(
        "scf.max_iterations: {} steps, at {reported} bands, leave {}{}{}; not met: {}",
        draft.max_iterations,
        each[..last].join(", "),
        if last > 1 { ", and " } else { " and " },
        each[last],
        unmet.join(", ")
    );
    Err(Failure::Unconverged(message))
}

pub fn eigensolver_floor(density_stop: f64) -> f64 {
    (EIGENSOLVER_FLOOR_PER_DENSITY_STOP * density_stop)
        .clamp(LOWEST_EIGENSOLVER_FLOOR, ACCEPTED_EIGENSOLVER_RESIDUAL)
}

fn pool(threads: NonZeroUsize) -> Result<Option<Pool>, String> {
    if threads.get() == 1 {
        return Ok(None);
    }
    Pool::try_from(threads.get())
        .map(Some)
        .map_err(|e| format!("parallel.threads: {e}"))
}

fn on<R: Send>(pool: Option<&Pool>, op: impl FnOnce() -> R + Send) -> R {
    match pool {
        Some(pool) => pool.run(op),
        None => op(),
    }
}

fn top_share(occupations: &[Vec<Vec<f64>>], spins: usize) -> f64 {
    let full = 2.0 / spins as f64;
    (occupations.iter().flatten())
        .filter_map(|k| k.last())
        .map(|f| f / full)
        .fold(0.0, f64::max)
}

fn holds_charge(share: f64) -> bool {
    share > EMPTY
}

fn below(change: (f64, f64, f64), bounds: (f64, f64, f64)) -> bool {
    below_limit(change.0, bounds.0)
        && below_limit(change.1, bounds.1)
        && below_limit(change.2, bounds.2)
}

fn below_limit(change: f64, limit: f64) -> bool {
    change < limit
}
