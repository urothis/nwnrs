# nwnrs for Visual Studio Code

This extension provides `.nss` language support and reports errors from the
real nwnrs NWScript compiler in VS Code's Problems panel and directly in the
editor. The compiler is linked into a bundled native Node module; the extension
does not launch or require the `nwnrs` CLI.

## Current support

- NWScript (`.nss`) language registration and syntax highlighting.
- Debounced compiler checks for unsaved edits, opens, and saves. Dirty NSS
  buffers are passed to the compiler as in-memory source overlays, including
  dirty include files.
- Manual checks for the current file or every NWScript project in the
  workspace.
- Multiple independent diagnostics per file, source locations, error codes,
  included-file diagnostics, and compiler output.
- Nearest-project discovery through `nwpkg.toml`.
- Non-mutating checks through the reusable nwnrs compiler API.
- Cancellable workspace progress, dependency-aware project deduplication,
  project/include watchers, and one persistent language worker with isolated,
  invalidatable indexes per `nwpkg.toml` package.
- Go to Definition and hover documentation for functions, macros, strong enum
  types, enum variants, enum compatibility aliases, and type aliases across
  the current project, configured include directories, and transitive local
  `nwpkg` dependencies.
- Go to Definition for vanilla functions, constants, and engine structures.
  Workspace files, unsaved overlays, configured include roots, and enabled
  game overrides take precedence over packed vanilla scripts. Packed fallbacks
  open read-only. Vanilla function hover text is extracted directly from the
  resolved source's adjacent `//` comments; no documentation sidecar or
  modified game source is involved.
- A compiler-backed Outline and breadcrumb tree for source-authored functions,
  globals, constants, structs and fields, strong enums and variants, type
  aliases, `#define` declarations, and extended macros. Enum variants and
  struct fields are nested under their owning declarations; event handlers
  show their event identity beside the function signature.
- Outline updates use dirty in-memory buffers, remain available through common
  parse and lexical errors, exclude included declarations and synthetic macro
  output, and work in read-only packed game scripts as well as physical files.
- Find References, safe package-wide Rename Symbol, workspace symbols, and
  incoming/outgoing call hierarchy. References cross sibling scripts and
  editable local dependencies while respecting local shadowing and enum
  qualification and receiver types for same-named structure fields.
  Packed/generated targets are read-only.
- Compiler-backed semantic highlighting for functions, parameters, variables,
  fields, types, enums, variants, and macros.
- Configurable enum-value and parameter-name inlay hints.
- Quick fixes for a missing semicolon and for a missing `#include` when exactly
  one accessible source provides the unresolved symbol.
- Cmd/Ctrl+Click navigation on `#include` paths using compiler precedence.
- `nwpkg.toml` syntax highlighting, typed validation, completion (including
  paths and project kinds), hover help, Outline, and source/dependency path
  navigation.
- Editable custom documents for the GFF family, 2DA tables, TLK string tables,
  DDS/TGA/PLT textures, ERF/HAK/MOD/NWM archives, and KEY-managed BIF sets.
  These use the nwnrs typed parsers and writers in the bundled native service;
  the webview never parses or rewrites a binary format itself.
- Native undo/redo edits, hot-exit backups, revert, multi-view synchronization,
  external-change detection, and atomic single-file saves for every custom
  document. KEY and its referenced BIF files are staged and committed as one
  rollback-capable transaction.
- Archive resource search, paging, add/remove/rename/replace, extraction, and
  nested custom editors. Saving a nested editor updates and dirties its owning
  archive instead of creating an unrelated temporary file.
- Read-only WebGL 2 scene views for MDL/WOK/DWK/PWK, UTC/UTD/UTP/UTI,
  ARE/GIT, and module IFO resources. The persistent native scene service uses
  the same package-aware ResMan precedence as compilation and sends packed
  binary geometry and texture buffers to the webview without JSON/base64
  expansion.

Remaining formats and editor features are tracked in
[VSCODE_TODO.md](./VSCODE_TODO.md).

## Resource editors

Opening a supported resource uses `nwnrs Resource Editor` by default. A 2DA is
always presented as a spreadsheet-style table, not as a plain text buffer.

- GFF editors expose every field kind, nested structures and lists, exact
  64-bit integer text, localized strings, and opaque byte payloads. Unchanged
  parsed fields retain their source provenance during an edited rewrite.
