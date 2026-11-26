use super::functional::matches_libxc;
use orbitar::model::Functional;

#[test]
fn lda_matches_libxc_and_its_partials() {
    matches_libxc(Functional::Lda);
}
