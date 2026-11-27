use super::{Metrics, rival, speed};
use crate::{
    Findings,
    case::{self, Bound, Case, Status, Tier},
    lock,
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    time::SystemTime,
};

pub(crate) const SCHEMA: u64 = 4;

pub(crate) fn judge(tolerance: &BTreeMap<String, f64>, metrics: &Metrics) -> Vec<String> {
    let mut failures = Vec::new();
    for (key, limit) in tolerance {
        let Some((metric, bound)) = crate::case::bounded(key) else {
            failures.push(format!("[tolerance].{key} must end with _max or _min"));
            continue;
        };
        match metrics.get(metric) {
            None => failures.push(format!("metric {metric} missing for [tolerance].{key}")),
            Some(v) if bound == Bound::Max && (v.is_nan() || v > limit) => {
                failures.push(format!("{metric} = {v:e} > {limit:e}"))
            }
            Some(v) if bound == Bound::Min && (v.is_nan() || v < limit) => {
                failures.push(format!("{metric} = {v:e} < {limit:e}"))
            }
            Some(_) => {}
        }
    }
    failures
}

pub(crate) fn measured(
    tolerance: &BTreeMap<String, f64>,
    metrics: &Metrics,
) -> BTreeMap<String, f64> {
    (tolerance.iter())
        .filter(|(key, _)| {
            crate::case::bounded(key).is_none_or(|(metric, _)| metrics.contains_key(metric))
        })
        .map(|(key, limit)| (key.clone(), *limit))
        .collect()
}

pub(crate) fn write(dir: &Path, id: &str, record: &Value) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{id}.json"));
    fs::write(&path, record.to_string()).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn verify(
    root: &Path,
    tier: Tier,
    dir: &Path,
    since: SystemTime,
    case: Option<&str>,
) -> Findings {
    let since_ms = since
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_or(0, |d| d.as_millis()) as f64;
    let mut findings = Vec::new();
    let ids = match case::ids(root) {
        Ok(ids) => ids,
        Err(e) => return vec![e],
    };
    let mut seconds = 0.0;
    let (mut active, mut proved) = (0, 0);
    findings.extend(unreviewed(root, lock::HARNESS).map(|why| format!("{}: {why}", tier.name())));
    for id in ids
        .iter()
        .filter(|id| case.is_none_or(|c| c == id.as_str()))
    {
        let case = match Case::load(root, id) {
            Ok(case) => case,
            Err(e) => {
                findings.push(e);
                continue;
            }
        };
        if !tier.includes(case.tier()) || case.status() == Status::Retired {
            continue;
        }
        active += usize::from(case.status() == Status::Active);
        findings.extend(unreviewed(root, id).map(|why| format!("{}: {why}", tier.name())));
        let record: Value = match load(&dir.join(format!("{id}.json"))) {
            Ok(record) => record,
            Err(e) => {
                findings.push(format!(
                    "{}: {id} left no readable record: {e}",
                    tier.name()
                ));
                continue;
            }
        };
        seconds += record["spent_s"].as_f64().unwrap_or(f64::INFINITY);
        if record["started_unix_ms"]
            .as_f64()
            .is_none_or(|t| t < since_ms)
        {
            findings.push(format!(
                "{}: {id} left a stale record from an earlier run",
                tier.name()
            ));
        }
        proved += usize::from(record["outcome"].as_str() == Some("shard"));
        findings.extend(proves(root, tier, id, &case, &record));
    }
    if active + proved == 0 {
        findings.push(format!(
            "{}: no active case in this tier; it proves nothing",
            tier.name()
        ));
    }
    if let Some(budget) = tier.policy().budget_s
        && seconds > budget as f64
    {
        findings.push(format!(
            "{}: engine time {seconds:.1} s exceeds the {budget} s budget",
            tier.name()
        ));
    }
    findings
}

