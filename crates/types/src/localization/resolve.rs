use tracing::instrument;

use crate::localization::prelude::*;

/// Resolves a language from a numeric id, short code, or English name.
///
/// # Errors
///
/// Returns [`ParseLanguageError`] if the input does not match a known language
/// id, shortcode, or name.
#[instrument(level = "debug", skip_all, err, fields(input = %input))]
pub fn resolve_language(input: &str) -> Result<Language, ParseLanguageError> {
    if input.chars().all(|ch| ch.is_ascii_digit()) {
        let id = input
            .parse::<u32>()
            .map_err(|error| ParseLanguageError::new(input, error.to_string()))?;
        return Language::from_id(id)
            .ok_or_else(|| ParseLanguageError::new(input, "no such language id"));
    }

    let normalized = input.to_ascii_lowercase();
    if normalized.len() == 2 {
        return match normalized.as_str() {
            "en" => Ok(Language::English),
            "fr" => Ok(Language::French),
            "de" => Ok(Language::German),
            "it" => Ok(Language::Italian),
            "es" => Ok(Language::Spanish),
            "pl" => Ok(Language::Polish),
            _ => Err(ParseLanguageError::new(input, "no such shortcode")),
        };
    }

    match normalized.as_str() {
        "english" => Ok(Language::English),
        "french" => Ok(Language::French),
        "german" => Ok(Language::German),
        "italian" => Ok(Language::Italian),
        "spanish" => Ok(Language::Spanish),
        "polish" => Ok(Language::Polish),
        _ => Err(ParseLanguageError::new(input, "no such language name")),
    }
}

#[cfg(test)]
mod tests {
    use crate::localization::{Language, resolve_language};

    #[test]
    fn resolves_languages_from_id_code_and_name() {
        assert_eq!(resolve_language("1"), Ok(Language::French));
        assert_eq!(resolve_language("EN"), Ok(Language::English));
        assert_eq!(resolve_language("polish"), Ok(Language::Polish));
    }

    #[test]
    fn rejects_unknown_languages_with_context() {
        let error = match resolve_language("xx") {
            Ok(_value) => panic!("invalid language should fail"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("xx"));
    }
}
