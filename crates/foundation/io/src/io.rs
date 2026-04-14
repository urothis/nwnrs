use std::io::{self, Read};

use tracing::instrument;

use crate::ExpectationError;

/// Reads exactly `size` bytes or returns the underlying IO error.
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_bytes_or_err<R: Read + ?Sized>(reader: &mut R, size: usize) -> io::Result<Vec<u8>> {
    let mut bytes = vec![0_u8; size];
    reader.read_exact(&mut bytes)?;
    Ok(bytes)
}

/// Reads exactly `size` bytes and decodes them as UTF-8.
#[instrument(level = "debug", skip_all, err, fields(size))]
pub fn read_str_or_err<R: Read + ?Sized>(reader: &mut R, size: usize) -> io::Result<String> {
    let bytes = read_bytes_or_err(reader, size)?;
    String::from_utf8(bytes).map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

/// Reads exactly `count` items, passing each zero-based index to `item_reader`.
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
pub fn swap_endian<T: SwappableEndian>(value: T) -> T {
    value.swap_endian()
}

/// Maps a slice while passing the zero-based index to the mapping function.
pub fn map_with_index<T, R, F>(data: &[T], mut op: F) -> Vec<R>
where
    F: FnMut(usize, &T) -> R,
{
    data.iter()
        .enumerate()
        .map(|(idx, item)| op(idx, item))
        .collect()
}
