use crate::ConcurrentBag;
use orx_pinned_vec::IntoConcurrentPinnedVec;
use std::fmt::Debug;

impl<T: Debug, P: IntoConcurrentPinnedVec<T>> Debug for ConcurrentBag<T, P> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConcurrentBag")
            .field("core", self.core())
            .finish()
    }
}
