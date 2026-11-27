use super::rival::Rival;
use super::{
    Block, Metrics,
    engine::{self, Failure, Run, Step},
    load::{During, Gate, Reading},
};
use crate::case::Speed;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use statrs::statistics::{Data, Median, Statistics};
use std::{cell::RefCell, collections::BTreeMap, num::NonZeroUsize};

pub(crate) const DELTA_HA_PER_ATOM: f64 = 1e-4 / 27.211_386_245_988;
const TIGHTER: f64 = 10.0;
const SHARE: f64 = 1e-2;

pub(crate) type Runner<'a> = &'a dyn Fn(&Block, f64, Pass) -> Result<Run, Failure>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Pass {
    Reference,
    Check,
    Timed,
}

pub(crate) struct Timed {
    pub(crate) stdout: String,
    pub(crate) runs: Vec<Run>,
    pub(crate) record: BlockRecord,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Code {
    Orbitar,
    #[serde(untagged)]
    Rival(Rival),
}

impl std::fmt::Display for Code {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(match self {
            Code::Orbitar => "orbitar",
            Code::Rival(rival) => rival.adapter().name(),
        })
    }
}

#[derive(Serialize, Deserialize)]
pub(crate) struct BlockRecord {
    pub(crate) code: Code,
    pub(crate) input: String,
    pub(crate) threads: NonZeroUsize,
    pub(crate) atoms: usize,
    pub(crate) e_ref_ha: Option<f64>,
    pub(crate) e_ref_shift_ha: Option<f64>,
    pub(crate) delta_ha: f64,
    pub(crate) warmup: Value,
    #[serde(default)]
    pub(crate) step_cap: Option<usize>,
    #[serde(default)]
    pub(crate) steps: Vec<usize>,
    #[serde(default)]
    pub(crate) e_ref_steps: Option<[usize; 2]>,
    pub(crate) t_delta_s: Vec<f64>,
    pub(crate) wall_s: Vec<f64>,
    pub(crate) peak_rss_bytes: Vec<u64>,
    pub(crate) peak_pss_bytes: Vec<u64>,
    pub(crate) losses: Vec<String>,
    #[serde(default)]
    pub(crate) load: Vec<Reading>,
    #[serde(default)]
    pub(crate) during: Vec<During>,
    pub(crate) noisy: bool,
    #[serde(flatten)]
    pub(crate) timing: Option<Timing>,
    pub(crate) rival: Option<Value>,
    pub(crate) accuracy: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) refused: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) failed_peak_bytes: Vec<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) gamma: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub(crate) struct Timing {
    pub(crate) median_t_delta_s: f64,
    pub(crate) range_t_delta_s: [f64; 2],
    pub(crate) cv_t_delta: Option<f64>,
    pub(crate) median_peak_rss_bytes: f64,
    pub(crate) median_peak_pss_bytes: f64,
}

fn reference_key<'a>(b: &Block<'a>) -> (Code, &'a str, Vec<String>) {
    let lines = (b.rival.as_ref()).map_or_else(Vec::new, |job| job.layout.lines().to_vec());
    (b.code(), b.input, lines)
}

fn number(x: f64) -> Option<f64> {
    (!x.is_nan()).then_some(x)
}

