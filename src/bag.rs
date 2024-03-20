use orx_fixed_vec::FixedVec;
use orx_split_vec::{Doubling, Linear, PinnedVec, Recursive, SplitVec};
use std::marker::PhantomData;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An efficient, convenient and lightweight grow-only concurrent collection, ideal for collecting results concurrently.
///
/// The bag preserves the order of elements with respect to the order the `push` method is called.
///
/// # Examples
///
/// Safety guarantees to push to the bag with an immutable reference makes it easy to share the bag among threads.
///
/// ## Using `std::sync::Arc`
///
/// We can share our bag among threads using `Arc` and collect results concurrently.
///
/// ```rust
/// use orx_concurrent_bag::*;
/// use std::{sync::Arc, thread};
///
/// let (num_threads, num_items_per_thread) = (4, 8);
///
/// let bag = Arc::new(ConcurrentBag::new());
/// let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();
///
/// for i in 0..num_threads {
///     let bag = bag.clone();
///     thread_vec.push(thread::spawn(move || {
///         for j in 0..num_items_per_thread {
///             // concurrently collect results simply by calling `push`
///             bag.push(i * 1000 + j);
///         }
///     }));
/// }
///
/// for handle in thread_vec {
///     handle.join().unwrap();
/// }
///
/// let mut vec_from_bag: Vec<_> = unsafe { bag.iter() }.copied().collect();
/// vec_from_bag.sort();
/// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
/// expected.sort();
/// assert_eq!(vec_from_bag, expected);
/// ```
///
/// ## Using `std::thread::scope`
///
/// An even more convenient approach would be to use thread scopes. This allows to use shared reference of the bag across threads, instead of `Arc`.
///
/// ```rust
/// use orx_concurrent_bag::*;
///
/// let (num_threads, num_items_per_thread) = (4, 8);
///
/// let bag = ConcurrentBag::new();
/// let bag_ref = &bag; // just take a reference
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
/// # Safety
///
/// `ConcurrentBag` uses a [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) implementation as the underlying storage (see [`SplitVec`](https://crates.io/crates/orx-split-vec) and [`Fixed`](https://crates.io/crates/orx-fixed-vec)).
/// `PinnedVec` guarantees that elements which are already pushed to the vector stay pinned to their memory locations unless explicitly changed due to removals, which is not the case here since `ConcurrentBag` is a grow-only collection.
/// This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).
///
/// In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `PinnedVec` with an `AtomicUsize`.
/// * `len: AtomicSize`: fixes the target memory location of each element to be pushed at the time the `push` method is called. Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
/// * `PinnedVec` guarantees that already pushed elements are not moved around in memory during growth. This also enables the following mode of concurrency:
///   * one thread might allocate new memory in order to grow when capacity is reached,
///   * while another thread might concurrently be writing to any of the already allocation memory locations.
///
/// The approach guarantees that
/// * only one thread can write to the memory location of an element being pushed to the bag,
/// * at any point in time, only one thread is responsible for the allocation of memory if the bag requires new memory,
/// * no thread reads any of the written elements (reading happens after converting the bag `into_inner`),
/// * hence, there exists no race condition.
///
/// # Construction
///
/// As explained above, `ConcurrentBag` is simply a tuple of a `PinnedVec` and an `AtomicUsize`.
/// Therefore, it can be constructed by wrapping any pinned vector; i.e., `ConcurrentBag<T>` implements `From<P: PinnedVec<T>>`.
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
/// // SplitVec with [Recursive](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Recursive.html) growth
/// let bag: ConcurrentBag<char, SplitVec<char, Recursive>> =
///     ConcurrentBag::with_recursive_growth();
/// let bag: ConcurrentBag<char, SplitVec<char, Recursive>> =
///     SplitVec::with_recursive_growth().into();
///
/// // SplitVec with [Linear](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html) growth
/// // each fragment will have capacity 2^10 = 1024
/// let bag: ConcurrentBag<char, SplitVec<char, Linear>> = ConcurrentBag::with_linear_growth(10);
/// let bag: ConcurrentBag<char, SplitVec<char, Linear>> = SplitVec::with_linear_growth(10).into();
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
/// # Write-Only vs Read-Write
///
/// The concurrent bag is write-only & grow-only bag which is convenient and efficient for collecting elements.
///
/// See [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec) for a read-and-write variant which
/// * guarantees that reading and writing never happen concurrently, and hence,
/// * allows safe iteration or access to already written elements of the concurrent vector,
/// * with a minor additional cost of values being wrapped by an `Option`.
pub struct ConcurrentBag<T, P = SplitVec<T, Doubling>>
where
    P: PinnedVec<T>,
{
    pinned: P,
    len: AtomicUsize,
    capacity: usize,
    phantom: PhantomData<T>,
}

