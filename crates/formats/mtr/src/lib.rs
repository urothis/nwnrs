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
use nwnrs_restype::prelude::*;
use tracing::instrument;

/// NWN resource type id for `mtr`.
pub const MTR_RES_TYPE: ResType = ResType(2072);

#[derive(Debug)]
/// Errors returned while reading or writing MTR payloads.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_mtr::MtrError>();
/// ```
pub enum MtrError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl MtrError {
    /// Creates a free-form MTR error message.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let err = nwnrs_mtr::MtrError::msg("something went wrong");
    /// ```
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for MtrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for MtrError {}

impl From<io::Error> for MtrError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for MtrError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for MTR operations.
pub type MtrResult<T> = Result<T, MtrError>;

#[derive(Debug, Clone, PartialEq)]
/// One typed MTR parameter row.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_mtr::MtrParameter>();
/// ```
pub struct MtrParameter {
    /// Parameter type token, usually `int` or `float`.
    pub param_type: String,
    /// Parameter values parsed as floats.
    pub values:     Vec<f32>,
}

#[derive(Debug, Clone, PartialEq)]
/// Parsed MTR material payload.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_mtr::MtrMaterial>();
/// ```
pub struct MtrMaterial {
    /// Optional `renderhint`.
    pub render_hint:      Option<String>,
    /// `textureN` entries keyed by slot index.
    pub textures:         BTreeMap<usize, String>,
    /// Named parameter rows.
    pub parameters:       BTreeMap<String, MtrParameter>,
    /// Optional custom vertex shader.
    pub custom_shader_vs: Option<String>,
    /// Optional custom geometry shader.
    pub custom_shader_gs: Option<String>,
    /// Optional custom fragment shader.
    pub custom_shader_fs: Option<String>,
}

impl MtrMaterial {
    /// Returns `texture0` when present.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_mtr::MtrMaterial::texture0;
    /// ```
    pub fn texture0(&self) -> Option<&str> {
        self.textures.get(&0).map(String::as_str)
    }

    /// Reads a typed MTR material from disk.
    ///
    /// # Errors
    ///
    /// Returns [`MtrError`] if the file cannot be opened or parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let material = nwnrs_mtr::MtrMaterial::from_file("material.mtr");
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> MtrResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_mtr(&mut file)
    }

