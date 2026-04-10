#![forbid(unsafe_code)]
//! Typed parser for Neverwinter Nights tileset (`SET`) payloads.
//!
//! `SET` files are INI-like text resources that describe a tileset's catalog
//! of tile models together with terrain metadata, edge crosser tags, grouped
//! layouts, and optional door marker data.

use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::ResolvedResRef;
use nwnrs_restype::prelude::*;
use tracing::instrument;

/// NWN resource type id for `set`.
pub const SET_RES_TYPE: ResType = ResType(2013);

/// Errors returned while reading or parsing `SET` payloads.
#[derive(Debug)]
pub enum SetError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl SetError {
    /// Creates a free-form `SET` error message.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for SetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for SetError {}

impl From<io::Error> for SetError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for SetError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for `SET` operations.
pub type SetResult<T> = Result<T, SetError>;

/// Parsed tileset payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetFile {
    /// Top-level `[GENERAL]` metadata.
    pub general:       SetGeneral,
    /// Optional `[GRASS]` block.
    pub grass:         Option<SetGrass>,
    /// `[TERRAINN]` entries keyed by terrain id.
    pub terrains:      BTreeMap<u32, SetNamedType>,
    /// `[CROSSERN]` entries keyed by crosser id.
    pub crossers:      BTreeMap<u32, SetNamedType>,
    /// `[PRIMARY RULEN]` entries keyed by rule id.
    pub primary_rules: BTreeMap<u32, SetPrimaryRule>,
    /// `[TILEN]` entries keyed by tile id.
    pub tiles:         BTreeMap<u32, SetTile>,
    /// `[TILENMDOORK]` entries keyed by `(tile_id, door_id)`.
    pub tile_doors:    BTreeMap<(u32, u32), SetTileDoor>,
    /// `[GROUPN]` entries keyed by group id.
    pub groups:        BTreeMap<u32, SetGroup>,
}

/// Parsed `[GENERAL]` section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetGeneral {
    /// Internal tileset name.
    pub name:                  Option<String>,
    /// Declared resource type, usually `SET`.
    pub file_type:             Option<String>,
    /// Declared version string.
    pub version:               Option<String>,
    /// Whether the tileset is interior.
    pub interior:              Option<bool>,
    /// Whether height transitions are enabled.
    pub has_height_transition: Option<bool>,
    /// Environment map name.
    pub env_map:               Option<String>,
    /// Transition type id.
    pub transition:            Option<i32>,
    /// Selector height hint.
    pub selector_height:       Option<i32>,
    /// Dialog.tlk string reference for the localized display name.
    pub display_name:          Option<i32>,
    /// Fallback unlocalized display name.
    pub unlocalized_name:      Option<String>,
    /// Default border terrain tag.
    pub border:                Option<String>,
    /// Default terrain tag.
    pub default_terrain:       Option<String>,
    /// Default floor terrain tag.
    pub floor:                 Option<String>,
}

/// Parsed `[GRASS]` section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetGrass {
    /// Whether grass rendering is enabled.
    pub grass:        Option<bool>,
    /// Grass texture resource name.
    pub texture_name: Option<String>,
    /// Grass density value.
    pub density:      Option<f32>,
    /// Grass height value.
    pub height:       Option<f32>,
    /// Ambient grass color.
    pub ambient:      Option<[f32; 3]>,
    /// Diffuse grass color.
    pub diffuse:      Option<[f32; 3]>,
}

/// Named tileset catalog entry such as `[TERRAIN0]` or `[CROSSER0]`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetNamedType {
    /// Entry id from the section suffix.
    pub id:      u32,
    /// Display or symbolic name.
    pub name:    Option<String>,
    /// Optional dialog.tlk string reference.
    pub str_ref: Option<i32>,
}

/// One terrain corner annotation on a tile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetTileCorner {
    /// Terrain tag for this corner.
    pub terrain: Option<String>,
    /// Height step at this corner.
    pub height:  Option<i32>,
}

/// One set of edge crosser tags on a tile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetTileEdges {
    /// Crosser tag on the top edge.
    pub top:    Option<String>,
    /// Crosser tag on the right edge.
    pub right:  Option<String>,
    /// Crosser tag on the bottom edge.
    pub bottom: Option<String>,
    /// Crosser tag on the left edge.
    pub left:   Option<String>,
}

