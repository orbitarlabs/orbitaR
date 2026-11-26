use super::{Cells, STOP_HA, TIMEOUT, TuningCell, failed};
use crate::{
    harness::rival::Rival,
    harness::rival::{Cell, Clocks, Mode, Rivals, Row, TUNED},
};

pub(super) fn calibrate(
    rivals: &mut Rivals,
    code: Rival,
    cells: &Cells,
    only: &[String],
) -> (bool, Vec<String>) {
    let target = &rivals.settings.target;
    let (scale, target) = (
        target.volume_scale,
        (target.energy_difference_ha_per_atom, target.pressure_gpa),
    );
    let mut frozen: Vec<(&str, &Cell, f64)> = Vec::new();
    let mut calibration = std::collections::BTreeMap::new();
    let mut failures = Vec::new();
    let mut kept_rows = std::collections::BTreeMap::new();
    for TuningCell { path: name, .. } in cells
        .iter()
        .filter(|cell| !only.is_empty() && !only.contains(&cell.path))
    {
        match kept(rivals, code, name) {
            Ok(rows) => _ = kept_rows.insert(name, rows),
            Err(e) => failures.push(e),
        }
    }
    if !failures.is_empty() {
        return (false, failures);
    }
    for TuningCell {
        path: name,
        cell,
        jth_files,
    } in cells
    {
        let rows = match kept_rows.remove(name) {
            Some(rows) => Ok(rows),
            None => measure(rivals, code, (name, cell, jth_files), scale),
        };
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                failures.push(e);
                continue;
            }
        };
        let meets = |r: &Row| meets(r, target);
        let from = rows.iter().rposition(|r| !meets(r)).map_or(0, |i| i + 1);
        match rows.get(from).filter(|_| from + 1 < rows.len()) {
            Some(row) => frozen.push((name, cell, row[0])),
            None => failures.push(format!(
                "{name}: no rung of the ladder meets the target with a rung above it that meets it too"
            )),
        }
        calibration.insert(name.clone(), rows);
    }
    let mut least: std::collections::BTreeMap<String, f64> = std::collections::BTreeMap::new();
    for &(_, cell, cutoff) in &frozen {
        for element in cell.elements() {
            let slot = least.entry(element.to_owned()).or_insert(cutoff);
            *slot = slot.min(cutoff);
        }
    }
    for &(name, cell, cutoff) in &frozen {
        let back = (cell.elements().iter())
            .map(|e| least[*e])
            .fold(0.0, f64::max);
        if back != cutoff {
            failures.push(format!(
                "{name} froze at {cutoff} Ha, but its elements give it back {back} Ha: another cell holds all of them \
                 at a lower rung; calibrate each element in a cell of its own"
            ));
            return (false, failures);
        }
    }
    if frozen.is_empty() {
        return (false, failures);
    }
    rivals.tuned.of_mut(code).calibration = calibration;
    rivals.tuned.of_mut(code).volume_scale = Some(scale);
    rivals.tuned.of_mut(code).cutoff_ha = least;
    (true, failures)
}

fn kept(rivals: &Rivals, code: Rival, name: &str) -> Result<Vec<Row>, String> {
    let rows = (rivals.tuned.of(code).calibration.get(name))
        .ok_or(format!("{name}: {TUNED} holds no rows of it; calibrate it"))?;
    let rungs: Vec<f64> = rows.iter().map(|r| r[0]).collect();
    match rungs == rivals.settings.setup(code).ladder_ha {
        true => {
            println!(
                "rivals: {} {name}: keeps its {} {TUNED} rows",
                code.adapter().name(),
                rows.len()
            );
            Ok(rows.clone())
        }
        false => Err(format!(
            "{name}: its {TUNED} rows hold rungs {rungs:?}, not today's ladder; calibrate it"
        )),
    }
}

struct Ran {
    e: f64,
    p: Option<f64>,
    moments: Option<Vec<f64>>,
    seconds: f64,
}

