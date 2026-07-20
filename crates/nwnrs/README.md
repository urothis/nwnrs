# nwnrs

`nwnrs` is the command-line frontend and native server launcher for this
workspace.

It is the tool you use when you want to inspect, convert, scaffold, pack,
unpack, or package Neverwinter Nights resources without writing Rust code
against the lower-level crates directly.

On docs.rs, the public Rust API for this crate is intentionally small. The main
value here is operational behavior: which commands exist, what workflows they
cover, and how they fit together with `nwnrs-types`, `nwnrs-nwscript`, and
`nwnrs-nwpkg`.

## What This Crate Does

`nwnrs` exposes these high-level workflows behind one executable:

- inspect typed NWN resources and archives
- convert between authored and compiled representations such as `MDL`, `OBJ`,
  and image formats
- scaffold `nwproject` workspaces for resource, ERF, and KEY/BIF packaging
- pack source trees back into distributable resources and archives
- unpack existing resources and archives into editable source trees
- work with `NWSync` manifests and repositories
- build slim install-backed KEY/BIF package views for deployment workflows
- identify and launch native macOS, Linux, or Windows servers with the exact matching
  nwnrs runtime target pack

## Quick Start

Install from the repository:

```bash
cargo install --git https://github.com/urothis/nwnrs --bin nwnrs
```

Build or run from the workspace root:

```bash
cargo run -p nwnrs -- new --kind utc my_creature
cargo run -p nwnrs -- init --kind mod
cargo run -p nwnrs -- inspect path/to/module.mod
cargo run -p nwnrs -- convert path/to/model.mdl out/model_ascii.mdl
cargo run -p nwnrs -- convert out/model_ascii.mdl rebuilt/model.mdl
cargo run -p nwnrs -- convert path/to/model.mdl out/model.obj
cargo run -p nwnrs -- convert --root /path/to/NWN --user /path/to/NWN path/to/creature.utc out/creature.obj
cargo run -p nwnrs -- unpack path/to/module.mod -d out/
cargo run -p nwnrs -- pack out/ rebuilt.mod
cargo run -p nwnrs -- compile -g -o rebuilt/script.ncs path/to/script.nss
cargo run -p nwnrs -- compile -R -d rebuilt/scripts scripts/
cargo run -p nwnrs -- compile -R -d rebuilt/scripts --graphviz graphs --graphviz-format svg scripts/
cargo run -p nwnrs -- compile --optimization O0 --optimization-flag remove-dead-branches path/to/script.nss
cargo run -p nwnrs -- pack --include-dir path/to/includes --optimization O2 scripts/ rebuilt.mod
cargo run -p nwnrs -- unpack path/to/script.ncs -d out/
cargo run -p nwnrs -- pack out/ rebuilt.ncs
cargo run -p nwnrs -- pack nwn_base.key docker/data/data
cargo run -p nwnrs -- nwsync print path/to/repository --manifest <sha1>
cargo run -p nwnrs -- nwsync fetch https://example.com/manifest/abc123 -o repo/
cargo run -p nwnrs -- nwsync prune path/to/repository --dry-run
cargo run -p nwnrs -- nwsync prune path/to/repository
cargo run -p nwnrs -- nwsync write path/to/resources/ output.manifest
cargo run -p nwnrs -- run --runtime target/debug/libnwnrs_runtime_sys.dylib --targets crates/runtime/targets -- /path/to/nwserver -module module_name
```

The equivalent Windows runtime path is
`target\debug\nwnrs_runtime_sys.dll`, and the server path ends in
`nwserver.exe`.

The default build enables both Cargo features:

- `tooling` provides the resource, package, compiler, and NWSync commands.
- `supervisor` provides the native `run` hypervisor.

Build only the supervisor for a deployment image without pulling the resource
toolchain into the executable:

```bash
cargo build --release --package nwnrs \
  --no-default-features --features supervisor
```

The feature split is additive. Normal installs retain the complete CLI and its
host-side Docker mode, while supervisor-only builds expose only the native
`nwnrs run` interface used inside the container. A build with neither feature
is rejected.

