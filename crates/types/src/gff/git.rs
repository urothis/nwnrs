#![forbid(unsafe_code)]

use std::{
    fmt,
    fs::File,
    io::{self, Read, Seek, Write},
    path::Path,
};

use nwnrs_types::resman::prelude::*;
use tracing::instrument;

use crate::gff::{
    GffCExoLocString, GffError, GffRoot, GffStruct, GffValue, read_gff_root, write_gff_root,
};

/// NWN resource type id for `git`.
pub const GIT_RES_TYPE: ResType = ResType(2023);

/// Errors returned while reading or parsing `GIT` payloads.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitError>();
/// ```
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
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let error = nwnrs_types::gff::GitError::msg("bad git");
    /// assert_eq!(error.to_string(), "bad git");
    /// ```
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
///
/// Each typed collection preserves the authored instance ordering for its
/// category. Where typed coverage is incomplete, the underlying raw GFF
/// structures remain available on the typed entries or through
/// [`GitFile::legacy_list`].
///
/// # Examples
///
/// ```rust,no_run
/// let git = nwnrs_types::gff::GitFile::default();
/// assert!(git.creatures.is_empty());
/// ```
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

impl GitFile {
    /// Reads a typed `GIT` file from disk.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] if the file cannot be opened or parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::gff::GitFile::from_file(std::path::Path::new("area.git"));
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> GitResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_git(&mut file)
    }

    /// Reads a typed `GIT` file from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] if the resource is not a GIT type or the bytes
    /// cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::gff::GitFile::from_res;
    /// ```
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> GitResult<Self> {
        if res.resref().res_type() != GIT_RES_TYPE {
            return Err(GitError::msg(format!(
                "expected git resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        let mut cursor = io::Cursor::new(bytes);
        read_git(&mut cursor)
    }

    /// Reads a typed `GIT` file from a [`ResMan`] by area name.
    ///
    /// # Errors
    ///
    /// Returns [`GitError`] if the resource cannot be found or parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::gff::GitFile::from_resman;
    /// ```
    pub fn from_resman(
        resman: &mut ResMan,
        area_name: &str,
        cache_policy: CachePolicy,
    ) -> GitResult<Self> {
        let resolved = ResolvedResRef::from_filename(&format!("{area_name}.git"))
            .map_err(|error| GitError::msg(format!("git resref: {error}")))?;
        let res = resman
            .get_resolved(&resolved)
            .ok_or_else(|| GitError::msg(format!("git not found in ResMan: {resolved}")))?;
        Self::from_res(&res, cache_policy)
    }
}

/// Parsed `AreaProperties` block.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitAreaProperties>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let transform = nwnrs_types::gff::GitTransform::default();
/// assert!(transform.x.is_none());
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let point = nwnrs_types::gff::GitPoint::default();
/// assert!(point.x.is_none());
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitCreature>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitDoor>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitEncounter>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let sound_ref = nwnrs_types::gff::GitSoundRef::default();
/// assert!(sound_ref.sound.is_none());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GitSoundRef {
    /// Referenced sound resource name.
    pub sound: Option<String>,
}

/// A sound emitter entry.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitSound>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitStore>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitTrigger>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitWaypoint>();
/// ```
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
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::gff::GitPlaceable>();
/// ```
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
///
/// # Errors
///
/// Returns [`GitError`] if the data cannot be read or does not conform to the
/// GIT format.
///
/// # Examples
///
/// ```rust,no_run
/// let mut reader = std::io::Cursor::new(Vec::<u8>::new());
/// let _ = nwnrs_types::gff::read_git(&mut reader);
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn read_git<R: Read + Seek>(reader: &mut R) -> GitResult<GitFile> {
    let root = read_gff_root(reader)?;
    parse_git_root(&root)
}

/// Parses a typed `GIT` file from a decoded [`GffRoot`].
///
/// # Errors
///
/// Returns [`GitError`] if the root file type is not `GIT `.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::gff::parse_git_root;
/// ```
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
        legacy_list:     gff_list(&root.root, "List").map_or_else(Vec::new, <[GffStruct]>::to_vec),
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

/// Builds a typed [`GffRoot`] from a [`GitFile`].
///
/// Known typed fields are rewritten from the typed model. Unknown fields stored
/// on per-entry raw structures are preserved.
///
/// # Errors
///
/// Returns [`GitError`] if any GFF field label is invalid.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::gff::build_git_root;
/// ```
pub fn build_git_root(git: &GitFile) -> GitResult<GffRoot> {
    let mut root = GffRoot::new("GIT ");

    if let Some(area_properties) = &git.area_properties {
        root.put_value(
            "AreaProperties",
            GffValue::Struct(build_area_properties(area_properties)?),
        )?;
    }

    put_list(
        &mut root.root,
        "Creature List",
        &git.creatures,
        build_creature,
    )?;
    put_list(&mut root.root, "Door List", &git.doors, build_door)?;
    put_list(
        &mut root.root,
        "Encounter List",
        &git.encounters,
        build_encounter,
    )?;
    put_list(&mut root.root, "List", &git.legacy_list, |value| {
        Ok(value.clone())
    })?;
    put_list(&mut root.root, "SoundList", &git.sounds, build_sound)?;
    put_list(&mut root.root, "StoreList", &git.stores, build_store)?;
    put_list(&mut root.root, "TriggerList", &git.triggers, build_trigger)?;
    put_list(
        &mut root.root,
        "WaypointList",
        &git.waypoints,
        build_waypoint,
    )?;
    put_list(
        &mut root.root,
        "Placeable List",
        &git.placeables,
        build_placeable,
    )?;

    Ok(root)
}

/// Writes a typed `GIT` file to `writer`.
///
/// This is equivalent to [`build_git_root`] followed by [`write_gff_root`].
///
/// # Errors
///
/// Returns [`GitError`] if building or writing the GFF root fails.
///
/// # Examples
///
/// ```rust,no_run
/// let git = nwnrs_types::gff::GitFile::default();
/// let mut writer = std::io::Cursor::new(Vec::new());
/// nwnrs_types::gff::write_git(&mut writer, &git)?;
/// # Ok::<(), nwnrs_types::gff::GitError>(())
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn write_git<W: Write + Seek>(writer: &mut W, git: &GitFile) -> GitResult<()> {
    let root = build_git_root(git)?;
    write_gff_root(writer, &root)?;
    Ok(())
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

#[allow(clippy::cast_precision_loss)]
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

fn build_area_properties(value: &GitAreaProperties) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "AmbientSndDay",
            "AmbientSndNight",
            "AmbientSndDayVol",
            "AmbientSndNitVol",
            "EnvAudio",
            "MusicBattle",
            "MusicDay",
            "MusicNight",
            "MusicDelay",
        ],
    );
    put_i32(&mut result, "AmbientSndDay", value.ambient_sound_day)?;
    put_i32(&mut result, "AmbientSndNight", value.ambient_sound_night)?;
    put_i32(
        &mut result,
        "AmbientSndDayVol",
        value.ambient_sound_day_volume,
    )?;
    put_i32(
        &mut result,
        "AmbientSndNitVol",
        value.ambient_sound_night_volume,
    )?;
    put_i32(&mut result, "EnvAudio", value.env_audio)?;
    put_i32(&mut result, "MusicBattle", value.music_battle)?;
    put_i32(&mut result, "MusicDay", value.music_day)?;
    put_i32(&mut result, "MusicNight", value.music_night)?;
    put_i32(&mut result, "MusicDelay", value.music_delay)?;
    Ok(result)
}

