#![allow(missing_docs)]

mod support;

use std::{error::Error, io::Cursor};

use nwnrs_types::{
    gff::{GffRoot, GffStruct, GffValue, read_gff_root, write_gff_root},
    resman::lookup_res_type,
};
use support::{
    read_first_shipped_resource_bytes_matching, require_game_resource, test_shipped_file,
    test_shipped_resource_by_extension,
};

#[test]
fn edited_source_backed_gff_preserves_value_edit() -> Result<(), Box<dyn Error>> {
    let mut parsed = shipped_gff_fixture()?;
    parsed.put_value("nr_test_cmt", GffValue::CExoString("before".to_string()))?;
    parsed.put_value("nr_test_cmt", GffValue::CExoString("after".to_string()))?;

    let mut output = Cursor::new(Vec::new());
    write_gff_root(&mut output, &parsed)?;
    let reparsed = read_gff_root(&mut Cursor::new(output.into_inner()))?;
    let comment = reparsed
        .root
        .get_field("nr_test_cmt")
        .and_then(|field| match field.value() {
            GffValue::CExoString(value) => Some(value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("comment field missing"));
    assert_eq!(comment, "after");
    Ok(())
}

#[test]
fn edited_source_backed_gff_allows_structural_field_edits() -> Result<(), Box<dyn Error>> {
    let mut parsed = shipped_gff_fixture()?;
    parsed.put_value("nr_test_rm", GffValue::Int(7))?;
    parsed.root.remove("nr_test_rm");
    parsed.put_value("nr_test_str", GffValue::Struct(GffStruct::new(7)))?;

    let mut output = Cursor::new(Vec::new());
    write_gff_root(&mut output, &parsed)?;
    let reparsed = read_gff_root(&mut Cursor::new(output.into_inner()))?;

    assert!(reparsed.root.get_field("nr_test_rm").is_none());
    assert!(matches!(
        reparsed
            .root
            .get_field("nr_test_str")
            .map(|field| field.value()),
        Some(GffValue::Struct(_))
    ));
    Ok(())
}

#[test]
fn edited_source_backed_gff_allows_list_resize() -> Result<(), Box<dyn Error>> {
    let mut parsed = shipped_gff_fixture()?;
    parsed.put_value(
        "nr_test_items",
        GffValue::List(vec![GffStruct::new(1), GffStruct::new(2)]),
    )?;
    parsed.put_value("nr_test_items", GffValue::List(vec![GffStruct::new(1)]))?;

    let mut output = Cursor::new(Vec::new());
    write_gff_root(&mut output, &parsed)?;
    let reparsed = read_gff_root(&mut Cursor::new(output.into_inner()))?;

    assert!(matches!(
        reparsed
            .root
            .get_field("nr_test_items")
            .map(|field| field.value()),
        Some(GffValue::List(items)) if items.len() == 1
    ));
    Ok(())
}

#[test]
fn gff_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("gff")
}

#[test]
fn bic_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_file("bic")
}

#[test]
fn dlg_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("dlg")
}

#[test]
fn itp_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("itp")
}

#[test]
fn utc_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("utc")
}

#[test]
fn utd_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("utd")
}

#[test]
fn ute_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("ute")
}

#[test]
fn uti_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("uti")
}

#[test]
fn utm_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("utm")
}

#[test]
fn utp_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("utp")
}

#[test]
fn uts_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("uts")
}

#[test]
fn utt_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("utt")
}

#[test]
fn utw_roundtrip() -> Result<(), Box<dyn Error>> {
    test_shipped_resource_by_extension("utw")
}

fn shipped_gff_fixture() -> Result<GffRoot, Box<dyn Error>> {
    let gff_type =
        lookup_res_type("gff").ok_or_else(|| std::io::Error::other("missing .gff res type"))?;
    let (_resref, bytes) = require_game_resource(read_first_shipped_resource_bytes_matching(
        gff_type,
        |_resref, bytes| read_gff_root(&mut Cursor::new(bytes)).is_ok(),
    ))?;
    Ok(read_gff_root(&mut Cursor::new(bytes))?)
}
