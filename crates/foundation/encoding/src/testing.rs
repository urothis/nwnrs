use std::{
    error::Error,
    io::{self, Cursor},
    path::{Path, PathBuf},
};

use nwnrs_erf::prelude::*;
use nwnrs_gff::prelude::*;
use nwnrs_ssf::prelude::*;
use nwnrs_tlk::prelude::*;
use nwnrs_twoda::prelude::*;

fn filename_from_resource(resource: &str) -> Result<String, Box<dyn Error>> {
    if let Ok(url) = reqwest::Url::parse(resource)
        && matches!(url.scheme(), "http" | "https")
    {
        return url
            .path_segments()
            .and_then(|mut segments| segments.rfind(|segment| !segment.is_empty()))
            .map(str::to_string)
            .ok_or_else(|| io::Error::other(format!("url has no filename: {url}")).into());
    }

    Path::new(resource)
        .file_name()
        .and_then(|name| name.to_str())
        .map(str::to_string)
        .ok_or_else(|| io::Error::other(format!("path has no filename: {resource}")).into())
}

fn resolve_local_resource_path(resource: &str) -> PathBuf {
    let path = Path::new(resource);
    if path.is_absolute() {
        return path.to_path_buf();
    }

    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(path)
}

fn roundtrip_gff(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut reader = Cursor::new(bytes);
    let root = read_gff_root(&mut reader)?;
    let mut writer = Cursor::new(Vec::new());
    write_gff_root(&mut writer, &root)?;
    Ok(writer.into_inner())
}

fn roundtrip_erf_like(bytes: &[u8], filename: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let erf = read_erf(Cursor::new(bytes.to_vec()), filename.to_string())?;
    let mut writer = Cursor::new(Vec::new());
    write_erf_archive(&mut writer, &erf)?;

    Ok(writer.into_inner())
}

fn roundtrip_tlk(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut tlk = read_single_tlk(Cursor::new(bytes.to_vec()), false)?;
    let mut writer = Cursor::new(Vec::new());
    write_single_tlk(&mut writer, &mut tlk)?;
    Ok(writer.into_inner())
}

fn roundtrip_twoda(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let twoda = read_twoda(Cursor::new(bytes))?;
    let mut writer = Cursor::new(Vec::new());
    write_twoda(&mut writer, &twoda, false)?;
    Ok(writer.into_inner())
}

fn roundtrip_ssf(bytes: &[u8]) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut reader = Cursor::new(bytes);
    let ssf = read_ssf(&mut reader)?;
    let mut writer = Cursor::new(Vec::new());
    write_ssf(&mut writer, &ssf)?;
    Ok(writer.into_inner())
}

fn roundtrip_bytes(bytes: &[u8], filename: &str) -> Result<Vec<u8>, Box<dyn Error>> {
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

async fn download_resource(url: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let response = reqwest::get(url).await?.error_for_status()?;
    Ok(response.bytes().await?.to_vec())
}

async fn load_resource(resource: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    if let Some(filename) = resource.strip_prefix("generated:") {
        return generated_resource(filename);
    }

    if let Ok(url) = reqwest::Url::parse(resource)
        && matches!(url.scheme(), "http" | "https")
    {
        return download_resource(url.as_str()).await;
    }

    Ok(tokio::fs::read(resolve_local_resource_path(resource)).await?)
}

fn generated_gff_bytes(file_type: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let mut root = new_gff_root(file_type);
    root.root.put_value(
        "Comment".to_string(),
        GffValue::CExoString("fixture".to_string()),
    )?;
    let mut writer = Cursor::new(Vec::new());
    write_gff_root(&mut writer, &root)?;
    Ok(writer.into_inner())
}

fn generated_twoda_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let mut twoda = TwoDa::new();
    twoda.set_columns(vec!["LABEL".to_string()])?;
    twoda.set_default(Some("****".to_string()));
    twoda.set_row(0, vec![Some("value".to_string())]);
    let mut writer = Vec::new();
    write_twoda(&mut writer, &twoda, false)?;
    Ok(writer)
}

fn generated_tlk_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let mut tlk = SingleTlk::new();
    tlk.set_entry(0, TlkEntry::new("fixture", "sound01", 1.25));
    let mut writer = Cursor::new(Vec::new());
    write_single_tlk(&mut writer, &mut tlk)?;
    Ok(writer.into_inner())
}

