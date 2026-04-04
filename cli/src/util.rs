use std::{
    ffi::OsStr,
    fs::{self, File},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use nwn_compressedbuf::prelude::*;
use nwn_erf::prelude::*;
use nwn_exo::prelude::*;
use nwn_game::prelude::*;
use nwn_key::prelude::*;
use nwn_resref::prelude::*;

pub(crate) const RESOURCE_METADATA_FILENAME: &str = "resource.json";

#[derive(Clone, Copy)]
pub(crate) enum Kind {
    Gff,
    Ssf,
    Tlk,
    TwoDa,
    Erf,
    Key,
}

pub(crate) struct DirEntryInfo {
    pub(crate) file_name: String,
    pub(crate) path:      PathBuf,
}

pub(crate) fn detect_kind(path: &Path) -> Option<Kind> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "gff" | "are" | "bic" | "dlg" | "git" | "ifo" | "itp" | "jrl" | "utc" | "utd" | "ute"
        | "uti" | "utm" | "utp" | "uts" | "utt" | "utw" => Kind::Gff,
        "ssf" => Kind::Ssf,
        "tlk" => Kind::Tlk,
        "2da" => Kind::TwoDa,
        "erf" | "hak" | "mod" | "nwm" => Kind::Erf,
        "key" => Kind::Key,
        _ => return None,
    })
}

pub(crate) fn is_gff_extension(extension: &str) -> bool {
    GFF_EXTENSIONS
        .iter()
        .any(|candidate| candidate.eq_ignore_ascii_case(extension))
}

pub(crate) fn unpacked_raw_target(destination: &Path, file_name: &str, extension: &str) -> PathBuf {
    if matches!(extension, "ncs" | "nss") {
        destination.join(extension).join(file_name)
    } else {
        destination.join(file_name)
    }
}

pub(crate) fn parse_key_version(value: &str) -> Result<KeyBifVersion, String> {
    match value.to_ascii_uppercase().as_str() {
        "V1" => Ok(KeyBifVersion::V1),
        "E1" => Ok(KeyBifVersion::E1),
        _ => Err(format!("unsupported key data version: {value}")),
    }
}

pub(crate) fn parse_erf_version(value: &str) -> Result<ErfVersion, String> {
    match value.to_ascii_uppercase().as_str() {
        "V1" => Ok(ErfVersion::V1),
        "E1" => Ok(ErfVersion::E1),
        _ => Err(format!("unsupported erf data version: {value}")),
    }
}

pub(crate) fn parse_algorithm(value: &str) -> Result<Algorithm, String> {
    match value.to_ascii_lowercase().as_str() {
        "none" => Ok(Algorithm::None),
        "zlib" => Ok(Algorithm::Zlib),
        "zstd" => Ok(Algorithm::Zstd),
        _ => Err(format!("unsupported compression algorithm: {value}")),
    }
}

pub(crate) fn exo_compression_from_algorithm(algorithm: Algorithm) -> ExoResFileCompressionType {
    match algorithm {
        Algorithm::None => ExoResFileCompressionType::None,
        _ => ExoResFileCompressionType::CompressedBuf,
    }
}

pub(crate) fn ensure_target_dir_ready(path: &Path, force: bool) -> Result<(), String> {
    if path.exists() {
        if !path.is_dir() {
            return Err(format!("target is not a directory: {}", path.display()));
        }
        if !force
            && fs::read_dir(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display()))?
                .next()
                .is_some()
        {
            return Err("target directory not empty; aborting for your own safety".to_string());
        }
    } else {
        fs::create_dir_all(path)
            .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn ensure_output_file_ready(path: &Path, force: bool) -> Result<(), String> {
    if path.exists() && !force {
        return Err(format!(
            "output file exists; use --force to overwrite: {}",
            path.display()
        ));
    }
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)
            .map_err(|error| format!("failed to create {}: {error}", parent.display()))?;
    }
    Ok(())
}

pub(crate) fn sorted_dir_entries(dir: &Path) -> Result<Vec<DirEntryInfo>, String> {
    let mut entries = fs::read_dir(dir)
        .map_err(|error| format!("failed to read {}: {error}", dir.display()))?
        .map(|entry| {
            let entry =
                entry.map_err(|error| format!("failed to read {}: {error}", dir.display()))?;
            let file_name = entry.file_name().to_string_lossy().into_owned();
            Ok(DirEntryInfo {
                file_name,
                path: entry.path(),
            })
        })
        .collect::<Result<Vec<_>, String>>()?;
    entries.sort_by(|lhs, rhs| {
        lhs.file_name
            .to_ascii_lowercase()
            .cmp(&rhs.file_name.to_ascii_lowercase())
    });
    Ok(entries)
}

