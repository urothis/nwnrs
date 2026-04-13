# nwnrs-mdl

`nwnrs-mdl` provides the model-facing portion of the workspace.

## Scope

- read and write Neverwinter Nights `MDL` payloads
- expose syntax-faithful ASCII and compiled-model parsing
- lower models into richer semantic and scene-oriented representations
- support inspection at multiple abstraction levels rather than only one
  canonical model

Choose the entry point that matches the fidelity you need rather than treating
`MDL` as a single monolithic parser.

## Invariants

- lower-level representations retain enough authored structure to support
  higher-level lowering without reparsing raw bytes
- scene and semantic layers make normalization explicit instead of hiding it
- model references, helper data, and material-facing metadata remain first-class
  concepts where the corresponding layer supports them

## Non-goals

- define engine-specific rendering policy
- collapse every authored MDL distinction into one flattened scene structure

## Internal Structure

- `ascii`: syntax-faithful ASCII parsing and typed source representation
- `binary`: compiled-model parsing for binary MDL payloads
- `semantic`: validated lowering from authored model syntax into typed NWN model
  concepts
- `scene`: engine-neutral scene lowering for rendering or tooling integrations
- `resolve`: texture and material-reference resolution helpers
- `io` and `types`: typed read/write entry points and shared vocabulary

## See also

- [`nwnrs-mtr`](https://docs.rs/nwnrs-mtr), which parses material descriptors
  referenced by MDL materials
- [`nwnrs-txi`](https://docs.rs/nwnrs-txi), which parses texture sidecar
  metadata often consumed with MDL assets
