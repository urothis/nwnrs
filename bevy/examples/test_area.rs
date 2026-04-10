//! Area viewer for local NWN modules with an in-app module and area selector.

use std::{
    collections::{BTreeMap, BTreeSet},
    f32::consts::FRAC_PI_2,
    io::Cursor,
    path::{Path, PathBuf},
    sync::Arc,
};

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    mesh::Mesh3d,
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::*,
};
use bevy_egui::{EguiContexts, EguiPlugin, EguiPrimaryContextPass, egui};
use nwnrs_bevy::{
    NwnBevyPlugin, NwnInstall, NwnInstallPlugin, NwnModelAsset, load_nwn_model_from_resman,
    spawn_nwn_model,
};
use nwnrs_erf::prelude::{Erf, read_erf_from_file};
use nwnrs_gff::prelude::{GffCExoLocString, GffStruct, GffValue, read_gff_root};
use nwnrs_resman::prelude::ResContainer;
use nwnrs_resref::prelude::ResolvedResRef;
use tracing::{debug, info, warn};

const DEFAULT_MODULE_PATH: &str = "assets/testing/test.mod";
const TILE_SIZE: f32 = 10.0;
const TILE_THICKNESS: f32 = 0.2;
const TILE_HEIGHT_STEP: f32 = 1.5;
const MODULE_SELECTOR_LIMIT: usize = 200;

#[derive(Component)]
struct FlyCam {
    move_speed: f32,
    boost_multiplier: f32,
    mouse_sensitivity: Vec2,
}

#[derive(Resource, Default)]
struct AreaViewerCatalog {
    modules: Vec<ModuleChoice>,
    module_index: usize,
    module_query: String,
    roots: Vec<PathBuf>,
    extra_search_path: Option<PathBuf>,
    needs_refresh: bool,
}

#[derive(Resource, Default)]
struct AreaViewerState {
    areas: Vec<AreaChoice>,
    area_index: usize,
    scene_root: Option<Entity>,
    active_module_path: Option<PathBuf>,
    active_module_archive: Option<Arc<Erf>>,
    active_module_container: Option<Arc<dyn ResContainer>>,
    needs_area_list_refresh: bool,
    needs_scene_reload: bool,
    status_message: String,
}

#[derive(Resource, Default)]
struct AreaRenderCache {
    models: BTreeMap<(PathBuf, String), NwnModelAsset>,
}

#[derive(Debug, Clone)]
struct ModuleChoice {
    path: PathBuf,
    label: String,
}

#[derive(Debug, Clone)]
struct AreaChoice {
    resref: String,
    label: String,
}

#[derive(Debug, Clone)]
struct TestArea {
    name: String,
    resref: String,
    tileset: String,
    width: u32,
    height: u32,
    tiles: Vec<TestAreaTile>,
}

#[derive(Debug, Clone, Copy)]
struct TestAreaTile {
    id: u32,
    orientation: u32,
    height: i32,
}

#[derive(Debug, Default)]
struct TilesetDefinition {
    tile_models: BTreeMap<u32, String>,
}

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
        Transform::from_xyz(10.0, -36.0, 24.0).looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Z),
        FlyCam {
            move_speed: 24.0,
            boost_multiplier: 3.0,
            mouse_sensitivity: Vec2::new(0.003, 0.002),
        },
    ));

    commands.spawn((
        DirectionalLight {
            illuminance: 35_000.0,
            shadows_enabled: true,
            ..Default::default()
        },
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.05, 0.65, 0.0)),
    ));
    commands.spawn((
        PointLight {
            intensity: 350_000.0,
            range: 120.0,
            shadows_enabled: false,
            ..Default::default()
        },
        Transform::from_xyz(0.0, 0.0, 28.0),
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
            default_module_path().and_then(|path| modules.iter().position(|module| module.path == path))
        })
        .unwrap_or(0);

    catalog.module_index = selected_index;
    catalog.modules = modules;
    state.area_index = previous_area
        .and_then(|area_resref| state.areas.iter().position(|area| area.resref == area_resref))
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
            let previous_resref = state.areas.get(state.area_index).map(|area| area.resref.clone());
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
    mut images: ResMut<'_, Assets<Image>>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
    mut camera_transform: Single<'_, '_, &mut Transform, With<FlyCam>>,
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

    let scene_root = match spawn_area_scene(
        &mut commands,
        &install,
        &module.path,
        &area,
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
        "loaded selected area"
    );

    state.scene_root = Some(scene_root);
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
                    state.status_message = format!(
                        "Inspecting {}",
                        catalog.modules[catalog.module_index].label
                    );
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
                ui.small("Tip: pass a directory or .mod/.nwm path after `--example test_area --` to add another search root.");
            }
        });

    Ok(())
}