pub(crate) fn time(
    run: Runner,
    blocks: &[Block],
    speed: &Speed,
    repeats: usize,
    seed: u64,
    gate: Option<&Gate>,
    guard: Option<f64>,
) -> Result<(Vec<Timed>, Option<Failure>), Failure> {
    for b in blocks {
        timed_stop(b)?;
        reference_stop(b)?;
    }
    type Key<'a> = (Code, &'a str, Vec<String>);
    let key = reference_key;
    let peaks: RefCell<Vec<(Comparable, Peak)>> = RefCell::default();
    type Owned = (Code, String, Vec<String>);
    let owned = |b: &Block| -> Owned {
        let (code, input, lines) = key(b);
        (code, input.to_owned(), lines)
    };
    let over: RefCell<BTreeMap<Owned, String>> = RefCell::default();
    let failed: RefCell<BTreeMap<Owned, Vec<u64>>> = RefCell::default();
    let run = |b: &Block, stop: f64, pass: Pass| {
        if let Some(why) = over.borrow().get(&owned(b)) {
            return Err(Failure::Engine(why.clone()));
        }
        engine::forget_peaks();
        let ran = run(b, stop, pass);
        let measured = match &ran {
            Ok(r) => Some([r.peak_rss_bytes, r.peak_pss_bytes]),
            Err(_) => engine::last_peaks(),
        };
        let measured = measured.map(|[rss, pss]| if pss == 0 { rss } else { pss });
        if let (Err(_), Some(bytes)) = (&ran, measured) {
            failed.borrow_mut().entry(owned(b)).or_default().push(bytes);
        }
        if let Some(limit) = guard
            && let Some(bytes) = measured
        {
            let peak = Peak {
                atoms: b.atoms,
                in_flight: b.in_flight().map_err(Failure::Harness)?,
                bytes: bytes as f64,
            };
            peaks.borrow_mut().push((b.comparable(), peak));
            if bytes as f64 > limit {
                let why = format!(
                    "{}: refused after a run: its measured peak {:.2} GB passes the {} GB guard",
                    b.at(),
                    bytes as f64 / 1e9,
                    limit / 1e9
                );
                println!("speed: {why}");
                over.borrow_mut().entry(owned(b)).or_insert(why);
            }
        }
        ran.map_err(|f| named(b, f))
    };
    let mut fatal: Option<Failure> = None;
    let lost = |b: &Block, f: Failure| match (b.rival.is_some(), f) {
        (true, Failure::Engine(e)) => Ok(e),
        (_, f) => Err(f),
    };
    type Reference = Result<(f64, f64, [usize; 2]), String>;
    let mut references: BTreeMap<Key, Reference> = BTreeMap::new();
    let mut refused: BTreeMap<Key, String> = BTreeMap::new();
    let mut order: Vec<usize> = (0..blocks.len()).collect();
    if guard.is_some() {
        order.sort_by_key(|&i| blocks[i].atoms);
    }
    for b in order.into_iter().map(|i| &blocks[i]) {
        let key = key(b);
        if references.contains_key(&key) {
            continue;
        }
        if let Some(limit) = guard
            && let Some(why) = refusal(b, &peaks.borrow(), limit).map_err(Failure::Harness)?
        {
            println!("speed: {why}");
            references.insert(key.clone(), Err(why.clone()));
            refused.insert(key, why);
            continue;
        }
        let stop = reference_stop(b)?;
        let energy = |stop: f64, pass: Pass| -> Result<(f64, usize), Failure> {
            let reference = run(b, stop, pass)?;
            let last = reference.steps.last().ok_or_else(|| {
                Failure::Engine(format!("{}: no SCF step lines on stderr", b.at()))
            })?;
            Ok((last.free_energy_ha, reference.steps.len()))
        };
        let checked = energy(stop, Pass::Reference).and_then(|(e_ref, n)| {
            let (check, m) = energy(stop / TIGHTER, Pass::Check)?;
            let shift = check - e_ref;
            if !e_ref_holds(shift, delta_ha(b.atoms)) {
                return Err(Failure::Engine(format!(
                    "{}: E_ref moves by {shift:e} Ha at a {TIGHTER}× tighter stop, past {SHARE} δ · atoms",
                    b.at()
                )));
            }
            Ok((e_ref, shift, [n, m]))
        });
        if let Some(why) = over.borrow().get(&owned(b)).cloned() {
            references.insert(key.clone(), Err(why.clone()));
            refused.insert(key, why);
            continue;
        }
        let checked = match checked.map_err(|f| lost(b, f)) {
            Ok(pair) => Ok(pair),
            Err(Ok(loss)) => Err(loss),
            Err(Err(f)) => {
                fatal = Some(f);
                break;
            }
        };
        match &checked {
            Ok((e_ref, shift, [n, m])) => {
                println!(
                    "speed: {}: E_ref {e_ref} Ha after {n} steps, shift {shift:e} Ha after {m}",
                    b.at()
                )
            }
            Err(loss) => println!("speed: {}: E_ref lost: {loss}", b.at()),
        }
        references.insert(key, checked);
    }
    let reached = |b: &Block, run: &Run| {
        let e_ref = (references.get(&key(b)))
            .ok_or_else(|| Failure::Engine(format!("{}: no E_ref", b.at())))?
            .as_ref()
            .map_err(|e| Failure::Engine(e.clone()))?;
        let met = settled(&run.steps, e_ref.0, delta_ha(b.atoms));
        met.map(|(_, t)| t).ok_or_else(|| {
            Failure::Engine(format!(
                "{}: the run never held |E − E_ref| < δ to its end",
                b.at()
            ))
        })
    };
    let judged = |b: &Block| -> Result<Result<(f64, Run), String>, Failure> {
        let attempt = run(b, timed_stop(b)?, Pass::Timed).and_then(|r| match reached(b, &r) {
            Ok(t) => Ok((t, r)),
            Err(f) => {
                let bytes = match r.peak_pss_bytes {
                    0 => r.peak_rss_bytes,
                    pss => pss,
                };
                failed.borrow_mut().entry(owned(b)).or_default().push(bytes);
                Err(f)
            }
        });
        let judged = match attempt {
            Ok(pair) => Ok(pair),
            Err(f) => Err(lost(b, f)?),
        };
        match &judged {
            Ok((t, r)) => println!(
                "speed: {}: T(δ) {t} s, wall {} s, {} steps",
                b.at(),
                r.seconds,
                r.steps.len()
            ),
            Err(loss) => println!("speed: {}: lost: {loss}", b.at()),
        }
        Ok(judged)
    };
    let load: RefCell<Vec<Vec<Reading>>> =
        RefCell::new(blocks.iter().map(|_| Vec::new()).collect());
    let during: RefCell<Vec<Vec<During>>> =
        RefCell::new(blocks.iter().map(|_| Vec::new()).collect());
    let judged = |i: usize| -> Result<Result<(f64, Run), String>, Failure> {
        let Some(gate) = gate else {
            return judged(&blocks[i]);
        };
        let at = blocks[i].at();
        let (reading, start) = gate.before(&at, speed)?;
        load.borrow_mut()[i].push(reading);
        let out = judged(&blocks[i])?;
        let child = out.as_ref().ok().map(|(_, run)| run.cpu_s);
        during.borrow_mut()[i].push(gate.after(&at, speed, start, child)?);
        Ok(out)
    };
    let mut warmups = vec![Value::Null; blocks.len()];
    for i in shuffled(seed, 0, blocks.len())
        .into_iter()
        .filter(|_| fatal.is_none())
    {
        let stopped = over.borrow().get(&owned(&blocks[i])).cloned();
        if let Some(why) = refused.get(&key(&blocks[i])).cloned().or(stopped) {
            warmups[i] = json!({ "refused": why });
            continue;
        }
        warmups[i] = match judged(i) {
            Err(f) => {
                fatal = Some(f);
                break;
            }
            Ok(Ok((t, warm))) => {
                json!({ "t_delta_s": t, "wall_s": warm.seconds, "peak_rss_bytes": warm.peak_rss_bytes, "peak_pss_bytes": warm.peak_pss_bytes, "steps": warm.steps.len() })
            }
            Ok(Err(loss)) => json!({ "loss": loss }),
        };
    }
    let mut runs: Vec<Vec<Run>> = blocks.iter().map(|_| Vec::new()).collect();
    let mut times: Vec<Vec<f64>> = blocks.iter().map(|_| Vec::new()).collect();
    let mut losses: Vec<Vec<String>> = blocks.iter().map(|_| Vec::new()).collect();
    let mut target: Vec<usize> = (blocks.iter())
        .map(|b| match refused.contains_key(&key(b)) {
            true => 0,
            false => repeats,
        })
        .collect();
    'rounds: for round in 1.. {
        if fatal.is_some() {
            break;
        }
        let due: Vec<usize> = (0..blocks.len())
            .filter(|&i| runs[i].len() + losses[i].len() < target[i])
            .filter(|&i| !over.borrow().contains_key(&owned(&blocks[i])))
            .collect();
        if due.is_empty() {
            let grown: Vec<usize> = (0..blocks.len())
                .filter(|&i| target[i] == repeats && runs_for(&times[i], speed, repeats) > repeats)
                .collect();
            if grown.is_empty() {
                break;
            }
            grown
                .into_iter()
                .for_each(|i| target[i] = runs_for(&times[i], speed, repeats));
            continue;
        }
        for k in shuffled(seed, round, due.len()) {
            let (b, i) = (&blocks[due[k]], due[k]);
            match judged(i) {
                Err(f) => {
                    fatal = Some(f);
                    break 'rounds;
                }
                Ok(Ok((t, next))) => {
                    if b.rival.is_none()
                        && let Err(f) = engine::same_output(&runs[i], &next)
                    {
                        fatal = Some(named(b, f));
                        break 'rounds;
                    }
                    times[i].push(t);
                    runs[i].push(next);
                }
                Ok(Err(loss)) => losses[i].push(loss),
            }
        }
    }
    let medians: Vec<f64> = times.iter().map(|t| median(t)).collect();
    let timed = (blocks
        .iter()
        .enumerate()
        .zip(runs)
        .zip(times)
        .zip(warmups)
        .zip(losses)
        .zip(load.into_inner())
        .zip(during.into_inner()))
    .map(
        |(((((((i, b), runs), t), warmup), losses), load), during)| {
            let refusal =
                (refused.get(&key(b)).cloned()).or_else(|| over.borrow().get(&owned(b)).cloned());
            let noisy = refusal.is_none() && noisy(&t, losses.len(), repeats);
            let (low, high) = (t.iter()).fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), &x| {
                (l.min(x), h.max(x))
            });
            let rss: Vec<u64> = runs.iter().map(|r| r.peak_rss_bytes).collect();
            let pss: Vec<u64> = runs.iter().map(|r| r.peak_pss_bytes).collect();
            let (e_ref_ha, e_ref_shift_ha, e_ref_steps) =
                (references.get(&key(b)).and_then(|r| r.as_ref().ok()))
                    .map_or((None, None, None), |&(e, s, n)| (Some(e), Some(s), Some(n)));
            let record = BlockRecord {
                code: b.code(),
                input: b.input.to_owned(),
                threads: b.threads.unwrap_or(NonZeroUsize::MIN),
                atoms: b.atoms,
                e_ref_ha,
                e_ref_shift_ha,
                delta_ha: delta_ha(b.atoms),
                warmup,
                step_cap: b.step_cap(),
                steps: runs.iter().map(|r| r.steps.len()).collect(),
                e_ref_steps,
                wall_s: runs.iter().map(|r| r.seconds).collect(),
                timing: (!t.is_empty()).then(|| Timing {
                    median_t_delta_s: medians[i],
                    range_t_delta_s: [low, high],
                    cv_t_delta: number(cv(&t)),
                    median_peak_rss_bytes: median(
                        &rss.iter().map(|&x| x as f64).collect::<Vec<_>>(),
                    ),
                    median_peak_pss_bytes: median(
                        &pss.iter().map(|&x| x as f64).collect::<Vec<_>>(),
                    ),
                }),
                peak_rss_bytes: rss,
                peak_pss_bytes: pss,
                losses,
                load,
                during,
                rival: (b.rival.as_ref()).map(|r| {
                    let claim = r.claim.as_ref();
                    json!({
                        "cutoff_ha": r.cutoff_ha,
                        "layout": r.layout,
                        "fitted": r.layout.added_to(&r.frozen),
                        "thread_env": super::rival::thread_env(&r.layout),
                        "mixing": r.mixing,
                        "mpiexec": r.mpiexec,
                        "datasets": claim.map(|c| &c.files),
                        "stop_floor_ha": claim.and_then(|c| c.floor_ha),
                        "busy_cores": claim.map(|c| c.busy_cores),
                    })
                }),
                accuracy: None,
                refused: refusal,
                failed_peak_bytes: failed.borrow().get(&owned(b)).cloned().unwrap_or_default(),
                gamma: b.gamma(),
                noisy,
                t_delta_s: t,
            };
            Timed {
                stdout: runs.first().map(|r| r.stdout.clone()).unwrap_or_default(),
                runs,
                record,
            }
        },
    )
    .collect();
    Ok((timed, fatal))
}

