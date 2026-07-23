use std::{f32::consts::FRAC_PI_2, io::Cursor};

use nwnrs_types::{
    gff::{
        AreFile, GffCExoLocString, GffRoot, GffStruct, GitFile, GitTransform, ModuleInfo,
        read_gff_root,
    },
    mdl::{
        ModelResourceKind, NwnAppearanceOverrides, NwnBlueprintKind, NwnBlueprintVisual,
        NwnComposedScene, NwnPropertyValue, compose_blueprint_visual_from_resman,
        compose_blueprint_visual_from_root, load_composed_scene_from_resman,
        parse_scene_resource_auto,
    },
    resman::{CachePolicy, ResMan, ResolvedResRef},
    set::SetFile,
    twoda::read_twoda,
};
use tracing::instrument;

use crate::scene::{
    DependencyGraph, DependencyKind, DependencyState, SceneArea, SceneAreaEnvironment,
    SceneAreaObject, SceneDiagnostic, SceneDiagnosticSeverity, SceneDocument, SceneEnvironment,
    SceneError, SceneInstance, SceneInstanceKind, SceneModel, SceneModule, SceneResult,
    SceneSource, assets::resolve_model_assets,
};

/// Stateful scene assembler over one layered NWN resource view.
pub struct SceneLoader<'a> {
    resman: &'a mut ResMan,
}

/// Builds the stable logical-object catalog shared by area packets and editor
/// navigation. Authored list indices remain category-local, matching GIT.
#[must_use]
pub fn area_object_catalog(git: &GitFile) -> Vec<SceneAreaObject> {
    let mut result = Vec::new();
    for (index, value) in git.creatures.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Creature,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            value.template_resref.as_deref(),
            &value.transform,
        );
    }
    for (index, value) in git.doors.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Door,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            value.template_resref.as_deref(),
            &value.transform,
        );
    }
    for (index, value) in git.placeables.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Placeable,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            value.template_resref.as_deref(),
            &value.transform,
        );
    }
    for (index, value) in git.encounters.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Encounter,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            None,
            &value.transform,
        );
    }
    for (index, value) in git.sounds.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Sound,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            value.template_resref.as_deref(),
            &value.transform,
        );
    }
    for (index, value) in git.stores.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Store,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            value.template_resref.as_deref(),
            &value.transform,
        );
    }
    for (index, value) in git.triggers.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Trigger,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            None,
            &value.transform,
        );
    }
    for (index, value) in git.waypoints.iter().enumerate() {
        push_area_object(
            &mut result,
            SceneInstanceKind::Waypoint,
            index,
            value.localized_name.as_ref(),
            value.tag.as_deref(),
            value.template_resref.as_deref(),
            &value.transform,
        );
    }
    result
}

fn push_area_object(
    target: &mut Vec<SceneAreaObject>,
    kind: SceneInstanceKind,
    source_index: usize,
    localized_name: Option<&GffCExoLocString>,
    tag: Option<&str>,
    template_resref: Option<&str>,
    transform: &GitTransform,
) {
    let kind_name = scene_instance_kind_name(kind);
    let tag = tag.filter(|value| !value.trim().is_empty());
    let template_resref = template_resref.filter(|value| !value.trim().is_empty());
    let label = localized_name
        .and_then(|value| {
            value
                .entries
                .iter()
                .map(|(_, text)| text.trim())
                .find(|text| !text.is_empty())
        })
        .or(tag)
        .or(template_resref)
        .map_or_else(
            || format!("{} {}", title_case_kind(kind_name), source_index + 1),
            str::to_string,
        );
    target.push(SceneAreaObject {
        key: format!("{kind_name}:{source_index}"),
        label,
        kind,
        source_index,
        tag: tag.map(str::to_string),
        template_resref: template_resref.map(str::to_string),
        position: transform_position(transform),
        rotation_axis_angle: transform_rotation(transform),
    });
}

const fn scene_instance_kind_name(kind: SceneInstanceKind) -> &'static str {
    match kind {
        SceneInstanceKind::Creature => "creature",
        SceneInstanceKind::Door => "door",
        SceneInstanceKind::Placeable => "placeable",
        SceneInstanceKind::Encounter => "encounter",
        SceneInstanceKind::Sound => "sound",
        SceneInstanceKind::Store => "store",
        SceneInstanceKind::Trigger => "trigger",
        SceneInstanceKind::Waypoint => "waypoint",
        SceneInstanceKind::Item => "item",
        SceneInstanceKind::Model => "model",
        SceneInstanceKind::Tile => "tile",
        SceneInstanceKind::Skybox => "skybox",
        SceneInstanceKind::Collision => "collision",
    }
}

fn title_case_kind(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().collect::<String>() + chars.as_str()
    })
}

impl<'a> SceneLoader<'a> {
    /// Creates a scene loader over the provided precedence-aware resource
    /// manager.
    pub fn new(resman: &'a mut ResMan) -> Self {
        Self {
            resman,
        }
    }

    /// Loads an MDL, WOK, DWK, or PWK resource and all required model
    /// dependencies.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when the root resource is missing or invalid.
    #[instrument(level = "debug", skip_all, err, fields(resource = %resource))]
    pub fn load_model(&mut self, resource: &ResolvedResRef) -> SceneResult<SceneDocument> {
        let kind =
            ModelResourceKind::from_res_type(resource.base().res_type()).ok_or_else(|| {
                SceneError::invalid(format!("{} is not a model-shaped resource", resource))
            })?;
        let root = self
            .resman
            .get_resolved(resource)
            .ok_or_else(|| SceneError::missing(resource.to_string()))?;
        let mut dependencies = DependencyGraph::default();
        let root_id = dependencies.record(
            resource.to_string(),
            DependencyKind::Root,
            DependencyState::Resolved,
            Some(root.origin().to_string()),
            None,
        );
        let (source, model) = if kind == ModelResourceKind::Model {
            let composed = load_composed_scene_from_resman(
                self.resman,
                resource.base().res_ref(),
                &NwnAppearanceOverrides::default(),
            )
            .map_err(|error| SceneError::scene(error.to_string()))?;
            self.record_composed_dependencies(root_id, &composed, &mut dependencies);
            (SceneSource::Model, SceneModel::Composed(composed))
        } else {
            let bytes = root
                .read_all(CachePolicy::Use)
                .map_err(|error| SceneError::invalid(format!("read {resource}: {error}")))?;
            let scene = parse_scene_resource_auto(kind, resource.base().res_ref(), &bytes)
                .map_err(|error| SceneError::invalid(format!("parse {resource}: {error}")))?;
            let source = match kind {
                ModelResourceKind::Model => SceneSource::Model,
                ModelResourceKind::Walkmesh => SceneSource::Walkmesh,
                ModelResourceKind::DoorWalkmesh => SceneSource::DoorWalkmesh,
                ModelResourceKind::PlaceableWalkmesh => SceneSource::PlaceableWalkmesh,
            };
            (source, SceneModel::Auxiliary(scene))
        };
        let mut models = vec![model];
        let mut instances = vec![SceneInstance {
            id:                    0,
            object_key:            None,
            label:                 resource.to_string(),
            kind:                  if kind == ModelResourceKind::Model {
                SceneInstanceKind::Model
            } else {
                SceneInstanceKind::Collision
            },
            model:                 Some(0),
            resource:              Some(resource.to_string()),
            position:              [0.0; 3],
            rotation_axis_angle:   [0.0, 0.0, 1.0, 0.0],
            scale:                 [1.0; 3],
            polygon:               Vec::new(),
            light_color_overrides: [None; 4],
        }];
        if kind == ModelResourceKind::Model {
            for (companion_kind, extension) in [
                (ModelResourceKind::DoorWalkmesh, "dwk"),
                (ModelResourceKind::PlaceableWalkmesh, "pwk"),
            ] {
                let filename = format!("{}.{extension}", resource.base().res_ref());
                let Ok(resolved) = ResolvedResRef::from_filename(&filename) else {
                    continue;
                };
                let Some(companion) = self.resman.get_resolved(&resolved) else {
                    continue;
                };
                let bytes = companion
                    .read_all(CachePolicy::Use)
                    .map_err(|error| SceneError::invalid(format!("read {filename}: {error}")))?;
                let scene =
                    parse_scene_resource_auto(companion_kind, resource.base().res_ref(), &bytes)
                        .map_err(|error| {
                            SceneError::invalid(format!("parse {filename}: {error}"))
                        })?;
                let collision_id = dependencies.record(
                    filename,
                    DependencyKind::Walkmesh,
                    DependencyState::Resolved,
                    Some(companion.origin().to_string()),
                    None,
                );
                dependencies.connect(root_id, collision_id, "collision");
                let model_index = models.len();
                models.push(SceneModel::Auxiliary(scene));
                instances.push(SceneInstance {
                    id:                    instances.len(),
                    object_key:            None,
                    label:                 resolved.to_string(),
                    kind:                  SceneInstanceKind::Collision,
                    model:                 Some(model_index),
                    resource:              Some(resolved.to_string()),
                    position:              [0.0; 3],
                    rotation_axis_angle:   [0.0, 0.0, 1.0, 0.0],
                    scale:                 [1.0; 3],
                    polygon:               Vec::new(),
                    light_color_overrides: [None; 4],
                });
            }
        }
        let mut diagnostics = Vec::new();
        self.append_emitter_chunk_models(&mut models, &mut dependencies, &mut diagnostics);
        let (model_assets, textures, shaders) =
            resolve_model_assets(self.resman, &models, &mut dependencies, &mut diagnostics)?;
        Ok(SceneDocument {
            name: resource.base().res_ref().to_string(),
            source,
            models,
            model_assets,
            textures,
            shaders,
            instances,
            area: None,
            module: None,
            environment: SceneEnvironment::Studio,
            dependencies,
            diagnostics,
        })
    }