fn generated_ssf_bytes() -> Result<Vec<u8>, Box<dyn Error>> {
    let mut ssf = new_ssf();
    ssf.entries.push(SsfEntry::new("hello", 7));
    let mut writer = Vec::new();
    write_ssf(&mut writer, &ssf)?;
    Ok(writer)
}

fn generated_resource(filename: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let extension = filename
        .rsplit('.')
        .next()
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| io::Error::other(format!("filename has no extension: {filename}")))?;
    match extension.as_str() {
        "gff" => generated_gff_bytes("GFF "),
        "are" => generated_gff_bytes("ARE "),
        "ifo" => generated_gff_bytes("IFO "),
        "bic" => generated_gff_bytes("BIC "),
        "dlg" => generated_gff_bytes("DLG "),
        "itp" => generated_gff_bytes("ITP "),
        "git" => generated_gff_bytes("GIT "),
        "utc" => generated_gff_bytes("UTC "),
        "utd" => generated_gff_bytes("UTD "),
        "ute" => generated_gff_bytes("UTE "),
        "uti" => generated_gff_bytes("UTI "),
        "utm" => generated_gff_bytes("UTM "),
        "utp" => generated_gff_bytes("UTP "),
        "uts" => generated_gff_bytes("UTS "),
        "utt" => generated_gff_bytes("UTT "),
        "utw" => generated_gff_bytes("UTW "),
        "fac" => generated_gff_bytes("FAC "),
        "gic" => generated_gff_bytes("GIC "),
        "jrl" => generated_gff_bytes("JRL "),
        "gui" => generated_gff_bytes("GUI "),
        "2da" => generated_twoda_bytes(),
        "tlk" => generated_tlk_bytes(),
        "ssf" => generated_ssf_bytes(),
        _ => Err(io::Error::other(format!(
            "unsupported generated fixture extension: {filename}"
        ))
        .into()),
    }
}

async fn test_resource(resource: &str) -> Result<(), Box<dyn Error>> {
    if resource.trim().is_empty() {
        return Ok(());
    }

    let filename = filename_from_resource(resource)?;
    let original = load_resource(resource).await?;
    let repacked = roundtrip_bytes(&original, &filename)?;
    assert_eq!(original, repacked, "roundtrip byte mismatch for {filename}");

    Ok(())
}

#[test]
fn resource_filename_supports_urls() -> Result<(), Box<dyn Error>> {
    let filename =
        filename_from_resource("https://example.com/assets/build8193.37/ssf/c_aasimar.ssf")?;
    assert_eq!(filename, "c_aasimar.ssf");
    Ok(())
}

#[test]
fn resource_filename_supports_local_paths() -> Result<(), Box<dyn Error>> {
    let filename = filename_from_resource("fixtures/archives/test.hak")?;
    assert_eq!(filename, "test.hak");
    Ok(())
}

#[tokio::test]
async fn erf_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("assets/testing/test.erf").await
}

#[tokio::test]
async fn gff_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:voiceset.gff").await
}

#[tokio::test]
async fn bic_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:aluviandarkstar.bic").await
}

#[tokio::test]
async fn dlg_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:m2q6a02aarin.dlg").await
}

#[tokio::test]
async fn itp_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:creaturepal.itp").await
}

#[tokio::test]
async fn mod_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("assets/testing/test.mod").await
}

#[tokio::test]
async fn hak_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("assets/testing/test.hak").await
}

#[tokio::test]
async fn tlk_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:dialog.tlk").await
}

#[tokio::test]
async fn twoda_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:appearance.2da").await
}

#[tokio::test]
async fn ssf_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:c_aasimar.ssf").await
}

#[tokio::test]
async fn utc_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:c_kocrachn.utc").await
}

#[tokio::test]
async fn utd_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:nw_door_evlstone.utd").await
}

#[tokio::test]
async fn ute_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:nw_aberration.ute").await
}

#[tokio::test]
async fn uti_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:am_it_kocra_hide.uti").await
}

#[tokio::test]
async fn utm_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:nw_lostitems.utm").await
}

#[tokio::test]
async fn utp_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:dd_pl_dagflag1.utp").await
}

#[tokio::test]
async fn uts_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:ailingmen.uts").await
}

#[tokio::test]
async fn utt_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:newgeneric.utt").await
}

#[tokio::test]
async fn utw_roundtrip() -> Result<(), Box<dyn Error>> {
    test_resource("generated:nw_mapnote001.utw").await
}
