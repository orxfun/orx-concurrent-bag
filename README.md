# orx-concurrent-bag

[![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
[![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)

An efficient and convenient thread-safe grow-only collection, ideal for collecting results concurrently.
* **convenient**: the bag can be shared among threads simply as a shared reference, not even requiring `Arc`,
* **efficient**: allows copy-free collecting which makes it performant especially when the type to be collected is not very small (please see the <a href="#section-benchmarks">benchmarks</a> section for tradeoffs and details).

The bag preserves the order of elements with respect to the order the `push` method is called.

# Examples

Safety guarantees to push to the bag with an immutable reference makes it easy to share the bag among threads.

## Using `std::sync::Arc`

Following the common approach of using an `Arc`, we can share our bag among threads and collect results concurrently.

```rust
use orx_concurrent_bag::prelude::*;
use std::{sync::Arc, thread};

let (num_threads, num_items_per_thread) = (4, 8);

let bag = Arc::new(ConcurrentBag::new());
let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();

for i in 0..num_threads {
    let bag = bag.clone();
    thread_vec.push(thread::spawn(move || {
        for j in 0..num_items_per_thread {
            // concurrently collect results simply by calling `push`
            bag.push(i * 1000 + j);
        }
    }));
}

for handle in thread_vec {
    handle.join().unwrap();
}

let mut vec_from_bag: Vec<_> = unsafe { bag.iter() }.copied().collect();
vec_from_bag.sort();
let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
expected.sort();
assert_eq!(vec_from_bag, expected);
```

## Using `std::thread::scope`

An even more convenient approach would be to use thread scopes. This allows to use shared reference of the bag across threads, instead of `Arc`.

```rust
use orx_concurrent_bag::prelude::*;
use std::thread;

let (num_threads, num_items_per_thread) = (4, 8);

let bag = ConcurrentBag::new();
let bag_ref = &bag; // just take a reference
std::thread::scope(|s| {
    for i in 0..num_threads {
        s.spawn(move || {
            for j in 0..num_items_per_thread {
                // concurrently collect results simply by calling `push`
                bag_ref.push(i * 1000 + j);
            }
        });
    }
});

let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
vec_from_bag.sort();
let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
expected.sort();
assert_eq!(vec_from_bag, expected);
```

# Safety

`ConcurrentBag` uses a [`SplitVec`](https://crates.io/crates/orx-split-vec) as the underlying storage.
`SplitVec` implements [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) which guarantees that elements which are already pushed to the vector stay pinned to their memory locations.
This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).

In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `SplitVec` with an `AtomicUsize`.
* `AtomicUsize` fixes the target memory location of each element to be pushed at the time the `push` method is called. Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
* `SplitVec` guarantees that already pushed elements are not moved around in memory and new elements are written to the reserved position.

The approach guarantees that
* only one thread can write to the memory location of an element being pushed to the bag,
* at any point in time, only one thread is responsible for the allocation of memory if the bag requires new memory,
* no thread reads any of the written elements (reading happens after converting the bag `into_inner`),
* hence, there exists no race condition.

This pair allows a lightweight and convenient concurrent bag which is ideal for collecting results concurrently.

# Write-Only vs Read-Write

The concurrent bag is write-only & grow-only bag which is convenient and efficient for collecting elements.

See [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read-and-write variant which
* guarantees that reading and writing never happen concurrently, and hence,
* allows safe iteration or access to already written elements of the concurrent vector,
* with a minor additional cost of values being wrapped by an `Option`.

<div id="section-benchmarks"></div>

# Benchmarks

*You may see the benchmark at [benches/grow.rs](https://github.com/orxfun/orx-split-vec/blob/main/benches/grow.rs).*

In this benchmark, concurrent results are collected using `ConcurrentBag` together with scoped threads and `Arc`. Computation time performance of these two is negligible, hence, only scoped thread implementation is reported. Results are compared by the `collect` method `rayon`s parallel iterator. 

<img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_grow.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_grow.PNG" />

We can see that:
* `rayon` is extremely performant when the data size to be collected is small and there is a huge concurrency load. We can see that it outperforms `ConcurrentBag` when the threads do not do any work at all to produce outputs and the output data is `i32`.
* On the other hand, when there exists some work to be done to produce the outputs (workload), `ConcurrentBag` starts to perform significantly faster.
* Similarly, when the output data is large (`[i32; 32]` in this example), regardless of the additional workload, `ConcurrentBag` performs faster.
