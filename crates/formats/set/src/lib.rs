#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::{
    collections::BTreeMap,
    fmt,
    fs::File,
    io::{self, Read, Write},
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
///
/// The representation preserves the authored section structure explicitly:
/// top-level metadata, terrain and crosser catalogs, primary rules, tiles,
/// tile-door metadata, and groups remain distinct keyed collections rather than
/// being flattened into one generic map.
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

impl SetFile {
    /// Reads a typed `SET` file from disk.
    pub fn from_file(path: impl AsRef<Path>) -> SetResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_set(&mut file)
    }

    /// Reads a typed `SET` file from a [`Res`].
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> SetResult<Self> {
        if res.resref().res_type() != SET_RES_TYPE {
            return Err(SetError::msg(format!(
                "expected set resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| SetError::msg(format!("SET payload is not valid UTF-8: {error}")))?;
        parse_set(&text)
    }

    /// Reads a typed `SET` file from a [`ResMan`] by tileset name.
    pub fn from_resman(
        resman: &mut ResMan,
        set_name: &str,
        cache_policy: CachePolicy,
    ) -> SetResult<Self> {
        let resolved = ResolvedResRef::from_filename(&format!("{set_name}.set"))
            .map_err(|error| SetError::msg(format!("set resref: {error}")))?;
        let res = resman
            .get_resolved(&resolved)
            .ok_or_else(|| SetError::msg(format!("tileset not found in ResMan: {resolved}")))?;
        Self::from_res(&res, cache_policy)
    }
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
                apply_section(&mut builder, &current_section, &current_entries);
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
        apply_section(&mut builder, &current_section, &current_entries);
    }

    if builder.tiles.is_empty() {
        return Err(SetError::msg(
            "tileset file contained no tile definitions".to_string(),
        ));
    }

    Ok(builder)
}

/// Builds deterministic `SET` text from a typed [`SetFile`].
///
/// The serializer emits the modeled section structure explicitly: general and
/// grass blocks first, followed by synthesized catalog count sections and then
/// the indexed terrain, crosser, rule, tile, tile-door, and group sections in
/// ascending key order.
pub fn build_set_text(set_file: &SetFile) -> SetResult<String> {
    if set_file.tiles.is_empty() {
        return Err(SetError::msg(
            "cannot build SET payload without at least one tile definition",
        ));
    }

    let mut text = String::new();
    write_general(&mut text, &set_file.general);
    if let Some(grass) = &set_file.grass {
        push_blank_line(&mut text);
        write_grass(&mut text, grass);
    }

    push_blank_line(&mut text);
    write_count_section(&mut text, "TERRAIN TYPES", set_file.terrains.len());
    for (id, terrain) in &set_file.terrains {
        push_blank_line(&mut text);
        write_named_type_section(&mut text, &format!("TERRAIN{id}"), terrain);
    }

    push_blank_line(&mut text);
    write_count_section(&mut text, "CROSSER TYPES", set_file.crossers.len());
    for (id, crosser) in &set_file.crossers {
        push_blank_line(&mut text);
        write_named_type_section(&mut text, &format!("CROSSER{id}"), crosser);
    }

    push_blank_line(&mut text);
    write_count_section(&mut text, "PRIMARY RULES", set_file.primary_rules.len());
    for (id, rule) in &set_file.primary_rules {
        push_blank_line(&mut text);
        write_primary_rule_section(&mut text, &format!("PRIMARY RULE{id}"), rule);
    }

    push_blank_line(&mut text);
    write_count_section(&mut text, "TILES", set_file.tiles.len());
    for (id, tile) in &set_file.tiles {
        push_blank_line(&mut text);
        write_tile_section(&mut text, &format!("TILE{id}"), tile);
    }

    for ((tile_id, door_id), door) in &set_file.tile_doors {
        push_blank_line(&mut text);
        write_tile_door_section(&mut text, &format!("TILE{tile_id}DOOR{door_id}"), door);
    }

    push_blank_line(&mut text);
    write_count_section(&mut text, "GROUPS", set_file.groups.len());
    for (id, group) in &set_file.groups {
        push_blank_line(&mut text);
        write_group_section(&mut text, &format!("GROUP{id}"), group);
    }

    Ok(text)
}

/// Writes deterministic `SET` text to `writer`.
pub fn write_set<W: Write>(writer: &mut W, set_file: &SetFile) -> SetResult<()> {
    let text = build_set_text(set_file)?;
    writer.write_all(text.as_bytes())?;
    Ok(())
}

fn apply_section(set_file: &mut SetFile, section_name: &str, entries: &BTreeMap<String, String>) {
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

fn push_blank_line(text: &mut String) {
    if !text.is_empty() && !text.ends_with("\n\n") {
        text.push('\n');
    }
}

fn write_section_header(text: &mut String, name: &str) {
    text.push('[');
    text.push_str(name);
    text.push_str("]\n");
}

fn write_string_value(text: &mut String, key: &str, value: Option<&String>) {
    if let Some(value) = value {
        text.push_str(key);
        text.push('=');
        text.push_str(value);
        text.push('\n');
    }
}

fn write_bool_value(text: &mut String, key: &str, value: Option<bool>) {
    if let Some(value) = value {
        text.push_str(key);
        text.push('=');
        text.push_str(if value { "1" } else { "0" });
        text.push('\n');
    }
}

fn write_u32_value(text: &mut String, key: &str, value: Option<u32>) {
    if let Some(value) = value {
        text.push_str(key);
        text.push('=');
        text.push_str(&value.to_string());
        text.push('\n');
    }
}

fn write_i32_value(text: &mut String, key: &str, value: Option<i32>) {
    if let Some(value) = value {
        text.push_str(key);
        text.push('=');
        text.push_str(&value.to_string());
        text.push('\n');
    }
}

fn write_f32_value(text: &mut String, key: &str, value: Option<f32>) {
    if let Some(value) = value {
        text.push_str(key);
        text.push('=');
        text.push_str(&value.to_string());
        text.push('\n');
    }
}

fn write_count_section(text: &mut String, name: &str, count: usize) {
    write_section_header(text, name);
    text.push_str("Count=");
    text.push_str(&count.to_string());
    text.push('\n');
}

fn write_general(text: &mut String, general: &SetGeneral) {
    write_section_header(text, "GENERAL");
    write_string_value(text, "Name", general.name.as_ref());
    write_string_value(text, "Type", general.file_type.as_ref());
    write_string_value(text, "Version", general.version.as_ref());
    write_bool_value(text, "Interior", general.interior);
    write_bool_value(text, "HasHeightTransition", general.has_height_transition);
    write_string_value(text, "EnvMap", general.env_map.as_ref());
    write_i32_value(text, "Transition", general.transition);
    write_i32_value(text, "SelectorHeight", general.selector_height);
    write_i32_value(text, "DisplayName", general.display_name);
    write_string_value(text, "UnlocalizedName", general.unlocalized_name.as_ref());
    write_string_value(text, "Border", general.border.as_ref());
    write_string_value(text, "Default", general.default_terrain.as_ref());
    write_string_value(text, "Floor", general.floor.as_ref());
}

fn write_grass(text: &mut String, grass: &SetGrass) {
    write_section_header(text, "GRASS");
    write_bool_value(text, "Grass", grass.grass);
    write_string_value(text, "GrassTextureName", grass.texture_name.as_ref());
    write_f32_value(text, "Density", grass.density);
    write_f32_value(text, "Height", grass.height);
    if let Some([red, green, blue]) = grass.ambient {
        write_f32_value(text, "AmbientRed", Some(red));
        write_f32_value(text, "AmbientGreen", Some(green));
        write_f32_value(text, "AmbientBlue", Some(blue));
    }
    if let Some([red, green, blue]) = grass.diffuse {
        write_f32_value(text, "DiffuseRed", Some(red));
        write_f32_value(text, "DiffuseGreen", Some(green));
        write_f32_value(text, "DiffuseBlue", Some(blue));
    }
}

fn write_named_type_section(text: &mut String, section_name: &str, named_type: &SetNamedType) {
    write_section_header(text, section_name);
    write_string_value(text, "Name", named_type.name.as_ref());
    write_i32_value(text, "StrRef", named_type.str_ref);
}

fn write_primary_rule_section(
    text: &mut String,
    section_name: &str,
    primary_rule: &SetPrimaryRule,
) {
    write_section_header(text, section_name);
    write_string_value(text, "Placed", primary_rule.placed.as_ref());
    write_i32_value(text, "PlacedHeight", primary_rule.placed_height);
    write_string_value(text, "Adjacent", primary_rule.adjacent.as_ref());
    write_i32_value(text, "AdjacentHeight", primary_rule.adjacent_height);
    write_string_value(text, "Changed", primary_rule.changed.as_ref());
    write_i32_value(text, "ChangedHeight", primary_rule.changed_height);
}

fn write_tile_section(text: &mut String, section_name: &str, tile: &SetTile) {
    write_section_header(text, section_name);
    write_string_value(text, "Model", tile.model.as_ref());
    write_string_value(text, "WalkMesh", tile.walkmesh.as_ref());
    write_string_value(text, "TopLeft", tile.top_left.terrain.as_ref());
    write_i32_value(text, "TopLeftHeight", tile.top_left.height);
    write_string_value(text, "TopRight", tile.top_right.terrain.as_ref());
    write_i32_value(text, "TopRightHeight", tile.top_right.height);
    write_string_value(text, "BottomLeft", tile.bottom_left.terrain.as_ref());
    write_i32_value(text, "BottomLeftHeight", tile.bottom_left.height);
    write_string_value(text, "BottomRight", tile.bottom_right.terrain.as_ref());
    write_i32_value(text, "BottomRightHeight", tile.bottom_right.height);
    write_string_value(text, "Top", tile.edge_crossers.top.as_ref());
    write_string_value(text, "Right", tile.edge_crossers.right.as_ref());
    write_string_value(text, "Bottom", tile.edge_crossers.bottom.as_ref());
    write_string_value(text, "Left", tile.edge_crossers.left.as_ref());
    write_bool_value(text, "MainLight1", tile.main_light_1);
    write_bool_value(text, "MainLight2", tile.main_light_2);
    write_bool_value(text, "SourceLight1", tile.source_light_1);
    write_bool_value(text, "SourceLight2", tile.source_light_2);
    write_bool_value(text, "AnimLoop1", tile.anim_loop_1);
    write_bool_value(text, "AnimLoop2", tile.anim_loop_2);
    write_bool_value(text, "AnimLoop3", tile.anim_loop_3);
    write_u32_value(text, "Doors", tile.doors);
    write_u32_value(text, "Sounds", tile.sounds);
    write_string_value(text, "PathNode", tile.path_node.as_ref());
    write_i32_value(text, "Orientation", tile.orientation);
    write_string_value(text, "VisibilityNode", tile.visibility_node.as_ref());
    write_i32_value(text, "VisibilityOrientation", tile.visibility_orientation);
    write_string_value(
        text,
        "DoorVisibilityNode",
        tile.door_visibility_node.as_ref(),
    );
    write_i32_value(
        text,
        "DoorVisibilityOrientation",
        tile.door_visibility_orientation,
    );
    write_string_value(text, "ImageMap2D", tile.image_map_2d.as_ref());
}

fn write_tile_door_section(text: &mut String, section_name: &str, tile_door: &SetTileDoor) {
    write_section_header(text, section_name);
    write_i32_value(text, "Type", tile_door.door_type);
    write_f32_value(text, "X", tile_door.x);
    write_f32_value(text, "Y", tile_door.y);
    write_f32_value(text, "Z", tile_door.z);
    write_i32_value(text, "Orientation", tile_door.orientation);
}

fn write_group_section(text: &mut String, section_name: &str, group: &SetGroup) {
    write_section_header(text, section_name);
    write_string_value(text, "Name", group.name.as_ref());
    write_i32_value(text, "StrRef", group.str_ref);
    write_u32_value(text, "Rows", group.rows);
    write_u32_value(text, "Columns", group.columns);
    for (index, tile_id) in &group.tiles {
        text.push_str("Tile");
        text.push_str(&index.to_string());
        text.push('=');
        match tile_id {
            Some(tile_id) => text.push_str(&tile_id.to_string()),
            None => text.push_str("-1"),
        }
        text.push('\n');
    }
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        SET_RES_TYPE, SetError, SetFile, SetGeneral, SetGrass, SetGroup, SetNamedType,
        SetPrimaryRule, SetResult, SetTile, SetTileCorner, SetTileDoor, SetTileEdges,
        build_set_text, parse_set, read_set, write_set,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use super::{SetFile, build_set_text, parse_set, write_set};

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
        if !root.is_dir() {
            return;
        }
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

            let parsed = SetFile::from_file(&path).unwrap_or_else(|error| {
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

    #[test]
    fn builds_and_reparses_structured_tileset() {
        let original = parse_set(
            r#"
                [GENERAL]
                Name=TST01
                Type=SET
                Version=V1.0
                Interior=0
                HasHeightTransition=1

                [GRASS]
                Grass=1
                GrassTextureName=grass01
                Density=1.5
                Height=2
                AmbientRed=0.1
                AmbientGreen=0.2
                AmbientBlue=0.3
                DiffuseRed=0.4
                DiffuseGreen=0.5
                DiffuseBlue=0.6

                [TERRAIN TYPES]
                Count=1

                [TERRAIN0]
                Name=Grass
                StrRef=42

                [CROSSER TYPES]
                Count=1

                [CROSSER0]
                Name=Road

                [PRIMARY RULES]
                Count=1

                [PRIMARY RULE0]
                Placed=Grass
                PlacedHeight=0
                Adjacent=Road
                AdjacentHeight=1
                Changed=Road
                ChangedHeight=2

                [TILES]
                Count=1

                [TILE0]
                Model=tst01_a01_01
                WalkMesh=msb01
                TopLeft=Grass
                TopLeftHeight=0
                TopRight=Grass
                TopRightHeight=1
                BottomLeft=Grass
                BottomLeftHeight=2
                BottomRight=Grass
                BottomRightHeight=3
                Top=Road
                Right=Road
                Bottom=Road
                Left=Road
                MainLight1=1
                SourceLight2=0
                AnimLoop3=1
                Doors=1
                Sounds=2
                PathNode=A
                Orientation=90
                VisibilityNode=V
                VisibilityOrientation=180
                DoorVisibilityNode=D
                DoorVisibilityOrientation=270
                ImageMap2D=tile0

                [TILE0DOOR0]
                Type=3
                X=1
                Y=2
                Z=3
                Orientation=45

                [GROUPS]
                Count=1

                [GROUP0]
                Name=Corner
                StrRef=7
                Rows=1
                Columns=2
                Tile0=0
                Tile1=-1
            "#,
        )
        .unwrap_or_else(|error| panic!("parse set: {error}"));

        let built = build_set_text(&original).unwrap_or_else(|error| panic!("build set: {error}"));
        assert!(built.contains("[TERRAIN TYPES]\nCount=1"));
        assert!(built.contains("[GROUPS]\nCount=1"));

        let reparsed = parse_set(&built).unwrap_or_else(|error| panic!("reparse set: {error}"));
        assert_eq!(reparsed, original);
    }

    #[test]
    fn write_set_matches_build_text() {
        let original = parse_set(
            r#"
                [GENERAL]
                Name=TST01

                [TILES]
                Count=1

                [TILE0]
                Model=tst01_a01_01
            "#,
        )
        .unwrap_or_else(|error| panic!("parse set: {error}"));

        let built = build_set_text(&original).unwrap_or_else(|error| panic!("build set: {error}"));
        let mut bytes = Vec::new();
        write_set(&mut bytes, &original).unwrap_or_else(|error| panic!("write set: {error}"));

        let written =
            String::from_utf8(bytes).unwrap_or_else(|error| panic!("utf8 write set: {error}"));
        assert_eq!(written, built);
        let reparsed = parse_set(&written).unwrap_or_else(|error| panic!("reparse set: {error}"));
        assert_eq!(reparsed, original);
    }

    #[test]
    fn rejects_building_tileset_without_tiles() {
        let error = build_set_text(&SetFile::default())
            .err()
            .unwrap_or_else(|| panic!("expected build error for empty tileset"));
        assert_eq!(
            error.to_string(),
            "cannot build SET payload without at least one tile definition"
        );
    }
}
