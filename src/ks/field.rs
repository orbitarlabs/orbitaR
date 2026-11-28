use crate::pw::Mat;
use crate::xc::{Rung, Variable};

#[derive(Clone, Debug, PartialEq)]
pub struct Local {
    v: Vec<f64>,
    v_tau: Option<Vec<f64>>,
}

impl Local {
    pub fn of(v: Vec<f64>, v_tau: Option<Vec<f64>>, rung: Rung) -> Result<Self, String> {
        match (&v_tau, rung.reads(Variable::TauUp)) {
            (Some(_), false) => Err(format!("v_τ: given, but the {rung:?} rung reads no τ")),
            (None, true) => Err(format!("v_τ: none, but the {rung:?} rung reads τ")),
            (Some(t), true) if t.len() != v.len() => {
                Err(format!("v_τ: {} values for {} of v", t.len(), v.len()))
            }
            _ => Ok(Self { v, v_tau }),
        }
    }

    pub fn v(&self) -> &[f64] {
        &self.v
    }

    pub fn v_tau(&self) -> Option<&[f64]> {
        self.v_tau.as_deref()
    }
}

pub(super) struct Field {
    pub(super) local: Vec<Local>,
    pub(super) d: Vec<Vec<Mat<f64>>>,
    pub(super) energy_d: Vec<Vec<Mat<f64>>>,
    pub(super) hartree_ha: f64,
    pub(super) xc_ha: f64,
    pub(super) local_ha: f64,
    pub(super) one_center_ha: f64,
}
