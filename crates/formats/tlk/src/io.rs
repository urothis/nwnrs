use std::io::{self, Cursor, Read, Seek, SeekFrom, Write};

use nwnrs_encoding::prelude::*;
use nwnrs_io::prelude::*;
use nwnrs_localization::prelude::*;
use nwnrs_lru::prelude::*;
use nwnrs_resman::prelude::*;
use tracing::{debug, instrument};

use crate::{
    DATA_ELEMENT_SIZE, HEADER_SIZE, SingleTlk, Tlk, TlkEntry, TlkError, TlkResult,
    decode_sound_res_ref,
};

/// Trait object helper for TLK write targets that must support both writing
/// and seeking.
pub trait TlkWriteStream: Write + Seek {}

impl<T: Write + Seek + ?Sized> TlkWriteStream for T {}

/// Write targets for one male/female TLK layer in a chain.
pub struct TlkLayerWriteTarget<'a> {
    /// Writer for the male TLK table, when present.
    pub male:   Option<&'a mut dyn TlkWriteStream>,
    /// Writer for the female TLK table, when present.
    pub female: Option<&'a mut dyn TlkWriteStream>,
}

/// Reads a single-language TLK table from a reader.
#[instrument(level = "debug", skip_all, err, fields(cache_policy = ?cache_policy))]
pub fn read_single_tlk<R>(mut reader: R, cache_policy: CachePolicy) -> TlkResult<SingleTlk>
where
    R: Read + Seek + Send + 'static,
{
    let start = reader.stream_position()?;
    reader.seek(SeekFrom::Start(start))?;
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let stream = shared_stream(Cursor::new(bytes.clone()));
    let mut locked = stream
        .lock()
        .map_err(|error| TlkError::msg(format!("tlk stream lock poisoned: {error}")))?;

    expect_header(&mut *locked, b"TLK ")?;
    expect_header(&mut *locked, b"V3.0")?;
    let language_id = u32::try_from(read_i32(&mut *locked)?)
        .map_err(|_error| TlkError::msg("invalid negative tlk language id"))?;
    let language = Language::from_id(language_id)
        .ok_or_else(|| TlkError::msg(format!("invalid tlk language id {language_id}")))?;
    let entry_count = usize::try_from(read_i32(&mut *locked)?)
        .map_err(|_error| TlkError::msg("invalid negative tlk entry count"))?;
    let entries_offset = u64::try_from(read_i32(&mut *locked)?)
        .map_err(|_error| TlkError::msg("invalid negative tlk entries offset"))?;
    drop(locked);

    let mut result = SingleTlk::new();
    result.language = language;
    result.stream = Some(stream);
    result.io_start_pos = 0;
    result.io_entry_count = entry_count;
    result.io_entries_offset = entries_offset;
    result.source_bytes = Some(bytes);
    result.source_language = Some(language);
    result.cache_policy = cache_policy;
    result.io_cache = cache_policy
        .uses_cache()
        .then(|| WeightedLru::new(std::mem::size_of::<TlkEntry>() * entry_count.max(1) / 2, 1));
    debug!(entry_count = result.io_entry_count, language = ?result.language, "read tlk");
    Ok(result)
}

