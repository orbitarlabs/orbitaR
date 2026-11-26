use crate::synthetic_draft;
use orbitar::io::{read_dataset, read_dataset_for};
use orbitar::model::Functional;
use orbitar::paw::{CoreKinetic, DatasetDraft, Shape};
use std::fmt::Write as _;

const ROOT: &str = "paw_dataset";

pub(crate) fn xml(draft: &DatasetDraft) -> String {
    let y = (4.0 * std::f64::consts::PI).sqrt();
    let list = |f: &[f64], scale: f64| {
        f.iter()
            .map(|x| format!("{:.17e}", x * scale))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let mut s = format!("<?xml version=\"1.0\"?>\n<{ROOT} version=\"0.7\">\n");
    let _ = writeln!(
        s,
        "<atom symbol=\"C\" Z=\"{:.2}\" core=\"{:.2}\" valence=\"{:.2}\"/>",
        draft.z, draft.core_electrons, draft.valence_electrons
    );
    let _ = writeln!(s, "<xc_functional type=\"GGA\" name=\"PBE\"/>");
    let _ = writeln!(s, "<paw_radius rc=\" {}\"/>", draft.sphere_radius);
    s.push_str("<valence_states>\n");
    for (i, c) in draft.channels.iter().enumerate() {
        let _ = writeln!(
            s,
            "  <state n=\" 2\" l=\"{}\" f=\" {:.1}\" rc=\" 1.5\" e=\"-0.5\" id= \"C{}\"/>",
            c.l,
            c.electrons(),
            i + 1
        );
    }
    s.push_str("</valence_states>\n");
    let _ = writeln!(
        s,
        "<radial_grid eq=\"r=a*(exp(d*i)-1)\" a=\" 1e-3\" d=\" 0.01\" istart=\"0\" iend=\" {}\" id=\"log1\">\n</radial_grid>",
        draft.grid.radii().len() - 1
    );
    let radius = draft.shape.radius();
    let _ = writeln!(s, "<shape_function type=\"bessel\" rc=\" {radius}\"/>");
    for (tag, f) in [
        ("ae_core_density", &draft.core_density),
        ("pseudo_core_density", &draft.pseudo_core_density),
        ("pseudo_valence_density", &draft.pseudo_valence_density),
        ("zero_potential", &draft.pseudo_valence_density),
        ("blochl_local_ionic_potential", &draft.local_potential),
    ] {
        let _ = writeln!(s, "<{tag} grid=\"log1\">\n{}\n</{tag}>", list(f, y));
    }
    if let Some(kinetic) = &draft.core_kinetic {
        for (tag, f) in [
            ("ae_core_kinetic_energy_density", kinetic.ae()),
            ("pseudo_core_kinetic_energy_density", kinetic.pseudo()),
        ] {
            let _ = writeln!(s, "<{tag} grid=\"log1\">\n{}\n</{tag}>", list(f, y));
        }
    }
    for (i, c) in draft.channels.iter().enumerate().rev() {
        let _ = writeln!(
            s,
            "<projector_function state= \"C{}\" grid=\"log1\">\n{}\n</projector_function>",
            i + 1,
            list(&c.projector, 1.0)
        );
    }
    for (i, c) in draft.channels.iter().enumerate().rev() {
        let _ = writeln!(
            s,
            "<ae_partial_wave state= \"C{}\" grid=\"log1\">\n{}\n</ae_partial_wave>",
            i + 1,
            list(&c.ae, 1.0)
        );
        let _ = writeln!(
            s,
            "<pseudo_partial_wave state= \"C{}\" grid=\"log1\">\n{}\n</pseudo_partial_wave>",
            i + 1,
            list(&c.pseudo, 1.0)
        );
    }
    let _ = writeln!(
        s,
        "<kinetic_energy_differences>\n{}\n</kinetic_energy_differences>",
        list(&draft.kinetic_differences, 1.0)
    );
    let _ = writeln!(s, "</{ROOT}>");
    s
}

fn close(a: &[f64], b: &[f64]) -> bool {
    a.len() == b.len()
        && a.iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= 1e-15 * x.abs().max(y.abs()))
}

