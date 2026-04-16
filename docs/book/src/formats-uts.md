# UTS Sound Blueprints

Registry identity:

- extension: `uts`
- resource type: `2035`
- top-level GFF tag: `"UTS "`

`UTS` is the canonical sound blueprint resource.

## Role

A `UTS` defines one authored sound object template: what sound resources are
used, how playback behaves, and which default policies apply before a concrete
sound emitter instance is placed into an area.

## Conceptual Shape

```text
UTS root
|
+-- identity            tag, localized/display metadata
+-- playback policy     radius/loop/randomization-style defaults
+-- sound references    nested or repeated sound identifiers
+-- scripts             event hooks when applicable
+-- authored defaults   object-level sound behavior
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTS` schema crate yet

## Logical Edges

- `UTS` is not the raw audio payload. The sound file itself is a different
  resource.
- A placed sound emitter in `GIT` adds concrete transform and instance
  geometry/state.
- Sound blueprints often mix content selection with playback policy, which is
  why they deserve their own schema class.

## Related Chapters

- [GFF](./formats-gff.md)
- [GIT Area Instances](./formats-git.md)
- [SoundSets (SSF)](./formats-ssf.md)
