#![forbid(unsafe_code)]
//! Typed parser for Neverwinter Nights area instance (`GIT`) resources.
//!
//! `GIT` files are `GFF V3.2` documents that describe the runtime contents of
//! an area such as placeables, doors, creatures, triggers, stores, sounds, and
//! waypoints. This crate layers a typed view over the raw GFF payload while
//! retaining the original entry structures for fields that are not typed yet.

use std::{
    fmt,
    fs::File,
    io::{self, Read, Seek},
    path::Path,
};

use nwnrs_gff::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::ResolvedResRef;
use nwnrs_restype::prelude::*;
use tracing::instrument;

/// NWN resource type id for `git`.
pub const GIT_RES_TYPE: ResType = ResType(2023);

/// Errors returned while reading or parsing `GIT` payloads.
#[derive(Debug)]
pub enum GitError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// GFF decoding failed.
    Gff(GffError),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl GitError {
    /// Creates a free-form `GIT` error message.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for GitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Gff(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for GitError {}

impl From<io::Error> for GitError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<GffError> for GitError {
    fn from(value: GffError) -> Self {
        Self::Gff(value)
    }
}

impl From<ResManError> for GitError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for `GIT` operations.
pub type GitResult<T> = Result<T, GitError>;

/// Parsed area instance payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitFile {
    /// Optional ambient/music settings for the area.
    pub area_properties: Option<GitAreaProperties>,
    /// Placed creatures.
    pub creatures:       Vec<GitCreature>,
    /// Placed doors.
    pub doors:           Vec<GitDoor>,
    /// Encounter volumes.
    pub encounters:      Vec<GitEncounter>,
    /// Raw legacy top-level `List` entries, preserved verbatim.
    pub legacy_list:     Vec<GffStruct>,
    /// Placed ambient or point sounds.
    pub sounds:          Vec<GitSound>,
    /// Placed stores.
    pub stores:          Vec<GitStore>,
    /// Trigger volumes.
    pub triggers:        Vec<GitTrigger>,
    /// Placed waypoints.
    pub waypoints:       Vec<GitWaypoint>,
    /// Placed placeables.
    pub placeables:      Vec<GitPlaceable>,
}

/// Parsed `AreaProperties` block.
#[derive(Debug, Clone, PartialEq)]
pub struct GitAreaProperties {
    /// Original raw GFF structure.
    pub raw: GffStruct,
    /// Day ambient sound id.
    pub ambient_sound_day: Option<i32>,
    /// Night ambient sound id.
    pub ambient_sound_night: Option<i32>,
    /// Day ambient sound volume.
    pub ambient_sound_day_volume: Option<i32>,
    /// Night ambient sound volume.
    pub ambient_sound_night_volume: Option<i32>,
    /// Environment audio profile id.
    pub env_audio: Option<i32>,
    /// Combat music id.
    pub music_battle: Option<i32>,
    /// Day music id.
    pub music_day: Option<i32>,
    /// Night music id.
    pub music_night: Option<i32>,
    /// Music delay value.
    pub music_delay: Option<i32>,
}

/// A world transform extracted from a GIT instance.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitTransform {
    /// World X position.
    pub x:             Option<f32>,
    /// World Y position.
    pub y:             Option<f32>,
    /// World Z position.
    pub z:             Option<f32>,
    /// Aurora planar bearing in radians for bearing-based instances.
    pub bearing:       Option<f32>,
    /// Orientation X component for vector-based instances.
    pub x_orientation: Option<f32>,
    /// Orientation Y component for vector-based instances.
    pub y_orientation: Option<f32>,
}

/// A geometry point used by triggers or encounters.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct GitPoint {
    /// Point X coordinate.
    pub x: Option<f32>,
    /// Point Y coordinate.
    pub y: Option<f32>,
    /// Point Z coordinate.
    pub z: Option<f32>,
}

