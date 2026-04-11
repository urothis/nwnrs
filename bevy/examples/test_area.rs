//! Area viewer for local NWN modules with an in-app module and area selector.

use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::FRAC_PI_2,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::{
    asset::RenderAssetUsages,
    input::mouse::AccumulatedMouseMotion,
    math::{Affine2, Mat2},
    mesh::{Indices, Mesh3d, PrimitiveTopology},
    pbr::{DistanceFog, FogFalloff, MeshMaterial3d, StandardMaterial},
    prelude::*,
};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use nwnrs_bevy::{
    NwnAppearanceOverrides, NwnAreaWind, NwnBevyPlugin, NwnInstall, NwnInstallPlugin,
    NwnModelAsset, NwnModelReferenceAsset, image_from_dds, image_from_plt, image_from_tga,
    load_nwn_model_from_resman, load_nwn_model_from_resman_with_overrides,
    material_requires_bitmap_resolution, spawn_nwn_model, spawn_nwn_model_with_animation,
};
use nwnrs_dds::prelude::read_dds_from_res;
use nwnrs_erf::prelude::{Erf, read_erf_from_file};
use nwnrs_gff::prelude::{GffCExoLocString, GffStruct, GffValue, read_gff_root};
use nwnrs_git::prelude::{GitFile, GitPoint, GitTransform, read_git_from_resman};
use nwnrs_mdl::prelude::MODEL_RES_TYPE;
use nwnrs_plt::prelude::read_plt_from_res;
use nwnrs_resman::prelude::ResContainer;
use nwnrs_resref::prelude::{ResRef, ResolvedResRef};
use nwnrs_set::prelude::{SetFile, read_set_from_resman};
use nwnrs_tga::prelude::read_tga_from_res;
use nwnrs_twoda::prelude::{TwoDa, as_2da};
use tracing::{debug, info, warn};

const DEFAULT_MODULE_PATH: &str = "assets/testing/test.mod";
const BASE_TILE_SPACING: f32 = 10.0;
const FALLBACK_TILE_SIZE: f32 = 10.0;
const TILE_THICKNESS: f32 = 0.2;
const TILE_HEIGHT_STEP: f32 = 1.5;
const MODULE_SELECTOR_LIMIT: usize = 200;
const INSTANCE_MARKER_SIZE: f32 = 0.8;
const TRIGGER_OVERLAY_HEIGHT: f32 = 2.0;
const INVENTORY_SLOT_MASK_HEAD: i32 = 1;
const INVENTORY_SLOT_MASK_CHEST: i32 = 2;
const INVENTORY_SLOT_MASK_RIGHT_HAND: i32 = 16;
const INVENTORY_SLOT_MASK_LEFT_HAND: i32 = 32;

#[derive(Clone)]
struct GrassVisual {
    blade_mesh:     Handle<Mesh>,
    blade_material: Handle<StandardMaterial>,
    blade_height:   f32,
    patch_count:    usize,
}

