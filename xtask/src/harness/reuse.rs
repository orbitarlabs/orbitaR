use super::{
    Block,
    engine::{Failure, Run},
    load::{During, Gate, Reading},
    record, rival,
    speed::{self, BlockRecord, Code, Pass, Runner, Timed},
};
use crate::case::{Case, Speed, Tier};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    cell::OnceCell,
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

pub(crate) const VAR: &str = "ORBITAR_RIVAL_RECORDS";
pub(crate) const DRIFT_SHARE: f64 = 0.1;
pub(crate) const DRIFT_RUNS: usize = 3;
pub const PROTOCOL_FILES: [&str; 11] = [
    "xtask/src/harness.rs",
    "xtask/src/harness/engine.rs",
    "xtask/src/harness/speed.rs",
    "xtask/src/harness/load.rs",
    "xtask/src/harness/rival.rs",
    "xtask/src/harness/rival/abinit.rs",
    "xtask/src/harness/rival/qe.rs",
    "xtask/src/harness/reuse.rs",
    "xtask/src/harness/record.rs",
    "xtask/src/case.rs",
    "benchmarks/RIVALS.toml",
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Protocol {
    pub(crate) measuring_code_sha256: String,
    pub(crate) containment: Value,
    pub(crate) memory_guard_bytes: Option<f64>,
}

impl Protocol {
    pub(crate) fn of(root: &Path, containment: Value, memory_guard_bytes: Option<f64>) -> Protocol {
        let mut all = Sha256::new();
        for file in PROTOCOL_FILES {
            let sha = fs::read(root.join(file))
                .map_or_else(|e| format!("unread: {}", e.kind()), |b| sha256(&b));
            all.update(format!("{sha}  {file}\n"));
        }
        Protocol {
            measuring_code_sha256: crate::lock::hex(&all.finalize()),
            containment,
            memory_guard_bytes,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Key {
    pub(crate) schema: u64,
    pub(crate) pilot: bool,
    pub(crate) repeats: usize,
    pub(crate) timeout_s: u64,
    pub(crate) protocol: Protocol,
    pub(crate) decks: BTreeMap<String, String>,
    pub(crate) identity: Value,
    pub(crate) frozen: rival::Layout,
    pub(crate) fitted: rival::Layout,
    pub(crate) layout_policy: Option<String>,
    pub(crate) cutoff_ha: f64,
    pub(crate) mixing: Vec<String>,
    pub(crate) thread_env: BTreeMap<String, String>,
    pub(crate) mpiexec: Vec<String>,
    pub(crate) invocation_sha256: String,
    pub(crate) runtime: BTreeMap<String, String>,
    pub(crate) datasets: BTreeMap<String, String>,
    pub(crate) stop_floor_ha: Option<f64>,
    pub(crate) density_ratio: f64,
    pub(crate) volume_scale: f64,
    pub(crate) package: Value,
    pub(crate) node: Value,
    pub(crate) declared_host: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) commit: String,
    pub(crate) record_sha256: String,
    pub(crate) started_unix_ms: u64,
}

pub(crate) struct Plan<'a> {
    pub(crate) pilot: bool,
    pub(crate) repeats: usize,
    pub(crate) timeout_s: u64,
    pub(crate) node: &'a toml::Table,
    pub(crate) identity: &'a Value,
    pub(crate) declared_host: &'a Value,
    pub(crate) protocol: &'a Protocol,
}

pub(crate) fn sha256(bytes: &[u8]) -> String {
    crate::lock::hex(&Sha256::digest(bytes))
}

pub(crate) fn key(
    rivals: &rival::Rivals,
    b: &Block,
    identity: Value,
    stressed: bool,
    plan: &Plan,
) -> Result<Key, String> {
    let job = (b.rival.as_ref()).ok_or("orbitaR's own blocks always run")?;
    let claim = (job.claim.as_ref()).ok_or("a tuning job is no claim block")?;
    let failed = |f: Failure| match f {
        Failure::Harness(e) | Failure::Engine(e) => e,
    };
    let timed = b
        .stop_ha
        .ok_or("a timed input needs scf.energy_tolerance_ha")?;
    let reference = speed::reference_stop(b).map_err(failed)?;
    let deck = |job: &rival::Job, stop: f64, mode: rival::Mode| {
        rivals
            .input_text(job, b.datasets, stop, mode)
            .map(|text| sha256(text.as_bytes()))
    };
    let mut decks = BTreeMap::from([
        ("timed".to_owned(), deck(job, timed, rival::Mode::Energy)?),
        (
            "reference".to_owned(),
            deck(job, reference, rival::Mode::Energy)?,
        ),
        (
            "check".to_owned(),
            deck(job, reference / speed::TIGHTER, rival::Mode::Energy)?,
        ),
    ]);
    if stressed {
        decks.insert(
            "reference_stress".into(),
            deck(job, reference, rival::Mode::Stress)?,
        );
    }
    let volume_scale = rivals.settings.target.volume_scale;
    if job.code == rival::Rival::Abinit {
        let p = b.threads.map_or(1, NonZeroUsize::get);
        let v2 = rivals.job(job.code, job.cell.scaled(volume_scale), p, b.datasets)?;
        decks.insert("v2".into(), deck(&v2, reference, rival::Mode::Energy)?);
    }
    let name = job.code.adapter().name();
    Ok(Key {
        schema: record::SCHEMA,
        pilot: plan.pilot,
        repeats: plan.repeats,
        timeout_s: plan.timeout_s,
        protocol: plan.protocol.clone(),
        decks,
        identity,
        frozen: job.frozen.clone(),
        fitted: job.layout.clone(),
        layout_policy: rivals.tuned.of(job.code).layout_policy.clone(),
        cutoff_ha: job.cutoff_ha,
        mixing: job.mixing.clone(),
        thread_env: (rival::thread_env(&job.layout).into_iter())
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        mpiexec: job.mpiexec.clone(),
        invocation_sha256: sha256(rivals.invocation(job).as_bytes()),
        runtime: rivals.runtime(job)?,
        datasets: claim.files.clone(),
        stop_floor_ha: claim.floor_ha,
        density_ratio: rivals.settings.setup(job.code).density_ratio,
        volume_scale,
        package: serde_json::to_value(plan.node.get(name)).map_err(|e| e.to_string())?,
        node: plan.identity.clone(),
        declared_host: plan.declared_host.clone(),
    })
}

fn read(dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let named = |e: std::io::Error| format!("{VAR} {}: {e}", dir.display());
    let mut files: Vec<PathBuf> = (fs::read_dir(dir).map_err(named)?)
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(named)?;
    files.retain(|p| p.extension().is_some_and(|x| x == "json"));
    let mut out = BTreeMap::new();
    for path in files {
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let line = text.strip_suffix('\n').unwrap_or(&text);
        out.insert(sha256(line.as_bytes()), line.to_owned());
    }
    Ok(out)
}

pub(crate) fn lines(text: &str) -> BTreeMap<String, String> {
    (text.lines())
        .map(|line| (sha256(line.as_bytes()), line.to_owned()))
        .collect()
}

pub(crate) struct Sources {
    pinned: BTreeMap<String, String>,
    dir: Option<PathBuf>,
    read: OnceCell<Result<BTreeMap<String, String>, String>>,
}

impl Sources {
    pub(crate) fn new(pinned: BTreeMap<String, String>, dir: Option<&Path>) -> Sources {
        Sources {
            pinned,
            dir: dir.map(Path::to_path_buf),
            read: OnceCell::new(),
        }
    }

    pub(crate) fn none() -> Sources {
        Sources::new(BTreeMap::new(), None)
    }

    pub(crate) fn get(&self, sha: &str) -> Result<&str, String> {
        if let Some(text) = self.pinned.get(sha) {
            return Ok(text);
        }
        let missing = |why: &str| {
            format!(
                "its source record {sha} is not available: {why} ({VAR} for a run, {} for results)",
                crate::results::REUSED
            )
        };
        let Some(dir) = &self.dir else {
            return Err(missing("no pinned line holds it and no directory is named"));
        };
        match self.read.get_or_init(|| read(dir)) {
            Ok(all) => (all.get(sha).map(String::as_str))
                .ok_or_else(|| missing("no record in the named directory has that sha256")),
            Err(e) => Err(missing(e)),
        }
    }
}

pub(crate) fn named(record: &Value) -> Vec<String> {
    let mut out: Vec<String> = (record["speed"].as_array().into_iter().flatten())
        .filter_map(|b| b["reused"]["record_sha256"].as_str())
        .map(str::to_owned)
        .collect();
    out.sort();
    out.dedup();
    out
}

pub(crate) struct Claimant<'a> {
    pub(crate) root: &'a Path,
    pub(crate) id: &'a str,
    pub(crate) case: &'a Case,
    pub(crate) pilot: bool,
    pub(crate) started_unix_ms: u64,
    pub(crate) commit: &'a str,
}

impl Claimant<'_> {
    fn repeats(&self) -> usize {
        match self.pilot {
            true => 1,
            false => self.case.repeats() as usize,
        }
    }

    fn speed(&self) -> Result<Speed, String> {
        let speed = (Tier::Claim.policy().speed).ok_or("the claim tier times nothing")?;
        Ok(match self.pilot {
            true => speed::never_grown(&speed),
            false => speed,
        })
    }

    fn source(&self, sha: &str, text: &str) -> Result<(Source, Value), String> {
        let record: Value = serde_json::from_str(text).map_err(|e| format!("not JSON: {e}"))?;
        let wanted = [
            ("case", json!(self.id)),
            ("schema", json!(record::SCHEMA)),
            ("tier", json!(Tier::Claim.name())),
            ("pilot", json!(self.pilot)),
            ("dirty", json!(false)),
        ];
        for (key, want) in wanted {
            if record[key] != want {
                return Err(format!("its {key} is {}, not {want}", record[key]));
            }
        }
        let commit = record["commit"].as_str().unwrap_or_default();
        let hex = commit.len() == 40 && commit.bytes().all(|b| b.is_ascii_hexdigit());
        if !hex {
            return Err(format!("it names no full commit hash ({commit:?})"));
        }
        let started = (record["started_unix_ms"].as_u64()).ok_or("it names no start time")?;
        if started >= self.started_unix_ms {
            return Err(format!(
                "it started at {started} ms, not before the reusing run's {} ms",
                self.started_unix_ms
            ));
        }
        let ancestor = crate::git(
            self.root,
            &["merge-base", "--is-ancestor", commit, self.commit],
        );
        if ancestor.is_err() {
            return Err(format!(
                "its commit {commit} is no ancestor of the reusing run's commit {}",
                self.commit
            ));
        }
        let declared = crate::node::declared(self.root)?;
        let theirs: toml::Table = serde_json::from_value(record["node"].clone())
            .map_err(|e| format!("its node block: {e}"))?;
        let apart = crate::node::differences(&theirs, &declared, &tables());
        if !apart.is_empty() {
            return Err(format!("its node is not NODE.toml's: {}", apart.join("; ")));
        }
        let source = Source {
            commit: commit.to_owned(),
            record_sha256: sha.to_owned(),
            started_unix_ms: started,
        };
        Ok((source, record))
    }

    fn block(
        &self,
        record: &Value,
        code: Code,
        input: &str,
        threads: NonZeroUsize,
    ) -> Result<BlockRecord, String> {
        let block = (record["speed"].as_array().into_iter().flatten())
            .find(|b| {
                b["code"] == json!(code)
                    && b["input"] == json!(input)
                    && b["threads"] == json!(threads)
                    && b["reused"].is_null()
            })
            .ok_or("it holds no fresh block of the same code, input and threads")?;
        let block: BlockRecord =
            serde_json::from_value(block.clone()).map_err(|e| format!("its block: {e}"))?;
        let summed = speed::timing(
            &block.t_delta_s,
            &block.peak_rss_bytes,
            &block.peak_pss_bytes,
        );
        let holes = [
            (block.reuse_key.is_none(), "it holds no reuse key"),
            (block.refused.is_some(), "it was refused"),
            (block.e_ref_ha.is_none(), "it holds no E_ref"),
            (block.timing.is_none(), "it holds no judged run"),
            (
                block.timing != summed,
                "its summary is not what its samples give",
            ),
        ];
        if let Some((_, why)) = holes.iter().find(|(hole, _)| *hole) {
            return Err((*why).to_owned());
        }
        let settings = rival::settings(self.root).ok();
        let speed = self.speed()?;
        let judged = record::block_findings(
            Tier::Claim,
            self.id,
            self.case,
            (&speed, self.repeats()),
            settings.as_ref(),
            &block,
        );
        match judged.first() {
            Some(first) => Err(format!("its block is not judged sound: {first}")),
            None => Ok(block),
        }
    }
}

