use super::{Parts, bits, entry};
use orbitar::pw::{
    Mat, Pool, accumulate, c64, per_chunk, per_column, per_point, per_point_with, pool_threads,
    product, product_tiles, random, times,
};
use rayon::ThreadPoolBuilder;
use rayon::prelude::*;
use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[test]
fn outside_a_pool_each_point_runs_in_order_on_the_calling_thread() {
    let caller = std::thread::current().id();
    let got = per_point(5, |i| (i * i, std::thread::current().id() == caller));
    assert_eq!(got, [0, 1, 4, 9, 16].map(|v| (v, true)));
}

#[test]
fn outside_a_pool_a_plain_par_iter_leaves_the_calling_thread_where_per_point_stays() {
    let plain: Vec<Option<usize>> = (0..64)
        .into_par_iter()
        .map(|_| rayon::current_thread_index())
        .collect();
    assert!(plain.iter().all(Option::is_some), "{plain:?}");
    let ours = per_point(64, |_| rayon::current_thread_index());
    assert!(ours.iter().all(Option::is_none), "{ours:?}");
}

#[test]
fn pool_threads_is_the_calling_pools_size_and_one_outside_a_pool() {
    assert_eq!(pool_threads(), 1);
    for n in [2, 3] {
        let pool = ThreadPoolBuilder::new().num_threads(n).build().unwrap();
        assert_eq!(pool.install(pool_threads), n);
    }
}

#[test]
fn inside_a_pool_the_points_spread_over_its_threads_and_land_in_order() {
    within(|| {
        let pool = ThreadPoolBuilder::new().num_threads(3).build().unwrap();
        let got = pool.install(|| {
            per_point(12, |i| {
                std::thread::sleep(Duration::from_millis(5));
                (i, rayon::current_thread_index())
            })
        });
        assert_eq!(
            got.iter().map(|p| p.0).collect::<Vec<_>>(),
            (0..12).collect::<Vec<_>>()
        );
        let ran: BTreeSet<usize> = got.iter().map(|p| p.1.unwrap()).collect();
        assert!(ran.len() >= 2, "{ran:?}");
    });
}

#[test]
fn outside_a_pool_one_buffer_serves_every_point() {
    let got = per_point_with(
        4,
        || 0,
        |seen: &mut usize, i| {
            *seen += 1;
            (i, *seen)
        },
    );
    assert_eq!(got, [(0, 1), (1, 2), (2, 3), (3, 4)]);
}

#[test]
fn outside_a_pool_each_chunk_runs_in_order_on_the_calling_thread_with_one_buffer() {
    let caller = std::thread::current().id();
    let mut values = [0usize; 8];
    per_chunk(
        &mut values,
        3,
        || 0,
        |seen: &mut usize, i, chunk| {
            *seen += 1;
            let here =
                std::thread::current().id() == caller && rayon::current_thread_index().is_none();
            chunk.fill(if here { 10 * i + *seen } else { 0 });
        },
    );
    assert_eq!(values, [1, 1, 1, 12, 12, 12, 23, 23]);
}

#[test]
fn a_product_in_blocks_has_the_bits_of_faers_whole_product_at_any_thread_count() {
    within(|| {
        a_product_in_blocks_has_the_bits_of_faers_whole_product_at_any_thread_count_in::<c64>();
        a_product_in_blocks_has_the_bits_of_faers_whole_product_at_any_thread_count_in::<f64>();
    });
}

