use crate::errors::{ERR_FAILED_TO_GROW, ERR_FAILED_TO_PUSH, ERR_REACHED_MAX_CAPACITY};
use crate::mem_fill::{FillStrategy, Lazy};
use orx_fixed_vec::FixedVec;
use orx_pinned_vec::{PinnedVec, PinnedVecGrowthError};
use orx_split_vec::{Doubling, Linear, SplitVec};
use std::cell::UnsafeCell;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An efficient, convenient and lightweight grow-only concurrent collection, ideal for collecting results concurrently.
///
/// * **convenient**: `ConcurrentBag` can safely be shared among threads simply as a shared reference. Further, it is just a wrapper around any [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) implementation adding concurrent safety guarantees. Therefore, underlying pinned vector and concurrent bag can be converted to each other back and forth without any cost.
/// * **lightweight**: This crate takes a simplistic approach built on pinned vector guarantees which leads to concurrent programs with few dependencies and small binaries (see <a href="#section-approach-and-safety">approach and safety</a> for details).
/// * **efficient**: `ConcurrentBag` is a lock free structure making use of a few atomic primitives. rayon is significantly faster when collecting small results under an extreme load (negligible work to compute results); however, `ConcurrentBag` starts to perform faster as result types get larger (see <a href="#section-benchmarks">benchmarks</a> for the experiments).
///
/// # Examples
///
/// Safety guarantees to push to the bag with a shared reference makes it easy to share the bag among threads. `std::sync::Arc` can be used; however, it is not required as demonstrated below.
///
/// ```rust
/// use orx_concurrent_bag::*;
///
/// let (num_threads, num_items_per_thread) = (4, 1_024);
///
/// let bag = ConcurrentBag::new();
/// let bag_ref = &bag; // just take a reference and share among threads
///
/// std::thread::scope(|s| {
///     for i in 0..num_threads {
///         s.spawn(move || {
///             for j in 0..num_items_per_thread {
///                 // concurrently collect results simply by calling `push`
///                 bag_ref.push(i * 1000 + j);
///             }
///         });
///     }
/// });
///
/// let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
/// vec_from_bag.sort();
/// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
/// expected.sort();
/// assert_eq!(vec_from_bag, expected);
/// ```
///
/// <div id="section-approach-and-safety"></div>
///
/// # Approach and Safety
///
/// `ConcurrentBag` aims to enable concurrent growth with a minimalistic approach. It requires two major components for this:
/// * The underlying storage, which is any `PinnedVec` implementation. This means that memory locations of elements that are already pushed to the vector will never change, unless explicitly changed. This guarantee eliminates a certain set of safety concerns and corresponding complexity.
/// * An atomic counter that is responsible for uniquely assigning one vector position to one and only one thread. `std::sync::atomic::AtomicUsize` and its `fetch_add` method are sufficient for this.
///
/// Simplicity and safety of the approach can be observed in the implementation of the `push` method.
///
/// ```rust ignore
/// pub fn push(&self, value: T) -> usize {
///     let idx = self.len.fetch_add(1, Ordering::AcqRel);
///     self.assert_has_capacity_for(idx);
///
///     loop {
///         let capacity = self.capacity.load(Ordering::Relaxed);
///
///         match idx.cmp(&capacity) {
///             // no need to grow, just push
///             std::cmp::Ordering::Less => {
///                 self.write(idx, value);
///                 break;
///             }
///
///             // we are responsible for growth
///             std::cmp::Ordering::Equal => {
///                 let new_capacity = self.grow_to(capacity + 1);
///                 self.write(idx, value);
///                 self.capacity.store(new_capacity, Ordering::Relaxed);
///                 break;
///             }
///
///             // spin to wait for responsible thread to handle growth
///             std::cmp::Ordering::Greater => {}
///         }
///     }
///
///     idx
/// }
/// ```
///
/// Below are some details about this implementation:
/// * `fetch_add` guarantees that each pushed `value` receives a unique idx.
/// * `assert_has_capacity_for` method is an additional safety guarantee added to pinned vectors to prevent any possible UB. It is not constraining for practical usage, see [`ConcurrentBag::maximum_capacity`] for details.
/// * Inside the loop, we read the current `capacity` and compare it with `idx`:
///   * `idx < capacity`:
///     * The `idx`-th position is already allocated and belongs to the bag. We can simply write. Note that concurrent bag is write-only. Therefore, there is no other thread writing to or reading from this position; and hence, no race condition is present.
///   * `idx > capacity`:
///     * The `idx`-th position is not yet allocated. Underlying pinned vector needs to grow.
///     * But another thread is responsible for the growth, we simply wait.
///   * `idx == capacity`:
///     * The `idx`-th position is not yet allocated. Underlying pinned vector needs to grow.
///     * Further, we are responsible for the growth. Note that this guarantees that:
///       * Only one thread will make the growth calls.
///       * Only one growth call can take place at a given time.
///       * There exists no race condition for the growth.
///     * We first grow the pinned vector, then write to the `idx`-th position, and finally update the `capacity` to the new capacity.
///
/// ## How many times will we spin?
///
/// This is **deterministic**. It is exactly equal to the number of growth calls of the underlying pinned vector, and pinned vector implementations give a detailed control on this. For instance, assume that we will push a total of 15_000 elements concurrently to an empty bag.
///
/// * Further assume we use the default `SplitVec<_, Doubling>` as the underlying pinned vector. Throughout the execution, we will allocate fragments of capacities [4, 8, 16, ..., 4096, 8192] which will lead to a total capacity of 16_380. In other words, we might possibly visit the `std::cmp::Ordering::Greater => {}` block in 12 points in time during the entire execution.
/// * If we use a `SplitVec<_, Linear>` with constant fragment lengths of 1_024, we will allocate 15 equal capacity fragments, which will lead to a total capacity of 15_360. So looping might only happen 15 times. We can drop this number to 8 if we set constant fragment capacity to 2_048; i.e., we can control the frequency of allocations.
/// * If we use the strict `FixedVec<_>`, we have to pre-allocate a safe amount and can never grow beyond this number. Therefore, there will never be any spinning.
///
/// ## When we spin, how long do we spin?
///
/// Not long because:
/// * Pinned vectors do not change memory locations of already pushed elements. In other words, growths are copy-free.
/// * We are only waiting for allocation of memory required for the growth with respect to the chosen growth strategy.
///
/// # Construction
///
/// `ConcurrentBag` can be constructed by wrapping any pinned vector; i.e., `ConcurrentBag<T>` implements `From<P: PinnedVec<T>>`.
/// Likewise, a concurrent vector can be unwrapped without any cost to the underlying pinned vector with `into_inner` method.
///
/// Further, there exist `with_` methods to directly construct the concurrent bag with common pinned vector implementations.
///
/// ```rust
/// use orx_concurrent_bag::*;
///
/// // default pinned vector -> SplitVec<T, Doubling>
/// let bag: ConcurrentBag<char> = ConcurrentBag::new();
/// let bag: ConcurrentBag<char> = Default::default();
/// let bag: ConcurrentBag<char> = ConcurrentBag::with_doubling_growth();
/// let bag: ConcurrentBag<char, SplitVec<char, Doubling>> = ConcurrentBag::with_doubling_growth();
///
/// let bag: ConcurrentBag<char> = SplitVec::new().into();
/// let bag: ConcurrentBag<char, SplitVec<char, Doubling>> = SplitVec::new().into();
///
/// // SplitVec with [Linear](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html) growth
/// // each fragment will have capacity 2^10 = 1024
/// // and the split vector can grow up to 32 fragments
/// let bag: ConcurrentBag<char, SplitVec<char, Linear>> = ConcurrentBag::with_linear_growth(10, 32);
/// let bag: ConcurrentBag<char, SplitVec<char, Linear>> = SplitVec::with_linear_growth_and_fragments_capacity(10, 32).into();
///
/// // [FixedVec](https://docs.rs/orx-fixed-vec/latest/orx_fixed_vec/) with fixed capacity.
/// // Fixed vector cannot grow; hence, pushing the 1025-th element to this bag will cause a panic!
/// let bag: ConcurrentBag<char, FixedVec<char>> = ConcurrentBag::with_fixed_capacity(1024);
/// let bag: ConcurrentBag<char, FixedVec<char>> = FixedVec::new(1024).into();
/// ```
///
/// Of course, the pinned vector to be wrapped does not need to be empty.
///
/// ```rust
/// use orx_concurrent_bag::*;
///
/// let split_vec: SplitVec<i32> = (0..1024).collect();
/// let bag: ConcurrentBag<_> = split_vec.into();
/// ```
pub struct ConcurrentBag<T, P = SplitVec<T, Doubling>>
where
    P: PinnedVec<T>,
{
    pinned: UnsafeCell<P>,
    len: AtomicUsize,
    capacity: AtomicUsize,
    maximum_capacity: UnsafeCell<usize>,
    phantom: PhantomData<T>,
}

