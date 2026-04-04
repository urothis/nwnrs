use std::{
    collections::HashSet,
    ffi::OsStr,
    fs::{self, File},
    io::{self, BufReader, Cursor},
    path::{Path, PathBuf},
};

use nwnrs::prelude::*;
use tracing::{debug, info, instrument, warn};

use crate::{
    args::{KeyPackCmd, PackCmd},
    metadata::{
        ErfPackMetadata, copy_original_key_set, read_erf_pack_metadata, read_key_pack_metadata,
        should_copy_original_erf, should_copy_original_key,
    },
    util::{
        Kind, RESOURCE_METADATA_FILENAME, collect_key_bif_entries, current_build_date, detect_kind,
        ensure_output_file_ready, ensure_target_dir_ready, entry_is_dir, entry_is_file,
        exo_compression_from_algorithm, infer_erf_type, is_gff_extension, parse_algorithm,
        parse_erf_version, parse_key_version, should_skip_top_level_dir, sorted_dir_entries,
    },
};

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %cmd.input.display(), output = %cmd.output.display(), force = cmd.force)
)]
pub(crate) fn run_pack(cmd: PackCmd) -> Result<(), String> {
    info!("packing input");
    match detect_kind(&cmd.output) {
        Some(Kind::Key) => run_pack_key(cmd),
        Some(Kind::Erf) => run_pack_erf(cmd),
        Some(Kind::Gff) => pack_gff_file(&cmd.input, &cmd.output, cmd.force),
        Some(Kind::TwoDa) => pack_twoda_file(&cmd.input, &cmd.output, cmd.force),
        Some(Kind::Tlk) => Err(format!(
            "generic pack does not yet support TLK import: {}",
            cmd.output.display()
        )),
        Some(Kind::Ssf) => Err(format!(
            "generic pack does not yet support SSF import: {}",
            cmd.output.display()
        )),
        None => Err(format!(
            "unsupported output file type for generic pack: {}",
            cmd.output.display()
        )),
    }
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(output = %cmd.output.display(), force = cmd.force)
)]
fn run_pack_key(cmd: PackCmd) -> Result<(), String> {
    if let Some(metadata) = read_key_pack_metadata(&cmd.input)?
        && should_copy_original_key(&metadata, &cmd.input)?
    {
        copy_original_key_set(&metadata, &cmd.output, cmd.force)?;
        return Ok(());
    }
    let key_name = cmd
        .output
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid key output name: {}", cmd.output.display()))?
        .to_string();
    let destination = cmd
        .output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    run_key_pack(KeyPackCmd {
        data_version: cmd.data_version,
        data_compression: cmd.data_compression,
        no_squash: cmd.no_squash,
        no_symlinks: cmd.no_symlinks,
        force: cmd.force,
        key: key_name,
        source: cmd.input,
        destination,
    })
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(output = %cmd.output.display(), force = cmd.force)
)]
fn run_pack_erf(cmd: PackCmd) -> Result<(), String> {
    let metadata = read_erf_pack_metadata(&cmd.input)?;
    if let Some(metadata) = metadata.as_ref()
        && should_copy_original_erf(metadata, &cmd.input)?
    {
        ensure_output_file_ready(&cmd.output, cmd.force)?;
        fs::copy(&metadata.source, &cmd.output).map_err(|error| {
            format!(
                "failed to copy original archive {} to {}: {error}",
                metadata.source.display(),
                cmd.output.display()
            )
        })?;
        return Ok(());
    }
    let version = metadata
        .as_ref()
        .map(|meta| meta.file_version)
        .unwrap_or(parse_erf_version(&cmd.data_version)?);
    let compalg = parse_algorithm(&cmd.data_compression)?;
    let exocomp = exo_compression_from_algorithm(compalg);
    let file_type = metadata
        .as_ref()
        .map(|meta| meta.file_type.clone())
        .unwrap_or(infer_erf_type(
            cmd.output.as_path(),
            cmd.erf_type.as_deref(),
        )?);
    let sources = collect_generic_pack_sources(&cmd.input, true, 1, cmd.recurse, cmd.no_symlinks)?;
    let sources = apply_erf_entry_order(metadata.as_ref(), sources);

    let mut seen = HashSet::new();
    for entry in &sources {
        if !seen.insert(entry.rr.clone()) {
            return Err(format!("duplicate resref {}", entry.source_label()));
        }
    }
    debug!(entry_count = sources.len(), "resolved archive pack sources");

    let refs = sources
        .iter()
        .map(|entry| entry.rr.clone())
        .collect::<Vec<_>>();
    let (build_year, build_day) = metadata
        .as_ref()
        .map(|meta| (meta.build_year as u32, meta.build_day as u32))
        .unwrap_or_else(current_build_date);
    let loc_strings = metadata
        .as_ref()
        .map(|meta| meta.loc_strings.clone())
        .unwrap_or_default();
    let str_ref = metadata.as_ref().map(|meta| meta.str_ref).unwrap_or(0);
    let oid = metadata.as_ref().and_then(|meta| meta.oid.as_deref());
    let mut out = Cursor::new(Vec::new());
    erf::write_erf(
        &mut out,
        &file_type,
        version,
        build_year,
        build_day,
        exocomp,
        compalg,
        &loc_strings,
        str_ref,
        &refs,
        oid,
        |rr, io| {
            let entry = sources
                .iter()
                .find(|entry| entry.rr == *rr)
                .ok_or_else(|| io::Error::other(format!("no source mapping for {rr}")))?;
            let data = entry.read_bytes().map_err(io::Error::other)?;
            io.write_all(&data)?;
            Ok((data.len(), checksums::secure_hash(&data)))
        },
    )
    .map_err(|error| format!("failed to pack {}: {error}", cmd.output.display()))?;

    ensure_output_file_ready(&cmd.output, cmd.force)?;
    fs::write(&cmd.output, out.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", cmd.output.display()))?;
    Ok(())
}

pub(crate) fn apply_erf_entry_order(
    metadata: Option<&ErfPackMetadata>,
    mut sources: Vec<PackSourceEntry>,
) -> Vec<PackSourceEntry> {
    let Some(metadata) = metadata else {
        return sources;
    };
    if metadata.entry_order.is_empty() {
        return sources;
    }

    let order = metadata
        .entry_order
        .iter()
        .enumerate()
        .map(|(index, rr)| (rr.clone(), index))
        .collect::<std::collections::HashMap<resref::ResRef, usize>>();
    sources.sort_by(|left, right| {
        let left_index = order.get(&left.rr).copied().unwrap_or(usize::MAX);
        let right_index = order.get(&right.rr).copied().unwrap_or(usize::MAX);
        left_index
            .cmp(&right_index)
            .then_with(|| left.rr.cmp(&right.rr))
    });
    sources
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(
        input = %cmd.source.display(),
        output = %cmd.destination.display(),
        key_name = %cmd.key,
        force = cmd.force
    )
)]
fn run_key_pack(cmd: KeyPackCmd) -> Result<(), String> {
    info!(source = %cmd.source.display(), destination = %cmd.destination.display(), key = %cmd.key, "packing key set");
    if !cmd.source.is_dir() {
        return Err(format!(
            "source does not contain any data: {}",
            cmd.source.display()
        ));
    }

    ensure_target_dir_ready(&cmd.destination, cmd.force)?;

    let version = parse_key_version(&cmd.data_version)?;
    let compalg = parse_algorithm(&cmd.data_compression)?;
    let exocomp = exo_compression_from_algorithm(compalg);
    let key_name = Path::new(&cmd.key)
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid key name: {}", cmd.key))?
        .to_string();
    let bif_prefix = "data";

    let mut bifs = Vec::new();
    let mut source_paths = std::collections::HashMap::<resref::ResRef, PathBuf>::new();
    for dir in sorted_dir_entries(&cmd.source)? {
        if should_skip_top_level_dir(&dir.path) {
            continue;
        }
        if !entry_is_dir(&dir.path, cmd.no_symlinks)? {
            continue;
        }

        let relative = dir.file_name;
        let name = Path::new(&relative)
            .file_stem()
            .and_then(OsStr::to_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("invalid source directory name: {relative}"))?
            .to_string();
        let entries = collect_key_bif_entries(&cmd.source.join(&relative), cmd.no_symlinks)?;
        for rr in &entries {
            let resolved = rr
                .resolve()
                .ok_or_else(|| format!("cannot resolve source file for {rr}"))?;
            source_paths.insert(
                rr.clone(),
                cmd.source.join(&relative).join(resolved.to_file()),
            );
        }
        bifs.push(key::KeyBifEntry {
            directory: if cmd.no_squash {
                bif_prefix.to_string()
            } else {
                String::new()
            },
            name,
            entries,
        });
    }

    let (build_year, build_day) = current_build_date();
    key::write_key_and_bif(
        version,
        exocomp,
        compalg,
        &cmd.destination,
        &key_name,
        bif_prefix,
        &bifs,
        build_year,
        build_day,
        None,
        |rr, io| {
            let full_path = source_paths
                .get(rr)
                .ok_or_else(|| io::Error::other(format!("no source mapping for {rr}")))?;
            let data = fs::read(full_path)?;
            io.write_all(&data)?;
            Ok((data.len(), checksums::secure_hash(&data)))
        },
    )
    .map_err(|error| format!("failed to pack key data: {error}"))?;

    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %input.display(), output = %output.display(), force)
)]
fn pack_gff_file(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let json = fs::read_to_string(input)
        .map_err(|error| format!("failed to read {}: {error}", input.display()))?;
    let root = gffjson::gff_root_from_json_str(&json)
        .map_err(|error| format!("failed to parse {} as GFF JSON: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Cursor::new(Vec::new());
    gff::write_gff_root(&mut bytes, &root)
        .map_err(|error| format!("failed to serialize {} as GFF: {error}", input.display()))?;
    fs::write(output, bytes.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    Ok(())
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(input = %input.display(), output = %output.display(), force)
)]
fn pack_twoda_file(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let file = File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut reader = BufReader::new(file);
    let twoda = twoda::read_twoda(&mut reader)
        .map_err(|error| format!("failed to parse {} as 2DA text: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Vec::new();
    twoda::write_twoda(&mut bytes, &twoda, false)
        .map_err(|error| format!("failed to serialize {} as 2DA: {error}", input.display()))?;
    fs::write(output, bytes)
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    Ok(())
}

#[derive(Clone)]
pub(crate) enum PackSourceKind {
    File(PathBuf),
    GffJson(PathBuf),
}

#[derive(Clone)]
pub(crate) struct PackSourceEntry {
    pub(crate) rr:     resref::ResRef,
    pub(crate) source: PackSourceKind,
}

impl PackSourceEntry {
    fn source_label(&self) -> String {
        match &self.source {
            PackSourceKind::File(path) | PackSourceKind::GffJson(path) => {
                path.display().to_string()
            }
        }
    }

    fn read_bytes(&self) -> Result<Vec<u8>, String> {
        match &self.source {
            PackSourceKind::File(path) => fs::read(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display())),
            PackSourceKind::GffJson(path) => {
                let json = fs::read_to_string(path)
                    .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
                let root = gffjson::gff_root_from_json_str(&json).map_err(|error| {
                    format!("failed to parse {} as GFF JSON: {error}", path.display())
                })?;
                let mut bytes = Cursor::new(Vec::new());
                gff::write_gff_root(&mut bytes, &root).map_err(|error| {
                    format!("failed to serialize {} as GFF: {error}", path.display())
                })?;
                Ok(bytes.into_inner())
            }
        }
    }
}

pub(crate) fn collect_generic_pack_sources(
    path: &Path,
    explicit: bool,
    recurse_level: usize,
    max_recurse_level: usize,
    no_symlinks: bool,
) -> Result<Vec<PackSourceEntry>, String> {
    let mut out = Vec::new();
    collect_generic_pack_entry(
        path,
        explicit,
        recurse_level,
        max_recurse_level,
        no_symlinks,
        &mut out,
    )?;
    Ok(out)
}

fn collect_generic_pack_entry(
    path: &Path,
    explicit: bool,
    recurse_level: usize,
    max_recurse_level: usize,
    no_symlinks: bool,
    out: &mut Vec<PackSourceEntry>,
) -> Result<(), String> {
    if recurse_level > max_recurse_level {
        return Ok(());
    }

    if entry_is_file(path, no_symlinks)? {
        if is_pack_metadata_file(path) {
            return Ok(());
        }
        match pack_source_for_file(path) {
            Ok(source) => out.push(source),
            Err(error) => {
                if explicit {
                    return Err(format!(
                        "invalid explicit entry {}: {error}",
                        path.display()
                    ));
                }
                warn!(error = %error, path = %path.display(), "skipping invalid directory entry during pack");
            }
        }
        return Ok(());
    }

    if entry_is_dir(path, no_symlinks)? {
        for entry in sorted_dir_entries(path)? {
            if should_skip_top_level_dir(&entry.path) {
                continue;
            }
            if entry_is_dir(&entry.path, no_symlinks)? {
                collect_generic_pack_entry(
                    &entry.path,
                    false,
                    recurse_level + 1,
                    max_recurse_level,
                    no_symlinks,
                    out,
                )?;
            } else if entry_is_file(&entry.path, no_symlinks)? {
                if is_pack_metadata_file(&entry.path) {
                    continue;
                }
                match pack_source_for_file(&entry.path) {
                    Ok(source) => out.push(source),
                    Err(error) => {
                        warn!(error = %error, path = %entry.path.display(), "skipping invalid file during pack");
                    }
                }
            }
        }
        return Ok(());
    }

    Err(format!("no idea what to do about: {}", path.display()))
}

fn pack_source_for_file(path: &Path) -> Result<PackSourceEntry, String> {
    if is_pack_metadata_file(path) {
        return Err(format!(
            "{} is pack metadata, not a resource",
            path.display()
        ));
    }
    let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
    if let Some(base_name) = file_name.strip_suffix(".json") {
        let resolved = resref::new_resolved_res_ref_from_filename(base_name).map_err(|error| {
            format!("{} is not a valid GFF JSON source: {error}", path.display())
        })?;
        if !is_gff_extension(resolved.res_ext()) {
            return Err(format!(
                "{} is JSON for unsupported pack type {}",
                path.display(),
                resolved.res_ext()
            ));
        }
        return Ok(PackSourceEntry {
            rr:     resolved.into(),
            source: PackSourceKind::GffJson(path.to_path_buf()),
        });
    }

    let resolved = resref::new_resolved_res_ref_from_filename(file_name)
        .map_err(|error| format!("{} is not a valid resref source: {error}", path.display()))?;
    Ok(PackSourceEntry {
        rr:     resolved.into(),
        source: PackSourceKind::File(path.to_path_buf()),
    })
}

fn is_pack_metadata_file(path: &Path) -> bool {
    path.file_name().and_then(OsStr::to_str) == Some(RESOURCE_METADATA_FILENAME)
}