#[derive(Debug, Clone)]
enum CreatureVisual {
    ModelCandidates(Vec<String>),
    ComposedModel(NwnModelAsset),
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PlayerCreatureFamily {
    base_model_name: String,
    model_prefix:    String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CreaturePartAttachment {
    node_name:            String,
    model_name:           String,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EquippedPaperdoll {
    armor:      Option<EquippedArmorVisual>,
    helmet:     Option<EquippedItemVisual>,
    right_hand: Option<EquippedItemVisual>,
    left_hand:  Option<EquippedItemVisual>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct EquippedItemVisual {
    model_name:           String,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BaseItemInfo {
    model_type: Option<u32>,
    item_class: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
struct EquippedArmorVisual {
    parts:                BTreeMap<String, u32>,
    robe_model_number:    Option<u32>,
    appearance_overrides: NwnAppearanceOverrides,
}

#[derive(Component)]
struct FlyCam {
    move_speed:        f32,
    boost_multiplier:  f32,
    mouse_sensitivity: Vec2,
}

#[derive(Component)]
struct AreaLightCamera;

#[derive(Component)]
struct AreaDirectionalLight;

#[derive(Resource, Default)]
struct AreaViewerCatalog {
    modules:           Vec<ModuleChoice>,
    module_index:      usize,
    module_query:      String,
    roots:             Vec<PathBuf>,
    extra_search_path: Option<PathBuf>,
    needs_refresh:     bool,
}

#[derive(Resource, Default)]
struct AreaViewerState {
    areas:                   Vec<AreaChoice>,
    area_index:              usize,
    scene_root:              Option<Entity>,
    active_module_path:      Option<PathBuf>,
    active_area_resref:      Option<String>,
    active_module_archive:   Option<Arc<Erf>>,
    active_module_container: Option<Arc<dyn ResContainer>>,
    needs_area_list_refresh: bool,
    needs_scene_reload:      bool,
    status_message:          String,
}

#[derive(Resource, Default)]
struct AreaRenderCache {
    models:               BTreeMap<(PathBuf, String), NwnModelAsset>,
    overridden_models:    BTreeMap<(PathBuf, String, NwnAppearanceOverrides), NwnModelAsset>,
    resolved_model_names: BTreeMap<(PathBuf, String), Option<String>>,
    tilesets:             BTreeMap<(PathBuf, String), SetFile>,
    git_instances:        BTreeMap<(PathBuf, String), GitFile>,
    blueprints:           BTreeMap<(PathBuf, String, String), Vec<String>>,
    twodas:               BTreeMap<String, TwoDa>,
}

#[derive(Debug, Clone)]
struct ModuleChoice {
    path:  PathBuf,
    label: String,
}

#[derive(Debug, Clone)]
struct AreaChoice {
    resref: String,
    label:  String,
}

#[derive(Debug, Clone)]
struct TestArea {
    name:       String,
    resref:     String,
    tileset:    String,
    width:      u32,
    height:     u32,
    tiles:      Vec<TestAreaTile>,
    wind_power: Option<i32>,
    lighting:   AreaLighting,
}

#[derive(Debug, Clone, Copy)]
struct TestAreaTile {
    id:          u32,
    orientation: u32,
    height:      i32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TileOrientationMapping {
    turn_offset:    u8,
    turn_direction: i8,
    rows_go_south:  bool,
}

#[derive(Debug, Clone)]
struct WaterFlowSample {
    row:            u32,
    col:            u32,
    tile_id:        u32,
    orientation:    u32,
    model:          String,
    texture:        String,
    node_name:      String,
    node_kind:      String,
    rotate_texture: i32,
    flow_world:     Vec2,
}

#[derive(Debug, Clone, Copy)]
struct AreaTileDoorMarker {
    door_type:      i32,
    world_position: Vec3,
}

#[derive(Debug, Clone, Copy)]
struct AreaLayoutContext {
    tile_spacing: f32,
    width:        u32,
    height:       u32,
}

#[derive(Debug, Clone, Default)]
struct AreaLighting {
    lighting_scheme: Option<u8>,
    shadow_opacity:  Option<u8>,
    day_night_cycle: bool,
    sun:             AreaLightSet,
    moon:            AreaLightSet,
}

#[derive(Debug, Clone, Default)]
struct AreaLightSet {
    ambient_color: Option<Color>,
    diffuse_color: Option<Color>,
    fog_color:     Option<Color>,
    fog_amount:    Option<u8>,
    shadows:       Option<bool>,
}

// SET tiles define their canonical shape in top/right/bottom/left order.
// ARE `Tile_Orientation` uses the opposite quarter-turn sign from the first
// convention we tried, and area rows advance opposite the assumed southward
// direction. In Bevy world space that means decrementing Z for later rows and
// rotating tile wrappers with the inverse quarter-turn order.
const AREA_TILE_ORIENTATION_MAPPING: TileOrientationMapping = TileOrientationMapping {
    turn_offset:    0,
    turn_direction: -1,
    rows_go_south:  false,
};

fn main() {
    let extra_search_path = std::env::args_os().nth(1).map(PathBuf::from);

    App::new()
        .add_plugins((
            DefaultPlugins,
            EguiPlugin::default(),
            NwnBevyPlugin,
            NwnInstallPlugin::default(),
        ))
        .insert_resource(AreaViewerCatalog {
            extra_search_path,
            needs_refresh: true,
            ..Default::default()
        })
        .init_resource::<AreaViewerState>()
        .init_resource::<AreaRenderCache>()
        .add_systems(Startup, setup)
        .add_systems(
            Update,
            (
                refresh_module_catalog,
                refresh_area_catalog,
                reload_selected_area_scene,
            )
                .chain(),
        )
        .add_systems(Update, update_flycam)
        .add_systems(EguiPrimaryContextPass, area_selector_panel)
        .run();
}

fn setup(mut commands: Commands<'_, '_>) {
    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(10.0, 24.0, -36.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y),
        AreaLightCamera,
        FlyCam {
            move_speed:        24.0,
            boost_multiplier:  3.0,
            mouse_sensitivity: Vec2::new(0.003, 0.002),
        },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 35_000.0,
            shadows_enabled: true,
            ..Default::default()
        },
        AreaDirectionalLight,
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.05, 0.65, 0.0)),
    ));
}

fn refresh_module_catalog(
    install: Option<Res<'_, NwnInstall>>,
    mut catalog: ResMut<'_, AreaViewerCatalog>,
    mut state: ResMut<'_, AreaViewerState>,
) {
    if !catalog.needs_refresh {
        return;
    }

    let Some(install) = install else {
        return;
    };

    let previous_selection = catalog
        .modules
        .get(catalog.module_index)
        .map(|module| module.path.clone());
    let previous_area = state
        .areas
        .get(state.area_index)
        .map(|area| area.resref.clone());
    let (modules, roots) = discover_modules(&install, catalog.extra_search_path.as_ref());

    catalog.roots = roots;
    catalog.needs_refresh = false;

    if modules.is_empty() {
        catalog.modules.clear();
        catalog.module_index = 0;
        state.areas.clear();
        state.area_index = 0;
        state.needs_area_list_refresh = false;
        state.needs_scene_reload = false;
        state.status_message =
            "No .mod or .nwm archives were found in the scanned module roots.".to_string();
        warn!("no local modules were discovered for the area selector");
        return;
    }

    let selected_index = previous_selection
        .as_ref()
        .and_then(|path| modules.iter().position(|module| &module.path == path))
        .or_else(|| {
            default_module_path()
                .and_then(|path| modules.iter().position(|module| module.path == path))
        })
        .unwrap_or(0);

    catalog.module_index = selected_index;
    catalog.modules = modules;
    state.area_index = previous_area
        .and_then(|area_resref| {
            state
                .areas
                .iter()
                .position(|area| area.resref == area_resref)
        })
        .unwrap_or(0);
    state.needs_area_list_refresh = true;

    if let Some(selected) = catalog.modules.get(catalog.module_index) {
        info!(
            module_count = catalog.modules.len(),
            selected = selected.path.display().to_string(),
            "initialized area module selector"
        );
        state.status_message = format!("Selected module {}", selected.label);
    }
}

fn refresh_area_catalog(
    catalog: Res<'_, AreaViewerCatalog>,
    mut state: ResMut<'_, AreaViewerState>,
) {
    if !state.needs_area_list_refresh {
        return;
    }

    let Some(module) = catalog.modules.get(catalog.module_index) else {
        state.needs_area_list_refresh = false;
        return;
    };

    match inspect_module_areas(&module.path) {
        Ok(areas) => {
            let previous_resref = state
                .areas
                .get(state.area_index)
                .map(|area| area.resref.clone());
            state.area_index = previous_resref
                .as_ref()
                .and_then(|resref| areas.iter().position(|area| &area.resref == resref))
                .unwrap_or(0);
            state.areas = areas;
            state.needs_scene_reload = true;
            state.status_message = format!(
                "Module {} exposes {} area(s)",
                module.label,
                state.areas.len()
            );
            info!(
                module = module.path.display().to_string(),
                area_count = state.areas.len(),
                "inspected module archive"
            );
        }
        Err(error) => {
            state.areas.clear();
            state.area_index = 0;
            state.needs_scene_reload = false;
            state.status_message = format!("Failed to inspect {}: {error}", module.label);
            warn!(
                module = module.path.display().to_string(),
                "failed to inspect local module: {error}"
            );
        }
    }

    state.needs_area_list_refresh = false;
}

fn reload_selected_area_scene(
    mut commands: Commands<'_, '_>,
    install: Option<Res<'_, NwnInstall>>,
    catalog: Res<'_, AreaViewerCatalog>,
    mut state: ResMut<'_, AreaViewerState>,
    mut render_cache: ResMut<'_, AreaRenderCache>,
    mut clear_color: ResMut<'_, ClearColor>,
    mut global_ambient: ResMut<'_, GlobalAmbientLight>,
    mut area_wind: ResMut<'_, NwnAreaWind>,
    mut images: ResMut<'_, Assets<Image>>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
    camera: Single<
        '_,
        '_,
        (Entity, &mut Transform),
        (With<AreaLightCamera>, Without<AreaDirectionalLight>),
    >,
    directional_light: Single<
        '_,
        '_,
        (Entity, &mut DirectionalLight, &mut Transform),
        (With<AreaDirectionalLight>, Without<AreaLightCamera>),
    >,
) {
    if !state.needs_scene_reload {
        return;
    }

    let Some(install) = install else {
        return;
    };

    let Some(module) = catalog.modules.get(catalog.module_index).cloned() else {
        state.needs_scene_reload = false;
        return;
    };
    let Some(area_choice) = state.areas.get(state.area_index).cloned() else {
        state.needs_scene_reload = false;
        return;
    };
    let (camera_entity, mut camera_transform) = camera.into_inner();
    let (directional_light_entity, mut directional_light, mut directional_light_transform) =
        directional_light.into_inner();

    let reusing_active_module = state
        .active_module_path
        .as_ref()
        .is_some_and(|path| path == &module.path);
    let archive = if reusing_active_module {
        match state.active_module_archive.clone() {
            Some(archive) => archive,
            None => match read_erf_from_file(&module.path) {
                Ok(archive) => Arc::new(archive),
                Err(error) => {
                    state.status_message = format!("Failed to reopen {}: {error}", module.label);
                    state.needs_scene_reload = false;
                    warn!(
                        module = module.path.display().to_string(),
                        "failed to reopen selected module: {error}"
                    );
                    return;
                }
            },
        }
    } else {
        match read_erf_from_file(&module.path) {
            Ok(archive) => Arc::new(archive),
            Err(error) => {
                state.status_message = format!("Failed to open {}: {error}", module.label);
                state.needs_scene_reload = false;
                warn!(
                    module = module.path.display().to_string(),
                    "failed to open selected module: {error}"
                );
                return;
            }
        }
    };

    let area = match load_area_from_archive(archive.as_ref(), Some(area_choice.resref.as_str())) {
        Ok(area) => area,
        Err(error) => {
            state.status_message = format!(
                "Failed to load area {} from {}: {error}",
                area_choice.resref, module.label
            );
            state.needs_scene_reload = false;
            warn!(
                module = module.path.display().to_string(),
                area = area_choice.resref.as_str(),
                "failed to load selected area: {error}"
            );
            return;
        }
    };

    if !reusing_active_module {
        let module_container: Arc<dyn ResContainer> = archive.clone();
        {
            let mut resman = match install.resman.lock() {
                Ok(resman) => resman,
                Err(error) => error.into_inner(),
            };
            if let Some(previous) = state.active_module_container.take() {
                resman.remove(&previous);
            }
            resman.add(Arc::clone(&module_container));
            if let Some(cache) = resman.cache() {
                cache.clear();
            }
        }
        state.active_module_path = Some(module.path.clone());
        state.active_module_archive = Some(Arc::clone(&archive));
        state.active_module_container = Some(module_container);
    }

    let reframe_camera = state.scene_root.is_none()
        || state
            .active_module_path
            .as_ref()
            .is_none_or(|path| path != &module.path)
        || state
            .active_area_resref
            .as_ref()
            .is_none_or(|resref| resref != &area_choice.resref);

    let scene_root = match spawn_area_scene(
        &mut commands,
        &install,
        &module.path,
        &area,
        reframe_camera,
        &mut render_cache,
        &mut images,
        &mut meshes,
        &mut materials,
        &mut camera_transform,
    ) {
        Ok(root) => root,
        Err(error) => {
            state.status_message = format!(
                "Failed to render {} from {}: {error}",
                area_choice.label, module.label
            );
            state.needs_scene_reload = false;
            warn!(
                module = module.path.display().to_string(),
                area = area.resref.as_str(),
                "failed to render selected area: {error}"
            );
            return;
        }
    };

    if let Some(previous_root) = state.scene_root.take() {
        let mut entity = commands.entity(previous_root);
        entity.despawn_children();
        entity.despawn();
    }

    info!(
        module = module.path.display().to_string(),
        area = area.resref.as_str(),
        name = area.name.as_str(),
        tileset = area.tileset.as_str(),
        width = area.width,
        height = area.height,
        tile_count = area.tiles.len(),
        wind_power = area.wind_power,
        "loaded selected area"
    );

    *area_wind = NwnAreaWind {
        direction: Vec2::new(1.0, -1.0).normalize(),
        magnitude: area.wind_power.unwrap_or(0).max(0) as f32,
    };

    apply_area_lighting(
        &mut commands,
        camera_entity,
        directional_light_entity,
        &area,
        &mut clear_color,
        &mut global_ambient,
        &mut directional_light,
        &mut directional_light_transform,
    );

    state.scene_root = Some(scene_root);
    state.active_area_resref = Some(area.resref.clone());
    state.needs_scene_reload = false;
    state.status_message = format!("Loaded {} from {}", area.name, module.label);
}

fn area_selector_panel(
    mut contexts: EguiContexts<'_, '_>,
    mut catalog: ResMut<'_, AreaViewerCatalog>,
    mut state: ResMut<'_, AreaViewerState>,
) -> bevy::ecs::error::Result {
    let ctx = contexts.ctx_mut()?;

    egui::SidePanel::left("area_selector_panel")
        .resizable(true)
        .default_width(320.0)
        .show(ctx, |ui| {
            ui.heading("Area Viewer");
            ui.label("Browse local modules and pick which area archive to render.");
            if ui.button("Refresh Modules").clicked() {
                catalog.needs_refresh = true;
            }
            if !state.status_message.is_empty() {
                ui.separator();
                ui.label(state.status_message.as_str());
            }

            ui.separator();
            ui.label("Module");
            ui.add(
                egui::TextEdit::singleline(&mut catalog.module_query)
                    .hint_text("Filter modules")
                    .desired_width(250.0),
            );

            if catalog.modules.is_empty() {
                ui.label("Waiting for module scan...");
            } else {
                let mut selected_module = catalog
                    .modules
                    .get(catalog.module_index)
                    .map(|module| module.label.clone())
                    .unwrap_or_default();
                let query = catalog.module_query.trim().to_ascii_lowercase();
                let filtered_modules = filtered_module_entries(&catalog, query.as_str());
                egui::ComboBox::from_id_salt("module_selector")
                    .selected_text(selected_module.as_str())
                    .width(250.0)
                    .show_ui(ui, |ui| {
                        for (index, label) in filtered_modules {
                            if ui
                                .selectable_label(index == catalog.module_index, label.as_str())
                                .clicked()
                            {
                                selected_module = label;
                            }
                        }
                    });
                if let Some(new_index) = catalog
                    .modules
                    .iter()
                    .position(|module| module.label == selected_module)
                    && new_index != catalog.module_index
                {
                    catalog.module_index = new_index;
                    state.area_index = 0;
                    state.needs_area_list_refresh = true;
                    state.status_message =
                        format!("Inspecting {}", catalog.modules[catalog.module_index].label);
                }
                if let Some(module) = catalog.modules.get(catalog.module_index) {
                    ui.small(module.path.display().to_string());
                }
                if !query.is_empty() {
                    let total_matches = catalog
                        .modules
                        .iter()
                        .filter(|module| {
                            module.label.to_ascii_lowercase().contains(query.as_str())
                                || module
                                    .path
                                    .display()
                                    .to_string()
                                    .to_ascii_lowercase()
                                    .contains(query.as_str())
                        })
                        .count();
                    if total_matches > MODULE_SELECTOR_LIMIT {
                        ui.small(format!(
                            "Showing first {} of {} matches",
                            MODULE_SELECTOR_LIMIT, total_matches
                        ));
                    }
                }
            }

            ui.separator();
            ui.label("Area");
            if state.areas.is_empty() {
                ui.label("No ARE resources available in the selected module.");
            } else {
                let current_area = state
                    .areas
                    .get(state.area_index)
                    .map(|area| area.label.clone())
                    .unwrap_or_default();
                let mut selected_area = current_area.clone();
                egui::ComboBox::from_id_salt("area_selector")
                    .selected_text(current_area)
                    .width(250.0)
                    .show_ui(ui, |ui| {
                        for (index, area) in state.areas.iter().enumerate() {
                            if ui
                                .selectable_label(index == state.area_index, area.label.as_str())
                                .clicked()
                            {
                                selected_area = area.label.clone();
                            }
                        }
                    });
                if let Some(new_index) = state
                    .areas
                    .iter()
                    .position(|area| area.label == selected_area)
                    && new_index != state.area_index
                {
                    state.area_index = new_index;
                    state.needs_scene_reload = true;
                }
                if ui.button("Reload Area").clicked() {
                    state.needs_scene_reload = true;
                }
            }
            ui.separator();
            ui.small("Camera: hold right mouse to look, WASD to move, Q/E up and down.");
            if !catalog.roots.is_empty() {
                ui.collapsing("Scanned module roots", |ui| {
                    for root in &catalog.roots {
                        ui.small(root.display().to_string());
                    }
                });
            }
            if let Some(extra_path) = &catalog.extra_search_path {
                ui.small(format!("Extra search path: {}", extra_path.display()));
            } else {
                ui.small(
                    "Tip: pass a directory or .mod/.nwm path after `--example test_area --` to \
                     add another search root.",
                );
            }
        });

    Ok(())
}

fn filtered_module_entries(catalog: &AreaViewerCatalog, query: &str) -> Vec<(usize, String)> {
    if catalog.modules.is_empty() {
        return Vec::new();
    }

    if query.is_empty() {
        let start = catalog
            .module_index
            .saturating_sub(MODULE_SELECTOR_LIMIT / 2);
        let end = (start + MODULE_SELECTOR_LIMIT).min(catalog.modules.len());
        return catalog.modules[start..end]
            .iter()
            .enumerate()
            .map(|(offset, module)| (start + offset, module.label.clone()))
            .collect();
    }

    catalog
        .modules
        .iter()
        .enumerate()
        .filter(|(_index, module)| {
            module.label.to_ascii_lowercase().contains(query)
                || module
                    .path
                    .display()
                    .to_string()
                    .to_ascii_lowercase()
                    .contains(query)
        })
        .take(MODULE_SELECTOR_LIMIT)
        .map(|(index, module)| (index, module.label.clone()))
        .collect()
}

fn discover_modules(
    install: &NwnInstall,
    extra_search_path: Option<&PathBuf>,
) -> (Vec<ModuleChoice>, Vec<PathBuf>) {
    let mut roots = Vec::new();
    let mut direct_files = Vec::new();

    if let Some(default_module) = default_module_path() {
        direct_files.push(default_module.clone());
        if let Some(parent) = default_module.parent() {
            roots.push(parent.to_path_buf());
        }
    }
    if install.user_root.is_dir() {
        roots.push(install.user_root.clone());
        roots.push(install.user_root.join("modules"));
    }
    if install.root.is_dir() {
        roots.push(install.root.clone());
        roots.push(install.root.join("modules"));
        roots.push(install.root.join("data").join("mod"));
    }
    if let Some(extra_path) = extra_search_path {
        let normalized = normalize_existing_path(extra_path);
        if normalized.is_file() {
            direct_files.push(normalized.clone());
            if let Some(parent) = normalized.parent() {
                roots.push(parent.to_path_buf());
            }
        } else {
            roots.push(normalized);
        }
    }

    let mut unique_roots = BTreeSet::new();
    roots.retain(|root| unique_roots.insert(root.clone()));

    let mut files = BTreeSet::new();
    for file in direct_files {
        if is_module_archive_path(&file) {
            files.insert(file);
        }
    }
    for root in &roots {
        let Ok(entries) = std::fs::read_dir(root) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = normalize_existing_path(&entry.path());
            if is_module_archive_path(&path) {
                files.insert(path);
            }
        }
    }

    let mut modules = files
        .into_iter()
        .map(|path| ModuleChoice {
            label: module_label(&path),
            path,
        })
        .collect::<Vec<_>>();
    modules.sort_by(|left, right| {
        left.label
            .to_ascii_lowercase()
            .cmp(&right.label.to_ascii_lowercase())
            .then_with(|| left.path.cmp(&right.path))
    });
    (modules, roots)
}

fn default_module_path() -> Option<PathBuf> {
    let path = PathBuf::from(DEFAULT_MODULE_PATH);
    path.exists().then(|| normalize_existing_path(&path))
}

fn normalize_existing_path(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

fn is_module_archive_path(path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .and_then(|extension| extension.to_str())
            .is_some_and(|extension| {
                extension.eq_ignore_ascii_case("mod") || extension.eq_ignore_ascii_case("nwm")
            })
}

fn module_label(path: &Path) -> String {
    let filename = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("<module>");
    let parent = path
        .parent()
        .and_then(|parent| parent.file_name())
        .and_then(|name| name.to_str())
        .unwrap_or("");
    if parent.is_empty() {
        filename.to_string()
    } else {
        format!("{filename} [{parent}]")
    }
}

fn inspect_module_areas(path: &Path) -> Result<Vec<AreaChoice>, String> {
    let archive = read_erf_from_file(path).map_err(|error| format!("read module: {error}"))?;
    let mut areas = archive
        .entries()
        .iter()
        .filter_map(|(resref, res)| {
            let resolved = resref.resolve()?;
            (resolved.res_ext() == "are").then_some((resolved.res_ref().to_string(), res))
        })
        .map(|(resref, area_res)| {
            let bytes = area_res
                .read_all(true)
                .map_err(|error| format!("read area resource {resref}: {error}"))?;
            let area = parse_area_bytes(&bytes)?;
            let label = if area.name.eq_ignore_ascii_case(area.resref.as_str()) {
                area.resref.clone()
            } else {
                format!("{} ({})", area.name, area.resref)
            };
            Ok(AreaChoice {
                resref: area.resref,
                label,
            })
        })
        .collect::<Result<Vec<_>, String>>()?;

    if areas.is_empty() {
        return Err("archive contains no .are resources".to_string());
    }

    areas.sort_by(|left, right| {
        left.label
            .to_ascii_lowercase()
            .cmp(&right.label.to_ascii_lowercase())
            .then_with(|| left.resref.cmp(&right.resref))
    });
    Ok(areas)
}

fn load_area_from_archive(
    archive: &Erf,
    requested_resref: Option<&str>,
) -> Result<TestArea, String> {
    let area_entry = archive
        .entries()
        .iter()
        .find(|(resref, _res)| {
            let Some(resolved) = resref.resolve() else {
                return false;
            };
            if resolved.res_ext() != "are" {
                return false;
            }
            requested_resref
                .map(|requested| resolved.res_ref().eq_ignore_ascii_case(requested))
                .unwrap_or(true)
        })
        .map(|(_resref, res)| res.clone())
        .ok_or_else(|| match requested_resref {
            Some(resref) => format!("no .are entry named {resref} found in module"),
            None => "no .are entry found in module".to_string(),
        })?;
    let bytes = area_entry
        .read_all(true)
        .map_err(|error| format!("read area resource: {error}"))?;
    parse_area_bytes(&bytes)
}

fn parse_area_bytes(bytes: &[u8]) -> Result<TestArea, String> {
    let root =
        read_gff_root(&mut Cursor::new(bytes)).map_err(|error| format!("read ARE: {error}"))?;
    let area_root = &root.root;

    let width = gff_u32(area_root.get_field("Width").map(|field| field.value()))
        .ok_or_else(|| "ARE missing Width".to_string())?;
    let height = gff_u32(area_root.get_field("Height").map(|field| field.value()))
        .ok_or_else(|| "ARE missing Height".to_string())?;
    let tileset = gff_string(area_root.get_field("Tileset").map(|field| field.value()))
        .unwrap_or_else(|| "unknown".to_string());
    let resref = gff_string(area_root.get_field("ResRef").map(|field| field.value()))
        .unwrap_or_else(|| "area".to_string());
    let name = gff_name(area_root.get_field("Name").map(|field| field.value()))
        .unwrap_or_else(|| resref.clone());
    let lighting = AreaLighting {
        lighting_scheme: gff_u8(
            area_root
                .get_field("LightingScheme")
                .map(|field| field.value()),
        ),
        shadow_opacity:  gff_u8(
            area_root
                .get_field("ShadowOpacity")
                .map(|field| field.value()),
        ),
        day_night_cycle: gff_u8(
            area_root
                .get_field("DayNightCycle")
                .map(|field| field.value()),
        )
        .is_some_and(|value| value != 0),
        sun:             AreaLightSet {
            ambient_color: gff_color(
                area_root
                    .get_field("SunAmbientColor")
                    .map(|field| field.value()),
            ),
            diffuse_color: gff_color(
                area_root
                    .get_field("SunDiffuseColor")
                    .map(|field| field.value()),
            ),
            fog_color:     gff_color(
                area_root
                    .get_field("SunFogColor")
                    .map(|field| field.value()),
            ),
            fog_amount:    gff_u8(
                area_root
                    .get_field("SunFogAmount")
                    .map(|field| field.value()),
            ),
            shadows:       gff_u8(area_root.get_field("SunShadows").map(|field| field.value()))
                .map(|value| value != 0),
        },
        moon:            AreaLightSet {
            ambient_color: gff_color(
                area_root
                    .get_field("MoonAmbientColor")
                    .map(|field| field.value()),
            ),
            diffuse_color: gff_color(
                area_root
                    .get_field("MoonDiffuseColor")
                    .map(|field| field.value()),
            ),
            fog_color:     gff_color(
                area_root
                    .get_field("MoonFogColor")
                    .map(|field| field.value()),
            ),
            fog_amount:    gff_u8(
                area_root
                    .get_field("MoonFogAmount")
                    .map(|field| field.value()),
            ),
            shadows:       gff_u8(
                area_root
                    .get_field("MoonShadows")
                    .map(|field| field.value()),
            )
            .map(|value| value != 0),
        },
    };
    let wind_power = gff_i32(area_root.get_field("WindPower").map(|field| field.value()));

    let tiles = root
        .root
        .get_field("Tile_List")
        .map(|field| field.value())
        .and_then(gff_list)
        .ok_or_else(|| "ARE missing Tile_List".to_string())?
        .iter()
        .map(parse_tile)
        .collect::<Result<Vec<_>, _>>()?;

    let expected_tile_count = area_tile_count(width, height)?;
    if tiles.len() != expected_tile_count {
        warn!(
            width,
            height,
            expected_tile_count,
            actual_tile_count = tiles.len(),
            "ARE tile list size does not match Width x Height"
        );
    }

    Ok(TestArea {
        name,
        resref,
        tileset,
        width,
        height,
        tiles,
        wind_power,
        lighting,
    })
}

fn spawn_area_scene(
    commands: &mut Commands<'_, '_>,
    install: &NwnInstall,
    module_path: &Path,
    area: &TestArea,
    reframe_camera: bool,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    camera_transform: &mut Transform,
) -> Result<Entity, String> {
    let tileset_cache_key = (module_path.to_path_buf(), area.tileset.clone());
    let tileset = if let Some(tileset) = render_cache.tilesets.get(&tileset_cache_key).cloned() {
        tileset
    } else {
        let parsed_tileset = {
            let mut resman = match install.resman.lock() {
                Ok(resman) => resman,
                Err(error) => error.into_inner(),
            };
            read_set_from_resman(&mut resman, &area.tileset, true)
                .map_err(|error| format!("read tileset {}: {error}", area.tileset))?
        };
        render_cache
            .tilesets
            .insert(tileset_cache_key.clone(), parsed_tileset.clone());
        parsed_tileset
    };
    let tile_spacing = BASE_TILE_SPACING;
    let layout = AreaLayoutContext {
        tile_spacing,
        width: area.width,
        height: area.height,
    };
    let orientation_mapping = AREA_TILE_ORIENTATION_MAPPING;
    log_area_layout(area, &tileset, orientation_mapping);

    if reframe_camera {
        let area_extent_x = area.width as f32 * tile_spacing;
        let area_extent_z = area.height as f32 * tile_spacing;
        let max_extent = area_extent_x.max(area_extent_z);
        let camera_distance = max_extent.max(40.0) * 0.9;
        let camera_height =
            (area.height.max(area.width) as f32 * TILE_HEIGHT_STEP) + max_extent * 0.55;

        *camera_transform =
            Transform::from_xyz(area_extent_x * 0.25, camera_height, -camera_distance)
                .looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
    }

    let x_origin = -((area.width as f32 - 1.0) * tile_spacing * 0.5);
    let z_origin = if orientation_mapping.rows_go_south {
        -((area.height as f32 - 1.0) * tile_spacing * 0.5)
    } else {
        (area.height as f32 - 1.0) * tile_spacing * 0.5
    };
    let row_spacing = if orientation_mapping.rows_go_south {
        tile_spacing
    } else {
        -tile_spacing
    };

    let mut resman = match install.resman.lock() {
        Ok(resman) => resman,
        Err(error) => error.into_inner(),
    };
    let git = load_cached_git_file(&mut resman, module_path, area.resref.as_str(), render_cache)?;
    let grass_visual = build_tileset_grass_visual(&mut resman, &tileset, images, meshes, materials);

    let area_root = commands
        .spawn((
            Name::new(format!("area_{}", area.resref)),
            area_spatial_components(Transform::default()),
        ))
        .id();
    let mut water_flow_samples = Vec::new();
    for row in 0..area.height {
        for col in 0..area.width {
            let tile_index = (row * area.width + col) as usize;
            let Some(tile) = area.tiles.get(tile_index).copied() else {
                warn!(
                    row,
                    col,
                    tile_index,
                    width = area.width,
                    height = area.height,
                    actual_tile_count = area.tiles.len(),
                    "area layout is missing a tile for this cell"
                );
                break;
            };

            let translation = Vec3::new(
                x_origin + col as f32 * tile_spacing,
                tile.height as f32 * TILE_HEIGHT_STEP,
                z_origin + row as f32 * row_spacing,
            );
            let clockwise_turns = tile_orientation_turns(tile.orientation, orientation_mapping);
            let orientation_angle = clockwise_turns as f32 * FRAC_PI_2;
            let tile_name = format!(
                "tile_{row}_{col}_id{}_rot{}_h{}",
                tile.id, tile.orientation, tile.height
            );
            let tile_entity = commands
                .spawn((
                    Name::new(tile_name),
                    area_spatial_components(
                        Transform::from_translation(translation)
                            .with_rotation(Quat::from_rotation_y(-orientation_angle)),
                    ),
                ))
                .id();
            commands.entity(area_root).add_child(tile_entity);

            let tile_definition = tileset.tiles.get(&tile.id);
            let maybe_model_name = tile_definition.and_then(|tile_def| tile_def.model.as_deref());
            let Some(model_name) = maybe_model_name else {
                spawn_tile_fallback(
                    commands,
                    meshes,
                    materials,
                    tile_entity,
                    tile,
                    FALLBACK_TILE_SIZE,
                    format!(
                        "tile {} is missing or has no model in {}.set",
                        tile.id, area.tileset
                    ),
                );
                continue;
            };
            debug!(
                row,
                col,
                tile_id = tile.id,
                orientation = tile.orientation,
                height = tile.height,
                model = model_name,
                path_node = tile_definition
                    .and_then(|tile_def| tile_def.path_node.as_deref())
                    .unwrap_or(""),
                door_count = tile_definition
                    .and_then(|tile_def| tile_def.doors)
                    .unwrap_or(0),
                "placing area tile"
            );

            let resolved_model_name =
                resolve_area_tile_model_name(&mut resman, module_path, model_name, render_cache)
                    .unwrap_or_else(|| model_name.to_string());
            let model = match load_cached_model_asset(
                &mut resman,
                module_path,
                &resolved_model_name,
                render_cache,
                images,
                meshes,
                materials,
            ) {
                Ok(model) => model,
                Err(error) => {
                    spawn_tile_fallback(
                        commands,
                        meshes,
                        materials,
                        tile_entity,
                        tile,
                        FALLBACK_TILE_SIZE,
                        format!("failed to load {resolved_model_name}.mdl: {error}"),
                    );
                    continue;
                }
            };
            collect_water_flow_samples(
                &mut water_flow_samples,
                row,
                col,
                tile,
                &resolved_model_name,
                -orientation_angle,
                &model,
            );
            let tile_animation =
                tile_definition.and_then(|tile_def| select_tile_animation_name(tile_def, &model));
            let model_root =
                spawn_nwn_model_with_animation(commands, &model, tile_animation.as_deref());
            commands.entity(model_root).insert(Transform::default());
            commands.entity(tile_entity).add_child(model_root);
            if let Some(grass_visual) = grass_visual.as_ref() {
                spawn_tile_grass(
                    commands,
                    tile_entity,
                    row,
                    col,
                    &tileset,
                    tile_definition,
                    tile_spacing,
                    grass_visual,
                );
            }
        }
    }

    log_water_flow_samples(area, &water_flow_samples);

    if let Some(git) = git.as_ref() {
        log_git_instances(area, git);
        spawn_git_instances(
            commands,
            &mut resman,
            module_path,
            area_root,
            area,
            layout,
            &tileset,
            git,
            render_cache,
            images,
            meshes,
            materials,
        );
    } else {
        info!(
            area = area.resref.as_str(),
            "area has no matching GIT resource"
        );
    }

    Ok(area_root)
}

fn load_cached_git_file(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    area_resref: &str,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<GitFile>, String> {
    let cache_key = (module_path.to_path_buf(), area_resref.to_string());
    if let Some(git) = render_cache.git_instances.get(&cache_key).cloned() {
        return Ok(Some(git));
    }

    let resolved = ResolvedResRef::from_filename(&format!("{area_resref}.git"))
        .map_err(|error| format!("git resref for {area_resref}: {error}"))?;
    let Some(_res) = resman.get_resolved(&resolved) else {
        return Ok(None);
    };

    let git = read_git_from_resman(resman, area_resref, true)
        .map_err(|error| format!("read git {area_resref}: {error}"))?;
    render_cache.git_instances.insert(cache_key, git.clone());
    Ok(Some(git))
}

#[allow(clippy::too_many_arguments)]
fn spawn_git_instances(
    commands: &mut Commands<'_, '_>,
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    area_root: Entity,
    area: &TestArea,
    layout: AreaLayoutContext,
    tileset: &SetFile,
    git: &GitFile,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) {
    let tile_door_markers =
        collect_area_tile_door_markers(area, tileset, layout, AREA_TILE_ORIENTATION_MAPPING);

    for placeable in &git.placeables {
        let display_name = placeable
            .localized_name
            .as_ref()
            .and_then(loc_string_name)
            .unwrap_or_else(|| {
                placeable
                    .tag
                    .clone()
                    .unwrap_or_else(|| "placeable".to_string())
            });
        let model_candidates = instance_model_candidates(
            resman,
            module_path,
            "utp",
            placeable.template_resref.as_deref(),
            placeable.appearance,
            None,
            render_cache,
        )
        .unwrap_or_else(|error| {
            warn!(
                template = placeable.template_resref.as_deref().unwrap_or(""),
                appearance = placeable.appearance,
                "{error}"
            );
            Vec::new()
        });
        spawn_git_model_instance(
            commands,
            resman,
            module_path,
            area_root,
            &display_name,
            "placeable",
            layout,
            &placeable.transform,
            &model_candidates,
            render_cache,
            images,
            meshes,
            materials,
            Color::srgb(0.86, 0.56, 0.22),
        );
    }

    for door in &git.doors {
        let display_name = door
            .localized_name
            .as_ref()
            .and_then(loc_string_name)
            .unwrap_or_else(|| door.tag.clone().unwrap_or_else(|| "door".to_string()));
        let tile_door_match =
            find_matching_tile_door_marker(&door.transform, layout, &tile_door_markers);
        let tile_door_model_candidate =
            resolve_tile_door_model_candidate(resman, tile_door_match, render_cache)
                .unwrap_or_else(|error| {
                    warn!(
                        template = door.template_resref.as_deref().unwrap_or(""),
                        appearance = door.appearance,
                        "{error}"
                    );
                    None
                });
        let mut model_candidates = instance_model_candidates(
            resman,
            module_path,
            "utd",
            door.template_resref.as_deref(),
            door.appearance,
            Some(area.tileset.as_str()),
            render_cache,
        )
        .unwrap_or_else(|error| {
            warn!(
                template = door.template_resref.as_deref().unwrap_or(""),
                appearance = door.appearance,
                "{error}"
            );
            Vec::new()
        });
        if let Some(candidate) = tile_door_model_candidate {
            model_candidates.push(candidate);
        }
        dedup_case_insensitive(&mut model_candidates);
        spawn_git_model_instance(
            commands,
            resman,
            module_path,
            area_root,
            &display_name,
            "door",
            layout,
            &door.transform,
            &model_candidates,
            render_cache,
            images,
            meshes,
            materials,
            Color::srgb(0.42, 0.68, 0.84),
        );
    }

    for trigger in &git.triggers {
        let display_name = trigger
            .localized_name
            .as_ref()
            .and_then(loc_string_name)
            .unwrap_or_else(|| trigger.tag.clone().unwrap_or_else(|| "trigger".to_string()));
        spawn_trigger_geometry(
            commands,
            meshes,
            materials,
            area_root,
            format!("trigger_{display_name}"),
            layout,
            &trigger.transform,
            &trigger.geometry,
            Color::srgba(0.92, 0.16, 0.72, 0.18),
            Color::srgb(0.96, 0.38, 0.84),
        );
    }

    for creature in &git.creatures {
        let display_name = creature
            .localized_name
            .as_ref()
            .and_then(loc_string_name)
            .unwrap_or_else(|| {
                creature
                    .tag
                    .clone()
                    .unwrap_or_else(|| "creature".to_string())
            });
        let creature_visual = resolve_creature_visual(
            resman,
            module_path,
            creature.template_resref.as_deref(),
            render_cache,
            images,
            meshes,
            materials,
        )
        .unwrap_or_else(|error| {
            warn!(
                template = creature.template_resref.as_deref().unwrap_or(""),
                "{error}"
            );
            CreatureVisual::ModelCandidates(Vec::new())
        });
        match &creature_visual {
            CreatureVisual::ComposedModel(model) => spawn_git_composed_model_instance(
                commands,
                area_root,
                &display_name,
                "creature",
                layout,
                &creature.transform,
                model,
            ),
            CreatureVisual::ModelCandidates(model_candidates) => {
                spawn_git_model_instance(
                    commands,
                    resman,
                    module_path,
                    area_root,
                    &display_name,
                    "creature",
                    layout,
                    &creature.transform,
                    model_candidates,
                    render_cache,
                    images,
                    meshes,
                    materials,
                    Color::srgb(0.82, 0.24, 0.24),
                );
                if model_candidates.is_empty() {
                    debug!(
                        template = creature.template_resref.as_deref().unwrap_or(""),
                        display_name,
                        "creature fell back to marker because no supported model candidates were \
                         resolved"
                    );
                }
            }
        }
    }

    for waypoint in &git.waypoints {
        let display_name = waypoint
            .localized_name
            .as_ref()
            .and_then(loc_string_name)
            .unwrap_or_else(|| {
                waypoint
                    .tag
                    .clone()
                    .unwrap_or_else(|| "waypoint".to_string())
            });
        spawn_waypoint_marker(
            commands,
            meshes,
            materials,
            area_root,
            format!("waypoint_{display_name}"),
            layout,
            &waypoint.transform,
            Color::srgb(0.35, 0.86, 0.52),
            Color::srgb(0.76, 0.97, 0.84),
        );
    }
}

fn collect_area_tile_door_markers(
    area: &TestArea,
    tileset: &SetFile,
    layout: AreaLayoutContext,
    orientation_mapping: TileOrientationMapping,
) -> Vec<AreaTileDoorMarker> {
    let x_origin = -((layout.width as f32 - 1.0) * layout.tile_spacing * 0.5);
    let z_origin = if orientation_mapping.rows_go_south {
        -((layout.height as f32 - 1.0) * layout.tile_spacing * 0.5)
    } else {
        (layout.height as f32 - 1.0) * layout.tile_spacing * 0.5
    };
    let row_spacing = if orientation_mapping.rows_go_south {
        layout.tile_spacing
    } else {
        -layout.tile_spacing
    };

    let mut markers = Vec::new();
    for row in 0..area.height {
        for col in 0..area.width {
            let tile_index = (row * area.width + col) as usize;
            let Some(tile) = area.tiles.get(tile_index).copied() else {
                continue;
            };
            let translation = Vec3::new(
                x_origin + col as f32 * layout.tile_spacing,
                tile.height as f32 * TILE_HEIGHT_STEP,
                z_origin + row as f32 * row_spacing,
            );
            let clockwise_turns = tile_orientation_turns(tile.orientation, orientation_mapping);
            let orientation_angle = clockwise_turns as f32 * FRAC_PI_2;
            let tile_rotation = Quat::from_rotation_y(-orientation_angle);

            for ((_tile_id, _door_id), tile_door) in tileset
                .tile_doors
                .iter()
                .filter(|((tile_id, _), _)| *tile_id == tile.id)
            {
                let Some(door_type) = tile_door.door_type else {
                    continue;
                };
                let local_position = Vec3::new(
                    tile_door.x.unwrap_or(5.0) - (layout.tile_spacing * 0.5),
                    tile_door.z.unwrap_or(0.0),
                    -(tile_door.y.unwrap_or(5.0) - (layout.tile_spacing * 0.5)),
                );
                let world_position = translation + tile_rotation * local_position;
                markers.push(AreaTileDoorMarker {
                    door_type,
                    world_position,
                });
            }
        }
    }

    markers
}

fn resolve_tile_door_model_candidate(
    resman: &mut nwnrs_resman::prelude::ResMan,
    marker: Option<AreaTileDoorMarker>,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    let Some(marker) = marker else {
        return Ok(None);
    };
    if marker.door_type <= 0 {
        return Ok(None);
    }

    let candidate = appearance_model_from_named_twoda(
        resman,
        "doortypes",
        marker.door_type as usize,
        render_cache,
    )?;
    Ok(candidate)
}

fn find_matching_tile_door_marker(
    transform: &GitTransform,
    layout: AreaLayoutContext,
    markers: &[AreaTileDoorMarker],
) -> Option<AreaTileDoorMarker> {
    let world_position = world_translation_from_git(transform, layout);
    markers
        .iter()
        .filter_map(|marker| {
            let distance_squared = marker.world_position.distance_squared(world_position);
            (distance_squared <= 2.25).then_some((distance_squared, *marker))
        })
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_distance_squared, marker)| marker)
}

fn instance_model_candidates(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    blueprint_ext: &str,
    template_resref: Option<&str>,
    instance_appearance: Option<i32>,
    preferred_doortype_tileset: Option<&str>,
    render_cache: &mut AreaRenderCache,
) -> Result<Vec<String>, String> {
    let blueprint_candidates = if let Some(template_resref) = template_resref {
        blueprint_model_candidates(
            resman,
            module_path,
            template_resref,
            blueprint_ext,
            preferred_doortype_tileset,
            render_cache,
        )?
    } else {
        Vec::new()
    };

    let mut candidates = match blueprint_ext {
        "utd" => {
            let instance_doortype_candidate = instance_appearance
                .filter(|appearance| *appearance > 0)
                .and_then(|appearance| usize::try_from(appearance).ok())
                .map(|appearance_index| {
                    appearance_model_from_named_twoda(
                        resman,
                        "doortypes",
                        appearance_index,
                        render_cache,
                    )
                })
                .transpose()?
                .flatten();
            combine_door_model_candidates(instance_doortype_candidate, blueprint_candidates)
        }
        _ => {
            let instance_appearance_candidate = instance_appearance
                .and_then(|appearance| usize::try_from(appearance).ok())
                .map(|appearance_index| {
                    appearance_model_from_twoda(
                        resman,
                        blueprint_ext,
                        appearance_index,
                        render_cache,
                    )
                })
                .transpose()?
                .flatten();
            combine_default_model_candidates(instance_appearance_candidate, blueprint_candidates)
        }
    };

    dedup_case_insensitive(&mut candidates);
    Ok(candidates)
}

#[allow(clippy::too_many_arguments)]
fn resolve_creature_visual(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    template_resref: Option<&str>,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<CreatureVisual, String> {
    let Some(template_resref) = template_resref else {
        return Ok(CreatureVisual::ModelCandidates(Vec::new()));
    };

    let resolved = ResolvedResRef::from_filename(&format!("{template_resref}.utc"))
        .map_err(|error| format!("blueprint resref {template_resref}.utc: {error}"))?;
    let Some(res) = resman.get_resolved(&resolved) else {
        return Ok(CreatureVisual::ModelCandidates(vec![
            template_resref.to_string(),
        ]));
    };
    let bytes = res
        .read_all(true)
        .map_err(|error| format!("read {template_resref}.utc: {error}"))?;
    let root = read_gff_root(&mut Cursor::new(bytes))
        .map_err(|error| format!("parse {template_resref}.utc: {error}"))?;

    if let Some(model) = player_creature_model_from_blueprint(
        resman,
        module_path,
        &root,
        render_cache,
        images,
        meshes,
        materials,
    )? {
        return Ok(CreatureVisual::ComposedModel(model));
    }

    Ok(CreatureVisual::ModelCandidates(
        creature_model_candidates_from_blueprint(resman, template_resref, &root, render_cache)?,
    ))
}

fn creature_model_candidates_from_blueprint(
    resman: &mut nwnrs_resman::prelude::ResMan,
    template_resref: &str,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
) -> Result<Vec<String>, String> {
    let mut candidates = Vec::new();
    if let Some(model_name) = creature_model_from_blueprint(resman, root, render_cache)? {
        candidates.push(model_name);
    }
    if let Some(model_name) = gff_string_any(&root.root, &["ModelName", "Model", "TemplateResRef"])
        && !model_name.eq_ignore_ascii_case(template_resref)
    {
        candidates.push(model_name);
    }
    candidates.push(template_resref.to_string());
    dedup_case_insensitive(&mut candidates);
    Ok(candidates)
}

#[allow(clippy::too_many_arguments)]
fn player_creature_model_from_blueprint(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<Option<NwnModelAsset>, String> {
    let Some(appearance) = creature_appearance_row_from_blueprint(resman, root, render_cache)?
    else {
        return Ok(None);
    };
    let table = load_cached_twoda(resman, "appearance", render_cache)?;
    let Some(row) = twoda_row_index_for_appearance(&table, appearance) else {
        return Ok(None);
    };
    let Some(race_token) = table
        .cell(row, "RACE")
        .map(|value| value.trim().to_string())
    else {
        return Ok(None);
    };
    let model_type = table
        .cell(row, "MODELTYPE")
        .map(|value| value.trim().to_ascii_uppercase())
        .unwrap_or_default();
    if model_type != "P" && !is_player_appearance_token(race_token.as_str()) {
        return Ok(None);
    }

    let Some(family) =
        player_creature_family_from_blueprint(resman, root, race_token.as_str(), render_cache)?
    else {
        return Ok(None);
    };
    let creature_overrides = creature_paperdoll_appearance_overrides(root);
    let equipped = resolve_equipped_paperdoll(resman, root, render_cache)?;

    let mut model = load_cached_model_asset_with_overrides(
        resman,
        module_path,
        family.base_model_name.as_str(),
        &creature_overrides,
        render_cache,
        images,
        meshes,
        materials,
    )?;
    let attachments = build_player_creature_part_attachments(
        resman,
        root,
        &family,
        &creature_overrides,
        &equipped,
        render_cache,
    )?;
    strip_replaced_player_creature_primitives(&mut model, &attachments);
    for attachment in attachments {
        match load_cached_model_asset_with_overrides(
            resman,
            module_path,
            attachment.model_name.as_str(),
            &attachment.appearance_overrides,
            render_cache,
            images,
            meshes,
            materials,
        ) {
            Ok(part_model) => {
                if !attach_model_reference(
                    &mut model,
                    attachment.node_name.as_str(),
                    NwnModelReferenceAsset {
                        model_name: attachment.model_name,
                        model:      Box::new(part_model),
                    },
                ) {
                    warn!(
                        base_model = family.base_model_name.as_str(),
                        node = attachment.node_name.as_str(),
                        "failed to attach creature part because the target node was not found"
                    );
                }
            }
            Err(error) => {
                warn!(
                    base_model = family.base_model_name.as_str(),
                    part_model = attachment.model_name.as_str(),
                    "{error}"
                );
            }
        }
    }

    Ok(Some(model))
}

fn creature_appearance_row_from_blueprint(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<usize>, String> {
    if let Some(appearance) = gff_u32_any(
        &root.root,
        &["Appearance_Type", "AppearanceType", "Appearance"],
    ) {
        return Ok(Some(appearance as usize));
    }

    let Some(race) = gff_u32_any(&root.root, &["Race", "Subrace", "RacialType"]) else {
        return Ok(None);
    };
    racialtype_appearance_row(resman, race as usize, render_cache)
}

fn player_creature_family_from_blueprint(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    race_token: &str,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<PlayerCreatureFamily>, String> {
    let gender = gff_u32_any(&root.root, &["Gender"]).unwrap_or(0);
    let gender_token = match gender {
        1 => 'f',
        _ => 'm',
    };
    let race_token = race_token.trim().to_ascii_lowercase();
    let Some(race_letter) = race_token
        .chars()
        .next()
        .filter(|ch| ch.is_ascii_alphabetic())
    else {
        return Ok(None);
    };
    let phenotype = gff_u32_any(&root.root, &["Phenotype"]).unwrap_or(0);
    let phenotype_digit =
        player_creature_phenotype_digit(resman, phenotype as usize, render_cache)?;
    let model_prefix = player_creature_model_prefix(gender_token, race_letter, phenotype_digit);
    Ok(Some(PlayerCreatureFamily {
        base_model_name: model_prefix.clone(),
        model_prefix,
    }))
}

fn player_creature_phenotype_digit(
    resman: &mut nwnrs_resman::prelude::ResMan,
    phenotype: usize,
    render_cache: &mut AreaRenderCache,
) -> Result<u8, String> {
    let table = load_cached_twoda(resman, "phenotype", render_cache)?;
    Ok(default_player_phenotype_digit(&table, phenotype))
}

fn player_creature_model_prefix(
    gender_token: char,
    race_letter: char,
    phenotype_digit: u8,
) -> String {
    format!(
        "p{gender_token}{}{phenotype_digit}",
        race_letter.to_ascii_lowercase()
    )
}

fn default_player_phenotype_digit(table: &TwoDa, phenotype: usize) -> u8 {
    let Some(row) = twoda_row_index_for_appearance(table, phenotype) else {
        return 0;
    };
    table
        .cell(row, "DefaultPhenoType")
        .and_then(|value| value.trim().parse::<u8>().ok())
        .unwrap_or(0)
}

fn build_player_creature_part_attachments(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    family: &PlayerCreatureFamily,
    creature_overrides: &NwnAppearanceOverrides,
    equipped: &EquippedPaperdoll,
    render_cache: &mut AreaRenderCache,
) -> Result<Vec<CreaturePartAttachment>, String> {
    let mut attachments = Vec::new();
    let armor_overrides = equipped
        .armor
        .as_ref()
        .map(|armor| merged_appearance_overrides(creature_overrides, &armor.appearance_overrides))
        .unwrap_or_else(|| creature_overrides.clone());
    let capart = load_cached_twoda(resman, "capart", render_cache)?;
    for row in 0..capart.len() {
        let Some(model_stem_value) = capart.cell(row, "MDLNAME") else {
            continue;
        };
        let Some(node_name_value) = capart.cell(row, "NODENAME") else {
            continue;
        };
        let model_stem = model_stem_value.trim();
        let node_name = node_name_value.trim();
        if model_stem.eq_ignore_ascii_case("robe") {
            if let Some(robe_model_number) = equipped
                .armor
                .as_ref()
                .and_then(|armor| armor.robe_model_number)
            {
                attachments.push(CreaturePartAttachment {
                    node_name:            "rootdummy".to_string(),
                    model_name:           format!(
                        "{}_robe{robe_model_number:03}",
                        family.model_prefix
                    ),
                    appearance_overrides: armor_overrides.clone(),
                });
            }
            continue;
        }
        if model_stem.is_empty()
            || model_stem == "****"
            || node_name.is_empty()
            || node_name == "****"
            || node_name.eq_ignore_ascii_case("root")
        {
            continue;
        }
        let model_number = equipped
            .armor
            .as_ref()
            .and_then(|armor| armor.parts.get(&model_stem.to_ascii_uppercase()).copied())
            .or_else(|| {
                creature_body_part_model_number(
                    &root.root,
                    creature_body_part_field_aliases(model_stem),
                )
            })
            .unwrap_or(1);
        if model_number == 0 {
            continue;
        }
        attachments.push(CreaturePartAttachment {
            node_name:            node_name.to_string(),
            model_name:           format!(
                "{}_{}{:03}",
                family.model_prefix,
                model_stem.to_ascii_lowercase(),
                model_number
            ),
            appearance_overrides: armor_overrides.clone(),
        });
    }

    let head_model_number =
        creature_body_part_model_number(&root.root, &["BodyPart_Head"]).unwrap_or(1);
    if head_model_number > 0 {
        attachments.push(CreaturePartAttachment {
            node_name:            "head_g".to_string(),
            model_name:           format!("{}_head{:03}", family.model_prefix, head_model_number),
            appearance_overrides: creature_overrides.clone(),
        });
    }

    if let Some(tail_model) = appearance_model_name_from_named_twoda(
        resman,
        "tailmodel",
        gff_u32_any(&root.root, &["Tail"]).unwrap_or(0) as usize,
        "MODEL",
        render_cache,
    )? {
        attachments.push(CreaturePartAttachment {
            node_name:            "tail".to_string(),
            model_name:           tail_model,
            appearance_overrides: creature_overrides.clone(),
        });
    }
    if let Some(wing_model) = appearance_model_name_from_named_twoda(
        resman,
        "wingmodel",
        gff_u32_any(&root.root, &["Wings"]).unwrap_or(0) as usize,
        "MODEL",
        render_cache,
    )? {
        attachments.push(CreaturePartAttachment {
            node_name:            "wings".to_string(),
            model_name:           wing_model,
            appearance_overrides: creature_overrides.clone(),
        });
    }
    if let Some(helmet) = equipped.helmet.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "head".to_string(),
            model_name:           helmet.model_name.clone(),
            appearance_overrides: merged_appearance_overrides(
                creature_overrides,
                &helmet.appearance_overrides,
            ),
        });
    }
    if let Some(right_hand) = equipped.right_hand.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "rhand".to_string(),
            model_name:           right_hand.model_name.clone(),
            appearance_overrides: right_hand.appearance_overrides.clone(),
        });
    }
    if let Some(left_hand) = equipped.left_hand.as_ref() {
        attachments.push(CreaturePartAttachment {
            node_name:            "lhand".to_string(),
            model_name:           left_hand.model_name.clone(),
            appearance_overrides: left_hand.appearance_overrides.clone(),
        });
    }

    Ok(attachments)
}

