use crate::{
    Env, Findings, bench,
    case::{self, Tier},
    git,
    harness::{
        self, Job,
        engine::{Run, free_energy_ha},
        rival,
        speed::{self, median},
    },
    lock,
};
use sha2::{Digest, Sha256};
use std::{
    fmt::Write,
    fs,
    hash::{BuildHasher, RandomState},
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

const RUNS: usize = 5;
const ABBA: [usize; 4] = [0, 1, 1, 0];

pub(crate) fn check(
    root: &Path,
    base: &str,
    time: &[String],
    claim_cutoff: bool,
    env: &Env,
) -> Findings {
    let (plan, mut findings) = plan(root, time, claim_cutoff);
    if !time.is_empty() && plan.jobs.is_empty() {
        return findings;
    }
    let checkout = match Checkout::add(root, base) {
        Ok(checkout) => checkout,
        Err(e) => return vec![format!("same: cannot check out {base}: {e}")],
    };
    let target = root.join("target");
    let engines = bench::engine(env, &checkout.dir, &target.join("same"))
        .and_then(|old| Ok((old, bench::engine(env, root, &target)?)));
    match engines {
        Ok((old, new)) if time.is_empty() => compare(root, base, &checkout.dir, &old, &new),
        Ok((old, new)) => {
            findings.extend(timing(root, base, [&old, &new], plan));
            findings
        }
        Err(findings) => findings,
    }
}

struct Checkout<'a> {
    root: &'a Path,
    dir: PathBuf,
}

impl<'a> Checkout<'a> {
    fn add(root: &'a Path, base: &str) -> Result<Self, String> {
        let tag = RandomState::new().hash_one(std::process::id());
        let dir = std::env::temp_dir().join(format!("orbitar-same-{tag:016x}"));
        git(
            root,
            &["worktree", "add", "--detach", &dir.to_string_lossy(), base],
        )?;
        Ok(Self { root, dir })
    }
}

impl Drop for Checkout<'_> {
    fn drop(&mut self) {
        let dir = self.dir.to_string_lossy();
        if git(self.root, &["worktree", "remove", "--force", &dir]).is_err() {
            let _ = fs::remove_dir_all(&self.dir);
            let _ = git(self.root, &["worktree", "prune"]);
        }
    }
}

fn sweep() -> [NonZeroUsize; 2] {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get().max(2));
    [1, cores].map(|p| NonZeroUsize::new(p).expect("1 and max(N, 2) are positive"))
}

fn compare(root: &Path, base: &str, dir: &Path, old: &Path, new: &Path) -> Findings {
    let (ids, mut findings) = case::active(root, Tier::Pr);
    let listed = match case::ids(dir) {
        Ok(_) => true,
        Err(e) => {
            findings.push(format!(
                "same: the cases at {base}: {e}; no input counts as new"
            ));
            false
        }
    };
    let mut compared: usize = 0;
    for id in ids {
        let jobs = match harness::jobs(root, &id) {
            Ok(jobs) => jobs,
            Err(e) => {
                findings.push(format!("same: {e}"));
                continue;
            }
        };
        let at_base = match listed.then(|| base_inputs(dir, &id)) {
            Some(Ok(names)) => Some(names),
            Some(Err(e)) => {
                findings.push(format!(
                    "same: {id}: its inputs at {base}: {e}; none counts as new"
                ));
                None
            }
            None => None,
        };
        let (new_jobs, jobs): (Vec<Job>, Vec<Job>) = (jobs.into_iter()).partition(|job| {
            at_base
                .as_ref()
                .is_some_and(|names| !names.contains(&job.at))
        });
        for job in &new_jobs {
            println!("same: {}: new since {base}, not compared", job.at);
        }
        for threads in sweep() {
            for job in &jobs {
                let at = format!("{} at {threads} threads", job.at);
                let runs = [old, new].map(|bin| job.run(bin, threads, false).map(|run| run.stdout));
                let (x, y) = match runs {
                    [Ok(x), Ok(y)] => (x, y),
                    runs => {
                        let sides = [format!("at {base}"), "here".to_owned()];
                        for (side, run) in sides.iter().zip(runs) {
                            if let Err(e) = run {
                                findings.push(format!("same: {at}: the engine {side} fails: {e}"));
                            }
                        }
                        continue;
                    }
                };
                compared += 1;
                if x == y {
                    println!("same: {at}: equal");
                    continue;
                }
                let apart = gap(&x, &y).map_or_else(String::new, |d| format!("; |ΔF| = {d:e} Ha"));
                findings.push(format!(
                    "same: {at}: stdout sha256 {} at {base}, {} here{apart}",
                    sha(&x),
                    sha(&y)
                ));
            }
        }
    }
    println!("same: {compared} input runs compared");
    if compared == 0 {
        findings.push("same: no active push or pr input; nothing was compared".into());
    }
    findings
}

