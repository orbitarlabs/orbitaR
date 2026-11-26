use crate::pw::{Basis, Complex64, Mat, MatRef, SphereFft, c64, kinetic_density, per_chunk};

#[derive(Clone)]
pub(super) struct Density {
    pub(super) smooth: Vec<Vec<c64>>,
    pub(super) onsite: Vec<Vec<Mat<f64>>>,
    pub(super) kinetic: Option<Vec<Vec<c64>>>,
}

impl Density {
    pub(super) fn moved(&self, from: &Density, to: &Density) -> Density {
        let fields = |s: &[Vec<c64>], f: &[Vec<c64>], t: &[Vec<c64>]| {
            (s.iter().zip(f).zip(t))
                .map(|((s, f), t)| {
                    (s.iter().zip(f).zip(t))
                        .map(|((s, f), t)| s - f + t)
                        .collect()
                })
                .collect()
        };
        let onsite = (self.onsite.iter().zip(&from.onsite).zip(&to.onsite))
            .map(|((s, f), t)| {
                (s.iter().zip(f).zip(t))
                    .map(|((s, f), t)| s - f + t)
                    .collect()
            })
            .collect();
        let kinetic = match (&self.kinetic, &from.kinetic, &to.kinetic) {
            (Some(s), Some(f), Some(t)) => Some(fields(s, f, t)),
            _ => None,
        };
        Density {
            smooth: fields(&self.smooth, &from.smooth, &to.smooth),
            onsite,
            kinetic,
        }
    }

    pub(super) fn turned(&self, scale: f64) -> Self {
        let [up, down] = self.smooth.as_slice() else {
            return self.clone();
        };
        let combined = |up: &[c64], down: &[c64]| {
            [1.0, -1.0].map(|sign| {
                (up.iter().zip(down))
                    .map(|(a, b)| scale * (a + sign * b))
                    .collect::<Vec<c64>>()
            })
        };
        let onsite = (self.onsite.iter())
            .map(|rho| vec![(&rho[0] + &rho[1]) * scale, (&rho[0] - &rho[1]) * scale])
            .collect();
        let kinetic = (self.kinetic.as_ref()).map(|tau| combined(&tau[0], &tau[1]).to_vec());
        Self {
            smooth: combined(up, down).to_vec(),
            onsite,
            kinetic,
        }
    }
}

pub(super) struct Occupied<'a> {
    pub(super) fft: &'a SphereFft,
    pub(super) basis: &'a Basis,
    pub(super) vectors: MatRef<'a, c64>,
    pub(super) occupations: &'a [f64],
    pub(super) weight: f64,
    pub(super) projectors: &'a [Mat<c64>],
}

pub(super) struct Sums {
    pub(super) density: Vec<f64>,
    pub(super) kinetic: Option<Vec<f64>>,
    pub(super) occupancies: Vec<Mat<f64>>,
    pub(super) kinetic_ha: f64,
}

impl Sums {
    pub(super) fn zero(points: usize, sizes: &[usize], kinetic: bool, kinetic_ha: f64) -> Self {
        Self {
            density: vec![0.0; points],
            kinetic: kinetic.then(|| vec![0.0; points]),
            occupancies: sizes.iter().map(|&size| Mat::zeros(size, size)).collect(),
            kinetic_ha,
        }
    }

    pub(super) fn add(&mut self, k: &Occupied<'_>, volume: f64, slab: &mut [Complex64]) {
        let n = self.density.len();
        let scale = (n * n) as f64 / volume;
        let (x, f) = (k.vectors.subcols(0, k.occupations.len()), k.occupations);
        let occupied: Vec<(usize, f64)> = (f.iter().enumerate())
            .filter(|&(_, &f)| f != 0.0)
            .map(|(band, &occupation)| (band, k.weight * occupation))
            .collect();
        for &(band, w) in &occupied {
            for (g, t) in k.basis.kinetic_ha().iter().enumerate() {
                self.kinetic_ha += w * t * x[(g, band)].norm_sqr();
            }
        }
        let tau = self.kinetic.is_some();
        for bands in occupied.chunks(slab.len() / n) {
            let slab = &mut slab[..bands.len() * n];
            per_chunk(
                slab,
                n,
                || tau.then(|| (vec![Complex64::default(); n], vec![0.0; n])),
                |buffers, i, u| {
                    let (band, w) = bands[i];
                    (k.fft).inverse((0..x.nrows()).map(|g| x[(g, band)]), u);
                    u.iter_mut()
                        .for_each(|z| *z = (w * scale * z.norm_sqr()).into());
                    if let Some((buffer, share)) = buffers {
                        let c = |g: usize| x[(g, band)];
                        kinetic_density(k.fft, k.basis, c, w / volume, buffer, share);
                        u.iter_mut().zip(share.iter()).for_each(|(z, t)| z.im = *t);
                    }
                },
            );
            for share in slab.chunks_exact(n) {
                for (value, s) in self.density.iter_mut().zip(share) {
                    *value += s.re;
                }
                if let Some(kinetic) = &mut self.kinetic {
                    for (value, s) in kinetic.iter_mut().zip(share) {
                        *value += s.im;
                    }
                }
            }
        }
        for (rho, projectors) in self.occupancies.iter_mut().zip(k.projectors) {
            let p = projectors.adjoint() * x;
            for i in 0..rho.nrows() {
                for j in 0..rho.ncols() {
                    rho[(i, j)] += (0..f.len())
                        .map(|band| k.weight * f[band] * (p[(i, band)] * p[(j, band)].conj()).re)
                        .sum::<f64>();
                }
            }
        }
    }
}
