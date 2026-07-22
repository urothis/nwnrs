use std::{
    fmt,
    fs::File,
    io::{self, Read, Seek},
    path::Path,
};

use nwnrs_types::resman::{CachePolicy, Res, ResMan, ResManError, ResType, ResolvedResRef};
use tracing::instrument;

use crate::gff::{GffRoot, GffStruct, GffValue, read_gff_root};

/// NWN resource type id for an area definition (`ARE`).
pub const ARE_RES_TYPE: ResType = ResType(2012);
/// NWN resource type id for module information (`IFO`).
pub const IFO_RES_TYPE: ResType = ResType(2014);

/// Errors returned while reading scene-facing ARE and IFO projections.
#[derive(Debug)]
pub enum AreaError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// GFF decoding failed.
    Gff(crate::gff::GffError),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was invalid for its declared resource type.
    Message(String),
}

impl AreaError {
    fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for AreaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Gff(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for AreaError {}

impl From<io::Error> for AreaError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<crate::gff::GffError> for AreaError {
    fn from(value: crate::gff::GffError) -> Self {
        Self::Gff(value)
    }
}

impl From<ResManError> for AreaError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for area and module projections.
pub type AreaResult<T> = Result<T, AreaError>;

/// Scene-facing environmental settings preserved from an ARE resource.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AreEnvironment {
    /// Whether the authored day/night cycle is enabled.
    pub day_night_cycle:    Option<bool>,
    /// Whether the area is currently authored as night.
    pub is_night:           Option<bool>,
    /// Lighting-scheme row.
    pub lighting_scheme:    Option<i32>,
    /// Fog clipping distance.
    pub fog_clip_distance:  Option<f32>,
    /// Sun ambient packed color.
    pub sun_ambient_color:  Option<u32>,
    /// Sun diffuse packed color.
    pub sun_diffuse_color:  Option<u32>,
    /// Sun fog packed color.
    pub sun_fog_color:      Option<u32>,
    /// Sun fog amount.
    pub sun_fog_amount:     Option<i32>,
    /// Whether sun shadows are enabled.
    pub sun_shadows:        Option<bool>,
    /// Moon ambient packed color.
    pub moon_ambient_color: Option<u32>,
    /// Moon diffuse packed color.
    pub moon_diffuse_color: Option<u32>,
    /// Moon fog packed color.
    pub moon_fog_color:     Option<u32>,
    /// Moon fog amount.
    pub moon_fog_amount:    Option<i32>,
    /// Whether moon shadows are enabled.
    pub moon_shadows:       Option<bool>,
    /// Skybox row or identifier.
    pub skybox:             Option<i32>,
    /// Wind-power setting.
    pub wind_power:         Option<i32>,
    /// Shadow opacity.
    pub shadow_opacity:     Option<i32>,
    /// Rain probability.
    pub chance_rain:        Option<i32>,
    /// Snow probability.
    pub chance_snow:        Option<i32>,
    /// Lightning probability.
    pub chance_lightning:   Option<i32>,
}

/// One tile placement in an ARE tile grid.
#[derive(Debug, Clone, PartialEq)]
pub struct AreTile {
    /// Original tile GFF structure, including fields not interpreted here.
    pub raw:             GffStruct,
    /// Source-order index in `Tile_List`.
    pub index:           usize,
    /// Grid X coordinate derived from the declared area width.
    pub x:               usize,
    /// Grid Y coordinate derived from the declared area width.
    pub y:               usize,
    /// Tileset tile id.
    pub tile_id:         Option<i32>,
    /// Vertical tile step.
    pub height:          Option<i32>,
    /// Quarter-turn orientation.
    pub orientation:     Option<i32>,
    /// Three authored animation-loop toggles.
    pub animation_loops: [Option<bool>; 3],
    /// Two authored main-light palette indices.
    pub main_lights:     [Option<i32>; 2],
    /// Two authored source-light palette indices.
    pub source_lights:   [Option<i32>; 2],
}

/// Read-only, lossless scene-facing projection of an ARE resource.
///
/// `raw` retains the complete GFF document. Typed fields provide the data
/// needed by area assembly without making the renderer understand GFF labels.
#[derive(Debug, Clone, PartialEq)]
pub struct AreFile {
    /// Original complete GFF root.
    pub raw:         GffRoot,
    /// Area resource reference.
    pub resref:      Option<String>,
    /// Area tag.
    pub tag:         Option<String>,
    /// Declared tile-grid width.
    pub width:       usize,
    /// Declared tile-grid height.
    pub height:      usize,
    /// Referenced tileset name.
    pub tileset:     Option<String>,
    /// Tile placements in source order.
    pub tiles:       Vec<AreTile>,
    /// Environment and lighting settings.
    pub environment: AreEnvironment,
}

impl AreFile {
    /// Reads an ARE resource from disk.
    ///
    /// # Errors
    ///
    /// Returns [`AreaError`] when the file cannot be opened or parsed.
    pub fn from_file(path: impl AsRef<Path>) -> AreaResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_are(&mut file)
    }

