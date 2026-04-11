#![forbid(unsafe_code)]
//! Typed parser for Neverwinter Nights texture info (`TXI`) resources.
//!
//! `TXI` files are line-oriented text sidecars attached to textures. They are
//! used for material-like rendering hints such as procedural water settings,
//! bump/env map bindings, and numeric channel transforms.

use std::{
    fmt,
    fs::File,
    io::{self, Read},
    path::Path,
};

use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::ResolvedResRef;
use nwnrs_restype::prelude::*;
use tracing::instrument;

/// NWN resource type id for `txi`.
pub const TXI_RES_TYPE: ResType = ResType(2022);

/// Errors returned while reading or parsing `TXI` payloads.
#[derive(Debug)]
pub enum TxiError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl TxiError {
    /// Creates a free-form `TXI` error message.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for TxiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TxiError {}

impl From<io::Error> for TxiError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for TxiError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for `TXI` operations.
pub type TxiResult<T> = Result<T, TxiError>;

/// Parsed texture-info payload.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TxiFile {
    /// Parsed directives in source order.
    pub directives:           Vec<TxiDirective>,
    /// `proceduretype`
    pub procedure_type:       Option<String>,
    /// `bumpmaptexture`
    pub bump_map_texture:     Option<String>,
    /// `bumpyshinytexture`
    pub bumpy_shiny_texture:  Option<String>,
    /// `channelscale`
    pub channel_scale:        Option<Vec<f32>>,
    /// `channeltranslate`
    pub channel_translate:    Option<Vec<f32>>,
    /// `distort`
    pub distort:              Option<i32>,
    /// `arturowidth`
    pub arturo_width:         Option<i32>,
    /// `arturoheight`
    pub arturo_height:        Option<i32>,
    /// `distortionamplitude`
    pub distortion_amplitude: Option<f32>,
    /// `speed`
    pub speed:                Option<f32>,
    /// `defaultheight`
    pub default_height:       Option<i32>,
    /// `defaultwidth`
    pub default_width:        Option<i32>,
    /// `alphamean`
    pub alpha_mean:           Option<f32>,
}

impl TxiFile {
    /// Returns the first directive named `name`, case-insensitively.
    pub fn directive(&self, name: &str) -> Option<&TxiDirective> {
        self.directives
            .iter()
            .find(|directive| directive.name.eq_ignore_ascii_case(name))
    }
}

/// One parsed TXI directive.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TxiDirective {
    /// Directive keyword as authored.
    pub name:          String,
    /// Inline tokens on the directive line after the keyword.
    pub arguments:     Vec<String>,
    /// Continuation lines attached to this directive.
    pub continuations: Vec<String>,
}

impl TxiDirective {
    /// Returns the first argument when present.
    pub fn first_argument(&self) -> Option<&str> {
        self.arguments.first().map(String::as_str)
    }

    /// Parses the directive as a counted float block like `channeltranslate 4`.
    pub fn counted_f32_values(&self) -> Option<Vec<f32>> {
        let count = self.first_argument()?.parse::<usize>().ok()?;
        let values = self
            .arguments
            .iter()
            .skip(1)
            .chain(self.continuations.iter())
            .take(count)
            .map(|value| value.parse::<f32>().ok())
            .collect::<Option<Vec<_>>>()?;
        (values.len() == count).then_some(values)
    }
}

/// Reads a typed `TXI` payload from any reader.
#[instrument(level = "debug", skip_all, err)]
pub fn read_txi(reader: &mut dyn Read) -> TxiResult<TxiFile> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    parse_txi(&text)
}

/// Reads a typed `TXI` payload from disk.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_txi_from_file(path: impl AsRef<Path>) -> TxiResult<TxiFile> {
    let mut file = File::open(path.as_ref())?;
    read_txi(&mut file)
}

/// Reads a typed `TXI` payload from a [`Res`].
#[instrument(level = "debug", skip_all, err, fields(resref = %res.resref(), use_cache))]
pub fn read_txi_from_res(res: &Res, use_cache: bool) -> TxiResult<TxiFile> {
    if res.resref().res_type() != TXI_RES_TYPE {
        return Err(TxiError::msg(format!(
            "expected txi resource, got {}",
            res.resref()
        )));
    }
    let bytes = res.read_all(use_cache)?;
    let text = String::from_utf8(bytes)
        .map_err(|error| TxiError::msg(format!("TXI payload is not valid UTF-8: {error}")))?;
    parse_txi(&text)
}