fn resolve_equipped_paperdoll(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
) -> Result<EquippedPaperdoll, String> {
    let mut equipped = EquippedPaperdoll::default();
    let Some(entries) = root
        .root
        .get_field("Equip_ItemList")
        .map(|field| field.value())
        .and_then(gff_list)
    else {
        return Ok(equipped);
    };

    for entry in entries {
        let Some(resref) = gff_string(entry.get_field("EquippedRes").map(|field| field.value()))
        else {
            continue;
        };
        let Some(item_root) = load_gff_root_from_resman(resman, resref.as_str(), "uti")? else {
            continue;
        };
        match entry.id {
            INVENTORY_SLOT_MASK_CHEST => {
                if let Some(armor) =
                    equipped_armor_visual_from_item(resman, &item_root, render_cache)?
                {
                    equipped.armor = Some(armor);
                }
            }
            INVENTORY_SLOT_MASK_HEAD => {
                equipped.helmet =
                    equipped_held_item_visual_from_item(resman, &item_root, render_cache)?;
            }
            INVENTORY_SLOT_MASK_RIGHT_HAND => {
                equipped.right_hand =
                    equipped_held_item_visual_from_item(resman, &item_root, render_cache)?;
            }
            INVENTORY_SLOT_MASK_LEFT_HAND => {
                equipped.left_hand =
                    equipped_held_item_visual_from_item(resman, &item_root, render_cache)?;
            }
            _ => {}
        }
    }

    Ok(equipped)
}