- TLK search and paging stay in the native worker, so the webview only receives
  the visible entries. Text, sound references, lengths, flags, and language are
  editable.
- DDS and TGA show decoded RGBA pixels and accept image imports. DDS keeps its
  DXT1/DXT5 format and regenerates a valid mip chain. Imported TGA pixels are
  deliberately encoded as a canonical top-left 32-bit image. PLT remains
  palette-aware: pixels are edited as value/layer pairs rather than being
  incorrectly flattened into ordinary RGBA data.
- ERF-family and KEY editors page large entry sets in native code. Supported
  nested resources open in the same custom editor; other resource types can
  still be extracted or replaced.

A BIF does not contain the resource names stored in its owning KEY table, so
the extension opens BIF contents through `.key` rather than pretending a
standalone BIF has enough identity information. KEY edits preserve BIF
filenames, drive flags, resource order, OIDs, and compression policy.

The custom-document lifecycle supports protected origins: Save routes those
through **Save as Override**, with `nwnrs.overrideDirectory` controlling the
default destination. Ordinary workspace files and user-owned archives save
normally. A general installed-resource browser that creates those protected
documents is still tracked separately; packed NSS source continues to use its
existing read-only source view.

## 3D scene viewer

Opening a model, walkmesh, visual blueprint, area, or module IFO selects the
read-only scene viewer. Module views provide an area selector, and available
animations are selected directly from the viewer toolbar. Scene Data and
Dependencies are collapsed, expandable panels over the viewport; material,
node, shader, diagnostic, and dependency details remain available without
taking permanent space beside the model. Scene files and their supporting
resources refresh automatically when workspace files change. One global
environment light illuminates model surfaces uniformly from every direction,
without a directional key, overhead sun, or rim light. Area day/night colors
tint that omnidirectional illumination automatically; standalone resources use
neutral white illumination. Explicit light nodes authored into scene content
remain visible as local contributions.

The scene service resolves supermodels, reference-model attachments, creature
and item appearance composition, door/placeable collision companions, tile
models and WOKs, skyboxes, 2DA appearance/light catalogs, DDS/TGA/PLT textures,
TXI directives, MTR descriptors, and SHD source through the active package's
resource graph. A physical workspace winner opens as its editable file from
the Dependencies tab. A packed game winner opens as an immutable virtual
resource; packed MTR/TXI/SHD/SET sources open as read-only text documents.

The viewport supports mouse orbit/pan/zoom, keyboard arrow orbit and `+`/`-`
zoom, frame-to-scene, GPU skinning, transform and animmesh playback, transition
blending, Bezier-key interpolation, timed animation-event display,
normal/specular/roughness/emissive material channels, cyclic TXI atlas
animation, model lights, tile light-color overrides, lens flares, emitters,
dangly motion, explosion chunk models, trigger/encounter polygons, sound radii,
waypoint/store markers, authored vertex colors, and material-colored collision
inspection. Standalone WOK/DWK/PWK resources open directly in Collision mode.
Shader sources and all diagnostics are visible instead of being silently
discarded. WebGL context loss is reported and the view reconstructs itself when
VS Code restores the context.

Scene catalogs keep animation tracks and texture pixels out of the initial
payload. The viewer fetches exact model/animation assets and each referenced
texture on demand, retains each packet with its owning binary buffer, and never
mutates the scene catalog. Playback propagates only through the selected model's
attachment tree. The viewer uploads authored DDS/DXT mip chains directly when the GPU supports S3TC,
and falls back to decoded RGBA safely. Dependency changes invalidate only scenes
that consume the changed resource. Per-scene render tables, uniforms, poses,
matrices, overlays, packet assets, and effect buffers are retained and reused;
evicted native scene assets are transparently rehydrated; explosion chunks
are GPU-instanced, static scenes use reduced shaders, and animated views lower
their internal resolution only when sustained frame cost requires it. Hidden
editors release their WebGL context while VS Code state preserves the camera and
selected animation for restoration.

Scene documents intentionally remain read-only. Editing model/area content is
deferred until it can share the typed custom-document undo, backup, conflict,
and atomic-save guarantees used by the existing binary editors.

## Prerequisite

Building or packaging the extension requires the repository's pinned Rust
toolchain and Node.js. Running the installed extension does not require Rust or
a separately installed `nwnrs` executable.

