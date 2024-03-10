use orx_split_vec::prelude::PinnedVec;
use orx_split_vec::{Doubling, Fragment, GrowthWithConstantTimeAccess, Linear, SplitVec};
use std::{cmp::Ordering, fmt::Debug, sync::atomic::AtomicUsize};

const ORDERING: core::sync::atomic::Ordering = core::sync::atomic::Ordering::Relaxed;

/// A thread-safe collection allowing growth with immutable reference, making it ideal for collecting results concurrently.
///
/// It preserves the order of elements with respect to the order the `push` method is called.
///
/// # Examples
///
/// Safety guarantees to push to the bag with an immutable reference makes it easy to share the bag among threads.
///
/// ## Using `std::sync::Arc`
///
/// Following the common approach of using an `Arc`, we can share our bag among threads and collect results concurrently.
///
/// ```rust
/// use orx_concurrent_bag::*;
/// use std::{sync::Arc, thread};
///
/// let (num_threads, num_items_per_thread) = (4, 8);
///
/// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
/// expected.sort();
///
/// let bag = Arc::new(ConcurrentBag::new());
/// let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();
///
/// for i in 0..num_threads {
///     let bag = bag.clone();
///     thread_vec.push(thread::spawn(move || {
///         for j in 0..num_items_per_thread {
///             bag.push(i * 1000 + j); // concurrently collect results simply by calling `push`
///         }
///     }));
/// }
///
/// for handle in thread_vec {
///     handle.join().unwrap();
/// }
///
/// let mut vec_from_bag: Vec<_> = bag.iter().copied().collect();
/// vec_from_bag.sort();
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
/// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
/// expected.sort();
///
/// let bag = ConcurrentBag::new();
/// let bag_ref = &bag; // just take a reference
/// std::thread::scope(|s| {
///     for i in 0..num_threads {
///         s.spawn(move || {
///             for j in 0..num_items_per_thread {
///                 bag_ref.push(i * 1000 + j); // concurrently collect results simply by calling `push`
///             }
///         });
///     }
/// });
///
/// let mut vec_from_bag: Vec<_> = bag.iter().copied().collect();
/// vec_from_bag.sort();
/// assert_eq!(vec_from_bag, expected);
/// ```
///
/// # Safety
///
/// `ConcurrentBag` uses a [`SplitVec`](https://crates.io/crates/orx-split-vec) as the underlying storage.
/// `SplitVec` implements [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) which guarantees that elements which are already pushed to the vector stay pinned to their memory locations.
/// This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).
///
/// In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `SplitVec` with an `AtomicUsize`.
/// * * `AtomicUsize` fixes the target memory location of each element to be pushed at the time the `push` method is called. Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
/// * `SplitVec` guarantees that already pushed elements are not moved around in memory and new elements are written to the reserved position.
///
/// This pair allows a lightweight and convenient concurrent bag which is ideal for collecting results concurrently.
#[derive(Debug)]
pub struct ConcurrentBag<T, G = Doubling>
where
    G: GrowthWithConstantTimeAccess,
{
    split: SplitVec<T, G>,
    len: AtomicUsize,
}

unsafe impl<T, G: GrowthWithConstantTimeAccess> Sync for ConcurrentBag<T, G> {}

unsafe impl<T, G: GrowthWithConstantTimeAccess> Send for ConcurrentBag<T, G> {}

impl<T> ConcurrentBag<T, Doubling> {
    /// Creates a new empty concurrent bag.
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
    /// assert_eq!(vec!['a', 'b'], bag.iter().copied().collect::<Vec<_>>());
    /// ```
    pub fn new() -> Self {
        Self::with_doubling_growth()
    }

    /// Creates a new empty concurrent bag with doubling growth strategy.
    ///
    /// Each fragment of the underlying split vector will have a capacity which is double the capacity of the prior fragment.
    ///
    /// More information about doubling strategy can be found here [`orx_split_vec::Doubling`](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Doubling.html).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::with_doubling_growth(); // fragments will have capacities 4, 8, 16, etc.
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// assert_eq!(vec!['a', 'b'], bag.iter().copied().collect::<Vec<_>>());
    /// ```
    #[allow(clippy::new_without_default)]
    pub fn with_doubling_growth() -> Self {
        let mut vec = SplitVec::new();
        let first_fragment = unsafe { vec.fragments_mut().get_unchecked_mut(0) };
        Self::set_len(first_fragment);
        Self {
            split: vec,
            len: AtomicUsize::new(0),
        }
    }
}

impl<T> Default for ConcurrentBag<T, Doubling> {
    /// Creates a new empty concurrent bag.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::default();
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// assert_eq!(vec!['a', 'b'], bag.iter().copied().collect::<Vec<_>>());
    /// ```
    fn default() -> Self {
        Self::new()
    }
}