fn equipped_armor_visual_from_item(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<EquippedArmorVisual>, String> {
    let Some(base_item) = gff_u32(root.root.get_field("BaseItem").map(|field| field.value()))
    else {
        return Ok(None);
    };
    if base_item_info(resman, base_item as usize, render_cache)?.and_then(|info| info.model_type)
        != Some(3)
    {
        return Ok(None);
    }

    let mut armor = EquippedArmorVisual::default();
    for model_stem in [
        "FOOTR", "FOOTL", "SHINR", "SHINL", "LEGR", "LEGL", "PELVIS", "CHEST", "BELT", "NECK",
        "FORER", "FOREL", "BICEPR", "BICEPL", "SHOR", "SHOL", "HANDR", "HANDL",
    ] {
        if let Some(model_number) =
            creature_body_part_model_number(&root.root, armor_part_field_aliases(model_stem))
                .filter(|value| *value > 0)
        {
            armor.parts.insert(model_stem.to_string(), model_number);
        }
    }
    if let Some(robe_model_number) =
        creature_body_part_model_number(&root.root, &["ArmorPart_Robe"]).filter(|value| *value > 0)
    {
        armor.robe_model_number = Some(robe_model_number);
    }
    armor.appearance_overrides = item_plt_appearance_overrides(&root.root);

    Ok(Some(armor))
}

fn equipped_held_item_visual_from_item(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<EquippedItemVisual>, String> {
    let Some(base_item) = gff_u32(root.root.get_field("BaseItem").map(|field| field.value()))
    else {
        return Ok(None);
    };
    let Some(base_item_info) = base_item_info(resman, base_item as usize, render_cache)? else {
        return Ok(None);
    };
    let Some(model_name) = held_item_model_name(resman, root, &base_item_info)? else {
        return Ok(None);
    };
    Ok(Some(EquippedItemVisual {
        model_name,
        appearance_overrides: item_plt_appearance_overrides(&root.root),
    }))
}

fn base_item_info(
    resman: &mut nwnrs_resman::prelude::ResMan,
    base_item: usize,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<BaseItemInfo>, String> {
    let table = load_cached_twoda(resman, "baseitems", render_cache)?;
    let Some(row) = twoda_row_index_for_appearance(&table, base_item) else {
        return Ok(None);
    };
    Ok(Some(BaseItemInfo {
        model_type: table
            .cell(row, "ModelType")
            .and_then(|value| value.trim().parse::<u32>().ok()),
        item_class: table
            .cell(row, "ItemClass")
            .map(|value| value.trim().to_ascii_lowercase())
            .filter(|value| !value.is_empty() && value != "****"),
    }))
}

fn creature_paperdoll_appearance_overrides(
    root: &nwnrs_gff::prelude::GffRoot,
) -> NwnAppearanceOverrides {
    let mut overrides = NwnAppearanceOverrides::default();
    insert_plt_row_override(
        &mut overrides,
        0,
        &root.root,
        &["Color_Skin", "SkinColor", "ColorSkin"],
    );
    insert_plt_row_override(
        &mut overrides,
        1,
        &root.root,
        &["Color_Hair", "HairColor", "ColorHair"],
    );
    insert_plt_row_override(
        &mut overrides,
        8,
        &root.root,
        &["Color_Tattoo1", "Tattoo1Color", "ColorTattoo1"],
    );
    insert_plt_row_override(
        &mut overrides,
        9,
        &root.root,
        &["Color_Tattoo2", "Tattoo2Color", "ColorTattoo2"],
    );
    overrides
}

fn item_plt_appearance_overrides(value: &GffStruct) -> NwnAppearanceOverrides {
    let mut overrides = NwnAppearanceOverrides::default();
    insert_plt_row_override(&mut overrides, 2, value, &["Metal1Color"]);
    insert_plt_row_override(&mut overrides, 3, value, &["Metal2Color"]);
    insert_plt_row_override(&mut overrides, 4, value, &["Cloth1Color"]);
    insert_plt_row_override(&mut overrides, 5, value, &["Cloth2Color"]);
    insert_plt_row_override(&mut overrides, 6, value, &["Leather1Color"]);
    insert_plt_row_override(&mut overrides, 7, value, &["Leather2Color"]);
    overrides
}

fn insert_plt_row_override(
    overrides: &mut NwnAppearanceOverrides,
    layer_id: u8,
    value: &GffStruct,
    fields: &[&str],
) {
    if let Some(row) = fields
        .iter()
        .find_map(|field| gff_u8(value.get_field(field).map(|entry| entry.value())))
    {
        overrides.plt_rows.insert(layer_id, row);
    }
}

fn merged_appearance_overrides(
    base: &NwnAppearanceOverrides,
    extra: &NwnAppearanceOverrides,
) -> NwnAppearanceOverrides {
    let mut merged = base.clone();
    for (slot, value) in &extra.slots {
        merged.slots.insert(slot.clone(), value.clone());
    }
    for (layer_id, row) in &extra.plt_rows {
        merged.plt_rows.insert(*layer_id, *row);
    }
    merged
}

fn held_item_model_name(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    base_item_info: &BaseItemInfo,
) -> Result<Option<String>, String> {
    let Some(model_part) = gff_u32(root.root.get_field("ModelPart1").map(|field| field.value()))
        .filter(|value| *value > 0)
    else {
        return Ok(None);
    };
    let Some(item_class) = base_item_info.item_class.as_deref() else {
        return Ok(None);
    };
    let model_candidates = match base_item_info.model_type {
        Some(1) => vec![format!("helm_{model_part:03}")],
        Some(2) => vec![
            format!("{item_class}_m_{model_part:03}"),
            format!("{item_class}_b_{model_part:03}"),
            format!("{item_class}_t_{model_part:03}"),
            format!("{item_class}_{model_part:03}"),
        ],
        Some(0) => vec![
            format!("{item_class}_{model_part:03}"),
            format!("{item_class}_m_{model_part:03}"),
            format!("{item_class}_b_{model_part:03}"),
            format!("{item_class}_t_{model_part:03}"),
        ],
        _ => Vec::new(),
    };

    Ok(first_existing_model_candidate(resman, &model_candidates))
}

fn first_existing_model_candidate(
    resman: &mut nwnrs_resman::prelude::ResMan,
    candidates: &[String],
) -> Option<String> {
    candidates.iter().find_map(|candidate| {
        ResRef::new(candidate.clone(), MODEL_RES_TYPE)
            .ok()
            .and_then(|resref| resman.get(&resref).map(|_res| candidate.clone()))
    })
}

fn load_gff_root_from_resman(
    resman: &mut nwnrs_resman::prelude::ResMan,
    stem: &str,
    extension: &str,
) -> Result<Option<nwnrs_gff::prelude::GffRoot>, String> {
    let resolved = ResolvedResRef::from_filename(&format!("{stem}.{extension}"))
        .map_err(|error| format!("invalid resref {stem}.{extension}: {error}"))?;
    let Some(res) = resman.get_resolved(&resolved) else {
        return Ok(None);
    };
    let bytes = res
        .read_all(true)
        .map_err(|error| format!("read {stem}.{extension}: {error}"))?;
    let root = read_gff_root(&mut Cursor::new(bytes))
        .map_err(|error| format!("parse {stem}.{extension}: {error}"))?;
    Ok(Some(root))
}

fn appearance_model_name_from_named_twoda(
    resman: &mut nwnrs_resman::prelude::ResMan,
    table_name: &str,
    appearance: usize,
    column: &str,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    if appearance == 0 {
        return Ok(None);
    }
    let table = load_cached_twoda(resman, table_name, render_cache)?;
    let Some(row) = twoda_row_index_for_appearance(&table, appearance) else {
        return Ok(None);
    };
    Ok(table
        .cell(row, column)
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty() && *value != "****")
        .map(str::to_string))
}

fn creature_body_part_model_number(value: &GffStruct, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .find_map(|field| gff_u32(value.get_field(field).map(|entry| entry.value())))
}

fn creature_body_part_field_aliases(model_stem: &str) -> &'static [&'static str] {
    match model_stem.to_ascii_uppercase().as_str() {
        "FOOTR" => &["BodyPart_RFoot", "BodyPart_Foot_R"],
        "FOOTL" => &["BodyPart_LFoot", "BodyPart_Foot_L"],
        "SHINR" => &["BodyPart_RShin", "BodyPart_Shin_R"],
        "SHINL" => &["BodyPart_LShin", "BodyPart_Shin_L"],
        "LEGR" => &["BodyPart_RThigh", "BodyPart_Thigh_R"],
        "LEGL" => &["BodyPart_LThigh", "BodyPart_Thigh_L"],
        "PELVIS" => &["BodyPart_Pelvis"],
        "CHEST" => &["BodyPart_Torso", "BodyPart_Chest"],
        "BELT" => &["BodyPart_Belt"],
        "NECK" => &["BodyPart_Neck"],
        "FORER" => &[
            "BodyPart_RForeArm",
            "BodyPart_RForearm",
            "BodyPart_ForeArm_R",
            "BodyPart_Forearm_R",
        ],
        "FOREL" => &[
            "BodyPart_LForeArm",
            "BodyPart_LForearm",
            "BodyPart_ForeArm_L",
            "BodyPart_Forearm_L",
        ],
        "BICEPR" => &["BodyPart_RBicep", "BodyPart_Bicep_R"],
        "BICEPL" => &["BodyPart_LBicep", "BodyPart_Bicep_L"],
        "SHOR" => &["BodyPart_RShoulder", "BodyPart_Shoulder_R"],
        "SHOL" => &["BodyPart_LShoulder", "BodyPart_Shoulder_L"],
        "HANDR" => &["BodyPart_RHand", "BodyPart_Hand_R"],
        "HANDL" => &["BodyPart_LHand", "BodyPart_Hand_L"],
        _ => &[],
    }
}

fn armor_part_field_aliases(model_stem: &str) -> &'static [&'static str] {
    match model_stem.to_ascii_uppercase().as_str() {
        "FOOTR" => &["ArmorPart_RFoot"],
        "FOOTL" => &["ArmorPart_LFoot"],
        "SHINR" => &["ArmorPart_RShin"],
        "SHINL" => &["ArmorPart_LShin"],
        "LEGR" => &["ArmorPart_RThigh"],
        "LEGL" => &["ArmorPart_LThigh"],
        "PELVIS" => &["ArmorPart_Pelvis"],
        "CHEST" => &["ArmorPart_Torso"],
        "BELT" => &["ArmorPart_Belt"],
        "NECK" => &["ArmorPart_Neck"],
        "FORER" => &["ArmorPart_RFArm", "ArmorPart_RForearm"],
        "FOREL" => &["ArmorPart_LFArm", "ArmorPart_LForearm"],
        "BICEPR" => &["ArmorPart_RBicep"],
        "BICEPL" => &["ArmorPart_LBicep"],
        "SHOR" => &["ArmorPart_RShoul", "ArmorPart_RShoulder"],
        "SHOL" => &["ArmorPart_LShoul", "ArmorPart_LShoulder"],
        "HANDR" => &["ArmorPart_RHand"],
        "HANDL" => &["ArmorPart_LHand"],
        _ => &[],
    }
}

fn strip_replaced_player_creature_primitives(
    model: &mut NwnModelAsset,
    attachments: &[CreaturePartAttachment],
) {
    for attachment in attachments {
        let Some(node) = model.nodes.iter_mut().find(|node| {
            node.name
                .eq_ignore_ascii_case(attachment.node_name.as_str())
        }) else {
            continue;
        };
        if !attachment.node_name.eq_ignore_ascii_case("head") {
            node.primitives.clear();
        }
    }
}

fn attach_model_reference(
    model: &mut NwnModelAsset,
    node_name: &str,
    reference: NwnModelReferenceAsset,
) -> bool {
    let Some(node) = model
        .nodes
        .iter_mut()
        .find(|node| node.name.eq_ignore_ascii_case(node_name))
    else {
        return false;
    };
    node.references.push(reference);
    true
}

fn combine_default_model_candidates(
    instance_appearance_candidate: Option<String>,
    blueprint_candidates: Vec<String>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(candidate) = instance_appearance_candidate {
        candidates.push(candidate);
    }
    candidates.extend(blueprint_candidates);
    candidates
}

fn combine_door_model_candidates(
    instance_doortype_candidate: Option<String>,
    blueprint_candidates: Vec<String>,
) -> Vec<String> {
    let mut candidates = Vec::new();
    if let Some(candidate) = instance_doortype_candidate {
        candidates.push(candidate);
    }
    candidates.extend(blueprint_candidates);
    candidates
}

fn blueprint_model_candidates(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    template_resref: &str,
    blueprint_ext: &str,
    preferred_doortype_tileset: Option<&str>,
    render_cache: &mut AreaRenderCache,
) -> Result<Vec<String>, String> {
    let cache_key = (
        module_path.to_path_buf(),
        template_resref.to_string(),
        blueprint_ext.to_string(),
    );
    if let Some(cached) = render_cache.blueprints.get(&cache_key).cloned() {
        return Ok(cached);
    }

    let mut candidates = Vec::new();
    let resolved = ResolvedResRef::from_filename(&format!("{template_resref}.{blueprint_ext}"))
        .map_err(|error| format!("blueprint resref {template_resref}.{blueprint_ext}: {error}"))?;
    if let Some(res) = resman.get_resolved(&resolved) {
        let bytes = res
            .read_all(true)
            .map_err(|error| format!("read {template_resref}.{blueprint_ext}: {error}"))?;
        let root = read_gff_root(&mut Cursor::new(bytes))
            .map_err(|error| format!("parse {template_resref}.{blueprint_ext}: {error}"))?;
        if blueprint_ext == "utd" {
            if let Some(appearance) =
                gff_i32(root.root.get_field("Appearance").map(|field| field.value()))
                    .filter(|appearance| *appearance > 0)
                && let Some(model_name) = appearance_model_from_named_twoda(
                    resman,
                    "doortypes",
                    appearance as usize,
                    render_cache,
                )?
            {
                candidates.push(model_name);
            }
            if let Some(model_name) = door_model_from_template_resref(
                resman,
                template_resref,
                preferred_doortype_tileset,
                render_cache,
            )? {
                candidates.push(model_name);
            }
            if let Some(generic_type) = gff_i32(
                root.root
                    .get_field("GenericType")
                    .map(|field| field.value()),
            )
            .filter(|value| *value > 0)
            .and_then(|value| usize::try_from(value).ok())
                && let Some(model_name) = appearance_model_from_named_twoda(
                    resman,
                    "genericdoors",
                    generic_type,
                    render_cache,
                )?
            {
                candidates.push(model_name);
            }
            if let Some(model_name) =
                gff_string(root.root.get_field("ModelName").map(|field| field.value()))
                && !model_name.eq_ignore_ascii_case(template_resref)
            {
                candidates.push(model_name);
            }
        } else if blueprint_ext == "utc" {
            if let Some(model_name) = creature_model_from_blueprint(resman, &root, render_cache)? {
                candidates.push(model_name);
            }
            if let Some(model_name) =
                gff_string_any(&root.root, &["ModelName", "Model", "TemplateResRef"])
                && !model_name.eq_ignore_ascii_case(template_resref)
            {
                candidates.push(model_name);
            }
        } else {
            if let Some(appearance) =
                gff_u32(root.root.get_field("Appearance").map(|field| field.value()))
                && let Some(model_name) = appearance_model_from_twoda(
                    resman,
                    blueprint_ext,
                    appearance as usize,
                    render_cache,
                )?
            {
                candidates.push(model_name);
            }
            if let Some(model_name) =
                gff_string(root.root.get_field("ModelName").map(|field| field.value()))
                && !model_name.eq_ignore_ascii_case(template_resref)
            {
                candidates.push(model_name);
            }
        }
    }

    candidates.push(template_resref.to_string());
    dedup_case_insensitive(&mut candidates);
    render_cache
        .blueprints
        .insert(cache_key, candidates.clone());
    Ok(candidates)
}

