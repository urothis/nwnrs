# GIT Area Instances

Docs:

- [crate docs](https://docs.rs/nwnrs-git/latest/nwnrs_git/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/git/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/formats/git/src/lib.rs)

`GIT` is a typed lift over `GFF` for placed area instances.

## Public Surface

- `GIT_RES_TYPE`
- `GitError`
- `GitResult`
- `GitFile`
- `GitAreaProperties`
- `GitTransform`
- `GitPoint`
- `GitCreature`
- `GitDoor`
- `GitEncounter`
- `GitSound`
- `GitStore`
- `GitTrigger`
- `GitWaypoint`
- `GitPlaceable`
- `read_git`
- `build_git_root`
- `write_git`

## Core Model

`GitFile` preserves:

- optional `area_properties`
- ordered collections for creatures, doors, encounters, sounds, stores,
  triggers, waypoints, and placeables
- `legacy_list` for raw top-level list entries not yet lifted by the typed view

Every typed entry retains its original raw `GffStruct`.

## Structural Layout

`GIT` is not its own independent binary container. Physically it is `GFF`.
Conceptually, the lifted structure looks like this:

```text
GFF root
|
+-- AreaProperties?
+-- Creature List[]    -> GitCreature
+-- Door List[]        -> GitDoor
+-- Encounter List[]   -> GitEncounter
+-- Sound List[]       -> GitSound
+-- Store List[]       -> GitStore
+-- Trigger List[]     -> GitTrigger
+-- Waypoint List[]    -> GitWaypoint
+-- Placeable List[]   -> GitPlaceable
+-- Legacy List[]      -> raw GFF structs not yet modeled
```

Common typed substructures:

- `GitTransform`
  - position
  - bearing or orientation vector
- `GitPoint`
  - explicit geometry vertices for triggers and encounters

## Logical Edges

- Typed collections preserve authored order per category.
- Raw `GFF` is retained rather than discarded after lifting.
- Rebuild logic rewrites owned typed fields while preserving unknown raw fields
  where possible.
- Placement transforms, polygon geometry, and local metadata stay explicit
  rather than being flattened into generic key-value maps.

## Why This Crate Exists

`GIT` is a good example of "typed-over-GFF" design. The point is not to erase
the `GFF` substrate. The point is to add a domain model on top of it while
keeping enough raw structure available that incomplete coverage does not become
destructive.