fn proves(root: &Path, tier: Tier, id: &str, case: &Case, record: &Value) -> Findings {
    let mut findings = Vec::new();
    let times = |key: &str| -> Vec<f64> {
        record[key]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_f64).collect())
            .unwrap_or_default()
    };
    if record["pilot"].as_bool() == Some(true) {
        findings.push(format!(
            "{}: {id} record is a pilot (ORBITAR_PILOT): it prices a claim run and proves nothing",
            tier.name()
        ));
    }
    let own = super::datasets::Table::of_case(root, &root.join("benchmarks").join(id), None)
        .map(|t| t.map_or(Value::Null, |t| t.named()["sha256"].clone()));
    let candidate = match own {
        Ok(sha) => record["table"]["sha256"] != sha,
        Err(e) => {
            findings.push(format!("{}: {id}: {e}", tier.name()));
            true
        }
    };
    if candidate {
        findings.push(format!(
            "{}: {id} ran the dataset table {}, not the case's own; it proves nothing",
            tier.name(),
            record["table"]
        ));
    }
    let runs = times("runs_s");
    let rotations = times("rotation_s");
    if record["seed_fixed"].as_bool() != Some(false) {
        findings.push(format!(
            "{}: {id} ran with a fixed rotation seed; a tier run draws a random one",
            tier.name()
        ));
    }
    let outcome = record["outcome"].as_str().unwrap_or("unreadable");
    let filtered = record["filter"].as_array().is_some_and(|f| !f.is_empty());
    if (filtered || candidate) != (outcome == "shard") && ["pass", "shard"].contains(&outcome) {
        findings.push(format!(
            "{}: {id} record says {outcome}, but its filter is {}; only a filtered run or a candidate-table run is a shard",
            tier.name(),
            record["filter"]
        ));
    }
    match (case.status(), outcome) {
        (Status::Active, "pass") | (_, "shard") => {
            let inputs = record["inputs"].as_array().map_or(0, Vec::len);
            let repeats = case.repeats() as usize;
            let speed = tier.policy().speed;
            let expected = match speed {
                None => inputs * repeats,
                Some(speed) => {
                    let blocks = record["speed"].as_array().cloned().unwrap_or_default();
                    let settings = match case.rivals() {
                        [] => None,
                        _ => match rival::settings(root) {
                            Ok(settings) => Some(settings),
                            Err(e) => {
                                findings.push(format!("{}: {id}: {e}", tier.name()));
                                None
                            }
                        },
                    };
                    let [p] = speed.threads;
                    let timed = settings.as_ref().is_none_or(|s| s.times(p.get()));
                    let due: usize = (record["inputs"].as_array().into_iter().flatten())
                        .filter_map(Value::as_str)
                        .map(|name| {
                            let path = rival::base_path(id, name);
                            let theirs = (case.rivals().iter())
                                .filter(|&&code| {
                                    !settings
                                        .as_ref()
                                        .is_some_and(|s| s.skips(code, &path).is_some())
                                })
                                .count();
                            match timed {
                                true => 1 + theirs,
                                false => 1,
                            }
                        })
                        .sum();
                    if blocks.len() != due {
                        findings.push(format!(
                                "{}: {id} record times {} blocks, but its inputs, codes, and thread counts call for {due}",
                                tier.name(),
                                blocks.len()
                            ));
                    }
                    let mut sum = 0;
                    for block in &blocks {
                        let block: speed::BlockRecord = match serde_json::from_value(block.clone())
                        {
                            Ok(block) => block,
                            Err(e) => {
                                findings.push(format!(
                                    "{}: {id} record holds a speed block that does not parse: {e}",
                                    tier.name()
                                ));
                                continue;
                            }
                        };
                        let (t, at) = (&block.t_delta_s, block.threads.get());
                        if block.code != speed::Code::Orbitar
                            && settings.as_ref().is_some_and(|s| !s.times(at))
                        {
                            findings.push(format!(
                                    "{}: {id} record holds a {} block at {at} threads, which {} does not time",
                                    tier.name(),
                                    block.code,
                                    rival::SETTINGS
                                ));
                        }
                        let lost = block.losses.len();
                        let first = &t[..repeats.min(t.len())];
                        let want = speed::runs_for(first, &speed, repeats);
                        if t.len() + lost != want {
                            findings.push(format!(
                                    "{}: {id} times {} runs of {at}; the CV of its first {repeats} asks for {want}",
                                    tier.name(),
                                    t.len() + lost,
                                ));
                        }
                        if case.timed() && (block.noisy || speed::noisy(t, lost, repeats)) {
                            findings.push(format!(
                                    "{}: {id} passes with T(δ) at {at} threads under {repeats} runs, judged or lost, under two judged times, or with no finite CV; the judge refuses the block",
                                    tier.name(),
                                ));
                        }
                        if block.delta_ha != speed::delta_ha(block.atoms) {
                            findings.push(format!(
                                "{}: {id} passes with δ {} at {at} threads, not δ · {} atoms",
                                tier.name(),
                                block.delta_ha,
                                block.atoms
                            ));
                        }
                        let holds = block.e_ref_shift_ha.map_or(block.rival.is_some(), |shift| {
                            speed::e_ref_holds(shift, block.delta_ha)
                        });
                        if !holds {
                            findings.push(format!(
                                    "{}: {id} passes with an E_ref at {at} threads whose shift {:?} is not below δ / 100",
                                    tier.name(),
                                    block.e_ref_shift_ha
                                ));
                        }
                        if block.steps.len() != block.wall_s.len() {
                            findings.push(format!(
                                "{}: {id} record holds {} step counts for the {} timed runs of {} at {at} threads",
                                tier.name(),
                                block.steps.len(),
                                block.wall_s.len(),
                                block.code
                            ));
                        }
                        let quiet = block.load.iter().filter(|r| r.quiet()).count();
                        if quiet != 1 + t.len() + lost {
                            findings.push(format!(
                                "{}: {id} record shows a quiet node before {quiet} runs of {} at {at} threads, not before its warm-up and each of its {} runs",
                                tier.name(),
                                block.code,
                                t.len() + lost
                            ));
                        }
                        let scored = usize::from(block.warmup.get("loss").is_none()) + t.len();
                        let alone = block.during.iter().filter(|d| d.quiet()).count();
                        if block.during.len() != 1 + t.len() + lost || alone != scored {
                            findings.push(format!(
                                "{}: {id} record shows {alone} runs of {} at {at} threads with no other load through them, not its {scored} warm-up and judged runs",
                                tier.name(),
                                block.code,
                            ));
                        }
                        if block.code == speed::Code::Orbitar {
                            sum += t.len();
                        }
                    }
                    if !case.rivals().is_empty()
                        && record["held_out"].as_array().is_none_or(Vec::is_empty)
                    {
                        findings.push(format!(
                                "{}: {id} passes beside rivals with no held-out variant (ORBITAR_HELDOUT)",
                                tier.name()
                            ));
                    }
                    let milestone = case.milestone();
                    for code in case
                        .rivals()
                        .iter()
                        .map(|&r| speed::Code::Rival(r).to_string())
                    {
                        let versus = (record["versus"].as_array().into_iter().flatten())
                            .find(|v| v["code"].as_str() == Some(code.as_str()));
                        let r = versus.and_then(|v| speed::read_finite(&v["r"]));
                        if !r.is_some_and(speed::r_holds) {
                            findings.push(format!(
                                    "{}: {id} passes, but its R against {code} is {r:?}, not at least {}",
                                    tier.name(),
                                    speed::R_MIN
                                ));
                        }
                        let low = versus.and_then(|v| speed::read_finite(&v["g_ci"][0]));
                        let g = versus.and_then(|v| speed::read_finite(&v["g"]));
                        let point = g.unwrap_or(f64::NAN);
                        if !low.is_some_and(|low| milestone.g_holds(point, low)) {
                            findings.push(match milestone {
                                speed::Milestone::M3 => format!(
                                    "{}: {id} passes, but the 95% CI lower bound of its G against {code} is {low:?}, not at least {}",
                                    tier.name(),
                                    speed::G_MIN
                                ),
                                speed::Milestone::M4 => format!(
                                    "{}: {id} passes, but its G against {code} is {g:?} with a 95% CI lower bound {low:?}: M4 asks G ≥ {} and a bound > {}",
                                    tier.name(),
                                    speed::G_M4,
                                    speed::G_MIN
                                ),
                            });
                        }
                    }
                    sum
                }
            };
            let rss = times("peak_rss_bytes").len();
            if rss != runs.len() {
                findings.push(format!(
                    "{}: {id} record has {rss} peak RSS values for {} timed runs",
                    tier.name(),
                    runs.len()
                ));
            }
            if inputs == 0 || runs.len() != expected {
                findings.push(format!(
                    "{}: {id} record shows {} timed runs for {inputs} inputs × {repeats} repeats",
                    tier.name(),
                    runs.len(),
                ));
            }
            let held = record["held_out"].as_array().map_or(0, Vec::len);
            if rotations.len() != inputs.saturating_sub(held)
                || !record["rotation_max"].is_number()
                || !record["rotation_used"].is_number()
            {
                findings.push(format!(
                    "{}: {id} record has no rotated rerun for each input",
                    tier.name()
                ));
            }
        }
        (Status::Planned, "planned") => {}
        (Status::Planned, "promote") => findings.push(format!(
            "{}: {id} is planned but passes; set status = \"active\" and lock it",
            tier.name()
        )),
        (_, other) => findings.push(format!(
            "{}: {id} {other}: {}",
            tier.name(),
            record["failures"]
        )),
    }
    if record["schema"].as_u64() != Some(SCHEMA) || record["case"].as_str() != Some(id) {
        findings.push(format!(
            "{}: {id} record has the wrong schema or case",
            tier.name()
        ));
    }
    findings
}

