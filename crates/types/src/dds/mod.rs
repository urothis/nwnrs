#![forbid(unsafe_code)]
#![doc = include_str!("README.md")]

use std::{
    fmt,
    fs::File,
    io::{self, Read, Write},
    path::Path,
};

use nwnrs_types::resman::{CachePolicy, Res, ResManError, ResType};
use texpresso::{Algorithm, Format as TexpressoFormat, Params as TexpressoParams};
use tracing::instrument;

/// NWN resource type id for `dds`.
pub const DDS_RES_TYPE: ResType = ResType(2033);
/// Size of the NWN DDS header.
pub const NWN_DDS_HEADER_SIZE: usize = 20;

#[derive(Debug)]
/// Errors returned while reading DDS payloads.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::dds::DdsError>();
/// ```
pub enum DdsError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// The payload was otherwise invalid or unsupported.
    Message(String),
}

impl DdsError {
    /// Creates a free-form DDS error message.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let error = nwnrs_types::dds::DdsError::msg("bad texture");
    /// assert_eq!(error.to_string(), "bad texture");
    /// ```
    pub fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for DdsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for DdsError {}

impl From<io::Error> for DdsError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for DdsError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

/// Result type for DDS operations.
pub type DdsResult<T> = Result<T, DdsError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
/// Packed NWN DDS pixel format.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::dds::DdsFormat>();
/// ```
pub enum DdsFormat {
    /// DXT1 block compression.
    Dxt1,
    /// DXT5 block compression.
    Dxt5,
}

impl DdsFormat {
    /// Returns the number of bytes per encoded 4x4 block.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsFormat::bytes_per_block;
    /// ```
    #[must_use]
    pub fn bytes_per_block(self) -> usize {
        match self {
            Self::Dxt1 => 8,
            Self::Dxt5 => 16,
        }
    }