fn filtered_module_entries(catalog: &AreaViewerCatalog, query: &str) -> Vec<(usize, String)> {
    if catalog.modules.is_empty() {
        return Vec::new();
    }

    if query.is_empty() {
        let start = catalog.module_index.saturating_sub(MODULE_SELECTOR_LIMIT / 2);
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

fn load_area_from_archive(archive: &Erf, requested_resref: Option<&str>) -> Result<TestArea, String> {
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

    let width = gff_u32(root.root.get_field("Width").map(|field| field.value()))
        .ok_or_else(|| "ARE missing Width".to_string())?;
    let height = gff_u32(root.root.get_field("Height").map(|field| field.value()))
        .ok_or_else(|| "ARE missing Height".to_string())?;
    let tileset = gff_string(root.root.get_field("Tileset").map(|field| field.value()))
        .unwrap_or_else(|| "unknown".to_string());
    let resref = gff_string(root.root.get_field("ResRef").map(|field| field.value()))
        .unwrap_or_else(|| "area".to_string());
    let name = gff_name(root.root.get_field("Name").map(|field| field.value()))
        .unwrap_or_else(|| resref.clone());

    let tiles = root
        .root
        .get_field("Tile_List")
        .map(|field| field.value())
        .and_then(gff_list)
        .ok_or_else(|| "ARE missing Tile_List".to_string())?
        .iter()
        .map(parse_tile)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(TestArea {
        name,
        resref,
        tileset,
        width,
        height,
        tiles,
    })
}

fn spawn_area_scene(
    commands: &mut Commands<'_, '_>,
    install: &NwnInstall,
    module_path: &Path,
    area: &TestArea,
    render_cache: &mut AreaRenderCache,
    images: &mut Assets<Image>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    camera_transform: &mut Transform,
) -> Result<Entity, String> {
    let tileset = {
        let mut resman = match install.resman.lock() {
            Ok(resman) => resman,
            Err(error) => error.into_inner(),
        };
        load_tileset_definition(&mut resman, &area.tileset)?
    };
    log_area_layout(area, &tileset);

    let area_extent_x = area.width as f32 * TILE_SIZE;
    let area_extent_y = area.height as f32 * TILE_SIZE;
    let max_extent = area_extent_x.max(area_extent_y);
    let camera_distance = max_extent.max(40.0) * 0.9;
    let camera_height = (area.height.max(area.width) as f32 * TILE_HEIGHT_STEP) + max_extent * 0.55;
    let x_origin = -((area.width as f32 - 1.0) * TILE_SIZE * 0.5);
    let y_origin = -((area.height as f32 - 1.0) * TILE_SIZE * 0.5);

    *camera_transform =
        Transform::from_xyz(area_extent_x * 0.25, -camera_distance, camera_height)
            .looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Z);

    let mut resman = match install.resman.lock() {
        Ok(resman) => resman,
        Err(error) => error.into_inner(),
    };

    let area_root = commands
        .spawn(Name::new(format!("area_{}", area.resref)))
        .id();
    let mut tile_index = 0_usize;
    for row in 0..area.height {
        for col in 0..area.width {
            let Some(tile) = area.tiles.get(tile_index).copied() else {
                break;
            };
            tile_index += 1;

            let translation = Vec3::new(
                x_origin + col as f32 * TILE_SIZE,
                y_origin + row as f32 * TILE_SIZE,
                tile.height as f32 * TILE_HEIGHT_STEP,
            );
            let orientation_angle = tile.orientation as f32 * FRAC_PI_2;
            let tile_name = format!(
                "tile_{row}_{col}_id{}_rot{}_h{}",
                tile.id, tile.orientation, tile.height
            );
            let tile_entity = commands
                .spawn((
                    Name::new(tile_name),
                    Transform::from_translation(translation)
                        .with_rotation(Quat::from_rotation_z(orientation_angle)),
                ))
                .id();
            commands.entity(area_root).add_child(tile_entity);

            let maybe_model_name = tileset.tile_models.get(&tile.id).cloned();
            let Some(model_name) = maybe_model_name else {
                spawn_tile_fallback(
                    commands,
                    meshes,
                    materials,
                    tile_entity,
                    tile,
                    format!("tile {} is missing from {}.set", tile.id, area.tileset),
                );
                continue;
            };
            debug!(
                row,
                col,
                tile_id = tile.id,
                orientation = tile.orientation,
                height = tile.height,
                model = model_name.as_str(),
                "placing area tile"
            );

            let cache_key = (module_path.to_path_buf(), model_name.clone());
            let model = if let Some(model) = render_cache.models.get(&cache_key).cloned() {
                model
            } else {
                match load_nwn_model_from_resman(
                    &mut resman,
                    &model_name,
                    images,
                    meshes,
                    materials,
                ) {
                    Ok(model) => {
                        log_model_diagnostics(&model_name, &model);
                        if !model.unresolved.is_empty() {
                            warn!(
                                model = model_name.as_str(),
                                unresolved = model.unresolved.len(),
                                "loaded tile model with unresolved textures"
                            );
                        }
                        render_cache.models.insert(cache_key, model.clone());
                        model
                    }
                    Err(error) => {
                        spawn_tile_fallback(
                            commands,
                            meshes,
                            materials,
                            tile_entity,
                            tile,
                            format!("failed to load {model_name}.mdl: {error}"),
                        );
                        continue;
                    }
                }
            };

            let model_root = spawn_nwn_model(commands, &model);
            commands.entity(model_root).insert(Transform::default());
            commands.entity(tile_entity).add_child(model_root);
        }
    }

    Ok(area_root)
}

fn spawn_tile_fallback(
    commands: &mut Commands<'_, '_>,
    meshes: &mut Assets<Mesh>,
    materials: &mut Assets<StandardMaterial>,
    tile_entity: Entity,
    tile: TestAreaTile,
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
        TILE_SIZE * 0.96,
        TILE_SIZE * 0.96,
        TILE_THICKNESS,
    ));
    let indicator_mesh = meshes.add(Cuboid::new(TILE_SIZE * 0.18, TILE_SIZE * 0.48, 0.4));
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
            Vec3::Y * (TILE_SIZE * 0.28) + Vec3::new(0.0, 0.0, TILE_THICKNESS * 0.5 + 0.25);
        children.spawn((
            Name::new("fallback_orientation"),
            Mesh3d(indicator_mesh),
            MeshMaterial3d(indicator_material),
            Transform::from_translation(indicator_offset),
        ));
    });
}

