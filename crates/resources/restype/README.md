# nwnrs-restype

`nwnrs-restype` is the registry that relates NWN's numeric resource kinds to
their conventional file extensions.

## Scope

- translate between numeric resource kinds and extensions
- provide a typed representation for resource kinds
- allow additional mappings where a project needs custom resource types

## Non-goals

- resolve resources from storage
- parse the payload format associated with a resource type

## See also

- [`nwnrs-resref`](https://docs.rs/nwnrs-resref), which combines resource names
  with these type identifiers
