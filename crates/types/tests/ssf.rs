#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    resman::lookup_res_type,
    ssf::{read_ssf, write_ssf},
};
use support::{
    read_first_shipped_file_bytes_matching, read_first_shipped_resource_bytes_matching,
    require_game_resource, test_shipped_file_matching, test_shipped_resource_by_extension_matching,
};

#[test]
fn ssf_preserves_raw_resref_bytes_when_only_strref_changes() -> Result<(), Box<dyn Error>> {
    let original = shipped_ssf_fixture_bytes()?;
    let mut cursor = Cursor::new(original.clone());
    let mut ssf = read_ssf(&mut cursor)?;
    let original_entry = ssf
        .entries
        .first()
        .cloned()
        .ok_or_else(|| std::io::Error::other("fixture should contain one SSF entry"))?;
    if let Some(entry) = ssf.entries.get_mut(0) {
        entry.strref = 9;
    } else {
        panic!("fixture should contain one SSF entry");
    }

    let mut encoded = Vec::new();
    write_ssf(&mut encoded, &ssf)?;
    let reparsed = read_ssf(&mut Cursor::new(encoded))?;
    let reparsed_entry = reparsed
        .entries
        .first()
        .ok_or_else(|| std::io::Error::other("reparsed fixture should contain one SSF entry"))?;
    assert_eq!(reparsed_entry.raw_resref, original_entry.raw_resref);
    assert_eq!(reparsed_entry.strref, 9);
    Ok(())
}

#[test]
fn ssf_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension_matching("ssf", |_resref, bytes| {
        read_ssf(&mut Cursor::new(bytes)).is_ok_and(|value| !value.entries.is_empty())
    })
    .or_else(|_error| {
        test_shipped_file_matching("ssf", |_path, bytes| {
            read_ssf(&mut Cursor::new(bytes)).is_ok_and(|value| !value.entries.is_empty())
        })
    })
}

fn shipped_ssf_fixture_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let ssf_type =
        lookup_res_type("ssf").ok_or_else(|| std::io::Error::other("missing .ssf res type"))?;
    if let Ok((_resref, bytes)) = require_game_resource(read_first_shipped_resource_bytes_matching(
        ssf_type,
        |_resref, bytes| {
            read_ssf(&mut Cursor::new(bytes)).is_ok_and(|value| !value.entries.is_empty())
        },
    )) {
        return Ok(bytes);
    }

    let (_path, bytes) = require_game_resource(read_first_shipped_file_bytes_matching(
        "ssf",
        |_path, bytes| {
            read_ssf(&mut Cursor::new(bytes)).is_ok_and(|value| !value.entries.is_empty())
        },
    ))?;
    Ok(bytes)
}
