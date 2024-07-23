use orx_concurrent_bag::*;
use orx_pinned_vec::IntoConcurrentPinnedVec;
use test_case::test_matrix;

const NUM_RERUNS: usize = 1;

#[test_matrix(
    [
        FixedVec::new(100000),
        SplitVec::with_doubling_growth_and_fragments_capacity(32),
        SplitVec::with_linear_growth_and_fragments_capacity(10, 64),
    ],
    [124, 348, 1024, 2587]
)]
fn dropped_as_bag<P: IntoConcurrentPinnedVec<String> + Clone>(pinned_vec: P, len: usize) {
    for _ in 0..NUM_RERUNS {
        let num_threads = 4;
        let num_items_per_thread = len / num_threads;

        let bag = fill_bag(pinned_vec.clone(), len);

        assert_eq!(bag.len(), num_threads * num_items_per_thread);
    }
}

#[test_matrix(
    [
        FixedVec::new(100000),
        SplitVec::with_doubling_growth_and_fragments_capacity(32),
        SplitVec::with_linear_growth_and_fragments_capacity(10, 64),
    ],
    [124, 348, 1024, 2587]
)]
fn dropped_after_into_inner<P: IntoConcurrentPinnedVec<String> + Clone>(pinned_vec: P, len: usize) {
    for _ in 0..NUM_RERUNS {
        let num_threads = 4;
        let num_items_per_thread = len / num_threads;

        let bag = fill_bag(pinned_vec.clone(), len);

        let inner = bag.into_inner();
        assert_eq!(inner.len(), num_threads * num_items_per_thread);
    }
}

fn fill_bag<P: IntoConcurrentPinnedVec<String>>(
    pinned_vec: P,
    len: usize,
) -> ConcurrentBag<String, P> {
    let num_threads = 4;
    let num_items_per_thread = len / num_threads;

    let bag: ConcurrentBag<_, _> = pinned_vec.into();
    let con_bag = &bag;
    std::thread::scope(move |s| {
        for _ in 0..num_threads {
            s.spawn(move || {
                for value in 0..num_items_per_thread {
                    let new_value = format!("from-thread-{}", value);
                    con_bag.push(new_value);
                }
            });
        }
    });

    bag
}
