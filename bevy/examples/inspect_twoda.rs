//! Prints one installed 2DA table with columns and a selectable row range.

use nwnrs_game::prelude::{find_nwnrs_root, find_user_root, new_default_resman};
use nwnrs_resref::prelude::ResolvedResRef;
use nwnrs_twoda::prelude::as_2da;

fn main() {
    let table_name = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "appearance".to_string());
    let start = std::env::args()
        .nth(2)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0);
    let count = std::env::args()
        .nth(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(16);

    let root = find_nwnrs_root("").unwrap_or_else(|error| {
        panic!("resolve NWN root: {error}");
    });
    let user_root = find_user_root("").unwrap_or_else(|error| {
        panic!("resolve NWN user root: {error}");
    });
    let mut resman = new_default_resman(
        &root,
        &user_root,
        "english",
        0,
        true,
        false,
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap_or_else(|error| {
        panic!("build default resman: {error}");
    });

    let resolved =
        ResolvedResRef::from_filename(&format!("{table_name}.2da")).unwrap_or_else(|error| {
            panic!("invalid 2DA resref {table_name}.2da: {error}");
        });
    let res = resman.get_resolved(&resolved).unwrap_or_else(|| {
        panic!("2DA not found in resman: {table_name}.2da");
    });
    let table = as_2da(&res).unwrap_or_else(|error| {
        panic!("read 2DA {table_name}: {error}");
    });

    println!("table={table_name}");
    println!("rows={}", table.len());
    println!("columns={}", table.columns().join(", "));

    let end = start.saturating_add(count).min(table.len());
    for row in start..end {
        let label = table.row_label(row).unwrap_or("");
        let values = table
            .columns()
            .iter()
            .map(|column| {
                format!(
                    "{}={}",
                    column,
                    table
                        .cell(row, column)
                        .unwrap_or_else(|| "****".to_string())
                )
            })
            .collect::<Vec<_>>();
        println!("row={row} label={label} {}", values.join(" "));
    }
}