## Command Overview

### `new` and `init`

These commands scaffold project directories with:

- `nwproject.toml`
- `nwpkg.lock`
- starter source content appropriate for the requested kind

Use `new` when you want a fresh directory. Use `init` when you want to turn an
existing directory into an `nwproject`.

### `inspect`

`inspect` reads a resource or archive and prints a human-readable view of its
structure. This is the fast path for understanding what is inside an unknown
`GFF`, `ERF`, `KEY`, `TLK`, `NCS`, or related file.

### `convert`

`convert` handles format-to-format workflows that are operationally useful but
do not fit simple pack/unpack semantics, such as:

- compiled `MDL` to canonical ASCII `MDL`
- ASCII `MDL` to a newly compiled binary `MDL`
- `MDL` to flattened `OBJ`
- image conversion into texture-oriented formats
- install-backed creature appearance export using model and texture resolution

Compiled-to-ASCII conversion omits embedded source bytes by default. Pass
`--preserve-compiled-source` when byte-exact restoration of an unchanged ASCII
conversion is worth the additional comment payload.

### `pack`

`pack` turns project source trees or individual authored resources back into
their packaged output forms.

That includes:

- compiling `.nss` into `.ncs` as a compatibility convenience
- packing directories into ERF-family archives such as `hak`, `mod`, and `nwm`
- rebuilding KEY/BIF sets
- preserving original archive ordering and reuse opportunities when
  `nwpkg.lock` metadata allows it

### `compile`

`compile` is the dedicated NWScript frontend. It accepts individual `.nss`
files or directories and writes `.ncs`, optional `.ndb`, and optional Graphviz
syntax-tree artifacts. Directory inputs can be recursive and compilation can
run in parallel, continue after errors, or simulate without writing files.
Recursive builds preserve the source hierarchy beneath both the artifact and
Graphviz output directories, preventing same-stem scripts in different folders
from colliding.

For a single input, `-o` honors the supplied relative or absolute output path
exactly, including its extension. Debug output uses the same path with an
`.ndb` extension.

Source lookup is local-first: the input directory and repeated `--include-dir`
roots override resources from the standard NWN installation. Installation and
user roots are autodetected, with `--root`, `--user`, `--language`, and
`--load-ovr` available for explicit control.

The default optimization preset is safe O1. `--optimization O0` through `O3`
select presets. Repeated `--optimization-flag` values select an exact custom
set and override the preset; accepted values are `remove-dead-code`,
`remove-dead-branches`, and `meld-instructions`.

Use `-g`/`--debug` for NDB output and `--max-include-depth` to control include
traversal. `--graphviz DIR` writes one styled syntax-tree image per script,
preserving directory hierarchy for recursive builds. SVG is the default;
`--graphviz-format png`, `pdf`, or `dot` selects another format, and
`--keep-graphviz-dot` retains DOT source alongside rendered images. SVG, PNG,
and PDF rendering require the Graphviz `dot` executable. Recompiling without
debug output removes a stale sibling NDB.

`inspect` uses the same installation-backed `nwscript.nss` lookup for NCS
action names and falls back to installed NSS resources when weaving NDB source
references. Use its matching `--root`, `--user`, `--language`, and `--load-ovr`
controls when autodetection is not appropriate.

### `unpack`

`unpack` takes an existing packaged resource and expands it into an editable
layout. The unpacked result is shaped so that a later `pack` can reconstruct
the original output with as much fidelity as possible.

### `nwsync`

The `nwsync` command family provides repository and manifest utilities:

- print a manifest
- fetch a manifest or repository state
- prune unused repository content
- write a manifest from a resource set

### `run`

`run` is the native macOS, Linux, and Windows launcher. It computes the
complete server SHA-256, reads its Mach-O, ELF, or PE architecture, selects
only the exact matching target pack, validates the runtime library
architecture, and supervises the server process. macOS uses
`DYLD_INSERT_LIBRARIES`; Linux uses `LD_PRELOAD`. Windows creates the server
suspended, loads and initializes the matching runtime DLL, and resumes the
primary thread only after initialization succeeds.

