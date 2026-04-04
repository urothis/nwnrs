use std::{fmt, io};

use nwn_resman::ResManError;
use nwn_util::EncodingConversionError;

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

#[derive(Debug, Clone, PartialEq, Eq)]
/// An in-memory `2DA V2.0` table.
pub struct TwoDa {
    pub(crate) default_value: Cell,
    pub(crate) headers: Vec<String>,
    pub(crate) headers_for_lookup: Vec<String>,
    /// Ordered row contents.
    pub rows: Vec<Row>,
}

impl TwoDa {
    /// Creates an empty table.
    pub fn new() -> Self {
        Self {
            default_value:      None,
            headers:            Vec::new(),
            headers_for_lookup: Vec::new(),
            rows:               Vec::new(),
        }
    }

    /// Returns a cloned row by index.
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
            }
            self.rows.push(data);
        }
    }

    /// Returns the cell at `row` and `column`, falling back to the table
    /// default.
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
            result = value.clone();
        }
        result
    }

    /// Returns the cell at `row` and `column`, substituting `default` when it
    /// is missing.
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
    pub fn low(&self) -> usize {
        0
    }

    /// Returns the highest valid row index, if any rows exist.
    pub fn high(&self) -> Option<usize> {
        self.rows.len().checked_sub(1)
    }

    /// Returns the number of rows in the table.
    pub fn len(&self) -> usize {
        self.rows.len()
    }

    /// Returns whether the table has no rows.
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Returns the table-wide default cell value.
    pub fn default(&self) -> Cell {
        self.default_value.clone()
    }

    /// Sets the table-wide default cell value.
    pub fn set_default(&mut self, value: Cell) {
        self.default_value = value;
    }

    /// Returns the ordered column names.
    pub fn columns(&self) -> &[String] {
        &self.headers
    }

    /// Replaces the column list.
    ///
    /// Column lookups are case-insensitive.
    pub fn set_columns(&mut self, columns: Vec<String>) -> TwoDaResult<()> {
        for column in &columns {
            if column.trim().is_empty() {
                return Err(TwoDaError::msg(format!(
                    "invalid column value: {:?}",
                    column
                )));
            }
        }
        self.headers_for_lookup = columns
            .iter()
            .map(|column| column.to_ascii_lowercase())
            .collect();
        self.headers = columns;
        Ok(())
    }
}

impl Default for TwoDa {
    fn default() -> Self {
        Self::new()
    }
}
