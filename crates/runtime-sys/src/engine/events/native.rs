use std::ffi::c_void;

use super::super::{EngineThreadToken, active_engine};

pub(super) const OBJECT_INVALID: u32 = 0x7f00_0000;
pub(super) const OBJECT_TYPE_CREATURE: u8 = 5;
pub(super) const OBJECT_TYPE_ITEM: u8 = 6;
pub(super) const OBJECT_TYPE_PLACEABLE: u8 = 9;

type GetGameObject = extern "C" fn(*mut c_void, u32) -> *mut c_void;
type GetNwsMessage = extern "C" fn(*mut c_void) -> *mut c_void;

pub(super) fn read_field<T: Copy>(object: *const c_void, layout: &str) -> Option<T> {
    if object.is_null() {
        return None;
    }
    let offset = active_engine()?.event_layout_offset(layout)?;
    // SAFETY: each caller names a compiler-derived field layout whose type is
    // checked by the Unified ABI probe, and calls synchronously in its hook.
    Some(unsafe { object.cast::<u8>().add(offset).cast::<T>().read_unaligned() })
}

pub(super) fn write_field<T: Copy>(object: *mut c_void, layout: &str, value: T) -> bool {
    if object.is_null() {
        return false;
    }
    let Some(offset) = active_engine().and_then(|engine| engine.event_layout_offset(layout)) else {
        return false;
    };
    // SAFETY: the target pack and ABI probe bind this name to the writable
    // field used by Unified for the same synchronous engine operation.
    unsafe {
        object
            .cast::<u8>()
            .add(offset)
            .cast::<T>()
            .write_unaligned(value);
    }
    true
}

pub(super) fn stats_creature(stats: *mut c_void) -> Option<*mut c_void> {
    read_field(stats, "creature_stats_base_creature")
}

pub(super) fn repository_parent(repository: *mut c_void) -> Option<u32> {
    read_field(repository, "item_repository_parent")
}

pub(super) fn object_type(object: *mut c_void) -> Option<u8> {
    read_field(object, "game_object_type")
}

pub(super) fn get_game_object(object_id: u32) -> Option<*mut c_void> {
    call_server_function("server_get_game_object", object_id)
}

pub(super) fn get_client_object(object_id: u32) -> Option<*mut c_void> {
    call_server_function("server_get_client_object", object_id)
}

pub(super) fn get_nws_message() -> Option<*mut c_void> {
    let engine = active_engine()?;
    let function = engine.event_function::<GetNwsMessage>("server_get_nws_message")?;
    // SAFETY: this token is scoped to the synchronous native engine hook.
    let thread = unsafe { EngineThreadToken::new() };
    let server = engine.event_server(&thread).ok()?;
    let message = function.get()(server);
    (!message.is_null()).then_some(message)
}

