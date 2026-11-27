use super::engine::Failure;
use crate::{case::Speed, node};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};

const READS_PER_WINDOW: usize = 5;
const READ_PERIOD: Duration = Duration::from_millis(200);
const THE_READER_ITSELF: u32 = 1;

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

pub(crate) struct Gate {
    proc: PathBuf,
    node_cpus: Result<usize, String>,
    refuse: bool,
}

impl Gate {
    pub(crate) fn new(proc: &Path, root: &Path, refuse: bool) -> Gate {
        Gate {
            proc: proc.to_owned(),
            node_cpus: node::declared(root).and_then(|d| node::allowed_cpus(&d)),
            refuse,
        }
    }

    pub(crate) fn before(&self, at: &str, speed: &Speed) -> Result<Reading, Failure> {
        let reading = match &self.node_cpus {
            Ok(cpus) => read(&self.proc, *cpus, speed.cv_max),
            Err(e) => Reading::Unread(e.clone()),
        };
        match (&reading, self.refuse) {
            (Reading::Loaded { why, .. } | Reading::Unread(why), true) => Err(Failure::Harness(
                format!("{at}: the node is not quiet before a timed run: {why}"),
            )),
            _ => Ok(reading),
        }
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
        others_runnable: (reads.iter().map(|r| r.runnable).min())
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

fn once(proc: &Path) -> Result<Read, String> {
    let text = |name: &str| {
        let path = proc.join(name);
        fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))
    };
    let at = Instant::now();
    let (load, pressure) = (text("loadavg")?, text("pressure/cpu")?);
    let bad = |name: &str, text: &str| {
        format!(
            "{}: cannot read `{}`",
            proc.join(name).display(),
            text.trim()
        )
    };
    let fields: Vec<&str> = load.split_whitespace().collect();
    let averages: Option<Vec<f64>> = fields.iter().take(3).map(|f| f.parse().ok()).collect();
    let runnable =
        (fields.get(3).and_then(|f| f.split_once('/'))).and_then(|(r, _)| r.parse().ok());
    let (Some(&[one, five, fifteen]), Some(runnable)) = (averages.as_deref(), runnable) else {
        return Err(bad("loadavg", &load));
    };
    let some = pressure.lines().find_map(|l| l.strip_prefix("some "));
    let value =
        |key: &str| some.and_then(|s| s.split_whitespace().find_map(|kv| kv.strip_prefix(key)));
    let avg10 = value("avg10=").and_then(|v| v.parse().ok());
    let total = value("total=").and_then(|v| v.parse().ok());
    let (Some(avg10), Some(total_us)) = (avg10, total) else {
        return Err(bad("pressure/cpu", &pressure));
    };
    Ok(Read {
        at,
        load_avg: [one, five, fifteen],
        runnable,
        avg10,
        total_us,
    })
}
