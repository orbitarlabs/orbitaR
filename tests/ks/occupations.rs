use orbitar::ks::{Occupied, occupy};
use orbitar::model::Smearing;
use std::f64::consts::PI;

const FERMI_DIRAC_BANDS: [[f64; 6]; 16] = [
    [
        -0.0846641689252729,
        0.791559732066477,
        0.7915597320754275,
        0.7915597320867035,
        0.8431675935023731,
        0.843167593519549,
    ],
    [
        -0.056962124031508465,
        0.5697301398085665,
        0.7432677343094046,
        0.7432677343168572,
        0.8617091807506638,
        0.9154083306051961,
    ],
    [
        0.02529302113589045,
        0.34334214532955964,
        0.721671857656022,
        0.7216718576632466,
        0.8175825441320341,
        0.8175825517285863,
    ],
    [
        0.15582781975286247,
        0.1639424241540014,
        0.7440890517348727,
        0.7440890517659132,
        0.7484595029956388,
        0.7484595030273469,
    ],
    [
        -0.04775929349198721,
        0.6569074491139734,
        0.6569074491198715,
        0.6634632660718368,
        0.7766321592695327,
        0.7853689655382788,
    ],
    [
        0.0162042447996043,
        0.43560312984631416,
        0.6177316368414563,
        0.6449152461161427,
        0.6789827660044041,
        0.8592815220856519,
    ],
    [
        0.13290300062117863,
        0.2391317365216714,
        0.5540572614471623,
        0.6261217759076011,
        0.6923558712307558,
        0.914392569913389,
    ],
    [
        0.08838744955729917,
        0.2991772023005571,
        0.5131165563235868,
        0.6820142857440651,
        0.7535101251268337,
        0.8749198362355024,
    ],
    [
        -0.011111391005682472,
        0.4903423957813339,
        0.5422582496324854,
        0.7840073696265325,
        0.858782922996805,
        0.8635652877059488,
    ],
    [
        0.061350073579210226,
        0.48637622918432033,
        0.5560339732869451,
        0.5621390386184354,
        0.5621390386232437,
        0.6918989930158101,
    ],
    [
        0.15876997709836066,
        0.36302116872874796,
        0.3805333819601481,
        0.5543475711201312,
        0.6107146320845344,
        0.8012630935480247,
    ],
    [
        0.20332697888351153,
        0.2853075636740502,
        0.3396518237865484,
        0.5942637488861248,
        0.6601187348977466,
        1.013946939190382,
    ],
    [
        0.21722298294958178,
        0.2663313880859748,
        0.5194742767066427,
        0.5305294073180549,
        0.5305294073226006,
        0.6590678624860145,
    ],
    [
        0.09725173152064953,
        0.38779563828534486,
        0.44162969004591696,
        0.5229141082887423,
        0.7978022397964146,
        0.8572300566875074,
    ],
    [
        0.2241859907623182,
        0.23422235655513746,
        0.43567653228994835,
        0.4567433909325393,
        0.8413026111453762,
        0.8607586996004484,
    ],
    [
        0.25123447877326327,
        0.3012596336218655,
        0.353636212949283,
        0.4116421149075369,
        0.7650825026797375,
        0.8323422976700072,
    ],
];
const WEIGHTS: [f64; 16] = [
    0.004629629629629629,
    0.037037037037037035,
    0.037037037037037035,
    0.018518518518518517,
    0.027777777777777776,
    0.11111111111111105,
    0.11111111111111105,
    0.11111111111111105,
    0.05555555555555555,
    0.027777777777777776,
    0.11111111111111105,
    0.05555555555555555,
    0.013888888888888888,
    0.11111111111111105,
    0.11111111111111105,
    0.05555555555555555,
];
const FERMI_DIRAC_ABINIT: (f64, f64) = (0.3382787836949563, -0.00021683748250361874);
const METHFESSEL_PAXTON_BANDS: [[f64; 6]; 16] = [
    [
        -0.08466021579886773,
        0.7915636186954285,
        0.7915636187042943,
        0.7915636187156758,
        0.8431779752985948,
        0.8431779753158957,
    ],
    [
        -0.05695812033302208,
        0.5697353380639117,
        0.7432737577762948,
        0.7432737577837928,
        0.8617155836059467,
        0.915418506824025,
    ],
    [
        0.025297157581697646,
        0.3433469552636034,
        0.7216782115263229,
        0.7216782115335757,
        0.8175884870777496,
        0.8175884946790795,
    ],
    [
        0.1558300545466975,
        0.16394906101434858,
        0.7440998594824918,
        0.7440998595135199,
        0.7484608553443015,
        0.748460855375912,
    ],
    [
        -0.0477552738917179,
        0.6569135356555433,
        0.6569135356614656,
        0.6634674178854132,
        0.776643461917002,
        0.7853693299501174,
    ],
    [
        0.01620836935553747,
        0.4356080694794685,
        0.6177377717396325,
        0.6449189112712426,
        0.678989266477715,
        0.8592880089690796,
    ],
    [
        0.13290719930202996,
        0.2391364746561236,
        0.5540619572855937,
        0.6261279416235127,
        0.6923613321951991,
        0.9143987021777665,
    ],
    [
        0.088391655293436,
        0.2991819861964089,
        0.5131214650786519,
        0.6820206487116258,
        0.7535154002142486,
        0.87492550357151,
    ],
    [
        -0.0111073095500713,
        0.4903474948370508,
        0.5422634297912152,
        0.7840149719030102,
        0.8587881661189671,
        0.8635693476962816,
    ],
    [
        0.06135427241713268,
        0.48638018528057725,
        0.5560381380876285,
        0.5621452040695443,
        0.5621452040743822,
        0.691905286606917,
    ],
    [
        0.15877433503071225,
        0.36302398153693644,
        0.38053974623746917,
        0.5543537516394881,
        0.6107196933013043,
        0.8012693009800619,
    ],
    [
        0.20333113134300837,
        0.2853126897596241,
        0.33965614100290226,
        0.594269945190928,
        0.6601238409319843,
        1.0139538243151238,
    ],
    [
        0.21722814450547812,
        0.26633454088637937,
        0.5194784416085808,
        0.5305356044508139,
        0.5305356044553934,
        0.6590736551181748,
    ],
    [
        0.09725596435505574,
        0.3878007362603242,
        0.441633630378426,
        0.5229202229884378,
        0.7978076729479818,
        0.8572372023265563,
    ],
    [
        0.22418854553437653,
        0.23422883879968812,
        0.4356782599557907,
        0.4567518658607831,
        0.8413094676603182,
        0.8607637020582675,
    ],
    [
        0.2512396216260605,
        0.30126212929802054,
        0.35364131731925097,
        0.41164854526313305,
        0.7650879662085945,
        0.8323488794762663,
    ],
];
const METHFESSEL_PAXTON_ABINIT: (f64, f64) = (0.3354579605849304, 0.00021831691119269);
const GAUSSIAN_BANDS: [[f64; 6]; 16] = [
    [
        -0.0846595884130924,
        0.7915725484742063,
        0.7915725484831665,
        0.7915725484944863,
        0.8431604461610452,
        0.8431604461782622,
    ],
    [
        -0.05695761647098306,
        0.5697369444269932,
        0.7432721681970117,
        0.7432721682044697,
        0.8617170167668888,
        0.9154117997134796,
    ],
    [
        0.025297385539020074,
        0.3433474069234532,
        0.7216746553967395,
        0.7216746554039574,
        0.8175867664055834,
        0.8175867739822641,
    ],
    [
        0.15582993894569988,
        0.16394934180495657,
        0.7440888951602277,
        0.7440888951914094,
        0.7484656716221056,
        0.748465671654038,
    ],
    [
        -0.04775480778301211,
        0.656909891520249,
        0.6569098915262226,
        0.6634827322761687,
        0.7766247398766501,
        0.7853714345424732,
    ],
    [
        0.016208626138881514,
        0.43560946599811157,
        0.6177331949689178,
        0.6449186073533558,
        0.6789898161058602,
        0.8592805025776717,
    ],
    [
        0.13290732964348628,
        0.2391367357382753,
        0.5540599576515155,
        0.6261232565784208,
        0.6923614705931846,
        0.914397709073161,
    ],
    [
        0.08839177824039343,
        0.2991827115412419,
        0.5131186510840867,
        0.6820163231778741,
        0.7535149668625171,
        0.8749250952365047,
    ],
    [
        -0.01110697597307078,
        0.4903527044389721,
        0.5422567191422805,
        0.7840101294839887,
        0.8587874016651236,
        0.8635781968765337,
    ],
    [
        0.061354472099332265,
        0.4863786750807437,
        0.5560546204669505,
        0.5621397919022554,
        0.5621397919071455,
        0.691890079256839,
    ],
    [
        0.1587748123198746,
        0.36302527258569495,
        0.3805374425593545,
        0.5543481362839342,
        0.6107226061050213,
        0.8012615593971107,
    ],
    [
        0.20333139602903336,
        0.28531718338601353,
        0.33964950329785576,
        0.5942647728186459,
        0.660124943283734,
        1.0139497394463917,
    ],
    [
        0.21723224560219723,
        0.26632861347501724,
        0.5194951377713083,
        0.5305296616104002,
        0.53052966161503,
        0.6590592409031845,
    ],
    [
        0.09725612252059118,
        0.38780577172385217,
        0.44162730670929506,
        0.5229168015587164,
        0.7978137963508928,
        0.8572244697601634,
    ],
    [
        0.22418918484749054,
        0.23422942139203798,
        0.43567801760337393,
        0.4567458693089778,
        0.8413053825655401,
        0.8607676891924374,
    ],
    [
        0.25124393526731675,
        0.30125630830215483,
        0.35364627894921763,
        0.41164034012532125,
        0.7650938735354035,
        0.8323369147486874,
    ],
];
const GAUSSIAN_ABINIT: (f64, f64) = (0.33069299994016504, -0.0012359529675918777);
const MARZARI_VANDERBILT_DFTK: (f64, f64) = (0.3267771231230328, 0.0009995699986391942);