// new
impl<T> ConcurrentBag<T, SplitVec<T, Doubling>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Doubling>` as the underlying storage.
    pub fn with_doubling_growth() -> Self {
        Self::new_from_pinned(SplitVec::with_doubling_growth_and_fragments_capacity(32))
    }

    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Doubling>` as the underlying storage.
    pub fn new() -> Self {
        Self::with_doubling_growth()
    }
}

impl<T> Default for ConcurrentBag<T, SplitVec<T, Doubling>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Doubling>` as the underlying storage.
    fn default() -> Self {
        Self::with_doubling_growth()
    }
}

impl<T> ConcurrentBag<T, SplitVec<T, Linear>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Linear>` as the underlying storage.
    ///
    /// # Notes
    ///
    /// * `Linear` can be chosen over `Doubling` whenever memory efficiency is more critical since linear allocations lead to less waste.
    /// * Choosing a small `constant_fragment_capacity_exponent` for a large bag to be filled might lead to too many growth calls.
    /// * Furthermore, `Linear` growth strategy leads to a hard upper bound on the maximum capacity, please see the Safety section.
    ///
    /// # Safety
    ///
    /// `SplitVec<T, Linear>` can grow indefinitely (almost).
    ///
    /// However, `ConcurrentBag<T, SplitVec<T, Linear>>` has an upper bound on its capacity.
    /// This capacity is computed as:
    ///
    /// ```rust ignore
    /// 2usize.pow(constant_fragment_capacity_exponent) * fragments_capacity
    /// ```
    ///
    /// For instance maximum capacity is 2^10 * 64 = 65536 when `constant_fragment_capacity_exponent=10` and `fragments_capacity=64`.
    ///
    /// Note that setting a relatively high and safe `fragments_capacity` is **not** costly, size of each element 2*usize.
    ///
    /// Pushing to the vector beyond this capacity leads to "out-of-capacity" error.
    ///
    /// This capacity can be accessed by [`ConcurrentBag::maximum_capacity`] method.
    pub fn with_linear_growth(
        constant_fragment_capacity_exponent: usize,
        fragments_capacity: usize,
    ) -> Self {
        Self::new_from_pinned(SplitVec::with_linear_growth_and_fragments_capacity(
            constant_fragment_capacity_exponent,
            fragments_capacity,
        ))
    }
}

impl<T> ConcurrentBag<T, FixedVec<T>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `FixedVec<T>` as the underlying storage.
    ///
    /// # Safety
    ///
    /// Note that a `FixedVec` cannot grow; i.e., it has a hard upper bound on the number of elements it can hold, which is the `fixed_capacity`.
    ///
    /// Pushing to the vector beyond this capacity leads to "out-of-capacity" error.
    ///
    /// This capacity can be accessed by [`ConcurrentBag::maximum_capacity`] method.
    pub fn with_fixed_capacity(fixed_capacity: usize) -> Self {
        Self::new_from_pinned(FixedVec::new(fixed_capacity))
    }
}

impl<T, P> From<P> for ConcurrentBag<T, P>
where
    P: PinnedVec<T>,
{
    fn from(value: P) -> Self {
        Self::new_from_pinned(value)
    }
}

