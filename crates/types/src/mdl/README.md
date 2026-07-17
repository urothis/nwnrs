# nwnrs-types

`nwnrs-types` provides the model-facing portion of the workspace: reading, writing, lowering, and exporting Neverwinter Nights `MDL` model assets.

## Why This Crate Exists

NWN ships two physically different MDL encodings — ASCII and compiled binary —
and tooling needs both. A single-layer parser forces every consumer to either
pick one encoding or carry its own lowering logic. This crate provides a layered
pipeline from raw bytes through semantic and scene representations so tools can
choose the fidelity they need without reimplementing each other's lowering.

## Scope

- read and write Neverwinter Nights `MDL` payloads
- expose syntax-faithful ASCII and compiled-model parsing
- lower models into richer semantic and scene-oriented representations
- rewrite appearance-token slots before texture and model resolution
- resolve equipped player-creature part attachments into composed scene trees
- resolve supermodel chains and inherit remapped animation tracks
- sample transform, material, light, animmesh, emitter, and danglymesh channels
- assemble renderer-neutral effective materials from MDL, MTR, TXI, and texture resources
- export scenes or composed scene trees as flattened Wavefront `OBJ`
- write semantic and scene-oriented representations back as canonical ASCII
- support inspection at multiple abstraction levels rather than only one
  canonical model

Choose the entry point that matches the fidelity you need rather than treating
`MDL` as a single monolithic parser.

## Quick Start

Parse authored ASCII, compile it to the Enhanced Edition binary layout, and
lower the immutable binary snapshot into an editable semantic model:

```rust
use nwnrs_types::mdl::{
    compile_ascii_model, lower_binary_model, parse_ascii_model,
};

let ascii = parse_ascii_model(
    "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent null
endnode
endmodelgeom demo
donemodel demo
",
)?;
let binary = compile_ascii_model(&ascii)?;
let semantic = lower_binary_model(&binary)?;

assert_eq!(binary.name(), "demo");
assert_eq!(semantic.geometry_name, "demo");
# Ok::<(), nwnrs_types::mdl::ModelError>(())
```

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
- `write_original_binary_model`

`BinaryModel` is deliberately a read-only parsed snapshot. Its accessors expose
the typed payload, while `write_original_binary_model` writes the exact source
bytes. Edit by lowering to `SemanticModel`, changing that value, and compiling
it into a new snapshot; mutating parsed binary fields would imply a serializer
that does not exist.

### Semantic and scene layers

- `SemanticModel`
- `SemanticNode`
- `SemanticAnimation`
- `SemanticController` and `SemanticControllerKey` for losslessly preserving
  compiled controllers whose engine meaning is unknown
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
- `resolve_scene_materials`
- `NwnComposedScene`
- `inherit_supermodel_animations`
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
- `compile_semantic_model`
- `compile_semantic_model_bytes`
- `validate_semantic_model`
- `restore_compiled_model` (restores reversible compiled-source metadata when
  exact original bytes are preferred over recompilation)
- `lower_ascii_model`
- `lower_binary_model`
- `lower_binary_model_to_ascii`
- `lower_binary_model_to_ascii_with_options`

## Representation Pipeline

```text
ASCII MDL ------> semantic model --compile--> binary MDL
                       |
                       v
                  scene model ------> composed scene -----> OBJ
                       ^
                       |
binary MDL ------------+
```

## Compiler Behavior

The compiled-model writer targets the Enhanced Edition binary format. It rejects
unknown or unrepresentable semantic input instead of silently discarding it.
Authored source-only annotations are accepted only where the corresponding
binary value is derived or has no runtime meaning.

Skin vertices are limited to the four strongest influences and renormalized,
matching the engine's fixed-width representation. Compilation also enforces the
EE limits of 64 skin bones, 65,535 generated GPU vertices, and 21,845 faces per
mesh.

Unknown compiled controller IDs remain available as opaque semantic controller
rows and can be re-emitted by `compile_semantic_model`. Canonical ASCII has no
numeric-controller syntax, so `write_semantic_model` and `write_scene_model`
reject models containing those opaque rows instead of silently dropping them.
Compiled-to-ASCII lowering applies the same check.

Diagnostics attached while lowering describe source provenance. Direct ASCII
compilation still rejects lossy parse diagnostics, but after a caller repairs a
`SemanticModel`, compilation and `validate_semantic_model` validate its current
fields rather than allowing stale diagnostics to reject the corrected value.

Compiled-to-ASCII lowering does not embed the original binary payload by
default. Set `BinaryToAsciiOptions::embed_original_binary` when calling
`lower_binary_model_to_ascii_with_options` if byte-exact restoration through
`restore_compiled_model` is required. The metadata is intentionally opt-in
because it can be large and is not meaningful to other MDL tools.

ASCII file and resource readers mirror Cleanmodels' byte-transparent behavior:
they do not require UTF-8, and arbitrary token/comment bytes survive a
read/write cycle. The `&str` parser remains available for programmatically
constructed text.

## Invariants

- lower-level representations retain enough authored structure to support
  higher-level lowering without reparsing raw bytes
- scene and semantic layers make normalization explicit instead of hiding it
- model references, helper data, and material-facing metadata remain first-class
  concepts where the corresponding layer supports them
- higher-level writers canonicalize through ASCII and do not preserve original
  authored formatting or compiled bytes
- opaque compiled controllers are binary-only unless and until their controller
  IDs gain a known ASCII property mapping
- ASCII, binary, semantic, scene, composed-scene, and `OBJ` export preserve
  different information on purpose

## Module Structure

- `mdl::ascii`: syntax-faithful ASCII parsing, writing, and source types
- `mdl::binary`: read-only compiled-model parsing and snapshot types
- `mdl::compiler`: validation and EE compiled-model generation
- `mdl::semantic`: editable typed NWN concepts and representation lowering
- `mdl::scene`: renderer-neutral scenes, animation sampling, and pose baking
- `mdl::runtime`: appearance overrides, resource resolution, and composition
- `mdl::export`: adapters such as flattened Wavefront OBJ output
- `mdl::raw`: raw payload, encoding detection, and format-dispatched I/O

The layer modules are public so applications can depend on a focused surface.
The existing flat `mdl::*` re-exports remain available for compatibility and
the prelude covers common cross-layer workflows. Semantic and scene values stay
publicly editable because they are the intended interchange representations;
only the parsed binary snapshot is immutable.

Export adapters operate over `SemanticModel` or `NwnScene`, depending on the
fidelity they need, without adding destination-specific fields to the core. MDL
remains one cohesive feature, including its resource-backed resolution and
composition workflows.

## Tests

The MDL integration-test harness is `crates/types/tests/mdl.rs`, with focused
modules under `crates/types/tests/mdl/`. The `game_corpus` module uses the
existing install test support and resource manager to sample shipped game MDLs;
it does not implement a separate asset loader.

## See also

- [`crate::mtr`], which parses material descriptors
  referenced by MDL materials
- [`crate::txi`], which parses texture sidecar
  metadata often consumed with MDL assets
- [`crate::plt`], which stores the recolorable
  palette-layer textures used for creature appearance overrides
- [`crate::resman`], which provides the resource
  layer used by install-backed model and texture resolution
