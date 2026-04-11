use bevy::{
    asset::Handle,
    ecs::system::EntityCommands,
    light::{NotShadowCaster, NotShadowReceiver},
    mesh::Mesh3d,
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::{
        Commands, Entity, GlobalTransform, InheritedVisibility, Name, Transform, ViewVisibility,
        Visibility,
    },
};

use crate::{
    NwnModelAsset,
    animation::{animated_txi_material_component, attach_model_animation_components},
    light::{animated_light_component, point_light_from_nwn},
    visibility::{NwnHelperSurface, NwnTileFade},
};

/// Spawns one loaded NWN model into the current Bevy world.
pub fn spawn_nwn_model(commands: &mut Commands<'_, '_>, model: &NwnModelAsset) -> Entity {
    spawn_nwn_model_with_animation(commands, model, None)
}

/// Spawns one loaded NWN model and optionally forces a specific NWN animation.
pub fn spawn_nwn_model_with_animation(
    commands: &mut Commands<'_, '_>,
    model: &NwnModelAsset,
    selected_animation: Option<&str>,
) -> Entity {
    let root = commands
        .spawn((model.root_name(), spatial_components(Transform::default())))
        .id();
    let mut node_entities = Vec::with_capacity(model.nodes.len());
    let mut primitive_entities = vec![Vec::new(); model.nodes.len()];

    for node in &model.nodes {
        let entity = commands
            .spawn((
                Name::new(node.name.clone()),
                spatial_components(node.transform),
            ))
            .id();
        node_entities.push(entity);
    }

    for &root_index in &model.root_nodes {
        if let Some(entity) = node_entities.get(root_index) {
            commands.entity(root).add_child(*entity);
        }
    }

    for (index, node) in model.nodes.iter().enumerate() {
        if let Some(parent_index) = node.parent
            && let (Some(parent_entity), Some(child_entity)) =
                (node_entities.get(parent_index), node_entities.get(index))
        {
            commands.entity(*parent_entity).add_child(*child_entity);
        }

        if let Some(node_entity) = node_entities.get(index) {
            let mut entity = commands.entity(*node_entity);
            let mut txi_inserts = Vec::new();
            if let Some(light) = &node.light {
                entity.insert(point_light_from_nwn(light));
                if let Some(animated_light) = animated_light_component(light) {
                    entity.insert(animated_light);
                }
            }
            if let Some(helper_surface) = &node.helper_surface {
                entity.insert(NwnHelperSurface {
                    bitmaps:        helper_surface.bitmaps.clone(),
                    surface_labels: helper_surface.surface_labels.clone(),
                    texture_names:  helper_surface.texture_names.clone(),
                });
            }
            for primitive in &node.primitives {
                let txi_component = animated_txi_material_component(primitive);
                let primitive_entity = {
                    spawn_primitive_child(
                        &mut entity,
                        primitive.mesh.clone(),
                        primitive.material.clone(),
                        primitive.label.clone(),
                        primitive.tilefade.clone(),
                        primitive.initially_visible,
                        primitive.shadow_enabled,
                    )
                };
                primitive_entities[index].push(primitive_entity);
                if let Some(txi) = txi_component {
                    txi_inserts.push((primitive_entity, txi));
                }
            }
            drop(entity);
            for (primitive_entity, txi) in txi_inserts {
                commands.entity(primitive_entity).insert(txi);
            }
            for reference in &node.references {
                let reference_root =
                    spawn_nwn_model_with_animation(commands, &reference.model, selected_animation);
                commands.entity(*node_entity).add_child(reference_root);
            }
        }
    }

    attach_model_animation_components(
        commands,
        model,
        &node_entities,
        &primitive_entities,
        selected_animation,
    );

    root
}

fn spawn_primitive_child(
    entity: &mut EntityCommands<'_>,
    mesh: Handle<bevy::mesh::Mesh>,
    material: Handle<StandardMaterial>,
    label: String,
    tilefade: Option<crate::NwnModelTileFadeAsset>,
    initially_visible: bool,
    shadow_enabled: bool,
) -> Entity {
    let mut child_entity = None;
    entity.with_children(|children| {
        let mut child = children.spawn((
            Name::new(label),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            spatial_components(Transform::default()),
        ));
        let entity = child.id();
        if let Some(tilefade) = tilefade {
            child.insert(NwnTileFade {
                mode:               tilefade.mode,
                authored_visible:   tilefade.authored_visible,
                local_center:       tilefade.local_center,
                local_half_extents: tilefade.local_half_extents,
            });
        }
        if !initially_visible {
            child.insert(Visibility::Hidden);
        }
        if !shadow_enabled {
            child.insert((NotShadowCaster, NotShadowReceiver));
        }
        child_entity = Some(entity);
    });
    child_entity.unwrap_or(Entity::PLACEHOLDER)
}

fn spatial_components(
    transform: Transform,
) -> (
    Transform,
    GlobalTransform,
    Visibility,
    InheritedVisibility,
    ViewVisibility,
) {
    (
        transform,
        GlobalTransform::default(),
        Visibility::Inherited,
        InheritedVisibility::default(),
        ViewVisibility::default(),
    )
}