/// Writes a single-language TLK table.
///
/// Missing string references up to [`SingleTlk::highest`] are emitted as empty
/// entries.
#[instrument(level = "debug", skip_all, err, fields(language = ?tlk.language))]
pub fn write_single_tlk<W: Write + Seek>(writer: &mut W, tlk: &mut SingleTlk) -> TlkResult<()> {
    if tlk.static_entries.is_empty()
        && tlk
            .source_language
            .is_some_and(|source_language| source_language == tlk.language)
        && let Some(source_bytes) = &tlk.source_bytes
    {
        writer.write_all(source_bytes)?;
        return Ok(());
    }

    let max_id = u32::try_from(tlk.highest().max(0))
        .map_err(|_error| TlkError::msg("TLK highest string reference exceeds 32-bit range"))?;
    let entry_count = max_id + 1;
    let entries_table_offset = writer.stream_position()? + HEADER_SIZE;
    let entries_table_size = DATA_ELEMENT_SIZE * u64::from(entry_count);
    let string_data_offset = entries_table_offset + entries_table_size;

    writer.write_all(b"TLK ")?;
    writer.write_all(b"V3.0")?;
    write_i32(
        writer,
        i32::try_from(tlk.language.id())
            .map_err(|_error| TlkError::msg("TLK language id exceeds 32-bit range"))?,
    )?;
    write_u32(writer, entry_count)?;
    write_u32(
        writer,
        u32::try_from(string_data_offset)
            .map_err(|_error| TlkError::msg("TLK string data offset exceeds 32-bit range"))?,
    )?;

    let current_pos = writer.stream_position()?;
    if current_pos < string_data_offset {
        let padding_len = usize::try_from(string_data_offset - current_pos)
            .map_err(|_error| TlkError::msg("TLK padding length exceeds usize"))?;
        writer.write_all(&vec![0_u8; padding_len])?;
    }

    let entries_capacity = usize::try_from(entries_table_size)
        .map_err(|_error| TlkError::msg("TLK entries table exceeds usize"))?;
    let mut entries_table = Cursor::new(Vec::with_capacity(entries_capacity));
    let mut offset = 0_i32;
    for index in 0..entry_count {
        if let Some(entry) = tlk.get(index)?.filter(TlkEntry::has_value) {
            write_i32(&mut entries_table, entry.stored_flags())?;
            entries_table.write_all(&entry.stored_sound_res_ref_bytes()?)?;
            write_i32(&mut entries_table, entry.volume_variance)?;
            write_i32(&mut entries_table, entry.pitch_variance)?;

            let text = entry.stored_text_bytes()?;
            write_i32(&mut entries_table, offset)?;
            let text_len = i32::try_from(text.len())
                .map_err(|_error| TlkError::msg("TLK text length exceeds 32-bit range"))?;
            write_i32(&mut entries_table, text_len)?;
            offset = offset
                .checked_add(text_len)
                .ok_or_else(|| TlkError::msg("TLK text offset overflow"))?;
            write_f32(
                &mut entries_table,
                f32::from_bits(entry.stored_sound_length_bits()),
            )?;

            writer.write_all(&text)?;
        } else {
            write_i32(&mut entries_table, 0)?;
            entries_table.write_all(&[0_u8; 16])?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;
            write_i32(&mut entries_table, 0)?;
            write_f32(&mut entries_table, 0.0)?;
        }
    }

    writer.seek(SeekFrom::Start(entries_table_offset))?;
    entries_table.set_position(0);
    writer.write_all(entries_table.get_ref())?;
    debug!(entry_count, "wrote tlk");
    Ok(())
}