/// A placed creature entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitCreature {
    /// Original raw GFF structure.
    pub raw:             GffStruct,
    /// Instance tag.
    pub tag:             Option<String>,
    /// Blueprint resource reference.
    pub template_resref: Option<String>,
    /// Localized display name when present.
    pub localized_name:  Option<GffCExoLocString>,
    /// Description string when present.
    pub description:     Option<GffCExoLocString>,
    /// Spawn transform.
    pub transform:       GitTransform,
}

/// A placed door entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitDoor {
    /// Original raw GFF structure.
    pub raw:             GffStruct,
    /// Instance tag.
    pub tag:             Option<String>,
    /// Localized display name.
    pub localized_name:  Option<GffCExoLocString>,
    /// Description string.
    pub description:     Option<GffCExoLocString>,
    /// Blueprint resource reference.
    pub template_resref: Option<String>,
    /// Door appearance id.
    pub appearance:      Option<i32>,
    /// Door animation state.
    pub animation_state: Option<i32>,
    /// Linked destination tag or waypoint.
    pub linked_to:       Option<String>,
    /// Placement transform.
    pub transform:       GitTransform,
}

/// An encounter volume entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitEncounter {
    /// Original raw GFF structure.
    pub raw:            GffStruct,
    /// Instance tag.
    pub tag:            Option<String>,
    /// Localized display name.
    pub localized_name: Option<GffCExoLocString>,
    /// Encounter origin or anchor transform when present.
    pub transform:      GitTransform,
    /// Polygon geometry points.
    pub geometry:       Vec<GitPoint>,
}

/// A single sound reference within a sound object.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSoundRef {
    /// Referenced sound resource name.
    pub sound: Option<String>,
}

/// A sound emitter entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitSound {
    /// Original raw GFF structure.
    pub raw:             GffStruct,
    /// Instance tag.
    pub tag:             Option<String>,
    /// Localized display name.
    pub localized_name:  Option<GffCExoLocString>,
    /// Template resource reference.
    pub template_resref: Option<String>,
    /// World transform.
    pub transform:       GitTransform,
    /// Whether the sound is positional.
    pub positional:      Option<bool>,
    /// Minimum audible distance.
    pub min_distance:    Option<f32>,
    /// Maximum audible distance.
    pub max_distance:    Option<f32>,
    /// Base volume.
    pub volume:          Option<i32>,
    /// Referenced sound entries.
    pub sounds:          Vec<GitSoundRef>,
}

/// A placed store entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitStore {
    /// Original raw GFF structure.
    pub raw:             GffStruct,
    /// Instance tag.
    pub tag:             Option<String>,
    /// Localized display name.
    pub localized_name:  Option<GffCExoLocString>,
    /// Blueprint resource reference.
    pub template_resref: Option<String>,
    /// Placement transform.
    pub transform:       GitTransform,
}

/// A trigger volume entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitTrigger {
    /// Original raw GFF structure.
    pub raw:            GffStruct,
    /// Instance tag.
    pub tag:            Option<String>,
    /// Localized display name.
    pub localized_name: Option<GffCExoLocString>,
    /// Trigger origin or anchor transform when present.
    pub transform:      GitTransform,
    /// Polygon geometry points.
    pub geometry:       Vec<GitPoint>,
}

/// A placed waypoint entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitWaypoint {
    /// Original raw GFF structure.
    pub raw:             GffStruct,
    /// Instance tag.
    pub tag:             Option<String>,
    /// Localized display name.
    pub localized_name:  Option<GffCExoLocString>,
    /// Description string.
    pub description:     Option<GffCExoLocString>,
    /// Waypoint template resource reference.
    pub template_resref: Option<String>,
    /// Linked destination tag or waypoint.
    pub linked_to:       Option<String>,
    /// Waypoint appearance id.
    pub appearance:      Option<i32>,
    /// Placement transform.
    pub transform:       GitTransform,
}