// impl
impl<T, P> ConcurrentBag<T, P>
where
    P: PinnedVec<T>,
{
    /// Consumes the concurrent vector and returns the underlying pinned vector.
    ///
    /// Note that
    /// * it is cheap to wrap a `SplitVec` as a `ConcurrentBag` using thee `From` trait;
    /// * and similarly to convert a `ConcurrentBag` to the underlying `SplitVec` using `into_inner` method.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// bag.push('a');
    /// bag.push('b');
    /// bag.push('c');
    /// bag.push('d');
    /// assert_eq!(vec!['a', 'b', 'c', 'd'], unsafe { bag.iter() }.copied().collect::<Vec<_>>());
    ///
    /// let mut split = bag.into_inner();
    /// assert_eq!(vec!['a', 'b', 'c', 'd'], split.iter().copied().collect::<Vec<_>>());
    ///
    /// split.push('e');
    /// *split.get_mut(0).expect("exists") = 'x';
    ///
    /// assert_eq!(vec!['x', 'b', 'c', 'd', 'e'], split.iter().copied().collect::<Vec<_>>());
    ///
    /// let mut bag: ConcurrentBag<_> = split.into();
    /// assert_eq!(vec!['x', 'b', 'c', 'd', 'e'], unsafe { bag.iter() }.copied().collect::<Vec<_>>());
    ///
    /// bag.clear();
    /// assert!(bag.is_empty());
    ///
    /// let split = bag.into_inner();
    /// assert!(split.is_empty());
    pub fn into_inner(self) -> P {
        unsafe { self.correct_pinned_len() };
        self.pinned.into_inner()
    }

    /// ***O(1)*** Returns the number of elements which are pushed to the vector, including the elements which received their reserved locations and are currently being pushed.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::new();
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// assert_eq!(2, bag.len());
    /// ```
    #[inline(always)]
    pub fn len(&self) -> usize {
        self.len.load(Ordering::SeqCst)
    }

    /// ***O(1)*** Returns the current capacity of the concurrent bag; i.e., the underlying pinned vector storage.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::new();
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// assert_eq!(4, bag.capacity());
    ///
    /// bag.push('c');
    /// bag.push('d');
    /// bag.push('e');
    ///
    /// assert_eq!(12, bag.capacity());
    /// ```
    #[inline(always)]
    pub fn capacity(&self) -> usize {
        self.capacity.load(Ordering::Relaxed)
    }

    /// Note that a `ConcurrentBag` contains two capacity methods:
    /// * `capacity`: returns current capacity of the underlying pinned vector; this capacity can grow concurrently with a `&self` reference; i.e., during a `push` call
    /// * `maximum_capacity`: returns the maximum potential capacity that the pinned vector can grow up to with a `&self` reference;
    ///   * `push`ing a new element while the bag is at `maximum_capacity` leads to panic.
    ///
    /// On the other hand, maximum capacity can safely be increased by a mutually exclusive `&mut self` reference using the `reserve_maximum_capacity`.
    /// However, underlying pinned vector must be able to provide pinned-element guarantees during this operation.
    ///
    /// Among the common pinned vector implementations:
    /// * `SplitVec<_, Doubling>`: supports this method; however, it does not require for any practical size.
    /// * `SplitVec<_, Linear>`: is guaranteed to succeed and increase its maximum capacity to the required value.
    /// * `FixedVec<_>`: is the most strict pinned vector which cannot grow even in a single-threaded setting. Currently, it will always return an error to this call.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use orx_pinned_vec::PinnedVecGrowthError;
    ///
    ///  // SplitVec<_, Doubling> (default)
    /// let bag: ConcurrentBag<char> = ConcurrentBag::new();
    /// assert_eq!(bag.capacity(), 4); // only allocates the first fragment of 4
    /// assert_eq!(bag.maximum_capacity(), 17_179_869_180); // it can grow safely & exponentially
    ///
    /// let bag: ConcurrentBag<char, _> = ConcurrentBag::with_doubling_growth();
    /// assert_eq!(bag.capacity(), 4);
    /// assert_eq!(bag.maximum_capacity(), 17_179_869_180);
    ///
    /// // SplitVec<_, Linear>
    /// let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_linear_growth(10, 20);
    /// assert_eq!(bag.capacity(), 2usize.pow(10)); // only allocates first fragment of 1024
    /// assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 20); // it can concurrently allocate 19 more
    ///
    /// // SplitVec<_, Linear> -> reserve_maximum_capacity
    /// let result = bag.reserve_maximum_capacity(2usize.pow(10) * 30);
    /// assert_eq!(result, Ok(2usize.pow(10) * 30));
    ///
    /// // actually no new allocation yet; precisely additional memory for 10 pairs of pointers is used
    /// assert_eq!(bag.capacity(), 2usize.pow(10)); // still only the first fragment capacity
    ///
    /// dbg!(bag.maximum_capacity(), 2usize.pow(10) * 30);
    /// assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 30); // now it can safely reach 2^10 * 30
    ///
    /// // FixedVec<_>: pre-allocated, exact and strict
    /// let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_fixed_capacity(42);
    /// assert_eq!(bag.capacity(), 42);
    /// assert_eq!(bag.maximum_capacity(), 42);
    ///
    /// let result = bag.reserve_maximum_capacity(43);
    /// assert_eq!(
    ///     result,
    ///     Err(PinnedVecGrowthError::FailedToGrowWhileKeepingElementsPinned)
    /// );
    /// ```
    pub fn reserve_maximum_capacity(
        &mut self,
        maximum_capacity: usize,
    ) -> Result<usize, PinnedVecGrowthError> {
        let result = self.try_grow_to(maximum_capacity);
        if let Ok(new_capacity) = result {
            let maximum_capacity = unsafe { &mut *self.maximum_capacity.get() };
            *maximum_capacity = new_capacity;
        }
        result
    }

    /// Returns the maximum possible capacity that the concurrent bag can reach.
    /// This is equivalent to the maximum capacity that the underlying pinned vector can safely reach in a concurrent program.
    ///
    /// Note that `maximum_capacity` differs from `capacity` due to the following:
    /// * `capacity` represents currently allocated memory owned by the underlying pinned vector.
    /// The bag can possibly grow beyond this value.
    /// * `maximum_capacity` represents the hard upper limit on the length of the concurrent bag.
    /// Attempting to grow the bag beyond this value leads to an error.
    /// Note that this is not necessarily a limitation for the underlying split vector; the limitation might be due to the additional safety requirements of concurrent programs.
    ///
    /// # Safety
    ///
    /// Calling `push` while the bag is at its `maximum_capacity` leads to a panic.    
    /// This condition is a safety requirement.
    /// Underlying pinned vector cannot safely grow beyond maximum capacity in a possibly concurrent call (with `&self`).
    /// This would lead to UB.
    ///
    /// It is easy to overcome this problem during construction:
    /// * It is not observed with the default pinned vector `SplitVec<_, Doubling>`:
    ///   * `ConcurrentBag::new()`
    ///   * `ConcurrentBag::with_doubling_growth()`
    /// * It can be avoided cheaply when `SplitVec<_, Linear>` is used by setting the second argument to a proper value:
    ///   * `ConcurrentBag::with_linear_growth(10, 32)`
    ///     * Each fragment of this vector will have a capacity of 2^10 = 1024.
    ///     * It can safely grow up to 32 fragments with `&self` reference.
    ///     * Therefore, it is safe to concurrently grow this vector to 32 * 1024 elements.
    ///     * Cost of replacing 32 with 64 is negligible in such a scenario (the difference in memory allocation is precisely 32 * 2*usize).
    /// * Using the variant `FixedVec<_>` requires a hard bound and pre-allocation regardless the program is concurrent or not.
    ///   * `ConcurrentBag::with_fixed_capacity(1024)`
    ///     * Unlike the split vector variants, this variant pre-allocates for 1024 elements.
    ///     * And can never grow beyond this value.
    ///
    /// Alternatively, caller can try to safely reserve the required capacity any time by a mutually exclusive reference:
    /// * `ConcurrentBag.reserve_maximum_capacity(&mut self, maximum_capacity: usize)`
    ///   * this call will always succeed with `SplitVec<_, Doubling>` and `SplitVec<_, Linear>`,
    ///   * will always fail for `FixedVec<_>`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use orx_pinned_vec::PinnedVecGrowthError;
    ///
    ///  // SplitVec<_, Doubling> (default)
    /// let bag: ConcurrentBag<char> = ConcurrentBag::new();
    /// assert_eq!(bag.capacity(), 4); // only allocates the first fragment of 4
    /// assert_eq!(bag.maximum_capacity(), 17_179_869_180); // it can grow safely & exponentially
    ///
    /// let bag: ConcurrentBag<char, _> = ConcurrentBag::with_doubling_growth();
    /// assert_eq!(bag.capacity(), 4);
    /// assert_eq!(bag.maximum_capacity(), 17_179_869_180);
    ///
    /// // SplitVec<_, Linear>
    /// let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_linear_growth(10, 20);
    /// assert_eq!(bag.capacity(), 2usize.pow(10)); // only allocates first fragment of 1024
    /// assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 20); // it can concurrently allocate 19 more
    ///
    /// // SplitVec<_, Linear> -> reserve_maximum_capacity
    /// let result = bag.reserve_maximum_capacity(2usize.pow(10) * 30);
    /// assert_eq!(result, Ok(2usize.pow(10) * 30));
    ///
    /// // actually no new allocation yet; precisely additional memory for 10 pairs of pointers is used
    /// assert_eq!(bag.capacity(), 2usize.pow(10)); // still only the first fragment capacity
    ///
    /// dbg!(bag.maximum_capacity(), 2usize.pow(10) * 30);
    /// assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 30); // now it can safely reach 2^10 * 30
    ///
    /// // FixedVec<_>: pre-allocated, exact and strict
    /// let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_fixed_capacity(42);
    /// assert_eq!(bag.capacity(), 42);
    /// assert_eq!(bag.maximum_capacity(), 42);
    ///
    /// let result = bag.reserve_maximum_capacity(43);
    /// assert_eq!(
    ///     result,
    ///     Err(PinnedVecGrowthError::FailedToGrowWhileKeepingElementsPinned)
    /// );
    /// ```
    #[inline]
    pub fn maximum_capacity(&self) -> usize {
        unsafe { *self.maximum_capacity.get() }
    }

    /// Returns whether the bag is empty or not.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let mut bag = ConcurrentBag::new();
    ///
    /// assert!(bag.is_empty());
    ///
    /// bag.push('a');
    /// bag.push('b');
    /// assert!(!bag.is_empty());
    ///
    /// bag.clear();
    /// assert!(bag.is_empty());
    /// ```
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns an iterator to elements of the bag.
    ///
    /// Iteration of elements is in the order the push method is called.
    ///
    /// # Safety
    ///
    /// This method is unsafe due to the possibility of the following scenario:
    /// * a thread reserves a position in the bag,
    /// * this increases the length of the bag by one, which includes this new element to the iteration,
    /// * however, before writing the value of the element completes, iterator reaches this element and reads uninitialized value.
    ///
    /// Note that [`ConcurrentBag`] is meant to be write-only, or even, grow-only.
    /// See [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read-and-write variant which
    /// * guarantees that reading and writing never happen concurrently, and hence,
    /// * allows safe iteration or access to already written elements of the concurrent vector,
    /// * with a minor additional cost of values being wrapped by an `Option`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::new();
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// let mut iter = unsafe { bag.iter() };
    /// assert_eq!(iter.next(), Some(&'a'));
    /// assert_eq!(iter.next(), Some(&'b'));
    /// assert_eq!(iter.next(), None);
    /// ```
    pub unsafe fn iter(&self) -> impl Iterator<Item = &T> {
        self.correct_pinned_len();
        let pinned = &*self.pinned.get() as &P;
        pinned.iter().take(self.len())
    }

    /// Returns a reference to the element at the `index`-th position of the bag.
    /// It returns `None` when index is out of bounds.
    ///
    /// # Safety
    ///
    /// `ConcurrentBag` guarantees that each position is written exactly once by a single thread.
    /// And further, no thread reads this position (see [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a safe read & write variant).
    /// Therefore, there exists no race condition.
    ///
    /// On the other, hand `get` method partially breaks this by allowing reading.
    /// Although it is still not very likely to have a race condition, there still exists a possibility which would lead to a race condition and UB.
    /// Therefore, this method is `unsafe`.
    ///
    /// The race condition could be observed in the following situation. Say we have a `bag` of `char`s and we allocate memory to store incoming characters, say 4 positions.
    /// If the following events happen in the exact order in time, we might have undefined behavior (UB):
    /// * `bag.push('a')` is called from thread#1.
    /// * `bag` atomically increases the `len` to 1.
    /// * thread#2 calls `bag.get(0)` which is now in bounds.
    /// * thread#2 receives uninitialized value (UB).
    /// * thread#1 completes writing `a` to the 0-th position (one instant too late).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// bag.push('a');
    /// bag.extend(['b', 'c', 'd']);
    ///
    /// unsafe {
    ///     assert_eq!(bag.get(0), Some(&'a'));
    ///     assert_eq!(bag.get(1), Some(&'b'));
    ///     assert_eq!(bag.get(2), Some(&'c'));
    ///     assert_eq!(bag.get(3), Some(&'d'));
    ///     assert_eq!(bag.get(4), None);
    /// }
    /// ```
    ///
    /// The following could be considered as a practical use case.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use std::time::Duration;
    ///
    /// // record measurements in (assume) random intervals
    /// let measurements = ConcurrentBag::<i32>::new();
    /// let rf_measurements = &measurements;
    ///
    /// // collect average of measurements every 50 milliseconds
    /// let averages = ConcurrentBag::new();
    /// let rf_averages = &averages;
    ///
    /// std::thread::scope(|s| {
    ///
    ///     // write to measurements
    ///     s.spawn(move || {
    ///         for i in 0..100 {
    ///             std::thread::sleep(Duration::from_millis(i % 5));
    ///             rf_measurements.push(i as i32);
    ///         }
    ///     });
    ///
    ///     // read from measurements & write to averages
    ///     s.spawn(move || {
    ///         for _ in 0..10 {
    ///             let count = rf_measurements.len();
    ///             if count == 0 {
    ///                 rf_averages.push(0.0);
    ///             } else {
    ///                 let mut sum = 0;
    ///                 for i in 0..rf_measurements.len() {
    ///                     sum += unsafe { rf_measurements.get(i) }.copied().unwrap_or(0);
    ///                 }
    ///                 let average = sum as f32 / count as f32;
    ///                 rf_averages.push(average);
    ///             }
    ///             std::thread::sleep(Duration::from_millis(10));
    ///         }
    ///     });
    /// });
    ///
    /// assert_eq!(measurements.len(), 100);
    /// assert_eq!(averages.len(), 10);
    /// ```
    pub unsafe fn get(&self, index: usize) -> Option<&T> {
        if index < self.len() && index < self.capacity() {
            let pinned = unsafe { &mut *self.pinned.get() };
            let ptr = unsafe { pinned.get_ptr_mut(index) };
            ptr.and_then(|x| x.as_ref())
        } else {
            None
        }
    }

    // mutate elements

    /// Concurrent, thread-safe method to push the given `value` to the back of the bag,
    /// and returns the position or index of the pushed value.
    ///
    /// It preserves the order of elements with respect to the order the `push` method is called.
    ///
    /// # Panics
    ///
    /// Panics if the concurrent bag is already at its maximum capacity; i.e., if `self.len() == self.maximum_capacity()`.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`ConcurrentBag::maximum_capacity`] for details.
    ///
    /// # Examples
    ///
    /// We can directly take a shared reference of the bag and share it among threads.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let (num_threads, num_items_per_thread) = (4, 1_024);
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// // just take a reference and share among threads
    /// let bag_ref = &bag;
    ///
    /// std::thread::scope(|s| {
    ///     for i in 0..num_threads {
    ///         s.spawn(move || {
    ///             for j in 0..num_items_per_thread {
    ///                 // concurrently collect results simply by calling `push`
    ///                 bag_ref.push(i * 1000 + j);
    ///             }
    ///         });
    ///     }
    /// });
    ///
    /// let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
    /// vec_from_bag.sort();
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// # Performance Notes - False Sharing
    ///
    /// [`ConcurrentBag::push`] method is implementation is simple, lock-free and efficient.
    /// However, we need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk.
    /// False sharing might lead to significant performance degradation; fortunately, it is possible to avoid in many cases.
    ///
    /// ## When?
    ///
    /// Performance degradation due to false sharing might be observed when both of the following conditions hold:
    /// * **small data**: data to be pushed is small, the more elements fitting in a cache line the bigger the risk,
    /// * **little work**: multiple threads/cores are pushing to the concurrent bag with high frequency; i.e.,
    ///   * very little or negligible work / time is required in between `push` calls.
    ///
    /// The example above fits this situation.
    /// Each thread only performs one multiplication and addition in between pushing elements, and the elements to be pushed are very small, just one `usize`.
    ///
    /// ## Why?
    ///
    /// * `ConcurrentBag` assigns unique positions to each value to be pushed. There is no *true* sharing among threads in the position level.
    /// * However, cache lines contain more than one position.
    /// * One thread updating a particular position invalidates the entire cache line on an other thread.
    /// * Threads end up frequently reloading cache lines instead of doing the actual work of writing elements to the bag.
    /// * This might lead to a significant performance degradation.
    ///
    /// Following two methods could be approached to deal with this problem.
    ///
    /// ## Solution-I: `extend` rather than `push`
    ///
    /// One very simple, effective and memory efficient solution to this problem is to use [`ConcurrentBag::extend`] rather than `push` in *small data & little work* situations.
    ///
    /// Assume that we will have 4 threads and each will push 1_024 elements.
    /// Instead of making 1_024 `push` calls from each thread, we can make one `extend` call from each.
    /// This would give the best performance.
    /// Further, it has zero buffer or memory cost:
    /// * it is important to note that the batch of 1_024 elements are not stored temporarily in another buffer,
    /// * there is no additional allocation,
    /// * `extend` does nothing more than reserving the position range for the thread by incrementing the atomic counter accordingly.
    ///
    /// However, we do not need to have such a perfect information about the number of elements to be pushed.
    /// Performance gains after reaching the cache line size are much lesser.
    ///
    /// For instance, consider the challenging super small element size case, where we are collecting `i32`s.
    /// We can already achieve a very high performance by simply `extend`ing the bag by batches of 16 elements.
    ///
    /// As the element size gets larger, required batch size to achieve a high performance gets smaller and smaller.
    ///
    /// Required change in the code from `push` to `extend` is not significant.
    /// The example above could be revised as follows to avoid the performance degrading of false sharing.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let (num_threads, num_items_per_thread) = (4, 1_024);
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// // just take a reference and share among threads
    /// let bag_ref = &bag;
    /// let batch_size = 16;
    ///
    /// std::thread::scope(|s| {
    ///     for i in 0..num_threads {
    ///         s.spawn(move || {
    ///             for j in (0..num_items_per_thread).step_by(batch_size) {
    ///                 let iter = (j..(j + batch_size)).map(|j| i * 1000 + j);
    ///                 // concurrently collect results simply by calling `extend`
    ///                 bag_ref.extend(iter);
    ///             }
    ///         });
    ///     }
    /// });
    ///
    /// let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
    /// vec_from_bag.sort();
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// ## Solution-II: Padding
    ///
    /// Another common approach to deal with false sharing is to add padding (unused bytes) between elements.
    /// There exist wrappers which automatically adds cache padding, such as crossbeam's [`CachePadded`](https://docs.rs/crossbeam-utils/latest/crossbeam_utils/struct.CachePadded.html).
    /// In other words, instead of using a `ConcurrentBag<T>`, we can use `ConcurrentBag<CachePadded<T>>`.
    /// However, this solution leads to increased memory requirement.
    pub fn push(&self, value: T) -> usize {
        self.push_and_fill::<Lazy>(value)
    }

    /// Core and generic version of the [`ConcurrentBag::push`] method.
    ///
    /// When an element is pushed to the bag, one of the two cases might happen:
    /// * we have capacity, and hence, we directly write,
    /// * we do not have capacity,
    ///   * we grow the underlying storage,
    ///   * and then we write the value.
    ///
    /// Since length of the concurrent bag is atomically and separately controlled, we are not required to fill or initialize the out-of-bounds memory when we grow.
    /// This is the case with the regular `push` and `extend` methods.
    ///
    /// However, in some situations, we might want to initialize memory of the out-of-bounds positions, such as the following:
    /// * we will use `unsafe get` or `unsafe iter` method,
    /// * we want avoid the possibility of undefined behavior due to reading uninitialized memory.
    ///
    /// We can achieve this by replacing our
    /// * `push(value)` calls with `push_and_fill::<EagerWithDefault>(value)`, and
    /// * `extend(values)` calls with `extend_and_fill::<_, _, EagerWithDefault>(values)` calls, and
    /// * `extend_n_items(values, len)` calls with `extend_n_items::<_, EagerWithDefault>(values, len)` calls.
    ///
    /// This will make sure that we will read the default value of the type in the possible case of race conditions we might experience by using the unsafe methods such as `get` or `iter`.
    ///
    /// # Panics
    ///
    /// Panics if `num_items` elements do not fit in the concurrent bag's maximum capacity.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`ConcurrentBag::maximum_capacity`] for details.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// bag.push('a');
    /// bag.push_and_fill::<Lazy>('b'); // equivalent to `push`
    ///
    /// // this will initialize out-of-bounds memory with `char::default()`
    /// // only if this call requires allocation
    /// bag.push_and_fill::<EagerWithDefault>('c');
    ///
    /// assert_eq!(bag.into_inner(), &['a', 'b', 'c'])
    /// ```
    pub fn push_and_fill<Fill: FillStrategy<T, P>>(&self, value: T) -> usize {
        let idx = self.len.fetch_add(1, Ordering::AcqRel);
        self.assert_has_capacity_for(idx);

        loop {
            let capacity = self.capacity.load(Ordering::Relaxed);

            match idx.cmp(&capacity) {
                // no need to grow, just write
                std::cmp::Ordering::Less => {
                    self.write(idx, value);
                    break;
                }

                // we are responsible for growth
                std::cmp::Ordering::Equal => {
                    let new_capacity = self.grow_to(capacity + 1);
                    Fill::fill_new_memory(self, idx, new_capacity);
                    self.capacity.store(new_capacity, Ordering::Relaxed);
                    self.write(idx, value);
                    break;
                }

                // spin to wait for responsible thread to handle growth
                std::cmp::Ordering::Greater => {}
            }
        }

        idx
    }

    /// Concurrent, thread-safe method to push all `values` that the given iterator will yield to the back of the bag.
    /// The method returns the position or index of the first pushed value (returns the length of the concurrent bag if the iterator is empty).
    ///
    /// All `values` in the iterator will be added to the bag consecutively:
    /// * the first yielded value will be written to the position which is equal to the current length of the bag, say `begin_idx`, which is the returned value,
    /// * the second yielded value will be written to the `begin_idx + 1`-th position,
    /// * ...
    /// * and the last value will be written to the `begin_idx + values.count() - 1`-th position of the bag.
    ///
    /// Important notes:
    /// * This method does not allocate at all to buffer elements to be pushed.
    /// * All it does is to increment the atomic counter by the length of the iterator (`push` would increment by 1) and reserve the range of positions for this operation.
    /// * Iterating over and writing elements to the bag happens afterwards.
    /// * This is a simple, effective and memory efficient solution to the false sharing problem which could be observed in *small data & little work* situations.
    ///
    /// For this reason, the method requires an `ExactSizeIterator`.
    /// There exists the variant [`ConcurrentBag::extend_n_items`] method which accepts any iterator together with the correct length to be passed by the caller, hence it is `unsafe`.
    ///
    /// # Panics
    ///
    /// Panics if not all of the `values` fit in the concurrent bag's maximum capacity.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`ConcurrentBag::maximum_capacity`] for details.
    ///
    /// # Examples
    ///
    /// We can directly take a shared reference of the bag and share it among threads.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let (num_threads, num_items_per_thread) = (4, 1_024);
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// // just take a reference and share among threads
    /// let bag_ref = &bag;
    /// let batch_size = 16;
    ///
    /// std::thread::scope(|s| {
    ///     for i in 0..num_threads {
    ///         s.spawn(move || {
    ///             for j in (0..num_items_per_thread).step_by(batch_size) {
    ///                 let iter = (j..(j + batch_size)).map(|j| i * 1000 + j);
    ///                 // concurrently collect results simply by calling `extend`
    ///                 bag_ref.extend(iter);
    ///             }
    ///         });
    ///     }
    /// });
    ///
    /// let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
    /// vec_from_bag.sort();
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// # Performance Notes - False Sharing
    ///
    /// [`ConcurrentBag::push`] method is implementation is simple, lock-free and efficient.
    /// However, we need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk.
    /// False sharing might lead to significant performance degradation; fortunately, it is possible to avoid in many cases.
    ///
    /// ## When?
    ///
    /// Performance degradation due to false sharing might be observed when both of the following conditions hold:
    /// * **small data**: data to be pushed is small, the more elements fitting in a cache line the bigger the risk,
    /// * **little work**: multiple threads/cores are pushing to the concurrent bag with high frequency; i.e.,
    ///   * very little or negligible work / time is required in between `push` calls.
    ///
    /// The example above fits this situation.
    /// Each thread only performs one multiplication and addition for computing elements, and the elements to be pushed are very small, just one `usize`.
    ///
    /// ## Why?
    ///
    /// * `ConcurrentBag` assigns unique positions to each value to be pushed. There is no *true* sharing among threads in the position level.
    /// * However, cache lines contain more than one position.
    /// * One thread updating a particular position invalidates the entire cache line on an other thread.
    /// * Threads end up frequently reloading cache lines instead of doing the actual work of writing elements to the bag.
    /// * This might lead to a significant performance degradation.
    ///
    /// Following two methods could be approached to deal with this problem.
    ///
    /// ## Solution-I: `extend` rather than `push`
    ///
    /// One very simple, effective and memory efficient solution to the false sharing problem is to use [`ConcurrentBag::extend`] rather than `push` in *small data & little work* situations.
    ///
    /// Assume that we will have 4 threads and each will push 1_024 elements.
    /// Instead of making 1_024 `push` calls from each thread, we can make one `extend` call from each.
    /// This would give the best performance.
    /// Further, it has zero buffer or memory cost:
    /// * it is important to note that the batch of 1_024 elements are not stored temporarily in another buffer,
    /// * there is no additional allocation,
    /// * `extend` does nothing more than reserving the position range for the thread by incrementing the atomic counter accordingly.
    ///
    /// However, we do not need to have such a perfect information about the number of elements to be pushed.
    /// Performance gains after reaching the cache line size are much lesser.
    ///
    /// For instance, consider the challenging super small element size case, where we are collecting `i32`s.
    /// We can already achieve a very high performance by simply `extend`ing the bag by batches of 16 elements.
    ///
    /// As the element size gets larger, required batch size to achieve a high performance gets smaller and smaller.
    ///
    /// The example code above already demonstrates the solution to a potentially problematic case in the [`ConcurrentBag::push`] example.
    ///
    /// ## Solution-II: Padding
    ///
    /// Another common approach to deal with false sharing is to add padding (unused bytes) between elements.
    /// There exist wrappers which automatically adds cache padding, such as crossbeam's [`CachePadded`](https://docs.rs/crossbeam-utils/latest/crossbeam_utils/struct.CachePadded.html).
    /// In other words, instead of using a `ConcurrentBag<T>`, we can use `ConcurrentBag<CachePadded<T>>`.
    /// However, this solution leads to increased memory requirement.
    pub fn extend<IntoIter, Iter>(&self, values: IntoIter) -> usize
    where
        IntoIter: IntoIterator<Item = T, IntoIter = Iter>,
        Iter: Iterator<Item = T> + ExactSizeIterator,
    {
        self.extend_and_fill::<_, _, Lazy>(values)
    }

    /// Core and generic version of the [`ConcurrentBag::extend`] method.
    ///
    /// When an element is pushed to the bag, one of the two cases might happen:
    /// * we have capacity, and hence, we directly write,
    /// * we do not have capacity,
    ///   * we grow the underlying storage,
    ///   * and then we write the value.
    ///
    /// Since length of the concurrent bag is atomically and separately controlled, we are not required to fill or initialize the out-of-bounds memory when we grow.
    /// This is the case with the regular `push` and `extend` methods.
    ///
    /// However, in some situations, we might want to initialize memory of the out-of-bounds positions, such as the following:
    /// * we will use `unsafe get` or `unsafe iter` method,
    /// * we want avoid the possibility of undefined behavior due to reading uninitialized memory.
    ///
    /// We can achieve this by replacing our
    /// * `push(value)` calls with `push_and_fill::<EagerWithDefault>(value)`, and
    /// * `extend(values)` calls with `extend_and_fill::<_, _, EagerWithDefault>(values)` calls, and
    /// * `extend_n_items(values, len)` calls with `extend_n_items::<_, EagerWithDefault>(values, len)` calls.
    ///
    /// This will make sure that we will read the default value of the type in the possible case of race conditions we might experience by using the unsafe methods such as `get` or `iter`.
    ///
    /// # Panics
    ///
    /// Panics if not all of the `values` fit in the concurrent bag's maximum capacity.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`ConcurrentBag::maximum_capacity`] for details.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// bag.extend(['a', 'b']);
    /// bag.extend_and_fill::<_, _, Lazy>(['c', 'd']); // equivalent to `extend`
    ///
    /// // this will initialize out-of-bounds memory with `char::default()`
    /// // only if this call requires allocation
    /// bag.extend_and_fill::<_, _, EagerWithDefault>(['e', 'f']);
    ///
    /// assert_eq!(bag.into_inner(), &['a', 'b', 'c', 'd', 'e', 'f'])
    /// ```
    pub fn extend_and_fill<IntoIter, Iter, Fill>(&self, values: IntoIter) -> usize
    where
        IntoIter: IntoIterator<Item = T, IntoIter = Iter>,
        Iter: Iterator<Item = T> + ExactSizeIterator,
        Fill: FillStrategy<T, P>,
    {
        let values = values.into_iter();
        let num_items = values.len();
        unsafe { self.extend_n_items_and_fill::<_, Fill>(values, num_items) }
    }

    /// Concurrent, thread-safe method to push `num_items` elements yielded by the `values` iterator to the back of the bag.
    /// The method returns the position or index of the first pushed value (returns the length of the concurrent bag if the iterator is empty).
    ///
    /// All `values` in the iterator will be added to the bag consecutively:
    /// * the first yielded value will be written to the position which is equal to the current length of the bag, say `begin_idx`, which is the returned value,
    /// * the second yielded value will be written to the `begin_idx + 1`-th position,
    /// * ...
    /// * and the last value will be written to the `begin_idx + num_items - 1`-th position of the bag.
    ///
    /// Important notes:
    /// * This method does not allocate at all to buffer elements to be pushed.
    /// * All it does is to increment the atomic counter by the length of the iterator (`push` would increment by 1) and reserve the range of positions for this operation.
    /// * Iterating over and writing elements to the bag happens afterwards.
    /// * This is a simple, effective and memory efficient solution to the false sharing problem which could be observed in *small data & little work* situations.
    ///
    /// For this reason, the method requires the additional `num_items` argument.
    /// There exists the variant [`ConcurrentBag::extend`] method which accepts only an `ExactSizeIterator`, hence it is **safe**.
    ///
    /// # Panics
    ///
    /// Panics if `num_items` elements do not fit in the concurrent bag's maximum capacity.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`ConcurrentBag::maximum_capacity`] for details.
    ///
    /// # Safety
    ///
    /// As explained above, extend method calls first increment the atomic counter by `num_items`.
    /// This thread is responsible for filling these reserved `num_items` positions.
    /// * with safe `extend` method, this is guaranteed and safe since the iterator is an `ExactSizeIterator`;
    /// * however, `extend_n_items` accepts any iterator and `num_items` is provided explicitly by the caller.
    ///
    /// Ideally, the `values` iterator must yield exactly `num_items` elements and the caller is responsible for this condition to hold.
    ///
    /// If the `values` iterator is capable of yielding more than `num_items` elements,
    /// the `extend` call will extend the bag with the first `num_items` yielded elements and ignore the rest of the iterator.
    /// This is most likely a bug; however, not an undefined behavior.
    ///
    /// On the other hand, if the `values` iterator is short of `num_items` elements,
    /// this will lead to uninitialized memory positions in underlying storage of the bag which is UB.
    /// Therefore, this method is `unsafe`.
    ///
    /// # Examples
    ///
    /// We can directly take a shared reference of the bag and share it among threads.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let (num_threads, num_items_per_thread) = (4, 1_024);
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// // just take a reference and share among threads
    /// let bag_ref = &bag;
    /// let batch_size = 16;
    ///
    /// std::thread::scope(|s| {
    ///     for i in 0..num_threads {
    ///         s.spawn(move || {
    ///             for j in (0..num_items_per_thread).step_by(batch_size) {
    ///                 let iter = (j..(j + batch_size)).map(|j| i * 1000 + j);
    ///                 // concurrently collect results simply by calling `extend_n_items`
    ///                 unsafe { bag_ref.extend_n_items(iter, batch_size) };
    ///             }
    ///         });
    ///     }
    /// });
    ///
    /// let mut vec_from_bag: Vec<_> = bag.into_inner().iter().copied().collect();
    /// vec_from_bag.sort();
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// # Performance Notes - False Sharing
    ///
    /// [`ConcurrentBag::push`] method is implementation is simple, lock-free and efficient.
    /// However, we need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk.
    /// False sharing might lead to significant performance degradation; fortunately, it is possible to avoid in many cases.
    ///
    /// ## When?
    ///
    /// Performance degradation due to false sharing might be observed when both of the following conditions hold:
    /// * **small data**: data to be pushed is small, the more elements fitting in a cache line the bigger the risk,
    /// * **little work**: multiple threads/cores are pushing to the concurrent bag with high frequency; i.e.,
    ///   * very little or negligible work / time is required in between `push` calls.
    ///
    /// The example above fits this situation.
    /// Each thread only performs one multiplication and addition for computing elements, and the elements to be pushed are very small, just one `usize`.
    ///
    /// ## Why?
    ///
    /// * `ConcurrentBag` assigns unique positions to each value to be pushed. There is no *true* sharing among threads in the position level.
    /// * However, cache lines contain more than one position.
    /// * One thread updating a particular position invalidates the entire cache line on an other thread.
    /// * Threads end up frequently reloading cache lines instead of doing the actual work of writing elements to the bag.
    /// * This might lead to a significant performance degradation.
    ///
    /// Following two methods could be approached to deal with this problem.
    ///
    /// ## Solution-I: `extend` rather than `push`
    ///
    /// One very simple, effective and memory efficient solution to the false sharing problem is to use [`ConcurrentBag::extend`] rather than `push` in *small data & little work* situations.
    ///
    /// Assume that we will have 4 threads and each will push 1_024 elements.
    /// Instead of making 1_024 `push` calls from each thread, we can make one `extend` call from each.
    /// This would give the best performance.
    /// Further, it has zero buffer or memory cost:
    /// * it is important to note that the batch of 1_024 elements are not stored temporarily in another buffer,
    /// * there is no additional allocation,
    /// * `extend` does nothing more than reserving the position range for the thread by incrementing the atomic counter accordingly.
    ///
    /// However, we do not need to have such a perfect information about the number of elements to be pushed.
    /// Performance gains after reaching the cache line size are much lesser.
    ///
    /// For instance, consider the challenging super small element size case, where we are collecting `i32`s.
    /// We can already achieve a very high performance by simply `extend`ing the bag by batches of 16 elements.
    ///
    /// As the element size gets larger, required batch size to achieve a high performance gets smaller and smaller.
    ///
    /// The example code above already demonstrates the solution to a potentially problematic case in the [`ConcurrentBag::push`] example.
    ///
    /// ## Solution-II: Padding
    ///
    /// Another common approach to deal with false sharing is to add padding (unused bytes) between elements.
    /// There exist wrappers which automatically adds cache padding, such as crossbeam's [`CachePadded`](https://docs.rs/crossbeam-utils/latest/crossbeam_utils/struct.CachePadded.html).
    /// In other words, instead of using a `ConcurrentBag<T>`, we can use `ConcurrentBag<CachePadded<T>>`.
    /// However, this solution leads to increased memory requirement.
    pub unsafe fn extend_n_items<IntoIter>(&self, values: IntoIter, num_items: usize) -> usize
    where
        IntoIter: IntoIterator<Item = T>,
    {
        self.extend_n_items_and_fill::<_, Lazy>(values, num_items)
    }

    /// Core and generic version of the [`ConcurrentBag::extend_n_items`] method.
    ///
    /// When an element is pushed to the bag, one of the two cases might happen:
    /// * we have capacity, and hence, we directly write,
    /// * we do not have capacity,
    ///   * we grow the underlying storage,
    ///   * and then we write the value.
    ///
    /// Since length of the concurrent bag is atomically and separately controlled, we are not required to fill or initialize the out-of-bounds memory when we grow.
    /// This is the case with the regular `push` and `extend` methods.
    ///
    /// However, in some situations, we might want to initialize memory of the out-of-bounds positions, such as the following:
    /// * we will use `unsafe get` or `unsafe iter` method,
    /// * we want avoid the possibility of undefined behavior due to reading uninitialized memory.
    ///
    /// We can achieve this by replacing our
    /// * `push(value)` calls with `push_and_fill::<EagerWithDefault>(value)`, and
    /// * `extend(values)` calls with `extend_and_fill::<_, _, EagerWithDefault>(values)` calls, and
    /// * `extend_n_items(values, len)` calls with `extend_n_items::<_, EagerWithDefault>(values, len)` calls.
    ///
    /// This will make sure that we will read the default value of the type in the possible case of race conditions we might experience by using the unsafe methods such as `get` or `iter`.
    ///
    /// # Panics
    ///
    /// Panics if `num_items` elements do not fit in the concurrent bag's maximum capacity.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`ConcurrentBag::maximum_capacity`] for details.
    ///
    /// # Safety
    ///
    /// As explained above, extend method calls first increment the atomic counter by `num_items`.
    /// This thread is responsible for filling these reserved `num_items` positions.
    /// * with safe `extend` method, this is guaranteed and safe since the iterator is an `ExactSizeIterator`;
    /// * however, `extend_n_items` accepts any iterator and `num_items` is provided explicitly by the caller.
    ///
    /// Ideally, the `values` iterator must yield exactly `num_items` elements and the caller is responsible for this condition to hold.
    ///
    /// If the `values` iterator is capable of yielding more than `num_items` elements,
    /// the `extend` call will extend the bag with the first `num_items` yielded elements and ignore the rest of the iterator.
    /// This is most likely a bug; however, not an undefined behavior.
    ///
    /// On the other hand, if the `values` iterator is short of `num_items` elements,
    /// this will lead to uninitialized memory positions in underlying storage of the bag which is UB.
    /// Therefore, this method is `unsafe`.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// unsafe { bag.extend_n_items(['a', 'b'], 2) };
    /// unsafe { bag.extend_n_items_and_fill::<_, Lazy>(['c', 'd'], 2) }; // equivalent to `extend_n_items`
    ///
    /// // this will initialize out-of-bounds memory with `char::default()`
    /// // only if this call requires allocation
    /// unsafe { bag.extend_n_items_and_fill::<_, EagerWithDefault>(['e', 'f'], 2) };
    ///
    /// assert_eq!(bag.into_inner(), &['a', 'b', 'c', 'd', 'e', 'f'])
    /// ```
    pub unsafe fn extend_n_items_and_fill<IntoIter, Fill>(
        &self,
        values: IntoIter,
        num_items: usize,
    ) -> usize
    where
        IntoIter: IntoIterator<Item = T>,
        Fill: FillStrategy<T, P>,
    {
        let values = values.into_iter();

        let beg_idx = self.len.fetch_add(num_items, Ordering::AcqRel);
        let new_len = beg_idx + num_items;
        let end_idx = new_len - 1;

        self.assert_has_capacity_for(end_idx);

        loop {
            let capacity = self.capacity.load(Ordering::Relaxed);

            match (beg_idx.cmp(&capacity), end_idx.cmp(&capacity)) {
                // no need to grow, just write
                (_, std::cmp::Ordering::Less) => {
                    self.write_many(beg_idx, values);
                    break;
                }

                // spin to wait for responsible thread to handle growth
                (std::cmp::Ordering::Greater, _) => {}

                // we are responsible for growth
                _ => {
                    let new_capacity = self.grow_to(new_len);
                    Fill::fill_new_memory(self, beg_idx, new_capacity);
                    self.capacity.store(new_capacity, Ordering::Relaxed);
                    self.write_many(beg_idx, values);
                    break;
                }
            }
        }

        beg_idx
    }

    /// Clears the bag removing all already pushed elements.
    ///
    /// # Safety
    ///
    /// This method requires a mutually exclusive reference.
    /// This guarantees that there might not be any continuing writing process of a `push` operation.
    /// Therefore, the elements can safely be cleared.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let mut bag = ConcurrentBag::new();
    ///
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// bag.clear();
    /// assert!(bag.is_empty());
    /// ```
    pub fn clear(&mut self) {
        let pinned = self.pinned.get_mut();
        pinned.clear();
        let capacity = pinned.capacity();

        self.capacity.store(capacity, Ordering::SeqCst);
        self.len.store(0, Ordering::SeqCst);
    }

    // helpers
    fn new_from_pinned(pinned: P) -> Self {
        Self {
            len: pinned.len().into(),
            capacity: pinned.capacity().into(),
            maximum_capacity: pinned.capacity_state().maximum_concurrent_capacity().into(),
            pinned: pinned.into(),
            phantom: Default::default(),
        }
    }

    /// Asserts that `idx < self.maximum_capacity()`.
    ///
    /// # Safety
    ///
    /// This condition is a safety requirement.
    /// Underlying pinned vector cannot safely grow beyond maximum capacity in a possibly concurrent call (`&self`).
    /// This would lead to UB.
    ///
    /// It is easy to overcome this problem during construction:
    /// * It is not observed with the default pinned vector `SplitVec<_, Doubling>`:
    ///   * `ConcurrentBag::new()`
    ///   * `ConcurrentBag::with_doubling_growth()`
    /// * It can be avoided cheaply when `SplitVec<_, Linear>` is used by setting the second argument to a proper value:
    ///   * `ConcurrentBag::with_linear_growth(10, 32)`
    ///     * Each fragment of this vector will have a capacity of 2^10 = 1024.
    ///     * It can safely grow up to 32 fragments with `&self` reference.
    ///     * Therefore, it is safe to concurrently grow this vector to 32 * 1024 elements.
    ///     * Cost of replacing 32 with 64 is negligible in such a scenario (the difference in memory allocation is precisely 32 * 2*usize).
    /// * Using the variant `FixedVec<_>` requires a hard bound and pre-allocation regardless the program is concurrent or not.
    ///   * `ConcurrentBag::with_fixed_capacity(1024)`
    ///     * Unlike the split vector variants, this variant pre-allocates for 1024 elements.
    ///     * And can never grow beyond this value.
    ///
    /// Alternatively, caller can try to safely reserve the required capacity any time by a mutually exclusive reference:
    /// * `ConcurrentBag.reserve_maximum_capacity(&mut self, maximum_capacity: usize)`
    ///   * this call will always succeed with `SplitVec<_, Doubling>` and `SplitVec<_, Linear>`,
    ///   * will always fail for `FixedVec<_>`.
    #[inline]
    fn assert_has_capacity_for(&self, idx: usize) {
        assert!(
            idx < self.maximum_capacity(),
            "{}",
            ERR_REACHED_MAX_CAPACITY
        );
    }

    #[inline]
    fn try_grow_to(&self, new_capacity: usize) -> Result<usize, PinnedVecGrowthError> {
        let pinned = unsafe { &mut *self.pinned.get() };
        unsafe { pinned.grow_to(new_capacity) }
    }

    #[inline]
    fn grow_to(&self, new_capacity: usize) -> usize {
        self.try_grow_to(new_capacity).expect(ERR_FAILED_TO_GROW)
    }

    #[inline]
    pub(crate) fn write(&self, idx: usize, value: T) {
        let pinned = unsafe { &mut *self.pinned.get() };
        let ptr = unsafe { pinned.get_ptr_mut(idx) }.expect(ERR_FAILED_TO_PUSH);
        unsafe { *ptr = value };
    }

    fn write_many<Iter: Iterator<Item = T>>(&self, beg_idx: usize, values: Iter) {
        for (i, value) in values.enumerate() {
            self.write(beg_idx + i, value);
        }
    }

    pub(crate) unsafe fn correct_pinned_len(&self) {
        let pinned = unsafe { &mut *self.pinned.get() };
        unsafe { pinned.set_len(self.len()) };
    }
}

