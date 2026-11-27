use super::{
    Block,
    engine::Failure,
    load::Gate,
    record, rival,
    speed::{self, BlockRecord, Code, Pass, Runner, Timed},
};
use crate::case::{Speed, Tier};
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

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Key {
    pub(crate) schema: u64,
    pub(crate) pilot: bool,
    pub(crate) repeats: usize,
    pub(crate) timeout_s: u64,
    pub(crate) input_sha256: String,
    pub(crate) variant: String,
    pub(crate) frozen: rival::Layout,
    pub(crate) layout_policy: Option<String>,
    pub(crate) cutoff_ha: f64,
    pub(crate) mixing: Vec<String>,
    pub(crate) thread_env: BTreeMap<String, String>,
    pub(crate) mpiexec: Vec<String>,
    pub(crate) datasets: BTreeMap<String, String>,
    pub(crate) stop_floor_ha: Option<f64>,
    pub(crate) density_ratio: f64,
    pub(crate) volume_scale: f64,
    pub(crate) package: Value,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Source {
    pub(crate) commit: String,
    pub(crate) record_sha256: String,
    pub(crate) started_unix_ms: u64,
}

pub(crate) type Sources = BTreeMap<String, String>;

pub(crate) struct Plan<'a> {
    pub(crate) pilot: bool,
    pub(crate) repeats: usize,
    pub(crate) timeout_s: u64,
    pub(crate) node: &'a toml::Table,
}

pub(crate) fn sha256(text: &str) -> String {
    crate::lock::hex(&Sha256::digest(text.as_bytes()))
}

pub(crate) fn key(
    rivals: &rival::Rivals,
    b: &Block,
    variant: String,
    plan: &Plan,
) -> Result<Key, String> {
    let job = (b.rival.as_ref()).ok_or("orbitaR's own blocks always run")?;
    let stop = b
        .stop_ha
        .ok_or("a timed input needs scf.energy_tolerance_ha")?;
    let claim = (job.claim.as_ref()).ok_or("a tuning job is no claim block")?;
    let name = job.code.adapter().name();
    Ok(Key {
        schema: record::SCHEMA,
        pilot: plan.pilot,
        repeats: plan.repeats,
        timeout_s: plan.timeout_s,
        input_sha256: sha256(&rivals.input_text(job, b.datasets, stop)?),
        variant,
        frozen: job.frozen.clone(),
        layout_policy: rivals.tuned.of(job.code).layout_policy.clone(),
        cutoff_ha: job.cutoff_ha,
        mixing: job.mixing.clone(),
        thread_env: (rival::thread_env(&job.layout).into_iter())
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
        mpiexec: job.mpiexec.clone(),
        datasets: claim.files.clone(),
        stop_floor_ha: claim.floor_ha,
        density_ratio: rivals.settings.setup(job.code).density_ratio,
        volume_scale: rivals.settings.target.volume_scale,
        package: serde_json::to_value(plan.node.get(name)).map_err(|e| e.to_string())?,
    })
}

pub(crate) fn sources(dir: &Path) -> Result<Sources, String> {
    let named = |e: std::io::Error| format!("{VAR} {}: {e}", dir.display());
    let mut files: Vec<PathBuf> = (fs::read_dir(dir).map_err(named)?)
        .map(|entry| entry.map(|e| e.path()))
        .collect::<Result<_, _>>()
        .map_err(named)?;
    files.retain(|p| p.extension().is_some_and(|x| x == "json"));
    let mut out = Sources::new();
    for path in files {
        let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let line = text.strip_suffix('\n').unwrap_or(&text);
        out.insert(sha256(line), line.to_owned());
    }
    Ok(out)
}

