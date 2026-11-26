use super::{Cells, TIMEOUT, energy_run, fastest, line, over_cells, seconds};
use crate::{
    harness::rival::Rival,
    harness::rival::{self, Layout, Rivals},
};
use std::time::{Duration, Instant};

pub(super) fn search(rivals: &mut Rivals, code: Rival, cells: &Cells) -> Result<(), String> {
    let sweep = rivals.settings.threads.clone();
    let kpoints: Vec<usize> = cells.iter().map(|cell| cell.cell.mesh_points()).collect();
    let candidates = |p: usize| code.adapter().candidates(p, &kpoints);
    let total: usize = sweep.iter().map(|&p| candidates(p).len()).sum();
    let budget = rivals.settings.budget_s;
    for cell in cells {
        rivals.cutoff(code, &cell.cell)?;
    }
    let (mut best, mut tried, mut untried, mut failed_all) =
        (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    for p in sweep {
        let clock = Instant::now();
        let share = budget * candidates(p).len() as f64 / total as f64;
        let mut ran_all: Vec<(f64, Layout)> = Vec::new();
        let mut log = Vec::new();
        let mut first_failure = None;
        let mut note = |line: String| {
            println!("rivals: {} at {p} threads: {line}", code.adapter().name());
            log.push(line);
        };
        for layout in candidates(p) {
            let left = Duration::from_secs_f64(share).checked_sub(clock.elapsed());
            let Some(left) = left.filter(|d| !d.is_zero()) else {
                note(format!(
                    "{layout:?}: not tried, the share of {share} s is spent"
                ));
                untried.push(p);
                continue;
            };
            let timeout = TIMEOUT.min(left);
            let ran = over_cells(cells, |cell, jth| {
                energy_run(rivals, code, (cell, jth), &layout, None, timeout)
            });
            note(line(&layout, cells, &ran));
            match ran {
                Ok(runs) => ran_all.push((seconds(&runs), layout)),
                Err(e) => {
                    first_failure.get_or_insert(format!("{layout:?}: {e}"));
                }
            }
        }
        match fastest(ran_all) {
            Some((_, layout)) => best.push((p.to_string(), layout)),
            None => failed_all.extend(first_failure.map(|e| (p, e))),
        }
        tried.push((p.to_string(), log));
    }
    if let Some(p) = untried.first() {
        return Err(format!(
            "{} layouts stayed untried, the first at {p} threads; raise budget_s in {}",
            untried.len(),
            rival::SETTINGS
        ));
    }
    if let Some((p, e)) = failed_all.first() {
        return Err(format!(
            "every layout failed at {p} threads; the first: {e}"
        ));
    }
    let frozen = rivals.tuned.of_mut(code);
    frozen.layout = best.into_iter().collect();
    frozen.searched = tried.into_iter().collect();
    Ok(())
}