/// Writes a layered TLK chain to explicit male/female layer targets.
#[instrument(level = "debug", skip_all, err, fields(layer_count = tlk.chain.len()))]
pub fn write_tlk_chain(targets: &mut [TlkLayerWriteTarget<'_>], tlk: &mut Tlk) -> TlkResult<()> {
    if targets.len() != tlk.chain.len() {
        return Err(TlkError::msg(format!(
            "tlk chain has {} layers but {} write targets were provided",
            tlk.chain.len(),
            targets.len()
        )));
    }

    for (layer_index, (target, pair)) in targets.iter_mut().zip(tlk.chain.iter_mut()).enumerate() {
        write_optional_layer(
            layer_index,
            "male",
            target.male.as_mut(),
            pair.male.as_mut(),
        )?;
        write_optional_layer(
            layer_index,
            "female",
            target.female.as_mut(),
            pair.female.as_mut(),
        )?;
    }

    Ok(())
}

fn write_optional_layer(
    layer_index: usize,
    gender: &str,
    writer: Option<&mut &mut dyn TlkWriteStream>,
    tlk: Option<&mut SingleTlk>,
) -> TlkResult<()> {
    match (writer, tlk) {
        (Some(writer), Some(tlk)) => write_single_tlk(writer, tlk),
        (None, Some(_)) => Err(TlkError::msg(format!(
            "tlk layer {layer_index} has a {gender} table but no writer target was provided"
        ))),
        (Some(_), None) => Err(TlkError::msg(format!(
            "tlk layer {layer_index} has a {gender} writer target but no table to write"
        ))),
        (None, None) => Ok(()),
    }
}

pub(crate) fn get_from_io(tlk: &SingleTlk, str_ref: StrRef) -> TlkResult<(usize, TlkEntry)> {
    let stream = tlk
        .stream
        .as_ref()
        .ok_or_else(|| TlkError::msg("tlk is not stream-backed"))?;
    let mut stream = stream
        .lock()
        .map_err(|error| TlkError::msg(format!("tlk stream lock poisoned: {error}")))?;

    stream.seek(SeekFrom::Start(
        tlk.io_start_pos + HEADER_SIZE + DATA_ELEMENT_SIZE * u64::from(str_ref),
    ))?;
    let flags = read_i32(stream.as_mut())?;
    let raw_sound_res_ref = {
        let bytes = read_bytes_or_err(stream.as_mut(), 16)?;
        let mut raw = [0_u8; 16];
        raw.copy_from_slice(&bytes);
        raw
    };
    let volume_variance = read_i32(stream.as_mut())?;
    let pitch_variance = read_i32(stream.as_mut())?;
    let offset_to_string = u64::try_from(read_i32(stream.as_mut())?)
        .map_err(|_error| TlkError::msg("invalid negative tlk string offset"))?;
    let string_size = usize::try_from(read_i32(stream.as_mut())?)
        .map_err(|_error| TlkError::msg("invalid negative tlk string size"))?;
    let sound_length_bits = read_u32(stream.as_mut())?;
    let stored_sound_res_ref = decode_sound_res_ref(&raw_sound_res_ref);
    let sound_length = f32::from_bits(sound_length_bits);
    let sound_res_ref = if flags & 0x2 != 0 {
        stored_sound_res_ref.clone()
    } else {
        String::new()
    };
    let source_len = tlk
        .source_bytes
        .as_ref()
        .map_or(0_u64, |bytes| bytes.len() as u64);
    let raw_text = if flags & 0x1 != 0 && string_size > 0 {
        let string_end = tlk
            .io_start_pos
            .checked_add(tlk.io_entries_offset)
            .and_then(|offset| offset.checked_add(offset_to_string))
            .and_then(|offset| offset.checked_add(u64::try_from(string_size).ok()?))
            .ok_or_else(|| TlkError::msg("TLK string extent overflow"))?;
        if source_len > 0 && string_end > source_len {
            None
        } else {
            stream.seek(SeekFrom::Start(
                tlk.io_start_pos + tlk.io_entries_offset + offset_to_string,
            ))?;
            Some(read_bytes_or_err(stream.as_mut(), string_size)?)
        }
    } else {
        None
    };
    let text = if flags & 0x1 != 0 {
        raw_text
            .as_ref()
            .map(|bytes| from_nwnrs_encoding(bytes))
            .transpose()?
            .unwrap_or_default()
    } else {
        String::new()
    };
    let entry = TlkEntry {
        text,
        raw_text,
        sound_res_ref,
        raw_sound_res_ref,
        sound_length,
        sound_length_bits,
        flags,
        volume_variance,
        pitch_variance,
    };
    let weight = std::mem::size_of::<TlkEntry>() + entry.sound_res_ref.len() + entry.text.len();

    Ok((weight, entry))
}

fn expect_header<R: Read + ?Sized>(reader: &mut R, expected: &[u8]) -> TlkResult<()> {
    let actual = read_bytes_or_err(reader, expected.len())?;
    if actual == expected {
        Ok(())
    } else {
        Err(TlkError::msg(format!(
            "invalid tlk header: expected {:?}, got {:?}",
            String::from_utf8_lossy(expected),
            String::from_utf8_lossy(&actual)
        )))
    }
}

fn read_i32<R: Read + ?Sized>(reader: &mut R) -> io::Result<i32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(i32::from_le_bytes(bytes))
}

