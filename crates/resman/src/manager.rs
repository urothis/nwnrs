use std::{collections::HashSet, fmt, sync::Arc};

use nwnrs_lru::prelude::*;
use nwnrs_resref::prelude::*;
use tracing::instrument;

use crate::prelude::*;

/// Layered resource manager.
///
/// Containers are searched from front to back, so newly added containers take
/// precedence over earlier ones. An optional weighted LRU cache can memoize
/// recent lookups.
pub struct ResMan {
    containers: Vec<Arc<dyn ResContainer>>,
    cache:      Option<WeightedLru<ResRef, Res>>,
}

impl fmt::Debug for ResMan {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ResMan")
            .field("container_count", &self.containers.len())
            .field("has_cache", &self.cache.is_some())
            .finish()
    }
}

impl ResMan {
    /// Creates an empty resource manager.
    ///
    /// `cache_size_mb` controls the optional lookup cache size in megabytes. A
    /// value of `0` disables the manager-level cache.
    pub fn new(cache_size_mb: usize) -> Self {
        Self {
            containers: Vec::new(),
            cache:      (cache_size_mb > 0)
                .then(|| WeightedLru::new(cache_size_mb * 1024 * 1024, 1)),
        }
    }

    /// Returns whether any container can resolve `rr`.
    ///
    /// When `use_cache` is `true`, the manager cache is checked first.
    #[instrument(level = "debug", skip_all, fields(resref = %rr, use_cache))]
    pub fn contains(&mut self, rr: &ResRef, use_cache: bool) -> bool {
        if use_cache
            && self
                .cache
                .as_mut()
                .is_some_and(|cache| cache.contains_key(rr))
        {
            return true;
        }

        self.containers
            .iter()
            .any(|container| container.contains(rr))
    }

    /// Resolves `rr` to the highest-precedence matching resource.
    ///
    /// When `use_cache` is `true`, successful lookups are memoized in the
    /// manager cache.
    #[instrument(level = "debug", skip_all, err, fields(resref = %rr, use_cache))]
    pub fn demand(&mut self, rr: &ResRef, use_cache: bool) -> ResManResult<Res> {
        if use_cache
            && let Some(cached) = self.cache.as_mut().and_then(|cache| cache.get(rr).cloned())
        {
            return Ok(cached);
        }

        for container in &self.containers {
            if container.contains(rr) {
                let result = container.demand(rr)?;
                if use_cache && let Some(cache) = self.cache.as_mut() {
                    let weight = usize::try_from(result.io_size().max(1)).unwrap_or(usize::MAX);
                    cache.insert_weighted(rr.clone(), weight, result.clone());
                }
                return Ok(result);
            }
        }

        Err(ResManError::msg(format!("not found: {rr}")))
    }

    /// Returns the union of resource references exposed by all containers.
    #[instrument(level = "debug", fields(container_count = self.containers.len()))]
    pub fn contents(&self) -> HashSet<ResRef> {
        let mut result = HashSet::new();
        for container in &self.containers {
            result.extend(container.contents());
        }
        result
    }

    /// Resolves a fully specified `name.ext` resource reference.
    #[instrument(level = "debug", skip_all, fields(resref = %rr))]
    pub fn get_resolved(&mut self, rr: &ResolvedResRef) -> Option<Res> {
        let base = rr.base().clone();
        self.contains(&base, true)
            .then(|| self.demand(&base, true).ok())
            .flatten()
    }

    /// Resolves `rr`, returning `None` instead of an error when absent.
    #[instrument(level = "debug", skip_all, fields(resref = %rr))]
    pub fn get(&mut self, rr: &ResRef) -> Option<Res> {
        self.contains(rr, true)
            .then(|| self.demand(rr, true).ok())
            .flatten()
    }

    /// Adds `container` at the front of the search order.
    #[instrument(level = "debug", skip_all)]
    pub fn add(&mut self, container: Arc<dyn ResContainer>) {
        self.containers.insert(0, container);
    }

    /// Returns the current container search order.
    pub fn containers(&self) -> &[Arc<dyn ResContainer>] {
        &self.containers
    }

    /// Removes the exact container instance when present.
    #[instrument(level = "debug", skip_all)]
    pub fn remove(&mut self, container: &Arc<dyn ResContainer>) -> bool {
        if let Some(index) = self
            .containers
            .iter()
            .position(|candidate| Arc::ptr_eq(candidate, container))
        {
            self.containers.remove(index);
            true
        } else {
            false
        }
    }

    /// Removes the container at `index`.
    #[instrument(level = "debug", fields(index))]
    pub fn remove_at(&mut self, index: usize) -> Option<Arc<dyn ResContainer>> {
        (index < self.containers.len()).then(|| self.containers.remove(index))
    }

    /// Returns the manager-level cache when caching is enabled.
    pub fn cache(&mut self) -> Option<&mut WeightedLru<ResRef, Res>> {
        self.cache.as_mut()
    }
}
