# Bevy Integration Project Plan

## Goal

Build a root-level Bevy integration crate that can load Neverwinter Nights
models and textures from this workspace and convert them into Bevy-native mesh,
material, and image assets.

The immediate objective is not "render everything at once". The immediate
objective is to establish a clean pipeline:

1. parse NWN model and texture formats into typed Rust structures
2. normalize them into an engine-neutral intermediate representation
3. convert that representation into Bevy assets through a custom asset loader

## Desired End State

At the end of this effort, the workspace should support:

- reading NWN `mdl` into a typed scene/model representation
- reading NWN `tga`, `dds`, and `plt` into typed texture representations
- resolving model texture/material references against a resource manager
- converting NWN geometry into Bevy `Mesh`
- converting NWN texture/material state into Bevy `Image` and `StandardMaterial`
- loading NWN assets through a Bevy plugin and asset loader API

## Recommended Crate Layout

Create these crates in stages rather than all at once:

- `crates/mdl`
  Purpose: full MDL parser and typed NWN model AST/IR
- `bevy`
  Purpose: Bevy plugin, asset loader, and conversion layer

Inside `crates/bevy`, keep responsibilities separated:

- `loader.rs`
  AssetLoader implementations and resource resolution entrypoints
- `convert_mesh.rs`
  NWN geometry to Bevy mesh conversion
- `convert_material.rs`
  NWN material/texture state to Bevy material conversion
- `assets.rs`
  Bevy-facing asset structs
- `plugin.rs`
  Plugin registration and app wiring

## Phases

### Phase 1: Texture Parsing

Status: complete for `tga`, `dds`, and `plt`.

Objective: make the dedicated image crates own real typed parsing rather than raw bytes.

Deliverables:

- `TgaTexture` parser for uncompressed and RLE-compressed TGA variants used by NWN
- `DdsTexture` parser for NWN DDS headers, mip chains, decode, and encode
- `PltTexture` parser for NWN palette texture data with typed per-pixel `value` and `layer_id`
- conversion helpers to RGBA8 pixel buffers where possible

Notes:

- `DDS` is now owned directly in-workspace as an NWN-specific format crate
- `PLT` is intentionally stopped at typed parse/write; palette-driven rendering
  remains future work
- preserve raw bytes where lossless round-trip matters

Exit criteria:

- each texture format can be parsed into a typed structure
- `tga` and `dds` have image conversion paths suitable for later Bevy upload work
- `plt` has documented typed ownership boundaries

### Phase 2: Model Parsing

Objective: make `nwnrs-mdl` own real MDL parsing rather than raw bytes.

Status: in progress.

Current state:

- syntax-faithful ASCII MDL capture exists
- geometry blocks, animation blocks, node blocks, comments, and multiline payloads are preserved
- this is still source-level capture, not validated semantic model data yet

Deliverables:

- lexer/parser for ASCII MDL
- typed AST for nodes, geometry, animations, materials, emitters, and metadata
- lower-level validated model IR suitable for engine conversion
- texture/material reference extraction
- walkmesh and animation support scoped explicitly as supported or deferred

Recommended split:

- AST: syntax-faithful and close to source
- IR: validated, canonical, conversion-oriented representation

Exit criteria:

- a representative NWN model can be parsed without stringly typed traversal
- mesh-bearing geometry and material references are accessible in a stable API

### Phase 2A: Semantic Lowering

Objective: convert the captured ASCII MDL surface into a validated,
engine-neutral NWN model representation.

Why this matters:

- parsing alone preserves source text structure, but rendering needs typed meaning
- Bevy conversion should consume validated model data, not raw statements
- animation playback needs stable channel data keyed to resolved nodes

Deliverables:

- typed classification of node kinds such as `dummy`, `trimesh`, `skin`, `emitter`, and light/camera variants
- typed extraction of model header data such as `newmodel`, `setsupermodel`, `classification`, and animation scale
- typed geometry extraction for:
  - parent links
  - transforms
  - mesh vertex positions
  - UVs
  - faces
  - material/texture names
  - render flags and relevant metadata
- typed animation extraction for:
  - animation headers
  - event tracks
  - per-node transform channels
  - animmesh payloads such as `animverts` and `animtverts`
- validation rules for:
  - duplicate node names
  - broken parent references
  - malformed keyed payload sizes
  - animation channels targeting unknown nodes

Recommended output shape:

- `ModelAst`
  Source-faithful parsed form, close to the authored ASCII
- `ModelSemantic`
  Validated NWN model data with typed nodes, typed meshes, and typed animations

Exit criteria:

- callers can query typed mesh, material, and animation data without parsing raw statement strings
- animation channels are attached to known nodes in a stable representation
- lowering reports diagnostics for malformed source instead of silently dropping data

### Phase 3: Resource Resolution Layer

Objective: connect parsed models to texture and related asset lookups.

Deliverables:

- resolver utilities that map model texture names to NWN resources
- rules for extension fallback and search order
- support for loading through `ResMan`
- diagnostics for missing texture/material references

Notes:

- this is where NWN-specific lookup behavior should live, not inside Bevy
- keep this reusable so CLI and tests can use it without Bevy

Exit criteria:

- model conversion code can request textures/material references through a
  single resolver API

### Phase 4: Engine-Neutral Scene Representation

Objective: define a stable bridge between NWN parsing and Bevy conversion.

Deliverables:

- `NwnScene`, `NwnMesh`, `NwnPrimitive`, `NwnMaterial`, and `NwnTextureRef`
- explicit coordinate-system and transform rules
- explicit vertex attribute layout
- explicit animation channel representation

Why this matters:

- it prevents the parser from depending on Bevy types
- it keeps Bevy integration replaceable
- it makes conversion testing much easier