/// Reads a typed `TXI` payload from a [`ResMan`] by texture name.
#[instrument(level = "debug", skip_all, err, fields(name, use_cache))]
pub fn read_txi_from_resman(
    resman: &mut ResMan,
    name: &str,
    use_cache: bool,
) -> TxiResult<TxiFile> {
    let resolved = ResolvedResRef::new(name.to_string(), TXI_RES_TYPE)
        .map_err(|error| TxiError::msg(format!("invalid txi resref {name}: {error}")))?;
    let res = resman
        .get_resolved(&resolved)
        .ok_or_else(|| TxiError::msg(format!("txi not found in ResMan: {resolved}")))?;
    read_txi_from_res(&res, use_cache)
}

/// Reads an optional typed `TXI` payload from a [`ResMan`] by texture name.
///
/// Missing sidecars are normal in NWN content, so this returns `Ok(None)`
/// when no `.txi` exists. Invalid resrefs or malformed payloads still return
/// an error.
#[instrument(level = "debug", skip_all, fields(name, use_cache))]
pub fn read_optional_txi_from_resman(
    resman: &mut ResMan,
    name: &str,
    use_cache: bool,
) -> TxiResult<Option<TxiFile>> {
    let resolved = ResolvedResRef::new(name.to_string(), TXI_RES_TYPE)
        .map_err(|error| TxiError::msg(format!("invalid txi resref {name}: {error}")))?;
    let Some(res) = resman.get_resolved(&resolved) else {
        return Ok(None);
    };
    read_txi_from_res(&res, use_cache).map(Some)
}

/// Parses a typed `TXI` payload from text.
pub fn parse_txi(text: &str) -> TxiResult<TxiFile> {
    let mut directives = Vec::new();
    for (line_index, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("//")
            || trimmed.starts_with(';')
        {
            continue;
        }

        if starts_new_directive(trimmed) {
            let mut parts = trimmed.split_whitespace();
            let Some(name) = parts.next() else {
                continue;
            };
            directives.push(TxiDirective {
                name:          name.to_string(),
                arguments:     parts.map(ToOwned::to_owned).collect(),
                continuations: Vec::new(),
            });
            continue;
        }

        let Some(last) = directives.last_mut() else {
            return Err(TxiError::msg(format!(
                "unexpected continuation before any directive on line {}",
                line_index + 1
            )));
        };
        last.continuations.push(trimmed.to_string());
    }

    Ok(TxiFile {
        procedure_type: first_argument_value(&directives, "proceduretype"),
        bump_map_texture: first_argument_value(&directives, "bumpmaptexture"),
        bumpy_shiny_texture: first_argument_value(&directives, "bumpyshinytexture"),
        channel_scale: first_counted_f32_values(&directives, "channelscale"),
        channel_translate: first_counted_f32_values(&directives, "channeltranslate"),
        distort: first_i32_value(&directives, "distort"),
        arturo_width: first_i32_value(&directives, "arturowidth"),
        arturo_height: first_i32_value(&directives, "arturoheight"),
        distortion_amplitude: first_f32_value(&directives, "distortionamplitude"),
        speed: first_f32_value(&directives, "speed"),
        default_height: first_i32_value(&directives, "defaultheight"),
        default_width: first_i32_value(&directives, "defaultwidth"),
        alpha_mean: first_f32_value(&directives, "alphamean"),
        directives,
    })
}

fn starts_new_directive(line: &str) -> bool {
    line.chars()
        .next()
        .is_some_and(|ch| ch.is_ascii_alphabetic())
}

fn first_argument_value(directives: &[TxiDirective], name: &str) -> Option<String> {
    directives
        .iter()
        .find(|directive| directive.name.eq_ignore_ascii_case(name))
        .and_then(|directive| directive.first_argument().map(ToOwned::to_owned))
}

fn first_counted_f32_values(directives: &[TxiDirective], name: &str) -> Option<Vec<f32>> {
    directives
        .iter()
        .find(|directive| directive.name.eq_ignore_ascii_case(name))
        .and_then(TxiDirective::counted_f32_values)
}

