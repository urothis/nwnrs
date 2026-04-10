//! Debug area viewer for the checked-in `assets/testing/test.mod` fixture.

use std::{f32::consts::FRAC_PI_2, io::Cursor, path::Path};

use bevy::{
    input::mouse::AccumulatedMouseMotion,
    mesh::Mesh3d,
    pbr::{MeshMaterial3d, StandardMaterial},
    prelude::*,
};
use nwnrs_erf::prelude::read_erf_from_file;
use nwnrs_gff::prelude::{GffCExoLocString, GffStruct, GffValue, read_gff_root};
use tracing::{info, warn};

const TEST_MOD_PATH: &str = "assets/testing/test.mod";
const TILE_SIZE: f32 = 10.0;
const TILE_THICKNESS: f32 = 0.2;
const TILE_HEIGHT_STEP: f32 = 1.5;

#[derive(Component)]
struct FlyCam {
    move_speed:        f32,
    boost_multiplier:  f32,
    mouse_sensitivity: Vec2,
}

#[derive(Debug, Clone)]
struct TestArea {
    name:    String,
    resref:  String,
    tileset: String,
    width:   u32,
    height:  u32,
    tiles:   Vec<TestAreaTile>,
}

#[derive(Debug, Clone, Copy)]
struct TestAreaTile {
    id:          u32,
    orientation: u32,
    height:      i32,
}

fn main() {
    App::new()
        .add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, update_flycam)
        .run();
}

fn setup(
    mut commands: Commands<'_, '_>,
    mut meshes: ResMut<'_, Assets<Mesh>>,
    mut materials: ResMut<'_, Assets<StandardMaterial>>,
) {
    let area = match load_test_area(Path::new(TEST_MOD_PATH)) {
        Ok(area) => area,
        Err(error) => {
            warn!("failed to load test area: {error}");
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

    let area_extent_x = area.width as f32 * TILE_SIZE;
    let area_extent_y = area.height as f32 * TILE_SIZE;
    let max_extent = area_extent_x.max(area_extent_y);
    let camera_distance = max_extent.max(40.0) * 0.9;
    let camera_height = (area.height.max(area.width) as f32 * TILE_HEIGHT_STEP) + max_extent * 0.55;

    commands.spawn((
        Camera3d::default(),
        Transform::from_xyz(area_extent_x * 0.25, -camera_distance, camera_height).looking_at(
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::Z,
        ),
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
        Transform::from_rotation(Quat::from_euler(EulerRot::XYZ, -1.05, 0.65, 0.0)),
    ));
    commands.spawn((
        PointLight {
            intensity: 350_000.0,
            range: max_extent * 2.0,
            shadows_enabled: false,
            ..Default::default()
        },
        Transform::from_xyz(0.0, 0.0, camera_height * 0.8),
    ));

    let tile_mesh = meshes.add(Cuboid::new(TILE_SIZE * 0.96, TILE_SIZE * 0.96, TILE_THICKNESS));
    let indicator_mesh = meshes.add(Cuboid::new(TILE_SIZE * 0.18, TILE_SIZE * 0.48, 0.4));

    let x_origin = -((area.width as f32 - 1.0) * TILE_SIZE * 0.5);
    let y_origin = -((area.height as f32 - 1.0) * TILE_SIZE * 0.5);
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
            let base_color = color_for_tile(tile.id);
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

            let tile_entity = commands
                .spawn((
                    Name::new(format!(
                        "tile_{row}_{col}_id{}_rot{}_h{}",
                        tile.id, tile.orientation, tile.height
                    )),
                    Mesh3d(tile_mesh.clone()),
                    MeshMaterial3d(tile_material),
                    Transform::from_translation(translation),
                ))
                .id();

            let orientation_angle = tile.orientation as f32 * FRAC_PI_2;
            let indicator_offset = Quat::from_rotation_z(orientation_angle) * Vec3::Y * (TILE_SIZE * 0.28);
            commands.entity(tile_entity).with_children(|children| {
                children.spawn((
                    Name::new("orientation"),
                    Mesh3d(indicator_mesh.clone()),
                    MeshMaterial3d(indicator_material),
                    Transform::from_translation(Vec3::new(
                        indicator_offset.x,
                        indicator_offset.y,
                        TILE_THICKNESS * 0.5 + 0.25,
                    ))
                    .with_rotation(Quat::from_rotation_z(orientation_angle)),
                ));
            });
        }
    }
}

fn load_test_area(path: &Path) -> Result<TestArea, String> {
    let archive = read_erf_from_file(path).map_err(|error| format!("read mod: {error}"))?;
    let area_entry = archive
        .entries()
        .iter()
        .find(|(resref, _res)| resref.resolve().is_some_and(|resolved| resolved.res_ext() == "are"))
        .map(|(_resref, res)| res.clone())
        .ok_or_else(|| "no .are entry found in test module".to_string())?;
    let bytes = area_entry
        .read_all(true)
        .map_err(|error| format!("read area resource: {error}"))?;
    let root = read_gff_root(&mut Cursor::new(bytes)).map_err(|error| format!("read ARE: {error}"))?;

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
    let orientation = gff_u32(value.get_field("Tile_Orientation").map(|field| field.value()))
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
    value
        .entries
        .first()
        .map(|(_language, text)| text.clone())
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
