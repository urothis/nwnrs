# nwnrs-lru

Minimal weighted least-recently-used cache.

## Scope

- store cached values with an explicit weight
- evict entries by recency subject to a total weight budget
- provide the small cache behavior needed by `nwnrs-resman` and `nwnrs-tlk`

Use [`WeightedLru`] when eviction should be based on approximate byte size
rather than item count alone.

## Public Surface

- `Weight`
- `WeightedLru`

## Logical Edges

- the cache is weight-driven, not item-count driven
- it exists to support workloads where a small number of large items can
  dominate memory pressure
- the crate intentionally does not model persistence, sharding, invalidation
  policy, or distributed behavior

## See also

- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which uses this cache for
  weighted resource payload eviction
- [`nwnrs-tlk`](https://docs.rs/nwnrs-tlk), which uses this cache for lazy
  stream-backed dialog-table entries

## Why This Crate Exists

`ResMan` and other consumers need cheap bounded caching, but "N items" is a bad
policy for variable-sized binary payloads.
