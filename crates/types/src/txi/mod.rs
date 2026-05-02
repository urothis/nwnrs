#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use nwnrs_types::resman::prelude::*;
use tracing::instrument;

/// NWN resource type id for `txi`.
pub const TXI_RES_TYPE: ResType = ResType(2022);

/// Errors returned while reading or parsing `TXI` payloads.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::txi::TxiError>();
/// ```
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
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let error = nwnrs_types::txi::TxiError::msg("bad txi");
    /// assert_eq!(error.to_string(), "bad txi");
    /// ```
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
///
/// [`TxiFile::directives`] preserves the directive stream in source order. The
/// dedicated typed fields are convenience views over recognized directives;
/// they do not replace the original directive list. Serialization therefore
/// treats [`TxiFile::directives`] as the authoritative representation when it
/// is non-empty.
///
/// # Examples
///
/// ```rust,no_run
/// let txi = nwnrs_types::txi::TxiFile::default();
/// assert!(txi.directives.is_empty());
/// ```
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
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiFile::directive;
    /// ```
    #[must_use]
    pub fn directive(&self, name: &str) -> Option<&TxiDirective> {
        self.directives
            .iter()
            .find(|directive| directive.name.eq_ignore_ascii_case(name))
    }

    /// Reads a typed `TXI` payload from disk.
    ///
    /// # Errors
    ///
    /// Returns [`TxiError`] if the file cannot be opened or parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiFile::from_file(std::path::Path::new("texture.txi"));
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> TxiResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_txi(&mut file)
    }

    /// Reads a typed `TXI` payload from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`TxiError`] if the resource is not a TXI type or the bytes
    /// cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiFile::from_res;
    /// ```
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> TxiResult<Self> {
        if res.resref().res_type() != TXI_RES_TYPE {
            return Err(TxiError::msg(format!(
                "expected txi resource, got {}",
                res.resref()
            )));
        }
        let bytes = res.read_all(cache_policy)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| TxiError::msg(format!("TXI payload is not valid UTF-8: {error}")))?;
        parse_txi(&text)
    }

    /// Reads a typed `TXI` payload from a [`ResMan`] by texture name.
    ///
    /// # Errors
    ///
    /// Returns [`TxiError`] if the resref is invalid, the resource is not
    /// found, or parsing fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiFile::from_resman;
    /// ```
    pub fn from_resman(
        resman: &mut ResMan,
        name: &str,
        cache_policy: CachePolicy,
    ) -> TxiResult<Self> {
        let resolved = ResolvedResRef::new(name.to_string(), TXI_RES_TYPE)
            .map_err(|error| TxiError::msg(format!("invalid txi resref {name}: {error}")))?;
        let res = resman
            .get_resolved(&resolved)
            .ok_or_else(|| TxiError::msg(format!("txi not found in ResMan: {resolved}")))?;
        Self::from_res(&res, cache_policy)
    }

    /// Reads an optional typed `TXI` payload from a [`ResMan`] by texture name.
    ///
    /// # Errors
    ///
    /// Returns [`TxiError`] if the resref is invalid or parsing fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiFile::optional_from_resman;
    /// ```
    pub fn optional_from_resman(
        resman: &mut ResMan,
        name: &str,
        cache_policy: CachePolicy,
    ) -> TxiResult<Option<Self>> {
        let resolved = ResolvedResRef::new(name.to_string(), TXI_RES_TYPE)
            .map_err(|error| TxiError::msg(format!("invalid txi resref {name}: {error}")))?;
        let Some(res) = resman.get_resolved(&resolved) else {
            return Ok(None);
        };
        Self::from_res(&res, cache_policy).map(Some)
    }
}

/// One parsed TXI directive.
///
/// # Examples
///
/// ```rust,no_run
/// let directive = nwnrs_types::txi::TxiDirective::default();
/// assert!(directive.arguments.is_empty());
/// ```
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
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiDirective::first_argument;
    /// ```
    pub fn first_argument(&self) -> Option<&str> {
        self.arguments.first().map(String::as_str)
    }

    /// Parses the directive as a counted float block like `channeltranslate 4`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::txi::TxiDirective::counted_f32_values;
    /// ```
    #[must_use]
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
///
/// # Errors
///
/// Returns [`TxiError`] if the data cannot be read or parsed.
///
/// # Examples
///
/// ```rust,no_run
/// let mut reader = std::io::Cursor::new(Vec::<u8>::new());
/// let _ = nwnrs_types::txi::read_txi(&mut reader);
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn read_txi(reader: &mut dyn Read) -> TxiResult<TxiFile> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    parse_txi(&text)
}

