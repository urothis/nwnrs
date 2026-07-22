use std::{
    ffi::OsStr,
    fs,
    io::{self, BufReader, Cursor},
    path::{Path, PathBuf},
    thread,
};

use nwnrs_nwpkg::{
    ErfPackMetadata, ResourcePackMetadata, copy_original_key_set, read_erf_pack_metadata,
    read_key_pack_metadata, read_resource_pack_metadata, should_copy_original_erf,
    should_copy_original_key, should_copy_original_resource,
};
use nwnrs_nwscript as nwscript;
use nwnrs_types::prelude::*;
use tracing::{debug, info, instrument, warn};

use crate::{
    args::{KeyPackCmd, PackCmd},
    compile::{
        CompileScriptOptions, CompileScriptOutcome, autodetected_install_resman,
        compile_generated_script, compile_script_file, compile_script_file_with_skip,
        parse_optimizations,
    },
    package::{PackageOptions, run_package},
    util::{
        Kind, current_build_date, detect_kind, ensure_output_file_ready, ensure_target_dir_ready,
        entry_is_dir, entry_is_file, exo_compression_from_algorithm, infer_erf_type,
        is_project_control_file, parse_algorithm, parse_erf_version, parse_key_version,
        should_skip_top_level_dir, sorted_dir_entries,
    },
};

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(path_count = cmd.paths.len(), force = cmd.force)
)]
pub(crate) fn run_pack(cmd: PackCmd) -> Result<(), String> {
    info!("packing input");
    if package_mode_args(&cmd)?.is_some() {
        return run_pack_install_package(cmd);
    }

    let (_input, output) = explicit_pack_paths(&cmd)?;
    match detect_kind(output) {
        Some(Kind::Key) => run_pack_key(cmd),
        Some(Kind::Erf) => run_pack_erf(cmd),
        Some(Kind::Ncs) => run_pack_resource(cmd, Kind::Ncs),
        Some(Kind::Gff) => run_pack_resource(cmd, Kind::Gff),
        Some(Kind::TwoDa) => run_pack_resource(cmd, Kind::TwoDa),
        Some(Kind::Tlk) => run_pack_resource(cmd, Kind::Tlk),
        Some(Kind::Ssf) => run_pack_resource(cmd, Kind::Ssf),
        Some(Kind::Model) => run_pack_resource(cmd, Kind::Model),
        Some(Kind::Texture) => run_pack_resource(cmd, Kind::Texture),
        None => Err(format!(
            "unsupported output file type for generic pack: {}",
            output.display()
        )),
    }
}

fn pack_usage_error(cmd: &PackCmd) -> String {
    if cmd.paths.is_empty() {
        "pack requires INPUT and OUTPUT for explicit packing, or KEY_NAME OUTPUT_DIR for \
         install-backed packaging"
            .to_string()
    } else {
        "pack requires INPUT OUTPUT for explicit packing, or KEY_NAME OUTPUT_DIR for \
         install-backed packaging"
            .to_string()
    }
}

fn explicit_pack_paths(cmd: &PackCmd) -> Result<(&PathBuf, &PathBuf), String> {
    match cmd.paths.as_slice() {
        [input, output] => Ok((input, output)),
        _ => Err(pack_usage_error(cmd)),
    }
}

fn package_mode_args(cmd: &PackCmd) -> Result<Option<(String, PathBuf)>, String> {
    let package_flags = cmd.root.is_some() || cmd.user.is_some() || cmd.language.is_some();

    match cmd.paths.as_slice() {
        [key_path, output_dir] => {
            let key_name = key_path
                .file_name()
                .and_then(OsStr::to_str)
                .filter(|name| name.to_ascii_lowercase().ends_with(".key"))
                .map(str::to_string);
            let output_kind = detect_kind(output_dir);

            if let Some(key_name) = key_name {
                if output_kind.is_none() {
                    return Ok(Some((key_name, output_dir.clone())));
                }
                if package_flags {
                    return Err(
                        "install-backed packaging requires KEY_NAME OUTPUT_DIR, where KEY_NAME \
                         ends in .key and OUTPUT_DIR is a directory path"
                            .to_string(),
                    );
                }
            }

            if package_flags {
                Err(
                    "install-backed packaging requires KEY_NAME OUTPUT_DIR, where KEY_NAME ends \
                     in .key and OUTPUT_DIR is a directory path"
                        .to_string(),
                )
            } else {
                Ok(None)
            }
        }
        _ if package_flags => Err(
            "install-backed packaging requires KEY_NAME OUTPUT_DIR, where KEY_NAME ends in .key \
             and OUTPUT_DIR is a directory path"
                .to_string(),
        ),
        _ => Ok(None),
    }
}

