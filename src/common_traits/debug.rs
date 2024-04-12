use crate::ConcurrentBag;
use orx_fixed_vec::PinnedVec;
use std::fmt::Debug;

impl<T: Debug, P: PinnedVec<T>> Debug for ConcurrentBag<T, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentBag")
            .field("core", self.core())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
            "ConcurrentBag { core: PinnedConcurrentCol { pinned_vec: \"PinnedVec\", state: ConcurrentBagState { len: 5 }, capacity: CapacityState { capacity: 12, maximum_capacity: 17179869180 } } }"
        );
    }
}
