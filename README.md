# orx-concurrent-bag

[![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
[![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)

An efficient, convenient and lightweight grow-only concurrent collection, ideal for collecting results concurrently.

* **convenient**: `ConcurrentBag` can safely be shared among threads simply as a shared reference. Further, it is just a wrapper around any [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) implementation adding concurrent safety guarantees. Therefore, underlying pinned vector and concurrent bag can be converted to each other back and forth without any cost.
* **lightweight**: This crate takes a simplistic approach built on pinned vector guarantees which leads to concurrent programs with few dependencies and small binaries (see <a href="#section-approach-and-safety">approach and safety</a> for details).
* **efficient**: `ConcurrentBag` is a lock free structure making use of a few atomic primitives. rayon is significantly faster when collecting small results under an extreme load (negligible work to compute results); however, `ConcurrentBag` starts to perform faster as result types get larger (see <a href="#section-benchmarks">benchmarks</a> for the experiments).

# Examples

Safety guarantees to push to the bag with an immutable reference makes it easy to share the bag among threads. `std::sync::Arc` can be used; however, it is not required as demonstrated below.

```rust
use orx_concurrent_bag::*;

let (num_threads, num_items_per_thread) = (4, 8);

let bag = ConcurrentBag::new();
let bag_ref = &bag; // just take a reference and share among threads

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

<div id="section-approach-and-safety"></div>

# Approach and Safety

`ConcurrentBag` aims to enable concurrent growth with a minimalistic approach. It requires two major components for this:
* The underlying storage, which is any `PinnedVec` implementation. This means that memory locations of elements that are already pushed to the vector will never change, unless explicitly changed. This guarantee eliminates a certain set of safety concerns and corresponding complexity.
* An atomic counter that is responsible for uniquely assigning one vector position to one and only one thread. `std::sync::atomic::AtomicUsize` and its `fetch_add` method are sufficient for this.

Simplicity and safety of the approach can be observed in the implementation of the `push` method.

```rust ignore
pub fn push(&self, value: T) -> usize {
    let idx = self.len.fetch_add(1, Ordering::AcqRel);
    self.assert_has_capacity_for(idx);

    loop {
        let capacity = self.capacity.load(Ordering::Relaxed);

        match idx.cmp(&capacity) {
            // no need to grow, just push
            std::cmp::Ordering::Less => {
                self.write(idx, value);
                break;
            }

            // we are responsible for growth
            std::cmp::Ordering::Equal => {
                let new_capacity = self.grow_to(capacity + 1);
                self.write(idx, value);
                self.capacity.store(new_capacity, Ordering::Relaxed);
                break;
            }

            // spin to wait for responsible thread to handle growth
            std::cmp::Ordering::Greater => {}
        }
    }

    idx
}
```

Below are some details about this implementation:
* `fetch_add` guarantees that each pushed `value` receives a unique idx.
* `assert_has_capacity_for` method is an additional safety guarantee added to pinned vectors to prevent any possible UB. It is not constraining for practical usage, see [`ConcurrentBag::maximum_capacity`] for details.
* Inside the loop, we read the current `capacity` and compare it with `idx`:
  * `idx < capacity`:
    * The `idx`-th position is already allocated and belongs to the bag. We can simply write. Note that concurrent bag is write-only. Therefore, there is no other thread writing to or reading from this position; and hence, no race condition is present.
  * `idx > capacity`:
    * The `idx`-th position is not yet allocated. Underlying pinned vector needs to grow.
    * But another thread is responsible for the growth, we simply wait.
  * `idx == capacity`:
    * The `idx`-th position is not yet allocated. Underlying pinned vector needs to grow.
    * Further, we are responsible for the growth. Note that this guarantees that:
      * Only one thread will make the growth calls.
      * Only one growth call can take place at a given time.
      * There exists no race condition for the growth.
    * We first grow the pinned vector, then write to the `idx`-th position, and finally update the `capacity` to the new capacity.

## How many times will we spin?

This is **deterministic**. It is exactly equal to the number of growth calls of the underlying pinned vector, and pinned vector implementations give a detailed control on this. For instance, assume that we will push a total of 15_000 elements concurrently to an empty bag.

* Further assume we use the default `SplitVec<_, Doubling>` as the underlying pinned vector. Throughout the execution, we will allocate fragments of capacities [4, 8, 16, ..., 4096, 8192] which will lead to a total capacity of 16_380. In other words, we might possibly visit the `std::cmp::Ordering::Greater => {}` block in 12 points in time during the entire execution.
* If we use a `SplitVec<_, Linear>` with constant fragment lengths of 1_024, we will allocate 15 equal capacity fragments, which will lead to a total capacity of 15_360. So looping might only happen 15 times. We can drop this number to 8 if we set constant fragment capacity to 2_048; i.e., we can control the frequency of allocations.
* If we use the strict `FixedVec<_>`, we have to pre-allocate a safe amount and can never grow beyond this number. Therefore, there will never be any spinning.

## When we spin, how long do we spin?

Not long because:
* Pinned vectors do not change memory locations of already pushed elements. In other words, growths are copy-free.
* We are only waiting for allocation of memory required for the growth with respect to the chosen growth strategy.

# Construction

`ConcurrentBag` can be constructed by wrapping any pinned vector; i.e., `ConcurrentBag<T>` implements `From<P: PinnedVec<T>>`.
Likewise, a concurrent vector can be unwrapped without any cost to the underlying pinned vector with `into_inner` method.

Further, there exist `with_` methods to directly construct the concurrent bag with common pinned vector implementations.

```rust
use orx_concurrent_bag::*;

// default pinned vector -> SplitVec<T, Doubling>
let bag: ConcurrentBag<char> = ConcurrentBag::new();
let bag: ConcurrentBag<char> = Default::default();
let bag: ConcurrentBag<char> = ConcurrentBag::with_doubling_growth();
let bag: ConcurrentBag<char, SplitVec<char, Doubling>> = ConcurrentBag::with_doubling_growth();

let bag: ConcurrentBag<char> = SplitVec::new().into();
let bag: ConcurrentBag<char, SplitVec<char, Doubling>> = SplitVec::new().into();

// SplitVec with [Linear](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html) growth
// each fragment will have capacity 2^10 = 1024
// and the split vector can grow up to 32 fragments
let bag: ConcurrentBag<char, SplitVec<char, Linear>> = ConcurrentBag::with_linear_growth(10, 32);
let bag: ConcurrentBag<char, SplitVec<char, Linear>> = SplitVec::with_linear_growth_and_fragments_capacity(10, 32).into();

// [FixedVec](https://docs.rs/orx-fixed-vec/latest/orx_fixed_vec/) with fixed capacity.
// Fixed vector cannot grow; hence, pushing the 1025-th element to this bag will cause a panic!
let bag: ConcurrentBag<char, FixedVec<char>> = ConcurrentBag::with_fixed_capacity(1024);
let bag: ConcurrentBag<char, FixedVec<char>> = FixedVec::new(1024).into();
```

Of course, the pinned vector to be wrapped does not need to be empty.

```rust
use orx_concurrent_bag::*;

let split_vec: SplitVec<i32> = (0..1024).collect();
let bag: ConcurrentBag<_> = split_vec.into();
```

# Write-Only vs Read-Write

The concurrent bag is a write-only bag which is convenient and efficient for collecting elements.

See [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read-and-write variant which

* guarantees that reading and writing never happen concurrently, and hence,
* allows safe iteration or access to already written elements of the concurrent vector.

However, `ConcurrentVec<T>` requires to use a `PinnedVec<Option<T>>` as the underlying storage rather than `PinnedVec<T>`.

<div id="section-benchmarks"></div>

# Benchmarks

*You may find the details of the benchmarks at [benches/grow.rs](https://github.com/orxfun/orx-concurrent-bag/blob/main/benches/grow.rs).*

In the experiment, `ConcurrentBag` variants and `rayon` is used to collect results from multiple threads. You may see in the table below that `rayon` is extremely fast with very small output data (`i32` in this case). As the output size gets larger and copies become costlier, `ConcurrentBag` starts to perform faster.

<img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_grow.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_grow.PNG" />


## License

This library is licensed under MIT license. See LICENSE for details.
