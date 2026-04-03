use crate::args::{KeyUnpackCmd, UnpackCmd};
use crate::metadata::write_erf_pack_metadata;
use crate::metadata::write_key_pack_metadata;
use crate::metadata::write_resource_metadata;
use crate::util::{
    Kind, detect_kind, ensure_output_file_ready, ensure_target_dir_ready, is_gff_extension,
    unpacked_raw_target, write_lines,
};
use nwn_erf::prelude::*;
use nwn_gff::prelude::*;
use nwn_gffjson::prelude::*;
use nwn_key::prelude::*;
use nwn_resman::prelude::*;
use nwn_resref::prelude::*;
use nwn_twoda::prelude::*;
use std::ffi::OsStr;
use std::fs::{self, File};
use std::io::{BufReader, Cursor};
use std::path::Path;
use tracing::{debug, info, instrument, warn};

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %cmd.input.display(), output = %cmd.directory.display(), force = cmd.force)
)]
pub(crate) fn run_unpack(cmd: UnpackCmd) -> Result<(), String> {
    info!("unpacking input");
    if cmd.directory.exists() {
        if !cmd.directory.is_dir() {
            return Err(format!(
                "destination is not a directory: {}",
                cmd.directory.display()
            ));
        }
    } else {
        fs::create_dir_all(&cmd.directory)
            .map_err(|error| format!("failed to create {}: {error}", cmd.directory.display()))?;
    }

    match detect_kind(&cmd.input) {
        Some(Kind::Erf) => unpack_erf_to_dir(&cmd.input, &cmd.directory, cmd.force),
        Some(Kind::Key) => run_key_unpack(KeyUnpackCmd {
            force: cmd.force,
            key: cmd.input,
            destination: cmd.directory,
        }),
        Some(Kind::Gff) => unpack_gff_to_json(&cmd.input, &cmd.directory, cmd.force),
        Some(Kind::TwoDa) => unpack_twoda_to_text(&cmd.input, &cmd.directory, cmd.force),
        Some(Kind::Tlk) => Err(format!(
            "generic unpack does not yet support TLK export: {}",
            cmd.input.display()
        )),
        Some(Kind::Ssf) => Err(format!(
            "generic unpack does not yet support SSF export: {}",
            cmd.input.display()
        )),
        None => Err(format!(
            "unsupported file type for generic unpack: {}",
            cmd.input.display()
        )),
    }
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %cmd.key.display(), output = %cmd.destination.display(), force = cmd.force)
)]
pub(crate) fn run_key_unpack(cmd: KeyUnpackCmd) -> Result<(), String> {
    info!("unpacking key set");
    ensure_target_dir_ready(&cmd.destination, cmd.force)?;
    let key = read_key_table_from_file(&cmd.key)
        .map_err(|error| format!("failed to parse {} as KEY: {error}", cmd.key.display()))?;
    fs::create_dir_all(&cmd.destination)
        .map_err(|error| format!("failed to create {}: {error}", cmd.destination.display()))?;

    write_lines(
        &cmd.destination.join("key_order.txt"),
        key.contents().into_iter().map(|rr| rr.to_string()),
    )?;
    write_lines(
        &cmd.destination.join("bif_order.txt"),
        key.bifs()
            .into_iter()
            .map(|bif| crate::util::file_name_string(&bif).unwrap_or(bif)),
    )?;

    for KeyBifContents {
        filename,
        resources,
    } in key.bif_contents().map_err(|error| {
        format!(
            "failed to load bif contents for {}: {error}",
            cmd.key.display()
        )
    })? {
        let bif_basename = crate::util::file_name_string(&filename)
            .ok_or_else(|| format!("invalid bif filename in key: {filename}"))?;
        let target_dir = cmd.destination.join(&bif_basename);
        fs::create_dir_all(&target_dir)
            .map_err(|error| format!("failed to create {}: {error}", target_dir.display()))?;
        write_lines(
            &cmd.destination.join(format!("{bif_basename}_order.txt")),
            resources.iter().map(ToString::to_string),
        )?;

        for rr in resources {
            let data = key
                .demand(&rr)
                .and_then(|res| res.read_all(false))
                .map_err(|error| {
                    format!("failed to extract {rr} from {}: {error}", cmd.key.display())
                })?;
            let resolved = rr
                .resolve()
                .ok_or_else(|| format!("cannot resolve resource filename for {rr}"))?;
            fs::write(target_dir.join(resolved.to_file()), data)
                .map_err(|error| format!("failed to write extracted {rr}: {error}"))?;
        }
    }

    write_key_pack_metadata(&cmd.destination, &cmd.key, &key, cmd.force)?;
    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %input.display(), output = %destination.display(), force)
)]
pub(crate) fn unpack_erf_to_dir(
    input: &Path,
    destination: &Path,
    force: bool,
) -> Result<(), String> {
    let erf = read_erf_from_file(input)
        .map_err(|error| format!("failed to parse {} as ERF/MOD: {error}", input.display()))?;
    let mut extracted = 0_usize;
    for rr in erf.contents() {
        let data = match erf.demand(&rr).and_then(|res| res.read_all(false)) {
            Ok(data) => data,
            Err(error) => {
                warn!(resource = %rr, archive = %input.display(), error = %error, "failed to extract archive entry");
                continue;
            }
        };
        debug!(resource = %rr, size = data.len(), "extracted archive entry");
        write_unpacked_archive_entry(&rr, &data, destination, force)?;
        extracted += 1;
    }
    if extracted == 0 {
        return Err(format!(
            "failed to extract any readable entries from {}",
            input.display()
        ));
    }
    write_erf_pack_metadata(destination, input, &erf, force)?;
    Ok(())
}