fn a_product_in_blocks_has_the_bits_of_faers_whole_product_at_any_thread_count_in<T: Parts>() {
    let pool = ThreadPoolBuilder::new().num_threads(3).build().unwrap();
    for (seed, (n, k, q)) in [
        (3000, 120, 40),
        (2741, 97, 139),
        (600, 31, 64),
        (40, 5, 3),
        (513, 1, 9),
        (700, 9, 1),
        (100, 31, 2),
        (128, 40, 4),
        (3000, 18, 1),
        (600, 1, 1000),
        (1500, 300, 300),
    ]
    .into_iter()
    .enumerate()
    {
        let seed = 3 * seed as u64 + 1;
        let (a, b, c) = (
            random::<T>(n, k, seed),
            random::<T>(k, q, seed + 1),
            random::<T>(n, q, seed + 2),
        );
        let tall: Mat<T> = &a * &b;
        let square: Mat<T> = a.adjoint() * &c;
        for threads in [None, Some(&pool)] {
            let run = |f: &(dyn Fn() -> Mat<T> + Sync)| match threads {
                Some(pool) => pool.install(f),
                None => f(),
            };
            assert_eq!(
                bits(run(&|| times(a.as_ref(), b.as_ref())).as_ref()),
                bits(tall.as_ref()),
                "{n}×{k}·{k}×{q}"
            );
            assert_eq!(
                bits(run(&|| times(a.adjoint(), c.as_ref())).as_ref()),
                bits(square.as_ref()),
                "{k}×{n}·{n}×{q}"
            );
        }
    }
}

#[test]
fn product_overwrites_its_target() {
    product_overwrites_its_target_in::<c64>();
    product_overwrites_its_target_in::<f64>();
}

fn product_overwrites_its_target_in<T: Parts>() {
    let (a, b) = (random::<T>(600, 20, 7), random::<T>(20, 30, 8));
    let mut target = Mat::from_fn(600, 30, |_, _| entry::<T>(f64::NAN, 1.0));
    product(target.as_mut(), a.as_ref(), b.as_ref());
    let whole: Mat<T> = &a * &b;
    assert_eq!(bits(target.as_ref()), bits(whole.as_ref()));
}

#[test]
fn per_column_hands_each_column_its_own_index_inside_and_outside_a_pool() {
    within(|| {
        per_column_hands_each_column_its_own_index_inside_and_outside_a_pool_in::<c64>();
        per_column_hands_each_column_its_own_index_inside_and_outside_a_pool_in::<f64>();
    });
}

fn per_column_hands_each_column_its_own_index_inside_and_outside_a_pool_in<T: Parts>() {
    let pool = ThreadPoolBuilder::new().num_threads(3).build().unwrap();
    let fill = || {
        let mut m = Mat::<T>::zeros(4, 9);
        per_column(
            m.as_mut(),
            || (),
            |(), j, mut column| {
                for i in 0..4 {
                    column[i] = entry::<T>(j as f64, i as f64);
                }
            },
        );
        m
    };
    let expected = Mat::from_fn(4, 9, |i, j| entry::<T>(j as f64, i as f64));
    assert_eq!(fill(), expected);
    assert_eq!(pool.install(fill), expected);
}

#[test]
fn accumulate_adds_each_term_in_order_with_the_bits_of_one_sum_per_term() {
    within(|| {
        accumulate_adds_each_term_in_order_with_the_bits_of_one_sum_per_term_in::<c64>();
        accumulate_adds_each_term_in_order_with_the_bits_of_one_sum_per_term_in::<f64>();
    });
}

fn accumulate_adds_each_term_in_order_with_the_bits_of_one_sum_per_term_in<T: Parts>() {
    let pool = ThreadPoolBuilder::new().num_threads(3).build().unwrap();
    let (n, q) = (1300, 20);
    let terms: Vec<(Mat<T>, Mat<T>)> = [48, 24, 6, 2]
        .into_iter()
        .zip(10..)
        .map(|(k, seed)| (random::<T>(n, k, seed), random::<T>(k, q, seed + 10)))
        .collect();
    let mut expected = Mat::<T>::zeros(n, q);
    for (a, b) in &terms {
        expected += a * b;
    }
    let refs: Vec<_> = terms
        .iter()
        .map(|(a, b)| (a.as_ref(), b.as_ref()))
        .collect();
    let sum = || {
        let mut target = Mat::<T>::zeros(n, q);
        accumulate(target.as_mut(), &refs);
        target
    };
    assert_eq!(bits(sum().as_ref()), bits(expected.as_ref()));
    for _ in 0..10 {
        assert_eq!(bits(pool.install(sum).as_ref()), bits(expected.as_ref()));
    }
}

