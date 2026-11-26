use super::{Dataset, Table};
use std::f64::consts::PI;

const LOCAL_RADIUS_BOHR: f64 = 20.0;
const STEP: f64 = 0.01;

pub struct FormFactors {
    local: Table,
    core: Table,
    core_kinetic: Option<Table>,
    valence: Table,
    epsatm: f64,
    z_valence: f64,
}

impl FormFactors {
    pub fn tabulated(dataset: &Dataset, q_max: f64) -> Self {
        let d = dataset.draft();
        let r = d.grid.radii();
        let z_valence = d.valence_electrons;
        let local_end = (d.grid.index_below(LOCAL_RADIUS_BOHR) + 1).min(r.len());
        let local: Vec<f64> = (0..local_end)
            .map(|i| {
                if r[i] > 0.0 {
                    d.local_potential[i] + z_valence / r[i]
                } else {
                    0.0
                }
            })
            .collect();
        let table = |f: &[f64]| {
            Table::sampled(q_max, STEP, |q| 4.0 * PI * d.grid.bessel_transform(f, 0, q))
        };
        Self {
            epsatm: 4.0 * PI * d.grid.bessel_transform(&local, 0, 0.0),
            local: table(&local),
            core: table(&d.pseudo_core_density),
            core_kinetic: (d.core_kinetic.as_ref()).map(|k| table(k.pseudo())),
            valence: table(&d.pseudo_valence_density),
            z_valence,
        }
    }

    pub fn local_potential(&self, q: f64) -> f64 {
        assert!(q > 0.0, "v(q) diverges at q = 0; use epsatm");
        self.local.at(q) - 4.0 * PI * self.z_valence / (q * q)
    }

    pub fn local_potential_slope(&self, q: f64) -> f64 {
        assert!(q > 0.0, "v(q) diverges at q = 0; use epsatm");
        self.local.slope(q) + 8.0 * PI * self.z_valence / (q * q * q)
    }

    pub fn epsatm(&self) -> f64 {
        self.epsatm
    }

    pub fn pseudo_core(&self, q: f64) -> f64 {
        self.core.at(q)
    }

    pub fn pseudo_core_slope(&self, q: f64) -> f64 {
        self.core.slope(q)
    }

    pub fn pseudo_core_kinetic(&self, q: f64) -> Option<f64> {
        self.core_kinetic.as_ref().map(|t| t.at(q))
    }

    pub fn pseudo_core_kinetic_slope(&self, q: f64) -> Option<f64> {
        self.core_kinetic.as_ref().map(|t| t.slope(q))
    }

    pub fn pseudo_valence(&self, q: f64) -> f64 {
        self.valence.at(q)
    }
}