impl<T> ConcurrentBag<T, Linear> {
    /// Creates a new empty concurrent bag with linear growth strategy.
    ///
    /// Each fragment of the underlying split vector will have a capacity of `2 ^ constant_fragment_capacity_exponent`.
    ///
    /// More information about doubling strategy can be found here [`orx_split_vec::Linear`](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html).
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::with_linear_growth(5); // each fragment will have a capacity of 2^5
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// assert_eq!(vec!['a', 'b'], bag.iter().copied().collect::<Vec<_>>());
    /// ```
    pub fn with_linear_growth(constant_fragment_capacity_exponent: usize) -> Self {
        let mut vec = SplitVec::with_linear_growth(constant_fragment_capacity_exponent);
        let first_fragment = unsafe { vec.fragments_mut().get_unchecked_mut(0) };
        Self::set_len(first_fragment);
        Self {
            split: vec,
            len: AtomicUsize::new(0),
        }
    }
}

impl<T, G: GrowthWithConstantTimeAccess> From<SplitVec<T, G>> for ConcurrentBag<T, G> {
    fn from(split: SplitVec<T, G>) -> Self {
        let len = AtomicUsize::new(split.len());
        Self { split, len }
    }
}

impl<T, G: GrowthWithConstantTimeAccess> ConcurrentBag<T, G> {
    /// Consumes the concurrent bag and returns the inner storage, the `SplitVec`.
    ///
    /// Note that
    /// * it is cheap to wrap a `SplitVec` as a `ConcurrentBag` using thee `From` trait;
    /// * and similarly to convert a `ConcurrentBag` to the underlying `SplitVec` using `into_inner` method.
    ///
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::prelude::*;
    ///
    /// let bag = ConcurrentBag::new();
    ///
    /// bag.push('a');
    /// bag.push('b');
    /// bag.push('c');
    /// bag.push('d');
    /// assert_eq!(vec!['a', 'b', 'c', 'd'], bag.iter().copied().collect::<Vec<_>>());
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
    /// assert_eq!(vec!['x', 'b', 'c', 'd', 'e'], bag.iter().copied().collect::<Vec<_>>());
    ///
    /// bag.clear();
    /// assert!(bag.is_empty());
    ///
    /// let split = bag.into_inner();
    /// assert!(split.is_empty());
    pub fn into_inner(self) -> SplitVec<T, G> {
        let (len, mut split) = (self.len(), self.split);
        Self::correct_split_lengths(&mut split, len);
        split
    }

