//! # orx-concurrent-bag
//!
//! [![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
//! [![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)
//!
//! An efficient, convenient and lightweight grow-only concurrent data structure allowing high performance concurrent collection.
//!
//! * **convenient**: `ConcurrentBag` can safely be shared among threads simply as a shared reference. Further, it is just a wrapper around any [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) implementation adding concurrent safety guarantees. Therefore, underlying pinned vector and concurrent bag can be converted to each other back and forth without any cost (see <a href="#section-construction-and-conversions">construction and conversions</a>).
//! * **lightweight**: This crate takes a simplistic approach built on pinned vector guarantees which leads to concurrent programs with few dependencies and small binaries (see <a href="#section-approach-and-safety">approach and safety</a> for details).
//! * **efficient**: `ConcurrentBag` is a lock free structure making use of atomic primitives, this leads to high performance growth. You may see the details in <a href="#section-benchmarks">benchmarks</a> and further <a href="#section-performance-notes">performance notes</a>.
//!
//! Note that `ConcurrentBag` is a **write only** collection which is optimized for high performance concurrent collection. When we need to safely read elements while the collection concurrently grows, [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) is preferable due to its additional safety guarantees. Having almost identical api, switching between `ConcurrentBag` and `ConcurrentVec` is straightforward.
//!
//! # A. Examples
//!
//! Safety guarantees to push to the bag with a shared reference makes it easy to share the bag among threads. `std::sync::Arc` can be used; however, it is not required as demonstrated below.
//!
//! ```rust
//! use orx_concurrent_bag::*;
//!
//! let (num_threads, num_items_per_thread) = (4, 1_024);
//!
//! let bag = ConcurrentBag::new();
//!
//! // just take a reference and share among threads
//! let bag_ref = &bag;
//!
//! std::thread::scope(|s| {
//!     for i in 0..num_threads {
//!         s.spawn(move || {
//!             for j in 0..num_items_per_thread {
//!                 // concurrently collect results simply by calling `push`
//!                 bag_ref.push(i * 1000 + j);
//!             }
//!         });
//!     }
//! });
//!
//! assert_eq!(bag.len(), num_threads * num_items_per_thread);
//!
//! let pinned_vec = bag.into_inner();
//! assert_eq!(pinned_vec.len(), num_threads * num_items_per_thread);
//! ```
//!
//! <div id="section-approach-and-safety"></div>
//!
//! # B. Approach and Safety
//!
//! `ConcurrentBag` aims to enable concurrent growth with a minimalistic approach. It requires two major components for this:
//! * The underlying storage, which is any `PinnedVec` implementation. Pinned vectors guarantee that memory locations of its elements will never change, unless explicitly changed. This guarantee eliminates a certain set of safety concerns and corresponding complexity.
//! * An atomic counter that is responsible for uniquely assigning one vector position to one and only one thread. `std::sync::atomic::AtomicUsize` and its `fetch_add` method are sufficient for this.
//!
//! Simplicity and safety of the approach can be observed in the implementation of the `push` method.
//!
//! ```rust ignore
//! pub fn push(&self, value: T) -> usize {
//!     let idx = self.len.fetch_add(1, Ordering::AcqRel);
//!     self.assert_has_capacity_for(idx);
//!
//!     loop {
//!         let capacity = self.capacity.load(Ordering::Relaxed);
//!
//!         match idx.cmp(&capacity) {
//!             // no need to grow, just push
//!             std::cmp::Ordering::Less => {
//!                 self.write(idx, value);
//!                 break;
//!             }
//!
//!             // we are responsible for growth
//!             std::cmp::Ordering::Equal => {
//!                 let new_capacity = self.grow_to(capacity + 1);
//!                 self.capacity.store(new_capacity, Ordering::Relaxed);
//!                 self.write(idx, value);
//!                 break;
//!             }
//!
//!             // spin to wait for responsible thread to handle growth
//!             std::cmp::Ordering::Greater => {}
//!         }
//!     }
//!
//!     idx
//! }
//! ```
//!
//! Below are some details about this implementation:
//! * `fetch_add` guarantees that each pushed `value` receives a unique idx.
//! * `assert_has_capacity_for` method is an additional safety guarantee added to pinned vectors to prevent any possible UB. It is not constraining for practical usage, see [`ConcurrentBag::maximum_capacity`] for details.
//! * Inside the loop, we read the current `capacity` and compare it with `idx`:
//!   * `idx < capacity`:
//!     * The `idx`-th position is already allocated and belongs to the bag. We can simply write. Note that concurrent bag is write-only. Therefore, there is no other thread writing to or reading from this position; and hence, no race condition is present.
//!   * `idx > capacity`:
//!     * The `idx`-th position is not yet allocated. Underlying pinned vector needs to grow.
//!     * But another thread is responsible for the growth, we simply wait.
//!   * `idx == capacity`:
//!     * The `idx`-th position is not yet allocated. Underlying pinned vector needs to grow.
//!     * Further, we are responsible for the growth. Note that this guarantees that:
//!       * Only one thread will make the growth calls.
//!       * Only one growth call can take place at a given time.
//!       * There exists no race condition for the growth.
//!     * We first grow the pinned vector, then write to the `idx`-th position, and finally update the `capacity` to the new capacity.
//!
//! <div id="section-benchmarks"></div>
//!
//! # C. Benchmarks
//!
//! ## Performance with `push`
//!
//! *You may find the details of the benchmarks at [benches/collect_with_push.rs](https://github.com/orxfun/orx-concurrent-bag/blob/main/benches/collect_with_push.rs).*
//!
//! In the experiment, `rayon`s parallel iterator and `ConcurrentBag`s `push` method are used to collect results from multiple threads.
//!
//! ```rust ignore
//! // reserve and push one position at a time
//! for j in 0..num_items_per_thread {
//!     bag_ref.push(i * 1000 + j);
//! }
//! ```
//!
//! * We observe that rayon is significantly faster when the output is very small (`i32` in this experiment).
//! * As the output gets larger and copies become costlier (`[i32; 32]` here), `ConcurrentBag::push` starts to outperform.
//!
//! The issue leading to poor performance in the *small data & little work* situation can be avoided by using `extend` method in such cases. You may see its impact in the succeeding subsections and related reasons in the <a href="#section-performance-notes">performance notes</a>.
//!
//! <img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_push.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_push.PNG" />
//!
//! ## Performance of `extend`
//!
//! *You may find the details of the benchmarks at [benches/collect_with_extend.rs](https://github.com/orxfun/orx-concurrent-bag/blob/main/benches/collect_with_extend.rs).*
//!
//! The only difference in this follow up experiment is that we use `extend` rather than `push` with `ConcurrentBag`. The expectation is that this approach will solve the performance degradation due to false sharing in the *small data & little work* situation.
//!
//! ```rust ignore
//! // reserve num_items_per_thread positions at a time
//! // and then push as the iterator yields
//! let iter = (0..num_items_per_thread).map(|j| i * 100000 + j);
//! bag_ref.extend(iter);
//! ```
//!
//! We now observe that `ConcurrentBag` is comparable to, if not faster than, rayon's parallel iterator in the small data case. The performance improvement is less dramatic but still significant in the larger output size case.
//!
//! <img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_extend.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_extend.PNG" />
//!
//! Note that we do not need to have perfect information on the number of items to be pushed per thread to get the benefits of `extend`, we can simply `step_by`. Extending by `batch_size` elements will already prevent the dramatic performance degradation provided that `batch_size` elements exceed a cache line.
//!
//! ```rust ignore
//! // reserve batch_size positions at a time
//! // and then push as the iterator yields
//! for j in (0..num_items_per_thread).step_by(batch_size) {
//!     let iter = (j..(j + batch_size)).map(|j| i * 100000 + j);
//!     bag_ref.extend(iter);
//! }
//! ```
//!
//! <div id="section-performance-notes"></div>
//!
//! # D. Performance Notes
//!
//! ## How many times and how long we spin?
//!
//! Consider the `push` method, implementation of which is provided above. We will spin in the `std::cmp::Ordering::Greater => {}` branch. Fortunately, number of times we will spin is **deterministic**. It is exactly equal to the number of growth calls of the underlying pinned vector, and pinned vector implementations give a detailed control on this. For instance, assume that we will push a total of 15_000 elements concurrently to an empty bag.
//!
//! * Further assume we use the default `SplitVec<_, Doubling>` as the underlying pinned vector. Throughout the execution, we will allocate fragments of capacities [4, 8, 16, ..., 4096, 8192] which will lead to a total capacity of 16_380. In other words, we might possibly visit the `std::cmp::Ordering::Greater => {}` block in 12 points in time during the entire execution.
//! * If we use a `SplitVec<_, Linear>` with constant fragment lengths of 1_024, we will allocate 15 equal capacity fragments, which will lead to a total capacity of 15_360. So looping might only happen 15 times. We can drop this number to 8 if we set constant fragment capacity to 2_048; i.e., we can control the frequency of allocations.
//! * If we use the strict `FixedVec<_>`, we have to pre-allocate a safe amount and can never grow beyond this number. Therefore, there will never be any spinning.
//!
//! When we spin, we do not spin long because:
//! * Pinned vectors do not change memory locations of already pushed elements. In other words, growths are copy-free.
//! * We are only waiting for allocation of memory required for the growth with respect to the chosen growth strategy.
//!
//! ## False Sharing and How to Avoid
//!
//! [`ConcurrentBag::push`] method is implementation is simple, lock-free and efficient. However, we need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk which might lead to significant performance degradation. Fortunately, it is possible to avoid in many cases.
//!
//! ### When?
//!
//! Performance degradation due to false sharing might be observed when both of the following conditions hold:
//! * **small data**: data to be pushed is small, the more elements fitting in a cache line the bigger the risk,
//! * **little work**: multiple threads/cores are pushing to the concurrent bag with high frequency; i.e., very little or negligible work / time is required in between `push` calls.
//!
//! The example above fits this situation. Each thread only performs one multiplication and addition in between pushing elements, and the elements to be pushed are very small, just one `usize`.
//!
//! ### Why?
//!
//! * `ConcurrentBag` assigns unique positions to each value to be pushed. There is no *true* sharing among threads in the position level.
//! * However, cache lines contain more than one position.
//! * One thread updating a particular position invalidates the entire cache line on an other thread.
//! * Threads end up frequently reloading cache lines instead of doing the actual work of writing elements to the bag.
//! * This might lead to a significant performance degradation.
//!
//! Following two methods could be approached to deal with this problem.
//!
//! ### Solution-I: `extend` rather than `push`
//!
//! One very simple, effective and memory efficient solution to this problem is to use [`ConcurrentBag::extend`] rather than `push` in *small data & little work* situations.
//!
//! Assume that we will have 4 threads and each will push 1_024 elements. Instead of making 1_024 `push` calls from each thread, we can make one `extend` call from each. This would give the best performance. Further, it has zero buffer or memory cost:
//! * it is important to note that the batch of 1_024 elements are not stored temporarily in another buffer,
//! * there is no additional allocation,
//! * `extend` does nothing more than reserving the position range for the thread by incrementing the atomic counter accordingly.
//!
//! However, we do not need to have such a perfect information about the number of elements to be pushed. Performance gains after reaching the cache line size are much lesser.
//!
//! For instance, consider the challenging super small element size case, where we are collecting `i32`s. We can already achieve a very high performance by simply `extend`ing the bag by batches of 16 elements.
//!
//! As the element size gets larger, required batch size to achieve a high performance gets smaller and smaller.
//!
//! Required change in the code from `push` to `extend` is not significant. The example above could be revised as follows to avoid the performance degrading of false sharing.
//!
//! ```rust
//! use orx_concurrent_bag::*;
//!
//! let (num_threads, num_items_per_thread) = (4, 1_024);
//! let bag = ConcurrentBag::new();
//! let bag_ref = &bag;
//! let batch_size = 16;
//!
//! std::thread::scope(|s| {
//!     for i in 0..num_threads {
//!         s.spawn(move || {
//!             for j in (0..num_items_per_thread).step_by(batch_size) {
//!                 let iter = (j..(j + batch_size)).map(|j| i * 1000 + j);
//!                 bag_ref.extend(iter);
//!             }
//!         });
//!     }
//! });
//! ```
//!
//! ### Solution-II: Padding
//!
//! Another common approach to deal with false sharing is to add padding (unused bytes) between elements. There exist wrappers which automatically adds cache padding, such as crossbeam's [`CachePadded`](https://docs.rs/crossbeam-utils/latest/crossbeam_utils/struct.CachePadded.html). In other words, instead of using a `ConcurrentBag<T>`, we can use `ConcurrentBag<CachePadded<T>>`. However, this solution leads to increased memory requirement.
//!
//! <div id="section-construction-and-conversions"></div>
//!
//! # E. `From` | `into_inner`
//!
//! `ConcurrentBag` can be constructed by wrapping any pinned vector; i.e., `ConcurrentBag<T>` implements `From<P: PinnedVec<T>>`.
//! Likewise, a concurrent vector can be unwrapped without any cost to the underlying pinned vector with `into_inner` method.
//!
//! Further, there exist `with_` methods to directly construct the concurrent bag with common pinned vector implementations.
//!
//! ```rust
//! use orx_concurrent_bag::*;
//!
//! // default pinned vector -> SplitVec<T, Doubling>
//! let bag: ConcurrentBag<char> = ConcurrentBag::new();
//! let bag: ConcurrentBag<char> = Default::default();
//! let bag: ConcurrentBag<char> = ConcurrentBag::with_doubling_growth();
//! let bag: ConcurrentBag<char, SplitVec<char, Doubling>> = ConcurrentBag::with_doubling_growth();
//!
//! let bag: ConcurrentBag<char> = SplitVec::new().into();
//! let bag: ConcurrentBag<char, SplitVec<char, Doubling>> = SplitVec::new().into();
//!
//! // SplitVec with [Linear](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html) growth
//! // each fragment will have capacity 2^10 = 1024
//! // and the split vector can grow up to 32 fragments
//! let bag: ConcurrentBag<char, SplitVec<char, Linear>> = ConcurrentBag::with_linear_growth(10, 32);
//! let bag: ConcurrentBag<char, SplitVec<char, Linear>> = SplitVec::with_linear_growth_and_fragments_capacity(10, 32).into();
//!
//! // [FixedVec](https://docs.rs/orx-fixed-vec/latest/orx_fixed_vec/) with fixed capacity.
//! // Fixed vector cannot grow; hence, pushing the 1025-th element to this bag will cause a panic!
//! let bag: ConcurrentBag<char, FixedVec<char>> = ConcurrentBag::with_fixed_capacity(1024);
//! let bag: ConcurrentBag<char, FixedVec<char>> = FixedVec::new(1024).into();
//! ```
//!
//! Of course, the pinned vector to be wrapped does not need to be empty.
//!
//! ```rust
//! use orx_concurrent_bag::*;
//!
//! let split_vec: SplitVec<i32> = (0..1024).collect();
//! let bag: ConcurrentBag<_> = split_vec.into();
//! ```
//!
//! # License
//!
//! This library is licensed under MIT license. See LICENSE for details.

#![warn(
    missing_docs,
    clippy::unwrap_in_result,
    clippy::unwrap_used,
    clippy::panic,
    clippy::panic_in_result_fn,
    clippy::float_cmp,
    clippy::float_cmp_const,
    clippy::missing_panics_doc,
    clippy::todo
)]

mod bag;
mod common_traits;
mod errors;
mod mem_fill;

/// Common relevant traits, structs, enums.
pub mod prelude;

pub use bag::ConcurrentBag;
pub use mem_fill::{EagerWithDefault, FillStrategy, Lazy};
pub use orx_fixed_vec::FixedVec;
pub use orx_pinned_vec::{CapacityState, PinnedVec, PinnedVecGrowthError};
pub use orx_split_vec::{Doubling, Linear, Recursive, SplitVec};