fn build_creature(value: &GitCreature) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "TemplateResRef",
            "LocName",
            "LocalizedName",
            "Description",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_resref(
        &mut result,
        "TemplateResRef",
        value.template_resref.as_deref(),
    )?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocName"),
        value.localized_name.as_ref(),
    )?;
    put_loc_string(&mut result, "Description", value.description.as_ref())?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    Ok(result)
}

fn build_door(value: &GitDoor) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "Description",
            "TemplateResRef",
            "Appearance",
            "AnimationState",
            "LinkedTo",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocName"),
        value.localized_name.as_ref(),
    )?;
    put_loc_string(&mut result, "Description", value.description.as_ref())?;
    put_resref(
        &mut result,
        "TemplateResRef",
        value.template_resref.as_deref(),
    )?;
    put_i32(&mut result, "Appearance", value.appearance)?;
    put_i32(&mut result, "AnimationState", value.animation_state)?;
    put_string(&mut result, "LinkedTo", value.linked_to.as_deref())?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    Ok(result)
}

fn build_encounter(value: &GitEncounter) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "Geometry",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocName"),
        value.localized_name.as_ref(),
    )?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    put_geometry(&mut result, &value.geometry)?;
    Ok(result)
}

fn build_sound(value: &GitSound) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "TemplateResRef",
            "Positional",
            "MinDistance",
            "MaxDistance",
            "Volume",
            "Sounds",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocName"),
        value.localized_name.as_ref(),
    )?;
    put_resref(
        &mut result,
        "TemplateResRef",
        value.template_resref.as_deref(),
    )?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    put_bool(&mut result, "Positional", value.positional)?;
    put_f32(&mut result, "MinDistance", value.min_distance)?;
    put_f32(&mut result, "MaxDistance", value.max_distance)?;
    put_i32(&mut result, "Volume", value.volume)?;

    let sounds = value
        .sounds
        .iter()
        .map(build_sound_ref)
        .collect::<GitResult<Vec<_>>>()?;
    put_list_value(&mut result, "Sounds", sounds)?;
    Ok(result)
}

