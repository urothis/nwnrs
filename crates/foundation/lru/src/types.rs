use std::collections::{HashMap, VecDeque};

/// The weight type used by [`WeightedLru`].
pub type Weight = usize;

#[derive(Debug, Clone)]
pub(crate) struct Entry<V> {
    pub(crate) value:    V,
    pub(crate) weight:   Weight,
    pub(crate) usecount: usize,
}

/// A recency-ordered cache that evicts by accumulated entry weight.
#[derive(Debug, Clone)]
pub struct WeightedLru<K, V> {
    pub(crate) min_size:       usize,
    pub(crate) max_weight:     Weight,
    pub(crate) current_weight: Weight,
    pub(crate) order:          VecDeque<K>,
    pub(crate) entries:        HashMap<K, Entry<V>>,
}
