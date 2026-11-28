use super::{num, quoted};
use crate::{
    case::{Case, Tier},
    harness::{
        self, record,
        speed::{BlockRecord, Code, read_finite},
    },
};
use serde::Deserialize;
use serde_json::Value;
use std::{fmt::Write as _, path::Path};

pub(super) const ORBITAR: &str = "orbitar";

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
    pub(super) coverage: String,
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
    pub(super) status: String,
    pub(super) t_delta_s: Option<f64>,
    pub(super) peak_pss_bytes: Option<f64>,
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
}

pub(super) fn read(root: &Path, id: &str, case: &Case, record: &Value) -> Result<Claim, String> {
    let proof = record::proves(root, Tier::Claim, id, case, record);
    if let Some(first) = proof.first() {
        return Err(format!(
            "the harness's claim proof refuses it ({} finding(s)): {first}",
            proof.len()
        ));
    }
    let made = harness::replay(root, record)?;
    let mut blocks: Vec<BlockRecord> = Vec::new();
    for b in record["speed"].as_array().into_iter().flatten() {
        blocks.push(
            serde_json::from_value(b.clone())
                .map_err(|e| format!("a speed block does not parse: {e}"))?,
        );
    }
    let atoms = |input: &str| {
        (blocks.iter())
            .find(|b| b.code == Code::Orbitar && b.input == input)
            .and_then(|b| u32::try_from(b.atoms).ok())
            .unwrap_or(0)
    };
    let mut skip = Vec::new();
    for s in made.skipped.as_array().into_iter().flatten() {
        let text = |key: &str| s[key].as_str().unwrap_or("").to_owned();
        skip.push(Skip {
            code: text("code"),
            input: text("input"),
            atoms: atoms(s["input"].as_str().unwrap_or("")),
            kind: Kind::Skipped,
            reason: text("reason"),
        });
    }
    for b in &blocks {
        let (kind, reason) = match (&b.refused, &b.timing) {
            (Some(why), _) => (Kind::Refused, why.clone()),
            (None, None) => (Kind::Untimed, format!("{} runs lost", b.losses.len())),
            (None, Some(_)) => continue,
        };
        skip.push(Skip {
            code: b.code.to_string(),
            input: b.input.clone(),
            atoms: atoms(&b.input),
            kind,
            reason,
        });
    }
    let number = |v: &Value| read_finite(v).unwrap_or(f64::NAN);
    let count = |v: &Value| {
        (v.as_u64())
            .and_then(|n| u32::try_from(n).ok())
            .unwrap_or(0)
    };
    let versus = (made.versus.as_array().into_iter().flatten())
        .map(|v| Versus {
            rival: v["code"].as_str().unwrap_or("").to_owned(),
            g: number(&v["g"]),
            g_ci: [number(&v["g_ci"][0]), number(&v["g_ci"][1])],
            r: number(&v["r"]),
            pairs: (v["pairs"].as_array().into_iter().flatten())
                .map(|p| Pair {
                    input: p["input"].as_str().unwrap_or("").to_owned(),
                    threads: count(&p["threads"]),
                    t_delta_s_rival: number(&p["medians_t_delta_s"][0]),
                    t_delta_s_orbitar: number(&p["medians_t_delta_s"][1]),
                    peak_pss_bytes_rival: number(&p["medians_peak_pss_bytes"][0]),
                    peak_pss_bytes_orbitar: number(&p["medians_peak_pss_bytes"][1]),
                    in_r: p["in_r"] == Value::Bool(true),
                    losses: count(&p["losses"]),
                })
                .collect(),
        })
        .collect();
    let series = (made.series.as_array().into_iter().flatten())
        .map(|e| Series {
            code: e["code"].as_str().unwrap_or("").to_owned(),
            coverage: e["coverage"].as_str().unwrap_or("").to_owned(),
            t_delta_exponent: number(&e["t_delta_exponent"]),
            pss_exponent: number(&e["pss_exponent"]),
            points: (e["points"].as_array().into_iter().flatten())
                .map(|p| Point {
                    input: p["input"].as_str().unwrap_or("").to_owned(),
                    atoms: count(&p["atoms"]),
                    status: p["status"].as_str().unwrap_or("").to_owned(),
                    t_delta_s: read_finite(&p["median_t_delta_s"]),
                    peak_pss_bytes: read_finite(&p["median_peak_pss_bytes"]),
                })
                .collect(),
        })
        .collect();
    let node: toml::Table = serde_json::from_value(record["node"].clone())
        .map_err(|e| format!("the record's node block: {e}"))?;
    let claim = Claim {
        versus,
        series,
        skip,
        node,
    };
    claim.cpu()?;
    for r in case.rivals() {
        claim.package(r.adapter().name())?;
    }
    Ok(claim)
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
            "\n[[case.claim.series]]\ncode = {}\ncoverage = {}\nt_delta_exponent = {}\npss_exponent = {}\n",
            quoted(&s.code),
            quoted(&s.coverage),
            num(s.t_delta_exponent),
            num(s.pss_exponent)
        );
        for p in &s.points {
            let _ = write!(
                out,
                "\n[[case.claim.series.point]]\ninput = {}\natoms = {}\nstatus = {}\n",
                quoted(&p.input),
                p.atoms,
                quoted(&p.status)
            );
            for (key, value) in [
                ("t_delta_s", p.t_delta_s),
                ("peak_pss_bytes", p.peak_pss_bytes),
            ] {
                if let Some(value) = value {
                    let _ = writeln!(out, "{key} = {}", num(value));
                }
            }
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
