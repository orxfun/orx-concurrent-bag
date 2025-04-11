use crate::state::ConcurrentBagState;
use orx_pinned_concurrent_col::PinnedConcurrentCol;
use orx_pinned_vec::{ConcurrentPinnedVec, IntoConcurrentPinnedVec};
use orx_split_vec::{Doubling, SplitVec};

/// An efficient, convenient and lightweight grow-only concurrent data structure allowing high performance concurrent collection.
///
/// * **convenient**: `ConcurrentBag` can safely be shared among threads simply as a shared reference. It is a [`PinnedConcurrentCol`](https://crates.io/crates/orx-pinned-concurrent-col) with a special concurrent state implementation. Underlying [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) and concurrent bag can be converted back and forth to each other.
/// * **efficient**: `ConcurrentBag` is a lock free structure making use of a few atomic primitives, this leads to high performance concurrent growth. You may see the details in <a href="#section-benchmarks">benchmarks</a> and further <a href="#section-performance-notes">performance notes</a>.
///
/// Note that `ConcurrentBag` is write only (with the safe api), see [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read & write variant.
///
/// # Examples
///
/// Safety guarantees to push to the bag with a shared reference makes it easy to share the bag among threads.
/// `std::sync::Arc` can be used; however, it is not required as demonstrated below.
///
/// ```rust
/// use orx_concurrent_bag::*;
/// use orx_iterable::*;
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
/// ## Construction
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
///
/// # Concurrent State and Properties
///
/// The concurrent state is modeled simply by an atomic length.
/// Combination of this state and `PinnedConcurrentCol` leads to the following properties:
/// * Writing to the collection does not block. Multiple writes can happen concurrently.
/// * Each position is written only and exactly once.
/// * Only one growth can happen at a given time.
/// * Underlying pinned vector can be extracted any time.
/// * Safe reading is only possible after converting the bag into the underlying `PinnedVec`.
///   No read & write race condition exists.
pub struct ConcurrentBag<T, P = SplitVec<T, Doubling>>
where
    P: IntoConcurrentPinnedVec<T>,
{
    core: PinnedConcurrentCol<T, P::ConPinnedVec, ConcurrentBagState>,
}