pub(crate) fn memory(blocks: &[Block], timed: &[Timed]) -> Metrics {
    let rss =
        |i: usize| (timed[i].record.timing.as_ref()).map_or(f64::NAN, |t| t.median_peak_rss_bytes);
    let mut by_atoms: BTreeMap<usize, f64> = BTreeMap::new();
    for i in (0..blocks.len())
        .filter(|&i| blocks[i].rival.is_none() && timed[i].record.refused.is_none())
    {
        let worst = by_atoms.entry(blocks[i].atoms).or_insert(rss(i));
        *worst = super::worst(*worst, rss(i));
    }
    let mut sizes = by_atoms.iter().rev();
    let exponent = match (sizes.next(), sizes.next()) {
        (Some((&n, &big)), Some((&m, &small))) => (big / small).ln() / (n as f64 / m as f64).ln(),
        _ => f64::NAN,
    };
    Metrics::from([(RSS_EXPONENT.to_owned(), exponent)])
}

pub(crate) const RSS_EXPONENT: &str = "rss_exponent";
pub(crate) const T_DELTA_EXPONENT: &str = "t_delta_exponent";
pub(crate) const PSS_EXPONENT: &str = "pss_exponent";
pub(crate) const SERIES: [&str; 2] = [T_DELTA_EXPONENT, PSS_EXPONENT];

