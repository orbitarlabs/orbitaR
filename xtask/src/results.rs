mod card;

use crate::{
    Findings,
    case::{self, Bound, Case, Status, Tier},
    git,
    harness::speed::read_finite,
    walk,
};
use serde::Deserialize;
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fmt::Write as _,
    fs,
    path::{Path, PathBuf},
};

pub(crate) const RESULTS: &str = "benchmarks/RESULTS.toml";
pub(crate) const WHAT: &str = "benchmarks/results-what.toml";
pub(crate) const PAGE: &str = "benchmarks/README.md";
pub(crate) const LIGHT: &str = "assets/benchmarks/results-light.svg";
pub(crate) const DARK: &str = "assets/benchmarks/results-dark.svg";
const SCHEMA: u32 = 1;
const MAX_WHAT: usize = 40;
const MAX_UNIT: usize = 16;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Row {
    number: u32,
    id: String,
    what: String,
    unit: String,
    exact: Option<String>,
    tier: Tier,
    run: Option<Tier>,
    outcome: Outcome,
    commit: Option<String>,
    engine_sha256: Option<String>,
    #[serde(default, rename = "metric")]
    metrics: Vec<Metric>,
    #[serde(default)]
    versus: Vec<Versus>,
    #[serde(default)]
    series: Vec<Series>,
    node: Option<toml::Table>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Versus {
    rival: String,
    g: f64,
    g_ci: [f64; 2],
    r: f64,
    #[serde(rename = "pair")]
    pairs: Vec<Pair>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Pair {
    input: String,
    threads: u32,
    t_delta_s_rival: f64,
    t_delta_s_orbitar: f64,
    peak_pss_bytes_rival: f64,
    peak_pss_bytes_orbitar: f64,
    in_r: bool,
}

impl Versus {
    fn in_r(&self) -> impl Iterator<Item = &Pair> {
        self.pairs.iter().filter(|p| p.in_r)
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Series {
    code: String,
    t_delta_exponent: f64,
    pss_exponent: f64,
    #[serde(rename = "point")]
    points: Vec<Point>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Point {
    input: String,
    atoms: u32,
    t_delta_s: f64,
    peak_pss_bytes: f64,
}

const ORBITAR: &str = "orbitar";

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
enum Outcome {
    #[serde(rename = "pass")]
    Pass,
    #[serde(rename = "fail")]
    Fail,
    #[serde(rename = "no record")]
    NoRecord,
}

impl Outcome {
    fn name(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Fail => "fail",
            Outcome::NoRecord => "no record",
        }
    }
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Metric {
    name: String,
    bound: Side,
    limit: f64,
    measured: f64,
    ratio: f64,
    #[serde(default)]
    headline: bool,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Side {
    Max,
    Min,
}

impl Side {
    fn of(bound: Bound) -> Side {
        match bound {
            Bound::Max => Side::Max,
            Bound::Min => Side::Min,
        }
    }

    fn bound(self) -> Bound {
        match self {
            Side::Max => Bound::Max,
            Side::Min => Bound::Min,
        }
    }

    fn sign(self) -> &'static str {
        match self {
            Side::Max => "≤",
            Side::Min => "≥",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    schema: u32,
    #[serde(default, rename = "case")]
    cases: Vec<Row>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct What {
    what: String,
    metric: Vec<String>,
    unit: String,
    exact: Option<String>,
}

pub(crate) fn ratio(bound: Bound, limit: f64, measured: f64) -> f64 {
    let (met, quotient) = match bound {
        Bound::Max => (measured <= limit, (limit > 0.0).then(|| measured / limit)),
        Bound::Min => (
            measured >= limit,
            (limit > 0.0 && measured > 0.0).then(|| limit / measured),
        ),
    };
    match (met, quotient) {
        (_, Some(q)) if q >= 0.0 => q,
        (true, _) => 0.0,
        (false, _) => f64::INFINITY,
    }
}

pub(crate) fn collect(root: &Path, tiers: &[String], records: Option<&Path>) -> Findings {
    let base = records.map_or_else(|| root.join("target/benchmarks"), Path::to_path_buf);
    let mut findings = Vec::new();
    let mut chosen = Vec::new();
    for name in tiers {
        match Tier::parse(name) {
            Some(tier) => chosen.push(tier),
            None => findings.push(format!("results: unknown tier {name}")),
        }
    }
    if tiers.is_empty() {
        chosen = (Tier::ALL.into_iter())
            .filter(|t| base.join(t.name()).is_dir())
            .collect();
        if chosen.is_empty() {
            findings.push(format!("results: no tier directory in {}", base.display()));
        }
    }
    chosen.sort();
    chosen.dedup();
    let cases = match active(root) {
        Ok(cases) => cases,
        Err(e) => return vec![e],
    };
    let whats = match whats(root, &cases) {
        Ok(whats) => whats,
        Err(e) => return e,
    };
    let mut rows: BTreeMap<String, Row> = match load(root) {
        Ok(Some(rows)) => rows.into_iter().map(|r| (r.id.clone(), r)).collect(),
        Ok(None) => BTreeMap::new(),
        Err(e) => return vec![e],
    };
    for tier in chosen {
        let dir = base.join(tier.name());
        let files = jsons(&dir);
        if files.is_empty() {
            findings.push(format!("results: no record in {}", dir.display()));
        }
        for path in files {
            match row(root, tier, &path, &cases, &whats) {
                Ok(Some(row)) => {
                    rows.insert(row.id.clone(), row);
                }
                Ok(None) => {}
                Err(e) => findings.push(format!("results: {}: {e}", path.display())),
            }
        }
    }
    let mut kept = Vec::new();
    for (number, (id, case)) in &cases {
        let what = &whats[number];
        let mut row = rows.remove(id).unwrap_or_else(|| Row {
            number: *number,
            id: id.clone(),
            what: String::new(),
            unit: String::new(),
            exact: None,
            tier: case.tier(),
            run: None,
            outcome: Outcome::NoRecord,
            commit: None,
            engine_sha256: None,
            metrics: Vec::new(),
            versus: Vec::new(),
            series: Vec::new(),
            node: None,
        });
        if row.outcome != Outcome::NoRecord
            && let Err(e) = headline(&mut row.metrics, &what.metric)
        {
            findings.push(format!("results: {RESULTS}: {id}: {e}"));
        }
        row.what.clone_from(&what.what);
        row.unit.clone_from(&what.unit);
        row.exact.clone_from(&what.exact);
        row.tier = case.tier();
        if let Err(e) = consistent(&row) {
            findings.push(format!("results: {RESULTS}: {id}: {e}"));
        }
        kept.push(row);
    }
    if !findings.is_empty() {
        return findings;
    }
    if let Err(e) = fs::write(root.join(RESULTS), write(&kept)) {
        return vec![format!("results: {RESULTS}: {e}")];
    }
    for row in &kept {
        let commit = row.commit.as_deref().map_or("none", |c| &c[..7]);
        println!("results: {:<24} {:<9} {commit}", row.id, row.outcome.name());
    }
    Vec::new()
}

fn active(root: &Path) -> Result<BTreeMap<u32, (String, Case)>, String> {
    let mut out = BTreeMap::new();
    for id in case::ids(root).map_err(|e| format!("results: {e}"))? {
        let case = Case::load(root, &id).map_err(|e| format!("results: {e}"))?;
        if case.status() == Status::Active {
            out.insert(case::parse_id(&id)?, (id, case));
        }
    }
    Ok(out)
}

fn jsons(dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = (fs::read_dir(dir).into_iter().flatten().flatten())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json"))
        .collect();
    files.sort();
    files
}

fn row(
    root: &Path,
    tier: Tier,
    path: &Path,
    cases: &BTreeMap<u32, (String, Case)>,
    whats: &BTreeMap<u32, What>,
) -> Result<Option<Row>, String> {
    let text = fs::read_to_string(path).map_err(|e| e.to_string())?;
    let record: Value = serde_json::from_str(&text).map_err(|e| format!("not JSON: {e}"))?;
    let id = record["case"].as_str().ok_or("no case")?;
    let number = case::parse_id(id)?;
    let Some((_, case)) = cases.get(&number).filter(|(known, _)| known == id) else {
        return Ok(None);
    };
    if record["dirty"] != Value::Bool(false) {
        return Err(format!(
            "{id} ran on a dirty tree; only a record of a commit stands behind a number"
        ));
    }
    if record["pilot"] == Value::Bool(true) {
        return Err(format!(
            "{id} is a pilot record; a pilot prices a run and proves nothing"
        ));
    }
    if record["tier"].as_str() != Some(tier.name()) {
        return Err(format!(
            "{id} names tier {}, not its directory's tier {}",
            record["tier"],
            tier.name()
        ));
    }
    let hex = |s: &str, n: usize| s.len() == n && s.bytes().all(|b| b.is_ascii_hexdigit());
    let commit = record["commit"].as_str().unwrap_or("");
    if !hex(commit, 40) {
        return Err(format!("{id} names no full commit hash"));
    }
    if git(root, &["merge-base", "--is-ancestor", commit, "HEAD"]).is_err() {
        return Err(format!(
            "{id} ran at {commit}, which is no ancestor of HEAD; collect this branch's records"
        ));
    }
    let engine = record["engine_sha256"].as_str().unwrap_or("");
    if !hex(engine, 64) {
        return Err(format!("{id} names no engine sha256"));
    }
    let outcome = match record["outcome"].as_str() {
        Some("pass") => Outcome::Pass,
        Some("fail") => Outcome::Fail,
        other => {
            return Err(format!(
                "{id} has outcome {other:?}; a row takes a full run's pass or fail"
            ));
        }
    };
    let tolerance: BTreeMap<String, f64> = serde_json::from_value(record["tolerance"].clone())
        .map_err(|e| format!("{id}: tolerance: {e}"))?;
    let mut metrics = Vec::new();
    for (key, limit) in tolerance {
        let (name, bound) = case::bounded(&key).ok_or(format!("{id}: [tolerance].{key}"))?;
        let measured = record["metrics"][name].as_f64().unwrap_or(f64::NAN);
        metrics.push(Metric {
            name: name.to_owned(),
            bound: Side::of(bound),
            limit,
            measured,
            ratio: ratio(bound, limit, measured),
            headline: false,
        });
    }
    headline(&mut metrics, &whats[&number].metric).map_err(|e| format!("{id}: {e}"))?;
    let (versus, series, node) = match (tier, outcome) {
        (Tier::Claim, Outcome::Pass) => rivals(case, &record).map_err(|e| format!("{id}: {e}"))?,
        _ => (Vec::new(), Vec::new(), None),
    };
    Ok(Some(Row {
        number,
        id: id.to_owned(),
        what: String::new(),
        unit: String::new(),
        exact: None,
        tier: case.tier(),
        run: Some(tier),
        outcome,
        commit: Some(commit.to_owned()),
        engine_sha256: Some(engine.to_owned()),
        metrics,
        versus,
        series,
        node,
    }))
}

type Claimed = (Vec<Versus>, Vec<Series>, Option<toml::Table>);

fn rivals(case: &Case, record: &Value) -> Result<Claimed, String> {
    let named: Vec<&str> = case.rivals().iter().map(|r| r.adapter().name()).collect();
    let entries = record["versus"].as_array().map_or(&[][..], Vec::as_slice);
    let mut versus = Vec::new();
    for v in entries {
        let rival = v["code"].as_str().ok_or("a versus entry names no code")?;
        if !named.contains(&rival) || versus.iter().any(|w: &Versus| w.rival == rival) {
            return Err(format!(
                "versus {rival}: the case names rivals {named:?}, each once"
            ));
        }
        let number = |field: &Value, name: &str| {
            read_finite(field).ok_or(format!("versus {rival}: no number {name}"))
        };
        let mut pairs = Vec::new();
        for p in v["pairs"].as_array().into_iter().flatten() {
            let medians = |key: &str| -> Result<[f64; 2], String> {
                Ok([number(&p[key][0], key)?, number(&p[key][1], key)?])
            };
            let [t_delta_s_rival, t_delta_s_orbitar] = medians("medians_t_delta_s")?;
            let [peak_pss_bytes_rival, peak_pss_bytes_orbitar] = medians("medians_peak_pss_bytes")?;
            pairs.push(Pair {
                input: (p["input"].as_str())
                    .ok_or(format!("versus {rival}: a pair names no input"))?
                    .to_owned(),
                threads: (p["threads"].as_u64())
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(format!("versus {rival}: a pair names no thread count"))?,
                t_delta_s_rival,
                t_delta_s_orbitar,
                peak_pss_bytes_rival,
                peak_pss_bytes_orbitar,
                in_r: (p["in_r"].as_bool()).ok_or(format!("versus {rival}: a pair has no in_r"))?,
            });
        }
        versus.push(Versus {
            rival: rival.to_owned(),
            g: number(&v["g"], "g")?,
            g_ci: [
                number(&v["g_ci"][0], "g_ci")?,
                number(&v["g_ci"][1], "g_ci")?,
            ],
            r: number(&v["r"], "r")?,
            pairs,
        });
    }
    if let Some(missing) = (named.iter()).find(|n| !versus.iter().any(|v| v.rival == **n)) {
        return Err(format!(
            "a passing claim record holds no versus entry for {missing}, which the case names"
        ));
    }
    let mut series = Vec::new();
    for s in record["series"].as_array().into_iter().flatten() {
        let code = s["code"].as_str().ok_or("a series entry names no code")?;
        let number = |field: &Value, name: &str| {
            read_finite(field).ok_or(format!("series {code}: no number {name}"))
        };
        let mut points = Vec::new();
        for p in s["points"].as_array().into_iter().flatten() {
            points.push(Point {
                input: (p["input"].as_str())
                    .ok_or(format!("series {code}: a point names no input"))?
                    .to_owned(),
                atoms: (p["atoms"].as_u64())
                    .and_then(|n| u32::try_from(n).ok())
                    .ok_or(format!("series {code}: a point names no atom count"))?,
                t_delta_s: number(&p["median_t_delta_s"], "median_t_delta_s")?,
                peak_pss_bytes: number(&p["median_peak_pss_bytes"], "median_peak_pss_bytes")?,
            });
        }
        series.push(Series {
            code: code.to_owned(),
            t_delta_exponent: number(&s["t_delta_exponent"], "t_delta_exponent")?,
            pss_exponent: number(&s["pss_exponent"], "pss_exponent")?,
            points,
        });
    }
    let node = match versus.is_empty() && series.is_empty() {
        true => None,
        false => Some(
            serde_json::from_value::<toml::Table>(record["node"].clone())
                .map_err(|e| format!("the record's node block: {e}"))?,
        ),
    };
    for code in versus.iter().map(|v| v.rival.as_str()) {
        package(node.as_ref(), code)?;
    }
    for code in series.iter().map(|s| s.code.as_str()) {
        if code != ORBITAR && !named.contains(&code) {
            return Err(format!(
                "series {code}: the case names rivals {named:?}, and orbitar"
            ));
        }
    }
    Ok((versus, series, node))
}

fn cpu(node: &toml::Table) -> Result<&str, String> {
    (node.get("host").and_then(|h| h.get("cpu")?.as_str()))
        .ok_or_else(|| "the node block names no CPU: [host].cpu".to_owned())
}

fn package(node: Option<&toml::Table>, code: &str) -> Result<(String, String), String> {
    let text = (node.and_then(|n| n.get(code)?.get(code)?.as_str())).unwrap_or("");
    let mut words = text.split_whitespace();
    match (words.next(), words.next()) {
        (Some(version), Some(build)) => Ok((version.to_owned(), build.to_owned())),
        _ => Err(format!(
            "the node block names no version and build of {code}: [{code}].{code}"
        )),
    }
}

fn headline(metrics: &mut [Metric], names: &[String]) -> Result<(), String> {
    let mut best: Option<usize> = None;
    for name in names {
        let mut found = false;
        for (i, m) in metrics.iter().enumerate().filter(|(_, m)| &m.name == name) {
            found = true;
            if best.is_none_or(|b| metrics[b].ratio < m.ratio) {
                best = Some(i);
            }
        }
        if !found {
            return Err(format!(
                "the record bounds no metric {name}, which {WHAT} names"
            ));
        }
    }
    let best = best.ok_or(format!("{WHAT} names no headline metric"))?;
    for (i, m) in metrics.iter_mut().enumerate() {
        m.headline = i == best;
    }
    Ok(())
}

fn whats(
    root: &Path,
    cases: &BTreeMap<u32, (String, Case)>,
) -> Result<BTreeMap<u32, What>, Findings> {
    let text = walk::read(root, WHAT);
    let table: BTreeMap<String, What> =
        toml::from_str(&text).map_err(|e| vec![format!("results: {WHAT}: {e}")])?;
    let mut out = BTreeMap::new();
    let mut findings = Vec::new();
    for (key, entry) in table {
        let Some(number) = (key.parse::<u32>().ok()).filter(|n| n.to_string() == key) else {
            findings.push(format!("results: {WHAT}: [{key}] is no case number"));
            continue;
        };
        let plain = |s: &str| !s.contains(['|', '<', '>', '&', '"', '\\', '\n', '`']);
        if entry.what.trim().is_empty() || entry.what.chars().count() > MAX_WHAT {
            findings.push(format!(
                "results: {WHAT}: [{number}] what must hold 1 to {MAX_WHAT} characters"
            ));
        }
        if entry.unit.chars().count() > MAX_UNIT {
            findings.push(format!(
                "results: {WHAT}: [{number}] unit holds over {MAX_UNIT} characters"
            ));
        }
        let exact = entry.exact.as_deref().unwrap_or("");
        if entry.exact.is_some() && (exact.trim().is_empty() || exact.chars().count() > MAX_WHAT) {
            findings.push(format!(
                "results: {WHAT}: [{number}] exact must hold 1 to {MAX_WHAT} characters"
            ));
        }
        if !plain(&entry.what) || !plain(&entry.unit) || !plain(exact) {
            findings.push(format!(
                "results: {WHAT}: [{number}] what, unit and exact are plain words, without | < > & \" \\ `"
            ));
        }
        out.insert(number, entry);
    }
    for (number, (id, case)) in cases {
        let Some(entry) = out.get(number) else {
            findings.push(format!(
                "results: {WHAT} has no [{number}] for {id}: what, metric, unit"
            ));
            continue;
        };
        if entry.metric.is_empty() {
            findings.push(format!(
                "results: {WHAT}: [{number}] names no metric for {id}"
            ));
        }
        for name in &entry.metric {
            let bounded = (case.tolerance().keys())
                .any(|key| case::bounded(key).is_some_and(|(metric, _)| metric == name));
            if !bounded {
                findings.push(format!(
                    "results: {WHAT}: [{number}] metric {name} is no bound of {id}'s [tolerance]"
                ));
            }
        }
    }
    if findings.is_empty() {
        Ok(out)
    } else {
        Err(findings)
    }
}

fn load(root: &Path) -> Result<Option<Vec<Row>>, String> {
    if fs::symlink_metadata(root.join(RESULTS)).is_err() {
        return Ok(None);
    }
    let text = walk::read(root, RESULTS);
    let file: File = toml::from_str(&text).map_err(|e| format!("results: {RESULTS}: {e}"))?;
    if file.schema != SCHEMA {
        return Err(format!(
            "results: {RESULTS}: schema {} is not {SCHEMA}",
            file.schema
        ));
    }
    for pair in file.cases.windows(2) {
        if pair[0].number >= pair[1].number {
            return Err(format!(
                "results: {RESULTS}: rows go by case number, once each: {} before {}",
                pair[0].id, pair[1].id
            ));
        }
    }
    for row in &file.cases {
        consistent(row).map_err(|e| format!("results: {RESULTS}: {}: {e}", row.id))?;
    }
    Ok(Some(file.cases))
}

fn consistent(row: &Row) -> Result<(), String> {
    if case::parse_id(&row.id) != Ok(row.number) {
        return Err(format!("number {} is not the case's", row.number));
    }
    let none = row.outcome == Outcome::NoRecord;
    let recorded = [
        row.run.is_some(),
        row.commit.is_some(),
        row.engine_sha256.is_some(),
        !row.metrics.is_empty(),
    ];
    if recorded.contains(&none) {
        return Err(
            "a row has run, commit, engine and metrics exactly when it has a record".into(),
        );
    }
    if row.commit.as_ref().is_some_and(|c| c.len() != 40) {
        return Err("commit is no full hash".into());
    }
    for m in &row.metrics {
        let want = ratio(m.bound.bound(), m.limit, m.measured);
        if want.to_bits() != m.ratio.to_bits() {
            return Err(format!(
                "{} has ratio {:e}, but its numbers give {want:e}",
                m.name, m.ratio
            ));
        }
    }
    if !none && row.metrics.iter().filter(|m| m.headline).count() != 1 {
        return Err("a recorded row has one headline metric".into());
    }
    if row.outcome == Outcome::Pass && row.metrics.iter().any(|m| m.ratio > 1.0) {
        return Err("a pass with a metric past its bound".into());
    }
    if row.what.trim().is_empty() {
        return Err("no what phrase".into());
    }
    let claimed = !row.versus.is_empty() || !row.series.is_empty();
    if claimed != row.node.is_some() {
        return Err("a row has a node block exactly when it has a versus or series entry".into());
    }
    if claimed && (row.run, row.outcome) != (Some(Tier::Claim), Outcome::Pass) {
        return Err("only a passing claim record gives versus and series entries".into());
    }
    if let Some(node) = &row.node {
        cpu(node)?;
    }
    for v in &row.versus {
        package(row.node.as_ref(), &v.rival)?;
        agrees(v)?;
    }
    let codes: BTreeSet<&str> = row.series.iter().map(|s| s.code.as_str()).collect();
    if codes.len() != row.series.len() {
        return Err("a series names each code once".into());
    }
    row.series.iter().try_for_each(fits)
}

const AGREE: f64 = 1e-9;

fn close(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a / b - 1.0).abs() <= AGREE
}

fn agrees(v: &Versus) -> Result<(), String> {
    let name = &v.rival;
    if v.pairs.is_empty() {
        return Err(format!("versus {name} has no pair"));
    }
    let medians = (v.pairs.iter()).flat_map(|p| {
        [
            p.t_delta_s_rival,
            p.t_delta_s_orbitar,
            p.peak_pss_bytes_rival,
            p.peak_pss_bytes_orbitar,
        ]
    });
    if medians.into_iter().any(|m| m.is_nan() || m <= 0.0) {
        return Err(format!("versus {name}: each median lies above 0"));
    }
    let g = geomean(v.pairs.iter().map(|p| p.t_delta_s_rival))
        / geomean(v.pairs.iter().map(|p| p.t_delta_s_orbitar));
    if !close(g, v.g) {
        return Err(format!(
            "versus {name}: G is {:e}, but its pairs give {g:e}",
            v.g
        ));
    }
    if !(v.g_ci[0] <= v.g && v.g <= v.g_ci[1]) {
        return Err(format!(
            "versus {name}: G {:e} lies outside its 95% CI [{:e}, {:e}]",
            v.g, v.g_ci[0], v.g_ci[1]
        ));
    }
    if v.in_r().next().is_none() {
        return Err(format!("versus {name}: R reads no pair"));
    }
    let r = geomean(v.in_r().map(|p| p.peak_pss_bytes_rival))
        / geomean(v.in_r().map(|p| p.peak_pss_bytes_orbitar));
    if !close(r, v.r) {
        return Err(format!(
            "versus {name}: R is {:e}, but its pairs give {r:e}",
            v.r
        ));
    }
    Ok(())
}

fn fits(s: &Series) -> Result<(), String> {
    let name = &s.code;
    let positive = |v: f64| v.is_finite() && v > 0.0;
    if (s.points.iter())
        .any(|p| p.atoms == 0 || !positive(p.t_delta_s) || !positive(p.peak_pss_bytes))
    {
        return Err(format!(
            "series {name}: each point holds atoms, T(δ) and peak Pss above 0"
        ));
    }
    if (s.points.windows(2)).any(|w| (w[0].atoms, &w[0].input) >= (w[1].atoms, &w[1].input)) {
        return Err(format!(
            "series {name}: points go by atoms, then input, once each"
        ));
    }
    for (exponent, label, y) in [
        (
            s.t_delta_exponent,
            "T(δ)",
            (|p: &Point| p.t_delta_s) as fn(&Point) -> f64,
        ),
        (s.pss_exponent, "peak Pss", |p: &Point| p.peak_pss_bytes),
    ] {
        let slope = slope(
            s.points
                .iter()
                .map(|p| (log10(f64::from(p.atoms)), log10(y(p)))),
        );
        let nan = slope.is_nan() && exponent.is_nan();
        if !(nan || close(slope, exponent) || (slope - exponent).abs() <= AGREE) {
            return Err(format!(
                "series {name}: the {label} exponent is {exponent:e}, but its points give {slope:e}"
            ));
        }
    }
    Ok(())
}

fn slope(points: impl Iterator<Item = (f64, f64)>) -> f64 {
    let points: Vec<(f64, f64)> = points.collect();
    let n = points.len() as f64;
    let mx = points.iter().map(|p| p.0).sum::<f64>() / n;
    let my = points.iter().map(|p| p.1).sum::<f64>() / n;
    let sxx: f64 = points.iter().map(|p| (p.0 - mx) * (p.0 - mx)).sum();
    let sxy: f64 = points.iter().map(|p| (p.0 - mx) * (p.1 - my)).sum();
    match sxx > 0.0 {
        true => sxy / sxx,
        false => f64::NAN,
    }
}

fn num(v: f64) -> String {
    if v.is_nan() {
        "nan".into()
    } else if v.is_infinite() {
        if v > 0.0 { "inf".into() } else { "-inf".into() }
    } else {
        format!("{v:e}")
    }
}

fn quoted(text: &str) -> String {
    format!("\"{}\"", text.replace('\\', "\\\\").replace('"', "\\\""))
}

fn write(rows: &[Row]) -> String {
    let mut out = String::new();
    out.push_str(
        "# Generated by `cargo xtask results collect` from the benchmark records; edit no line by hand.\n",
    );
    out.push_str("# Each row keeps its record's commit and engine. `cargo xtask results render` draws from it.\n");
    let _ = writeln!(out, "schema = {SCHEMA}");
    for row in rows {
        let _ = write!(
            out,
            "\n[[case]]\nnumber = {}\nid = {}\nwhat = {}\nunit = {}\ntier = \"{}\"\n",
            row.number,
            quoted(&row.id),
            quoted(&row.what),
            quoted(&row.unit),
            row.tier.name()
        );
        if let Some(exact) = &row.exact {
            let _ = writeln!(out, "exact = {}", quoted(exact));
        }
        if let Some(run) = row.run {
            let _ = writeln!(out, "run = \"{}\"", run.name());
        }
        let _ = writeln!(out, "outcome = \"{}\"", row.outcome.name());
        if let (Some(commit), Some(engine)) = (&row.commit, &row.engine_sha256) {
            let _ = writeln!(
                out,
                "commit = {}\nengine_sha256 = {}",
                quoted(commit),
                quoted(engine)
            );
        }
        for m in &row.metrics {
            let side = match m.bound {
                Side::Max => "max",
                Side::Min => "min",
            };
            let _ = write!(
                out,
                "\n[[case.metric]]\nname = {}\nbound = \"{side}\"\nlimit = {}\nmeasured = {}\nratio = {}\n",
                quoted(&m.name),
                num(m.limit),
                num(m.measured),
                num(m.ratio)
            );
            if m.headline {
                out.push_str("headline = true\n");
            }
        }
        for v in &row.versus {
            let _ = write!(
                out,
                "\n[[case.versus]]\nrival = {}\ng = {}\ng_ci = [{}, {}]\nr = {}\n",
                quoted(&v.rival),
                num(v.g),
                num(v.g_ci[0]),
                num(v.g_ci[1]),
                num(v.r)
            );
            for p in &v.pairs {
                let _ = write!(
                    out,
                    "\n[[case.versus.pair]]\ninput = {}\nthreads = {}\nt_delta_s_rival = {}\nt_delta_s_orbitar = {}\npeak_pss_bytes_rival = {}\npeak_pss_bytes_orbitar = {}\nin_r = {}\n",
                    quoted(&p.input),
                    p.threads,
                    num(p.t_delta_s_rival),
                    num(p.t_delta_s_orbitar),
                    num(p.peak_pss_bytes_rival),
                    num(p.peak_pss_bytes_orbitar),
                    p.in_r
                );
            }
        }
        for s in &row.series {
            let _ = write!(
                out,
                "\n[[case.series]]\ncode = {}\nt_delta_exponent = {}\npss_exponent = {}\n",
                quoted(&s.code),
                num(s.t_delta_exponent),
                num(s.pss_exponent)
            );
            for p in &s.points {
                let _ = write!(
                    out,
                    "\n[[case.series.point]]\ninput = {}\natoms = {}\nt_delta_s = {}\npeak_pss_bytes = {}\n",
                    quoted(&p.input),
                    p.atoms,
                    num(p.t_delta_s),
                    num(p.peak_pss_bytes)
                );
            }
        }
        if let Some(node) = &row.node {
            table(&mut out, "case.node", node);
        }
    }
    out
}

fn table(out: &mut String, path: &str, table: &toml::Table) {
    let _ = writeln!(out, "\n[{path}]");
    let key = |k: &str| match !k.is_empty()
        && (k.bytes()).all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    {
        true => k.to_owned(),
        false => quoted(k),
    };
    for (k, v) in table.iter().filter(|(_, v)| !v.is_table()) {
        let _ = writeln!(out, "{} = {}", key(k), value(v));
    }
    for (k, v) in table {
        if let toml::Value::Table(inner) = v {
            self::table(out, &format!("{path}.{}", key(k)), inner);
        }
    }
}

fn value(v: &toml::Value) -> String {
    match v {
        toml::Value::String(s) => quoted(s),
        toml::Value::Float(f) => num(*f),
        toml::Value::Array(items) => {
            format!(
                "[{}]",
                items.iter().map(value).collect::<Vec<_>>().join(", ")
            )
        }
        toml::Value::Table(inner) => format!(
            "{{ {} }}",
            (inner.iter())
                .map(|(k, v)| format!("{} = {}", quoted(k), value(v)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        other => other.to_string(),
    }
}

fn rendered(rows: &[Row]) -> Vec<(&'static str, Option<String>)> {
    [
        (PAGE, Some(page(rows))),
        (LIGHT, Some(chart(rows, &LIGHT_THEME))),
        (DARK, Some(chart(rows, &DARK_THEME))),
    ]
    .into_iter()
    .chain(card::cards(rows))
    .collect()
}

pub(crate) fn render(root: &Path) -> Findings {
    let rows = match load(root) {
        Ok(Some(rows)) => rows,
        Ok(None) => {
            return vec![format!(
                "results: {RESULTS} is missing; run `cargo xtask results collect`"
            )];
        }
        Err(e) => return vec![e],
    };
    let mut findings = Vec::new();
    for (path, body) in rendered(&rows) {
        let path = root.join(path);
        let done = match body {
            Some(body) => (path.parent().map_or(Ok(()), fs::create_dir_all))
                .and_then(|()| fs::write(&path, body)),
            None if fs::symlink_metadata(&path).is_ok() => fs::remove_file(&path),
            None => Ok(()),
        };
        if let Err(e) = done {
            findings.push(format!("results: {}: {e}", path.display()));
        }
    }
    findings
}

pub(crate) fn check(root: &Path) -> Findings {
    let rows = match load(root) {
        Ok(rows) => rows,
        Err(e) => return vec![format!("science: S11 {e}")],
    };
    let exists = |p: &str| fs::symlink_metadata(root.join(p)).is_ok();
    let Some(rows) = rows else {
        return ([PAGE, LIGHT, DARK].into_iter().chain(card::CARDS))
            .filter(|p| exists(p))
            .map(|p| {
                format!("science: S11 {p} exists without {RESULTS}; render it from the records")
            })
            .collect();
    };
    let mut findings = Vec::new();
    for (path, want) in rendered(&rows) {
        let Some(want) = want else {
            if exists(path) {
                findings.push(format!(
                    "science: S11 {path} exists, but no row of {RESULTS} backs it; `cargo xtask results render` removes it"
                ));
            }
            continue;
        };
        let file = fs::symlink_metadata(root.join(path)).is_ok_and(|m| m.is_file());
        if !file || walk::read(root, path) != want {
            findings.push(format!(
                "science: S11 {path} differs from what `cargo xtask results render` makes of {RESULTS}"
            ));
        }
    }
    findings
}

fn headline_of(row: &Row) -> Option<&Metric> {
    row.metrics.iter().find(|m| m.headline)
}

fn short(v: f64) -> String {
    if v == 0.0 || !v.is_finite() {
        return if v == 0.0 { "0".into() } else { num(v) };
    }
    let text = format!("{v:.1e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let exponent: i32 = exponent.parse().unwrap_or(0);
    if !(-3..=2).contains(&exponent) {
        return text;
    }
    let (sign, digits) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |rest| ("-", rest));
    let digits = digits.replace('.', "");
    let plain = match exponent {
        0 => format!("{}.{}", &digits[..1], &digits[1..]),
        1 => digits,
        2 => format!("{digits}0"),
        e => format!("0.{}{digits}", "0".repeat((-e - 1) as usize)),
    };
    format!("{sign}{plain}")
}

fn plain(v: f64) -> String {
    if v == 0.0 || (1e-3..1e3).contains(&v.abs()) {
        format!("{v}")
    } else {
        num(v)
    }
}

fn claimless(rows: &[Row]) -> bool {
    !(rows.iter()).any(|r| r.tier == Tier::Claim && r.outcome != Outcome::NoRecord)
}

fn page(rows: &[Row]) -> String {
    let mut out = String::from("# Benchmark results\n\n");
    out.push_str("Generated by `cargo xtask results render` from [RESULTS.toml](RESULTS.toml); edit no line by hand.\n");
    out.push_str("Each row comes from one record; RESULTS.toml keeps its commit, its engine sha256 and every bound.\n");
    out.push_str("A row shows its case's headline metric: of those that results-what.toml names, the nearest to its bound.\n");
    if claimless(rows) {
        out.push_str("No claim-tier row: these results make no speed or memory claim.\n");
    }
    out.push_str("\n<picture>\n  <source media=\"(prefers-color-scheme: dark)\" srcset=\"../assets/benchmarks/results-dark.svg\">\n");
    out.push_str("  <img alt=\"Each case's measured error against its bound, log scale\" src=\"../assets/benchmarks/results-light.svg\">\n</picture>\n");
    for tier in Tier::ALL {
        let rows: Vec<&Row> = rows.iter().filter(|r| r.tier == tier).collect();
        if rows.is_empty() {
            continue;
        }
        let _ = write!(
            out,
            "\n## {} tier\n\n| case | what | metric | measured | bound | unit | outcome | commit |\n|---|---|---|---|---|---|---|---|\n",
            tier.name()
        );
        for row in rows {
            let commit = row.commit.as_deref().map_or("none", |c| &c[..7]);
            let cells = headline_of(row).map_or_else(
                || " | | ".to_owned(),
                |m| {
                    format!(
                        "`{}` | {} | {} {}",
                        m.name,
                        short(m.measured),
                        m.bound.sign(),
                        plain(m.limit)
                    )
                },
            );
            let slug = row.id.split_once('_').map_or("", |(_, slug)| slug);
            let _ = writeln!(
                out,
                "| {} `{slug}` | {} | {cells} | {} | {} | {commit} |",
                row.number,
                row.what,
                row.unit,
                row.outcome.name(),
            );
        }
    }
    out
}

struct Theme {
    surface: &'static str,
    text: &'static str,
    muted: &'static str,
    grid: &'static str,
    series: &'static str,
    pass: &'static str,
    fail: &'static str,
}

const LIGHT_THEME: Theme = Theme {
    surface: "#ffffff",
    text: "#191919",
    muted: "#666666",
    grid: "#ebebeb",
    series: "#257de0",
    pass: "#0aac23",
    fail: "#dc3c3d",
};

const DARK_THEME: Theme = Theme {
    surface: "#191919",
    text: "#efefef",
    muted: "#9c9c9c",
    grid: "#2e2e2e",
    series: "#338ced",
    pass: "#0aac23",
    fail: "#dc3c3d",
};

fn log10(x: f64) -> f64 {
    let text = format!("{x:e}");
    let (mantissa, exponent) = text.split_once('e').unwrap_or((&text, "0"));
    let m: f64 = mantissa.parse().unwrap_or(1.0);
    let e = exponent.parse::<i32>().map_or(0.0, f64::from);
    let y = m.sqrt().sqrt();
    let z = (y - 1.0) / (y + 1.0);
    let (mut term, mut sum) = (z, 0.0);
    for k in 0..30 {
        sum += term / f64::from(2 * k + 1);
        term *= z * z;
    }
    e + 8.0 * sum / std::f64::consts::LN_10
}

fn pow10(y: f64) -> f64 {
    let k = y.floor();
    let whole: f64 = format!("1e{k}").parse().unwrap_or(f64::NAN);
    let z = (y - k) * std::f64::consts::LN_10 / 256.0;
    let (mut term, mut sum) = (1.0, 1.0);
    for n in 1..12 {
        term *= z / f64::from(n);
        sum += term;
    }
    for _ in 0..8 {
        sum *= sum;
    }
    whole * sum
}

fn geomean(values: impl Iterator<Item = f64>) -> f64 {
    let values: Vec<f64> = values.collect();
    match values.iter().any(|v| v.is_infinite()) {
        true => f64::INFINITY,
        false => pow10(values.iter().map(|&v| log10(v)).sum::<f64>() / values.len() as f64),
    }
}

fn escaped(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

fn px(v: f64) -> String {
    format!("{v:.1}")
}

fn chart(rows: &[Row], theme: &Theme) -> String {
    const WIDTH: f64 = 960.0;
    const ROW: f64 = 26.0;
    const TOP: f64 = 64.0;
    const STATUS_END: f64 = WIDTH - 16.0;
    const VALUES: f64 = STATUS_END - 56.0 - 226.0;
    const PLOT_RIGHT: f64 = VALUES - 24.0;
    const PLOT_LEFT: f64 = 340.0;
    let values: Vec<f64> = (rows.iter().filter_map(headline_of))
        .flat_map(|m| [m.measured, m.limit])
        .filter(|v| v.is_finite() && *v > 0.0)
        .collect();
    let even_down = |d: f64| d - d.rem_euclid(2.0);
    let low = even_down(values.iter().map(|v| log10(*v).floor()).fold(0.0, f64::min) - 1.0);
    let high = values.iter().map(|v| log10(*v).ceil()).fold(0.0, f64::max);
    let high = high + high.rem_euclid(2.0);
    let x = |v: f64| PLOT_LEFT + (log10(v) - low) / (high - low) * (PLOT_RIGHT - PLOT_LEFT);
    let bottom = TOP + ROW * rows.len() as f64;
    let height = bottom + 52.0;
    let t = theme;
    let mut out = String::new();
    let _ = writeln!(
        out,
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{w}\" height=\"{h}\" viewBox=\"0 0 {w} {h}\" font-family=\"system-ui, sans-serif\">",
        w = px(WIDTH),
        h = px(height)
    );
    let _ = writeln!(
        out,
        "<rect width=\"{}\" height=\"{}\" fill=\"{}\"/>",
        px(WIDTH),
        px(height),
        t.surface
    );
    let _ = writeln!(
        out,
        "<text x=\"16\" y=\"26\" font-size=\"16\" font-weight=\"600\" fill=\"{}\">orbitaR benchmarks: each case against its bound</text>",
        t.text
    );
    let claim = if claimless(rows) {
        " · no speed or memory claim yet"
    } else {
        ""
    };
    let _ = writeln!(
        out,
        "<text x=\"16\" y=\"46\" font-size=\"12\" fill=\"{}\">dot: measured error · tick: the case bound · log scale{claim}</text>",
        t.muted
    );
    let mut decade = low;
    while decade <= high {
        let at = px(PLOT_LEFT + (decade - low) / (high - low) * (PLOT_RIGHT - PLOT_LEFT));
        let _ = writeln!(
            out,
            "<line x1=\"{at}\" y1=\"{}\" x2=\"{at}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"1\"/>",
            px(TOP - 4.0),
            px(bottom),
            t.grid
        );
        if (high - decade).rem_euclid(4.0) == 0.0 {
            let label = if decade == 0.0 {
                "1".to_owned()
            } else {
                format!("1e{decade}")
            };
            let _ = writeln!(
                out,
                "<text x=\"{at}\" y=\"{}\" font-size=\"11\" fill=\"{}\" text-anchor=\"middle\">{label}</text>",
                px(bottom + 16.0),
                t.muted
            );
        }
        decade += 2.0;
    }
    let _ = writeln!(
        out,
        "<text x=\"{}\" y=\"{}\" font-size=\"11\" fill=\"{}\" text-anchor=\"middle\">error (unit per row; smaller is better)</text>",
        px((PLOT_LEFT + PLOT_RIGHT) / 2.0),
        px(bottom + 36.0),
        t.muted
    );
    for (n, row) in rows.iter().enumerate() {
        let y = TOP + ROW * n as f64 + ROW / 2.0;
        let base = px(y + 4.0);
        let _ = writeln!(
            out,
            "<text x=\"40\" y=\"{base}\" font-size=\"12.5\" font-weight=\"600\" fill=\"{}\" text-anchor=\"end\">{}</text>\n<text x=\"48\" y=\"{base}\" font-size=\"12.5\" fill=\"{}\">{}</text>",
            t.text,
            row.number,
            t.text,
            escaped(&row.what)
        );
        let (status, colour) = match row.outcome {
            Outcome::Pass => ("✓ pass", t.pass),
            Outcome::Fail => ("✕ fail", t.fail),
            Outcome::NoRecord => ("no record", t.muted),
        };
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"{base}\" font-size=\"11.5\" fill=\"{colour}\" text-anchor=\"end\">{status}</text>",
            px(STATUS_END)
        );
        let Some(m) = headline_of(row) else {
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{base}\" font-size=\"12\" fill=\"{}\">no record yet: it runs in the {} tier</text>",
                px(PLOT_LEFT),
                t.muted,
                row.tier.name()
            );
            continue;
        };
        if m.limit == 0.0 && m.measured == 0.0 {
            let exact = row
                .exact
                .as_deref()
                .unwrap_or("bit-identical: measured 0 against a bound of 0");
            let _ = writeln!(
                out,
                "<text x=\"{}\" y=\"{base}\" font-size=\"12\" fill=\"{}\">{}</text>",
                px(PLOT_LEFT),
                t.text,
                escaped(exact)
            );
            continue;
        }
        let values = format!(
            "{} {} {} {}",
            short(m.measured),
            m.bound.sign(),
            plain(m.limit),
            row.unit
        );
        let _ = writeln!(
            out,
            "<text x=\"{}\" y=\"{base}\" font-size=\"11.5\" fill=\"{}\">{}</text>",
            px(VALUES),
            t.muted,
            escaped(values.trim_end())
        );
        let at = |v: f64| {
            if v.is_finite() && v > 0.0 {
                x(v)
            } else if v.is_finite() {
                PLOT_LEFT
            } else {
                PLOT_RIGHT
            }
        };
        let (dot, tick) = (px(at(m.measured)), px(at(m.limit)));
        let _ = writeln!(
            out,
            "<line x1=\"{dot}\" y1=\"{yc}\" x2=\"{tick}\" y2=\"{yc}\" stroke=\"{s}\" stroke-opacity=\"0.35\" stroke-width=\"2\"/>\n<line x1=\"{tick}\" y1=\"{}\" x2=\"{tick}\" y2=\"{}\" stroke=\"{}\" stroke-width=\"2\"/>\n<circle cx=\"{dot}\" cy=\"{yc}\" r=\"5\" fill=\"{s}\"/>",
            px(y - 7.0),
            px(y + 7.0),
            t.text,
            yc = px(y),
            s = t.series
        );
    }
    out.push_str("</svg>\n");
    out
}
