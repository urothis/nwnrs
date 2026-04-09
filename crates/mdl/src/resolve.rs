use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use nwnrs_restype::prelude::*;

use crate::{NwnMaterial, NwnScene, NwnTextureRef};

/// NWN texture resource kinds the model resolver can search for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureResourceKind {
    /// NWN compact DDS texture.
    Dds,
    /// TGA texture.
    Tga,
    /// PLT palette texture.
    Plt,
}

impl TextureResourceKind {
    /// Returns the registered NWN resource type for this kind.
    pub fn res_type(self) -> ResType {
        match self {
            Self::Dds => get_res_type("dds"),
            Self::Tga => get_res_type("tga"),
            Self::Plt => get_res_type("plt"),
        }
    }

    /// Returns the file extension for this kind.
    pub fn extension(self) -> &'static str {
        match self {
            Self::Dds => "dds",
            Self::Tga => "tga",
            Self::Plt => "plt",
        }
    }
}

/// Resolver options for scene texture lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextureResolverOptions {
    /// Fallback order attempted for bare texture names.
    pub fallback_order: Vec<TextureResourceKind>,
}

impl Default for TextureResolverOptions {
    fn default() -> Self {
        Self {
            fallback_order: vec![
                TextureResourceKind::Dds,
                TextureResourceKind::Tga,
                TextureResourceKind::Plt,
            ],
        }
    }
}

/// One resolved texture reference.
#[derive(Debug, Clone)]
pub struct ResolvedTexture {
    /// Original texture reference from the scene material.
    pub texture:  NwnTextureRef,
    /// Matched texture resource kind.
    pub kind:     TextureResourceKind,
    /// Fully resolved `name.ext` candidate that matched.
    pub resolved: ResolvedResRef,
    /// Resolved resource entry.
    pub resource: Res,
}

/// One unresolved texture reference plus the candidates that were tried.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnresolvedTexture {
    /// Original texture reference from the scene material.
    pub texture:   NwnTextureRef,
    /// Fully resolved `name.ext` candidates attempted in order.
    pub attempted: Vec<ResolvedResRef>,
}

/// Texture lookup results for one scene material.
#[derive(Debug, Clone)]
pub struct ResolvedMaterialTextures {
    /// Material index within [`NwnScene::materials`].
    pub material_index: usize,
    /// Source scene node index that authored the material.
    pub source_node:    usize,
    /// Successfully resolved textures.
    pub resolved:       Vec<ResolvedTexture>,
    /// Texture references that could not be resolved.
    pub missing:        Vec<UnresolvedTexture>,
}

/// Resolves one texture reference through `resman`.
pub fn resolve_texture_ref(
    texture: &NwnTextureRef,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
) -> Result<ResolvedTexture, UnresolvedTexture> {
    let candidates = texture_candidates(texture.name.as_str(), options);
    for (kind, candidate) in &candidates {
        if let Some(resource) = resman.get_resolved(candidate) {
            return Ok(ResolvedTexture {
                texture: texture.clone(),
                kind: *kind,
                resolved: candidate.clone(),
                resource,
            });
        }
    }

    Err(UnresolvedTexture {
        texture:   texture.clone(),
        attempted: candidates
            .into_iter()
            .map(|(_kind, candidate)| candidate)
            .collect(),
    })
}

/// Resolves all textures referenced by one material.
pub fn resolve_material_textures(
    material_index: usize,
    material: &NwnMaterial,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
) -> ResolvedMaterialTextures {
    let mut resolved = Vec::new();
    let mut missing = Vec::new();

    for texture in &material.textures {
        match resolve_texture_ref(texture, resman, options) {
            Ok(hit) => resolved.push(hit),
            Err(miss) => missing.push(miss),
        }
    }

    ResolvedMaterialTextures {
        material_index,
        source_node: material.source_node,
        resolved,
        missing,
    }
}

/// Resolves all textures referenced by every material in a scene.
pub fn resolve_scene_textures(
    scene: &NwnScene,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
) -> Vec<ResolvedMaterialTextures> {
    scene
        .materials
        .iter()
        .enumerate()
        .map(|(material_index, material)| {
            resolve_material_textures(material_index, material, resman, options)
        })
        .collect()
}

fn texture_candidates(
    name: &str,
    options: &TextureResolverOptions,
) -> Vec<(TextureResourceKind, ResolvedResRef)> {
    if let Some(candidate) = explicit_texture_candidate(name) {
        return vec![candidate];
    }

    options
        .fallback_order
        .iter()
        .filter_map(|kind| {
            ResolvedResRef::new(name.to_string(), kind.res_type())
                .ok()
                .map(|candidate| (*kind, candidate))
        })
        .collect()
}

