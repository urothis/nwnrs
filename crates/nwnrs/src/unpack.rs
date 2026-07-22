use std::{fs, io::Cursor, path::Path};

use nwnrs_nwpkg::{
    ProjectKind, write_erf_pack_metadata, write_key_pack_metadata, write_project_manifest,
    write_resource_pack_metadata,
};
use nwnrs_nwscript as nwscript;
use nwnrs_types::{
    prelude::{resman::ResContainer, *},
    resman::CachePolicy,
};
use tracing::{debug, info, instrument, warn};

use crate::{
    args::{KeyUnpackCmd, UnpackCmd},
    util::{
        Kind, detect_kind, ensure_output_file_ready, ensure_target_dir_ready, unpacked_raw_target,
        write_lines,
    },
};

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
            force:       cmd.force,
            key:         cmd.input,
            destination: cmd.directory,
        }),
        Some(Kind::Ncs) => unpack_ncs_to_dir(&cmd.input, &cmd.directory, cmd.force),
        Some(Kind::Gff) => unpack_resource_to_dir(&cmd.input, &cmd.directory, "gff", cmd.force),
        Some(Kind::TwoDa) => unpack_resource_to_dir(&cmd.input, &cmd.directory, "2da", cmd.force),
        Some(Kind::Tlk) => unpack_resource_to_dir(&cmd.input, &cmd.directory, "tlk", cmd.force),
        Some(Kind::Ssf) => unpack_resource_to_dir(&cmd.input, &cmd.directory, "ssf", cmd.force),
        Some(Kind::Model) => unpack_resource_to_dir(&cmd.input, &cmd.directory, "mdl", cmd.force),
        Some(Kind::Texture) => {
            unpack_resource_to_dir(&cmd.input, &cmd.directory, "texture", cmd.force)
        }
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
    let key = key::read_key_table_from_file(&cmd.key)
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

    for key::KeyBifContents {
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
                .and_then(|res| res.read_all(CachePolicy::Bypass))
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
    write_project_manifest(
        &cmd.destination,
        ProjectKind::Key,
        project_name_from_input(&cmd.key),
        ".",
        cmd.force,
    )?;
    Ok(())
}