Exit criteria:

- parser output can be lowered into a scene graph without Bevy imports

### Phase 5: Bevy Crate and Asset Loader

Objective: build the root Bevy integration crate.

Deliverables:

- `nwnrs-bevy` crate under `crates/bevy`
- `NwnBevyPlugin`
- custom Bevy asset types for loaded NWN models/scenes
- `AssetLoader` implementation that reads `.mdl`
- dependency hooks for texture resolution through the workspace crates

Recommended asset boundary:

- load `.mdl` as a custom Bevy asset first
- then spawn or convert into Bevy `Scene`, `Mesh`, `Image`, and materials
- avoid collapsing parsing, resolution, and spawning into one loader type

Exit criteria:

- Bevy app can load an NWN model asset and produce visible mesh/material output

### Phase 6: Material and Mesh Conversion

Objective: make converted assets render correctly enough to be useful.

Deliverables:

- geometry triangulation and vertex buffer generation
- normals, UVs, tangents where available or derivable
- texture-to-material mapping
- alpha, cutout, double-sided, and unlit policy decisions
- basic animation path if model support is ready

Notes:

- start with a narrow supported material subset and state that explicitly
- it is better to have one correct path than partial support for every feature

Exit criteria:

- a curated sample set renders acceptably in Bevy

## Milestones

### Milestone A

Complete.

Typed texture parsing complete, with image conversion for `tga` and `dds`, and
documented `plt` behavior.

### Milestone B

Typed model capture complete, plus semantic lowering for geometry, materials,
and animation data.

### Milestone C

Intermediate NWN scene model defined and stable enough for conversion tests.

### Milestone D

Bevy loader crate can load one NWN model with resolved textures into visible
meshes and materials.

## Suggested Implementation Order

1. treat the completed texture crates as the stable image input layer
2. finish source-faithful ASCII MDL capture
3. add semantic lowering for model headers, nodes, meshes, and animations
4. define engine-neutral scene/material IR
5. add resource resolution helpers
6. create `crates/bevy`
7. implement Bevy image/material conversion
8. implement Bevy mesh conversion
9. add sample app and regression fixtures

This order keeps Bevy off the critical path until the NWN-side data model is
stable.

## Near-Term Todo

The next concrete steps from the current repo state are:

- `nwnrs-mdl`: split the current ASCII capture layer into an explicit source AST module
- `nwnrs-mdl`: introduce typed semantic structs for model header, nodes, meshes, materials, and animations
- `nwnrs-mdl`: add lowering from ASCII statements into typed header fields
- `nwnrs-mdl`: add lowering for geometry node transforms and parent relationships
- `nwnrs-mdl`: add lowering for mesh data such as `verts`, `tverts`, `faces`, and texture/material references
- `nwnrs-mdl`: add lowering for animation headers: `length`, `transtime`, `animroot`, and `event`
- `nwnrs-mdl`: add lowering for animation node channels: `positionkey`, `orientationkey`, `scalekey`, and similar tracks
- `nwnrs-mdl`: decide and document quaternion/orientation representation without normalizing away authored values too early
- `nwnrs-mdl`: add diagnostics for duplicate nodes, unknown parents, unknown animation targets, and malformed payload row widths
- `nwnrs-mdl`: add real-fixture tests covering:
  - static mesh-heavy model
  - animation-heavy model
  - animmesh cases
  - intentionally malformed MDL
- `nwnrs-mdl`: expose stable query helpers so downstream conversion code never needs raw statement traversal
- after that, define the engine-neutral scene IR that Bevy conversion will consume

## Direct Answer

Yes. Semantic lowering is the step that makes Bevy rendering practical.

The chain is:

1. capture the authored MDL faithfully
2. lower it into validated typed model data
3. lower that into an engine-neutral scene/material/animation IR
4. convert that IR into Bevy assets

Without step 2, the Bevy side would end up parsing ad hoc strings and
re-implementing MDL rules in the renderer layer, which is exactly what we want
to avoid.

## Testing Strategy

Use layered tests from the start:

- parser unit tests with small hand-authored fixtures
- golden tests against real NWN assets
- IR lowering tests
- conversion tests for mesh/material output shape
- Bevy integration smoke tests for the asset loader

Recommended fixture buckets:

- tiny static mesh
- textured static mesh
- animated model
- alpha texture
- palette texture
- intentionally malformed files

## Risks and Unknowns

### MDL complexity

The model format is the main risk. Keep the parser incremental:

- static geometry first
- animation second
- exotic nodes later

### PLT conversion

Palette textures are NWN-specific and may not map directly to Bevy materials.
This likely needs a custom decode step or shader strategy.

### Material fidelity

Bevy `StandardMaterial` may not capture all NWN material behavior. Plan for a
usable subset first, then decide whether a custom material pipeline is needed.

### Resource resolution

Model-to-texture resolution will fail in confusing ways if it is not explicit
and testable. Treat this as a first-class subsystem.

## First Concrete Tasks

When implementation begins, start here:

1. treat `nwnrs-tga`, `nwnrs-dds`, and `nwnrs-plt` as the completed texture input layer
2. collect a small real-world fixture set for `tga`, `dds`, `plt`, and `mdl`
3. only then move on to the next format and the scene bridge work

## Non-Goals For The First Pass

Do not block the first usable Bevy loader on:

- perfect material fidelity
- full animation coverage
- every NWN node type
- editor tooling
- round-trip writing for Bevy-generated assets

## Success Criteria For V1

V1 is successful if:

- a representative NWN static model parses into typed structures
- referenced textures resolve through the workspace
- the Bevy plugin loads that model into visible meshes and materials
- failure modes for unsupported model/material features are explicit
