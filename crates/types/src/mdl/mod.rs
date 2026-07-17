#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

/// Source-faithful ASCII syntax, parsing, and writing.
pub mod ascii;
/// Read-only compiled-model snapshots and binary parsing.
pub mod binary;
/// Validated ASCII/semantic compilation to NWN:EE binary MDL.
pub mod compiler;
mod controllers;
/// Export adapters for external interchange formats.
pub mod export;
mod layout;
/// Raw model payload and format-neutral I/O.
pub mod raw;
/// Asset resolution, appearance, and composed-model workflows.
pub mod runtime;
/// Renderer-neutral scenes, animation sampling, and pose baking.
pub mod scene;
/// Typed semantic models and representation lowering.
pub mod semantic;

pub use ascii::*;
pub use binary::*;
pub use compiler::*;
pub use export::*;
pub use raw::*;
pub use runtime::*;
pub use scene::*;
pub use semantic::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::mdl::{
        AnimationEvent, AsciiAnimation, AsciiBodyItem, AsciiElement, AsciiModel, AsciiNode,
        AsciiPayloadKind, AsciiStatement, BinaryAabb, BinaryAabbEntry, BinaryAnimMesh,
        BinaryAnimation, BinaryArrayDefinition, BinaryController, BinaryDangly, BinaryEmitter,
        BinaryEmitterFlags, BinaryEvent, BinaryFace, BinaryHeader, BinaryLight, BinaryMesh,
        BinaryModel, BinaryNode, BinaryNodeContent, BinaryReference, BinarySkin,
        BinaryToAsciiOptions, BinaryUvSet, MODEL_RES_TYPE, Model, ModelClassification,
        ModelDiagnostic, ModelDiagnosticKind, ModelEncoding, ModelError, ModelResult, NodeKind,
        NwnAnimMeshTrack, NwnAnimation, NwnAppearanceOverrides, NwnAppearanceSlot,
        NwnComposedScene, NwnCoordinateSystem, NwnDangly, NwnEffectTrack, NwnEmitter,
        NwnEmitterController, NwnEmitterControllerTrack, NwnEmitterKey, NwnEmitterProperty,
        NwnFace, NwnLight, NwnMaterial, NwnMaterialTextureRole, NwnMaterialTextureSource,
        NwnMaterialTrack, NwnMesh, NwnNodeAnimationTrack, NwnPrimitive, NwnPropertyValue,
        NwnReference, NwnScene, NwnSceneAttachment, NwnSceneNode, NwnSkinWeight, NwnTextureRef,
        NwnTextureSlot, NwnTransform, NwnTransformTrack, NwnUvSet, NwnVec2Sample, NwnVec3Sample,
        ParsedModel, ResolvedMaterialSlot, ResolvedMaterialTextures, ResolvedMtrMaterial,
        ResolvedSceneMaterial, ResolvedTexture, ScalarKey, SceneTextureResolution,
        SemanticAnimation, SemanticAnimationNode, SemanticController, SemanticControllerKey,
        SemanticDangly, SemanticEmitter, SemanticEmitterController, SemanticEmitterKey,
        SemanticEmitterProperty, SemanticFace, SemanticHeader, SemanticLight, SemanticMaterial,
        SemanticMesh, SemanticModel, SemanticNode, SemanticPropertyValue, SemanticReference,
        SemanticSkinWeight, SemanticTextureBinding, SemanticUvLayer, TextureResolverOptions,
        TextureResourceKind, UnknownBinaryBlock, UnresolvedTexture, Vec3Key, Vec4Key,
        apply_appearance_overrides, bake_composed_scene_pose, bake_scene_pose,
        collect_appearance_slots, compile_ascii_model, compile_semantic_model,
        compile_semantic_model_bytes, compose_player_creature_from_resman,
        compose_player_creature_from_utc, composed_scene_animation_names, default_scene_animation,
        detect_model_encoding, find_scene_animation, inherit_supermodel_animations,
        load_composed_scene_from_resman, lower_ascii_model, lower_binary_model,
        lower_binary_model_to_ascii, lower_binary_model_to_ascii_with_options,
        lower_semantic_model_to_scene, parse_ascii_model, parse_binary_model_bytes,
        parse_model_bytes, parse_scene_model, parse_scene_model_auto, parse_semantic_model,
        parse_semantic_model_auto, read_ascii_model, read_binary_model, read_model,
        read_parsed_model, read_scene_model, read_scene_model_auto, read_semantic_model,
        read_semantic_model_auto, resolve_material_textures, resolve_scene_material,
        resolve_scene_materials, resolve_scene_texture_ref, resolve_scene_texture_ref_with_policy,
        resolve_scene_textures, resolve_texture_ref, restore_compiled_model,
        sample_composed_scene_animation, sample_composed_scene_default_animation,
        sample_scene_animation, sample_scene_default_animation, scene_animation_names,
        scene_texture_resolution_names, validate_semantic_model, write_ascii_model,
        write_composed_scene_obj, write_model, write_original_binary_model, write_parsed_model,
        write_scene_model, write_scene_obj, write_semantic_model,
    };
}
