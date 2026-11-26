use super::paw_xml::xml;
use super::run::{carbon, directory};
use crate::synthetic_draft;
use orbitar::io::{DatasetFiles, drive, read_spec};
use orbitar::ks::{Failure, run};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

#[expect(
    clippy::disallowed_types,
    reason = "the test plays the driver's end of the socket that README ## Relaxation names"
)]
type Socket = std::os::unix::net::UnixStream;

const SPECIES: &str =
    "species = [\"C\", \"C\"]\npositions_fractional = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.5]]";
const CUBE: [[f64; 3]; 3] = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]];

fn template(name: &str) -> (PathBuf, String) {
    let root = directory(name, &[("datasets/own/C.xml", xml(&synthetic_draft()))]);
    let text = carbon("\n[datasets]\nfiles = { C = \"own/C.xml\" }\n").replace(
        "species = [\"C\"]\npositions_fractional = [[0.1, 0.2, 0.3]]",
        SPECIES,
    );
    (root, text)
}

type Served = std::thread::JoinHandle<(Result<(), Failure>, Vec<String>)>;

fn engine(root: &Path, template: &str) -> (Socket, Served) {
    let (driver, served) = Socket::pair().unwrap();
    let patience = Some(std::time::Duration::from_secs(120));
    driver.set_read_timeout(patience).unwrap();
    served.set_read_timeout(patience).unwrap();
    let (root, template) = (root.to_path_buf(), template.to_owned());
    let handle = std::thread::spawn(move || {
        let mut lines = Vec::new();
        let result = drive(served, &template, &root, |line| lines.push(line.to_owned()));
        (result, lines)
    });
    (driver, handle)
}

fn send_word(s: &mut Socket, word: &str) {
    s.write_all(format!("{word:<12}").as_bytes()).unwrap();
}

fn recv_word(s: &mut Socket) -> String {
    let mut b = [0u8; 12];
    s.read_exact(&mut b).unwrap();
    String::from_utf8(b.to_vec()).unwrap().trim_end().to_owned()
}

fn recv_f64s(s: &mut Socket, n: usize) -> Vec<f64> {
    let mut b = vec![0u8; 8 * n];
    s.read_exact(&mut b).unwrap();
    b.as_chunks::<8>()
        .0
        .iter()
        .map(|&c| f64::from_le_bytes(c))
        .collect()
}

fn recv_i32(s: &mut Socket) -> i32 {
    let mut b = [0u8; 4];
    s.read_exact(&mut b).unwrap();
    i32::from_le_bytes(b)
}

fn send_geometry(s: &mut Socket, cell: [[f64; 3]; 3], fractional: &[[f64; 3]]) {
    send_word(s, "POSDATA");
    let mut out = Vec::new();
    for j in 0..3 {
        for row in &cell {
            out.extend(row[j].to_le_bytes());
        }
    }
    out.extend([0.0f64; 9].iter().flat_map(|v| v.to_le_bytes()));
    out.extend(i32::try_from(fractional.len()).unwrap().to_le_bytes());
    for f in fractional {
        let r = (0..3).map(|j| (0..3).map(|i| f[i] * cell[i][j]).sum::<f64>());
        out.extend(r.flat_map(f64::to_le_bytes));
    }
    s.write_all(&out).unwrap();
}

fn exchange(
    s: &mut Socket,
    cell: [[f64; 3]; 3],
    fractional: &[[f64; 3]],
) -> (f64, Vec<f64>, [[f64; 3]; 3]) {
    send_word(s, "STATUS");
    assert_eq!(recv_word(s), "READY");
    send_geometry(s, cell, fractional);
    send_word(s, "STATUS");
    assert_eq!(recv_word(s), "HAVEDATA");
    send_word(s, "GETFORCE");
    assert_eq!(recv_word(s), "FORCEREADY");
    let free = recv_f64s(s, 1)[0];
    let n = usize::try_from(recv_i32(s)).unwrap();
    let forces = recv_f64s(s, 3 * n);
    let sent = recv_f64s(s, 9);
    let virial = std::array::from_fn(|i| std::array::from_fn(|j| sent[3 * j + i]));
    assert_eq!(recv_i32(s), 1);
    let mut extra = [0u8; 1];
    s.read_exact(&mut extra).unwrap();
    (free, forces, virial)
}

fn calculate(s: &mut Socket, cell: [[f64; 3]; 3], fractional: &[[f64; 3]]) -> (f64, Vec<f64>) {
    let (free, forces, _) = exchange(s, cell, fractional);
    (free, forces)
}

fn cold(
    root: &Path,
    template: &str,
    cell: [[f64; 3]; 3],
    fractional: &[[f64; 3]],
) -> (f64, Vec<f64>) {
    let (free, forces, _) = cold_outcome(root, template, cell, fractional);
    (free, forces)
}

