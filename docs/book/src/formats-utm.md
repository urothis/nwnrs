# UTM Store Blueprints

Registry identity:

- extension: `utm`
- resource type: `2051`
- top-level GFF tag: `"UTM "`

`UTM` is the canonical store or merchant blueprint resource.

## Role

A `UTM` defines one authored store template: catalog behavior, economic defaults,
and the set of items a merchant can expose before the object is tied into a
module or area.

## Conceptual Shape

```text
UTM root
|
+-- identity            tag, localized name, blueprint identity
+-- economics           price modifiers and store policy
+-- inventory/catalog   nested saleable item list
+-- scripts             event hooks
+-- metadata            authored flags and store defaults
```

## Current Code Coverage

- generic container support: `nwnrs-gff`
- resource identity: `nwnrs-restype`
- no dedicated lifted `UTM` schema crate yet

## Logical Edges

- A store blueprint is not itself a placed merchant actor. It is the catalog
  definition and store behavior layer.
- Nested inventory in `UTM` behaves more like a curated resource catalog than a
  loose runtime container.
- Economic behavior is part of the schema, not an external spreadsheet layered
  on later.

## Related Chapters

- [GFF](./formats-gff.md)
- [UTI Item Blueprints](./formats-uti.md)
- [GIT Area Instances](./formats-git.md)