unsafe impl<T: Sync, P: PinnedVec<T>> Sync for ConcurrentBag<T, P> {}

unsafe impl<T: Send, P: PinnedVec<T>> Send for ConcurrentBag<T, P> {}

#[cfg(test)]
mod tests {
    use crate::EagerWithDefault;

    use super::*;
    use orx_split_vec::Recursive;
    use std::{
        collections::HashSet,
        sync::{Arc, Mutex},
        time::Duration,
    };

    #[test]
    fn new_len_empty_clear() {
        fn test<P: PinnedVec<char>>(bag: ConcurrentBag<char, P>) {
            let mut bag = bag;

            assert!(bag.is_empty());
            assert_eq!(0, bag.len());

            bag.push('a');

            assert!(!bag.is_empty());
            assert_eq!(1, bag.len());

            bag.push('b');
            bag.push('c');
            bag.push('d');

            assert!(!bag.is_empty());
            assert_eq!(4, bag.len());

            bag.clear();
            assert!(bag.is_empty());
            assert_eq!(0, bag.len());
        }

        test(ConcurrentBag::new());
        test(ConcurrentBag::default());
        test(ConcurrentBag::with_doubling_growth());
        test(ConcurrentBag::with_linear_growth(2, 8));
        test(ConcurrentBag::with_linear_growth(4, 8));
        test(ConcurrentBag::with_fixed_capacity(64));
    }