fn run_pack_install_package(cmd: PackCmd) -> Result<(), String> {
    let (key_name, output_dir) = package_mode_args(&cmd)?.ok_or_else(|| pack_usage_error(&cmd))?;

    run_package(PackageOptions {
        directory:        output_dir,
        key:              key_name,
        root:             cmd.root,
        userdirectory:    cmd.user,
        language:         cmd.language.unwrap_or_else(|| "english".to_string()),
        data_version:     cmd.data_version,
        data_compression: cmd.data_compression,
        force:            cmd.force,
    })
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(force = cmd.force)
)]
fn run_pack_key(cmd: PackCmd) -> Result<(), String> {
    let (input, output) = explicit_pack_paths(&cmd)?;
    let input = input.clone();
    let output = output.clone();
    if let Some(metadata) = read_key_pack_metadata(&input)?
        && should_copy_original_key(&metadata, &input)?
    {
        copy_original_key_set(&metadata, &output, cmd.force)?;
        return Ok(());
    }
    let key_name = output
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid key output name: {}", output.display()))?
        .to_string();
    let destination = output
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map_or_else(|| PathBuf::from("."), Path::to_path_buf);
    run_key_pack(KeyPackCmd {
        data_version: cmd.data_version,
        data_compression: cmd.data_compression,
        no_squash: cmd.no_squash,
        no_symlinks: cmd.no_symlinks,
        force: cmd.force,
        debug: cmd.debug,
        no_entrypoint_check: cmd.no_entrypoint_check,
        langspec: cmd.langspec,
        include_dir: cmd.include_dir,
        optimization: cmd.optimization,
        optimization_flag: cmd.optimization_flag,
        jobs: cmd.jobs,
        key: key_name,
        source: input,
        destination,
    })
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(force = cmd.force)
)]
fn run_pack_erf(cmd: PackCmd) -> Result<(), String> {
    let (input, output) = explicit_pack_paths(&cmd)?;
    let input = input.clone();
    let output = output.clone();
    let dispatcher = nwnrs_nwpkg::generate_event_dispatcher(&input)?;
    let metadata = read_erf_pack_metadata(&input)?;
    if let Some(metadata) = metadata.as_ref()
        && dispatcher.is_none()
        && should_copy_original_erf(metadata, &input)?
    {
        ensure_output_file_ready(&output, cmd.force)?;
        fs::copy(&metadata.source, &output).map_err(|error| {
            format!(
                "failed to copy original archive {} to {}: {error}",
                metadata.source.display(),
                output.display()
            )
        })?;
        return Ok(());
    }
    let version = metadata
        .as_ref()
        .map_or(parse_erf_version(&cmd.data_version)?, |meta| {
            meta.file_version
        });
    let compalg = parse_algorithm(&cmd.data_compression)?;
    let exocomp = exo_compression_from_algorithm(compalg);
    let file_type = metadata.as_ref().map_or_else(
        || infer_erf_type(output.as_path(), cmd.erf_type.as_deref()),
        |meta| meta.file_type.clone(),
    );
    let compile_config = ScriptCompileConfig::from_pack_cmd(&cmd, &input)?;
    let sources = collect_generic_pack_sources(&input, true, cmd.no_symlinks)?;
    let sources = add_generated_event_dispatcher(&input, sources, &compile_config, dispatcher)?;
    let sources = apply_erf_entry_order(metadata.as_ref(), sources);
    let sources = normalize_pack_sources(sources)?;
    let sources = compile_pack_sources(sources, &compile_config)?;
    debug!(entry_count = sources.len(), "resolved archive pack sources");

    let refs = sources
        .iter()
        .map(|entry| entry.rr.clone())
        .collect::<Vec<_>>();
    let (build_year, build_day) = metadata.as_ref().map_or_else(current_build_date, |meta| {
        (
            meta.build_year.cast_unsigned(),
            meta.build_day.cast_unsigned(),
        )
    });
    let loc_strings = metadata
        .as_ref()
        .map(|meta| meta.loc_strings.clone())
        .unwrap_or_default();
    let str_ref = metadata.as_ref().map_or(0, |meta| meta.str_ref);
    let oid = metadata.as_ref().and_then(|meta| meta.oid.as_deref());
    let entry_algorithms = metadata
        .as_ref()
        .map(|meta| meta.entry_algorithms.clone())
        .unwrap_or_default();
    let mut out = Cursor::new(Vec::new());
    let resource_list_padding = metadata
        .as_ref()
        .map_or(0, |meta| meta.resource_list_padding);
    erf::write_erf_with_options(
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
        erf::ErfWriteOptions {
            resource_list_padding,
        },
        |rr, io| {
            let entry = sources
                .iter()
                .find(|entry| entry.rr == *rr)
                .ok_or_else(|| io::Error::other(format!("no source mapping for {rr}")))?;
            let data = entry.read_bytes().map_err(io::Error::other)?;
            io.write_all(&data)?;
            Ok((data.len(), checksums::sha1_digest(&data)))
        },
        |rr| entry_algorithms.get(rr).copied().unwrap_or(compalg),
    )
    .map_err(|error| format!("failed to pack {}: {error}", output.display()))?;

    ensure_output_file_ready(&output, cmd.force)?;
    fs::write(&output, out.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
    Ok(())
}

fn add_generated_event_dispatcher(
    input: &Path,
    mut sources: Vec<PackSourceEntry>,
    compile_config: &ScriptCompileConfig,
    dispatcher: Option<nwnrs_nwpkg::GeneratedEventDispatcher>,
) -> Result<Vec<PackSourceEntry>, String> {
    let Some(dispatcher) = dispatcher else {
        return Ok(sources);
    };
    let options = pack_compile_options(
        compile_config,
        vec![dispatcher.include_root.clone()],
        compile_config.debug,
    )?;
    let artifacts = compile_generated_script(
        &dispatcher.name,
        dispatcher.source.as_bytes(),
        std::slice::from_ref(&dispatcher.include_root),
        &options,
    )?;
    let rr = resman::ResolvedResRef::from_filename(&format!("{}.ncs", dispatcher.name))
        .map_err(|error| format!("invalid generated dispatcher name: {error}"))?;
    sources.push(PackSourceEntry {
        rr:     rr.into(),
        source: PackSourceKind::CompiledScript {
            path: input.join(format!("{}.nss", dispatcher.name)),
            ncs:  artifacts.ncs,
            ndb:  artifacts.ndb,
        },
    });
    Ok(sources)
}

#[instrument(
    level = "info",
    skip_all,
    err,
    fields(force = cmd.force)
)]
fn run_pack_resource(cmd: PackCmd, kind: Kind) -> Result<(), String> {
    let (input, output) = explicit_pack_paths(&cmd)?;
    let input = input.clone();
    let output = output.clone();
    if let Some(metadata) = read_resource_pack_metadata(&input)?
        && should_copy_original_resource(&metadata, &input)?
    {
        ensure_output_file_ready(&output, cmd.force)?;
        fs::copy(&metadata.source, &output).map_err(|error| {
            format!(
                "failed to copy original resource {} to {}: {error}",
                metadata.source.display(),
                output.display()
            )
        })?;
        return Ok(());
    }

    let source = resolve_resource_pack_source(&input, kind, read_resource_pack_metadata(&input)?)?;
    match kind {
        Kind::Gff => pack_gff_resource(&source, &output, cmd.force),
        Kind::TwoDa => pack_twoda_resource(&source, &output, cmd.force),
        Kind::Tlk => pack_tlk_resource(&source, &output, cmd.force),
        Kind::Ssf => pack_ssf_resource(&source, &output, cmd.force),
        Kind::Model => pack_model_resource(&source, &output, cmd.force),
        Kind::Texture => pack_texture_resource(&source, &output, cmd.force),
        Kind::Ncs => pack_ncs_resource(&source, &output, &cmd),
        Kind::Erf | Kind::Key => Err(format!(
            "unsupported standalone resource pack kind for {}",
            output.display()
        )),
    }
}

fn resolve_resource_pack_source(
    input: &Path,
    expected_kind: Kind,
    metadata: Option<ResourcePackMetadata>,
) -> Result<PathBuf, String> {
    if input.is_file() {
        if !path_matches_pack_kind(input, expected_kind) {
            return Err(format!(
                "input file type does not match output resource kind: {}",
                input.display()
            ));
        }
        return Ok(input.to_path_buf());
    }
    if !input.is_dir() {
        return Err(format!("source does not exist: {}", input.display()));
    }

    if let Some(metadata) = metadata {
        let candidate = input.join(&metadata.file_name);
        if candidate.is_file() {
            if !path_matches_pack_kind(&candidate, expected_kind) {
                return Err(format!(
                    "resource metadata file type does not match output resource kind: {}",
                    candidate.display()
                ));
            }
            return Ok(candidate);
        }
    }

    let mut files = sorted_dir_entries(input)?
        .into_iter()
        .filter(|entry| entry_is_file(&entry.path, false).unwrap_or(false))
        .filter(|entry| !is_project_control_file(&entry.path))
        .collect::<Vec<_>>();
    if files.len() != 1 {
        return Err(format!(
            "resource directory must contain exactly one packable file: {}",
            input.display()
        ));
    }
    let source = files.remove(0).path;
    if !path_matches_pack_kind(&source, expected_kind) {
        return Err(format!(
            "resource directory file type does not match output resource kind: {}",
            source.display()
        ));
    }
    Ok(source)
}