fn read_u32<R: Read + ?Sized>(reader: &mut R) -> io::Result<u32> {
    let mut bytes = [0_u8; 4];
    reader.read_exact(&mut bytes)?;
    Ok(u32::from_le_bytes(bytes))
}

fn write_i32<W: Write + ?Sized>(writer: &mut W, value: i32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_u32<W: Write + ?Sized>(writer: &mut W, value: u32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

fn write_f32<W: Write + ?Sized>(writer: &mut W, value: f32) -> io::Result<()> {
    writer.write_all(&value.to_le_bytes())
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use nwnrs_localization::Gender;
    use nwnrs_resman::CachePolicy;

    use super::{TlkLayerWriteTarget, read_single_tlk, write_single_tlk, write_tlk_chain};
    use crate::{SingleTlk, Tlk, TlkEntry, TlkPair};

    #[test]
    fn tlk_preserves_descriptor_fields_when_only_text_changes() {
        let mut tlk = SingleTlk::new();
        tlk.set_entry(
            0,
            TlkEntry {
                text:              "old\r\ntext".to_string(),
                raw_text:          Some(b"old\r\ntext".to_vec()),
                sound_res_ref:     "snd".to_string(),
                raw_sound_res_ref: {
                    let mut raw = [0_u8; 16];
                    raw[..7].copy_from_slice(b"snd \0 \0");
                    raw
                },
                sound_length:      f32::from_bits(0x3fa00001),
                sound_length_bits: 0x3fa00001,
                flags:             0x7,
                volume_variance:   11,
                pitch_variance:    22,
            },
        );

        let mut encoded = Cursor::new(Vec::new());
        write_single_tlk(&mut encoded, &mut tlk).expect("encode source tlk");

        let mut parsed = read_single_tlk(Cursor::new(encoded.into_inner()), CachePolicy::Bypass)
            .expect("parse tlk");
        let mut edited = parsed.get(0).expect("load entry").expect("entry present");
        edited.text = "new\r\ntext".to_string();
        parsed.set_entry(0, edited);

        let mut rewritten = Cursor::new(Vec::new());
        write_single_tlk(&mut rewritten, &mut parsed).expect("rewrite tlk");

        let mut reparsed =
            read_single_tlk(Cursor::new(rewritten.into_inner()), CachePolicy::Bypass)
                .expect("reparse tlk");
        let entry = reparsed
            .get(0)
            .expect("load rewritten entry")
            .expect("entry present");

        assert_eq!(entry.text, "new\r\ntext");
        assert_eq!(entry.flags, 0x7);
        assert_eq!(entry.volume_variance, 11);
        assert_eq!(entry.pitch_variance, 22);
        assert_eq!(entry.sound_length_bits, 0x3fa00001);
        assert_eq!(entry.raw_sound_res_ref[..7], *b"snd \0 \0");
    }

    #[test]
    fn tlk_new_entry_writes_canonical_descriptor_defaults() {
        let mut tlk = SingleTlk::new();
        tlk.set_entry(0, TlkEntry::new("fixture", "sound01", 1.25));

        let mut encoded = Cursor::new(Vec::new());
        write_single_tlk(&mut encoded, &mut tlk).expect("encode tlk");

        let mut parsed = read_single_tlk(Cursor::new(encoded.into_inner()), CachePolicy::Bypass)
            .expect("parse tlk");
        let entry = parsed.get(0).expect("load entry").expect("entry present");

        assert_eq!(entry.text, "fixture");
        assert_eq!(entry.sound_res_ref, "sound01");
        assert_eq!(entry.flags, 0x7);
        assert_eq!(entry.volume_variance, 0);
        assert_eq!(entry.pitch_variance, 0);
        assert_eq!(entry.sound_length, 1.25);
    }

    #[test]
    fn tlk_chain_writes_each_layer_and_preserves_lookup_order() {
        let mut high_male = SingleTlk::new();
        high_male.set_entry(0, TlkEntry::new("high male", "", 0.0));

        let mut low_male = SingleTlk::new();
        low_male.set_entry(0, TlkEntry::new("low male", "", 0.0));
        let mut low_female = SingleTlk::new();
        low_female.set_entry(0, TlkEntry::new("low female", "", 0.0));

        let mut chain = Tlk::new(vec![
            TlkPair {
                male:   Some(high_male),
                female: None,
            },
            TlkPair {
                male:   Some(low_male),
                female: Some(low_female),
            },
        ]);

        let mut high_male_bytes = Cursor::new(Vec::new());
        let mut low_male_bytes = Cursor::new(Vec::new());
        let mut low_female_bytes = Cursor::new(Vec::new());
        let mut targets = [
            TlkLayerWriteTarget {
                male:   Some(&mut high_male_bytes),
                female: None,
            },
            TlkLayerWriteTarget {
                male:   Some(&mut low_male_bytes),
                female: Some(&mut low_female_bytes),
            },
        ];

        write_tlk_chain(&mut targets, &mut chain).expect("write tlk chain");

        let reparsed_high_male = read_single_tlk(
            Cursor::new(high_male_bytes.into_inner()),
            CachePolicy::Bypass,
        )
        .expect("read back high male");
        let reparsed_low_male = read_single_tlk(
            Cursor::new(low_male_bytes.into_inner()),
            CachePolicy::Bypass,
        )
        .expect("read back low male");
        let reparsed_low_female = read_single_tlk(
            Cursor::new(low_female_bytes.into_inner()),
            CachePolicy::Bypass,
        )
        .expect("read back low female");

        let mut reparsed_chain = Tlk::new(vec![
            TlkPair {
                male:   Some(reparsed_high_male),
                female: None,
            },
            TlkPair {
                male:   Some(reparsed_low_male),
                female: Some(reparsed_low_female),
            },
        ]);

        let male = reparsed_chain
            .get(0, Gender::Male)
            .expect("query male")
            .expect("male entry present");
        let female = reparsed_chain
            .get(0, Gender::Female)
            .expect("query female")
            .expect("female entry present");

        assert_eq!(male.text, "high male");
        assert_eq!(female.text, "low female");
    }

    #[test]
    fn tlk_chain_rejects_target_count_mismatch() {
        let mut tlk = Tlk::new(vec![TlkPair {
            male:   Some(SingleTlk::new()),
            female: None,
        }]);
        let mut targets: [TlkLayerWriteTarget<'_>; 0] = [];

        let error = write_tlk_chain(&mut targets, &mut tlk).unwrap_err();
        assert!(error.to_string().contains("write targets"));
    }

    #[test]
    fn tlk_chain_rejects_missing_writer_for_present_table() {
        let mut tlk = Tlk::new(vec![TlkPair {
            male:   Some(SingleTlk::new()),
            female: None,
        }]);
        let mut targets = [TlkLayerWriteTarget {
            male:   None,
            female: None,
        }];

        let error = write_tlk_chain(&mut targets, &mut tlk).unwrap_err();
        assert!(error.to_string().contains("no writer target"));
    }

    #[test]
    fn tlk_chain_rejects_writer_for_missing_table() {
        let mut tlk = Tlk::new(vec![TlkPair {
            male:   None,
            female: None,
        }]);
        let mut male_bytes = Cursor::new(Vec::new());
        let mut targets = [TlkLayerWriteTarget {
            male:   Some(&mut male_bytes),
            female: None,
        }];

        let error = write_tlk_chain(&mut targets, &mut tlk).unwrap_err();
        assert!(error.to_string().contains("no table to write"));
    }
}
