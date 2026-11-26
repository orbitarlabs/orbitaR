use super::{DatasetFiles, read_spec};
use crate::ks::{Failure, Session};
use crate::model::{Cell, Spec};
use std::f64::consts::TAU;
use std::io::{Read, Write};
use std::path::Path;

const HEADER: usize = 12;

pub fn serve(path: &str, socket: &str, on_outcome: impl FnMut(&str)) -> Result<(), Failure> {
    let template = super::run::read(Path::new(path)).map_err(Failure::Invalid)?;
    let stream = super::run::connect(socket).map_err(Failure::Invalid)?;
    let directory = Path::new(path).parent().unwrap_or(Path::new(""));
    drive(stream, &template, directory, on_outcome)
}

pub fn drive(
    mut stream: impl Read + Write,
    template: &str,
    directory: &Path,
    mut on_outcome: impl FnMut(&str),
) -> Result<(), Failure> {
    let template = read_spec(template).map_err(Failure::Invalid)?;
    let mut session = Session::default();
    let mut answer: Option<Answer> = None;
    loop {
        let Some(word) = header(&mut stream)? else {
            return Ok(());
        };
        match word.as_str() {
            "STATUS" => send_word(
                &mut stream,
                if answer.is_some() {
                    "HAVEDATA"
                } else {
                    "READY"
                },
            )?,
            "POSDATA" => {
                let cell = transposed(read_f64s(&mut stream, 9)?);
                let _inverse = read_f64s(&mut stream, 9)?;
                let atoms = usize::try_from(read_i32(&mut stream)?)
                    .map_err(|_| protocol("POSDATA: a negative atom count"))?;
                count(&template, atoms)?;
                let positions = read_f64s(&mut stream, 3 * atoms)?;
                let spec = geometry(&template, cell, &positions)?;
                let datasets = directory.join(&spec.draft().dataset_directory);
                let mut files = DatasetFiles::at(&spec, &datasets);
                let outcome = session.run(&spec, |z| files.read(z), |_, _| {})?;
                on_outcome(&files.json(&outcome));
                let scf = outcome.scf().expect("a geometry has atoms");
                let forces = scf
                    .forces_ha_bohr
                    .clone()
                    .expect("each geometry asks for forces");
                let sigma = scf.stress_ha_bohr3.expect("each geometry asks for stress");
                let volume = spec.draft().cell.volume();
                answer = Some(Answer {
                    free_ha: scf.energies.free_ha(),
                    forces,
                    virial: sigma.map(|row| row.map(|s| -volume * s)),
                });
            }
            "GETFORCE" => {
                let Answer {
                    free_ha,
                    forces,
                    virial,
                } = answer
                    .take()
                    .ok_or_else(|| protocol("GETFORCE before POSDATA"))?;
                send_word(&mut stream, "FORCEREADY")?;
                let count = i32::try_from(forces.len()).map_err(|_| protocol("too many atoms"))?;
                let mut out = free_ha.to_le_bytes().to_vec();
                out.extend(count.to_le_bytes());
                out.extend(forces.iter().flatten().flat_map(|f| f.to_le_bytes()));
                let columns = (0..3).flat_map(|j| (0..3).map(move |i| (i, j)));
                out.extend(columns.flat_map(|(i, j)| virial[i][j].to_le_bytes()));
                out.extend(1i32.to_le_bytes());
                out.push(0);
                write(&mut stream, &out)?;
            }
            "EXIT" => return Ok(()),
            other => return Err(protocol(&format!("unknown message {other:?}"))),
        }
    }
}

struct Answer {
    free_ha: f64,
    forces: Vec<[f64; 3]>,
    virial: [[f64; 3]; 3],
}

fn geometry(template: &Spec, rows: [[f64; 3]; 3], positions: &[f64]) -> Result<Spec, Failure> {
    let mut draft = template.draft().clone();
    let cell = Cell::try_from(rows).map_err(Failure::Invalid)?;
    let b = cell.reciprocal();
    for ((_, f), r) in draft.atoms.iter_mut().zip(positions.as_chunks::<3>().0) {
        *f = b.map(|bi| (0..3).map(|j| r[j] * bi[j]).sum::<f64>() / TAU);
    }
    draft.cell = cell;
    draft.forces = true;
    draft.stress = true;
    Spec::try_from(draft).map_err(Failure::Invalid)
}

fn count(template: &Spec, atoms: usize) -> Result<(), Failure> {
    let want = template.draft().atoms.len();
    if atoms == want {
        return Ok(());
    }
    Err(Failure::Invalid(format!(
        "structure.species: the template has {want} atoms, the driver sent {atoms}"
    )))
}

fn transposed(m: Vec<f64>) -> [[f64; 3]; 3] {
    std::array::from_fn(|i| std::array::from_fn(|j| m[3 * j + i]))
}

fn protocol(message: &str) -> Failure {
    Failure::Invalid(format!("ipi: {message}"))
}

fn header(stream: &mut impl Read) -> Result<Option<String>, Failure> {
    let mut bytes = [0u8; HEADER];
    let mut filled = 0;
    while filled < HEADER {
        match stream.read(&mut bytes[filled..]) {
            Ok(0) if filled == 0 => return Ok(None),
            Ok(0) => return Err(protocol("the socket closed inside a header")),
            Ok(n) => filled += n,
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
            Err(e) => return Err(protocol(&e.to_string())),
        }
    }
    let word = std::str::from_utf8(&bytes).map_err(|_| protocol("a header that is not ASCII"))?;
    Ok(Some(word.trim_end().to_owned()))
}

fn read_exact(stream: &mut impl Read, n: usize) -> Result<Vec<u8>, Failure> {
    let mut bytes = vec![0u8; n];
    stream
        .read_exact(&mut bytes)
        .map_err(|e| protocol(&e.to_string()))?;
    Ok(bytes)
}

fn read_f64s(stream: &mut impl Read, n: usize) -> Result<Vec<f64>, Failure> {
    let bytes = read_exact(stream, 8 * n)?;
    let values: Vec<f64> = (bytes.as_chunks::<8>().0.iter())
        .map(|&b| f64::from_le_bytes(b))
        .collect();
    if values.iter().all(|v| v.is_finite()) {
        Ok(values)
    } else {
        Err(protocol("POSDATA: a number that is not finite"))
    }
}

fn read_i32(stream: &mut impl Read) -> Result<i32, Failure> {
    let bytes = read_exact(stream, 4)?;
    Ok(i32::from_le_bytes(bytes.try_into().expect("4 bytes")))
}

fn send_word(stream: &mut impl Write, word: &str) -> Result<(), Failure> {
    write(stream, format!("{word:<HEADER$}").as_bytes())
}

fn write(stream: &mut impl Write, bytes: &[u8]) -> Result<(), Failure> {
    stream
        .write_all(bytes)
        .and_then(|()| stream.flush())
        .map_err(|e| protocol(&e.to_string()))
}