fn creature_model_from_blueprint(
    resman: &mut nwnrs_resman::prelude::ResMan,
    root: &nwnrs_gff::prelude::GffRoot,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    if let Some(appearance) = gff_u32_any(
        &root.root,
        &["Appearance_Type", "AppearanceType", "Appearance"],
    ) && let Some(model_name) =
        creature_model_from_appearance_row(resman, appearance as usize, render_cache)?
    {
        return Ok(Some(model_name));
    }

    let Some(race) = gff_u32_any(&root.root, &["Race", "Subrace", "RacialType"]) else {
        return Ok(None);
    };
    let Some(appearance) = racialtype_appearance_row(resman, race as usize, render_cache)? else {
        return Ok(None);
    };
    creature_model_from_appearance_row(resman, appearance, render_cache)
}

fn racialtype_appearance_row(
    resman: &mut nwnrs_resman::prelude::ResMan,
    race: usize,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<usize>, String> {
    let table = load_cached_twoda(resman, "racialtypes", render_cache)?;
    let Some(row) = twoda_row_index_for_appearance(&table, race) else {
        return Ok(None);
    };
    Ok(table
        .cell(row, "Appearance")
        .and_then(|value| value.trim().parse::<usize>().ok()))
}

fn creature_model_from_appearance_row(
    resman: &mut nwnrs_resman::prelude::ResMan,
    appearance: usize,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    let table = load_cached_twoda(resman, "appearance", render_cache)?;
    let Some(row) = twoda_row_index_for_appearance(&table, appearance) else {
        return Ok(None);
    };
    let Some(race_model) = table
        .cell(row, "RACE")
        .map(|value| value.trim().to_string())
    else {
        return Ok(None);
    };
    if race_model.is_empty() || race_model == "****" {
        return Ok(None);
    }

    let model_type = table
        .cell(row, "MODELTYPE")
        .map(|value| value.trim().to_ascii_uppercase())
        .unwrap_or_default();
    if model_type == "P" || is_player_appearance_token(race_model.as_str()) {
        return Ok(None);
    }
    Ok(Some(race_model))
}

fn is_player_appearance_token(value: &str) -> bool {
    let trimmed = value.trim();
    trimmed.len() <= 2 && trimmed.chars().all(|ch| ch.is_ascii_alphabetic())
}

fn appearance_model_from_twoda(
    resman: &mut nwnrs_resman::prelude::ResMan,
    blueprint_ext: &str,
    appearance: usize,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    let table_name = match blueprint_ext {
        "utp" => "placeables",
        "utd" => "genericdoors",
        _ => return Ok(None),
    };
    appearance_model_from_named_twoda(resman, table_name, appearance, render_cache)
}

fn appearance_model_from_named_twoda(
    resman: &mut nwnrs_resman::prelude::ResMan,
    table_name: &str,
    appearance: usize,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    let table = load_cached_twoda(resman, table_name, render_cache)?;
    let Some(row) = twoda_row_index_for_appearance(&table, appearance) else {
        return Ok(None);
    };
    for column in twoda_model_columns(table_name) {
        if let Some(value) = table.cell(row, column)
            && !value.trim().is_empty()
            && value != "****"
        {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn door_model_from_template_resref(
    resman: &mut nwnrs_resman::prelude::ResMan,
    template_resref: &str,
    preferred_doortype_tileset: Option<&str>,
    render_cache: &mut AreaRenderCache,
) -> Result<Option<String>, String> {
    let table = load_cached_twoda(resman, "doortypes", render_cache)?;
    let matching_rows =
        twoda_row_indices_for_text(&table, "TemplateResRef", template_resref).collect::<Vec<_>>();
    let Some(row) = preferred_doortype_tileset
        .and_then(|tileset| {
            matching_rows.iter().copied().find(|row| {
                table
                    .cell(*row, "TileSet")
                    .is_some_and(|value| value.eq_ignore_ascii_case(tileset))
            })
        })
        .or_else(|| matching_rows.first().copied())
    else {
        return Ok(None);
    };
    for column in twoda_model_columns("doortypes") {
        if let Some(value) = table.cell(row, column)
            && !value.trim().is_empty()
            && value != "****"
        {
            return Ok(Some(value));
        }
    }
    Ok(None)
}

fn twoda_model_columns(table_name: &str) -> &'static [&'static str] {
    match table_name {
        "doortypes" => &["Model", "ModelName"],
        _ => &["ModelName", "Model"],
    }
}

fn twoda_row_index_for_appearance(table: &TwoDa, appearance: usize) -> Option<usize> {
    (0..table.len())
        .find(|&row| {
            table
                .row_label(row)
                .and_then(|label| label.trim().parse::<usize>().ok())
                .is_some_and(|row_id| row_id == appearance)
        })
        .or_else(|| (appearance < table.len()).then_some(appearance))
}

#[cfg(test)]
fn twoda_row_index_for_text(table: &TwoDa, column: &str, needle: &str) -> Option<usize> {
    twoda_row_indices_for_text(table, column, needle).next()
}

fn twoda_row_indices_for_text<'a>(
    table: &'a TwoDa,
    column: &'a str,
    needle: &'a str,
) -> impl Iterator<Item = usize> + 'a {
    (0..table.len()).filter(move |&row| {
        table
            .cell(row, column)
            .is_some_and(|value| value.eq_ignore_ascii_case(needle))
    })
}

fn load_cached_twoda(
    resman: &mut nwnrs_resman::prelude::ResMan,
    table_name: &str,
    render_cache: &mut AreaRenderCache,
) -> Result<TwoDa, String> {
    if let Some(table) = render_cache.twodas.get(table_name).cloned() {
        return Ok(table);
    }

    let resolved = ResolvedResRef::from_filename(&format!("{table_name}.2da"))
        .map_err(|error| format!("2da resref {table_name}.2da: {error}"))?;
    let res = resman
        .get_resolved(&resolved)
        .ok_or_else(|| format!("2da not found in ResMan: {table_name}.2da"))?;
    let table = as_2da(&res).map_err(|error| format!("read {table_name}.2da: {error}"))?;
    render_cache
        .twodas
        .insert(table_name.to_string(), table.clone());
    Ok(table)
}

#[allow(clippy::too_many_arguments)]
fn spawn_git_model_instance(
    commands: &mut Commands<'_, '_>,
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    area_root: Entity,
    display_name: &str,
    category: &str,
    layout: AreaLayoutContext,
    transform: &GitTransform,
    model_candidates: &[String],
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    fallback_color: Color,
) {
    let translation = world_translation_from_git(transform, layout);
    let rotation = world_rotation_from_git(transform);
    let instance_name = format!("{category}_{display_name}");
    let instance_root = commands
        .spawn((
            Name::new(instance_name.clone()),
            area_spatial_components(
                Transform::from_translation(translation).with_rotation(rotation),
            ),
        ))
        .id();
    commands.entity(area_root).add_child(instance_root);

    let mut attempted_models = Vec::new();
    let mut failed_models = Vec::new();
    for model_name in model_candidates {
        attempted_models.push(model_name.clone());
        match load_cached_model_asset(
            resman,
            module_path,
            model_name,
            render_cache,
            images,
            meshes,
            materials,
        ) {
            Ok(model) => {
                let model_root = spawn_nwn_model(commands, &model);
                commands.entity(model_root).insert(Transform::default());
                commands.entity(instance_root).add_child(model_root);
                debug!(
                    category,
                    display_name,
                    model = model_name,
                    candidates = attempted_models.join(", "),
                    "spawned git model instance"
                );
                return;
            }
            Err(error) => {
                failed_models.push(format!("{model_name}: {error}"));
            }
        }
    }

    if failed_models.is_empty() {
        warn!(
            category,
            display_name, "no model candidates were resolved for git instance"
        );
    } else {
        warn!(
            category,
            display_name,
            candidates = attempted_models.join(", "),
            failures = failed_models.join(" | "),
            "failed to resolve any git instance model candidates"
        );
    }
    spawn_instance_marker_with_root(
        commands,
        meshes,
        materials,
        instance_root,
        Vec3::new(INSTANCE_MARKER_SIZE, 1.4, INSTANCE_MARKER_SIZE),
        fallback_color,
        true,
    );
}

fn spawn_git_composed_model_instance(
    commands: &mut Commands<'_, '_>,
    area_root: Entity,
    display_name: &str,
    category: &str,
    layout: AreaLayoutContext,
    transform: &GitTransform,
    model: &NwnModelAsset,
) {
    let translation = world_translation_from_git(transform, layout);
    let rotation = world_rotation_from_git(transform);
    let instance_name = format!("{category}_{display_name}");
    let instance_root = commands
        .spawn((
            Name::new(instance_name),
            area_spatial_components(
                Transform::from_translation(translation).with_rotation(rotation),
            ),
        ))
        .id();
    commands.entity(area_root).add_child(instance_root);

    let model_root = spawn_nwn_model(commands, model);
    commands.entity(model_root).insert(Transform::default());
    commands.entity(instance_root).add_child(model_root);
}

fn spawn_instance_marker(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    area_root: Entity,
    name: String,
    layout: AreaLayoutContext,
    transform: &GitTransform,
    marker_scale: Vec3,
    color: Color,
    with_forward_indicator: bool,
) {
    let instance_root = commands
        .spawn((
            Name::new(name),
            area_spatial_components(
                Transform::from_translation(world_translation_from_git(transform, layout))
                    .with_rotation(world_rotation_from_git(transform)),
            ),
        ))
        .id();
    commands.entity(area_root).add_child(instance_root);
    spawn_instance_marker_with_root(
        commands,
        meshes,
        materials,
        instance_root,
        marker_scale,
        color,
        with_forward_indicator,
    );
}

fn spawn_waypoint_marker(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    area_root: Entity,
    name: String,
    layout: AreaLayoutContext,
    transform: &GitTransform,
    body_color: Color,
    tip_color: Color,
) {
    let instance_root = commands
        .spawn((
            Name::new(name),
            area_spatial_components(
                Transform::from_translation(world_translation_from_git(transform, layout))
                    .with_rotation(world_rotation_from_git(transform)),
            ),
        ))
        .id();
    commands.entity(area_root).add_child(instance_root);

    let shaft_mesh = meshes.add(Cylinder::new(0.12, 1.35));
    let shaft_material = materials.add(StandardMaterial {
        base_color: body_color,
        emissive: body_color.into(),
        perceptual_roughness: 0.85,
        ..Default::default()
    });
    let head_mesh = meshes.add(Cone::new(0.28, 0.5));
    let head_material = materials.add(StandardMaterial {
        base_color: tip_color,
        emissive: tip_color.into(),
        perceptual_roughness: 0.7,
        ..Default::default()
    });
    let base_mesh = meshes.add(Cylinder::new(0.32, 0.08));
    let base_material = materials.add(StandardMaterial {
        base_color: body_color.with_alpha(0.4),
        emissive: body_color.mix(&Color::WHITE, 0.15).into(),
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        ..Default::default()
    });

    commands.entity(instance_root).with_children(|children| {
        children.spawn((
            Name::new("waypoint_base"),
            Mesh3d(base_mesh),
            MeshMaterial3d(base_material),
            Transform::from_translation(Vec3::Y * 0.04),
        ));
        children.spawn((
            Name::new("waypoint_shaft"),
            Mesh3d(shaft_mesh),
            MeshMaterial3d(shaft_material),
            Transform::from_translation(Vec3::Y * 0.68),
        ));
        children.spawn((
            Name::new("waypoint_head"),
            Mesh3d(head_mesh),
            MeshMaterial3d(head_material),
            Transform::from_translation(Vec3::Y * 1.55),
        ));
    });
}

fn spawn_trigger_geometry(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    area_root: Entity,
    name: String,
    layout: AreaLayoutContext,
    transform: &GitTransform,
    geometry: &[GitPoint],
    fill_color: Color,
    edge_color: Color,
) {
    if geometry.len() < 3 {
        spawn_instance_marker(
            commands,
            meshes,
            materials,
            area_root,
            name,
            layout,
            transform,
            Vec3::new(INSTANCE_MARKER_SIZE, 1.2, INSTANCE_MARKER_SIZE),
            edge_color,
            false,
        );
        return;
    }

    let trigger_root = commands
        .spawn((
            Name::new(name),
            area_spatial_components(Transform::default()),
        ))
        .id();
    commands.entity(area_root).add_child(trigger_root);

    let polygon = geometry
        .iter()
        .map(|point| world_translation_from_git_point(point, layout))
        .collect::<Vec<_>>();

    if let Some(fill_mesh) = build_trigger_fill_mesh(&polygon) {
        let fill_material = materials.add(StandardMaterial {
            base_color: fill_color,
            emissive: edge_color.with_alpha(0.08).into(),
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            unlit: true,
            ..Default::default()
        });
        commands.entity(trigger_root).with_children(|children| {
            children.spawn((
                Name::new("trigger_fill"),
                Mesh3d(meshes.add(fill_mesh)),
                MeshMaterial3d(fill_material),
            ));
        });
    }

    let edge_material = materials.add(StandardMaterial {
        base_color: edge_color,
        emissive: edge_color.into(),
        perceptual_roughness: 0.8,
        unlit: true,
        ..Default::default()
    });
    commands.entity(trigger_root).with_children(|children| {
        for (index, start) in polygon.iter().enumerate() {
            let end = polygon[(index + 1) % polygon.len()];
            let span = end - *start;
            let length = span.length();
            if length <= f32::EPSILON {
                continue;
            }
            let midpoint = *start + span * 0.5 + Vec3::Y * 0.08;
            let yaw = span.x.atan2(span.z);
            children.spawn((
                Name::new(format!("trigger_edge_{index}")),
                Mesh3d(meshes.add(Cuboid::new(0.08, 0.16, length))),
                MeshMaterial3d(edge_material.clone()),
                Transform::from_translation(midpoint).with_rotation(Quat::from_rotation_y(yaw)),
            ));
            children.spawn((
                Name::new(format!("trigger_post_{index}")),
                Mesh3d(meshes.add(Cuboid::new(0.08, TRIGGER_OVERLAY_HEIGHT, 0.08))),
                MeshMaterial3d(edge_material.clone()),
                Transform::from_translation(*start + Vec3::Y * (TRIGGER_OVERLAY_HEIGHT * 0.5)),
            ));
        }
    });
}

fn spawn_instance_marker_with_root(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    instance_root: Entity,
    marker_scale: Vec3,
    color: Color,
    with_forward_indicator: bool,
) {
    let body_mesh = meshes.add(Cuboid::new(marker_scale.x, marker_scale.y, marker_scale.z));
    let body_material = materials.add(StandardMaterial {
        base_color: color,
        emissive: color.into(),
        perceptual_roughness: 0.8,
        alpha_mode: AlphaMode::Blend,
        ..Default::default()
    });

    commands.entity(instance_root).with_children(|children| {
        children.spawn((
            Name::new("marker_body"),
            Mesh3d(body_mesh),
            MeshMaterial3d(body_material),
            Transform::from_translation(Vec3::Y * (marker_scale.y * 0.5)),
        ));

        if with_forward_indicator {
            let indicator_mesh = meshes.add(Cuboid::new(
                marker_scale.x * 0.4,
                marker_scale.y * 0.18,
                marker_scale.z * 0.8,
            ));
            let indicator_material = materials.add(StandardMaterial {
                base_color: color.mix(&Color::WHITE, 0.4),
                emissive: color.mix(&Color::WHITE, 0.2).into(),
                ..Default::default()
            });
            children.spawn((
                Name::new("marker_forward"),
                Mesh3d(indicator_mesh),
                MeshMaterial3d(indicator_material),
                Transform::from_translation(Vec3::new(
                    0.0,
                    marker_scale.y * 0.78,
                    marker_scale.z * 0.42,
                )),
            ));
        }
    });
}

fn build_trigger_fill_mesh(points: &[Vec3]) -> Option<Mesh> {
    if points.len() < 3 {
        return None;
    }

    let origin = points[0];
    let mut positions = Vec::<[f32; 3]>::with_capacity(points.len());
    let mut normals = Vec::<[f32; 3]>::with_capacity(points.len());
    let mut uvs = Vec::<[f32; 2]>::with_capacity(points.len());
    let mut min = Vec2::splat(f32::MAX);
    let mut max = Vec2::splat(f32::MIN);
    for point in points {
        let planar = Vec2::new(point.x, point.z);
        min = min.min(planar);
        max = max.max(planar);
    }
    let extent = (max - min).max(Vec2::splat(0.001));
    for point in points {
        positions.push([point.x, point.y + 0.02, point.z]);
        normals.push(Vec3::Y.to_array());
        let planar = Vec2::new(point.x, point.z);
        let uv = (planar - min) / extent;
        uvs.push(uv.to_array());
    }

    let mut indices = Vec::<u32>::with_capacity((points.len() - 2) * 3);
    for index in 1..(points.len() - 1) {
        indices.push(0);
        indices.push(index as u32);
        indices.push((index + 1) as u32);
    }

    if (points[1] - origin).cross(points[2] - origin).y < 0.0 {
        for triangle in indices.chunks_exact_mut(3) {
            triangle.swap(1, 2);
        }
    }

    let mut mesh = Mesh::new(
        PrimitiveTopology::TriangleList,
        RenderAssetUsages::default(),
    );
    mesh.insert_attribute(Mesh::ATTRIBUTE_POSITION, positions);
    mesh.insert_attribute(Mesh::ATTRIBUTE_NORMAL, normals);
    mesh.insert_attribute(Mesh::ATTRIBUTE_UV_0, uvs);
    mesh.insert_indices(Indices::U32(indices));
    Some(mesh)
}

fn world_translation_from_git(transform: &GitTransform, layout: AreaLayoutContext) -> Vec3 {
    let area_width = layout.width as f32 * layout.tile_spacing;
    let area_height = layout.height as f32 * layout.tile_spacing;
    Vec3::new(
        transform.x.unwrap_or(0.0) - area_width * 0.5,
        transform.z.unwrap_or(0.0),
        (area_height * 0.5) - transform.y.unwrap_or(0.0),
    )
}

fn world_translation_from_git_point(point: &GitPoint, layout: AreaLayoutContext) -> Vec3 {
    let area_width = layout.width as f32 * layout.tile_spacing;
    let area_height = layout.height as f32 * layout.tile_spacing;
    Vec3::new(
        point.x.unwrap_or(0.0) - area_width * 0.5,
        point.z.unwrap_or(0.0),
        (area_height * 0.5) - point.y.unwrap_or(0.0),
    )
}

fn world_rotation_from_git(transform: &GitTransform) -> Quat {
    if let (Some(x), Some(y)) = (transform.x_orientation, transform.y_orientation) {
        let direction = Vec3::new(x, 0.0, -y);
        if let Some(normalized) = direction.try_normalize() {
            return Quat::from_rotation_arc(Vec3::NEG_Z, normalized);
        }
    }

    transform
        .bearing
        .map(|bearing| Quat::from_rotation_y(-bearing))
        .unwrap_or(Quat::IDENTITY)
}

fn log_git_instances(area: &TestArea, git: &GitFile) {
    info!(
        area = area.resref.as_str(),
        creature_count = git.creatures.len(),
        door_count = git.doors.len(),
        encounter_count = git.encounters.len(),
        placeable_count = git.placeables.len(),
        sound_count = git.sounds.len(),
        store_count = git.stores.len(),
        trigger_count = git.triggers.len(),
        waypoint_count = git.waypoints.len(),
        "loaded area git instances"
    );
}

fn dedup_case_insensitive(values: &mut Vec<String>) {
    let mut seen = BTreeSet::new();
    values.retain(|value| seen.insert(value.to_ascii_lowercase()));
}

fn build_tileset_grass_visual(
    resman: &mut nwnrs_resman::prelude::ResMan,
    tileset: &SetFile,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Option<GrassVisual> {
    let grass = tileset.grass.as_ref()?;
    if grass.grass == Some(false) || tileset.general.interior == Some(true) {
        return None;
    }

    let texture = grass
        .texture_name
        .as_deref()
        .and_then(|name| load_named_image_from_resman(resman, name, images));
    let diffuse = grass.diffuse.unwrap_or([0.34, 0.55, 0.22]);
    let ambient = grass.ambient.unwrap_or([0.2, 0.32, 0.14]);
    let blade_height = grass.height.unwrap_or(1.15).clamp(0.35, 2.2);
    let density = grass.density.unwrap_or(1.0).clamp(0.25, 3.0);
    let patch_count = (density * 6.0).round().clamp(2.0, 18.0) as usize;
    let blade_width = (blade_height * 0.42).clamp(0.18, 0.7);
    let blade_mesh = meshes.add(Rectangle::new(blade_width, blade_height));
    let blade_material = materials.add(StandardMaterial {
        base_color: Color::srgb(diffuse[0], diffuse[1], diffuse[2]),
        emissive: Color::srgb(ambient[0], ambient[1], ambient[2]).into(),
        base_color_texture: texture,
        alpha_mode: AlphaMode::Blend,
        cull_mode: None,
        perceptual_roughness: 1.0,
        ..Default::default()
    });

    Some(GrassVisual {
        blade_mesh,
        blade_material,
        blade_height,
        patch_count,
    })
}

fn spawn_tile_grass(
    commands: &mut Commands<'_, '_>,
    tile_entity: Entity,
    row: u32,
    col: u32,
    tileset: &SetFile,
    tile_definition: Option<&nwnrs_set::prelude::SetTile>,
    tile_spacing: f32,
    grass_visual: &GrassVisual,
) {
    if !tile_supports_grass(tileset, tile_definition) {
        return;
    }

    commands.entity(tile_entity).with_children(|children| {
        for patch_index in 0..grass_visual.patch_count {
            let patch_seed = deterministic_patch_seed(row, col, patch_index as u32);
            let local_x = hash_to_unit(patch_seed ^ 0x9e37_79b9) * (tile_spacing * 0.72)
                - (tile_spacing * 0.36);
            let local_z = hash_to_unit(patch_seed ^ 0x7f4a_7c15) * (tile_spacing * 0.72)
                - (tile_spacing * 0.36);
            let yaw = hash_to_unit(patch_seed ^ 0x85eb_ca6b) * std::f32::consts::TAU;
            let scale = 0.82 + hash_to_unit(patch_seed ^ 0xc2b2_ae35) * 0.55;
            children
                .spawn((
                    Name::new(format!("grass_patch_{patch_index}")),
                    area_spatial_components(
                        Transform::from_translation(Vec3::new(local_x, 0.04, local_z))
                            .with_rotation(Quat::from_rotation_y(yaw))
                            .with_scale(Vec3::splat(scale)),
                    ),
                ))
                .with_children(|patch_children| {
                    for blade_index in 0..3 {
                        let rotation =
                            Quat::from_rotation_y(blade_index as f32 * (FRAC_PI_2 * 0.67));
                        patch_children.spawn((
                            Name::new(format!("grass_blade_{blade_index}")),
                            Mesh3d(grass_visual.blade_mesh.clone()),
                            MeshMaterial3d(grass_visual.blade_material.clone()),
                            Transform::from_translation(
                                Vec3::Y * (grass_visual.blade_height * 0.5),
                            )
                            .with_rotation(rotation),
                        ));
                    }
                });
        }
    });
}

fn tile_supports_grass(
    tileset: &SetFile,
    tile_definition: Option<&nwnrs_set::prelude::SetTile>,
) -> bool {
    let Some(tile_definition) = tile_definition else {
        return false;
    };

    let terrains = [
        tile_definition.top_left.terrain.as_deref(),
        tile_definition.top_right.terrain.as_deref(),
        tile_definition.bottom_left.terrain.as_deref(),
        tile_definition.bottom_right.terrain.as_deref(),
    ]
    .into_iter()
    .flatten()
    .map(str::trim)
    .filter(|value| !value.is_empty() && *value != "****")
    .collect::<Vec<_>>();
    if terrains.is_empty() {
        return false;
    }

    let first = terrains[0];
    if !terrains
        .iter()
        .all(|terrain| terrain.eq_ignore_ascii_case(first))
    {
        return false;
    }
    if tileset
        .general
        .border
        .as_deref()
        .is_some_and(|border| border.eq_ignore_ascii_case(first))
    {
        return false;
    }
    tileset
        .general
        .default_terrain
        .as_deref()
        .is_some_and(|terrain| terrain.eq_ignore_ascii_case(first))
        || tileset
            .general
            .floor
            .as_deref()
            .is_some_and(|terrain| terrain.eq_ignore_ascii_case(first))
        || first.to_ascii_lowercase().contains("grass")
}

fn deterministic_patch_seed(row: u32, col: u32, patch: u32) -> u32 {
    row.wrapping_mul(0x9e37_79b9) ^ col.wrapping_mul(0x85eb_ca6b) ^ patch.wrapping_mul(0xc2b2_ae35)
}

fn hash_to_unit(seed: u32) -> f32 {
    let value = seed ^ (seed >> 16);
    (value as f32) / (u32::MAX as f32)
}

fn load_named_image_from_resman(
    resman: &mut nwnrs_resman::prelude::ResMan,
    stem: &str,
    images: &mut Assets<Image>,
) -> Option<Handle<Image>> {
    let stem = stem.trim();
    if stem.is_empty() || stem == "****" {
        return None;
    }

    let mut try_load = |resman: &mut nwnrs_resman::prelude::ResMan,
                        stem: &str,
                        extension: &str|
     -> Option<Handle<Image>> {
        let resolved = ResolvedResRef::from_filename(&format!("{stem}.{extension}")).ok()?;
        let res = resman.get_resolved(&resolved)?;
        let image = match extension {
            "dds" => image_from_dds(&read_dds_from_res(&res, true).ok()?).ok()?,
            "tga" => image_from_tga(&read_tga_from_res(&res, true).ok()?).ok()?,
            "plt" => image_from_plt(&read_plt_from_res(&res, true).ok()?).ok()?,
            _ => return None,
        };
        Some(images.add(image))
    };

    try_load(resman, stem, "dds")
        .or_else(|| try_load(resman, stem, "tga"))
        .or_else(|| try_load(resman, stem, "plt"))
}

fn load_cached_model_asset(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    model_name: &str,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<NwnModelAsset, String> {
    load_cached_model_asset_with_overrides(
        resman,
        module_path,
        model_name,
        &NwnAppearanceOverrides::default(),
        render_cache,
        images,
        meshes,
        materials,
    )
}

fn load_cached_model_asset_with_overrides(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    model_name: &str,
    overrides: &NwnAppearanceOverrides,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
) -> Result<NwnModelAsset, String> {
    if overrides == &NwnAppearanceOverrides::default() {
        let cache_key = (module_path.to_path_buf(), model_name.to_string());
        if let Some(model) = render_cache.models.get(&cache_key).cloned() {
            return Ok(model);
        }

        let model = load_nwn_model_from_resman(resman, model_name, images, meshes, materials)
            .map_err(|error| error.to_string())?;
        log_model_diagnostics(model_name, &model);
        if !model.unresolved.is_empty() {
            warn!(
                model = model_name,
                unresolved = model.unresolved.len(),
                "loaded tile model with unresolved textures"
            );
        }
        render_cache.models.insert(cache_key, model.clone());
        return Ok(model);
    }

    let cache_key = (
        module_path.to_path_buf(),
        model_name.to_string(),
        overrides.clone(),
    );
    if let Some(model) = render_cache.overridden_models.get(&cache_key).cloned() {
        return Ok(model);
    }

    let model = load_nwn_model_from_resman_with_overrides(
        resman, model_name, overrides, images, meshes, materials,
    )
    .map_err(|error| error.to_string())?;
    log_model_diagnostics(model_name, &model);
    render_cache
        .overridden_models
        .insert(cache_key, model.clone());
    Ok(model)
}

fn resolve_area_tile_model_name(
    resman: &mut nwnrs_resman::prelude::ResMan,
    module_path: &Path,
    model_name: &str,
    render_cache: &mut AreaRenderCache,
) -> Option<String> {
    let cache_key = (module_path.to_path_buf(), model_name.to_string());
    if let Some(cached) = render_cache.resolved_model_names.get(&cache_key).cloned() {
        return cached;
    }

    let resolved = if ResRef::new(model_name.to_string(), MODEL_RES_TYPE)
        .ok()
        .and_then(|resref| resman.get(&resref))
        .is_some()
    {
        Some(model_name.to_string())
    } else {
        let available_models = resman
            .contents()
            .into_iter()
            .filter(|resref| resref.res_type() == MODEL_RES_TYPE)
            .map(|resref| resref.res_ref().to_string())
            .collect::<Vec<_>>();
        infer_unambiguous_model_variant(model_name, &available_models)
    };

    if let Some(resolved_model_name) = resolved.as_deref()
        && !resolved_model_name.eq_ignore_ascii_case(model_name)
    {
        warn!(
            requested = model_name,
            resolved = resolved_model_name,
            "using unambiguous sibling mdl for missing tileset model"
        );
    }

    render_cache
        .resolved_model_names
        .insert(cache_key, resolved.clone());
    resolved
}

fn infer_unambiguous_model_variant(
    model_name: &str,
    available_models: &[String],
) -> Option<String> {
    let (prefix, suffix) = model_name.rsplit_once('_')?;
    let stem = format!("{prefix}_");
    let mut candidates = available_models
        .iter()
        .filter(|candidate| {
            candidate.len() == model_name.len()
                && candidate
                    .to_ascii_lowercase()
                    .starts_with(&stem.to_ascii_lowercase())
                && !candidate.eq_ignore_ascii_case(model_name)
        })
        .cloned()
        .collect::<Vec<_>>();
    candidates.retain(|candidate| {
        candidate
            .rsplit_once('_')
            .is_some_and(|(candidate_prefix, candidate_suffix)| {
                candidate_prefix.eq_ignore_ascii_case(prefix)
                    && candidate_suffix.len() == suffix.len()
            })
    });
    (candidates.len() == 1).then(|| candidates.remove(0))
}

#[derive(Debug, Clone, Copy, Default)]
struct TileOrientationValidation {
    horizontal_matches:       usize,
    horizontal_mismatches:    usize,
    vertical_matches:         usize,
    vertical_mismatches:      usize,
    missing_tile_definitions: usize,
}

fn validate_tile_orientation_mapping(
    area: &TestArea,
    tileset: &SetFile,
    mapping: TileOrientationMapping,
) -> TileOrientationValidation {
    let mut validation = TileOrientationValidation::default();

    for row in 0..area.height {
        for col in 0..area.width {
            let index = (row * area.width + col) as usize;
            let Some(tile) = area.tiles.get(index) else {
                continue;
            };
            let Some(tile_set) = tileset.tiles.get(&tile.id) else {
                validation.missing_tile_definitions += 1;
                continue;
            };
            let current = rotated_tile_signature(tile_set, tile.orientation, mapping);

            if col + 1 < area.width {
                let east_index = (row * area.width + (col + 1)) as usize;
                if let Some(east_tile) = area.tiles.get(east_index) {
                    let Some(east_set) = tileset.tiles.get(&east_tile.id) else {
                        validation.missing_tile_definitions += 1;
                        continue;
                    };
                    let east = rotated_tile_signature(east_set, east_tile.orientation, mapping);
                    if tile_edges_match(current.right, east.left)
                        && tile_corners_match(current.top_right, east.top_left)
                        && tile_corners_match(current.bottom_right, east.bottom_left)
                    {
                        validation.horizontal_matches += 1;
                    } else {
                        validation.horizontal_mismatches += 1;
                    }
                }
            }

            if row + 1 < area.height {
                let next_row_index = ((row + 1) * area.width + col) as usize;
                if let Some(next_tile) = area.tiles.get(next_row_index) {
                    let Some(next_set) = tileset.tiles.get(&next_tile.id) else {
                        validation.missing_tile_definitions += 1;
                        continue;
                    };
                    let next = rotated_tile_signature(next_set, next_tile.orientation, mapping);
                    let matches = if mapping.rows_go_south {
                        tile_edges_match(current.bottom, next.top)
                            && tile_corners_match(current.bottom_left, next.top_left)
                            && tile_corners_match(current.bottom_right, next.top_right)
                    } else {
                        tile_edges_match(current.top, next.bottom)
                            && tile_corners_match(current.top_left, next.bottom_left)
                            && tile_corners_match(current.top_right, next.bottom_right)
                    };
                    if matches {
                        validation.vertical_matches += 1;
                    } else {
                        validation.vertical_mismatches += 1;
                    }
                }
            }
        }
    }

    validation
}

#[derive(Clone, Copy)]
struct RotatedTileSignature<'a> {
    top:          Option<&'a str>,
    right:        Option<&'a str>,
    bottom:       Option<&'a str>,
    left:         Option<&'a str>,
    top_left:     TileCornerSignature<'a>,
    top_right:    TileCornerSignature<'a>,
    bottom_right: TileCornerSignature<'a>,
    bottom_left:  TileCornerSignature<'a>,
}

#[derive(Clone, Copy)]
struct TileCornerSignature<'a> {
    terrain: Option<&'a str>,
    height:  Option<i32>,
}

fn rotated_tile_signature<'a>(
    tile: &'a nwnrs_set::prelude::SetTile,
    raw_orientation: u32,
    mapping: TileOrientationMapping,
) -> RotatedTileSignature<'a> {
    let turns = tile_orientation_turns(raw_orientation, mapping) as usize;

    let edges = [
        tile.edge_crossers.top.as_deref(),
        tile.edge_crossers.right.as_deref(),
        tile.edge_crossers.bottom.as_deref(),
        tile.edge_crossers.left.as_deref(),
    ];
    let corners = [
        TileCornerSignature {
            terrain: tile.top_left.terrain.as_deref(),
            height:  tile.top_left.height,
        },
        TileCornerSignature {
            terrain: tile.top_right.terrain.as_deref(),
            height:  tile.top_right.height,
        },
        TileCornerSignature {
            terrain: tile.bottom_right.terrain.as_deref(),
            height:  tile.bottom_right.height,
        },
        TileCornerSignature {
            terrain: tile.bottom_left.terrain.as_deref(),
            height:  tile.bottom_left.height,
        },
    ];

    let mut rotated_edges = [None; 4];
    let mut rotated_corners = [TileCornerSignature {
        terrain: None,
        height:  None,
    }; 4];

    for (index, edge) in edges.into_iter().enumerate() {
        rotated_edges[(index + turns) % 4] = edge;
    }
    for (index, corner) in corners.into_iter().enumerate() {
        rotated_corners[(index + turns) % 4] = corner;
    }

    RotatedTileSignature {
        top:          rotated_edges[0],
        right:        rotated_edges[1],
        bottom:       rotated_edges[2],
        left:         rotated_edges[3],
        top_left:     rotated_corners[0],
        top_right:    rotated_corners[1],
        bottom_right: rotated_corners[2],
        bottom_left:  rotated_corners[3],
    }
}