pub(crate) fn lines(text: &str) -> Sources {
    (text.lines())
        .map(|line| (sha256(line), line.to_owned()))
        .collect()
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

pub(crate) struct Want {
    pub(crate) code: Code,
    pub(crate) input: String,
    pub(crate) threads: NonZeroUsize,
    pub(crate) key: Key,
    pub(crate) needs_v2: bool,
    pub(crate) needs_pressure: bool,
}

pub(crate) struct Lookup {
    pub(crate) found: Vec<Option<BlockRecord>>,
    dir: PathBuf,
    refused: Option<String>,
    unfit: Vec<String>,
    rerun: Vec<Value>,
    drift: Value,
}

struct Candidate {
    file: String,
    source: Source,
    record: Value,
}

impl Lookup {
    pub(crate) fn find(
        root: &Path,
        dir: &Path,
        id: &str,
        tier: Tier,
        node: &toml::Table,
        wants: &[Option<Want>],
    ) -> Lookup {
        let mut lookup = Lookup {
            found: wants.iter().map(|_| None).collect(),
            dir: dir.to_owned(),
            refused: None,
            unfit: Vec::new(),
            rerun: Vec::new(),
            drift: Value::Null,
        };
        let candidates = match candidates(root, dir, id, tier, node, wants) {
            Ok((candidates, unfit)) => {
                lookup.unfit = unfit;
                candidates
            }
            Err(why) => {
                lookup.refused = Some(why);
                return lookup;
            }
        };
        for (i, want) in wants.iter().enumerate() {
            let Some(want) = want else { continue };
            let mut why = Vec::new();
            for c in &candidates {
                match matched(c, want) {
                    Ok(block) => {
                        lookup.found[i] = Some(block);
                        break;
                    }
                    Err(e) => why.push(format!("{}: {e}", c.file)),
                }
            }
            if lookup.found[i].is_none() {
                if candidates.is_empty() {
                    why.push(format!("{} holds no record of {id}", dir.display()));
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
        let smallest = (self.found.iter().enumerate())
            .filter_map(|(i, f)| f.as_ref().map(|f| (i, f)))
            .min_by_key(|(i, _)| (blocks[*i].atoms, *i));
        let Some((i, source)) = smallest else {
            return Ok(());
        };
        let b = &blocks[i];
        let (Some(timing), Some(e_ref), Some(stop)) = (&source.timing, source.e_ref_ha, b.stop_ha)
        else {
            return Err(Failure::Harness(format!(
                "{}: a reused block holds no timing, E_ref or stop",
                b.at()
            )));
        };
        let at = b.at();
        let started = gate.map(|g| g.before(&at, speed)).transpose()?;
        let (reading, start) = started.map_or((None, None), |(r, s)| (Some(r), Some(s)));
        let ran = run(b, stop, Pass::Timed);
        let during = match (gate, start) {
            (Some(g), Some(start)) => Some(g.after(&at, speed, start)?),
            _ => None,
        };
        let t = match ran {
            Ok(r) => speed::settled(&r.steps, e_ref, speed::delta_ha(b.atoms))
                .map(|(_, t)| t)
                .ok_or_else(|| "the run never held |E − E_ref| < δ to its end".to_owned()),
            Err(Failure::Engine(e)) => Err(e),
            Err(harness) => return Err(harness),
        };
        let median = timing.median_t_delta_s;
        let [low, high] = timing.range_t_delta_s;
        let bound = (DRIFT_SHARE * median).max(high - low);
        let held = t.as_ref().is_ok_and(|t| (t - median).abs() <= bound);
        println!(
            "reuse: {at}: drift check T(δ) {t:?} s against the stored median {median} s ± {bound} s: {}",
            if held { "held" } else { "refused" }
        );
        self.drift = json!({
            "code": b.code(), "input": b.input, "threads": b.threads, "atoms": b.atoms,
            "source": source.reused, "t_delta_s": t.as_ref().ok(), "loss": t.as_ref().err(),
            "median_t_delta_s": median, "range_t_delta_s": [low, high], "share": DRIFT_SHARE,
            "bound_s": bound, "held": held, "load": reading, "during": during,
        });
        if !held {
            self.refused = Some(format!(
                "the drift check of {at} moved past {bound} s of the stored median {median} s; every rival runs again"
            ));
            self.found.iter_mut().for_each(|f| *f = None);
        }
        Ok(())
    }

    pub(crate) fn record(&self) -> Value {
        json!({
            "records": self.dir,
            "reused": self.found.iter().flatten().count(),
            "refused": self.refused,
            "unfit": self.unfit,
            "drift": self.drift,
            "rerun": self.rerun,
        })
    }
}

fn candidates(
    root: &Path,
    dir: &Path,
    id: &str,
    tier: Tier,
    node: &toml::Table,
    wants: &[Option<Want>],
) -> Result<(Vec<Candidate>, Vec<String>), String> {
    let declared = crate::node::declared(root)?;
    let codes: BTreeSet<String> = (wants.iter().flatten())
        .map(|w| w.code.to_string())
        .collect();
    let tables: Vec<&str> = std::iter::once("host")
        .chain(codes.iter().map(String::as_str))
        .collect();
    let here = crate::node::differences(node, &declared, &tables);
    if !here.is_empty() {
        return Err(format!(
            "this host is not {}'s node: {}",
            crate::node::PATH,
            here.join("; ")
        ));
    }
    let (mut out, mut unfit) = (Vec::new(), Vec::new());
    for (sha, text) in sources(dir)? {
        let Ok(record) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if record["case"].as_str() != Some(id) {
            continue;
        }
        let file = format!("record {sha}");
        let eligible = (record["schema"].as_u64() == Some(record::SCHEMA))
            .then_some(())
            .ok_or_else(|| format!("schema {}, not {}", record["schema"], record::SCHEMA))
            .and_then(|()| match record["tier"].as_str() == Some(tier.name()) {
                true => Ok(()),
                false => Err(format!("tier {}, not {}", record["tier"], tier.name())),
            })
            .and_then(|()| {
                let theirs: toml::Table = serde_json::from_value(record["node"].clone())
                    .map_err(|e| format!("its node block: {e}"))?;
                let apart = crate::node::differences(&theirs, &declared, &tables);
                match apart.is_empty() {
                    true => Ok(()),
                    false => Err(format!("its node is not NODE.toml's: {}", apart.join("; "))),
                }
            })
            .and_then(|()| {
                Ok(Source {
                    commit: (record["commit"].as_str())
                        .ok_or("it names no commit")?
                        .to_owned(),
                    record_sha256: sha.clone(),
                    started_unix_ms: (record["started_unix_ms"].as_u64())
                        .ok_or("it names no start time")?,
                })
            });
        match eligible {
            Ok(source) => out.push(Candidate {
                file,
                source,
                record,
            }),
            Err(why) => unfit.push(format!("{file}: {why}")),
        }
    }
    out.sort_by_key(|c| std::cmp::Reverse(c.source.started_unix_ms));
    Ok((out, unfit))
}

fn matched(c: &Candidate, want: &Want) -> Result<BlockRecord, String> {
    let block = (c.record["speed"].as_array().into_iter().flatten())
        .find(|b| {
            b["code"] == json!(want.code)
                && b["input"] == json!(want.input)
                && b["threads"] == json!(want.threads)
                && b["reused"].is_null()
        })
        .ok_or("no timed block of the same code, input and threads")?;
    let mut block: BlockRecord =
        serde_json::from_value(block.clone()).map_err(|e| format!("its block: {e}"))?;
    let key = block
        .reuse_key
        .as_ref()
        .ok_or("its block holds no reuse key")?;
    let apart = differing(key, &want.key);
    if !apart.is_empty() {
        return Err(format!("its key differs in {}", apart.join(", ")));
    }
    let holes = [
        (block.refused.is_some(), "it was refused"),
        (block.timing.is_none(), "it holds no judged run"),
        (block.e_ref_ha.is_none(), "it holds no E_ref"),
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
    block.reused = Some(c.source.clone());
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

fn normalized(block: &Value) -> Result<Value, String> {
    let mut block: BlockRecord =
        serde_json::from_value(block.clone()).map_err(|e| e.to_string())?;
    block.accuracy = None;
    block.reused = None;
    Ok(json!(block))
}

pub(crate) fn proven(id: &str, record: &Value, sources: &Sources) -> Vec<String> {
    let mut findings = Vec::new();
    let reused: Vec<&Value> = (record["speed"].as_array().into_iter().flatten())
        .filter(|b| !b["reused"].is_null())
        .collect();
    for block in &reused {
        let text = |key: &str| block[key].as_str().unwrap_or_default().to_owned();
        let at = format!(
            "{} {} at {} threads",
            text("code"),
            text("input"),
            block["threads"]
        );
        if let Err(why) = from_source(id, record, block, sources) {
            findings.push(format!("{id}: its reused block {at}: {why}"));
        }
    }
    if !reused.is_empty() && record["rival_reuse"]["drift"]["held"] != Value::Bool(true) {
        findings.push(format!(
            "{id} reuses {} rival blocks without a drift check that held: {}",
            reused.len(),
            record["rival_reuse"]["drift"]
        ));
    }
    findings
}

fn from_source(id: &str, record: &Value, block: &Value, sources: &Sources) -> Result<(), String> {
    if block["code"] == json!(Code::Orbitar) {
        return Err("orbitaR's own blocks always run".into());
    }
    if block["reuse_key"].is_null() {
        return Err("it holds no reuse key".into());
    }
    let source: Source =
        serde_json::from_value(block["reused"].clone()).map_err(|e| format!("reused: {e}"))?;
    let text = sources.get(&source.record_sha256).ok_or_else(|| {
        format!(
            "its source record {} is not available ({VAR}, or {} for results)",
            source.record_sha256,
            crate::results::REUSED
        )
    })?;
    let theirs: Value = serde_json::from_str(text).map_err(|e| format!("its source: {e}"))?;
    for (key, want) in [
        ("commit", json!(source.commit)),
        ("started_unix_ms", json!(source.started_unix_ms)),
        ("schema", json!(record::SCHEMA)),
        ("case", json!(id)),
        ("tier", record["tier"].clone()),
        ("pilot", record["pilot"].clone()),
    ] {
        if theirs[key] != want {
            return Err(format!("its source's {key} is {}, not {want}", theirs[key]));
        }
    }
    let table = |r: &Value| serde_json::from_value::<toml::Table>(r["node"].clone());
    let (ours, node) = (table(record), table(&theirs));
    let (Ok(ours), Ok(node)) = (ours, node) else {
        return Err("a node block does not parse".into());
    };
    let code = block["code"].as_str().unwrap_or_default();
    let apart = crate::node::differences(&ours, &node, &["host", code]);
    if !apart.is_empty() {
        return Err(format!(
            "its source ran on another node: {}",
            apart.join("; ")
        ));
    }
    let same = (theirs["speed"].as_array().into_iter().flatten()).find(|b| {
        ["code", "input", "threads"]
            .iter()
            .all(|k| b[*k] == block[*k])
            && b["reused"].is_null()
    });
    let same = same.ok_or("its source holds no timed block of the same code, input and threads")?;
    match normalized(same)? == normalized(block)? {
        true => Ok(()),
        false => Err("its samples, key or E_ref differ from its source's block".into()),
    }
}
