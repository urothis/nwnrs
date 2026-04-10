//! Minimal phase-1 viewer for static NWN models loaded from the installed game.

use std::{collections::BTreeSet, f32::consts::FRAC_PI_2};

use bevy::{input::mouse::AccumulatedMouseMotion, prelude::*};
use nwnrs_bevy::{
    NwnBevyPlugin, NwnInstall, NwnInstallPlugin, load_nwn_model_from_resman, spawn_nwn_model,
};
use nwnrs_mdl::prelude::MODEL_RES_TYPE;
use tracing::{info, warn};

#[derive(Resource, Default)]
struct DemoModelCatalog {
    names: Vec<String>,
    index: usize,
}

#[derive(Resource, Default)]
struct DemoModelState {
    root:         Option<Entity>,
    needs_reload: bool,
}

#[derive(Component)]
struct FlyCam {
    move_speed:        f32,
    boost_multiplier:  f32,
    mouse_sensitivity: Vec2,
}

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, NwnBevyPlugin, NwnInstallPlugin::default()))
        .init_resource::<DemoModelCatalog>()
        .init_resource::<DemoModelState>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                initialize_model_catalog,
                cycle_models,
                reload_current_model,
                update_flycam,
                rotate_loaded_model,
            ),
        )
        .run();
}

fn setup(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.5, -6.0, 2.5).looking_at(Vec3::new(0.0, 0.0, 1.0), Vec3::Z),
        FlyCam {
            move_speed:        4.0,
            boost_multiplier:  3.0,
            mouse_sensitivity: Vec2::new(0.003, 0.002),
        },
    ));
    commands.spawn((
        PointLight {
            intensity: 1_200_000.0,
            range: 100.0,
            shadows_enabled: true,
            ..Default::default()
        },
        Transform::from_xyz(6.0, -4.0, 8.0),
    ));
}

fn initialize_model_catalog(
    install: Option<Res<'_, NwnInstall>>,
    mut catalog: ResMut<'_, DemoModelCatalog>,
    mut state: ResMut<'_, DemoModelState>,
) {
    if !catalog.names.is_empty() {
        return;
    }

    let Some(install) = install else {
        return;
    };

    let manager = match install.resman.lock() {
        Ok(manager) => manager,
        Err(error) => error.into_inner(),
    };
    let mut names = manager
        .contents()
        .into_iter()
        .filter(|resref| resref.res_type() == MODEL_RES_TYPE)
        .map(|resref| resref.res_ref().to_string())
        .collect::<Vec<_>>();
    drop(manager);

    names.sort_unstable();
    names.dedup();

    if names.is_empty() {
        warn!("no mdl resources were found in the NWN install");
        return;
    }

    catalog.index = names
        .iter()
        .position(|name| name == "a_ba_casts")
        .unwrap_or(0);
    catalog.names = names;
    state.needs_reload = true;
    if let Some(current) = catalog.names.get(catalog.index) {
        info!(
            model_count = catalog.names.len(),
            current = current.as_str(),
            "initialized mdl catalog"
        );
    }
}

fn cycle_models(
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mut catalog: ResMut<'_, DemoModelCatalog>,
    mut state: ResMut<'_, DemoModelState>,
) {
    if catalog.names.is_empty() {
        return;
    }

    let mut new_index = catalog.index;
    if keyboard.just_pressed(KeyCode::KeyZ) {
        new_index = if catalog.index == 0 {
            catalog.names.len() - 1
        } else {
            catalog.index - 1
        };
    }
    if keyboard.just_pressed(KeyCode::KeyX) {
        new_index = (catalog.index + 1) % catalog.names.len();
    }

    if new_index != catalog.index {
        catalog.index = new_index;
        state.needs_reload = true;
        if let Some(current) = catalog.names.get(catalog.index) {
            info!(current = current.as_str(), "queued mdl swap");
        }
    }
}