fn unreviewed(root: &Path, id: &str) -> Option<String> {
    let name = if id == lock::HARNESS {
        "the harness"
    } else {
        id
    };
    match lock::state(root, id) {
        Ok(lock::State::Reviewed) => None,
        Ok(lock::State::Stale) => Some(format!("{name} changed since its LOCK review")),
        Ok(lock::State::Unlocked) => Some(format!("{name} is not in {}", lock::PATH)),
        Ok(lock::State::Broken(e)) => Some(format!("{name}: {e}")),
        Err(e) => (id == lock::HARNESS).then_some(e),
    }
}

pub fn union(root: &Path, id: &str, dir: &Path) -> Findings {
    let case = match Case::load(root, id) {
        Ok(case) => case,
        Err(e) => return vec![e],
    };
    let tier = case.tier();
    let mut findings: Findings = [lock::HARNESS, id]
        .into_iter()
        .filter_map(|block| unreviewed(root, block))
        .map(|why| format!("shards: {why}"))
        .collect();
    let all = match super::inputs(root, &root.join("benchmarks").join(id), None) {
        Ok((all, _)) => all.into_iter().map(|(name, _)| name).collect::<Vec<_>>(),
        Err(e) => return vec![format!("shards: {id}: {e}")],
    };
    let mut files: Vec<_> = (fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    let digests = [
        ("case", lock::digest(root, id)),
        ("harness", lock::digest(root, lock::HARNESS)),
    ];
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut judged: BTreeSet<String> = BTreeSet::new();
    let mut runs: BTreeMap<&str, BTreeMap<String, Vec<String>>> = BTreeMap::new();
    for path in &files {
        let at = path.display().to_string();
        let record: Value = match load(path) {
            Ok(record) => record,
            Err(e) => {
                findings.push(format!("shards: {at}: {e}"));
                continue;
            }
        };
        if record["case"].as_str() != Some(id) {
            continue;
        }
        let outcome = record["outcome"].as_str().unwrap_or("unreadable");
        if !["pass", "shard"].contains(&outcome) {
            findings.push(format!(
                "shards: {at}: outcome {outcome}, not pass or shard; its inputs count for nothing"
            ));
            continue;
        }
        for key in ["commit", "engine_sha256"] {
            let value = record[key].as_str().unwrap_or_default();
            if value.is_empty() || value == "unknown" || value.starts_with("unreadable") {
                findings.push(format!("shards: {at}: no known {key} ({})", record[key]));
            }
        }
        let filter: Option<Vec<String>> = record["filter"]
            .as_array()
            .map(|f| f.iter().map(text).collect());
        let names: Vec<&str> = all.iter().map(String::as_str).collect();
        let due = match &filter {
            None => Ok(all.clone()),
            Some(filter) => super::selected(&names, filter).map_err(|e| format!("its filter {e}")),
        };
        let ran: Vec<String> = (record["inputs"].as_array().into_iter().flatten())
            .map(text)
            .collect();
        match due {
            Ok(due) if due == ran => {}
            Ok(due) => findings.push(format!(
                "shards: {at}: its filter {} picks {due:?}, but it ran {ran:?}",
                record["filter"]
            )),
            Err(e) => findings.push(format!("shards: {at}: {e}")),
        }
        let status = format!("{:?}", case.status()).to_lowercase();
        for (key, want) in [("status", status.as_str()), ("case_tier", tier.name())] {
            if record[key].as_str() != Some(want) {
                findings.push(format!(
                    "shards: {at}: {key} {}, but the case is {want}",
                    record[key]
                ));
            }
        }
        if record["failures"].as_array().is_none_or(|f| !f.is_empty()) {
            findings.push(format!("shards: {at}: it failed: {}", record["failures"]));
        }
        if record["seed"]
            .as_str()
            .is_none_or(|s| s.parse::<u64>().is_err())
        {
            findings.push(format!(
                "shards: {at}: seed {} is no drawn u64",
                record["seed"]
            ));
        }
        if !record["rotation_used"]
            .as_f64()
            .is_some_and(super::engine::rigid)
        {
            findings.push(format!(
                "shards: {at}: rotation_used {} of `{}`'s bound is past 1",
                record["rotation_used"], record["rotation_field"]
            ));
        }
        for key in ["runs_s", "rotation_s", "peak_rss_bytes"] {
            let real = (record[key].as_array()).is_some_and(|v| {
                v.iter()
                    .all(|x| x.as_f64().is_some_and(|x| x.is_finite() && x > 0.0))
            });
            if !real {
                findings.push(format!(
                    "shards: {at}: {key} holds a value that is not positive and finite"
                ));
            }
        }
        let metrics: Metrics = (record["metrics"].as_object().into_iter().flatten())
            .filter_map(|(k, v)| Some((k.clone(), v.as_f64()?)))
            .collect();
        let bounds = measured(case.tolerance(), &metrics);
        findings.extend(
            judge(&bounds, &metrics)
                .into_iter()
                .map(|f| format!("shards: {at}: by [tolerance]: {f}")),
        );
        judged.extend(bounds.into_keys());
        findings.extend(
            proves(root, tier, id, &case, &record)
                .into_iter()
                .map(|f| format!("shards: {at}: {f}")),
        );
        if record["tier"].as_str() != Some(tier.name()) {
            findings.push(format!(
                "shards: {at} ran tier {}, not {}",
                record["tier"],
                tier.name()
            ));
        }
        if record["dirty"].as_bool() != Some(false) {
            findings.push(format!(
                "shards: {at} ran a dirty tree; a shard names one commit"
            ));
        }
        for (block, digest) in &digests {
            if record["lock"][block].as_str() != Some(digest.as_str()) {
                findings.push(format!(
                    "shards: {at} ran other {block} files than this tree's reviewed LOCK hashes"
                ));
            }
        }
        for (key, value) in [
            ("commit", &record["commit"]),
            ("engine", &record["engine_sha256"]),
            ("CPU model", &record["node"]["host"]["cpu"]),
        ] {
            (runs.entry(key).or_default().entry(text(value)).or_default()).push(at.clone());
        }
        for name in ran {
            if !all.contains(&name) {
                findings.push(format!(
                    "shards: {at} ran {name}, which the case does not hold"
                ));
            } else if let Some(first) = seen.insert(name.clone(), at.clone()) {
                findings.push(format!("shards: {name} runs in both {first} and {at}"));
            }
        }
    }
    for (key, values) in runs.iter().filter(|(key, _)| **key != "CPU model") {
        if values.len() > 1 {
            let each: Vec<String> = (values.iter())
                .map(|(v, files)| format!("{v} ({})", files.join(", ")))
                .collect();
            findings.push(format!(
                "shards: the records ran more than one {key}: {}",
                each.join("; ")
            ));
        }
    }
    let unjudged: Vec<String> = (case.tolerance().keys())
        .filter(|key| !judged.contains(*key))
        .map(|key| format!("[tolerance].{key}"))
        .collect();
    if !unjudged.is_empty() {
        findings.push(format!(
            "shards: {id}: no record judges {}",
            unjudged.join(", ")
        ));
    }
    let missing: Vec<&str> = (all.iter())
        .filter(|n| !seen.contains_key(*n))
        .map(String::as_str)
        .collect();
    if !missing.is_empty() {
        findings.push(format!(
            "shards: {id}: no record runs {} of its {} inputs: {}",
            missing.len(),
            all.len(),
            missing.join(", ")
        ));
    }
    if findings.is_empty() {
        let all_of = |key: &str| {
            (runs.get(key).into_iter().flat_map(|v| v.keys().cloned())).collect::<Vec<_>>()
        };
        let head = crate::git(root, &["rev-parse", "HEAD"]).unwrap_or_else(|e| e.to_string());
        println!(
            "shards: {id}: the records cover its {} inputs once each at commit {} (this tree's HEAD: {head}) on {}",
            all.len(),
            all_of("commit").join(", "),
            all_of("CPU model").join("; ")
        );
        if case.status() == Status::Planned {
            findings.push(format!(
                "shards: {id} is planned but its shards pass; set status = \"active\" and lock it"
            ));
        }
    }
    findings
}

fn load(path: &Path) -> Result<Value, String> {
    fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|t| serde_json::from_str(&t).map_err(|e| e.to_string()))
}