pub(crate) fn should_skip_top_level_dir(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(OsStr::to_str),
        Some(".git") | Some(".svn")
    )
}

pub(crate) fn entry_is_dir(path: &Path, no_symlinks: bool) -> Result<bool, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to stat {}: {error}", path.display()))?;
    if meta.file_type().is_symlink() {
        if no_symlinks {
            return Ok(false);
        }
        return Ok(path.is_dir());
    }
    Ok(meta.is_dir())
}

pub(crate) fn entry_is_file(path: &Path, no_symlinks: bool) -> Result<bool, String> {
    let meta = fs::symlink_metadata(path)
        .map_err(|error| format!("failed to stat {}: {error}", path.display()))?;
    if meta.file_type().is_symlink() {
        if no_symlinks {
            return Ok(false);
        }
        return Ok(path.is_file());
    }
    Ok(meta.is_file())
}

pub(crate) fn collect_key_bif_entries(
    dir: &Path,
    no_symlinks: bool,
) -> Result<Vec<ResRef>, String> {
    let mut entries = Vec::new();
    for entry in sorted_dir_entries(dir)? {
        if !entry_is_file(&entry.path, no_symlinks)? {
            continue;
        }
        let file_name = entry.path.file_name().and_then(OsStr::to_str).unwrap_or("");
        let rr = new_resolved_res_ref_from_filename(file_name)
            .map_err(|error| format!("invalid source file {}: {error}", entry.path.display()))?;
        entries.push(rr.into());
    }
    entries.sort_by(|lhs: &ResRef, rhs: &ResRef| {
        lhs.resolve()
            .map(|resolved| resolved.to_file().to_ascii_uppercase())
            .cmp(
                &rhs.resolve()
                    .map(|resolved| resolved.to_file().to_ascii_uppercase()),
            )
    });
    Ok(entries)
}

pub(crate) fn infer_erf_type(path: &Path, explicit: Option<&str>) -> Result<String, String> {
    if let Some(value) = explicit {
        let mut type_name = value.to_ascii_uppercase();
        type_name.truncate(4);
        while type_name.len() < 4 {
            type_name.push(' ');
        }
        return Ok(type_name);
    }

    let ext = path
        .extension()
        .and_then(OsStr::to_str)
        .unwrap_or("")
        .to_ascii_uppercase();
    let inferred = match ext.as_str() {
        "" => "ERF".to_string(),
        "NWM" => "MOD".to_string(),
        other => other.chars().take(4).collect::<String>(),
    };

    let mut padded = inferred.trim().to_string();
    while padded.len() < 4 {
        padded.push(' ');
    }
    Ok(padded)
}

pub(crate) fn write_lines<I>(path: &Path, lines: I) -> Result<(), String>
where
    I: IntoIterator<Item = String>,
{
    let mut file = File::create(path)
        .map_err(|error| format!("failed to create {}: {error}", path.display()))?;
    for line in lines {
        writeln!(file, "{line}")
            .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    }
    Ok(())
}

pub(crate) fn write_stdout_line(message: &str) -> Result<(), String> {
    let mut stdout = io::stdout();
    writeln!(stdout, "{message}").map_err(|error| format!("failed to write stdout: {error}"))
}

pub(crate) fn normalize_key_bif_filename(filename: &str) -> String {
    filename.replace('\\', "/")
}

pub(crate) fn file_name_string(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(OsStr::to_str)
        .map(ToOwned::to_owned)
}

pub(crate) fn current_build_date() -> (u32, u32) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let days = i64::try_from(now.as_secs() / 86_400).unwrap_or(i64::MAX);
    let (year, month, day) = civil_from_days(days);
    let build_day = ordinal_day(year, month, day);
    (u32::try_from(year).unwrap_or(0), build_day)
}

fn civil_from_days(days_since_epoch: i64) -> (i32, u32, u32) {
    let z = days_since_epoch + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = mp + if mp < 10 { 3 } else { -9 };
    let year = y + if m <= 2 { 1 } else { 0 };
    (
        i32::try_from(year).unwrap_or(i32::MAX),
        u32::try_from(m).unwrap_or(0),
        u32::try_from(d).unwrap_or(0),
    )
}

fn ordinal_day(year: i32, month: u32, day: u32) -> u32 {
    const DAYS_BEFORE_MONTH: [u32; 12] = [0, 31, 59, 90, 120, 151, 181, 212, 243, 273, 304, 334];
    let leap = is_leap_year(year) && month > 2;
    let month_index = usize::try_from(month.saturating_sub(1)).unwrap_or(0);
    DAYS_BEFORE_MONTH.get(month_index).copied().unwrap_or(0) + day + u32::from(leap)
}

fn is_leap_year(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}