// new
impl<T> ConcurrentBag<T, SplitVec<T, Doubling>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Doubling>` as the underlying storage.
    pub fn with_doubling_growth() -> Self {
        Self::new_from_pinned(SplitVec::with_doubling_growth())
    }

    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Doubling>` as the underlying storage.
    pub fn new() -> Self {
        Self::new_from_pinned(SplitVec::new())
    }
}

impl<T> Default for ConcurrentBag<T, SplitVec<T, Doubling>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Doubling>` as the underlying storage.
    fn default() -> Self {
        Self::with_doubling_growth()
    }
}

impl<T> ConcurrentBag<T, SplitVec<T, Recursive>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Recursive>` as the underlying storage.
    pub fn with_recursive_growth() -> Self {
        Self::new_from_pinned(SplitVec::with_recursive_growth())
    }
}

impl<T> ConcurrentBag<T, SplitVec<T, Linear>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `SplitVec<T, Linear>` as the underlying storage.
    ///
    /// Note that choosing a small `constant_fragment_capacity_exponent` for a large bag to be filled might lead to too many growth calls which might be computationally costly.
    pub fn with_linear_growth(constant_fragment_capacity_exponent: usize) -> Self {
        Self::new_from_pinned(SplitVec::with_linear_growth(
            constant_fragment_capacity_exponent,
        ))
    }
}

