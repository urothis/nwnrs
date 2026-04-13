use std::{fmt, hash::Hash};

use tracing::{debug, instrument};

use crate::prelude::*;

impl<K, V> WeightedLru<K, V>
where
    K: Eq + Hash + Clone,
{
    /// Creates a cache with the given weight ceiling and minimum retained size.
    #[instrument(level = "debug", fields(max_weight, min_size))]
    pub fn new(max_weight: Weight, min_size: usize) -> Self {
        assert!(max_weight > 0, "max_weight must be a positive integer");

        Self {
            min_size,
            max_weight,
            current_weight: 0,
            order: std::collections::VecDeque::new(),
            entries: std::collections::HashMap::new(),
        }
    }

    /// Returns `true` if the cache contains `key`.
    #[instrument(level = "debug", skip_all)]
    pub fn contains_key(&self, key: &K) -> bool {
        self.entries.contains_key(key)
    }

    /// Returns the cached value for `key`, promoting it to most-recently-used.
    #[instrument(level = "debug", skip_all)]
    pub fn get(&mut self, key: &K) -> Option<&V> {
        if !self.entries.contains_key(key) {
            return None;
        }

        self.move_to_front(key);
        let entry = self.entries.get_mut(key)?;
        entry.usecount += 1;
        Some(&entry.value)
    }

    /// Returns the cached value for `key`, inserting one with an explicit
    /// weight if needed.
    #[instrument(level = "debug", skip_all)]
    pub fn get_or_put_with_weight<F>(&mut self, key: K, when_missing: F) -> &V
    where
        F: FnOnce(&K) -> (Weight, V),
    {
        if self.entries.contains_key(&key) {
            return match self.get(&key) {
                Some(value) => value,
                None => unreachable!("cache entry must exist after contains_key"),
            };
        }

        let (weight, value) = when_missing(&key);
        self.insert_weighted(key.clone(), weight, value);
        match self.entries.get(&key) {
            Some(entry) => &entry.value,
            None => unreachable!("cache entry must exist after insertion"),
        }
    }

    /// Returns the cached value for `key`, inserting one with weight `1` if
    /// needed.
    #[instrument(level = "debug", skip_all)]
    pub fn get_or_put<F>(&mut self, key: K, when_missing: F) -> &V
    where
        F: FnOnce(&K) -> V,
    {
        self.get_or_put_with_weight(key, |key| (1, when_missing(key)))
    }

    /// Returns the number of cached entries.
    #[must_use] 
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns `true` if the cache has no entries.
    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Returns the total cached weight.
    #[must_use] 
    pub fn weight(&self) -> Weight {
        self.current_weight
    }

    /// Removes every cached entry.
    #[instrument(
        level = "debug",
        skip_all,
        fields(entry_count = self.entries.len(), current_weight = self.current_weight)
    )]
    pub fn clear(&mut self) {
        self.order.clear();
        self.entries.clear();
        self.current_weight = 0;
        debug!("cleared weighted lru cache");
    }

    /// Removes `key` from the cache and returns its value when present.
    #[instrument(level = "debug", skip_all)]
    pub fn remove(&mut self, key: &K) -> Option<V> {
        let entry = self.entries.remove(key)?;
        self.remove_from_order(key);
        self.current_weight = self.current_weight.saturating_sub(entry.weight);
        debug!(
            entry_count = self.entries.len(),
            current_weight = self.current_weight,
            "removed cache entry"
        );
        Some(entry.value)
    }

    /// Inserts `value` with weight `1`.
    #[instrument(level = "debug", skip_all)]
    pub fn insert(&mut self, key: K, value: V) {
        self.insert_weighted(key, 1, value);
    }

    /// Inserts `value` with an explicit weight.
    #[instrument(level = "debug", skip_all, fields(weight))]
    pub fn insert_weighted(&mut self, key: K, weight: Weight, value: V) {
        if let Some(existing) = self.entries.get_mut(&key) {
            self.current_weight = self.current_weight.saturating_sub(existing.weight);
            existing.value = value;
            existing.weight = weight;
            self.current_weight += weight;
        } else {
            self.order.push_front(key.clone());
            self.entries.insert(
                key,
                crate::Entry {
                    value,
                    weight,
                    usecount: 0,
                },
            );
            self.current_weight += weight;
        }

        self.evict_if_needed();
        debug!(
            entry_count = self.entries.len(),
            current_weight = self.current_weight,
            max_weight = self.max_weight,
            "inserted cache entry"
        );
    }

    /// Returns the current keys from most-recently-used to least-recently-used.
    #[must_use] 
    pub fn keys(&self) -> Vec<K> {
        self.order.iter().cloned().collect()
    }

    /// Returns the number of successful `get` operations for `key`.
    pub fn uses(&self, key: &K) -> usize {
        self.entries.get(key).map_or(0, |entry| entry.usecount)
    }

    #[instrument(
        level = "debug",
        skip_all,
        fields(
            entry_count = self.entries.len(),
            current_weight = self.current_weight,
            max_weight = self.max_weight,
            min_size = self.min_size
        )
    )]
    fn evict_if_needed(&mut self) {
        while self.len() > self.min_size && self.current_weight > self.max_weight {
            let Some(key) = self.order.pop_back() else {
                break;
            };

            if let Some(entry) = self.entries.remove(&key) {
                self.current_weight = self.current_weight.saturating_sub(entry.weight);
                debug!(
                    entry_count = self.entries.len(),
                    current_weight = self.current_weight,
                    evicted_weight = entry.weight,
                    "evicted cache entry"
                );
            }
        }
    }

    fn move_to_front(&mut self, key: &K) {
        self.remove_from_order(key);
        self.order.push_front(key.clone());
    }

    fn remove_from_order(&mut self, key: &K) {
        if let Some(position) = self.order.iter().position(|existing| existing == key) {
            self.order.remove(position);
        }
    }
}

impl<K, V> fmt::Display for WeightedLru<K, V>
where
    K: Eq + Hash + Clone,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "<WeightedLRU weight={}/{} len={}>",
            self.current_weight,
            self.max_weight,
            self.entries.len()
        )
    }
}

#[cfg(test)]
mod tests {
    use crate::WeightedLru;

    #[test]
    fn get_promotes_entries_and_tracks_usecount() {
        let mut cache = WeightedLru::new(3, 1);
        cache.insert("a", 1);
        cache.insert("b", 2);
        assert_eq!(cache.keys(), vec!["b", "a"]);

        assert_eq!(cache.get(&"a"), Some(&1));
        assert_eq!(cache.keys(), vec!["a", "b"]);
        assert_eq!(cache.uses(&"a"), 1);
    }

    #[test]
    fn weighted_insert_evicts_oldest_entries_over_budget() {
        let mut cache = WeightedLru::new(3, 1);
        cache.insert_weighted("a", 2, 1);
        cache.insert_weighted("b", 1, 2);
        cache.insert_weighted("c", 2, 3);

        assert!(!cache.contains_key(&"a"));
        assert!(cache.contains_key(&"b"));
        assert!(cache.contains_key(&"c"));
        assert_eq!(cache.weight(), 3);
    }
}
