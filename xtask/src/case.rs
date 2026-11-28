use crate::harness::{
    rival::Rival,
    speed::{Milestone, RSS_EXPONENT, SERIES},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    num::NonZeroUsize,
    path::Path,
};

#[derive(Clone, Debug)]
pub(crate) struct Case {
    case: Meta,
    tolerance: BTreeMap<String, f64>,
    relax: BTreeMap<String, Relax>,
    why: BTreeMap<String, String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    case: Meta,
    #[serde(default)]
    tolerance: BTreeMap<String, f64>,
    #[serde(default)]
    relax: BTreeMap<String, Relax>,
    #[serde(default)]
    why: BTreeMap<String, String>,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Relax {
    Ions,
    Cell,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Meta {
    tier: Tier,
    status: Status,
    claims: Vec<Claim>,
    reference: Reference,
    timeout_s: Option<u64>,
    repeats: Option<u32>,
    #[serde(default)]
    rivals: Vec<Rival>,
    #[serde(default)]
    milestone: Milestone,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    Push,
    Pr,
    Nightly,
    Claim,
    Node,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    Active,
    Planned,
    Retired,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Claim {
    Correctness,
    Accuracy,
    Speed,
    Memory,
    Optimization,
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Reference {
    Analytic,
    Literature,
    Oracle,
    SameDataset,
    AllElectron,
    VaspPublished,
    VaspMeasured,
}

impl Reference {
    pub(crate) const NAMES: [&str; 7] = [
        "analytic",
        "literature",
        "oracle",
        "same-dataset",
        "all-electron",
        "vasp-published",
        "vasp-measured",
    ];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Reference::Analytic => Self::NAMES[0],
            Reference::Literature => Self::NAMES[1],
            Reference::Oracle => Self::NAMES[2],
            Reference::SameDataset => Self::NAMES[3],
            Reference::AllElectron => Self::NAMES[4],
            Reference::VaspPublished => Self::NAMES[5],
            Reference::VaspMeasured => Self::NAMES[6],
        }
    }
}

pub(crate) struct Policy {
    pub(crate) budget_s: Option<u64>,
    pub(crate) repeats: u32,
    pub(crate) timeout_s: u64,
    pub(crate) one_case_at_a_time: bool,
    pub(crate) speed: Option<Speed>,
}

pub(crate) struct Speed {
    pub(crate) threads: &'static [NonZeroUsize; 1],
    pub(crate) cv_max: f64,
    pub(crate) factor: usize,
}

const SWEEP: [NonZeroUsize; 1] = [NonZeroUsize::new(8).unwrap()];

impl Tier {
    pub(crate) const ALL: [Tier; 5] =
        [Tier::Push, Tier::Pr, Tier::Nightly, Tier::Claim, Tier::Node];

    pub(crate) fn name(self) -> &'static str {
        match self {
            Tier::Push => "push",
            Tier::Pr => "pr",
            Tier::Nightly => "nightly",
            Tier::Claim => "claim",
            Tier::Node => "node",
        }
    }

    pub(crate) fn parse(name: &str) -> Option<Tier> {
        Tier::ALL.into_iter().find(|t| t.name() == name)
    }

    pub(crate) fn includes(self, case: Tier) -> bool {
        match self {
            Tier::Claim | Tier::Node => case == self,
            _ => case <= self,
        }
    }

    pub(crate) fn policy(self) -> Policy {
        match self {
            Tier::Push => Policy {
                budget_s: Some(60),
                repeats: 1,
                timeout_s: 30,
                one_case_at_a_time: false,
                speed: None,
            },
            Tier::Pr => Policy {
                budget_s: Some(2_700),
                repeats: 1,
                timeout_s: 600,
                one_case_at_a_time: false,
                speed: None,
            },
            Tier::Nightly => Policy {
                budget_s: Some(18_000),
                repeats: 1,
                timeout_s: 7_200,
                one_case_at_a_time: false,
                speed: None,
            },
            Tier::Claim => Policy {
                budget_s: None,
                repeats: 5,
                timeout_s: 86_400,
                one_case_at_a_time: true,
                speed: Some(Speed {
                    threads: &SWEEP,
                    cv_max: 0.05,
                    factor: 2,
                }),
            },
            Tier::Node => Policy {
                budget_s: None,
                repeats: 1,
                timeout_s: 86_400,
                one_case_at_a_time: true,
                speed: None,
            },
        }
    }
}

impl Case {
    pub(crate) fn load(root: &Path, id: &str) -> Result<Case, String> {
        let path = format!("benchmarks/{id}/case.toml");
        let text = fs::read_to_string(root.join(&path)).map_err(|e| format!("{path}: {e}"))?;
        let case = Case::parse(&text).map_err(|e| format!("{path}: {e}"))?;
        if case.why.contains_key(WHY_INPUTS)
            && !holds_inputs(&root.join(format!("benchmarks/{id}")))
        {
            return Err(format!(
                "{path}: [why].{WHY_INPUTS} names the case's inputs, but benchmarks/{id} holds no {INPUT_FILE}, {INPUTS_DIR}/<stem>.toml, or {CRYSTALS}"
            ));
        }
        Ok(case)
    }

