#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    resman::lookup_res_type,
    twoda::{TwoDa, read_twoda, write_twoda},
};
use support::{
    read_first_shipped_resource_bytes_matching, require_game_resource,
    test_shipped_resource_by_extension_matching,
};

#[test]
fn edited_source_backed_twoda_preserves_row_layout_on_cell_edit() -> Result<(), Box<dyn Error>> {
    let mut value = shipped_twoda_fixture()?;
    let first_column = value
        .columns()
        .first()
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture must have one column"))?;
    value
        .set_cell(0, &first_column, Some("changed".to_string()))
        .expect("edit cell");

    let mut output = Vec::new();
    write_twoda(&mut output, &value, false).expect("write twoda");
    let reparsed = read_twoda(Cursor::new(output)).expect("reparse twoda");

    assert_eq!(
        reparsed
            .rows
            .first()
            .and_then(|row| row.first())
            .and_then(|cell| cell.as_deref()),
        Some("changed")
    );
    Ok(())
}

#[test]
fn edited_source_backed_twoda_preserves_header_and_row_label_edits() -> Result<(), Box<dyn Error>> {
    let mut value = shipped_twoda_fixture()?;
    let mut columns = value.columns().to_vec();
    let first_column = columns
        .get_mut(0)
        .ok_or_else(|| std::io::Error::other("fixture must have one column"))?;
    *first_column = "__nwnrs_test_col".to_string();
    value.set_columns(columns).expect("rename header");
    value.set_row_label(0, "custom0").expect("rename row label");

    let mut output = Vec::new();
    write_twoda(&mut output, &value, false).expect("write twoda");
    let reparsed = read_twoda(Cursor::new(output)).expect("reparse twoda");

    assert_eq!(
        reparsed.columns().first().map(String::as_str),
        Some("__nwnrs_test_col")
    );
    assert_eq!(reparsed.row_label(0), Some("custom0"));
    Ok(())
}

#[test]
fn edited_source_backed_twoda_appends_rows_and_columns() -> Result<(), Box<dyn Error>> {
    let source = shipped_twoda_fixture()?;
    let mut value = TwoDa::new();
    value.set_default(source.default());
    value
        .set_columns(source.columns().to_vec())
        .expect("copy columns");
    value
        .replace_rows(
            source.rows.clone(),
            (0..source.len())
                .map(|index| source.row_label(index).unwrap_or("").to_string())
                .collect(),
        )
        .expect("copy rows");
    let original_row_count = value.rows.len();
    let mut columns = value.columns().to_vec();
    columns.push("__nwnrs_test_extra".to_string());
    value.set_columns(columns.clone()).expect("add column");

    let mut rows = value.rows.clone();
    let mut row_labels = (0..value.len())
        .map(|index| value.row_label(index).unwrap_or("").to_string())
        .collect::<Vec<_>>();
    let mut first_row = rows
        .first()
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture must have one row"))?;
    first_row.push(Some("beta".to_string()));
    let first_row_slot = rows
        .get_mut(0)
        .ok_or_else(|| std::io::Error::other("fixture must have one row"))?;
    *first_row_slot = first_row;
    rows.push(
        columns
            .iter()
            .enumerate()
            .map(|(index, _column)| {
                Some(
                    if index + 1 == columns.len() {
                        "delta"
                    } else {
                        "gamma"
                    }
                    .to_string(),
                )
            })
            .collect(),
    );
    row_labels.push("newrow".to_string());
    value.replace_rows(rows, row_labels).expect("replace rows");

    let mut output = Vec::new();
    write_twoda(&mut output, &value, false).expect("write twoda");
    let reparsed = read_twoda(Cursor::new(output)).expect("reparse twoda");

    assert_eq!(
        reparsed.columns().last().map(String::as_str),
        Some("__nwnrs_test_extra")
    );
    assert_eq!(reparsed.rows.len(), original_row_count + 1);
    assert_eq!(
        reparsed
            .rows
            .last()
            .and_then(|row| row.first())
            .and_then(|cell| cell.as_deref()),
        Some("gamma")
    );
    Ok(())
}

#[test]
fn twoda_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension_matching("2da", |_resref, bytes| {
        read_twoda(Cursor::new(bytes))
            .is_ok_and(|table| !table.columns().is_empty() && !table.rows.is_empty())
    })
}

fn shipped_twoda_fixture() -> Result<TwoDa, Box<dyn Error>> {
    let twoda_type =
        lookup_res_type("2da").ok_or_else(|| std::io::Error::other("missing .2da res type"))?;
    let (_resref, bytes) = require_game_resource(read_first_shipped_resource_bytes_matching(
        twoda_type,
        |_resref, bytes| {
            read_twoda(Cursor::new(bytes))
                .is_ok_and(|table| !table.columns().is_empty() && !table.rows.is_empty())
        },
    ))?;
    Ok(read_twoda(Cursor::new(bytes))?)
}