fn path_matches_pack_kind(path: &Path, expected_kind: Kind) -> bool {
    detect_kind(path) == Some(expected_kind)
        || (expected_kind == Kind::Ncs && (is_ncs_asm_file(path) || is_nwscript_source_file(path)))
}

fn pack_gff_resource(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let file = fs::File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut reader = BufReader::new(file);
    let root = gff::read_gff_root(&mut reader)
        .map_err(|error| format!("failed to parse {} as GFF: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Cursor::new(Vec::new());
    gff::write_gff_root(&mut bytes, &root)
        .map_err(|error| format!("failed to write {} as GFF: {error}", output.display()))?;
    fs::write(output, bytes.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
}

fn pack_twoda_resource(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let file = fs::File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut reader = BufReader::new(file);
    let value = twoda::read_twoda(&mut reader)
        .map_err(|error| format!("failed to parse {} as 2DA: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Vec::new();
    twoda::write_twoda(&mut bytes, &value, false)
        .map_err(|error| format!("failed to write {} as 2DA: {error}", output.display()))?;
    fs::write(output, bytes)
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
}

fn pack_ncs_resource(input: &Path, output: &Path, cmd: &PackCmd) -> Result<(), String> {
    let bytes = if is_ncs_asm_file(input) {
        let text = fs::read_to_string(input)
            .map_err(|error| format!("failed to read {}: {error}", input.display()))?;
        nwscript::assemble_ncs_bytes(&text, None)
            .map_err(|error| format!("failed to assemble {}: {error}", input.display()))?
    } else if is_nwscript_source_file(input) {
        let options = pack_compile_options(
            &ScriptCompileConfig::from_pack_cmd(cmd, input)?,
            Vec::new(),
            cmd.debug,
        )?;
        let artifacts = compile_script_file(input, &options)?;
        ensure_output_file_ready(output, cmd.force)?;
        fs::write(output, &artifacts.ncs)
            .map_err(|error| format!("failed to write {}: {error}", output.display()))?;
        if cmd.debug {
            let debug_output = output.with_extension("ndb");
            ensure_output_file_ready(&debug_output, cmd.force)?;
            let ndb = artifacts
                .ndb
                .ok_or_else(|| "compiler did not produce NDB output".to_string())?;
            fs::write(&debug_output, ndb)
                .map_err(|error| format!("failed to write {}: {error}", debug_output.display()))?;
        } else {
            let debug_output = output.with_extension("ndb");
            if debug_output.is_file() {
                fs::remove_file(&debug_output).map_err(|error| {
                    format!(
                        "failed to remove stale debugger output {}: {error}",
                        debug_output.display()
                    )
                })?;
            }
        }
        return Ok(());
    } else {
        fs::read(input).map_err(|error| format!("failed to read {}: {error}", input.display()))?
    };
    ensure_output_file_ready(output, cmd.force)?;
    fs::write(output, bytes)
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
}

fn pack_tlk_resource(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let file = fs::File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut value = tlk::read_single_tlk(
        BufReader::new(file),
        nwnrs_types::resman::CachePolicy::Bypass,
    )
    .map_err(|error| format!("failed to parse {} as TLK: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Cursor::new(Vec::new());
    tlk::write_single_tlk(&mut bytes, &mut value)
        .map_err(|error| format!("failed to write {} as TLK: {error}", output.display()))?;
    fs::write(output, bytes.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
}

fn pack_ssf_resource(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let file = fs::File::open(input)
        .map_err(|error| format!("failed to open {}: {error}", input.display()))?;
    let mut reader = BufReader::new(file);
    let value = ssf::read_ssf(&mut reader)
        .map_err(|error| format!("failed to parse {} as SSF: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Vec::new();
    ssf::write_ssf(&mut bytes, &value)
        .map_err(|error| format!("failed to write {} as SSF: {error}", output.display()))?;
    fs::write(output, bytes)
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
}

fn pack_model_resource(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let value = mdl::Model::from_file(input)
        .map_err(|error| format!("failed to parse {} as MDL: {error}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Cursor::new(Vec::new());
    mdl::write_model(&mut bytes, &value)
        .map_err(|error| format!("failed to write {} as MDL: {error}", output.display()))?;
    fs::write(output, bytes.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
}

fn pack_texture_resource(input: &Path, output: &Path, force: bool) -> Result<(), String> {
    let extension = input
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("failed to infer texture format from {}", input.display()))?;
    ensure_output_file_ready(output, force)?;
    let mut bytes = Cursor::new(Vec::new());
    match extension.as_str() {
        "tga" => {
            let value = tga::TgaTexture::from_file(input)
                .map_err(|error| format!("failed to parse {} as TGA: {error}", input.display()))?;
            tga::write_tga(&mut bytes, &value)
                .map_err(|error| format!("failed to write {} as TGA: {error}", output.display()))?;
        }
        "dds" => {
            let value = dds::DdsTexture::from_file(input)
                .map_err(|error| format!("failed to parse {} as DDS: {error}", input.display()))?;
            dds::write_dds(&mut bytes, &value)
                .map_err(|error| format!("failed to write {} as DDS: {error}", output.display()))?;
        }
        "plt" => {
            let value = plt::PltTexture::from_file(input)
                .map_err(|error| format!("failed to parse {} as PLT: {error}", input.display()))?;
            plt::write_plt(&mut bytes, &value)
                .map_err(|error| format!("failed to write {} as PLT: {error}", output.display()))?;
        }
        _ => {
            return Err(format!(
                "unsupported texture format for {}",
                input.display()
            ));
        }
    }
    fs::write(output, bytes.into_inner())
        .map_err(|error| format!("failed to write {}: {error}", output.display()))
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

    for source in &mut sources {
        if let Some(original) = metadata
            .entry_order
            .iter()
            .find(|original| **original == source.rr)
        {
            source.rr = original.clone();
        }
    }

    let order = metadata
        .entry_order
        .iter()
        .enumerate()
        .map(|(index, rr)| (rr.clone(), index))
        .collect::<std::collections::HashMap<resman::ResRef, usize>>();
    sources.sort_by(|left, right| {
        let left_index = order.get(&left.rr).copied().unwrap_or(usize::MAX);
        let right_index = order.get(&right.rr).copied().unwrap_or(usize::MAX);
        left_index
            .cmp(&right_index)
            .then_with(|| left.rr.cmp(&right.rr))
    });
    sources
}

pub(crate) struct KeyPackageBif {
    pub(crate) directory: String,
    pub(crate) name:      String,
    pub(crate) entries:   Vec<resman::ResRef>,
}

pub(crate) fn write_key_package<F>(
    destination: &Path,
    force: bool,
    key_name: &str,
    bif_prefix: &str,
    bifs: &[KeyPackageBif],
    data_version: &str,
    data_compression: &str,
    mut resolve_bytes: F,
) -> Result<(), String>
where
    F: FnMut(&resman::ResRef) -> Result<Vec<u8>, String>,
{
    ensure_target_dir_ready(destination, force)?;

    let version = parse_key_version(data_version)?;
    let compalg = parse_algorithm(data_compression)?;
    let exocomp = exo_compression_from_algorithm(compalg);
    let key_name = Path::new(key_name)
        .file_stem()
        .and_then(OsStr::to_str)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| format!("invalid key name: {key_name}"))?
        .to_string();
    let (build_year, build_day) = current_build_date();
    let key_build_year = build_year.saturating_sub(1900);
    let key_bifs = bifs
        .iter()
        .map(|bif| key::KeyBifEntry {
            directory:         bif.directory.clone(),
            name:              bif.name.clone(),
            recorded_filename: None,
            drives:            0,
            bif_oid:           None,
            entries:           bif.entries.clone(),
        })
        .collect::<Vec<_>>();

    key::write_key_and_bif(
        version,
        exocomp,
        compalg,
        destination,
        &key_name,
        bif_prefix,
        &key_bifs,
        key_build_year,
        build_day,
        None,
        |rr, io| {
            let data = resolve_bytes(rr).map_err(io::Error::other)?;
            io.write_all(&data)?;
            Ok((data.len(), checksums::sha1_digest(&data)))
        },
    )
    .map_err(|error| format!("failed to pack key data: {error}"))?;

    Ok(())
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
    let bif_prefix = "data";
    let compile_config = ScriptCompileConfig::from_key_pack_cmd(&cmd)?;

    let mut bifs = Vec::new();
    let mut source_entries = std::collections::HashMap::<resman::ResRef, PackSourceEntry>::new();
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
        let mut entries = Vec::new();
        for entry in sorted_dir_entries(&cmd.source.join(&relative))? {
            if !entry_is_file(&entry.path, cmd.no_symlinks)? {
                continue;
            }
            if is_project_control_file(&entry.path) {
                continue;
            }
            match pack_source_for_file(&entry.path, &cmd.source) {
                Ok(source) => entries.push(source),
                Err(error) => {
                    warn!(error = %error, path = %entry.path.display(), "skipping invalid key pack file");
                }
            }
        }
        let entries = normalize_pack_sources(entries)?;
        let entries = compile_pack_sources(entries, &compile_config)?;
        let refs = entries
            .iter()
            .map(|entry| entry.rr.clone())
            .collect::<Vec<_>>();
        for entry in entries {
            source_entries.insert(entry.rr.clone(), entry);
        }
        bifs.push(KeyPackageBif {
            directory: if cmd.no_squash {
                bif_prefix.to_string()
            } else {
                String::new()
            },
            name,
            entries: refs,
        });
    }

    write_key_package(
        &cmd.destination,
        cmd.force,
        &cmd.key,
        bif_prefix,
        &bifs,
        &cmd.data_version,
        &cmd.data_compression,
        |rr| {
            let entry = source_entries
                .get(rr)
                .ok_or_else(|| format!("no source mapping for {rr}"))?;
            entry.read_bytes()
        },
    )
}

#[derive(Clone)]
pub(crate) enum PackSourceKind {
    File(PathBuf),
    PendingScript {
        path:         PathBuf,
        include_dirs: Vec<PathBuf>,
    },
    CompiledScript {
        path: PathBuf,
        ncs:  Vec<u8>,
        ndb:  Option<Vec<u8>>,
    },
    GeneratedBytes {
        path:  PathBuf,
        bytes: Vec<u8>,
    },
    AssembledNcs {
        path:  PathBuf,
        bytes: Vec<u8>,
    },
}

#[derive(Clone)]
pub(crate) struct PackSourceEntry {
    pub(crate) rr:     resman::ResRef,
    pub(crate) source: PackSourceKind,
}

impl PackSourceEntry {
    fn source_label(&self) -> String {
        match &self.source {
            PackSourceKind::File(path)
            | PackSourceKind::PendingScript {
                path, ..
            }
            | PackSourceKind::CompiledScript {
                path, ..
            }
            | PackSourceKind::GeneratedBytes {
                path, ..
            }
            | PackSourceKind::AssembledNcs {
                path, ..
            } => path.display().to_string(),
        }
    }

    fn read_bytes(&self) -> Result<Vec<u8>, String> {
        match &self.source {
            PackSourceKind::File(path) => fs::read(path)
                .map_err(|error| format!("failed to read {}: {error}", path.display())),
            PackSourceKind::PendingScript {
                path, ..
            } => Err(format!(
                "script source {} was not compiled before pack write",
                path.display()
            )),
            PackSourceKind::CompiledScript {
                ncs, ..
            } => Ok(ncs.clone()),
            PackSourceKind::GeneratedBytes {
                bytes, ..
            }
            | PackSourceKind::AssembledNcs {
                bytes, ..
            } => Ok(bytes.clone()),
        }
    }

    fn prefers_over(&self, other: &Self) -> bool {
        matches!(
            self.source,
            PackSourceKind::PendingScript { .. }
                | PackSourceKind::CompiledScript { .. }
                | PackSourceKind::GeneratedBytes { .. }
                | PackSourceKind::AssembledNcs { .. }
        ) && matches!(other.source, PackSourceKind::File(_))
    }
}

pub(crate) fn collect_generic_pack_sources(
    path: &Path,
    explicit: bool,
    no_symlinks: bool,
) -> Result<Vec<PackSourceEntry>, String> {
    let mut out = Vec::new();
    let pack_root = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf()
    };
    collect_generic_pack_entry(path, &pack_root, explicit, no_symlinks, &mut out)?;
    Ok(out)
}

fn collect_generic_pack_entry(
    path: &Path,
    pack_root: &Path,
    explicit: bool,
    no_symlinks: bool,
    out: &mut Vec<PackSourceEntry>,
) -> Result<(), String> {
    if entry_is_file(path, no_symlinks)? {
        if is_project_control_file(path) {
            return Ok(());
        }
        match pack_source_for_file(path, pack_root) {
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
                collect_generic_pack_entry(&entry.path, pack_root, false, no_symlinks, out)?;
            } else if entry_is_file(&entry.path, no_symlinks)? {
                if is_project_control_file(&entry.path) {
                    continue;
                }
                match pack_source_for_file(&entry.path, pack_root) {
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

fn pack_source_for_file(path: &Path, pack_root: &Path) -> Result<PackSourceEntry, String> {
    if is_project_control_file(path) {
        return Err(format!(
            "{} is project control data, not a resource",
            path.display()
        ));
    }
    if is_nwscript_langspec_file(path) {
        return Err(format!(
            "{} is a NWScript langspec file, not a packable resource",
            path.display()
        ));
    }

    if path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("nss"))
    {
        let stem = path
            .file_stem()
            .and_then(OsStr::to_str)
            .filter(|value| !value.is_empty())
            .ok_or_else(|| format!("invalid script source filename: {}", path.display()))?;
        let resolved = resman::ResolvedResRef::from_filename(&format!("{stem}.ncs"))
            .map_err(|error| format!("{} is not a valid script source: {error}", path.display()))?;
        let include_dirs = pack_script_search_roots(path, pack_root);
        return Ok(PackSourceEntry {
            rr:     resolved.into(),
            source: PackSourceKind::PendingScript {
                path: path.to_path_buf(),
                include_dirs,
            },
        });
    }

    if let Some(resolved) = resolved_ncs_asm_resref(path) {
        let text = fs::read_to_string(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let bytes = nwscript::assemble_ncs_bytes(&text, None)
            .map_err(|error| format!("failed to assemble {}: {error}", path.display()))?;
        return Ok(PackSourceEntry {
            rr:     resolved.into(),
            source: PackSourceKind::AssembledNcs {
                path: path.to_path_buf(),
                bytes,
            },
        });
    }

    if path
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("json"))
        && let Some(source_name) = path.file_stem().and_then(OsStr::to_str)
        && detect_kind(Path::new(source_name)) == Some(Kind::Gff)
    {
        let resolved = resman::ResolvedResRef::from_filename(source_name).map_err(|error| {
            format!("{} is not a valid GFF JSON source: {error}", path.display())
        })?;
        let source = fs::read(path)
            .map_err(|error| format!("failed to read {}: {error}", path.display()))?;
        let root = gff::gff_root_from_json_bytes(&source)
            .map_err(|error| format!("failed to parse {} as GFF JSON: {error}", path.display()))?;
        let mut output = Cursor::new(Vec::new());
        gff::write_gff_root(&mut output, &root)
            .map_err(|error| format!("failed to encode {} as GFF: {error}", path.display()))?;
        return Ok(PackSourceEntry {
            rr:     resolved.into(),
            source: PackSourceKind::GeneratedBytes {
                path:  path.to_path_buf(),
                bytes: output.into_inner(),
            },
        });
    }

    let file_name = path.file_name().and_then(OsStr::to_str).unwrap_or("");
    let resolved = resman::ResolvedResRef::from_filename(file_name)
        .map_err(|error| format!("{} is not a valid resref source: {error}", path.display()))?;
    Ok(PackSourceEntry {
        rr:     resolved.into(),
        source: PackSourceKind::File(path.to_path_buf()),
    })
}

fn is_nwscript_source_file(path: &Path) -> bool {
    path.extension()
        .and_then(OsStr::to_str)
        .is_some_and(|value| value.eq_ignore_ascii_case("nss"))
        && !is_nwscript_langspec_file(path)
}

#[derive(Clone)]
struct ScriptCompileConfig {
    debug:               bool,
    no_entrypoint_check: bool,
    langspec:            Option<PathBuf>,
    include_dirs:        Vec<PathBuf>,
    optimization:        String,
    optimization_flags:  Vec<String>,
    jobs:                Option<usize>,
}

impl ScriptCompileConfig {
    fn from_pack_cmd(value: &PackCmd, input: &Path) -> Result<Self, String> {
        let mut config = Self {
            debug:               value.debug,
            no_entrypoint_check: value.no_entrypoint_check,
            langspec:            value.langspec.clone(),
            include_dirs:        value.include_dir.clone(),
            optimization:        value.optimization.clone(),
            optimization_flags:  value.optimization_flag.clone(),
            jobs:                value.jobs,
        };
        config.add_project_dependencies(input)?;
        Ok(config)
    }

    fn from_key_pack_cmd(value: &KeyPackCmd) -> Result<Self, String> {
        let mut config = Self {
            debug:               value.debug,
            no_entrypoint_check: value.no_entrypoint_check,
            langspec:            value.langspec.clone(),
            include_dirs:        value.include_dir.clone(),
            optimization:        value.optimization.clone(),
            optimization_flags:  value.optimization_flag.clone(),
            jobs:                value.jobs,
        };
        config.add_project_dependencies(&value.source)?;
        Ok(config)
    }

    fn add_project_dependencies(&mut self, input: &Path) -> Result<(), String> {
        for dependency in nwnrs_nwpkg::resolve_include_dependencies(input)? {
            if !self.include_dirs.contains(&dependency.source_root) {
                self.include_dirs.push(dependency.source_root);
            }
        }
        Ok(())
    }
}

fn pack_compile_options(
    config: &ScriptCompileConfig,
    mut include_dirs: Vec<PathBuf>,
    debug: bool,
) -> Result<CompileScriptOptions, String> {
    for dir in &config.include_dirs {
        if !include_dirs.iter().any(|existing| existing == dir) {
            include_dirs.push(dir.clone());
        }
    }
    Ok(CompileScriptOptions {
        debug,
        no_entrypoint_check: config.no_entrypoint_check,
        langspec: config.langspec.clone(),
        include_dirs,
        optimizations: parse_optimizations(&config.optimization, &config.optimization_flags)?,
        max_include_depth: nwscript::DEFAULT_MAX_INCLUDE_DEPTH,
        install_resman: autodetected_install_resman(),
    })
}

fn default_pack_jobs(config: &ScriptCompileConfig) -> usize {
    config.jobs.unwrap_or_else(|| {
        thread::available_parallelism()
            .map(std::num::NonZeroUsize::get)
            .unwrap_or(1)
    })
}

fn compile_pack_sources(
    sources: Vec<PackSourceEntry>,
    config: &ScriptCompileConfig,
) -> Result<Vec<PackSourceEntry>, String> {
    let pending = sources
        .iter()
        .enumerate()
        .filter_map(|(index, entry)| match &entry.source {
            PackSourceKind::PendingScript {
                path,
                include_dirs,
            } => Some((index, path.clone(), include_dirs.clone())),
            _ => None,
        })
        .collect::<Vec<_>>();
    if pending.is_empty() {
        return Ok(sources);
    }

    let worker_count = pending.len().min(default_pack_jobs(config).max(1));
    let chunk_size = pending.len().div_ceil(worker_count);
    let mut compiled = std::collections::HashMap::<usize, nwscript::CompileArtifacts>::new();
    let mut skipped = std::collections::HashSet::<usize>::new();

    thread::scope(|scope| -> Result<(), String> {
        let mut handles = Vec::new();
        for chunk in pending.chunks(chunk_size) {
            let jobs = chunk.to_vec();
            let config = config.clone();
            handles.push(scope.spawn(
                move || -> Result<Vec<(usize, Option<nwscript::CompileArtifacts>)>, String> {
                    let mut results = Vec::with_capacity(jobs.len());
                    for (index, path, include_dirs) in jobs {
                        let options = pack_compile_options(&config, include_dirs, config.debug)?;
                        let outcome = compile_script_file_with_skip(
                            &path,
                            &options,
                            !config.no_entrypoint_check,
                        )?;
                        match outcome {
                            CompileScriptOutcome::Compiled(artifacts) => {
                                results.push((index, Some(artifacts)));
                            }
                            CompileScriptOutcome::SkippedNoEntrypoint => {
                                results.push((index, None));
                            }
                        }
                    }
                    Ok(results)
                },
            ));
        }

        for handle in handles {
            let entries = handle
                .join()
                .map_err(|_panic| "parallel NWScript pack worker panicked".to_string())??;
            for (index, artifacts) in entries {
                if let Some(artifacts) = artifacts {
                    compiled.insert(index, artifacts);
                } else {
                    skipped.insert(index);
                }
            }
        }
        Ok(())
    })?;

    let mut resolved = Vec::with_capacity(sources.len());
    for (index, mut entry) in sources.into_iter().enumerate() {
        if skipped.contains(&index) {
            continue;
        }
        if let Some(artifacts) = compiled.remove(&index) {
            let PackSourceKind::PendingScript {
                path, ..
            } = entry.source
            else {
                return Err("compiled script index no longer points at script source".to_string());
            };
            let ndb = artifacts.ndb.clone();
            entry.source = PackSourceKind::CompiledScript {
                path: path.clone(),
                ncs: artifacts.ncs,
                ndb,
            };
            resolved.push(entry.clone());
            if config.debug
                && let Some(ndb) = match &entry.source {
                    PackSourceKind::CompiledScript {
                        ndb, ..
                    } => ndb.clone(),
                    _ => None,
                }
            {
                let ndb_rr =
                    resman::ResRef::new(entry.rr.res_ref().to_string(), resman::ResType(2064))
                        .map_err(|error| {
                            format!("failed to create NDB resref for {}: {error}", entry.rr)
                        })?;
                resolved.push(PackSourceEntry {
                    rr:     ndb_rr,
                    source: PackSourceKind::GeneratedBytes {
                        path:  path.with_extension("ndb"),
                        bytes: ndb,
                    },
                });
            }
            continue;
        }
        if matches!(entry.source, PackSourceKind::PendingScript { .. }) {
            return Err("pending script source was neither compiled nor skipped".to_string());
        }
        resolved.push(entry);
    }
    Ok(resolved)
}

fn is_ncs_asm_file(path: &Path) -> bool {
    resolved_ncs_asm_resref(path).is_some()
}

fn resolved_ncs_asm_resref(path: &Path) -> Option<resman::ResolvedResRef> {
    let file_name = path.file_name()?.to_str()?;
    let suffix = ".ncs.asm";
    if file_name.len() <= suffix.len() || !file_name.to_ascii_lowercase().ends_with(suffix) {
        return None;
    }
    let base = &file_name[..file_name.len() - ".asm".len()];
    resman::ResolvedResRef::from_filename(base).ok()
}

fn is_nwscript_langspec_file(path: &Path) -> bool {
    let Some(file_name) = path.file_name().and_then(OsStr::to_str) else {
        return false;
    };
    file_name.eq_ignore_ascii_case("nwscript.nss")
        || file_name.eq_ignore_ascii_case(nwscript::DEFAULT_LANGSPEC_SCRIPT_NAME)
}

fn pack_script_search_roots(path: &Path, pack_root: &Path) -> Vec<PathBuf> {
    let mut roots = Vec::new();
    let mut current = path.parent();
    while let Some(dir) = current {
        if !dir.as_os_str().is_empty() && !roots.iter().any(|root| root == dir) {
            roots.push(dir.to_path_buf());
        }
        if dir == pack_root {
            break;
        }
        current = dir.parent();
    }
    if !pack_root.as_os_str().is_empty() && !roots.iter().any(|root| root == pack_root) {
        roots.push(pack_root.to_path_buf());
    }
    roots
}

fn normalize_pack_sources(sources: Vec<PackSourceEntry>) -> Result<Vec<PackSourceEntry>, String> {
    let mut normalized = Vec::new();

    for source in sources {
        if let Some(index) = normalized
            .iter()
            .position(|existing: &PackSourceEntry| existing.rr == source.rr)
        {
            let existing = normalized
                .get(index)
                .ok_or_else(|| "duplicate source index out of bounds".to_string())?;
            if source.prefers_over(existing) {
                let slot = normalized
                    .get_mut(index)
                    .ok_or_else(|| "duplicate source index out of bounds".to_string())?;
                *slot = source;
                continue;
            }
            if existing.prefers_over(&source) {
                continue;
            }
            return Err(format!(
                "duplicate resref {} from {} and {}",
                existing.rr,
                existing.source_label(),
                source.source_label()
            ));
        }

        normalized.push(source);
    }

    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_types::{prelude::resman::ResContainer, resman::CachePolicy};

    use super::*;
    use crate::args::PackCmd;

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-{prefix}-{nanos}"))
    }

    fn base_pack_cmd(paths: Vec<PathBuf>) -> PackCmd {
        PackCmd {
            force: false,
            debug: false,
            no_entrypoint_check: false,
            langspec: None,
            include_dir: Vec::new(),
            optimization: crate::compile::DEFAULT_OPTIMIZATION.to_string(),
            optimization_flag: Vec::new(),
            jobs: None,
            data_version: "V1".to_string(),
            data_compression: "none".to_string(),
            no_squash: false,
            no_symlinks: false,
            erf_type: None,
            root: None,
            user: None,
            language: None,
            paths,
        }
    }

    #[test]
    fn erf_metadata_restores_original_resource_casing() -> Result<(), String> {
        let lower = resman::ResolvedResRef::from_filename("repute.fac")
            .map_err(|error| error.to_string())?;
        let original = resman::ResolvedResRef::from_filename_preserving_case("Repute.fac")
            .map_err(|error| error.to_string())?;
        let metadata = ErfPackMetadata {
            source:                PathBuf::new(),
            source_sha256:         String::new(),
            file_type:             "MOD ".to_string(),
            file_version:          erf::ErfVersion::V1,
            build_year:            0,
            build_day:             0,
            str_ref:               -1,
            loc_strings:           std::collections::BTreeMap::new(),
            oid:                   None,
            resource_list_padding: 0,
            entry_order:           vec![original.into()],
            entry_algorithms:      std::collections::BTreeMap::new(),
            file_sha256s:          std::collections::BTreeMap::new(),
        };
        let sources = apply_erf_entry_order(
            Some(&metadata),
            vec![PackSourceEntry {
                rr:     lower.into(),
                source: PackSourceKind::GeneratedBytes {
                    path:  PathBuf::from("repute.fac.json"),
                    bytes: Vec::new(),
                },
            }],
        );

        let source = sources.first().expect("ordered source");
        assert_eq!(source.rr.res_ref(), "Repute");
        Ok(())
    }

    #[test]
    fn pack_supports_binary_gff_resource() {
        let root = gff::GffRoot::new("UTC ");
        let temp_dir = unique_test_dir("gff-pack");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("fixture.utc");
        let output = temp_dir.join("copy.utc");
        let mut bytes = Cursor::new(Vec::new());
        gff::write_gff_root(&mut bytes, &root).expect("write gff fixture");
        fs::write(&input, bytes.into_inner()).expect("write input fixture");

        run_pack(base_pack_cmd(vec![input.clone(), output.clone()])).expect("pack gff resource");

        assert_eq!(
            fs::read(&input).expect("read input"),
            fs::read(&output).expect("read output")
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_supports_binary_twoda_resource() {
        let temp_dir = unique_test_dir("twoda-pack");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("appearance.2da");
        let output = temp_dir.join("copy.2da");
        fs::write(&input, b"2DA V2.0\nDEFAULT: ****\n\nLABEL\n0 value\n")
            .expect("write twoda fixture");

        run_pack(base_pack_cmd(vec![input.clone(), output.clone()])).expect("pack twoda resource");

        assert_eq!(
            fs::read(&input).expect("read input"),
            fs::read(&output).expect("read output")
        );
        let _ = fs::remove_dir_all(temp_dir);
    }

    fn minimal_langspec() -> &'static str {
        r#"
#define ENGINE_NUM_STRUCTURES 0

int TRUE = 1;
int FALSE = 0;
"#
    }

    fn event_langspec() -> &'static str {
        r#"
#define ENGINE_NUM_STRUCTURES 1
#define ENGINE_STRUCTURE_0 json

int TRUE = 1;
int FALSE = 0;
void NWNXCall(string sNamespace, string sFunction);
void NWNXPushString(string sValue);
string NWNXPopString();
json JsonParse(string sJson);
json JsonObjectGet(json jObject, string sKey);
string JsonGetString(json jValue);
"#
    }

    #[test]
    fn pack_compiles_nwscript_sources_into_erf_entries() {
        let temp_dir = unique_test_dir("erf-pack-nwscript");
        let input = temp_dir.join("src");
        let output = temp_dir.join("test.mod");
        let scripts = input.join("nss");
        fs::create_dir_all(&scripts).expect("create script dir");
        fs::write(input.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(scripts.join("helper.nss"), "int helper() { return TRUE; }")
            .expect("write include");
        fs::write(
            scripts.join("test.nss"),
            "#include \"helper\"\nint StartingConditional() { return helper(); }",
        )
        .expect("write script");
        fs::write(scripts.join("test.ncs"), b"stale").expect("write stale ncs");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        cmd.debug = true;
        run_pack(cmd).expect("pack mod with scripts");

        let archive = erf::read_erf_from_file(&output).expect("read packed archive");
        let compiled_res = archive
            .demand(&resman::ResRef::new("test", resman::ResType(2010)).expect("build ncs rr"))
            .expect("read compiled script resource");
        let compiled = compiled_res
            .read_all(CachePolicy::Bypass)
            .expect("read compiled script bytes");
        assert_ne!(compiled, b"stale".to_vec());
        assert!(
            nwscript::decode_ncs_instructions(&compiled).is_ok(),
            "compiled bytes should decode as NCS"
        );
        let debug_res = archive
            .demand(&resman::ResRef::new("test", resman::ResType(2064)).expect("build ndb rr"))
            .expect("read debug script resource");
        let debug = debug_res
            .read_all(CachePolicy::Bypass)
            .expect("read debug script bytes");
        assert!(
            std::str::from_utf8(&debug)
                .ok()
                .and_then(|text| nwscript::parse_ndb_str(text).ok())
                .is_some(),
            "packed debug bytes should decode as NDB"
        );
        assert!(
            archive
                .demand(
                    &resman::ResRef::new("helper", resman::ResType(2010))
                        .expect("build helper ncs rr"),
                )
                .is_err(),
            "include-only helper should not be packed as standalone NCS"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_resolves_local_nwpkg_include_dependency() {
        let temp_dir = unique_test_dir("erf-pack-nwpkg-include");
        let input = temp_dir.join("module");
        let include = temp_dir.join("include");
        let output = temp_dir.join("test.mod");
        fs::create_dir_all(&input).expect("create module dir");
        fs::create_dir_all(&include).expect("create include dir");
        fs::write(
            input.join("nwpkg.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \
             \".\"\n\n[dependencies]\nfixture = { path = \"../include\" }\n",
        )
        .expect("write module manifest");
        fs::write(
            include.join("nwpkg.toml"),
            "[project]\nname = \"fixture\"\nkind = \"include\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write include manifest");
        fs::write(input.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(
            include.join("fixture.nss"),
            "int FixtureValue() { return TRUE; }\n",
        )
        .expect("write dependency include");
        fs::write(
            input.join("main.nss"),
            "#include \"fixture\"\nvoid main() { int nValue = FixtureValue(); }\n",
        )
        .expect("write module script");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        run_pack(cmd).expect("pack module with local include dependency");

        let archive = erf::read_erf_from_file(&output).expect("read packed archive");
        let compiled = archive
            .demand(&resman::ResRef::new("main", resman::ResType(2010)).expect("build ncs rr"))
            .expect("read compiled script")
            .read_all(CachePolicy::Bypass)
            .expect("read compiled bytes");
        assert!(nwscript::decode_ncs_instructions(&compiled).is_ok());
        assert!(
            archive
                .demand(
                    &resman::ResRef::new("fixture", resman::ResType(2009)).expect("build nss rr")
                )
                .is_err(),
            "dependency source should not be packed into the module"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_bakes_nss_generated_event_dispatcher() {
        let temp_dir = unique_test_dir("erf-pack-event-macro");
        let input = temp_dir.join("module");
        let output = temp_dir.join("test.mod");
        fs::create_dir_all(&input).expect("create module dir");
        fs::write(
            input.join("nwpkg.toml"),
            "[project]\nname = \"fixture\"\nkind = \"mod\"\n\n[source]\npath = \".\"\n",
        )
        .expect("write module manifest");
        fs::write(input.join("nwscript.nss"), event_langspec()).expect("write langspec");
        fs::write(
            input.join("startup.nss"),
            "#[nwnrs::events(module_load)]\nvoid ProjectStart(json jEvent) \
             {}\n#[nwnrs::events(associate_add_before)]\nvoid BeforeAssociateAdded(json jEvent) \
             {}\n#[nwnrs::events(object_broadcast_safe_projectile_after)]\nvoid \
             AfterProjectile(json jEvent) {}\n#[nwnrs::events(skill_use_before)]\nvoid \
             BeforeSkill(json jEvent) {}\n#[nwnrs::events(item_decrement_stack_size_after)]\nvoid \
             AfterStackChange(json jEvent) {}\n",
        )
        .expect("write event source");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        run_pack(cmd).expect("pack module with NSS event macro");

        let archive = erf::read_erf_from_file(&output).expect("read packed archive");
        let compiled = archive
            .demand(
                &resman::ResRef::new("_nwnrs_onload", resman::ResType(2010))
                    .expect("build dispatcher ncs rr"),
            )
            .expect("read generated dispatcher")
            .read_all(CachePolicy::Bypass)
            .expect("read dispatcher bytes");
        assert!(nwscript::decode_ncs_instructions(&compiled).is_ok());

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_walks_nested_archive_directories_without_depth_limit() {
        let temp_dir = unique_test_dir("erf-pack-deep-nesting");
        let input = temp_dir.join("src");
        let output = temp_dir.join("test.mod");
        let deep = input.join("a").join("b").join("c").join("d");
        fs::create_dir_all(&deep).expect("create nested script dir");
        fs::write(input.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(
            deep.join("deep_script.nss"),
            "int StartingConditional() { return TRUE; }",
        )
        .expect("write nested script");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        run_pack(cmd).expect("pack deeply nested mod");

        let archive = erf::read_erf_from_file(&output).expect("read packed archive");
        let compiled_res = archive
            .demand(
                &resman::ResRef::new("deep_script", resman::ResType(2010))
                    .expect("build nested ncs rr"),
            )
            .expect("read nested compiled script resource");
        let compiled = compiled_res
            .read_all(CachePolicy::Bypass)
            .expect("read nested compiled script bytes");
        assert!(
            nwscript::decode_ncs_instructions(&compiled).is_ok(),
            "nested compiled bytes should decode as NCS"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_compiles_nwscript_sources_into_key_bif_entries() {
        let temp_dir = unique_test_dir("key-pack-nwscript");
        let input = temp_dir.join("src");
        let output = temp_dir.join("scripts.key");
        let bif_dir = input.join("scripts");
        fs::create_dir_all(&bif_dir).expect("create bif source dir");
        fs::write(input.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(
            bif_dir.join("hello.nss"),
            "void main() { int value = TRUE; }",
        )
        .expect("write script");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        run_pack(cmd).expect("pack key with scripts");

        let key = key::read_key_table_from_file(&output).expect("read packed key");
        let compiled_res = key
            .demand(&resman::ResRef::new("hello", resman::ResType(2010)).expect("build ncs rr"))
            .expect("read compiled script resource");
        let compiled = compiled_res
            .read_all(CachePolicy::Bypass)
            .expect("read compiled script bytes");
        assert!(
            nwscript::decode_ncs_instructions(&compiled).is_ok(),
            "compiled bytes should decode as NCS"
        );

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_assembles_ncs_asm_entries_into_archives() {
        let temp_dir = unique_test_dir("erf-pack-ncs-asm");
        let input = temp_dir.join("src");
        let output = temp_dir.join("test.mod");
        let ncs_dir = input.join("ncs");
        fs::create_dir_all(&ncs_dir).expect("create ncs dir");
        let original = nwscript::encode_ncs_instructions(&[
            nwscript::NcsInstruction {
                opcode:  nwscript::NcsOpcode::Constant,
                auxcode: nwscript::NcsAuxCode::TypeInteger,
                extra:   7_i32.to_be_bytes().to_vec(),
            },
            nwscript::NcsInstruction {
                opcode:  nwscript::NcsOpcode::Ret,
                auxcode: nwscript::NcsAuxCode::None,
                extra:   Vec::new(),
            },
        ]);
        let asm = nwscript::render_ncs_disassembly(
            &original,
            None,
            nwscript::NcsDisassemblyOptions {
                max_string_length: usize::MAX,
                ..nwscript::NcsDisassemblyOptions::default()
            },
        )
        .expect("render ncs asm");
        fs::write(ncs_dir.join("hello.ncs.asm"), asm).expect("write ncs asm");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        run_pack(cmd).expect("pack mod with ncs asm");

        let archive = erf::read_erf_from_file(&output).expect("read packed archive");
        let compiled = archive
            .demand(&resman::ResRef::new("hello", resman::ResType(2010)).expect("build ncs rr"))
            .expect("read assembled ncs resource")
            .read_all(CachePolicy::Bypass)
            .expect("read assembled ncs bytes");
        assert_eq!(compiled, original);

        let _ = fs::remove_dir_all(temp_dir);
    }

    #[test]
    fn pack_compiles_single_nwscript_source_to_ncs_and_ndb() {
        let temp_dir = unique_test_dir("nss-pack-standalone");
        fs::create_dir_all(&temp_dir).expect("create temp dir");
        let input = temp_dir.join("test.nss");
        let output = temp_dir.join("test.ncs");
        let debug = temp_dir.join("test.ndb");
        fs::write(temp_dir.join("nwscript.nss"), minimal_langspec()).expect("write langspec");
        fs::write(&input, "int StartingConditional() { return TRUE; }").expect("write input");

        let mut cmd = base_pack_cmd(vec![input.clone(), output.clone()]);
        cmd.force = true;
        cmd.debug = true;
        run_pack(cmd).expect("pack standalone script");

        assert!(output.is_file(), "NCS output should exist");
        assert!(debug.is_file(), "NDB output should exist");
        let compiled = fs::read(&output).expect("read compiled ncs");
        assert!(nwscript::decode_ncs_instructions(&compiled).is_ok());

        let mut cmd = base_pack_cmd(vec![input, output]);
        cmd.force = true;
        run_pack(cmd).expect("repack standalone script without debug output");
        assert!(!debug.exists(), "stale NDB output should be removed");

        let _ = fs::remove_dir_all(temp_dir);
    }
}
