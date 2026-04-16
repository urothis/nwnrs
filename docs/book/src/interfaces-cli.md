# Command-Line Interface

Docs:

- crate: `nwnrs-cli`
- [README](https://github.com/urothis/nwnrs/blob/main/cli/README.md)
- [source tree](https://github.com/urothis/nwnrs/tree/main/cli/src)

## Scope

`nwnrs-cli` is the operational shell over the workspace. It is where the library layers become concrete workflows.

## Command Surface

- `inspect`
- `compile`
- `convert`
- `pack`
- `unpack`
- `nwsync`

## Logical Edges

- The CLI is not supposed to be a second implementation of the library semantics.
- The interesting work happens in the lower crates; the CLI's role is orchestration and workflow exposure.
- Several commands cross crate families: `convert` touches models, textures, installs, and resource resolution; `compile` touches source loading, language specs, and codegen.

## Why This Interface Exists

The CLI is the shortest path from "I know the workspace exists" to "I can do real work with it." It is also the easiest place to observe how the lower layers compose in practice.
