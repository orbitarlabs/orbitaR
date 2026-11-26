use super::run::read;
use super::spec::{atomic_number, symbol};
use crate::model::{Functional, inside};
use serde::Deserialize;
use serde_json::json;
use std::collections::BTreeMap;
use std::path::Path;

#[derive(Clone, Debug, PartialEq)]
pub struct Family {
    name: String,
    rows: Vec<Resolved>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Resolved {
    pub element: &'static str,
    pub functional: Functional,
    pub path: String,
    pub sha256: String,
    pub url: Option<String>,
    pub cutoff_hint_ha: Option<f64>,
}

#[derive(Deserialize)]
struct External {
    url: String,
    #[serde(flatten)]
    functionals: BTreeMap<String, Layout>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Layout {
    layout: String,
    element: BTreeMap<String, Pin>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    sha256: String,
    cutoff_hint_ha: Option<f64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ours {
    base: String,
    functional: String,
    element: BTreeMap<String, OwnRow>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnRow {
    file: String,
    sha256: String,
    cutoff_hint_ha: f64,
}

impl Family {
    pub fn read(directory: &Path, name: &str) -> Result<Family, String> {
        let at = |e: String| format!("datasets.family: {e}");
        let (text, path) = table(directory, name).map_err(at)?;
        Family::parse(name, &text, |base| {
            let (text, path) = table(directory, base)?;
            Family::parse(base, &text, |_| {
                Err("a base is an external family, not one of ours".into())
            })
            .map_err(|e| format!("{path}: {e}"))
        })
        .map_err(|e| at(format!("{path}: {e}")))
    }

    pub fn parse(
        name: &str,
        text: &str,
        base: impl FnOnce(&str) -> Result<Family, String>,
    ) -> Result<Family, String> {
        let table: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
        let rows = match table.contains_key("base") {
            false => external(table.try_into().map_err(|e: toml::de::Error| e.to_string()))?,
            true => ours(
                table.try_into().map_err(|e: toml::de::Error| e.to_string()),
                base,
            )?,
        };
        Ok(Family {
            name: name.to_owned(),
            rows,
        })
    }

    pub fn resolve(&self, functional: Functional, z: u8) -> Result<&Resolved, String> {
        let element = symbol(z);
        (self.rows.iter())
            .find(|row| row.functional == maker(functional) && row.element == element)
            .ok_or_else(|| {
                format!(
                    "datasets.family: {} holds no {} dataset for {element}; name a file in [datasets] files",
                    self.name,
                    word(functional)
                )
            })
    }

    pub fn rows(&self) -> &[Resolved] {
        &self.rows
    }

    pub fn json(&self) -> String {
        let rows: Vec<serde_json::Value> = (self.rows.iter())
            .map(|row| {
                let mut value = json!({
                    "element": row.element,
                    "functional": word(row.functional),
                    "path": row.path,
                    "sha256": row.sha256,
                });
                if let Some(url) = &row.url {
                    value["url"] = json!(url);
                }
                if let Some(hint) = row.cutoff_hint_ha {
                    value["cutoff_hint_ha"] = json!(hint);
                }
                value
            })
            .collect();
        json!(rows).to_string()
    }
}

fn maker(functional: Functional) -> Functional {
    let (kind, name) = functional.dataset();
    Functional::of_dataset(kind, name).expect("Functional::ALL holds the row of each dataset")
}

fn word(functional: Functional) -> &'static str {
    maker(functional).row().word
}

fn functional(key: &str) -> Result<Functional, String> {
    let functional = (Functional::ALL.iter())
        .find(|row| row.word == key)
        .map(|row| row.functional)
        .ok_or_else(|| format!("{key:?} is not a functional of xc.functional"))?;
    match maker(functional) == functional {
        true => Ok(functional),
        false => Err(format!(
            "{key:?} reads the datasets of {0}: key its files by {0:?}",
            word(functional)
        )),
    }
}

fn external(table: Result<External, String>) -> Result<Vec<Resolved>, String> {
    let table = table?;
    let mut rows = Vec::new();
    for (key, layout) in table.functionals {
        let functional = functional(&key)?;
        if !layout.layout.contains("{element}") {
            return Err(format!(
                "{key}.layout: {:?} has no {{element}}",
                layout.layout
            ));
        }
        for (element, pin) in layout.element {
            let path = layout.layout.replace("{element}", &element);
            rows.push(row(
                &element,
                functional,
                path.clone(),
                pin.sha256,
                Some(format!("{}/{path}", table.url)),
                pin.cutoff_hint_ha,
            )?);
        }
    }
    Ok(rows)
}

fn ours(
    table: Result<Ours, String>,
    base: impl FnOnce(&str) -> Result<Family, String>,
) -> Result<Vec<Resolved>, String> {
    let table = table?;
    let base = base(&table.base)?;
    let functional = functional(&table.functional)?;
    let mut rows: Vec<Resolved> = (base.rows.into_iter())
        .filter(|row| row.functional == functional && !table.element.contains_key(row.element))
        .collect();
    for (element, own) in table.element {
        rows.push(row(
            &element,
            functional,
            own.file,
            own.sha256,
            None,
            Some(own.cutoff_hint_ha),
        )?);
    }
    rows.sort_by_key(|row| row.element);
    Ok(rows)
}

fn row(
    element: &str,
    functional: Functional,
    path: String,
    sha256: String,
    url: Option<String>,
    cutoff_hint_ha: Option<f64>,
) -> Result<Resolved, String> {
    let at = format!("{}.element.{element}", word(functional));
    let z = atomic_number(&at, element)?;
    if !inside(&path) {
        return Err(format!(
            "{at}: {path:?} must be a relative path of plain names"
        ));
    }
    if sha256.len() != 64
        || !sha256
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
    {
        return Err(format!(
            "{at}: sha256 {sha256:?} is not 64 lowercase hex digits"
        ));
    }
    if cutoff_hint_ha.is_some_and(|h| !(h > 0.0 && h.is_finite())) {
        return Err(format!("{at}: cutoff_hint_ha must be positive"));
    }
    Ok(Resolved {
        element: symbol(z),
        functional,
        path,
        sha256,
        url,
        cutoff_hint_ha,
    })
}

pub(super) fn plain_name(name: &str) -> Result<(), String> {
    match inside(name) && !name.contains(['/', '\\']) {
        true => Ok(()),
        false => Err(format!(
            "{name:?} must be a file name: datasets.directory holds <family>.toml"
        )),
    }
}

fn table(directory: &Path, name: &str) -> Result<(String, String), String> {
    plain_name(name)?;
    let path = directory.join(format!("{name}.toml"));
    let text = read(&path).map_err(|e| {
        format!(
            "{e}; put the table {name}.toml there: datasets/{name}.toml of the orbitaR repository"
        )
    })?;
    Ok((text, path.display().to_string()))
}