/// A placed placeable entry.
#[derive(Debug, Clone, PartialEq)]
pub struct GitPlaceable {
    /// Original raw GFF structure.
    pub raw:             GffStruct,
    /// Instance tag.
    pub tag:             Option<String>,
    /// Localized display name.
    pub localized_name:  Option<GffCExoLocString>,
    /// Description string.
    pub description:     Option<GffCExoLocString>,
    /// Blueprint resource reference.
    pub template_resref: Option<String>,
    /// Placeable appearance id.
    pub appearance:      Option<i32>,
    /// Whether the placeable is static.
    pub static_object:   Option<bool>,
    /// Whether the placeable is useable.
    pub useable:         Option<bool>,
    /// Whether the placeable has inventory.
    pub has_inventory:   Option<bool>,
    /// Placement transform.
    pub transform:       GitTransform,
}

/// Reads a typed `GIT` file from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_git<R: Read + Seek>(reader: &mut R) -> GitResult<GitFile> {
    let root = read_gff_root(reader)?;
    parse_git_root(&root)
}

/// Reads a typed `GIT` file from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_git_from_file(path: impl AsRef<Path>) -> GitResult<GitFile> {
    let mut file = File::open(path.as_ref())?;
    read_git(&mut file)
}

/// Reads a typed `GIT` file from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_git_from_res(res: &Res, use_cache: bool) -> GitResult<GitFile> {
    if res.resref().res_type() != GIT_RES_TYPE {
        return Err(GitError::msg(format!(
            "expected git resource, got {}",
            res.resref()
        )));
    }

    let bytes = res.read_all(use_cache)?;
    let mut cursor = io::Cursor::new(bytes);
    read_git(&mut cursor)
}

/// Reads a typed `GIT` file from a [`ResMan`] by area name.
#[instrument(level = "debug", skip_all, err, fields(area_name, use_cache))]
pub fn read_git_from_resman(
    resman: &mut ResMan,
    area_name: &str,
    use_cache: bool,
) -> GitResult<GitFile> {
    let resolved = ResolvedResRef::from_filename(&format!("{area_name}.git"))
        .map_err(|error| GitError::msg(format!("git resref: {error}")))?;
    let res = resman
        .get_resolved(&resolved)
        .ok_or_else(|| GitError::msg(format!("git not found in ResMan: {resolved}")))?;
    read_git_from_res(&res, use_cache)
}

/// Parses a typed `GIT` file from a decoded [`GffRoot`].
pub fn parse_git_root(root: &GffRoot) -> GitResult<GitFile> {
    if root.file_type != "GIT " {
        return Err(GitError::msg(format!(
            "expected GIT root, got {:?}",
            root.file_type
        )));
    }

    Ok(GitFile {
        area_properties: gff_struct(&root.root, "AreaProperties").map(parse_area_properties),
        creatures:       gff_list(&root.root, "Creature List")
            .into_iter()
            .flatten()
            .map(parse_creature)
            .collect(),
        doors:           gff_list(&root.root, "Door List")
            .into_iter()
            .flatten()
            .map(parse_door)
            .collect(),
        encounters:      gff_list(&root.root, "Encounter List")
            .into_iter()
            .flatten()
            .map(parse_encounter)
            .collect(),
        legacy_list:     gff_list(&root.root, "List")
            .map_or_else(Vec::new, |entries| entries.to_vec()),
        sounds:          gff_list(&root.root, "SoundList")
            .into_iter()
            .flatten()
            .map(parse_sound)
            .collect(),
        stores:          gff_list(&root.root, "StoreList")
            .into_iter()
            .flatten()
            .map(parse_store)
            .collect(),
        triggers:        gff_list(&root.root, "TriggerList")
            .into_iter()
            .flatten()
            .map(parse_trigger)
            .collect(),
        waypoints:       gff_list(&root.root, "WaypointList")
            .into_iter()
            .flatten()
            .map(parse_waypoint)
            .collect(),
        placeables:      gff_list(&root.root, "Placeable List")
            .into_iter()
            .flatten()
            .map(parse_placeable)
            .collect(),
    })
}

