use crate::ConcurrentBag;
use core::fmt::Debug;
use orx_pinned_vec::IntoConcurrentPinnedVec;

impl<T: Debug, P: IntoConcurrentPinnedVec<T>> Debug for ConcurrentBag<T, P> {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("ConcurrentBag")
            .field("core", self.core())
            .finish()
    }
}
