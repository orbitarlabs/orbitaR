use super::Basis;
use super::parallel::{per_chunk, per_point, per_point_with};
use crate::model::Cell;
use compensated_summation::KahanBabuskaNeumaier;
use rustfft::{Fft, FftPlanner, num_complex::Complex64};
use std::sync::Arc;

const RECIPROCAL_SPHERE_SQUARED_MARGIN: f64 = 1.0000001;

pub struct Grid {
    cell: Cell,
    dims: [usize; 3],
    forward: [Arc<dyn Fft<f64>>; 3],
    inverse: [Arc<dyn Fft<f64>>; 3],
}

impl Grid {
    pub fn coarse(cell: &Cell, cutoff_ha: f64) -> Self {
        Self::sized(cell, box_size(cell, cutoff_ha, [2; 3]))
    }

    pub fn fine(cutoff_ha: f64, coarse: &Grid) -> Self {
        Self::sized(&coarse.cell, box_size(&coarse.cell, cutoff_ha, coarse.dims))
    }

    fn sized(cell: &Cell, dims: [usize; 3]) -> Self {
        let mut planner = FftPlanner::new();
        Self {
            cell: *cell,
            dims,
            forward: dims.map(|n| planner.plan_fft_forward(n)),
            inverse: dims.map(|n| planner.plan_fft_inverse(n)),
        }
    }

    pub fn cell(&self) -> &Cell {
        &self.cell
    }

    pub fn dims(&self) -> [usize; 3] {
        self.dims
    }

    pub fn points(&self) -> Vec<[f64; 3]> {
        self.indices()
            .map(|t| std::array::from_fn(|k| t[k] as f64 / self.dims[k] as f64))
            .collect()
    }

    pub fn within(&self, center: [f64; 3], radius: f64) -> Vec<(usize, [f64; 3], f64)> {
        let reach = self.cell.reach(radius);
        let [near1, near2, near3] =
            std::array::from_fn(|k| planes(center[k], reach[k], self.dims[k]));
        let [_, n2, n3] = self.dims;
        let mut found = Vec::new();
        for &t1 in &near1 {
            for &t2 in &near2 {
                for &t3 in &near3 {
                    let t = [t1, t2, t3];
                    let offset =
                        std::array::from_fn(|k| t[k] as f64 / self.dims[k] as f64 - center[k]);
                    let images = self.cell.translates_within(offset, radius);
                    found.extend(
                        (images.into_iter())
                            .map(|(shift, distance)| ((t1 * n2 + t2) * n3 + t3, shift, distance)),
                    );
                }
            }
        }
        found
    }

    pub fn millers(&self) -> Vec<[i32; 3]> {
        let signed = |t: usize, n: usize| {
            if t <= n / 2 {
                t as i32
            } else {
                t as i32 - n as i32
            }
        };
        self.indices()
            .map(|t| std::array::from_fn(|k| signed(t[k], self.dims[k])))
            .collect()
    }

    pub fn wavevectors(&self) -> Vec<[f64; 3]> {
        self.millers()
            .into_iter()
            .map(|m| self.cell.wavevector(m.map(f64::from)))
            .collect()
    }

    pub fn density_sphere(&self, cutoff_ha: f64) -> Vec<(usize, f64)> {
        let limit = 8.0 * cutoff_ha * RECIPROCAL_SPHERE_SQUARED_MARGIN;
        let (axes, b) = (self.axes(), self.cell.reciprocal());
        let [_, n2, n3] = self.dims;
        let planes = per_point(self.dims[0], |i1| {
            let mut found = Vec::new();
            for i2 in 0..n2 {
                for i3 in 0..n3 {
                    let m = [axes[0][i1].miller, axes[1][i2].miller, axes[2][i3].miller];
                    let g2 = Cell::wavevector_in(b, m).iter().map(|x| x * x).sum::<f64>();
                    if g2 <= limit {
                        found.push(((i1 * n2 + i2) * n3 + i3, g2));
                    }
                }
            }
            found
        });
        planes.concat()
    }

    pub fn slot(&self, m: [i32; 3]) -> Option<usize> {
        let mut index = 0;
        for (k, &n) in self.dims.iter().enumerate() {
            let n = n as i32;
            if !(-(n - 1) / 2..=n / 2).contains(&m[k]) {
                return None;
            }
            index = index * n as usize + m[k].rem_euclid(n) as usize;
        }
        Some(index)
    }