fn load_tileset_definition(
    resman: &mut nwnrs_resman::prelude::ResMan,
    tileset_name: &str,
) -> Result<TilesetDefinition, String> {
    let resolved = ResolvedResRef::from_filename(&format!("{tileset_name}.set"))
        .map_err(|error| format!("tileset resref: {error}"))?;
    let res = resman
        .get_resolved(&resolved)
        .ok_or_else(|| format!("tileset not found in ResMan: {resolved}"))?;
    let bytes = res
        .read_all(true)
        .map_err(|error| format!("read tileset: {error}"))?;
    parse_tileset_definition(&bytes)
}

fn log_area_layout(area: &TestArea, tileset: &TilesetDefinition) {
    info!(
        tileset = area.tileset.as_str(),
        mapped_tiles = tileset.tile_models.len(),
        "resolved tileset definition"
    );

    for row in 0..area.height {
        let mut cells = Vec::new();
        for col in 0..area.width {
            let index = (row * area.width + col) as usize;
            let Some(tile) = area.tiles.get(index) else {
                continue;
            };
            let model_name = tileset
                .tile_models
                .get(&tile.id)
                .map(String::as_str)
                .unwrap_or("<missing>");
            cells.push(format!(
                "({row},{col}) id={} rot={} h={} model={model_name}",
                tile.id, tile.orientation, tile.height
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
}

fn parse_tileset_definition(bytes: &[u8]) -> Result<TilesetDefinition, String> {
    let text = String::from_utf8_lossy(bytes);
    let mut current_section = String::new();
    let mut tile_models = BTreeMap::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            current_section = line[1..line.len() - 1].trim().to_ascii_lowercase();
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !current_section.starts_with("tile") {
            continue;
        }

        let tile_id = current_section["tile".len()..].trim().parse::<u32>().ok();
        if !key.trim().eq_ignore_ascii_case("model") {
            continue;
        }

        let Some(tile_id) = tile_id else {
            continue;
        };
        let model_name = value.trim().trim_matches('"');
        if model_name.is_empty() || model_name == "****" {
            continue;
        }
        tile_models.insert(tile_id, model_name.to_string());
    }

    if tile_models.is_empty() {
        return Err("tileset file contained no tile model mappings".to_string());
    }

    Ok(TilesetDefinition { tile_models })
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

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::{is_module_archive_path, module_label};

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
}