fn tile_orientation_turns(raw_orientation: u32, mapping: TileOrientationMapping) -> u8 {
    let raw = (raw_orientation % 4) as i32;
    let direction = i32::from(mapping.turn_direction);
    ((i32::from(mapping.turn_offset) + (direction * raw)).rem_euclid(4)) as u8
}

fn select_tile_animation_name(
    tile: &nwnrs_set::prelude::SetTile,
    model: &NwnModelAsset,
) -> Option<String> {
    let enabled_loops = [
        tile.anim_loop_1.unwrap_or(false).then_some(1_u8),
        tile.anim_loop_2.unwrap_or(false).then_some(2_u8),
        tile.anim_loop_3.unwrap_or(false).then_some(3_u8),
    ];

    for loop_index in enabled_loops.into_iter().flatten() {
        for candidate in tile_animation_candidates(loop_index) {
            if let Some(animation) = model.scene.animation(candidate.as_str()) {
                return Some(animation.name.clone());
            }
        }
    }

    None
}

fn tile_animation_candidates(loop_index: u8) -> [String; 8] {
    [
        format!("animloop{loop_index}"),
        format!("animloop0{loop_index}"),
        format!("anim_loop{loop_index}"),
        format!("anim_loop0{loop_index}"),
        format!("loop{loop_index}"),
        format!("loop0{loop_index}"),
        format!("anim{loop_index}"),
        format!("anim0{loop_index}"),
    ]
}

fn tile_edges_match(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (Some(left), Some(right)) => left == right,
        (None, None) => true,
        _ => false,
    }
}

fn tile_corners_match(left: TileCornerSignature<'_>, right: TileCornerSignature<'_>) -> bool {
    left.terrain == right.terrain && left.height == right.height
}