    pub fn forward(&self, values: &mut [Complex64]) {
        self.transform(values, &self.forward);
    }

    pub fn inverse(&self, values: &mut [Complex64]) {
        self.transform(values, &self.inverse);
        let scale = 1.0 / values.len() as f64;
        per_chunk(
            values,
            self.plane(),
            || (),
            |(), _, slab| {
                slab.iter_mut().for_each(|v| *v *= scale);
            },
        );
    }

    pub fn gradient(&self, f: &[f64]) -> [Vec<f64>; 3] {
        let [gradient] = self.gradients(&[f]).try_into().expect("one field");
        gradient
    }

    pub fn gradients(&self, fields: &[&[f64]]) -> Vec<[Vec<f64>; 3]> {
        let n = self.len();
        assert!(
            fields.iter().all(|f| f.len() == n),
            "one value per grid point"
        );
        let spectra = self.forward_real(fields.len(), |c, r| fields[c][r]);
        let mut parts = self
            .inverse_real(3 * fields.len(), |k, at| {
                spectra.at(k / 3, at) * Complex64::new(0.0, at.g[k % 3])
            })
            .into_iter();
        (0..fields.len())
            .map(|_| std::array::from_fn(|_| parts.next().expect("three per field")))
            .collect()
    }

    pub fn divergence(&self, h: &[Vec<f64>; 3]) -> Vec<f64> {
        let n = self.len();
        assert!(h.iter().all(|f| f.len() == n), "one value per grid point");
        let [divergence] = (self.divergences(1, |_, a, r| h[a][r]))
            .try_into()
            .expect("one field");
        divergence
    }

    pub fn divergences(
        &self,
        count: usize,
        h: impl Fn(usize, usize, usize) -> f64 + Sync + Send,
    ) -> Vec<Vec<f64>> {
        let spectra = self.forward_real(3 * count, |k, r| h(k / 3, k % 3, r));
        self.inverse_real(count, |s, at| {
            let mut sum = Complex64::default();
            for axis in 0..3 {
                sum += spectra.at(3 * s + axis, at) * Complex64::new(0.0, at.g[axis]);
            }
            sum
        })
    }

    pub fn integral(&self, values: impl ExactSizeIterator<Item = f64>) -> f64 {
        let n = self.len();
        assert_eq!(values.len(), n, "one value per grid point");
        let element = self.cell.volume() / n as f64;
        values.sum::<KahanBabuskaNeumaier<f64>>().total() * element
    }

    pub fn synthesized(
        &self,
        slots: impl Iterator<Item = usize>,
        coefficients: impl Iterator<Item = Complex64>,
    ) -> Vec<f64> {
        let n = self.len();
        let mut values = vec![Complex64::default(); n];
        for (slot, c) in slots.zip(coefficients) {
            values[slot] = c * n as f64;
        }
        self.inverse(&mut values);
        per_point(n, |r| values[r].re)
    }

    pub fn coefficients(
        &self,
        values: &[f64],
        slots: impl Iterator<Item = usize>,
    ) -> Vec<Complex64> {
        let n = self.len();
        assert_eq!(values.len(), n, "one value per grid point");
        let mut values: Vec<Complex64> = values.iter().map(|&x| x.into()).collect();
        self.forward(&mut values);
        slots.map(|slot| values[slot] / n as f64).collect()
    }

    pub fn transfer(&self, values: &[f64], to: &Grid) -> Vec<f64> {
        assert_eq!(values.len(), self.len(), "one value per grid point");
        let [transferred] = (self.transfers(1, |_, r| values[r], to))
            .try_into()
            .expect("one field");
        transferred
    }

