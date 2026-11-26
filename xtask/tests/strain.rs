use crate::{has, lever, tree};
use std::{fs, path::Path};

const CASE: &str = "benchmarks/15_stress_derivative/inputs";

const TRIPLES: [(&str, &str); 4] = [
    ("fe", "1,2,-3,2,0,2,-3,2,0"),
    ("feo", "-2,1,3,1,1,-1,3,-1,2"),
    ("fe-lda", "1,3,-3,3,-2,2,-3,2,2"),
    ("afm", "3,1,-2,1,2,1,-2,1,0"),
];

fn repo() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}

#[test]
fn it_writes_case_15s_ends_byte_for_byte() {
    for (stem, k) in TRIPLES {
        let centre = fs::read_to_string(repo().join(CASE).join(format!("{stem}.toml"))).unwrap();
        let root = tree(
            &format!("strain-{stem}"),
            &[(&format!("{stem}.toml"), &centre)],
        );
        let found = lever(
            &[
                "strain",
                &format!("{stem}.toml"),
                "--direction",
                k,
                "--h",
                "1e-5",
            ],
            &root,
        );
        assert!(found.is_empty(), "{stem}: {found:?}");
        for end in ["plus", "minus"] {
            let name = format!("{stem}-{end}.toml");
            let want = fs::read(repo().join(CASE).join(&name)).unwrap();
            assert_eq!(fs::read(root.join(&name)).unwrap(), want, "{name}");
        }
    }
}

#[test]
fn a_stem_names_the_ends() {
    let centre = "[structure]\ncell_bohr = [[2.0, 0.0, 0.0],\n             [0.0, 2.0, 0.0],\n             [0.0, 0.0, 2.0]]\n\n[outputs]\nstress = true\n";
    let root = tree("strain-stem", &[("input.toml", centre)]);
    let found = lever(
        &[
            "strain",
            "input.toml",
            "--direction",
            "1,0,0,0,1,0,0,0,1",
            "--h",
            "0.5",
            "--stem",
            "volume",
        ],
        &root,
    );
    assert!(found.is_empty(), "{found:?}");
    let plus = fs::read_to_string(root.join("volume-plus.toml")).unwrap();
    let side = 2.0 * (1.0 + 0.5 / 3f64.sqrt());
    assert!(plus.contains(&format!("[[{side:?}, 0.0, 0.0],")), "{plus}");
    assert!(
        !plus.contains("[outputs]") && !plus.contains("stress"),
        "{plus}"
    );
    assert!(root.join("volume-minus.toml").exists());
}

#[test]
fn bad_directions_and_inputs_are_caught() {
    let centre = "[structure]\ncell_bohr = [[2.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]]\n";
    let root = tree(
        "strain-bad",
        &[
            ("c.toml", centre),
            ("none.toml", "[structure]\n"),
            (
                "wide.toml",
                "[structure]\ncell_bohr = [[2.0, 0.0, 0.0, 0.0], [0.0, 2.0, 0.0], [0.0, 0.0, 2.0]]\n",
            ),
        ],
    );
    let run =
        |file: &str, k: &str, h: &str| lever(&["strain", file, "--direction", k, "--h", h], &root);
    assert!(has(&run("c.toml", "1,0,0,0,1,0,0,0", "1e-5"), "9 numbers"));
    assert!(has(
        &run("c.toml", "1,2,0,0,1,0,0,0,1", "1e-5"),
        "symmetric"
    ));
    assert!(has(&run("c.toml", "0,0,0,0,0,0,0,0,0", "1e-5"), "nonzero"));
    assert!(has(&run("c.toml", "1,0,0,0,1,0,0,0,1", "0"), "positive"));
    assert!(has(
        &run("none.toml", "1,0,0,0,1,0,0,0,1", "1e-5"),
        "no structure.cell_bohr"
    ));
    assert!(has(
        &run("wide.toml", "1,0,0,0,1,0,0,0,1", "1e-5"),
        "3 rows of 3 numbers"
    ));
    assert!(has(
        &run("gone.toml", "1,0,0,0,1,0,0,0,1", "1e-5"),
        "gone.toml"
    ));
    assert!(
        !root.join("c-plus.toml").exists(),
        "a refused triple writes nothing"
    );
}