fn spawn_tile_fallback(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    tile_entity: Entity,
    tile: TestAreaTile,
    tile_size: f32,
    reason: String,
) {
    warn!(
        tile_id = tile.id,
        orientation = tile.orientation,
        height = tile.height,
        "{reason}"
    );

    let base_color = color_for_tile(tile.id);
    let tile_mesh = meshes.add(Cuboid::new(
        tile_size * 0.96,
        TILE_THICKNESS,
        tile_size * 0.96,
    ));
    let indicator_mesh = meshes.add(Cuboid::new(tile_size * 0.18, 0.4, tile_size * 0.48));
    let tile_material = materials.add(StandardMaterial {
        base_color,
        perceptual_roughness: 0.92,
        metallic: 0.02,
        ..Default::default()
    });
    let indicator_material = materials.add(StandardMaterial {
        base_color: base_color.mix(&Color::WHITE, 0.45),
        emissive: base_color.into(),
        ..Default::default()
    });

    commands.entity(tile_entity).with_children(|children| {
        children.spawn((
            Name::new("fallback_tile"),
            Mesh3d(tile_mesh),
            MeshMaterial3d(tile_material),
            Transform::default(),
        ));

        let indicator_offset =
            Vec3::Z * (tile_size * 0.28) + Vec3::new(0.0, TILE_THICKNESS * 0.5 + 0.25, 0.0);
        children.spawn((
            Name::new("fallback_orientation"),
            Mesh3d(indicator_mesh),
            MeshMaterial3d(indicator_material),
            Transform::from_translation(indicator_offset),
        ));
    });
}

fn log_area_layout(
    area: &TestArea,
    tileset: &SetFile,
    orientation_mapping: TileOrientationMapping,
) {
    let validation = validate_tile_orientation_mapping(area, tileset, orientation_mapping);
    info!(
        width = area.width,
        height = area.height,
        actual_tile_count = area.tiles.len(),
        expected_tile_count = area_tile_count(area.width, area.height).unwrap_or(0),
        tileset = area.tileset.as_str(),
        mapped_tiles = tileset.tiles.len(),
        group_count = tileset.groups.len(),
        terrain_count = tileset.terrains.len(),
        crosser_count = tileset.crossers.len(),
        orientation_turn_offset = orientation_mapping.turn_offset,
        orientation_turn_direction = orientation_mapping.turn_direction,
        orientation_rows_go_south = orientation_mapping.rows_go_south,
        horizontal_matches = validation.horizontal_matches,
        horizontal_mismatches = validation.horizontal_mismatches,
        vertical_matches = validation.vertical_matches,
        vertical_mismatches = validation.vertical_mismatches,
        missing_tile_definitions = validation.missing_tile_definitions,
        "resolved tileset definition"
    );
    if validation.horizontal_mismatches > 0
        || validation.vertical_mismatches > 0
        || validation.missing_tile_definitions > 0
    {
        warn!(
            tileset = area.tileset.as_str(),
            horizontal_mismatches = validation.horizontal_mismatches,
            vertical_mismatches = validation.vertical_mismatches,
            missing_tile_definitions = validation.missing_tile_definitions,
            "SET-driven tile orientation validation found mismatches"
        );
    }

    for row in 0..area.height {
        let mut cells = Vec::new();
        for col in 0..area.width {
            let index = (row * area.width + col) as usize;
            let Some(tile) = area.tiles.get(index) else {
                continue;
            };
            let turns = tile_orientation_turns(tile.orientation, orientation_mapping);
            let model_name = tileset
                .tiles
                .get(&tile.id)
                .and_then(|tile_def| tile_def.model.as_deref())
                .unwrap_or("<missing>");
            cells.push(format!(
                "({row},{col}) id={} raw_rot={} turns={} h={} model={model_name}",
                tile.id, tile.orientation, turns, tile.height
            ));
        }
        debug!(row, layout = cells.join(" | "), "expected area row");
    }
}

fn log_model_diagnostics(model_name: &str, model: &NwnModelAsset) {
    let primitive_count = model
        .nodes
        .iter()
        .map(|node| node.primitives.len())
        .sum::<usize>();
    debug!(
        model = model_name,
        node_count = model.nodes.len(),
        root_count = model.root_nodes.len(),
        primitive_count,
        material_count = model.scene.materials.len(),
        unresolved = model.unresolved.len(),
        "loaded area tile model"
    );

    for (material_index, material) in model.scene.materials.iter().enumerate() {
        let texture_slots = material
            .textures
            .iter()
            .map(|texture| match &texture.slot {
                nwnrs_mdl::prelude::NwnTextureSlot::Bitmap => format!("bitmap={}", texture.name),
                nwnrs_mdl::prelude::NwnTextureSlot::Texture(slot) => {
                    format!("texture{slot}={}", texture.name)
                }
            })
            .collect::<Vec<_>>();
        debug!(
            model = model_name,
            material_index,
            source_node = material.source_node,
            rotate_texture = material.rotate_texture,
            tilefade = material.tilefade,
            alpha = material.alpha,
            render_hint = material.render_hint.as_deref().unwrap_or(""),
            material_name = material.material_name.as_deref().unwrap_or(""),
            textures = texture_slots.join(", "),
            "tile model material"
        );
    }

    if !model.unresolved.is_empty() {
        let unique_unresolved = model
            .unresolved
            .iter()
            .map(|unresolved_texture| {
                (
                    unresolved_texture.material_index,
                    format!("{:?}", unresolved_texture.slot),
                    unresolved_texture.name.clone(),
                    format!("{:?}", unresolved_texture.reason),
                    unresolved_texture.attempted.clone(),
                )
            })
            .collect::<BTreeSet<_>>();
        warn!(
            model = model_name,
            unresolved_textures = unique_unresolved.len(),
            "loaded mdl with unresolved textures"
        );
        for (material_index, slot, texture, reason, attempted) in unique_unresolved {
            let material = model.scene.materials.get(material_index);
            let source_node =
                material.and_then(|material| model.scene.nodes.get(material.source_node));
            warn!(
                model = model_name,
                material_index,
                source_node = material.map(|material| material.source_node as i64).unwrap_or(-1),
                source_name = source_node.map(|node| node.name.as_str()).unwrap_or(""),
                source_kind = source_node
                    .map(|node| format!("{:?}", node.kind))
                    .unwrap_or_default(),
                render_relevant = material_requires_bitmap_resolution(&model.scene, material_index),
                slot = slot.as_str(),
                texture = texture.as_str(),
                reason = reason.as_str(),
                attempted = ?attempted,
                "unresolved mdl texture"
            );
        }
    }
}

fn collect_water_flow_samples(
    samples: &mut Vec<WaterFlowSample>,
    row: u32,
    col: u32,
    tile: TestAreaTile,
    model_name: &str,
    tile_world_yaw: f32,
    model: &NwnModelAsset,
) {
    let tile_transform = Transform::from_rotation(Quat::from_rotation_y(tile_world_yaw));
    for (node_index, scene_node) in model.scene.nodes.iter().enumerate() {
        let Some(mesh_index) = scene_node.mesh else {
            continue;
        };
        let Some(mesh) = model.scene.meshes.get(mesh_index) else {
            continue;
        };
        let node_world_transform =
            tile_transform.mul_transform(model_node_world_transform(model, node_index));
        let Some(node_asset) = model.nodes.get(node_index) else {
            continue;
        };

        for (scene_primitive_index, scene_primitive) in mesh.primitives.iter().enumerate() {
            let Some(material_index) = scene_primitive.material else {
                continue;
            };
            let Some(material) = model.scene.materials.get(material_index) else {
                continue;
            };
            let texture_name = material
                .textures
                .iter()
                .find_map(|texture| {
                    matches!(texture.slot, nwnrs_mdl::prelude::NwnTextureSlot::Bitmap)
                        .then_some(texture.name.as_str())
                })
                .unwrap_or_default();
            let is_water_like = texture_name.to_ascii_lowercase().contains("water");
            if !is_water_like {
                continue;
            }

            let Some(primitive_asset) = node_asset
                .primitives
                .iter()
                .find(|primitive| primitive.scene_primitive_index == scene_primitive_index)
            else {
                continue;
            };
            let Some(flow_world) = predicted_world_water_flow(
                primitive_asset,
                material.rotate_texture,
                &node_world_transform,
            ) else {
                continue;
            };

            samples.push(WaterFlowSample {
                row,
                col,
                tile_id: tile.id,
                orientation: tile.orientation,
                model: model_name.to_string(),
                texture: texture_name.to_string(),
                node_name: scene_node.name.clone(),
                node_kind: format!("{:?}", scene_node.kind),
                rotate_texture: material.rotate_texture,
                flow_world,
            });
        }
    }
}

fn model_node_world_transform(model: &NwnModelAsset, node_index: usize) -> Transform {
    let mut chain = Vec::new();
    let mut current = Some(node_index);
    while let Some(index) = current {
        chain.push(index);
        current = model.nodes.get(index).and_then(|node| node.parent);
    }
    chain.reverse();

    let mut transform = Transform::IDENTITY;
    for index in chain {
        if let Some(node) = model.nodes.get(index) {
            transform = transform.mul_transform(node.transform);
        }
    }
    transform
}

fn predicted_world_water_flow(
    primitive: &nwnrs_bevy::NwnPrimitiveAsset,
    rotate_texture: i32,
    node_world_transform: &Transform,
) -> Option<Vec2> {
    let txi = primitive.txi.as_ref()?;
    let procedure = txi.procedure.as_ref()?;
    let velocity = match procedure {
        nwnrs_bevy::NwnModelTxiProcedureAsset::Arturo {
            channel_translate, ..
        } if channel_translate.len() >= 4 => Vec2::new(channel_translate[2], channel_translate[3]),
        nwnrs_bevy::NwnModelTxiProcedureAsset::Arturo {
            channel_translate, ..
        } if channel_translate.len() >= 2 => Vec2::new(channel_translate[0], channel_translate[1]),
        _ => Vec2::ZERO,
    };
    if velocity.length_squared() <= f32::EPSILON {
        return None;
    }

    let _ = primitive.txi_uv_to_local_horizontal?;
    let _ = world_from_local_horizontal(node_world_transform)?;
    let canonical_uv_to_world_horizontal = Mat2::from_cols(Vec2::X, -Vec2::Y);
    let _ = rotate_texture;
    let flow_world = canonical_uv_to_world_horizontal * velocity;
    Some(flow_world.normalize())
}

fn world_from_local_horizontal(transform: &Transform) -> Option<Affine2> {
    let world_x = transform.rotation * (Vec3::X * transform.scale.x);
    let world_z = transform.rotation * (Vec3::Z * transform.scale.z);
    let basis = Mat2::from_cols(
        Vec2::new(world_x.x, world_x.z),
        Vec2::new(world_z.x, world_z.z),
    );
    if basis.determinant().abs() <= f32::EPSILON {
        return None;
    }

    Some(Affine2::from_mat2_translation(
        basis,
        Vec2::new(transform.translation.x, transform.translation.z),
    ))
}

fn log_water_flow_samples(area: &TestArea, samples: &[WaterFlowSample]) {
    if samples.is_empty() {
        return;
    }

    let mut distinct_flows = BTreeMap::<(i32, i32), usize>::new();
    for sample in samples {
        let key = (
            (sample.flow_world.x * 1000.0).round() as i32,
            (sample.flow_world.y * 1000.0).round() as i32,
        );
        *distinct_flows.entry(key).or_default() += 1;
    }
    let (baseline_key, baseline_count) = distinct_flows
        .iter()
        .max_by_key(|(_key, count)| **count)
        .map(|(key, count)| (*key, *count))
        .unwrap_or(((0, 0), 0));
    let baseline = Vec2::new(
        baseline_key.0 as f32 / 1000.0,
        baseline_key.1 as f32 / 1000.0,
    );
    let mismatches = samples
        .iter()
        .filter(|sample| {
            (
                (sample.flow_world.x * 1000.0).round() as i32,
                (sample.flow_world.y * 1000.0).round() as i32,
            ) != baseline_key
        })
        .count();

    info!(
        area = area.resref.as_str(),
        sample_count = samples.len(),
        baseline_flow = format_vec2(baseline),
        baseline_count,
        distinct_flow_count = distinct_flows.len(),
        mismatches,
        "resolved water flow samples"
    );

    for sample in samples {
        debug!(
            area = area.resref.as_str(),
            row = sample.row,
            col = sample.col,
            tile_id = sample.tile_id,
            orientation = sample.orientation,
            model = sample.model.as_str(),
            texture = sample.texture.as_str(),
            node = sample.node_name.as_str(),
            kind = sample.node_kind.as_str(),
            rotate_texture = sample.rotate_texture,
            flow_world = format_vec2(sample.flow_world),
            "water tile flow sample"
        );
    }

    if mismatches > 0 {
        let distinct_summary = distinct_flows
            .iter()
            .map(|((x, y), count)| {
                format!(
                    "{:.3},{:.3} x{}",
                    *x as f32 / 1000.0,
                    *y as f32 / 1000.0,
                    count
                )
            })
            .take(8)
            .collect::<Vec<_>>();
        let mismatch_examples = samples
            .iter()
            .filter(|sample| {
                (
                    (sample.flow_world.x * 1000.0).round() as i32,
                    (sample.flow_world.y * 1000.0).round() as i32,
                ) != baseline_key
            })
            .take(8)
            .map(|sample| {
                format!(
                    "r{}c{} tile={} orient={} {} {} {} rotate_texture={} flow={}",
                    sample.row,
                    sample.col,
                    sample.tile_id,
                    sample.orientation,
                    sample.model,
                    sample.texture,
                    sample.node_name,
                    sample.rotate_texture,
                    format_vec2(sample.flow_world)
                )
            })
            .collect::<Vec<_>>();
        warn!(
            area = area.resref.as_str(),
            sample_count = samples.len(),
            baseline_flow = format_vec2(baseline),
            baseline_count,
            distinct_flows = distinct_summary.join(" | "),
            mismatch_examples = mismatch_examples.join(" | "),
            mismatches,
            "water tiles resolved to multiple world flow directions"
        );
    }
}

fn format_vec2(value: Vec2) -> String {
    format!("{:.4},{:.4}", value.x, value.y)
}

fn parse_tile(value: &GffStruct) -> Result<TestAreaTile, String> {
    let id = gff_u32(value.get_field("Tile_ID").map(|field| field.value()))
        .ok_or_else(|| "tile missing Tile_ID".to_string())?;
    let orientation = gff_u32(
        value
            .get_field("Tile_Orientation")
            .map(|field| field.value()),
    )
    .ok_or_else(|| "tile missing Tile_Orientation".to_string())?;
    let height = gff_i32(value.get_field("Tile_Height").map(|field| field.value())).unwrap_or(0);
    Ok(TestAreaTile {
        id,
        orientation,
        height,
    })
}

fn gff_u32(value: Option<&GffValue>) -> Option<u32> {
    match value? {
        GffValue::Byte(value) => Some(u32::from(*value)),
        GffValue::Word(value) => Some(u32::from(*value)),
        GffValue::Dword(value) => Some(*value),
        GffValue::Int(value) => u32::try_from(*value).ok(),
        _ => None,
    }
}

fn gff_u32_any(value: &GffStruct, fields: &[&str]) -> Option<u32> {
    fields
        .iter()
        .find_map(|field| gff_u32(value.get_field(field).map(|entry| entry.value())))
}

fn gff_i32(value: Option<&GffValue>) -> Option<i32> {
    match value? {
        GffValue::Byte(value) => Some(i32::from(*value)),
        GffValue::Word(value) => Some(i32::from(*value)),
        GffValue::Dword(value) => i32::try_from(*value).ok(),
        GffValue::Int(value) => Some(*value),
        _ => None,
    }
}

fn gff_string(value: Option<&GffValue>) -> Option<String> {
    match value? {
        GffValue::CExoString(value) => Some(value.clone()),
        GffValue::ResRef(value) => Some(value.clone()),
        _ => None,
    }
}

fn gff_string_any(value: &GffStruct, fields: &[&str]) -> Option<String> {
    fields
        .iter()
        .find_map(|field| gff_string(value.get_field(field).map(|entry| entry.value())))
}

fn gff_name(value: Option<&GffValue>) -> Option<String> {
    match value? {
        GffValue::CExoString(value) => Some(value.clone()),
        GffValue::ResRef(value) => Some(value.clone()),
        GffValue::CExoLocString(value) => loc_string_name(value),
        _ => None,
    }
}

fn loc_string_name(value: &GffCExoLocString) -> Option<String> {
    value.entries.first().map(|(_language, text)| text.clone())
}

fn gff_list(value: &GffValue) -> Option<&Vec<GffStruct>> {
    match value {
        GffValue::List(value) => Some(value),
        _ => None,
    }
}

fn area_tile_count(width: u32, height: u32) -> Result<usize, String> {
    let width =
        usize::try_from(width).map_err(|error| format!("area width out of range: {error}"))?;
    let height =
        usize::try_from(height).map_err(|error| format!("area height out of range: {error}"))?;
    width
        .checked_mul(height)
        .ok_or_else(|| format!("area tile count overflows usize: {width} x {height}"))
}