    pub fn transfers(
        &self,
        count: usize,
        f: impl Fn(usize, usize) -> f64 + Sync + Send,
        to: &Grid,
    ) -> Vec<Vec<f64>> {
        assert_eq!(self.cell, to.cell, "transfer between boxes of one cell");
        let sources: [Vec<Option<usize>>; 3] = std::array::from_fn(|k| {
            (to.axes()[k].iter())
                .map(|a| {
                    let n = self.dims[k] as i32;
                    let held = (-(n - 1) / 2..=n / 2).contains(&a.index);
                    let nyquist = |m: i32, n: usize| n.is_multiple_of(2) && m == n as i32 / 2;
                    (held && !a.nyquist && !nyquist(a.index, self.dims[k]))
                        .then(|| a.index.rem_euclid(n) as usize)
                })
                .collect()
        });
        let scale = to.len() as f64 / self.len() as f64;
        let [_, n2, n3] = to.dims;
        let [_, m2, m3] = self.dims;
        let mut transferred = Vec::with_capacity(count);
        for pair in (0..count).step_by(2) {
            let mut values = self.packed(count, pair, &f);
            self.forward(&mut values);
            let mut g = vec![Complex64::default(); to.len()];
            per_chunk(
                &mut g,
                n2 * n3,
                || (),
                |(), i1, slab| {
                    let Some(s1) = sources[0][i1] else { return };
                    for (i2, row) in slab.chunks_exact_mut(n3).enumerate() {
                        let Some(s2) = sources[1][i2] else { continue };
                        for (i3, x) in row.iter_mut().enumerate() {
                            if let Some(s3) = sources[2][i3] {
                                *x = values[(s1 * m2 + s2) * m3 + s3] * scale;
                            }
                        }
                    }
                },
            );
            to.inverse(&mut g);
            transferred.push(per_point(to.len(), |r| g[r].re));
            if pair + 1 < count {
                transferred.push(per_point(to.len(), |r| g[r].im));
            }
        }
        transferred
    }

    pub fn sphere(&self, millers: &[[i32; 3]]) -> Result<SphereFft, String> {
        let slots = (millers.iter())
            .map(|&m| {
                self.slot(m).ok_or_else(|| {
                    format!("plane wave {m:?} lies outside the {:?} grid", self.dims)
                })
            })
            .collect::<Result<Vec<_>, _>>()?;
        let [n1, n2, n3] = self.dims;
        let (mut rows, mut planes) = (vec![false; n1 * n2], vec![false; n1]);
        let (mut columns, mut lines) = (vec![false; n3], vec![false; n2 * n3]);
        for &slot in &slots {
            let (row, i3) = (slot / n3, slot % n3);
            (rows[row], planes[row / n2], columns[i3]) = (true, true, true);
            lines[row % n2 * n3 + i3] = true;
        }
        let picked = |mask: &[bool]| (0..mask.len()).filter(|&i| mask[i]).collect::<Vec<_>>();
        Ok(SphereFft {
            dims: self.dims,
            forward: self.forward.clone(),
            inverse: self.inverse.clone(),
            slots,
            rows,
            planes,
            columns: picked(&columns),
            lines: lines.chunks_exact(n3).map(picked).collect(),
        })
    }

    fn len(&self) -> usize {
        self.dims.iter().product()
    }

    fn plane(&self) -> usize {
        self.dims[1] * self.dims[2]
    }

    fn axes(&self) -> [Vec<Axis>; 3] {
        std::array::from_fn(|k| {
            let n = self.dims[k];
            (0..n)
                .map(|t| {
                    let index = if t <= n / 2 {
                        t as i32
                    } else {
                        t as i32 - n as i32
                    };
                    let nyquist = n.is_multiple_of(2) && t == n / 2;
                    Axis {
                        index,
                        miller: f64::from(index),
                        tilde: if nyquist { 0.0 } else { f64::from(index) },
                        nyquist,
                        partner: (n - t) % n,
                    }
                })
                .collect()
        })
    }

    fn packed(
        &self,
        count: usize,
        pair: usize,
        f: &(impl Fn(usize, usize) -> f64 + Sync + Send),
    ) -> Vec<Complex64> {
        per_point(self.len(), |r| {
            let im = if pair + 1 < count {
                f(pair + 1, r)
            } else {
                0.0
            };
            Complex64::new(f(pair, r), im)
        })
    }

    fn forward_real(&self, count: usize, f: impl Fn(usize, usize) -> f64 + Sync + Send) -> Spectra {
        let transforms = (0..count)
            .step_by(2)
            .map(|pair| {
                let mut values = self.packed(count, pair, &f);
                self.forward(&mut values);
                values
            })
            .collect();
        Spectra(transforms)
    }

