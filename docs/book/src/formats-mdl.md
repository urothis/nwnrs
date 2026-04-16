# MDL Models

Docs:

- [crate docs](https://docs.rs/nwnrs-mdl/latest/nwnrs_mdl/)
- [README](https://github.com/urothis/nwnrs/blob/main/crates/formats/mdl/README.md)

`MDL` is the most structurally layered format family in the workspace. Treating
it as one representation would be a category error.

## Layered Public Surface

### Authored ASCII Layer

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

### Compiled Binary Layer

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

### Semantic Layer

- `SemanticModel`
- `SemanticHeader`
- `SemanticNode`
- `SemanticMesh`
- `SemanticSkinWeight`
- `SemanticAnimation`
- `SemanticAnimationNode`
- `SemanticMaterial`
- `SemanticTextureBinding`
- `SemanticEmitter`
- `SemanticEmitterProperty`
- `SemanticLight`
- `SemanticReference`
- `SemanticPropertyValue`
- `parse_semantic_model`
- `parse_semantic_model_auto`
- `read_semantic_model`
- `read_semantic_model_auto`
- `write_semantic_model`

### Scene Layer

- `NwnScene`
- `NwnSceneNode`
- `NwnPrimitive`
- `NwnMesh`
- `NwnFace`
- `NwnSkinWeight`
- `NwnAnimation`
- `NwnNodeAnimationTrack`
- `NwnTransformTrack`
- `NwnMaterial`
- `NwnMaterialTrack`
- `NwnTextureRef`
- `NwnTextureSlot`
- `NwnUvSet`
- `NwnLight`
- `NwnEmitter`
- `NwnEmitterProperty`
- `NwnReference`
- `NwnTransform`
- `NwnCoordinateSystem`
- `NwnVec2Sample`
- `NwnVec3Sample`
- `ScalarKey`
- `Vec3Key`
- `Vec4Key`
- `lower_semantic_model_to_scene`
- `parse_scene_model`
- `parse_scene_model_auto`
- `read_scene_model`
- `read_scene_model_auto`
- `write_scene_model`

### Appearance, Resolution, Composition, Export

- `NwnAppearanceOverrides`
- `NwnAppearanceSlot`
- `collect_appearance_slots`
- `apply_appearance_overrides`
- `TextureResolverOptions`
- `TextureResourceKind`
- `ResolvedTexture`
- `UnresolvedTexture`
- `ResolvedMaterialTextures`
- `SceneTextureResolution`
- `resolve_texture_ref`
- `resolve_scene_texture_ref`
- `resolve_scene_texture_ref_with_policy`
- `resolve_scene_textures`
- `resolve_material_textures`
- `NwnComposedScene`
- `NwnSceneAttachment`
- `compose_player_creature_from_resman`
- `compose_player_creature_from_utc`
- `load_composed_scene_from_resman`
- `write_scene_obj`
- `write_composed_scene_obj`

### Cross-Layer Entry Points

- `Model`
- `ParsedModel`
- `ModelEncoding`
- `ModelClassification`
- `NodeKind`
- `ModelDiagnostic`
- `ModelDiagnosticKind`
- `MODEL_RES_TYPE`
- `detect_model_encoding`
- `parse_model_bytes`
- `read_model`
- `read_parsed_model`
- `write_model`
- `write_parsed_model`
- `compile_ascii_model`
- `lower_ascii_model`
- `lower_binary_model_to_ascii`
- `bake_scene_pose`
- `bake_composed_scene_pose`
- `default_scene_animation`
- `find_scene_animation`
- `scene_animation_names`
- `composed_scene_animation_names`
- `sample_scene_animation`
- `sample_scene_default_animation`
- `sample_composed_scene_animation`
- `sample_composed_scene_default_animation`

## Representation Pipeline

The important diagram for `MDL` is not one exact byte layout. It is the stack of
representations:

```text
ASCII MDL -----------+
                     |
                     v
                semantic model ------> scene model ------> composed scene -----> OBJ
                     ^
                     |
binary MDL ----------+
```

Another way to read it:

```text
authored syntax   != compiled storage != semantic meaning != scene graph != export mesh
```

## Logical Edges

- `MDL` is intentionally not collapsed into one universal representation.
- ASCII, binary, semantic, scene, composed-scene, and `OBJ` export all
  preserve different information.
- Higher-level writers canonicalize through normalized forms and do not promise
  authored byte preservation.
- Texture/material resolution and appearance rewriting are separate concerns
  from parsing.
- Composition reconstructs install-backed creature assembly rather than just
  exposing one file in isolation.

## Why This Crate Exists

`MDL` is where reverse engineering stops being "define structs that match the
bytes" and becomes "define a sound lattice of representations." There is no
single correct universal model because different tasks need different fidelity:

- byte-accurate compiled analysis
- authored ASCII tooling
- semantic graph reasoning
- scene export and downstream interchange

The crate exposes those layers explicitly so each operation states what
information it preserves and what information it intentionally discards.