/// Parses a typed `TXI` payload from text.
///
/// # Errors
///
/// Returns [`TxiError`] if a continuation line appears before any directive.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::txi::parse_txi;
/// ```
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

/// Builds deterministic `TXI` text from a typed [`TxiFile`].
///
/// When [`TxiFile::directives`] is non-empty, the directive stream is emitted
/// directly in preserved order. When no directives are present, the serializer
/// synthesizes a directive stream from the recognized typed fields in a stable
/// order.
///
/// # Errors
///
/// This function currently always succeeds but returns [`TxiResult`] for
/// forward compatibility.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_types::txi::build_txi_text;
/// ```
pub fn build_txi_text(txi_file: &TxiFile) -> TxiResult<String> {
    let directives = if txi_file.directives.is_empty() {
        synthesize_directives(txi_file)
    } else {
        txi_file.directives.clone()
    };

    let mut text = String::new();
    for (index, directive) in directives.iter().enumerate() {
        if index > 0 {
            text.push('\n');
        }
        text.push_str(&directive.name);
        if !directive.arguments.is_empty() {
            text.push(' ');
            text.push_str(&directive.arguments.join(" "));
        }
        text.push('\n');
        for continuation in &directive.continuations {
            text.push_str(continuation);
            text.push('\n');
        }
    }

    Ok(text)
}