fn cold_outcome(
    root: &Path,
    template: &str,
    cell: [[f64; 3]; 3],
    fractional: &[[f64; 3]],
) -> (f64, Vec<f64>, Option<[[f64; 3]; 3]>) {
    let rows: Vec<String> = fractional.iter().map(|p| format!("{p:?}")).collect();
    let text = template
        .replace(
            SPECIES,
            &format!(
                "species = [\"C\", \"C\"]\npositions_fractional = [{}]",
                rows.join(", ")
            ),
        )
        .replace(
            "cell_bohr = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [0.0, 0.0, 6.0]]",
            &format!("cell_bohr = {cell:?}"),
        );
    let text = match text.contains("[outputs]\n") {
        true => text.replace("[outputs]\n", "[outputs]\nforces = true\n"),
        false => text + "\n[outputs]\nforces = true\n",
    };
    let spec = read_spec(&text).unwrap();
    let mut datasets = DatasetFiles::at(&spec, &root.join("datasets"));
    let outcome = run(&spec, |z| datasets.read(z), |_, _| {}).unwrap();
    let scf = outcome.scf().unwrap();
    let forces = scf
        .forces_ha_bohr
        .clone()
        .unwrap()
        .into_iter()
        .flatten()
        .collect();
    (scf.energies.free_ha(), forces, scf.stress_ha_bohr3)
}

fn largest_gap(a: &[f64], b: &[f64]) -> f64 {
    assert_eq!(a.len(), b.len());
    a.iter()
        .zip(b)
        .map(|(x, y)| (x - y).abs())
        .fold(0.0, f64::max)
}

#[test]
fn the_driver_gets_f_and_the_forces_of_each_geometry_and_the_engine_prints_its_outcome() {
    let (root, template) = template("ipi-the-driver-gets-f-and-the-forces-of-each");
    let (mut driver, handle) = engine(&root, &template);
    let first = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.5]];
    let moved = [[0.1, 0.2, 0.3], [0.61, 0.55, 0.5]];
    let got = [
        calculate(&mut driver, CUBE, &first),
        calculate(&mut driver, CUBE, &moved),
    ];
    send_word(&mut driver, "EXIT");
    let (result, lines) = handle.join().unwrap();
    assert_eq!(result, Ok(()));
    assert_eq!(lines.len(), 2);
    assert!(
        lines
            .iter()
            .all(|l| l.contains("\"forces_ha_bohr\":") && l.contains("\"scf_steps\":"))
    );
    let files =
        |l: &String| serde_json::from_str::<serde_json::Value>(l).unwrap()["datasets"].clone();
    assert!(files(&lines[0])["C"]["sha256"].is_string());
    assert_eq!(files(&lines[0]), files(&lines[1]));
    for ((free, forces), fractional) in got.iter().zip([first, moved]) {
        let (want_free, want_forces) = cold(&root, &template, CUBE, &fractional);
        assert!((free - want_free).abs() < 1.1e-11, "{free} {want_free}");
        assert!(
            largest_gap(forces, &want_forces) < 1e-9,
            "{forces:?} {want_forces:?}"
        );
    }
}

#[test]
fn a_skewed_cell_arrives_as_its_rows_and_its_positions_as_fractions() {
    let (root, template) = template("ipi-a-skewed-cell-arrives-as-its-rows-and-it");
    let (mut driver, handle) = engine(&root, &template);
    let skewed = [[6.0, 0.0, 0.0], [1.0, 5.5, 0.0], [0.5, 0.3, 6.2]];
    let fractional = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.5]];
    let (free, forces) = calculate(&mut driver, skewed, &fractional);
    drop(driver);
    let (result, _) = handle.join().unwrap();
    assert_eq!(
        result,
        Ok(()),
        "a socket that closes between messages ends the session"
    );
    let (want_free, want_forces) = cold(&root, &template, skewed, &fractional);
    assert!((free - want_free).abs() < 1.1e-11, "{free} {want_free}");
    assert!(largest_gap(&forces, &want_forces) < 1e-9);
}

#[test]
fn the_virial_is_minus_the_volume_times_the_stress_of_run() {
    let (root, template) = template("ipi-the-virial-is-minus-the-volume-times-the-st");
    let (mut driver, handle) = engine(&root, &template);
    let skewed = [[6.0, 0.0, 0.0], [1.0, 5.5, 0.0], [0.5, 0.3, 6.2]];
    let volume = 6.0 * 5.5 * 6.2;
    let first = [[0.1, 0.2, 0.3], [0.6, 0.55, 0.5]];
    let moved = [[0.1, 0.2, 0.3], [0.61, 0.55, 0.5]];
    let got = [
        exchange(&mut driver, skewed, &first),
        exchange(&mut driver, skewed, &moved),
    ];
    send_word(&mut driver, "EXIT");
    assert_eq!(handle.join().unwrap().0, Ok(()));
    for ((_, _, virial), fractional) in got.iter().zip([first, moved]) {
        let asked = template.clone() + "\n[outputs]\nstress = true\n";
        let (_, _, sigma) = cold_outcome(&root, &asked, skewed, &fractional);
        let sigma = sigma.expect("the template asks for stress");
        for i in 0..3 {
            for j in 0..3 {
                let want = -volume * sigma[i][j];
                assert!(
                    (virial[i][j] - want).abs() < 1e-9,
                    "[{i}][{j}]: {} against −Ω σ = {want}",
                    virial[i][j]
                );
            }
        }
    }
}

