//! Lists installed resources of one type matching a case-insensitive substring.

use nwnrs_game::prelude::{find_nwnrs_root, find_user_root, new_default_resman};
use nwnrs_resref::prelude::ResolvedResRef;

fn main() {
    let extension = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "mdl".to_string())
        .to_ascii_lowercase();
    let needle = std::env::args()
        .nth(2)
        .unwrap_or_default()
        .to_ascii_lowercase();
    let limit = std::env::args()
        .nth(3)
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(100);

    let res_type = ResolvedResRef::from_filename(format!("dummy.{extension}").as_str())
        .unwrap_or_else(|_error| {
            panic!("unknown resource extension: {extension}");
        })
        .res_type();

    let root = find_nwnrs_root("").unwrap_or_else(|error| {
        panic!("resolve NWN root: {error}");
    });
    let user_root = find_user_root("").unwrap_or_else(|error| {
        panic!("resolve NWN user root: {error}");
    });
    let resman = new_default_resman(
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

    let mut matches = resman
        .contents()
        .into_iter()
        .filter(|resref| resref.res_type() == res_type)
        .map(|resref| resref.res_ref().to_string())
        .filter(|name| needle.is_empty() || name.to_ascii_lowercase().contains(&needle))
        .collect::<Vec<_>>();
    matches.sort_unstable();
    matches.dedup();

    println!("extension={extension}");
    println!("matches={}", matches.len());
    for name in matches.into_iter().take(limit) {
        println!("{name}");
    }
}
