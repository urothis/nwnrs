#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod animation;
mod appearance;
mod ascii;
mod binary;
mod compose;
mod io;
mod obj;
mod pose;
mod resolve;
mod scene;
mod semantic;
mod types;

pub use animation::*;
pub use appearance::*;
pub use ascii::*;
pub use binary::*;
pub use compose::*;
pub use io::*;
pub use obj::*;
pub use pose::*;
pub use resolve::*;
pub use scene::*;
pub use semantic::*;
pub use types::*;

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        AnimationEvent, AsciiAnimation, AsciiBodyItem, AsciiElement, AsciiModel, AsciiNode,
        AsciiPayloadKind, AsciiStatement, BinaryAabb, BinaryAabbEntry, BinaryAnimMesh,
        BinaryAnimation, BinaryArrayDefinition, BinaryController, BinaryDangly, BinaryEmitter,
        BinaryEmitterFlags, BinaryEvent, BinaryFace, BinaryHeader, BinaryLight, BinaryMesh,
        BinaryModel, BinaryNode, BinaryNodeContent, BinaryReference, BinarySkin, BinaryUvSet,
        MODEL_RES_TYPE, Model, ModelClassification, ModelDiagnostic, ModelDiagnosticKind,
        ModelEncoding, ModelError, ModelResult, NodeKind, NwnAnimMeshTrack, NwnAnimation,
        NwnAppearanceOverrides, NwnAppearanceSlot, NwnComposedScene, NwnCoordinateSystem,
        NwnEmitter, NwnEmitterProperty, NwnFace, NwnLight, NwnMaterial, NwnMaterialTrack, NwnMesh,
        NwnNodeAnimationTrack, NwnPrimitive, NwnPropertyValue, NwnReference, NwnScene,
        NwnSceneAttachment, NwnSceneNode, NwnSkinWeight, NwnTextureRef, NwnTextureSlot,
        NwnTransform, NwnTransformTrack, NwnUvSet, NwnVec2Sample, NwnVec3Sample, ParsedModel,
        ResolvedMaterialTextures, ResolvedTexture, ScalarKey, SceneTextureResolution,
        SemanticAnimation, SemanticAnimationNode, SemanticEmitter, SemanticEmitterProperty,
        SemanticFace, SemanticHeader, SemanticLight, SemanticMaterial, SemanticMesh, SemanticModel,
        SemanticNode, SemanticPropertyValue, SemanticReference, SemanticSkinWeight,
        SemanticTextureBinding, SemanticUvLayer, TextureResolverOptions, TextureResourceKind,
        UnknownBinaryBlock, UnresolvedTexture, Vec3Key, Vec4Key, apply_appearance_overrides,
        bake_composed_scene_pose, bake_scene_pose, collect_appearance_slots, compile_ascii_model,
        compose_player_creature_from_resman, compose_player_creature_from_utc,
        composed_scene_animation_names, default_scene_animation, detect_model_encoding,
        find_scene_animation, load_composed_scene_from_resman, lower_ascii_model,
        lower_binary_model_to_ascii, lower_semantic_model_to_scene, parse_ascii_model,
        parse_binary_model_bytes, parse_model_bytes, parse_scene_model, parse_scene_model_auto,
        parse_semantic_model, parse_semantic_model_auto, read_ascii_model, read_binary_model,
        read_model, read_parsed_model, read_scene_model, read_scene_model_auto,
        read_semantic_model, read_semantic_model_auto, resolve_material_textures,
        resolve_scene_texture_ref, resolve_scene_texture_ref_with_policy, resolve_scene_textures,
        resolve_texture_ref, sample_composed_scene_animation,
        sample_composed_scene_default_animation, sample_scene_animation,
        sample_scene_default_animation, scene_animation_names, scene_texture_resolution_names,
        write_ascii_model, write_binary_model, write_composed_scene_obj, write_model,
        write_parsed_model, write_scene_model, write_scene_obj, write_semantic_model,
    };
}
