use faer::linalg::matmul::matmul;
use faer::traits::math_utils::one;
use faer::traits::{ComplexField, Conjugate};
use faer::{Accum, ColMut, Mat, MatMut, MatRef, Par};
use rayon::{ThreadPool, ThreadPoolBuilder, Yield};
use std::cell::Cell;
use std::hint::spin_loop;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

const LINGER: Duration = Duration::from_millis(1);
const SPINS: usize = 64;
const BATCHES_PER_THREAD: usize = 4;

pub struct Pool {
    threads: ThreadPool,
}

thread_local! {
    static DEPTH: Cell<usize> = const { Cell::new(0) };
}

struct Frame;

impl Frame {
    fn enter() -> Frame {
        DEPTH.with(|d| d.set(d.get() + 1));
        Frame
    }
}

impl Drop for Frame {
    fn drop(&mut self) {
        DEPTH.with(|d| d.set(d.get() - 1));
    }
}

impl TryFrom<usize> for Pool {
    type Error = String;

    fn try_from(threads: usize) -> Result<Pool, String> {
        (ThreadPoolBuilder::new().num_threads(threads).build())
            .map(|threads| Pool { threads })
            .map_err(|e| format!("{threads} threads cannot start: {e}"))
    }
}

impl Pool {
    pub fn run<R: Send>(&self, op: impl FnOnce() -> R + Send) -> R {
        self.threads.spawn_broadcast(|_| look());
        self.threads.install(|| {
            let _frame = Frame::enter();
            op()
        })
    }
}

fn look() {
    if DEPTH.with(Cell::get) > 0 {
        return;
    }
    let _frame = Frame::enter();
    let mut last = Instant::now();
    loop {
        if worked() {
            last = Instant::now();
        } else if last.elapsed() >= LINGER {
            break;
        }
    }
}

fn worked() -> bool {
    if rayon::yield_now() == Some(Yield::Executed) {
        return true;
    }
    for _ in 0..SPINS {
        spin_loop();
    }
    false
}

pub fn per_point<T: Send>(n: usize, f: impl Fn(usize) -> T + Sync + Send) -> Vec<T> {
    per_point_with(n, || (), |(), i| f(i))
}

pub fn per_point_with<T: Send, B>(
    n: usize,
    init: impl Fn() -> B + Sync + Send,
    f: impl Fn(&mut B, usize) -> T + Sync + Send,
) -> Vec<T> {
    if !pooled() {
        let mut buffer = init();
        return (0..n).map(|i| f(&mut buffer, i)).collect();
    }
    per_batch(n, init, |buffer, points, out| {
        out.extend(points.map(|i| f(buffer, i)))
    })
}

pub fn per_chunk<T: Send, B>(
    values: &mut [T],
    size: usize,
    init: impl Fn() -> B + Sync + Send,
    f: impl Fn(&mut B, usize, &mut [T]) + Sync + Send,
) {
    let chunks: Vec<_> = values.chunks_mut(size).enumerate().collect();
    per_part(chunks, init, |b, (i, chunk)| f(b, i, chunk));
}

pub fn pool_threads() -> usize {
    if pooled() {
        rayon::current_num_threads()
    } else {
        1
    }
}

fn per_part<I, P, T, B>(
    parts: I,
    init: impl Fn() -> B + Sync + Send,
    f: impl Fn(&mut B, P) -> T + Sync + Send,
) -> Vec<T>
where
    I: IntoIterator<Item = P>,
    P: Send,
    T: Send,
{
    if !pooled() {
        let mut buffer = init();
        return parts.into_iter().map(|part| f(&mut buffer, part)).collect();
    }
    let parts: Vec<Mutex<Option<P>>> = parts.into_iter().map(|p| Mutex::new(Some(p))).collect();
    per_batch(parts.len(), init, |buffer, indices, out| {
        out.extend(indices.map(|i| {
            let part = parts[i]
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .take();
            f(buffer, part.expect("each part is taken once"))
        }));
    })
}

fn per_batch<T: Send, B>(
    n: usize,
    init: impl Fn() -> B + Sync + Send,
    run: impl Fn(&mut B, std::ops::Range<usize>, &mut Vec<T>) + Sync + Send,
) -> Vec<T> {
    let threads = rayon::current_num_threads();
    let size = batch_size(n, threads);
    let batches = n.div_ceil(size);
    let _frame = Frame::enter();
    let next = AtomicUsize::new(0);
    let range = |b: usize| b * size..n.min((b + 1) * size);
    let done: Vec<Mutex<Vec<T>>> = (0..batches)
        .map(|b| Mutex::new(Vec::with_capacity(range(b).len())))
        .collect();
    let take = || {
        let mut buffer = None;
        loop {
            let b = next.fetch_add(1, Ordering::Relaxed);
            let Some(slot) = done.get(b) else { break };
            let mut out = slot.lock().unwrap_or_else(PoisonError::into_inner);
            run(buffer.get_or_insert_with(&init), range(b), &mut out);
        }
    };
    let helpers = threads.min(batches).saturating_sub(1);
    let finished = AtomicUsize::new(0);
    rayon::scope(|s| {
        for _ in 0..helpers {
            s.spawn(|_| {
                let _frame = Frame::enter();
                let _helper_completion = HelperCompletion(&finished);
                take();
                rayon::spawn(look);
            });
        }
        take();
        while finished.load(Ordering::Acquire) < helpers {
            worked();
        }
    });
    let mut out = Vec::with_capacity(n);
    for batch in done {
        out.append(&mut batch.into_inner().unwrap_or_else(PoisonError::into_inner));
    }
    out
}