    /// Reads a typed MTR material from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`MtrError`] if the resource is not an MTR type or the bytes
    /// cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_mtr::MtrMaterial::from_res;
    /// ```
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> MtrResult<Self> {
        if res.resref().res_type() != MTR_RES_TYPE {
            return Err(MtrError::msg(format!(
                "expected mtr resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        let text = String::from_utf8(bytes)
            .map_err(|error| MtrError::msg(format!("MTR payload is not valid UTF-8: {error}")))?;
        parse_mtr(&text)
    }
}

/// Reads a typed MTR material from `reader`.
///
/// # Errors
///
/// Returns [`MtrError`] if the data cannot be read or parsed.
///
/// # Examples
///
/// ```rust,no_run
/// let mut reader = std::io::Cursor::new(vec![]);
/// let material = nwnrs_mtr::read_mtr(&mut reader);
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn read_mtr<R: Read>(reader: &mut R) -> MtrResult<MtrMaterial> {
    let mut text = String::new();
    reader.read_to_string(&mut text)?;
    parse_mtr(&text)
}

/// Parses a typed MTR material from ASCII text.
///
/// # Errors
///
/// Returns [`MtrError`] if parsing fails.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = nwnrs_mtr::parse_mtr;
/// ```
pub fn parse_mtr(text: &str) -> MtrResult<MtrMaterial> {
    let mut material = MtrMaterial {
        render_hint:      None,
        textures:         BTreeMap::new(),
        parameters:       BTreeMap::new(),
        custom_shader_vs: None,
        custom_shader_gs: None,
        custom_shader_fs: None,
    };

    for raw_line in text.lines() {
        let line = strip_comment(raw_line).trim();
        if line.is_empty() {
            continue;
        }

        let parts = line.split_whitespace().collect::<Vec<_>>();
        let Some(label) = parts.first().copied() else {
            continue;
        };

        if label.eq_ignore_ascii_case("renderhint") {
            if let Some(value) = parts.get(1) {
                material.render_hint = Some((*value).to_string());
            }
        } else if label.eq_ignore_ascii_case("customshadervs") {
            if let Some(value) = parts.get(1) {
                material.custom_shader_vs = Some((*value).to_string());
            }
        } else if label.eq_ignore_ascii_case("customshadergs") {
            if let Some(value) = parts.get(1) {
                material.custom_shader_gs = Some((*value).to_string());
            }
        } else if label.eq_ignore_ascii_case("customshaderfs") {
            if let Some(value) = parts.get(1) {
                material.custom_shader_fs = Some((*value).to_string());
            }
        } else if let Some(index) = texture_index(label) {
            if let Some(value) = parts.get(1) {
                material.textures.insert(index, (*value).to_string());
            }
        } else if label.eq_ignore_ascii_case("parameter") {
            let Some(param_type) = parts.get(1) else {
                continue;
            };
            let Some(param_name) = parts.get(2) else {
                continue;
            };
            let values = parts
                .iter()
                .skip(3)
                .take(4)
                .filter_map(|value| value.parse::<f32>().ok())
                .collect::<Vec<_>>();
            material.parameters.insert(
                (*param_name).to_string(),
                MtrParameter {
                    param_type: (*param_type).to_string(),
                    values,
                },
            );
        }
    }

    Ok(material)
}

/// Writes a typed MTR material to `writer`.
///
/// # Errors
///
/// Returns [`MtrError`] if the write fails.
///
/// # Examples
///
/// ```rust,no_run
/// let mut writer = std::io::Cursor::new(vec![]);
/// let material = nwnrs_mtr::MtrMaterial::from_file("material.mtr").unwrap();
/// nwnrs_mtr::write_mtr(&mut writer, &material).unwrap();
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn write_mtr<W: Write>(writer: &mut W, material: &MtrMaterial) -> MtrResult<()> {
    if let Some(shader) = &material.custom_shader_vs {
        writeln!(writer, "customshaderVS {shader}")?;
    }
    if let Some(shader) = &material.custom_shader_fs {
        writeln!(writer, "customshaderFS {shader}")?;
    }
    if let Some(shader) = &material.custom_shader_gs {
        writeln!(writer, "customshaderGS {shader}")?;
    }
    if let Some(render_hint) = &material.render_hint {
        writeln!(writer, "renderhint {render_hint}")?;
    }
    for (index, texture) in &material.textures {
        writeln!(writer, "texture{index} {texture}")?;
    }
    for (name, parameter) in &material.parameters {
        write!(writer, "parameter {} {}", parameter.param_type, name)?;
        for value in &parameter.values {
            write!(writer, " {value}")?;
        }
        writeln!(writer)?;
    }
    Ok(())
}

fn strip_comment(line: &str) -> &str {
    if let Some((before, _comment)) = line.split_once("//") {
        before
    } else {
        line
    }
}

fn texture_index(label: &str) -> Option<usize> {
    let tail = label
        .strip_prefix("texture")
        .or_else(|| label.strip_prefix("TEXTURE"))?;
    if tail.is_empty() {
        return None;
    }
    tail.parse::<usize>().ok()
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        MTR_RES_TYPE, MtrError, MtrMaterial, MtrParameter, MtrResult, parse_mtr, read_mtr,
        write_mtr,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::{MTR_RES_TYPE, parse_mtr, read_mtr, write_mtr};

    #[test]
    fn parses_mtr_texture_and_shader_fields() {
        let material = parse_mtr(
            "\
// comment
renderhint NormalAndSpecMapped
texture0 StoneWall
texture1 StoneWall_n
parameter float Specularity 1.0 0.5
customshaderVS my_vs
customshaderFS my_fs
",
        )
        .unwrap_or_else(|error| panic!("parse mtr: {error}"));

        assert_eq!(MTR_RES_TYPE.0, 2072);
        assert_eq!(material.render_hint.as_deref(), Some("NormalAndSpecMapped"));
        assert_eq!(material.texture0(), Some("StoneWall"));
        assert_eq!(
            material.textures.get(&1).map(String::as_str),
            Some("StoneWall_n")
        );
        assert_eq!(
            material
                .parameters
                .get("Specularity")
                .map(|parameter| parameter.values.clone()),
            Some(vec![1.0, 0.5])
        );
        assert_eq!(material.custom_shader_vs.as_deref(), Some("my_vs"));
        assert_eq!(material.custom_shader_fs.as_deref(), Some("my_fs"));
    }

    #[test]
    fn roundtrips_typed_mtr_through_write_and_read() {
        let original = parse_mtr(
            "\
renderhint Simple
texture0 stone
parameter int Envmap 1
",
        )
        .unwrap_or_else(|error| panic!("parse original mtr: {error}"));

        let mut encoded = Vec::new();
        write_mtr(&mut encoded, &original).unwrap_or_else(|error| panic!("write mtr: {error}"));

        let mut cursor = Cursor::new(encoded);
        let decoded = read_mtr(&mut cursor).unwrap_or_else(|error| panic!("read mtr: {error}"));
        assert_eq!(decoded, original);
    }
}