fn build_sound_ref(value: &GitSoundRef) -> GitResult<GffStruct> {
    let mut result = GffStruct::new(0);
    put_resref(&mut result, "Sound", value.sound.as_deref())?;
    Ok(result)
}

fn build_store(value: &GitStore) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "ResRef",
            "TemplateResRef",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocalizedName"),
        value.localized_name.as_ref(),
    )?;
    put_resref(
        &mut result,
        preferred_resref_label(&value.raw, &["ResRef", "TemplateResRef"], "ResRef"),
        value.template_resref.as_deref(),
    )?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    Ok(result)
}

fn build_trigger(value: &GitTrigger) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "Geometry",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocName"),
        value.localized_name.as_ref(),
    )?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    put_geometry(&mut result, &value.geometry)?;
    Ok(result)
}

fn build_waypoint(value: &GitWaypoint) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "Description",
            "TemplateResRef",
            "LinkedTo",
            "Appearance",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocalizedName", "LocName"], "LocalizedName"),
        value.localized_name.as_ref(),
    )?;
    put_loc_string(&mut result, "Description", value.description.as_ref())?;
    put_resref(
        &mut result,
        "TemplateResRef",
        value.template_resref.as_deref(),
    )?;
    put_string(&mut result, "LinkedTo", value.linked_to.as_deref())?;
    put_i32(&mut result, "Appearance", value.appearance)?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    Ok(result)
}

fn build_placeable(value: &GitPlaceable) -> GitResult<GffStruct> {
    let mut result = value.raw.clone();
    clear_labels(
        &mut result,
        &[
            "Tag",
            "LocName",
            "LocalizedName",
            "Description",
            "TemplateResRef",
            "Appearance",
            "Static",
            "Useable",
            "HasInventory",
            "XPosition",
            "YPosition",
            "ZPosition",
            "X",
            "Y",
            "Z",
            "Bearing",
            "XOrientation",
            "YOrientation",
        ],
    );
    put_string(&mut result, "Tag", value.tag.as_deref())?;
    put_loc_string(
        &mut result,
        preferred_loc_label(&value.raw, &["LocName", "LocalizedName"], "LocName"),
        value.localized_name.as_ref(),
    )?;
    put_loc_string(&mut result, "Description", value.description.as_ref())?;
    put_resref(
        &mut result,
        "TemplateResRef",
        value.template_resref.as_deref(),
    )?;
    put_i32(&mut result, "Appearance", value.appearance)?;
    put_bool(&mut result, "Static", value.static_object)?;
    put_bool(&mut result, "Useable", value.useable)?;
    put_bool(&mut result, "HasInventory", value.has_inventory)?;
    put_transform(&mut result, &value.raw, &value.transform)?;
    Ok(result)
}