    /// Returns the number of elements which are pushed to the vector, including the elements which received their reserved locations and currently being pushed.
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
        unsafe { self.len.as_ptr().read() }
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
    /// # Examples
    ///
    /// ```rust
    /// use orx_concurrent_bag::ConcurrentBag;
    ///
    /// let bag = ConcurrentBag::new();
    /// bag.push('a');
    /// bag.push('b');
    ///
    /// let mut iter = bag.iter();
    /// assert_eq!(iter.next(), Some(&'a'));
    /// assert_eq!(iter.next(), Some(&'b'));
    /// assert_eq!(iter.next(), None);
    /// ```
    pub fn iter(&self) -> impl Iterator<Item = &T> {
        self.split.iter().take(self.len())
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
    /// Following the common approach of using an `Arc`, we can share our bag among threads and collect results concurrently.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use std::{sync::Arc, thread};
    ///
    /// let (num_threads, num_items_per_thread) = (4, 8);
    ///
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    ///
    /// let bag = Arc::new(ConcurrentBag::new());
    /// let mut thread_vec: Vec<thread::JoinHandle<()>> = Vec::new();
    ///
    /// for i in 0..num_threads {
    ///     let bag = bag.clone();
    ///     thread_vec.push(thread::spawn(move || {
    ///         for j in 0..num_items_per_thread {
    ///             bag.push(i * 1000 + j); // concurrently collect results simply by calling `push`
    ///         }
    ///     }));
    /// }
    ///
    /// for handle in thread_vec {
    ///     handle.join().unwrap();
    /// }
    ///
    /// let mut vec_from_bag: Vec<_> = bag.iter().copied().collect();
    /// vec_from_bag.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// ## Using `std::thread::scope`
    ///
    /// An even more convenient approach would be to use thread scopes.
    /// This allows to use shared reference to the bag directly, instead of `Arc`.
    ///
    /// ```rust
    /// use orx_concurrent_bag::*;
    /// use std::thread;
    ///
    /// let (num_threads, num_items_per_thread) = (4, 8);
    ///
    /// let mut expected: Vec<_> = (0..num_threads).flat_map(|i| (0..num_items_per_thread).map(move |j| i * 1000 + j)).collect();
    /// expected.sort();
    ///
    /// let bag = ConcurrentBag::new();
    /// let bag_ref = &bag; // just take a reference
    /// std::thread::scope(|s| {
    ///     for i in 0..num_threads {
    ///         s.spawn(move || {
    ///             for j in 0..num_items_per_thread {
    ///                 bag_ref.push(i * 1000 + j); // concurrently collect results simply by calling `push`
    ///             }
    ///         });
    ///     }
    /// });
    ///
    /// let mut vec_from_bag: Vec<_> = bag.iter().copied().collect();
    /// vec_from_bag.sort();
    /// assert_eq!(vec_from_bag, expected);
    /// ```
    ///
    /// # Safety
    ///
    /// `ConcurrentBag` uses a [`SplitVec`](https://crates.io/crates/orx-split-vec) as the underlying storage.
    /// `SplitVec` implements [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) which guarantees that elements which are already pushed to the vector stay pinned to their memory locations.
    /// This feature makes it safe to grow with a shared reference on a single thread, as implemented by [`ImpVec`](https://crates.io/crates/orx-imp-vec).
    ///
    /// In order to achieve this feature in a concurrent program, `ConcurrentBag` pairs the `SplitVec` with an `AtomicUsize`.
    /// * `AtomicUsize` fixes the target memory location of each element being pushed at the point the `push` method is called.
    /// Regardless of whether or not writing to memory completes before another element is pushed, every pushed element receives a unique position reserved for it.
    /// * `SplitVec` guarantees that already pushed elements are not moved around in memory and new elements are written to the reserved position.
    ///
    /// This pair allows a lightweight and convenient concurrent bag which is ideal for collecting results concurrently.
    pub fn push(&self, value: T) {
        #[allow(invalid_reference_casting)]
        unsafe fn into_mut<'a, T>(reference: &T) -> &'a mut T {
            &mut *(reference as *const T as *mut T)
        }

        let idx = self.len.fetch_add(1, ORDERING);

        loop {
            let capacity = self.split.capacity();

            match idx.cmp(&capacity) {
                Ordering::Less => {
                    let split = std::hint::black_box(unsafe { into_mut(&self.split) });
                    if let Some(ptr) = unsafe { split.ptr_mut(idx) } {
                        unsafe { *ptr = value };
                        break;
                    }
                }
                Ordering::Equal => {
                    let split = unsafe { into_mut(&self.split) };
                    let next_capacity = split.growth.new_fragment_capacity(split.fragments());
                    let mut fragment = Vec::with_capacity(next_capacity).into();
                    Self::set_len(&mut fragment);
                    fragment[0] = value;
                    let fragments = unsafe { split.fragments_mut() };
                    fragments.push(fragment);
                    break;
                }
                Ordering::Greater => {}
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
        self.len.store(0, ORDERING);
        self.split.clear();
    }

    // helpers
    fn set_len(fragment: &mut Fragment<T>) {
        debug_assert_eq!(0, fragment.len());
        let len = fragment.capacity();
        unsafe { fragment.set_len(len) }
    }

    fn correct_split_lengths(split: &mut SplitVec<T, G>, len: usize) {
        let mut remaining = len;

        let fragments = unsafe { split.fragments_mut() };

        for fragment in fragments {
            let capacity = fragment.capacity();
            if remaining <= capacity {
                unsafe { fragment.set_len(remaining) };
            } else {
                unsafe { fragment.set_len(capacity) };
                remaining -= capacity;
            }
        }

        unsafe { split.set_len(len) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_len_empty_clear() {
        fn test<G: GrowthWithConstantTimeAccess>(bag: ConcurrentBag<char, G>) {
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
        test(ConcurrentBag::with_linear_growth(2));
        test(ConcurrentBag::with_linear_growth(4));
    }

    #[test]
    fn debug() {
        let bag = ConcurrentBag::new();

        bag.push('a');
        bag.push('b');
        bag.push('c');
        bag.push('d');

        let str = format!("{:?}", bag);
        assert_eq!(
            str,
            "ConcurrentBag { split: SplitVec [\n    ['a', 'b', 'c', 'd']\n]\n, len: 4 }"
        );
    }

    #[test]
    fn iter() {
        let mut bag = ConcurrentBag::new();

        assert_eq!(0, bag.iter().count());

        bag.push('a');

        assert_eq!(vec!['a'], bag.iter().copied().collect::<Vec<_>>());

        bag.push('b');
        bag.push('c');
        bag.push('d');

        assert_eq!(
            vec!['a', 'b', 'c', 'd'],
            bag.iter().copied().collect::<Vec<_>>()
        );

        bag.clear();
        assert_eq!(0, bag.iter().count());
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
            bag.iter().copied().collect::<Vec<_>>()
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
            bag.iter().copied().collect::<Vec<_>>()
        );

        bag.clear();
        assert!(bag.is_empty());

        let split = bag.into_inner();
        assert!(split.is_empty());
    }
}
