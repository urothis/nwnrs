use bevy::prelude::{Camera, Camera3d, Component, GlobalTransform, Query, Vec3, Visibility, With};

/// Runtime tilefade behavior attached to a spawned primitive entity.
#[derive(Component, Debug, Clone, PartialEq)]
pub struct NwnTileFade {
    /// Authored tilefade mode from the source material.
    pub mode:               i32,
    /// Whether the authored material started with `render 1`.
    pub authored_visible:   bool,
    /// Primitive bounds center in local Bevy space.
    pub local_center:       Vec3,
    /// Primitive bounds half extents in local Bevy space.
    pub local_half_extents: Vec3,
}

/// Runtime helper-surface metadata attached to a spawned helper node.
#[derive(Component, Debug, Clone, PartialEq, Eq)]
pub struct NwnHelperSurface {
    /// Helper bitmap tokens authored on this helper node.
    pub bitmaps:        Vec<String>,
    /// Surface labels captured from the source mesh.
    pub surface_labels: Vec<String>,
    /// Per-primitive texture-name labels captured from the source mesh.
    pub texture_names:  Vec<String>,
}

/// Chooses the default visible state for one tilefade primitive before any
/// camera-driven fade logic has run.
pub fn tilefade_default_visibility(mode: i32, authored_visible: bool) -> bool {
    match mode {
        1 => true,
        2.. => false,
        _ => authored_visible,
    }
}

/// Updates tilefade primitive visibility from the active 3D camera position.
pub fn update_nwn_tilefade_visibility(
    cameras: Query<'_, '_, (&Camera, &GlobalTransform), With<Camera3d>>,
    mut tilefade_primitives: Query<'_, '_, (&NwnTileFade, &GlobalTransform, &mut Visibility)>,
) {
    let Some((_, camera_transform)) = cameras.iter().find(|(camera, _)| camera.is_active) else {
        return;
    };
    let camera_world = camera_transform.translation();

    for (tilefade, transform, mut visibility) in &mut tilefade_primitives {
        let visible = tilefade_runtime_visibility(tilefade, transform, camera_world);
        *visibility = if visible {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        };
    }
}

fn tilefade_runtime_visibility(
    tilefade: &NwnTileFade,
    primitive_transform: &GlobalTransform,
    camera_world: Vec3,
) -> bool {
    let primitive_from_world = primitive_transform.to_matrix().inverse();
    let camera_local = primitive_from_world.transform_point3(camera_world);
    let local_min = tilefade.local_center - tilefade.local_half_extents;
    let local_max = tilefade.local_center + tilefade.local_half_extents;
    let horizontal_margin = tilefade
        .local_half_extents
        .x
        .max(tilefade.local_half_extents.z)
        .max(1.0)
        * 0.35;
    let vertical_margin = tilefade.local_half_extents.y.max(1.0) * 0.25 + 1.0;
    let fade_active = camera_local.x >= local_min.x - horizontal_margin
        && camera_local.x <= local_max.x + horizontal_margin
        && camera_local.z >= local_min.z - horizontal_margin
        && camera_local.z <= local_max.z + horizontal_margin
        && camera_local.y <= local_max.y + vertical_margin;

    match tilefade.mode {
        1 => !fade_active,
        2.. => fade_active,
        _ => tilefade.authored_visible,
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        ecs::system::RunSystemOnce,
        prelude::{Camera3d, GlobalTransform, Transform, Vec3, World},
    };

    use super::{NwnTileFade, tilefade_default_visibility, update_nwn_tilefade_visibility};

    #[test]
    fn tilefade_mode_one_starts_visible() {
        assert!(tilefade_default_visibility(1, false));
    }

    #[test]
    fn tilefade_mask_modes_start_hidden() {
        assert!(!tilefade_default_visibility(2, true));
        assert!(!tilefade_default_visibility(4, true));
    }

    #[test]
    fn tilefade_mode_one_hides_when_camera_moves_under_bounds() {
        let mut world = World::new();
        world.spawn((
            Camera3d::default(),
            bevy::prelude::Camera::default(),
            Transform::from_xyz(0.0, 0.5, 0.0),
            GlobalTransform::from(Transform::from_xyz(0.0, 0.5, 0.0)),
        ));
        let entity = world
            .spawn((
                NwnTileFade {
                    mode:               1,
                    authored_visible:   false,
                    local_center:       Vec3::ZERO,
                    local_half_extents: Vec3::splat(1.0),
                },
                GlobalTransform::IDENTITY,
                bevy::prelude::Visibility::Inherited,
            ))
            .id();

        let _ = world.run_system_once(update_nwn_tilefade_visibility);
        let visibility = world
            .entity(entity)
            .get::<bevy::prelude::Visibility>()
            .copied()
            .unwrap_or(bevy::prelude::Visibility::Inherited);
        assert_eq!(visibility, bevy::prelude::Visibility::Hidden);
    }

    #[test]
    fn tilefade_mask_mode_shows_when_camera_moves_under_bounds() {
        let mut world = World::new();
        world.spawn((
            Camera3d::default(),
            bevy::prelude::Camera::default(),
            Transform::from_xyz(0.0, 0.5, 0.0),
            GlobalTransform::from(Transform::from_xyz(0.0, 0.5, 0.0)),
        ));
        let entity = world
            .spawn((
                NwnTileFade {
                    mode:               2,
                    authored_visible:   true,
                    local_center:       Vec3::ZERO,
                    local_half_extents: Vec3::splat(1.0),
                },
                GlobalTransform::IDENTITY,
                bevy::prelude::Visibility::Hidden,
            ))
            .id();

        let _ = world.run_system_once(update_nwn_tilefade_visibility);
        let visibility = world
            .entity(entity)
            .get::<bevy::prelude::Visibility>()
            .copied()
            .unwrap_or(bevy::prelude::Visibility::Hidden);
        assert_eq!(visibility, bevy::prelude::Visibility::Inherited);
    }
}