/// Parsed `[TILEN]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetTile {
    /// Tile id from the section suffix.
    pub id: u32,
    /// MDL resource name.
    pub model: Option<String>,
    /// Walkmesh identifier.
    pub walkmesh: Option<String>,
    /// Top-left terrain annotation.
    pub top_left: SetTileCorner,
    /// Top-right terrain annotation.
    pub top_right: SetTileCorner,
    /// Bottom-left terrain annotation.
    pub bottom_left: SetTileCorner,
    /// Bottom-right terrain annotation.
    pub bottom_right: SetTileCorner,
    /// Edge crosser tags.
    pub edge_crossers: SetTileEdges,
    /// First main-light flag.
    pub main_light_1: Option<bool>,
    /// Second main-light flag.
    pub main_light_2: Option<bool>,
    /// First source-light flag.
    pub source_light_1: Option<bool>,
    /// Second source-light flag.
    pub source_light_2: Option<bool>,
    /// First animation-loop flag.
    pub anim_loop_1: Option<bool>,
    /// Second animation-loop flag.
    pub anim_loop_2: Option<bool>,
    /// Third animation-loop flag.
    pub anim_loop_3: Option<bool>,
    /// Door count declared on the tile.
    pub doors: Option<u32>,
    /// Sound count declared on the tile.
    pub sounds: Option<u32>,
    /// Path node marker.
    pub path_node: Option<String>,
    /// Path node orientation.
    pub orientation: Option<i32>,
    /// Visibility node marker.
    pub visibility_node: Option<String>,
    /// Visibility node orientation.
    pub visibility_orientation: Option<i32>,
    /// Optional door visibility node marker.
    pub door_visibility_node: Option<String>,
    /// Optional door visibility node orientation.
    pub door_visibility_orientation: Option<i32>,
    /// 2D selector image name.
    pub image_map_2d: Option<String>,
}

/// Parsed `[TILENDOORK]` section.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SetTileDoor {
    /// Tile id from the section prefix.
    pub tile_id:     u32,
    /// Door id from the section suffix.
    pub door_id:     u32,
    /// Door type identifier.
    pub door_type:   Option<i32>,
    /// Door marker X coordinate.
    pub x:           Option<f32>,
    /// Door marker Y coordinate.
    pub y:           Option<f32>,
    /// Door marker Z coordinate.
    pub z:           Option<f32>,
    /// Door marker orientation.
    pub orientation: Option<i32>,
}

/// Parsed `[GROUPN]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetGroup {
    /// Group id from the section suffix.
    pub id:      u32,
    /// Group display name.
    pub name:    Option<String>,
    /// Optional dialog.tlk string reference.
    pub str_ref: Option<i32>,
    /// Group row count.
    pub rows:    Option<u32>,
    /// Group column count.
    pub columns: Option<u32>,
    /// Group tile layout keyed by zero-based cell index.
    pub tiles:   BTreeMap<u32, Option<u32>>,
}

/// Parsed `[PRIMARY RULEN]` section.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetPrimaryRule {
    /// Rule id from the section suffix.
    pub id:              u32,
    /// Terrain tag for the placed tile.
    pub placed:          Option<String>,
    /// Height for the placed terrain.
    pub placed_height:   Option<i32>,
    /// Terrain tag for the adjacent tile.
    pub adjacent:        Option<String>,
    /// Height for the adjacent terrain.
    pub adjacent_height: Option<i32>,
    /// Terrain tag after applying the rule.
    pub changed:         Option<String>,
    /// Height after applying the rule.
    pub changed_height:  Option<i32>,
}

/// Reads a typed `SET` file from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_set<R: Read>(reader: &mut R) -> SetResult<SetFile> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    parse_set(&text)
}

/// Reads a typed `SET` file from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_set_from_file(path: impl AsRef<Path>) -> SetResult<SetFile> {
    let mut file = File::open(path.as_ref())?;
    read_set(&mut file)
}

/// Reads a typed `SET` file from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_set_from_res(res: &Res, use_cache: bool) -> SetResult<SetFile> {
    if res.resref().res_type() != SET_RES_TYPE {
        return Err(SetError::msg(format!(
            "expected set resource, got {}",
            res.resref()
        )));
    }

    let bytes = res.read_all(use_cache)?;
    let text = String::from_utf8(bytes)
        .map_err(|error| SetError::msg(format!("SET payload is not valid UTF-8: {error}")))?;
    parse_set(&text)
}