    #[test]
    fn capacity() {
        let mut split: SplitVec<_, Doubling> = (0..4).collect();
        split.concurrent_reserve(5);
        let bag: ConcurrentBag<_, _> = split.into();
        assert_eq!(bag.capacity(), 4);
        bag.push(42);
        assert_eq!(bag.capacity(), 12);

        let mut split: SplitVec<_, Recursive> = (0..4).collect();
        split.concurrent_reserve(5);
        let bag: ConcurrentBag<_, _> = split.into();
        assert_eq!(bag.capacity(), 4);
        bag.push(42);
        assert_eq!(bag.capacity(), 12);

        let mut split: SplitVec<_, Linear> = SplitVec::with_linear_growth(2);
        split.extend_from_slice(&[0, 1, 2, 3]);
        split.concurrent_reserve(5);
        let bag: ConcurrentBag<_, _> = split.into();
        assert_eq!(bag.capacity(), 4);
        bag.push(42);
        assert_eq!(bag.capacity(), 8);

        let mut fixed: FixedVec<_> = FixedVec::new(5);
        fixed.extend_from_slice(&[0, 1, 2, 3]);
        let bag: ConcurrentBag<_, _> = fixed.into();
        assert_eq!(bag.capacity(), 5);
        bag.push(42);
        assert_eq!(bag.capacity(), 5);
    }