/// Writes deterministic `TXI` text to `writer`.
///
/// # Errors
///
/// Returns [`TxiError`] if the write fails.
///
/// # Examples
///
/// ```rust,no_run
/// let txi_file = nwnrs_types::txi::TxiFile::default();
/// let mut writer = Vec::new();
/// nwnrs_types::txi::write_txi(&mut writer, &txi_file)?;
/// # Ok::<(), nwnrs_types::txi::TxiError>(())
/// ```
pub fn write_txi<W: Write>(writer: &mut W, txi_file: &TxiFile) -> TxiResult<()> {
    let text = build_txi_text(txi_file)?;
    writer.write_all(text.as_bytes())?;
    Ok(())
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

fn synthesize_directives(txi_file: &TxiFile) -> Vec<TxiDirective> {
    let mut directives = Vec::new();
    push_scalar_directive(
        &mut directives,
        "proceduretype",
        txi_file.procedure_type.as_deref(),
    );
    push_scalar_directive(
        &mut directives,
        "bumpmaptexture",
        txi_file.bump_map_texture.as_deref(),
    );
    push_scalar_directive(
        &mut directives,
        "bumpyshinytexture",
        txi_file.bumpy_shiny_texture.as_deref(),
    );
    push_counted_f32_directive(
        &mut directives,
        "channelscale",
        txi_file.channel_scale.as_deref(),
    );
    push_counted_f32_directive(
        &mut directives,
        "channeltranslate",
        txi_file.channel_translate.as_deref(),
    );
    push_scalar_directive_from_display(&mut directives, "distort", txi_file.distort);
    push_scalar_directive_from_display(&mut directives, "arturowidth", txi_file.arturo_width);
    push_scalar_directive_from_display(&mut directives, "arturoheight", txi_file.arturo_height);
    push_scalar_directive_from_display(
        &mut directives,
        "distortionamplitude",
        txi_file.distortion_amplitude,
    );
    push_scalar_directive_from_display(&mut directives, "speed", txi_file.speed);
    push_scalar_directive_from_display(&mut directives, "defaultheight", txi_file.default_height);
    push_scalar_directive_from_display(&mut directives, "defaultwidth", txi_file.default_width);
    push_scalar_directive_from_display(&mut directives, "alphamean", txi_file.alpha_mean);
    directives
}

fn push_scalar_directive(directives: &mut Vec<TxiDirective>, name: &str, value: Option<&str>) {
    let Some(value) = value else {
        return;
    };
    directives.push(TxiDirective {
        name:          name.to_string(),
        arguments:     vec![value.to_string()],
        continuations: Vec::new(),
    });
}

fn push_scalar_directive_from_display<T: fmt::Display + Copy>(
    directives: &mut Vec<TxiDirective>,
    name: &str,
    value: Option<T>,
) {
    let Some(value) = value else {
        return;
    };
    directives.push(TxiDirective {
        name:          name.to_string(),
        arguments:     vec![value.to_string()],
        continuations: Vec::new(),
    });
}

fn push_counted_f32_directive(
    directives: &mut Vec<TxiDirective>,
    name: &str,
    values: Option<&[f32]>,
) {
    let Some(values) = values else {
        return;
    };
    directives.push(TxiDirective {
        name:          name.to_string(),
        arguments:     vec![values.len().to_string()],
        continuations: values.iter().map(ToString::to_string).collect(),
    });
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::txi::{
        TXI_RES_TYPE, TxiDirective, TxiError, TxiFile, TxiResult, build_txi_text, parse_txi,
        read_txi, write_txi,
    };
}

#[cfg(test)]
mod tests {
    use nwnrs_types::resman::{
        CachePolicy, ResContainer, ResMan, prelude::ResolvedResRef, read_resmemfile,
    };

    use super::{TXI_RES_TYPE, TxiDirective, TxiFile, build_txi_text, parse_txi, write_txi};

    #[test]
    fn parses_water_txi_directives_and_channel_blocks() {
        let parsed_result = parse_txi(
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
        );
        assert!(
            parsed_result.is_ok(),
            "parse txi: {:?}",
            parsed_result.as_ref().err()
        );
        let parsed = match parsed_result {
            Ok(parsed) => parsed,
            Err(_error) => return,
        };

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
        let resolved_result = ResolvedResRef::new("water01".to_string(), TXI_RES_TYPE);
        assert!(
            resolved_result.is_ok(),
            "resolve txi resref: {:?}",
            resolved_result.as_ref().err()
        );
        let resolved = match resolved_result {
            Ok(resolved) => resolved,
            Err(_error) => return,
        };
        let container_result = read_resmemfile(
            "txi".to_string(),
            resolved.into(),
            b"proceduretype arturo\nchanneltranslate 2\n0\n1\n".to_vec(),
        );
        assert!(
            container_result.is_ok(),
            "build txi memfile: {:?}",
            container_result.as_ref().err()
        );
        let container = match container_result {
            Ok(container) => container,
            Err(_error) => return,
        };
        let mut resman = ResMan::new(0);
        resman.add(std::sync::Arc::new(container) as std::sync::Arc<dyn ResContainer>);

        let parsed_result = TxiFile::from_resman(&mut resman, "water01", CachePolicy::Use);
        assert!(
            parsed_result.is_ok(),
            "read txi from resman: {:?}",
            parsed_result.as_ref().err()
        );
        let parsed = match parsed_result {
            Ok(parsed) => parsed,
            Err(_error) => return,
        };

        assert_eq!(parsed.procedure_type.as_deref(), Some("arturo"));
        assert_eq!(parsed.channel_translate, Some(vec![0.0, 1.0]));
    }

    #[test]
    fn missing_txi_from_resman_is_optional() {
        let mut resman = ResMan::new(0);
        let parsed_result =
            TxiFile::optional_from_resman(&mut resman, "missing_water", CachePolicy::Use);
        assert!(
            parsed_result.is_ok(),
            "read optional txi from resman: {:?}",
            parsed_result.as_ref().err()
        );
        let parsed = match parsed_result {
            Ok(parsed) => parsed,
            Err(_error) => return,
        };
        assert!(parsed.is_none());
    }

    #[test]
    fn parses_single_value_water_controls() {
        let parsed_result = parse_txi(
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
        );
        assert!(
            parsed_result.is_ok(),
            "parse txi controls: {:?}",
            parsed_result.as_ref().err()
        );
        let parsed = match parsed_result {
            Ok(parsed) => parsed,
            Err(_error) => return,
        };

        assert_eq!(parsed.distort, Some(1));
        assert_eq!(parsed.arturo_width, Some(32));
        assert_eq!(parsed.arturo_height, Some(32));
        assert_eq!(parsed.distortion_amplitude, Some(6.0));
        assert_eq!(parsed.speed, Some(20.0));
        assert_eq!(parsed.default_height, Some(64));
        assert_eq!(parsed.default_width, Some(64));
        assert_eq!(parsed.alpha_mean, Some(0.999));
    }

    #[test]
    fn builds_and_reparses_directive_stream() {
        let original_result = parse_txi(
            "\
bumpyshinytexture ttr01__env
bumpmaptexture shinywater
proceduretype arturo
channeltranslate 4
0
0.25
0.5
0.75
",
        );
        assert!(
            original_result.is_ok(),
            "parse txi: {:?}",
            original_result.as_ref().err()
        );
        let original = match original_result {
            Ok(original) => original,
            Err(_error) => return,
        };

        let built_result = build_txi_text(&original);
        assert!(
            built_result.is_ok(),
            "build txi: {:?}",
            built_result.as_ref().err()
        );
        let built = match built_result {
            Ok(built) => built,
            Err(_error) => return,
        };
        let reparsed_result = parse_txi(&built);
        assert!(
            reparsed_result.is_ok(),
            "reparse txi: {:?}",
            reparsed_result.as_ref().err()
        );
        let reparsed = match reparsed_result {
            Ok(reparsed) => reparsed,
            Err(_error) => return,
        };
        assert_eq!(reparsed, original);
    }

    #[test]
    fn synthesizes_directives_from_typed_fields_when_stream_is_empty() {
        let txi = TxiFile {
            procedure_type: Some("arturo".to_string()),
            bump_map_texture: Some("shinywater".to_string()),
            channel_scale: Some(vec![0.0, 1.0]),
            distort: Some(1),
            speed: Some(20.0),
            ..TxiFile::default()
        };

        let built_result = build_txi_text(&txi);
        assert!(
            built_result.is_ok(),
            "build txi: {:?}",
            built_result.as_ref().err()
        );
        let built = match built_result {
            Ok(built) => built,
            Err(_error) => return,
        };
        assert!(built.contains("proceduretype arturo"));
        assert!(built.contains("channelscale 2\n0\n1\n"));

        let reparsed_result = parse_txi(&built);
        assert!(
            reparsed_result.is_ok(),
            "reparse txi: {:?}",
            reparsed_result.as_ref().err()
        );
        let reparsed = match reparsed_result {
            Ok(reparsed) => reparsed,
            Err(_error) => return,
        };
        assert_eq!(reparsed.procedure_type, txi.procedure_type);
        assert_eq!(reparsed.bump_map_texture, txi.bump_map_texture);
        assert_eq!(reparsed.channel_scale, txi.channel_scale);
        assert_eq!(reparsed.distort, txi.distort);
        assert_eq!(reparsed.speed, txi.speed);
    }

    #[test]
    fn directive_stream_is_authoritative_when_present() {
        let txi = TxiFile {
            directives: vec![TxiDirective {
                name:          "proceduretype".to_string(),
                arguments:     vec!["cycle".to_string()],
                continuations: Vec::new(),
            }],
            procedure_type: Some("arturo".to_string()),
            ..TxiFile::default()
        };

        let built_result = build_txi_text(&txi);
        assert!(
            built_result.is_ok(),
            "build txi: {:?}",
            built_result.as_ref().err()
        );
        let built = match built_result {
            Ok(built) => built,
            Err(_error) => return,
        };
        assert_eq!(built, "proceduretype cycle\n");
    }

    #[test]
    fn write_txi_matches_build_text() {
        let txi = TxiFile {
            procedure_type: Some("arturo".to_string()),
            alpha_mean: Some(0.999),
            ..TxiFile::default()
        };

        let built_result = build_txi_text(&txi);
        assert!(
            built_result.is_ok(),
            "build txi: {:?}",
            built_result.as_ref().err()
        );
        let built = match built_result {
            Ok(built) => built,
            Err(_error) => return,
        };
        let mut bytes = Vec::new();
        let write_result = write_txi(&mut bytes, &txi);
        assert!(
            write_result.is_ok(),
            "write txi: {:?}",
            write_result.as_ref().err()
        );
        let written_result = String::from_utf8(bytes);
        assert!(
            written_result.is_ok(),
            "utf8 write txi: {:?}",
            written_result.as_ref().err()
        );
        let written = match written_result {
            Ok(written) => written,
            Err(_error) => return,
        };
        assert_eq!(written, built);
    }
}
