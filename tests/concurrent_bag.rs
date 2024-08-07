use orx_concurrent_bag::*;
use orx_pinned_vec::IntoConcurrentPinnedVec;
use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};
use test_case::test_matrix;

#[test_matrix([
    FixedVec::new(2132),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 33)
])]
fn into_inner_from<P: IntoConcurrentPinnedVec<char> + Clone>(pinned: P) {
    let elements = vec!['a', 'b', 'c', 'd', 'e'];

    let bag = ConcurrentBag::from(pinned);

    for c in &elements {
        bag.push(*c);
    }

    assert_eq!(
        &elements,
        &unsafe { bag.iter() }.copied().collect::<Vec<_>>()
    );
    for (i, c) in elements.iter().enumerate() {
        assert_eq!(Some(c), unsafe { bag.get(i) });
    }

    let mut pinned = bag.into_inner();
    let vec: Vec<_> = pinned.iter().copied().collect();
    assert_eq!(&elements, &vec);

    pinned.push('f');
    *pinned.get_mut(0).expect("exists") = 'x';

    let elements = vec!['x', 'b', 'c', 'd', 'e', 'f'];

    let vec: Vec<_> = pinned.iter().copied().collect();
    assert_eq!(&elements, &vec);

    let mut bag = ConcurrentBag::from(pinned);
    assert_eq!(
        &elements,
        &unsafe { bag.iter() }.copied().collect::<Vec<_>>()
    );
    for (i, c) in elements.iter().enumerate() {
        assert_eq!(Some(c), unsafe { bag.get(i) });
    }

    bag.clear();
    assert!(bag.is_empty());

    let split = bag.into_inner();
    assert!(split.is_empty());
}

#[test_matrix([
    FixedVec::new(5000),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 33)
])]
fn ok_at_num_threads<P: IntoConcurrentPinnedVec<String> + Clone>(pinned: P) {
    let num_threads = 8;
    let num_items_per_thread = 500;

    let bag = ConcurrentBag::from(pinned);
    let bag_ref = &bag;
    std::thread::scope(|s| {
        for i in 0..num_threads {
            s.spawn(move || {
                for j in 0..num_items_per_thread {
                    bag_ref.push((i * 100000 + j).to_string());
                }
            });
        }
    });

    let pinned = bag.into_inner();
    assert_eq!(pinned.len(), num_threads * num_items_per_thread);
}

#[test_matrix([
    FixedVec::new(333),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 33)
])]
fn push_indices<P: IntoConcurrentPinnedVec<String> + Clone>(pinned: P) {
    let num_threads = 4;
    let num_items_per_thread = 64;

    let indices_set = Arc::new(Mutex::new(HashSet::new()));

    let bag = ConcurrentBag::from(pinned);
    let bag_ref = &bag;
    std::thread::scope(|s| {
        for i in 0..num_threads {
            let indices_set = indices_set.clone();
            s.spawn(move || {
                for j in 0..num_items_per_thread {
                    let idx = bag_ref.push((i * 100000 + j).to_string());
                    let mut set = indices_set.lock().expect("is ok");
                    set.insert(idx);
                }
            });
        }
    });

    let set = indices_set.lock().expect("is ok");
    assert_eq!(set.len(), num_threads * num_items_per_thread);
    for i in 0..(num_threads * num_items_per_thread) {
        assert!(set.contains(&i));
    }
}

#[test_matrix([
    FixedVec::new(733),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 33)
])]
fn extend_indices<P: IntoConcurrentPinnedVec<String> + Clone>(pinned: P) {
    let num_threads = 4;
    let num_items_per_thread = 128;

    let indices_set = Arc::new(Mutex::new(HashSet::new()));

    let bag = ConcurrentBag::from(pinned);
    let bag_ref = &bag;
    std::thread::scope(|s| {
        for i in 0..num_threads {
            let indices_set = indices_set.clone();
            s.spawn(move || {
                let iter = (0..num_items_per_thread).map(|j| (i * 100000 + j).to_string());

                let begin_idx = bag_ref.extend(iter);

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

#[test_matrix([
    FixedVec::new(733),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 33)
])]
fn extend_n_items_indices<P: IntoConcurrentPinnedVec<String> + Clone>(pinned: P) {
    let num_threads = 4;
    let num_items_per_thread = 128;

    let indices_set = Arc::new(Mutex::new(HashSet::new()));

    let bag = ConcurrentBag::from(pinned);
    let bag_ref = &bag;
    std::thread::scope(|s| {
        for i in 0..num_threads {
            let indices_set = indices_set.clone();
            s.spawn(move || {
                let iter = (0..num_items_per_thread).map(|j| (i * 100000 + j).to_string());

                let begin_idx = unsafe { bag_ref.extend_n_items(iter, num_items_per_thread) };

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