    pub(crate) fn parse(text: &str) -> Result<Case, String> {
        let File {
            case,
            tolerance,
            relax,
            why,
        } = toml::from_str(text).map_err(|e| e.to_string())?;
        let raw: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        let set: BTreeSet<String> = (raw.get("case").and_then(toml::Value::as_table))
            .map(|table| table.keys().cloned().collect())
            .unwrap_or_default();
        let case = Case {
            case,
            tolerance,
            relax,
            why,
        };
        case.validate(&set)?;
        Ok(case)
    }

    fn validate(&self, set: &BTreeSet<String>) -> Result<(), String> {
        let m = &self.case;
        if m.status == Status::Active && self.tolerance.is_empty() {
            return Err("an active case needs a non-empty [tolerance]".into());
        }
        for (key, value) in &self.tolerance {
            if bounded(key).is_none() {
                return Err(format!("[tolerance].{key} must end with _max or _min"));
            }
            if !value.is_finite() {
                return Err(format!("[tolerance].{key} must be finite, got {value}"));
            }
        }
        if m.claims.is_empty() {
            return Err(
                "claims must name at least one of correctness, accuracy, speed, memory, optimization"
                    .into(),
            );
        }
        let timed = (m.claims.iter())
            .any(|c| [Claim::Speed, Claim::Memory, Claim::Optimization].contains(c));
        if (timed || m.reference == Reference::VaspMeasured) && m.tier != Tier::Claim {
            return Err(
                "speed, memory, optimization, and vasp-measured cases run only in the claim tier"
                    .into(),
            );
        }
        let checked = m.claims.contains(&Claim::Accuracy) || m.claims.contains(&Claim::Correctness);
        let measured = self.tolerance.keys().any(|key| !is_reserved(key));
        if timed && m.status == Status::Active && !(checked && measured) {
            return Err(format!(
                "a speed, memory, or optimization claim needs an accuracy or correctness claim and a tolerance on a measured metric, not only on {RESERVED:?}"
            ));
        }
        let ceiling = m.tier.policy().timeout_s;
        if m.timeout_s.is_some_and(|t| t == 0 || t > ceiling) {
            return Err(format!(
                "timeout_s must be between 1 and the {} tier ceiling {ceiling}",
                m.tier.name()
            ));
        }
        if !m.rivals.is_empty() && !m.claims.contains(&Claim::Speed) {
            return Err("rivals time only beside a speed claim".into());
        }
        if m.milestone != Milestone::default() && m.rivals.is_empty() {
            return Err("a milestone sets the bar on G, which only rivals give".into());
        }
        let judged = !m.rivals.is_empty()
            || (self.tolerance.keys()).any(|key| bounded(key) == Some((RSS_EXPONENT, Bound::Max)));
        if m.claims.contains(&Claim::Memory) && m.status == Status::Active && !judged {
            return Err(format!(
                "an active memory claim needs rivals, whose R judges it, or {RSS_EXPONENT}_max"
            ));
        }
        if m.repeats == Some(0) {
            return Err("repeats must be at least 1".into());
        }
        for (key, text) in &self.why {
            let other = set.contains(key) || WHY_GROUPS.contains(&key.as_str());
            if other && self.relax.contains_key(key) {
                return Err(format!(
                    "[why].{key} names both [relax].{key} and another target of the case file; rename the input"
                ));
            }
            if key == WHY_TOLERANCE && self.tolerance.is_empty() {
                return Err(format!(
                    "[why].{WHY_TOLERANCE} names the whole [tolerance] table, which is empty"
                ));
            }
            let named = set.contains(key)
                || WHY_GROUPS.contains(&key.as_str())
                || self.tolerance.contains_key(key)
                || self.relax.contains_key(key);
            if !named {
                return Err(format!(
                    "[why].{key} names no value of the case file: a [case] key that it sets, a [tolerance] key, a [relax] input, `{WHY_TOLERANCE}` (the whole [tolerance] table), or `{WHY_INPUTS}` (the input files)"
                ));
            }
            if text.trim().is_empty() {
                return Err(format!("[why].{key} is empty"));
            }
        }
        Ok(())
    }

    pub(crate) fn tier(&self) -> Tier {
        self.case.tier
    }

    pub(crate) fn status(&self) -> Status {
        self.case.status
    }

    pub(crate) fn timed(&self) -> bool {
        (self.case.claims.iter()).any(|c| [Claim::Speed, Claim::Optimization].contains(c))
    }

