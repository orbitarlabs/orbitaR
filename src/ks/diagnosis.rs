use crate::model::Functional;
use crate::paw::Species;
pub use crate::pw::GridGap;
use crate::pw::{GridPoint, Mat, grid_gap};
use std::sync::Arc;

pub struct Diagnosis {
    pub(super) functional: Functional,
    pub(super) frame: [[f64; 3]; 3],
    pub(super) element: f64,
    pub(super) dims: [usize; 3],
    pub(super) spheres: Vec<SphereState>,
    pub(super) grid: Vec<GridPoint>,
}

pub(super) struct SphereState {
    pub(super) species: Arc<Species>,
    pub(super) images: Vec<Mat<f64>>,
    pub(super) rho: Vec<Mat<f64>>,
}

impl Diagnosis {
    pub fn sphere_energy_ha(&self, frame_of: &Diagnosis) -> f64 {
        (self.spheres.iter())
            .map(|s| {
                let turn = s.species.rotation(frame_of.frame);
                s.species.sphere_terms(Some(&turn), &s.images, &s.rho).0
            })
            .sum()
    }

    pub fn sphere_contraction_ha(&self, other: &Diagnosis) -> Result<f64, String> {
        self.same_layout(other)?;
        let mut sum = 0.0;
        for (s, o) in self.spheres.iter().zip(&other.spheres) {
            let turn = s.species.rotation(self.frame);
            let (_, d) = s.species.sphere_terms(Some(&turn), &s.images, &s.rho);
            for (d, rho) in d.iter().zip(&o.rho) {
                for i in 0..d.nrows() {
                    for j in 0..d.ncols() {
                        sum += d[(i, j)] * rho[(i, j)];
                    }
                }
            }
        }
        Ok(sum)
    }

    pub fn grid_gap(&self, plus: &Diagnosis, minus: &Diagnosis) -> Result<GridGap, String> {
        for other in [plus, minus] {
            self.same_layout(other)?;
        }
        grid_gap(
            self.functional,
            self.element,
            &self.grid,
            &plus.grid,
            &minus.grid,
        )
    }

    fn same_layout(&self, other: &Diagnosis) -> Result<(), String> {
        if self.functional != other.functional {
            return Err(format!(
                "diagnosis: the functionals differ ({:?}, {:?})",
                self.functional, other.functional
            ));
        }
        if self.spheres.len() != other.spheres.len() {
            return Err(format!(
                "diagnosis: {} atoms against {}",
                self.spheres.len(),
                other.spheres.len()
            ));
        }
        for (a, (s, o)) in self.spheres.iter().zip(&other.spheres).enumerate() {
            let (ds, d_o) = (s.species.dataset().draft(), o.species.dataset().draft());
            if ds != d_o {
                let channels = |d: &crate::paw::DatasetDraft| {
                    d.channels.iter().map(|c| c.l).collect::<Vec<_>>()
                };
                return Err(format!(
                    "diagnosis: atom {a} has another dataset (Z {} with channels l = {:?}, against Z {} with {:?})",
                    ds.z,
                    channels(ds),
                    d_o.z,
                    channels(d_o)
                ));
            }
            if s.rho.len() != o.rho.len() {
                return Err(format!(
                    "diagnosis: atom {a} has {} spins against {}",
                    s.rho.len(),
                    o.rho.len()
                ));
            }
        }
        if self.dims != other.dims {
            return Err(format!(
                "diagnosis: the fine grids differ ({:?}, {:?})",
                self.dims, other.dims
            ));
        }
        Ok(())
    }
}