fn put_transform(target: &mut GffStruct, raw: &GffStruct, value: &GitTransform) -> GitResult<()> {
    let (x_label, y_label, z_label) = preferred_position_labels(raw);
    put_f32(target, x_label, value.x)?;
    put_f32(target, y_label, value.y)?;
    put_f32(target, z_label, value.z)?;
    put_f32(target, "Bearing", value.bearing)?;
    put_f32(target, "XOrientation", value.x_orientation)?;
    put_f32(target, "YOrientation", value.y_orientation)?;
    Ok(())
}

fn put_geometry(target: &mut GffStruct, value: &[GitPoint]) -> GitResult<()> {
    let geometry = value
        .iter()
        .map(|point| {
            let mut result = GffStruct::new(0);
            put_f32(&mut result, "X", point.x)?;
            put_f32(&mut result, "Y", point.y)?;
            put_f32(&mut result, "Z", point.z)?;
            Ok(result)
        })
        .collect::<GitResult<Vec<_>>>()?;
    put_list_value(target, "Geometry", geometry)?;
    Ok(())
}

fn preferred_position_labels(raw: &GffStruct) -> (&'static str, &'static str, &'static str) {
    if raw.get_field("XPosition").is_some()
        || raw.get_field("YPosition").is_some()
        || raw.get_field("ZPosition").is_some()
    {
        ("XPosition", "YPosition", "ZPosition")
    } else {
        ("X", "Y", "Z")
    }
}

fn preferred_loc_label<'a>(raw: &GffStruct, labels: &[&'a str], fallback: &'a str) -> &'a str {
    labels
        .iter()
        .copied()
        .find(|label| raw.get_field(label).is_some())
        .unwrap_or(fallback)
}

fn preferred_resref_label<'a>(raw: &GffStruct, labels: &[&'a str], fallback: &'a str) -> &'a str {
    labels
        .iter()
        .copied()
        .find(|label| raw.get_field(label).is_some())
        .unwrap_or(fallback)
}

fn clear_labels(target: &mut GffStruct, labels: &[&str]) {
    for label in labels {
        let _ = target.remove(label);
    }
}

fn put_list<T, F>(target: &mut GffStruct, label: &str, values: &[T], mut build: F) -> GitResult<()>
where
    F: FnMut(&T) -> GitResult<GffStruct>,
{
    let structs = values
        .iter()
        .map(&mut build)
        .collect::<GitResult<Vec<_>>>()?;
    put_list_value(target, label, structs)?;
    Ok(())
}

fn put_list_value(target: &mut GffStruct, label: &str, values: Vec<GffStruct>) -> GitResult<()> {
    target.put_value(label, GffValue::List(values))?;
    Ok(())
}

fn put_string(target: &mut GffStruct, label: &str, value: Option<&str>) -> GitResult<()> {
    if let Some(value) = value {
        target.put_value(label, GffValue::CExoString(value.to_string()))?;
    }
    Ok(())
}

fn put_resref(target: &mut GffStruct, label: &str, value: Option<&str>) -> GitResult<()> {
    if let Some(value) = value {
        target.put_value(label, GffValue::ResRef(value.to_string()))?;
    }
    Ok(())
}

fn put_loc_string(
    target: &mut GffStruct,
    label: &str,
    value: Option<&GffCExoLocString>,
) -> GitResult<()> {
    if let Some(value) = value {
        target.put_value(label, GffValue::CExoLocString(value.clone()))?;
    }
    Ok(())
}