fn reload_current_model(
    mut commands: Commands<'_, '_>,
    install: Option<Res<'_, NwnInstall>>,
    catalog: Res<'_, DemoModelCatalog>,
    mut state: ResMut<'_, DemoModelState>,
    mut images: ResMut<'_, Assets<Image>>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
) {
    if !state.needs_reload || catalog.names.is_empty() {
        return;
    }

    let Some(install) = install else {
        return;
    };

    let Some(current) = catalog.names.get(catalog.index).cloned() else {
        state.needs_reload = false;
        return;
    };
    let mut manager = match install.resman.lock() {
        Ok(manager) => manager,
        Err(error) => error.into_inner(),
    };
    let model = load_nwn_model_from_resman(
        &mut manager,
        &current,
        &mut images,
        &mut meshes,
        &mut materials,
    );
    drop(manager);

    let model = match model {
        Ok(model) => model,
        Err(error) => {
            warn!(
                model = current.as_str(),
                "failed to load mdl from install: {error}"
            );
            state.needs_reload = false;
            return;
        }
    };

    if let Some(root) = state.root.take() {
        let mut entity = commands.entity(root);
        entity.despawn_children();
        entity.despawn();
    }

    let unresolved = model.unresolved.len();
    let root = spawn_nwn_model(&mut commands, &model);
    state.root = Some(root);
    state.needs_reload = false;

    if unresolved == 0 {
        info!(model = current.as_str(), "loaded mdl from install");
    } else {
        let unique_unresolved = model
            .unresolved
            .iter()
            .map(|unresolved_texture| {
                (
                    unresolved_texture.name.clone(),
                    format!("{:?}", unresolved_texture.reason),
                    unresolved_texture.attempted.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        warn!(
            model = current.as_str(),
            unresolved_textures = unique_unresolved.len(),
            "loaded mdl with unresolved textures"
        );
        for (texture, reason, attempted) in unique_unresolved {
            warn!(
                model = current.as_str(),
                texture = texture.as_str(),
                reason = reason.as_str(),
                attempted = ?attempted,
                "unresolved mdl texture"
            );
        }
    }
}

fn update_flycam(
    time: Res<'_, Time>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    mouse_buttons: Res<'_, ButtonInput<MouseButton>>,
    accumulated_mouse_motion: Res<'_, AccumulatedMouseMotion>,
    flycam: Single<'_, '_, (&mut Transform, &FlyCam)>,
) {
    let (mut transform, flycam) = flycam.into_inner();

    let speed = if keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight) {
        flycam.move_speed * flycam.boost_multiplier
    } else {
        flycam.move_speed
    };

    let mut movement = Vec3::ZERO;
    if keyboard.pressed(KeyCode::KeyW) {
        movement.z -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyS) {
        movement.z += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyA) {
        movement.x -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyD) {
        movement.x += 1.0;
    }
    if keyboard.pressed(KeyCode::KeyQ) {
        movement.y -= 1.0;
    }
    if keyboard.pressed(KeyCode::KeyE) {
        movement.y += 1.0;
    }

    if movement != Vec3::ZERO {
        let movement = movement.normalize();
        let forward = transform.rotation * Vec3::NEG_Z;
        let right = transform.rotation * Vec3::X;
        let up = Vec3::Z;
        transform.translation += (right * movement.x + up * movement.y + forward * movement.z)
            * speed
            * time.delta_secs();
    }

    if !mouse_buttons.pressed(MouseButton::Right) {
        return;
    }

    let delta = accumulated_mouse_motion.delta;
    if delta == Vec2::ZERO {
        return;
    }

    let delta_yaw = -delta.x * flycam.mouse_sensitivity.x;
    let delta_pitch = -delta.y * flycam.mouse_sensitivity.y;
    let (yaw, pitch, roll) = transform.rotation.to_euler(EulerRot::YXZ);
    let yaw = yaw + delta_yaw;
    let pitch = (pitch + delta_pitch).clamp(-(FRAC_PI_2 - 0.01), FRAC_PI_2 - 0.01);
    transform.rotation = Quat::from_euler(EulerRot::YXZ, yaw, pitch, roll);
}

fn rotate_loaded_model(
    time: Res<'_, Time>,
    keyboard: Res<'_, ButtonInput<KeyCode>>,
    state: Res<'_, DemoModelState>,
    mut transforms: Query<'_, '_, &mut Transform>,
) {
    let Some(model_entity) = state.root else {
        return;
    };

    let mut direction = 0.0_f32;
    if keyboard.pressed(KeyCode::ArrowLeft) {
        direction += 1.0;
    }
    if keyboard.pressed(KeyCode::ArrowRight) {
        direction -= 1.0;
    }
    if direction == 0.0 {
        return;
    }

    let Ok(mut transform) = transforms.get_mut(model_entity) else {
        return;
    };
    transform.rotate_local_z(direction * time.delta_secs());
}