fn text(v: &Value) -> String {
    v.as_str().map_or_else(|| v.to_string(), str::to_owned)
}

pub fn readout(dir: &Path, id: &str) -> Findings {
    let path = dir.join(format!("{id}.json"));
    let at = path.display();
    let record = match load(&path) {
        Ok(record) => record,
        Err(e) => return vec![format!("readout: {at}: {e}")],
    };
    if record["case"].as_str() != Some(id) {
        return vec![format!("readout: {at}: case {}, not {id}", record["case"])];
    }
    if record["schema"].as_u64() != Some(SCHEMA) {
        return vec![format!(
            "readout: {at}: schema {}, not {SCHEMA}: another harness wrote it",
            record["schema"]
        )];
    }
    let blocks: Vec<speed::BlockRecord> = match serde_json::from_value(record["speed"].clone()) {
        Ok(blocks) => blocks,
        Err(e) => return vec![format!("readout: {at}: speed: {e}")],
    };
    if let Some(b) = blocks
        .iter()
        .find(|b| !b.t_delta_s.is_empty() && b.timing.is_none())
    {
        return vec![format!(
            "readout: {at}: block {} {} p={} holds {} runs but no timing",
            b.code,
            b.input,
            b.threads,
            b.t_delta_s.len()
        )];
    }
    let field = |key: &str| text(&record[key]);
    println!("readout: {at}");
    println!(
        "tier {} schema {} outcome {} commit {} dirty {} engine {} seed {} seed_fixed {}",
        field("tier"),
        field("schema"),
        field("outcome"),
        field("commit"),
        field("dirty"),
        field("engine_sha256"),
        field("seed"),
        field("seed_fixed")
    );
    println!(
        "held_out {} tuned {} pilot {}",
        field("held_out"),
        field("tuned"),
        field("pilot")
    );
    println!("skipped {} spent_s {}", field("skipped"), field("spent_s"));
    for failure in record["failures"].as_array().into_iter().flatten() {
        println!("failure: {}", text(failure));
    }
    for v in record["versus"].as_array().into_iter().flatten() {
        println!(
            "versus {}: R {} (judged, R_min {}) G {} 95% CI {} by {} resamples, seed {} (g_judged {}, G_min {}, \
             milestone {}, G point min {})",
            text(&v["code"]),
            text(&v["r"]),
            text(&v["r_min"]),
            text(&v["g"]),
            text(&v["g_ci"]),
            text(&v["g_resamples"]),
            text(&v["g_seed"]),
            text(&v["g_judged"]),
            text(&v["g_min"]),
            text(&v["milestone"]),
            text(&v["g_point_min"])
        );
        for p in v["pairs"].as_array().into_iter().flatten() {
            println!(
                "  pair {} p={}: t_ratio {} pss_ratio {} in_r {} medians with losses as +inf (rival, orbitar): \
                 T(δ) {} s, Pss {} bytes; rival losses {}",
                text(&p["input"]),
                text(&p["threads"]),
                text(&p["t_ratio"]),
                text(&p["pss_ratio"]),
                text(&p["in_r"]),
                text(&p["medians_t_delta_s"]),
                text(&p["medians_peak_pss_bytes"]),
                text(&p["losses"])
            );
        }
    }
    for b in &blocks {
        let median = |key: fn(&speed::Timing) -> f64| {
            (b.timing.as_ref()).map_or_else(|| "none".to_owned(), |t| key(t).to_string())
        };
        println!(
            "block {} {} p={}: timed runs {} losses {}; medians of the runs alone: T(δ) {} s, wall {} s, peak Pss {} \
             bytes; noisy {} E_ref {} accuracy {}; SCF steps {:?}, E_ref's {:?}, cap {:?}; a quiet node before {} of {} \
             runs, the warm-up first: {:?}; no other load through {} of them: {:?}",
            b.code,
            b.input,
            b.threads,
            b.wall_s.len(),
            b.losses.len(),
            median(|t| t.median_t_delta_s),
            if b.wall_s.is_empty() {
                "none".to_owned()
            } else {
                speed::median(&b.wall_s).to_string()
            },
            median(|t| t.median_peak_pss_bytes),
            b.noisy,
            b.e_ref_ha
                .map_or_else(|| "none".to_owned(), |e| e.to_string()),
            b.accuracy
                .as_ref()
                .map_or_else(|| "none".to_owned(), Value::to_string),
            b.steps,
            b.e_ref_steps,
            b.step_cap,
            b.load.iter().filter(|r| r.quiet()).count(),
            b.load.len(),
            b.load,
            b.during.iter().filter(|d| d.quiet()).count(),
            b.during
        );
    }
    let mut lost: Vec<&str> = (blocks.iter())
        .filter(|b| b.code != speed::Code::Orbitar)
        .flat_map(|b| b.losses.iter().map(String::as_str))
        .collect();
    lost.sort_unstable();
    lost.dedup();
    if !lost.is_empty() {
        println!(
            "rival losses, each counted as +inf in R and G: {}",
            lost.join(" | ")
        );
        println!(
            "read each rival block's E_ref (none: its reference run was lost, \"E_ref lost\" in the tier log) before its \
             losses count as a win for orbitaR"
        );
    }
    println!("metrics {}", record["metrics"]);
    println!(
        "record outcome {}; the PASS below says only that the record is readable",
        field("outcome")
    );
    Vec::new()
}
