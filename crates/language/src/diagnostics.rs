use std::path::{Path, PathBuf};

/// Diagnostic severity independent of any editor protocol.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LanguageDiagnosticSeverity {
    /// Compilation cannot proceed.
    Error,
    /// Suspicious input that remains usable.
    Warning,
    /// Informational feedback.
    Information,
    /// Low-priority guidance.
    Hint,
}

/// Stable compiler or language-service diagnostic code.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LanguageDiagnosticCode {
    /// Numeric compiler diagnostic code.
    Numeric(i32),
    /// Named language-service diagnostic code.
    Named(String),
}

/// Zero-based UTF-16 position suitable for Language Server Protocol clients.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LanguagePosition {
    /// Zero-based line.
    pub line:      u32,
    /// Zero-based UTF-16 code-unit offset within the line.
    pub character: u32,
}

/// Half-open UTF-16 source range.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LanguageRange {
    /// First included position.
    pub start: LanguagePosition,
    /// First excluded position.
    pub end:   LanguagePosition,
}

/// Exact source identity attached to a language result.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum LanguageSource {
    /// Mutable or on-disk physical source.
    Physical(PathBuf),
    /// Immutable compiler-produced or installation-backed virtual source.
    Virtual(String),
}

/// Canonical diagnostic shared by compiler, macro, manifest, and editor
/// recovery analysis.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LanguageDiagnostic {
    /// Diagnostic severity.
    pub severity: LanguageDiagnosticSeverity,
    /// Optional stable diagnostic code.
    pub code:     Option<LanguageDiagnosticCode>,
    /// Human-readable diagnostic text.
    pub message:  String,
    /// Exact physical or virtual source.
    pub source:   LanguageSource,
    /// Exact zero-based UTF-16 range.
    pub range:    LanguageRange,
}

/// Summary counts for one compiler-backed check.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LanguageCheckSummary {
    /// Inputs compiled successfully.
    pub compiled: usize,
    /// Include-style inputs skipped because they have no entrypoint.
    pub skipped:  usize,
    /// Inputs that failed.
    pub failed:   usize,
}

/// Canonical editor-facing result of a compiler check.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct LanguageCheckReport {
    /// Exact source-aware diagnostics.
    pub diagnostics: Vec<LanguageDiagnostic>,
    /// Aggregate compiler counts.
    pub summary:     LanguageCheckSummary,
}

/// Converts a UTF-8 byte offset into a zero-based UTF-16 position.
#[must_use]
pub fn utf16_position(source: &str, byte_offset: usize) -> LanguagePosition {
    let clamped = byte_offset.min(source.len());
    let boundary = (0..=clamped)
        .rev()
        .find(|index| source.is_char_boundary(*index))
        .unwrap_or(0);
    let prefix = source.get(..boundary).unwrap_or_default();
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count();
    let line_start = prefix.rfind('\n').map_or(0, |index| index + 1);
    let character = prefix
        .get(line_start..)
        .unwrap_or_default()
        .encode_utf16()
        .count();
    LanguagePosition {
        line:      u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(character).unwrap_or(u32::MAX),
    }
}

/// Converts a half-open UTF-8 byte span into a UTF-16 range.
#[must_use]
pub fn utf16_range(source: &str, start: usize, end: usize) -> LanguageRange {
    LanguageRange {
        start: utf16_position(source, start),
        end:   utf16_position(source, end.max(start)),
    }
}

/// Converts a Windows-1252 NWScript byte offset into a zero-based UTF-16
/// position.
///
/// Windows-1252 is single-byte and every decoded source byte occupies exactly
/// one UTF-16 code unit in VS Code. Undefined byte values are still counted as
/// one displayed replacement unit, so source spans remain stable.
#[must_use]
pub fn nwscript_utf16_position(source: &[u8], byte_offset: usize) -> LanguagePosition {
    let clamped = byte_offset.min(source.len());
    let prefix = source.get(..clamped).unwrap_or_default();
    let line = prefix.iter().filter(|byte| **byte == b'\n').count();
    let line_start = prefix
        .iter()
        .rposition(|byte| *byte == b'\n')
        .map_or(0, |index| index + 1);
    LanguagePosition {
        line:      u32::try_from(line).unwrap_or(u32::MAX),
        character: u32::try_from(clamped.saturating_sub(line_start)).unwrap_or(u32::MAX),
    }
}

/// Converts a half-open Windows-1252 NWScript byte span into a UTF-16 range.
#[must_use]
pub fn nwscript_utf16_range(source: &[u8], start: usize, end: usize) -> LanguageRange {
    LanguageRange {
        start: nwscript_utf16_position(source, start),
        end:   nwscript_utf16_position(source, end.max(start)),
    }
}

/// Creates a physical-source diagnostic from an exact UTF-8 byte span.
#[must_use]
pub fn physical_diagnostic(
    path: &Path,
    source: &str,
    severity: LanguageDiagnosticSeverity,
    code: Option<LanguageDiagnosticCode>,
    message: impl Into<String>,
    start: usize,
    end: usize,
) -> LanguageDiagnostic {
    LanguageDiagnostic {
        severity,
        code,
        message: message.into(),
        source: LanguageSource::Physical(path.to_path_buf()),
        range: utf16_range(source, start, end),
    }
}

#[cfg(test)]
mod tests {
    use super::{
        LanguagePosition, nwscript_utf16_position, nwscript_utf16_range, utf16_position,
        utf16_range,
    };

    #[test]
    fn converts_utf8_offsets_to_utf16() {
        let source = "α😀x\nβ";
        assert_eq!(
            utf16_position(source, "α😀".len()),
            LanguagePosition {
                line:      0,
                character: 3,
            }
        );
        let range = utf16_range(source, "α".len(), "α😀".len());
        assert_eq!(range.start.character, 1);
        assert_eq!(range.end.character, 3);
    }

    #[test]
    fn converts_windows_1252_offsets_without_utf8_interpretation() {
        let source = b"caf\xe9\r\nnext";
        assert_eq!(
            nwscript_utf16_position(source, 4),
            LanguagePosition {
                line:      0,
                character: 4,
            }
        );
        let range = nwscript_utf16_range(source, 3, 4);
        assert_eq!(range.start.character, 3);
        assert_eq!(range.end.character, 4);
        assert_eq!(nwscript_utf16_position(source, 7).line, 1);
    }
}
