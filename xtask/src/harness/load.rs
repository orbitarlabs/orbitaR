use super::engine::Failure;
use crate::{case::Speed, node};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    cell::RefCell,
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const READS_PER_WINDOW: usize = 5;
const READ_PERIOD: Duration = Duration::from_millis(200);
const WINDOWS_BEFORE_A_REFUSAL: usize = 3;
const THE_READER_ITSELF: u32 = 1;
const USER_HZ: f64 = 100.0;

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Sample {
    pub(crate) load_avg: [f64; 3],
    pub(crate) others_runnable: u32,
    pub(crate) psi_some_avg10: f64,
    pub(crate) psi_some_share: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reading {
    Quiet(Sample),
    Loaded { sample: Sample, why: String },
    Unread(String),
}

impl Reading {
    pub(crate) fn quiet(&self) -> bool {
        matches!(self, Reading::Quiet(_))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub(crate) struct Contention {
    pub(crate) wall_s: f64,
    pub(crate) busy_cpu_s: f64,
    pub(crate) child_cpu_s: f64,
    pub(crate) harness_cpu_s: f64,
    pub(crate) others_cpu_s: f64,
    pub(crate) share: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum During {
    Quiet(Contention),
    Loaded { contention: Contention, why: String },
    Unread(String),
}

impl During {
    pub(crate) fn quiet(&self) -> bool {
        matches!(self, During::Quiet(_))
    }
}

pub(crate) struct Start {
    at: Instant,
    counters: Result<Counters, String>,
}

pub(crate) struct Gate {
    proc: PathBuf,
    node_cpus: Result<usize, String>,
    refuse: bool,
    log: RefCell<Vec<Value>>,
}

impl Gate {
    pub(crate) fn new(proc: &Path, root: &Path, refuse: bool) -> Gate {
        Gate {
            proc: proc.to_owned(),
            node_cpus: node::declared(root).and_then(|d| node::allowed_cpus(&d)),
            refuse,
            log: RefCell::new(Vec::new()),
        }
    }

    pub(crate) fn log(&self) -> Vec<Value> {
        self.log.borrow().clone()
    }

    fn refused(&self, at: &str, why: &str) -> Result<(), Failure> {
        match self.refuse {
            true => Err(Failure::Harness(format!(
                "{at}: the node is not quiet for a timed run: {why}"
            ))),
            false => Ok(()),
        }
    }

    pub(crate) fn before(&self, at: &str, speed: &Speed) -> Result<(Reading, Start), Failure> {
        let mut reading = Reading::Unread("no window read".to_owned());
        for _ in 0..WINDOWS_BEFORE_A_REFUSAL {
            reading = match &self.node_cpus {
                Ok(cpus) => read(&self.proc, *cpus, speed.cv_max),
                Err(e) => Reading::Unread(e.clone()),
            };
            self.log
                .borrow_mut()
                .push(json!({ "at": at, "before": reading }));
            if reading.quiet() || matches!(reading, Reading::Unread(_)) {
                break;
            }
        }
        if let Reading::Loaded { why, .. } | Reading::Unread(why) = &reading {
            self.refused(at, why)?;
        }
        let start = Start {
            at: Instant::now(),
            counters: counters(&self.proc),
        };
        Ok((reading, start))
    }

    pub(crate) fn after(&self, at: &str, speed: &Speed, start: Start) -> Result<During, Failure> {
        let wall_s = start.at.elapsed().as_secs_f64();
        let during = match (&self.node_cpus, &start.counters) {
            (Err(e), _) | (_, Err(e)) => During::Unread(e.clone()),
            (Ok(cpus), Ok(first)) => match counters(&self.proc) {
                Err(e) => During::Unread(e),
                Ok(last) => {
                    let busy_cpu_s = last.busy_s - first.busy_s;
                    let harness_cpu_s = last.harness_s - first.harness_s;
                    let child_cpu_s = last.reaped_s - first.reaped_s;
                    let others_cpu_s = (busy_cpu_s - child_cpu_s - harness_cpu_s).max(0.0);
                    let share = others_cpu_s / (wall_s * *cpus as f64);
                    let contention = Contention {
                        wall_s,
                        busy_cpu_s,
                        child_cpu_s,
                        harness_cpu_s,
                        others_cpu_s,
                        share,
                    };
                    match share >= speed.cv_max {
                        true => During::Loaded {
                            why: format!(
                                "other tasks took {others_cpu_s} CPU s over the {wall_s} s of a timed run on the node's {cpus} CPUs: {share} of them, not under {}",
                                speed.cv_max
                            ),
                            contention,
                        },
                        false => During::Quiet(contention),
                    }
                }
            },
        };
        self.log
            .borrow_mut()
            .push(json!({ "at": at, "during": during }));
        if let During::Loaded { why, .. } | During::Unread(why) = &during {
            self.refused(at, why)?;
        }
        Ok(during)
    }
}

fn window(proc: &Path) -> Result<Vec<Read>, String> {
    let mut reads = vec![once(proc)?];
    for _ in 1..READS_PER_WINDOW {
        thread::sleep(READ_PERIOD);
        reads.push(once(proc)?);
    }
    Ok(reads)
}

fn read(proc: &Path, node_cpus: usize, share_max: f64) -> Reading {
    let reads = match window(proc) {
        Ok(reads) => reads,
        Err(e) => return Reading::Unread(e),
    };
    let (first, last) = (&reads[0], &reads[READS_PER_WINDOW - 1]);
    let micros = last.at.duration_since(first.at).as_micros() as f64;
    let sample = Sample {
        load_avg: last.load_avg,
        others_runnable: (reads.iter().map(|r| r.runnable).max())
            .map_or(0, |n| n.saturating_sub(THE_READER_ITSELF)),
        psi_some_avg10: last.avg10,
        psi_some_share: last.total_us.saturating_sub(first.total_us) as f64 / micros,
    };
    let held = f64::from(sample.others_runnable) / node_cpus as f64;
    let why = if held >= share_max {
        Some(format!(
            "{} other runnable tasks hold {held} of the node's {node_cpus} CPUs, not under {share_max}",
            sample.others_runnable
        ))
    } else if sample.psi_some_share >= share_max {
        Some(format!(
            "a task waited for a CPU {} of the window, not under {share_max}",
            sample.psi_some_share
        ))
    } else {
        None
    };
    match why {
        Some(why) => Reading::Loaded { sample, why },
        None => Reading::Quiet(sample),
    }
}

struct Read {
    at: Instant,
    load_avg: [f64; 3],
    runnable: u32,
    avg10: f64,
    total_us: u64,
}

fn text(proc: &Path, name: &str) -> Result<String, String> {
    let path = proc.join(name);
    fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
}

fn bad(proc: &Path, name: &str, text: &str) -> String {
    format!(
        "{}: cannot read `{}`",
        proc.join(name).display(),
        text.trim()
    )
}

fn once(proc: &Path) -> Result<Read, String> {
    let at = Instant::now();
    let (load, pressure) = (text(proc, "loadavg")?, text(proc, "pressure/cpu")?);
    let fields: Vec<&str> = load.split_whitespace().collect();
    let averages: Option<Vec<f64>> = fields.iter().take(3).map(|f| f.parse().ok()).collect();
    let runnable =
        (fields.get(3).and_then(|f| f.split_once('/'))).and_then(|(r, _)| r.parse().ok());
    let (Some(&[one, five, fifteen]), Some(runnable)) = (averages.as_deref(), runnable) else {
        return Err(bad(proc, "loadavg", &load));
    };
    let some = pressure.lines().find_map(|l| l.strip_prefix("some "));
    let value =
        |key: &str| some.and_then(|s| s.split_whitespace().find_map(|kv| kv.strip_prefix(key)));
    let avg10 = value("avg10=").and_then(|v| v.parse().ok());
    let total = value("total=").and_then(|v| v.parse().ok());
    let (Some(avg10), Some(total_us)) = (avg10, total) else {
        return Err(bad(proc, "pressure/cpu", &pressure));
    };
    Ok(Read {
        at,
        load_avg: [one, five, fifteen],
        runnable,
        avg10,
        total_us,
    })
}

struct Counters {
    busy_s: f64,
    harness_s: f64,
    reaped_s: f64,
}

fn numbers<'a>(fields: impl Iterator<Item = &'a str>) -> Option<Vec<f64>> {
    fields.map(|f| f.parse().ok()).collect()
}

fn counters(proc: &Path) -> Result<Counters, String> {
    let stat = text(proc, "stat")?;
    let cpu = (stat.lines().find_map(|l| l.strip_prefix("cpu ")))
        .and_then(|c| numbers(c.split_whitespace()));
    let Some([user, nice, system, _idle, _iowait, irq, softirq, steal, ..]) = cpu.as_deref() else {
        return Err(bad(proc, "stat", &stat));
    };
    let own = text(proc, "self/stat")?;
    let times = (own.rsplit_once(')'))
        .and_then(|(_, rest)| numbers(rest.split_whitespace().skip(11).take(4)));
    let Some([utime, stime, cutime, cstime]) = times.as_deref() else {
        return Err(bad(proc, "self/stat", &own));
    };
    Ok(Counters {
        busy_s: (user + nice + system + irq + softirq + steal) / USER_HZ,
        harness_s: (utime + stime) / USER_HZ,
        reaped_s: (cutime + cstime) / USER_HZ,
    })
}
