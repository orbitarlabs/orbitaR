use super::{Adapter, Cell, Clock, Job, Layout, Mode, Smearing, attribute, jth_file, rows};
use crate::harness::engine;
use std::{fmt::Write as _, path::Path, process::Command};

pub(super) struct Abinit;

impl Adapter for Abinit {
    fn name(&self) -> &'static str {
        "abinit"
    }

    fn command(&self) -> Command {
        Command::new("abinit")
    }

    fn input_file(&self) -> (&'static str, &'static [&'static str]) {
        ("run.abi", &["run.abi"])
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
        let elements = cell.elements();
        let mut znucl = Vec::new();
        let mut paths = Vec::new();
        for element in &elements {
            let (_, path, bytes) = (datasets.iter().find(|(e, _, _)| e == element))
                .ok_or_else(|| format!("rivals: no ABINIT dataset for {element}"))?;
            znucl.push(
                attribute(bytes, "atom", "Z")
                    .ok_or_else(|| format!("rivals: {path}: no Z"))?
                    .to_string(),
            );
            paths.push(path.as_str());
        }
        let typat: Vec<String> = (cell.species.iter())
            .map(|s| (elements.iter().position(|e| e == s).unwrap_or(0) + 1).to_string())
            .collect();
        let mut s = String::from("acell 3*1.0\nrprim\n");
        rows(&mut s, &cell.cell);
        let _ = writeln!(
            s,
            "natom {}\nntypat {}\nznucl {}\ntypat {}\nxred",
            cell.atoms(),
            elements.len(),
            znucl.join(" "),
            typat.join(" ")
        );
        rows(&mut s, &cell.positions);
        let _ = writeln!(s, "pp_dirpath \"./\"\npseudos \"{}\"", paths.join(", "));
        let _ = writeln!(s, "ecut {cutoff_ha:?}\npawecutdg {:?}", cutoff_ha * ratio);
        let [a, b, c] = cell.mesh;
        let [i, j, k] = cell.shifted.map(|h| if h { 0.5 } else { 0.0 });
        let _ = writeln!(s, "ngkpt {a} {b} {c}\nnshiftk 1\nshiftk {i:?} {j:?} {k:?}");
        s.push_str(if cell.symmetry {
            "kptopt 1\n"
        } else {
            "kptopt 3\nnsym 1\n"
        });
        match &cell.smearing {
            None => s.push_str("occopt 1\n"),
            Some((smearing, width)) => {
                let occopt = match smearing {
                    Smearing::FermiDirac => 3,
                    Smearing::Gaussian => 7,
                    Smearing::MethfesselPaxton => 6,
                    Smearing::MarzariVanderbilt => 4,
                };
                let _ = writeln!(s, "occopt {occopt}\ntsmear {width:?}");
            }
        }
        if let Some(n) = cell.bands {
            let _ = writeln!(s, "nband {n}");
        }
        if let Some(moments) = &cell.moments {
            s.push_str("nsppol 2\nspinat\n");
            let spins: Vec<[f64; 3]> = moments.iter().map(|&m| [0.0, 0.0, m]).collect();
            rows(&mut s, &spins);
        }
        let stress = i32::from(mode == Mode::Stress);
        let nstep = cell.steps.unwrap_or(500);
        let _ = writeln!(
            s,
            "toldfe {stop_ha:?}\nnstep {nstep}\noptstress {stress}\nchkprim 0\nchksymbreak 0\n\
             prtwf 0\nprtden 0\nprteig 0\nprtgsr 0\nprtebands 0"
        );
        for line in job.mixing.iter().chain(&job.layout.input) {
            let _ = writeln!(s, "{line}");
        }
        Ok(s)
    }

    fn step(&self) -> engine::StepLine {
        |line| match line.split_whitespace().collect::<Vec<_>>()[..] {
            ["ETOT", _, e, ..] => e.parse().ok(),
            _ => None,
        }
    }

    fn clock(&self, line: &str) -> Option<Clock> {
        match line.strip_prefix("Proc.   0 individual time (sec):") {
            Some(rest) => rest
                .split_once("wall=")?
                .1
                .trim()
                .parse()
                .ok()
                .map(Clock::Run),
            None if line.starts_with("{SCF_istep:") => {
                let (_, rest) = line.split_once("wall_time: '")?;
                rest.split_whitespace()
                    .next()?
                    .parse()
                    .ok()
                    .map(Clock::Step)
            }
            None => None,
        }
    }

    fn unconverged(&self, line: &str) -> bool {
        line.contains("was not enough SCF cycles to converge")
    }

    fn pressure(&self, line: &str) -> Option<f64> {
        super::number(line.split_once("Pressure=")?.1)
    }

    fn moments(&self, stdout: &str) -> Option<Vec<f64>> {
        let (_, table) = stdout
            .rsplit_once("Integrated electronic and magnetization densities in atomic spheres")?;
        let rows = (table.lines())
            .skip_while(|l| !l.trim_start().starts_with("Atom"))
            .skip(1)
            .take_while(|l| !l.trim_start().starts_with('-') && !l.trim().is_empty());
        rows.map(|l| l.split_whitespace().nth(5)?.parse().ok())
            .collect()
    }

    fn valence(&self, bytes: &[u8]) -> Option<f64> {
        attribute(bytes, "atom", "valence")
    }

    fn datasets(
        &self,
        _root: &Path,
        _prefix: &Path,
        cell: &Cell,
        jth: &[(String, Vec<u8>)],
    ) -> Result<Vec<(String, String, Vec<u8>)>, String> {
        (cell.elements().into_iter())
            .map(|element| {
                let (path, bytes) = jth_file(jth, element)?;
                Ok((element.to_owned(), path.clone(), bytes.clone()))
            })
            .collect()
    }

    fn calibration_layout(&self, cell: &Cell, cores: usize) -> Layout {
        Layout::new(cores.min(cell.k_spins()), 1, Vec::new(), Vec::new())
    }

    fn candidates(&self, p: usize, _kpoints: &[usize]) -> Vec<Layout> {
        let mut out = Vec::new();
        for ranks in (1..=p).filter(|&d| p.is_multiple_of(d)) {
            out.push(Layout::new(ranks, p / ranks, Vec::new(), Vec::new()));
            if ranks > 1 {
                let npband = format!("npband {ranks}");
                let auto: &[&str] = &["paral_kgb 1", "autoparal 1", "istwfk *1"];
                let bands: &[&str] = &["paral_kgb 1", "np_spkpt 1", &npband, "npfft 1"];
                for kgb in [auto, bands] {
                    let input = kgb.iter().map(|&line| line.to_owned()).collect();
                    out.push(Layout::new(ranks, p / ranks, Vec::new(), input));
                }
            }
        }
        out
    }

    fn mixings(&self) -> Vec<Vec<String>> {
        vec![vec!["diemac 10.0".to_owned(), "diemix 0.3".to_owned()]]
    }

    fn fitted(
        &self,
        mut layout: Layout,
        cell: &Cell,
        jth: &[(String, Vec<u8>)],
    ) -> Result<Layout, String> {
        let split = (layout.input.iter())
            .find_map(|line| line.strip_prefix("npband ")?.parse::<usize>().ok());
        let Some(split) = split else {
            return Ok(layout);
        };
        if let Some(n) = cell.bands {
            return match n.is_multiple_of(split) {
                true => Ok(layout),
                false => Err(format!(
                    "rivals: the case's {n} bands do not split over npband {split}"
                )),
            };
        }
        let electrons = cell
            .jth_electrons(jth)
            .map_err(|e| format!("rivals: {e}"))?;
        let moment: f64 = cell.moments.iter().flatten().sum();
        let fband = if cell.smearing.is_some() { 0.5 } else { 0.125 };
        let own =
            (electrons / 2.0 + moment.abs() / 2.0 + fband * cell.atoms() as f64).ceil() as usize;
        layout
            .input
            .push(format!("nband {}", own.div_ceil(split) * split));
        Ok(layout)
    }
}