fn one_spin(bands: &[[f64; 6]; 16]) -> Vec<Vec<Vec<f64>>> {
    vec![bands.iter().map(|k| k.to_vec()).collect()]
}

#[test]
fn fermi_level_and_entropy_match_abinit_and_dftk_for_aluminium() {
    for (smearing, width, bands, (fermi, entropy)) in [
        (
            Smearing::FermiDirac,
            0.00225,
            &FERMI_DIRAC_BANDS,
            FERMI_DIRAC_ABINIT,
        ),
        (
            Smearing::MethfesselPaxton,
            0.02,
            &METHFESSEL_PAXTON_BANDS,
            METHFESSEL_PAXTON_ABINIT,
        ),
        (Smearing::Gaussian, 0.02, &GAUSSIAN_BANDS, GAUSSIAN_ABINIT),
        (
            Smearing::MarzariVanderbilt,
            0.02,
            &GAUSSIAN_BANDS,
            MARZARI_VANDERBILT_DFTK,
        ),
    ] {
        let weights = WEIGHTS;
        let got = occupy(smearing, width, &one_spin(bands), &weights, 3.0).unwrap();
        assert!(
            (got.fermi_ha - fermi).abs() < 1e-12,
            "{smearing:?}: {} vs {fermi}",
            got.fermi_ha
        );
        assert!(
            (got.smearing_ha - entropy).abs() < 1e-12,
            "{smearing:?}: {} vs {entropy}",
            got.smearing_ha
        );
        let total: f64 = got.occupations[0]
            .iter()
            .zip(weights)
            .map(|(k, w)| w * k.iter().sum::<f64>())
            .sum();
        assert!((total - 3.0).abs() < 1e-12);
    }
}

