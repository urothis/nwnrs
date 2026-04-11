use std::io::{Read, Write};

use nwnrs_resman::prelude::*;
use nwnrs_util::prelude::*;
use tracing::{debug, instrument};

use crate::{
    CELL_PADDING, CELL_PADDING_MINI, MAX_COLUMNS, TwoDaSourceLayout, TwoDaSourceLine,
    TwoDaTokenLayout, prelude::*,
};

/// Reads a `2DA V2.0` table from text.
#[instrument(level = "debug", skip_all, err)]
pub fn read_twoda<R: Read>(mut reader: R) -> TwoDaResult<TwoDa> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let decoded = from_nwnrs_encoding(&bytes)?;
    let source_lines = split_source_lines(&decoded);

    let mut twoda = TwoDa::new();
    let mut next_nonempty = source_lines
        .iter()
        .enumerate()
        .filter(|(_, line)| !line.text.trim().is_empty());

    let Some((header_idx, header_line)) = next_nonempty.next() else {
        return Err(TwoDaError::msg("EOF while reading 2da"));
    };
    if header_line.text.trim() != TWO_DA_HEADER {
        return Err(TwoDaError::msg("invalid 2da header"));
    }
    let mut layout_lines = vec![TwoDaSourceLine::HeaderMagic {
        raw:         header_line.text.to_string(),
        line_ending: header_line.line_ending.to_string(),
    }];
    for line in source_lines.iter().take(header_idx) {
        layout_lines.push(TwoDaSourceLine::Blank {
            raw:         line.text.to_string(),
            line_ending: line.line_ending.to_string(),
        });
    }

    let Some((second_idx, second_line)) = next_nonempty.next() else {
        return Err(TwoDaError::msg("EOF while reading 2da"));
    };
    for line in source_lines
        .iter()
        .skip(header_idx + 1)
        .take(second_idx.saturating_sub(header_idx + 1))
    {
        layout_lines.push(TwoDaSourceLine::Blank {
            raw:         line.text.to_string(),
            line_ending: line.line_ending.to_string(),
        });
    }

    if let Some(default_layout) = parse_default_line(second_line)? {
        twoda.default_value = default_layout
            .value
            .as_ref()
            .and_then(|token| token.value.clone());
        layout_lines.push(TwoDaSourceLine::Default {
            prefix:      default_layout.prefix,
            value:       default_layout.value,
            trailing:    default_layout.trailing,
            line_ending: second_line.line_ending.to_string(),
        });

        let Some((header_row_idx, header_row_line)) = next_nonempty.next() else {
            return Err(TwoDaError::msg("EOF while reading 2da"));
        };
        for line in source_lines
            .iter()
            .skip(second_idx + 1)
            .take(header_row_idx.saturating_sub(second_idx + 1))
        {
            layout_lines.push(TwoDaSourceLine::Blank {
                raw:         line.text.to_string(),
                line_ending: line.line_ending.to_string(),
            });
        }
        let header_layout = parse_header_line(header_row_line, MAX_COLUMNS)?;
        let headers = header_layout.columns.clone();
        if headers.iter().any(Option::is_none) {
            return Err(TwoDaError::msg("empty header fields not supported"));
        }
        twoda.set_columns(headers.into_iter().map(Option::unwrap).collect())?;
        layout_lines.push(TwoDaSourceLine::HeaderRow {
            leading:     header_layout.leading,
            columns:     twoda.headers.clone(),
            separators:  header_layout.separators,
            trailing:    header_layout.trailing,
            line_ending: header_row_line.line_ending.to_string(),
        });

        for line in source_lines.iter().skip(header_row_idx + 1) {
            append_data_line(line, &mut twoda, &mut layout_lines)?;
        }
    } else {
        let header_layout = parse_header_line(second_line, MAX_COLUMNS)?;
        let headers = header_layout.columns.clone();
        if headers.iter().any(Option::is_none) {
            return Err(TwoDaError::msg("empty header fields not supported"));
        }
        twoda.set_columns(headers.into_iter().map(Option::unwrap).collect())?;
        layout_lines.push(TwoDaSourceLine::HeaderRow {
            leading:     header_layout.leading,
            columns:     twoda.headers.clone(),
            separators:  header_layout.separators,
            trailing:    header_layout.trailing,
            line_ending: second_line.line_ending.to_string(),
        });

        for line in source_lines.iter().skip(second_idx + 1) {
            append_data_line(line, &mut twoda, &mut layout_lines)?;
        }
    }

    debug!(
        rows = twoda.rows.len(),
        columns = twoda.headers.len(),
        "read 2da"
    );
    twoda.source_snapshot = Some(twoda.snapshot());
    twoda.source_bytes = Some(bytes);
    twoda.source_layout = Some(TwoDaSourceLayout {
        lines: layout_lines,
    });
    Ok(twoda)
}

