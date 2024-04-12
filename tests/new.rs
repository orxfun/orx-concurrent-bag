use orx_concurrent_bag::*;

#[test]
fn new_len_empty_clear() {
    fn test<P: PinnedVec<char>>(bag: ConcurrentBag<char, P>) {
        assert!(!bag.zeroes_memory_on_allocation());

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
