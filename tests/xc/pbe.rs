use super::functional::matches_libxc;
use orbitar::model::Functional;

#[test]
fn pbe_matches_libxc_and_its_partials() {
    matches_libxc(Functional::Pbe);
}
