use crate::ConcurrentBag;
use orx_fixed_vec::PinnedVec;

mod sealed {
    pub trait FillStrategy {}
}

/// Strategy defining how out-of-bounds positions of the concurrent bag will be filled.
pub trait FillStrategy<T, P>: sealed::FillStrategy
where
    P: PinnedVec<T>,
{
    /// Fills the `bag` positions between `begin_idx` and `new_capacity` with the defined strategy.
    fn fill_new_memory(bag: &ConcurrentBag<T, P>, begin_idx: usize, new_capacity: usize);
}

/// Lazy strategy does not fill the positions between `len()` and `capacity`.
/// Note that this is perfectly safe for `ConcurrentBag`.
///
/// However, if the caller decides to use the unsafe read methods such as `get` and `iter`,
/// using `EagerWithDefault` completely prevents the possibility of an undefined behavior due to reading an uninitialized memory.
pub struct Lazy;

impl sealed::FillStrategy for Lazy {}

impl<T, P: PinnedVec<T>> FillStrategy<T, P> for Lazy {
    fn fill_new_memory(_: &ConcurrentBag<T, P>, _: usize, _: usize) {}
}

// default
/// EagerWithDefault strategy fills the positions between `len()` and `capacity` with the default value of the element type.
/// Note that this is an additional safety guarantee which is required only if unsafe read methods of the `ConcurrentBag, such as `get` and `iter`, are to be used.
///
/// Filling with defaults completely prevents the possibility of an undefined behavior due to reading an uninitialized memory.
///
/// As expected, the tradeoff is initialization of allocation memory before pushing the elements to it.
pub struct EagerWithDefault;

impl sealed::FillStrategy for EagerWithDefault {}

impl<T: Default, P: PinnedVec<T>> FillStrategy<T, P> for EagerWithDefault {
    fn fill_new_memory(bag: &ConcurrentBag<T, P>, begin_idx: usize, new_capacity: usize) {
        for i in begin_idx..new_capacity {
            bag.write(i, T::default());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_case::test_case;

    fn validate<P: PinnedVec<char>>(bag: ConcurrentBag<char, P>) {
        assert!(bag.capacity() > 4);

        unsafe {
            assert_eq!(bag.get(4), Some(&'x'));

            for i in 5..bag.capacity() + 10 {
                assert_eq!(bag.get(i), None);
            }
        }

        let mut pinned = bag.into_inner();
        assert_eq!(5, pinned.len());
        assert!(pinned.capacity() > 4);

        unsafe {
            let ptr = pinned.get_ptr_mut(4).expect("in-capacity");
            assert_eq!(*ptr, 'x');

            for i in 5..pinned.capacity() {
                let ptr = pinned.get_ptr_mut(i).expect("in-capacity");
                assert_eq!(*ptr, char::default());
            }

            for i in pinned.capacity()..(pinned.capacity() + 10) {
                let ptr = pinned.get_ptr_mut(i);
                assert_eq!(ptr, None);
            }
        }
    }

    #[test_case(ConcurrentBag::with_linear_growth(2, 64))]
    #[test_case(ConcurrentBag::with_doubling_growth())]
    fn eager_push_and_fill<P: PinnedVec<char>>(bag: ConcurrentBag<char, P>) {
        bag.push_and_fill::<EagerWithDefault>('a');
        bag.push_and_fill::<EagerWithDefault>('b');
        bag.push_and_fill::<EagerWithDefault>('c');
        bag.push_and_fill::<EagerWithDefault>('d');

        assert_eq!(4, bag.capacity());

        let pinned = bag.into_inner();
        assert_eq!(
            pinned.iter().copied().collect::<Vec<_>>(),
            ['a', 'b', 'c', 'd']
        );

        let bag: ConcurrentBag<_, _> = pinned.into();
        bag.push_and_fill::<EagerWithDefault>('x');

        validate(bag);
    }

    #[test_case(ConcurrentBag::with_linear_growth(2, 64))]
    #[test_case(ConcurrentBag::with_doubling_growth())]
    fn eager_extend_and_fill<P: PinnedVec<char>>(bag: ConcurrentBag<char, P>) {
        bag.extend_and_fill::<_, _, EagerWithDefault>(['a', 'b', 'c']);

        assert_eq!(4, bag.capacity());

        let pinned = bag.into_inner();
        assert_eq!(pinned.iter().copied().collect::<Vec<_>>(), ['a', 'b', 'c']);

        let bag: ConcurrentBag<_, _> = pinned.into();
        bag.extend_and_fill::<_, _, EagerWithDefault>(['d', 'x']);

        validate(bag);
    }

    #[test_case(ConcurrentBag::with_linear_growth(2, 64))]
    #[test_case(ConcurrentBag::with_doubling_growth())]
    fn eager_extend_n_items_and_fill<P: PinnedVec<char>>(bag: ConcurrentBag<char, P>) {
        unsafe { bag.extend_n_items_and_fill::<_, EagerWithDefault>(['a', 'b', 'c'], 3) };

        assert_eq!(4, bag.capacity());

        let pinned = bag.into_inner();
        assert_eq!(pinned.iter().copied().collect::<Vec<_>>(), ['a', 'b', 'c']);

        let bag: ConcurrentBag<_, _> = pinned.into();
        unsafe { bag.extend_n_items_and_fill::<_, EagerWithDefault>(['d', 'x'], 2) };

        validate(bag);
    }
}
