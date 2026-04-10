use bevy::{
    asset::Handle,
    ecs::system::EntityCommands,
    mesh::Mesh3d,
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::{
        Commands, Entity, GlobalTransform, InheritedVisibility, Name, Transform, ViewVisibility,
        Visibility,
    },
};

use crate::NwnModelAsset;

/// Spawns one loaded NWN model into the current Bevy world.
pub fn spawn_nwn_model(commands: &mut Commands<'_, '_>, model: &NwnModelAsset) -> Entity {
    let root = commands
        .spawn((model.root_name(), spatial_components(Transform::default())))
        .id();
    let mut node_entities = Vec::with_capacity(model.nodes.len());

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
            for primitive in &node.primitives {
                spawn_primitive_child(
                    &mut entity,
                    primitive.mesh.clone(),
                    primitive.material.clone(),
                    primitive.label.clone(),
                );
            }
        }
    }

    root
}

fn spawn_primitive_child(
    entity: &mut EntityCommands<'_>,
    mesh: Handle<bevy::mesh::Mesh>,
    material: Handle<StandardMaterial>,
    label: String,
) {
    entity.with_children(|children| {
        children.spawn((
            Name::new(label),
            Mesh3d(mesh),
            MeshMaterial3d(material),
            spatial_components(Transform::default()),
        ));
    });
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