#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(resref = %rr, output = %destination.display(), force)
)]
fn write_unpacked_archive_entry(
    rr: &ResRef,
    data: &[u8],
    destination: &Path,
    force: bool,
) -> Result<(), String> {
    let Some(resolved) = rr.resolve() else {
        let target = destination.join(rr.to_string());
        ensure_output_file_ready(&target, force)?;
        return fs::write(&target, data)
            .map_err(|error| format!("failed to write {}: {error}", target.display()));
    };

    if is_gff_extension(resolved.res_ext()) {
        match write_unpacked_gff_json(&resolved.to_file(), data, destination, force) {
            Ok(()) => return Ok(()),
            Err(error) => {
                warn!(resource = %rr, error = %error, "failed to convert GFF resource to JSON; writing raw bytes");
            }
        }
    }

    let target = unpacked_raw_target(destination, &resolved.to_file(), resolved.res_ext());
    ensure_output_file_ready(&target, force)?;
    fs::write(&target, data)
        .map_err(|error| format!("failed to write {}: {error}", target.display()))
}

#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(path = %file_name, output = %destination.display(), force)
)]
fn write_unpacked_gff_json(
    file_name: &str,
    data: &[u8],
    destination: &Path,
    force: bool,
) -> Result<(), String> {
    let mut reader = Cursor::new(data);
    let gff = read_gff_root(&mut reader)
        .map_err(|error| format!("failed to parse {file_name} as GFF: {error}"))?;
    let json = gff_root_to_pretty_json_string(&gff)
        .map_err(|error| format!("failed to convert {file_name} to JSON: {error}"))?;
    let extension = Path::new(file_name)
        .extension()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("missing file extension for {file_name}"))?;
    let type_dir = destination.join(extension);
    fs::create_dir_all(&type_dir)
        .map_err(|error| format!("failed to create {}: {error}", type_dir.display()))?;
    let target = type_dir.join(format!("{file_name}.json"));
    ensure_output_file_ready(&target, force)?;
    fs::write(&target, json)
        .map_err(|error| format!("failed to write {}: {error}", target.display()))
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %input.display(), output = %destination.display(), force)
)]
fn unpack_gff_to_json(input: &Path, destination: &Path, force: bool) -> Result<(), String> {
    info!(input = %input.display(), destination = %destination.display(), "unpacking standalone GFF to JSON");
    let file = File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut reader = BufReader::new(file);
    let gff = read_gff_root(&mut reader)
        .map_err(|error| format!("failed to parse {} as GFF: {error}", input.display()))?;
    let json = gff_root_to_pretty_json_string(&gff)
        .map_err(|error| format!("failed to convert {} to JSON: {error}", input.display()))?;
    let file_name = input
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("invalid input filename: {}", input.display()))?;
    let target = destination.join(format!("{file_name}.json"));
    ensure_output_file_ready(&target, force)?;
    fs::write(&target, json)
        .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
    write_resource_metadata(destination, input, "gff", force)?;
    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %input.display(), output = %destination.display(), force)
)]
fn unpack_twoda_to_text(input: &Path, destination: &Path, force: bool) -> Result<(), String> {
    info!(input = %input.display(), destination = %destination.display(), "unpacking standalone 2DA to text");
    let file = File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut reader = BufReader::new(file);
    let twoda = read_twoda(&mut reader)
        .map_err(|error| format!("failed to parse {} as 2DA: {error}", input.display()))?;
    let file_name = input
        .file_name()
        .and_then(OsStr::to_str)
        .ok_or_else(|| format!("invalid input filename: {}", input.display()))?;
    let target = destination.join(file_name);
    ensure_output_file_ready(&target, force)?;
    let mut output = Vec::new();
    write_twoda(&mut output, &twoda, false)
        .map_err(|error| format!("failed to serialize {} as 2DA: {error}", input.display()))?;
    fs::write(&target, output)
        .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
    write_resource_metadata(destination, input, "2da", force)?;
    Ok(())
}
