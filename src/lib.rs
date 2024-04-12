//! # orx-concurrent-bag
//!
//! [![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
//! [![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)
//!
//! An efficient, convenient and lightweight grow-only concurrent data structure allowing high performance concurrent collection.
//!
//! * **convenient**: `ConcurrentBag` can safely be shared among threads simply as a shared reference. It is a [`PinnedConcurrentCol`](https://crates.io/crates/orx-pinned-concurrent-col) with a special concurrent state implementation. Underlying [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) and concurrent bag can be converted back and forth to each other.
//! * **efficient**: `ConcurrentBag` is a lock free structure making use of a few atomic primitives, this leads to high performance concurrent growth. You may see the details in <a href="#section-benchmarks">benchmarks</a> and further <a href="#section-performance-notes">performance notes</a>.
//!
//! Note that `ConcurrentBag` is write only (with the safe api), see [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read & write variant.
//!
//! # Examples
//!
//! Safety guarantees to push to the bag with a shared reference makes it easy to share the bag among threads. `std::sync::Arc` can be used; however, it is not required as demonstrated below.
//!
//!  ```rust
//! use orx_concurrent_bag::*;
//!
//! let (num_threads, num_items_per_thread) = (4, 1_024);
//!
//! let bag = ConcurrentBag::new();
//!
//! // just take a reference and share among threads
//! let con_bag = &bag;
//!
//! std::thread::scope(|s| {
//!     for i in 0..num_threads {
//!         s.spawn(move || {
//!             for j in 0..num_items_per_thread {
//!                 // concurrently collect results simply by calling `push`
//!                 con_bag.push(i * 1000 + j);
//!             }
//!         });
//!     }
//! });
//!
//! let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
//! vec_from_bag.sort();
//! let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
//! expected.sort();
//! assert_eq!(vec_from_bag, expected);
//! ```
//!
//! ### Construction
//!
//! `ConcurrentBag` can be constructed by wrapping any pinned vector; i.e., `ConcurrentBag<T>` implements `From<P: PinnedVec<T>>`. Likewise, a concurrent vector can be unwrapped without any allocation to the underlying pinned vector with `into_inner` method.
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
//! # Concurrent State and Properties
//!
//! The concurrent state is modeled simply by an atomic length. Combination of this state and `PinnedConcurrentCol` leads to the following properties:
//! * Writing to a position of the collection does not block other writes, multiple writes can happen concurrently.
//! * Each position is written exactly once.
//! * ⟹ no write & write race condition exists.
//! * Only one growth can happen at a given time.
//! * Underlying pinned vector is always valid and can be taken out any time by `into_inner(self)`.
//! * Reading is only possible after converting the bag into the underlying `PinnedVec`.
//! * ⟹ no read & write race condition exists.
//!
//! <div id="section-benchmarks"></div>
//!
//! # Benchmarks
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
//! ## Performance with `extend`
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
//! ## Performance Notes
//!
//! ### How many times and how long we spin?
//!
//! There is only one waiting or spinning condition of the push and extend methods: whenever the underlying pinned vector needs to grow. Note that growth with pinned vector is copy free. Therefore, when it spins, all it waits for is the allocation. Further, note that number of times we will allocate, and hence spin, is deterministic.
//!
//! For instance, assume that we will push a total of 15_000 elements concurrently to an empty bag.
//!
//! * Further assume we use the default `SplitVec<_, Doubling>` as the underlying pinned vector. Throughout the execution, we will allocate fragments of capacities [4, 8, 16, ..., 4096, 8192] which will lead to a total capacity of 16_380. In other words, we will allocate exactly 12 times during the entire execution.
//! * If we use a `SplitVec<_, Linear>` with constant fragment lengths of 1_024, we will allocate 15 equal capacity fragments.
//! * If we use the strict `FixedVec<_>`, we have to pre-allocate a safe amount and can never grow beyond this number. Therefore, there will never be any spinning.
//!
//! ### False Sharing and How to Avoid
//!
//! [`ConcurrentBag::push`] method is implementation is simple, lock-free and efficient. However, we need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk which might lead to significant performance degradation. Fortunately, it is possible to avoid in many cases.
//!
//! #### When?
//!
//! Performance degradation due to false sharing might be observed specifically when both of the following conditions hold:
//! * **small data**: data to be pushed is small, the more elements fitting in a cache line the bigger the risk,
//! * **little work**: multiple threads/cores are pushing to the concurrent bag with high frequency; i.e., very little or negligible work / time is required in between `push` calls.
//!
//! The example above fits this situation. Each thread only performs one multiplication and addition in between pushing elements, and the elements to be pushed are very small, just one `usize`.
//!
//! #### Why?
//!
//! * `ConcurrentBag` assigns unique positions to each value to be pushed. There is no *true* sharing among threads in the position level.
//! * However, cache lines contain more than one position.
//! * One thread updating a particular position invalidates the entire cache line on an other thread.
//! * Threads end up frequently reloading cache lines instead of doing the actual work of writing elements to the bag.
//! * This might lead to a significant performance degradation.
//!
//! Following two methods could be approached to deal with this problem.
//!
//! #### Solution-I: `extend` rather than `push`
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
//! #### Solution-II: Padding
//!
//! Another common approach to deal with false sharing is to add padding (unused bytes) between elements. There exist wrappers which automatically adds cache padding, such as crossbeam's [`CachePadded`](https://docs.rs/crossbeam-utils/latest/crossbeam_utils/struct.CachePadded.html). In other words, instead of using a `ConcurrentBag<T>`, we can use `ConcurrentBag<CachePadded<T>>`. However, this solution leads to increased memory requirement.
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
mod new;
mod state;

/// Common relevant traits, structs, enums.
pub mod prelude;

pub use bag::ConcurrentBag;
pub use orx_fixed_vec::FixedVec;
pub use orx_pinned_vec::{CapacityState, PinnedVec, PinnedVecGrowthError};
pub use orx_split_vec::{Doubling, Linear, Recursive, SplitVec};