The currently bundled native target is macOS Apple Silicon. Windows, Linux,
and Intel macOS packages are explicitly tracked in
[VSCODE_TODO.md](./VSCODE_TODO.md).

The extension icon is copied from the repository's canonical
`assets/logo/icon.png` during the native/package build, so the VSIX carries the
same project artwork without maintaining a second icon source.

## Run the extension during development

Open `editors/vscode-nwnrs` as the VS Code workspace and press F5. The included
launch configuration builds the native compiler module and opens an Extension
Development Host. Open a folder containing `.nss` files in that host.

No `npm install` step is required for extension development. The JavaScript
side uses only the VS Code and Node.js APIs.

## Package and install locally

From the extension directory:

```sh
npm run build-native
npx --yes @vscode/vsce package --target darwin-arm64 --out nwnrs-0.0.2.vsix
code --install-extension nwnrs-0.0.2.vsix --force
```

`vsce package` automatically runs the native build through the extension's
`vscode:prepublish` script, so the explicit first command is optional. It is
shown because it provides a clearer build failure before packaging.

## Commands

- `nwnrs: Check Current NWScript File` checks the active NSS buffer without
  forcing it to be saved.
- `nwnrs: Check NWScript Workspace` finds `nwpkg.toml` manifests and checks
  their source trees recursively. If there is no manifest, it checks the open
  workspace folder.
- `nwnrs: Show Compiler Output` opens compiler activity and failure details.

Click the `nwnrs` status-bar item to reindex the current package, restart the
language service, show compiler output, clear diagnostics, or open the nwnrs
settings.

Cmd+Click a supported symbol on macOS, or Ctrl+Click it on Windows/Linux, to
jump to its definition. The standard **Go to Definition** command and F12 use
the same provider. A workspace override opens as its normal editable file;
otherwise the resolved packed game script opens as an immutable `nwnrs-game`
virtual document.

Open VS Code's **Outline** view or use breadcrumbs to navigate the declarations
in the active NSS document. The tree represents that document itself rather
than the complete include graph, so opening an include or packed game script
shows its own declarations without duplicating them into every consumer.

Use **Find All References**, **Rename Symbol**, **Go to Symbol in Workspace**,
and **Call Hierarchy** through their normal VS Code commands. Renaming is
limited to editable physical sources and is computed from the compiler's
resolved symbol identity; packed game sources are intentionally rejected.

## Settings

| Setting | Default | Purpose |
| --- | --- | --- |
| `nwnrs.checkOnSave` | `true` | Check an NSS file after it is saved. |
| `nwnrs.checkOnChange` | `true` | Check dirty in-memory NSS contents while editing. |
| `nwnrs.checkOnOpen` | `true` | Check an NSS file after it is opened. |
| `nwnrs.debounceMilliseconds` | `250` | Delay automatic checks and supersede an older check for the same file. |
| `nwnrs.noEntrypointCheck` | `true` | Accept include files without `main()` or `StartingConditional()`. |
| `nwnrs.includeDirectories` | `[]` | Additional compiler include roots. Relative paths use the owning project root. |
| `nwnrs.langspecPath` | empty | Explicit `nwscript.nss` path. |
| `nwnrs.rootPath` | empty | Optional NWN installation root; empty uses platform discovery. |
| `nwnrs.userPath` | empty | Optional NWN user directory; empty uses platform discovery. |
| `nwnrs.overrideDirectory` | empty | Save-as destination for protected game resources; empty falls back to `userPath/override` or prompts. |
| `nwnrs.language` | `english` | Installation language used for resource lookup. |
| `nwnrs.loadOvr` | `false` | Include the user override directory during resource lookup. |
| `nwnrs.maxIncludeDepth` | `16` | Maximum recursive include depth. |
| `nwnrs.maxDiagnosticsPerFile` | `50` | Bound independent diagnostic recovery for one failed input. |
| `nwnrs.inlayHints.enumValues` | `true` | Show automatic integer enum values. |
| `nwnrs.inlayHints.parameterNames` | `literals` | Show parameter hints for `off`, literal arguments, or `all` arguments. |

## Current limits

- The bundled native compiler currently supports macOS Apple Silicon only.
- NSS completion, signature help, formatting, event-specific views, format
  schemas, and VS Code-host integration tests remain future work.
- Vanilla constants without adjacent source comments navigate correctly but
  intentionally have no fabricated hover description.

## Verify

```sh
npm run build-native
npm run check
npm test
```
