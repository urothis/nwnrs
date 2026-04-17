# nwnrs-restype

`nwnrs-restype` is the registry that relates NWN's numeric resource kinds to
their conventional file extensions.

## Scope

- translate between numeric resource kinds and extensions
- provide a typed representation for resource kinds
- allow additional mappings where a project needs custom resource types

## Public Surface

### Core type

- `ResType`

### Registry operations

- `get_res_ext`
- `get_res_type`
- `lookup_res_ext`
- `lookup_res_type`
- `register_custom_res_type`
- `res_ext_registered`
- `res_type_registered`
- `RegisterResTypeError`

## Logical Edges

- `ResType` is a typed numeric identifier, not just a filename extension
  wrapper
- the registry is bi-directional: type-to-extension and extension-to-type
- custom registration exists because the ecosystem is not closed
- this crate does not resolve storage, and it does not imply anything about
  payload semantics

## Why This Crate Exists

Without a single registry layer, every container and parser would end up
inventing its own extension and type mapping logic.

## See also

- [`nwnrs-resref`](https://docs.rs/nwnrs-resref), which combines resource names
  with these type identifiers