    /// Loads a UTC, UTD, UTP, or UTI and resolves its complete visual model
    /// set through the shared blueprint composer.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when the resource is not a supported
    /// blueprint or any required appearance dependency is invalid.
    #[instrument(level = "debug", skip_all, err, fields(resource = %resource))]
    pub fn load_blueprint(&mut self, resource: &ResolvedResRef) -> SceneResult<SceneDocument> {
        let kind =
            NwnBlueprintKind::from_res_type(resource.base().res_type()).ok_or_else(|| {
                SceneError::invalid(format!("{} is not a visual blueprint", resource))
            })?;
        let root = self
            .resman
            .get_resolved(resource)
            .ok_or_else(|| SceneError::missing(resource.to_string()))?;
        let mut dependencies = DependencyGraph::default();
        let root_id = dependencies.record(
            resource.to_string(),
            DependencyKind::Root,
            DependencyState::Resolved,
            Some(root.origin().to_string()),
            None,
        );
        let visual =
            compose_blueprint_visual_from_resman(self.resman, kind, resource.base().res_ref())
                .map_err(|error| SceneError::scene(error.to_string()))?;
        let mut models = Vec::with_capacity(visual.models.len());
        let mut instances = Vec::with_capacity(visual.models.len());
        let mut model_indices = std::collections::BTreeMap::new();
        let mut diagnostics = Vec::new();
        for composed in visual.models {
            let model_id = self.record_resolved(
                &mut dependencies,
                &format!("{}.mdl", composed.model_name),
                DependencyKind::Model,
            );
            dependencies.connect(root_id, model_id, "visual");
            self.record_composed_dependencies(model_id, &composed, &mut dependencies);
            let model_index = models.len();
            let model_name = composed.model_name.clone();
            models.push(SceneModel::Composed(composed));
            instances.push(SceneInstance {
                id:                    instances.len(),
                object_key:            None,
                label:                 resource.to_string(),
                kind:                  render_kind_for_blueprint(kind),
                model:                 Some(model_index),
                resource:              Some(format!("{model_name}.mdl")),
                position:              [0.0; 3],
                rotation_axis_angle:   [0.0, 0.0, 1.0, 0.0],
                scale:                 [1.0; 3],
                polygon:               Vec::new(),
                light_color_overrides: [None; 4],
            });
            self.append_blueprint_collision(
                kind,
                &model_name,
                None,
                root_id,
                [0.0; 3],
                [0.0, 0.0, 1.0, 0.0],
                &mut model_indices,
                &mut models,
                &mut instances,
                &mut dependencies,
                &mut diagnostics,
            );
        }
        self.append_emitter_chunk_models(&mut models, &mut dependencies, &mut diagnostics);
        let (model_assets, textures, shaders) =
            resolve_model_assets(self.resman, &models, &mut dependencies, &mut diagnostics)?;
        Ok(SceneDocument {
            name: resource.base().res_ref().to_string(),
            source: source_for_blueprint(kind),
            models,
            model_assets,
            textures,
            shaders,
            instances,
            area: None,
            module: None,
            environment: SceneEnvironment::Studio,
            dependencies,
            diagnostics,
        })
    }