fn put_i32(target: &mut GffStruct, label: &str, value: Option<i32>) -> GitResult<()> {
    if let Some(value) = value {
        target.put_value(label, GffValue::Int(value))?;
    }
    Ok(())
}

fn put_f32(target: &mut GffStruct, label: &str, value: Option<f32>) -> GitResult<()> {
    if let Some(value) = value {
        target.put_value(label, GffValue::Float(value))?;
    }
    Ok(())
}

fn put_bool(target: &mut GffStruct, label: &str, value: Option<bool>) -> GitResult<()> {
    if let Some(value) = value {
        target.put_value(label, GffValue::Byte(u8::from(value)))?;
    }
    Ok(())
}

/// Common imports for consumers of this crate.
#[cfg(test)]
mod tests {
    use std::{io::Cursor, sync::Arc};

    use nwnrs_types::resman::{CachePolicy, ResContainer, ResMan, ResRef, read_resmemfile};

    use super::{GIT_RES_TYPE, GitFile, build_git_root, parse_git_root, read_git, write_git};
    use crate::gff::{
        GffCExoLocString, GffField, GffRoot, GffStruct, GffValue, read_gff_root, write_gff_root,
    };

    fn encode_root(root: &GffRoot) -> Vec<u8> {
        let mut output = Cursor::new(Vec::new());
        write_gff_root(&mut output, root).unwrap_or_else(|error| {
            panic!("encode gff: {error}");
        });
        output.into_inner()
    }

    fn make_loc_string(text: &str) -> GffCExoLocString {
        let mut result = GffCExoLocString::default();
        result.entries.push((0, text.to_string()));
        result
    }

    fn sample_git_root() -> GffRoot {
        let mut root = GffRoot::new("GIT ");

        let mut area = GffStruct::new(100);
        area.put_value("AmbientSndDay", GffValue::Int(81))
            .unwrap_or_else(|error| panic!("area ambient day: {error}"));
        area.put_value("MusicDay", GffValue::Int(12))
            .unwrap_or_else(|error| panic!("area music day: {error}"));
        root.put_value("AreaProperties", GffValue::Struct(area))
            .unwrap_or_else(|error| panic!("root area properties: {error}"));

        let mut creature = GffStruct::new(1);
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

        let mut door = GffStruct::new(2);
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

        let mut sound_ref = GffStruct::new(0);
        sound_ref
            .put_value("Sound", GffValue::ResRef("as_pl_creak1".to_string()))
            .unwrap_or_else(|error| panic!("sound ref: {error}"));

        let mut sound = GffStruct::new(3);
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

        let mut waypoint = GffStruct::new(4);
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

        let mut placeable = GffStruct::new(5);
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
            parsed
                .creatures
                .first()
                .and_then(|value| value.template_resref.as_deref()),
            Some("orcblueprint")
        );
        assert_eq!(
            parsed
                .doors
                .first()
                .and_then(|value| value.transform.bearing),
            Some(1.57)
        );
        assert_eq!(
            parsed
                .sounds
                .first()
                .and_then(|value| value.sounds.first())
                .and_then(|value| value.sound.as_deref()),
            Some("as_pl_creak1")
        );
        assert_eq!(
            parsed
                .waypoints
                .first()
                .and_then(|value| value.transform.y_orientation),
            Some(1.0)
        );
        assert_eq!(
            parsed
                .placeables
                .first()
                .and_then(|value| value.static_object),
            Some(true)
        );