fn explicit_texture_candidate(name: &str) -> Option<(TextureResourceKind, ResolvedResRef)> {
    let resolved = ResolvedResRef::try_from_filename(name)?;
    let kind = match resolved.res_ext() {
        "dds" => TextureResourceKind::Dds,
        "tga" => TextureResourceKind::Tga,
        "plt" => TextureResourceKind::Plt,
        _ => return None,
    };
    Some((kind, resolved))
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nwnrs_resman::{ResContainer, ResMan};
    use nwnrs_resmemfile::prelude::*;
    use nwnrs_resref::ResolvedResRef;

    use crate::{
        NwnMaterial, NwnTextureRef, NwnTextureSlot, TextureResolverOptions, TextureResourceKind,
        parse_scene_model, resolve_material_textures, resolve_scene_textures, resolve_texture_ref,
    };

    fn build_manager(entries: &[(&str, &str, &[u8])]) -> ResMan {
        let mut manager = ResMan::new(1);
        for (label, filename, bytes) in entries {
            let resref = ResolvedResRef::from_filename(filename)
                .unwrap_or_else(|error| panic!("resolved {filename}: {error}"));
            let container = read_resmemfile((*label).to_string(), resref.into(), bytes.to_vec())
                .unwrap_or_else(|error| panic!("resmem {filename}: {error}"));
            manager.add(Arc::new(container) as Arc<dyn ResContainer>);
        }
        manager
    }

    #[test]
    fn resolves_bare_texture_names_in_default_order() {
        let texture = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: "stone".to_string(),
        };
        let mut manager =
            build_manager(&[("tga", "stone.tga", b"tga"), ("dds", "stone.dds", b"dds")]);

        let resolved =
            resolve_texture_ref(&texture, &mut manager, &TextureResolverOptions::default())
                .unwrap_or_else(|error| panic!("resolve bare texture: {:?}", error));

        assert_eq!(resolved.kind, TextureResourceKind::Dds);
        assert_eq!(resolved.resolved.to_file(), "stone.dds");
    }

    #[test]
    fn resolves_explicit_texture_extension_exactly() {
        let texture = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: "cloak_001.plt".to_string(),
        };
        let mut manager = build_manager(&[
            ("plt", "cloak_001.plt", b"plt"),
            ("dds", "cloak_001.dds", b"dds"),
        ]);

        let resolved =
            resolve_texture_ref(&texture, &mut manager, &TextureResolverOptions::default())
                .unwrap_or_else(|error| panic!("resolve explicit texture: {:?}", error));

        assert_eq!(resolved.kind, TextureResourceKind::Plt);
        assert_eq!(resolved.resolved.to_file(), "cloak_001.plt");
    }

    #[test]
    fn reports_attempted_candidates_for_missing_textures() {
        let texture = NwnTextureRef {
            slot: NwnTextureSlot::Bitmap,
            name: "missing".to_string(),
        };
        let mut manager = build_manager(&[]);

        let missing =
            resolve_texture_ref(&texture, &mut manager, &TextureResolverOptions::default())
                .err()
                .unwrap_or_else(|| panic!("expected missing texture"));

        let attempted = missing
            .attempted
            .iter()
            .map(ResolvedResRef::to_file)
            .collect::<Vec<_>>();
        assert_eq!(attempted, vec!["missing.dds", "missing.tga", "missing.plt"]);
    }

    #[test]
    fn resolves_scene_materials_from_lowered_scene() {
        let scene = parse_scene_model(
            "\
newmodel demo
setsupermodel demo null
classification character
setanimationscale 1
beginmodelgeom demo
node dummy demo
  parent NULL
endnode
node trimesh mesh01
  parent demo
  render 1
  bitmap tex_a
  texture1 tex_b
  verts 3
    0 0 0
    1 0 0
    0 1 0
  faces 1
    0 1 2  0  0 1 2  0
  tverts 3
    0 0 0
    1 0 0
    0 1 0
endnode
endmodelgeom demo
donemodel demo
",
        )
        .unwrap_or_else(|error| panic!("parse scene fixture: {error}"));

        let mut manager =
            build_manager(&[("tex_a", "tex_a.dds", b"a"), ("tex_b", "tex_b.tga", b"b")]);
        let resolutions =
            resolve_scene_textures(&scene, &mut manager, &TextureResolverOptions::default());
        let material_resolution = resolutions.first().unwrap_or_else(|| {
            panic!("expected one material resolution");
        });

        assert_eq!(resolutions.len(), 1);
        assert_eq!(material_resolution.resolved.len(), 2);
        assert!(material_resolution.missing.is_empty());
        assert_eq!(
            material_resolution
                .resolved
                .iter()
                .map(|hit| hit.resolved.to_file())
                .collect::<Vec<_>>(),
            vec!["tex_a.dds", "tex_b.tga"]
        );
    }

    #[test]
    fn resolves_single_material_with_missing_entries() {
        let material = NwnMaterial {
            source_node:       3,
            render_enabled:    true,
            shadow_enabled:    true,
            beaming:           0,
            inherit_color:     0,
            tilefade:          0,
            rotate_texture:    0,
            transparency_hint: 0,
            shininess:         0.0,
            alpha:             1.0,
            ambient:           [1.0, 1.0, 1.0],
            diffuse:           [1.0, 1.0, 1.0],
            specular:          [0.0, 0.0, 0.0],
            self_illum_color:  [0.0, 0.0, 0.0],
            material_name:     None,
            render_hint:       None,
            textures:          vec![
                NwnTextureRef {
                    slot: NwnTextureSlot::Bitmap,
                    name: "present".to_string(),
                },
                NwnTextureRef {
                    slot: NwnTextureSlot::Texture(1),
                    name: "missing".to_string(),
                },
            ],
        };
        let mut manager = build_manager(&[("present", "present.dds", b"present")]);
        let resolved = resolve_material_textures(
            0,
            &material,
            &mut manager,
            &TextureResolverOptions::default(),
        );

        assert_eq!(resolved.source_node, 3);
        assert_eq!(resolved.resolved.len(), 1);
        assert_eq!(resolved.missing.len(), 1);
        assert_eq!(
            resolved
                .resolved
                .first()
                .map(|hit| hit.resolved.to_file())
                .as_deref(),
            Some("present.dds")
        );
        assert_eq!(
            resolved.missing.first().map(|miss| miss.attempted.len()),
            Some(3)
        );
    }
}
