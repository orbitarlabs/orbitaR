use xtask::harness::eos::{self, Bm};

fn bm(v0_ang3: f64, b0_ev_ang3: f64, b1: f64) -> Bm {
    Bm {
        v0_ang3,
        b0_ev_ang3,
        b1,
    }
}

fn off(fit: Bm, expected: [f64; 3]) -> f64 {
    let got = [fit.v0_ang3, fit.b0_ev_ang3, fit.b1];
    (got.iter().zip(expected))
        .map(|(g, e)| ((g - e) / e).abs())
        .fold(0.0, f64::max)
}

fn pairs(volumes: &[f64], energies: &[f64]) -> Vec<(f64, f64)> {
    volumes
        .iter()
        .copied()
        .zip(energies.iter().copied())
        .collect()
}

fn abinit_runs() -> [([f64; 7], [f64; 7], [f64; 3]); 3] {
    [
        (
            [
                22.61015101369531,
                23.091218056539894,
                23.572285099384473,
                24.053352142229055,
                24.534419185073638,
                25.015486227918217,
                25.4965532707628,
            ],
            [
                -987.7228260021111,
                -987.7642229833325,
                -987.7862386002772,
                -987.7908354469671,
                -987.7797719776041,
                -987.7547238149307,
                -987.7171043727708,
            ],
            [23.946478639494703, 1.6539641073117282, 4.403888863567861],
        ),
        (
            [
                32.46722046950209,
                33.15801239438511,
                33.84880431926813,
                34.539596244151156,
                35.23038816903418,
                35.9211800939172,
                36.61197201880022,
            ],
            [
                -11210.682188389428,
                -11210.712449083616,
                -11210.729779199173,
                -11210.73556297932,
                -11210.731081264006,
                -11210.717306043945,
                -11210.695348632156,
            ],
            [34.56956156926469, 0.7426607969363896, 4.618494505930269],
        ),
        (
            [
                32.363044253676556,
                33.05161966332925,
                33.74019507298194,
                34.42877048263463,
                35.117345892287325,
                35.80592130194002,
                36.49449671159271,
            ],
            [
                -1313.870866412502,
                -1313.9053906683218,
                -1313.9261711332338,
                -1313.9346773037287,
                -1313.9322029179546,
                -1313.9198650033165,
                -1313.898713892113,
            ],
            [34.60741353796459, 0.7790033458819486, 4.548189565372777],
        ),
    ]
}

#[test]
fn v0_and_b0_equal_the_abinit_pipeline_fits_of_si_xo2_cu_x2o_and_la_xo() {
    for (volumes, energies, [v0, b0, _]) in abinit_runs() {
        let fit = eos::fit(&pairs(&volumes, &energies)).unwrap();
        let off = [
            ((fit.v0_ang3 - v0) / v0).abs(),
            ((fit.b0_ev_ang3 - b0) / b0).abs(),
        ];
        assert!(off.iter().all(|o| *o < 1e-10), "{fit:?} against {v0}, {b0}");
    }
}

#[test]
fn b1_is_the_limit_of_the_abinit_pipelines_central_difference() {
    for (volumes, energies, [_, _, b1]) in abinit_runs() {
        let fit = eos::fit(&pairs(&volumes, &energies)).unwrap();
        assert!(((fit.b1 - b1) / b1).abs() < 5e-8, "{fit:?} against B1 {b1}");
    }
    let [(volumes, energies, _), ..] = abinit_runs();
    let fit = eos::fit(&pairs(&volumes, &energies)).unwrap();
    assert!(
        ((fit.b1 - 4.4038886970948115) / fit.b1).abs() < 1e-10,
        "{fit:?}"
    );
}

#[test]
fn the_fit_returns_the_parameters_of_points_on_a_birch_murnaghan_curve() {
    let curve = bm(20.0, 0.6, 4.5);
    let volumes: Vec<f64> = [0.94, 0.96, 0.98, 1.0, 1.02, 1.04, 1.06]
        .iter()
        .map(|s| 20.4 * s)
        .collect();
    let energies: Vec<f64> = volumes
        .iter()
        .map(|&v| curve.energy_ev(v) - 1234.5)
        .collect();
    let fit = eos::fit(&pairs(&volumes, &energies)).unwrap();
    assert!(off(fit, [20.0, 0.6, 4.5]) < 1e-10, "{fit:?}");
}

#[test]
fn four_distinct_volumes_determine_the_cubic() {
    let curve = bm(20.0, 0.6, 4.5);
    let volumes = [19.0, 19.8, 20.6, 21.4];
    let energies = volumes.map(|v| curve.energy_ev(v));
    let fit = eos::fit(&pairs(&volumes, &energies)).unwrap();
    assert!(off(fit, [20.0, 0.6, 4.5]) < 1e-10, "{fit:?}");
}

#[test]
fn a_fit_with_fewer_than_four_distinct_volumes_fails() {
    let points = [(10.0, 1.0), (10.0, 1.0), (11.0, 0.5), (12.0, 0.8)];
    let error = eos::fit(&points).unwrap_err();
    assert!(
        error.contains("4 or more distinct volumes; got 3"),
        "{error}"
    );
}

