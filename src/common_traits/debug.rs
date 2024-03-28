use crate::ConcurrentBag;
use orx_fixed_vec::PinnedVec;
use std::fmt::Debug;

impl<T: Debug, P: PinnedVec<T>> Debug for ConcurrentBag<T, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        unsafe { self.correct_pinned_len() };

        f.debug_struct("ConcurrentBag")
            .field("pinned", &unsafe { self.iter().collect::<Vec<_>>() })
            .field("len", &self.len())
            .field("capacity", &self.capacity())
            .field("maximum_capacity", &self.maximum_capacity())
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
            "ConcurrentBag { pinned: ['a', 'b', 'c', 'd', 'e'], len: 5, capacity: 12, maximum_capacity: 17179869180 }"
        );
    }
}
