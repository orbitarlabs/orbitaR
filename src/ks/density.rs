use crate::pw::{
    Basis, Complex64, Mat, MatRef, Pairs, SphereFft, c64, kinetic_density, kinetic_density_of_pair,
    per_chunk, per_point,
};

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

#[derive(Clone, Copy)]
pub(super) enum Columns<'a> {
    Complex(MatRef<'a, c64>),
    Real {
        vectors: MatRef<'a, f64>,
        pairs: &'a Pairs,
        phases: &'a [Complex64],
    },
}

impl Columns<'_> {
    fn coefficient(&self, g: usize, band: usize) -> c64 {
        match self {
            Self::Complex(x) => x[(g, band)],
            Self::Real { vectors, pairs, .. } => pairs.coefficient(|row| vectors[(row, band)], g),
        }
    }

    fn square(&self, g: usize, band: usize) -> f64 {
        match self {
            Self::Complex(x) => x[(g, band)].norm_sqr(),
            Self::Real { vectors, .. } => vectors[(g, band)] * vectors[(g, band)],
        }
    }

    fn projections(&self, projectors: &Mat<c64>, bands: usize) -> Mat<c64> {
        match self {
            Self::Complex(x) => projectors.adjoint() * x.subcols(0, bands),
            Self::Real { vectors, pairs, .. } => {
                let p = pairs.real(projectors.as_ref()).transpose() * vectors.subcols(0, bands);
                Mat::from_fn(p.nrows(), p.ncols(), |i, j| c64::new(p[(i, j)], 0.0))
            }
        }
    }
}

pub(super) struct Occupied<'a> {
    pub(super) fft: &'a SphereFft,
    pub(super) basis: &'a Basis,
    pub(super) vectors: Columns<'a>,
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
        let (x, f) = (k.vectors, k.occupations);
        let occupied: Vec<(usize, f64)> = (f.iter().enumerate())
            .filter(|&(_, &f)| f != 0.0)
            .map(|(band, &occupation)| (band, k.weight * occupation))
            .collect();
        for &(band, w) in &occupied {
            for (g, t) in k.basis.kinetic_ha().iter().enumerate() {
                self.kinetic_ha += w * t * x.square(g, band);
            }
        }
        let tau = self.kinetic.is_some();
        let per_transform = match x {
            Columns::Complex(_) => 1,
            Columns::Real { .. } => 2,
        };
        let tasks: Vec<&[(usize, f64)]> = occupied.chunks(per_transform).collect();
        let rows = k.basis.millers().len();
        for bands in tasks.chunks(slab.len() / n) {
            let slab = &mut slab[..bands.len() * n];
            per_chunk(
                slab,
                n,
                || tau.then(|| (vec![Complex64::default(); n], vec![0.0; n])),
                |buffers, i, u| match (bands[i], x) {
                    (
                        &[(a, wa), (b, wb)],
                        Columns::Real {
                            vectors,
                            pairs,
                            phases,
                        },
                    ) => {
                        let packed = |g: usize| {
                            pairs.packed(|row| vectors[(row, a)], |row| vectors[(row, b)], g)
                        };
                        (k.fft).inverse((0..rows).map(packed), u);
                        for (z, phase) in u.iter_mut().zip(phases) {
                            let y = phase * *z;
                            *z = (scale * (wa * y.re * y.re + wb * y.im * y.im)).into();
                        }
                        if let Some((buffer, share)) = buffers {
                            let scales = [wa / volume, wb / volume];
                            kinetic_density_of_pair(
                                k.fft, k.basis, packed, scales, phases, buffer, share,
                            );
                            u.iter_mut().zip(share.iter()).for_each(|(z, t)| z.im = *t);
                        }
                    }
                    (task, _) => {
                        let [(band, w)] = task else {
                            unreachable!("two bands share a transform only in the real form")
                        };
                        let c = |g: usize| x.coefficient(g, *band);
                        (k.fft).inverse((0..rows).map(c), u);
                        u.iter_mut()
                            .for_each(|z| *z = (w * scale * z.norm_sqr()).into());
                        if let Some((buffer, share)) = buffers {
                            kinetic_density(k.fft, k.basis, c, w / volume, buffer, share);
                            u.iter_mut().zip(share.iter()).for_each(|(z, t)| z.im = *t);
                        }
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
        let added = per_point(k.projectors.len(), |atom| {
            let p = x.projections(&k.projectors[atom], f.len());
            Mat::from_fn(p.nrows(), p.nrows(), |i, j| {
                (0..f.len())
                    .map(|band| k.weight * f[band] * (p[(i, band)] * p[(j, band)].conj()).re)
                    .sum::<f64>()
            })
        });
        for (rho, added) in self.occupancies.iter_mut().zip(added) {
            *rho += added;
        }
    }
}
