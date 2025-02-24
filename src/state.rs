use core::{
    cmp::Ordering,
    sync::atomic::{self, AtomicUsize},
};
use orx_pinned_concurrent_col::{ConcurrentState, PinnedConcurrentCol, WritePermit};
use orx_pinned_vec::ConcurrentPinnedVec;

#[derive(Debug)]
pub struct ConcurrentBagState {
    len: AtomicUsize,
    written_len: AtomicUsize,
}

impl<T> ConcurrentState<T> for ConcurrentBagState {
    fn fill_memory_with(&self) -> Option<fn() -> T> {
        None
    }

    fn new_for_pinned_vec<P: orx_fixed_vec::prelude::PinnedVec<T>>(pinned_vec: &P) -> Self {
        Self {
            len: pinned_vec.len().into(),
            written_len: pinned_vec.len().into(),
        }
    }

    fn new_for_con_pinned_vec<P: ConcurrentPinnedVec<T>>(_: &P, len: usize) -> Self {
        Self {
            len: len.into(),
            written_len: len.into(),
        }
    }

    fn write_permit<P>(&self, col: &PinnedConcurrentCol<T, P, Self>, idx: usize) -> WritePermit
    where
        P: ConcurrentPinnedVec<T>,
    {
        let capacity = col.capacity();

        match idx.cmp(&capacity) {
            Ordering::Less => WritePermit::JustWrite,
            Ordering::Equal => WritePermit::GrowThenWrite,
            Ordering::Greater => WritePermit::Spin,
        }
    }

    fn write_permit_n_items<P>(
        &self,
        col: &PinnedConcurrentCol<T, P, Self>,
        begin_idx: usize,
        num_items: usize,
    ) -> WritePermit
    where
        P: ConcurrentPinnedVec<T>,
    {
        let capacity = col.capacity();
        let last_idx = begin_idx + num_items - 1;

        match (begin_idx.cmp(&capacity), last_idx.cmp(&capacity)) {
            (_, core::cmp::Ordering::Less) => WritePermit::JustWrite,
            (core::cmp::Ordering::Greater, _) => WritePermit::Spin,
            _ => WritePermit::GrowThenWrite,
        }
    }

    #[inline(always)]
    fn release_growth_handle(&self) {}

    #[inline(always)]
    fn update_after_write(&self, _: usize, end_idx: usize) {
        self.written_len.store(end_idx, atomic::Ordering::Release);
    }

    fn try_get_no_gap_len(&self) -> Option<usize> {
        Some(self.len())
    }
}

impl ConcurrentBagState {
    #[inline(always)]
    pub(crate) fn fetch_increment_len(&self, increment_by: usize) -> usize {
        self.len.fetch_add(increment_by, atomic::Ordering::SeqCst)
    }

    #[inline(always)]
    pub(crate) fn len(&self) -> usize {
        self.len.load(atomic::Ordering::Acquire)
    }

    #[inline(always)]
    pub(crate) fn written_len(&self) -> usize {
        self.written_len.load(atomic::Ordering::Acquire)
    }
}