    /// Reads an ARE resource from a resource container.
    ///
    /// # Errors
    ///
    /// Returns [`AreaError`] when the resource type or payload is invalid.
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> AreaResult<Self> {
        if res.resref().res_type() != ARE_RES_TYPE {
            return Err(AreaError::msg(format!(
                "expected are resource, got {}",
                res.resref()
            )));
        }
        let bytes = res.read_all(cache_policy)?;
        read_are(&mut io::Cursor::new(bytes))
    }

    /// Resolves and reads an ARE resource by name.
    ///
    /// # Errors
    ///
    /// Returns [`AreaError`] when the resource cannot be found or parsed.
    pub fn from_resman(
        resman: &mut ResMan,
        area_name: &str,
        cache_policy: CachePolicy,
    ) -> AreaResult<Self> {
        let resolved = ResolvedResRef::from_filename(&format!("{area_name}.are"))
            .map_err(|error| AreaError::msg(format!("ARE resref: {error}")))?;
        let resource = resman
            .get_resolved(&resolved)
            .ok_or_else(|| AreaError::msg(format!("ARE not found in ResMan: {resolved}")))?;
        Self::from_res(&resource, cache_policy)
    }
}

/// Module entry position and facing from an IFO resource.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ModuleEntryPoint {
    /// Entry area resource reference.
    pub area:      Option<String>,
    /// Entry position.
    pub position:  [Option<f32>; 3],
    /// Entry facing vector in the XY plane.
    pub direction: [Option<f32>; 2],
}

/// Read-only, lossless scene-facing projection of a module IFO resource.
#[derive(Debug, Clone, PartialEq)]
pub struct ModuleInfo {
    /// Original complete GFF root.
    pub raw:        GffRoot,
    /// Areas declared by the module in authored order.
    pub areas:      Vec<String>,
    /// Module entry location.
    pub entry:      ModuleEntryPoint,
    /// Custom TLK name, when configured.
    pub custom_tlk: Option<String>,
    /// Module HAK names in authored order.
    pub haks:       Vec<String>,
}

impl ModuleInfo {
    /// Reads an IFO resource from disk.
    ///
    /// # Errors
    ///
    /// Returns [`AreaError`] when the file cannot be opened or parsed.
    pub fn from_file(path: impl AsRef<Path>) -> AreaResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_module_info(&mut file)
    }

    /// Reads an IFO resource from a resource container.
    ///
    /// # Errors
    ///
    /// Returns [`AreaError`] when the resource type or payload is invalid.
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> AreaResult<Self> {
        if res.resref().res_type() != IFO_RES_TYPE {
            return Err(AreaError::msg(format!(
                "expected ifo resource, got {}",
                res.resref()
            )));
        }
        let bytes = res.read_all(cache_policy)?;
        read_module_info(&mut io::Cursor::new(bytes))
    }

    /// Resolves and reads an IFO resource by name.
    ///
    /// # Errors
    ///
    /// Returns [`AreaError`] when the resource cannot be found or parsed.
    pub fn from_resman(
        resman: &mut ResMan,
        module_name: &str,
        cache_policy: CachePolicy,
    ) -> AreaResult<Self> {
        let resolved = ResolvedResRef::from_filename(&format!("{module_name}.ifo"))
            .map_err(|error| AreaError::msg(format!("IFO resref: {error}")))?;
        let resource = resman
            .get_resolved(&resolved)
            .ok_or_else(|| AreaError::msg(format!("IFO not found in ResMan: {resolved}")))?;
        Self::from_res(&resource, cache_policy)
    }
}

/// Reads an ARE projection from a binary GFF reader.
///
/// # Errors
///
/// Returns [`AreaError`] when decoding or projection fails.
#[instrument(level = "debug", skip_all, err)]
pub fn read_are<R: Read + Seek>(reader: &mut R) -> AreaResult<AreFile> {
    parse_are_root(&read_gff_root(reader)?)
}