    /// Returns the effective bits per pixel for the packed format.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsFormat::bits_per_pixel;
    /// ```
    #[must_use]
    pub fn bits_per_pixel(self) -> usize {
        match self {
            Self::Dxt1 => 4,
            Self::Dxt5 => 8,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
/// NWN compact DDS header.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::dds::NwnDdsHeader>();
/// ```
pub struct NwnDdsHeader {
    /// Width in pixels.
    pub width:       u32,
    /// Height in pixels.
    pub height:      u32,
    /// Channel count marker from the file. `3` => DXT1, `4` => DXT5.
    pub channels:    u32,
    /// Stored pitch/linear size from the header.
    pub linear_size: u32,
    /// Average alpha value recorded by the encoder.
    pub alpha_mean:  f32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// One encoded mip level.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::dds::DdsMipLevel>();
/// ```
pub struct DdsMipLevel {
    /// Mip level index, zero-based.
    pub level:  usize,
    /// Width in pixels.
    pub width:  u32,
    /// Height in pixels.
    pub height: u32,
    /// Raw packed bytes for this level.
    pub data:   Vec<u8>,
}

impl DdsMipLevel {
    /// Returns the total number of pixels in this mip level.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if the pixel count overflows `usize`.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsMipLevel::pixel_count;
    /// ```
    pub fn pixel_count(&self) -> DdsResult<usize> {
        usize::try_from(self.width)
            .ok()
            .and_then(|width| {
                usize::try_from(self.height)
                    .ok()
                    .and_then(|height| width.checked_mul(height))
            })
            .ok_or_else(|| DdsError::msg("DDS pixel count overflow"))
    }

    /// Decodes this mip level into top-left-origin RGBA8 pixels.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if the format is unsupported or the pixel data is
    /// malformed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsMipLevel::decode_rgba8;
    /// ```
    pub fn decode_rgba8(&self, format: DdsFormat) -> DdsResult<Vec<u8>> {
        decode_mip_rgba8(self, format)
    }
}

#[derive(Debug, Clone, PartialEq)]
/// Parsed NWN DDS texture.
///
/// # Examples
///
/// ```rust,no_run
/// let _ = std::mem::size_of::<nwnrs_types::dds::DdsTexture>();
/// ```
pub struct DdsTexture {
    /// Packed pixel format.
    pub format:     DdsFormat,
    /// Top-level width.
    pub width:      u32,
    /// Top-level height.
    pub height:     u32,
    /// Ordered mip levels.
    pub mip_levels: Vec<DdsMipLevel>,
    /// NWN DDS header.
    pub nwn_header: NwnDdsHeader,
}

impl DdsTexture {
    /// Returns the number of mip levels.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::mip_count;
    /// ```
    #[must_use]
    pub fn mip_count(&self) -> usize {
        self.mip_levels.len()
    }

    /// Parses an NWN DDS texture directly from raw bytes.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if the bytes do not conform to the NWN DDS format.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::read_from_texture_bytes;
    /// ```
    pub fn read_from_texture_bytes(bytes: &[u8]) -> DdsResult<Self> {
        parse_dds_bytes(bytes)
    }

    /// Encodes top-left-origin RGBA8 pixels into an NWN DDS texture with a full
    /// mip chain.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if `rgba` does not match the expected length or
    /// encoding fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::encode_rgba8;
    /// ```
    pub fn encode_rgba8(
        width: u32,
        height: u32,
        format: DdsFormat,
        rgba: &[u8],
    ) -> DdsResult<Self> {
        let base_len = rgba_len(width, height)?;
        if rgba.len() != base_len {
            return Err(DdsError::msg(format!(
                "DDS RGBA input expected {base_len} bytes, got {}",
                rgba.len()
            )));
        }

        let mip_rgba = generate_mip_chain_rgba8(width, height, rgba)?;
        let mut mip_levels = Vec::with_capacity(mip_rgba.len());
        for (level, (mip_width, mip_height, mip_bytes)) in mip_rgba.into_iter().enumerate() {
            mip_levels.push(DdsMipLevel {
                level,
                width: mip_width,
                height: mip_height,
                data: encode_mip_to_blocks(mip_width, mip_height, format, &mip_bytes)?,
            });
        }

        let top_level_size = mip_levels
            .first()
            .map(|mip| mip.data.len())
            .ok_or_else(|| DdsError::msg("DDS mip generation produced no levels"))?;
        let linear_size = u32::try_from(top_level_size).map_err(|error| {
            DdsError::msg(format!("DDS top-level packed size out of range: {error}"))
        })?;
        let alpha_mean = alpha_mean_rgba8(rgba)?;
        let channels = match format {
            DdsFormat::Dxt1 => 3,
            DdsFormat::Dxt5 => 4,
        };

        Ok(Self {
            format,
            width,
            height,
            mip_levels,
            nwn_header: NwnDdsHeader {
                width,
                height,
                channels,
                linear_size,
                alpha_mean,
            },
        })
    }

    /// Decodes the top-level mip into RGBA8 pixels.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if the texture has no mip levels or decoding fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::decode_rgba8;
    /// ```
    pub fn decode_rgba8(&self) -> DdsResult<Vec<u8>> {
        self.mip_levels
            .first()
            .ok_or_else(|| DdsError::msg("DDS contains no mip levels"))?
            .decode_rgba8(self.format)
    }

    /// Decodes a specific mip level into RGBA8 pixels.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if `level` is out of range or decoding fails.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::decode_mip_rgba8;
    /// ```
    pub fn decode_mip_rgba8(&self, level: usize) -> DdsResult<Vec<u8>> {
        self.mip_levels
            .get(level)
            .ok_or_else(|| DdsError::msg(format!("DDS mip level {level} is out of range")))?
            .decode_rgba8(self.format)
    }

    /// Reads a typed DDS texture from disk.
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if the file cannot be opened or parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::from_file(std::path::Path::new("texture.dds"));
    /// ```
    pub fn from_file(path: impl AsRef<Path>) -> DdsResult<Self> {
        let mut file = File::open(path.as_ref())?;
        read_dds(&mut file)
    }

    /// Reads a typed DDS texture from a [`Res`].
    ///
    /// # Errors
    ///
    /// Returns [`DdsError`] if the resource is not a DDS type or the bytes
    /// cannot be parsed.
    ///
    /// # Examples
    ///
    /// ```rust,no_run
    /// let _ = nwnrs_types::dds::DdsTexture::from_res;
    /// ```
    pub fn from_res(res: &Res, cache_policy: CachePolicy) -> DdsResult<Self> {
        if res.resref().res_type() != DDS_RES_TYPE {
            return Err(DdsError::msg(format!(
                "expected dds resource, got {}",
                res.resref()
            )));
        }

        let bytes = res.read_all(cache_policy)?;
        parse_dds_bytes(&bytes)
    }
}

/// Reads a typed DDS texture from `reader`.
///
/// # Errors
///
/// Returns [`DdsError`] if the data cannot be read or does not conform to the
/// NWN DDS format.
///
/// # Examples
///
/// ```rust,no_run
/// let mut reader = std::io::Cursor::new(Vec::<u8>::new());
/// let _ = nwnrs_types::dds::read_dds(&mut reader);
/// ```
#[instrument(level = "debug", skip_all, err)]
pub fn read_dds<R: Read>(reader: &mut R) -> DdsResult<DdsTexture> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    parse_dds_bytes(&bytes)
}

/// Writes a typed NWN DDS texture to `writer`.
///
/// # Errors
///
/// Returns [`DdsError`] if the DDS data is invalid or the write fails.
///
/// # Examples
///
/// ```rust,no_run
/// let rgba = [255_u8; 4 * 4 * 4];
/// let dds = nwnrs_types::dds::DdsTexture::encode_rgba8(
///     4,
///     4,
///     nwnrs_types::dds::DdsFormat::Dxt1,
///     &rgba,
/// )?;
/// let mut writer = Vec::new();
/// nwnrs_types::dds::write_dds(&mut writer, &dds)?;
/// # Ok::<(), nwnrs_types::dds::DdsError>(())
/// ```
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(width = dds.width, height = dds.height, format = ?dds.format, mip_count = dds.mip_levels.len())
)]
pub fn write_dds<W: Write>(writer: &mut W, dds: &DdsTexture) -> DdsResult<()> {
    validate_writable_dds(dds)?;

    let channels = match dds.format {
        DdsFormat::Dxt1 => 3_u32,
        DdsFormat::Dxt5 => 4_u32,
    };
    let top_level = dds
        .mip_levels
        .first()
        .ok_or_else(|| DdsError::msg("DDS contains no mip levels"))?;
    let linear_size = u32::try_from(top_level.data.len()).map_err(|error| {
        DdsError::msg(format!("DDS top-level packed size out of range: {error}"))
    })?;

    writer.write_all(&dds.width.to_le_bytes())?;
    writer.write_all(&dds.height.to_le_bytes())?;
    writer.write_all(&channels.to_le_bytes())?;
    writer.write_all(&linear_size.to_le_bytes())?;
    writer.write_all(&dds.nwn_header.alpha_mean.to_bits().to_le_bytes())?;

    for mip in &dds.mip_levels {
        writer.write_all(&mip.data)?;
    }

    Ok(())
}

fn parse_dds_bytes(bytes: &[u8]) -> DdsResult<DdsTexture> {
    let header = bytes
        .get(..NWN_DDS_HEADER_SIZE)
        .ok_or_else(|| DdsError::msg("NWN DDS header extends past end of file"))?;
    let width = read_u32_at(header, 0)?;
    let height = read_u32_at(header, 4)?;
    let channels = read_u32_at(header, 8)?;
    let linear_size = read_u32_at(header, 12)?;
    let alpha_mean = f32::from_bits(read_u32_at(header, 16)?);

    let format = match channels {
        3 => DdsFormat::Dxt1,
        4 => DdsFormat::Dxt5,
        _ => {
            return Err(DdsError::msg(format!(
                "unsupported NWN DDS channel count: {channels}"
            )));
        }
    };

    let payload = bytes
        .get(NWN_DDS_HEADER_SIZE..)
        .ok_or_else(|| DdsError::msg("NWN DDS payload missing"))?;
    let mip_levels = split_nwn_mips(payload, width, height, format, linear_size)?;

    Ok(DdsTexture {
        format,
        width,
        height,
        mip_levels,
        nwn_header: NwnDdsHeader {
            width,
            height,
            channels,
            linear_size,
            alpha_mean,
        },
    })
}

fn split_nwn_mips(
    payload: &[u8],
    width: u32,
    height: u32,
    format: DdsFormat,
    top_level_pitch: u32,
) -> DdsResult<Vec<DdsMipLevel>> {
    let mut mip_levels = Vec::new();
    let mut cursor = 0_usize;
    let mut mip_width = width;
    let mut mip_height = height;
    let max_levels = compute_max_mips(width, height);

    for level in 0..=max_levels {
        if mip_width == 0 || mip_height == 0 {
            break;
        }

        let actual_level_size = packed_level_size(mip_width, mip_height, format)?;
        let stored_level_size = if level == 0 {
            usize::try_from(top_level_pitch)
                .map_err(|error| DdsError::msg(format!("NWN DDS pitch out of range: {error}")))?
        } else {
            actual_level_size
        };

        let level_slice = payload
            .get(cursor..cursor + actual_level_size)
            .ok_or_else(|| DdsError::msg("NWN DDS mip level extends past end of file"))?;
        mip_levels.push(DdsMipLevel {
            level,
            width: mip_width,
            height: mip_height,
            data: level_slice.to_vec(),
        });

        cursor = cursor
            .checked_add(stored_level_size)
            .ok_or_else(|| DdsError::msg("NWN DDS mip cursor overflow"))?;
        if cursor > payload.len() {
            return Err(DdsError::msg(
                "NWN DDS stored mip stride extends past end of file",
            ));
        }

        if mip_width == 1 && mip_height == 1 {
            break;
        }
        mip_width = mip_width.max(2) / 2;
        mip_height = mip_height.max(2) / 2;
    }

    if mip_levels.is_empty() {
        return Err(DdsError::msg("NWN DDS contains no readable mip levels"));
    }
    if cursor != payload.len() {
        return Err(DdsError::msg(format!(
            "NWN DDS payload has {} trailing bytes after mip chain",
            payload.len().saturating_sub(cursor)
        )));
    }

    Ok(mip_levels)
}

fn validate_writable_dds(dds: &DdsTexture) -> DdsResult<()> {
    let Some(first_mip) = dds.mip_levels.first() else {
        return Err(DdsError::msg("DDS contains no mip levels"));
    };

    if first_mip.width != dds.width || first_mip.height != dds.height {
        return Err(DdsError::msg(format!(
            "DDS top-level mip dimensions {}x{} do not match texture dimensions {}x{}",
            first_mip.width, first_mip.height, dds.width, dds.height
        )));
    }

    let mut expected_width = dds.width;
    let mut expected_height = dds.height;
    for (index, mip) in dds.mip_levels.iter().enumerate() {
        if mip.level != index {
            return Err(DdsError::msg(format!(
                "DDS mip level index mismatch: expected {index}, got {}",
                mip.level
            )));
        }
        if mip.width != expected_width || mip.height != expected_height {
            return Err(DdsError::msg(format!(
                "DDS mip level {index} dimensions {}x{} do not match expected {}x{}",
                mip.width, mip.height, expected_width, expected_height
            )));
        }
        let expected_len = packed_level_size(mip.width, mip.height, dds.format)?;
        if mip.data.len() != expected_len {
            return Err(DdsError::msg(format!(
                "DDS mip level {index} expected {expected_len} packed bytes, got {}",
                mip.data.len()
            )));
        }

        if expected_width == 1 && expected_height == 1 {
            if index + 1 != dds.mip_levels.len() {
                return Err(DdsError::msg("DDS mip chain contains levels beyond 1x1"));
            }
            break;
        }
        expected_width = expected_width.max(2) / 2;
        expected_height = expected_height.max(2) / 2;
    }

    Ok(())
}

fn encode_mip_to_blocks(
    width: u32,
    height: u32,
    format: DdsFormat,
    rgba: &[u8],
) -> DdsResult<Vec<u8>> {
    let expected_len = rgba_len(width, height)?;
    if rgba.len() != expected_len {
        return Err(DdsError::msg(format!(
            "DDS mip RGBA input expected {expected_len} bytes, got {}",
            rgba.len()
        )));
    }

    let blocks_len = packed_level_size(width, height, format)?;
    let mut packed = vec![0_u8; blocks_len];
    let block_format = match format {
        DdsFormat::Dxt1 => TexpressoFormat::Bc1,
        DdsFormat::Dxt5 => TexpressoFormat::Bc3,
    };
    let params = TexpressoParams {
        algorithm: Algorithm::ClusterFit,
        weigh_colour_by_alpha: matches!(format, DdsFormat::Dxt5),
        ..TexpressoParams::default()
    };

    block_format.compress(
        rgba,
        usize::try_from(width)
            .map_err(|error| DdsError::msg(format!("DDS width out of range: {error}")))?,
        usize::try_from(height)
            .map_err(|error| DdsError::msg(format!("DDS height out of range: {error}")))?,
        params,
        &mut packed,
    );

    Ok(packed)
}

fn generate_mip_chain_rgba8(
    width: u32,
    height: u32,
    rgba: &[u8],
) -> DdsResult<Vec<(u32, u32, Vec<u8>)>> {
    let mut levels = Vec::new();
    let mut current_width = width;
    let mut current_height = height;
    let mut current_rgba = rgba.to_vec();

    loop {
        levels.push((current_width, current_height, current_rgba.clone()));
        if current_width == 1 && current_height == 1 {
            break;
        }
        let next_width = current_width.max(2) / 2;
        let next_height = current_height.max(2) / 2;
        current_rgba = downsample_rgba8(
            &current_rgba,
            current_width,
            current_height,
            next_width,
            next_height,
        )?;
        current_width = next_width;
        current_height = next_height;
    }

    Ok(levels)
}

fn downsample_rgba8(
    src: &[u8],
    src_width: u32,
    src_height: u32,
    dst_width: u32,
    dst_height: u32,
) -> DdsResult<Vec<u8>> {
    let mut dst = vec![0_u8; rgba_len(dst_width, dst_height)?];
    for dst_y in 0..dst_height {
        for dst_x in 0..dst_width {
            let mut sum = [0_u32; 4];
            let mut samples = 0_u32;
            let src_x0 = dst_x * 2;
            let src_y_base = dst_y * 2;
            for y in src_y_base..(src_y_base + 2).min(src_height) {
                for x in src_x0..(src_x0 + 2).min(src_width) {
                    let pixel = rgba_pixel(src, src_width, x, y)?;
                    sum[0] += u32::from(pixel[0]);
                    sum[1] += u32::from(pixel[1]);
                    sum[2] += u32::from(pixel[2]);
                    sum[3] += u32::from(pixel[3]);
                    samples += 1;
                }
            }
            let pixel = [
                narrow_u32_to_u8(sum[0] / samples.max(1)),
                narrow_u32_to_u8(sum[1] / samples.max(1)),
                narrow_u32_to_u8(sum[2] / samples.max(1)),
                narrow_u32_to_u8(sum[3] / samples.max(1)),
            ];
            write_rgba_pixel(&mut dst, dst_width, dst_x, dst_y, pixel)?;
        }
    }
    Ok(dst)
}

#[allow(clippy::cast_precision_loss)]
fn alpha_mean_rgba8(rgba: &[u8]) -> DdsResult<f32> {
    let pixel_count = rgba
        .len()
        .checked_div(4)
        .ok_or_else(|| DdsError::msg("DDS RGBA pixel count overflow"))?;
    if pixel_count == 0 {
        return Err(DdsError::msg("DDS RGBA input contains no pixels"));
    }
    let alpha_sum = rgba.chunks_exact(4).try_fold(0_u64, |acc, pixel| {
        pixel
            .get(3)
            .copied()
            .map(u64::from)
            .and_then(|alpha| acc.checked_add(alpha))
            .ok_or_else(|| DdsError::msg("DDS alpha sum overflow"))
    })?;
    Ok((alpha_sum as f32) / (pixel_count as f32 * 255.0))
}

fn rgba_len(width: u32, height: u32) -> DdsResult<usize> {
    usize::try_from(width)
        .ok()
        .and_then(|w| usize::try_from(height).ok().and_then(|h| w.checked_mul(h)))
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| DdsError::msg("DDS RGBA buffer length overflow"))
}

fn rgba_pixel(rgba: &[u8], width: u32, x: u32, y: u32) -> DdsResult<[u8; 4]> {
    let index = usize::try_from(y)
        .ok()
        .and_then(|row| {
            usize::try_from(width)
                .ok()
                .and_then(|stride| row.checked_mul(stride))
        })
        .and_then(|row| usize::try_from(x).ok().and_then(|col| row.checked_add(col)))
        .and_then(|pixel| pixel.checked_mul(4))
        .ok_or_else(|| DdsError::msg("DDS RGBA pixel index overflow"))?;
    let pixel = rgba
        .get(index..index + 4)
        .ok_or_else(|| DdsError::msg("DDS RGBA pixel slice out of range"))?;
    <[u8; 4]>::try_from(pixel).map_err(|_error| DdsError::msg("DDS RGBA pixel slice out of range"))
}

fn write_rgba_pixel(rgba: &mut [u8], width: u32, x: u32, y: u32, pixel: [u8; 4]) -> DdsResult<()> {
    let index = usize::try_from(y)
        .ok()
        .and_then(|row| {
            usize::try_from(width)
                .ok()
                .and_then(|stride| row.checked_mul(stride))
        })
        .and_then(|row| usize::try_from(x).ok().and_then(|col| row.checked_add(col)))
        .and_then(|pixel_index| pixel_index.checked_mul(4))
        .ok_or_else(|| DdsError::msg("DDS RGBA write index overflow"))?;
    let out = rgba
        .get_mut(index..index + 4)
        .ok_or_else(|| DdsError::msg("DDS RGBA write slice out of range"))?;
    out.copy_from_slice(&pixel);
    Ok(())
}

fn decode_mip_rgba8(mip: &DdsMipLevel, format: DdsFormat) -> DdsResult<Vec<u8>> {
    let pixel_count = mip.pixel_count()?;
    let mut rgba = vec![
        0_u8;
        pixel_count
            .checked_mul(4)
            .ok_or_else(|| DdsError::msg("DDS RGBA length overflow"))?
    ];
    let blocks_x = mip.width.div_ceil(4);
    let blocks_y = mip.height.div_ceil(4);
    let expected_len = packed_level_size(mip.width, mip.height, format)?;
    if mip.data.len() != expected_len {
        return Err(DdsError::msg(format!(
            "DDS mip level {} expected {expected_len} packed bytes, got {}",
            mip.level,
            mip.data.len()
        )));
    }

    for block_y in 0..blocks_y {
        for block_x in 0..blocks_x {
            let block_index = usize::try_from(block_y)
                .ok()
                .and_then(|row| {
                    usize::try_from(blocks_x)
                        .ok()
                        .and_then(|stride| row.checked_mul(stride))
                })
                .and_then(|row| {
                    usize::try_from(block_x)
                        .ok()
                        .and_then(|col| row.checked_add(col))
                })
                .ok_or_else(|| DdsError::msg("DDS block index overflow"))?;
            let block_offset = block_index
                .checked_mul(format.bytes_per_block())
                .ok_or_else(|| DdsError::msg("DDS block offset overflow"))?;
            let block = mip
                .data
                .get(block_offset..block_offset + format.bytes_per_block())
                .ok_or_else(|| DdsError::msg("DDS block slice out of range"))?;

            match format {
                DdsFormat::Dxt1 => decode_dxt1_block_into(block, mip, block_x, block_y, &mut rgba)?,
                DdsFormat::Dxt5 => decode_dxt5_block_into(block, mip, block_x, block_y, &mut rgba)?,
            }
        }
    }

    Ok(rgba)
}

fn decode_dxt1_block_into(
    block: &[u8],
    mip: &DdsMipLevel,
    block_x: u32,
    block_y: u32,
    rgba: &mut [u8],
) -> DdsResult<()> {
    let color0 = read_u16_at(block, 0)?;
    let color1 = read_u16_at(block, 2)?;
    let selector_bytes = block
        .get(4..8)
        .ok_or_else(|| DdsError::msg("DDS DXT1 selector bytes missing"))
        .and_then(|bytes| {
            <[u8; 4]>::try_from(bytes)
                .map_err(|_error| DdsError::msg("DDS DXT1 selector bytes missing"))
        })?;
    let palette = decode_dxt_colors(color0, color1, true);
    blit_block_rgba8(mip, block_x, block_y, rgba, |x, y| {
        let row = match y {
            0 => selector_bytes[0],
            1 => selector_bytes[1],
            2 => selector_bytes[2],
            _ => selector_bytes[3],
        };
        let selector = (row >> (x * 2)) & 0x03;
        match selector {
            0 => palette[0],
            1 => palette[1],
            2 => palette[2],
            _ => palette[3],
        }
    })
}

fn decode_dxt5_block_into(
    block: &[u8],
    mip: &DdsMipLevel,
    block_x: u32,
    block_y: u32,
    rgba: &mut [u8],
) -> DdsResult<()> {
    let alpha0 = *block
        .first()
        .ok_or_else(|| DdsError::msg("DDS DXT5 alpha endpoint 0 missing"))?;
    let alpha1 = *block
        .get(1)
        .ok_or_else(|| DdsError::msg("DDS DXT5 alpha endpoint 1 missing"))?;
    let alpha_selectors = block
        .get(2..8)
        .ok_or_else(|| DdsError::msg("DDS DXT5 alpha selector bytes missing"))?;
    let color0 = read_u16_at(block, 8)?;
    let color1 = read_u16_at(block, 10)?;
    let color_selector_bytes = block
        .get(12..16)
        .ok_or_else(|| DdsError::msg("DDS DXT5 color selector bytes missing"))
        .and_then(|bytes| {
            <[u8; 4]>::try_from(bytes)
                .map_err(|_error| DdsError::msg("DDS DXT5 color selector bytes missing"))
        })?;
    let alpha_values = decode_dxt5_alpha_values(alpha0, alpha1);
    let color_table = decode_dxt_colors(color0, color1, false);

    blit_block_rgba8(mip, block_x, block_y, rgba, |x, y| {
        let selector_index = y * 4 + x;
        let alpha_selector = dxt5_selector(alpha_selectors, selector_index);
        let row = match y {
            0 => color_selector_bytes[0],
            1 => color_selector_bytes[1],
            2 => color_selector_bytes[2],
            _ => color_selector_bytes[3],
        };
        let color_selector = (row >> (x * 2)) & 0x03;
        let [r, g, b, _a] = match color_selector {
            0 => color_table[0],
            1 => color_table[1],
            2 => color_table[2],
            _ => color_table[3],
        };
        let alpha = match alpha_selector {
            0 => alpha_values[0],
            1 => alpha_values[1],
            2 => alpha_values[2],
            3 => alpha_values[3],
            4 => alpha_values[4],
            5 => alpha_values[5],
            6 => alpha_values[6],
            _ => alpha_values[7],
        };
        [r, g, b, alpha]
    })
}

fn blit_block_rgba8(
    mip: &DdsMipLevel,
    block_x: u32,
    block_y: u32,
    rgba: &mut [u8],
    mut pixel_fn: impl FnMut(u32, u32) -> [u8; 4],
) -> DdsResult<()> {
    let base_x = block_x
        .checked_mul(4)
        .ok_or_else(|| DdsError::msg("DDS block x overflow"))?;
    let base_y = block_y
        .checked_mul(4)
        .ok_or_else(|| DdsError::msg("DDS block y overflow"))?;

    for y in 0..4 {
        let dst_y = base_y + y;
        if dst_y >= mip.height {
            break;
        }
        for x in 0..4 {
            let dst_x = base_x + x;
            if dst_x >= mip.width {
                break;
            }
            let pixel_index = usize::try_from(dst_y)
                .ok()
                .and_then(|row| {
                    usize::try_from(mip.width)
                        .ok()
                        .and_then(|stride| row.checked_mul(stride))
                })
                .and_then(|row| {
                    usize::try_from(dst_x)
                        .ok()
                        .and_then(|col| row.checked_add(col))
                })
                .ok_or_else(|| DdsError::msg("DDS pixel index overflow"))?;
            let dst = pixel_index
                .checked_mul(4)
                .ok_or_else(|| DdsError::msg("DDS RGBA index overflow"))?;
            let out = rgba
                .get_mut(dst..dst + 4)
                .ok_or_else(|| DdsError::msg("DDS output slice out of range"))?;
            out.copy_from_slice(&pixel_fn(x, y));
        }
    }

    Ok(())
}

fn decode_dxt_colors(color0: u16, color1: u16, allow_transparency: bool) -> [[u8; 4]; 4] {
    let c0 = unpack_rgb565(color0);
    let c1 = unpack_rgb565(color1);
    if color0 > color1 {
        [
            c0,
            c1,
            [
                narrow_u16_to_u8((u16::from(c0[0]) * 2 + u16::from(c1[0])) / 3),
                narrow_u16_to_u8((u16::from(c0[1]) * 2 + u16::from(c1[1])) / 3),
                narrow_u16_to_u8((u16::from(c0[2]) * 2 + u16::from(c1[2])) / 3),
                255,
            ],
            [
                narrow_u16_to_u8((u16::from(c1[0]) * 2 + u16::from(c0[0])) / 3),
                narrow_u16_to_u8((u16::from(c1[1]) * 2 + u16::from(c0[1])) / 3),
                narrow_u16_to_u8((u16::from(c1[2]) * 2 + u16::from(c0[2])) / 3),
                255,
            ],
        ]
    } else if allow_transparency {
        [
            c0,
            c1,
            [
                narrow_u16_to_u8(u16::midpoint(u16::from(c0[0]), u16::from(c1[0]))),
                narrow_u16_to_u8(u16::midpoint(u16::from(c0[1]), u16::from(c1[1]))),
                narrow_u16_to_u8(u16::midpoint(u16::from(c0[2]), u16::from(c1[2]))),
                255,
            ],
            [0, 0, 0, 0],
        ]
    } else {
        [
            c0,
            c1,
            [
                narrow_u16_to_u8(u16::midpoint(u16::from(c0[0]), u16::from(c1[0]))),
                narrow_u16_to_u8(u16::midpoint(u16::from(c0[1]), u16::from(c1[1]))),
                narrow_u16_to_u8(u16::midpoint(u16::from(c0[2]), u16::from(c1[2]))),
                255,
            ],
            [0, 0, 0, 255],
        ]
    }
}

fn unpack_rgb565(packed: u16) -> [u8; 4] {
    let b = narrow_u16_to_u8(packed & 0x001f);
    let g = narrow_u16_to_u8((packed >> 5) & 0x003f);
    let r = narrow_u16_to_u8((packed >> 11) & 0x001f);
    [
        (r << 3) | (r >> 2),
        (g << 2) | (g >> 4),
        (b << 3) | (b >> 2),
        255,
    ]
}

fn decode_dxt5_alpha_values(alpha0: u8, alpha1: u8) -> [u8; 8] {
    if alpha0 > alpha1 {
        [
            alpha0,
            alpha1,
            narrow_u16_to_u8((u16::from(alpha0) * 6 + u16::from(alpha1)) / 7),
            narrow_u16_to_u8((u16::from(alpha0) * 5 + u16::from(alpha1) * 2) / 7),
            narrow_u16_to_u8((u16::from(alpha0) * 4 + u16::from(alpha1) * 3) / 7),
            narrow_u16_to_u8((u16::from(alpha0) * 3 + u16::from(alpha1) * 4) / 7),
            narrow_u16_to_u8((u16::from(alpha0) * 2 + u16::from(alpha1) * 5) / 7),
            narrow_u16_to_u8((u16::from(alpha0) + u16::from(alpha1) * 6) / 7),
        ]
    } else {
        [
            alpha0,
            alpha1,
            narrow_u16_to_u8((u16::from(alpha0) * 4 + u16::from(alpha1)) / 5),
            narrow_u16_to_u8((u16::from(alpha0) * 3 + u16::from(alpha1) * 2) / 5),
            narrow_u16_to_u8((u16::from(alpha0) * 2 + u16::from(alpha1) * 3) / 5),
            narrow_u16_to_u8((u16::from(alpha0) + u16::from(alpha1) * 4) / 5),
            0,
            255,
        ]
    }
}

fn dxt5_selector(selectors: &[u8], selector_index: u32) -> u8 {
    let bit_index = selector_index * 3;
    let byte_index = usize::try_from(bit_index >> 3).unwrap_or(0);
    let bit_offset = bit_index & 7;
    let low = selectors.get(byte_index).copied().map_or(0, u16::from);
    let high = selectors.get(byte_index + 1).copied().map_or(0, u16::from);
    narrow_u16_to_u8((low | (high << 8)) >> bit_offset) & 0x07
}

fn packed_level_size(width: u32, height: u32, format: DdsFormat) -> DdsResult<usize> {
    let blocks_x = width.div_ceil(4);
    let blocks_y = height.div_ceil(4);
    usize::try_from(blocks_x)
        .ok()
        .and_then(|x| {
            usize::try_from(blocks_y)
                .ok()
                .and_then(|y| x.checked_mul(y))
        })
        .and_then(|blocks| blocks.checked_mul(format.bytes_per_block()))
        .ok_or_else(|| DdsError::msg("DDS packed level size overflow"))
}

fn compute_max_mips(width: u32, height: u32) -> usize {
    let mut levels = 0_usize;
    let mut width = width.max(1);
    let mut height = height.max(1);
    while width > 1 || height > 1 {
        width = width.max(2) / 2;
        height = height.max(2) / 2;
        levels += 1;
    }
    levels
}

fn read_u32_at(bytes: &[u8], offset: usize) -> DdsResult<u32> {
    let quad = bytes
        .get(offset..offset + 4)
        .ok_or_else(|| DdsError::msg(format!("DDS u32 field out of range at byte {offset}")))?;
    let [a, b, c, d] = <[u8; 4]>::try_from(quad)
        .map_err(|_error| DdsError::msg(format!("DDS u32 field out of range at byte {offset}")))?;
    Ok(u32::from_le_bytes([a, b, c, d]))
}

fn read_u16_at(bytes: &[u8], offset: usize) -> DdsResult<u16> {
    let pair = bytes
        .get(offset..offset + 2)
        .ok_or_else(|| DdsError::msg(format!("DDS u16 field out of range at byte {offset}")))?;
    let [a, b] = <[u8; 2]>::try_from(pair)
        .map_err(|_error| DdsError::msg(format!("DDS u16 field out of range at byte {offset}")))?;
    Ok(u16::from_le_bytes([a, b]))
}

fn narrow_u16_to_u8(value: u16) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

fn narrow_u32_to_u8(value: u32) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}

