use append_only_vec::AppendOnlyVec;
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};
use orx_concurrent_bag::*;
use std::fmt::Debug;

#[allow(dead_code)]
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
struct LargeData {
    a: [i32; 64],
}

#[allow(dead_code)]
fn compute_data_i32(i: usize, j: usize) -> i32 {
    (i * 100000 + j) as i32
}

#[allow(dead_code)]
fn compute_large_data(i: usize, j: usize) -> LargeData {
    let mut a = [0i32; 64];

    #[allow(clippy::needless_range_loop)]
    for k in 0..64 {
        if k == i {
            a[k] = (i - j) as i32;
        } else if k == j {
            a[k] = (j + i) as i32;
        } else {
            a[k] = (i + j + k) as i32;
        }
    }

    LargeData { a }
}

fn validate<T: PartialEq + Eq + PartialOrd + Ord + Debug>(expected: &[T], other: &mut [T]) {
    assert_eq!(expected.len(), other.len());
    other.sort();
    assert_eq!(expected, other);
}

fn seq<T>(
    num_threads: usize,
    num_items_per_thread: usize,
    compute: fn(usize, usize) -> T,
) -> Vec<T> {
    let result: Vec<_> = (0..num_threads)
        .into_iter()
        .flat_map(|_| {
            (0..num_items_per_thread)
                .map(|j| std::hint::black_box(compute(j, j + 1)))
                .collect::<Vec<_>>()
        })
        .collect();

    result
}

fn with_concurrent_bag<T: Sync, P: IntoConcurrentPinnedVec<T>>(
    num_threads: usize,
    num_items_per_thread: usize,
    compute: fn(usize, usize) -> T,
    batch_size: usize,
    bag: ConcurrentBag<T, P>,
) -> ConcurrentBag<T, P> {
    let bag_ref = &bag;
    std::thread::scope(|s| {
        for _ in 0..num_threads {
            s.spawn(|| {
                for j in (0..num_items_per_thread).step_by(batch_size) {
                    let into_iter =
                        (j..(j + batch_size)).map(|j| std::hint::black_box(compute(j, j + 1)));
                    bag_ref.extend(into_iter);
                }
            });
        }
    });

    bag
}

fn rayon<T: Send + Sync + Clone + Copy>(
    num_threads: usize,
    num_items_per_thread: usize,
    compute: fn(usize, usize) -> T,
) -> Vec<T> {
    use rayon::prelude::*;

    let result: Vec<_> = (0..num_threads)
        .into_par_iter()
        .flat_map(|_| {
            (0..num_items_per_thread)
                .map(|j| std::hint::black_box(compute(j, j + 1)))
                .collect::<Vec<_>>()
        })
        .collect();

    result
}

fn append_only_vec<T: Send + Sync + Clone + Copy>(
    num_threads: usize,
    num_items_per_thread: usize,
    compute: fn(usize, usize) -> T,
    vec: AppendOnlyVec<T>,
) -> AppendOnlyVec<T> {
    std::thread::scope(|s| {
        for _ in 0..num_threads {
            s.spawn(|| {
                for j in 0..num_items_per_thread {
                    vec.push(std::hint::black_box(compute(j, j + 1)));
                }
            });
        }
    });

    vec
}

fn boxcar<T: Send + Sync + Clone + Copy>(
    num_threads: usize,
    num_items_per_thread: usize,
    compute: fn(usize, usize) -> T,
    vec: boxcar::Vec<T>,
) -> boxcar::Vec<T> {
    std::thread::scope(|s| {
        for _ in 0..num_threads {
            s.spawn(|| {
                for j in 0..num_items_per_thread {
                    vec.push(std::hint::black_box(compute(j, j + 1)));
                }
            });
        }
    });

    vec
}