#[cfg(test)]
mod tests {
    use bevy::{
        asset::Handle,
        ecs::world::CommandQueue,
        prelude::{ChildOf, Entity, Name, PointLight, Transform, Visibility, World},
    };
    use nwnrs_mdl::prelude::{
        AnimationEvent, NodeKind, NwnAnimation, NwnCoordinateSystem, NwnMaterialTrack,
        NwnNodeAnimationTrack, NwnScene, NwnTransformTrack, ScalarKey, Vec3Key,
    };

    use crate::{
        NwnHelperSurface, NwnModelAsset, NwnModelHelperSurfaceAsset, NwnModelLightAnimationAsset,
        NwnModelLightAsset, NwnModelNodeAsset, animation::NwnAnimatedTransform,
    };

    #[test]
    fn spawn_nwn_model_attaches_point_light_to_light_node_entity() {
        let model = NwnModelAsset {
            scene:      NwnScene {
                name:              "torch".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![nwnrs_mdl::prelude::NwnSceneNode {
                    kind:            NodeKind::Light,
                    node_type:       "light".to_string(),
                    name:            "TorchLight".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: nwnrs_mdl::prelude::NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       None,
                    mesh:            None,
                }],
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        vec![NwnAnimation {
                    name:            "default".to_string(),
                    model_name:      "torch".to_string(),
                    length:          1.0,
                    transition_time: 0.0,
                    root_name:       None,
                    root_node:       None,
                    events:          Vec::<AnimationEvent>::new(),
                    node_tracks:     vec![NwnNodeAnimationTrack {
                        target_name: "TorchLight".to_string(),
                        target_node: Some(0),
                        kind:        NodeKind::Light,
                        transform:   NwnTransformTrack {
                            translation_keys:         vec![Vec3Key {
                                time:  0.0,
                                value: [0.0, 0.0, 0.0],
                            }],
                            rotation_axis_angle_keys: Vec::new(),
                            scale_keys:               Vec::new(),
                        },
                        material:    NwnMaterialTrack {
                            color_keys:            Vec::new(),
                            radius_keys:           Vec::new(),
                            alpha_keys:            Vec::new(),
                            self_illum_color_keys: Vec::new(),
                        },
                        animmesh:    None,
                    }],
                }],
                diagnostics:       Vec::new(),
            },
            nodes:      vec![NwnModelNodeAsset {
                name:           "TorchLight".to_string(),
                kind:           NodeKind::Light,
                parent:         None,
                transform:      Transform::default(),
                light:          Some(NwnModelLightAsset {
                    base_color:     [1.0, 0.6, 0.2],
                    base_radius:    4.0,
                    base_alpha:     0.5,
                    multiplier:     1.0,
                    shadow_enabled: true,
                    animation:      Some(NwnModelLightAnimationAsset {
                        length:      1.0,
                        color_keys:  vec![Vec3Key {
                            time:  0.0,
                            value: [1.0, 0.6, 0.2],
                        }],
                        radius_keys: vec![ScalarKey {
                            time:  0.0,
                            value: 4.0,
                        }],
                        alpha_keys:  vec![ScalarKey {
                            time:  0.0,
                            value: 0.5,
                        }],
                    }),
                }),
                references:     Vec::new(),
                helper_surface: None,
                primitives:     Vec::new(),
            }],
            root_nodes: vec![0],
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };

        let mut world = World::new();
        let mut queue = CommandQueue::default();
        let root = {
            let mut commands = bevy::prelude::Commands::new(&mut queue, &world);
            super::spawn_nwn_model(&mut commands, &model)
        };
        queue.apply(&mut world);

        let mut query = world.query::<(Entity, &PointLight, &ChildOf)>();
        let lights = query.iter(&world).collect::<Vec<_>>();
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].2.0, root);
        assert!(lights[0].1.shadows_enabled);

        let mut animated_query = world.query::<&NwnAnimatedTransform>();
        assert_eq!(animated_query.iter(&world).count(), 1);
    }

    #[test]
    fn spawn_nwn_model_spawns_referenced_child_models() {
        let referenced = NwnModelAsset {
            scene:      NwnScene {
                name:              "child".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![nwnrs_mdl::prelude::NwnSceneNode {
                    kind:            NodeKind::Dummy,
                    node_type:       "dummy".to_string(),
                    name:            "ChildRoot".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: nwnrs_mdl::prelude::NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       None,
                    mesh:            None,
                }],
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      vec![NwnModelNodeAsset {
                name:           "ChildRoot".to_string(),
                kind:           NodeKind::Dummy,
                parent:         None,
                transform:      Transform::default(),
                light:          None,
                references:     Vec::new(),
                helper_surface: None,
                primitives:     Vec::new(),
            }],
            root_nodes: vec![0],
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };
        let model = NwnModelAsset {
            scene:      NwnScene {
                name:              "parent".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![nwnrs_mdl::prelude::NwnSceneNode {
                    kind:            NodeKind::Reference,
                    node_type:       "reference".to_string(),
                    name:            "RefNode".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: nwnrs_mdl::prelude::NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       Some(nwnrs_mdl::prelude::NwnReference {
                        model:        Some("child".to_string()),
                        reattachable: 0,
                    }),
                    mesh:            None,
                }],
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      vec![NwnModelNodeAsset {
                name:           "RefNode".to_string(),
                kind:           NodeKind::Reference,
                parent:         None,
                transform:      Transform::default(),
                light:          None,
                references:     vec![crate::NwnModelReferenceAsset {
                    model_name: "child".to_string(),
                    model:      Box::new(referenced),
                }],
                helper_surface: None,
                primitives:     Vec::new(),
            }],
            root_nodes: vec![0],
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };

        let mut world = World::new();
        let mut queue = CommandQueue::default();
        {
            let mut commands = bevy::prelude::Commands::new(&mut queue, &world);
            super::spawn_nwn_model(&mut commands, &model);
        }
        queue.apply(&mut world);

        let mut query = world.query::<&Name>();
        let names = query
            .iter(&world)
            .map(|name| name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"RefNode"));
        assert!(names.contains(&"child"));
        assert!(names.contains(&"ChildRoot"));
    }

    #[test]
    fn spawn_nwn_model_hides_primitives_marked_not_initially_visible() {
        let model = NwnModelAsset {
            scene:      NwnScene {
                name:              "hidden-primitive".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![nwnrs_mdl::prelude::NwnSceneNode {
                    kind:            NodeKind::Trimesh,
                    node_type:       "trimesh".to_string(),
                    name:            "MeshNode".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: nwnrs_mdl::prelude::NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       None,
                    mesh:            Some(0),
                }],
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      vec![NwnModelNodeAsset {
                name:           "MeshNode".to_string(),
                kind:           NodeKind::Trimesh,
                parent:         None,
                transform:      Transform::default(),
                light:          None,
                references:     Vec::new(),
                helper_surface: None,
                primitives:     vec![crate::NwnPrimitiveAsset {
                    label: "MeshNode:0".to_string(),
                    scene_primitive_index: 0,
                    txi: None,
                    txi_uv_to_local_horizontal: None,
                    mesh: Handle::default(),
                    material: Handle::default(),
                    tilefade: None,
                    initially_visible: false,
                    shadow_enabled: true,
                }],
            }],
            root_nodes: vec![0],
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };

        let mut world = World::new();
        let mut queue = CommandQueue::default();
        {
            let mut commands = bevy::prelude::Commands::new(&mut queue, &world);
            super::spawn_nwn_model(&mut commands, &model);
        }
        queue.apply(&mut world);

        let mut query = world.query::<(&Name, &Visibility)>();
        let mesh_visibility = query
            .iter(&world)
            .find_map(|(name, visibility)| (name.as_str() == "MeshNode:0").then_some(*visibility))
            .unwrap_or_else(|| panic!("missing spawned primitive"));
        assert_eq!(mesh_visibility, Visibility::Hidden);
    }

    #[test]
    fn spawn_nwn_model_attaches_helper_surface_to_node_entity() {
        let model = NwnModelAsset {
            scene:      NwnScene {
                name:              "helper".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             vec![nwnrs_mdl::prelude::NwnSceneNode {
                    kind:            NodeKind::Aabb,
                    node_type:       "aabb".to_string(),
                    name:            "wm_demo".to_string(),
                    parent:          None,
                    part_number:     None,
                    local_transform: nwnrs_mdl::prelude::NwnTransform {
                        translation:         [0.0, 0.0, 0.0],
                        rotation_axis_angle: [0.0, 0.0, 1.0, 0.0],
                        scale:               [1.0, 1.0, 1.0],
                    },
                    center:          None,
                    color:           None,
                    radius:          None,
                    alpha:           None,
                    wirecolor:       None,
                    light:           None,
                    emitter:         None,
                    reference:       None,
                    mesh:            Some(0),
                }],
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      vec![NwnModelNodeAsset {
                name:           "wm_demo".to_string(),
                kind:           NodeKind::Aabb,
                parent:         None,
                transform:      Transform::default(),
                light:          None,
                references:     Vec::new(),
                helper_surface: Some(NwnModelHelperSurfaceAsset {
                    bitmaps:        vec!["Stone".to_string()],
                    surface_labels: vec!["stone".to_string()],
                    texture_names:  vec!["walk".to_string()],
                }),
                primitives:     Vec::new(),
            }],
            root_nodes: vec![0],
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };

        let mut world = World::new();
        let mut queue = CommandQueue::default();
        {
            let mut commands = bevy::prelude::Commands::new(&mut queue, &world);
            super::spawn_nwn_model(&mut commands, &model);
        }
        queue.apply(&mut world);

        let mut query = world.query::<(&Name, &NwnHelperSurface)>();
        let helper = query
            .iter(&world)
            .find_map(|(name, helper)| (name.as_str() == "wm_demo").then_some(helper))
            .unwrap_or_else(|| panic!("missing helper surface component"));
        assert_eq!(helper.bitmaps, vec!["Stone".to_string()]);
        assert_eq!(helper.surface_labels, vec!["stone".to_string()]);
        assert_eq!(helper.texture_names, vec!["walk".to_string()]);
    }
}