#[test]
fn a_product_has_the_same_bits_at_any_thread_count() {
    within(|| {
        a_product_has_the_same_bits_at_any_thread_count_in::<c64>();
        a_product_has_the_same_bits_at_any_thread_count_in::<f64>();
    });
}

fn a_product_has_the_same_bits_at_any_thread_count_in<T: Parts>() {
    let pools: Vec<_> = [1, 2, 3, 4, 8]
        .into_iter()
        .map(|n| ThreadPoolBuilder::new().num_threads(n).build().unwrap())
        .collect();
    let cancelling = Mat::from_fn(20_000, 4, |_, j| {
        entry::<T>([1e16, 1.0, -1e16, 1.0][j], 0.0)
    });
    let ones = Mat::from_fn(4, 1, |_, _| entry::<T>(1.0, 0.0));
    let mut cases: Vec<(Mat<T>, Mat<T>, bool)> = vec![(cancelling, ones, false)];
    for (seed, (n, k, q)) in [
        (4000, 200, 200),
        (1500, 300, 300),
        (20_000, 4, 1),
        (3000, 1, 700),
    ]
    .into_iter()
    .enumerate()
    {
        let seed = 2 * seed as u64 + 31;
        cases.push((random::<T>(n, k, seed), random::<T>(n, q, seed + 1), true));
    }
    for (seed, (m, k, n)) in [(4097, 17, 131), (4097, 21, 1), (120, 8, 16_384)]
        .into_iter()
        .enumerate()
    {
        let seed = 2 * seed as u64 + 71;
        cases.push((random::<T>(m, k, seed), random::<T>(k, n, seed + 1), false));
    }
    for (a, b, adjoint) in &cases {
        let product = || match adjoint {
            true => times(a.adjoint(), b.as_ref()),
            false => times(a.as_ref(), b.as_ref()),
        };
        let (rows, columns) = match adjoint {
            true => (a.ncols(), a.nrows()),
            false => (a.nrows(), a.ncols()),
        };
        let alone = bits(product().as_ref());
        for pool in &pools {
            assert_eq!(
                bits(pool.install(product).as_ref()),
                alone,
                "{rows}×{columns}·{}×{} on {} threads",
                b.nrows(),
                b.ncols(),
                pool.current_num_threads()
            );
        }
    }
}

#[test]
fn an_empty_target_stays_empty_and_an_empty_inner_dimension_zeroes_the_target() {
    let empty: Mat<f64> = times(
        random::<f64>(0, 65, 1).as_ref(),
        random::<f64>(65, 4096, 2).as_ref(),
    );
    assert_eq!((empty.nrows(), empty.ncols()), (0, 4096));
    let (a, b) = (Mat::<f64>::zeros(3, 0), Mat::<f64>::zeros(0, 4));
    let mut target = Mat::from_fn(3, 4, |_, _| 7.0);
    product(target.as_mut(), a.as_ref(), b.as_ref());
    assert!((0..3).all(|i| (0..4).all(|j| target[(i, j)] == 0.0)));
}

#[test]
#[should_panic(expected = "a 10×3 target for a 10×5 · 5×4 product")]
fn a_product_into_a_target_of_another_shape_is_refused() {
    let (a, b) = (random::<f64>(10, 5, 1), random::<f64>(5, 4, 2));
    let mut target = Mat::<f64>::zeros(10, 3);
    product(target.as_mut(), a.as_ref(), b.as_ref());
}

fn within<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    let (sent, received) = mpsc::channel();
    std::thread::spawn(move || sent.send(f()));
    received
        .recv_timeout(Duration::from_secs(10))
        .expect("the pool returns within 10 s")
}