/// Reads a typed `SET` file from a [`ResMan`] by tileset name.
#[instrument(level = "debug", skip_all, err, fields(set_name, use_cache))]
pub fn read_set_from_resman(
    resman: &mut ResMan,
    set_name: &str,
    use_cache: bool,
) -> SetResult<SetFile> {
    let resolved = ResolvedResRef::from_filename(&format!("{set_name}.set"))
        .map_err(|error| SetError::msg(format!("set resref: {error}")))?;
    let res = resman
        .get_resolved(&resolved)
        .ok_or_else(|| SetError::msg(format!("tileset not found in ResMan: {resolved}")))?;
    read_set_from_res(&res, use_cache)
}

/// Parses a typed `SET` file from text.
pub fn parse_set(text: &str) -> SetResult<SetFile> {
    let mut builder = SetFile::default();
    let mut current_section = String::new();
    let mut current_entries = BTreeMap::new();

    for raw_line in text.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with("//") {
            continue;
        }

        if line.starts_with('[') && line.ends_with(']') {
            if !current_section.is_empty() {
                apply_section(&mut builder, &current_section, &current_entries)?;
                current_entries.clear();
            }
            current_section = line[1..line.len() - 1].trim().to_string();
            continue;
        }

        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        current_entries.insert(key.trim().to_ascii_lowercase(), value.trim().to_string());
    }

    if !current_section.is_empty() {
        apply_section(&mut builder, &current_section, &current_entries)?;
    }

    if builder.tiles.is_empty() {
        return Err(SetError::msg(
            "tileset file contained no tile definitions".to_string(),
        ));
    }

    Ok(builder)
}

fn apply_section(
    set_file: &mut SetFile,
    section_name: &str,
    entries: &BTreeMap<String, String>,
) -> SetResult<()> {
    let section_upper = section_name.to_ascii_uppercase();

    match section_upper.as_str() {
        "GENERAL" => set_file.general = parse_general(entries),
        "GRASS" => set_file.grass = Some(parse_grass(entries)),
        "TERRAIN TYPES" | "CROSSER TYPES" | "PRIMARY RULES" | "SECONDARY RULES" | "TILES"
        | "GROUPS" => {}
        _ => {
            if let Some(index) = parse_indexed_section(&section_upper, "TERRAIN") {
                set_file
                    .terrains
                    .insert(index, parse_named_type(index, entries));
            } else if let Some(index) = parse_indexed_section(&section_upper, "CROSSER") {
                set_file
                    .crossers
                    .insert(index, parse_named_type(index, entries));
            } else if let Some(index) = parse_indexed_section(&section_upper, "GROUP") {
                set_file.groups.insert(index, parse_group(index, entries));
            } else if let Some(index) = parse_indexed_section(&section_upper, "PRIMARY RULE") {
                set_file
                    .primary_rules
                    .insert(index, parse_primary_rule(index, entries));
            } else if let Some((tile_id, door_id)) = parse_tile_door_section(&section_upper) {
                set_file.tile_doors.insert(
                    (tile_id, door_id),
                    parse_tile_door(tile_id, door_id, entries),
                );
            } else if let Some(index) = parse_indexed_section(&section_upper, "TILE") {
                set_file.tiles.insert(index, parse_tile(index, entries));
            }
        }
    }

    Ok(())
}

fn parse_general(entries: &BTreeMap<String, String>) -> SetGeneral {
    SetGeneral {
        name:                  read_text(entries, "name"),
        file_type:             read_text(entries, "type"),
        version:               read_text(entries, "version"),
        interior:              read_bool(entries, "interior"),
        has_height_transition: read_bool(entries, "hasheighttransition"),
        env_map:               read_text(entries, "envmap"),
        transition:            read_i32(entries, "transition"),
        selector_height:       read_i32(entries, "selectorheight"),
        display_name:          read_i32(entries, "displayname"),
        unlocalized_name:      read_text(entries, "unlocalizedname"),
        border:                read_text(entries, "border"),
        default_terrain:       read_text(entries, "default"),
        floor:                 read_text(entries, "floor"),
    }
}

