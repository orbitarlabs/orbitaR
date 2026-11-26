use crate::{
    Env, Findings,
    harness::rival::Rival,
    harness::{
        datasets,
        engine::{self, Failure},
        rival::{self, Cell, Clocks, Mode, Rivals},
    },
};
use std::{fs, path::Path, time::Duration};

mod calibrate;
mod layout;
mod mixing;

const STOP_HA: f64 = 1e-9;
const TIMEOUT: Duration = Duration::from_secs(3_600);

#[derive(Clone, Copy, PartialEq, clap::ValueEnum)]
pub enum Job {
    #[value(help = "Each element's cutoff, from RIVALS.toml's calibration cells")]
    Calibrate,
    #[value(help = "Each code's SCF mixing, from RIVALS.toml's mixing cells (tuning cells only)")]
    Mixing,
    #[value(help = "Each thread count's layout, from RIVALS.toml's layout cells")]
    Layout,
}

#[derive(Clone)]
struct TuningCell {
    path: String,
    cell: Cell,
    jth_files: Vec<(String, Vec<u8>)>,
}

type Cells = Vec<TuningCell>;

pub(crate) fn run(root: &Path, job: Job, codes: &[Rival], only: &[String], env: &Env) -> Findings {
    let chosen = match codes {
        [] => Rival::all(),
        named => named,
    };
    if !only.is_empty() && job != Job::Calibrate {
        return vec![
            "rivals: --cell scopes calibrate alone; mixing and layout judge every cell".into(),
        ];
    }
    let Some(prefix) = env.rivals.as_deref() else {
        return vec!["rivals: set ORBITAR_RIVALS to the directory of the rival installs".into()];
    };
    let mut rivals = match Rivals::load(root, prefix) {
        Ok(rivals) => rivals,
        Err(e) => return vec![format!("rivals: {e}")],
    };
    let paths = match job {
        Job::Layout => rivals.settings.layout_cells.clone(),
        Job::Mixing => rivals.settings.mixing_cells.clone(),
        Job::Calibrate => rivals.settings.calibration_cells.clone(),
    };
    if let Some(cell) = only.iter().find(|cell| !paths.contains(cell)) {
        return vec![format!(
            "rivals: --cell {cell}: not one of {}'s calibration_cells",
            rival::SETTINGS
        )];
    }
    let cells = match cells(root, &paths) {
        Ok(cells) => cells,
        Err(e) => return vec![format!("rivals: {e}")],
    };
    let mut findings = Vec::new();
    let mut tuned_any = false;
    for &code in chosen {
        let cells: Cells = (cells.iter())
            .filter(|cell| rivals.settings.skips(code, &cell.path).is_none())
            .cloned()
            .collect();
        let (wrote, failures) = match job {
            Job::Calibrate => calibrate::calibrate(&mut rivals, code, &cells, only),
            Job::Mixing => frozen(mixing::search(&mut rivals, code, &cells)),
            Job::Layout => frozen(layout::search(&mut rivals, code, &cells)),
        };
        tuned_any |= wrote;
        findings.extend(
            failures
                .into_iter()
                .map(|e| format!("rivals: {}: {e}", code.adapter().name())),
        );
    }
    if !tuned_any {
        return findings;
    }
    let settings = &rivals.settings;
    rivals.tuned.tuning_set = (settings.calibration_cells.iter())
        .chain(&settings.layout_cells)
        .chain(&settings.mixing_cells)
        .cloned()
        .collect();
    let text = match toml::to_string(&rivals.tuned) {
        Ok(text) => format!(
            "# Written by `cargo xtask rivals` on the node; frozen before any timed run.\n{text}"
        ),
        Err(e) => return vec![format!("rivals: {e}")],
    };
    print!("{text}");
    if let Err(e) = fs::write(root.join(rival::TUNED), &text) {
        findings.push(format!("{}: {e}", rival::TUNED));
    }
    findings
}

