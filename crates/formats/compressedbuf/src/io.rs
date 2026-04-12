use std::io::{self, Cursor, Read, Write};

use flate2::{Compression, read::ZlibDecoder, write::ZlibEncoder};
use nwnrs_io::prelude::*;
use tracing::{debug, instrument};

use crate::{
    Algorithm, AlgorithmHeader, CompressedBufError, CompressedBufPayload, CompressedBufResult,
    VERSION, ZLIB_VERSION, ZSTD_VERSION,
};

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
    Ok(read_payload_bytes(bytes, expect_magic)?.data)
}

/// Decompresses a compressed buffer payload from `reader`.
#[instrument(level = "debug", skip_all, err, fields(expect_magic))]
pub fn decompress_reader<R: Read>(
    reader: &mut R,
    expect_magic: u32,
) -> CompressedBufResult<Vec<u8>> {
    Ok(read_payload_reader(reader, expect_magic)?.data)
}

/// Reads a complete compressed-buffer payload from memory.
#[instrument(level = "debug", skip_all, err, fields(expect_magic))]
pub fn read_payload_bytes(
    bytes: &[u8],
    expect_magic: u32,
) -> CompressedBufResult<CompressedBufPayload> {
    let mut reader = Cursor::new(bytes);
    let mut payload = read_payload_reader(&mut reader, expect_magic)?;
    payload.original_bytes = Some(bytes.to_vec());
    Ok(payload)
}

/// Reads a compressed-buffer payload from `reader`.
#[instrument(level = "debug", skip_all, err, fields(expect_magic))]
pub fn read_payload_reader<R: Read>(
    reader: &mut R,
    expect_magic: u32,
) -> CompressedBufResult<CompressedBufPayload> {
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
        return Ok(CompressedBufPayload {
            magic,
            header_version,
            algorithm,
            algorithm_header: AlgorithmHeader::canonical_for(algorithm),
            data: Vec::new(),
            original_bytes: None,
        });
    }

    let (algorithm_header, payload) = match algorithm {
        Algorithm::None => (
            AlgorithmHeader::None,
            read_bytes_or_err(reader, uncompressed_size)?,
        ),
        Algorithm::Zlib => {
            let version = read_u32(reader)?;
            expect(
                version == ZLIB_VERSION,
                format!("invalid zlib header version: {version}"),
            )?;

            let mut decoder = ZlibDecoder::new(reader);
            let mut payload = Vec::with_capacity(uncompressed_size);
            decoder.read_to_end(&mut payload)?;
            (
                AlgorithmHeader::Zlib {
                    version,
                },
                payload,
            )
        }
        Algorithm::Zstd => {
            let version = read_u32(reader)?;
            expect(
                version == ZSTD_VERSION,
                format!("invalid zstd header version: {version}"),
            )?;

            let dictionary = read_u32(reader)?;
            expect(dictionary == 0, "dictionaries are not supported")?;
            (
                AlgorithmHeader::Zstd {
                    version,
                    dictionary,
                },
                zstd::stream::decode_all(reader)?,
            )
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
    Ok(CompressedBufPayload {
        magic,
        header_version,
        algorithm,
        algorithm_header,
        data: payload,
        original_bytes: None,
    })
}

/// Compresses a payload in memory and returns the encoded buffer.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?algorithm, magic, input_len = data.len()))]
pub fn compress_bytes(
    data: &[u8],
    algorithm: Algorithm,
    magic: u32,
) -> CompressedBufResult<Vec<u8>> {
    write_payload_bytes(&CompressedBufPayload::new(magic, algorithm, data.to_vec()))
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
    write_payload_writer(writer, &CompressedBufPayload::new(magic, algorithm, data))
}

/// Compresses `data` and writes the encoded buffer to `writer`.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?algorithm, magic, input_len = data.len()))]
pub fn compress_writer<W: Write + ?Sized>(
    writer: &mut W,
    data: &[u8],
    algorithm: Algorithm,
    magic: u32,
) -> CompressedBufResult<()> {
    write_payload_writer(
        writer,
        &CompressedBufPayload::new(magic, algorithm, data.to_vec()),
    )
}

/// Encodes a provenance-rich payload back into memory.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?payload.algorithm, magic = payload.magic, input_len = payload.data.len()))]
pub fn write_payload_bytes(payload: &CompressedBufPayload) -> CompressedBufResult<Vec<u8>> {
    if let Some(original_bytes) = &payload.original_bytes {
        let mut original = payload.clone();
        original.original_bytes = None;
        let mut reparsed = read_payload_bytes(original_bytes, payload.magic)?;
        reparsed.original_bytes = None;
        if reparsed == original {
            return Ok(original_bytes.clone());
        }
    }

    let mut output = Vec::new();
    write_payload_writer(&mut output, payload)?;
    Ok(output)
}

/// Encodes a provenance-rich payload to `writer`.
#[instrument(level = "debug", skip_all, err, fields(algorithm = ?payload.algorithm, magic = payload.magic, input_len = payload.data.len()))]
pub fn write_payload_writer<W: Write + ?Sized>(
    writer: &mut W,
    payload: &CompressedBufPayload,
) -> CompressedBufResult<()> {
    write_u32(writer, payload.magic)?;
    write_u32(writer, payload.header_version)?;
    write_u32(writer, payload.algorithm as u32)?;
    write_u32(
        writer,
        u32::try_from(payload.data.len()).map_err(|_error| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "compressed buffer payload exceeds 32-bit size",
            )
        })?,
    )?;

    match (&payload.algorithm, &payload.algorithm_header) {
        (Algorithm::None, AlgorithmHeader::None) => writer.write_all(&payload.data)?,
        (
            Algorithm::Zlib,
            AlgorithmHeader::Zlib {
                version,
            },
        ) => {
            write_u32(writer, *version)?;
            let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(&payload.data)?;
            let encoded = encoder.finish()?;
            writer.write_all(&encoded)?;
        }
        (
            Algorithm::Zstd,
            AlgorithmHeader::Zstd {
                version,
                dictionary,
            },
        ) => {
            expect(*dictionary == 0, "dictionaries are not supported")?;
            write_u32(writer, *version)?;
            write_u32(writer, *dictionary)?;
            let encoded = zstd::stream::encode_all(Cursor::new(&payload.data), 0)?;
            writer.write_all(&encoded)?;
        }
        _ => {
            return Err(CompressedBufError::msg(
                "algorithm header does not match algorithm",
            ));
        }
    }

    debug!(algorithm = ?payload.algorithm, len = payload.data.len(), "compressed buffer payload");
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

