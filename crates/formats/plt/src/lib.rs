#![forbid(unsafe_code)]
#![doc = include_str!("../README.md")]

use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use nwnrs_resman::prelude::*;
use nwnrs_restype::prelude::*;
use tracing::instrument;

/// NWN resource type id for `plt`.
pub const PLT_RES_TYPE: ResType = ResType(6);
/// Fixed PLT magic/version tag.
pub const PLT_SIGNATURE: &[u8; 8] = b"PLT V1  ";
/// Size in bytes of the fixed PLT header.
pub const PLT_HEADER_SIZE: usize = 24;

#[derive(Debug)]
/// Errors returned while reading or writing PLT payloads.
pub enum PltError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl PltError {
    /// Creates a free-form PLT error message.
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for PltError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for PltError {}

impl From<io::Error> for PltError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for PltError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for PLT operations.
pub type PltResult<T> = Result<T, PltError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
/// Known PLT material layer ids.
pub enum PltLayer {
    /// Skin region.
    Skin = 0,
    /// Hair region.
    Hair = 1,
    /// First metal region.
    Metal1 = 2,
    /// Second metal region.
    Metal2 = 3,
    /// First cloth region.
    Cloth1 = 4,
    /// Second cloth region.
    Cloth2 = 5,
    /// First leather region.
    Leather1 = 6,
    /// Second leather region.
    Leather2 = 7,
    /// First tattoo region.
    Tattoo1 = 8,
    /// Second tattoo region.
    Tattoo2 = 9,
}

impl PltLayer {
    /// Resolves a known PLT layer id.
    #[must_use]
    pub fn from_id(value: u8) -> Option<Self> {
        match value {
            0 => Some(Self::Skin),
            1 => Some(Self::Hair),
            2 => Some(Self::Metal1),
            3 => Some(Self::Metal2),
            4 => Some(Self::Cloth1),
            5 => Some(Self::Cloth2),
            6 => Some(Self::Leather1),
            7 => Some(Self::Leather2),
            8 => Some(Self::Tattoo1),
            9 => Some(Self::Tattoo2),
            _ => None,
        }
    }

    /// Returns the on-disk layer id.
    #[must_use]
    pub fn id(self) -> u8 {
        self as u8
    }