/// Common imports for consumers of this crate.
pub mod prelude {
    pub use crate::dds::{
        DDS_RES_TYPE, DdsError, DdsFormat, DdsMipLevel, DdsResult, DdsTexture, NWN_DDS_HEADER_SIZE,
        NwnDdsHeader, read_dds, write_dds,
    };
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use crate::dds::{
        DdsFormat, DdsMipLevel, DdsTexture, NwnDdsHeader, decode_dxt5_alpha_values, read_dds,
        write_dds,
    };

    #[test]
    fn decodes_dxt1_block_to_rgba8() {
        let mip = DdsMipLevel {
            level:  0,
            width:  4,
            height: 4,
            data:   vec![
                0x00, 0xf8, 0xe0, 0x07, // red, green
                0xe4, 0xe4, 0xe4, 0xe4, // selectors: 0,1,2,3 across each row
            ],
        };

        let rgba = mip.decode_rgba8(DdsFormat::Dxt1).unwrap_or_else(|error| {
            panic!("decode dxt1 mip: {error}");
        });

        assert_eq!(rgba.len(), 64);
        assert_eq!(rgba.get(0..4), Some([255, 0, 0, 255].as_slice()));
        assert_eq!(rgba.get(4..8), Some([0, 255, 0, 255].as_slice()));
        assert_eq!(rgba.get(8..12), Some([170, 85, 0, 255].as_slice()));
        assert_eq!(rgba.get(12..16), Some([85, 170, 0, 255].as_slice()));
    }