    pub(crate) fn claims(&self) -> &[Claim] {
        &self.case.claims
    }

    pub(crate) fn reference(&self) -> Reference {
        self.case.reference
    }

    pub(crate) fn rivals(&self) -> &[Rival] {
        &self.case.rivals
    }

    pub(crate) fn milestone(&self) -> Milestone {
        self.case.milestone
    }

    pub(crate) fn tolerance(&self) -> &BTreeMap<String, f64> {
        &self.tolerance
    }

    pub(crate) fn relax(&self) -> &BTreeMap<String, Relax> {
        &self.relax
    }

    pub(crate) fn why(&self) -> &BTreeMap<String, String> {
        &self.why
    }

    pub(crate) fn repeats(&self) -> u32 {
        let floor = self.case.tier.policy().repeats;
        self.case.repeats.unwrap_or(floor).max(floor)
    }

    pub(crate) fn timeout_s(&self) -> u64 {
        self.case
            .timeout_s
            .unwrap_or(self.case.tier.policy().timeout_s)
    }
}

pub(crate) fn active(root: &Path, tier: Tier) -> (Vec<String>, Vec<String>) {
    let ids = match ids(root) {
        Ok(ids) => ids,
        Err(e) => return (Vec::new(), vec![e]),
    };
    let (mut active, mut findings) = (Vec::new(), Vec::new());
    for id in ids {
        match Case::load(root, &id) {
            Ok(case) if tier.includes(case.tier()) && case.status() == Status::Active => {
                active.push(id);
            }
            Ok(_) => {}
            Err(e) => findings.push(e),
        }
    }
    (active, findings)
}

pub(crate) fn ids(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join("benchmarks");
    let entries = fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut ids: Vec<(u32, String)> = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("{}: {e}", dir.display()))?;
        let kind = entry
            .file_type()
            .map_err(|e| format!("{}: {e}", entry.path().display()))?;
        if !kind.is_dir()
            || entry
                .file_name()
                .to_str()
                .is_some_and(crate::lock::reserved)
        {
            continue;
        }
        let id = (entry.file_name().into_string())
            .map_err(|name| format!("benchmarks/{} is no UTF-8 name", name.to_string_lossy()))?;
        let number = parse_id(&id).map_err(|e| format!("benchmarks/{id}: {e}"))?;
        ids.push((number, id));
    }
    ids.sort();
    for pair in ids.windows(2) {
        if pair[0].0 == pair[1].0 {
            return Err(format!(
                "case number {} is used twice: {} and {}",
                pair[0].0, pair[0].1, pair[1].1
            ));
        }
    }
    Ok(ids.into_iter().map(|(_, id)| id).collect())
}

pub(crate) const INPUT_FILE: &str = "input.toml";
pub(crate) const INPUTS_DIR: &str = "inputs";
pub(crate) const CRYSTALS: &str = "crystals.json";

fn holds_inputs(dir: &Path) -> bool {
    let toml = |entry: &fs::DirEntry| {
        let path = entry.path();
        path.is_file() && path.extension().is_some_and(|e| e == "toml")
    };
    dir.join(INPUT_FILE).is_file()
        || dir.join(CRYSTALS).is_file()
        || fs::read_dir(dir.join(INPUTS_DIR))
            .is_ok_and(|entries| entries.flatten().any(|e| toml(&e)))
}

const WHY_INPUTS: &str = "inputs";
const WHY_TOLERANCE: &str = "tolerance";
const WHY_GROUPS: [&str; 2] = [WHY_INPUTS, WHY_TOLERANCE];

pub(crate) const RESERVED: [&str; 3] = ["wall_s", "speedup", RSS_EXPONENT];

pub(crate) fn is_reserved(key: &str) -> bool {
    reserved_metric(bounded(key).map_or(key, |(metric, _)| metric))
}

pub(crate) fn reserved_metric(name: &str) -> bool {
    RESERVED.contains(&name)
        || (SERIES.iter()).any(|prefix| {
            name.strip_prefix(prefix)
                .is_some_and(|code| code.starts_with('_'))
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Bound {
    Max,
    Min,
}

pub(crate) fn bounded(key: &str) -> Option<(&str, Bound)> {
    (key.strip_suffix("_max").map(|m| (m, Bound::Max)))
        .or_else(|| key.strip_suffix("_min").map(|m| (m, Bound::Min)))
}

pub(crate) fn parse_id(id: &str) -> Result<u32, String> {
    let (n, slug) = id.split_once('_').ok_or("must be <N>_<slug>")?;
    let slug_ok = !slug.is_empty()
        && slug
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_');
    let digits = !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    match n.parse::<u32>() {
        Ok(number) if digits && !n.starts_with('0') && slug_ok => Ok(number),
        _ => Err("must match [1-9][0-9]*_[a-z0-9_]+".into()),
    }
}
