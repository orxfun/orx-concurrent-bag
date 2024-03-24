pub(crate) const ERR_FAILED_TO_GROW: &str =
    "The underlying pinned vector reached its capacity and failed to grow";

pub(crate) const ERR_FAILED_TO_PUSH: &str = "Failed to push new element to the concurrent bag";

pub(crate) const ERR_REACHED_MAX_CAPACITY: &str = "Out of capacity. Underlying pinned vector cannot grow any further while being concurrently safe.";
