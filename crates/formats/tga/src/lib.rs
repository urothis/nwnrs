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

/// NWN resource type id for `tga`.
pub const TGA_RES_TYPE: ResType = ResType(3);
/// Size in bytes of the fixed TGA header.
pub const TGA_HEADER_SIZE: usize = 18;

const TGA_FOOTER_SIZE: usize = 26;
const TGA_FOOTER_SIGNATURE: &[u8; 18] = b"TRUEVISION-XFILE.\0";

#[derive(Debug)]
/// Errors returned while reading, decoding, or writing TGA textures.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_tga::TgaError>();
/// ```
pub enum TgaError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl TgaError {
    /// Creates a free-form TGA error message.
    ///
    /// # Examples
    ///
    /// ```rust
    /// let err = nwnrs_tga::TgaError::msg("something went wrong");
    /// ```
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for TgaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TgaError {}

impl From<io::Error> for TgaError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for TgaError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for TGA operations.
pub type TgaResult<T> = Result<T, TgaError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// TGA image storage mode.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_tga::TgaImageType>();
/// ```
pub enum TgaImageType {
    /// No image data is present.
    NoImage,
    /// Indexed pixels using a color map.
    ColorMapped,
    /// Direct-color pixels.
    TrueColor,
    /// Monochrome pixels.
    BlackAndWhite,
    /// RLE-compressed indexed pixels.
    RleColorMapped,
    /// RLE-compressed direct-color pixels.
    RleTrueColor,
    /// RLE-compressed monochrome pixels.
    RleBlackAndWhite,
}

impl TgaImageType {
    fn from_byte(value: u8) -> TgaResult<Self> {
        match value {
            0 => Ok(Self::NoImage),
            1 => Ok(Self::ColorMapped),
            2 => Ok(Self::TrueColor),
            3 => Ok(Self::BlackAndWhite),
            9 => Ok(Self::RleColorMapped),
            10 => Ok(Self::RleTrueColor),
            11 => Ok(Self::RleBlackAndWhite),
            _ => Err(TgaError::msg(format!(
                "unsupported TGA image type: {value}"
            ))),
        }
    }

    /// Returns `true` when the TGA image data uses RLE packets.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaImageType::is_rle;
    /// ```
    #[must_use]
    pub fn is_rle(self) -> bool {
        matches!(
            self,
            Self::RleColorMapped | Self::RleTrueColor | Self::RleBlackAndWhite
        )
    }