fn parse_area_properties(value: &GffStruct) -> GitAreaProperties {
    GitAreaProperties {
        raw: value.clone(),
        ambient_sound_day: gff_i32(value, "AmbientSndDay"),
        ambient_sound_night: gff_i32(value, "AmbientSndNight"),
        ambient_sound_day_volume: gff_i32(value, "AmbientSndDayVol"),
        ambient_sound_night_volume: gff_i32(value, "AmbientSndNitVol"),
        env_audio: gff_i32(value, "EnvAudio"),
        music_battle: gff_i32(value, "MusicBattle"),
        music_day: gff_i32(value, "MusicDay"),
        music_night: gff_i32(value, "MusicNight"),
        music_delay: gff_i32(value, "MusicDelay"),
    }
}

fn parse_creature(value: &GffStruct) -> GitCreature {
    GitCreature {
        raw:             value.clone(),
        tag:             gff_string(value, "Tag"),
        template_resref: gff_resref(value, "TemplateResRef"),
        localized_name:  gff_loc_string_any(value, &["LocName", "LocalizedName"]),
        description:     gff_loc_string(value, "Description"),
        transform:       parse_transform(value),
    }
}

fn parse_door(value: &GffStruct) -> GitDoor {
    GitDoor {
        raw:             value.clone(),
        tag:             gff_string(value, "Tag"),
        localized_name:  gff_loc_string(value, "LocName"),
        description:     gff_loc_string(value, "Description"),
        template_resref: gff_resref(value, "TemplateResRef"),
        appearance:      gff_i32(value, "Appearance"),
        animation_state: gff_i32(value, "AnimationState"),
        linked_to:       gff_string(value, "LinkedTo"),
        transform:       parse_transform(value),
    }
}

fn parse_encounter(value: &GffStruct) -> GitEncounter {
    GitEncounter {
        raw:            value.clone(),
        tag:            gff_string(value, "Tag"),
        localized_name: gff_loc_string_any(value, &["LocName", "LocalizedName"]),
        transform:      parse_transform(value),
        geometry:       parse_geometry(value),
    }
}

fn parse_sound(value: &GffStruct) -> GitSound {
    let sounds = gff_list(value, "Sounds")
        .into_iter()
        .flatten()
        .map(|entry| GitSoundRef {
            sound: gff_string_any(entry, &["Sound", "SoundResRef"]),
        })
        .collect();

    GitSound {
        raw: value.clone(),
        tag: gff_string(value, "Tag"),
        localized_name: gff_loc_string(value, "LocName"),
        template_resref: gff_resref(value, "TemplateResRef"),
        transform: parse_transform(value),
        positional: gff_bool(value, "Positional"),
        min_distance: gff_f32(value, "MinDistance"),
        max_distance: gff_f32(value, "MaxDistance"),
        volume: gff_i32(value, "Volume"),
        sounds,
    }
}

fn parse_store(value: &GffStruct) -> GitStore {
    GitStore {
        raw:             value.clone(),
        tag:             gff_string(value, "Tag"),
        localized_name:  gff_loc_string_any(value, &["LocName", "LocalizedName"]),
        template_resref: gff_string_any(value, &["ResRef", "TemplateResRef"]),
        transform:       parse_transform(value),
    }
}

fn parse_trigger(value: &GffStruct) -> GitTrigger {
    GitTrigger {
        raw:            value.clone(),
        tag:            gff_string(value, "Tag"),
        localized_name: gff_loc_string_any(value, &["LocName", "LocalizedName"]),
        transform:      parse_transform(value),
        geometry:       parse_geometry(value),
    }
}

fn parse_waypoint(value: &GffStruct) -> GitWaypoint {
    GitWaypoint {
        raw:             value.clone(),
        tag:             gff_string(value, "Tag"),
        localized_name:  gff_loc_string_any(value, &["LocalizedName", "LocName"]),
        description:     gff_loc_string(value, "Description"),
        template_resref: gff_resref(value, "TemplateResRef"),
        linked_to:       gff_string(value, "LinkedTo"),
        appearance:      gff_i32(value, "Appearance"),
        transform:       parse_transform(value),
    }
}