impl<T, P> ConcurrentBag<T, P>
where
    P: IntoConcurrentPinnedVec<T>,
{
    /// Consumes the concurrent bag and returns the underlying pinned vector.
    ///
    /// Any `PinnedVec` implementation can be converted to a `ConcurrentBag` using the `From` trait.
    /// Similarly, underlying pinned vector can be obtained by calling the consuming `into_inner` method.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use orx_iterable::*;
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
        let len = self.core.state().len();
        // # SAFETY: ConcurrentBag only allows to push to the end of the bag, keeping track of the length.
        // Therefore, the underlying pinned vector is in a valid condition at any given time.
        unsafe { self.core.into_inner(len) }
    }

    /// ***O(1)*** Returns the number of elements which are pushed to the bag, including the elements which received their reserved locations and are currently being pushed.
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
        self.core.state().len()
    }

    /// Returns whether or not the bag is empty.
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
    ///
    /// assert!(!bag.is_empty());
    ///
    /// bag.clear();
    /// assert!(bag.is_empty());
    /// ```
    #[inline(always)]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns a reference to the element at the `index`-th position of the bag.
    /// It returns `None` when index is out of bounds.
    ///
    /// # Safety
    ///
    /// `ConcurrentBag` guarantees that each position is written only and exactly once.
    /// And further, no thread reads this position (see [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a safe read & write variant).
    /// Therefore, there exists no race condition.
    ///
    /// The race condition could be observed in the following unsafe usage.
    /// Say we have a `bag` of `char`s and we allocate memory to store incoming characters, say 4 positions.
    /// If the following events happen in the exact order in time, we might have undefined behavior (UB):
    /// * `bag.push('a')` is called from thread#1.
    /// * `bag` atomically increases the `len` to 1.
    /// * thread#2 calls `bag.get(0)` which is now in bounds.
    /// * thread#2 receives uninitialized value (UB).
    /// * thread#1 completes writing `'a'` to the 0-th position (one moment too late).
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
        match index < self.core.state().written_len() {
            true => unsafe { self.core.get(index) },
            false => None,
        }
    }

    /// Returns a mutable reference to the element at the `index`-th position of the bag.
    /// It returns `None` when index is out of bounds.
    ///
    /// # Safety
    ///
    /// At first it might be confusing that `get` method is unsafe; however, `get_mut` is safe.
    /// This is due to `&mut self` requirement of the `get_mut` method.
    ///
    /// The following paragraph from `get` docs demonstrates an example that could lead to undefined behavior.
    /// The race condition (with `get`) could be observed in the following unsafe usage.
    /// Say we have a `bag` of `char`s and we allocate memory to store incoming characters, say 4 positions.
    /// If the following events happen in the exact order in time, we might have undefined behavior (UB):
    /// * `bag.push('a')` is called from thread#1.
    /// * `bag` atomically increases the `len` to 1.
    /// * thread#2 calls `bag.get(0)` which is now in bounds.
    /// * thread#2 receives uninitialized value (UB).
    /// * thread#1 completes writing `'a'` to the 0-th position (one moment too late).
    ///
    /// This scenario would not compile with `get_mut` requiring a `&mut self`. Therefore, `get_mut` is safe.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    ///
    /// let mut bag = ConcurrentBag::new();
    ///
    /// bag.push('a');
    /// bag.extend(['b', 'c', 'd']);
    ///
    /// assert_eq!(unsafe { bag.get_mut(4) }, None);
    ///
    /// *bag.get_mut(1).unwrap() = 'x';
    /// assert_eq!(unsafe { bag.get(1) }, Some(&'x'));
    /// ```
    pub fn get_mut(&mut self, index: usize) -> Option<&mut T> {
        if index < self.len() {
            unsafe { self.core.get_mut(index) }
        } else {
            None
        }
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
        unsafe { self.core.iter(self.core.state().written_len()) }
    }

    /// Returns an iterator to elements of the bag.
    ///
    /// Iteration of elements is in the order the push method is called.
    ///
    /// # Safety
    ///
    /// At first it might be confusing that `iter` method is unsafe; however, `iter_mut` is safe.
    /// This is due to `&mut self` requirement of the `iter_mut` method.
    ///
    /// The following paragraph from `iter` docs demonstrates an example that could lead to undefined behavior.
    /// The `iter` method is unsafe due to the possibility of the following scenario:
    /// * a thread reserves a position in the bag,
    /// * this increases the length of the bag by one, which includes this new element to the iteration,
    /// * however, before writing the value of the element completes, iterator reaches this element and reads uninitialized value.
    ///
    /// This scenario would not compile with `get_mut` requiring a `&mut self`. Therefore, `get_mut` is safe.
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
    /// let mut bag = ConcurrentBag::new();
    /// bag.push("a".to_string());
    /// bag.push("b".to_string());
    ///
    /// for x in bag.iter_mut() {
    ///     *x = format!("{}!", x);
    /// }
    ///
    /// let mut iter = unsafe { bag.iter() };
    /// assert_eq!(iter.next(), Some(&String::from("a!")));
    /// assert_eq!(iter.next(), Some(&String::from("b!")));
    /// assert_eq!(iter.next(), None);
    /// ```
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut T> {
        unsafe { self.core.iter_mut(self.len()) }
    }

    /// Concurrent, thread-safe method to push the given `value` to the back of the bag, and returns the position or index of the pushed value.
    ///
    /// It preserves the order of elements with respect to the order the `push` method is called.
    ///
    /// # Panics
    ///
    /// Panics if the concurrent bag is already at its maximum capacity; i.e., if `self.len() == self.maximum_capacity()`.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`PinnedConcurrentCol::maximum_capacity`] for details.
    ///
    /// # Examples
    ///
    /// We can directly take a shared reference of the bag, share it among threads and collect results concurrently.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use orx_iterable::*;
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
    /// [`ConcurrentBag::push`] implementation is lock-free and focuses on efficiency.
    /// However, we need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk.
    /// False sharing might lead to significant performance degradation.
    /// However, it is possible to avoid in many cases.
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
    /// use orx_iterable::*;
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
    /// Another approach to deal with false sharing is to add padding (unused bytes) between elements.
    /// There exist wrappers which automatically adds cache padding, such as crossbeam's [`CachePadded`](https://docs.rs/crossbeam-utils/latest/crossbeam_utils/struct.CachePadded.html).
    /// In other words, instead of using a `ConcurrentBag<T>`, we can use `ConcurrentBag<CachePadded<T>>`.
    /// However, this solution leads to increased memory requirement.
    pub fn push(&self, value: T) -> usize {
        let idx = self.core.state().fetch_increment_len(1);
        // # SAFETY: ConcurrentBag ensures that each `idx` will be written only and exactly once.
        unsafe { self.core.write(idx, value) };
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
    /// * This method does not allocate to buffer.
    /// * All it does is to increment the atomic counter by the length of the iterator (`push` would increment by 1) and reserve the range of positions for this operation.
    /// * If there is not sufficient space, the vector grows first; iterating over and writing elements to the bag happens afterwards.
    /// * Therefore, other threads do not wait for the `extend` method to complete, they can concurrently write.
    /// * This is a simple and effective approach to deal with the false sharing problem which could be observed in *small data & little work* situations.
    ///
    /// For this reason, the method requires an `ExactSizeIterator`.
    /// There exists the variant [`ConcurrentBag::extend_n_items`] method which accepts any iterator together with the correct length to be passed by the caller.
    /// It is `unsafe` as the caller must guarantee that the iterator yields at least the number of elements explicitly passed in as an argument.
    ///
    /// # Panics
    ///
    /// Panics if not all of the `values` fit in the concurrent bag's maximum capacity.
    ///
    /// Note that this is an important safety assertion in the concurrent context; however, not a practical limitation.
    /// Please see the [`PinnedConcurrentCol::maximum_capacity`] for details.
    ///
    /// # Examples
    ///
    /// We can directly take a shared reference of the bag and share it among threads.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use orx_iterable::*;
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
        let values = values.into_iter();
        let num_items = values.len();
        // # SAFETY: ConcurrentBag ensures that each `idx` will be written only and exactly once.
        unsafe { self.extend_n_items::<_>(values, num_items) }
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
    /// Please see the [`PinnedConcurrentCol::maximum_capacity`] for details.
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
    /// use orx_iterable::*;
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
        let begin_idx = self.core.state().fetch_increment_len(num_items);
        unsafe { self.core.write_n_items(begin_idx, num_items, values) };
        begin_idx
    }

    /// Reserves and returns an iterator of mutable slices for `num_items` positions starting from the `begin_idx`-th position.
    ///
    /// The caller is responsible for filling all `num_items` positions in the returned iterator of slices with values to avoid gaps.
    ///
    /// # Safety
    ///
    /// This method makes sure that the values are written to positions owned by the underlying pinned vector.
    /// Furthermore, it makes sure that the growth of the vector happens thread-safely whenever necessary.
    ///
    /// On the other hand, it is unsafe due to the possibility of a race condition.
    /// Multiple threads can try to write to the same position at the same time.
    /// The wrapper is responsible for preventing this.
    ///
    /// Furthermore, the caller is responsible to write all positions of the acquired slices to make sure that the collection is gap free.
    ///
    /// Note that although both methods are unsafe, it is much easier to achieve required safety guarantees with `extend` or `extend_n_items`;
    /// hence, they must be preferred unless there is a good reason to acquire mutable slices.
    /// One such example case is to copy results directly into the output's slices, which could be more performant in a very critical scenario.
    pub unsafe fn n_items_buffer_as_mut_slices(
        &self,
        num_items: usize,
    ) -> (
        usize,
        <P::ConPinnedVec as ConcurrentPinnedVec<T>>::SliceMutIter<'_>,
    ) {
        let begin_idx = self.core.state().fetch_increment_len(num_items);
        (begin_idx, unsafe {
            self.core.n_items_buffer_as_mut_slices(begin_idx, num_items)
        })
    }

    /// Clears the concurrent bag.
    pub fn clear(&mut self) {
        unsafe { self.core.clear(self.core.state().len()) };
    }

    /// Note that [`ConcurrentBag::maximum_capacity`] returns the maximum possible number of elements that the underlying pinned vector can grow to without reserving maximum capacity.
    ///
    /// In other words, the pinned vector can automatically grow up to the [`ConcurrentBag::maximum_capacity`] with `write` and `write_n_items` methods, using only a shared reference.
    ///
    /// When required, this maximum capacity can be attempted to increase by this method with a mutable reference.
    ///
    /// Importantly note that maximum capacity does not correspond to the allocated memory.
    ///
    /// Among the common pinned vector implementations:
    /// * `SplitVec<_, Doubling>`: supports this method; however, it does not require for any practical size.
    /// * `SplitVec<_, Linear>`: is guaranteed to succeed and increase its maximum capacity to the required value.
    /// * `FixedVec<_>`: is the most strict pinned vector which cannot grow even in a single-threaded setting. Currently, it will always return an error to this call.
    ///
    /// # Safety
    /// This method is unsafe since the concurrent pinned vector might contain gaps. The vector must be gap-free while increasing the maximum capacity.
    ///
    /// This method can safely be called if entries in all positions 0..len are written.
    pub fn reserve_maximum_capacity(&mut self, new_maximum_capacity: usize) -> usize {
        unsafe {
            self.core
                .reserve_maximum_capacity(self.core.state().written_len(), new_maximum_capacity)
        }
    }

    /// Returns the current allocated capacity of the collection.
    pub fn capacity(&self) -> usize {
        self.core.capacity()
    }

    /// Returns maximum possible capacity that the collection can reach without calling [`ConcurrentBag::reserve_maximum_capacity`].
    ///
    /// Importantly note that maximum capacity does not correspond to the allocated memory.
    pub fn maximum_capacity(&self) -> usize {
        self.core.maximum_capacity()
    }
}

// HELPERS

impl<T, P> ConcurrentBag<T, P>
where
    P: IntoConcurrentPinnedVec<T>,
{
    pub(crate) fn new_from_pinned(pinned_vec: P) -> Self {
        let core = PinnedConcurrentCol::new_from_pinned(pinned_vec);
        Self { core }
    }

    #[inline]
    pub(crate) fn core(&self) -> &PinnedConcurrentCol<T, P::ConPinnedVec, ConcurrentBagState> {
        &self.core
    }
}

unsafe impl<T: Sync, P: IntoConcurrentPinnedVec<T>> Sync for ConcurrentBag<T, P> {}

unsafe impl<T: Send, P: IntoConcurrentPinnedVec<T>> Send for ConcurrentBag<T, P> {}
