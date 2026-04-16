# Resource Types

Docs:

- crate: `nwnrs-restype`
- [crate docs](https://docs.rs/nwnrs-restype/latest/nwnrs_restype/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/resources/restype/README.md)
- [source](https://github.com/urothis/nwnrs/blob/main/crates/resources/restype/src/lib.rs)

## Scope

`nwnrs-restype` is the registry for NWN resource-kind identity. It answers the question "what kind of resource is this?" before any resource resolution or payload parsing happens.

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

- `ResType` is a typed numeric identifier, not just a filename extension wrapper.
- The registry is bi-directional: type-to-extension and extension-to-type.
- Custom registration exists because the ecosystem is not closed. Tooling can need project-specific or non-stock resource kinds.
- This crate does not resolve storage, and it does not imply anything about payload semantics. It is identity only.

## Why This Crate Exists

Without a single registry layer, every container and parser would end up inventing its own extension/type mapping logic. This crate centralizes that vocabulary.

For the shipped built-in mappings themselves, see the
[Built-In Resource Catalog](./resources-catalog.md).