#[test]
fn a_spin_split_band_set_shares_one_fermi_level() {
    let one = occupy(
        Smearing::FermiDirac,
        0.00225,
        &one_spin(&FERMI_DIRAC_BANDS),
        &WEIGHTS,
        3.0,
    )
    .unwrap();
    let two_channels = vec![one_spin(&FERMI_DIRAC_BANDS)[0].clone(); 2];
    let two = occupy(Smearing::FermiDirac, 0.00225, &two_channels, &WEIGHTS, 3.0).unwrap();
    assert!(
        (one.fermi_ha - two.fermi_ha).abs() < 1e-13
            && (one.smearing_ha - two.smearing_ha).abs() < 1e-13
    );
    assert!(
        two.occupations
            .iter()
            .flatten()
            .flatten()
            .all(|&f| f <= 1.0 + 1e-15)
    );
}

#[test]
fn a_threefold_level_holding_two_of_six_states_sits_at_minus_width_ln2() {
    for width in [2.25e-3, 1e-14] {
        let bands = vec![vec![vec![-1.0, 0.5, 0.5, 0.5, 3.0]]];
        let got = occupy(Smearing::FermiDirac, width, &bands, &[1.0], 4.0).unwrap();
        assert!((got.fermi_ha - (0.5 - width * 2f64.ln())).abs() < 1e-12 * (1.0 + 1e3 * width));
    }
    let bands = vec![vec![vec![-1.0, 0.5, 0.5, 0.5, 3.0]]];
    let got = occupy(Smearing::FermiDirac, 2.25e-3, &bands, &[1.0], 4.0).unwrap();
    assert!(
        got.occupations[0][0][1..4]
            .iter()
            .all(|f| (f - 2.0 / 3.0).abs() < 1e-12)
    );
}