    fn inverse_real(
        &self,
        count: usize,
        spectrum: impl Fn(usize, At) -> Complex64 + Sync + Send,
    ) -> Vec<Vec<f64>> {
        let (axes, b) = (self.axes(), self.cell.reciprocal());
        let [_, n2, n3] = self.dims;
        let mut fields = Vec::with_capacity(count);
        for pair in (0..count).step_by(2) {
            let mut values = vec![Complex64::default(); self.len()];
            per_chunk(
                &mut values,
                n2 * n3,
                || (),
                |(), i1, slab| {
                    let (a1, p1) = (&axes[0][i1], axes[0][i1].partner);
                    for (i2, a2) in axes[1].iter().enumerate() {
                        for (i3, a3) in axes[2].iter().enumerate() {
                            let at = At {
                                slot: (i1 * n2 + i2) * n3 + i3,
                                partner: (p1 * n2 + a2.partner) * n3 + a3.partner,
                                g: Cell::wavevector_in(b, [a1.tilde, a2.tilde, a3.tilde]),
                            };
                            let x = spectrum(pair, at);
                            let y = match pair + 1 < count {
                                true => spectrum(pair + 1, at),
                                false => Complex64::default(),
                            };
                            slab[i2 * n3 + i3] = Complex64::new(x.re - y.im, x.im + y.re);
                        }
                    }
                },
            );
            self.inverse(&mut values);
            fields.push(per_point(self.len(), |r| values[r].re));
            if pair + 1 < count {
                fields.push(per_point(self.len(), |r| values[r].im));
            }
        }
        fields
    }

    fn indices(&self) -> impl Iterator<Item = [usize; 3]> {
        let [n1, n2, n3] = self.dims;
        (0..n1 * n2 * n3).map(move |i| [i / (n2 * n3), i / n3 % n2, i % n3])
    }

    fn transform(&self, values: &mut [Complex64], ffts: &[Arc<dyn Fft<f64>>; 3]) {
        let [n1, n2, n3] = self.dims;
        assert_eq!(values.len(), n1 * n2 * n3, "one value per grid point");
        let scratch = || vec![Complex64::default(); scratch_len(ffts)];
        let buffers = || (scratch(), vec![Complex64::default(); n2 * n3]);
        per_chunk(
            values,
            self.plane(),
            buffers,
            |(scratch, lines), _, slab| {
                ffts[2].process_with_scratch(slab, scratch);
                transpose(slab, lines, n2, n3);
                ffts[1].process_with_scratch(lines, scratch);
                transpose(lines, slab, n3, n2);
            },
        );
        let values_ref = &*values;
        let gathered = per_point_with(n2, scratch, |scratch, i2| {
            let mut lines = vec![Complex64::default(); n1 * n3];
            for i1 in 0..n1 {
                let row = &values_ref[(i1 * n2 + i2) * n3..][..n3];
                for (i3, x) in row.iter().enumerate() {
                    lines[i3 * n1 + i1] = *x;
                }
            }
            ffts[0].process_with_scratch(&mut lines, scratch);
            lines
        });
        per_chunk(
            values,
            n2 * n3,
            || (),
            |(), i1, slab| {
                for (i2, lines) in gathered.iter().enumerate() {
                    for (i3, x) in slab[i2 * n3..][..n3].iter_mut().enumerate() {
                        *x = lines[i3 * n1 + i1];
                    }
                }
            },
        );
    }
}

pub struct SphereFft {
    dims: [usize; 3],
    forward: [Arc<dyn Fft<f64>>; 3],
    inverse: [Arc<dyn Fft<f64>>; 3],
    slots: Vec<usize>,
    rows: Vec<bool>,
    planes: Vec<bool>,
    columns: Vec<usize>,
    lines: Vec<Vec<usize>>,
}

impl SphereFft {
    pub(crate) fn len(&self) -> usize {
        self.slots.len()
    }

