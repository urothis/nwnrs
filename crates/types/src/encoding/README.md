# NWN text encoding

`nwnrs-types` is the workspace policy boundary for text stored by Neverwinter
Nights. Native resources are byte-oriented and do not use UTF-8.

## Explicit policy

Every native reader, writer, resource manager, and editor session receives an
immutable `NwnTextContext`. There is no process-global or thread-local
encoding. The currently supported language mapping is:

- English, French, German, Italian, and Spanish: Windows-1252
- Polish: Windows-1250

`LegacyEncoding` names these code pages. `NwnTextContext` carries both the NWN
language identifier and its selected encoding. Callers must derive it from the
package, installed-game language, embedded resource language, or an explicit
standalone choice.

## Lossless native text

`NwnText` preserves:

- decoded legacy text
- bytes undefined by the selected code page as opaque segments
- NWN `<cRGB>` color starts with their exact three RGB bytes
- `</c>` color ends

Unknown angle-bracket content, including TLK-style tokens, remains ordinary
text. An unchanged value emits its original bytes. Edited text is encoded
strictly, while opaque and control segments are copied exactly.

For text formats whose parsers retain authored layout in strings,
`decode_legacy_preserving` maps opaque bytes to a private internal scalar and
`encode_legacy_preserving` reverses that mapping. This representation is an
implementation detail for lossless parsing, not a Unicode file format.

## Public surface

- `LegacyEncoding`
- `NwnTextContext`
- `NwnText`
- `NwnTextSegment`
- `encode_legacy` and `decode_legacy`
- `encode_legacy_preserving` and `decode_legacy_preserving`
- `decode_legacy_lossy` for display-only fallback
- `display_legacy_preserving`
- `EncodingConversionError` and `UnknownEncodingError`

Modern nwnrs-owned files such as TOML, JSON, TypeScript, and Markdown remain
UTF-8. Strict engine identifiers use their own ASCII/byte-bounded types instead
of the human-text encoding policy.

## See also

- [`crate::localization`], which owns NWN language and string-reference
  vocabulary
- [`crate::resman`], whose managers retain one immutable text context
