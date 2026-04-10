//! Area viewer for the checked-in `assets/testing/test.mod` fixture.

use std::{collections::BTreeMap, f32::consts::FRAC_PI_2, io::Cursor, path::Path};

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    mesh::Mesh3d,
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::*,
};
use nwnrs_bevy::{
    NwnBevyPlugin, NwnInstall, NwnInstallPlugin, NwnModelAsset, load_nwn_model_from_resman,
    spawn_nwn_model,
};
use nwnrs_erf::prelude::read_erf_from_file;
use nwnrs_gff::prelude::{GffCExoLocString, GffStruct, GffValue, read_gff_root};
use nwnrs_resref::prelude::ResolvedResRef;
use tracing::{info, warn};

const TEST_MOD_PATH: &str = "assets/testing/test.mod";
const TILE_SIZE: f32 = 10.0;
const TILE_THICKNESS: f32 = 0.2;
const TILE_HEIGHT_STEP: f32 = 1.5;

#[derive(Component)]
struct FlyCam {
    move_speed: f32,
    boost_multiplier: f32,
    mouse_sensitivity: Vec2,
}

#[derive(Resource, Default)]
struct AreaSceneState {
    initialized: bool,
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
    App::new()
        .add_plugins((DefaultPlugins, NwnBevyPlugin, NwnInstallPlugin::default()))
        .init_resource::<AreaSceneState>()
        .add_systems(Startup, setup)
        .add_systems(Update, (initialize_area_scene, update_flycam))
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

fn initialize_area_scene(
    mut commands: Commands<'_, '_>,
    install: Option<Res<'_, NwnInstall>>,
    mut state: ResMut<'_, AreaSceneState>,
    mut images: ResMut<'_, Assets<Image>>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
    mut camera_transform: Single<'_, '_, &mut Transform, With<FlyCam>>,
) {
    if state.initialized {
        return;
    }

    let Some(install) = install else {
        return;
    };

    let area = match load_test_area(Path::new(TEST_MOD_PATH)) {
        Ok(area) => area,
        Err(error) => {
            warn!("failed to load test area: {error}");
            state.initialized = true;
            return;
        }
    };

    info!(
        name = area.name.as_str(),
        resref = area.resref.as_str(),
        tileset = area.tileset.as_str(),
        width = area.width,
        height = area.height,
        tile_count = area.tiles.len(),
        "loaded test area"
    );

    let tileset = {
        let mut resman = match install.resman.lock() {
            Ok(resman) => resman,
            Err(error) => error.into_inner(),
        };

        match load_tileset_definition(&mut resman, &area.tileset) {
            Ok(tileset) => tileset,
            Err(error) => {
                warn!(
                    tileset = area.tileset.as_str(),
                    "failed to load tileset definition: {error}"
                );
                state.initialized = true;
                return;
            }
        }
    };
    log_area_layout(&area, &tileset);

    let area_extent_x = area.width as f32 * TILE_SIZE;
    let area_extent_y = area.height as f32 * TILE_SIZE;
    let max_extent = area_extent_x.max(area_extent_y);
    let camera_distance = max_extent.max(40.0) * 0.9;
    let camera_height = (area.height.max(area.width) as f32 * TILE_HEIGHT_STEP) + max_extent * 0.55;
    let x_origin = -((area.width as f32 - 1.0) * TILE_SIZE * 0.5);
    let y_origin = -((area.height as f32 - 1.0) * TILE_SIZE * 0.5);

    **camera_transform = Transform::from_xyz(area_extent_x * 0.25, -camera_distance, camera_height)
        .looking_at(Vec3::new(0.0, 0.0, 0.0), Vec3::Z);

    let mut model_cache = BTreeMap::<String, NwnModelAsset>::new();
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
                    &mut commands,
                    &mut meshes,
                    &mut materials,
                    tile_entity,
                    tile,
                    format!("tile {} is missing from {}.set", tile.id, area.tileset),
                );
                continue;
            };
            info!(
                row,
                col,
                tile_id = tile.id,
                orientation = tile.orientation,
                height = tile.height,
                model = model_name.as_str(),
                "placing area tile"
            );

            let model = if let Some(model) = model_cache.get(&model_name).cloned() {
                model
            } else {
                match load_nwn_model_from_resman(
                    &mut resman,
                    &model_name,
                    &mut images,
                    &mut meshes,
                    &mut materials,
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
                        model_cache.insert(model_name.clone(), model.clone());
                        model
                    }
                    Err(error) => {
                        spawn_tile_fallback(
                            &mut commands,
                            &mut meshes,
                            &mut materials,
                            tile_entity,
                            tile,
                            format!("failed to load {model_name}.mdl: {error}"),
                        );
                        continue;
                    }
                }
            };

            let model_root = spawn_nwn_model(&mut commands, &model);
            commands.entity(model_root).insert(Transform::default());
            commands.entity(tile_entity).add_child(model_root);
        }
    }

    state.initialized = true;
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
        info!(row, layout = cells.join(" | "), "expected area row");
    }
}

fn log_model_diagnostics(model_name: &str, model: &NwnModelAsset) {
    let primitive_count = model
        .nodes
        .iter()
        .map(|node| node.primitives.len())
        .sum::<usize>();
    info!(
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
        info!(
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

fn load_test_area(path: &Path) -> Result<TestArea, String> {
    let archive = read_erf_from_file(path).map_err(|error| format!("read mod: {error}"))?;
    let area_entry = archive
        .entries()
        .iter()
        .find(|(resref, _res)| {
            resref
                .resolve()
                .is_some_and(|resolved| resolved.res_ext() == "are")
        })
        .map(|(_resref, res)| res.clone())
        .ok_or_else(|| "no .are entry found in test module".to_string())?;
    let bytes = area_entry
        .read_all(true)
        .map_err(|error| format!("read area resource: {error}"))?;
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