fn measure(
    rivals: &Rivals,
    code: Rival,
    (name, cell, jth): (&str, &Cell, &[(String, Vec<u8>)]),
    scale: f64,
) -> Result<Vec<Row>, String> {
    let setup = rivals.settings.setup(code);
    let (ladder, converged, ratio) = (&setup.ladder_ha, setup.converged_ha, setup.density_ratio);
    let layout = code.calibration_layout(cell);
    let (compressed, v2) = (cell.scaled(scale), format!("{scale} V₁"));
    let frozen = &rivals.tuned.of(code).mixing.input;
    let other: Vec<String> = (code.adapter().mixings().into_iter())
        .chain([Vec::new()])
        .find(|m| m != frozen)
        .unwrap_or_default();
    let one = |cell: &Cell,
               cutoff_ha: f64,
               mode: Mode,
               mixing: Option<&[String]>|
     -> Result<Ran, String> {
        let mut job = rivals.tune_job(code, cell, cutoff_ha, layout.clone(), jth)?;
        if let Some(mixing) = mixing {
            job.mixing = mixing.to_vec();
        }
        let run =
            (rivals.run(&job, jth, STOP_HA, mode, Clocks::Warning, TIMEOUT)).map_err(failed)?;
        let e = (run.steps.last())
            .ok_or(format!("{name} at {cutoff_ha} Ha: no step lines"))?
            .free_energy_ha;
        Ok(Ran {
            e,
            p: code.pressure(&run.stdout),
            moments: code.adapter().moments(&run.stdout),
            seconds: run.seconds,
        })
    };
    let settled = |(cell, volume): (&Cell, &str),
                   cutoff_ha: f64,
                   mode: Mode,
                   state: Option<&[f64]>|
     -> Result<Ran, String> {
        let ran = one(cell, cutoff_ha, mode, None)?;
        let Some(state) = state.filter(|s| !cell.same_state(ran.moments.as_deref(), s)) else {
            return Ok(ran);
        };
        let first = ran.moments;
        println!(
            "rivals: {} {name} at {volume}, {cutoff_ha} Ha: moments {first:?} μB, not the converged run's {state:?}: \
             another magnetic state, so once more on {other:?}",
            code.adapter().name()
        );
        let again = one(cell, cutoff_ha, mode, Some(&other))?;
        match cell.same_state(again.moments.as_deref(), state) {
            true => {
                println!(
                    "rivals: {} {name} at {volume}, {cutoff_ha} Ha on {other:?}: moments {:?} μB, the converged run's \
                     state",
                    code.adapter().name(),
                    again.moments
                );
                Ok(again)
            }
            false => Err(format!(
                "moments {first:?} μB on the frozen mixing {frozen:?} and {:?} on {other:?}, not the converged run's \
                 {state:?}",
                again.moments
            )),
        }
    };
    let at = |cutoff_ha: f64, state: Option<&[f64]>| -> Result<(Ran, f64), String> {
        let ran = settled((cell, "V₁"), cutoff_ha, Mode::Stress, state)?;
        let p = ran
            .p
            .ok_or(format!("{name} at {cutoff_ha} Ha: no pressure line"))?;
        Ok((ran, p))
    };
    let at_v2 = |cutoff_ha: f64, state: Option<&[f64]>| -> Result<Ran, String> {
        settled((&compressed, &v2), cutoff_ha, Mode::Energy, state)
            .map_err(|e| format!("{name} at {scale} V₁, {cutoff_ha} Ha: {e}"))
    };
    let state = |ran: &Ran| -> Result<Vec<f64>, String> {
        let moments = ran.moments.clone().unwrap_or_default();
        match cell.same_state(Some(&moments), &moments) {
            true => Ok(moments),
            false => Err(format!(
                "{name} at {converged} Ha: moments {:?} μB, no moment on an atom that the cell starts magnetic",
                ran.moments
            )),
        }
    };
    let converged_v2 = at_v2(converged, None);
    let runs = (at(converged, None).and_then(|v1| Ok((v1, converged_v2?))))
        .and_then(|((c1, p0), c2)| Ok(((state(&c1)?, state(&c2)?), (c1, p0, c2))));
    let ((s1, s2), (c1, p0, c2)) = match runs {
        Ok(runs) => runs,
        Err(e) => {
            println!(
                "rivals: {} {name} at {converged} Ha: {e}",
                code.adapter().name()
            );
            return Err(e);
        }
    };
    let (e0, s0) = ((c1.e, c2.e), c1.seconds);
    println!(
        "rivals: {} {name} at {converged} Ha on {layout:?}: E(V₁) {} Ha, E({scale} V₁) {} Ha, P {p0} GPa in \
         {s0:.0} s at V₁; its {} rungs below take about {:.0} s at V₁, and as many runs at V₂",
        code.adapter().name(),
        e0.0,
        e0.1,
        ladder.len(),
        ladder.len() as f64 * s0
    );
    let atoms = cell.atoms() as f64;
    let mut rows = Vec::new();
    for &cutoff in ladder {
        let say = |e: &str| {
            println!(
                "rivals: {} {name} at {cutoff} Ha: {e}",
                code.adapter().name()
            )
        };
        let (e1, p) = match at(cutoff, Some(&s1)) {
            Ok((ran, p)) => (ran.e, p),
            Err(e) => {
                say(&e);
                (f64::NAN, f64::NAN)
            }
        };
        let e2 = at_v2(cutoff, Some(&s2)).map_or_else(
            |e| {
                say(&e);
                f64::NAN
            },
            |ran| ran.e,
        );
        let row = row(cutoff, [e1, e2], p, (e0, p0), atoms);
        println!(
            "rivals: {} {name}: [cutoff Ha, |Δ(E₁ − E₂)| Ha per atom, |ΔP| GPa, |ΔE₁| Ha per atom, P GPa] = {row:?}, \
             density cutoff {} Ha",
            code.adapter().name(),
            cutoff * ratio
        );
        rows.push(row);
    }
    Ok(rows)
}

fn row(
    cutoff: f64,
    [e1, e2]: [f64; 2],
    p: f64,
    ((c1, c2), cp): ((f64, f64), f64),
    atoms: f64,
) -> Row {
    [
        cutoff,
        ((e1 - e2) - (c1 - c2)).abs() / atoms,
        (p - cp).abs(),
        (e1 - c1).abs() / atoms,
        p,
    ]
}

fn meets(r: &Row, (energy, pressure): (f64, f64)) -> bool {
    r[1] <= energy && r[2] <= pressure
}