#[test]
fn a_pool_run_spreads_the_points_over_its_threads_and_lands_them_in_order() {
    let (got, threads) = within(|| {
        let pool = Pool::try_from(3).unwrap();
        let got = pool.run(|| {
            per_point(12, |i| {
                std::thread::sleep(Duration::from_millis(5));
                (i, rayon::current_thread_index())
            })
        });
        (got, pool.run(pool_threads))
    });
    assert_eq!(
        got.iter().map(|p| p.0).collect::<Vec<_>>(),
        (0..12).collect::<Vec<_>>()
    );
    let ran: BTreeSet<usize> = got.iter().map(|p| p.1.unwrap()).collect();
    assert!(ran.len() >= 2, "{ran:?}");
    assert_eq!(threads, 3);
}

#[test]
fn inside_a_pool_no_more_buffers_start_than_threads_take_points() {
    let (got, made) = within(|| {
        let pool = Pool::try_from(3).unwrap();
        let made = AtomicUsize::new(0);
        let got = pool.run(|| {
            per_point_with(
                40,
                || made.fetch_add(1, Ordering::SeqCst),
                |_, i| {
                    std::thread::sleep(Duration::from_millis(1));
                    i
                },
            )
        });
        (got, made.into_inner())
    });
    assert_eq!(got, (0..40).collect::<Vec<_>>());
    assert!((1..=3).contains(&made), "{made} buffers");
}

#[test]
fn nested_points_in_many_runs_of_a_pool_all_finish_in_order() {
    let sums = within(|| {
        let pool = Pool::try_from(4).unwrap();
        (0..200)
            .map(|run| pool.run(|| per_point(3, |i| per_point(5, |j| run + i * j).iter().sum())))
            .collect::<Vec<Vec<usize>>>()
    });
    for (run, got) in sums.iter().enumerate() {
        assert_eq!(*got, [5 * run, 5 * run + 10, 5 * run + 20], "run {run}");
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the kernel shows a thread only in /proc"
)]
fn tid() -> String {
    let link = std::fs::read_link("/proc/thread-self").unwrap();
    link.file_name().unwrap().to_string_lossy().into_owned()
}

#[expect(
    clippy::disallowed_methods,
    reason = "the kernel shows a thread's CPU time only in /proc"
)]
fn cpu_ns(tids: &BTreeSet<String>) -> u64 {
    (tids.iter())
        .filter_map(|t| std::fs::read_to_string(format!("/proc/self/task/{t}/schedstat")).ok())
        .filter_map(|stat| stat.split_whitespace().next()?.parse::<u64>().ok())
        .sum()
}

#[expect(
    clippy::disallowed_methods,
    reason = "the kernel shows a thread's state only in /proc"
)]
fn states(tids: &BTreeSet<String>) -> Vec<char> {
    (tids.iter())
        .filter_map(|t| std::fs::read_to_string(format!("/proc/self/task/{t}/stat")).ok())
        .filter_map(|stat| stat.rsplit_once(") ")?.1.chars().next())
        .collect()
}

fn threads_of(pool: &Pool) -> BTreeSet<String> {
    let all = std::sync::Barrier::new(3);
    let tids: BTreeSet<String> = pool
        .run(|| per_point(3, |_| (all.wait(), tid()).1))
        .into_iter()
        .collect();
    assert_eq!(tids.len(), 3, "{tids:?}");
    tids
}

fn busy(d: Duration) {
    let start = Instant::now();
    while start.elapsed() < d {
        std::hint::spin_loop();
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "the kernel shows threads only in /proc"
)]
fn proc_threads() -> bool {
    std::path::Path::new("/proc/thread-self/schedstat").exists()
}