#[test]
fn a_file_reads_back_with_densities_and_potential_divided_by_the_y00_factor() {
    let want = synthetic_draft();
    let dataset = read_dataset(&xml(&want)).unwrap();
    let got = dataset.draft();
    assert_eq!(
        (got.z, got.core_electrons, got.valence_electrons),
        (6.0, 2.0, 4.0)
    );
    assert_eq!(got.functional, Functional::Pbe);
    assert_eq!(got.sphere_radius, 1.5);
    assert_eq!(got.shape, Shape::Bessel { radius: 1.2 });
    assert_eq!(got.grid.radii().len(), 1001);
    for (a, b) in [
        (&got.core_density, &want.core_density),
        (&got.pseudo_core_density, &want.pseudo_core_density),
        (&got.pseudo_valence_density, &want.pseudo_valence_density),
        (&got.local_potential, &want.local_potential),
        (&got.kinetic_differences, &want.kinetic_differences),
    ] {
        assert!(close(a, b));
    }
    assert_eq!(got.channels.iter().map(|c| c.l).collect::<Vec<_>>(), [0, 1]);
    for (a, b) in got.channels.iter().zip(&want.channels) {
        assert!(
            close(&a.ae, &b.ae) && close(&a.pseudo, &b.pseudo) && close(&a.projector, &b.projector)
        );
    }
}

#[test]
fn a_pseudo_core_is_kept_at_the_threshold_and_dropped_below_it() {
    for (peak, kept) in [(9.9e-7, false), (1e-6, true)] {
        let mut draft = synthetic_draft();
        let core = &mut draft.pseudo_core_density;
        let (top, largest) = (core.iter().enumerate()).fold((0, 0.0_f64), |(i, m), (j, x)| {
            if x.abs() > m { (j, x.abs()) } else { (i, m) }
        });
        core.iter_mut().for_each(|x| *x *= peak / largest);
        core[top] = peak;
        let got = read_dataset(&xml(&draft)).unwrap();
        let core = &got.draft().pseudo_core_density;
        assert_eq!(core.iter().any(|&x| x != 0.0), kept, "{peak}");
        if kept {
            assert_eq!(core[top], peak, "the file holds the peak exactly");
        }
    }
}

fn with_core_kinetic() -> DatasetDraft {
    let draft = synthetic_draft();
    let times = |f: &[f64], k: f64| f.iter().map(|x| k * x).collect::<Vec<f64>>();
    let kinetic = CoreKinetic::checked(
        &draft.grid,
        times(&draft.core_density, 3.0),
        times(&draft.pseudo_core_density, 2.0),
    )
    .unwrap();
    DatasetDraft {
        core_kinetic: Some(kinetic),
        ..draft
    }
}

#[test]
fn a_meta_gga_reads_both_core_kinetic_energy_densities_divided_by_the_y00_factor() {
    let want = with_core_kinetic();
    let got = read_dataset_for(&xml(&want), Functional::R2scan).unwrap();
    let got = got.draft().core_kinetic.clone().unwrap();
    let want = want.core_kinetic.unwrap();
    assert!(close(got.ae(), want.ae()) && close(got.pseudo(), want.pseudo()));
}

#[test]
fn lda_and_pbe_never_read_the_core_kinetic_energy_densities() {
    let text = xml(&with_core_kinetic()).replace(
        "<pseudo_core_kinetic_energy_density grid=\"log1\">",
        "<pseudo_core_kinetic_energy_density grid=\"log2\">",
    );
    for functional in [Functional::Lda, Functional::Pbe] {
        let read = read_dataset_for(&text, functional).unwrap();
        assert_eq!(read.draft().core_kinetic, None);
    }
    assert_eq!(read_dataset(&text).unwrap().draft().core_kinetic, None);
    assert_eq!(
        read_dataset_for(&text, Functional::R2scan).unwrap_err(),
        "pseudo_core_kinetic_energy_density: must use grid \"log1\""
    );
}

#[test]
fn a_meta_gga_stops_at_a_missing_core_kinetic_energy_density_by_its_tag() {
    let full = xml(&with_core_kinetic());
    for tag in [
        "ae_core_kinetic_energy_density",
        "pseudo_core_kinetic_energy_density",
    ] {
        let start = full.find(&format!("<{tag} ")).unwrap();
        let end = full.find(&format!("</{tag}>")).unwrap() + tag.len() + 3;
        let text = format!("{}{}", &full[..start], &full[end..]);
        assert_eq!(
            read_dataset_for(&text, Functional::R2scan).unwrap_err(),
            format!("PAW-XML: no <{tag}>, which r2scan reads")
        );
    }
}

#[test]
fn lda_files_read_as_lda() {
    let text =
        xml(&synthetic_draft()).replace("type=\"GGA\" name=\"PBE\"", "type=\"LDA\" name=\"PW\"");
    assert_eq!(
        read_dataset(&text).unwrap().draft().functional,
        Functional::Lda
    );
}

#[test]
fn sinc_shapes_read_with_their_radius() {
    let text = xml(&synthetic_draft()).replace("type=\"bessel\"", "type=\"sinc\"");
    assert_eq!(
        read_dataset(&text).unwrap().draft().shape,
        Shape::Sinc { radius: 1.2 }
    );
}