#[cfg(test)]
mod tests {
    use crate::{
        Algorithm, AlgorithmHeader, CompressedBufPayload, make_magic, read_payload_bytes,
        write_payload_bytes,
    };

    #[test]
    fn payload_roundtrip_reuses_original_bytes_when_unchanged() {
        let magic = make_magic("TEST").expect("magic");
        let original = crate::compress_bytes(b"fixture", Algorithm::Zlib, magic).expect("compress");

        let payload = read_payload_bytes(&original, magic).expect("read payload");
        let rewritten = write_payload_bytes(&payload).expect("write payload");

        assert_eq!(rewritten, original);
    }

    #[test]
    fn payload_edit_preserves_header_shape() {
        let magic = make_magic("TEST").expect("magic");
        let original = crate::compress_bytes(b"fixture", Algorithm::Zstd, magic).expect("compress");

        let mut payload = read_payload_bytes(&original, magic).expect("read payload");
        payload.data = b"changed".to_vec();

        let rewritten = write_payload_bytes(&payload).expect("write payload");
        let reparsed = read_payload_bytes(&rewritten, magic).expect("reparse");

        assert_eq!(reparsed.magic, magic);
        assert_eq!(reparsed.header_version, payload.header_version);
        assert_eq!(reparsed.algorithm, Algorithm::Zstd);
        assert_eq!(
            reparsed.algorithm_header,
            AlgorithmHeader::Zstd {
                version:    1,
                dictionary: 0,
            }
        );
        assert_eq!(reparsed.data, b"changed");
    }

    #[test]
    fn payload_rejects_mismatched_algorithm_header() {
        let payload = CompressedBufPayload {
            magic:            make_magic("TEST").expect("magic"),
            header_version:   3,
            algorithm:        Algorithm::Zlib,
            algorithm_header: AlgorithmHeader::None,
            data:             b"fixture".to_vec(),
            original_bytes:   None,
        };

        let error = write_payload_bytes(&payload).expect_err("expected mismatched header failure");
        assert!(error.to_string().contains("algorithm header"));
    }
}