    #[test]
    fn decodes_dxt5_block_to_rgba8() {
        let mip = DdsMipLevel {
            level:  0,
            width:  4,
            height: 4,
            data:   vec![
                0xff, 0x00, // alpha endpoints
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // alpha selectors: all 0
                0x00, 0xf8, 0xe0, 0x07, // red, green
                0xe4, 0xe4, 0xe4, 0xe4, // selectors: 0,1,2,3
            ],
        };

        let rgba = mip.decode_rgba8(DdsFormat::Dxt5).unwrap_or_else(|error| {
            panic!("decode dxt5 mip: {error}");
        });

        assert_eq!(rgba.len(), 64);
        assert_eq!(rgba.get(0..4), Some([255, 0, 0, 255].as_slice()));
        assert_eq!(rgba.get(4..8), Some([0, 255, 0, 255].as_slice()));
        assert_eq!(rgba.get(8..12), Some([170, 85, 0, 255].as_slice()));
        assert_eq!(rgba.get(12..16), Some([85, 170, 0, 255].as_slice()));
    }

    #[test]
    fn dxt5_alpha_palette_matches_crunch_rules() {
        assert_eq!(
            decode_dxt5_alpha_values(255, 0),
            [255, 0, 218, 182, 145, 109, 72, 36]
        );
        assert_eq!(
            decode_dxt5_alpha_values(0, 255),
            [0, 255, 51, 102, 153, 204, 0, 255]
        );
    }