On Windows, `run` is headless by default: NWServer retains its hidden native
window and message loop for compatibility while launcher and server output is
rendered in the terminal. Pass `--gui` to show the native control panel.

When enabled, the runtime themes the native NWServer control panel. The
title bar, background, labels, inputs, lists, buttons, checkboxes, combo boxes,
and numeric spinners use a dark palette; orange is limited to the window
border, focus/checked states, dropdown and spinner glyphs, and a two-pixel
client accent line. Painting is control-local and leaves the original native
input and command behavior intact.

The launcher mirrors new server-log output to its terminal. macOS and Linux
follow `logs.0/nwserverLog1.txt` and `logs.0/nwserverError1.txt`. Windows uses
its native `logs` directory and follows `nwserverLog1.txt`,
`nwserverError1.txt`, and the more detailed `nwengineLog.txt`. It cleans up the
log followers and returns the server's exit status.

On Unix the launcher forwards `TERM` and `HUP` to the server.
Terminal `INT` (`Ctrl-C`) requests a clean NWServer shutdown. On Unix this
sends the native interactive `quit` command; on Windows it posts `WM_QUIT` to
the server's primary thread. Typing `quit` at the Windows launcher prompt uses
the same path. A second shutdown request forces the server to terminate.
Interactive terminal input is otherwise proxied directly to NWServer. The
launcher obtains the log root from the last forwarded
`-userdirectory` option, or uses normal NWN user-directory discovery when that
option is absent. Pass `--no-tail-logs` before `--` to disable log mirroring.

Launcher messages, NWServer console output, and followed server logs use
structured `tracing` levels. NWServer console lines use `nwnrs::console`;
normal server-log lines are `INFO`; error-log lines are `ERROR`; supervision
details are `DEBUG`; forced shutdowns are `WARN`; and the injected runtime uses
`nwnrs::runtime` for initialization and bridge diagnostics. NWScript messages
emitted by `NWNRS_Log` use `nwnrs::script` and retain their requested level.
Configure filtering with `RUST_LOG`, for example:

```bash
RUST_LOG=nwnrs::launcher=debug,nwnrs::console=info,nwnrs::server=info,nwnrs::runtime=info,nwnrs::script=debug nwnrs run ...
```

Color defaults to automatic terminal detection and honors `NO_COLOR`. Use
`--color always` or `--color never` before the server path to override it. The
launcher owns final rendering for both NWServer and injected-runtime output, so
each emitted line uses the same color policy.

Both `--runtime` and `--targets` are explicit. Put `--` before the server path
when passing server options so they are forwarded without interpretation:

```bash
nwnrs run \
  --runtime path/to/libnwnrs_runtime_sys.dylib \
  --targets crates/runtime/targets \
  -- /path/to/nwserver -module module_name -userdirectory path/to/server-home
```

The full/default CLI can instead start the published Linux image through the
host Docker daemon:

```bash
nwnrs run --docker
```

This defaults to the development tag `nwserver:local`, publishes
`5121:5121/udp`, and mounts the `nwserver-home` volume at `/nwn/home`. Registry
pulling is disabled by default, so a missing local tag fails instead of
contacting Docker Hub. Registry images are selected explicitly with
`--docker-image` and `--docker-arg pull=always`. Common overrides and exact
server arguments can be supplied without changing the image:

```bash
nwnrs run --docker \
  --docker-name my-server \
  --docker-publish 127.0.0.1:5121:5121/udp \
  --docker-arg pull=always \
  -- -module custom
```

Use `--docker-image` for another image, `--docker-home` for another named
volume or host path, and repeat `--docker-publish` or `--docker-arg` as needed.
Docker arguments are long options written without their leading dashes, such
as `--docker-arg env=NWN_MODULE=custom`; nwnrs adds the two leading dashes
before invoking Docker.
When any publish option is supplied it replaces the default mapping. The full
CLI replaces itself with the local Docker client so attached input, terminal
behavior, signals, daemon compatibility, and the final exit status stay under
Docker's control.