fn parse_grass(entries: &BTreeMap<String, String>) -> SetGrass {
    SetGrass {
        grass:        read_bool(entries, "grass"),
        texture_name: read_text(entries, "grasstexturename"),
        density:      read_f32(entries, "density"),
        height:       read_f32(entries, "height"),
        ambient:      parse_rgb(entries, "ambientred", "ambientgreen", "ambientblue"),
        diffuse:      parse_rgb(entries, "diffusered", "diffusegreen", "diffuseblue"),
    }
}

fn parse_named_type(id: u32, entries: &BTreeMap<String, String>) -> SetNamedType {
    SetNamedType {
        id,
        name: read_text(entries, "name"),
        str_ref: read_i32(entries, "strref"),
    }
}

fn parse_group(id: u32, entries: &BTreeMap<String, String>) -> SetGroup {
    let mut tiles = BTreeMap::new();
    for (key, value) in entries {
        if let Some(index) = key
            .strip_prefix("tile")
            .and_then(|suffix| suffix.parse::<u32>().ok())
        {
            tiles.insert(
                index,
                value
                    .parse::<i32>()
                    .ok()
                    .and_then(|raw| u32::try_from(raw).ok()),
            );
        }
    }

    SetGroup {
        id,
        name: read_text(entries, "name"),
        str_ref: read_i32(entries, "strref"),
        rows: read_u32(entries, "rows"),
        columns: read_u32(entries, "columns"),
        tiles,
    }
}

fn parse_primary_rule(id: u32, entries: &BTreeMap<String, String>) -> SetPrimaryRule {
    SetPrimaryRule {
        id,
        placed: read_text(entries, "placed"),
        placed_height: read_i32(entries, "placedheight"),
        adjacent: read_text(entries, "adjacent"),
        adjacent_height: read_i32(entries, "adjacentheight"),
        changed: read_text(entries, "changed"),
        changed_height: read_i32(entries, "changedheight"),
    }
}

fn parse_tile(id: u32, entries: &BTreeMap<String, String>) -> SetTile {
    SetTile {
        id,
        model: read_text(entries, "model"),
        walkmesh: read_text(entries, "walkmesh"),
        top_left: parse_tile_corner(entries, "topleft", "topleftheight"),
        top_right: parse_tile_corner(entries, "topright", "toprightheight"),
        bottom_left: parse_tile_corner(entries, "bottomleft", "bottomleftheight"),
        bottom_right: parse_tile_corner(entries, "bottomright", "bottomrightheight"),
        edge_crossers: SetTileEdges {
            top:    read_text(entries, "top"),
            right:  read_text(entries, "right"),
            bottom: read_text(entries, "bottom"),
            left:   read_text(entries, "left"),
        },
        main_light_1: read_bool(entries, "mainlight1"),
        main_light_2: read_bool(entries, "mainlight2"),
        source_light_1: read_bool(entries, "sourcelight1"),
        source_light_2: read_bool(entries, "sourcelight2"),
        anim_loop_1: read_bool(entries, "animloop1"),
        anim_loop_2: read_bool(entries, "animloop2"),
        anim_loop_3: read_bool(entries, "animloop3"),
        doors: read_u32(entries, "doors"),
        sounds: read_u32(entries, "sounds"),
        path_node: read_text(entries, "pathnode"),
        orientation: read_i32(entries, "orientation"),
        visibility_node: read_text(entries, "visibilitynode"),
        visibility_orientation: read_i32(entries, "visibilityorientation"),
        door_visibility_node: read_text(entries, "doorvisibilitynode"),
        door_visibility_orientation: read_i32(entries, "doorvisibilityorientation"),
        image_map_2d: read_text(entries, "imagemap2d"),
    }
}

fn parse_tile_door(tile_id: u32, door_id: u32, entries: &BTreeMap<String, String>) -> SetTileDoor {
    SetTileDoor {
        tile_id,
        door_id,
        door_type: read_i32(entries, "type"),
        x: read_f32(entries, "x"),
        y: read_f32(entries, "y"),
        z: read_f32(entries, "z"),
        orientation: read_i32(entries, "orientation"),
    }
}

fn parse_tile_corner(
    entries: &BTreeMap<String, String>,
    terrain_key: &str,
    height_key: &str,
) -> SetTileCorner {
    SetTileCorner {
        terrain: read_text(entries, terrain_key)
            .filter(|value| !value.eq_ignore_ascii_case("invalid")),
        height:  read_i32(entries, height_key),
    }
}

