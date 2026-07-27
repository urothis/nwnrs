# nwnrs-nwpkg

`nwnrs-nwpkg` defines the typed `nwpkg.toml` and `nwpkg.lock` behavior
used by the workspace packaging tools.

It owns:

- the supported nwpkg project-kind taxonomy
- strict, validated TOML manifest and lockfile boundaries
- local and Git-backed transitive `include` package dependencies
- `nwpkg.lock` read/write behavior with SHA-256 source snapshots
- repack optimization helpers such as exact original-file reuse
- project-wide nwnrs event collection and deterministic dispatcher generation
- reusable resolved package sessions for compilers and editors

The crate depends on `nwnrs-types` for NWN-specific archive/resource vocabulary
such as `ResRef`, ERF versions, KEY/BIF versions, checksum helpers, and
compression algorithms.

Each manifest may select the native resource language:

```toml
[project]
name = "example"
kind = "module"
language = "polish"

[source]
path = "module"
```

`project.language` is optional and defaults to `english`. English, French,
German, Italian, and Spanish native text uses Windows-1252; Polish uses
Windows-1250. This policy is part of the resolved package fingerprint and is
passed explicitly to native resource readers and writers. NSS is always
Windows-1252 regardless of the package language. The manifest and lockfile
themselves remain UTF-8.

## Package boundaries

`nwpkg.toml` and `nwpkg.lock` are strict inputs. Unknown fields are rejected
instead of being silently ignored. Manifests also reject empty project names,
absolute or parent-traversing source paths, source roots outside the package,
and source paths whose file/directory shape does not match the project kind.
Public package APIs return `NwpkgError`, whose kind distinguishes IO, parse,
validation, not-found, cancellation, and compiler failures.

Manifest and lockfile writes use a temporary file in the destination directory,
synchronize its contents, and atomically persist it. Without `force`, an
existing target is never replaced. Lock snapshots hash files through a
streaming reader rather than retaining complete resource contents in memory.
Cancellation-aware lock APIs check the caller's token during directory
traversal, between KEY/BIF entries, and between hash-reader chunks.

Package source and lock snapshot traversal never follows symbolic links. This
gives every platform the same package boundary and prevents cycles or
out-of-package files from entering a compile or pack.

An include library is an nwpkg project with `kind = "include"`. A consuming
project may declare a live local path or an explicitly fetched Git source:

```toml
[dependencies]
nwnrs = { path = "../include/nwnrs" }
shared = { git = "https://example.invalid/shared.git", branch = "main" }
release = { git = "https://example.invalid/release.git", tag = "v1", subdir = "nwpkg" }
```

The resolver rejects missing or non-include packages, dependency cycles,
source roots outside their package, and case-insensitive `.nss` filename
collisions. `rev`, `tag`, and `branch` are mutually exclusive. Git operations
are explicit through `nwnrs nwpkg fetch` and `nwnrs nwpkg update`; ordinary
builds and editor analysis use only exact commits already recorded in
`nwpkg.lock` and materialized in the immutable user cache. System SSH agents
and credential helpers provide authentication, and credentials are never
stored in manifests, locks, logs, or cache metadata.

Remote Git sources are parsed into a canonical identity before they enter the
lock or cache key. Scheme and host casing, default ports, dot segments,
trailing separators, and equivalent SCP-style SSH spelling therefore cannot
create duplicate package identities.

Cache cleanup is global rather than workspace-relative. Every lock written by
an explicit fetch or update registers its exact native path and immutable
cache keys in the user cache. Ordinary builds and editor indexing remain
read-only. Cleanup removes only entries created by the managed resolver and
proven unreferenced by every registered lock. A missing owner path is retained
conservatively because an unmounted or moved project is not proof that its
pins are disposable.

## Resolved sessions

`ResolvedPackageSession` is the canonical package snapshot for compiler and
editor work. Opening it once resolves and validates:

- the owning package and source root
- the complete deterministic include dependency graph
- normalized unsaved source overlays
- the manifest-derived immutable native text context
- a filesystem resolver scoped to those roots
- one lazily initialized, shared compiler-macro registry
- the caller's cooperative cancellation token

Consumers should reuse this session instead of rediscovering manifests,
dependencies, include roots, and macros independently. Newly created unsaved
`.nss` overlays participate in source enumeration and macro discovery exactly
like files on disk. An overlay outside the package or a resolved dependency is
rejected by the strict constructor. Editor and workspace services should use
`open_with_workspace_overlays`; it accepts a workspace-wide absolute overlay
map and selects only entries belonging to the resolved package graph, while
retaining strict validation for every selected or relative overlay.

Package discovery, dependency validation, `.nss` enumeration, compiler-macro
loading, event generation, and lock snapshot hashing all accept the same
caller-owned cancellation token. Legacy convenience entry points create a
fresh token, while persistent compiler and editor requests reuse their package
session so cancellation reaches every expensive stage.

Compiler macros have no reserved filename. Before a project-wide transform,
nwpkg discovers `proc_macro!` and `macro_rules!` exports across every resolved
include dependency, preprocesses each exporting source through its ordinary
include and object-define graph, and builds one deterministic macro registry.
Transitive exports work identically, while duplicate qualified macro paths are
rejected.

For module projects, packing always generates and compiles
`_nwnrs_onload.nss` in memory. Functions marked with
`#[nwnrs::event::module_load::before]` and native event phase handlers such as
`#[nwnrs::event::associate_add::before]` are gathered across the module source
root.
Each handler must have the exact `void Handler(json event)` signature. When at
least one event attribute is present, the project preprocessor passes every
module source to the `nwnrs::__build_event_dispatcher!` procedural macro from
the resolved package macro registry. There is no private fallback copy or
reserved nwnrs macro filename: modules using canonical
`#[nwnrs::event::<subject>::<phase>]` attributes must
declare a dependency that exports that qualified macro. Modules without event
attributes remain dependency-free and receive the empty dispatcher directly.
The runtime event catalog is supplied to the macro as structured invocation
input; package source is never rewritten. The NSS macro discovers and
validates registrations,
constructs deduplicated includes and subscriptions, resolves stable relational
ordering per lane, and uses `quote!` to return
the complete dispatcher token stream. Rust only enumerates source files,
renders the returned virtual source, and compiles it. Handler source files are
included once, the current event is fetched and parsed once, and only the
handlers registered for its semantic name and phase receive the same immutable
JSON snapshot, in deterministic name order. With no handlers the generated
script is simply an empty `void main() {}`. The attribute and generated source
are compiler inputs only and never appear in NCS bytecode or the source tree.