Docker mode is compiled only when both the `supervisor` and `tooling` features
are present. The supervisor-only executable inside the image has no Docker
flag, Docker dependency, or Docker-in-Docker behavior.

Host Docker mode applies a read-only root filesystem, drops all Linux
capabilities, enables `no-new-privileges`, and mounts writable tmpfs instances
at `/nwn/run` and `/tmp`. These defaults can still be replaced by invoking
Docker directly when an unusual deployment requires a different policy.

The Docker image builds this supervisor-only executable and
`nwnrs-runtime-sys` in a repository-root multi-stage build. The shell-free
distroless entrypoint invokes the internal `nwnrs run --container` mode, which
owns volume/configuration setup, process supervision, runtime injection,
target-pack selection, signal handling, crash-log preservation, and native
server-log following.

Once injected, the runtime installs the NWScript bridge described by the
source-controlled `module/nwnrs.nss` include. It reports runtime identity,
server state, and active module/area/object event context, and exposes validated
administration operations such as session settings, ban lists, graceful
shutdown, rules reload, TURD recovery, and deferred server-vault character
deletion. No HTTP or metrics service is started.

## Common Workflows

- Compile one script with `compile -o`, or compile a source tree with
  `compile -R -d`.
- Scaffold a new resource project with `new` or `init`, edit the source file,
  then run `pack`.
- Unpack a module or hak, edit the unpacked sources, then `pack` the directory
  back into an archive.
- Unpack raw `.ncs` to `.ncs.asm`, edit it, and `pack` it back into bytecode.
- Lower compiled `MDL` files to canonical ASCII with `convert` for inspection,
  edit them, and compile the ASCII back to binary with another `convert` call.
- Export static or install-backed model geometry to `OBJ` for tooling or asset
  inspection.
- Package an install-backed resource view into a slim KEY/BIF set for server or
  deployment scenarios.

## Project Files

`nwnrs` relies on the `nwnrs-nwpkg` crate for project control files:

- `nwproject.toml` describes project identity, kind, and source root
- `nwpkg.lock` stores repack metadata that helps preserve archive structure
  and reuse original content when possible

That metadata is what makes unpack-edit-repack workflows more faithful than a
blind “read everything and write everything from scratch” loop.

## Relationship To The Other Crates

- [`nwnrs-types`](https://docs.rs/nwnrs-types/latest/nwnrs_types/) provides the
  actual typed resource readers, writers, install helpers, and resource
  management layers.
- [`nwnrs-nwscript`](https://docs.rs/nwnrs-nwscript/latest/nwnrs_nwscript/)
  provides the NWScript frontend, compiler, debugger-oriented VM, and
  associated language machinery.
- [`nwnrs-nwpkg`](https://docs.rs/nwnrs-nwpkg/latest/nwnrs_nwpkg/) provides the
  typed `nwproject` manifest and lockfile model used by the packaging flows.
- [`nwnrs-runtime`](https://docs.rs/nwnrs-runtime/latest/nwnrs_runtime/)
  provides executable identification and exact target-pack validation.

`nwnrs` is the operational shell that composes those crates into one CLI.

## Rust API Surface

The library API is intentionally minimal. The main public entrypoint is:

- [`main_entry`](crate::main_entry), which runs the CLI process and returns one
  process exit code

This crate is primarily documented as a tool, not as a reusable library.

## Internal Layout

The command implementations live in:

- [`args.rs`](./src/args.rs)
- [`compile.rs`](./src/compile.rs)
- [`convert.rs`](./src/convert.rs)
- [`inspect.rs`](./src/inspect.rs)
- [`nwsync.rs`](./src/nwsync.rs)
- [`pack.rs`](./src/pack.rs)
- [`package.rs`](./src/package.rs)
- [`project.rs`](./src/project.rs)
- [`run.rs`](./src/run.rs)
- [`unpack.rs`](./src/unpack.rs)

The library and binary entrypoints live in:

- [`lib.rs`](./src/lib.rs)
- [`main.rs`](./src/main.rs)
