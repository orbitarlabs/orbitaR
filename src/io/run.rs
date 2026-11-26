use super::{Family, read_dataset_for, spec::symbol};
use crate::ks::{Failure, Outcome, paw_probe, probe, run};
use crate::model::Spec;
use crate::paw::Dataset;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub fn run_file(path: &str, on_step: impl FnMut(usize, f64)) -> Result<String, Failure> {
    let text = read(Path::new(path)).map_err(Failure::Invalid)?;
    let spec = super::read_spec(&text).map_err(Failure::Invalid)?;
    let directory = (Path::new(path).parent())
        .unwrap_or(Path::new(""))
        .join(&spec.draft().dataset_directory);
    let mut datasets = DatasetFiles::at(&spec, &directory);
    let probed = match paw_probe(&spec, |z| datasets.read(z))? {
        Some(probed) => Some(probed),
        None => probe(&spec)?,
    };
    match probed {
        Some(probed) => Ok(datasets.named(super::probe_json(&probed))),
        None => run(&spec, |z| datasets.read(z), on_step).map(|outcome| datasets.json(&outcome)),
    }
}

pub struct DatasetFiles<'a> {
    spec: &'a Spec,
    directory: PathBuf,
    read: BTreeMap<&'static str, serde_json::Value>,
    family: Option<Family>,
}

impl<'a> DatasetFiles<'a> {
    pub fn at(spec: &'a Spec, directory: &Path) -> Self {
        Self {
            spec,
            directory: directory.to_owned(),
            read: BTreeMap::new(),
            family: None,
        }
    }

    pub fn read(&mut self, z: u8) -> Result<Dataset, Failure> {
        let element = symbol(z);
        let (key, relative, pinned) = match self.spec.draft().dataset_files.get(&z) {
            Some(path) => ("datasets.files", path.clone(), None),
            None => {
                if self.family.is_none() {
                    let read = Family::read(&self.directory, &self.spec.draft().family);
                    self.family = Some(read.map_err(Failure::Invalid)?);
                }
                let family = self.family.as_ref().expect("read above");
                let row = (family.resolve(self.spec.functional(), z)).map_err(Failure::Invalid)?;
                (
                    "datasets.directory",
                    row.path.clone(),
                    Some(row.sha256.clone()),
                )
            }
        };
        let path = self.directory.join(&relative);
        let family = &self.spec.draft().family;
        let text = read(&path).map_err(|e| {
            Failure::Invalid(match &pinned {
                None => format!("{key}: {e}"),
                Some(pin) => format!(
                    "{key}: {e}; put the file that {family} pins for {element} there (sha256 {pin}), or name your \
                     own file in [datasets] files"
                ),
            })
        })?;
        let sha256: String = (Sha256::digest(text.as_bytes()).iter())
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if let Some(pin) = pinned.filter(|pin| *pin != sha256) {
            return Err(Failure::Invalid(format!(
                "datasets.family: {}: sha256 {sha256}, but {family} pins {pin} for {element}; put the pinned file \
                 there, or name your own file in [datasets] files",
                path.display(),
            )));
        }
        let invalid = |e: String| Failure::Invalid(format!("{key}: {}: {e}", path.display()));
        let dataset = read_dataset_for(&text, self.spec.functional()).map_err(invalid)?;
        let held = dataset.draft().z;
        if held != f64::from(z) {
            return Err(invalid(format!("holds Z = {held}, not {element}")));
        }
        (self.read).insert(element, json!({ "path": relative, "sha256": sha256 }));
        Ok(dataset)
    }

    pub fn json(&self, outcome: &Outcome) -> String {
        self.named(super::outcome::fields(outcome).to_string())
    }

    fn named(&self, line: String) -> String {
        if self.read.is_empty() {
            return line;
        }
        let mut fields: serde_json::Value =
            serde_json::from_str(&line).expect("the engine's own JSON");
        fields["datasets"] = json!(self.read);
        fields.to_string()
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the engine reads its input file and its datasets here"
)]
pub(super) fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

#[expect(
    clippy::disallowed_types,
    reason = "README ## Relaxation: the driver reaches the engine over this one socket"
)]
pub(super) fn connect(socket: &str) -> Result<impl std::io::Read + std::io::Write, String> {
    std::os::unix::net::UnixStream::connect(socket).map_err(|e| format!("ipi --unix {socket}: {e}"))
}