fn area_spatial_components(
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

fn apply_area_lighting(
    commands: &mut Commands<'_, '_>,
    camera_entity: Entity,
    _directional_light_entity: Entity,
    area: &TestArea,
    clear_color: &mut ClearColor,
    global_ambient: &mut GlobalAmbientLight,
    directional_light: &mut DirectionalLight,
    directional_light_transform: &mut Transform,
) {
    let (active_label, active_set) = select_active_area_light_set(&area.lighting);
    let ambient_color = active_set
        .ambient_color
        .or(active_set.diffuse_color)
        .unwrap_or(Color::srgb(0.18, 0.18, 0.2));
    let diffuse_color = active_set
        .diffuse_color
        .or(active_set.ambient_color)
        .unwrap_or(Color::srgb(0.8, 0.82, 0.88));
    let fog_color = active_set
        .fog_color
        .or(active_set.ambient_color)
        .unwrap_or(Color::srgb(0.08, 0.09, 0.11));
    let fog_amount = active_set.fog_amount.unwrap_or(0);
    let max_extent = area.width.max(area.height) as f32 * BASE_TILE_SPACING;

    *clear_color = ClearColor(fog_color);
    *global_ambient = GlobalAmbientLight {
        color: ambient_color,
        brightness: area_ambient_brightness(ambient_color, active_label),
        affects_lightmapped_meshes: true,
    };

    directional_light.color = diffuse_color;
    directional_light.illuminance = area_directional_illuminance(diffuse_color, active_label);
    directional_light.shadows_enabled = active_set.shadows.unwrap_or_else(|| fog_amount < 12);
    *directional_light_transform = area_directional_light_transform(active_label);

    if fog_amount > 0 {
        commands.entity(camera_entity).insert(DistanceFog {
            color: fog_color.with_alpha(area_fog_alpha(fog_amount)),
            directional_light_color: diffuse_color.with_alpha(0.4),
            directional_light_exponent: if active_label == "moon" { 24.0 } else { 10.0 },
            falloff: FogFalloff::from_visibility(area_fog_visibility(max_extent, fog_amount)),
        });
    } else {
        commands.entity(camera_entity).remove::<DistanceFog>();
    }

    info!(
        area = area.resref.as_str(),
        lighting_scheme = area.lighting.lighting_scheme,
        shadow_opacity = area.lighting.shadow_opacity,
        day_night_cycle = area.lighting.day_night_cycle,
        active_light = active_label,
        fog_amount,
        "applied area lighting"
    );
}

fn select_active_area_light_set(lighting: &AreaLighting) -> (&'static str, &AreaLightSet) {
    let sun_active = lighting.sun.has_data();
    let moon_active = lighting.moon.has_data();

    if lighting.day_night_cycle {
        if sun_active {
            return ("sun", &lighting.sun);
        }
        if moon_active {
            return ("moon", &lighting.moon);
        }
    }

    match (sun_active, moon_active) {
        (true, false) => ("sun", &lighting.sun),
        (false, true) => ("moon", &lighting.moon),
        (true, true) => {
            if lighting.lighting_scheme.unwrap_or_default() == 0 {
                ("sun", &lighting.sun)
            } else {
                ("moon", &lighting.moon)
            }
        }
        (false, false) => ("sun", &lighting.sun),
    }
}

impl AreaLightSet {
    fn has_data(&self) -> bool {
        self.ambient_color.is_some()
            || self.diffuse_color.is_some()
            || self.fog_color.is_some()
            || self.fog_amount.is_some_and(|value| value > 0)
            || self.shadows.is_some_and(|value| value)
    }
}

fn area_ambient_brightness(color: Color, active_label: &str) -> f32 {
    let intensity = color.to_linear().to_f32_array()[..3]
        .iter()
        .copied()
        .fold(0.0_f32, f32::max);
    let base = if active_label == "moon" { 22.0 } else { 45.0 };
    base + intensity * if active_label == "moon" { 70.0 } else { 110.0 }
}

fn area_directional_illuminance(color: Color, active_label: &str) -> f32 {
    let intensity = color.to_linear().to_f32_array()[..3]
        .iter()
        .copied()
        .fold(0.0_f32, f32::max);
    let base = if active_label == "moon" {
        450.0
    } else {
        12_000.0
    };
    let range = if active_label == "moon" {
        2_500.0
    } else {
        28_000.0
    };
    base + intensity * range
}

fn area_directional_light_transform(active_label: &str) -> Transform {
    let (pitch, yaw) = if active_label == "moon" {
        (-0.82, -0.55)
    } else {
        (-1.05, 0.65)
    };
    Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, pitch, yaw, 0.0))
}

fn area_fog_alpha(fog_amount: u8) -> f32 {
    (f32::from(fog_amount) / 15.0).clamp(0.18, 0.92)
}

fn area_fog_visibility(max_extent: f32, fog_amount: u8) -> f32 {
    let amount = (f32::from(fog_amount) / 15.0).clamp(0.0, 1.0);
    let min_visibility = (max_extent * 0.35).max(30.0);
    let max_visibility = (max_extent * 1.6).max(90.0);
    max_visibility - (max_visibility - min_visibility) * amount
}

fn gff_u8(value: Option<&GffValue>) -> Option<u8> {
    match value? {
        GffValue::Byte(value) => Some(*value),
        GffValue::Word(value) => u8::try_from(*value).ok(),
        GffValue::Dword(value) => u8::try_from(*value).ok(),
        GffValue::Int(value) => u8::try_from(*value).ok(),
        _ => None,
    }
}

fn gff_color(value: Option<&GffValue>) -> Option<Color> {
    let packed = gff_u32(value)?;
    (packed != 0).then(|| aurora_bgr_color_to_bevy(packed))
}

fn aurora_bgr_color_to_bevy(packed: u32) -> Color {
    let red = (packed & 0xff) as u8;
    let green = ((packed >> 8) & 0xff) as u8;
    let blue = ((packed >> 16) & 0xff) as u8;
    Color::srgb_u8(red, green, blue)
}

fn color_for_tile(tile_id: u32) -> Color {
    let hue = (tile_id.wrapping_mul(37) % 360) as f32;
    Color::hsl(hue, 0.45, 0.52)
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

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::path::Path;

    use bevy::{asset::Handle, prelude::Transform};
    use nwnrs_bevy::{
        NwnAppearanceOverrides, NwnModelAsset, NwnModelNodeAsset, NwnModelReferenceAsset,
        NwnPrimitiveAsset,
    };
    use nwnrs_gff::prelude::{GffValue, new_gff_struct};
    use nwnrs_mdl::prelude::{NodeKind, NwnCoordinateSystem, NwnScene};
    use nwnrs_twoda::prelude::TwoDa;

    use super::{
        CreaturePartAttachment, armor_part_field_aliases, attach_model_reference,
        combine_default_model_candidates, combine_door_model_candidates,
        creature_body_part_field_aliases, default_player_phenotype_digit,
        infer_unambiguous_model_variant, is_module_archive_path, is_player_appearance_token,
        item_plt_appearance_overrides, merged_appearance_overrides, module_label,
        player_creature_model_prefix, strip_replaced_player_creature_primitives,
        tile_supports_grass, twoda_row_index_for_appearance, twoda_row_index_for_text,
    };

    #[test]
    fn module_archive_detection_accepts_mod_and_nwm() {
        let root = std::env::temp_dir().join("nwnrs-test-area-module-detection");
        std::fs::create_dir_all(&root).unwrap_or_else(|error| {
            panic!("create temp root: {error}");
        });
        let mod_file = root.join("alpha.MOD");
        let nwm_file = root.join("chapter.nwm");
        let txt_file = root.join("notes.txt");
        std::fs::write(&mod_file, []).unwrap_or_else(|error| {
            panic!("write .mod file: {error}");
        });
        std::fs::write(&nwm_file, []).unwrap_or_else(|error| {
            panic!("write .nwm file: {error}");
        });
        std::fs::write(&txt_file, []).unwrap_or_else(|error| {
            panic!("write .txt file: {error}");
        });

        assert!(is_module_archive_path(&mod_file));
        assert!(is_module_archive_path(&nwm_file));
        assert!(!is_module_archive_path(&txt_file));
    }

    #[test]
    fn module_label_includes_parent_directory() {
        let path = Path::new("/tmp/modules/story.mod");
        assert_eq!(module_label(path), "story.mod [modules]");
    }

    #[test]
    fn appearance_lookup_prefers_matching_row_label_over_storage_index() {
        let mut table = TwoDa::new();
        table
            .set_columns(vec!["ModelName".to_string()])
            .unwrap_or_else(|error| panic!("set columns: {error}"));
        table
            .replace_rows(
                vec![
                    vec![Some("door_a".to_string())],
                    vec![Some("door_b".to_string())],
                    vec![Some("door_c".to_string())],
                ],
                vec!["1".to_string(), "2".to_string(), "3".to_string()],
            )
            .unwrap_or_else(|error| panic!("replace rows: {error}"));

        assert_eq!(twoda_row_index_for_appearance(&table, 2), Some(1));
    }

    #[test]
    fn appearance_lookup_falls_back_to_storage_index_when_row_labels_do_not_match() {
        let mut table = TwoDa::new();
        table
            .set_columns(vec!["ModelName".to_string()])
            .unwrap_or_else(|error| panic!("set columns: {error}"));
        table
            .replace_rows(
                vec![
                    vec![Some("door_a".to_string())],
                    vec![Some("door_b".to_string())],
                ],
                vec!["alpha".to_string(), "beta".to_string()],
            )
            .unwrap_or_else(|error| panic!("replace rows: {error}"));

        assert_eq!(twoda_row_index_for_appearance(&table, 1), Some(1));
        assert_eq!(twoda_row_index_for_appearance(&table, 2), None);
    }

    #[test]
    fn text_lookup_matches_template_resref_case_insensitively() {
        let mut table = TwoDa::new();
        table
            .set_columns(vec!["TemplateResRef".to_string(), "Model".to_string()])
            .unwrap_or_else(|error| panic!("set columns: {error}"));
        table
            .replace_rows(
                vec![
                    vec![
                        Some("nw_door_ttr_02".to_string()),
                        Some("TTR_UDoor_02".to_string()),
                    ],
                    vec![
                        Some("nw_door_ttr_03".to_string()),
                        Some("TTR_UDoor_03".to_string()),
                    ],
                ],
                vec!["2".to_string(), "3".to_string()],
            )
            .unwrap_or_else(|error| panic!("replace rows: {error}"));

        assert_eq!(
            twoda_row_index_for_text(&table, "TemplateResRef", "NW_DOOR_TTR_03"),
            Some(1)
        );
    }

    #[test]
    fn model_variant_fallback_uses_only_unambiguous_sibling() {
        let available = vec!["tcm02_b92_01".to_string(), "tcm02_b93_01".to_string()];
        assert_eq!(
            infer_unambiguous_model_variant("tcm02_b92_02", &available),
            Some("tcm02_b92_01".to_string())
        );
    }

    #[test]
    fn model_variant_fallback_rejects_ambiguous_siblings() {
        let available = vec!["tcm02_b92_01".to_string(), "tcm02_b92_03".to_string()];
        assert_eq!(
            infer_unambiguous_model_variant("tcm02_b92_02", &available),
            None
        );
    }

    #[test]
    fn door_candidates_prefer_instance_doortype_before_blueprint_fallbacks() {
        let candidates = combine_door_model_candidates(
            Some("instance_doortype".to_string()),
            vec![
                "blueprint_doortype".to_string(),
                "blueprint_generic".to_string(),
                "blueprint_model".to_string(),
            ],
        );
        assert_eq!(
            candidates,
            vec![
                "instance_doortype".to_string(),
                "blueprint_doortype".to_string(),
                "blueprint_generic".to_string(),
                "blueprint_model".to_string(),
            ]
        );
    }

    #[test]
    fn placeable_candidates_still_prefer_instance_appearance() {
        let candidates = combine_default_model_candidates(
            Some("instance_placeable".to_string()),
            vec!["blueprint_model".to_string()],
        );
        assert_eq!(
            candidates,
            vec![
                "instance_placeable".to_string(),
                "blueprint_model".to_string(),
            ]
        );
    }

    #[test]
    fn player_appearance_tokens_are_not_treated_as_static_model_names() {
        assert!(is_player_appearance_token("H"));
        assert!(is_player_appearance_token("DW"));
        assert!(!is_player_appearance_token("c_orc"));
    }

    #[test]
    fn player_creature_model_prefix_uses_gender_race_and_phenotype_digit() {
        assert_eq!(player_creature_model_prefix('m', 'H', 0), "pmh0");
        assert_eq!(player_creature_model_prefix('f', 'A', 2), "pfa2");
    }

    #[test]
    fn phenotype_rows_use_default_part_family_digit() {
        let mut table = TwoDa::new();
        table
            .set_columns(vec!["DefaultPhenoType".to_string()])
            .unwrap_or_else(|error| panic!("set columns: {error}"));
        table
            .replace_rows(
                vec![
                    vec![Some("0".to_string())],
                    vec![Some("0".to_string())],
                    vec![Some("2".to_string())],
                    vec![Some("0".to_string())],
                    vec![Some("0".to_string())],
                    vec![Some("2".to_string())],
                ],
                vec![
                    "0".to_string(),
                    "1".to_string(),
                    "2".to_string(),
                    "3".to_string(),
                    "4".to_string(),
                    "5".to_string(),
                ],
            )
            .unwrap_or_else(|error| panic!("replace rows: {error}"));

        assert_eq!(default_player_phenotype_digit(&table, 0), 0);
        assert_eq!(default_player_phenotype_digit(&table, 1), 0);
        assert_eq!(default_player_phenotype_digit(&table, 2), 2);
        assert_eq!(default_player_phenotype_digit(&table, 5), 2);
    }

    #[test]
    fn creature_body_part_aliases_cover_canonical_toolset_field_names() {
        assert_eq!(
            creature_body_part_field_aliases("FORER"),
            &[
                "BodyPart_RForeArm",
                "BodyPart_RForearm",
                "BodyPart_ForeArm_R",
                "BodyPart_Forearm_R",
            ]
        );
        assert_eq!(
            creature_body_part_field_aliases("CHEST"),
            &["BodyPart_Torso", "BodyPart_Chest"]
        );
    }

    #[test]
    fn armor_part_aliases_cover_toolset_armor_fields() {
        assert_eq!(
            armor_part_field_aliases("FORER"),
            &["ArmorPart_RFArm", "ArmorPart_RForearm"]
        );
        assert_eq!(
            armor_part_field_aliases("SHOR"),
            &["ArmorPart_RShoul", "ArmorPart_RShoulder"]
        );
    }

    #[test]
    fn item_plt_overrides_read_equipment_palette_rows() {
        let mut item = new_gff_struct(0);
        item.put_value("Metal1Color", GffValue::Byte(49))
            .unwrap_or_else(|error| panic!("insert Metal1Color: {error}"));
        item.put_value("Leather2Color", GffValue::Byte(23))
            .unwrap_or_else(|error| panic!("insert Leather2Color: {error}"));

        let overrides = item_plt_appearance_overrides(&item);
        assert_eq!(overrides.plt_rows.get(&2), Some(&49));
        assert_eq!(overrides.plt_rows.get(&7), Some(&23));
    }

    #[test]
    fn merged_appearance_overrides_prefers_item_palette_rows() {
        let base = NwnAppearanceOverrides {
            slots:    Default::default(),
            plt_rows: [(0, 12), (2, 10)].into_iter().collect(),
        };
        let extra = NwnAppearanceOverrides {
            slots:    Default::default(),
            plt_rows: [(2, 49), (7, 23)].into_iter().collect(),
        };

        let merged = merged_appearance_overrides(&base, &extra);
        assert_eq!(merged.plt_rows.get(&0), Some(&12));
        assert_eq!(merged.plt_rows.get(&2), Some(&49));
        assert_eq!(merged.plt_rows.get(&7), Some(&23));
    }

    #[test]
    fn attach_model_reference_matches_nodes_case_insensitively() {
        let child = NwnModelAsset {
            scene:      NwnScene {
                name:              "part".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             Vec::new(),
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      Vec::new(),
            root_nodes: Vec::new(),
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };
        let mut model = NwnModelAsset {
            scene:      NwnScene {
                name:              "base".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             Vec::new(),
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      vec![NwnModelNodeAsset {
                name:           "Head_G".to_string(),
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

        assert!(attach_model_reference(
            &mut model,
            "head_g",
            NwnModelReferenceAsset {
                model_name: "pmh0_head001".to_string(),
                model:      Box::new(child),
            }
        ));
        assert_eq!(
            model.nodes[0]
                .references
                .first()
                .map(|reference| reference.model_name.as_str()),
            Some("pmh0_head001")
        );

        assert!(attach_model_reference(
            &mut model,
            "HEAD_G",
            NwnModelReferenceAsset {
                model_name: "helm_001".to_string(),
                model:      Box::new(NwnModelAsset {
                    scene:      NwnScene {
                        name:              "helm".to_string(),
                        supermodel:        None,
                        classification:    None,
                        animation_scale:   None,
                        coordinate_system: NwnCoordinateSystem::AuroraSource,
                        nodes:             Vec::new(),
                        meshes:            Vec::new(),
                        materials:         Vec::new(),
                        animations:        Vec::new(),
                        diagnostics:       Vec::new(),
                    },
                    nodes:      Vec::new(),
                    root_nodes: Vec::new(),
                    materials:  Vec::new(),
                    meshes:     Vec::new(),
                    textures:   Vec::new(),
                    unresolved: Vec::new(),
                }),
            }
        ));
        assert_eq!(model.nodes[0].references.len(), 2);
        assert_eq!(model.nodes[0].references[1].model_name.as_str(), "helm_001");
    }

    #[test]
    fn strip_replaced_player_creature_primitives_preserves_head_accessories() {
        let primitive = || NwnPrimitiveAsset {
            label: "primitive".to_string(),
            scene_primitive_index: 0,
            txi: None,
            txi_uv_to_local_horizontal: None,
            mesh: Handle::default(),
            material: Handle::default(),
            tilefade: None,
            initially_visible: true,
            shadow_enabled: true,
        };
        let mut model = NwnModelAsset {
            scene:      NwnScene {
                name:              "base".to_string(),
                supermodel:        None,
                classification:    None,
                animation_scale:   None,
                coordinate_system: NwnCoordinateSystem::AuroraSource,
                nodes:             Vec::new(),
                meshes:            Vec::new(),
                materials:         Vec::new(),
                animations:        Vec::new(),
                diagnostics:       Vec::new(),
            },
            nodes:      vec![
                NwnModelNodeAsset {
                    name:           "torso_g".to_string(),
                    kind:           NodeKind::Dummy,
                    parent:         None,
                    transform:      Transform::default(),
                    light:          None,
                    references:     Vec::new(),
                    helper_surface: None,
                    primitives:     vec![primitive()],
                },
                NwnModelNodeAsset {
                    name:           "head".to_string(),
                    kind:           NodeKind::Dummy,
                    parent:         None,
                    transform:      Transform::default(),
                    light:          None,
                    references:     Vec::new(),
                    helper_surface: None,
                    primitives:     vec![primitive()],
                },
            ],
            root_nodes: vec![0, 1],
            materials:  Vec::new(),
            meshes:     Vec::new(),
            textures:   Vec::new(),
            unresolved: Vec::new(),
        };

        strip_replaced_player_creature_primitives(
            &mut model,
            &[
                CreaturePartAttachment {
                    node_name:            "torso_g".to_string(),
                    model_name:           "pmh0_chest025".to_string(),
                    appearance_overrides: NwnAppearanceOverrides::default(),
                },
                CreaturePartAttachment {
                    node_name:            "head".to_string(),
                    model_name:           "helm_001".to_string(),
                    appearance_overrides: NwnAppearanceOverrides::default(),
                },
            ],
        );

        assert!(model.nodes[0].primitives.is_empty());
        assert_eq!(model.nodes[1].primitives.len(), 1);
    }

    #[test]
    fn tile_grass_requires_uniform_supported_terrain() {
        let mut tileset = nwnrs_set::prelude::SetFile::default();
        tileset.general.default_terrain = Some("GRASS".to_string());

        let supported_tile = nwnrs_set::prelude::SetTile {
            top_left: nwnrs_set::prelude::SetTileCorner {
                terrain: Some("grass".to_string()),
                ..Default::default()
            },
            top_right: nwnrs_set::prelude::SetTileCorner {
                terrain: Some("GRASS".to_string()),
                ..Default::default()
            },
            bottom_left: nwnrs_set::prelude::SetTileCorner {
                terrain: Some("Grass".to_string()),
                ..Default::default()
            },
            bottom_right: nwnrs_set::prelude::SetTileCorner {
                terrain: Some("grass".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(tile_supports_grass(&tileset, Some(&supported_tile)));

        let mixed_tile = nwnrs_set::prelude::SetTile {
            top_left: nwnrs_set::prelude::SetTileCorner {
                terrain: Some("grass".to_string()),
                ..Default::default()
            },
            top_right: nwnrs_set::prelude::SetTileCorner {
                terrain: Some("stone".to_string()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(!tile_supports_grass(&tileset, Some(&mixed_tile)));
    }
}