fn parse_placeable(value: &GffStruct) -> GitPlaceable {
    GitPlaceable {
        raw:             value.clone(),
        tag:             gff_string(value, "Tag"),
        localized_name:  gff_loc_string(value, "LocName"),
        description:     gff_loc_string(value, "Description"),
        template_resref: gff_resref(value, "TemplateResRef"),
        appearance:      gff_i32(value, "Appearance"),
        static_object:   gff_bool(value, "Static"),
        useable:         gff_bool(value, "Useable"),
        has_inventory:   gff_bool(value, "HasInventory"),
        transform:       parse_transform(value),
    }
}

fn parse_transform(value: &GffStruct) -> GitTransform {
    GitTransform {
        x:             gff_f32_any(value, &["X", "XPosition"]),
        y:             gff_f32_any(value, &["Y", "YPosition"]),
        z:             gff_f32_any(value, &["Z", "ZPosition"]),
        bearing:       gff_f32(value, "Bearing"),
        x_orientation: gff_f32(value, "XOrientation"),
        y_orientation: gff_f32(value, "YOrientation"),
    }
}

fn parse_geometry(value: &GffStruct) -> Vec<GitPoint> {
    gff_list(value, "Geometry")
        .into_iter()
        .flatten()
        .map(|point| GitPoint {
            x: gff_f32(point, "X"),
            y: gff_f32(point, "Y"),
            z: gff_f32(point, "Z"),
        })
        .collect()
}

fn gff_struct<'a>(value: &'a GffStruct, label: &str) -> Option<&'a GffStruct> {
    match value.get_field(label)?.value() {
        GffValue::Struct(child) => Some(child),
        _ => None,
    }
}

fn gff_list<'a>(value: &'a GffStruct, label: &str) -> Option<&'a [GffStruct]> {
    match value.get_field(label)?.value() {
        GffValue::List(items) => Some(items.as_slice()),
        _ => None,
    }
}

fn gff_bool(value: &GffStruct, label: &str) -> Option<bool> {
    match value.get_field(label)?.value() {
        GffValue::Byte(raw) => Some(*raw != 0),
        GffValue::Char(raw) => Some(*raw != 0),
        GffValue::Word(raw) => Some(*raw != 0),
        GffValue::Short(raw) => Some(*raw != 0),
        GffValue::Dword(raw) => Some(*raw != 0),
        GffValue::Int(raw) => Some(*raw != 0),
        _ => None,
    }
}

fn gff_i32(value: &GffStruct, label: &str) -> Option<i32> {
    match value.get_field(label)?.value() {
        GffValue::Byte(raw) => Some(i32::from(*raw)),
        GffValue::Char(raw) => Some(i32::from(*raw)),
        GffValue::Word(raw) => Some(i32::from(*raw)),
        GffValue::Short(raw) => Some(i32::from(*raw)),
        GffValue::Dword(raw) => i32::try_from(*raw).ok(),
        GffValue::Int(raw) => Some(*raw),
        _ => None,
    }
}

fn gff_f32(value: &GffStruct, label: &str) -> Option<f32> {
    match value.get_field(label)?.value() {
        GffValue::Byte(raw) => Some(f32::from(*raw)),
        GffValue::Char(raw) => Some(f32::from(*raw)),
        GffValue::Word(raw) => Some(f32::from(*raw)),
        GffValue::Short(raw) => Some(f32::from(*raw)),
        GffValue::Dword(raw) => Some(*raw as f32),
        GffValue::Int(raw) => Some(*raw as f32),
        GffValue::Float(raw) => Some(*raw),
        _ => None,
    }
}

fn gff_string(value: &GffStruct, label: &str) -> Option<String> {
    gff_string_any(value, &[label])
}