    pub fn inverse(
        &self,
        coefficients: impl ExactSizeIterator<Item = Complex64>,
        values: &mut [Complex64],
    ) {
        let [n1, n2, n3] = self.dims;
        assert_eq!(values.len(), n1 * n2 * n3, "one value per grid point");
        let count = coefficients.len();
        assert_eq!(count, self.slots.len(), "one coefficient per plane wave");
        let zero = Complex64::default();
        let mut scratch = vec![zero; scratch_len(&self.inverse)];
        let live = |row: usize| self.rows[row];
        for row in (0..n1 * n2).filter(|&r| live(r)) {
            values[row * n3..][..n3].fill(zero);
        }
        for (&slot, c) in self.slots.iter().zip(coefficients) {
            values[slot] = c;
        }
        for row in (0..n1 * n2).filter(|&r| live(r)) {
            self.inverse[2].process_with_scratch(&mut values[row * n3..][..n3], &mut scratch);
        }
        let mut plane = vec![zero; n1.max(n2) * n3];
        let lines = &mut plane[..n2 * n3];
        for i1 in (0..n1).filter(|&i1| self.planes[i1]) {
            let slab = &mut values[i1 * n2 * n3..][..n2 * n3];
            for i2 in 0..n2 {
                let row = &slab[i2 * n3..][..n3];
                for i3 in 0..n3 {
                    lines[i3 * n2 + i2] = if live(i1 * n2 + i2) { row[i3] } else { zero };
                }
            }
            self.inverse[1].process_with_scratch(lines, &mut scratch);
            transpose(lines, slab, n3, n2);
        }
        let scale = 1.0 / values.len() as f64;
        let lines = &mut plane[..n1 * n3];
        for i2 in 0..n2 {
            for i1 in 0..n1 {
                let row = &values[(i1 * n2 + i2) * n3..][..n3];
                for i3 in 0..n3 {
                    lines[i3 * n1 + i1] = if self.planes[i1] { row[i3] } else { zero };
                }
            }
            self.inverse[0].process_with_scratch(lines, &mut scratch);
            for i1 in 0..n1 {
                let row = &mut values[(i1 * n2 + i2) * n3..][..n3];
                for (i3, x) in row.iter_mut().enumerate() {
                    *x = lines[i3 * n1 + i1] * scale;
                }
            }
        }
    }

    pub fn forward(&self, values: &mut [Complex64]) -> impl Iterator<Item = Complex64> {
        let [n1, n2, n3] = self.dims;
        assert_eq!(values.len(), n1 * n2 * n3, "one value per grid point");
        let zero = Complex64::default();
        let mut scratch = vec![zero; scratch_len(&self.forward)];
        self.forward[2].process_with_scratch(values, &mut scratch);
        let mut plane = vec![zero; n1.max(n2) * n3];
        let lines = &mut plane[..n2 * self.columns.len()];
        for slab in values.chunks_exact_mut(n2 * n3) {
            for (c, &i3) in self.columns.iter().enumerate() {
                for i2 in 0..n2 {
                    lines[c * n2 + i2] = slab[i2 * n3 + i3];
                }
            }
            self.forward[1].process_with_scratch(lines, &mut scratch);
            for (c, &i3) in self.columns.iter().enumerate() {
                for i2 in 0..n2 {
                    slab[i2 * n3 + i3] = lines[c * n2 + i2];
                }
            }
        }
        for (i2, columns) in self.lines.iter().enumerate() {
            let lines = &mut plane[..n1 * columns.len()];
            for (c, &i3) in columns.iter().enumerate() {
                for i1 in 0..n1 {
                    lines[c * n1 + i1] = values[(i1 * n2 + i2) * n3 + i3];
                }
            }
            self.forward[0].process_with_scratch(lines, &mut scratch);
            for (c, &i3) in columns.iter().enumerate() {
                for i1 in 0..n1 {
                    values[(i1 * n2 + i2) * n3 + i3] = lines[c * n1 + i1];
                }
            }
        }
        let values = &*values;
        self.slots.iter().map(move |&slot| values[slot])
    }
}

pub fn kinetic_density(
    fft: &SphereFft,
    basis: &Basis,
    coefficients: impl Fn(usize) -> Complex64,
    scale: f64,
    buffer: &mut [Complex64],
    tau: &mut [f64],
) {
    let n = buffer.len();
    let wavevectors = basis.wavevectors();
    assert_eq!(tau.len(), n, "one τ per grid point");
    assert_eq!(
        wavevectors.len(),
        fft.len(),
        "`fft` is the sphere of `basis`"
    );
    for axis in 0..3 {
        let column = (0..wavevectors.len()).map(|g| coefficients(g) * wavevectors[g][axis]);
        fft.inverse(column, buffer);
        for (t, u) in tau.iter_mut().zip(buffer.iter()) {
            *t = if axis == 0 {
                u.norm_sqr()
            } else {
                *t + u.norm_sqr()
            };
        }
    }
    let factor = 0.5 * scale * (n * n) as f64;
    tau.iter_mut().for_each(|t| *t *= factor);
}

