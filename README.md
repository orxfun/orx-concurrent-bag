# orx-concurrent-bag

[![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
[![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)

A thread-safe collection allowing growth with immutable reference, making it ideal for collecting results concurrently.

It preserves the order of elements with respect to the order the `push` method is called.

# Examples

Safety guarantees to push to the bag with an immutable reference makes it easy to share the bag among threads.

## Using `std::sync::Arc`

Following the common approach of using an `Arc`, we can share our bag among threads and collect results concurrently.

```rust
use orx_concurrent_bag::*;
use std::{sync::Arc, thread};

let (num_threads, num_items_per_thread) = (4, 8);

let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
expected.sort();

let bag = Arc::new(ConcurrentBag::new());
let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();

for i in 0..num_threads {
    let bag = bag.clone();
    thread_vec.push(thread::spawn(move || {
        for j in 0..num_items_per_thread {
            bag.push(i * 1000 + j); // concurrently collect results simply by calling `push`
        }
    }));
}

for handle in thread_vec {
    handle.join().unwrap();
}

let mut vec_from_bag: Vec<_> = bag.iter().copied().collect();
vec_from_bag.sort();
assert_eq!(vec_from_bag, expected);
```

## Using `std::thread::scope`

An even more convenient approach would be to use thread scopes. This allows to use shared reference of the bag across threads, instead of `Arc`.

```rust
use orx_concurrent_bag::*;
use std::thread;

let (num_threads, num_items_per_thread) = (4, 8);

let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
expected.sort();

let bag = ConcurrentBag::new();
let bag_ref = &bag; // just take a reference
std::thread::scope(|s| {
    for i in 0..num_threads {
        s.spawn(move || {
            for j in 0..num_items_per_thread {
                bag_ref.push(i * 1000 + j); // concurrently collect results simply by calling `push`
            }
        });
    }
});

let mut vec_from_bag: Vec<_> = bag.iter().copied().collect();
vec_from_bag.sort();
assert_eq!(vec_from_bag, expected);
```

# Safety

`ConcurrentBag` uses a [`SplitVec`](https://crates.io/crates/orx-split-vec) as the underlying storage. `SplitVec` implements [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) which guarantees that elements which are already pushed to the vector stay pinned to their memory locations. This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).

In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `SplitVec` with an `AtomicUsize`.
* `AtomicUsize` fixes the target memory location of each element to be pushed at the time the `push` method is called. Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
* `SplitVec` guarantees that already pushed elements are not moved around in memory and new elements are written to the reserved position.

This pair allows a lightweight and convenient concurrent bag which is ideal for collecting results concurrently.