#[test]
fn a_gap_puts_the_level_in_its_middle_whatever_the_band_count() {
    for bands in [
        vec![vec![vec![-0.3, 0.2, 0.4, 0.9]]],
        vec![vec![vec![-0.3, 0.2, 0.4]]],
    ] {
        let got = occupy(Smearing::FermiDirac, 0.00225, &bands, &[1.0], 4.0).unwrap();
        assert!((got.fermi_ha - 0.3).abs() < 1e-15);
        assert!(got.smearing_ha.abs() < 1e-17);
    }
}

#[test]
fn each_level_and_entropy_has_its_closed_form_for_a_half_filled_band() {
    let s0 = [
        (Smearing::FermiDirac, 2f64.ln()),
        (Smearing::Gaussian, 0.5 / PI.sqrt()),
        (Smearing::MethfesselPaxton, 0.25 / PI.sqrt()),
    ];
    for (smearing, s) in s0 {
        let got = occupy(smearing, 0.01, &[vec![vec![0.0, 9.0]]], &[1.0], 1.0).unwrap();
        assert!(got.fermi_ha.abs() < 1e-12, "{smearing:?}");
        assert!(
            (got.smearing_ha + 0.01 * 2.0 * s).abs() < 1e-14,
            "{smearing:?}"
        );
    }
    let theta = |x: f64| {
        let u = x + std::f64::consts::FRAC_1_SQRT_2;
        libm::erfc(u) / 2.0 + (-u * u).exp() / (2.0 * PI).sqrt()
    };
    let (mut lo, mut hi) = (-1.0_f64, 1.0_f64);
    for _ in 0..200 {
        let mid = 0.5 * (lo + hi);
        if theta(mid) > 0.5 { lo = mid } else { hi = mid }
    }
    let u = lo + std::f64::consts::FRAC_1_SQRT_2;
    let s = u * (-u * u).exp() / (2.0 * PI).sqrt();
    let got = occupy(
        Smearing::MarzariVanderbilt,
        0.01,
        &[vec![vec![0.0, 9.0]]],
        &[1.0],
        1.0,
    )
    .unwrap();
    assert!(
        (got.fermi_ha + 0.01 * lo).abs() < 1e-14,
        "{} vs {}",
        got.fermi_ha,
        -0.01 * lo
    );
    assert!(
        (got.smearing_ha + 0.01 * 2.0 * s).abs() < 1e-14,
        "{} vs {}",
        got.smearing_ha,
        -0.02 * s
    );
}