fn tables() -> Vec<&'static str> {
    std::iter::once("host")
        .chain(rival::Rival::all().iter().map(|r| r.adapter().name()))
        .collect()
}

pub(crate) struct Want {
    pub(crate) code: Code,
    pub(crate) input: String,
    pub(crate) threads: NonZeroUsize,
    pub(crate) key: Key,
    pub(crate) needs_v2: bool,
    pub(crate) needs_pressure: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DriftRun {
    pub(crate) t_delta_s: Option<f64>,
    pub(crate) peak_pss_bytes: u64,
    pub(crate) loss: Option<String>,
}

pub(crate) fn drift_holds(stored: &BlockRecord, runs: &[DriftRun]) -> Result<(), String> {
    if runs.len() != DRIFT_RUNS {
        return Err(format!("{} drift runs, not {DRIFT_RUNS}", runs.len()));
    }
    if let Some(loss) = runs.iter().find_map(|r| r.loss.as_ref()) {
        return Err(format!("a drift run was lost: {loss}"));
    }
    let timing = (stored.timing.as_ref()).ok_or("the stored block holds no judged run")?;
    let t: Vec<f64> = runs.iter().filter_map(|r| r.t_delta_s).collect();
    let pss: Vec<f64> = runs.iter().map(|r| r.peak_pss_bytes as f64).collect();
    let (t, pss) = (speed::median(&t), speed::median(&pss));
    let near = |x: f64, stored: f64| (x - stored).abs() <= DRIFT_SHARE * stored;
    if !near(t, timing.median_t_delta_s) {
        return Err(format!(
            "the drift runs' median T(δ) {t} s is more than {DRIFT_SHARE} of the stored median {} s away",
            timing.median_t_delta_s
        ));
    }
    if !near(pss, timing.median_peak_pss_bytes) {
        return Err(format!(
            "the drift runs' median peak Pss {pss} bytes is more than {DRIFT_SHARE} of the stored median {} bytes away",
            timing.median_peak_pss_bytes
        ));
    }
    Ok(())
}

pub(crate) struct Lookup {
    pub(crate) found: Vec<Option<BlockRecord>>,
    pilot: bool,
    protocol: Protocol,
    refused: Option<String>,
    refused_codes: BTreeMap<String, String>,
    unfit: Vec<String>,
    rerun: Vec<Value>,
    drift: Vec<Value>,
}

impl Lookup {
    pub(crate) fn find(
        claimant: &Claimant,
        dir: &Path,
        node: &toml::Table,
        protocol: &Protocol,
        wants: &[Option<Want>],
    ) -> Lookup {
        let mut lookup = Lookup {
            found: wants.iter().map(|_| None).collect(),
            pilot: claimant.pilot,
            protocol: protocol.clone(),
            refused: None,
            refused_codes: BTreeMap::new(),
            unfit: Vec::new(),
            rerun: Vec::new(),
            drift: Vec::new(),
        };
        let here = crate::node::declared(claimant.root)
            .map(|declared| crate::node::differences(node, &declared, &tables()));
        match here {
            Ok(apart) if apart.is_empty() => {}
            Ok(apart) => {
                lookup.refused = Some(format!(
                    "this host is not {}'s node: {}",
                    crate::node::PATH,
                    apart.join("; ")
                ));
                return lookup;
            }
            Err(e) => {
                lookup.refused = Some(e);
                return lookup;
            }
        }
        let all = match read(dir) {
            Ok(all) => all,
            Err(e) => {
                println!("reuse: {e}");
                lookup.refused = Some(format!("{VAR} names a directory that does not read"));
                return lookup;
            }
        };
        let mut candidates = Vec::new();
        for (sha, text) in &all {
            let ours =
                serde_json::from_str::<Value>(text).is_ok_and(|r| r["case"] == json!(claimant.id));
            if !ours {
                continue;
            }
            match claimant.source(sha, text) {
                Ok(pair) => candidates.push(pair),
                Err(why) => lookup.unfit.push(format!("record {sha}: {why}")),
            }
        }
        candidates.sort_by_key(|(s, _)| std::cmp::Reverse(s.started_unix_ms));
        for (i, want) in wants.iter().enumerate() {
            let Some(want) = want else { continue };
            let mut why = Vec::new();
            for (source, record) in &candidates {
                match matched(claimant, source, record, want) {
                    Ok(block) => {
                        lookup.found[i] = Some(block);
                        break;
                    }
                    Err(e) => why.push(format!("record {}: {e}", source.record_sha256)),
                }
            }
            if lookup.found[i].is_none() {
                if candidates.is_empty() {
                    why.push(format!("no fit record of {VAR} holds this case"));
                }
                lookup.rerun.push(json!({
                    "code": want.code, "input": want.input, "threads": want.threads, "why": why,
                }));
            }
        }
        lookup
    }