impl<T> ConcurrentBag<T, FixedVec<T>> {
    /// Creates a new concurrent vector by creating and wrapping up a new `FixedVec<T>` as the underlying storage.
    ///
    /// # Safety
    ///
    /// Note that a `FixedVec` cannot grow.
    /// Therefore, pushing the `(fixed_capacity + 1)`-th element to the bag will lead to a panic.
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
        let (len, mut pinned) = (self.len(), self.pinned);
        unsafe { pinned.set_len(len) };
        pinned
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
        self.capacity
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
        self.pinned.iter().take(self.len())
    }

    /// Concurrent & thread-safe method to push the given `value` to the back of the bag.
    ///
    /// It preserves the order of elements with respect to the order the `push` method is called.
    ///
    /// # Examples
    ///
    /// Allowing to safely push to the bag with an immutable reference, it is trivial to share the bag among threads.
    ///
    /// ## Using `std::sync::Arc`
    ///
    /// We can share our bag among threads using `Arc` and collect results concurrently.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use std::{sync::Arc, thread};
    ///
    /// let (num_threads, num_items_per_thread) = (4, 8);
    ///
    /// let bag = Arc::new(ConcurrentBag::new());
    /// let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();
    ///
    /// for i in 0..num_threads {
    ///     let bag = bag.clone();
    ///     thread_vec.push(thread::spawn(move || {
    ///         for j in 0..num_items_per_thread {
    ///             // concurrently collect results simply by calling `push`
    ///             bag.push(i * 1000 + j);
    ///         }
    ///     }));
    /// }
    ///
    /// for handle in thread_vec {
    ///     handle.join().unwrap();
    /// }
    ///
    /// let mut vec_from_bag: Vec<_> = unsafe { bag.iter() }.copied().collect();
    /// vec_from_bag.sort();
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// ## Using `std::thread::scope`
    ///
    /// An even more convenient approach would be to use thread scopes. This allows to use shared reference of the bag across threads, instead of `Arc`.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use std::thread;
    ///
    /// let (num_threads, num_items_per_thread) = (4, 8);
    ///
    /// let bag = ConcurrentBag::new();
    /// let bag_ref = &bag; // just take a reference
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
    /// # Safety
    ///
    /// `ConcurrentBag` uses a [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) implementation as the underlying storage (see [`SplitVec`](https://crates.io/crates/orx-split-vec) and [`Fixed`](https://crates.io/crates/orx-fixed-vec)).
    /// `PinnedVec` guarantees that elements which are already pushed to the vector stay pinned to their memory locations unless explicitly changed due to removals, which is not the case here since `ConcurrentBag` is a grow-only collection.
    /// This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).
    ///
    /// In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `PinnedVec` with an `AtomicUsize`.
    /// * `len: AtomicSize`: fixes the target memory location of each element to be pushed at the time the `push` method is called. Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
    /// * `PinnedVec` guarantees that already pushed elements are not moved around in memory during growth. This also enables the following mode of concurrency:
    ///   * one thread might allocate new memory in order to grow when capacity is reached,
    ///   * while another thread might concurrently be writing to any of the already allocation memory locations.
    ///
    /// The approach guarantees that
    /// * only one thread can write to the memory location of an element being pushed to the bag,
    /// * at any point in time, only one thread is responsible for the allocation of memory if the bag requires new memory,
    /// * no thread reads any of the written elements (reading happens after converting the bag `into_inner`),
    /// * hence, there exists no race condition.
    ///
    /// # Panics
    ///
    /// Panics if the underlying pinned vector fails to grow.
    /// * Note that `FixedVec` cannot grow beyond its fixed capacity;
    /// * `SplitVec`, on the other hand, can grow without dynamically.
    pub fn push(&self, value: T) {
        #[allow(invalid_reference_casting)]
        unsafe fn into_mut<'a, T>(reference: &T) -> &'a mut T {
            &mut *(reference as *const T as *mut T)
        }

        let col = std::hint::black_box(unsafe { into_mut(self) });
        let idx = self.len.fetch_add(1, Ordering::Relaxed);

        loop {
            let capacity = std::hint::black_box(col.capacity);

            match idx.cmp(&capacity) {
                std::cmp::Ordering::Less => {
                    let ptr =
                        unsafe { col.pinned.get_ptr_mut(idx) }.expect("failed to push element");
                    unsafe { *ptr = value };
                    break;
                }
                std::cmp::Ordering::Equal => {
                    col.capacity >>= 2;
                    let new_capacity = col
                        .pinned
                        .try_grow()
                        .expect("failed to grow the collection");

                    unsafe { col.pinned.set_len(new_capacity) };

                    let ptr =
                        unsafe { col.pinned.get_ptr_mut(idx) }.expect("failed to push element");
                    unsafe { *ptr = value };

                    col.capacity = new_capacity;

                    break;
                }
                std::cmp::Ordering::Greater => {}
            }
        }
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
        self.pinned.clear();
        self.len.store(0, Ordering::SeqCst);
        self.capacity = self.pinned.capacity();
    }

    // helpers
    fn new_from_pinned(pinned: P) -> Self {
        let len = pinned.len().into();
        let capacity = pinned.capacity();
        let mut bag = Self {
            pinned,
            len,
            capacity,
            phantom: Default::default(),
        };
        unsafe { bag.set_pinned_len_to_capacity() };
        bag
    }

    unsafe fn set_pinned_len_to_capacity(&mut self) {
        unsafe { self.pinned.set_len(self.pinned.capacity()) }
    }
}

unsafe impl<T, P: PinnedVec<T>> Sync for ConcurrentBag<T, P> {}

unsafe impl<T, P: PinnedVec<T>> Send for ConcurrentBag<T, P> {}

#[cfg(test)]
mod tests {
    use super::*;

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
        test(ConcurrentBag::with_recursive_growth());
        test(ConcurrentBag::with_linear_growth(2));
        test(ConcurrentBag::with_linear_growth(4));
        test(ConcurrentBag::with_fixed_capacity(64));
    }

    #[test]
    fn capacity() {
        let split: SplitVec<_, Doubling> = (0..4).collect();
        let bag: ConcurrentBag<_, _> = split.into();
        assert_eq!(bag.capacity(), 4);
        bag.push(42);
        assert_eq!(bag.capacity(), 12);

        let split: SplitVec<_, Recursive> = (0..4).collect();
        let bag: ConcurrentBag<_, _> = split.into();
        assert_eq!(bag.capacity(), 4);
        bag.push(42);
        assert_eq!(bag.capacity(), 12);

        let mut split: SplitVec<_, Linear> = SplitVec::with_linear_growth(2);
        split.extend_from_slice(&[0, 1, 2, 3]);
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
}
