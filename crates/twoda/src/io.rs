use std::io::{Read, Write};

use nwnrs_resman::prelude::*;
use nwnrs_util::prelude::*;
use tracing::{debug, instrument};

use crate::{CELL_PADDING, CELL_PADDING_MINI, MAX_COLUMNS, prelude::*};

/// Reads a `2DA V2.0` table from text.
#[instrument(level = "debug", skip_all, err)]
pub fn read_twoda<R: Read>(mut reader: R) -> TwoDaResult<TwoDa> {
    let mut bytes = Vec::new();
    reader.read_to_end(&mut bytes)?;
    let decoded = from_nwnrs_encoding(&bytes)?;
    let normalized = decoded.replace("\r\n", "\n").replace('\r', "\n");
    let mut lines = normalized.lines().map(str::trim).peekable();

    let mut next_nonempty = || -> TwoDaResult<String> {
        for line in lines.by_ref() {
            if !line.is_empty() {
                return Ok(line.to_string());
            }
        }
        Err(TwoDaError::msg("EOF while reading 2da"))
    };

    let mut twoda = TwoDa::new();
    if next_nonempty()? != TWO_DA_HEADER {
        return Err(TwoDaError::msg("invalid 2da header"));
    }

    let default_or_headers = next_nonempty()?;
    if let Some(rest) = default_or_headers.strip_prefix("DEFAULT:") {
        twoda.default_value = read_fields(rest.trim_start(), 1, 0)?
            .into_iter()
            .next()
            .unwrap_or(None);
        let headers = read_fields(&next_nonempty()?, MAX_COLUMNS, 0)?;
        if headers.iter().any(Option::is_none) {
            return Err(TwoDaError::msg("empty header fields not supported"));
        }
        twoda.set_columns(headers.into_iter().map(Option::unwrap).collect())?;
    } else {
        let headers = read_fields(&default_or_headers, MAX_COLUMNS, 0)?;
        if headers.iter().any(Option::is_none) {
            return Err(TwoDaError::msg("empty header fields not supported"));
        }
        twoda.set_columns(headers.into_iter().map(Option::unwrap).collect())?;
    }

    for line in lines {
        if line.is_empty() {
            continue;
        }
        let fields = read_fields(line, twoda.headers.len() + 1, 0)?;
        let mut row = fields.iter().skip(1).cloned().collect::<Vec<_>>();
        while row.len() < twoda.headers.len() {
            row.push(None);
        }
        twoda.rows.push(row);
    }

    while twoda
        .rows
        .last()
        .is_some_and(|row| row.iter().all(Option::is_none))
    {
        twoda.rows.pop();
    }

    debug!(
        rows = twoda.rows.len(),
        columns = twoda.headers.len(),
        "read 2da"
    );
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

fn read_fields(line: &str, maxcount: usize, minpad: usize) -> TwoDaResult<Vec<Cell>> {
    let mut result = Vec::new();
    let mut current = String::new();
    let mut quotes = false;

    let add_field = |result: &mut Vec<Cell>, current: &mut String| -> TwoDaResult<bool> {
        if current.is_empty() || current == "****" {
            result.push(None);
        } else {
            result.push(Some(current.clone()));
        }
        current.clear();
        Ok(result.len() >= maxcount)
    };

    for ch in line.chars() {
        match ch {
            '"' => quotes = !quotes,
            ' ' | '\t' => {
                if !quotes && !current.is_empty() {
                    if add_field(&mut result, &mut current)? {
                        return Ok(result);
                    }
                } else if quotes {
                    current.push(ch);
                }
            }
            _ => current.push(ch),
        }
    }
    let _ = add_field(&mut result, &mut current)?;

    while result.len() < minpad {
        result.push(None);
    }
    Ok(result)
}
