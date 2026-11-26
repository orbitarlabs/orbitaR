use crate::model::Functional;
use crate::paw::{Channel, CoreKinetic, Dataset, DatasetDraft, RadialGrid, Shape};
use crate::xc::Variable;
use roxmltree::{Document, Node};
use std::{f64::consts::PI, str::FromStr};

pub fn read_dataset(text: &str) -> Result<Dataset, String> {
    read(text, None)
}

pub fn read_dataset_for(text: &str, requested: Functional) -> Result<Dataset, String> {
    read(text, Some(requested))
}

fn read(text: &str, requested: Option<Functional>) -> Result<Dataset, String> {
    let document = Document::parse(text).map_err(|e| format!("PAW-XML: {e}"))?;
    let root = document.root_element();
    let all = |tag: &'static str| root.children().filter(move |n| n.has_tag_name(tag));
    let one = |tag: &'static str| {
        all(tag)
            .next()
            .ok_or_else(|| format!("PAW-XML: no <{tag}>"))
    };
    let grid_node = one("radial_grid")?;
    if grid_node.attribute("eq") != Some("r=a*(exp(d*i)-1)") || all("radial_grid").count() > 1 {
        return Err("radial_grid: read one grid with eq=\"r=a*(exp(d*i)-1)\"".into());
    }
    let grid_id = grid_node.attribute("id").ok_or("radial_grid: no id")?;
    if number::<u32>(grid_node, "istart")? != 0 {
        return Err("radial_grid: istart must be 0".into());
    }
    let values = |node: Node| -> Result<Vec<f64>, String> {
        if node.attribute("grid") != Some(grid_id) {
            return Err(format!(
                "{}: must use grid {grid_id:?}",
                node.tag_name().name()
            ));
        }
        numbers(node)
    };
    let spherical = |tag: &'static str| -> Result<Vec<f64>, String> {
        let scale = (4.0 * PI).sqrt();
        Ok(values(one(tag)?)?.into_iter().map(|x| x / scale).collect())
    };
    let core_density = spherical("ae_core_density")?;
    if Ok(Some(core_density.len()))
        != number::<usize>(grid_node, "iend").map(|end| end.checked_add(1))
    {
        return Err("radial_grid: iend must be the last index of each radial part".into());
    }
    let grid = RadialGrid::logarithmic(
        number(grid_node, "a")?,
        number(grid_node, "d")?,
        core_density.len(),
    )?;
    let wave = |tag: &'static str, state: &str| -> Result<Vec<f64>, String> {
        let node = all(tag)
            .find(|n| n.attribute("state").map(str::trim) == Some(state))
            .ok_or_else(|| format!("{tag}: none for state {state:?}"))?;
        values(node)
    };
    let mut channels = Vec::new();
    for state in one("valence_states")?
        .children()
        .filter(|n| n.has_tag_name("state"))
    {
        let id = state.attribute("id").map(str::trim).unwrap_or_default();
        channels.push(Channel {
            l: number(state, "l")?,
            occupation: match state.has_attribute("f") {
                true => Some(number(state, "f")?),
                false => None,
            },
            ae: wave("ae_partial_wave", id)?,
            pseudo: wave("pseudo_partial_wave", id)?,
            projector: wave("projector_function", id)?,
        });
    }
    let xc = one("xc_functional")?;
    let (kind, name) = (xc.attribute("type"), xc.attribute("name"));
    let functional =
        Functional::of_dataset(kind.unwrap_or("?"), name.unwrap_or("?")).ok_or_else(|| {
            let mut known: Vec<String> = Vec::new();
            for (kind, name) in Functional::ALL.map(|row| row.dataset) {
                let header = format!("{kind} {name}");
                if !known.contains(&header) {
                    known.push(header);
                }
            }
            format!(
                "xc_functional: {} {} is none of {}",
                kind.unwrap_or("?"),
                name.unwrap_or("?"),
                known.join(", ")
            )
        })?;
    let requested = requested.unwrap_or(functional);
    let shape_node = one("shape_function")?;
    let radius = number(shape_node, "rc")?;
    let shape = match shape_node.attribute("type") {
        Some("bessel") => Shape::Bessel { radius },
        Some("sinc") => Shape::Sinc { radius },
        other => {
            return Err(format!(
                "shape_function: type {other:?} is neither bessel nor sinc"
            ));
        }
    };
    let core_kinetic = match requested.rung().reads(Variable::TauUp) {
        true => {
            let tagged = |tag: &'static str| {
                all(tag).next().ok_or_else(|| {
                    format!("PAW-XML: no <{tag}>, which {} reads", requested.row().word)
                })?;
                spherical(tag)
            };
            Some(CoreKinetic::checked(
                &grid,
                tagged("ae_core_kinetic_energy_density")?,
                tagged("pseudo_core_kinetic_energy_density")?,
            )?)
        }
        false => None,
    };
    let atom = one("atom")?;
    let pseudo_core_density = spherical("pseudo_core_density")?;
    Dataset::try_from(DatasetDraft {
        z: number(atom, "Z")?,
        core_electrons: number(atom, "core")?,
        valence_electrons: number(atom, "valence")?,
        functional,
        grid,
        sphere_radius: number(one("paw_radius")?, "rc")?,
        shape,
        channels,
        kinetic_differences: numbers(one("kinetic_energy_differences")?)?,
        core_density,
        pseudo_core_density,
        pseudo_valence_density: spherical("pseudo_valence_density")?,
        local_potential: spherical("blochl_local_ionic_potential")?,
        core_kinetic,
    })
}

fn number<T: FromStr>(node: Node, key: &str) -> Result<T, String> {
    let tag = node.tag_name().name();
    let raw = node
        .attribute(key)
        .ok_or_else(|| format!("{tag}: no {key}"))?;
    raw.trim()
        .parse()
        .map_err(|_| format!("{tag}: {key} {raw:?} does not parse"))
}

fn numbers(node: Node) -> Result<Vec<f64>, String> {
    let tag = node.tag_name().name();
    node.text()
        .unwrap_or_default()
        .split_whitespace()
        .map(|x| {
            x.parse()
                .map_err(|_| format!("{tag}: {x:?} is not a number"))
        })
        .collect()
}
