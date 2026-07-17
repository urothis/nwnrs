//! Byte-transparent MDL text conversion.

/// Decodes model text without requiring UTF-8.
///
/// Cleanmodels treats ASCII MDL input as an arbitrary byte stream carried in
/// strings. Mapping each byte to the same-valued Unicode scalar gives the Rust
/// parser equivalent byte-transparent behavior while retaining its `String`
/// based syntax tree.
pub(crate) fn decode_model_text(bytes: &[u8]) -> String {
    bytes.iter().copied().map(char::from).collect()
}

/// Encodes model text while reversing [`decode_model_text`].
///
/// Scalars from `U+0000` through `U+00FF` are emitted as one byte so decoded
/// source bytes round-trip exactly. Scalars above `U+00FF` supplied through
/// the `&str` APIs retain their normal UTF-8 representation.
pub(crate) fn encode_model_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(model_text_byte_len(text));
    for character in text.chars() {
        let value = u32::from(character);
        if let Ok(byte) = u8::try_from(value) {
            bytes.push(byte);
        } else {
            let mut encoded = [0_u8; 4];
            bytes.extend_from_slice(character.encode_utf8(&mut encoded).as_bytes());
        }
    }
    bytes
}

pub(crate) fn model_text_byte_len(text: &str) -> usize {
    text.chars()
        .map(|character| {
            if u32::from(character) <= u32::from(u8::MAX) {
                1
            } else {
                character.len_utf8()
            }
        })
        .sum()
}

/// Parses ordinary Rust floats and the non-standard spellings emitted by
/// legacy BioWare-era tools.
pub(crate) fn parse_legacy_f32(value: &str) -> Option<f32> {
    value
        .parse::<f32>()
        .ok()
        .or_else(|| match value.to_ascii_uppercase().as_str() {
            "1.#INF" | "+1.#INF" => Some(f32::INFINITY),
            "-1.#INF" => Some(f32::NEG_INFINITY),
            "1.#QNAN" | "-1.#QNAN" | "1.#IND" | "-1.#IND" => Some(f32::NAN),
            _ => None,
        })
}

#[cfg(test)]
mod tests {
    use super::{decode_model_text, encode_model_text, model_text_byte_len, parse_legacy_f32};

    #[test]
    fn arbitrary_bytes_roundtrip() {
        let bytes = b"demo\x80\xff";
        let decoded = decode_model_text(bytes);
        assert_eq!(encode_model_text(&decoded), bytes);
        assert_eq!(model_text_byte_len(&decoded), bytes.len());
    }

    #[test]
    fn unicode_str_input_remains_utf8() {
        let text = "demo \u{20ac}";
        assert_eq!(encode_model_text(text), text.as_bytes());
        assert_eq!(model_text_byte_len(text), text.len());
    }

    #[test]
    fn legacy_floats_accept_toolchain_specific_non_finite_values() {
        assert_eq!(parse_legacy_f32("1.25"), Some(1.25));
        assert_eq!(parse_legacy_f32("1.#INF"), Some(f32::INFINITY));
        assert_eq!(parse_legacy_f32("-1.#INF"), Some(f32::NEG_INFINITY));
        assert!(parse_legacy_f32("1.#QNAN").is_some_and(f32::is_nan));
        assert!(parse_legacy_f32("-1.#IND").is_some_and(f32::is_nan));
        assert_eq!(parse_legacy_f32("not-a-number"), None);
    }
}