/// Projects a decoded ARE GFF root into scene-facing typed data.
///
/// # Errors
///
/// Returns [`AreaError`] when the GFF root is not an ARE resource or declares
/// impossible dimensions.
pub fn parse_are_root(root: &GffRoot) -> AreaResult<AreFile> {
    if root.file_type != "ARE " {
        return Err(AreaError::msg(format!(
            "expected ARE root, got {:?}",
            root.file_type
        )));
    }
    let width = usize_value(&root.root, "Width").unwrap_or_default();
    let height = usize_value(&root.root, "Height").unwrap_or_default();
    let tiles = list(&root.root, "Tile_List")
        .unwrap_or_default()
        .iter()
        .enumerate()
        .map(|(index, tile)| AreTile {
            raw: tile.clone(),
            index,
            x: if width == 0 { 0 } else { index % width },
            y: index.checked_div(width).unwrap_or(index),
            tile_id: integer(tile, "Tile_ID"),
            height: integer(tile, "Tile_Height"),
            orientation: integer(tile, "Tile_Orientation"),
            animation_loops: [
                boolean(tile, "Tile_AnimLoop1"),
                boolean(tile, "Tile_AnimLoop2"),
                boolean(tile, "Tile_AnimLoop3"),
            ],
            main_lights: [
                integer(tile, "Tile_MainLight1"),
                integer(tile, "Tile_MainLight2"),
            ],
            source_lights: [
                integer(tile, "Tile_SrcLight1"),
                integer(tile, "Tile_SrcLight2"),
            ],
        })
        .collect::<Vec<_>>();
    let declared_tile_count = width
        .checked_mul(height)
        .ok_or_else(|| AreaError::msg(format!("ARE dimensions overflow: {width} by {height}")))?;
    if declared_tile_count != 0 && tiles.len() != declared_tile_count {
        return Err(AreaError::msg(format!(
            "ARE declares {width} by {height} tiles but Tile_List contains {} entries",
            tiles.len()
        )));
    }

    Ok(AreFile {
        raw: root.clone(),
        resref: string(&root.root, "ResRef"),
        tag: string(&root.root, "Tag"),
        width,
        height,
        tileset: string(&root.root, "Tileset"),
        tiles,
        environment: AreEnvironment {
            day_night_cycle:    boolean(&root.root, "DayNightCycle"),
            is_night:           boolean(&root.root, "IsNight"),
            lighting_scheme:    integer(&root.root, "LightingScheme"),
            fog_clip_distance:  float(&root.root, "FogClipDist"),
            sun_ambient_color:  unsigned(&root.root, "SunAmbientColor"),
            sun_diffuse_color:  unsigned(&root.root, "SunDiffuseColor"),
            sun_fog_color:      unsigned(&root.root, "SunFogColor"),
            sun_fog_amount:     integer(&root.root, "SunFogAmount"),
            sun_shadows:        boolean(&root.root, "SunShadows"),
            moon_ambient_color: unsigned(&root.root, "MoonAmbientColor"),
            moon_diffuse_color: unsigned(&root.root, "MoonDiffuseColor"),
            moon_fog_color:     unsigned(&root.root, "MoonFogColor"),
            moon_fog_amount:    integer(&root.root, "MoonFogAmount"),
            moon_shadows:       boolean(&root.root, "MoonShadows"),
            skybox:             integer(&root.root, "SkyBox"),
            wind_power:         integer(&root.root, "WindPower"),
            shadow_opacity:     integer(&root.root, "ShadowOpacity"),
            chance_rain:        integer(&root.root, "ChanceRain"),
            chance_snow:        integer(&root.root, "ChanceSnow"),
            chance_lightning:   integer(&root.root, "ChanceLightning"),
        },
    })
}

/// Reads a module IFO projection from a binary GFF reader.
///
/// # Errors
///
/// Returns [`AreaError`] when decoding or projection fails.
#[instrument(level = "debug", skip_all, err)]
pub fn read_module_info<R: Read + Seek>(reader: &mut R) -> AreaResult<ModuleInfo> {
    parse_module_info_root(&read_gff_root(reader)?)
}

/// Projects a decoded module IFO GFF root into scene-facing typed data.
///
/// # Errors
///
/// Returns [`AreaError`] when the root is not an IFO resource.
pub fn parse_module_info_root(root: &GffRoot) -> AreaResult<ModuleInfo> {
    if root.file_type != "IFO " {
        return Err(AreaError::msg(format!(
            "expected IFO root, got {:?}",
            root.file_type
        )));
    }
    let areas = list(&root.root, "Mod_Area_list")
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| string(entry, "Area_Name"))
        .collect();
    let haks = list(&root.root, "Mod_HakList")
        .unwrap_or_default()
        .iter()
        .filter_map(|entry| string_any(entry, &["Mod_Hak", "Hak", "Name"]))
        .collect();
    Ok(ModuleInfo {
        raw: root.clone(),
        areas,
        entry: ModuleEntryPoint {
            area:      string(&root.root, "Mod_Entry_Area"),
            position:  [
                float(&root.root, "Mod_Entry_X"),
                float(&root.root, "Mod_Entry_Y"),
                float(&root.root, "Mod_Entry_Z"),
            ],
            direction: [
                float(&root.root, "Mod_Entry_Dir_X"),
                float(&root.root, "Mod_Entry_Dir_Y"),
            ],
        },
        custom_tlk: string(&root.root, "Mod_CustomTlk"),
        haks,
    })
}