#[test]
fn a_geometry_of_another_atom_count_is_refused() {
    let (root, template) = template("ipi-another-atom-count");
    let (mut driver, handle) = engine(&root, &template);
    send_geometry(&mut driver, CUBE, &[[0.1, 0.2, 0.3]]);
    let (result, _) = handle.join().unwrap();
    assert_eq!(
        result,
        Err(Failure::Invalid(
            "structure.species: the template has 2 atoms, the driver sent 1".into()
        ))
    );
}

#[test]
fn forces_before_a_geometry_are_refused() {
    let (root, template) = template("ipi-forces-before-a-geometry");
    let (mut driver, handle) = engine(&root, &template);
    send_word(&mut driver, "GETFORCE");
    let (result, _) = handle.join().unwrap();
    assert_eq!(
        result,
        Err(Failure::Invalid("ipi: GETFORCE before POSDATA".into()))
    );
}

#[test]
fn a_singular_cell_is_refused() {
    let (root, template) = template("ipi-a-singular-cell-is-refused");
    let (mut driver, handle) = engine(&root, &template);
    let flat = [[6.0, 0.0, 0.0], [0.0, 6.0, 0.0], [6.0, 6.0, 0.0]];
    send_geometry(&mut driver, flat, &[[0.1, 0.2, 0.3], [0.6, 0.55, 0.5]]);
    let (result, _) = handle.join().unwrap();
    assert_eq!(
        result,
        Err(Failure::Invalid(
            "structure.cell_bohr: the three lattice vectors must be finite and span a volume"
                .into()
        ))
    );
}

struct Scripted {
    reads: std::collections::VecDeque<std::io::Result<Vec<u8>>>,
}

impl Read for Scripted {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.reads.pop_front() {
            None => Ok(0),
            Some(Err(e)) => Err(e),
            Some(Ok(bytes)) => {
                buf[..bytes.len()].copy_from_slice(&bytes);
                Ok(bytes.len())
            }
        }
    }
}

impl Write for Scripted {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        Ok(buf.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

fn scripted(name: &str, reads: Vec<std::io::Result<Vec<u8>>>) -> Result<(), Failure> {
    let (root, template) = template(name);
    let stream = Scripted {
        reads: reads.into(),
    };
    drive(stream, &template, &root, |_| {})
}

#[test]
fn an_interrupted_read_is_retried_and_any_other_read_error_ends_the_session() {
    use std::io::{Error, ErrorKind};
    let exit = format!("{:<12}", "EXIT").into_bytes();
    let interrupted = scripted(
        "ipi-interrupted",
        vec![Err(Error::from(ErrorKind::Interrupted)), Ok(exit)],
    );
    assert_eq!(interrupted, Ok(()));
    let reset = scripted(
        "ipi-reset",
        vec![Err(Error::from(ErrorKind::ConnectionReset))],
    );
    assert!(
        matches!(&reset, Err(Failure::Invalid(m)) if m.starts_with("ipi: ")),
        "{reset:?}"
    );
}

#[test]
fn an_atom_count_past_the_template_is_refused_before_its_positions_are_read() {
    let matrix: Vec<u8> = [1.0f64; 9].iter().flat_map(|v| v.to_le_bytes()).collect();
    let reads = vec![
        Ok(format!("{:<12}", "POSDATA").into_bytes()),
        Ok(matrix.clone()),
        Ok(matrix),
        Ok(i32::MAX.to_le_bytes().to_vec()),
    ];
    let result = scripted("ipi-an-atom-count-past-the-template", reads);
    assert_eq!(
        result,
        Err(Failure::Invalid(
            "structure.species: the template has 2 atoms, the driver sent 2147483647".into()
        ))
    );
}

#[test]
fn a_negative_atom_count_is_refused() {
    let matrix: Vec<u8> = [1.0f64; 9].iter().flat_map(|v| v.to_le_bytes()).collect();
    let reads = vec![
        Ok(format!("{:<12}", "POSDATA").into_bytes()),
        Ok(matrix.clone()),
        Ok(matrix),
        Ok((-1i32).to_le_bytes().to_vec()),
    ];
    let result = scripted("ipi-a-negative-atom-count", reads);
    assert_eq!(
        result,
        Err(Failure::Invalid(
            "ipi: POSDATA: a negative atom count".into()
        ))
    );
}

#[test]
fn a_socket_that_closes_inside_a_header_is_an_error() {
    let half = scripted("ipi-half", vec![Ok(b"STAT".to_vec())]);
    assert_eq!(
        half,
        Err(Failure::Invalid(
            "ipi: the socket closed inside a header".into()
        ))
    );
}