fn batch_size(n: usize, threads: usize) -> usize {
    n.div_ceil(BATCHES_PER_THREAD * threads).max(1)
}

struct HelperCompletion<'a>(&'a AtomicUsize);

impl Drop for HelperCompletion<'_> {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::Release);
    }
}

fn pooled() -> bool {
    rayon::current_thread_index().is_some()
}

const NANO: usize = 16 * 16 * 16;
const TALL: usize = 256;
const SHORT: usize = 8;
const SQUARE: usize = 64;
const WIDE: usize = 64;
const ENOUGH_TILES: usize = 16;

fn block_rows(rows: usize, columns: usize, inners: &[usize]) -> (usize, usize) {
    let size = if rows >= 2 * TALL { TALL } else { SHORT };
    let kernel = (inners.iter()).all(|&k| size * columns * k > NANO);
    (size, if kernel { (rows / size).max(1) } else { 1 })
}

fn per_block<T: Send>(
    target: MatMut<'_, T>,
    inners: &[usize],
    f: impl Fn(usize, MatMut<'_, T>) + Sync + Send,
) {
    let (size, blocks) = block_rows(target.nrows(), target.ncols(), inners);
    let mut parts = Vec::with_capacity(blocks);
    let (mut start, mut rest) = (0, target);
    for _ in 1..blocks {
        let (block, tail) = rest.split_at_row_mut(size);
        parts.push((start, block));
        (start, rest) = (start + size, tail);
    }
    parts.push((start, rest));
    per_part(parts, || (), |(), (start, block)| f(start, block));
}

fn spans(length: usize, size: usize) -> Vec<(usize, usize)> {
    let count = (length / size.max(1)).max(1);
    (0..count)
        .map(|i| {
            let start = i * size;
            (start, if i + 1 == count { length - start } else { size })
        })
        .collect()
}

fn tiles(rows: usize, columns: usize, inner: usize) -> [Vec<(usize, usize)>; 2] {
    let shape = |row: usize| {
        let few = rows / row < ENOUGH_TILES;
        let column = if few && columns >= 2 * WIDE {
            WIDE
        } else {
            columns
        };
        (row, column, (rows / row) * (columns / column.max(1)).max(1))
    };
    let candidates = [TALL, SQUARE, SHORT]
        .into_iter()
        .filter(|&row| rows >= 2 * row);
    let (row, column, _) = (candidates.map(shape))
        .find(|&(row, _, count)| count >= ENOUGH_TILES || row == SHORT)
        .unwrap_or_else(|| shape(rows.max(1)));
    if row * column * inner <= NANO {
        return [vec![(0, rows)], vec![(0, columns)]];
    }
    [spans(rows, row), spans(columns, column)]
}

pub fn product<T, A, B>(target: MatMut<'_, T>, a: MatRef<'_, A>, b: MatRef<'_, B>)
where
    T: ComplexField,
    A: Conjugate<Canonical = T>,
    B: Conjugate<Canonical = T>,
{
    assert!(
        target.nrows() == a.nrows() && target.ncols() == b.ncols() && a.ncols() == b.nrows(),
        "a {}×{} target for a {}×{} · {}×{} product",
        target.nrows(),
        target.ncols(),
        a.nrows(),
        a.ncols(),
        b.nrows(),
        b.ncols()
    );
    let [rows, columns] = tiles(target.nrows(), target.ncols(), a.ncols());
    let mut parts = Vec::with_capacity(rows.len() * columns.len());
    let mut below = target;
    for &(row, height) in &rows {
        let (band, rest) = below.split_at_row_mut(height);
        below = rest;
        let mut right = band;
        for &(column, width) in &columns {
            let (tile, rest) = right.split_at_col_mut(width);
            right = rest;
            parts.push((row, column, tile));
        }
    }
    per_part(
        parts,
        || (),
        |(), (row, column, tile)| {
            let (a, b) = (
                a.subrows(row, tile.nrows()),
                b.subcols(column, tile.ncols()),
            );
            matmul(tile, Accum::Replace, a, b, one::<T>(), Par::Seq);
        },
    );
}

pub fn times<T, A, B>(a: MatRef<'_, A>, b: MatRef<'_, B>) -> Mat<T>
where
    T: ComplexField,
    A: Conjugate<Canonical = T>,
    B: Conjugate<Canonical = T>,
{
    let mut target = Mat::zeros(a.nrows(), b.ncols());
    product(target.as_mut(), a, b);
    target
}

pub fn accumulate<T: ComplexField>(
    target: MatMut<'_, T>,
    terms: &[(MatRef<'_, T>, MatRef<'_, T>)],
) {
    let inners: Vec<usize> = terms.iter().map(|(a, _)| a.ncols()).collect();
    per_block(target, &inners, |start, mut block| {
        let mut term = Mat::zeros(block.nrows(), block.ncols());
        for &(a, b) in terms {
            let a = a.subrows(start, block.nrows());
            matmul(term.as_mut(), Accum::Replace, a, b, one::<T>(), Par::Seq);
            block += &term;
        }
    });
}

pub fn per_column<T: Send, B>(
    target: MatMut<'_, T>,
    init: impl Fn() -> B + Sync + Send,
    f: impl Fn(&mut B, usize, ColMut<'_, T>) + Sync + Send,
) {
    let columns: Vec<_> = target.col_iter_mut().enumerate().collect();
    per_part(columns, init, |b, (j, column)| f(b, j, column));
}