fn first_i32_value(directives: &[TxiDirective], name: &str) -> Option<i32> {
    directives
        .iter()
        .find(|directive| directive.name.eq_ignore_ascii_case(name))
        .and_then(TxiDirective::first_argument)
        .and_then(|value| value.parse::<i32>().ok())
}

fn first_f32_value(directives: &[TxiDirective], name: &str) -> Option<f32> {
    directives
        .iter()
        .find(|directive| directive.name.eq_ignore_ascii_case(name))
        .and_then(TxiDirective::first_argument)
        .and_then(|value| value.parse::<f32>().ok())
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        TXI_RES_TYPE, TxiDirective, TxiError, TxiFile, TxiResult, parse_txi,
        read_optional_txi_from_resman, read_txi, read_txi_from_file, read_txi_from_res,
        read_txi_from_resman,
    };
}

#[cfg(test)]
mod tests {
    use nwnrs_resman::{ResContainer, ResMan};
    use nwnrs_resmemfile::prelude::read_resmemfile;
    use nwnrs_resref::prelude::ResolvedResRef;

    use super::{TXI_RES_TYPE, parse_txi, read_optional_txi_from_resman, read_txi_from_resman};

    #[test]
    fn parses_water_txi_directives_and_channel_blocks() {
        let parsed = parse_txi(
            "\
// shiny water
bumpyshinytexture ttr01__env
bumpmaptexture shinywater

proceduretype arturo
channelscale 4
0
0
0
0
channeltranslate 4
0
0.25
0.5
0.75
",
        )
        .unwrap_or_else(|error| {
            panic!("parse txi: {error}");
        });

        assert_eq!(parsed.procedure_type.as_deref(), Some("arturo"));
        assert_eq!(parsed.bump_map_texture.as_deref(), Some("shinywater"));
        assert_eq!(parsed.bumpy_shiny_texture.as_deref(), Some("ttr01__env"));
        assert_eq!(parsed.channel_scale, Some(vec![0.0, 0.0, 0.0, 0.0]));
        assert_eq!(parsed.channel_translate, Some(vec![0.0, 0.25, 0.5, 0.75]));
        assert_eq!(parsed.distort, None);
        assert_eq!(parsed.directives.len(), 5);
    }

    #[test]
    fn reads_txi_from_resman() {
        let resolved =
            ResolvedResRef::new("water01".to_string(), TXI_RES_TYPE).unwrap_or_else(|error| {
                panic!("resolve txi resref: {error}");
            });
        let container = read_resmemfile(
            "txi".to_string(),
            resolved.into(),
            b"proceduretype arturo\nchanneltranslate 2\n0\n1\n".to_vec(),
        )
        .unwrap_or_else(|error| {
            panic!("build txi memfile: {error}");
        });
        let mut resman = ResMan::new(0);
        resman.add(std::sync::Arc::new(container) as std::sync::Arc<dyn ResContainer>);

        let parsed = read_txi_from_resman(&mut resman, "water01", true).unwrap_or_else(|error| {
            panic!("read txi from resman: {error}");
        });

        assert_eq!(parsed.procedure_type.as_deref(), Some("arturo"));
        assert_eq!(parsed.channel_translate, Some(vec![0.0, 1.0]));
    }

    #[test]
    fn missing_txi_from_resman_is_optional() {
        let mut resman = ResMan::new(0);
        let parsed = read_optional_txi_from_resman(&mut resman, "missing_water", true)
            .unwrap_or_else(|error| {
                panic!("read optional txi from resman: {error}");
            });
        assert!(parsed.is_none());
    }

    #[test]
    fn parses_single_value_water_controls() {
        let parsed = parse_txi(
            "\
distort 1
arturowidth 32
arturoheight 32
distortionamplitude 6
speed 20
defaultheight 64
defaultwidth 64
alphamean 0.999
",
        )
        .unwrap_or_else(|error| {
            panic!("parse txi controls: {error}");
        });

        assert_eq!(parsed.distort, Some(1));
        assert_eq!(parsed.arturo_width, Some(32));
        assert_eq!(parsed.arturo_height, Some(32));
        assert_eq!(parsed.distortion_amplitude, Some(6.0));
        assert_eq!(parsed.speed, Some(20.0));
        assert_eq!(parsed.default_height, Some(64));
        assert_eq!(parsed.default_width, Some(64));
        assert_eq!(parsed.alpha_mean, Some(0.999));
    }
}
