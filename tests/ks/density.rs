use super::scf::{ONE, input, solve};

#[test]
fn zero_moments_give_the_unpolarized_state_in_both_spins() {
    let plain = solve(&input(&ONE, "")).unwrap();
    let spin = solve(&input(&ONE, "\n[spin]\nmoments = [0.0]\n")).unwrap();
    let [one] = plain.eigenvalues_ha() else {
        panic!("one spin without spin polarization")
    };
    for channel in spin.eigenvalues_ha() {
        for (a, b) in channel.iter().flatten().zip(one.iter().flatten()) {
            assert!((a - b).abs() < 1e-10, "{a} {b}");
        }
    }
    let (p, s) = (plain.scf().unwrap(), spin.scf().unwrap());
    assert!((p.energies.free_ha() - s.energies.free_ha()).abs() < 1e-10);
    assert!(
        s.magnetization_bohr_magneton
            .is_some_and(|m| m.abs() < 1e-10)
    );
    assert_eq!(p.magnetization_bohr_magneton, None);
}