    #[test]
    #[should_panic]
    fn exceeding_fixed_capacity_panics() {
        let mut fixed: FixedVec<_> = FixedVec::new(5);
        fixed.extend_from_slice(&[0, 1, 2, 3]);
        let bag: ConcurrentBag<_, _> = fixed.into();
        assert_eq!(bag.capacity(), 5);
        bag.push(42);
        bag.push(7);
    }

    #[test]
    #[should_panic]
    fn exceeding_fixed_capacity_panics_concurrently() {
        let bag = ConcurrentBag::with_fixed_capacity(10);
        let bag_ref = &bag;
        std::thread::scope(|s| {
            for _ in 0..4 {
                s.spawn(move || {
                    for _ in 0..3 {
                        // in total there will be 4*3 = 12 pushes
                        bag_ref.push(42);
                    }
                });
            }
        });
    }

    #[test]
    fn debug() {
        let bag = ConcurrentBag::new();

        bag.push('a');
        bag.push('b');
        bag.push('c');
        bag.push('d');
        bag.push('e');

        let str = format!("{:?}", bag);
        assert_eq!(
            str,
            "ConcurrentBag { pinned: ['a', 'b', 'c', 'd', 'e'], len: 5, capacity: 12 }"
        );
    }

    #[test]
    fn get() {
        // record measurements in (assume) random intervals
        let measurements = ConcurrentBag::<i32>::new();
        let rf_measurements = &measurements;

        // collect sum of measurements every 50 milliseconds
        let sums = ConcurrentBag::new();
        let rf_sums = &sums;

        // collect average of measurements every 50 milliseconds
        let averages = ConcurrentBag::new();
        let rf_averages = &averages;

        std::thread::scope(|s| {
            s.spawn(move || {
                for i in 0..100 {
                    std::thread::sleep(Duration::from_millis(i % 5));
                    rf_measurements.push(i as i32);
                }
            });

            s.spawn(move || {
                for _ in 0..10 {
                    let mut sum = 0;
                    for i in 0..rf_measurements.len() {
                        sum += unsafe { rf_measurements.get(i) }.copied().unwrap_or(0);
                    }
                    rf_sums.push(sum);
                    std::thread::sleep(Duration::from_millis(10));
                }
            });

            s.spawn(move || {
                for _ in 0..10 {
                    let count = rf_measurements.len();
                    if count == 0 {
                        rf_averages.push(0.0);
                    } else {
                        let mut sum = 0;
                        for i in 0..rf_measurements.len() {
                            sum += unsafe { rf_measurements.get(i) }.copied().unwrap_or(0);
                        }
                        let average = sum as f32 / count as f32;
                        rf_averages.push(average);
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
            });
        });

        assert_eq!(measurements.len(), 100);
        assert_eq!(sums.len(), 10);
        assert_eq!(averages.len(), 10);
    }

    #[test]
    fn iter() {
        let mut bag = ConcurrentBag::new();

        assert_eq!(0, unsafe { bag.iter() }.count());

        bag.push('a');

        assert_eq!(
            vec!['a'],
            unsafe { bag.iter() }.copied().collect::<Vec<_>>()
        );

        bag.push('b');
        bag.push('c');
        bag.push('d');

        assert_eq!(
            vec!['a', 'b', 'c', 'd'],
            unsafe { bag.iter() }.copied().collect::<Vec<_>>()
        );

        bag.clear();
        assert_eq!(0, unsafe { bag.iter() }.count());
    }

    #[test]
    fn into_inner_from() {
        let bag = ConcurrentBag::new();

        bag.push('a');
        bag.push('b');
        bag.push('c');
        bag.push('d');
        assert_eq!(
            vec!['a', 'b', 'c', 'd'],
            unsafe { bag.iter() }.copied().collect::<Vec<_>>()
        );

        let mut split = bag.into_inner();
        assert_eq!(
            vec!['a', 'b', 'c', 'd'],
            split.iter().copied().collect::<Vec<_>>()
        );

        split.push('e');
        *split.get_mut(0).expect("exists") = 'x';

        assert_eq!(
            vec!['x', 'b', 'c', 'd', 'e'],
            split.iter().copied().collect::<Vec<_>>()
        );

        let mut bag: ConcurrentBag<_> = split.into();
        assert_eq!(
            vec!['x', 'b', 'c', 'd', 'e'],
            unsafe { bag.iter() }.copied().collect::<Vec<_>>()
        );

        bag.clear();
        assert!(bag.is_empty());

        let split = bag.into_inner();
        assert!(split.is_empty());
    }

    #[test]
    fn ok_at_num_threads() {
        use std::thread::available_parallelism;
        let default_parallelism_approx = available_parallelism().expect("is-ok").get();
        dbg!(default_parallelism_approx);

        let num_threads = default_parallelism_approx;
        let num_items_per_thread = 16384;

        let bag = ConcurrentBag::new();
        let bag_ref = &bag;
        std::thread::scope(|s| {
            for i in 0..num_threads {
                s.spawn(move || {
                    for j in 0..num_items_per_thread {
                        bag_ref.push((i * 100000 + j) as i32);
                    }
                });
            }
        });

        let pinned = bag.into_inner();
        assert_eq!(pinned.len(), num_threads * num_items_per_thread);
    }

    #[test]
    fn push_indices() {
        let num_threads = 4;
        let num_items_per_thread = 16;

        let indices_set = Arc::new(Mutex::new(HashSet::new()));

        let bag = ConcurrentBag::new();
        let bag_ref = &bag;
        std::thread::scope(|s| {
            for i in 0..num_threads {
                let indices_set = indices_set.clone();
                s.spawn(move || {
                    for j in 0..num_items_per_thread {
                        let idx = if j % 3 == 0 {
                            bag_ref.push(i * 100000 + j)
                        } else if j % 3 == 1 {
                            bag_ref.push_and_fill::<EagerWithDefault>(i * 100000 + j)
                        } else {
                            bag_ref.push_and_fill::<Lazy>(i * 100000 + j)
                        };
                        let mut set = indices_set.lock().expect("is ok");
                        set.insert(idx);
                    }
                });
            }
        });

        let set = indices_set.lock().expect("is ok");
        assert_eq!(set.len(), 4 * 16);
        for i in 0..(4 * 16) {
            assert!(set.contains(&i));
        }
    }

    #[test]
    fn reserve_maximum_capacity() {
        // SplitVec<_, Doubling>
        let bag: ConcurrentBag<char> = ConcurrentBag::new();
        assert_eq!(bag.capacity(), 4); // only allocates the first fragment of 4
        assert_eq!(bag.maximum_capacity(), 17_179_869_180); // it can grow safely & exponentially

        let bag: ConcurrentBag<char, _> = ConcurrentBag::with_doubling_growth();
        assert_eq!(bag.capacity(), 4);
        assert_eq!(bag.maximum_capacity(), 17_179_869_180);

        // SplitVec<_, Linear>
        let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_linear_growth(10, 20);
        assert_eq!(bag.capacity(), 2usize.pow(10)); // only allocates first fragment of 1024
        assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 20); // it can concurrently allocate 19 more

        // SplitVec<_, Linear> -> reserve_maximum_capacity
        let result = bag.reserve_maximum_capacity(2usize.pow(10) * 30);
        assert_eq!(result, Ok(2usize.pow(10) * 30));

        // actually no new allocation yet; precisely additional memory for 10 pairs of pointers is used
        assert_eq!(bag.capacity(), 2usize.pow(10)); // first fragment capacity

        dbg!(bag.maximum_capacity(), 2usize.pow(10) * 30);
        assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 30); // now it can safely reach 2^10 * 30

        // FixedVec<_>: pre-allocated, exact and strict
        let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_fixed_capacity(42);
        assert_eq!(bag.capacity(), 42);
        assert_eq!(bag.maximum_capacity(), 42);

        let result = bag.reserve_maximum_capacity(43);
        assert_eq!(
            result,
            Err(PinnedVecGrowthError::FailedToGrowWhileKeepingElementsPinned)
        );
    }