fn base_inputs(dir: &Path, id: &str) -> Result<Vec<String>, String> {
    let (named, _) = harness::inputs(dir, &dir.join("benchmarks").join(id), None)?;
    Ok((named.into_iter())
        .map(|(name, _)| format!("{id}/{name}"))
        .collect())
}

#[derive(Default)]
struct Plan {
    jobs: Vec<(Job, String)>,
    cutoff_lines: String,
}

fn plan(root: &Path, names: &[String], claim_cutoff: bool) -> (Plan, Findings) {
    let mut findings = Vec::new();
    let mut jobs = Vec::new();
    for name in names {
        let Some((id, input)) = name.split_once('/') else {
            findings.push(format!(
                "same: --time {name}: name an input as <case>/<input>"
            ));
            continue;
        };
        let job = harness::jobs(root, id).and_then(|jobs| {
            (jobs.into_iter().find(|job| job.at == *name))
                .ok_or_else(|| format!("--time {name}: case {id} has no input {input}"))
        });
        match job {
            Ok(job) => jobs.push(job),
            Err(e) => findings.push(format!("same: {e}")),
        }
    }
    if !claim_cutoff {
        let jobs = jobs.into_iter().map(|job| {
            let label = job.at.clone();
            (job, label)
        });
        let plan = Plan {
            jobs: jobs.collect(),
            cutoff_lines: String::new(),
        };
        return (plan, findings);
    }
    let tuned = root.join(rival::TUNED);
    let loaded = rival::ClaimCutoff::load(root).and_then(|c| {
        Ok((
            c,
            fs::read(&tuned).map_err(|e| format!("{}: {e}", rival::TUNED))?,
        ))
    });
    let (cutoffs, bytes) = match loaded {
        Ok(pair) => pair,
        Err(e) => {
            findings.push(format!("same: --claim-cutoff: {e}"));
            return (Plan::default(), findings);
        }
    };
    let mut plan = Plan::default();
    let _ = writeln!(
        plan.cutoff_lines,
        "# cutoffs: the claim's, ABINIT's frozen ones in {} sha256 {}",
        rival::TUNED,
        sha_bytes(&bytes)
    );
    for job in &jobs {
        match job.at_claim_cutoff(&cutoffs) {
            Ok((job, (cutoff, density))) => {
                let _ = writeln!(
                    plan.cutoff_lines,
                    "# {}: cutoff {cutoff} Ha, density {density} Ha",
                    job.at
                );
                let label = format!("{} at {cutoff} Ha (density {density} Ha)", job.at);
                plan.jobs.push((job, label));
            }
            Err(e) => findings.push(format!("same: --time {e}")),
        }
    }
    (plan, findings)
}

fn timing(root: &Path, base: &str, engines: [&Path; 2], plan: Plan) -> Findings {
    let mut findings = Vec::new();
    let Plan { jobs, cutoff_lines } = plan;
    let revision =
        |rev: &str| git(root, &["rev-parse", rev]).unwrap_or_else(|e| format!("unknown ({e})"));
    let dirty = git(root, &["status", "--porcelain"])
        .map_or("unknown", |s| if s.is_empty() { "clean" } else { "dirty" });
    let mut log = format!(
        "# base {base} {}\n# here {} with a {dirty} work tree\n# every timed run with step energies on, after one unlogged warm-up run per engine and thread count\n\
         {cutoff_lines}# input threads engine seconds t_delta_s t_delta_step peak_rss_bytes stdout_sha256\n",
        revision(&format!("{base}^{{commit}}")),
        revision("HEAD")
    );
    let mut summaries = String::from("# summary\n");
    for (job, label) in &jobs {
        for threads in sweep() {
            match time_one(job, threads, engines, &mut log) {
                Ok(runs) => {
                    let at = format!("{label} at {threads} threads");
                    let line = summary(&at, base, speed::delta_ha(job.atoms), &runs);
                    println!("same: time {line}");
                    summaries.push_str(&line);
                    summaries.push('\n');
                }
                Err(e) => findings.push(format!("same: {e}")),
            }
        }
    }
    let path = root.join("target").join("same").join("time.log");
    log.push_str(&summaries);
    match (path.parent())
        .map_or(Ok(()), fs::create_dir_all)
        .and_then(|()| fs::write(&path, log))
    {
        Ok(()) => println!("same: each run in {}", path.display()),
        Err(e) => findings.push(format!("same: {}: {e}", path.display())),
    }
    findings
}