#[test]
fn inside_a_run_idle_pool_threads_keep_looking_through_short_serial_gaps() {
    if !proc_threads() {
        return;
    }
    let rounds = 200;
    let (helpers, op, wall) = within(move || {
        let pool = Pool::try_from(3).unwrap();
        let tids = threads_of(&pool);
        std::thread::sleep(Duration::from_millis(50));
        pool.run(|| {
            let op = BTreeSet::from([tid()]);
            let helpers: BTreeSet<String> = tids.difference(&op).cloned().collect();
            let before = (cpu_ns(&helpers), cpu_ns(&op));
            let start = Instant::now();
            for _ in 0..rounds {
                per_point(6, |i| i);
                busy(Duration::from_micros(800));
            }
            let wall = start.elapsed().as_nanos() as f64;
            (cpu_ns(&helpers) - before.0, cpu_ns(&op) - before.1, wall)
        })
    });
    let each = helpers as f64 / (2.0 * op as f64);
    let share = op as f64 / wall;
    assert!(
        each > 0.1 * share,
        "each helper ran {each:.3} of the op's {op} ns, which ran {share:.2} of the wall"
    );
}

#[test]
fn a_caller_waits_for_its_helpers_by_looking_for_work_rather_than_parking() {
    if !proc_threads() {
        return;
    }
    let (caller, helper) = within(|| {
        let pool = Pool::try_from(3).unwrap();
        let tids = threads_of(&pool);
        pool.run(|| {
            let op = BTreeSet::from([tid()]);
            let others: BTreeSet<String> = tids.difference(&op).cloned().collect();
            let both = std::sync::Barrier::new(2);
            let before = (cpu_ns(&op), cpu_ns(&others));
            per_point(2, |_| {
                both.wait();
                if !op.contains(&tid()) {
                    busy(Duration::from_millis(50));
                }
            });
            (cpu_ns(&op) - before.0, cpu_ns(&others) - before.1)
        })
    });
    let share = caller as f64 / helper as f64;
    assert!(
        share > 0.1,
        "the caller ran {caller} ns while its helper ran {helper} ns: {share:.2} of it"
    );
}

#[test]
fn after_a_run_idle_pool_threads_park() {
    if !proc_threads() {
        return;
    }
    let states = within(|| {
        let pool = Pool::try_from(3).unwrap();
        let tids = threads_of(&pool);
        pool.run(|| per_point(6, |_| busy(Duration::from_micros(50))));
        let start = Instant::now();
        let mut now = states(&tids);
        while !now.iter().all(|&s| s == 'S') && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(10));
            now = states(&tids);
        }
        (now, pool)
    });
    assert!(
        states.0.iter().all(|&s| s == 'S'),
        "{:?} after 5 s",
        states.0
    );
}

#[test]
fn many_short_parallel_steps_in_a_run_do_not_wait_for_idle_threads_to_stop_looking() {
    let steps = 1000;
    let (sums, took) = within(move || {
        let pool = Pool::try_from(3).unwrap();
        let start = Instant::now();
        let sums: Vec<usize> = pool.run(|| {
            (0..steps)
                .map(|_| per_point(6, |i| i).iter().sum())
                .collect()
        });
        (sums, start.elapsed())
    });
    assert!(sums.iter().all(|&s| s == 15));
    assert!(
        took < Duration::from_millis(500),
        "{took:?} for {steps} steps"
    );
}

#[test]
fn a_target_with_few_tall_tiles_takes_the_first_row_height_that_gives_sixteen() {
    let rows: Vec<(usize, usize)> = (0..8).map(|i| (64 * i, 64)).chain([(512, 88)]).collect();
    assert_eq!(
        product_tiles(600, 191, 1000),
        [rows, vec![(0, 64), (64, 127)]]
    );
}

#[test]
fn eight_row_tiles_stay_apart_while_each_holds_more_than_sixteen_cubed_multiply_adds() {
    let rows: Vec<(usize, usize)> = (0..11).map(|i| (8 * i, 8)).chain([(88, 12)]).collect();
    assert_eq!(product_tiles(100, 10, 100), [rows, vec![(0, 10)]]);
}
