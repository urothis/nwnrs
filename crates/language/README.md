# nwnrs-language

`nwnrs-language` is the persistent, compiler-owned language intelligence core
for NWScript and `nwpkg.toml` projects.

It provides reusable checking, indexing, source resolution, diagnostics,
manifest intelligence, and package-session management without depending on the
`nwnrs` command-line application or an editor protocol.

Open NSS documents are retained as exact `Arc<[u8]>` Windows-1252 snapshots.
The service never converts an overlay to UTF-8 before lexing, preprocessing,
indexing, or compilation. Invalid editor versions explicitly invalidate the
current overlay and cannot fall back to older editor contents or the file on
disk.

Compiler byte spans are converted to zero-based VS Code UTF-16 ranges inside
this crate. Manifest analysis uses its separate UTF-8 source-map path because
`nwpkg.toml` is a modern tool-owned format.

Dependency operations return `LanguageOperationError` with a stable code,
message, physical path, and manifest-field path. The native and worker layers
preserve those fields instead of flattening failures into display strings.
Explicit Git fetch and update operations also publish sanitized progress
events while they run.