    pub(crate) fn any(&self) -> bool {
        self.found.iter().any(Option::is_some)
    }

    pub(crate) fn guard(
        &mut self,
        blocks: &[Block],
        run: Runner,
        gate: Option<&Gate>,
        speed: &Speed,
    ) -> Result<(), Failure> {
        let codes: BTreeSet<Code> = (self.found.iter().zip(blocks))
            .filter(|(f, _)| f.is_some())
            .map(|(_, b)| b.code())
            .collect();
        for code in codes {
            let reused = (blocks.iter().zip(&self.found))
                .map(|(b, f)| (b.code() == code && f.is_some()).then_some(b.atoms));
            let Some(i) = smallest(reused) else { continue };
            let (b, Some(stored)) = (&blocks[i], self.found[i].as_ref()) else {
                continue;
            };
            let at = b.at();
            let (mut runs, mut load, mut during) = (Vec::new(), Vec::new(), Vec::new());
            for _ in 0..DRIFT_RUNS {
                let started = gate.map(|g| g.before(&at, speed)).transpose()?;
                let (reading, start) = started.map_or((None, None), |(r, s)| (Some(r), Some(s)));
                let ran = match b.stop_ha {
                    Some(stop) => run(b, stop, Pass::Timed),
                    None => Err(Failure::Harness("no timed stop".into())),
                };
                if let (Some(g), Some(start)) = (gate, start) {
                    during.push(g.after(&at, speed, start)?);
                }
                load.extend(reading);
                runs.push(drift_run(stored, b.atoms, ran));
            }
            let held = drift_holds(stored, &runs);
            match &held {
                Ok(()) => println!("reuse: {at}: the drift check held"),
                Err(why) => println!("reuse: {at}: the drift check refuses {code}: {why}"),
            }
            let entry = drift_entry(b, stored, &runs, &load, &during, &held);
            self.drift.push(entry);
            if let Err(why) = held {
                self.refused_codes.insert(code.to_string(), why);
                for (f, _) in (self.found.iter_mut().zip(blocks)).filter(|(_, b)| b.code() == code)
                {
                    *f = None;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn record(&self) -> Value {
        json!({
            "mode": if self.pilot { "pilot" } else { "claim" },
            "protocol": self.protocol,
            "reused": self.found.iter().flatten().count(),
            "refused": self.refused,
            "refused_codes": self.refused_codes,
            "unfit": self.unfit,
            "drift": self.drift,
            "rerun": self.rerun,
        })
    }
}

fn smallest(atoms: impl Iterator<Item = Option<usize>>) -> Option<usize> {
    (atoms.enumerate())
        .filter_map(|(i, a)| a.map(|a| (a, i)))
        .min()
        .map(|(_, i)| i)
}

fn drift_run(stored: &BlockRecord, atoms: usize, ran: Result<Run, Failure>) -> DriftRun {
    let delta = speed::delta_ha(atoms);
    match ran {
        Ok(r) => {
            let t = (stored.e_ref_ha)
                .and_then(|e| speed::settled(&r.steps, e, delta))
                .map(|(_, t)| t);
            DriftRun {
                t_delta_s: t,
                peak_pss_bytes: r.peak_pss_bytes,
                loss: t
                    .is_none()
                    .then(|| "the run never held |E − E_ref| < δ to its end".into()),
            }
        }
        Err(Failure::Engine(e) | Failure::Harness(e)) => DriftRun {
            t_delta_s: None,
            peak_pss_bytes: 0,
            loss: Some(e),
        },
    }
}

fn drift_entry(
    b: &Block,
    stored: &BlockRecord,
    runs: &[DriftRun],
    load: &[Reading],
    during: &[During],
    held: &Result<(), String>,
) -> Value {
    json!({
        "code": b.code(), "input": b.input, "threads": b.threads, "atoms": b.atoms,
        "source": stored.reused, "runs": runs, "load": load, "during": during,
        "share": DRIFT_SHARE, "held": held.is_ok(), "why": held.as_ref().err(),
    })
}

fn matched(
    claimant: &Claimant,
    source: &Source,
    record: &Value,
    want: &Want,
) -> Result<BlockRecord, String> {
    let mut block = claimant.block(record, want.code, &want.input, want.threads)?;
    let key = block
        .reuse_key
        .as_ref()
        .ok_or("its block holds no reuse key")?;
    let apart = differing(key, &want.key);
    if !apart.is_empty() {
        return Err(format!("its key differs in {}", apart.join(", ")));
    }
    let holes = [
        (
            want.needs_v2 && block.e_v2_ha.is_none(),
            "it holds no energy at V₂",
        ),
        (
            want.needs_pressure && block.p_ref_gpa.is_none(),
            "it holds no reference pressure",
        ),
    ];
    if let Some((_, why)) = holes.iter().find(|(hole, _)| *hole) {
        return Err((*why).to_owned());
    }
    block.reused = Some(source.clone());
    Ok(block)
}

fn differing(a: &Key, b: &Key) -> Vec<String> {
    let (a, b) = (json!(a), json!(b));
    (a.as_object().into_iter().flatten())
        .filter(|(k, v)| b[k.as_str()] != **v)
        .map(|(k, _)| k.clone())
        .collect()
}

pub(crate) struct Split<'a> {
    pub(crate) fresh: Vec<Block<'a>>,
    at: Vec<usize>,
    kept: Vec<(usize, Block<'a>, BlockRecord)>,
}

pub(crate) fn split<'a>(blocks: Vec<Block<'a>>, found: Vec<Option<BlockRecord>>) -> Split<'a> {
    let mut split = Split {
        fresh: Vec::new(),
        at: Vec::new(),
        kept: Vec::new(),
    };
    for (i, (b, found)) in blocks.into_iter().zip(found).enumerate() {
        match found {
            Some(record) => split.kept.push((i, b, record)),
            None => {
                split.at.push(i);
                split.fresh.push(b);
            }
        }
    }
    split
}

impl<'a> Split<'a> {
    pub(crate) fn merged(self, timed: Vec<Timed>) -> (Vec<Block<'a>>, Vec<Timed>) {
        let fresh = (self.at.into_iter()).zip(self.fresh.into_iter().zip(timed));
        let kept = (self.kept.into_iter()).map(|(i, b, record)| {
            let timed = Timed {
                stdout: String::new(),
                runs: Vec::new(),
                record,
            };
            (i, (b, timed))
        });
        let mut all: Vec<(usize, (Block<'a>, Timed))> = fresh.chain(kept).collect();
        all.sort_by_key(|(i, _)| *i);
        all.into_iter().map(|(_, pair)| pair).unzip()
    }
}

fn normalized(block: &BlockRecord) -> Value {
    let mut block = json!(block);
    for key in ["accuracy", "reused"] {
        block[key] = Value::Null;
    }
    block
}

pub(crate) fn proven(
    root: &Path,
    id: &str,
    case: &Case,
    record: &Value,
    sources: &Sources,
) -> Vec<String> {
    let blocks: Vec<BlockRecord> = (record["speed"].as_array().into_iter().flatten())
        .filter_map(|b| serde_json::from_value(b.clone()).ok())
        .collect();
    let reused: Vec<&BlockRecord> = blocks.iter().filter(|b| b.reused.is_some()).collect();
    let reuse = &record["rival_reuse"];
    let mut findings = Vec::new();
    let counted = reuse["reused"].as_u64().unwrap_or(0);
    if counted != reused.len() as u64 {
        findings.push(format!(
            "{id} marks {} reused blocks, but its rival_reuse counts {counted}",
            reused.len()
        ));
    }
    if reused.is_empty() {
        return findings;
    }
    let pilot = record["pilot"].as_bool().unwrap_or(false);
    let mode = if pilot { "pilot" } else { "claim" };
    if record["tier"] != json!(Tier::Claim.name()) || reuse["mode"] != json!(mode) {
        findings.push(format!(
            "{id} reuses rival blocks in tier {} and mode {}, not the claim tier's {mode}",
            record["tier"], reuse["mode"]
        ));
    }
    let claimant = Claimant {
        root,
        id,
        case,
        pilot,
        started_unix_ms: record["started_unix_ms"].as_u64().unwrap_or(0),
        commit: record["commit"].as_str().unwrap_or_default(),
    };
    let ours = serde_json::from_value::<toml::Table>(record["node"].clone());
    match (crate::node::declared(root), ours) {
        (Ok(declared), Ok(ours)) => {
            let apart = crate::node::differences(&ours, &declared, &tables());
            if !apart.is_empty() {
                findings.push(format!(
                    "{id} reuses rival blocks on a node that is not NODE.toml's: {}",
                    apart.join("; ")
                ));
            }
        }
        (Err(e), _) => findings.push(format!("{id}: {e}")),
        (_, Err(e)) => findings.push(format!("{id}: its node block: {e}")),
    }
    for b in blocks.iter().filter(|b| b.code != Code::Orbitar) {
        let protocol = b.reuse_key.as_ref().map(|k| json!(k.protocol));
        if protocol.as_ref() != Some(&reuse["protocol"]) {
            findings.push(format!(
                "{id}: the {} block {} at {} threads was keyed under another protocol than the run's",
                b.code, b.input, b.threads
            ));
        }
    }
    for b in &reused {
        if let Err(why) = from_source(&claimant, b, sources) {
            findings.push(format!(
                "{id}: its reused block {} {} at {} threads: {why}",
                b.code, b.input, b.threads
            ));
        }
    }
    let codes: BTreeSet<Code> = reused.iter().map(|b| b.code).collect();
    for code in codes {
        if let Err(why) = drifted(&blocks, code, reuse, pilot) {
            findings.push(format!(
                "{id} reuses {code} blocks without a drift check that holds: {why}"
            ));
        }
    }
    findings
}

fn from_source(claimant: &Claimant, block: &BlockRecord, sources: &Sources) -> Result<(), String> {
    if block.code == Code::Orbitar {
        return Err("orbitaR's own blocks always run".into());
    }
    let named = block.reused.as_ref().ok_or("it is no reused block")?;
    let text = sources.get(&named.record_sha256)?;
    let (source, record) = claimant.source(&named.record_sha256, text)?;
    if source != *named {
        return Err(format!(
            "it names commit {} started at {}, but its source record says commit {} started at {}",
            named.commit, named.started_unix_ms, source.commit, source.started_unix_ms
        ));
    }
    let theirs = claimant.block(&record, block.code, &block.input, block.threads)?;
    if normalized(&theirs) != normalized(block) {
        return Err("its samples, key or E_ref differ from its source's block".into());
    }
    let key = (block.reuse_key.as_ref()).ok_or("it holds no reuse key")?;
    let expected = [
        (key.schema == record::SCHEMA, "schema"),
        (key.pilot == claimant.pilot, "pilot"),
        (key.repeats == claimant.repeats(), "repeats"),
        (key.timeout_s == claimant.case.timeout_s(), "timeout_s"),
    ];
    match expected.iter().find(|(holds, _)| !holds) {
        Some((_, field)) => Err(format!("its key's {field} is not this run's")),
        None => Ok(()),
    }
}

fn drifted(blocks: &[BlockRecord], code: Code, reuse: &Value, pilot: bool) -> Result<(), String> {
    let reused = (blocks.iter()).map(|b| (b.code == code && b.reused.is_some()).then_some(b.atoms));
    let b = &blocks[smallest(reused).ok_or("no reused block")?];
    let entry = (reuse["drift"].as_array().into_iter().flatten())
        .find(|d| d["code"] == json!(code))
        .ok_or("the record holds no drift entry for it")?;
    let cell = [
        (entry["input"] == json!(b.input), "input"),
        (entry["threads"] == json!(b.threads), "threads"),
        (entry["source"] == json!(b.reused), "source"),
    ];
    if let Some((_, field)) = cell.iter().find(|(same, _)| !same) {
        return Err(format!(
            "its drift entry's {field} is not that of its smallest reused cell {}",
            b.input
        ));
    }
    let runs: Vec<DriftRun> = serde_json::from_value(entry["runs"].clone())
        .map_err(|e| format!("its drift runs: {e}"))?;
    drift_holds(b, &runs)?;
    if entry["held"] != json!(true) {
        return Err("its drift entry does not say held".into());
    }
    if !pilot {
        let quiet = |key: &str, quiet: &dyn Fn(&Value) -> bool| {
            (entry[key].as_array())
                .is_some_and(|all| all.len() == DRIFT_RUNS && all.iter().all(quiet))
        };
        let reading =
            |v: &Value| serde_json::from_value::<Reading>(v.clone()).is_ok_and(|r| r.quiet());
        let alone =
            |v: &Value| serde_json::from_value::<During>(v.clone()).is_ok_and(|d| d.quiet());
        if !quiet("load", &reading) || !quiet("during", &alone) {
            return Err(format!(
                "its {DRIFT_RUNS} drift runs did not each start on a quiet node and run alone"
            ));
        }
    }
    Ok(())
}
