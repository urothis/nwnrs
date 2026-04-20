use std::io::{self, Read, Write};

use tracing::instrument;

use crate::prelude::*;

/// Reads a byte buffer prefixed by a little-endian length.
///
/// # Errors
///
/// Returns an [`io::Error`] if the prefix or the subsequent bytes cannot be read, or if the prefix length does not match `expect_fixed_size`.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(expect_fixed_size = expect_fixed_size)
)]
pub fn read_size_prefixed_bytes<P, R>(
    reader: &mut R,
    expect_fixed_size: Option<usize>,
) -> io::Result<Vec<u8>>
where
    P: SizePrefix,
    R: Read,
{
    let prefix = P::read_from(reader)?;
    let prefix_len = prefix.as_usize();
    if let Some(expected) = expect_fixed_size
        && prefix_len != expected
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("expected a size of {expected}, but got {prefix_len}"),
        ));
    }

    read_bytes(reader, prefix_len)
}

/// Reads a UTF-8 string prefixed by a little-endian length.
///
/// # Errors
///
/// Returns an [`io::Error`] if the bytes cannot be read or are not valid UTF-8.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(expect_fixed_size = expect_fixed_size)
)]
pub fn read_size_prefixed_string<P, R>(
    reader: &mut R,
    expect_fixed_size: Option<usize>,
) -> io::Result<String>
where
    P: SizePrefix,
    R: Read,
{
    let bytes = read_size_prefixed_bytes::<P, _>(reader, expect_fixed_size)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads exactly `size` bytes.
///
/// # Errors
///
/// Returns an [`io::Error`] if the reader cannot supply exactly `size` bytes.
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_bytes<R: Read>(reader: &mut R, size: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Reads exactly `size` bytes and decodes them as UTF-8.
///
/// # Errors
///
/// Returns an [`io::Error`] if the reader cannot supply exactly `size` bytes or the bytes are not valid UTF-8.
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_string<R: Read>(reader: &mut R, size: usize) -> io::Result<String> {
    let bytes = read_bytes(reader, size)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads and validates a fixed byte sequence.
///
/// # Errors
///
/// Returns an [`io::Error`] if the bytes cannot be read or do not match `value`.
#[instrument(level = "debug", skip_all, err, fields(size = value.len()))]
pub fn read_fixed_value<R: Read>(reader: &mut R, value: &[u8]) -> io::Result<()> {
    let data = read_bytes(reader, value.len())?;
    if data != value {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "wanted to read fixed value {:?}, but got {:?}",
                String::from_utf8_lossy(value),
                String::from_utf8_lossy(&data)
            ),
        ));
    }
    Ok(())
}

/// Reads exactly `count` elements using `item_reader`.
///
/// # Errors
///
/// Returns an [`io::Error`] if `item_reader` fails for any element.
#[instrument(level = "debug", skip_all, err, fields(count))]
pub fn read_fixed_count_seq<R, T, F>(
    reader: &mut R,
    count: usize,
    mut item_reader: F,
) -> io::Result<Vec<T>>
where
    R: Read,
    F: FnMut(&mut R) -> io::Result<T>,
{
    let mut result = Vec::with_capacity(count);
    for _ in 0..count {
        result.push(item_reader(reader)?);
    }
    Ok(result)
}

/// Reads a sequence prefixed by an element count.
///
/// # Errors
///
/// Returns an [`io::Error`] if the prefix or any element cannot be read.
#[instrument(level = "debug", skip_all, err)]
pub fn read_size_prefixed_seq<P, R, T, F>(reader: &mut R, item_reader: F) -> io::Result<Vec<T>>
where
    P: SizePrefix,
    R: Read,
    F: FnMut(&mut R) -> io::Result<T>,
{
    let prefix = P::read_from(reader)?;
    read_fixed_count_seq(reader, prefix.as_usize(), item_reader)
}

