//! # orx-concurrent-bag
//!
//! [![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
//! [![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)
//!
//! An efficient, convenient and lightweight grow-only concurrent collection, ideal for collecting results concurrently.
//! * **convenient**: the bag can be shared among threads simply as a shared reference, not even requiring `Arc`,
//! * **efficient**: for collecting results concurrently:
//!   * rayon is significantly faster when the elements are small and there is an extreme load (no work at all among push calls);
//!   * `ConcurrentBag` starts to perform faster as elements or the computation in between push calls get larger (see <a href="#section-benchmarks">E. Benchmarks</a> for the experiments).
//! * **lightweight**: due to the simplistic approach taken, it enables concurrent programs with smaller binary sizes.
//!
//! The bag preserves the order of elements with respect to the order the `push` method is called.
//!
//! # Examples
//!
//! Safety guarantees to push to the bag with an immutable reference makes it easy to share the bag among threads.
//!
//! ## Using `std::sync::Arc`
//!
//! We can share our bag among threads using `Arc` and collect results concurrently.
//!
//! ```rust
//! use orx_concurrent_bag::*;
//! use std::{sync::Arc, thread};
//!
//! let (num_threads, num_items_per_thread) = (4, 8);
//!
//! let bag = Arc::new(ConcurrentBag::new());
//! let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();
//!
//! for i in 0..num_threads {
//!     let bag = bag.clone();
//!     thread_vec.push(thread::spawn(move || {
//!         for j in 0..num_items_per_thread {
//!             // concurrently collect results simply by calling `push`
//!             bag.push(i * 1000 + j);
//!         }
//!     }));
//! }
//!
//! for handle in thread_vec {
//!     handle.join().unwrap();
//! }
//!
//! let mut vec_from_bag: Vec<_> = unsafe { bag.iter() }.copied().collect();
//! vec_from_bag.sort();
//! let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
//! expected.sort();
//! assert_eq!(vec_from_bag, expected);
//! ```
//!
//! ## Using `std::thread::scope`
//!
//! An even more convenient approach would be to use thread scopes. This allows to use shared reference of the bag across threads, instead of `Arc`.
//!
//! ```rust
//! use orx_concurrent_bag::*;
//!
//! let (num_threads, num_items_per_thread) = (4, 8);
//!
//! let bag = ConcurrentBag::new();
//! let bag_ref = &bag; // just take a reference
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
//! let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
//! vec_from_bag.sort();
//! let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
//! expected.sort();
//! assert_eq!(vec_from_bag, expected);
//! ```
//!
//! # Safety
//!
//! `ConcurrentBag` uses a [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) implementation as the underlying storage (see [`SplitVec`](https://crates.io/crates/orx-split-vec) and [`Fixed`](https://crates.io/crates/orx-fixed-vec)).
//! `PinnedVec` guarantees that elements which are already pushed to the vector stay pinned to their memory locations unless explicitly changed due to removals, which is not the case here since `ConcurrentBag` is a grow-only collection.
//! This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).
//!
//! In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `PinnedVec` with an `AtomicUsize`.
//! * `len: AtomicSize`: fixes the target memory location of each element to be pushed at the time the `push` method is called. Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
//! * `PinnedVec` guarantees that already pushed elements are not moved around in memory during growth. This also enables the following mode of concurrency:
//!   * one thread might allocate new memory in order to grow when capacity is reached,
//!   * while another thread might concurrently be writing to any of the already allocation memory locations.
//!
//! The approach guarantees that
//! * only one thread can write to the memory location of an element being pushed to the bag,
//! * at any point in time, only one thread is responsible for the allocation of memory if the bag requires new memory,
//! * no thread reads any of the written elements (reading happens after converting the bag `into_inner`),
//! * hence, there exists no race condition.
//!
//! # Construction
//!
//! As explained above, `ConcurrentBag` is simply a tuple of a `PinnedVec` and an `AtomicUsize`..
//! Therefore, it can be constructed by wrapping any pinned vector; i.e., `ConcurrentBag<T>` implements `From<P: PinnedVec<T>>`.
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
//! // SplitVec with [Recursive](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Recursive.html) growth
//! let bag: ConcurrentBag<char, SplitVec<char, Recursive>> =
//!     ConcurrentBag::with_recursive_growth();
//! let bag: ConcurrentBag<char, SplitVec<char, Recursive>> =
//!     SplitVec::with_recursive_growth().into();
//!
//! // SplitVec with [Linear](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html) growth
//! // each fragment will have capacity 2^10 = 1024
//! let bag: ConcurrentBag<char, SplitVec<char, Linear>> = ConcurrentBag::with_linear_growth(10);
//! let bag: ConcurrentBag<char, SplitVec<char, Linear>> = SplitVec::with_linear_growth(10).into();
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
//! # Write-Only vs Read-Write
//!
//! The concurrent bag is write-only & grow-only bag which is convenient and efficient for collecting elements.
//!
//! See [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read-and-write variant which
//! * guarantees that reading and writing never happen concurrently, and hence,
//! * allows safe iteration or access to already written elements of the concurrent vector,
//! * with a minor additional cost of values being wrapped by an `Option`.
//!
//! <div id="section-benchmarks"></div>
//!
//! # Benchmarks
//!
//! *You may find the details of the benchmarks at [benches/grow.rs](https://github.com/orxfun/orx-concurrent-bag/blob/main/benches/grow.rs).*
//!
//! In the experiment, `ConcurrentBag` variants and `rayon` is used to collect results from multiple threads. You may see in the table below that `rayon` is extremely fast with very small output data (`i32` in this case). As the output size gets larger and copies become costlier, `ConcurrentBag` starts to perform faster.
//!
//! <img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_grow.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_grow.PNG" />

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
/// The concurrent bag prelude, along with the `ConcurrentBag`, imports relevant `SplitVec` and `PinnedVec` types.
pub mod prelude;

pub use bag::ConcurrentBag;
pub use orx_fixed_vec::FixedVec;
pub use orx_pinned_vec::PinnedVec;
pub use orx_split_vec::{Doubling, Linear, Recursive, SplitVec};