#[test]
fn the_free_energy_derivative_in_one_eigenvalue_is_its_weighted_occupation() {
    for smearing in [
        Smearing::FermiDirac,
        Smearing::Gaussian,
        Smearing::MethfesselPaxton,
        Smearing::MarzariVanderbilt,
    ] {
        let base = one_spin(&FERMI_DIRAC_BANDS);
        let free = |shift: f64| {
            let mut bands = base.clone();
            bands[0][2][1] += shift;
            let o = occupy(smearing, 0.02, &bands, &WEIGHTS, 3.0).unwrap();
            let band: f64 = (0..16)
                .map(|k| {
                    WEIGHTS[k]
                        * (0..6)
                            .map(|n| o.occupations[0][k][n] * bands[0][k][n])
                            .sum::<f64>()
                })
                .sum();
            (band + o.smearing_ha, o.occupations[0][2][1])
        };
        let h = 1e-5;
        let slope = (free(h).0 - free(-h).0) / (2.0 * h);
        let want = WEIGHTS[2] * free(0.0).1;
        assert!(
            (slope - want).abs() < 1e-5 * want,
            "{smearing:?}: {slope} vs {want}"
        );
    }
}

#[test]
fn too_few_bands_or_an_odd_count_for_fixed_occupations_are_refused() {
    let err = occupy(Smearing::FermiDirac, 0.01, &[vec![vec![0.0]]], &[1.0], 3.0).unwrap_err();
    assert!(err.starts_with("bands: 3 electrons need 2 bands"), "{err}");
    let err = occupy(Smearing::Fixed, 0.0, &[vec![vec![0.0, 1.0]]], &[1.0], 3.0).unwrap_err();
    assert!(
        err.starts_with("occupations.smearing: fixed occupations cannot hold 3"),
        "{err}"
    );
    let fixed = occupy(
        Smearing::Fixed,
        0.0,
        &[vec![vec![0.0, 1.0, 2.0]]],
        &[1.0],
        4.0,
    )
    .unwrap();
    assert_eq!(
        (
            fixed.fermi_ha,
            fixed.occupations[0][0].clone(),
            fixed.smearing_ha
        ),
        (1.0, vec![2.0, 2.0, 0.0], 0.0)
    );
}

#[test]
fn fixed_occupations_fill_whole_bands_of_each_spin() {
    let spin = vec![vec![0.0, 1.0, 2.0, 3.0]];
    let fixed = occupy(Smearing::Fixed, 0.0, &[spin.clone(), spin], &[1.0], 6.0).unwrap();
    let filled = vec![vec![1.0, 1.0, 1.0, 0.0]];
    assert_eq!(
        (fixed.fermi_ha, fixed.occupations),
        (2.0, vec![filled.clone(), filled])
    );
}

#[test]
fn one_band_under_fermi_dirac_sits_at_its_closed_form_level() {
    let width = 0.01;
    for electrons in [1.5, 1.999_999_9] {
        let theta: f64 = electrons / 2.0;
        let got = occupy(
            Smearing::FermiDirac,
            width,
            &[vec![vec![1.0]]],
            &[1.0],
            electrons,
        )
        .unwrap();
        let want = 1.0 + width * (theta / (1.0 - theta)).ln();
        assert!(
            (got.fermi_ha - want).abs() < 1e-8,
            "{electrons}: {}",
            got.fermi_ha
        );
    }
}