#[test]
fn a_fit_without_a_stationary_point_has_no_minimum() {
    let points: Vec<(f64, f64)> = [10.0, 11.0, 12.0, 13.0, 14.0]
        .map(|v: f64| {
            let x = v.powf(-2.0 / 3.0) - 0.2;
            (v, x.powi(3) + x)
        })
        .into();
    let error = eos::fit(&points).unwrap_err();
    assert!(error.contains("no minimum"), "{error}");
}

#[test]
fn a_minimum_at_negative_x_is_not_a_volume() {
    let points: Vec<(f64, f64)> = [10.0, 11.0, 12.0, 13.0, 14.0]
        .map(|v: f64| (v, (v.powf(-2.0 / 3.0) + 0.1).powi(2)))
        .into();
    let error = eos::fit(&points).unwrap_err();
    assert!(error.contains("V0 is not a positive volume"), "{error}");
}

#[test]
fn epsilon_equals_the_acwf_definition_integrated_numerically() {
    let rows = [
        (
            bm(17.87215944100911, 0.7557899299338641, 5.416925337447787),
            bm(17.86134273356192, 0.7571782007651459, 5.454137933935573),
            0.03748108145973028,
        ),
        (
            bm(71.88177555981177, 0.24682674564549933, 5.090397890082898),
            bm(72.18837519600658, 0.24637058117315613, 5.104969597652214),
            0.27070976712479655,
        ),
        (
            bm(79.98783645180751, 0.4177684309533024, 6.814677554656953),
            bm(79.51922007961696, 0.430908087628455, 6.687991575195888),
            0.3642553785197435,
        ),
    ];
    for (code, reference, expected) in rows {
        let got = eos::epsilon(&code, &reference);
        assert!((got - expected).abs() < 1e-12, "{got} against {expected}");
    }
}

#[test]
fn epsilon_vanishes_against_itself() {
    let w = bm(28.9, 0.12, 4.0);
    assert_eq!(eos::epsilon(&w, &w), 0.0);
}

#[test]
fn epsilon_grows_linearly_with_a_small_v0_shift() {
    let w = bm(28.9, 0.12, 4.0);
    let shifted = |s: f64| eos::epsilon(&w, &bm(28.9 * (1.0 + s), 0.12, 4.0));
    let ratio = shifted(1e-5) / shifted(1e-6);
    assert!((ratio - 10.0).abs() < 1e-4, "{ratio}");
    assert!(shifted(1e-6) > 0.0);
}

#[test]
fn nu_of_pd_x_sc_equals_the_acwf_formula() {
    let w = bm(17.87215944100911, 0.7557899299338641, 5.416925337447787);
    let f = bm(17.86134273356192, 0.7571782007651459, 5.454137933935573);
    assert!((eos::nu(&w, &f) - 0.06125631376415494).abs() < 1e-15);
}

#[test]
fn per_divides_only_the_volume() {
    assert_eq!(bm(40.0, 0.5, 4.2).per(2.0), bm(20.0, 0.5, 4.2));
}

#[test]
fn case_10_s_tolerance_keeps_epsilon_below_3_9e_3_on_every_abinit_curve() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../benchmarks/10_acwf_subset/reference/abinit.jsonl");
    let text = std::fs::read_to_string(path).unwrap();
    let tolerance = 1e-7 * 27.211386245988;
    let mut worst = (0.0, String::new());
    for line in text.lines() {
        let row: serde_json::Value = serde_json::from_str(line).unwrap();
        let numbers = |key: &str| -> Vec<f64> {
            (row[key].as_array().unwrap().iter())
                .map(|x| x.as_f64().unwrap())
                .collect()
        };
        let points: Vec<(f64, f64)> = numbers("volumes")
            .into_iter()
            .zip(numbers("energies_ev"))
            .collect();
        let atoms = row["natoms"].as_f64().unwrap();
        let unmoved = eos::fit(&points).unwrap().per(atoms);
        for signs in 0..1_u32 << points.len() {
            let moved: Vec<(f64, f64)> = (points.iter().enumerate())
                .map(|(i, &(v, e))| {
                    (
                        v,
                        e + tolerance * if signs >> i & 1 == 1 { 1.0 } else { -1.0 },
                    )
                })
                .collect();
            let epsilon = eos::epsilon(&eos::fit(&moved).unwrap().per(atoms), &unmoved);
            if epsilon > worst.0 {
                worst = (epsilon, row["system"].as_str().unwrap().to_owned());
            }
        }
    }
    assert!(worst.0 < 3.9e-3 && worst.1 == "Na-X/BCC", "{worst:?}");
}

const AE: &str = include_str!("../../../benchmarks/10_acwf_subset/reference/acwf-ae-average.json");
const VASP: &str = include_str!("../../../benchmarks/10_acwf_subset/reference/acwf-vasp.json");