/// Writes a `2DA V2.0` table to text.
///
/// When `minify` is `true`, column padding is reduced to the minimum required
/// whitespace.
#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(rows = twoda.rows.len(), columns = twoda.headers.len(), minify)
)]
pub fn write_twoda<W: Write>(writer: &mut W, twoda: &TwoDa, minify: bool) -> TwoDaResult<()> {
    if twoda.headers.is_empty() {
        return Err(TwoDaError::msg("no columns configured"));
    }
    if !minify
        && let (Some(source_bytes), Some(source_snapshot)) =
            (&twoda.source_bytes, &twoda.source_snapshot)
    {
        if *source_snapshot == twoda.snapshot() {
            writer.write_all(source_bytes)?;
            return Ok(());
        }
        if let Some(source_layout) = &twoda.source_layout {
            return write_twoda_with_layout(writer, twoda, source_layout);
        }
    }

    let max_col_width: Vec<usize> = twoda
        .headers
        .iter()
        .enumerate()
        .map(|(idx, header)| {
            let row_max = twoda
                .rows
                .iter()
                .map(|row| {
                    row.get(idx)
                        .map_or(Ok(0), |cell| escape_field(cell).map(|value| value.len()))
                })
                .collect::<TwoDaResult<Vec<_>>>()?
                .into_iter()
                .max()
                .unwrap_or(0);
            Ok(header.len().max(row_max))
        })
        .collect::<TwoDaResult<Vec<_>>>()?;

    let id_width = 3.max(twoda.rows.len().to_string().len());

    writer.write_all(TWO_DA_HEADER.as_bytes())?;
    writer.write_all(b"\n")?;
    if let Some(default) = &twoda.default_value {
        writer.write_all(b"DEFAULT: ")?;
        writer.write_all(escape_field(&Some(default.clone()))?.as_bytes())?;
    }
    writer.write_all(b"\n")?;

    writer.write_all(
        " ".repeat(if minify {
            CELL_PADDING_MINI
        } else {
            id_width + CELL_PADDING
        })
        .as_bytes(),
    )?;
    for (idx, header) in twoda.headers.iter().enumerate() {
        writer.write_all(header.as_bytes())?;
        if idx != twoda.headers.len() - 1 {
            let width = max_col_width
                .get(idx)
                .copied()
                .ok_or_else(|| TwoDaError::msg("column width index out of range"))?;
            writer.write_all(
                " ".repeat(if minify {
                    CELL_PADDING_MINI
                } else {
                    width - header.len() + 3 + CELL_PADDING
                })
                .as_bytes(),
            )?;
        }
    }
    writer.write_all(b"\n")?;

    for (row_idx, row) in twoda.rows.iter().enumerate() {
        let row_label = row_idx.to_string();
        writer.write_all(row_label.as_bytes())?;
        writer.write_all(
            " ".repeat(if minify {
                CELL_PADDING_MINI
            } else {
                id_width + CELL_PADDING - row_label.len()
            })
            .as_bytes(),
        )?;

        for (cell_idx, cell) in row.iter().enumerate() {
            let formatted = escape_field(cell)?;
            writer.write_all(&to_nwnrs_encoding(&formatted)?)?;
            if cell_idx != twoda.headers.len() - 1 {
                let width = max_col_width
                    .get(cell_idx)
                    .copied()
                    .ok_or_else(|| TwoDaError::msg("cell width index out of range"))?;
                writer.write_all(
                    " ".repeat(if minify {
                        CELL_PADDING_MINI
                    } else {
                        width - formatted.len() + 3 + CELL_PADDING
                    })
                    .as_bytes(),
                )?;
            }
        }
        writer.write_all(b"\n")?;
    }

    debug!(
        rows = twoda.rows.len(),
        columns = twoda.headers.len(),
        "wrote 2da"
    );
    Ok(())
}

