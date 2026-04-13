# nwnrs-exo

Shared EXO-level constants and enums.

## Scope

- define the small set of magic values and compression markers shared by
  EXO-backed container formats
- prevent those low-level constants from being redefined inconsistently across
  multiple crates

## Invariants

- each constant or enum value corresponds directly to a known EXO wire-level
  concept

## Non-goals

- parse full EXO-backed containers on its own
- provide a general binary-protocol abstraction