fn energy_run(
    rivals: &Rivals,
    code: Rival,
    (cell, jth): (&Cell, &[(String, Vec<u8>)]),
    layout: &rival::Layout,
    mixing: Option<&[String]>,
    timeout: Duration,
) -> Result<engine::Run, Failure> {
    let mut job = (rivals.cutoff(code, cell))
        .and_then(|cutoff| rivals.tune_job(code, cell, cutoff, layout.clone(), jth))
        .map_err(Failure::Harness)?;
    if let Some(mixing) = mixing {
        job.mixing = mixing.to_vec();
    }
    let fitted = job.layout.added_to(layout);
    (rivals.run(&job, jth, STOP_HA, Mode::Energy, Clocks::Loss, timeout)).map_err(|f| match fitted
        .is_empty()
    {
        true => f,
        false => f.map(|e| format!("with {}: {e}", fitted.join(", "))),
    })
}

fn on_cell(
    TuningCell {
        path,
        cell,
        jth_files,
    }: &TuningCell,
    run: &mut impl FnMut(&Cell, &[(String, Vec<u8>)]) -> Result<engine::Run, Failure>,
) -> Result<(f64, f64), String> {
    let done = run(cell, jth_files).map_err(|f| format!("{path}: {}", failed(f)))?;
    let last = (done.steps.last()).ok_or_else(|| format!("{path}: no step lines"))?;
    Ok((done.seconds, last.free_energy_ha))
}

fn over_cells(
    cells: &Cells,
    mut run: impl FnMut(&Cell, &[(String, Vec<u8>)]) -> Result<engine::Run, Failure>,
) -> Result<Vec<(f64, f64)>, String> {
    cells.iter().map(|cell| on_cell(cell, &mut run)).collect()
}

fn each_cell(
    cells: &Cells,
    mut run: impl FnMut(&Cell, &[(String, Vec<u8>)]) -> Result<engine::Run, Failure>,
) -> Vec<Result<(f64, f64), String>> {
    cells.iter().map(|cell| on_cell(cell, &mut run)).collect()
}

fn seconds(runs: &[(f64, f64)]) -> f64 {
    runs.iter().map(|(s, _)| s).sum()
}

fn line(
    candidate: &impl std::fmt::Debug,
    cells: &Cells,
    ran: &Result<Vec<(f64, f64)>, String>,
) -> String {
    match ran {
        Ok(runs) => {
            let each: Vec<String> = (cells.iter().zip(runs))
                .map(|(cell, (s, _))| format!("{} {s:.3} s", cell.path))
                .collect();
            format!(
                "{candidate:?}: {:.3} s ({})",
                seconds(runs),
                each.join(", ")
            )
        }
        Err(e) => format!("{candidate:?}: failed: {e}"),
    }
}

fn fastest<T>(ran: Vec<(f64, T)>) -> Option<(f64, T)> {
    ran.into_iter().min_by(|a, b| a.0.total_cmp(&b.0))
}

fn frozen(done: Result<(), String>) -> (bool, Vec<String>) {
    (done.is_ok(), done.err().into_iter().collect())
}

fn cells(root: &Path, paths: &[String]) -> Result<Cells, String> {
    (paths.iter())
        .map(|path| {
            let text = fs::read_to_string(root.join(path)).map_err(|e| format!("{path}: {e}"))?;
            let table: toml::Table = toml::from_str(&text).map_err(|e| format!("{path}: {e}"))?;
            let cell = Cell::parse(&table)?;
            let species: Vec<String> = cell.elements().into_iter().map(str::to_owned).collect();
            Ok(TuningCell {
                path: path.clone(),
                cell,
                jth_files: datasets::for_species(root, datasets::DEFAULT_FAMILY, &species)?,
            })
        })
        .collect()
}

fn failed(f: Failure) -> String {
    match f {
        Failure::Harness(e) | Failure::Engine(e) => e,
    }
}