/// Reads a `2DA V2.0` table from a [`Res`].
#[instrument(level = "debug", skip_all, err)]
pub fn as_2da(res: &Res) -> TwoDaResult<TwoDa> {
    read_twoda(std::io::Cursor::new(res.read_all(false)?))
}

/// Formats a cell for textual 2DA output.
pub fn escape_field(field: &Cell) -> TwoDaResult<String> {
    match field {
        None => Ok("****".to_string()),
        Some(value) => {
            if value.contains('"') {
                Err(TwoDaError::msg("Cannot properly escape doublequotes"))
            } else if value.is_empty() || value.chars().any(char::is_whitespace) {
                Ok(format!("\"{value}\""))
            } else {
                Ok(value.clone())
            }
        }
    }
}

fn append_data_line(
    line: &SourceLine<'_>,
    twoda: &mut TwoDa,
    layout_lines: &mut Vec<TwoDaSourceLine>,
) -> TwoDaResult<()> {
    if line.text.trim().is_empty() {
        layout_lines.push(TwoDaSourceLine::Blank {
            raw:         line.text.to_string(),
            line_ending: line.line_ending.to_string(),
        });
        return Ok(());
    }

    let layout = parse_row_line(line, twoda.headers.len())?;
    let mut row = layout
        .cells
        .iter()
        .map(|cell| cell.value.clone())
        .collect::<Vec<_>>();
    while row.len() < twoda.headers.len() {
        row.push(None);
    }
    twoda.row_labels.push(layout.label.clone());
    twoda.rows.push(row);
    layout_lines.push(TwoDaSourceLine::DataRow {
        row_index:   twoda.rows.len() - 1,
        after_label: layout.after_label,
        cells:       layout.cells,
        separators:  layout.separators,
        trailing:    layout.trailing,
        line_ending: line.line_ending.to_string(),
    });
    Ok(())
}

