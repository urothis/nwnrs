use std::{fmt, io};

use nwnrs_encoding::prelude::*;
use nwnrs_resman::prelude::*;

/// Canonical header string for `2DA V2.0` files.
pub const TWO_DA_HEADER: &str = "2DA V2.0";
pub(crate) const CELL_PADDING: usize = 2;
pub(crate) const CELL_PADDING_MINI: usize = 1;
pub(crate) const MAX_COLUMNS: usize = 1024;

/// A single 2DA cell.
///
/// `None` represents the `****` sentinel used for missing values.
pub type Cell = Option<String>;
/// A single 2DA row.
pub type Row = Vec<Cell>;

#[derive(Debug)]
/// Errors returned while reading or writing 2DA tables.
pub enum TwoDaError {
    /// An underlying IO operation failed.
    Io(io::Error),
    /// Resource-manager access failed.
    ResMan(ResManError),
    /// Text could not be converted using the configured NWN encoding.
    Encoding(EncodingConversionError),
    /// The table contents were otherwise invalid.
    Message(String),
}

impl TwoDaError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for TwoDaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Encoding(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for TwoDaError {}

impl From<io::Error> for TwoDaError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<ResManError> for TwoDaError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<EncodingConversionError> for TwoDaError {
    fn from(value: EncodingConversionError) -> Self {
        Self::Encoding(value)
    }
}

/// Result type for 2DA operations.
pub type TwoDaResult<T> = Result<T, TwoDaError>;

#[derive(Debug, Clone)]
/// An in-memory `2DA V2.0` table.
///
/// The representation preserves the authored table shape: ordered columns,
/// ordered rows, explicit row labels, and an optional table-wide default cell
/// value.
pub struct TwoDa {
    pub(crate) default_value: Cell,
    pub(crate) headers: Vec<String>,
    pub(crate) headers_for_lookup: Vec<String>,
    pub(crate) row_labels: Vec<String>,
    /// Ordered row contents.
    pub rows: Vec<Row>,
    pub(crate) source_bytes: Option<Vec<u8>>,
    pub(crate) source_snapshot: Option<TwoDaSnapshot>,
    pub(crate) source_layout: Option<TwoDaSourceLayout>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct TwoDaSnapshot {
    pub(crate) default_value: Cell,
    pub(crate) headers:       Vec<String>,
    pub(crate) row_labels:    Vec<String>,
    pub(crate) rows:          Vec<Row>,
}

#[derive(Debug, Clone)]
pub(crate) struct TwoDaSourceLayout {
    pub(crate) lines: Vec<TwoDaSourceLine>,
}

#[derive(Debug, Clone)]
pub(crate) enum TwoDaSourceLine {
    HeaderMagic {
        raw:         String,
        line_ending: String,
    },
    Blank {
        raw:         String,
        line_ending: String,
    },
    Default {
        prefix:      String,
        value:       Option<TwoDaTokenLayout>,
        trailing:    String,
        line_ending: String,
    },
    HeaderRow {
        leading:     String,
        columns:     Vec<String>,
        separators:  Vec<String>,
        trailing:    String,
        line_ending: String,
    },
    DataRow {
        row_index:   usize,
        after_label: String,
        cells:       Vec<TwoDaTokenLayout>,
        separators:  Vec<String>,
        trailing:    String,
        line_ending: String,
    },
}

#[derive(Debug, Clone)]
pub(crate) struct TwoDaTokenLayout {
    pub(crate) value:  Cell,
    pub(crate) quoted: bool,
    pub(crate) raw:    String,
}

impl TwoDa {
    /// Creates an empty table.
    ///
    /// Columns, rows, row labels, and the table-wide default value may then be
    /// populated incrementally.
    #[must_use]
    pub fn new() -> Self {
        Self {
            default_value:      None,
            headers:            Vec::new(),
            headers_for_lookup: Vec::new(),
            row_labels:         Vec::new(),
            rows:               Vec::new(),
            source_bytes:       None,
            source_snapshot:    None,
            source_layout:      None,
        }
    }

    /// Returns a cloned row by index.
    #[must_use]
    pub fn row(&self, row: usize) -> Option<Row> {
        self.rows.get(row).cloned()
    }

    /// Replaces a row, extending the table with empty rows when necessary.
    pub fn set_row(&mut self, row: usize, data: Row) {
        if let Some(slot) = self.rows.get_mut(row) {
            *slot = data;
        } else {
            while self.rows.len() < row {
                self.rows.push(Vec::new());
                self.row_labels.push(self.row_labels.len().to_string());
            }
            self.rows.push(data);
            self.row_labels.push(row.to_string());
        }
    }

    /// Returns the cell at `row` and `column`, falling back to the table
    /// default.
    #[must_use]
    pub fn cell(&self, row: usize, column: &str) -> Cell {
        let mut result = self.default_value.clone();
        if let Some(row_data) = self.rows.get(row)
            && let Some(column_id) = self
                .headers_for_lookup
                .iter()
                .position(|hdr| hdr == &column.to_ascii_lowercase())
            && let Some(value) = row_data.get(column_id)
            && value.is_some()
        {
            result.clone_from(value);
        }
        result
    }

    /// Returns the cell at `row` and `column`, substituting `default` when it
    /// is missing.
    #[must_use]
    pub fn cell_or(&self, row: usize, column: &str, default: &str) -> String {
        self.cell(row, column)
            .unwrap_or_else(|| default.to_string())
    }

    /// Sets the cell at `row` and `column`.
    pub fn set_cell(&mut self, row: usize, column: &str, value: Cell) -> TwoDaResult<()> {
        if row >= self.rows.len() {
            return Err(TwoDaError::msg("Row out of bounds"));
        }

        let Some(column_id) = self
            .headers_for_lookup
            .iter()
            .position(|hdr| hdr == &column.to_ascii_lowercase())
        else {
            return Err(TwoDaError::msg(format!("Column not found: {column}")));
        };

        let row_data = self
            .rows
            .get_mut(row)
            .ok_or_else(|| TwoDaError::msg("Row out of bounds"))?;
        if row_data.len() <= column_id {
            row_data.resize(column_id + 1, None);
        }
        let slot = row_data
            .get_mut(column_id)
            .ok_or_else(|| TwoDaError::msg("Column out of bounds"))?;
        *slot = value;
        Ok(())
    }

    /// Returns the lowest valid row index, which is always `0`.
    #[must_use]
    pub fn low(&self) -> usize {
        0
    }

    /// Returns the highest valid row index, if any rows exist.
    #[must_use]
    pub fn high(&self) -> Option<usize> {
        self.rows.len().checked_sub(1)
    }

    /// Returns the number of rows in the table.
    #[must_use]
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns whether the table has no rows.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Returns the table-wide default cell value.
    #[must_use]
    pub fn default(&self) -> Cell {
        self.default_value.clone()
    }

    /// Sets the table-wide default cell value.
    pub fn set_default(&mut self, value: Cell) {
        self.default_value = value;
    }

    /// Returns the stored row label, if present.
    pub fn row_label(&self, row: usize) -> Option<&str> {
        self.row_labels.get(row).map(String::as_str)
    }

    /// Replaces the stored row label.
    pub fn set_row_label(&mut self, row: usize, label: impl Into<String>) -> TwoDaResult<()> {
        let slot = self
            .row_labels
            .get_mut(row)
            .ok_or_else(|| TwoDaError::msg("Row out of bounds"))?;
        *slot = label.into();
        Ok(())
    }

    /// Replaces all rows and row labels at once.
    pub fn replace_rows(&mut self, rows: Vec<Row>, row_labels: Vec<String>) -> TwoDaResult<()> {
        if rows.len() != row_labels.len() {
            return Err(TwoDaError::msg("row data and row labels length mismatch"));
        }
        self.rows = rows;
        self.row_labels = row_labels;
        Ok(())
    }

    /// Returns the ordered column names.
    #[must_use]
    pub fn columns(&self) -> &[String] {
        &self.headers
    }

    /// Replaces the column list.
    ///
    /// Column lookups are case-insensitive.
    pub fn set_columns(&mut self, columns: Vec<String>) -> TwoDaResult<()> {
        for column in &columns {
            if column.trim().is_empty() {
                return Err(TwoDaError::msg(format!("invalid column value: {column:?}")));
            }
        }
        self.headers_for_lookup = columns
            .iter()
            .map(|column| column.to_ascii_lowercase())
            .collect();
        self.headers = columns;
        Ok(())
    }

    pub(crate) fn snapshot(&self) -> TwoDaSnapshot {
        TwoDaSnapshot {
            default_value: self.default_value.clone(),
            headers:       self.headers.clone(),
            row_labels:    self.row_labels.clone(),
            rows:          self.rows.clone(),
        }
    }
}

impl Default for TwoDa {
    fn default() -> Self {
        Self::new()
    }
}

impl PartialEq for TwoDa {
    fn eq(&self, other: &Self) -> bool {
        self.default_value == other.default_value
            && self.headers == other.headers
            && self.row_labels == other.row_labels
            && self.rows == other.rows
    }
}

impl Eq for TwoDa {}
