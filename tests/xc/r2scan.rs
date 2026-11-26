use super::functional::matches_libxc;
use orbitar::model::Functional;
use orbitar::xc::unpolarized;

#[test]
fn r2scan_matches_libxc_and_its_partials() {
    matches_libxc(Functional::R2scan);
}

#[test]
fn r2scan_v_tau_is_locally_linear_in_tau_where_the_switch_slope_cancels() {
    let v_tau = |tau: f64| unpolarized(Functional::R2scan, 1e110, 0.0, Some(tau))[1].vtau;
    let centre = 1e184_f64;
    let ulps = |step: i64| v_tau(f64::from_bits(centre.to_bits().wrapping_add_signed(step)));
    for step in 1..=8 {
        let curvature = (ulps(step) - 2.0 * ulps(0) + ulps(-step)).abs() / ulps(0).abs();
        assert!(
            curvature <= 1e-13,
            "τ ±{step} ulps from 1e184: v_tau bends {curvature:e}"
        );
    }
}