fn list<'a>(value: &'a GffStruct, label: &str) -> Option<&'a [GffStruct]> {
    match value.get_field(label)?.value() {
        GffValue::List(entries) => Some(entries),
        _ => None,
    }
}

fn string(value: &GffStruct, label: &str) -> Option<String> {
    string_any(value, &[label])
}

fn string_any(value: &GffStruct, labels: &[&str]) -> Option<String> {
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

fn integer(value: &GffStruct, label: &str) -> Option<i32> {
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

fn unsigned(value: &GffStruct, label: &str) -> Option<u32> {
    match value.get_field(label)?.value() {
        GffValue::Byte(raw) => Some(u32::from(*raw)),
        GffValue::Char(raw) => u32::try_from(*raw).ok(),
        GffValue::Word(raw) => Some(u32::from(*raw)),
        GffValue::Short(raw) => u32::try_from(*raw).ok(),
        GffValue::Dword(raw) => Some(*raw),
        GffValue::Int(raw) => u32::try_from(*raw).ok(),
        _ => None,
    }
}

fn usize_value(value: &GffStruct, label: &str) -> Option<usize> {
    integer(value, label).and_then(|raw| usize::try_from(raw).ok())
}

fn boolean(value: &GffStruct, label: &str) -> Option<bool> {
    integer(value, label).map(|raw| raw != 0)
}

#[allow(clippy::cast_precision_loss)]
fn float(value: &GffStruct, label: &str) -> Option<f32> {
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

#[cfg(test)]
mod tests {
    use crate::gff::{GffRoot, GffStruct, GffValue, parse_are_root, parse_module_info_root};

    #[test]
    fn projects_area_grid_and_environment() {
        let mut root = GffRoot::new("ARE ");
        root.put_value("Width", GffValue::Int(2))
            .unwrap_or_else(|error| panic!("width: {error}"));
        root.put_value("Height", GffValue::Int(1))
            .unwrap_or_else(|error| panic!("height: {error}"));
        root.put_value("Tileset", GffValue::ResRef("tno01".into()))
            .unwrap_or_else(|error| panic!("tileset: {error}"));
        root.put_value("IsNight", GffValue::Byte(1))
            .unwrap_or_else(|error| panic!("night: {error}"));
        let mut first = GffStruct::new(1);
        first
            .put_value("Tile_ID", GffValue::Int(7))
            .unwrap_or_else(|error| panic!("tile id: {error}"));
        let mut second = GffStruct::new(1);
        second
            .put_value("Tile_ID", GffValue::Int(8))
            .unwrap_or_else(|error| panic!("tile id: {error}"));
        root.put_value("Tile_List", GffValue::List(vec![first, second]))
            .unwrap_or_else(|error| panic!("tile list: {error}"));

        let area = parse_are_root(&root).unwrap_or_else(|error| panic!("parse ARE: {error}"));
        assert_eq!((area.width, area.height), (2, 1));
        let second = area
            .tiles
            .get(1)
            .unwrap_or_else(|| panic!("second tile must be present"));
        assert_eq!((second.x, second.y), (1, 0));
        assert_eq!(second.tile_id, Some(8));
        assert_eq!(area.environment.is_night, Some(true));
    }

    #[test]
    fn projects_module_area_list_and_entry_point() {
        let mut root = GffRoot::new("IFO ");
        let mut area = GffStruct::new(6);
        area.put_value("Area_Name", GffValue::ResRef("start".into()))
            .unwrap_or_else(|error| panic!("area name: {error}"));
        root.put_value("Mod_Area_list", GffValue::List(vec![area]))
            .unwrap_or_else(|error| panic!("area list: {error}"));
        root.put_value("Mod_Entry_Area", GffValue::ResRef("start".into()))
            .unwrap_or_else(|error| panic!("entry area: {error}"));
        root.put_value("Mod_Entry_X", GffValue::Float(3.5))
            .unwrap_or_else(|error| panic!("entry x: {error}"));

        let info =
            parse_module_info_root(&root).unwrap_or_else(|error| panic!("parse IFO: {error}"));
        assert_eq!(info.areas, ["start"]);
        assert_eq!(info.entry.area.as_deref(), Some("start"));
        assert_eq!(info.entry.position[0], Some(3.5));
    }
}