fn vasp_bar<'a>(systems: impl IntoIterator<Item = &'a str>) -> (usize, String, String, usize) {
    let (ae, vasp) = (
        xtask::harness::acwf::published(AE).unwrap(),
        xtask::harness::acwf::published(VASP).unwrap(),
    );
    let eps: Vec<f64> = (systems.into_iter())
        .map(|s| eos::epsilon(&vasp(s).unwrap(), &ae(s).unwrap()))
        .collect();
    let n = eps.len();
    let median = statrs::statistics::Median::median(&statrs::statistics::Data::new(eps.clone()));
    let share = eps.iter().filter(|&&e| e <= 0.06).count() as f64 / n as f64;
    let above = eps.iter().filter(|&&e| e > 1.0).count();
    (n, format!("{median:.4}"), format!("{share:.3}"), above)
}

#[test]
fn epsilon_gives_acwf_s_vasp_bar_from_the_published_fits() {
    let systems = xtask::harness::acwf::systems(AE).unwrap();
    assert_eq!(
        vasp_bar(systems.iter().map(String::as_str)),
        (960, "0.0758".to_owned(), "0.420".to_owned(), 9)
    );
}

#[test]
fn epsilon_gives_vasp_s_bar_on_the_exam_part_of_96_elements_and_of_the_86_with_a_jth_file() {
    let (split, jth) = (super::acwf::split(), super::acwf::jth());
    let exam: Vec<String> = (xtask::harness::acwf::systems(AE).unwrap().into_iter())
        .filter(|s| split.part(s).unwrap() == xtask::harness::acwf::Part::Exam)
        .collect();
    let on_jth = exam
        .iter()
        .filter(|s| jth.contains(s.split_once('-').unwrap().0));
    assert_eq!(
        vasp_bar(exam.iter().map(String::as_str)),
        (480, "0.0782".to_owned(), "0.402".to_owned(), 4)
    );
    assert_eq!(
        vasp_bar(on_jth.map(String::as_str)),
        (430, "0.0737".to_owned(), "0.421".to_owned(), 3)
    );
}

fn delta_by_simpson(a: &Bm, b: &Bm) -> f64 {
    let energy = |p: &Bm, v: f64| {
        let x = (p.v0_ang3 / v).powf(2.0 / 3.0) - 1.0;
        9.0 * p.v0_ang3 * p.b0_ev_ang3 / 16.0
            * (x.powi(3) * p.b1 + x.powi(2) * (6.0 - 4.0 * (x + 1.0)))
    };
    let mean = (a.v0_ang3 + b.v0_ang3) / 2.0;
    let (low, high, steps) = (0.94 * mean, 1.06 * mean, 4000);
    let h = (high - low) / steps as f64;
    let sum: f64 = (0..=steps)
        .map(|i| {
            let v = low + i as f64 * h;
            let weight = match i {
                0 | 4000 => 1.0,
                _ if i % 2 == 1 => 4.0,
                _ => 2.0,
            };
            weight * (energy(a, v) - energy(b, v)).powi(2)
        })
        .sum();
    1000.0 * (sum * h / 3.0 / (high - low)).sqrt()
}

fn delta_pairs() -> Vec<(Bm, Bm)> {
    let gpa = |v0: f64, b0: f64, b1: f64| bm(v0, b0 * eos::GPA_EV_ANG3, b1);
    vec![
        (gpa(11.37, 190.0, 4.9), gpa(11.40, 190.0, 4.9)),
        (gpa(11.37, 190.0, 4.9), gpa(11.37, 200.0, 4.9)),
        (gpa(11.37, 190.0, 4.9), gpa(11.37, 190.0, 5.6)),
        (gpa(10.89, 200.4, 5.0), gpa(10.95, 192.0, 4.6)),
        (gpa(36.92, 42.6, 4.8), gpa(37.40, 40.1, 5.3)),
        (gpa(11.84, 255.0, 4.2), gpa(11.60, 280.0, 5.1)),
        (gpa(93.2, 0.50, 8.1), gpa(90.0, 0.65, 7.0)),
    ]
}

#[test]
fn delta_equals_its_definition_integrated_numerically() {
    for (a, b) in delta_pairs() {
        let (got, expected) = (eos::delta(&a, &b), delta_by_simpson(&a, &b));
        assert!(
            (got - expected).abs() <= 1e-9 * expected,
            "{got} against {expected}"
        );
    }
}

#[test]
fn delta_vanishes_against_itself_and_is_symmetric() {
    for (a, b) in delta_pairs() {
        assert_eq!(eos::delta(&a, &a), 0.0);
        assert!((eos::delta(&a, &b) - eos::delta(&b, &a)).abs() <= 1e-12 * eos::delta(&a, &b));
    }
}

#[test]
fn delta_grows_linearly_with_a_small_v0_shift() {
    let shifted = |d: f64| {
        let a = bm(11.37, 190.0 * eos::GPA_EV_ANG3, 4.9);
        eos::delta(&a, &bm(11.37 * (1.0 + d), a.b0_ev_ang3, a.b1))
    };
    let ratio = shifted(1e-3) / shifted(1e-4);
    assert!((ratio - 10.0).abs() < 1e-2, "{ratio}");
}

#[test]
fn a_gigapascal_is_one_160_2176565th_of_an_ev_per_cubic_angstrom() {
    assert!((eos::GPA_EV_ANG3 * 160.2176565 - 1.0).abs() < 1e-15);
}