        let reparsed = read_git(&mut Cursor::new(encoded)).unwrap_or_else(|error| {
            panic!("read git: {error}");
        });
        assert_eq!(
            reparsed
                .placeables
                .first()
                .and_then(|value| value.transform.x),
            Some(11.0)
        );
    }

    #[test]
    fn reads_git_from_resman() {
        let bytes = encode_root(&sample_git_root());
        let rr = ResRef::new("arena", GIT_RES_TYPE).unwrap_or_else(|error| {
            panic!("arena rr: {error}");
        });
        let resmem = read_resmemfile("arena.git", rr, bytes).unwrap_or_else(|error| {
            panic!("resmem file: {error}");
        });

        let mut resman = ResMan::new(0);
        resman.add(Arc::new(resmem) as Arc<dyn ResContainer>);

        let parsed = GitFile::from_resman(&mut resman, "arena", CachePolicy::Bypass)
            .unwrap_or_else(|error| {
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
            parsed
                .placeables
                .first()
                .and_then(|value| value.template_resref.as_deref()),
            Some("plc_chest")
        );
    }

    #[test]
    fn writes_git_round_trip_from_typed_model() {
        let original = read_git(&mut Cursor::new(encode_root(&sample_git_root())))
            .unwrap_or_else(|error| panic!("read original git: {error}"));

        let mut encoded = Cursor::new(Vec::new());
        write_git(&mut encoded, &original).unwrap_or_else(|error| {
            panic!("write git: {error}");
        });

        let reparsed = read_git(&mut Cursor::new(encoded.into_inner())).unwrap_or_else(|error| {
            panic!("re-read git: {error}");
        });
        assert_eq!(
            reparsed
                .area_properties
                .as_ref()
                .and_then(|value| value.music_day),
            original
                .area_properties
                .as_ref()
                .and_then(|value| value.music_day)
        );
        assert_eq!(reparsed.creatures.len(), original.creatures.len());
        assert_eq!(reparsed.doors.len(), original.doors.len());
        assert_eq!(reparsed.sounds.len(), original.sounds.len());
        assert_eq!(reparsed.waypoints.len(), original.waypoints.len());
        assert_eq!(reparsed.placeables.len(), original.placeables.len());
        assert_eq!(
            reparsed
                .creatures
                .first()
                .and_then(|value| value.template_resref.as_deref()),
            original
                .creatures
                .first()
                .and_then(|value| value.template_resref.as_deref())
        );
        assert_eq!(
            reparsed
                .doors
                .first()
                .and_then(|value| value.transform.bearing),
            original
                .doors
                .first()
                .and_then(|value| value.transform.bearing)
        );
        assert_eq!(
            reparsed
                .sounds
                .first()
                .and_then(|value| value.sounds.first())
                .and_then(|value| value.sound.as_deref()),
            original
                .sounds
                .first()
                .and_then(|value| value.sounds.first())
                .and_then(|value| value.sound.as_deref())
        );
        assert_eq!(
            reparsed
                .waypoints
                .first()
                .and_then(|value| value.transform.y_orientation),
            original
                .waypoints
                .first()
                .and_then(|value| value.transform.y_orientation)
        );
        assert_eq!(
            reparsed
                .placeables
                .first()
                .and_then(|value| value.static_object),
            original
                .placeables
                .first()
                .and_then(|value| value.static_object)
        );
    }

    #[test]
    fn build_git_root_preserves_unknown_fields_from_raw_entries() {
        let mut parsed = read_git(&mut Cursor::new(encode_root(&sample_git_root())))
            .unwrap_or_else(|error| panic!("read original git: {error}"));
        parsed
            .creatures
            .first_mut()
            .unwrap_or_else(|| panic!("creature should exist"))
            .raw
            .put_value("CustomField", GffValue::Int(1234))
            .unwrap_or_else(|error| panic!("insert custom field: {error}"));

        let root = build_git_root(&parsed).unwrap_or_else(|error| {
            panic!("build git root: {error}");
        });

        let creature_list = match root
            .root
            .get_field("Creature List")
            .map(|field| field.value())
        {
            Some(GffValue::List(creatures)) => creatures,
            other => panic!("expected creature list, got {other:?}"),
        };
        assert_eq!(
            creature_list
                .first()
                .and_then(|creature: &GffStruct| creature.get_field("CustomField"))
                .map(|field: &GffField| field.value()),
            Some(&GffValue::Int(1234))
        );
    }
}