    #[test]
    fn manual_dds_roundtrips_through_read_and_write() {
        let original = DdsTexture {
            format:     DdsFormat::Dxt1,
            width:      4,
            height:     4,
            mip_levels: vec![
                DdsMipLevel {
                    level:  0,
                    width:  4,
                    height: 4,
                    data:   vec![0x00, 0xf8, 0xe0, 0x07, 0xe4, 0xe4, 0xe4, 0xe4],
                },
                DdsMipLevel {
                    level:  1,
                    width:  2,
                    height: 2,
                    data:   vec![0x00, 0xf8, 0xe0, 0x07, 0x00, 0x00, 0x00, 0x00],
                },
                DdsMipLevel {
                    level:  2,
                    width:  1,
                    height: 1,
                    data:   vec![0x00, 0xf8, 0xe0, 0x07, 0x55, 0x55, 0x55, 0x55],
                },
            ],
            nwn_header: NwnDdsHeader {
                width:       4,
                height:      4,
                channels:    3,
                linear_size: 8,
                alpha_mean:  1.0,
            },
        };

        let mut encoded = Vec::new();
        if let Err(error) = write_dds(&mut encoded, &original) {
            panic!("write manual dds: {error}");
        }

        let mut cursor = Cursor::new(encoded);
        let decoded = read_dds(&mut cursor).unwrap_or_else(|error| {
            panic!("read manual dds: {error}");
        });

        assert_eq!(decoded, original);
    }