pub(crate) struct Expected<'a> {
    pub(crate) code: Code,
    pub(crate) input: &'a str,
    pub(crate) atoms: usize,
    pub(crate) skipped: Option<&'a str>,
}

struct Point<'a> {
    expected: &'a Expected<'a>,
    status: String,
    t_delta_s: Option<f64>,
    pss_bytes: Option<f64>,
    polled: [usize; 2],
}

pub(crate) struct Series {
    pub(crate) entries: Value,
    pub(crate) exponents: Metrics,
    pub(crate) holes: Vec<String>,
}

pub(crate) fn series(blocks: &[Block], timed: &[Timed], expected: &[Expected]) -> Series {
    let mut codes: Vec<Code> = expected.iter().map(|e| e.code).collect();
    codes.sort();
    codes.dedup();
    let (mut entries, mut exponents, mut holes) = (Vec::new(), Metrics::new(), Vec::new());
    for code in codes {
        let mut points: Vec<Point> = Vec::new();
        for e in expected.iter().filter(|e| e.code == code) {
            let found =
                (blocks.iter().zip(timed)).find(|(b, _)| b.code() == code && b.input == e.input);
            let record = found.map(|(_, t)| &t.record);
            let t_delta = record
                .and_then(|r| r.timing.as_ref())
                .map(|t| t.median_t_delta_s);
            let measured: Vec<f64> = (record.into_iter())
                .flat_map(|r| r.peak_pss_bytes.iter())
                .filter(|&&bytes| bytes > 0)
                .map(|&bytes| bytes as f64)
                .collect();
            let pss = (!measured.is_empty()).then(|| median(&measured));
            let runs = record.map_or(0, |r| r.peak_pss_bytes.len());
            let status = match (record, e.skipped) {
                (None, Some(why)) => format!("skipped: {why}"),
                (None, None) => "no block".to_owned(),
                (Some(r), _) if r.refused.is_some() => "refused".to_owned(),
                (Some(r), _) if t_delta.is_none() => format!("lost: {} losses", r.losses.len()),
                (Some(_), _) if pss.is_none() => "pss unmeasured: no Pss poll landed".to_owned(),
                (Some(_), _) => "measured".to_owned(),
            };
            if status != "measured" {
                holes.push(format!("{code} {}: {status}", e.input));
            }
            points.push(Point {
                expected: e,
                status,
                t_delta_s: t_delta,
                pss_bytes: pss,
                polled: [measured.len(), runs],
            });
        }
        points.sort_by_key(|p| (p.expected.atoms, p.expected.input));
        let fit = |y: fn(&Point) -> Option<f64>| {
            let xy: Vec<(f64, f64)> = (points.iter())
                .filter_map(|p| Some(((p.expected.atoms as f64).ln(), y(p)?.ln())))
                .collect();
            log_log_slope(&xy)
        };
        let (t_alpha, pss_alpha) = (fit(|p| p.t_delta_s), fit(|p| p.pss_bytes));
        exponents.insert(format!("{T_DELTA_EXPONENT}_{code}"), t_alpha);
        exponents.insert(format!("{PSS_EXPONENT}_{code}"), pss_alpha);
        let full = points.iter().all(|p| p.status == "measured");
        entries.push(json!({
            "code": code,
            "coverage": if full { "full" } else { "partial" },
            "t_delta_exponent": finite(t_alpha),
            "pss_exponent": finite(pss_alpha),
            "points": (points.iter()).map(|p| json!({
                "input": p.expected.input,
                "atoms": p.expected.atoms,
                "status": p.status,
                "median_t_delta_s": p.t_delta_s,
                "median_peak_pss_bytes": p.pss_bytes,
                "pss_polled_runs": p.polled,
            })).collect::<Vec<_>>(),
        }));
    }
    Series {
        entries: Value::Array(entries),
        exponents,
        holes,
    }
}

