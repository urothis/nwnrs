# nwnrs-localization

`nwnrs-localization` defines the small vocabulary that recurs across TLK, GFF,
SSF, and installation-facing resource loading.

## Scope

- represent NWN language identifiers
- represent dialog string references
- represent the male/female selector used by TLK lookup
- keep those foundational concepts consistent across the workspace

The relevant entry points are [`Language`], [`StrRef`], and
[`resolve_language`].

## Non-goals

- implement TLK storage or lookup on its own
- provide translation workflows or localization tooling