#[test]
fn each_unreadable_part_names_its_element() {
    let text = xml(&synthetic_draft());
    let cases = [
        ("version=\"0.7\">", "version=\"0.7\"><broken", "PAW-XML: "),
        (
            "name=\"PBE\"",
            "name=\"PW91\"",
            "xc_functional: GGA PW91 is none of LDA PW, GGA PBE",
        ),
        (
            "type=\"bessel\"",
            "type=\"gauss\"",
            "shape_function: type Some(\"gauss\") is neither bessel nor sinc",
        ),
        (
            "eq=\"r=a*(exp(d*i)-1)\"",
            "eq=\"r=a*exp(d*i)\"",
            "radial_grid: read one grid with eq=\"r=a*(exp(d*i)-1)\"",
        ),
        (
            "istart=\"0\"",
            "istart=\"1\"",
            "radial_grid: istart must be 0",
        ),
        (
            "istart=\"0\"",
            "istart=\"-1\"",
            "radial_grid: istart \"-1\" does not parse",
        ),
        (
            "iend=\" 1000\"",
            "iend=\" 999\"",
            "radial_grid: iend must be the last index of each radial part",
        ),
        (
            "iend=\" 1000\"",
            "iend=\" 1000.5\"",
            "radial_grid: iend must be the last index of each radial part",
        ),
        (
            "iend=\" 1000\"",
            "iend=\" 1e30\"",
            "radial_grid: iend must be the last index of each radial part",
        ),
        (
            "iend=\" 1000\"",
            "iend=\" 18446744073709551615\"",
            "radial_grid: iend must be the last index of each radial part",
        ),
        ("id=\"log1\">", ">", "radial_grid: no id"),
        ("a=\" 1e-3\"", "a=\" -1e-3\"", "radial_grid: a = -0.001"),
        (
            "<ae_core_density grid=\"log1\">",
            "<ae_core_density grid=\"log2\">",
            "ae_core_density: must use grid \"log1\"",
        ),
        ("<paw_radius rc=\" 1.5\"/>", "", "PAW-XML: no <paw_radius>"),
        (
            "<paw_radius rc=\" 1.5\"/>",
            "<paw_radius rc=\"wide\"/>",
            "paw_radius: rc \"wide\" does not parse",
        ),
        (
            "<paw_radius rc=\" 1.5\"/>",
            "<paw_radius/>",
            "paw_radius: no rc",
        ),
        ("l=\"1\"", "l=\"1.5\"", "state: l \"1.5\" does not parse"),
        ("f=\" 2.0\"", "f=\"two\"", "state: f \"two\" does not parse"),
        (
            "f=\" 2.0\"",
            "f=\" 3.0\"",
            "valence_states: each f must lie in [0, 2(2l + 1)]",
        ),
        (
            "<projector_function state= \"C1\"",
            "<projector_function state= \"C9\"",
            "projector_function: none for state \"C1\"",
        ),
        (
            "</kinetic_energy_differences>",
            "x </kinetic_energy_differences>",
            "kinetic_energy_differences: \"x\" is not a number",
        ),
        (
            "Z=\"6.00\"",
            "Z=\"7.00\"",
            "atom: Z must equal core + valence",
        ),
    ];
    for (from, to, message) in cases {
        assert!(text.contains(from), "{from}");
        let err = read_dataset(&text.replacen(from, to, 1)).unwrap_err();
        assert!(err.starts_with(message), "{from} → {err}");
    }
    let two_grids = text.replace("</radial_grid>", "</radial_grid>\n<radial_grid eq=\"r=a*(exp(d*i)-1)\" a=\"1\" d=\"1\" istart=\"0\" iend=\"3\" id=\"g2\"></radial_grid>");
    assert!(
        read_dataset(&two_grids)
            .unwrap_err()
            .starts_with("radial_grid: read one grid")
    );
    let bad_number = text.replacen(
        "<ae_core_density grid=\"log1\">\n",
        "<ae_core_density grid=\"log1\">\n1.0.0 ",
        1,
    );
    assert!(
        read_dataset(&bad_number)
            .unwrap_err()
            .starts_with("ae_core_density: \"1.0.0\" is not a number")
    );
}

#[test]
fn a_state_without_an_occupation_is_unbound() {
    let text = xml(&synthetic_draft())
        .replacen("l=\"1\" f=\" 2.0\"", "l=\"1\"", 1)
        .replacen("Z=\"6.00\"", "Z=\"4.00\"", 1)
        .replacen("valence=\"4.00\"", "valence=\"2.00\"", 1);
    let dataset = read_dataset(&text).unwrap();
    let occupations: Vec<Option<f64>> = dataset
        .draft()
        .channels
        .iter()
        .map(|c| c.occupation)
        .collect();
    assert_eq!(occupations, [Some(2.0), None]);
}