pub fn log_log_slope(points: &[(f64, f64)]) -> f64 {
    let n = points.len() as f64;
    let (mx, my) = (
        points.iter().map(|p| p.0).sum::<f64>() / n,
        points.iter().map(|p| p.1).sum::<f64>() / n,
    );
    let sxx: f64 = points.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    match sxx > 0.0 {
        true => sxy / sxx,
        false => f64::NAN,
    }
}

pub const PEAK_PSS_GUARD_BYTES: f64 = 14e9;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    pub atoms: usize,
    pub in_flight: usize,
    pub bytes: f64,
}

pub fn estimated_peak(smaller: &[Peak], atoms: usize, in_flight: usize) -> Option<f64> {
    let largest = |below: usize| {
        (smaller.iter())
            .filter(|p| p.atoms < below)
            .max_by(|a, b| (a.atoms.cmp(&b.atoms)).then(a.bytes.total_cmp(&b.bytes)))
    };
    let m = largest(atoms)?;
    let size = atoms as f64 / m.atoms as f64;
    let dense = size.powi(2) * in_flight as f64 / m.in_flight as f64;
    let fitted = largest(m.atoms)
        .map(|l| (m.bytes / l.bytes).ln() / (m.atoms as f64 / l.atoms as f64).ln())
        .filter(|alpha| alpha.is_finite())
        .map_or(0.0, |alpha| size.powf(alpha));
    Some(m.bytes * dense.max(fitted))
}

