use orx_concurrent_bag::ConcurrentBag;
use orx_fixed_vec::FixedVec;
use orx_pinned_vec::IntoConcurrentPinnedVec;
use orx_split_vec::SplitVec;
use test_case::test_matrix;

#[test_matrix([
    FixedVec::new(100),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 16)
], [
    FixedVec::new(100),
    SplitVec::with_doubling_growth_and_fragments_capacity(16),
    SplitVec::with_linear_growth_and_fragments_capacity(10, 16)
])]
fn concurrent_get_and_iter<P, Q>(pinned_i32: P, pinned_f32: Q)
where
    P: IntoConcurrentPinnedVec<i32> + Clone,
    Q: IntoConcurrentPinnedVec<f32>,
{
    // record measurements in (assume) random intervals
    let measurements: ConcurrentBag<_, _> = pinned_i32.clone().into();
    let rf_measurements = &measurements;

    // collect sum of measurements every 50 milliseconds
    let sums: ConcurrentBag<_, _> = pinned_i32.into();
    let rf_sums = &sums;

    // collect average of measurements every 50 milliseconds
    let averages: ConcurrentBag<_, _> = pinned_f32.into();
    let rf_averages = &averages;

    std::thread::scope(|s| {
        s.spawn(move || {
            for i in 0..100 {
                std::thread::sleep(std::time::Duration::from_millis(i % 5));
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
                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });

        s.spawn(move || {
            for _ in 0..10 {
                // better to have count & add as an atomic reduction for correctness
                // calling .len() and .iter().sum() separately might possibly give a false average
                fn count_and_add(len_and_sum: (usize, i32), x: &i32) -> (usize, i32) {
                    (len_and_sum.0 + 1, len_and_sum.1 + x)
                }

                let (len, sum) = unsafe { rf_measurements.iter() }.fold((0, 0), count_and_add);
                let average = sum as f32 / len as f32;
                rf_averages.push(average);

                std::thread::sleep(std::time::Duration::from_millis(50));
            }
        });
    });

    assert_eq!(measurements.len(), 100);
    assert_eq!(sums.len(), 10);
    assert_eq!(averages.len(), 10);
}