#[test]
fn a_millionth_of_an_electron_is_met_to_its_own_relative_tolerance() {
    let got = occupy(Smearing::FermiDirac, 0.01, &[vec![vec![1.0]]], &[1.0], 1e-6).unwrap();
    assert!(
        (got.occupations[0][0][0] - 1e-6).abs() <= 1e-13 * 1e-6,
        "{got:?}"
    );
}

#[test]
fn a_count_short_by_exactly_the_tolerance_still_fits() {
    let fits = occupy(
        Smearing::FermiDirac,
        0.01,
        &[vec![vec![1.0]]],
        &[1.0],
        1.999_999_999_999_833,
    );
    assert!(fits.is_ok(), "{fits:?}");
}

#[test]
fn a_band_that_the_electrons_fill_holds_them_under_gaussian_smearing() {
    let got = occupy(Smearing::Gaussian, 0.01, &[vec![vec![1.0]]], &[1.0], 2.0).unwrap();
    assert!((got.occupations[0][0][0] - 2.0).abs() < 1e-13, "{got:?}");
}

type Refused = (
    Smearing,
    f64,
    Vec<Vec<Vec<f64>>>,
    Vec<f64>,
    f64,
    &'static str,
);

#[test]
fn shapes_that_occupy_cannot_read_are_refused() {
    let one = vec![vec![vec![0.0, 1.0]]];
    let cases: [Refused; 9] = [
        (
            Smearing::Gaussian,
            0.01,
            vec![one[0].clone(); 3],
            vec![1.0],
            1.0,
            "bands: give one or two spins",
        ),
        (
            Smearing::Gaussian,
            0.01,
            one.clone(),
            vec![0.5, 0.5],
            1.0,
            "bands: each spin needs one row per k-point",
        ),
        (
            Smearing::Gaussian,
            0.01,
            vec![vec![vec![0.0, 1.0]; 2]],
            vec![1.0, 1.0],
            1.0,
            "weights: k-point weights",
        ),
        (
            Smearing::Gaussian,
            0.01,
            vec![vec![vec![0.0, 1.0]; 2]],
            vec![1.5, -0.5],
            1.0,
            "weights: k-point weights",
        ),
        (
            Smearing::Gaussian,
            0.01,
            vec![vec![vec![0.0, 1.0], vec![0.0]]],
            vec![0.5, 0.5],
            1.0,
            "bands: every k-point",
        ),
        (
            Smearing::Gaussian,
            0.01,
            vec![vec![vec![1.0, 0.0]]],
            vec![1.0],
            1.0,
            "bands: every k-point",
        ),
        (
            Smearing::Gaussian,
            0.01,
            one.clone(),
            vec![1.0],
            0.0,
            "electrons: need a positive count",
        ),
        (
            Smearing::Gaussian,
            0.0,
            one.clone(),
            vec![1.0],
            1.0,
            "occupations.width: a smearing needs",
        ),
        (
            Smearing::Fixed,
            0.0,
            vec![vec![Vec::new()]],
            vec![1.0],
            2.0,
            "bands: every k-point",
        ),
    ];
    for (smearing, width, bands, weights, electrons, message) in cases {
        let err = occupy(smearing, width, &bands, &weights, electrons).unwrap_err();
        assert!(err.starts_with(message), "{message}: {err}");
    }
}

#[test]
fn bands_at_or_below_abinits_tol8_count_in_no_density() {
    let occupied = Occupied {
        fermi_ha: 0.0,
        occupations: vec![vec![vec![
            1.0, 1.01e-8, 1e-8, 9.99e-9, -9.99e-9, -1e-8, -1.01e-8, 0.0,
        ]]],
        smearing_ha: 0.0,
    };
    assert_eq!(
        occupied.counted(),
        vec![vec![vec![1.0, 1.01e-8, 0.0, 0.0, 0.0, 0.0, -1.01e-8, 0.0]]]
    );
}
