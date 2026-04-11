//! Minimal phase-1 viewer for static NWN models loaded from the installed game.

use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::{FRAC_PI_2, PI},
};

use bevy::{input::mouse::AccumulatedMouseMotion, prelude::*};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use nwnrs_bevy::{
    NwnAppearanceOverrides, NwnAppearanceSlot, NwnBevyPlugin, NwnInstall, NwnInstallPlugin,
    collect_appearance_slots, load_nwn_model_from_resman_with_overrides, spawn_nwn_model,
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

#[derive(Resource, Default)]
struct DemoAppearanceState {
    model_name: String,
    slots:      Vec<NwnAppearanceSlot>,
    overrides:  BTreeMap<String, String>,
}

#[derive(Resource, Default)]
struct DemoUiState {
    model_query: String,
}

#[derive(Component)]
struct FlyCam {
    move_speed:        f32,
    boost_multiplier:  f32,
    mouse_sensitivity: Vec2,
}

fn main() {
    App::new()
        .add_plugins((
            DefaultPlugins,
            EguiPlugin::default(),
            NwnBevyPlugin,
            NwnInstallPlugin::default(),
        ))
        .init_resource::<DemoModelCatalog>()
        .init_resource::<DemoModelState>()
        .init_resource::<DemoAppearanceState>()
        .init_resource::<DemoUiState>()
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
        .add_systems(EguiPrimaryContextPass, appearance_panel)
        .run();
}

fn setup(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(2.5, 2.5, -6.0).looking_at(Vec3::new(0.0, 1.0, 0.0), Vec3::Y),
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
        Transform::from_xyz(6.0, 8.0, -4.0),
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
    mut appearance: ResMut<'_, DemoAppearanceState>,
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
    let overrides = NwnAppearanceOverrides {
        slots:    appearance.overrides.clone(),
        plt_rows: Default::default(),
    };
    let model = load_nwn_model_from_resman_with_overrides(
        &mut manager,
        &current,
        &overrides,
        &mut images,
        &mut meshes,
        &mut materials,
    );

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

    let collected_slots = collect_appearance_slots(&model.scene, &manager);
    drop(manager);

    match collected_slots {
        Ok(slots) => {
            appearance.model_name = current.clone();
            appearance.slots = slots;
            let valid_slots = appearance.slots.clone();
            appearance.overrides.retain(|token, selected| {
                valid_slots.iter().any(|slot| {
                    slot.id.eq_ignore_ascii_case(token)
                        && slot
                            .options
                            .iter()
                            .any(|candidate| candidate.eq_ignore_ascii_case(selected))
                })
            });
        }
        Err(error) => {
            warn!(
                model = current.as_str(),
                "failed to collect appearance slots: {error}"
            );
        }
    }

    if let Some(root) = state.root.take() {
        let mut entity = commands.entity(root);
        entity.despawn_children();
        entity.despawn();
    }

    let unresolved = model.unresolved.len();
    let root = spawn_nwn_model(&mut commands, &model);
    commands
        .entity(root)
        .insert(Transform::from_rotation(Quat::from_rotation_y(PI)));
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

fn appearance_panel(
    mut contexts: EguiContexts<'_, '_>,
    mut catalog: ResMut<'_, DemoModelCatalog>,
    mut appearance: ResMut<'_, DemoAppearanceState>,
    mut ui_state: ResMut<'_, DemoUiState>,
    mut model_state: ResMut<'_, DemoModelState>,
) -> bevy::ecs::error::Result {
    let Some(current_model) = catalog.names.get(catalog.index).cloned() else {
        return Ok(());
    };
    let ctx = contexts.ctx_mut()?;

    egui::SidePanel::left("appearance_panel")
        .resizable(true)
        .default_width(280.0)
        .show(ctx, |ui| {
            ui.heading("Appearance");
            let mut selected_model = current_model.clone();
            ui.label("Model");
            ui.add(
                egui::TextEdit::singleline(&mut ui_state.model_query)
                    .hint_text("Filter models")
                    .desired_width(220.0),
            );
            let query = ui_state.model_query.trim().to_ascii_lowercase();
            let filtered_names = filtered_model_names(&catalog, query.as_str());
            egui::ComboBox::from_id_salt("model_selector")
                .selected_text(selected_model.as_str())
                .width(220.0)
                .show_ui(ui, |ui| {
                    for (index, name) in filtered_names {
                        if ui
                            .selectable_label(index == catalog.index, name.as_str())
                            .clicked()
                        {
                            selected_model = name.clone();
                        }
                    }
                });
            if let Some(new_index) = catalog
                .names
                .iter()
                .position(|name| *name == selected_model)
                && new_index != catalog.index
            {
                catalog.index = new_index;
                model_state.needs_reload = true;
            }
            ui.small("Keyboard: Z/X also cycle models");
            if !query.is_empty() {
                let total_matches = catalog
                    .names
                    .iter()
                    .filter(|name| name.to_ascii_lowercase().contains(query.as_str()))
                    .count();
                if total_matches > MODEL_SELECTOR_LIMIT {
                    ui.small(format!(
                        "Showing first {} of {} matches",
                        MODEL_SELECTOR_LIMIT, total_matches
                    ));
                }
            }

            if appearance.model_name != current_model {
                ui.separator();
                ui.label("Waiting for model load...");
                return;
            }

            if appearance.slots.is_empty() {
                ui.separator();
                ui.label("No selectable part slots detected.");
                return;
            }

            let mut clear_all = false;
            if ui.button("Reset Overrides").clicked() {
                clear_all = true;
            }
            if clear_all {
                appearance.overrides.clear();
                model_state.needs_reload = true;
            }

            ui.separator();
            let slots = appearance.slots.clone();
            for slot in &slots {
                ui.label(slot.label.as_str());
                let current_value = appearance
                    .overrides
                    .get(slot.id.as_str())
                    .cloned()
                    .unwrap_or_default();
                let selected_label = if current_value.is_empty() {
                    if slot.token.eq_ignore_ascii_case(slot.normalized.as_str()) {
                        format!("authored ({})", slot.token)
                    } else {
                        format!("authored ({})", slot.normalized)
                    }
                } else {
                    current_value.clone()
                };

                let mut chosen = current_value;
                egui::ComboBox::from_id_salt(format!("appearance_slot:{}", slot.id))
                    .selected_text(selected_label)
                    .width(220.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut chosen, String::new(), "authored");
                        for option in &slot.options {
                            ui.selectable_value(&mut chosen, option.clone(), option);
                        }
                    });

                let previous = appearance
                    .overrides
                    .get(slot.id.as_str())
                    .cloned()
                    .unwrap_or_default();
                if chosen != previous {
                    if chosen.is_empty() {
                        appearance.overrides.remove(slot.id.as_str());
                    } else {
                        appearance.overrides.insert(slot.id.clone(), chosen);
                    }
                    model_state.needs_reload = true;
                }
                ui.small(format!("family: {}", slot.family));
                if !slot.node_names.is_empty() {
                    ui.small(format!("nodes: {}", slot.node_names.join(", ")));
                }
                ui.separator();
            }
        });

    Ok(())
}

const MODEL_SELECTOR_LIMIT: usize = 200;

fn filtered_model_names(catalog: &DemoModelCatalog, query: &str) -> Vec<(usize, String)> {
    if catalog.names.is_empty() {
        return Vec::new();
    }

    if query.is_empty() {
        let start = catalog.index.saturating_sub(MODEL_SELECTOR_LIMIT / 2);
        let end = (start + MODEL_SELECTOR_LIMIT).min(catalog.names.len());
        return catalog.names[start..end]
            .iter()
            .enumerate()
            .map(|(offset, name)| (start + offset, name.clone()))
            .collect();
    }

    catalog
        .names
        .iter()
        .enumerate()
        .filter(|(_index, name)| name.to_ascii_lowercase().contains(query))
        .take(MODEL_SELECTOR_LIMIT)
        .map(|(index, name)| (index, name.clone()))
        .collect()
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
        let up = Vec3::Y;
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