fn call_server_function(name: &str, object_id: u32) -> Option<*mut c_void> {
    let engine = active_engine()?;
    let function = engine.event_function::<GetGameObject>(name)?;
    // SAFETY: this token is scoped to the synchronous native engine hook.
    let thread = unsafe { EngineThreadToken::new() };
    let server = engine.event_server(&thread).ok()?;
    let object = function.get()(server, object_id);
    (!object.is_null()).then_some(object)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub(super) enum MessageReadError {
    InvalidWidth,
    LengthExceeded,
    Overflow,
    Underflow,
}

/// Bounded snapshot of one engine message cursor.
///
/// NWN serializes the least-significant bit first within each byte. Typed
/// methods consume from a private cursor so event families cannot perform
/// unbounded or unaligned pointer reads.
#[derive(Clone, Copy)]
pub(super) struct MessageReader {
    buffer:        *const u8,
    total_bits:    usize,
    position_bits: usize,
}

#[allow(dead_code)]
impl MessageReader {
    const MAX_STRING_BYTES: usize = 64 * 1024;

    pub(super) fn from_engine(message: *const c_void) -> Option<Self> {
        let buffer: *const u8 = read_field(message, "message_read_buffer")?;
        let size = usize::try_from(read_field::<u32>(message, "message_read_buffer_size")?).ok()?;
        let position =
            usize::try_from(read_field::<u32>(message, "message_read_buffer_position")?).ok()?;
        let fragment_size = read_field::<u32>(message, "message_read_fragments_size")?;
        let fragment_position = read_field::<u32>(message, "message_read_fragments_position")?;
        let current_bit = read_field::<u8>(message, "message_current_read_bit")?;
        let last_byte_bits = read_field::<u8>(message, "message_last_byte_bits")?;
        if position > size
            || fragment_position > fragment_size
            || current_bit > 7
            || last_byte_bits > 8
            || (buffer.is_null() && size != 0)
        {
            return None;
        }
        let total_bits = if size == 0 {
            0
        } else {
            let last_bits = if last_byte_bits == 0 {
                u8::BITS as usize
            } else {
                usize::from(last_byte_bits)
            };
            size.checked_sub(1)?
                .checked_mul(u8::BITS as usize)?
                .checked_add(last_bits)?
        };
        let position_bits = if position == size {
            total_bits
        } else {
            position
                .checked_mul(u8::BITS as usize)?
                .checked_add(usize::from(current_bit))?
        };
        (position_bits <= total_bits).then_some(Self {
            buffer,
            total_bits,
            position_bits,
        })
    }

    pub(super) const fn remaining_bits(&self) -> usize {
        self.total_bits - self.position_bits
    }

    pub(super) const fn is_byte_aligned(&self) -> bool {
        self.position_bits.is_multiple_of(u8::BITS as usize)
    }

    pub(super) fn read_bits(&mut self, count: u8) -> Result<u64, MessageReadError> {
        if count > 64 {
            return Err(MessageReadError::InvalidWidth);
        }
        let end = self
            .position_bits
            .checked_add(usize::from(count))
            .ok_or(MessageReadError::Overflow)?;
        if end > self.total_bits {
            return Err(MessageReadError::Underflow);
        }
        let mut value = 0_u64;
        for output_bit in 0..count {
            let position = self.position_bits + usize::from(output_bit);
            let byte_index = position / u8::BITS as usize;
            let bit_index = position % u8::BITS as usize;
            // SAFETY: end is bounded by total_bits, which was derived from the
            // engine allocation size at reader construction.
            let byte = unsafe { self.buffer.add(byte_index).read() };
            value |= u64::from((byte >> bit_index) & 1) << output_bit;
        }
        self.position_bits = end;
        Ok(value)
    }

    pub(super) fn read_unsigned(&mut self, count: u8) -> Result<u64, MessageReadError> {
        self.read_bits(count)
    }

    pub(super) fn read_signed(&mut self, count: u8) -> Result<i64, MessageReadError> {
        if count == 0 || count > 64 {
            return Err(MessageReadError::InvalidWidth);
        }
        let value = self.read_bits(count)?;
        if count == 64 {
            return Ok(i64::from_ne_bytes(value.to_ne_bytes()));
        }
        let sign = 1_u64 << (count - 1);
        let extended = if value & sign == 0 {
            value
        } else {
            value | (!0_u64 << count)
        };
        Ok(i64::from_ne_bytes(extended.to_ne_bytes()))
    }

    pub(super) fn read_bool(&mut self) -> Result<bool, MessageReadError> {
        self.read_bits(1).map(|value| value != 0)
    }

    pub(super) fn read_u8(&mut self) -> Result<u8, MessageReadError> {
        u8::try_from(self.read_bits(8)?).map_err(|_error| MessageReadError::Overflow)
    }

    pub(super) fn read_u16_le(&mut self) -> Result<u16, MessageReadError> {
        u16::try_from(self.read_bits(16)?).map_err(|_error| MessageReadError::Overflow)
    }

    pub(super) fn read_u32_le(&mut self) -> Result<u32, MessageReadError> {
        u32::try_from(self.read_bits(32)?).map_err(|_error| MessageReadError::Overflow)
    }

    pub(super) fn read_i32_le(&mut self) -> Result<i32, MessageReadError> {
        i32::try_from(self.read_signed(32)?).map_err(|_error| MessageReadError::Overflow)
    }

    pub(super) fn read_f32_le(&mut self) -> Result<f32, MessageReadError> {
        self.read_u32_le().map(f32::from_bits)
    }

    pub(super) fn read_object_id(&mut self) -> Result<u32, MessageReadError> {
        self.read_u32_le()
    }

    pub(super) fn read_bytes(&mut self, length: usize) -> Result<Vec<u8>, MessageReadError> {
        let bit_length = length
            .checked_mul(u8::BITS as usize)
            .ok_or(MessageReadError::Overflow)?;
        if bit_length > self.remaining_bits() {
            return Err(MessageReadError::Underflow);
        }
        let mut bytes = Vec::with_capacity(length);
        for _ in 0..length {
            bytes.push(self.read_u8()?);
        }
        Ok(bytes)
    }

    pub(super) fn read_length_prefixed_bytes(
        &mut self,
        length_bits: u8,
        maximum: usize,
    ) -> Result<Vec<u8>, MessageReadError> {
        let length = usize::try_from(self.read_unsigned(length_bits)?)
            .map_err(|_error| MessageReadError::LengthExceeded)?;
        if length > maximum {
            return Err(MessageReadError::LengthExceeded);
        }
        self.read_bytes(length)
    }

    pub(super) fn read_string(&mut self, length_bits: u8) -> Result<Vec<u8>, MessageReadError> {
        self.read_length_prefixed_bytes(length_bits, Self::MAX_STRING_BYTES)
    }

    pub(super) fn read_resref(&mut self, length_bits: u8) -> Result<Vec<u8>, MessageReadError> {
        let mut value = self.read_length_prefixed_bytes(length_bits, 16)?;
        if let Some(nul) = value.iter().position(|byte| *byte == 0) {
            value.truncate(nul);
        }
        Ok(value)
    }

    pub(super) fn skip_bits(&mut self, count: usize) -> Result<(), MessageReadError> {
        let end = self
            .position_bits
            .checked_add(count)
            .ok_or(MessageReadError::Overflow)?;
        if end > self.total_bits {
            return Err(MessageReadError::Underflow);
        }
        self.position_bits = end;
        Ok(())
    }

    pub(super) fn byte(&self, delta: usize) -> Option<u8> {
        let mut reader = *self;
        reader
            .skip_bits(delta.checked_mul(u8::BITS as usize)?)
            .ok()?;
        reader.read_u8().ok()
    }

    pub(super) fn u32_le(&self, delta: usize) -> Option<u32> {
        let mut reader = *self;
        reader
            .skip_bits(delta.checked_mul(u8::BITS as usize)?)
            .ok()?;
        reader.read_u32_le().ok()
    }
}

pub(super) fn message_reader(message: *const c_void) -> Option<MessageReader> {
    MessageReader::from_engine(message)
}

pub(super) fn clear_read_message(message: *mut c_void) -> bool {
    let Some(read_size) = read_field::<u32>(message, "message_read_buffer_size") else {
        return false;
    };
    let Some(fragment_size) = read_field::<u32>(message, "message_read_fragments_size") else {
        return false;
    };
    let Some(last_bits) = read_field::<u8>(message, "message_last_byte_bits") else {
        return false;
    };
    write_field(message, "message_read_buffer_position", read_size)
        && write_field(message, "message_read_fragments_position", fragment_size)
        && write_field(message, "message_current_read_bit", last_bits)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn message_reader_uses_explicit_little_endian_bounded_reads() {
        let bytes = [0x78, 0x56, 0x34, 0x12, 0xab];
        let reader = MessageReader {
            buffer:        bytes.as_ptr(),
            total_bits:    bytes.len() * u8::BITS as usize,
            position_bits: 0,
        };
        assert_eq!(reader.u32_le(0), Some(0x1234_5678));
        assert_eq!(reader.byte(4), Some(0xab));
        assert_eq!(reader.u32_le(2), None);
        assert_eq!(reader.byte(5), None);
    }

    #[test]
    fn message_reader_handles_bit_alignment_and_signed_values() {
        let bytes = [0b1011_0110, 0b0000_0011];
        let mut reader = MessageReader {
            buffer:        bytes.as_ptr(),
            total_bits:    10,
            position_bits: 0,
        };
        assert!(!reader.read_bool().expect("first bit"));
        assert_eq!(reader.read_unsigned(3), Ok(3));
        assert_eq!(reader.read_signed(4), Ok(-5));
        assert_eq!(reader.read_unsigned(2), Ok(3));
        assert_eq!(reader.remaining_bits(), 0);
        assert_eq!(reader.read_bool(), Err(MessageReadError::Underflow));
    }

    #[test]
    fn message_reader_decodes_scalars_strings_and_resrefs() {
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&42_u32.to_le_bytes());
        bytes.extend_from_slice(&1.5_f32.to_bits().to_le_bytes());
        bytes.push(5);
        bytes.extend_from_slice(b"hello");
        bytes.push(4);
        bytes.extend_from_slice(b"test");
        let mut reader = MessageReader {
            buffer:        bytes.as_ptr(),
            total_bits:    bytes.len() * u8::BITS as usize,
            position_bits: 0,
        };
        assert_eq!(reader.read_object_id(), Ok(42));
        assert_eq!(reader.read_f32_le(), Ok(1.5));
        assert_eq!(reader.read_string(8), Ok(b"hello".to_vec()));
        assert_eq!(reader.read_resref(8), Ok(b"test".to_vec()));
        assert!(reader.is_byte_aligned());
        assert_eq!(reader.remaining_bits(), 0);
    }

    #[test]
    fn message_reader_rejects_oversized_lengths_without_reading_past_bounds() {
        let bytes = [17_u8];
        let mut reader = MessageReader {
            buffer:        bytes.as_ptr(),
            total_bits:    bytes.len() * u8::BITS as usize,
            position_bits: 0,
        };
        assert_eq!(reader.read_resref(8), Err(MessageReadError::LengthExceeded));
        assert_eq!(reader.remaining_bits(), 0);
    }
}
