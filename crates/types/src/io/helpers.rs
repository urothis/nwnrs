use std::io::{self, Read};

use tracing::instrument;

use crate::io::ExpectationError;

/// Reads exactly `size` bytes or returns the underlying IO error.
///
/// # Errors
///
/// Returns an [`io::Error`] if the reader cannot supply exactly `size` bytes.
///
/// # Examples
///
/// ```
/// let mut reader = std::io::Cursor::new(*b"hello");
/// let bytes = nwnrs_types::io::read_bytes_or_err(&mut reader, 5)?;
/// assert_eq!(bytes, b"hello");
/// # Ok::<(), std::io::Error>(())
/// ```
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_bytes_or_err<R: Read + ?Sized>(reader: &mut R, size: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Reads exactly `size` bytes and decodes them as UTF-8.
///
/// # Errors
///
/// Returns an [`io::Error`] if the reader cannot supply exactly `size` bytes or
/// the bytes are not valid UTF-8.
///
/// # Examples
///
/// ```
/// let mut reader = std::io::Cursor::new(*b"tile");
/// let text = nwnrs_types::io::read_str_or_err(&mut reader, 4)?;
/// assert_eq!(text, "tile");
/// # Ok::<(), std::io::Error>(())
/// ```
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_str_or_err<R: Read + ?Sized>(reader: &mut R, size: usize) -> io::Result<String> {
    let bytes = read_bytes_or_err(reader, size)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads exactly `count` items, passing each zero-based index to `item_reader`.
///
/// # Errors
///
/// Returns an [`io::Error`] if `item_reader` fails for any item.
///
/// # Examples
///
/// ```
/// let mut reader = std::io::Cursor::new([1_u8, 2, 3]);
/// let values = nwnrs_types::io::read_fixed_count_seq(&mut reader, 3, |_idx, reader| {
///     let mut byte = [0_u8; 1];
///     std::io::Read::read_exact(reader, &mut byte)?;
///     Ok(byte[0])
/// })?;
/// assert_eq!(values, vec![1, 2, 3]);
/// # Ok::<(), std::io::Error>(())
/// ```
#[instrument(level = "debug", skip_all, err, fields(count))]
pub fn read_fixed_count_seq<R, T, F>(
    reader: &mut R,
    count: usize,
    mut item_reader: F,
) -> io::Result<Vec<T>>
where
    R: Read,
    F: FnMut(usize, &mut R) -> io::Result<T>,
{
    let mut result = Vec::with_capacity(count);
    for idx in 0..count {
        result.push(item_reader(idx, reader)?);
    }
    Ok(result)
}

/// Returns `Ok(())` when `condition` is true, otherwise an
/// [`ExpectationError`].
///
/// # Errors
///
/// Returns [`ExpectationError`] if `condition` is false.
///
/// # Examples
///
/// ```
/// assert!(nwnrs_types::io::expect(true, "reachable").is_ok());
/// assert!(nwnrs_types::io::expect(false, "boom").is_err());
/// ```
pub fn expect(condition: bool, message: impl Into<String>) -> Result<(), ExpectationError> {
    if condition {
        Ok(())
    } else {
        Err(ExpectationError::new(message))
    }
}

/// A value that can be byte-swapped.
pub trait SwappableEndian: Sized {
    /// Returns the value with its byte order reversed.
    #[must_use]
    fn swap_endian(self) -> Self;
}

macro_rules! impl_swappable_int {
    ($($ty:ty),+ $(,)?) => {
        $(
            impl SwappableEndian for $ty {
                fn swap_endian(self) -> Self {
                    self.swap_bytes()
                }
            }
        )+
    };
}

impl_swappable_int!(
    u8, i8, u16, i16, u32, i32, u64, i64, u128, i128, usize, isize
);

impl SwappableEndian for f32 {
    fn swap_endian(self) -> Self {
        Self::from_bits(self.to_bits().swap_bytes())
    }
}

impl SwappableEndian for f64 {
    fn swap_endian(self) -> Self {
        Self::from_bits(self.to_bits().swap_bytes())
    }
}

/// Swaps the byte order of `value`.
///
/// # Examples
///
/// ```
/// assert_eq!(nwnrs_types::io::swap_endian(0x1234_u16), 0x3412);
/// ```
pub fn swap_endian<T: SwappableEndian>(value: T) -> T {
    value.swap_endian()
}

/// Maps a slice while passing the zero-based index to the mapping function.
///
/// # Examples
///
/// ```
/// let mapped = nwnrs_types::io::map_with_index(&["a", "b", "c"], |idx, item| {
///     format!("{idx}:{item}")
/// });
/// assert_eq!(mapped, vec!["0:a", "1:b", "2:c"]);
/// ```
pub fn map_with_index<T, R, F>(data: &[T], mut op: F) -> Vec<R>
where
    F: FnMut(usize, &T) -> R,
{
    data.iter()
        .enumerate()
        .map(|(idx, item)| op(idx, item))
        .collect()
}
