export interface Utf16OffsetRange {
  readonly start: number;
  readonly end: number;
}

/**
 * Returns every contiguous UTF-16 range that did not survive a
 * Windows-1252 encode/decode round trip.
 *
 * VS Code replaces each unrepresentable Unicode scalar during encoding, so
 * source and decoded text retain the same scalar ordering. Offsets deliberately
 * use JavaScript string lengths because VS Code positions are UTF-16 based.
 */
export function unrepresentableUtf16Ranges(
  source: string,
  decoded: string,
): readonly Utf16OffsetRange[] {
  const sourceCharacters = [...source];
  const decodedCharacters = [...decoded];
  const ranges: Utf16OffsetRange[] = [];
  let sourceOffset = 0;
  let index = 0;

  while (index < sourceCharacters.length) {
    const sourceCharacter = sourceCharacters[index] ?? '';
    const decodedCharacter = decodedCharacters[index] ?? '';
    if (sourceCharacter === decodedCharacter) {
      sourceOffset += sourceCharacter.length;
      index += 1;
      continue;
    }

    const start = sourceOffset;
    while (
      index < sourceCharacters.length
      && sourceCharacters[index] !== decodedCharacters[index]
    ) {
      sourceOffset += (sourceCharacters[index] ?? '').length;
      index += 1;
    }
    ranges.push({ start, end: Math.max(start + 1, sourceOffset) });
  }

  return ranges;
}
