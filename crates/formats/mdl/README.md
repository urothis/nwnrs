# nwnrs-mdl

`nwnrs-mdl` provides the model-facing portion of the workspace: reading, writing, lowering, and exporting Neverwinter Nights `MDL` model assets.

## Scope

- read and write Neverwinter Nights `MDL` payloads
- expose syntax-faithful ASCII and compiled-model parsing
- lower models into richer semantic and scene-oriented representations
- rewrite appearance-token slots before texture and model resolution
- resolve equipped player-creature part attachments into composed scene trees
- export scenes or composed scene trees as flattened Wavefront `OBJ`
- write semantic and scene-oriented representations back as canonical ASCII
- support inspection at multiple abstraction levels rather than only one
  canonical model

Choose the entry point that matches the fidelity you need rather than treating
`MDL` as a single monolithic parser.

## Layered Public Surface

### Authored ASCII layer

- `AsciiModel`
- `AsciiAnimation`
- `AsciiNode`
- `AsciiStatement`
- `AsciiElement`
- `AsciiBodyItem`
- `AsciiPayloadKind`
- `parse_ascii_model`
- `read_ascii_model`
- `write_ascii_model`

### Compiled binary layer

- `BinaryModel`
- `BinaryHeader`
- `BinaryNode`
- `BinaryNodeContent`
- `BinaryMesh`
- `BinarySkin`
- `BinaryAnimMesh`
- `BinaryDangly`
- `BinaryEmitter`
- `BinaryEmitterFlags`
- `BinaryLight`
- `BinaryAnimation`
- `BinaryController`
- `BinaryReference`
- `BinaryFace`
- `BinaryUvSet`
- `BinaryAabb`
- `BinaryAabbEntry`
- `BinaryArrayDefinition`
- `UnknownBinaryBlock`
- `parse_binary_model_bytes`
- `read_binary_model`
- `write_binary_model`

### Semantic and scene layers

- `SemanticModel`
- `SemanticNode`
- `SemanticAnimation`
- `NwnScene`
- `NwnSceneNode`
- `NwnPrimitive`
- `NwnAnimation`
- `lower_semantic_model_to_scene`
- `parse_scene_model`
- `read_scene_model`
- `write_scene_model`

### Composition and export

- `NwnAppearanceOverrides`
- `collect_appearance_slots`
- `apply_appearance_overrides`
- `resolve_scene_textures`
- `NwnComposedScene`
- `compose_player_creature_from_resman`
- `compose_player_creature_from_utc`
- `write_scene_obj`
- `write_composed_scene_obj`

### Cross-layer entry points

- `Model`
- `ParsedModel`
- `ModelEncoding`
- `ModelClassification`
- `MODEL_RES_TYPE`
- `detect_model_encoding`
- `parse_model_bytes`
- `read_model`
- `write_model`
- `compile_ascii_model`
- `lower_ascii_model`
- `lower_binary_model_to_ascii`

## Representation Pipeline

```text
ASCII MDL -----------+
                     |
                     v
                semantic model ------> scene model ------> composed scene -----> OBJ
                     ^
                     |
binary MDL ----------+
```

## Invariants

- lower-level representations retain enough authored structure to support
  higher-level lowering without reparsing raw bytes
- scene and semantic layers make normalization explicit instead of hiding it
- model references, helper data, and material-facing metadata remain first-class
  concepts where the corresponding layer supports them
- higher-level writers canonicalize through ASCII and do not preserve original
  authored formatting or compiled bytes
- ASCII, binary, semantic, scene, composed-scene, and `OBJ` export preserve
  different information on purpose

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
- `appearance`: appearance-slot discovery and override application
- `compose`: install-backed player-creature composition helpers
- `obj`: flattened Wavefront OBJ export
- `io` and `types`: typed read and write entry points and shared vocabulary

## See also

- [`nwnrs-mtr`](https://docs.rs/nwnrs-mtr), which parses material descriptors
  referenced by MDL materials
- [`nwnrs-txi`](https://docs.rs/nwnrs-txi), which parses texture sidecar
  metadata often consumed with MDL assets
- [`nwnrs-plt`](https://docs.rs/nwnrs-plt), which stores the recolorable
  palette-layer textures used for creature appearance overrides
- [`nwnrs-resman`](https://docs.rs/nwnrs-resman), which provides the resource
  layer used by install-backed model and texture resolution
