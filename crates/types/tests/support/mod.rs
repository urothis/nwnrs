#![allow(unused_imports)]
#![allow(dead_code)]

use std::{
    error::Error,
    io::{self, Cursor},
    path::Path,
};

pub use nwnrs_types::test_support::{
    TestResourceError, demand_resource, find_first_shipped_file, materialize_bytes_to_temp_file,
    materialize_resource_to_temp_file, read_first_shipped_file_bytes_matching,
    read_first_shipped_resource_bytes, read_first_shipped_resource_bytes_matching,
    read_resource_bytes, read_shipped_dialog_tlk_bytes, require_game_resource,
    shipped_archive_candidates, skip_if_game_resources_unavailable,
};
use nwnrs_types::{
    erf::prelude::*,
    gff::prelude::*,
    resman::{ResType, ResolvedResRef, lookup_res_type},
    ssf::prelude::*,
    tlk::prelude::*,
    twoda::prelude::*,
};

pub fn require_res_type(extension: &str) -> Result<ResType, Box<dyn Error>> {
    lookup_res_type(extension)
        .ok_or_else(|| io::Error::other(format!("missing res type for .{extension}")).into())
}

pub fn filename_from_path(path: &Path) -> Result<String, Box<dyn Error>> {
    path.file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| io::Error::other(format!("path has no filename: {}", path.display())).into())
}

pub fn roundtrip_gff(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut reader = Cursor::new(bytes);
    let root = read_gff_root(&mut reader)?;
    let mut writer = Cursor::new(Vec::new());
    write_gff_root(&mut writer, &root)?;
    Ok(writer.into_inner())
}

pub fn roundtrip_erf_like(bytes: &[u8], filename: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let erf = read_erf(Cursor::new(bytes.to_vec()), filename.to_string())?;
    let mut writer = Cursor::new(Vec::new());
    write_erf_archive(&mut writer, &erf)?;
    Ok(writer.into_inner())
}

pub fn roundtrip_tlk(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut tlk = read_single_tlk(Cursor::new(bytes.to_vec()), CachePolicy::Bypass)?;
    let mut writer = Cursor::new(Vec::new());
    write_single_tlk(&mut writer, &mut tlk)?;
    Ok(writer.into_inner())
}

pub fn roundtrip_twoda(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let twoda = read_twoda(Cursor::new(bytes))?;
    let mut writer = Cursor::new(Vec::new());
    write_twoda(&mut writer, &twoda, false)?;
    Ok(writer.into_inner())
}

pub fn roundtrip_ssf(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut reader = Cursor::new(bytes);
    let ssf = read_ssf(&mut reader)?;
    let mut writer = Cursor::new(Vec::new());
    write_ssf(&mut writer, &ssf)?;
    Ok(writer.into_inner())
}

pub fn roundtrip_bytes(bytes: &[u8], filename: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let extension = filename
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| io::Error::other(format!("filename has no extension: {filename}")))?;

    match extension.as_str() {
        "gff" | "bic" | "dlg" | "itp" | "utc" | "utd" | "ute" | "uti" | "utm" | "utp" | "uts"
        | "utt" | "utw" => roundtrip_gff(bytes),
        "erf" | "mod" | "hak" | "nwm" => roundtrip_erf_like(bytes, filename),
        "tlk" => roundtrip_tlk(bytes),
        "2da" => roundtrip_twoda(bytes),
        "ssf" => roundtrip_ssf(bytes),
        _ => Err(
            io::Error::other(format!("unsupported roundtrip test extension: {filename}")).into(),
        ),
    }
}

pub fn test_roundtrip_bytes(bytes: &[u8], filename: &str) -> Result<(), Box<dyn Error>> {
    let repacked = roundtrip_bytes(bytes, filename)?;
    assert_eq!(bytes, repacked, "roundtrip byte mismatch for {filename}");
    Ok(())
}

pub fn test_shipped_resource_by_extension(extension: &str) -> Result<(), Box<dyn Error>> {
    let res_type = require_res_type(extension)?;
    let (resref, bytes) = require_game_resource(read_first_shipped_resource_bytes(res_type))?;
    test_roundtrip_bytes(&bytes, &resref.to_file())
}

pub fn test_shipped_resource_by_extension_matching<F>(
    extension: &str,
    predicate: F,
) -> Result<(), Box<dyn Error>>
where
    F: FnMut(&ResolvedResRef, &[u8]) -> bool,
{
    let res_type = require_res_type(extension)?;
    let (resref, bytes) = require_game_resource(read_first_shipped_resource_bytes_matching(
        res_type, predicate,
    ))?;
    test_roundtrip_bytes(&bytes, &resref.to_file())
}

pub fn test_shipped_file(extension: &str) -> Result<(), Box<dyn Error>> {
    let path = require_game_resource(find_first_shipped_file(extension))?;
    let bytes = std::fs::read(&path)?;
    let filename = filename_from_path(&path)?;
    test_roundtrip_bytes(&bytes, &filename)
}

pub fn test_shipped_file_matching<F>(extension: &str, predicate: F) -> Result<(), Box<dyn Error>>
where
    F: FnMut(&Path, &[u8]) -> bool,
{
    let (path, bytes) =
        require_game_resource(read_first_shipped_file_bytes_matching(extension, predicate))?;
    let filename = filename_from_path(&path)?;
    test_roundtrip_bytes(&bytes, &filename)
}

pub fn test_shipped_archive(extension: &str) -> Result<(), Box<dyn Error>> {
    let candidates = require_game_resource(shipped_archive_candidates(extension))?;
    for path in candidates {
        let original = std::fs::read(&path)?;
        let filename = filename_from_path(&path)?;
        let repacked = roundtrip_bytes(&original, &filename)?;
        if original == repacked {
            return Ok(());
        }
    }

    Err(io::Error::other(format!(
        "no shipped .{extension} archive roundtripped byte-exactly"
    ))
    .into())
}