/// Writes a byte buffer prefixed by its length.
///
/// # Errors
///
/// Returns an [`io::Error`] if the length does not fit the prefix type or the write fails.
#[instrument(level = "debug", skip_all, err, fields(size = value.len()))]
pub fn write_size_prefixed_bytes<P, W>(writer: &mut W, value: &[u8]) -> io::Result<()>
where
    P: SizePrefix,
    W: Write,
{
    let prefix = P::try_from(value.len()).map_err(|_error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "value length does not fit into the selected prefix type",
        )
    })?;
    prefix.write_to(writer)?;
    writer.write_all(value)
}

/// Writes a UTF-8 string prefixed by its byte length.
///
/// # Errors
///
/// Returns an [`io::Error`] if the byte length does not fit the prefix type or the write fails.
#[instrument(level = "debug", skip_all, err, fields(size = value.len()))]
pub fn write_size_prefixed_string<P, W>(writer: &mut W, value: &str) -> io::Result<()>
where
    P: SizePrefix,
    W: Write,
{
    write_size_prefixed_bytes::<P, _>(writer, value.as_bytes())
}

/// Writes a sequence prefixed by its element count.
///
/// # Errors
///
/// Returns an [`io::Error`] if the element count does not fit the prefix type or any write fails.
#[instrument(level = "debug", skip_all, err, fields(entry_count = elements.len()))]
pub fn write_size_prefixed_seq<P, W, T, F>(
    writer: &mut W,
    elements: &[T],
    mut item_writer: F,
) -> io::Result<()>
where
    P: SizePrefix,
    W: Write,
    F: FnMut(&mut W, &T) -> io::Result<()>,
{
    let prefix = P::try_from(elements.len()).map_err(|_error| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "element count does not fit into the selected prefix type",
        )
    })?;
    prefix.write_to(writer)?;
    for element in elements {
        item_writer(writer, element)?;
    }
    Ok(())
}

/// Reads an array of exactly `N` elements.
///
/// # Errors
///
/// Returns an [`io::Error`] if `item_reader` fails for any element.
#[instrument(level = "debug", skip_all, err, fields(count = N))]
pub fn read_array<const N: usize, R, T, F>(reader: &mut R, mut item_reader: F) -> io::Result<[T; N]>
where
    R: Read,
    F: FnMut(&mut R) -> io::Result<T>,
{
    let mut items = Vec::with_capacity(N);
    for _ in 0..N {
        items.push(item_reader(reader)?);
    }
    items.try_into().map_err(|_error| {
        io::Error::other("internal error: collected array length did not match requested size")
    })
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::io::{Cursor, Read, Write};

    use crate::{
        read_fixed_value, read_size_prefixed_seq, write_size_prefixed_seq,
        write_size_prefixed_string,
    };

    #[test]
    fn size_prefixed_sequences_roundtrip() {
        let mut bytes = Vec::new();
        if let Err(error) =
            write_size_prefixed_seq::<u16, _, _, _>(&mut bytes, &[1_u8, 2, 3], |w, v| {
                w.write_all(&[*v])
            })
        {
            panic!("write seq: {error}");
        }

        let values = match read_size_prefixed_seq::<u16, _, _, _>(&mut Cursor::new(bytes), |r| {
            let mut byte = [0_u8; 1];
            r.read_exact(&mut byte)?;
            Ok(byte[0])
        }) {
            Ok(values) => values,
            Err(error) => panic!("read seq: {error}"),
        };
        assert_eq!(values, vec![1, 2, 3]);
    }

    #[test]
    fn fixed_value_reports_mismatch() {
        let mut bytes = Vec::new();
        if let Err(error) = write_size_prefixed_string::<u8, _>(&mut bytes, "ok") {
            panic!("write string: {error}");
        }
        let error = match read_fixed_value(&mut Cursor::new(bytes), b"no") {
            Ok(()) => panic!("fixed value mismatch should fail"),
            Err(error) => error,
        };
        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    }
}
