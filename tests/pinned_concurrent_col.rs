use orx_concurrent_bag::*;
use orx_pinned_vec::IntoConcurrentPinnedVec;
use test_case::test_matrix;

#[test]
fn capacity() {
    let mut split: SplitVec<_, Doubling> = (0..4).collect();
    split.concurrent_reserve(5).expect("is-ok");
    let bag: ConcurrentBag<_, _> = split.into();
    assert_eq!(bag.capacity(), 4);
    bag.push(42);
    assert_eq!(bag.capacity(), 12);

    let mut split: SplitVec<_, Linear> = SplitVec::with_linear_growth(2);
    split.extend_from_slice(&[0, 1, 2, 3]);
    split.concurrent_reserve(5).expect("is-ok");
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

#[test_matrix([
    FixedVec::new(5),
    SplitVec::with_doubling_growth_and_fragments_capacity(1),
    SplitVec::with_linear_growth_and_fragments_capacity(2, 1)
])]
#[should_panic]
fn exceeding_fixed_capacity_panics<P: IntoConcurrentPinnedVec<usize>>(mut pinned_vec: P) {
    pinned_vec.clear();
    pinned_vec.extend_from_slice(&[0, 1, 2, 3]);
    let bag: ConcurrentBag<_, _> = pinned_vec.into();
    assert_eq!(bag.capacity(), 5);
    bag.push(42);
    bag.push(7);
}

#[test_matrix([
    FixedVec::new(10),
    SplitVec::with_doubling_growth_and_fragments_capacity(2),
    SplitVec::with_linear_growth_and_fragments_capacity(2, 3)
])]
#[should_panic]
fn exceeding_fixed_capacity_panics_concurrently<P: IntoConcurrentPinnedVec<usize>>(pinned_vec: P) {
    let bag: ConcurrentBag<_, _> = pinned_vec.into();
    let bag_ref = &bag;
    std::thread::scope(|s| {
        for _ in 0..4 {
            s.spawn(move || {
                for _ in 0..4 {
                    // in total there will be 4*3 = 12 pushes
                    bag_ref.push(42);
                }
            });
        }
    });
}

#[test_matrix([
    FixedVec::new(10),
    SplitVec::with_doubling_growth_and_fragments_capacity(2),
    SplitVec::with_linear_growth_and_fragments_capacity(2, 3)
])]
fn get_iter<P: IntoConcurrentPinnedVec<char>>(pinned_vec: P) {
    let mut bag: ConcurrentBag<_, _> = pinned_vec.into();

    assert_eq!(0, unsafe { bag.iter() }.count());
    assert_eq!(None, unsafe { bag.get(0) });

    bag.push('a');

    assert_eq!(
        vec!['a'],
        unsafe { bag.iter() }.copied().collect::<Vec<_>>()
    );
    assert_eq!(Some(&'a'), unsafe { bag.get(0) });
    assert_eq!(None, unsafe { bag.get(1) });

    bag.push('b');
    bag.push('c');
    bag.push('d');
    bag.push('e');

    assert_eq!(
        vec!['a', 'b', 'c', 'd', 'e'],
        unsafe { bag.iter() }.copied().collect::<Vec<_>>()
    );
    assert_eq!(Some(&'d'), unsafe { bag.get(3) });
    assert_eq!(None, unsafe { bag.get(5) });

    bag.clear();
    assert_eq!(0, unsafe { bag.iter() }.count());
}

#[test_matrix([
    FixedVec::new(10),
    SplitVec::with_doubling_growth_and_fragments_capacity(2),
    SplitVec::with_linear_growth_and_fragments_capacity(2, 3)
])]
fn get_mut<P: IntoConcurrentPinnedVec<String>>(pinned_vec: P) {
    let mut bag: ConcurrentBag<_, _> = pinned_vec.into();

    assert_eq!(None, bag.get_mut(0));

    bag.push("a".to_string());
    bag.push("b".to_string());
    bag.push("c".to_string());
    bag.push("d".to_string());
    bag.push("e".to_string());

    assert_eq!(None, bag.get_mut(5));
    assert_eq!(Some("c"), bag.get_mut(2).map(|x| x.as_str()));
    *bag.get_mut(2).unwrap() = "c!".to_string();

    let vec: Vec<_> = unsafe { bag.iter() }.collect();
    assert_eq!(vec, ["a", "b", "c!", "d", "e"]);
}

#[test_matrix([
    FixedVec::new(10),
    SplitVec::with_doubling_growth_and_fragments_capacity(2),
    SplitVec::with_linear_growth_and_fragments_capacity(2, 3),
])]
fn iter_mut<P: IntoConcurrentPinnedVec<String>>(pinned_vec: P) {
    let mut bag: ConcurrentBag<_, _> = pinned_vec.into();

    assert_eq!(0, bag.iter_mut().count());

    bag.push("a".to_string());

    assert_eq!(Some("a"), bag.iter_mut().next().map(|x| x.as_str()));

    bag.push("b".to_string());
    bag.push("c".to_string());
    bag.push("d".to_string());
    bag.push("e".to_string());

    for x in bag.iter_mut().filter(|x| x.as_str() != "c") {
        *x = format!("{}!", x);
    }

    let vec: Vec<_> = unsafe { bag.iter() }.collect();
    assert_eq!(vec, ["a!", "b!", "c", "d!", "e!"]);
}

#[test]
fn reserve_maximum_capacity() {
    // SplitVec<_, Doubling>
    let bag: ConcurrentBag<char> = ConcurrentBag::new();
    assert_eq!(bag.capacity(), 4); // only allocates the first fragment of 4

    #[cfg(target_pointer_width = "64")]
    assert_eq!(bag.maximum_capacity(), 17_179_869_180); // it can grow safely & exponentially
    #[cfg(target_pointer_width = "32")]
    assert_eq!(bag.maximum_capacity(), 2_147_483_644); // it can grow safely & exponentially

    let bag: ConcurrentBag<char, _> = ConcurrentBag::with_doubling_growth();
    assert_eq!(bag.capacity(), 4);

    #[cfg(target_pointer_width = "64")]
    assert_eq!(bag.maximum_capacity(), 17_179_869_180);
    #[cfg(target_pointer_width = "32")]
    assert_eq!(bag.maximum_capacity(), 2_147_483_644);

    // SplitVec<_, Linear>
    let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_linear_growth(10, 20);
    assert_eq!(bag.capacity(), 2usize.pow(10)); // only allocates first fragment of 1024
    assert_eq!(bag.maximum_capacity(), 2usize.pow(10) * 20); // it can concurrently allocate 19 more

    // SplitVec<_, Linear> -> reserve_maximum_capacity
    let new_max_capacity = bag.reserve_maximum_capacity(2usize.pow(10) * 30);
    assert!(new_max_capacity >= 2usize.pow(10) * 30);

    // actually no new allocation yet; precisely additional memory for 10 pairs of pointers is used
    assert_eq!(bag.capacity(), 2usize.pow(10)); // first fragment capacity

    assert!(bag.maximum_capacity() >= 2usize.pow(10) * 30); // now it can safely reach 2^10 * 30

    // FixedVec<_>
    let mut bag: ConcurrentBag<char, _> = ConcurrentBag::with_fixed_capacity(42);
    assert_eq!(bag.capacity(), 42);
    assert_eq!(bag.maximum_capacity(), 42);

    let new_max_capacity = bag.reserve_maximum_capacity(1024);
    assert!(new_max_capacity >= 1024);
}