pub(crate) type Comparable = (Code, String, Option<NonZeroUsize>, Vec<String>);

pub const ORBITAR_SOLVES_IN_FLIGHT: usize = 2;

fn refusal(b: &Block, peaks: &[(Comparable, Peak)], limit: f64) -> Result<Option<String>, String> {
    let same = b.comparable();
    let smaller: Vec<Peak> = (peaks.iter())
        .filter(|(of, _)| *of == same)
        .map(|(_, peak)| *peak)
        .collect();
    let estimate = estimated_peak(&smaller, b.atoms, b.in_flight()?);
    Ok(estimate.filter(|&e| e > limit).map(|e| {
        format!(
            "{}: refused before it runs: its peak memory estimate {:.2} GB from the smaller sizes of {} passes the {} \
             GB guard",
            b.at(),
            e / 1e9,
            b.code(),
            limit / 1e9
        )
    }))
}

pub(crate) const R_MIN: f64 = 1.0;
pub const G_MIN: f64 = 1.0;
pub const G_M4: f64 = 1.25;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Milestone {
    #[default]
    M3,
    M4,
}

impl Milestone {
    pub(crate) fn g_holds(self, g: f64, low: f64) -> bool {
        match self {
            Milestone::M3 => low >= G_MIN,
            Milestone::M4 => g >= G_M4 && low > G_MIN,
        }
    }

    fn g_point_min(self) -> Option<f64> {
        match self {
            Milestone::M3 => None,
            Milestone::M4 => Some(G_M4),
        }
    }
}

pub const G_RESAMPLES: usize = 10_000;
pub const G_SEED: u64 = 0x2026_1113_0095_0001;
const R_POLLS: u32 = 20;