fn unpack_ncs_to_dir(input: &Path, destination: &Path, force: bool) -> Result<(), String> {
    let file_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid input filename: {}", input.display()))?;
    let target = destination.join(format!("{file_name}.asm"));
    ensure_output_file_ready(&target, force)?;
    let bytes =
        fs::read(input).map_err(|error| format!("failed to read {}: {error}", input.display()))?;
    let rendered = nwscript::render_ncs_disassembly(
        &bytes,
        None,
        nwscript::NcsDisassemblyOptions {
            max_string_length: usize::MAX,
            ..nwscript::NcsDisassemblyOptions::default()
        },
    )
    .map_err(|error| format!("failed to disassemble {}: {error}", input.display()))?;
    fs::write(&target, rendered)
        .map_err(|error| format!("failed to write {}: {error}", target.display()))?;
    write_resource_pack_metadata(
        destination,
        input,
        "ncs",
        &format!("{file_name}.asm"),
        force,
    )?;
    write_project_manifest(
        destination,
        ProjectKind::Ncs,
        project_name_from_input(input),
        &format!("{file_name}.asm"),
        force,
    )?;
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
    let erf = erf::read_erf_from_file(input)
        .map_err(|error| format!("failed to parse {} as ERF/MOD: {error}", input.display()))?;
    let mut extracted = 0_usize;
    for rr in erf.contents() {
        let data = match erf
            .demand(&rr)
            .and_then(|res| res.read_all(CachePolicy::Bypass))
        {
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
    write_project_manifest(
        destination,
        file_extension_kind(input)?,
        project_name_from_input(input),
        ".",
        force,
    )?;
    Ok(())
}

fn unpack_resource_to_dir(
    input: &Path,
    destination: &Path,
    source_kind: &str,
    force: bool,
) -> Result<(), String> {
    let file_name = input
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| format!("invalid input filename: {}", input.display()))?;
    let target = destination.join(file_name);
    ensure_output_file_ready(&target, force)?;
    fs::copy(input, &target).map_err(|error| {
        format!(
            "failed to copy resource {} to {}: {error}",
            input.display(),
            target.display()
        )
    })?;
    write_resource_pack_metadata(destination, input, source_kind, file_name, force)?;
    write_project_manifest(
        destination,
        file_extension_kind(input)?,
        project_name_from_input(input),
        file_name,
        force,
    )?;
    Ok(())
}

fn file_extension_kind(path: &Path) -> Result<ProjectKind, String> {
    path.extension()
        .and_then(|value| value.to_str())
        .ok_or_else(|| format!("failed to infer file kind from {}", path.display()))?
        .parse()
}

fn project_name_from_input(path: &Path) -> &str {
    path.file_stem()
        .and_then(|value| value.to_str())
        .filter(|value| !value.is_empty())
        .unwrap_or("nwpkg")
}

#[instrument(
    level = "debug",
    skip_all,
    err,
    fields(resref = %rr, output = %destination.display(), force)
)]
fn write_unpacked_archive_entry(
    rr: &resman::ResRef,
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

    if resolved.res_ext().eq_ignore_ascii_case("ncs") {
        let target = unpacked_raw_target(
            destination,
            &format!("{}.asm", resolved.to_file()),
            resolved.res_ext(),
        );
        ensure_output_file_ready(&target, force)?;
        let rendered = nwscript::render_ncs_disassembly(
            data,
            None,
            nwscript::NcsDisassemblyOptions {
                max_string_length: usize::MAX,
                ..nwscript::NcsDisassemblyOptions::default()
            },
        )
        .map_err(|error| format!("failed to disassemble archive entry {rr}: {error}"))?;
        return fs::write(&target, rendered)
            .map_err(|error| format!("failed to write {}: {error}", target.display()));
    }

    if detect_kind(Path::new(&resolved.to_file())) == Some(Kind::Gff) {
        let target = destination.join(format!("{}.json", resolved.to_file()));
        ensure_output_file_ready(&target, force)?;
        let root = gff::read_gff_root(&mut Cursor::new(data))
            .map_err(|error| format!("failed to parse archive entry {rr} as GFF: {error}"))?;
        let rendered = gff::gff_root_to_json_bytes(&root)
            .map_err(|error| format!("failed to serialize archive entry {rr}: {error}"))?;
        return fs::write(&target, rendered)
            .map_err(|error| format!("failed to write {}: {error}", target.display()));
    }

    let target = unpacked_raw_target(destination, &resolved.to_file(), resolved.res_ext());
    ensure_output_file_ready(&target, force)?;
    fs::write(&target, data)
        .map_err(|error| format!("failed to write {}: {error}", target.display()))
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        io::Cursor,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use super::*;
    use crate::{
        args::{PackCmd, UnpackCmd},
        pack::run_pack,
    };

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock drift before unix epoch")
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-{prefix}-{nanos}"))
    }

    #[test]
    fn unpack_supports_binary_gff_resource() {
        let directory = unique_test_dir("gff-unpack");
        fs::create_dir_all(&directory).expect("create temp dir");
        let input = directory.join("fixture.utc");
        let source_dir = unique_test_dir("gff-unpack-source");
        fs::create_dir_all(&source_dir).expect("create source dir");
        let source = source_dir.join("fixture.utc");
        let mut bytes = Cursor::new(Vec::new());
        gff::write_gff_root(&mut bytes, &gff::GffRoot::new("UTC ")).expect("write gff fixture");
        fs::write(&source, bytes.into_inner()).expect("write source fixture");

        run_unpack(UnpackCmd {
            directory: directory.clone(),
            force:     false,
            input:     source.clone(),
        })
        .expect("unpack gff resource");

        assert_eq!(
            fs::read(&source).expect("read source"),
            fs::read(&input).expect("read unpacked")
        );
        let _ = fs::remove_dir_all(directory);
        let _ = fs::remove_dir_all(source_dir);
    }

    #[test]
    fn unpack_supports_binary_twoda_resource() {
        let directory = unique_test_dir("twoda-unpack");
        let source_dir = unique_test_dir("twoda-unpack-source");
        fs::create_dir_all(&source_dir).expect("create source dir");
        let source = source_dir.join("appearance.2da");
        fs::write(&source, b"2DA V2.0\nDEFAULT: ****\n\nLABEL\n0 value\n")
            .expect("write source fixture");

        run_unpack(UnpackCmd {
            directory: directory.clone(),
            force:     false,
            input:     source.clone(),
        })
        .expect("unpack twoda resource");

        assert_eq!(
            fs::read(&source).expect("read source"),
            fs::read(directory.join("appearance.2da")).expect("read unpacked")
        );
        let _ = fs::remove_dir_all(directory);
        let _ = fs::remove_dir_all(source_dir);
    }

    #[test]
    fn unpack_and_pack_roundtrip_raw_ncs_as_asm() {
        let directory = unique_test_dir("ncs-unpack");
        fs::create_dir_all(&directory).expect("create temp dir");
        let input = directory.join("fixture.ncs");
        let repacked = directory.join("roundtrip.ncs");
        let bytes = nwscript::encode_ncs_instructions(&[nwscript::NcsInstruction {
            opcode:  nwscript::NcsOpcode::Ret,
            auxcode: nwscript::NcsAuxCode::None,
            extra:   Vec::new(),
        }]);
        fs::write(&input, &bytes).expect("write ncs fixture");

        run_unpack(UnpackCmd {
            directory: directory.clone(),
            force:     true,
            input:     input.clone(),
        })
        .expect("unpack ncs resource");

        let asm = directory.join("fixture.ncs.asm");
        let text = fs::read_to_string(&asm).expect("read unpacked asm");
        assert!(text.contains("RET"));

        run_pack(PackCmd {
            force:               true,
            debug:               false,
            no_entrypoint_check: false,
            langspec:            None,
            include_dir:         Vec::new(),
            optimization:        crate::compile::DEFAULT_OPTIMIZATION.to_string(),
            optimization_flag:   Vec::new(),
            jobs:                None,
            data_version:        "V1".to_string(),
            data_compression:    "none".to_string(),
            no_squash:           false,
            no_symlinks:         false,
            erf_type:            None,
            root:                None,
            user:                None,
            language:            None,
            paths:               vec![directory.clone(), repacked.clone()],
        })
        .expect("pack asm back into ncs");

        assert_eq!(fs::read(&repacked).expect("read repacked ncs"), bytes);
        let _ = fs::remove_dir_all(directory);
    }
}