fn write_twoda_with_layout<W: Write>(
    writer: &mut W,
    twoda: &TwoDa,
    layout: &TwoDaSourceLayout,
) -> TwoDaResult<()> {
    if twoda.headers.is_empty() {
        return Err(TwoDaError::msg("no columns configured"));
    }

    let header_layout = layout.lines.iter().find_map(|line| {
        if let TwoDaSourceLine::HeaderRow {
            leading,
            columns,
            separators,
            trailing,
            line_ending,
        } = line
        {
            Some((leading, columns, separators, trailing, line_ending))
        } else {
            None
        }
    });
    let Some((
        _header_leading,
        _original_columns,
        _header_separators,
        _header_trailing,
        header_line_ending,
    )) = header_layout
    else {
        return Err(TwoDaError::msg("missing 2DA header layout"));
    };
    let trailing_blank_start = layout
        .lines
        .iter()
        .rposition(|line| !matches!(line, TwoDaSourceLine::Blank { .. }))
        .map(|idx| idx + 1)
        .unwrap_or(0);
    let preferred_line_ending = preferred_line_ending(layout);
    let mut emitted_rows = vec![false; twoda.rows.len()];
    let mut saw_default_line = false;
    let mut inserted_missing_default = false;

    for (line_idx, line) in layout.lines.iter().enumerate() {
        if !inserted_missing_default
            && line_idx == trailing_blank_start
            && twoda.default().is_some()
            && !saw_default_line
        {
            write_default_line(writer, twoda.default(), header_line_ending)?;
            inserted_missing_default = true;
        }
        if line_idx == trailing_blank_start {
            write_extra_rows(writer, twoda, &emitted_rows, preferred_line_ending)?;
        }

        match line {
            TwoDaSourceLine::HeaderMagic {
                raw,
                line_ending,
            }
            | TwoDaSourceLine::Blank {
                raw,
                line_ending,
            } => {
                writer.write_all(raw.as_bytes())?;
                writer.write_all(line_ending.as_bytes())?;
            }
            TwoDaSourceLine::Default {
                prefix,
                value,
                trailing,
                line_ending,
            } => {
                saw_default_line = true;
                writer.write_all(prefix.as_bytes())?;
                if let Some(token) = value {
                    write_token(
                        writer,
                        twoda.default(),
                        Some(token),
                        trailing.is_empty() && line_ending.is_empty(),
                    )?;
                }
                writer.write_all(trailing.as_bytes())?;
                writer.write_all(line_ending.as_bytes())?;
            }
            TwoDaSourceLine::HeaderRow {
                leading,
                columns,
                separators,
                trailing,
                line_ending,
            } => {
                writer.write_all(leading.as_bytes())?;
                write_string_tokens(
                    writer,
                    twoda.columns(),
                    columns,
                    separators,
                    trailing,
                    line_ending,
                )?;
            }
            TwoDaSourceLine::DataRow {
                row_index,
                after_label,
                cells,
                separators,
                trailing,
                line_ending,
            } => {
                let Some(stored_label) = twoda.row_label(*row_index) else {
                    continue;
                };
                let emitted = emitted_rows
                    .get_mut(*row_index)
                    .ok_or_else(|| TwoDaError::msg("row layout index out of bounds"))?;
                *emitted = true;
                writer.write_all(stored_label.as_bytes())?;
                writer.write_all(after_label.as_bytes())?;
                let row = twoda
                    .rows
                    .get(*row_index)
                    .ok_or_else(|| TwoDaError::msg("row out of bounds"))?;
                write_row_cells(writer, row, cells, separators)?;
                writer.write_all(trailing.as_bytes())?;
                writer.write_all(line_ending.as_bytes())?;
            }
        }
    }

    if !inserted_missing_default && twoda.default().is_some() && !saw_default_line {
        write_default_line(writer, twoda.default(), preferred_line_ending)?;
    }
    if trailing_blank_start >= layout.lines.len() {
        write_extra_rows(writer, twoda, &emitted_rows, preferred_line_ending)?;
    }
    Ok(())
}

fn write_string_tokens<W: Write>(
    writer: &mut W,
    columns: &[String],
    original_columns: &[String],
    separators: &[String],
    trailing: &str,
    line_ending: &str,
) -> TwoDaResult<()> {
    for (idx, column) in columns.iter().enumerate() {
        writer.write_all(column.as_bytes())?;
        if idx + 1 < columns.len() {
            writer
                .write_all(select_separator(separators, idx, original_columns.len()).as_bytes())?;
        }
    }
    writer.write_all(trailing.as_bytes())?;
    writer.write_all(line_ending.as_bytes())?;
    Ok(())
}

fn write_row_cells<W: Write>(
    writer: &mut W,
    row: &Row,
    cells: &[TwoDaTokenLayout],
    separators: &[String],
) -> TwoDaResult<()> {
    let cell_count = row.len().max(cells.len());
    for cell_idx in 0..cell_count {
        write_token(
            writer,
            row.get(cell_idx).cloned().unwrap_or(None),
            cells.get(cell_idx),
            false,
        )?;
        if cell_idx + 1 < cell_count {
            writer.write_all(select_separator(separators, cell_idx, cells.len()).as_bytes())?;
        }
    }
    Ok(())
}

