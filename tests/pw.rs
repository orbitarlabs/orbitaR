mod basis;
mod eigensolver;
mod grid;
mod parallel;
mod poisson;
mod ritz;
mod xc;

use orbitar::pw::{MatRef, Scalar, c64};

trait Parts: Scalar {
    fn parts(self) -> [f64; 2];
}

impl Parts for c64 {
    fn parts(self) -> [f64; 2] {
        [self.re, self.im]
    }
}

impl Parts for f64 {
    fn parts(self) -> [f64; 2] {
        [self, 0.0]
    }
}

fn entry<T: Scalar>(re: f64, im: f64) -> T {
    let mut parts = [re, im].into_iter();
    T::draw(|| parts.next().expect("two parts"))
}

fn bits<T: Parts>(m: MatRef<'_, T>) -> Vec<[u64; 2]> {
    (0..m.ncols())
        .flat_map(|j| (0..m.nrows()).map(move |i| m[(i, j)].parts().map(f64::to_bits)))
        .collect()
}
