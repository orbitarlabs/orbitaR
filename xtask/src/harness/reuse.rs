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
    pub(crate) repeats: usize,
    pub(crate) speed: &'a Speed,
    pub(crate) started_unix_ms: u64,
    pub(crate) commit: &'a str,
}

impl Claimant<'_> {
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
        let judged = record::block_findings(
            Tier::Claim,
            self.id,
            self.case,
            (self.speed, self.repeats, self.pilot),
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
    pub(crate) key: Result<Key, String>,
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
    let wanted =
        (want.key.as_ref()).map_err(|e| format!("this run's block has no reuse key: {e}"))?;
    let mut block = claimant.block(record, want.code, &want.input, want.threads)?;
    let key = block
        .reuse_key
        .as_ref()
        .ok_or("its block holds no reuse key")?;
    let apart = differing(key, wanted);
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

pub(crate) fn allowed(tier: crate::case::Tier, pilot: bool) -> Result<(), String> {
    match tier == Tier::Claim && pilot {
        true => Ok(()),
        false => Err(format!(
            "{VAR} reuses rival blocks in a pilot alone (ORBITAR_PILOT=1, claim tier), not in {}{}: a claim times every rival fresh",
            tier.name(),
            if tier == Tier::Claim {
                " without ORBITAR_PILOT"
            } else {
                ""
            }
        )),
    }
}

pub(crate) fn refused_in(id: &str, record: &Value) -> Option<String> {
    let reused = named(record);
    (!reused.is_empty() && record["pilot"] != json!(true)).then(|| {
        format!(
            "{id} holds rival blocks reused from the records {}; only a pilot reuses, and a claim times every rival fresh",
            reused.join(", ")
        )
    })
}
