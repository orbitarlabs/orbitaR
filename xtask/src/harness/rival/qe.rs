use super::{Adapter, Cell, Clock, Job, Layout, Mode, Rival, Smearing, attribute, rows};
use crate::harness::{datasets, engine};
use serde::Deserialize;
use std::{collections::BTreeMap, fmt::Write as _, path::Path, process::Command};

const RY_PER_HA: f64 = 2.0;
const DIAGONALIZATION_GROUPS: [usize; 4] = [1, 4, 9, 16];

pub(super) struct Qe;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pins {
    #[serde(rename = "archive")]
    _archive: String,
    #[serde(rename = "archive_sha256")]
    _archive_sha256: String,
    directory: String,
    files: BTreeMap<String, Pin>,
    #[serde(default, rename = "built")]
    _built: toml::Table,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pin {
    file: String,
    sha256: String,
}

impl Adapter for Qe {
    fn name(&self) -> &'static str {
        "qe"
    }

    fn command(&self) -> Command {
        Command::new("pw.x")
    }

    fn input_file(&self) -> (&'static str, &'static [&'static str]) {
        ("pw.in", &["-in", "pw.in"])
    }

    fn input(
        &self,
        job: &Job,
        ratio: f64,
        stop_ha: f64,
        mode: Mode,
        datasets: &[(String, String, Vec<u8>)],
    ) -> Result<String, String> {
        let (cell, cutoff_ha) = (&job.cell, job.cutoff_ha);
        let (types, of) = cell.types();
        let file = |element: &str| datasets.iter().find(|(e, _, _)| e == element);
        let mut s = String::new();
        let stress = if mode == Mode::Stress {
            ".true."
        } else {
            ".false."
        };
        let _ = writeln!(
            s,
            "&control\n  calculation = 'scf'\n  disk_io = 'none'\n  pseudo_dir = './'\n  outdir = './'\n  tstress = {stress}\n/"
        );
        let _ = writeln!(
            s,
            "&system\n  ibrav = 0\n  nat = {}\n  ntyp = {}\n  ecutwfc = {:?}\n  ecutrho = {:?}",
            cell.atoms(),
            types.len(),
            RY_PER_HA * cutoff_ha,
            RY_PER_HA * cutoff_ha * ratio
        );
        match &cell.smearing {
            None => s.push_str("  occupations = 'fixed'\n"),
            Some((smearing, width)) => {
                let kind = match smearing {
                    Smearing::FermiDirac => "fd",
                    Smearing::Gaussian => "gaussian",
                    Smearing::MethfesselPaxton => "mp",
                    Smearing::MarzariVanderbilt => "mv",
                };
                let _ = writeln!(
                    s,
                    "  occupations = 'smearing'\n  smearing = '{kind}'\n  degauss = {:?}",
                    RY_PER_HA * width
                );
            }
        }
        if let Some(n) = cell.bands {
            let _ = writeln!(s, "  nbnd = {n}");
        }
        if !cell.symmetry {
            s.push_str("  nosym = .true.\n  noinv = .true.\n");
        }
        if cell.moments.is_some() {
            s.push_str("  nspin = 2\n");
            for (i, (_, element, m)) in types.iter().enumerate() {
                let (_, _, bytes) =
                    file(element).ok_or_else(|| format!("rivals: no QE dataset for {element}"))?;
                let valence = valence(bytes)
                    .ok_or_else(|| format!("rivals: {element}: no valence in its UPF header"))?;
                let _ = writeln!(
                    s,
                    "  starting_magnetization({}) = {:?}",
                    i + 1,
                    (m / valence).clamp(-1.0, 1.0)
                );
            }
        }
        let _ = writeln!(
            s,
            "/\n&electrons\n  conv_thr = {:?}\n  electron_maxstep = {}\n  startingpot = 'atomic'\n  startingwfc = 'atomic+random'",
            RY_PER_HA * stop_ha,
            cell.steps.unwrap_or(100)
        );
        for line in &job.mixing {
            let _ = writeln!(s, "  {line}");
        }
        s.push_str("/\n");
        s.push_str("ATOMIC_SPECIES\n");
        for (label, element, _) in &types {
            let (_, name, _) =
                file(element).ok_or_else(|| format!("rivals: no QE dataset for {element}"))?;
            let _ = writeln!(s, "  {label} 1.0 {name}");
        }
        s.push_str("CELL_PARAMETERS bohr\n");
        rows(&mut s, &cell.cell);
        s.push_str("ATOMIC_POSITIONS crystal\n");
        for (p, &t) in cell.positions.iter().zip(&of) {
            let [x, y, z] = p;
            let _ = writeln!(s, "  {} {x:?} {y:?} {z:?}", types[t].0);
        }
        if cell.mesh == [1, 1, 1] && cell.shifted == [false; 3] {
            s.push_str("K_POINTS gamma\n");
        } else {
            let [a, b, c] = cell.mesh;
            let [i, j, k] = cell.shifted.map(i32::from);
            let _ = writeln!(s, "K_POINTS automatic\n  {a} {b} {c} {i} {j} {k}");
        }
        Ok(s)
    }

    fn step(&self) -> engine::StepLine {
        |line| {
            let rest = line
                .trim_start_matches([' ', '!'])
                .strip_prefix("total energy")?;
            let ry = rest
                .trim_start()
                .strip_prefix('=')?
                .trim()
                .strip_suffix("Ry")?;
            ry.trim().parse::<f64>().ok().map(|e| e / RY_PER_HA)
        }
    }

    fn clock(&self, line: &str) -> Option<Clock> {
        match line.strip_prefix("total cpu time spent up to now is") {
            Some(rest) => rest
                .trim()
                .strip_suffix("secs")?
                .trim()
                .parse()
                .ok()
                .map(Clock::Since),
            None => {
                let (_, times) = line.strip_prefix("PWSCF")?.split_once(':')?;
                duration(times.split_once("CPU")?.1.trim().strip_suffix("WALL")?).map(Clock::Run)
            }
        }
    }

    fn unconverged(&self, line: &str) -> bool {
        line.contains("convergence NOT achieved")
    }

    fn pressure(&self, line: &str) -> Option<f64> {
        super::number(line.split_once("P=")?.1).map(|kbar| kbar / 10.0)
    }

    fn moments(&self, stdout: &str) -> Option<Vec<f64>> {
        let (_, block) = stdout.rsplit_once("Magnetic moment per site")?;
        let rows = (block.lines().skip(1)).take_while(|l| !l.trim().is_empty());
        rows.map(|l| {
            l.split_once("magn=")?
                .1
                .split_whitespace()
                .next()?
                .parse()
                .ok()
        })
        .collect()
    }

    fn valence(&self, bytes: &[u8]) -> Option<f64> {
        valence(bytes)
    }

    fn datasets(
        &self,
        root: &Path,
        prefix: &Path,
        cell: &Cell,
        _jth: &[(String, Vec<u8>)],
    ) -> Result<Vec<(String, String, Vec<u8>)>, String> {
        let pins: Pins = (datasets::rival_table(root, Rival::Qe)?.try_into())
            .map_err(|e| format!("{} [qe]: {e}", datasets::PATH))?;
        (cell.elements().into_iter())
            .map(|element| {
                let pin = (pins.files.get(element))
                    .ok_or_else(|| format!("{} [qe] pins no file for {element}", datasets::PATH))?;
                let path = prefix.join(&pins.directory).join(&pin.file);
                let bytes = datasets::checked(&path, &pin.sha256)?;
                Ok((element.to_owned(), pin.file.clone(), bytes))
            })
            .collect()
    }

    fn calibration_layout(&self, cell: &Cell, cores: usize) -> Layout {
        let pairs = cell.k_spins();
        let pools = (1..=cores)
            .filter(|&n| cores.is_multiple_of(n) && pairs.is_multiple_of(n))
            .max()
            .unwrap_or(1);
        Layout::new(
            cores,
            1,
            vec!["-nk".to_owned(), pools.to_string()],
            Vec::new(),
        )
    }

    fn mixings(&self) -> Vec<Vec<String>> {
        vec![vec!["mixing_beta = 0.3".to_owned()]]
    }

    fn candidates(&self, p: usize, kpoints: &[usize]) -> Vec<Layout> {
        let divisors = |n: usize| (1..=n).filter(move |&d| n.is_multiple_of(d));
        let mut out = Vec::new();
        for ranks in divisors(p) {
            for pools in divisors(ranks).filter(|&n| kpoints.iter().all(|k| k.is_multiple_of(n))) {
                for nd in DIAGONALIZATION_GROUPS
                    .into_iter()
                    .filter(|&d| d <= ranks / pools)
                {
                    let flags = [("-nk", pools), ("-nd", nd)]
                        .into_iter()
                        .filter(|&(_, n)| n > 1)
                        .flat_map(|(f, n)| [f.to_owned(), n.to_string()])
                        .collect();
                    out.push(Layout::new(ranks, p / ranks, flags, Vec::new()));
                }
            }
        }
        out
    }
}

fn duration(text: &str) -> Option<f64> {
    let mut total = 0.0;
    let mut number = String::new();
    for c in text.chars().filter(|c| !c.is_whitespace()) {
        let unit = match c {
            'h' => 3600.0,
            'm' => 60.0,
            's' => 1.0,
            _ => {
                number.push(c);
                continue;
            }
        };
        total += unit * number.parse::<f64>().ok()?;
        number.clear();
    }
    number.is_empty().then_some(total)
}

pub(super) fn valence(bytes: &[u8]) -> Option<f64> {
    attribute(bytes, "PP_HEADER", "z_valence").or_else(|| {
        let text = std::str::from_utf8(bytes).ok()?;
        let line = text.lines().find(|l| l.trim_end().ends_with("Z valence"))?;
        line.split_whitespace().next()?.parse().ok()
    })
}