    #[test]
    fn extend_indices() {
        let num_threads = 4;
        let num_items_per_thread = 16;

        let indices_set = Arc::new(Mutex::new(HashSet::new()));

        let bag = ConcurrentBag::new();
        let bag_ref = &bag;
        std::thread::scope(|s| {
            for i in 0..num_threads {
                let indices_set = indices_set.clone();
                s.spawn(move || {
                    let iter = (0..num_items_per_thread).map(|j| i * 100000 + j);

                    let begin_idx = if i % 3 == 0 {
                        bag_ref.extend(iter)
                    } else if i % 3 == 1 {
                        bag_ref.extend_and_fill::<_, _, EagerWithDefault>(iter)
                    } else {
                        bag_ref.extend_and_fill::<_, _, Lazy>(iter)
                    };

                    let mut set = indices_set.lock().expect("is ok");
                    set.insert(begin_idx);
                });
            }
        });

        let set = indices_set.lock().expect("is ok");
        assert_eq!(set.len(), num_threads);
        for i in 0..num_threads {
            assert!(set.contains(&(i * num_items_per_thread)));
        }
    }

    #[test]
    fn extend_n_items_indices() {
        let num_threads = 4;
        let num_items_per_thread = 16;

        let indices_set = Arc::new(Mutex::new(HashSet::new()));

        let bag = ConcurrentBag::new();
        let bag_ref = &bag;
        std::thread::scope(|s| {
            for i in 0..num_threads {
                let indices_set = indices_set.clone();
                s.spawn(move || {
                    let iter = (0..num_items_per_thread).map(|j| i * 100000 + j);

                    let begin_idx = unsafe {
                        if i % 3 == 0 {
                            bag_ref.extend_n_items(iter, num_items_per_thread)
                        } else if i % 3 == 1 {
                            bag_ref.extend_n_items_and_fill::<_, EagerWithDefault>(
                                iter,
                                num_items_per_thread,
                            )
                        } else {
                            bag_ref.extend_n_items_and_fill::<_, Lazy>(iter, num_items_per_thread)
                        }
                    };

                    let mut set = indices_set.lock().expect("is ok");
                    set.insert(begin_idx);
                });
            }
        });

        let set = indices_set.lock().expect("is ok");
        assert_eq!(set.len(), num_threads);
        for i in 0..num_threads {
            assert!(set.contains(&(i * num_items_per_thread)));
        }
    }
}
