# nwnrs-lru

Minimal weighted least-recently-used cache.

## Scope

- store cached values with an explicit weight
- evict entries by recency subject to a total weight budget
- provide the small cache behavior needed by `nwnrs-resman` and `nwnrs-tlk`

Use [`WeightedLru`] when eviction should be based on approximate byte size
rather than item count alone.

## Non-goals

- provide a general-purpose caching framework
- model persistence, sharding, or distributed cache behavior
