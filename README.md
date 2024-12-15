# orx-concurrent-bag

[![orx-concurrent-bag crate](https://img.shields.io/crates/v/orx-concurrent-bag.svg)](https://crates.io/crates/orx-concurrent-bag)
[![orx-concurrent-bag documentation](https://docs.rs/orx-concurrent-bag/badge.svg)](https://docs.rs/orx-concurrent-bag)

An efficient, convenient and lightweight grow-only concurrent data structure allowing high performance concurrent collection.

A [`ConcurrentBag`](https://docs.rs/orx-concurrent-bag/latest/orx_concurrent_bag/struct.ConcurrentBag.html) can safely be shared among threads simply as a shared reference. It is a lock free structure enabling efficient and copy free concurrent collection of elements.
* It is a [`PinnedConcurrentCol`](https://crates.io/crates/orx-pinned-concurrent-col)  with a concurrent state definition specialized for efficient growth.
* It is built upon the pinned element guarantees of the underlying [`PinnedVec`](https://crates.io/crates/orx-pinned-vec) storage.

*You may see <a href="#section-benchmarks">benchmarks</a> and further <a href="#section-performance-notes">performance notes</a> for details.*

## Examples

The example below demonstrate sharing references of concurrent bag across multiple threads and simply pushing elements to it concurrently.

Although returned value is not used in this example, [`push`](https://docs.rs/orx-concurrent-bag/latest/orx_concurrent_bag/struct.ConcurrentBag.html#method.push) returns the index or position of the pushed element. Note that this information is not otherwise trivially available in a concurrent execution.

 ```rust
use orx_concurrent_bag::*;

let bag = ConcurrentBag::new();

let (num_threads, num_items_per_thread) = (4, 1024);

let bag_ref = &bag;
std::thread::scope(|s| {
    for i in 0..num_threads {
        s.spawn(move || {
            for j in 0..num_items_per_thread {
                // concurrently collect results simply by calling `push`
                bag_ref.push(i * 1024 + j);
            }
        });
    }
});

let mut vec = bag.into_inner();
vec.sort();

let expected: Vec<_> = (0..num_threads * num_items_per_thread).collect();

assert_eq!(vec, expected);
```

## Concurrent State and Properties

The concurrent state is modeled simply by an atomic length. Combination of this state and PinnedConcurrentCol leads to the following properties:
* Writing to a position of the collection does not block other writes, multiple writes can happen concurrently.
* Each position is written exactly once.
* **⟹ no write & write race condition exists**
* Only one growth can happen at a given time. Growth is copy-free and does not change memory locations of already pushed elements.
* Underlying pinned vector is always valid and can be taken out any time by [`into_inner`](https://docs.rs/orx-concurrent-bag/latest/orx_concurrent_bag/struct.ConcurrentBag.html#method.into_inner). This is simply unwrapping the vector, and hence, a cheap transformation. Similarly, the pinned vec can be wrapped into a ConcurrentBag using the [`From`](https://docs.rs/orx-concurrent-bag/latest/orx_concurrent_bag/struct.ConcurrentBag.html#impl-From%3CP%3E-for-ConcurrentBag%3CT,+P%3E) trait implementation.
* Reading is only possible after converting the bag into the underlying PinnedVec.
* **⟹ no read & write race condition exists**

<div id="section-benchmarks"></div>

## Benchmarks

### Performance with `push`

*You may find the details of the benchmarks at [benches/collect_with_push.rs](https://github.com/orxfun/orx-concurrent-bag/blob/main/benches/collect_with_push.rs).*

In the experiment, **rayon**s parallel iterator, and push methods of **AppendOnlyVec**, **boxcar::Vec** and **ConcurrentBag are used to collect results from multiple threads. Further, different underlying pinned vectors of the ConcurrentBag are evaluated. 

```rust ignore
// reserve and push one position at a time
for j in 0..num_items_per_thread {
    bag.push(i * 1024 + j);
}
```

<img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_push.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_push.PNG" />

We observe that **ConcurrentBag** allows for a highly efficient concurrent collection of elements:
* [`Doubling`](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Doubling.html) growth strategy of the concurrent bag outperforms alternatives. Note that it is the default and most flexible growth strategy which does not require any prior knowledge about the size of the vector. Therefore, the default concurrent bag can be used in most situations.
* [`Linear`](https://docs.rs/orx-split-vec/latest/orx_split_vec/struct.Linear.html) growth strategy requires the equal capacity of each fragment of the underlying [`SplitVec`](https://docs.rs/orx-split-vec/latest/orx_split_vec/index.html). This strategy might be preferred when memory is more valuable or more scarce, since it can be more conservative in allocations.
* Finally [`Fixed`](https://docs.rs/orx-fixed-vec/latest/orx_fixed_vec/) is the least flexible and requires an fixed upper bound on capacity of the bag (panics when exceeded). It might be preferred only when we have a safe and good upper bound.

The performance can further be improved **significantly** by using [`extend`](https://docs.rs/orx-concurrent-bag/latest/orx_concurrent_bag/struct.ConcurrentBag.html#method.extend) method instead of **push**. You may see results in the next subsection and details in the <a href="#section-performance-notes">performance notes</a>.

### Performance with `extend`

*You may find the details of the benchmarks at [benches/collect_with_extend.rs](https://github.com/orxfun/orx-concurrent-bag/blob/main/benches/collect_with_extend.rs).*

The only difference in this follow up experiment is that we use **extend** rather than **push** with ConcurrentBag. The expectation is that this approach will solve the performance degradation due to a potential false sharing situation.

In a perfectly homogeneous scenario, we can evenly share the work to threads as follows.

```rust ignore
// reserve num_items_per_thread positions at a time
// and then push as the iterator yields
let iter = (0..num_items_per_thread).map(|j| i * 1024 + j);
bag.extend(iter);
```

However, we **do not need perfect homogeneity** or perfect information on the number of items to be pushed per thread to get the benefits of **extend**. A `batch_size` that is large enough so that batch size elements exceed a cache line would be sufficient to prevent the dramatic performance degradation of false sharing.

Using **extend** is also convenient. We can simply `step_by` and extend by `batch_size` elements.

```rust ignore
// reserve batch_size positions at each iteration
// and then push as the iterator yields
for j in (0..num_items_per_thread).step_by(batch_size) {
    let iter = (j..(j + batch_size)).map(|j| i * 1024 + j);
    bag.extend(iter);
}
```

Although concurrent collection via **push** is highly efficient, collecting elements with **extend** provides significant improvement. The following graph demonstrates this significant impact achieved by using a batch size of only 64 elements while collecting tens of thousands of elements.

<img src="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_extend.PNG" alt="https://raw.githubusercontent.com/orxfun/orx-concurrent-bag/main/docs/img/bench_collect_with_extend.PNG" />

## Comparison with Other Concurrent Collections with Pinned Elements

There are a few concurrent data structures that are built on pinned element guarantees of pinned vectors. They have different pros and cons which are summarized in the table below.

|                      | [`ConcurrentBag`](https://crates.io/crates/orx-concurrent-bag)                                                                                                                                                                                            | [`ConcurrentOrderedBag`](https://crates.io/crates/orx-concurrent-ordered-bag)                                                                                                                                                                                                                                                                                                                                                                        | [`ConcurrentVec`](https://crates.io/crates/orx-concurrent-vec)                                                                                                                                                                                            |
|----------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|-----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------|
| Write                | Guarantees that each element is written exactly once via **push** or **extend** methods.                                                                                                                                                                       | Different in two ways. First, a position can be written multiple times. Second, an arbitrary element of the bag can be written at any time at any order using **set_value** and **set_values** methods. This provides a great flexibility while moving the safety responsibility to the caller; hence, the set methods are **unsafe**.                                                                                                                     | Guarantees that each element is written exactly once via **push** or **extend** methods. Further, since it is a concurrent **grow, read & update** collection, it additionally allows to concurrently mutate already pushed elements.                                                                                                                                                                       |
| Read                 | A grow-only collection. Concurrent reading of elements is through **unsafe** **get** and **iter** methods. The caller is required to avoid race conditions.                                                                             | Not supported currently. Due to the flexible but unsafe nature of write operations, it is difficult to provide required safety guarantees as a caller.                                                                                                                                                                                                                                                                                               | A concurrent grow, read and update collection. Already pushed elements can safely be read through methods such as **get** and **iter** methods.                                                                                                                                                 |
| Ordering of Elements | Two multi-threaded executions of a program collecting elements into a bag might result in the elements being collected in different orders. | Allows to collect elements concurrently and in the correct or desired order. It is trivial to provide safety guarantees in certain situations; for instance, when used together with a [`ConcurrentIter`](https://crates.io/crates/orx-concurrent-iter).                                                                                                                                         | Two multi-threaded executions of a program collecting elements into a bag might result in the elements being collected in different orders. |
| `into_inner`         | At any time, the bag can safely and cheaply be converted to its underlying PinnedVec, and vice versa.                                                                                                                                   | Growing through flexible setters allowing to write to any position, ConcurrentOrderedBag has the risk of containing gaps. The caller is required to get the underlying PinnedVec through an **unsafe** into_inner call. | At any time, the bag can safely and cheaply be converted to its underlying PinnedVec, and vice versa. However, the pinned vector stores elements wrapped in a [`ConcurrentOption`](https://crates.io/crates/orx-concurrent-option).               |
|                      |                                                                                                          

<div id="section-performance-notes"></div>

## Performance Notes

### How many times and how long we spin?

There is only one waiting or spinning condition of the push and extend methods: whenever the underlying PinnedVec needs to grow. Note that growth with pinned vector is copy free. Therefore, when it spins, it only waits for the allocation. Since number of growth is deterministic, so is the number of spins.

For instance, assume that we will push a total of 15_000 elements concurrently to an empty bag.

* Further assume we use the default `SplitVec<_, Doubling>` as the underlying pinned vector. Throughout the execution, we will allocate fragments of capacities [4, 8, 16, ..., 4096, 8192] which will lead to a total capacity of 16_380. In other words, we will allocate exactly 12 times during the entire execution.
* If we use a `SplitVec<_, Linear>` with constant fragment lengths of 1_024, we will allocate 15 equal capacity fragments.
* If we use the strict `FixedVec<_>`, we have to pre-allocate a safe amount and can never grow beyond this number. Therefore, there will never be any spinning.

### False Sharing

We need to be aware of the potential [false sharing](https://en.wikipedia.org/wiki/False_sharing) risk which might lead to significant performance degradation when we are filling the bag with one by one with **push**.

Performance degradation due to false sharing might be observed specifically when both of the following conditions hold:
* **small data**: data to be pushed is small, the more elements fitting in a cache line the bigger the risk,
* **little work**: multiple threads/cores are pushing to the concurrent bag with high frequency; i.e., very little or negligible work / time is required in between **push** calls.

The example above fits this situation. Each thread only performs one multiplication and addition in between pushing elements, and the elements to be pushed are small.

* Actually, ConcurrentBag assigns unique positions to each value to be pushed. There is no *true* sharing among threads in the position level.
* However, cache lines contain more than one position. One thread updating a particular position invalidates the entire cache line on an other thread.
* Threads end up frequently reloading cache lines instead of doing the actual work of writing elements to the bag. This might lead to a significant performance degradation.

### `extend` to Avoid False Sharing

Assume that we are filling a ConcurrentBag from n threads. At any given point, thread A calls **extend** by passing in an iterator which will yield 64 elements. Concurrent bag will reserve 64 consecutive positions for this extend call. Concurrent push or extend calls from other threads will not have access to these positions. Assuming that size of 64 elements is large enough:
* Thread A writing to these 64 positions will not invalidate cache lines of other threads. Similarly, other threads writing to their reserved positions will not invalidate thread A's cache line.
* Further, this will reduce the number of atomic updates compared to pushing elements one at a time.

#### Refactoring from `push` to `extend`

Required change to convert the code using `push` into one using `extend` is minimal, thanks to `step_by`. The example above could be revised as follows to avoid the performance degrading of false sharing.

```rust
use orx_concurrent_bag::*;

let (num_threads, num_items_per_thread) = (4, 1_024);
let batch_size = 64;

let bag = ConcurrentBag::new();

let bag_ref = &bag;
std::thread::scope(|s| {
    for i in 0..num_threads {
        s.spawn(move || {
            for j in (0..num_items_per_thread).step_by(batch_size) {
                let iter = (j..(j + batch_size)).map(|j| i * 1024 + j);
                bag_ref.extend(iter);
            }
        });
    }
});
```

## Contributing

Contributions are welcome! If you notice an error, have a question or think something could be improved, please open an [issue](https://github.com/orxfun/orx-concurrent-bag/issues/new) or create a PR.

## License

This library is licensed under MIT license. See LICENSE for details.