pub(crate) fn versus(
    blocks: &[Block],
    timed: &[Timed],
    milestone: Milestone,
) -> (Vec<Value>, Vec<String>) {
    let lost = |i: usize, values: Vec<f64>| -> Vec<f64> {
        let losses = timed[i].record.losses.len();
        (values.into_iter())
            .chain(std::iter::repeat_n(f64::INFINITY, losses))
            .collect()
    };
    let runs = |i: usize| lost(i, timed[i].record.t_delta_s.clone());
    let times = |i: usize| median(&runs(i));
    let pss = |i: usize| {
        lost(
            i,
            timed[i]
                .record
                .peak_pss_bytes
                .iter()
                .map(|&b| b as f64)
                .collect(),
        )
    };
    let pss = |i: usize| median(&pss(i));
    let mut codes: Vec<Rival> = (blocks.iter())
        .filter_map(|b| b.rival.as_ref().map(|r| r.code))
        .collect();
    codes.sort_by_key(|&code| Code::Rival(code));
    codes.dedup();
    let (mut entries, mut failures) = (Vec::new(), Vec::new());
    for code in codes {
        let (mut pairs, mut t_ratios, mut pss_ratios) = (Vec::new(), Vec::new(), Vec::new());
        let mut t_runs = Vec::new();
        for (r, b) in blocks
            .iter()
            .enumerate()
            .filter(|(_, b)| b.rival.as_ref().is_some_and(|j| j.code == code))
        {
            let Some(o) = (0..blocks.len()).find(|&o| {
                blocks[o].rival.is_none()
                    && blocks[o].input == b.input
                    && blocks[o].threads == b.threads
            }) else {
                failures.push(format!(
                    "{}: orbitaR has no block of the same input and threads",
                    b.at()
                ));
                continue;
            };
            if timed[r].record.refused.is_some() || timed[o].record.refused.is_some() {
                continue;
            }
            let (t_delta, peak_pss) = ([times(r), times(o)], [pss(r), pss(o)]);
            let (t, m) = (t_delta[0] / t_delta[1], peak_pss[0] / peak_pss[1]);
            let shortest = (timed[r].record.wall_s.iter())
                .chain(&timed[o].record.wall_s)
                .copied()
                .fold(f64::INFINITY, f64::min);
            let in_r = shortest >= (engine::POLL * R_POLLS).as_secs_f64();
            t_ratios.push(t);
            t_runs.push([runs(r), runs(o)]);
            if in_r {
                pss_ratios.push(m);
            }
            pairs.push(json!({
                "input": b.input,
                "threads": b.threads.map_or(1, NonZeroUsize::get),
                "medians_t_delta_s": t_delta.map(finite),
                "medians_peak_pss_bytes": peak_pss.map(finite),
                "t_ratio": finite(t),
                "pss_ratio": finite(m),
                "in_r": in_r,
                "shortest_run_s": finite(shortest),
                "losses": timed[r].record.losses.len(),
            }));
        }
        let (n, long) = (pairs.len(), pss_ratios.len());
        let (g, r) = (t_ratios.geometric_mean(), pss_ratios.geometric_mean());
        let [low, high] = g_interval(&t_runs, G_RESAMPLES, G_SEED);
        let name = Code::Rival(code);
        if long == 0 {
            failures.push(format!(
                "R against {name} is unmeasured: no pair whose runs all last {R_POLLS} Pss polls ({} s)",
                (engine::POLL * R_POLLS).as_secs_f64()
            ));
        } else if !r_holds(r) {
            failures.push(format!(
                "R against {name} = {r} over {long} pairs is below {R_MIN}: orbitaR holds more memory"
            ));
        }
        let bar = match milestone {
            Milestone::M3 => format!("lower ≥ {G_MIN}"),
            Milestone::M4 => format!("M4: G ≥ {G_M4}, lower > {G_MIN}"),
        };
        if !milestone.g_holds(g, low) {
            failures.push(match milestone {
                Milestone::M3 => format!(
                    "G against {name} = {g} over {n} pairs has a 95% CI [{low}, {high}] whose lower bound is not at \
                     least {G_MIN}: orbitaR is not shown faster"
                ),
                Milestone::M4 => format!(
                    "G against {name} = {g} over {n} pairs with a 95% CI [{low}, {high}] misses M4 (README Benchmark status): G ≥ \
                     {G_M4} and a lower bound > {G_MIN}"
                ),
            });
        }
        println!(
            "versus {name}: R {r} (judged, ≥ {R_MIN}) over {long} pairs, G {g} with 95% CI [{low}, {high}] (judged, \
             {bar}) over {n} pairs"
        );
        entries.push(json!({
            "code": name,
            "r": finite(r),
            "r_min": R_MIN,
            "g": finite(g),
            "g_ci": [finite(low), finite(high)],
            "g_min": G_MIN,
            "milestone": milestone,
            "g_point_min": milestone.g_point_min(),
            "g_resamples": G_RESAMPLES,
            "g_seed": G_SEED,
            "g_judged": true,
            "pairs": pairs,
        }));
    }
    (entries, failures)
}

fn finite(x: f64) -> Value {
    if x.is_finite() {
        json!(x)
    } else {
        json!(x.to_string())
    }
}