    /// Loads an ARE, its GIT, SET, tile models, tile walkmeshes, and placed
    /// overlays.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when required area resources are missing or
    /// invalid.
    #[instrument(level = "debug", skip_all, err, fields(area_name))]
    pub fn load_area(&mut self, area_name: &str) -> SceneResult<SceneDocument> {
        let area = AreFile::from_resman(self.resman, area_name, CachePolicy::Use)
            .map_err(|error| SceneError::invalid(error.to_string()))?;
        let git = GitFile::from_resman(self.resman, area_name, CachePolicy::Use)
            .map_err(|error| SceneError::invalid(error.to_string()))?;
        let area_objects = area_object_catalog(&git);
        let tileset_name = area
            .tileset
            .as_deref()
            .ok_or_else(|| SceneError::invalid(format!("{area_name}.are has no Tileset")))?;
        let tileset = SetFile::from_resman(self.resman, tileset_name, CachePolicy::Use)
            .map_err(|error| SceneError::invalid(error.to_string()))?;

        let mut dependencies = DependencyGraph::default();
        let area_id = self.record_resolved(
            &mut dependencies,
            &format!("{area_name}.are"),
            DependencyKind::Root,
        );
        let git_id = self.record_resolved(
            &mut dependencies,
            &format!("{area_name}.git"),
            DependencyKind::AreaInstances,
        );
        dependencies.connect(area_id, git_id, "instances");
        let set_id = self.record_resolved(
            &mut dependencies,
            &format!("{tileset_name}.set"),
            DependencyKind::Tileset,
        );
        dependencies.connect(area_id, set_id, "tileset");
        let mut diagnostics = Vec::new();
        let light_palette =
            self.load_light_palette(&area, area_id, &mut dependencies, &mut diagnostics);

        let mut models = Vec::new();
        let mut model_indices = std::collections::BTreeMap::<String, usize>::new();
        let mut instances = Vec::new();
        for tile in &area.tiles {
            let Some(tile_id) = tile.tile_id.and_then(|id| u32::try_from(id).ok()) else {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     "area.tile.missingId".into(),
                    message:  format!("tile {} has no valid Tile_ID", tile.index),
                    resource: Some(format!("{area_name}.are")),
                });
                continue;
            };
            let Some(definition) = tileset.tiles.get(&tile_id) else {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     "area.tile.unknownId".into(),
                    message:  format!("tile {} references missing SET tile {tile_id}", tile.index),
                    resource: Some(format!("{tileset_name}.set")),
                });
                continue;
            };
            let Some(model_name) = definition.model.as_deref() else {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     "area.tile.missingModel".into(),
                    message:  format!("SET tile {tile_id} has no model"),
                    resource: Some(format!("{tileset_name}.set")),
                });
                continue;
            };
            let model_key = format!("mdl:{}", model_name.to_ascii_lowercase());
            let model_index = if let Some(index) = model_indices.get(&model_key).copied() {
                index
            } else {
                match load_composed_scene_from_resman(
                    self.resman,
                    model_name,
                    &NwnAppearanceOverrides::default(),
                ) {
                    Ok(model) => {
                        let model_id = self.record_resolved(
                            &mut dependencies,
                            &format!("{model_name}.mdl"),
                            DependencyKind::Model,
                        );
                        dependencies.connect(set_id, model_id, format!("tile:{tile_id}:model"));
                        self.record_composed_dependencies(model_id, &model, &mut dependencies);
                        let index = models.len();
                        models.push(SceneModel::Composed(model));
                        model_indices.insert(model_key, index);
                        index
                    }
                    Err(error) => {
                        let missing_id = dependencies.record(
                            format!("{model_name}.mdl"),
                            DependencyKind::Model,
                            DependencyState::Invalid,
                            None,
                            Some(error.to_string()),
                        );
                        dependencies.connect(set_id, missing_id, format!("tile:{tile_id}:model"));
                        diagnostics.push(SceneDiagnostic {
                            severity: SceneDiagnosticSeverity::Error,
                            code:     "area.tile.modelFailed".into(),
                            message:  format!(
                                "could not load tile {tile_id} model {model_name}: {error}"
                            ),
                            resource: Some(format!("{model_name}.mdl")),
                        });
                        continue;
                    }
                }
            };
            let orientation = tile.orientation.unwrap_or_default().rem_euclid(4) as f32 * FRAC_PI_2;
            instances.push(SceneInstance {
                id:                    instances.len(),
                object_key:            None,
                label:                 format!("Tile {} ({}, {})", tile.index, tile.x, tile.y),
                kind:                  SceneInstanceKind::Tile,
                model:                 Some(model_index),
                resource:              Some(format!("{model_name}.mdl")),
                position:              [
                    tile.x as f32 * 10.0,
                    tile.y as f32 * 10.0,
                    tile.height.unwrap_or_default() as f32 * 5.0,
                ],
                rotation_axis_angle:   [0.0, 0.0, 1.0, orientation],
                scale:                 [1.0; 3],
                polygon:               Vec::new(),
                light_color_overrides: [
                    palette_color(&light_palette, tile.main_lights[0]),
                    palette_color(&light_palette, tile.main_lights[1]),
                    palette_color(&light_palette, tile.source_lights[0]),
                    palette_color(&light_palette, tile.source_lights[1]),
                ],
            });

            if definition.walkmesh.is_some() {
                let walkmesh_name = model_name;
                let walkmesh_key = format!("wok:{}", walkmesh_name.to_ascii_lowercase());
                let walkmesh_index = if let Some(index) = model_indices.get(&walkmesh_key).copied()
                {
                    Some(index)
                } else {
                    let filename = format!("{walkmesh_name}.wok");
                    let resolved = ResolvedResRef::from_filename(&filename).map_err(|error| {
                        SceneError::invalid(format!("invalid walkmesh {filename}: {error}"))
                    })?;
                    match self.resman.get_resolved(&resolved) {
                        Some(resource) => {
                            let bytes = resource.read_all(CachePolicy::Use).map_err(|error| {
                                SceneError::invalid(format!("read {filename}: {error}"))
                            })?;
                            match parse_scene_resource_auto(
                                ModelResourceKind::Walkmesh,
                                walkmesh_name,
                                &bytes,
                            ) {
                                Ok(scene) => {
                                    let dependency_id = dependencies.record(
                                        filename.clone(),
                                        DependencyKind::Walkmesh,
                                        DependencyState::Resolved,
                                        Some(resource.origin().to_string()),
                                        None,
                                    );
                                    dependencies.connect(
                                        set_id,
                                        dependency_id,
                                        format!("tile:{tile_id}:walkmesh"),
                                    );
                                    let index = models.len();
                                    models.push(SceneModel::Auxiliary(scene));
                                    model_indices.insert(walkmesh_key, index);
                                    Some(index)
                                }
                                Err(error) => {
                                    let dependency_id = dependencies.record(
                                        filename.clone(),
                                        DependencyKind::Walkmesh,
                                        DependencyState::Invalid,
                                        Some(resource.origin().to_string()),
                                        Some(error.to_string()),
                                    );
                                    dependencies.connect(
                                        set_id,
                                        dependency_id,
                                        format!("tile:{tile_id}:walkmesh"),
                                    );
                                    diagnostics.push(SceneDiagnostic {
                                        severity: SceneDiagnosticSeverity::Error,
                                        code:     "area.tile.walkmeshInvalid".into(),
                                        message:  format!(
                                            "could not parse tile {tile_id} walkmesh {filename}: \
                                             {error}"
                                        ),
                                        resource: Some(filename),
                                    });
                                    None
                                }
                            }
                        }
                        None => {
                            let dependency_id = dependencies.record(
                                filename.clone(),
                                DependencyKind::Walkmesh,
                                DependencyState::Missing,
                                None,
                                Some("resource was not found in the layered resource view".into()),
                            );
                            dependencies.connect(
                                set_id,
                                dependency_id,
                                format!("tile:{tile_id}:walkmesh"),
                            );
                            diagnostics.push(SceneDiagnostic {
                                severity: SceneDiagnosticSeverity::Error,
                                code:     "area.tile.walkmeshMissing".into(),
                                message:  format!(
                                    "tile {tile_id} requires missing walkmesh {filename}"
                                ),
                                resource: Some(filename),
                            });
                            None
                        }
                    }
                };
                if let Some(walkmesh_index) = walkmesh_index {
                    instances.push(SceneInstance {
                        id:                    instances.len(),
                        object_key:            None,
                        label:                 format!("Tile {} collision", tile.index),
                        kind:                  SceneInstanceKind::Collision,
                        model:                 Some(walkmesh_index),
                        resource:              Some(format!("{walkmesh_name}.wok")),
                        position:              [
                            tile.x as f32 * 10.0,
                            tile.y as f32 * 10.0,
                            tile.height.unwrap_or_default() as f32 * 5.0,
                        ],
                        rotation_axis_angle:   [0.0, 0.0, 1.0, orientation],
                        scale:                 [1.0; 3],
                        polygon:               Vec::new(),
                        light_color_overrides: [None; 4],
                    });
                }
            }
        }
        self.append_git_visuals(
            &git,
            &area_objects,
            git_id,
            &mut model_indices,
            &mut models,
            &mut instances,
            &mut dependencies,
            &mut diagnostics,
        )?;
        append_git_overlays(&git, &area_objects, &mut instances)?;
        self.append_skybox(
            &area,
            area_id,
            &mut model_indices,
            &mut models,
            &mut instances,
            &mut dependencies,
            &mut diagnostics,
        );

        self.append_emitter_chunk_models(&mut models, &mut dependencies, &mut diagnostics);
        let (model_assets, textures, shaders) =
            resolve_model_assets(self.resman, &models, &mut dependencies, &mut diagnostics)?;

        Ok(SceneDocument {
            name: area_name.to_string(),
            source: SceneSource::Area,
            models,
            model_assets,
            textures,
            shaders,
            instances,
            area: Some(SceneArea {
                area: area.clone(),
                instances: git,
                tileset,
            }),
            module: None,
            environment: SceneEnvironment::Nwn(SceneAreaEnvironment::from(&area.environment)),
            dependencies,
            diagnostics,
        })
    }

    /// Loads a module IFO and assembles its authored entry area. The returned
    /// module catalog allows the persistent viewer session to switch to every
    /// other declared area without rebuilding the resource view.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when the IFO has no areas or the selected
    /// entry area cannot be assembled.
    pub fn load_module(&mut self, module_name: &str) -> SceneResult<SceneDocument> {
        self.load_module_area(module_name, None)
    }

    /// Loads one explicitly selected area from a module IFO while preserving
    /// the complete module area catalog in the returned scene.
    ///
    /// # Errors
    ///
    /// Returns [`SceneError`] when the requested area is not declared by
    /// the module or cannot be assembled.
    pub fn load_module_area(
        &mut self,
        module_name: &str,
        selected_area: Option<&str>,
    ) -> SceneResult<SceneDocument> {
        let info = ModuleInfo::from_resman(self.resman, module_name, CachePolicy::Use)
            .map_err(|error| SceneError::invalid(error.to_string()))?;
        let entry_area = selected_area
            .map(str::to_string)
            .or_else(|| info.entry.area.clone())
            .or_else(|| info.areas.first().cloned())
            .ok_or_else(|| SceneError::invalid(format!("{module_name}.ifo declares no areas")))?;
        if !info
            .areas
            .iter()
            .any(|area| area.eq_ignore_ascii_case(&entry_area))
        {
            return Err(SceneError::invalid(format!(
                "{module_name}.ifo entry area {entry_area} is not present in Mod_Area_list"
            )));
        }
        let mut scene = self.load_area(&entry_area)?;
        let module_id = self.record_resolved(
            &mut scene.dependencies,
            &format!("{module_name}.ifo"),
            DependencyKind::Module,
        );
        if let Some(area_id) = scene.dependencies.id_for(&format!("{entry_area}.are")) {
            scene.dependencies.connect(module_id, area_id, "entryArea");
        }
        scene.name = module_name.to_string();
        scene.source = SceneSource::Module;
        scene.module = Some(SceneModule {
            areas: info.areas,
            entry_area,
            entry_position: info.entry.position,
            entry_direction: info.entry.direction,
            custom_tlk: info.custom_tlk,
            haks: info.haks,
        });
        Ok(scene)
    }

    fn append_git_visuals(
        &mut self,
        git: &GitFile,
        area_objects: &[SceneAreaObject],
        git_id: usize,
        model_indices: &mut std::collections::BTreeMap<String, usize>,
        models: &mut Vec<SceneModel>,
        instances: &mut Vec<SceneInstance>,
        dependencies: &mut DependencyGraph,
        diagnostics: &mut Vec<SceneDiagnostic>,
    ) -> SceneResult<()> {
        for (index, creature) in git.creatures.iter().enumerate() {
            self.append_blueprint_instance(
                NwnBlueprintKind::Creature,
                area_object_key(area_objects, SceneInstanceKind::Creature, index)?,
                creature.template_resref.as_deref(),
                &creature.raw,
                creature.tag.as_deref().unwrap_or("Creature"),
                &creature.transform,
                git_id,
                model_indices,
                models,
                instances,
                dependencies,
                diagnostics,
            );
        }
        for (index, door) in git.doors.iter().enumerate() {
            self.append_blueprint_instance(
                NwnBlueprintKind::Door,
                area_object_key(area_objects, SceneInstanceKind::Door, index)?,
                door.template_resref.as_deref(),
                &door.raw,
                door.tag.as_deref().unwrap_or("Door"),
                &door.transform,
                git_id,
                model_indices,
                models,
                instances,
                dependencies,
                diagnostics,
            );
        }
        for (index, placeable) in git.placeables.iter().enumerate() {
            self.append_blueprint_instance(
                NwnBlueprintKind::Placeable,
                area_object_key(area_objects, SceneInstanceKind::Placeable, index)?,
                placeable.template_resref.as_deref(),
                &placeable.raw,
                placeable.tag.as_deref().unwrap_or("Placeable"),
                &placeable.transform,
                git_id,
                model_indices,
                models,
                instances,
                dependencies,
                diagnostics,
            );
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn append_skybox(
        &mut self,
        area: &AreFile,
        area_id: usize,
        model_indices: &mut std::collections::BTreeMap<String, usize>,
        models: &mut Vec<SceneModel>,
        instances: &mut Vec<SceneInstance>,
        dependencies: &mut DependencyGraph,
        diagnostics: &mut Vec<SceneDiagnostic>,
    ) {
        let Some(skybox_id) = area
            .environment
            .skybox
            .and_then(|value| usize::try_from(value).ok())
        else {
            return;
        };
        if skybox_id == 0 {
            return;
        }
        let table_id = self.record_resolved(dependencies, "skyboxes.2da", DependencyKind::TwoDa);
        dependencies.connect(area_id, table_id, "skyboxCatalog");
        let table_resource = ResolvedResRef::from_filename("skyboxes.2da")
            .ok()
            .and_then(|resolved| self.resman.get_resolved(&resolved));
        let Some(table_resource) = table_resource else {
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Error,
                code:     "area.skybox.catalogMissing".into(),
                message:  "area selects a skybox, but skyboxes.2da is unavailable".into(),
                resource: Some("skyboxes.2da".into()),
            });
            return;
        };
        let table_bytes = match table_resource.read_all(CachePolicy::Use) {
            Ok(bytes) => bytes,
            Err(error) => {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     "area.skybox.catalogReadFailed".into(),
                    message:  format!("could not read skyboxes.2da: {error}"),
                    resource: Some("skyboxes.2da".into()),
                });
                return;
            }
        };
        let table = match read_twoda(&mut Cursor::new(table_bytes)) {
            Ok(table) => table,
            Err(error) => {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     "area.skybox.catalogInvalid".into(),
                    message:  format!("could not parse skyboxes.2da: {error}"),
                    resource: Some("skyboxes.2da".into()),
                });
                return;
            }
        };
        let column = if area.environment.is_night == Some(true) {
            "NIGHT"
        } else {
            "DAY"
        };
        let Some(model_name) = table
            .cell(skybox_id, column)
            .filter(|value| !value.trim().is_empty() && !value.eq_ignore_ascii_case("****"))
        else {
            diagnostics.push(SceneDiagnostic {
                severity: SceneDiagnosticSeverity::Error,
                code:     "area.skybox.rowInvalid".into(),
                message:  format!("skyboxes.2da row {skybox_id} has no {column} model"),
                resource: Some("skyboxes.2da".into()),
            });
            return;
        };
        let model_key = format!("mdl:{}", model_name.to_ascii_lowercase());
        let model_index = if let Some(index) = model_indices.get(&model_key).copied() {
            index
        } else {
            match load_composed_scene_from_resman(
                self.resman,
                &model_name,
                &NwnAppearanceOverrides::default(),
            ) {
                Ok(model) => {
                    let model_id = self.record_resolved(
                        dependencies,
                        &format!("{model_name}.mdl"),
                        DependencyKind::Model,
                    );
                    dependencies.connect(table_id, model_id, format!("row:{skybox_id}:{column}"));
                    self.record_composed_dependencies(model_id, &model, dependencies);
                    let index = models.len();
                    models.push(SceneModel::Composed(model));
                    model_indices.insert(model_key, index);
                    index
                }
                Err(error) => {
                    diagnostics.push(SceneDiagnostic {
                        severity: SceneDiagnosticSeverity::Error,
                        code:     "area.skybox.modelFailed".into(),
                        message:  format!("could not load skybox model {model_name}: {error}"),
                        resource: Some(format!("{model_name}.mdl")),
                    });
                    return;
                }
            }
        };
        instances.push(SceneInstance {
            id:                    instances.len(),
            object_key:            None,
            label:                 format!("Skybox {skybox_id} ({model_name})"),
            kind:                  SceneInstanceKind::Skybox,
            model:                 Some(model_index),
            resource:              Some(format!("{model_name}.mdl")),
            position:              [0.0; 3],
            rotation_axis_angle:   [0.0, 0.0, 1.0, 0.0],
            scale:                 [1.0; 3],
            polygon:               Vec::new(),
            light_color_overrides: [None; 4],
        });
    }

    fn load_light_palette(
        &mut self,
        area: &AreFile,
        area_id: usize,
        dependencies: &mut DependencyGraph,
        diagnostics: &mut Vec<SceneDiagnostic>,
    ) -> Vec<Option<[f32; 3]>> {
        if !area.tiles.iter().any(|tile| {
            tile.main_lights
                .iter()
                .chain(&tile.source_lights)
                .any(Option::is_some)
        }) {
            return Vec::new();
        }
        let table_id = self.record_resolved(dependencies, "lightcolor.2da", DependencyKind::TwoDa);
        dependencies.connect(area_id, table_id, "tileLightPalette");
        let result = (|| {
            let resolved = ResolvedResRef::from_filename("lightcolor.2da")
                .map_err(|error| error.to_string())?;
            let resource = self
                .resman
                .get_resolved(&resolved)
                .ok_or_else(|| "lightcolor.2da was not found".to_string())?;
            let bytes = resource
                .read_all(CachePolicy::Use)
                .map_err(|error| error.to_string())?;
            let table = read_twoda(&mut Cursor::new(bytes)).map_err(|error| error.to_string())?;
            Ok::<_, String>(
                (0..table.rows.len())
                    .map(|row| {
                        let [Some(red), Some(green), Some(blue)] =
                            ["RED", "GREEN", "BLUE"].map(|column| {
                                table.cell(row, column).and_then(|value| value.parse().ok())
                            })
                        else {
                            return None;
                        };
                        Some([red, green, blue])
                    })
                    .collect(),
            )
        })();
        match result {
            Ok(palette) => palette,
            Err(error) => {
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     "area.lighting.paletteInvalid".into(),
                    message:  format!("could not load tile light palette: {error}"),
                    resource: Some("lightcolor.2da".into()),
                });
                Vec::new()
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn append_blueprint_instance(
        &mut self,
        kind: NwnBlueprintKind,
        object_key: &str,
        template: Option<&str>,
        raw: &GffStruct,
        label: &str,
        transform: &GitTransform,
        git_id: usize,
        model_indices: &mut std::collections::BTreeMap<String, usize>,
        models: &mut Vec<SceneModel>,
        instances: &mut Vec<SceneInstance>,
        dependencies: &mut DependencyGraph,
        diagnostics: &mut Vec<SceneDiagnostic>,
    ) {
        let blueprint_id = template.map(|name| {
            let filename = format!("{name}.{}", kind.extension());
            let id = self.record_resolved(dependencies, &filename, DependencyKind::Blueprint);
            dependencies.connect(git_id, id, format!("{}:template", kind.extension()));
            id
        });
        let visual = match self.compose_instance_visual(kind, template, raw) {
            Ok(visual) => visual,
            Err(error) => {
                if let Some(id) = blueprint_id
                    && let Some(node) = dependencies.nodes.get_mut(id)
                {
                    node.state = DependencyState::Invalid;
                    node.message = Some(error.to_string());
                }
                diagnostics.push(SceneDiagnostic {
                    severity: SceneDiagnosticSeverity::Error,
                    code:     format!("area.{}.visualFailed", kind.extension()),
                    message:  format!("could not resolve {label} visual: {error}"),
                    resource: template.map(|name| format!("{name}.{}", kind.extension())),
                });
                return;
            }
        };
        for composed in visual.models {
            let model_name = composed.model_name.clone();
            let model_id = self.record_resolved(
                dependencies,
                &format!("{}.mdl", composed.model_name),
                DependencyKind::Model,
            );
            dependencies.connect(
                blueprint_id.unwrap_or(git_id),
                model_id,
                format!("{}:visual", kind.extension()),
            );
            self.record_composed_dependencies(model_id, &composed, dependencies);
            let model_index = models
                .iter()
                .position(|existing| matches!(existing, SceneModel::Composed(existing) if existing == &composed))
                .unwrap_or_else(|| {
                    let index = models.len();
                    models.push(SceneModel::Composed(composed));
                    index
                });
            instances.push(SceneInstance {
                id:                    instances.len(),
                object_key:            Some(object_key.to_string()),
                label:                 label.to_string(),
                kind:                  render_kind_for_blueprint(kind),
                model:                 Some(model_index),
                resource:              Some(format!("{model_name}.mdl")),
                position:              transform_position(transform),
                rotation_axis_angle:   transform_rotation(transform),
                scale:                 [1.0; 3],
                polygon:               Vec::new(),
                light_color_overrides: [None; 4],
            });
            self.append_blueprint_collision(
                kind,
                &model_name,
                Some(object_key),
                blueprint_id.unwrap_or(git_id),
                transform_position(transform),
                transform_rotation(transform),
                model_indices,
                models,
                instances,
                dependencies,
                diagnostics,
            );
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn append_blueprint_collision(
        &mut self,
        kind: NwnBlueprintKind,
        model_name: &str,
        object_key: Option<&str>,
        parent_id: usize,
        position: [f32; 3],
        rotation: [f32; 4],
        model_indices: &mut std::collections::BTreeMap<String, usize>,
        models: &mut Vec<SceneModel>,
        instances: &mut Vec<SceneInstance>,
        dependencies: &mut DependencyGraph,
        diagnostics: &mut Vec<SceneDiagnostic>,
    ) {
        let (resource_kind, extension) = match kind {
            NwnBlueprintKind::Door => (ModelResourceKind::DoorWalkmesh, "dwk"),
            NwnBlueprintKind::Placeable => (ModelResourceKind::PlaceableWalkmesh, "pwk"),
            NwnBlueprintKind::Creature | NwnBlueprintKind::Item => return,
        };
        let filename = format!("{model_name}.{extension}");
        let cache_key = filename.to_ascii_lowercase();
        let model_index = if let Some(index) = model_indices.get(&cache_key).copied() {
            Some(index)
        } else {
            let resolved = match ResolvedResRef::from_filename(&filename) {
                Ok(resolved) => resolved,
                Err(error) => {
                    diagnostics.push(SceneDiagnostic {
                        severity: SceneDiagnosticSeverity::Error,
                        code:     "blueprint.collision.invalidReference".into(),
                        message:  error.to_string(),
                        resource: Some(filename),
                    });
                    return;
                }
            };
            match self.resman.get_resolved(&resolved) {
                Some(resource) => match resource
                    .read_all(CachePolicy::Use)
                    .map_err(|error| error.to_string())
                    .and_then(|bytes| {
                        parse_scene_resource_auto(resource_kind, model_name, &bytes)
                            .map_err(|error| error.to_string())
                    }) {
                    Ok(scene) => {
                        let id = dependencies.record(
                            filename.clone(),
                            DependencyKind::Walkmesh,
                            DependencyState::Resolved,
                            Some(resource.origin().to_string()),
                            None,
                        );
                        dependencies.connect(parent_id, id, "collision");
                        let index = models.len();
                        models.push(SceneModel::Auxiliary(scene));
                        model_indices.insert(cache_key, index);
                        Some(index)
                    }
                    Err(error) => {
                        let id = dependencies.record(
                            filename.clone(),
                            DependencyKind::Walkmesh,
                            DependencyState::Invalid,
                            Some(resource.origin().to_string()),
                            Some(error.to_string()),
                        );
                        dependencies.connect(parent_id, id, "collision");
                        diagnostics.push(SceneDiagnostic {
                            severity: SceneDiagnosticSeverity::Error,
                            code:     "blueprint.collision.invalid".into(),
                            message:  format!("could not parse {filename}: {error}"),
                            resource: Some(filename.clone()),
                        });
                        None
                    }
                },
                None => {
                    let id = dependencies.record(
                        filename.clone(),
                        DependencyKind::Walkmesh,
                        DependencyState::OptionalMissing,
                        None,
                        Some("the visual model has no companion collision resource".into()),
                    );
                    dependencies.connect(parent_id, id, "collision");
                    None
                }
            }
        };
        if let Some(model) = model_index {
            instances.push(SceneInstance {
                id: instances.len(),
                object_key: object_key.map(str::to_string),
                label: format!("{model_name} collision"),
                kind: SceneInstanceKind::Collision,
                model: Some(model),
                resource: Some(filename),
                position,
                rotation_axis_angle: rotation,
                scale: [1.0; 3],
                polygon: Vec::new(),
                light_color_overrides: [None; 4],
            });
        }
    }

    fn compose_instance_visual(
        &mut self,
        kind: NwnBlueprintKind,
        template: Option<&str>,
        raw: &GffStruct,
    ) -> SceneResult<NwnBlueprintVisual> {
        let mut root = if let Some(template) = template {
            let resolved =
                ResolvedResRef::from_filename(&format!("{template}.{}", kind.extension()))
                    .map_err(|error| SceneError::invalid(error.to_string()))?;
            let resource = self
                .resman
                .get_resolved(&resolved)
                .ok_or_else(|| SceneError::missing(resolved.to_string()))?;
            let bytes = resource
                .read_all(CachePolicy::Use)
                .map_err(|error| SceneError::invalid(error.to_string()))?;
            read_gff_root(&mut Cursor::new(bytes))
                .map_err(|error| SceneError::invalid(error.to_string()))?
        } else {
            GffRoot::new(match kind {
                NwnBlueprintKind::Creature => "UTC ",
                NwnBlueprintKind::Door => "UTD ",
                NwnBlueprintKind::Placeable => "UTP ",
                NwnBlueprintKind::Item => "UTI ",
            })
        };
        for (label, field) in raw.fields() {
            root.root
                .put_field(label.clone(), field.clone())
                .map_err(|error| SceneError::invalid(error.to_string()))?;
        }
        compose_blueprint_visual_from_root(self.resman, kind, &root)
            .map_err(|error| SceneError::scene(error.to_string()))
    }

    fn append_emitter_chunk_models(
        &mut self,
        models: &mut Vec<SceneModel>,
        dependencies: &mut DependencyGraph,
        diagnostics: &mut Vec<SceneDiagnostic>,
    ) {
        let mut source_index = 0;
        while let Some(source) = models.get(source_index) {
            let references = emitter_chunk_references(source);
            source_index += 1;
            for (parent, chunk_name) in references {
                if models
                    .iter()
                    .any(|model| model_tree_contains_name(model, &chunk_name))
                {
                    continue;
                }
                let parent_id = self.record_resolved(
                    dependencies,
                    &format!("{parent}.mdl"),
                    DependencyKind::Model,
                );
                match load_composed_scene_from_resman(
                    self.resman,
                    &chunk_name,
                    &NwnAppearanceOverrides::default(),
                ) {
                    Ok(chunk) => {
                        let chunk_id = self.record_resolved(
                            dependencies,
                            &format!("{chunk_name}.mdl"),
                            DependencyKind::ReferenceModel,
                        );
                        dependencies.connect(parent_id, chunk_id, "emitterChunk");
                        self.record_composed_dependencies(chunk_id, &chunk, dependencies);
                        models.push(SceneModel::Composed(chunk));
                    }
                    Err(error) => {
                        let chunk_id = dependencies.record(
                            format!("{chunk_name}.mdl"),
                            DependencyKind::ReferenceModel,
                            DependencyState::Missing,
                            None,
                            Some(error.to_string()),
                        );
                        dependencies.connect(parent_id, chunk_id, "emitterChunk");
                        diagnostics.push(SceneDiagnostic {
                            severity: SceneDiagnosticSeverity::Error,
                            code:     "emitter.chunkModel.missing".into(),
                            message:  format!(
                                "{parent} references missing emitter chunk model {chunk_name}: \
                                 {error}"
                            ),
                            resource: Some(format!("{chunk_name}.mdl")),
                        });
                    }
                }
            }
        }
    }

    fn record_resolved(
        &mut self,
        dependencies: &mut DependencyGraph,
        filename: &str,
        kind: DependencyKind,
    ) -> usize {
        let Ok(resolved) = ResolvedResRef::from_filename(filename) else {
            return dependencies.record(
                filename,
                kind,
                DependencyState::Invalid,
                None,
                Some("invalid resource reference".into()),
            );
        };
        match self.resman.get_resolved(&resolved) {
            Some(resource) => dependencies.record(
                filename,
                kind,
                DependencyState::Resolved,
                Some(resource.origin().to_string()),
                None,
            ),
            None => dependencies.record(
                filename,
                kind,
                DependencyState::Missing,
                None,
                Some("resource was not found in the layered resource view".into()),
            ),
        }
    }

    fn record_composed_dependencies(
        &mut self,
        parent_id: usize,
        composed: &NwnComposedScene,
        dependencies: &mut DependencyGraph,
    ) {
        if let Some(supermodel) = composed
            .scene
            .supermodel
            .as_deref()
            .filter(|name| !name.eq_ignore_ascii_case("null"))
            .filter(|name| !name.eq_ignore_ascii_case(&composed.model_name))
        {
            let id = self.record_resolved(
                dependencies,
                &format!("{supermodel}.mdl"),
                DependencyKind::Supermodel,
            );
            dependencies.connect(parent_id, id, "supermodel");
        }
        for attachment in &composed.attachments {
            let id = self.record_resolved(
                dependencies,
                &format!("{}.mdl", attachment.model_name),
                DependencyKind::ReferenceModel,
            );
            dependencies.connect(
                parent_id,
                id,
                format!("refmodel:{}", attachment.target_node_name),
            );
            self.record_composed_dependencies(id, &attachment.scene, dependencies);
        }
    }
}

fn emitter_chunk_references(model: &SceneModel) -> Vec<(String, String)> {
    fn collect(scene: &NwnComposedScene, target: &mut Vec<(String, String)>) {
        collect_scene_emitter_chunks(&scene.scene, target);
        for attachment in &scene.attachments {
            collect(&attachment.scene, target);
        }
    }
    let mut result = Vec::new();
    match model {
        SceneModel::Composed(scene) => collect(scene, &mut result),
        SceneModel::Auxiliary(scene) => collect_scene_emitter_chunks(scene, &mut result),
    }
    result
}

fn collect_scene_emitter_chunks(
    scene: &nwnrs_types::mdl::NwnScene,
    target: &mut Vec<(String, String)>,
) {
    for node in &scene.nodes {
        let Some(emitter) = &node.emitter else {
            continue;
        };
        let explosion = emitter.properties.iter().any(|property| {
            property.name.eq_ignore_ascii_case("update")
                && property.values.iter().any(|value| {
                    matches!(value, NwnPropertyValue::Text(value) if value.eq_ignore_ascii_case("explosion"))
                })
        });
        if !explosion {
            continue;
        }
        let Some(chunk_name) = emitter.properties.iter().find_map(|property| {
            property
                .name
                .eq_ignore_ascii_case("chunkname")
                .then(|| property.values.first())
                .flatten()
                .and_then(|value| match value {
                    NwnPropertyValue::Text(value)
                        if !value.trim().is_empty() && !value.eq_ignore_ascii_case("null") =>
                    {
                        Some(value.trim().to_string())
                    }
                    _ => None,
                })
        }) else {
            continue;
        };
        if !target.iter().any(|(parent, chunk)| {
            parent.eq_ignore_ascii_case(&scene.name) && chunk.eq_ignore_ascii_case(&chunk_name)
        }) {
            target.push((scene.name.clone(), chunk_name));
        }
    }
}

fn model_tree_contains_name(model: &SceneModel, name: &str) -> bool {
    fn composed_contains(scene: &NwnComposedScene, name: &str) -> bool {
        scene.model_name.eq_ignore_ascii_case(name)
            || scene
                .attachments
                .iter()
                .any(|attachment| composed_contains(&attachment.scene, name))
    }
    match model {
        SceneModel::Composed(scene) => composed_contains(scene, name),
        SceneModel::Auxiliary(scene) => scene.name.eq_ignore_ascii_case(name),
    }
}

fn area_object_key(
    objects: &[SceneAreaObject],
    kind: SceneInstanceKind,
    source_index: usize,
) -> SceneResult<&str> {
    objects
        .iter()
        .find(|object| object.kind == kind && object.source_index == source_index)
        .map(|object| object.key.as_str())
        .ok_or_else(|| {
            SceneError::scene(format!(
                "area object catalog is missing {kind:?} GIT entry {source_index}"
            ))
        })
}

fn append_git_overlays(
    git: &GitFile,
    area_objects: &[SceneAreaObject],
    target: &mut Vec<SceneInstance>,
) -> SceneResult<()> {
    for (index, trigger) in git.triggers.iter().enumerate() {
        push_polygon_instance(
            target,
            area_object_key(area_objects, SceneInstanceKind::Trigger, index)?,
            trigger.tag.as_deref().unwrap_or("Trigger"),
            SceneInstanceKind::Trigger,
            &trigger.transform,
            &trigger.geometry,
        );
    }
    for (index, encounter) in git.encounters.iter().enumerate() {
        push_polygon_instance(
            target,
            area_object_key(area_objects, SceneInstanceKind::Encounter, index)?,
            encounter.tag.as_deref().unwrap_or("Encounter"),
            SceneInstanceKind::Encounter,
            &encounter.transform,
            &encounter.geometry,
        );
    }
    for (index, waypoint) in git.waypoints.iter().enumerate() {
        push_marker_instance(
            target,
            area_object_key(area_objects, SceneInstanceKind::Waypoint, index)?,
            waypoint.tag.as_deref().unwrap_or("Waypoint"),
            SceneInstanceKind::Waypoint,
            &waypoint.transform,
            0.35,
            4,
        );
    }
    for (index, sound) in git.sounds.iter().enumerate() {
        push_marker_instance(
            target,
            area_object_key(area_objects, SceneInstanceKind::Sound, index)?,
            sound.tag.as_deref().unwrap_or("Sound"),
            SceneInstanceKind::Sound,
            &sound.transform,
            sound.max_distance.unwrap_or(1.0).max(0.1),
            48,
        );
    }
    for (index, store) in git.stores.iter().enumerate() {
        push_marker_instance(
            target,
            area_object_key(area_objects, SceneInstanceKind::Store, index)?,
            store.tag.as_deref().unwrap_or("Store"),
            SceneInstanceKind::Store,
            &store.transform,
            0.35,
            4,
        );
    }
    Ok(())
}

const fn source_for_blueprint(kind: NwnBlueprintKind) -> SceneSource {
    match kind {
        NwnBlueprintKind::Creature => SceneSource::Creature,
        NwnBlueprintKind::Door => SceneSource::Door,
        NwnBlueprintKind::Placeable => SceneSource::Placeable,
        NwnBlueprintKind::Item => SceneSource::Item,
    }
}

const fn render_kind_for_blueprint(kind: NwnBlueprintKind) -> SceneInstanceKind {
    match kind {
        NwnBlueprintKind::Creature => SceneInstanceKind::Creature,
        NwnBlueprintKind::Door => SceneInstanceKind::Door,
        NwnBlueprintKind::Placeable => SceneInstanceKind::Placeable,
        NwnBlueprintKind::Item => SceneInstanceKind::Item,
    }
}

fn transform_position(transform: &GitTransform) -> [f32; 3] {
    [
        transform.x.unwrap_or_default(),
        transform.y.unwrap_or_default(),
        transform.z.unwrap_or_default(),
    ]
}

fn transform_rotation(transform: &GitTransform) -> [f32; 4] {
    let angle = transform.bearing.unwrap_or_else(|| {
        transform
            .y_orientation
            .unwrap_or_default()
            .atan2(transform.x_orientation.unwrap_or(1.0))
    });
    [0.0, 0.0, 1.0, angle]
}

fn palette_color(palette: &[Option<[f32; 3]>], index: Option<i32>) -> Option<[f32; 3]> {
    index
        .and_then(|value| usize::try_from(value).ok())
        .and_then(|value| palette.get(value).copied().flatten())
}

fn push_marker_instance(
    target: &mut Vec<SceneInstance>,
    object_key: &str,
    label: &str,
    kind: SceneInstanceKind,
    transform: &nwnrs_types::gff::GitTransform,
    radius: f32,
    segments: usize,
) {
    target.push(SceneInstance {
        id: target.len(),
        object_key: Some(object_key.to_string()),
        label: label.to_string(),
        kind,
        model: None,
        resource: None,
        position: transform_position(transform),
        rotation_axis_angle: transform_rotation(transform),
        scale: [1.0; 3],
        polygon: (0..segments)
            .map(|index| {
                let angle = index as f32 / segments as f32 * std::f32::consts::TAU;
                [angle.cos() * radius, angle.sin() * radius, 0.05]
            })
            .collect(),
        light_color_overrides: [None; 4],
    });
}

fn push_polygon_instance(
    target: &mut Vec<SceneInstance>,
    object_key: &str,
    label: &str,
    kind: SceneInstanceKind,
    transform: &nwnrs_types::gff::GitTransform,
    points: &[nwnrs_types::gff::GitPoint],
) {
    target.push(SceneInstance {
        id: target.len(),
        object_key: Some(object_key.to_string()),
        label: label.to_string(),
        kind,
        model: None,
        resource: None,
        position: transform_position(transform),
        rotation_axis_angle: transform_rotation(transform),
        scale: [1.0; 3],
        polygon: points
            .iter()
            .map(|point| {
                [
                    point.x.unwrap_or_default(),
                    point.y.unwrap_or_default(),
                    point.z.unwrap_or_default(),
                ]
            })
            .collect(),
        light_color_overrides: [None; 4],
    });
}

#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use nwnrs_types::{
        gff::{GffRoot, GffStruct, GffValue, GitFile, write_gff_root, write_git},
        resman::{ResContainer, ResMan, ResolvedResRef, read_resmemfile},
    };

    use crate::scene::{SceneInstanceKind, SceneLoader, SceneSource};

    #[test]
    fn loads_model_resource_through_shared_scene_assembler() {
        let mut manager = ResMan::new(1);
        add_resource(
            &mut manager,
            "demo.mdl",
            b"newmodel demo\nsetsupermodel demo null\nbeginmodelgeom demo\nnode dummy demo\n parent null\nendnode\nendmodelgeom demo\ndonemodel demo\n",
        );

        let resource = ResolvedResRef::from_filename("demo.mdl")
            .unwrap_or_else(|error| panic!("resolve model: {error}"));
        let scene = SceneLoader::new(&mut manager)
            .load_model(&resource)
            .unwrap_or_else(|error| panic!("load model: {error}"));

        assert_eq!(scene.source, SceneSource::Model);
        assert_eq!(scene.models.len(), 1);
        assert_eq!(scene.instances.len(), 1);
        assert_eq!(scene.dependencies.nodes.len(), 1);
    }

    #[test]
    fn resolves_explosion_emitters_as_chunk_models_not_sprite_textures() {
        let mut manager = ResMan::new(1);
        add_resource(
            &mut manager,
            "emitter.mdl",
            b"newmodel emitter\nsetsupermodel emitter null\nbeginmodelgeom emitter\nnode dummy emitter\n parent null\nendnode\nnode emitter debris\n parent emitter\n update Explosion\n chunkname chunk\n texture intentionally_missing\nendnode\nendmodelgeom emitter\ndonemodel emitter\n",
        );
        add_resource(
            &mut manager,
            "chunk.mdl",
            b"newmodel chunk\nsetsupermodel chunk null\nbeginmodelgeom chunk\nnode dummy chunk\n parent null\nendnode\nendmodelgeom chunk\ndonemodel chunk\n",
        );
        let resource = ResolvedResRef::from_filename("emitter.mdl")
            .unwrap_or_else(|error| panic!("resolve emitter: {error}"));
        let scene = SceneLoader::new(&mut manager)
            .load_model(&resource)
            .unwrap_or_else(|error| panic!("load emitter: {error}"));

        assert_eq!(scene.models.len(), 2);
        assert!(scene.dependencies.id_for("chunk.mdl").is_some());
        assert!(
            scene
                .diagnostics
                .iter()
                .all(|diagnostic| diagnostic.code != "node.texture.missing")
        );
    }

    #[test]
    fn assembles_area_tiles_and_volume_overlays() {
        let mut manager = ResMan::new(1);
        add_resource(
            &mut manager,
            "tilemodel.mdl",
            b"newmodel tilemodel\nsetsupermodel tilemodel null\nclassification tile\nbeginmodelgeom tilemodel\nnode dummy tilemodel\n parent null\nendnode\nendmodelgeom tilemodel\ndonemodel tilemodel\n",
        );
        add_resource(
            &mut manager,
            "tilemodel.wok",
            b"beginwalkmeshgeom tilewalk\nnode aabb walkmesh\n parent tilewalk\n verts 3\n  0 0 0\n  1 0 0\n  0 1 0\n faces 1\n  0 1 2 0 0 1 2 3\nendnode\nendwalkmeshgeom tilewalk\n",
        );
        add_resource(
            &mut manager,
            "sky_night.mdl",
            b"newmodel sky_night\nsetsupermodel sky_night null\nclassification other\nbeginmodelgeom sky_night\nnode dummy sky_night\n parent null\nendnode\nendmodelgeom sky_night\ndonemodel sky_night\n",
        );
        add_resource(
            &mut manager,
            "skyboxes.2da",
            b"2DA V2.0\n\n LABEL DAY NIGHT\n0 None **** ****\n1 Test sky_day sky_night\n",
        );
        add_resource(
            &mut manager,
            "lightcolor.2da",
            b"2DA V2.0\n\n RED GREEN BLUE\n0 0 0 0\n1 1.2 0.5 0.25\n",
        );
        add_resource(
            &mut manager,
            "testset.set",
            br#"[GENERAL]
Name=TESTSET
Type=SET
Version=V1.0
Interior=0
[TILES]
Count=1
[TILE0]
Model=tilemodel
WalkMesh=tilewalk
"#,
        );

        let mut area = GffRoot::new("ARE ");
        area.put_value("ResRef", GffValue::ResRef("testarea".into()))
            .unwrap_or_else(|error| panic!("area resref: {error}"));
        area.put_value("Width", GffValue::Int(1))
            .unwrap_or_else(|error| panic!("area width: {error}"));
        area.put_value("Height", GffValue::Int(1))
            .unwrap_or_else(|error| panic!("area height: {error}"));
        area.put_value("Tileset", GffValue::ResRef("testset".into()))
            .unwrap_or_else(|error| panic!("area tileset: {error}"));
        area.put_value("SkyBox", GffValue::Int(1))
            .unwrap_or_else(|error| panic!("area skybox: {error}"));
        area.put_value("IsNight", GffValue::Byte(1))
            .unwrap_or_else(|error| panic!("area night flag: {error}"));
        let mut tile = GffStruct::new(1);
        tile.put_value("Tile_ID", GffValue::Int(0))
            .unwrap_or_else(|error| panic!("tile id: {error}"));
        tile.put_value("Tile_Height", GffValue::Int(1))
            .unwrap_or_else(|error| panic!("tile height: {error}"));
        tile.put_value("Tile_Orientation", GffValue::Int(2))
            .unwrap_or_else(|error| panic!("tile orientation: {error}"));
        tile.put_value("Tile_MainLight1", GffValue::Int(1))
            .unwrap_or_else(|error| panic!("tile light: {error}"));
        area.put_value("Tile_List", GffValue::List(vec![tile]))
            .unwrap_or_else(|error| panic!("tile list: {error}"));
        let mut area_bytes = Cursor::new(Vec::new());
        write_gff_root(&mut area_bytes, &area).unwrap_or_else(|error| panic!("write ARE: {error}"));
        add_resource(&mut manager, "testarea.are", area_bytes.get_ref());

        let mut git = GitFile::default();
        let mut trigger_raw = GffStruct::new(1);
        trigger_raw
            .put_value("Tag", GffValue::CExoString("trigger".into()))
            .unwrap_or_else(|error| panic!("trigger tag: {error}"));
        git.triggers.push(nwnrs_types::gff::GitTrigger {
            raw:            trigger_raw,
            tag:            Some("trigger".into()),
            localized_name: None,
            transform:      Default::default(),
            geometry:       vec![
                nwnrs_types::gff::GitPoint {
                    x: Some(0.0),
                    y: Some(0.0),
                    z: Some(0.0),
                },
                nwnrs_types::gff::GitPoint {
                    x: Some(1.0),
                    y: Some(0.0),
                    z: Some(0.0),
                },
                nwnrs_types::gff::GitPoint {
                    x: Some(0.0),
                    y: Some(1.0),
                    z: Some(0.0),
                },
            ],
        });
        let mut git_bytes = Cursor::new(Vec::new());
        write_git(&mut git_bytes, &git).unwrap_or_else(|error| panic!("write GIT: {error}"));
        add_resource(&mut manager, "testarea.git", git_bytes.get_ref());

        let scene = SceneLoader::new(&mut manager)
            .load_area("testarea")
            .unwrap_or_else(|error| panic!("load area: {error}"));

        assert_eq!(scene.source, SceneSource::Area);
        assert_eq!(scene.models.len(), 3);
        let tile = scene
            .instances
            .first()
            .unwrap_or_else(|| panic!("expected a tile instance"));
        assert_eq!(tile.kind, SceneInstanceKind::Tile);
        assert_eq!(tile.position, [0.0, 0.0, 5.0]);
        assert_eq!(
            tile.light_color_overrides.first().copied().flatten(),
            Some([1.2, 0.5, 0.25])
        );
        assert!(
            scene
                .instances
                .iter()
                .any(|instance| instance.kind == SceneInstanceKind::Trigger)
        );
        assert!(
            scene
                .instances
                .iter()
                .any(|instance| instance.kind == SceneInstanceKind::Skybox)
        );
        assert!(scene.diagnostics.is_empty());
    }

    fn add_resource(manager: &mut ResMan, filename: &str, bytes: &[u8]) {
        let resolved = ResolvedResRef::from_filename(filename)
            .unwrap_or_else(|error| panic!("resolve {filename}: {error}"));
        let resource = read_resmemfile(filename, resolved.into(), bytes.to_vec())
            .unwrap_or_else(|error| panic!("build {filename}: {error}"));
        manager.add(Arc::new(resource) as Arc<dyn ResContainer>);
    }
}
