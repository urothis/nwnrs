use std::collections::BTreeSet;

use nwnrs_types::resman::{
    CachePolicy, Res, ResMan, ResRef, ResType, ResolvedResRef, get_res_type,
};

use crate::mdl::{MODEL_RES_TYPE, NwnMaterial, NwnScene, NwnTextureRef, NwnTextureSlot};

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
    #[must_use]
    pub fn res_type(self) -> ResType {
        match self {
            Self::Dds => get_res_type("dds"),
            Self::Tga => get_res_type("tga"),
            Self::Plt => get_res_type("plt"),
        }
    }

    /// Returns the file extension for this kind.
    #[must_use]
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

/// One scene-aware texture resolution outcome.
#[derive(Debug, Clone)]
pub enum SceneTextureResolution {
    /// A final texture asset was resolved.
    Resolved(ResolvedTexture),
    /// The token is an appearance/helper reference and should not be treated as
    /// a missing standalone texture.
    Ignored,
    /// No texture asset could be resolved.
    Missing(UnresolvedTexture),
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

/// Returns ordered texture-name fallbacks for one scene material texture.
///
/// The returned names always start with the original texture name and then add
/// appearance-aware aliases, such as body-part name normalization and nearest
/// ancestor bitmap inheritance for placeholder child meshes.
#[must_use]
pub fn scene_texture_resolution_names(
    scene: &NwnScene,
    material: &NwnMaterial,
    texture: &NwnTextureRef,
) -> Vec<String> {
    let mut names = vec![texture.name.clone()];

    if let Some(normalized) = normalize_body_part_texture_name(texture.name.as_str()) {
        push_unique_case_insensitive(&mut names, normalized);
    }

    for inherited in inherited_bitmap_names(scene, material.source_node) {
        push_unique_case_insensitive(&mut names, inherited);
    }

    names
}

/// Resolves one texture reference through `resman`, including scene-aware
/// appearance fallbacks.
///
/// # Errors
///
/// Returns [`UnresolvedTexture`] if the texture cannot be found.
pub fn resolve_scene_texture_ref(
    scene: &NwnScene,
    material: &NwnMaterial,
    texture: &NwnTextureRef,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
) -> Result<ResolvedTexture, UnresolvedTexture> {
    match resolve_scene_texture_ref_with_policy(scene, material, texture, resman, options) {
        SceneTextureResolution::Resolved(hit) => Ok(hit),
        SceneTextureResolution::Ignored => Err(UnresolvedTexture {
            texture:   texture.clone(),
            attempted: Vec::new(),
        }),
        SceneTextureResolution::Missing(miss) => Err(miss),
    }
}

/// Resolves one texture reference through `resman`, including scene-aware
/// appearance fallbacks and model-backed body-part references.
pub fn resolve_scene_texture_ref_with_policy(
    scene: &NwnScene,
    material: &NwnMaterial,
    texture: &NwnTextureRef,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
) -> SceneTextureResolution {
    let mut visited_models = BTreeSet::new();
    resolve_scene_texture_ref_internal(
        scene,
        material,
        texture,
        resman,
        options,
        &mut visited_models,
    )
}

/// Resolves all textures referenced by one material.
pub fn resolve_material_textures(
    scene: &NwnScene,
    material_index: usize,
    material: &NwnMaterial,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
) -> ResolvedMaterialTextures {
    let mut resolved = Vec::new();
    let mut missing = Vec::new();

    for texture in &material.textures {
        match resolve_scene_texture_ref_with_policy(scene, material, texture, resman, options) {
            SceneTextureResolution::Resolved(hit) => resolved.push(hit),
            SceneTextureResolution::Ignored => {}
            SceneTextureResolution::Missing(miss) => missing.push(miss),
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
            resolve_material_textures(scene, material_index, material, resman, options)
        })
        .collect()
}

fn normalize_body_part_texture_name(name: &str) -> Option<String> {
    let trimmed = name.trim();
    let lower = trimmed.to_ascii_lowercase();
    if lower.len() < 2 || !lower.ends_with('g') {
        return None;
    }

    let base = &trimmed[..trimmed.len() - 1];
    let base_lower = base.to_ascii_lowercase();
    let looks_like_body_part = (base_lower.starts_with('p') || base_lower.starts_with('i'))
        && base_lower.contains('_')
        && base_lower.chars().any(|ch| ch.is_ascii_digit());
    looks_like_body_part.then(|| base.to_string())
}

fn resolve_scene_texture_ref_internal(
    scene: &NwnScene,
    material: &NwnMaterial,
    texture: &NwnTextureRef,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
    visited_models: &mut BTreeSet<String>,
) -> SceneTextureResolution {
    let resolution_names = scene_texture_resolution_names(scene, material, texture);
    if resolution_names
        .iter()
        .any(|name| is_helper_texture_token(name))
    {
        return SceneTextureResolution::Ignored;
    }
    let mut attempted = Vec::new();

    for candidate_name in &resolution_names {
        let candidate_ref = NwnTextureRef {
            slot: texture.slot.clone(),
            name: candidate_name.clone(),
        };
        match resolve_texture_ref(&candidate_ref, resman, options) {
            Ok(mut hit) => {
                hit.texture = texture.clone();
                return SceneTextureResolution::Resolved(hit);
            }
            Err(miss) => merge_attempted(&mut attempted, miss.attempted),
        }

        match resolve_model_backed_texture_candidate(
            candidate_name,
            texture,
            resman,
            options,
            visited_models,
        ) {
            SceneTextureResolution::Resolved(hit) => return SceneTextureResolution::Resolved(hit),
            SceneTextureResolution::Ignored => {}
            SceneTextureResolution::Missing(miss) => {
                merge_attempted(&mut attempted, miss.attempted);
            }
        }
    }

    if should_ignore_unresolved_texture(&resolution_names, resman) {
        return SceneTextureResolution::Ignored;
    }

    SceneTextureResolution::Missing(UnresolvedTexture {
        texture: texture.clone(),
        attempted,
    })
}

fn resolve_model_backed_texture_candidate(
    candidate_name: &str,
    original_texture: &NwnTextureRef,
    resman: &mut ResMan,
    options: &TextureResolverOptions,
    visited_models: &mut BTreeSet<String>,
) -> SceneTextureResolution {
    let trimmed = candidate_name.trim();
    if trimmed.is_empty() {
        return SceneTextureResolution::Missing(UnresolvedTexture {
            texture:   original_texture.clone(),
            attempted: Vec::new(),
        });
    }

    let Some(model_ref) = ResRef::new(trimmed.to_string(), MODEL_RES_TYPE).ok() else {
        return SceneTextureResolution::Missing(UnresolvedTexture {
            texture:   original_texture.clone(),
            attempted: Vec::new(),
        });
    };
    let Some(model_resource) = resman.get(&model_ref) else {
        return SceneTextureResolution::Missing(UnresolvedTexture {
            texture:   original_texture.clone(),
            attempted: Vec::new(),
        });
    };
    if !visited_models.insert(trimmed.to_ascii_lowercase()) {
        return SceneTextureResolution::Ignored;
    }

    let Ok(nested_scene) = NwnScene::from_auto_res(&model_resource, CachePolicy::Use) else {
        visited_models.remove(&trimmed.to_ascii_lowercase());
        return SceneTextureResolution::Ignored;
    };

    let mut attempted = Vec::new();
    for nested_material in &nested_scene.materials {
        let Some(nested_texture) = nested_material
            .textures
            .iter()
            .find(|texture| matches!(texture.slot, NwnTextureSlot::Bitmap))
        else {
            continue;
        };
        match resolve_scene_texture_ref_internal(
            &nested_scene,
            nested_material,
            nested_texture,
            resman,
            options,
            visited_models,
        ) {
            SceneTextureResolution::Resolved(mut hit) => {
                hit.texture = original_texture.clone();
                visited_models.remove(&trimmed.to_ascii_lowercase());
                return SceneTextureResolution::Resolved(hit);
            }
            SceneTextureResolution::Ignored => {}
            SceneTextureResolution::Missing(miss) => {
                merge_attempted(&mut attempted, miss.attempted);
            }
        }
    }

    visited_models.remove(&trimmed.to_ascii_lowercase());
    if attempted.is_empty() {
        SceneTextureResolution::Ignored
    } else {
        SceneTextureResolution::Missing(UnresolvedTexture {
            texture: original_texture.clone(),
            attempted,
        })
    }
}

fn should_ignore_unresolved_texture(candidate_names: &[String], resman: &mut ResMan) -> bool {
    candidate_names.iter().any(|name| {
        let trimmed = name.trim();
        is_helper_texture_token(trimmed)
            || looks_like_body_part_model_name(trimmed)
            || ResRef::new(trimmed.to_string(), MODEL_RES_TYPE)
                .ok()
                .and_then(|resref| resman.get(&resref))
                .is_some()
    })
}

fn is_helper_texture_token(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    lower == "coat_bones" || lower.starts_with("nonwalk")
}

fn looks_like_body_part_model_name(name: &str) -> bool {
    let lower = name.trim().to_ascii_lowercase();
    if lower.is_empty() || lower.contains('.') || !lower.contains('_') {
        return false;
    }

    let mut chars = lower.chars();
    matches!(
        (chars.next(), chars.next(), chars.next()),
        (Some('p' | 'i'), Some('m' | 'f'), Some(race))
            if race.is_ascii_alphabetic()
                && lower.chars().any(|ch| ch.is_ascii_digit())
    )
}

fn merge_attempted(
    attempted: &mut Vec<ResolvedResRef>,
    candidates: impl IntoIterator<Item = ResolvedResRef>,
) {
    for candidate in candidates {
        if !attempted.iter().any(|existing| existing == &candidate) {
            attempted.push(candidate);
        }
    }
}

fn inherited_bitmap_names(scene: &NwnScene, source_node: usize) -> Vec<String> {
    let mut names = Vec::new();
    let mut current = scene.nodes.get(source_node).and_then(|node| node.parent);

    while let Some(node_index) = current {
        if let Some(material) = scene
            .materials
            .iter()
            .find(|material| material.source_node == node_index)
        {
            for texture in &material.textures {
                if matches!(texture.slot, NwnTextureSlot::Bitmap) {
                    push_unique_case_insensitive(&mut names, texture.name.clone());
                }
            }
        }
        current = scene.nodes.get(node_index).and_then(|node| node.parent);
    }

    names
}

fn push_unique_case_insensitive(values: &mut Vec<String>, candidate: String) {
    if !values
        .iter()
        .any(|existing| existing.eq_ignore_ascii_case(candidate.as_str()))
    {
        values.push(candidate);
    }
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

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nwnrs_types::resman::{ResContainer, ResMan, ResolvedResRef, read_resmemfile};

    use crate::mdl::{
        NwnMaterial, NwnTextureRef, NwnTextureSlot, SceneTextureResolution, TextureResolverOptions,
        TextureResourceKind, parse_scene_model, resolve_material_textures,
        resolve_scene_texture_ref_with_policy, resolve_scene_textures, resolve_texture_ref,
        scene_texture_resolution_names,
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
  bitmap present
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
node trimesh mesh02
  parent mesh01
  render 1
  bitmap missing
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
            helper_bitmap:     None,
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
            &scene,
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

    #[test]
    fn scene_texture_resolution_normalizes_body_part_suffixes() {
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
node trimesh head
  parent demo
  render 1
  bitmap pmh0_head001g
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
        let material = scene
            .materials
            .first()
            .unwrap_or_else(|| panic!("material"));
        let texture = material
            .textures
            .first()
            .unwrap_or_else(|| panic!("bitmap texture"));

        assert_eq!(
            scene_texture_resolution_names(&scene, material, texture),
            vec!["pmh0_head001g".to_string(), "pmh0_head001".to_string()]
        );
    }

    #[test]
    fn scene_texture_resolution_inherits_parent_bitmap_names() {
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
node trimesh belt
  parent demo
  render 1
  bitmap pmh0_pelvis001
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
node trimesh flap
  parent belt
  render 1
  bitmap TF3_g
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
        let material = scene
            .materials
            .iter()
            .find(|material| material.source_node == 2)
            .unwrap_or_else(|| panic!("child material"));
        let texture = material
            .textures
            .first()
            .unwrap_or_else(|| panic!("bitmap texture"));

        assert_eq!(
            scene_texture_resolution_names(&scene, material, texture),
            vec!["TF3_g".to_string(), "pmh0_pelvis001".to_string()]
        );
    }

    #[test]
    fn resolves_body_part_models_to_nested_texture_assets() {
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
node trimesh pelvis
  parent demo
  render 1
  bitmap pmh2_pelvis001
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
        let material = scene
            .materials
            .first()
            .unwrap_or_else(|| panic!("material"));
        let texture = material
            .textures
            .first()
            .unwrap_or_else(|| panic!("bitmap texture"));
        let mut manager = build_manager(&[
            (
                "pmh2_pelvis001 mdl",
                "pmh2_pelvis001.mdl",
                br#"newmodel pmh2_pelvis001
setsupermodel pmh2_pelvis001 null
classification character
setanimationscale 1
beginmodelgeom pmh2_pelvis001
node dummy pmh2_pelvis001
  parent NULL
endnode
node trimesh pelvis
  parent pmh2_pelvis001
  render 1
  bitmap pmh0_pelvis001
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
endmodelgeom pmh2_pelvis001
donemodel pmh2_pelvis001"#,
            ),
            ("pmh0_pelvis001 plt", "pmh0_pelvis001.plt", b"plt"),
        ]);

        let outcome = resolve_scene_texture_ref_with_policy(
            &scene,
            material,
            texture,
            &mut manager,
            &TextureResolverOptions::default(),
        );

        match outcome {
            SceneTextureResolution::Resolved(hit) => {
                assert_eq!(hit.resolved.to_file(), "pmh0_pelvis001.plt");
                assert_eq!(hit.texture.name, "pmh2_pelvis001");
            }
            other => panic!("expected resolved nested texture, got {:?}", other),
        }
    }

    #[test]
    fn ignores_helper_and_appearance_tokens_without_warning() {
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
node trimesh coat
  parent demo
  render 1
  bitmap coat_bones
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
node trimesh robe
  parent demo
  render 1
  bitmap pmh0_robe035
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
node trimesh nonwalk
  parent demo
  render 1
  bitmap Nonwalk
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
node trimesh invisible
  parent demo
  render 1
  bitmap placeholdermaterial
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
        let mut manager = build_manager(&[]);

        for material in &scene.materials {
            let Some(texture) = material
                .textures
                .iter()
                .find(|texture| matches!(texture.slot, NwnTextureSlot::Bitmap))
            else {
                continue;
            };
            let outcome = resolve_scene_texture_ref_with_policy(
                &scene,
                material,
                texture,
                &mut manager,
                &TextureResolverOptions::default(),
            );
            if texture.name.eq_ignore_ascii_case("placeholdermaterial") {
                assert!(matches!(outcome, SceneTextureResolution::Missing(_)));
            } else {
                assert!(matches!(outcome, SceneTextureResolution::Ignored));
            }
        }
    }
}