fn write_token<W: Write>(
    writer: &mut W,
    value: Cell,
    layout: Option<&TwoDaTokenLayout>,
    _terminal: bool,
) -> TwoDaResult<()> {
    if let Some(layout) = layout
        && layout.value == value
    {
        writer.write_all(layout.raw.as_bytes())?;
        return Ok(());
    }

    let rendered = match value {
        None => "****".to_string(),
        Some(value) => {
            let prefer_quotes = layout.is_some_and(|layout| layout.quoted);
            if value.contains('"') {
                return Err(TwoDaError::msg("Cannot properly escape doublequotes"));
            }
            if prefer_quotes || value.is_empty() || value.chars().any(char::is_whitespace) {
                format!("\"{value}\"")
            } else {
                value
            }
        }
    };
    writer.write_all(rendered.as_bytes())?;
    Ok(())
}

fn write_extra_rows<W: Write>(
    writer: &mut W,
    twoda: &TwoDa,
    emitted_rows: &[bool],
    line_ending: &str,
) -> TwoDaResult<()> {
    for row_idx in 0..twoda.rows.len() {
        if emitted_rows.get(row_idx).copied().unwrap_or(false) {
            continue;
        }
        let label = twoda
            .row_label(row_idx)
            .ok_or_else(|| TwoDaError::msg("row label out of bounds"))?;
        writer.write_all(label.as_bytes())?;
        writer.write_all(default_row_separator().as_bytes())?;
        let row = twoda
            .rows
            .get(row_idx)
            .ok_or_else(|| TwoDaError::msg("row out of bounds"))?;
        for (cell_idx, cell) in row.iter().enumerate() {
            write_token(writer, cell.clone(), None, false)?;
            if cell_idx + 1 < row.len() {
                writer.write_all(default_cell_separator().as_bytes())?;
            }
        }
        writer.write_all(line_ending.as_bytes())?;
    }
    Ok(())
}

fn write_default_line<W: Write>(writer: &mut W, value: Cell, line_ending: &str) -> TwoDaResult<()> {
    writer.write_all(b"DEFAULT: ")?;
    write_token(writer, value, None, false)?;
    writer.write_all(line_ending.as_bytes())?;
    Ok(())
}

fn preferred_line_ending(layout: &TwoDaSourceLayout) -> &str {
    layout
        .lines
        .iter()
        .find_map(|line| match line {
            TwoDaSourceLine::HeaderMagic {
                line_ending, ..
            }
            | TwoDaSourceLine::Blank {
                line_ending, ..
            }
            | TwoDaSourceLine::Default {
                line_ending, ..
            }
            | TwoDaSourceLine::HeaderRow {
                line_ending, ..
            }
            | TwoDaSourceLine::DataRow {
                line_ending, ..
            } if !line_ending.is_empty() => Some(line_ending.as_str()),
            _ => None,
        })
        .unwrap_or("\n")
}

fn select_separator(separators: &[String], idx: usize, original_count: usize) -> String {
    if idx + 1 < original_count {
        return separators
            .get(idx)
            .filter(|separator| !separator.is_empty())
            .cloned()
            .unwrap_or_else(default_cell_separator);
    }

    separators
        .iter()
        .rev()
        .find(|separator| !separator.is_empty())
        .cloned()
        .unwrap_or_else(default_cell_separator)
}

fn default_row_separator() -> String {
    "   ".to_string()
}

fn default_cell_separator() -> String {
    " ".to_string()
}

struct SourceLine<'a> {
    text:        &'a str,
    line_ending: &'a str,
}

struct ParsedDefaultLine {
    prefix:   String,
    value:    Option<TwoDaTokenLayout>,
    trailing: String,
}

struct ParsedHeaderLine {
    leading:    String,
    columns:    Vec<Cell>,
    separators: Vec<String>,
    trailing:   String,
}

struct ParsedRowLine {
    label:       String,
    after_label: String,
    cells:       Vec<TwoDaTokenLayout>,
    separators:  Vec<String>,
    trailing:    String,
}

