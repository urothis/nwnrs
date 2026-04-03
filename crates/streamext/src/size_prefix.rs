use std::io::{self, Read, Write};

/// A length prefix type used by the size-prefixed helpers in this crate.
pub trait SizePrefix: Copy + TryFrom<usize> {
    /// Reads a prefix value from `reader` using little-endian encoding.
    fn read_from<R: Read>(reader: &mut R) -> io::Result<Self>;
    /// Writes a prefix value to `writer` using little-endian encoding.
    fn write_to<W: Write>(self, writer: &mut W) -> io::Result<()>;
    /// Converts the prefix value into a `usize`.
    fn as_usize(self) -> usize;
}

macro_rules! impl_size_prefix {
    ($ty:ty) => {
        impl SizePrefix for $ty {
            fn read_from<R: Read>(reader: &mut R) -> io::Result<Self> {
                let mut bytes = [0_u8; std::mem::size_of::<$ty>()];
                reader.read_exact(&mut bytes)?;
                Ok(<$ty>::from_le_bytes(bytes))
            }

            fn write_to<W: Write>(self, writer: &mut W) -> io::Result<()> {
                writer.write_all(&self.to_le_bytes())
            }

            fn as_usize(self) -> usize {
                usize::try_from(self).unwrap_or(usize::MAX)
            }
        }
    };
}

impl_size_prefix!(u8);
impl_size_prefix!(u16);
impl_size_prefix!(u32);
impl_size_prefix!(u64);