fn gff_string_any(value: &GffStruct, labels: &[&str]) -> Option<String> {
    labels
        .iter()
        .find_map(|label| match value.get_field(label)?.value() {
            GffValue::CExoString(raw) | GffValue::ResRef(raw) => {
                let trimmed = raw.trim();
                (!trimmed.is_empty()).then(|| trimmed.to_string())
            }
            _ => None,
        })
}

fn gff_resref(value: &GffStruct, label: &str) -> Option<String> {
    gff_string_any(value, &[label])
}

fn gff_loc_string(value: &GffStruct, label: &str) -> Option<GffCExoLocString> {
    gff_loc_string_any(value, &[label])
}

fn gff_loc_string_any(value: &GffStruct, labels: &[&str]) -> Option<GffCExoLocString> {
    labels
        .iter()
        .find_map(|label| match value.get_field(label)?.value() {
            GffValue::CExoLocString(raw) => Some(raw.clone()),
            _ => None,
        })
}

fn gff_f32_any(value: &GffStruct, labels: &[&str]) -> Option<f32> {
    labels.iter().find_map(|label| gff_f32(value, label))
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        GIT_RES_TYPE, GitAreaProperties, GitCreature, GitDoor, GitEncounter, GitError, GitFile,
        GitPlaceable, GitPoint, GitResult, GitSound, GitSoundRef, GitStore, GitTransform,
        GitTrigger, GitWaypoint, parse_git_root, read_git, read_git_from_file, read_git_from_res,
        read_git_from_resman,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use nwnrs_gff::prelude::{
        GffCExoLocString, GffRoot, GffValue, new_c_exo_loc_string, new_gff_root, new_gff_struct,
        read_gff_root, write_gff_root,
    };
    use nwnrs_resman::{ResContainer, ResMan};
    use nwnrs_resmemfile::prelude::read_resmemfile;
    use nwnrs_resref::prelude::new_res_ref;

    use super::{GIT_RES_TYPE, parse_git_root, read_git, read_git_from_resman};

    fn encode_root(root: &GffRoot) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        write_gff_root(&mut output, root).unwrap_or_else(|error| {
            panic!("encode gff: {error}");
        });
        output.into_inner()
    }

    fn make_loc_string(text: &str) -> GffCExoLocString {
        let mut result = new_c_exo_loc_string();
        result.entries.push((0, text.to_string()));
        result
    }

    fn sample_git_root() -> GffRoot {
        let mut root = new_gff_root("GIT ");

        let mut area = new_gff_struct(100);
        area.put_value("AmbientSndDay", GffValue::Int(81))
            .unwrap_or_else(|error| panic!("area ambient day: {error}"));
        area.put_value("MusicDay", GffValue::Int(12))
            .unwrap_or_else(|error| panic!("area music day: {error}"));
        root.put_value("AreaProperties", GffValue::Struct(area))
            .unwrap_or_else(|error| panic!("root area properties: {error}"));

        let mut creature = new_gff_struct(1);
        creature
            .put_value("Tag", GffValue::CExoString("orc_01".to_string()))
            .unwrap_or_else(|error| panic!("creature tag: {error}"));
        creature
            .put_value(
                "TemplateResRef",
                GffValue::ResRef("orcblueprint".to_string()),
            )
            .unwrap_or_else(|error| panic!("creature template: {error}"));
        creature
            .put_value("LocName", GffValue::CExoLocString(make_loc_string("Orc")))
            .unwrap_or_else(|error| panic!("creature loc name: {error}"));
        creature
            .put_value("XPosition", GffValue::Float(1.0))
            .unwrap_or_else(|error| panic!("creature x: {error}"));
        creature
            .put_value("YPosition", GffValue::Float(2.0))
            .unwrap_or_else(|error| panic!("creature y: {error}"));
        creature
            .put_value("ZPosition", GffValue::Float(3.0))
            .unwrap_or_else(|error| panic!("creature z: {error}"));
        root.put_value("Creature List", GffValue::List(vec![creature]))
            .unwrap_or_else(|error| panic!("root creature list: {error}"));

        let mut door = new_gff_struct(2);
        door.put_value("Tag", GffValue::CExoString("gate".to_string()))
            .unwrap_or_else(|error| panic!("door tag: {error}"));
        door.put_value("TemplateResRef", GffValue::ResRef("door_gate".to_string()))
            .unwrap_or_else(|error| panic!("door template: {error}"));
        door.put_value("Appearance", GffValue::Int(4))
            .unwrap_or_else(|error| panic!("door appearance: {error}"));
        door.put_value("Bearing", GffValue::Float(1.57))
            .unwrap_or_else(|error| panic!("door bearing: {error}"));
        door.put_value("X", GffValue::Float(10.0))
            .unwrap_or_else(|error| panic!("door x: {error}"));
        door.put_value("Y", GffValue::Float(20.0))
            .unwrap_or_else(|error| panic!("door y: {error}"));
        door.put_value("Z", GffValue::Float(0.5))
            .unwrap_or_else(|error| panic!("door z: {error}"));
        root.put_value("Door List", GffValue::List(vec![door]))
            .unwrap_or_else(|error| panic!("root door list: {error}"));

        let mut sound_ref = new_gff_struct(0);
        sound_ref
            .put_value("Sound", GffValue::ResRef("as_pl_creak1".to_string()))
            .unwrap_or_else(|error| panic!("sound ref: {error}"));

        let mut sound = new_gff_struct(3);
        sound
            .put_value("Tag", GffValue::CExoString("creak".to_string()))
            .unwrap_or_else(|error| panic!("sound tag: {error}"));
        sound
            .put_value("Positional", GffValue::Byte(1))
            .unwrap_or_else(|error| panic!("sound positional: {error}"));
        sound
            .put_value("Volume", GffValue::Int(64))
            .unwrap_or_else(|error| panic!("sound volume: {error}"));
        sound
            .put_value("Sounds", GffValue::List(vec![sound_ref]))
            .unwrap_or_else(|error| panic!("sound list: {error}"));
        root.put_value("SoundList", GffValue::List(vec![sound]))
            .unwrap_or_else(|error| panic!("root sound list: {error}"));

        let mut waypoint = new_gff_struct(4);
        waypoint
            .put_value("Tag", GffValue::CExoString("spawn0".to_string()))
            .unwrap_or_else(|error| panic!("waypoint tag: {error}"));
        waypoint
            .put_value(
                "LocalizedName",
                GffValue::CExoLocString(make_loc_string("Spawn")),
            )
            .unwrap_or_else(|error| panic!("waypoint loc name: {error}"));
        waypoint
            .put_value("TemplateResRef", GffValue::ResRef("spawn0".to_string()))
            .unwrap_or_else(|error| panic!("waypoint template: {error}"));
        waypoint
            .put_value("XPosition", GffValue::Float(5.0))
            .unwrap_or_else(|error| panic!("waypoint x: {error}"));
        waypoint
            .put_value("YPosition", GffValue::Float(6.0))
            .unwrap_or_else(|error| panic!("waypoint y: {error}"));
        waypoint
            .put_value("ZPosition", GffValue::Float(7.0))
            .unwrap_or_else(|error| panic!("waypoint z: {error}"));
        waypoint
            .put_value("XOrientation", GffValue::Float(0.0))
            .unwrap_or_else(|error| panic!("waypoint xo: {error}"));
        waypoint
            .put_value("YOrientation", GffValue::Float(1.0))
            .unwrap_or_else(|error| panic!("waypoint yo: {error}"));
        root.put_value("WaypointList", GffValue::List(vec![waypoint]))
            .unwrap_or_else(|error| panic!("root waypoint list: {error}"));

        let mut placeable = new_gff_struct(5);
        placeable
            .put_value("Tag", GffValue::CExoString("chest_01".to_string()))
            .unwrap_or_else(|error| panic!("placeable tag: {error}"));
        placeable
            .put_value("LocName", GffValue::CExoLocString(make_loc_string("Chest")))
            .unwrap_or_else(|error| panic!("placeable loc name: {error}"));
        placeable
            .put_value("TemplateResRef", GffValue::ResRef("plc_chest".to_string()))
            .unwrap_or_else(|error| panic!("placeable template: {error}"));
        placeable
            .put_value("Appearance", GffValue::Int(99))
            .unwrap_or_else(|error| panic!("placeable appearance: {error}"));
        placeable
            .put_value("Static", GffValue::Byte(1))
            .unwrap_or_else(|error| panic!("placeable static: {error}"));
        placeable
            .put_value("Useable", GffValue::Byte(1))
            .unwrap_or_else(|error| panic!("placeable useable: {error}"));
        placeable
            .put_value("HasInventory", GffValue::Byte(1))
            .unwrap_or_else(|error| panic!("placeable inventory: {error}"));
        placeable
            .put_value("X", GffValue::Float(11.0))
            .unwrap_or_else(|error| panic!("placeable x: {error}"));
        placeable
            .put_value("Y", GffValue::Float(12.0))
            .unwrap_or_else(|error| panic!("placeable y: {error}"));
        placeable
            .put_value("Z", GffValue::Float(0.0))
            .unwrap_or_else(|error| panic!("placeable z: {error}"));
        placeable
            .put_value("Bearing", GffValue::Float(0.25))
            .unwrap_or_else(|error| panic!("placeable bearing: {error}"));
        root.put_value("Placeable List", GffValue::List(vec![placeable]))
            .unwrap_or_else(|error| panic!("root placeable list: {error}"));

        root
    }

    #[test]
    fn parses_typed_git_root() {
        let encoded = encode_root(&sample_git_root());
        let reparsed_root =
            read_gff_root(&mut Cursor::new(encoded.clone())).unwrap_or_else(|error| {
                panic!("re-read gff root: {error}");
            });

        let parsed = parse_git_root(&reparsed_root).unwrap_or_else(|error| {
            panic!("parse git root: {error}");
        });

        assert_eq!(
            parsed
                .area_properties
                .as_ref()
                .and_then(|value| value.ambient_sound_day),
            Some(81)
        );
        assert_eq!(parsed.creatures.len(), 1);
        assert_eq!(parsed.doors.len(), 1);
        assert_eq!(parsed.sounds.len(), 1);
        assert_eq!(parsed.waypoints.len(), 1);
        assert_eq!(parsed.placeables.len(), 1);
        assert_eq!(
            parsed.creatures[0].template_resref.as_deref(),
            Some("orcblueprint")
        );
        assert_eq!(parsed.doors[0].transform.bearing, Some(1.57));
        assert_eq!(
            parsed.sounds[0].sounds[0].sound.as_deref(),
            Some("as_pl_creak1")
        );
        assert_eq!(parsed.waypoints[0].transform.y_orientation, Some(1.0));
        assert_eq!(parsed.placeables[0].static_object, Some(true));

        let reparsed = read_git(&mut Cursor::new(encoded)).unwrap_or_else(|error| {
            panic!("read git: {error}");
        });
        assert_eq!(reparsed.placeables[0].transform.x, Some(11.0));
    }

    #[test]
    fn reads_git_from_resman() {
        let bytes = encode_root(&sample_git_root());
        let rr = new_res_ref("arena", GIT_RES_TYPE).unwrap_or_else(|error| {
            panic!("arena rr: {error}");
        });
        let resmem = read_resmemfile("arena.git", rr, bytes).unwrap_or_else(|error| {
            panic!("resmem file: {error}");
        });

        let mut resman = ResMan::new(0);
        resman.add(Arc::new(resmem) as Arc<dyn ResContainer>);

        let parsed = read_git_from_resman(&mut resman, "arena", false).unwrap_or_else(|error| {
            panic!("read git from resman: {error}");
        });
        assert_eq!(
            parsed
                .area_properties
                .as_ref()
                .and_then(|value| value.music_day),
            Some(12)
        );
        assert_eq!(
            parsed.placeables[0].template_resref.as_deref(),
            Some("plc_chest")
        );
    }
}