    /// Returns `true` when the TGA image data references a color map.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaImageType::uses_color_map;
    /// ```
    #[must_use]
    pub fn uses_color_map(self) -> bool {
        matches!(self, Self::ColorMapped | Self::RleColorMapped)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Optional TGA 2.0 footer descriptor.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_tga::TgaFooter>();
/// ```
pub struct TgaFooter {
    /// Offset to the extension area, when present.
    pub extension_area_offset:      u32,
    /// Offset to the developer directory, when present.
    pub developer_directory_offset: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Parsed TGA texture payload.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_tga::TgaTexture>();
/// ```
pub struct TgaTexture {
    /// Length of the optional image ID section.
    pub id_length: u8,
    /// Color map presence flag from the header.
    pub color_map_type: u8,
    /// Parsed image storage mode.
    pub image_type: TgaImageType,
    /// First color-map entry index.
    pub color_map_first_entry_index: u16,
    /// Number of color-map entries.
    pub color_map_length: u16,
    /// Bits per color-map entry.
    pub color_map_entry_size: u8,
    /// X origin from the TGA header.
    pub x_origin: u16,
    /// Y origin from the TGA header.
    pub y_origin: u16,
    /// Image width in pixels.
    pub width: u16,
    /// Image height in pixels.
    pub height: u16,
    /// Bits per image pixel.
    pub pixel_depth: u8,
    /// Image descriptor byte.
    pub image_descriptor: u8,
    /// Raw image ID payload.
    pub image_id: Vec<u8>,
    /// Raw color-map payload.
    pub color_map_data: Vec<u8>,
    /// Raw image-data payload, still compressed when the image type uses RLE.
    pub image_data: Vec<u8>,
    /// Bytes stored after image data and before the optional footer.
    pub trailing_data: Vec<u8>,
    /// Optional TGA 2.0 footer.
    pub footer: Option<TgaFooter>,
}

impl TgaTexture {
    /// Encodes top-left-origin RGBA8 pixels into an uncompressed 32-bit TGA.
    ///
    /// # Errors
    ///
    /// Returns [`TgaError`] if `rgba` does not match the expected length for
    /// the given dimensions.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::encode_rgba8;
    /// ```
    pub fn encode_rgba8(width: u16, height: u16, rgba: &[u8]) -> TgaResult<Self> {
        let expected_len = rgba_len(width, height)?;
        if rgba.len() != expected_len {
            return Err(TgaError::msg(format!(
                "TGA RGBA input expected {expected_len} bytes, got {}",
                rgba.len()
            )));
        }

        let mut image_data = Vec::with_capacity(expected_len);
        for pixel in rgba.chunks_exact(4) {
            let [r, g, b, a] = <[u8; 4]>::try_from(pixel)
                .map_err(|_error| TgaError::msg("TGA RGBA pixel slice length mismatch"))?;
            image_data.extend_from_slice(&[b, g, r, a]);
        }

        Ok(Self {
            id_length: 0,
            color_map_type: 0,
            image_type: TgaImageType::TrueColor,
            color_map_first_entry_index: 0,
            color_map_length: 0,
            color_map_entry_size: 0,
            x_origin: 0,
            y_origin: 0,
            width,
            height,
            pixel_depth: 32,
            image_descriptor: 0x28,
            image_id: Vec::new(),
            color_map_data: Vec::new(),
            image_data,
            trailing_data: Vec::new(),
            footer: None,
        })
    }

    /// Returns the total number of pixels.
    ///
    /// # Errors
    ///
    /// Returns [`TgaError`] if the pixel count overflows `usize`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::pixel_count;
    /// ```
    pub fn pixel_count(&self) -> TgaResult<usize> {
        usize::from(self.width)
            .checked_mul(usize::from(self.height))
            .ok_or_else(|| TgaError::msg("TGA pixel count overflow"))
    }

    /// Returns `true` when the origin bit marks rows as top-to-bottom.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::top_to_bottom;
    /// ```
    #[must_use]
    pub fn top_to_bottom(&self) -> bool {
        self.image_descriptor & 0x20 != 0
    }

    /// Returns `true` when the origin bit marks pixels as right-to-left.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::right_to_left;
    /// ```
    #[must_use]
    pub fn right_to_left(&self) -> bool {
        self.image_descriptor & 0x10 != 0
    }

    /// Returns the number of attribute bits declared in the descriptor.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::attribute_bits;
    /// ```
    #[must_use]
    pub fn attribute_bits(&self) -> u8 {
        self.image_descriptor & 0x0f
    }

    /// Returns the stored image payload expanded into raw pixel bytes.
    ///
    /// # Errors
    ///
    /// Returns [`TgaError`] if the pixel depth is unsupported or the image data
    /// is malformed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::expanded_image_data;
    /// ```
    pub fn expanded_image_data(&self) -> TgaResult<Vec<u8>> {
        let bytes_per_pixel = pixel_size_bytes(self.pixel_depth)?;
        let pixel_count = self.pixel_count()?;
        if self.image_type.is_rle() {
            expand_rle_image_data(&self.image_data, pixel_count, bytes_per_pixel)
        } else {
            let expected_len = pixel_count
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA image data length overflow"))?;
            if self.image_data.len() != expected_len {
                return Err(TgaError::msg(format!(
                    "expected {expected_len} bytes of TGA image data, got {}",
                    self.image_data.len()
                )));
            }
            Ok(self.image_data.clone())
        }
    }

    /// Decodes the TGA image into top-left-origin RGBA8 pixels.
    ///
    /// # Errors
    ///
    /// Returns [`TgaError`] if the image type is unsupported or the pixel data
    /// is malformed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::decode_rgba8;
    /// ```
    #[allow(clippy::many_single_char_names)]
    pub fn decode_rgba8(&self) -> TgaResult<Vec<u8>> {
        if self.image_type.uses_color_map() {
            return Err(TgaError::msg(
                "TGA color-mapped decode is not implemented yet",
            ));
        }

        let pixel_count = self.pixel_count()?;
        let width = usize::from(self.width);
        let height = usize::from(self.height);
        let expanded = self.expanded_image_data()?;
        let bytes_per_pixel = pixel_size_bytes(self.pixel_depth)?;
        let mut rgba = vec![
            0_u8;
            pixel_count
                .checked_mul(4)
                .ok_or_else(|| TgaError::msg("TGA RGBA length overflow"))?
        ];

        for idx in 0..pixel_count {
            let src = idx
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA source index overflow"))?;
            let pixel = expanded
                .get(src..src + bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA pixel slice out of range"))?;
            let [r, g, b, a] = decode_tga_pixel(pixel, self.pixel_depth, self.image_type)?;

            let row = idx / width;
            let column = idx % width;
            let x = if self.right_to_left() {
                width.saturating_sub(1).saturating_sub(column)
            } else {
                column
            };
            let y = if self.top_to_bottom() {
                row
            } else {
                height.saturating_sub(1).saturating_sub(row)
            };
            let dst = (y
                .checked_mul(width)
                .and_then(|value| value.checked_add(x))
                .and_then(|value| value.checked_mul(4)))
            .ok_or_else(|| TgaError::msg("TGA destination index overflow"))?;
            let out = rgba
                .get_mut(dst..dst + 4)
                .ok_or_else(|| TgaError::msg("TGA output slice out of range"))?;
            out.copy_from_slice(&[r, g, b, a]);
        }

        Ok(rgba)
    }

    /// Reads a typed TGA texture from disk.
    ///
    /// # Errors
    ///
    /// Returns [`TgaError`] if the file cannot be opened or parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let texture = nwnrs_tga::TgaTexture::from_file("texture.tga");
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> TgaResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_tga(&mut file)
    }

    /// Reads a typed TGA texture from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`TgaError`] if the resource is not a TGA type or the bytes
    /// cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_tga::TgaTexture::from_res;
    /// ```
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> TgaResult<Self> {
        if res.resref().res_type() != TGA_RES_TYPE {
            return Err(TgaError::msg(format!(
                "expected tga resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        parse_tga_bytes(&bytes)
    }
}

/// Reads a typed TGA texture from `reader`.
///
/// # Errors
///
/// Returns [`TgaError`] if the data cannot be read or does not conform to the
/// TGA format.
///
/// # Examples
///
/// ```rust,no_run
/// let mut reader = std::io::Cursor::new(vec![]);
/// let texture = nwnrs_tga::read_tga(&mut reader);
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn read_tga<R: Read>(reader: &mut R) -> TgaResult<TgaTexture> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_tga_bytes(&bytes)
}

/// Writes a typed TGA texture to `writer`.
///
/// # Errors
///
/// Returns [`io::Error`] if the write fails.
///
/// # Examples
///
/// ```rust,no_run
/// let mut writer = std::io::Cursor::new(vec![]);
/// let texture = nwnrs_tga::TgaTexture::from_file("texture.tga").unwrap();
/// nwnrs_tga::write_tga(&mut writer, &texture).unwrap();
/// ```
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(width = tga.width, height = tga.height, pixel_depth = tga.pixel_depth)
)]
pub fn write_tga<W: Write>(writer: &mut W, tga: &TgaTexture) -> io::Result<()> {
    writer.write_all(&[tga.id_length])?;
    writer.write_all(&[tga.color_map_type])?;
    writer.write_all(&[encode_image_type(tga.image_type)])?;
    writer.write_all(&tga.color_map_first_entry_index.to_le_bytes())?;
    writer.write_all(&tga.color_map_length.to_le_bytes())?;
    writer.write_all(&[tga.color_map_entry_size])?;
    writer.write_all(&tga.x_origin.to_le_bytes())?;
    writer.write_all(&tga.y_origin.to_le_bytes())?;
    writer.write_all(&tga.width.to_le_bytes())?;
    writer.write_all(&tga.height.to_le_bytes())?;
    writer.write_all(&[tga.pixel_depth])?;
    writer.write_all(&[tga.image_descriptor])?;
    writer.write_all(&tga.image_id)?;
    writer.write_all(&tga.color_map_data)?;
    writer.write_all(&tga.image_data)?;
    writer.write_all(&tga.trailing_data)?;
    if let Some(footer) = &tga.footer {
        writer.write_all(&footer.extension_area_offset.to_le_bytes())?;
        writer.write_all(&footer.developer_directory_offset.to_le_bytes())?;
        writer.write_all(TGA_FOOTER_SIGNATURE)?;
    }
    Ok(())
}

fn parse_tga_bytes(bytes: &[u8]) -> TgaResult<TgaTexture> {
    if bytes.len() < TGA_HEADER_SIZE {
        return Err(TgaError::msg(format!(
            "TGA payload too small: expected at least {TGA_HEADER_SIZE} bytes, got {}",
            bytes.len()
        )));
    }

    let header = bytes
        .get(..TGA_HEADER_SIZE)
        .ok_or_else(|| TgaError::msg("TGA header extends past end of file"))?;
    let id_length = *header
        .first()
        .ok_or_else(|| TgaError::msg("TGA header missing id_length"))?;
    let color_map_type = *header
        .get(1)
        .ok_or_else(|| TgaError::msg("TGA header missing color_map_type"))?;
    let image_type = TgaImageType::from_byte(
        *header
            .get(2)
            .ok_or_else(|| TgaError::msg("TGA header missing image_type"))?,
    )?;
    let color_map_first_entry_index = read_u16_at(header, 3)?;
    let color_map_length = read_u16_at(header, 5)?;
    let color_map_entry_size = *header
        .get(7)
        .ok_or_else(|| TgaError::msg("TGA header missing color_map_entry_size"))?;
    let x_origin = read_u16_at(header, 8)?;
    let y_origin = read_u16_at(header, 10)?;
    let width = read_u16_at(header, 12)?;
    let height = read_u16_at(header, 14)?;
    let pixel_depth = *header
        .get(16)
        .ok_or_else(|| TgaError::msg("TGA header missing pixel_depth"))?;
    let image_descriptor = *header
        .get(17)
        .ok_or_else(|| TgaError::msg("TGA header missing image_descriptor"))?;

    if image_type.uses_color_map() && (color_map_type == 0 || color_map_length == 0) {
        return Err(TgaError::msg(
            "TGA color-mapped images require a populated color map",
        ));
    }

    let mut cursor = TGA_HEADER_SIZE;
    let id_len = usize::from(id_length);
    if bytes.len() < cursor + id_len {
        return Err(TgaError::msg("TGA image ID extends past end of file"));
    }
    let image_id = bytes
        .get(cursor..cursor + id_len)
        .ok_or_else(|| TgaError::msg("TGA image ID extends past end of file"))?
        .to_vec();
    cursor += id_len;

    let color_map_len =
        color_map_storage_len(color_map_type, color_map_length, color_map_entry_size)?;
    if bytes.len() < cursor + color_map_len {
        return Err(TgaError::msg("TGA color map extends past end of file"));
    }
    let color_map_data = bytes
        .get(cursor..cursor + color_map_len)
        .ok_or_else(|| TgaError::msg("TGA color map extends past end of file"))?
        .to_vec();
    cursor += color_map_len;

    let footer = parse_tga_footer(bytes);
    let payload_end = footer
        .as_ref()
        .map_or(bytes.len(), |_| bytes.len().saturating_sub(TGA_FOOTER_SIZE));
    if payload_end < cursor {
        return Err(TgaError::msg("TGA payload ended before image data"));
    }
    let payload = bytes
        .get(cursor..payload_end)
        .ok_or_else(|| TgaError::msg("TGA payload ended before image data"))?;
    let image_data_len = image_data_storage_len(image_type, width, height, pixel_depth, payload)?;
    let image_data = payload
        .get(..image_data_len)
        .ok_or_else(|| TgaError::msg("TGA image data extends past payload"))?
        .to_vec();
    let trailing_data = payload
        .get(image_data_len..)
        .ok_or_else(|| TgaError::msg("TGA trailing data extends past payload"))?
        .to_vec();

    Ok(TgaTexture {
        id_length,
        color_map_type,
        image_type,
        color_map_first_entry_index,
        color_map_length,
        color_map_entry_size,
        x_origin,
        y_origin,
        width,
        height,
        pixel_depth,
        image_descriptor,
        image_id,
        color_map_data,
        image_data,
        trailing_data,
        footer,
    })
}

fn parse_tga_footer(bytes: &[u8]) -> Option<TgaFooter> {
    if bytes.len() < TGA_FOOTER_SIZE {
        return None;
    }
    let footer = bytes.get(bytes.len() - TGA_FOOTER_SIZE..)?;
    let signature = footer.get(8..26)?;
    if signature != TGA_FOOTER_SIGNATURE {
        return None;
    }
    Some(TgaFooter {
        extension_area_offset:      read_u32_at(footer, 0).ok()?,
        developer_directory_offset: read_u32_at(footer, 4).ok()?,
    })
}

fn read_u16_at(bytes: &[u8], offset: usize) -> TgaResult<u16> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| TgaError::msg(format!("TGA u16 field out of range at byte {offset}")))?;
    let [lo, hi] = <[u8; 2]>::try_from(pair)
        .map_err(|_error| TgaError::msg(format!("TGA u16 field out of range at byte {offset}")))?;
    Ok(u16::from_le_bytes([lo, hi]))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> TgaResult<u32> {
    let quad = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| TgaError::msg(format!("TGA u32 field out of range at byte {offset}")))?;
    let [a, b, c, d] = <[u8; 4]>::try_from(quad)
        .map_err(|_error| TgaError::msg(format!("TGA u32 field out of range at byte {offset}")))?;
    Ok(u32::from_le_bytes([a, b, c, d]))
}

fn color_map_storage_len(
    color_map_type: u8,
    color_map_length: u16,
    color_map_entry_size: u8,
) -> TgaResult<usize> {
    if color_map_type == 0 {
        return Ok(0);
    }

    usize::from(color_map_length)
        .checked_mul(pixel_size_bytes(color_map_entry_size)?)
        .ok_or_else(|| TgaError::msg("TGA color map length overflow"))
}

fn image_data_storage_len(
    image_type: TgaImageType,
    width: u16,
    height: u16,
    pixel_depth: u8,
    payload: &[u8],
) -> TgaResult<usize> {
    let pixel_count = usize::from(width)
        .checked_mul(usize::from(height))
        .ok_or_else(|| TgaError::msg("TGA pixel count overflow"))?;
    let bytes_per_pixel = pixel_size_bytes(pixel_depth)?;

    match image_type {
        TgaImageType::NoImage => Ok(0),
        TgaImageType::ColorMapped | TgaImageType::TrueColor | TgaImageType::BlackAndWhite => {
            let expected_len = pixel_count
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA image data length overflow"))?;
            if payload.len() < expected_len {
                return Err(TgaError::msg(format!(
                    "expected {expected_len} bytes of TGA image data, got {}",
                    payload.len()
                )));
            }
            Ok(expected_len)
        }
        TgaImageType::RleColorMapped
        | TgaImageType::RleTrueColor
        | TgaImageType::RleBlackAndWhite => rle_encoded_len(payload, pixel_count, bytes_per_pixel),
    }
}

fn pixel_size_bytes(bits: u8) -> TgaResult<usize> {
    if bits == 0 {
        return Err(TgaError::msg("TGA pixel size cannot be zero"));
    }
    Ok(usize::from(bits).div_ceil(8))
}

fn rgba_len(width: u16, height: u16) -> TgaResult<usize> {
    usize::from(width)
        .checked_mul(usize::from(height))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| TgaError::msg("TGA RGBA buffer length overflow"))
}

fn rle_encoded_len(payload: &[u8], pixel_count: usize, bytes_per_pixel: usize) -> TgaResult<usize> {
    let mut cursor = 0_usize;
    let mut decoded_pixels = 0_usize;

    while decoded_pixels < pixel_count {
        let header = *payload
            .get(cursor)
            .ok_or_else(|| TgaError::msg("TGA RLE packet header extends past end of file"))?;
        cursor += 1;
        let run_len = usize::from(header & 0x7f) + 1;
        if header & 0x80 != 0 {
            cursor = cursor
                .checked_add(bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA RLE cursor overflow"))?;
            if cursor > payload.len() {
                return Err(TgaError::msg(
                    "TGA RLE packet pixel extends past end of file",
                ));
            }
        } else {
            let packet_bytes = run_len
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA raw packet length overflow"))?;
            cursor = cursor
                .checked_add(packet_bytes)
                .ok_or_else(|| TgaError::msg("TGA RLE cursor overflow"))?;
            if cursor > payload.len() {
                return Err(TgaError::msg(
                    "TGA raw packet data extends past end of file",
                ));
            }
        }
        decoded_pixels = decoded_pixels
            .checked_add(run_len)
            .ok_or_else(|| TgaError::msg("TGA decoded pixel count overflow"))?;
        if decoded_pixels > pixel_count {
            return Err(TgaError::msg(
                "TGA RLE stream expands beyond the declared pixel count",
            ));
        }
    }

    Ok(cursor)
}

fn expand_rle_image_data(
    payload: &[u8],
    pixel_count: usize,
    bytes_per_pixel: usize,
) -> TgaResult<Vec<u8>> {
    let mut cursor = 0_usize;
    let mut out = Vec::with_capacity(
        pixel_count
            .checked_mul(bytes_per_pixel)
            .ok_or_else(|| TgaError::msg("TGA expanded image length overflow"))?,
    );

    while out.len() < pixel_count * bytes_per_pixel {
        let header = *payload
            .get(cursor)
            .ok_or_else(|| TgaError::msg("TGA RLE packet header extends past end of file"))?;
        cursor += 1;
        let run_len = usize::from(header & 0x7f) + 1;
        if header & 0x80 != 0 {
            let pixel = payload
                .get(cursor..cursor + bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA RLE packet pixel extends past end of file"))?;
            cursor += bytes_per_pixel;
            for _ in 0..run_len {
                out.extend_from_slice(pixel);
            }
        } else {
            let packet_len = run_len
                .checked_mul(bytes_per_pixel)
                .ok_or_else(|| TgaError::msg("TGA raw packet length overflow"))?;
            let packet = payload
                .get(cursor..cursor + packet_len)
                .ok_or_else(|| TgaError::msg("TGA raw packet data extends past end of file"))?;
            out.extend_from_slice(packet);
            cursor += packet_len;
        }
    }

    if out.len() != pixel_count * bytes_per_pixel {
        return Err(TgaError::msg(
            "TGA RLE stream did not expand to the declared image size",
        ));
    }

    Ok(out)
}

fn decode_tga_pixel(pixel: &[u8], pixel_depth: u8, image_type: TgaImageType) -> TgaResult<[u8; 4]> {
    match image_type {
        TgaImageType::TrueColor | TgaImageType::RleTrueColor => match pixel_depth {
            24 => match pixel {
                [b, g, r] => Ok([*r, *g, *b, 255]),
                _ => Err(TgaError::msg("TGA 24-bit pixel slice length mismatch")),
            },
            32 => match pixel {
                [b, g, r, a] => Ok([*r, *g, *b, *a]),
                _ => Err(TgaError::msg("TGA 32-bit pixel slice length mismatch")),
            },
            16 => {
                let [lo, hi] = <[u8; 2]>::try_from(pixel)
                    .map_err(|_error| TgaError::msg("TGA 16-bit pixel slice length mismatch"))?;
                let value = u16::from_le_bytes([lo, hi]);
                let b = ((value & 0x1f) as u8) * 255 / 31;
                let g = (((value >> 5) & 0x1f) as u8) * 255 / 31;
                let r = (((value >> 10) & 0x1f) as u8) * 255 / 31;
                let a = if value & 0x8000 != 0 { 255 } else { 0 };
                Ok([r, g, b, a])
            }
            _ => Err(TgaError::msg(format!(
                "unsupported TGA true-color pixel depth: {pixel_depth}"
            ))),
        },
        TgaImageType::BlackAndWhite | TgaImageType::RleBlackAndWhite => match pixel_depth {
            8 => match pixel {
                [gray] => Ok([*gray, *gray, *gray, 255]),
                _ => Err(TgaError::msg(
                    "TGA 8-bit grayscale pixel slice length mismatch",
                )),
            },
            16 => match pixel {
                [gray, alpha] => Ok([*gray, *gray, *gray, *alpha]),
                _ => Err(TgaError::msg(
                    "TGA 16-bit grayscale pixel slice length mismatch",
                )),
            },
            _ => Err(TgaError::msg(format!(
                "unsupported TGA grayscale pixel depth: {pixel_depth}"
            ))),
        },
        TgaImageType::NoImage => Err(TgaError::msg("TGA contains no image data")),
        TgaImageType::ColorMapped | TgaImageType::RleColorMapped => Err(TgaError::msg(
            "TGA color-mapped decode is not implemented yet",
        )),
    }
}

fn encode_image_type(image_type: TgaImageType) -> u8 {
    match image_type {
        TgaImageType::NoImage => 0,
        TgaImageType::ColorMapped => 1,
        TgaImageType::TrueColor => 2,
        TgaImageType::BlackAndWhite => 3,
        TgaImageType::RleColorMapped => 9,
        TgaImageType::RleTrueColor => 10,
        TgaImageType::RleBlackAndWhite => 11,
    }
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::{
        TGA_HEADER_SIZE, TGA_RES_TYPE, TgaError, TgaFooter, TgaImageType, TgaResult, TgaTexture,
        read_tga, write_tga,
    };
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::{TGA_HEADER_SIZE, TgaImageType, TgaTexture, read_tga, write_tga};

    fn fixture_texture() -> TgaTexture {
        let mut image_data = Vec::with_capacity(16 * 16 * 3);
        for _index in 0..(16 * 16) {
            image_data.extend_from_slice(&[0x67, 0x72, 0x7e]);
        }

        TgaTexture {
            id_length: 0,
            color_map_type: 0,
            image_type: TgaImageType::TrueColor,
            color_map_first_entry_index: 0,
            color_map_length: 0,
            color_map_entry_size: 0,
            x_origin: 0,
            y_origin: 0,
            width: 16,
            height: 16,
            pixel_depth: 24,
            image_descriptor: 0,
            image_id: Vec::new(),
            color_map_data: Vec::new(),
            image_data,
            trailing_data: Vec::new(),
            footer: None,
        }
    }

    fn fixture_bytes() -> Vec<u8> {
        let mut encoded = Vec::new();
        if let Err(error) = write_tga(&mut encoded, &fixture_texture()) {
            panic!("write synthetic fixture tga: {error}");
        }
        encoded
    }

    #[test]
    fn fixture_tga_parses_expected_header_fields() {
        let tga = read_tga(&mut Cursor::new(fixture_bytes())).unwrap_or_else(|error| {
            panic!("read synthetic tga fixture: {error}");
        });

        assert_eq!(tga.image_type, TgaImageType::TrueColor);
        assert_eq!(tga.width, 16);
        assert_eq!(tga.height, 16);
        assert_eq!(tga.pixel_depth, 24);
        assert_eq!(tga.image_descriptor, 0);
        assert_eq!(tga.image_data.len(), 16 * 16 * 3);
        assert_eq!(tga.image_data.get(..3), Some(&[0x67, 0x72, 0x7e][..]));
        assert!(tga.footer.is_none());
    }

    #[test]
    fn fixture_tga_decodes_to_rgba8_with_bottom_left_origin_fixup() {
        let tga = read_tga(&mut Cursor::new(fixture_bytes())).unwrap_or_else(|error| {
            panic!("read synthetic tga fixture: {error}");
        });
        let rgba = tga.decode_rgba8().unwrap_or_else(|error| {
            panic!("decode tga fixture: {error}");
        });

        assert_eq!(rgba.len(), 16 * 16 * 4);
        let bottom_left = (15 * 16) * 4;
        assert_eq!(
            rgba.get(bottom_left..bottom_left + 4),
            Some(&[0x7e, 0x72, 0x67, 255][..])
        );
    }

    #[test]
    fn write_tga_roundtrips_fixture_bytes() {
        let original = fixture_bytes();
        let mut cursor = Cursor::new(original.clone());
        let tga = read_tga(&mut cursor).unwrap_or_else(|error| {
            panic!("parse fixture tga: {error}");
        });

        let mut encoded = Vec::new();
        if let Err(error) = write_tga(&mut encoded, &tga) {
            panic!("write tga: {error}");
        }

        assert_eq!(encoded, original);
    }

    #[test]
    fn parse_tga_supports_rle_truecolor() {
        let bytes = [
            0_u8, 0, 10, 0, 0, 0, 0, 0, 0, 0, 0, 0, 2, 0, 1, 0, 24, 0, 0x81, 1, 2, 3,
        ];
        let tga = read_tga(&mut Cursor::new(bytes)).unwrap_or_else(|error| {
            panic!("parse rle tga: {error}");
        });
        let rgba = tga.decode_rgba8().unwrap_or_else(|error| {
            panic!("decode rle tga: {error}");
        });

        assert_eq!(rgba, vec![3, 2, 1, 255, 3, 2, 1, 255]);
        assert_eq!(TGA_HEADER_SIZE, 18);
    }

    #[test]
    fn encode_rgba8_roundtrips_through_decode() {
        let rgba = vec![
            255, 0, 0, 255, 0, 255, 0, 128, 0, 0, 255, 64, 255, 255, 0, 0,
        ];
        let tga = TgaTexture::encode_rgba8(2, 2, &rgba).unwrap_or_else(|error| {
            panic!("encode rgba8 tga: {error}");
        });
        let decoded = tga.decode_rgba8().unwrap_or_else(|error| {
            panic!("decode encoded tga: {error}");
        });

        assert_eq!(tga.image_type, TgaImageType::TrueColor);
        assert_eq!(tga.pixel_depth, 32);
        assert_eq!(tga.image_descriptor, 0x28);
        assert_eq!(decoded, rgba);
    }
}
