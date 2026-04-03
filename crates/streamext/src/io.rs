use crate::SizePrefix;
use std::io::{self, Read, Write};
use tracing::instrument;

/// Reads a byte buffer prefixed by a little-endian length.
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
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_bytes<R: Read>(reader: &mut R, size: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Reads exactly `size` bytes and decodes them as UTF-8.
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_string<R: Read>(reader: &mut R, size: usize) -> io::Result<String> {
    let bytes = read_bytes(reader, size)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads and validates a fixed byte sequence.
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
#[instrument(level = "debug", skip_all, err, fields(size = value.len()))]
pub fn write_size_prefixed_string<P, W>(writer: &mut W, value: &str) -> io::Result<()>
where
    P: SizePrefix,
    W: Write,
{
    write_size_prefixed_bytes::<P, _>(writer, value.as_bytes())
}

/// Writes a sequence prefixed by its element count.
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
