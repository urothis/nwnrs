use std::io::{self, Cursor, Read, Write};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use nwnrs_util::{ExpectationError, expect, read_bytes_or_err};
use tracing::{debug, instrument};

use crate::{Algorithm, CompressedBufResult, VERSION, ZLIB_VERSION, ZSTD_VERSION};

/// Encodes a four-byte ASCII magic string into the packed integer form used by
/// the format.
pub fn make_magic(magic: &str) -> Result<u32, ExpectationError> {
    expect(magic.len() == 4, "magic needs to be 4 bytes exactly")?;
    let bytes: [u8; 4] = magic
        .as_bytes()
        .try_into()
        .map_err(|_error| ExpectationError::new("magic needs to be 4 bytes exactly"))?;
    Ok(u32::from_le_bytes(bytes))
}

/// Decompresses a complete compressed buffer payload from memory.
#[instrument(level = "debug", skip_all, err, fields(expect_magic))]
pub fn decompress_bytes(bytes: &[u8], expect_magic: u32) -> CompressedBufResult<Vec<u8>> {
    let mut reader = Cursor::new(bytes);
    decompress_reader(&mut reader, expect_magic)
}

/// Decompresses a compressed buffer payload from `reader`.
#[instrument(level = "debug", skip_all, err, fields(expect_magic))]
pub fn decompress_reader<R: Read>(
    reader: &mut R,
    expect_magic: u32,
) -> CompressedBufResult<Vec<u8>> {
    let magic = read_u32(reader)?;
    expect(magic == expect_magic, format!("invalid magic: {magic}"))?;

    let header_version = read_u32(reader)?;
    expect(
        header_version == VERSION,
        format!("invalid header version: {header_version}"),
    )?;

    let algorithm = Algorithm::from_u32(read_u32(reader)?)?;
    let uncompressed_size = read_u32(reader)? as usize;
    if uncompressed_size == 0 {
        return Ok(Vec::new());
    }

    let payload = match algorithm {
        Algorithm::None => read_bytes_or_err(reader, uncompressed_size)?,
        Algorithm::Zlib => {
            let version = read_u32(reader)?;
            expect(
                version == ZLIB_VERSION,
                format!("invalid zlib header version: {version}"),
            )?;

            let mut decoder = ZlibDecoder::new(reader);
            let mut payload = Vec::with_capacity(uncompressed_size);
            decoder.read_to_end(&mut payload)?;
            payload
        }
        Algorithm::Zstd => {
            let version = read_u32(reader)?;
            expect(
                version == ZSTD_VERSION,
                format!("invalid zstd header version: {version}"),
            )?;

            let dictionary = read_u32(reader)?;
            expect(dictionary == 0, "dictionaries are not supported")?;
            zstd::stream::decode_all(reader)?
        }
    };

    expect(
        payload.len() == uncompressed_size,
        format!(
            "uncompressed payload length mismatch: expected {uncompressed_size}, got {}",
            payload.len()
        ),
    )?;
    debug!(algorithm = ?algorithm, uncompressed_size, "decompressed compressed buffer");
    Ok(payload)
}

/// Compresses a payload in memory and returns the encoded buffer.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?algorithm, magic, input_len = data.len()))]
pub fn compress_bytes(
    data: &[u8],
    algorithm: Algorithm,
    magic: u32,
) -> CompressedBufResult<Vec<u8>> {
    let mut output = Vec::new();
    compress_writer(&mut output, data, algorithm, magic)?;
    Ok(output)
}

/// Reads all bytes from `reader`, compresses them, and writes the encoded
/// buffer.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?algorithm, magic))]
pub fn compress_reader<R: Read, W: Write>(
    writer: &mut W,
    reader: &mut R,
    algorithm: Algorithm,
    magic: u32,
) -> CompressedBufResult<()> {
    let mut data = Vec::new();
    reader.read_to_end(&mut data)?;
    compress_writer(writer, &data, algorithm, magic)
}

/// Compresses `data` and writes the encoded buffer to `writer`.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?algorithm, magic, input_len = data.len()))]
pub fn compress_writer<W: Write + ?Sized>(
    writer: &mut W,
    data: &[u8],
    algorithm: Algorithm,
    magic: u32,
) -> CompressedBufResult<()> {
    write_u32(writer, magic)?;
    write_u32(writer, VERSION)?;
    write_u32(writer, algorithm as u32)?;
    write_u32(
        writer,
        u32::try_from(data.len()).map_err(|_error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "compressed buffer payload exceeds 32-bit size",
            )
        })?,
    )?;

    match algorithm {
        Algorithm::None => writer.write_all(data)?,
        Algorithm::Zlib => {
            write_u32(writer, ZLIB_VERSION)?;
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(data)?;
            let payload = encoder.finish()?;
            writer.write_all(&payload)?;
        }
        Algorithm::Zstd => {
            write_u32(writer, ZSTD_VERSION)?;
            write_u32(writer, 0)?;
            let payload = zstd::stream::encode_all(Cursor::new(data), 0)?;
            writer.write_all(&payload)?;
        }
    }

    debug!(algorithm = ?algorithm, len = data.len(), "compressed buffer payload");
    Ok(())
}

fn read_u32<R: Read>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_u32<W: Write + ?Sized>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}
