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

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{collections::HashMap, io::Cursor, sync::Arc, time::SystemTime};

    use nwnrs_checksums::EMPTY_SECURE_HASH;
    use nwnrs_exo::ExoResFileCompressionType;
    use nwnrs_resref::{ResolvedResRef, new_res_ref};
    use nwnrs_restype::ResType;

    use crate::{Res, ResContainer, ResMan, ResManError, ResManResult, new_res_origin, shared_stream};

    #[derive(Clone)]
    struct TestContainer {
        label:   &'static str,
        entries: HashMap<nwnrs_resref::ResRef, Res>,
    }

    impl std::fmt::Display for TestContainer {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str(self.label)
        }
    }

    impl ResContainer for TestContainer {
        fn contains(&self, rr: &nwnrs_resref::ResRef) -> bool {
            self.entries.contains_key(rr)
        }

        fn demand(&self, rr: &nwnrs_resref::ResRef) -> ResManResult<Res> {
            self.entries
                .get(rr)
                .cloned()
                .ok_or_else(|| ResManError::msg(format!("not found: {rr}")))
        }

        fn count(&self) -> usize {
            self.entries.len()
        }

        fn contents(&self) -> Vec<nwnrs_resref::ResRef> {
            self.entries.keys().cloned().collect()
        }
    }

    fn make_res(name: &str, ty: u16, bytes: &[u8], label: &str) -> Res {
        let rr = new_res_ref(name, ResType(ty)).unwrap_or_else(|error| {
            panic!("make rr: {error}");
        });
        Res::new_with_stream(
            new_res_origin("TestContainer", label),
            rr,
            SystemTime::UNIX_EPOCH,
            shared_stream(Cursor::new(bytes.to_vec())),
            bytes.len() as i64,
            0,
            ExoResFileCompressionType::None,
            None,
            bytes.len(),
            EMPTY_SECURE_HASH,
        )
    }

    #[test]
    fn resolves_latest_container_first_and_unions_contents() {
        let shared = new_res_ref("shared", ResType(2027)).unwrap_or_else(|error| {
            panic!("shared rr: {error}");
        });
        let older = TestContainer {
            label: "older",
            entries: HashMap::from([
                (shared.clone(), make_res("shared", 2027, b"older", "older")),
                (
                    new_res_ref("only_old", ResType(2027)).unwrap_or_else(|error| {
                        panic!("only_old rr: {error}");
                    }),
                    make_res("only_old", 2027, b"old", "older"),
                ),
            ]),
        };
        let newer = TestContainer {
            label: "newer",
            entries: HashMap::from([(
                shared.clone(),
                make_res("shared", 2027, b"newer", "newer"),
            )]),
        };

        let mut manager = ResMan::new(1);
        manager.add(Arc::new(older));
        manager.add(Arc::new(newer));

        let res = match manager.demand(&shared, false) {
            Ok(value) => value,
            Err(error) => panic!("demand shared: {error}"),
        };
        let bytes = match res.read_all(false) {
            Ok(value) => value,
            Err(error) => panic!("read shared bytes: {error}"),
        };
        assert_eq!(bytes, b"newer".to_vec());
        assert_eq!(manager.contents().len(), 2);
    }

    #[test]
    fn resolves_fully_specified_references() {
        let rr = new_res_ref("alpha", ResType(2027)).unwrap_or_else(|error| {
            panic!("alpha rr: {error}");
        });
        let container = TestContainer {
            label: "single",
            entries: HashMap::from([(rr.clone(), make_res("alpha", 2027, b"alpha", "single"))]),
        };
        let mut manager = ResMan::new(1);
        manager.add(Arc::new(container));

        let resolved = ResolvedResRef::from_filename("alpha.utc").unwrap_or_else(|error| {
            panic!("resolved rr: {error}");
        });
        assert!(manager.get_resolved(&resolved).is_some());
    }
}
