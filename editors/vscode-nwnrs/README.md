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

NSS and `nwpkg.toml` are supported today. Other planned formats and editor
features are tracked in [VSCODE_TODO.md](./VSCODE_TODO.md).

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
npx --yes @vscode/vsce package --target darwin-arm64 --out nwnrs-0.0.1.vsix
code --install-extension nwnrs-0.0.1.vsix --force
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
| `nwnrs.language` | `english` | Installation language used for resource lookup. |
| `nwnrs.loadOvr` | `false` | Include the user override directory during resource lookup. |
| `nwnrs.maxIncludeDepth` | `16` | Maximum recursive include depth. |
| `nwnrs.maxDiagnosticsPerFile` | `50` | Bound independent diagnostic recovery for one failed input. |
| `nwnrs.inlayHints.enumValues` | `true` | Show automatic integer enum values. |
| `nwnrs.inlayHints.parameterNames` | `literals` | Show parameter hints for `off`, literal arguments, or `all` arguments. |

## Current limits

- The bundled native compiler currently supports macOS Apple Silicon only.
- NSS completion, signature help, formatting, event-specific views, and VS
  Code-host integration tests remain future work.
- Vanilla constants without adjacent source comments navigate correctly but
  intentionally have no fabricated hover description.

## Verify

```sh
npm run build-native
npm run check
npm test
```
