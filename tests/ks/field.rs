use orbitar::ks::Local;
use orbitar::xc::Rung;

#[test]
fn v_tau_belongs_to_the_meta_gga_rung_alone() {
    let v = vec![0.5; 8];
    for rung in [Rung::Lda, Rung::Gga] {
        assert!(Local::of(v.clone(), None, rung).is_ok());
        assert_eq!(
            Local::of(v.clone(), Some(vec![0.0; 8]), rung)
                .err()
                .unwrap(),
            format!("v_τ: given, but the {rung:?} rung reads no τ")
        );
    }
    assert!(Local::of(v.clone(), Some(vec![0.0; 8]), Rung::MetaGga).is_ok());
    assert_eq!(
        Local::of(v.clone(), None, Rung::MetaGga).err().unwrap(),
        "v_τ: none, but the MetaGga rung reads τ"
    );
}

#[test]
fn v_tau_has_one_value_per_value_of_v() {
    assert_eq!(
        Local::of(vec![0.5; 8], Some(vec![0.0; 7]), Rung::MetaGga)
            .err()
            .unwrap(),
        "v_τ: 7 values for 8 of v"
    );
}

#[test]
fn a_local_part_hands_back_its_v_and_v_tau() {
    let local = Local::of(vec![0.5, -0.25], Some(vec![0.125, 1.0]), Rung::MetaGga).unwrap();
    assert_eq!(local.v(), [0.5, -0.25]);
    assert_eq!(local.v_tau(), Some([0.125, 1.0].as_slice()));
    let plain = Local::of(vec![0.5], None, Rung::Gga).unwrap();
    assert_eq!(plain.v_tau(), None);
}
