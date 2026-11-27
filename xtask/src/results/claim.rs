use super::{log10, num, quoted};
use crate::{
    case::{Case, Tier},
    harness::{
        record,
        speed::{self, BlockRecord, Code, read_finite},
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::{collections::BTreeSet, fmt::Write as _, path::Path};

pub(super) const ORBITAR: &str = "orbitar";
const AGREE: f64 = 1e-9;

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Claim {
    #[serde(default)]
    pub(super) versus: Vec<Versus>,
    #[serde(default)]
    pub(super) series: Vec<Series>,
    #[serde(default)]
    pub(super) skip: Vec<Skip>,
    pub(super) node: toml::Table,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Versus {
    pub(super) rival: String,
    pub(super) g: f64,
    pub(super) g_ci: [f64; 2],
    pub(super) r: f64,
    #[serde(rename = "pair")]
    pub(super) pairs: Vec<Pair>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Pair {
    pub(super) input: String,
    pub(super) threads: u32,
    pub(super) t_delta_s_rival: f64,
    pub(super) t_delta_s_orbitar: f64,
    pub(super) peak_pss_bytes_rival: f64,
    pub(super) peak_pss_bytes_orbitar: f64,
    pub(super) in_r: bool,
    pub(super) losses: u32,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Series {
    pub(super) code: String,
    pub(super) t_delta_exponent: f64,
    pub(super) pss_exponent: f64,
    #[serde(default, rename = "point")]
    pub(super) points: Vec<Point>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Point {
    pub(super) input: String,
    pub(super) atoms: u32,
    pub(super) t_delta_s: f64,
    pub(super) peak_pss_bytes: f64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Skip {
    pub(super) code: String,
    pub(super) input: String,
    pub(super) atoms: u32,
    pub(super) kind: Kind,
    pub(super) reason: String,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    Skipped,
    Refused,
    Untimed,
}

impl Kind {
    pub(super) fn name(self) -> &'static str {
        match self {
            Kind::Skipped => "skipped",
            Kind::Refused => "refused",
            Kind::Untimed => "untimed",
        }
    }
}

impl Claim {
    pub(super) fn cpu(&self) -> Result<&str, String> {
        (self.node.get("host").and_then(|h| h.get("cpu")?.as_str()))
            .ok_or_else(|| "the node block names no CPU: [host].cpu".to_owned())
    }

    pub(super) fn package(&self, code: &str) -> Result<(String, String), String> {
        let text = (self.node.get(code).and_then(|t| t.get(code)?.as_str())).unwrap_or("");
        let mut words = text.split_whitespace();
        match (words.next(), words.next()) {
            (Some(version), Some(build)) => Ok((version.to_owned(), build.to_owned())),
            _ => Err(format!(
                "the node block names no version and build of {code}: [{code}].{code}"
            )),
        }
    }

    pub(super) fn skipped(&self, code: &str, input: &str) -> Option<&Skip> {
        (self.skip.iter()).find(|s| s.code == code && s.input == input)
    }
}

fn close(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits() || (a / b - 1.0).abs() <= AGREE
}

struct Block {
    record: BlockRecord,
    refused: Option<String>,
}

pub(super) fn read(root: &Path, id: &str, case: &Case, record: &Value) -> Result<Claim, String> {
    let proof = record::proves(root, Tier::Claim, id, case, record);
    if let Some(first) = proof.first() {
        return Err(format!(
            "the harness's claim proof refuses it ({} finding(s)): {first}",
            proof.len()
        ));
    }
    let named: Vec<&str> = case.rivals().iter().map(|r| r.adapter().name()).collect();
    let mut blocks = Vec::new();
    for b in record["speed"].as_array().into_iter().flatten() {
        let parsed: BlockRecord = serde_json::from_value(b.clone())
            .map_err(|e| format!("a speed block does not parse: {e}"))?;
        let refused = b["refused"].as_str().map(str::to_owned);
        blocks.push(Block {
            record: parsed,
            refused,
        });
    }
    let mut seen = BTreeSet::new();
    for b in &blocks {
        let r = &b.record;
        if !seen.insert((r.code.to_string(), r.input.clone(), r.threads.get())) {
            return Err(format!(
                "the speed blocks hold {} {} at {} threads twice",
                r.code, r.input, r.threads
            ));
        }
    }
    let threads: BTreeSet<usize> = blocks.iter().map(|b| b.record.threads.get()).collect();
    if threads.len() != 1 {
        return Err(format!(
            "the speed blocks run {threads:?} threads; a card compares one thread count"
        ));
    }
    let ours: Vec<&BlockRecord> = (blocks.iter())
        .map(|b| &b.record)
        .filter(|r| r.code == Code::Orbitar)
        .collect();
    let atoms = |input: &str| {
        (ours.iter())
            .find(|o| o.input == input)
            .and_then(|o| u32::try_from(o.atoms).ok())
            .ok_or(format!("{input}: orbitaR has no speed block of it"))
    };
    let mut skip = Vec::new();
    for s in record["skipped"].as_array().into_iter().flatten() {
        let (Some(code), Some(input), Some(reason)) = (
            s["code"].as_str(),
            s["input"].as_str(),
            s["reason"].as_str(),
        ) else {
            return Err(format!(
                "a skipped entry lacks its code, input or reason: {s}"
            ));
        };
        if !named.contains(&code) {
            return Err(format!(
                "a skipped entry names {code}, which the case does not name"
            ));
        }
        skip.push(Skip {
            code: code.to_owned(),
            input: input.to_owned(),
            atoms: atoms(input)?,
            kind: Kind::Skipped,
            reason: reason.to_owned(),
        });
    }
    for b in &blocks {
        let r = &b.record;
        let (kind, reason) = match (&b.refused, &r.timing) {
            (Some(why), _) => (Kind::Refused, why.clone()),
            (None, None) => (Kind::Untimed, format!("{} runs lost", r.losses.len())),
            (None, Some(_)) => continue,
        };
        skip.push(Skip {
            code: r.code.to_string(),
            input: r.input.clone(),
            atoms: atoms(&r.input)?,
            kind,
            reason,
        });
    }
    for code in &named {
        for o in &ours {
            let timed = (blocks.iter()).any(|b| {
                b.refused.is_none()
                    && b.record.code.to_string() == *code
                    && b.record.input == o.input
            });
            let skipped = (skip.iter()).any(|s| s.code == *code && s.input == o.input);
            if !timed && !skipped {
                return Err(format!(
                    "{code} {}: neither a speed block nor a recorded skip",
                    o.input
                ));
            }
        }
    }
    for b in &blocks {
        atoms(&b.record.input)?;
    }
    let live: Vec<&BlockRecord> = (blocks.iter())
        .filter(|b| b.refused.is_none())
        .map(|b| &b.record)
        .collect();
    let (wanted, _) = speed::compare(&live, case.milestone());
    let given = record["versus"].as_array().map_or(&[][..], Vec::as_slice);
    let mut versus = Vec::new();
    for code in &named {
        let find = |list: &[Value]| {
            (list.iter())
                .find(|v| v["code"].as_str() == Some(code))
                .cloned()
        };
        let (Some(want), Some(got)) = (find(&wanted), find(given)) else {
            return Err(format!(
                "a passing claim record holds no versus entry for {code}, which the case names"
            ));
        };
        versus.push(agreed(code, &want, &got)?);
    }
    if let Some(extra) =
        (given.iter()).find(|v| !named.iter().any(|n| v["code"].as_str() == Some(n)))
    {
        return Err(format!(
            "versus {}: the case names rivals {named:?}",
            extra["code"]
        ));
    }
    let node: toml::Table = serde_json::from_value(record["node"].clone())
        .map_err(|e| format!("the record's node block: {e}"))?;
    let mut claim = Claim {
        versus,
        series: Vec::new(),
        skip,
        node,
    };
    claim.cpu()?;
    for code in &named {
        claim.package(code)?;
    }
    if let Some(entries) = record["series"].as_array() {
        claim.series = series(entries, &named, &blocks, &claim)?;
    }
    Ok(claim)
}

fn agreed(code: &str, want: &Value, got: &Value) -> Result<Versus, String> {
    let number =
        |v: &Value, name: &str| read_finite(v).ok_or(format!("versus {code}: no number {name}"));
    let check = |name: &str, want: &Value, got: &Value| -> Result<f64, String> {
        let (w, g) = (number(want, name)?, number(got, name)?);
        match close(w, g) {
            true => Ok(g),
            false => Err(format!(
                "versus {code}: {name} is {g:e}, but its speed blocks give {w:e}"
            )),
        }
    };
    let (wants, gots) = (
        want["pairs"].as_array().map_or(&[][..], Vec::as_slice),
        got["pairs"].as_array().map_or(&[][..], Vec::as_slice),
    );
    if wants.len() != gots.len() {
        return Err(format!(
            "versus {code}: {} pairs, but its speed blocks give {}",
            gots.len(),
            wants.len()
        ));
    }
    let mut pairs = Vec::new();
    for (w, g) in wants.iter().zip(gots) {
        let same = |key: &str| w[key] == g[key];
        if !(same("input") && same("threads") && same("in_r") && same("losses")) {
            return Err(format!(
                "versus {code}: the pair {} differs from its speed blocks in input, threads, in_r or losses",
                g["input"]
            ));
        }
        let medians = |key: &str| -> Result<[f64; 2], String> {
            Ok([
                check(key, &w[key][0], &g[key][0])?,
                check(key, &w[key][1], &g[key][1])?,
            ])
        };
        let [t_delta_s_rival, t_delta_s_orbitar] = medians("medians_t_delta_s")?;
        let [peak_pss_bytes_rival, peak_pss_bytes_orbitar] = medians("medians_peak_pss_bytes")?;
        pairs.push(Pair {
            input: g["input"].as_str().unwrap_or("").to_owned(),
            threads: (g["threads"].as_u64())
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
            t_delta_s_rival,
            t_delta_s_orbitar,
            peak_pss_bytes_rival,
            peak_pss_bytes_orbitar,
            in_r: g["in_r"] == Value::Bool(true),
            losses: (g["losses"].as_u64())
                .and_then(|n| u32::try_from(n).ok())
                .unwrap_or(0),
        });
    }
    Ok(Versus {
        rival: code.to_owned(),
        g: check("g", &want["g"], &got["g"])?,
        g_ci: [
            check("g_ci", &want["g_ci"][0], &got["g_ci"][0])?,
            check("g_ci", &want["g_ci"][1], &got["g_ci"][1])?,
        ],
        r: check("r", &want["r"], &got["r"])?,
        pairs,
    })
}

fn series(
    entries: &[Value],
    named: &[&str],
    blocks: &[Block],
    claim: &Claim,
) -> Result<Vec<Series>, String> {
    let mut out: Vec<Series> = Vec::new();
    for e in entries {
        let code = e["code"].as_str().ok_or("a series entry names no code")?;
        if (code != ORBITAR && !named.contains(&code)) || out.iter().any(|s| s.code == code) {
            return Err(format!(
                "series {code}: the case names rivals {named:?}, and orbitar, each once"
            ));
        }
        let number = |v: &Value, name: &str| {
            read_finite(v).ok_or(format!("series {code}: no number {name}"))
        };
        let mut points = Vec::new();
        for p in e["points"].as_array().into_iter().flatten() {
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
        out.push(Series {
            code: code.to_owned(),
            t_delta_exponent: number(&e["t_delta_exponent"], "t_delta_exponent")?,
            pss_exponent: number(&e["pss_exponent"], "pss_exponent")?,
            points,
        });
    }
    let inputs: Vec<&str> = (blocks.iter())
        .filter(|b| b.record.code == Code::Orbitar)
        .map(|b| b.record.input.as_str())
        .collect();
    for code in std::iter::once(ORBITAR).chain(named.iter().copied()) {
        let points = (out.iter())
            .find(|s| s.code == code)
            .map_or(&[][..], |s| &s.points[..]);
        for p in points {
            if !inputs.contains(&p.input.as_str()) {
                return Err(format!(
                    "series {code}: {} is no input of the speed blocks",
                    p.input
                ));
            }
        }
        for input in &inputs {
            let block = (blocks.iter())
                .find(|b| {
                    b.refused.is_none()
                        && b.record.code.to_string() == code
                        && b.record.input == *input
                })
                .and_then(|b| Some((b.record.atoms, b.record.timing.as_ref()?)));
            let point = points.iter().find(|p| p.input == *input);
            match (point, block) {
                (Some(p), Some((atoms, t))) => {
                    let same = usize::try_from(p.atoms).is_ok_and(|a| a == atoms)
                        && close(p.t_delta_s, t.median_t_delta_s)
                        && close(p.peak_pss_bytes, t.median_peak_pss_bytes);
                    if !same {
                        return Err(format!(
                            "series {code}: the point of {input} differs from its speed block's atoms and medians"
                        ));
                    }
                }
                (None, None) if claim.skipped(code, input).is_some() => {}
                (None, _) => {
                    return Err(format!(
                        "series {code}: {input} has neither a point nor a recorded skip"
                    ));
                }
                (Some(_), None) => {
                    return Err(format!(
                        "series {code}: {input} has a point but no timed speed block"
                    ));
                }
            }
        }
    }
    for s in &out {
        let name = &s.code;
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
    }
    Ok(out)
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

pub(super) fn write(out: &mut String, claim: &Claim) {
    for v in &claim.versus {
        let _ = write!(
            out,
            "\n[[case.claim.versus]]\nrival = {}\ng = {}\ng_ci = [{}, {}]\nr = {}\n",
            quoted(&v.rival),
            num(v.g),
            num(v.g_ci[0]),
            num(v.g_ci[1]),
            num(v.r)
        );
        for p in &v.pairs {
            let _ = write!(
                out,
                "\n[[case.claim.versus.pair]]\ninput = {}\nthreads = {}\nt_delta_s_rival = {}\nt_delta_s_orbitar = {}\npeak_pss_bytes_rival = {}\npeak_pss_bytes_orbitar = {}\nin_r = {}\nlosses = {}\n",
                quoted(&p.input),
                p.threads,
                num(p.t_delta_s_rival),
                num(p.t_delta_s_orbitar),
                num(p.peak_pss_bytes_rival),
                num(p.peak_pss_bytes_orbitar),
                p.in_r,
                p.losses
            );
        }
    }
    for s in &claim.series {
        let _ = write!(
            out,
            "\n[[case.claim.series]]\ncode = {}\nt_delta_exponent = {}\npss_exponent = {}\n",
            quoted(&s.code),
            num(s.t_delta_exponent),
            num(s.pss_exponent)
        );
        for p in &s.points {
            let _ = write!(
                out,
                "\n[[case.claim.series.point]]\ninput = {}\natoms = {}\nt_delta_s = {}\npeak_pss_bytes = {}\n",
                quoted(&p.input),
                p.atoms,
                num(p.t_delta_s),
                num(p.peak_pss_bytes)
            );
        }
    }
    for s in &claim.skip {
        let _ = write!(
            out,
            "\n[[case.claim.skip]]\ncode = {}\ninput = {}\natoms = {}\nkind = \"{}\"\nreason = {}\n",
            quoted(&s.code),
            quoted(&s.input),
            s.atoms,
            s.kind.name(),
            quoted(&s.reason)
        );
    }
    table(out, "case.claim.node", &claim.node);
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
