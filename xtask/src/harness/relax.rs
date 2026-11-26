use super::engine::{self, Failure, Stream};
pub use crate::case::Relax;
use serde_json::Value;
use std::{path::Path, process::Command, time::Duration};

pub const FMAX_EV_ANGSTROM: f64 = 0.01;
pub const FMAX_HA_BOHR: f64 = FMAX_EV_ANGSTROM * 0.529_177_210_903 / 27.211_386_245_988;
pub const STEPS: u32 = 200;

const DRIVER: &str = r#"
import json, os, subprocess, sys, tomllib
import numpy as np
from ase import Atoms
from ase.calculators.socketio import SocketIOCalculator, actualunixsocketname
from ase.filters import FrechetCellFilter
from ase.optimize import BFGS
from ase.units import Bohr

engine, kind, fmax, steps = sys.argv[1], sys.argv[2], float(sys.argv[3]), int(sys.argv[4])
with open("input.toml", "rb") as f:
    structure = tomllib.load(f)["structure"]
atoms = Atoms(structure["species"], cell=np.array(structure["cell_bohr"]) * Bohr,
              scaled_positions=structure["positions_fractional"], pbc=True)

clients = []

def launch(atoms, properties, port, unixsocket):
    with open("outcomes", "w") as out:
        clients.append(subprocess.Popen([engine, "ipi", "--unix", actualunixsocketname(unixsocket), "input.toml"],
                                        stdout=out))
    return clients[-1]

with SocketIOCalculator(unixsocket=os.path.basename(os.getcwd()), launch_client=launch) as calc:
    atoms.calc = calc
    optimizer = BFGS(FrechetCellFilter(atoms) if kind == "cell" else atoms, logfile=None)
    converged = optimizer.run(fmax=fmax, steps=steps)
    server = calc.server
server.close()  # the socket closes, so orbitar ipi returns; close waits for it
if clients[0].wait() != 0:
    sys.exit(f"orbitar ipi exited with {clients[0].returncode}")
if not converged:
    sys.exit(f"BFGS did not reach fmax {fmax} eV/Å in {steps} steps")
with open("outcomes") as f:
    lines = f.read().splitlines()
print(json.dumps({
    "steps": optimizer.nsteps,
    "geometries": len(lines),
    "cell_bohr": (np.array(atoms.cell) / Bohr).tolist(),
    "positions_fractional": atoms.get_scaled_positions(wrap=False).tolist(),
    "warm": json.loads(lines[-1]),
}))
"#;

pub struct Relaxed {
    pub endpoint: toml::Table,
    pub warm: Value,
    pub steps: u64,
    pub geometries: u64,
    pub seconds: f64,
}

pub(crate) fn run(
    bin: &Path,
    input: &toml::Table,
    datasets: &[(String, Vec<u8>)],
    kind: Relax,
    timeout: Duration,
) -> Result<Relaxed, Failure> {
    let text = toml::to_string(input).map_err(|e| Failure::Harness(e.to_string()))?;
    let files: Vec<(String, &[u8])> = std::iter::once(("input.toml".to_owned(), text.as_bytes()))
        .chain(
            datasets
                .iter()
                .map(|(p, b)| (format!("datasets/{p}"), b.as_slice())),
        )
        .collect();
    let kind = match kind {
        Relax::Ions => "ions",
        Relax::Cell => "cell",
    };
    let build = |dir: &Path| {
        let copy = dir.join("orbitar");
        std::fs::copy(bin, &copy)
            .map_err(|e| Failure::Harness(format!("{}: {e}", bin.display())))?;
        let mut command = Command::new("python3");
        command.args(["-I", "-c", DRIVER]).arg(&copy).args([
            kind,
            &FMAX_EV_ANGSTROM.to_string(),
            &STEPS.to_string(),
        ]);
        if let Some(path) = std::env::var_os("PATH") {
            command.env("PATH", path);
        }
        Ok(command)
    };
    let run = engine::cold(&files, build, Stream::Stdout, |_| None, timeout)
        .map_err(|f| f.map(|e| format!("relax ({kind}): {e}")))?;
    let printed: Value = serde_json::from_str(&run.stdout)
        .map_err(|e| Failure::Harness(format!("relax ({kind}): stdout is not JSON: {e}")))?;
    let count = |key: &str| {
        printed[key]
            .as_u64()
            .ok_or_else(|| Failure::Harness(format!("relax ({kind}): no {key}")))
    };
    Ok(Relaxed {
        endpoint: endpoint(input, &printed).map_err(Failure::Harness)?,
        warm: printed["warm"].clone(),
        steps: count("steps")?,
        geometries: count("geometries")?,
        seconds: run.seconds,
    })
}

pub fn endpoint(input: &toml::Table, printed: &Value) -> Result<toml::Table, String> {
    let mut table = input.clone();
    for key in ["cell_bohr", "positions_fractional"] {
        let rows: toml::Value = (printed[key].as_array())
            .and_then(|rows| {
                (rows.iter())
                    .map(|row| {
                        (row.as_array()?.iter())
                            .map(|x| x.as_f64().map(toml::Value::from))
                            .collect::<Option<Vec<_>>>()
                            .filter(|row| row.len() == 3)
                            .map(toml::Value::from)
                    })
                    .collect::<Option<Vec<_>>>()
            })
            .ok_or_else(|| format!("relax: {key} must be rows of 3 numbers"))?
            .into();
        engine::set(&mut table, "structure", key, rows)?;
    }
    for key in ["forces", "stress"] {
        engine::set(&mut table, "outputs", key, true.into())?;
    }
    Ok(table)
}