struct Timed {
    run: Run,
    met: Option<(usize, f64)>,
}

fn time_one(
    job: &Job,
    threads: NonZeroUsize,
    engines: [&Path; 2],
    log: &mut String,
) -> Result<[Vec<Timed>; 2], String> {
    for engine in engines {
        job.run(engine, threads, true)?;
    }
    let mut runs: [Vec<Timed>; 2] = [Vec::new(), Vec::new()];
    for &side in ABBA.iter().cycle().take(2 * RUNS) {
        let run = job.run(engines[side], threads, true)?;
        let met = free_energy_ha(&run.stdout)
            .and_then(|end| speed::settled(&run.steps, end, speed::delta_ha(job.atoms)))
            .map(|(i, t)| (i + 1, t));
        let engine = ["base", "here"][side];
        let row = format!(
            "{} {threads} {engine} {:.6} {} {} {}",
            job.at,
            run.seconds,
            met.map_or_else(|| "none none".to_owned(), |(n, t)| format!("{t:.6} {n}")),
            run.peak_rss_bytes,
            sha(&run.stdout)
        );
        println!("same: run {row}");
        log.push_str(&row);
        log.push('\n');
        if (runs[side].first()).is_some_and(|first| first.run.stdout != run.stdout) {
            return Err(format!(
                "{} at {threads} threads: the {engine} engine's runs print different stdouts",
                job.at
            ));
        }
        runs[side].push(Timed { run, met });
    }
    Ok(runs)
}

fn compared(base: &str, times: [Vec<f64>; 2]) -> String {
    let spread = |s: &[f64]| {
        let lowest = s.iter().copied().fold(f64::INFINITY, f64::min);
        let highest = s.iter().copied().fold(f64::NEG_INFINITY, f64::max);
        (median(s), lowest, highest)
    };
    let [(a, a_min, a_max), (b, b_min, b_max)] = [spread(&times[0]), spread(&times[1])];
    let apart = a_max < b_min || b_max < a_min;
    format!(
        "{base} {a:.3} s ({a_min:.3}–{a_max:.3}), here {b:.3} s ({b_min:.3}–{b_max:.3}), here/base {:.3}, ranges {}",
        b / a,
        if apart { "apart" } else { "overlap" },
    )
}

fn summary(at: &str, base: &str, delta: f64, runs: &[Vec<Timed>; 2]) -> String {
    let wall = compared(
        base,
        runs.each_ref()
            .map(|r| r.iter().map(|t| t.run.seconds).collect()),
    );
    let met = runs
        .each_ref()
        .map(|r| r.iter().filter_map(|t| t.met).collect::<Vec<_>>());
    let missing = [0, 1].map(|i| runs[i].len() - met[i].len());
    let t_delta = if missing == [0, 0] {
        let steps = met.each_ref().map(|m| {
            let n = m.iter().map(|&(n, _)| n);
            (n.clone().min().unwrap_or(0), n.max().unwrap_or(0))
        });
        format!(
            "{}, steps {base} {}–{}, here {}–{}",
            compared(base, met.map(|m| m.iter().map(|&(_, t)| t).collect())),
            steps[0].0,
            steps[0].1,
            steps[1].0,
            steps[1].1
        )
    } else {
        format!(
            "{base} {} of {} runs, here {} of {} runs have none: no step line, no free_energy_ha, or no atoms",
            missing[0],
            runs[0].len(),
            missing[1],
            runs[1].len()
        )
    };
    let [a, b] = [&runs[0][0].run.stdout, &runs[1][0].run.stdout];
    let stdout = if a == b {
        "equal".to_owned()
    } else {
        gap(a, b).map_or_else(
            || "differs".to_owned(),
            |d| format!("differs, |ΔF| {d:e} Ha against δ·atoms {delta:e} Ha"),
        )
    };
    format!("{at}: wall {wall}; T(δ) {t_delta}; stdout {stdout}")
}

fn gap(a: &str, b: &str) -> Option<f64> {
    (free_energy_ha(a).zip(free_energy_ha(b))).map(|(f, g)| (f - g).abs())
}

fn sha_bytes(bytes: &[u8]) -> String {
    lock::hex(&Sha256::digest(bytes))
}

fn sha(text: &str) -> String {
    lock::hex(&Sha256::digest(text))
}
