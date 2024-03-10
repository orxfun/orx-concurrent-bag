use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use orx_concurrent_bag::*;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

fn with_arc(num_threads: usize, num_items_per_thread: usize, do_sleep: bool) {
    let bag = Arc::new(ConcurrentBag::new());
    let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();

    for i in 0..num_threads {
        let bag = bag.clone();
        thread_vec.push(thread::spawn(move || {
            sleep(do_sleep, i);
            for j in 0..num_items_per_thread {
                bag.push((i * 100000 + j) as i32);
            }
        }));
    }

    for handle in thread_vec {
        handle.join().unwrap();
    }
}

fn with_scope(num_threads: usize, num_items_per_thread: usize, do_sleep: bool) {
    let bag = ConcurrentBag::new();

    let bag_ref = &bag;
    std::thread::scope(|s| {
        for i in 0..num_threads {
            s.spawn(move || {
                sleep(do_sleep, i);
                for j in 0..num_items_per_thread {
                    bag_ref.push((i * 100000 + j) as i32);
                }
            });
        }
    });
}

fn sleep(do_sleep: bool, i: usize) {
    if do_sleep {
        let modulus = i % 3;
        let milliseconds = match modulus {
            0 => 0,
            1 => 10 + (i % 11) * 4,
            _ => 20 - (i % 5) * 3,
        } as u64;
        let duration = Duration::from_millis(milliseconds);
        std::thread::sleep(duration);
    }
}

fn bench_grow(c: &mut Criterion) {
    let treatments = vec![(4, 16384), (8, 131072)];

    let mut group = c.benchmark_group("grow");

    for (num_threads, num_items_per_thread) in treatments {
        let treatment = format!(
            "num_threads={},num_items_per_thread-type=[{}]",
            num_threads, num_items_per_thread
        );

        group.bench_with_input(BenchmarkId::new("with_arc", &treatment), &(), |b, _| {
            b.iter(|| with_arc(num_threads, num_items_per_thread, false))
        });

        group.bench_with_input(BenchmarkId::new("with_scope", &treatment), &(), |b, _| {
            b.iter(|| with_scope(num_threads, num_items_per_thread, false))
        });
    }

    group.finish();
}

criterion_group!(benches, bench_grow);
criterion_main!(benches);