    /// Returns a stable display label for the layer.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Skin => "skin",
            Self::Hair => "hair",
            Self::Metal1 => "metal1",
            Self::Metal2 => "metal2",
            Self::Cloth1 => "cloth1",
            Self::Cloth2 => "cloth2",
            Self::Leather1 => "leather1",
            Self::Leather2 => "leather2",
            Self::Tattoo1 => "tattoo1",
            Self::Tattoo2 => "tattoo2",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// One typed PLT pixel entry.
pub struct PltPixel {
    /// Per-pixel value byte from the file.
    pub value:    u8,
    /// Layer id byte for the pixel.
    pub layer_id: u8,
}

impl PltPixel {
    /// Resolves the pixel's layer id to a known PLT layer when possible.
    #[must_use]
    pub fn layer(self) -> Option<PltLayer> {
        PltLayer::from_id(self.layer_id)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Simple RGBA color policy for rendering a PLT into a final bitmap.
pub struct PltRenderSpec {
    /// RGBA colors keyed by known [`PltLayer`] id.
    pub layer_colors:        [[u8; 4]; 10],
    /// Fallback color for unknown layer ids.
    pub unknown_layer_color: [u8; 4],
}

impl Default for PltRenderSpec {
    fn default() -> Self {
        Self {
            layer_colors:        [
                [224, 191, 160, 255],
                [74, 52, 26, 255],
                [176, 184, 192, 255],
                [226, 168, 60, 255],
                [171, 47, 39, 255],
                [42, 86, 173, 255],
                [92, 62, 41, 255],
                [128, 92, 58, 255],
                [37, 152, 117, 255],
                [108, 64, 160, 255],
            ],
            unknown_layer_color: [255, 0, 255, 255],
        }
    }
}

impl PltRenderSpec {
    /// Returns the base RGBA color for one PLT layer id.
    #[must_use]
    pub fn color_for_layer_id(&self, layer_id: u8) -> [u8; 4] {
        self.layer_colors
            .get(usize::from(layer_id))
            .copied()
            .unwrap_or(self.unknown_layer_color)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Parsed PLT texture payload.
pub struct PltTexture {
    /// Four-byte file type tag, typically `PLT `.
    pub file_type:     [u8; 4],
    /// Four-byte version tag, typically `V1  `.
    pub file_version:  [u8; 4],
    /// First unused four-byte header field.
    pub unused1:       [u8; 4],
    /// Second unused four-byte header field.
    pub unused2:       [u8; 4],
    /// Image width in pixels.
    pub width:         u32,
    /// Image height in pixels.
    pub height:        u32,
    /// One typed entry per pixel.
    ///
    /// `value` corresponds to the VB source's luminance/value byte.
    /// `layer_id` selects the material layer for that pixel.
    pub pixels:        Vec<PltPixel>,
    /// Bytes stored after the pixel payload, if any.
    pub trailing_data: Vec<u8>,
}

impl PltTexture {
    /// Returns the total number of pixels declared by the image dimensions.
    pub fn pixel_count(&self) -> PltResult<usize> {
        usize::try_from(self.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| PltError::msg("PLT pixel count overflow"))
    }

    /// Returns the pixel entry at `(x, y)`.
    pub fn pixel_at(&self, x: u32, y: u32) -> PltResult<PltPixel> {
        if x >= self.width || y >= self.height {
            return Err(PltError::msg(format!(
                "PLT pixel coordinate out of range: ({x}, {y}) for {}x{}",
                self.width, self.height
            )));
        }
        let index = usize::try_from(y)
            .ok()
            .and_then(|row| {
                usize::try_from(self.width)
                    .ok()
                    .and_then(|stride| row.checked_mul(stride))
            })
            .and_then(|row| usize::try_from(x).ok().and_then(|col| row.checked_add(col)))
            .ok_or_else(|| PltError::msg("PLT pixel index overflow"))?;
        self.pixels
            .get(index)
            .copied()
            .ok_or_else(|| PltError::msg("PLT pixel index out of range"))
    }

    /// Parses a typed PLT texture directly from raw bytes.
    pub fn read_from_texture_bytes(bytes: &[u8]) -> PltResult<Self> {
        parse_plt_bytes(bytes)
    }

    /// Renders the PLT into RGBA8 pixels using the provided render spec.
    pub fn render_rgba8(&self, spec: &PltRenderSpec) -> PltResult<Vec<u8>> {
        let expected_pixels = self.pixel_count()?;
        if self.pixels.len() != expected_pixels {
            return Err(PltError::msg(format!(
                "PLT pixel buffer has {} entries but dimensions {}x{} require {}",
                self.pixels.len(),
                self.width,
                self.height,
                expected_pixels
            )));
        }

        let mut rgba = Vec::with_capacity(expected_pixels.saturating_mul(4));
        for pixel in &self.pixels {
            let [r, g, b, a] = spec.color_for_layer_id(pixel.layer_id);
            let value = u16::from(pixel.value);
            rgba.push(scale_channel(r, value));
            rgba.push(scale_channel(g, value));
            rgba.push(scale_channel(b, value));
            rgba.push(scale_channel(a, value));
        }
        Ok(rgba)
    }

    /// Reads a typed PLT texture from disk.
    pub fn from_file(path: impl AsRef<Path>) -> PltResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_plt(&mut file)
    }

    /// Reads a typed PLT texture from a [`Res`].
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> PltResult<Self> {
        if res.resref().res_type() != PLT_RES_TYPE {
            return Err(PltError::msg(format!(
                "expected plt resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        parse_plt_bytes(&bytes)
    }
}

/// Reads a typed PLT texture from `reader`.
#[instrument(level = "debug", skip_all, err)]
pub fn read_plt<R: Read>(reader: &mut R) -> PltResult<PltTexture> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_plt_bytes(&bytes)
}

/// Writes a typed PLT texture to `writer`.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(width = plt.width, height = plt.height, pixel_count = plt.pixels.len())
)]
pub fn write_plt<W: Write>(writer: &mut W, plt: &PltTexture) -> PltResult<()> {
    validate_writable_plt(plt)?;

    writer.write_all(&plt.file_type)?;
    writer.write_all(&plt.file_version)?;
    writer.write_all(&plt.unused1)?;
    writer.write_all(&plt.unused2)?;
    writer.write_all(&plt.width.to_le_bytes())?;
    writer.write_all(&plt.height.to_le_bytes())?;
    for pixel in &plt.pixels {
        writer.write_all(&[pixel.value, pixel.layer_id])?;
    }
    writer.write_all(&plt.trailing_data)?;
    Ok(())
}

fn parse_plt_bytes(bytes: &[u8]) -> PltResult<PltTexture> {
    if bytes.len() < PLT_HEADER_SIZE {
        return Err(PltError::msg(format!(
            "PLT payload too small: expected at least {PLT_HEADER_SIZE} bytes, got {}",
            bytes.len()
        )));
    }

    let signature = bytes
        .get(..PLT_SIGNATURE.len())
        .ok_or_else(|| PltError::msg("PLT signature extends past end of file"))?;
    if signature != PLT_SIGNATURE {
        return Err(PltError::msg(format!(
            "unsupported PLT signature: {signature:?}"
        )));
    }

    let header = bytes
        .get(..PLT_HEADER_SIZE)
        .ok_or_else(|| PltError::msg("PLT header extends past end of file"))?;
    let file_type = <[u8; 4]>::try_from(
        header
            .get(0..4)
            .ok_or_else(|| PltError::msg("PLT file type out of range"))?,
    )
    .map_err(|_error| PltError::msg("PLT file type out of range"))?;
    let file_version = <[u8; 4]>::try_from(
        header
            .get(4..8)
            .ok_or_else(|| PltError::msg("PLT file version out of range"))?,
    )
    .map_err(|_error| PltError::msg("PLT file version out of range"))?;
    let unused1 = <[u8; 4]>::try_from(
        header
            .get(8..12)
            .ok_or_else(|| PltError::msg("PLT unused1 out of range"))?,
    )
    .map_err(|_error| PltError::msg("PLT unused1 out of range"))?;
    let unused2 = <[u8; 4]>::try_from(
        header
            .get(12..16)
            .ok_or_else(|| PltError::msg("PLT unused2 out of range"))?,
    )
    .map_err(|_error| PltError::msg("PLT unused2 out of range"))?;
    let width = read_u32_at(header, 16)?;
    let height = read_u32_at(header, 20)?;
    let pixel_count = usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .ok_or_else(|| PltError::msg("PLT pixel count overflow"))?;
    let payload_len = pixel_count
        .checked_mul(2)
        .ok_or_else(|| PltError::msg("PLT payload length overflow"))?;

    let payload = bytes
        .get(PLT_HEADER_SIZE..PLT_HEADER_SIZE + payload_len)
        .ok_or_else(|| PltError::msg("PLT pixel payload extends past end of file"))?;
    let mut pixels = Vec::with_capacity(pixel_count);
    for entry in payload.chunks_exact(2) {
        let [value, layer_id] = <[u8; 2]>::try_from(entry)
            .map_err(|_error| PltError::msg("PLT pixel entry length mismatch"))?;
        pixels.push(PltPixel {
            value,
            layer_id,
        });
    }

    let trailing_data = bytes
        .get(PLT_HEADER_SIZE + payload_len..)
        .ok_or_else(|| PltError::msg("PLT trailing data extends past end of file"))?
        .to_vec();

    Ok(PltTexture {
        file_type,
        file_version,
        unused1,
        unused2,
        width,
        height,
        pixels,
        trailing_data,
    })
}

fn validate_writable_plt(plt: &PltTexture) -> PltResult<()> {
    let expected_pixels = plt.pixel_count()?;
    if plt.pixels.len() != expected_pixels {
        return Err(PltError::msg(format!(
            "PLT expected {expected_pixels} pixels for {}x{}, got {}",
            plt.width,
            plt.height,
            plt.pixels.len()
        )));
    }
    Ok(())
}

fn read_u32_at(bytes: &[u8], offset: usize) -> PltResult<u32> {
    let quad = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| PltError::msg(format!("PLT u32 field out of range at byte {offset}")))?;
    let [a, b, c, d] = <[u8; 4]>::try_from(quad)
        .map_err(|_error| PltError::msg(format!("PLT u32 field out of range at byte {offset}")))?;
    Ok(u32::from_le_bytes([a, b, c, d]))
}

fn scale_channel(channel: u8, value: u16) -> u8 {
    let scaled = (u16::from(channel) * value) / 255;
    u8::try_from(scaled).unwrap_or(u8::MAX)
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        PLT_HEADER_SIZE, PLT_RES_TYPE, PLT_SIGNATURE, PltError, PltLayer, PltPixel, PltRenderSpec,
        PltResult, PltTexture, read_plt, write_plt,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{error::Error, io::Cursor};

    use nwnrs_resman::CachePolicy;
    use nwnrs_test_support::{
        demand_resource, read_resource_bytes, require_game_resource,
        skip_if_game_resources_unavailable,
    };

    use crate::{
        PLT_HEADER_SIZE, PLT_RES_TYPE, PLT_SIGNATURE, PltLayer, PltPixel, PltRenderSpec,
        PltTexture, read_plt, write_plt,
    };

    #[test]
    fn fixture_plt_parses_expected_header_fields() -> Result<(), Box<dyn Error>> {
        let res = match require_game_resource(demand_resource("cloak_001", PLT_RES_TYPE)) {
            Ok(res) => res,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let plt = PltTexture::from_res(&res, CachePolicy::Use).unwrap_or_else(|error| {
            panic!("read shipped plt fixture: {error}");
        });

        assert_eq!(PLT_SIGNATURE, b"PLT V1  ");
        assert_eq!(PLT_HEADER_SIZE, 24);
        assert_eq!(PLT_RES_TYPE.0, 6);
        assert_eq!(plt.file_type, *b"PLT ");
        assert_eq!(plt.file_version, *b"V1  ");
        assert_eq!(plt.unused1, [10, 0, 0, 0]);
        assert_eq!(plt.unused2, [0, 0, 0, 0]);
        assert_eq!(plt.width, 512);
        assert_eq!(plt.height, 512);
        assert_eq!(plt.pixels.len(), 512 * 512);
        assert_eq!(
            plt.pixels.first(),
            Some(&PltPixel {
                value:    71,
                layer_id: 5,
            })
        );
        assert_eq!(
            plt.pixels.first().copied().and_then(PltPixel::layer),
            Some(PltLayer::Cloth2)
        );
        assert!(plt.trailing_data.is_empty());
        Ok(())
    }

    #[test]
    fn write_plt_roundtrips_fixture_bytes() -> Result<(), Box<dyn Error>> {
        let original = match require_game_resource(read_resource_bytes("cloak_001", PLT_RES_TYPE)) {
            Ok(bytes) => bytes,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let mut cursor = Cursor::new(original.clone());
        let plt = read_plt(&mut cursor).unwrap_or_else(|error| {
            panic!("parse fixture plt: {error}");
        });

        let mut encoded = Vec::new();
        if let Err(error) = write_plt(&mut encoded, &plt) {
            panic!("write plt: {error}");
        }

        assert_eq!(encoded, original);
        Ok(())
    }

    #[test]
    fn manual_plt_roundtrips_through_read_and_write() {
        let original = PltTexture {
            file_type:     *b"PLT ",
            file_version:  *b"V1  ",
            unused1:       [10, 0, 0, 0],
            unused2:       [0, 0, 0, 0],
            width:         2,
            height:        2,
            pixels:        vec![
                PltPixel {
                    value:    1,
                    layer_id: 3,
                },
                PltPixel {
                    value:    2,
                    layer_id: 5,
                },
                PltPixel {
                    value:    3,
                    layer_id: 5,
                },
                PltPixel {
                    value:    4,
                    layer_id: 3,
                },
            ],
            trailing_data: vec![0xaa, 0xbb],
        };

        let mut encoded = Vec::new();
        if let Err(error) = write_plt(&mut encoded, &original) {
            panic!("write manual plt: {error}");
        }

        let mut cursor = Cursor::new(encoded);
        let decoded = read_plt(&mut cursor).unwrap_or_else(|error| {
            panic!("read manual plt: {error}");
        });

        assert_eq!(decoded, original);
    }

    #[test]
    fn known_layer_ids_match_plttools_source() {
        assert_eq!(PltLayer::Skin.id(), 0);
        assert_eq!(PltLayer::Hair.id(), 1);
        assert_eq!(PltLayer::Metal1.id(), 2);
        assert_eq!(PltLayer::Metal2.id(), 3);
        assert_eq!(PltLayer::Cloth1.id(), 4);
        assert_eq!(PltLayer::Cloth2.id(), 5);
        assert_eq!(PltLayer::Leather1.id(), 6);
        assert_eq!(PltLayer::Leather2.id(), 7);
        assert_eq!(PltLayer::Tattoo1.id(), 8);
        assert_eq!(PltLayer::Tattoo2.id(), 9);
        assert_eq!(PltLayer::from_id(10), None);
    }

    #[test]
    fn render_rgba8_modulates_default_layer_color_by_value() {
        let plt = PltTexture {
            file_type:     *b"PLT ",
            file_version:  *b"V1  ",
            unused1:       [10, 0, 0, 0],
            unused2:       [0, 0, 0, 0],
            width:         2,
            height:        1,
            pixels:        vec![
                PltPixel {
                    value:    255,
                    layer_id: PltLayer::Cloth1.id(),
                },
                PltPixel {
                    value:    128,
                    layer_id: 99,
                },
            ],
            trailing_data: Vec::new(),
        };

        let rendered = plt
            .render_rgba8(&PltRenderSpec::default())
            .unwrap_or_else(|error| panic!("render plt: {error}"));

        assert_eq!(rendered.get(0..4), Some(&[171, 47, 39, 255][..]));
        assert_eq!(rendered.get(4..8), Some(&[128, 0, 128, 128][..]));
    }
}