fn parse_rgb(
    entries: &BTreeMap<String, String>,
    red_key: &str,
    green_key: &str,
    blue_key: &str,
) -> Option<[f32; 3]> {
    Some([
        read_f32(entries, red_key)?,
        read_f32(entries, green_key)?,
        read_f32(entries, blue_key)?,
    ])
}

fn parse_indexed_section(section_name: &str, prefix: &str) -> Option<u32> {
    let suffix = section_name.strip_prefix(prefix)?;
    if suffix.is_empty() {
        return None;
    }
    suffix.parse::<u32>().ok()
}

fn parse_tile_door_section(section_name: &str) -> Option<(u32, u32)> {
    let (tile_part, door_part) = section_name.split_once("DOOR")?;
    let tile_id = tile_part.strip_prefix("TILE")?.parse::<u32>().ok()?;
    let door_id = door_part.parse::<u32>().ok()?;
    Some((tile_id, door_id))
}

fn read_text(entries: &BTreeMap<String, String>, key: &str) -> Option<String> {
    let value = entries.get(key)?.trim().trim_matches('"');
    if value.is_empty() || value == "****" {
        return None;
    }
    Some(value.to_string())
}

fn read_bool(entries: &BTreeMap<String, String>, key: &str) -> Option<bool> {
    let value = entries.get(key)?.trim();
    match value {
        "1" => Some(true),
        "0" => Some(false),
        _ if value.eq_ignore_ascii_case("true") => Some(true),
        _ if value.eq_ignore_ascii_case("false") => Some(false),
        _ => None,
    }
}

fn read_u32(entries: &BTreeMap<String, String>, key: &str) -> Option<u32> {
    entries.get(key)?.trim().parse::<u32>().ok()
}

fn read_i32(entries: &BTreeMap<String, String>, key: &str) -> Option<i32> {
    entries.get(key)?.trim().parse::<i32>().ok()
}

fn read_f32(entries: &BTreeMap<String, String>, key: &str) -> Option<f32> {
    entries.get(key)?.trim().parse::<f32>().ok()
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        SET_RES_TYPE, SetError, SetFile, SetGeneral, SetGrass, SetGroup, SetNamedType,
        SetPrimaryRule, SetResult, SetTile, SetTileCorner, SetTileDoor, SetTileEdges, parse_set,
        read_set, read_set_from_file, read_set_from_res, read_set_from_resman,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{parse_set, read_set_from_file};

    #[test]
    fn parses_minimal_tileset() {
        let parsed = parse_set(
            r#"
                [GENERAL]
                Name=TST01
                Type=SET
                Version=V1.0
                Interior=0

                [TERRAIN TYPES]
                Count=1

                [TERRAIN0]
                Name=Grass
                StrRef=42

                [TILES]
                Count=1

                [TILE0]
                Model=tst01_a01_01
                WalkMesh=msb01
                TopLeft=Grass
                TopLeftHeight=0
                TopRight=Grass
                TopRightHeight=0
                BottomLeft=Grass
                BottomLeftHeight=0
                BottomRight=Grass
                BottomRightHeight=0
                PathNode=A
                Orientation=90
            "#,
        )
        .unwrap_or_else(|error| panic!("parse set: {error}"));

        assert_eq!(parsed.general.name.as_deref(), Some("TST01"));
        assert_eq!(
            parsed
                .terrains
                .get(&0)
                .and_then(|terrain| terrain.name.as_deref()),
            Some("Grass")
        );
        assert_eq!(
            parsed.tiles.get(&0).and_then(|tile| tile.model.as_deref()),
            Some("tst01_a01_01")
        );
        assert_eq!(
            parsed.tiles.get(&0).and_then(|tile| tile.orientation),
            Some(90)
        );
    }

    #[test]
    fn parses_workspace_set_samples() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../set");
        let entries = fs::read_dir(&root).unwrap_or_else(|error| {
            panic!("read set sample dir {}: {error}", root.display());
        });

        let mut parsed_files = 0_usize;
        for entry in entries {
            let entry = entry.unwrap_or_else(|error| panic!("read dir entry: {error}"));
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("set") {
                continue;
            }

            let parsed = read_set_from_file(&path).unwrap_or_else(|error| {
                panic!("parse {}: {error}", path.display());
            });
            assert!(
                !parsed.tiles.is_empty(),
                "expected at least one tile in {}",
                path.display()
            );
            parsed_files += 1;
        }

        assert!(parsed_files > 0, "expected at least one sample .set file");
    }
}