    #[test]
    fn encode_rgba8_supports_dxt1_solid_color() {
        let rgba = vec![
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0,
            0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
            255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255, 255, 0, 0, 255,
        ];
        let dds = DdsTexture::encode_rgba8(4, 4, DdsFormat::Dxt1, &rgba).unwrap_or_else(|error| {
            panic!("encode dxt1 rgba8: {error}");
        });
        let decoded = dds.decode_rgba8().unwrap_or_else(|error| {
            panic!("decode encoded dxt1: {error}");
        });

        assert_eq!(dds.width, 4);
        assert_eq!(dds.height, 4);
        assert_eq!(dds.mip_count(), 3);
        assert_eq!(decoded.get(0..4), Some([255, 0, 0, 255].as_slice()));
    }

    #[test]
    fn encode_rgba8_supports_dxt5_solid_alpha() {
        let rgba = vec![
            0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255,
            128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128,
            255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0, 128, 255, 128, 0,
            128, 255, 128, 0, 128, 255, 128,
        ];
        let dds = DdsTexture::encode_rgba8(4, 4, DdsFormat::Dxt5, &rgba).unwrap_or_else(|error| {
            panic!("encode dxt5 rgba8: {error}");
        });
        let decoded = dds.decode_rgba8().unwrap_or_else(|error| {
            panic!("decode encoded dxt5: {error}");
        });

        assert_eq!(dds.width, 4);
        assert_eq!(dds.height, 4);
        assert_eq!(dds.mip_count(), 3);
        assert_eq!(decoded.get(0..4), Some([0, 128, 255, 128].as_slice()));
    }
}
