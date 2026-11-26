use super::{
    Cells, Failure, TIMEOUT, TuningCell, each_cell, energy_run, fastest, line, over_cells, seconds,
};
use crate::harness::{
    engine::Run,
    rival::{self, Mixing, Rival, Rivals},
};

pub(super) fn search(rivals: &mut Rivals, code: Rival, cells: &Cells) -> Result<(), String> {
    if cells.is_empty() {
        return Err(format!(
            "no mixing cells; name the tuning cells in {}'s mixing_cells",
            rival::SETTINGS
        ));
    }
    let same_state = rivals.settings.target.energy_difference_ha_per_atom;
    let none: Vec<String> = Vec::new();
    let base = each_cell(cells, |cell, jth| run(rivals, code, &none, (cell, jth)));
    let failed: Vec<&String> = base.iter().filter_map(|r| r.as_ref().err()).collect();
    let defaults: Result<Vec<(f64, f64)>, String> = match failed.is_empty() {
        true => Ok(base.iter().flatten().copied().collect()),
        false => Err((failed.iter().map(|e| e.as_str()))
            .collect::<Vec<_>>()
            .join("; ")),
    };
    let mut searched = vec![line(&none, cells, &defaults)];
    println!("rivals: {} mixing: {}", code.adapter().name(), searched[0]);
    if failed.len() == cells.len() {
        return Err(format!(
            "the defaults converged on no mixing cell, so no state can check a candidate: {}",
            searched[0]
        ));
    }
    let mut ran_all = Vec::new();
    if let Ok(runs) = &defaults {
        ran_all.push((seconds(runs), none.clone()));
    }
    for mixing in code.adapter().mixings() {
        let mut ran = over_cells(cells, |cell, jth| run(rivals, code, &mixing, (cell, jth)));
        if let Ok(runs) = &ran {
            let off = (cells.iter().zip(runs.iter().zip(&base))).find_map(
                |(
                    TuningCell {
                        path: name, cell, ..
                    },
                    ((_, e), b),
                )| {
                    let (_, e0) = b.as_ref().ok()?;
                    let per_atom = (e - e0).abs() / cell.atoms() as f64;
                    (per_atom > same_state).then_some((name, per_atom))
                },
            );
            if let Some((name, per_atom)) = off {
                ran = Err(format!(
                    "{name}: another state than the defaults': {per_atom:e} Ha per atom off, past {same_state:e} Ha per atom"
                ));
            }
        }
        let text = line(&mixing, cells, &ran);
        println!("rivals: {} mixing: {text}", code.adapter().name());
        searched.push(text);
        if let Ok(runs) = ran {
            ran_all.push((seconds(&runs), mixing));
        }
    }
    let Some((s, input)) = fastest(ran_all) else {
        return Err(format!(
            "every mixing failed; the defaults: {}",
            searched[0]
        ));
    };
    let reason = match (input.is_empty(), &defaults) {
        (true, _) => format!("the defaults were fastest: {s:.3} s over the mixing cells"),
        (false, Ok(runs)) => format!(
            "fastest: {s:.3} s over the mixing cells, against {:.3} s on the defaults, at their state",
            seconds(runs)
        ),
        (false, Err(e)) => format!(
            "fastest: {s:.3} s over the mixing cells, at the defaults' state where they converged, unchecked where they failed: {e}"
        ),
    };
    rivals.tuned.of_mut(code).mixing = Mixing {
        input,
        reason,
        searched,
    };
    Ok(())
}

fn run(
    rivals: &Rivals,
    code: Rival,
    mixing: &[String],
    (cell, jth): (&rival::Cell, &[(String, Vec<u8>)]),
) -> Result<Run, Failure> {
    let layout = code.calibration_layout(cell);
    energy_run(rivals, code, (cell, jth), &layout, Some(mixing), TIMEOUT)
}