pub(crate) fn read_finite(v: &Value) -> Option<f64> {
    v.as_f64().or_else(|| v.as_str()?.parse().ok())
}

pub(crate) fn r_holds(r: f64) -> bool {
    r >= R_MIN
}

pub fn g_interval(pairs: &[[Vec<f64>; 2]], resamples: usize, seed: u64) -> [f64; 2] {
    let mut state = seed;
    let mut draw = |runs: &[f64]| -> f64 {
        let n = runs.len() as u128;
        let drawn: Vec<f64> = (0..runs.len())
            .map(|_| runs[((u128::from(engine::splitmix(&mut state)) * n) >> 64) as usize])
            .collect();
        median(&drawn)
    };
    let mut g: Vec<f64> = (0..resamples)
        .map(|_| {
            (pairs.iter())
                .map(|[rival, ours]| draw(rival) / draw(ours))
                .collect::<Vec<_>>()
                .geometric_mean()
        })
        .collect();
    g.sort_by(f64::total_cmp);
    let k = resamples / 40;
    let at = |rank: usize| rank.checked_sub(1).and_then(|i| g.get(i)).copied();
    [at(k), at(resamples - k)].map(|x| x.unwrap_or(f64::NAN))
}

pub(crate) fn e_ref_holds(shift: f64, delta_ha: f64) -> bool {
    shift.abs() < SHARE * delta_ha
}

fn named(b: &Block, failure: Failure) -> Failure {
    match failure {
        Failure::Engine(e) => Failure::Engine(format!("{}: {e}", b.at())),
        harness => harness,
    }
}

pub(crate) fn delta_ha(atoms: usize) -> f64 {
    DELTA_HA_PER_ATOM * atoms as f64
}

fn timed_stop(b: &Block) -> Result<f64, Failure> {
    b.stop_ha.ok_or_else(|| {
        Failure::Harness(
            "a timed input needs a number scf.energy_tolerance_ha: its timed runs stop there"
                .into(),
        )
    })
}

pub(crate) fn reference_stop(b: &Block) -> Result<f64, Failure> {
    let (bound, floor) = (SHARE * delta_ha(b.atoms), b.floor_ha());
    if TIGHTER * floor > bound {
        return Err(Failure::Harness(format!(
            "{}: its stop floor {floor:e} Ha puts E_ref's stop at TIGHTER × floor = {:e} Ha, past the bound {bound:e} \
             Ha, so E_ref cannot be shown good to SHARE · δ · atoms",
            b.at(),
            TIGHTER * floor
        )));
    }
    Ok((bound / TIGHTER).max(TIGHTER * floor))
}

pub(crate) fn settled(steps: &[Step], reference: f64, delta: f64) -> Option<(usize, f64)> {
    let outside = (steps.iter()).rposition(|s| (s.free_energy_ha - reference).abs() >= delta);
    let from = outside.map_or(0, |i| i + 1);
    steps.get(from).map(|s| (from, s.seconds))
}

fn shuffled(seed: u64, round: u64, n: usize) -> Vec<usize> {
    let mut state = seed ^ round.wrapping_mul(0xd1b5_4a32_d192_ed03);
    let mut order: Vec<usize> = (0..n).collect();
    for i in (1..n).rev() {
        let j = (engine::splitmix(&mut state) % (i as u64 + 1)) as usize;
        order.swap(i, j);
    }
    order
}

pub(crate) fn never_grown(speed: &Speed) -> Speed {
    Speed {
        factor: 1,
        ..*speed
    }
}

pub(crate) fn runs_for(first: &[f64], speed: &Speed, repeats: usize) -> usize {
    let cv = cv(first);
    if first.len() == repeats && (cv.is_nan() || cv > speed.cv_max) {
        speed.factor * repeats
    } else {
        repeats
    }
}

pub(crate) fn noisy(t: &[f64], lost: usize, repeats: usize) -> bool {
    let spread = match t.len() {
        0 | 1 => lost == 0,
        _ => !cv(t).is_finite(),
    };
    t.len() + lost < repeats || spread
}

fn cv(values: &[f64]) -> f64 {
    values.std_dev() / values.mean()
}

pub(crate) fn median(values: &[f64]) -> f64 {
    if values.is_empty() {
        return f64::NAN;
    }
    Data::new(values.to_vec()).median()
}
