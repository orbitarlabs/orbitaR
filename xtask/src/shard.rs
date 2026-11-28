pub const CAP: [&str; 5] = [
    "env",
    "MALLOC_ARENA_MAX=1",
    "prlimit",
    "--as=2147483648",
    "--",
];

pub const NAMED: [&[&str]; 2] = [
    &[
        "ks::crystal::r2scan_forces_and_stress_take_the_pseudo_core_tau_only_where_the_grid_holds_it",
        "ks::crystal::the_r2scan_free_energy_is_stationary_at_the_self_consistent_state",
        "ks::scf::an_r2scan_step_energy_lies_within_the_second_order_of_the_step_from_its_outputs_own_energy",
        "ks::scf::a_run_without_any_pseudo_core_takes_no_pseudo_core_tau_on_the_grid",
        "ks::scf::at_the_stop_eigenvalues_and_e_f_lie_within_0_54_terms_within_1_density_limit_of_a_tight_stop",
    ],
    &[
        "ks::crystal::r2scan_forces_and_stress_are_the_free_energy_derivatives_with_and_without_spin",
        "ks::crystal::r2scan_stress_holds_the_turn_of_the_spheres_upright_frame",
        "ks::scf::a_rust_dataset_whose_pseudo_core_stays_below_the_threshold_runs_as_one_without",
        "ks::scf::a_run_with_a_pseudo_core_takes_every_species_pseudo_core_tau_on_the_grid",
        "ks::crystal::a_rotated_r2scan_crystal_keeps_its_free_energy_and_eigenvalues",
    ],
];

pub const SHARDS: usize = NAMED.len() + 1;

pub const UNIT: [&str; 2] = ["--test", "unit"];

pub struct Selection {
    pub targets: &'static [&'static str],
    pub filter: Vec<&'static str>,
}

pub fn selection(shard: usize) -> Result<Selection, String> {
    match shard {
        0 => Err(format!("check --unit 0: name a shard 1 to {SHARDS}")),
        k if k <= NAMED.len() => Ok(Selection {
            targets: &UNIT,
            filter: [&["--exact"][..], NAMED[k - 1]].concat(),
        }),
        SHARDS => Ok(Selection {
            targets: &[],
            filter: std::iter::once("--exact")
                .chain(
                    NAMED
                        .iter()
                        .flat_map(|s| s.iter().flat_map(|t| ["--skip", t])),
                )
                .collect(),
        }),
        k => Err(format!("check --unit {k}: name a shard 1 to {SHARDS}")),
    }
}

pub(crate) fn runner_var(cargo_version: &str) -> Result<String, String> {
    let host = (cargo_version.lines())
        .find_map(|l| l.strip_prefix("host: "))
        .ok_or("check --unit: `cargo -vV` names no host")?;
    Ok(format!(
        "CARGO_TARGET_{}_RUNNER",
        host.trim().to_uppercase().replace('-', "_")
    ))
}