fn split_source_lines(text: &str) -> Vec<SourceLine<'_>> {
    let mut lines = Vec::new();
    let mut start = 0;
    let bytes = text.as_bytes();
    let mut idx = 0;
    while idx < bytes.len() {
        if bytes.get(idx) == Some(&b'\r') {
            let line_ending = if bytes.get(idx + 1) == Some(&b'\n') {
                "\r\n"
            } else {
                "\r"
            };
            lines.push(SourceLine {
                text: &text[start..idx],
                line_ending,
            });
            idx += line_ending.len();
            start = idx;
            continue;
        }
        if bytes.get(idx) == Some(&b'\n') {
            lines.push(SourceLine {
                text:        &text[start..idx],
                line_ending: "\n",
            });
            idx += 1;
            start = idx;
            continue;
        }
        idx += 1;
    }
    if start < text.len() || text.is_empty() {
        lines.push(SourceLine {
            text:        &text[start..],
            line_ending: "",
        });
    }
    lines
}

fn parse_default_line(line: &SourceLine<'_>) -> TwoDaResult<Option<ParsedDefaultLine>> {
    let Some(prefix_idx) = line.text.find("DEFAULT:") else {
        return Ok(None);
    };
    if !line.text[..prefix_idx].trim().is_empty() {
        return Ok(None);
    }
    let prefix_end = prefix_idx + "DEFAULT:".len();
    let remainder = &line.text[prefix_end..];
    let consumed = remainder
        .chars()
        .take_while(|ch| ch.is_ascii_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    let prefix = format!("{}{}", &line.text[..prefix_end], &remainder[..consumed]);
    let token_remainder = &remainder[consumed..];
    if token_remainder.is_empty() {
        return Ok(Some(ParsedDefaultLine {
            prefix,
            value: None,
            trailing: String::new(),
        }));
    }
    let parsed = parse_token_sequence(token_remainder, 1)?;
    Ok(Some(ParsedDefaultLine {
        prefix,
        value: parsed.tokens.into_iter().next(),
        trailing: parsed.trailing,
    }))
}

fn parse_header_line(line: &SourceLine<'_>, maxcount: usize) -> TwoDaResult<ParsedHeaderLine> {
    let parsed = parse_token_sequence(line.text, maxcount)?;
    Ok(ParsedHeaderLine {
        leading:    parsed.leading,
        columns:    parsed.tokens.into_iter().map(|token| token.value).collect(),
        separators: parsed.separators,
        trailing:   parsed.trailing,
    })
}

fn parse_row_line(line: &SourceLine<'_>, header_count: usize) -> TwoDaResult<ParsedRowLine> {
    let parsed = parse_token_sequence(line.text, header_count + 1)?;
    let mut separators = parsed.separators;
    let mut tokens = parsed.tokens.into_iter();
    let label = tokens
        .next()
        .and_then(|token| token.value)
        .ok_or_else(|| TwoDaError::msg("missing row label"))?;
    let after_label = separators.first().cloned().unwrap_or_default();
    Ok(ParsedRowLine {
        label,
        after_label,
        cells: tokens.collect(),
        separators: separators.drain(1..).collect(),
        trailing: parsed.trailing,
    })
}

struct ParsedTokenSequence {
    leading:    String,
    tokens:     Vec<TwoDaTokenLayout>,
    separators: Vec<String>,
    trailing:   String,
}

fn parse_token_sequence(line: &str, maxcount: usize) -> TwoDaResult<ParsedTokenSequence> {
    let leading_end = consume_ws(line, 0);
    let leading = line[..leading_end].to_string();
    let mut cursor = leading_end;
    let mut tokens = Vec::new();
    let mut separators = Vec::new();
    while cursor < line.len() && tokens.len() < maxcount {
        let (raw, value, quoted, next) = parse_one_token(line, cursor)?;
        tokens.push(TwoDaTokenLayout {
            value,
            quoted,
            raw,
        });
        let next_ws = consume_ws(line, next);
        separators.push(line[next..next_ws].to_string());
        cursor = next_ws;
        if next == next_ws {
            break;
        }
    }
    let trailing = if cursor <= line.len() {
        line[cursor..].to_string()
    } else {
        String::new()
    };
    Ok(ParsedTokenSequence {
        leading,
        tokens,
        separators,
        trailing,
    })
}

fn consume_ws(line: &str, mut idx: usize) -> usize {
    while idx < line.len() {
        let ch = line[idx..].chars().next().unwrap_or_default();
        if !matches!(ch, ' ' | '\t') {
            break;
        }
        idx += ch.len_utf8();
    }
    idx
}

fn parse_one_token(line: &str, start: usize) -> TwoDaResult<(String, Cell, bool, usize)> {
    let mut chars = line[start..].char_indices();
    let Some((_, first)) = chars.next() else {
        return Err(TwoDaError::msg("unexpected end of 2DA token"));
    };
    if first == '"' {
        let mut value = String::new();
        for (idx, ch) in line[start + 1..].char_indices() {
            let end = start + 1 + idx + ch.len_utf8();
            if ch == '"' {
                let raw = line[start..end].to_string();
                return Ok((
                    raw,
                    if value == "****" { None } else { Some(value) },
                    true,
                    end,
                ));
            }
            value.push(ch);
        }
        return Err(TwoDaError::msg("unterminated quoted 2DA token"));
    }

    let end = line[start..]
        .char_indices()
        .find_map(|(idx, ch)| ch.is_ascii_whitespace().then_some(start + idx))
        .unwrap_or(line.len());
    let raw = line[start..end].to_string();
    let value = if raw.is_empty() || raw == "****" {
        None
    } else {
        Some(raw.clone())
    };
    Ok((raw, value, false, end))
}

#[cfg(test)]
mod tests {
    use super::{read_twoda, write_twoda};

    #[test]
    fn edited_source_backed_twoda_preserves_row_layout_on_cell_edit() {
        let bytes = b"2DA V2.0\r\n\r\n   LABEL\r\nrow0   \"a b\"\r\n\r\n".to_vec();
        let mut value = read_twoda(std::io::Cursor::new(bytes)).expect("parse twoda");
        value
            .set_cell(0, "LABEL", Some("changed".to_string()))
            .expect("edit cell");

        let mut output = Vec::new();
        write_twoda(&mut output, &value, false).expect("write twoda");
        let text = String::from_utf8(output).expect("utf8 output");

        assert!(text.contains("\r\n\r\n   LABEL\r\n"));
        assert!(text.contains("row0   \"changed\"\r\n"));
        assert!(text.ends_with("\r\n\r\n"));
    }

    #[test]
    fn edited_source_backed_twoda_preserves_header_and_row_label_edits() {
        let bytes = b"2DA V2.0\nDEFAULT: ****\n   OLD\nrow0   value\n".to_vec();
        let mut value = read_twoda(std::io::Cursor::new(bytes)).expect("parse twoda");
        value
            .set_columns(vec!["NEW".to_string()])
            .expect("rename header");
        value.set_row_label(0, "custom0").expect("rename row label");

        let mut output = Vec::new();
        write_twoda(&mut output, &value, false).expect("write twoda");
        let text = String::from_utf8(output).expect("utf8 output");

        assert!(text.contains("\n   NEW\n"));
        assert!(text.contains("custom0   value\n"));
    }

    #[test]
    fn edited_source_backed_twoda_appends_rows_and_columns() {
        let bytes = b"2DA V2.0\n\n   COL0\n0   alpha\n".to_vec();
        let mut value = read_twoda(std::io::Cursor::new(bytes)).expect("parse twoda");
        value
            .set_columns(vec!["COL0".to_string(), "COL1".to_string()])
            .expect("add column");
        value.set_row(0, vec![Some("alpha".to_string()), Some("beta".to_string())]);
        value.set_row(
            1,
            vec![Some("gamma".to_string()), Some("delta".to_string())],
        );
        value.set_row_label(1, "newrow").expect("set new row label");

        let mut output = Vec::new();
        write_twoda(&mut output, &value, false).expect("write twoda");
        let text = String::from_utf8(output).expect("utf8 output");

        assert!(text.contains("\n   COL0 COL1\n"));
        assert!(text.contains("0   alpha beta\n"));
        assert!(text.contains("newrow   gamma delta\n"));
    }
}
