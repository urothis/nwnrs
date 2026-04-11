#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

mod ascii;
mod binary;
mod io;
mod resolve;
mod scene;
mod semantic;
mod types;

pub use ascii::*;
pub use binary::*;
pub use io::*;
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
        NwnCoordinateSystem, NwnEmitter, NwnEmitterProperty, NwnFace, NwnLight, NwnMaterial,
        NwnMaterialTrack, NwnMesh, NwnNodeAnimationTrack, NwnPrimitive, NwnPropertyValue,
        NwnReference, NwnScene, NwnSceneNode, NwnSkinWeight, NwnTextureRef, NwnTextureSlot,
        NwnTransform, NwnTransformTrack, NwnUvSet, NwnVec2Sample, NwnVec3Sample, ParsedModel,
        ResolvedMaterialTextures, ResolvedTexture, ScalarKey, SceneTextureResolution,
        SemanticAnimation, SemanticAnimationNode, SemanticEmitter, SemanticEmitterProperty,
        SemanticFace, SemanticHeader, SemanticLight, SemanticMaterial, SemanticMesh, SemanticModel,
        SemanticNode, SemanticPropertyValue, SemanticReference, SemanticSkinWeight,
        SemanticTextureBinding, SemanticUvLayer, TextureResolverOptions, TextureResourceKind,
        UnknownBinaryBlock, UnresolvedTexture, Vec3Key, Vec4Key, compile_ascii_model,
        detect_model_encoding, lower_ascii_model, lower_binary_model_to_ascii,
        lower_semantic_model_to_scene, parse_ascii_model, parse_binary_model_bytes,
        parse_model_bytes, parse_scene_model, parse_scene_model_auto, parse_semantic_model,
        parse_semantic_model_auto, read_ascii_model, read_ascii_model_from_file,
        read_ascii_model_from_res, read_binary_model, read_binary_model_from_file,
        read_binary_model_from_res, read_model, read_model_from_file, read_model_from_res,
        read_parsed_model, read_parsed_model_from_file, read_parsed_model_from_res,
        read_scene_model, read_scene_model_auto, read_scene_model_auto_from_file,
        read_scene_model_auto_from_res, read_scene_model_from_file, read_scene_model_from_res,
        read_semantic_model, read_semantic_model_auto, read_semantic_model_auto_from_file,
        read_semantic_model_auto_from_res, read_semantic_model_from_file,
        read_semantic_model_from_res, resolve_material_textures, resolve_scene_texture_ref,
        resolve_scene_texture_ref_with_policy, resolve_scene_textures, resolve_texture_ref,
        scene_texture_resolution_names, write_ascii_model, write_model,
    };
}