pub fn kinetic_strain(
    fft: &SphereFft,
    basis: &Basis,
    coefficients: impl Fn(usize) -> Complex64,
    scale: f64,
    v: &[f64],
    buffers: &mut [Complex64],
) -> [[f64; 3]; 3] {
    let n = v.len();
    let wavevectors = basis.wavevectors();
    assert_eq!(buffers.len(), 3 * n, "three grids of scratch");
    assert_eq!(
        wavevectors.len(),
        fft.len(),
        "`fft` is the sphere of `basis`"
    );
    for (axis, buffer) in buffers.chunks_exact_mut(n).enumerate() {
        let column = (0..wavevectors.len()).map(|g| coefficients(g) * wavevectors[g][axis]);
        fft.inverse(column, buffer);
    }
    let [x, y, z] = [0, 1, 2].map(|axis| &buffers[axis * n..(axis + 1) * n]);
    let parts = [x, y, z];
    let mut sums = [[0.0; 3]; 3];
    for i in 0..3 {
        for j in i..3 {
            let sum: f64 = (parts[i].iter().zip(parts[j]).zip(v))
                .map(|((a, b), v)| v * (a.conj() * b).re)
                .sum();
            sums[i][j] = sum;
            sums[j][i] = sum;
        }
    }
    let factor = scale * (n * n) as f64;
    sums.map(|row| row.map(|s| s * factor))
}

fn scratch_len(ffts: &[Arc<dyn Fft<f64>>; 3]) -> usize {
    (ffts.iter())
        .map(|f| f.get_inplace_scratch_len())
        .max()
        .unwrap_or(0)
}

fn transpose(from: &[Complex64], to: &mut [Complex64], rows: usize, columns: usize) {
    for r in 0..rows {
        for c in 0..columns {
            to[c * rows + r] = from[r * columns + c];
        }
    }
}

fn box_size(cell: &Cell, cutoff_ha: f64, start: [usize; 3]) -> [usize; 3] {
    assert!(
        cutoff_ha.is_finite() && cutoff_ha > 0.0,
        "a box needs a finite positive cutoff"
    );
    let mut n = start;
    loop {
        let h = n.map(|n| (n / 2) as i32);
        let (mut shortest, mut axis) = (f64::INFINITY, 0);
        for p in 0..3 {
            let (a, b) = [(1, 2), (0, 2), (0, 1)][p];
            for u in -h[a]..=h[a] {
                for v in -h[b]..=h[b] {
                    let mut m = [0.0; 3];
                    (m[p], m[a], m[b]) = (f64::from(h[p]), f64::from(u), f64::from(v));
                    let g2: f64 = cell.wavevector(m).iter().map(|x| x * x).sum();
                    if g2 < shortest {
                        (shortest, axis) = (g2, p);
                    }
                }
            }
        }
        if shortest >= 8.0 * cutoff_ha {
            return n;
        }
        n[axis] = smooth(n[axis] + 1);
    }
}

fn planes(center: f64, reach: f64, n: usize) -> Vec<usize> {
    let plane = |x: f64| x * n as f64;
    let unwrapped = plane(center - reach).floor() as i64..=plane(center + reach).ceil() as i64;
    let mut planes: Vec<usize> = unwrapped.map(|u| u.rem_euclid(n as i64) as usize).collect();
    planes.sort_unstable();
    planes.dedup();
    planes
}

fn smooth(n: usize) -> usize {
    let only_2_3_5 = |mut m: usize| {
        for p in [2, 3, 5] {
            while m.is_multiple_of(p) {
                m /= p;
            }
        }
        m == 1
    };
    (n..)
        .find(|&m| only_2_3_5(m))
        .expect("a power of 2 lies ahead")
}

struct Axis {
    index: i32,
    miller: f64,
    tilde: f64,
    nyquist: bool,
    partner: usize,
}

#[derive(Clone, Copy)]
struct At {
    slot: usize,
    partner: usize,
    g: [f64; 3],
}

struct Spectra(Vec<Vec<Complex64>>);

impl Spectra {
    fn at(&self, k: usize, at: At) -> Complex64 {
        let packed = &self.0[k / 2];
        let (p, q) = (packed[at.slot], packed[at.partner].conj());
        match k % 2 {
            0 => (p + q) * 0.5,
            _ => (p - q) * Complex64::new(0.0, -0.5),
        }
    }
}