fn bench_grow(c: &mut Criterion) {
    let thread_info = [(8, 4096), (8, 16384)];

    let mut group = c.benchmark_group("grow");

    for (num_threads, num_items_per_thread) in thread_info {
        let treatment = format!(
            "num_threads={},num_items_per_thread-type=[{}]",
            num_threads, num_items_per_thread
        );

        let max_len = num_threads * num_items_per_thread;
        let fragment_size = 2usize.pow(12);
        let num_linear_fragments = (max_len / fragment_size) + 1;

        let compute = compute_large_data;

        let mut expected = seq(num_threads, num_items_per_thread, compute);
        expected.sort();
        validate(
            &expected,
            &mut rayon(num_threads, num_items_per_thread, compute),
        );

        validate(
            &expected,
            &mut append_only_vec(
                num_threads,
                num_items_per_thread,
                compute,
                AppendOnlyVec::new(),
            )
            .into_iter()
            .collect::<Vec<_>>(),
        );

        validate(
            &expected,
            &mut boxcar(
                num_threads,
                num_items_per_thread,
                compute,
                boxcar::Vec::new(),
            )
            .into_iter()
            .collect::<Vec<_>>(),
        );

        validate(
            &expected,
            &mut with_concurrent_bag(
                num_threads,
                num_items_per_thread,
                compute,
                64,
                ConcurrentBag::with_doubling_growth(),
            )
            .into_inner()
            .to_vec(),
        );

        validate(
            &expected,
            &mut with_concurrent_bag(
                num_threads,
                num_items_per_thread,
                compute,
                64,
                ConcurrentBag::with_linear_growth(12, num_linear_fragments),
            )
            .into_inner()
            .to_vec(),
        );

        validate(
            &expected,
            &mut with_concurrent_bag(
                num_threads,
                num_items_per_thread,
                compute,
                64,
                ConcurrentBag::with_fixed_capacity(num_threads * num_items_per_thread),
            )
            .into_inner()
            .to_vec(),
        );

        // rayon

        group.bench_with_input(BenchmarkId::new("rayon", &treatment), &(), |b, _| {
            b.iter(|| {
                black_box(rayon(
                    black_box(num_threads),
                    black_box(num_items_per_thread),
                    compute,
                ))
            })
        });

        // APPEND-ONLY-VEC

        group.bench_with_input(
            BenchmarkId::new("append_only_vec", &treatment),
            &(),
            |b, _| {
                b.iter(|| {
                    black_box(append_only_vec(
                        black_box(num_threads),
                        black_box(num_items_per_thread),
                        compute,
                        AppendOnlyVec::new(),
                    ))
                })
            },
        );

        // BOXCAR

        group.bench_with_input(BenchmarkId::new("boxcar", &treatment), &(), |b, _| {
            b.iter(|| {
                black_box(boxcar(
                    black_box(num_threads),
                    black_box(num_items_per_thread),
                    compute,
                    boxcar::Vec::new(),
                ))
            })
        });

        // ConcurrentBag

        let batch_sizes = vec![64, num_items_per_thread];

        for batch_size in batch_sizes {
            let name = |pinned_type: &str| {
                format!(
                    "with_concurrent_bag({}) | batch-size={}",
                    pinned_type, batch_size
                )
            };

            group.bench_with_input(
                BenchmarkId::new(name("Doubling"), &treatment),
                &(),
                |b, _| {
                    b.iter(|| {
                        black_box(with_concurrent_bag(
                            black_box(num_threads),
                            black_box(num_items_per_thread),
                            compute,
                            batch_size,
                            ConcurrentBag::with_doubling_growth(),
                        ))
                    })
                },
            );

            group.bench_with_input(
                BenchmarkId::new(name("Linear(12)"), &treatment),
                &(),
                |b, _| {
                    b.iter(|| {
                        black_box(with_concurrent_bag(
                            black_box(num_threads),
                            black_box(num_items_per_thread),
                            compute,
                            batch_size,
                            ConcurrentBag::with_linear_growth(12, num_linear_fragments),
                        ))
                    })
                },
            );

            group.bench_with_input(BenchmarkId::new(name("Fixed"), &treatment), &(), |b, _| {
                b.iter(|| {
                    black_box(with_concurrent_bag(
                        black_box(num_threads),
                        black_box(num_items_per_thread),
                        compute,
                        batch_size,
                        ConcurrentBag::with_fixed_capacity(num_threads * num_items_per_thread),
                    ))
                })
            });
        }
    }

    group.finish();
}

criterion_group!(benches, bench_grow);
criterion_main!(benches);
