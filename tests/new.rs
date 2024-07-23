use orx_concurrent_bag::*;
use orx_pinned_vec::IntoConcurrentPinnedVec;

#[test]
fn new_len_empty_clear() {
    fn test<P: IntoConcurrentPinnedVec<char>>(bag: ConcurrentBag<char, P>) {
        let mut bag = bag;

        assert!(bag.is_empty());
        assert_eq!(0, bag.len());

        bag.push('a');

        assert!(!bag.is_empty());
        assert_eq!(1, bag.len());

        bag.push('b');
        bag.push('c');
        bag.push('d');
        bag.push('e');

        assert!(!bag.is_empty());
        assert_eq!(5, bag.len());

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
