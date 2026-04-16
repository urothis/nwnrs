# Weighted LRU

Docs:

- crate: `nwnrs-lru`
- [crate docs](https://docs.rs/nwnrs-lru/latest/nwnrs_lru/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/foundation/lru/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/foundation/lru/src/lib.rs)

## Scope

`nwnrs-lru` is the minimal weighted least-recently-used cache used where entry count is the wrong metric and byte-ish size is the right one.

## Public Surface

- `Weight`
- `WeightedLru`

## Logical Edges

- The cache is weight-driven, not item-count driven.
- It exists to support resource and text-table workloads where a small number of large items can dominate memory pressure.
- The crate intentionally does not model persistence, sharding, invalidation policy, or distributed behavior. It is a local eviction primitive.

## Why This Crate Exists

`ResMan` and other consumers need cheap bounded caching, but "N items" is a bad policy for variable-sized binary payloads. This crate provides the narrower abstraction that those consumers actually need.
