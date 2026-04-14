use std::{collections::BTreeMap, fs, fs::File, io::BufReader, path::Path};

use nwnrs::{prelude::*, resman::CachePolicy};
use tracing::{debug, info, instrument};

use crate::{
    args::InspectCmd,
    util::{Kind, detect_kind, write_stdout_line},
};

#[instrument(level = "info", skip_all, err, fields(path = %cmd.path.display()))]
pub(crate) fn run_inspect(cmd: &InspectCmd) -> Result<(), String> {
    let path = &cmd.path;
    info!("inspecting file");
    match detect_kind(path) {
        Some(Kind::Erf) => {
            debug!("detected ERF-family input");
            let erf = erf::read_erf_from_file(path).map_err(|error| {
                format!("failed to parse {} as ERF/MOD: {error}", path.display())
            })?;
            write_stdout_line(&format!("{erf:#?}"))
        }
        Some(Kind::Ncs) => {
            debug!("detected NCS input");
            inspect_ncs(cmd)
        }
        Some(Kind::Key) => {
            debug!("detected KEY input");
            let key = key::read_key_table_from_file(path)
                .map_err(|error| format!("failed to parse {} as KEY: {error}", path.display()))?;
            write_stdout_line(&format!("{key:#?}"))
        }
        Some(Kind::Ssf) => {
            debug!("detected SSF input");
            let file = File::open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut reader = BufReader::new(file);
            let ssf = ssf::read_ssf(&mut reader)
                .map_err(|error| format!("failed to parse {} as SSF: {error}", path.display()))?;
            write_stdout_line(&format!("{ssf:#?}"))
        }
        Some(Kind::Model) => {
            debug!("detected MDL input");
            let summary = inspect_model(path)?;
            write_stdout_line(&summary)
        }
        Some(Kind::Texture) => {
            debug!("detected texture input");
            inspect_texture(path)
        }
        Some(Kind::Tlk) => {
            debug!("detected TLK input");
            let tlk = tlk::SingleTlk::from_file(path, CachePolicy::Use)
                .map_err(|error| format!("failed to parse {} as TLK: {error}", path.display()))?;
            write_stdout_line(&format!("{tlk:#?}"))
        }
        Some(Kind::TwoDa) => {
            debug!("detected 2DA input");
            let file = File::open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut reader = BufReader::new(file);
            let twoda = twoda::read_twoda(&mut reader)
                .map_err(|error| format!("failed to parse {} as 2DA: {error}", path.display()))?;
            write_stdout_line(&format!("{twoda:#?}"))
        }
        Some(Kind::Gff) => {
            debug!("detected GFF-family input");
            let file = File::open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut reader = BufReader::new(file);
            let gff = gff::read_gff_root(&mut reader)
                .map_err(|error| format!("failed to parse {} as GFF: {error}", path.display()))?;
            write_stdout_line(&format!("{gff:#?}"))
        }
        None => Err(format!("unsupported file type for {}", path.display())),
    }
}

fn inspect_texture(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .map(str::to_ascii_lowercase)
        .ok_or_else(|| format!("failed to infer texture format from {}", path.display()))?;
    match extension.as_str() {
        "tga" => {
            let texture = tga::TgaTexture::from_file(path)
                .map_err(|error| format!("failed to parse {} as TGA: {error}", path.display()))?;
            write_stdout_line(&format!("{texture:#?}"))
        }
        "dds" => {
            let texture = dds::DdsTexture::from_file(path)
                .map_err(|error| format!("failed to parse {} as DDS: {error}", path.display()))?;
            write_stdout_line(&format!("{texture:#?}"))
        }
        "plt" => {
            let texture = plt::PltTexture::from_file(path)
                .map_err(|error| format!("failed to parse {} as PLT: {error}", path.display()))?;
            write_stdout_line(&format!("{texture:#?}"))
        }
        _ => Err(format!("unsupported texture format for {}", path.display())),
    }
}

fn inspect_model(path: &Path) -> Result<String, String> {
    let parsed = mdl::ParsedModel::from_file(path)
        .map_err(|error| format!("failed to parse {} as MDL: {error}", path.display()))?;
    match parsed {
        mdl::ParsedModel::Ascii(model) => Ok(format!("MDL encoding: ascii\n{model:#?}")),
        mdl::ParsedModel::Compiled(model) => {
            let block_kinds = compiled_block_kinds(&model).join(", ");
            Ok(format!(
                "MDL encoding: compiled\nmodel: {}\nnode_count: {}\nanimation_count: \
                 {}\nrecognized_block_kinds: {}\ndiagnostic_count: {}",
                model.name,
                model.nodes.len(),
                model.animations.len(),
                if block_kinds.is_empty() {
                    "none"
                } else {
                    &block_kinds
                },
                model.diagnostics.len(),
            ))
        }
    }
}

fn inspect_ncs(cmd: &InspectCmd) -> Result<(), String> {
    let path = &cmd.path;
    let bytes =
        fs::read(path).map_err(|error| format!("failed to read {}: {error}", path.display()))?;
    let langspec = load_langspec_for_ncs(cmd);
    let ndb = load_ndb_for_ncs(cmd)?;
    let source_files = ndb
        .as_ref()
        .filter(|_| !cmd.no_source_weave)
        .map(|ndb| load_adjacent_source_files(path, ndb))
        .unwrap_or_default();
    let rendered = nwscript::render_ncs_disassembly_with_ndb(
        &bytes,
        langspec.as_ref(),
        ndb.as_ref(),
        (!source_files.is_empty()).then_some(&source_files),
        nwscript::NcsDisassemblyOptions {
            internal_names:    cmd.internal_names,
            max_string_length: cmd.max_string_length,
            labels:            !cmd.no_labels,
            offsets:           !cmd.no_offsets,
            local_offsets:     !cmd.no_local_offsets,
            source_weave:      !cmd.no_source_weave,
        },
    )
    .map_err(|error| format!("failed to disassemble {}: {error}", path.display()))?;
    write_stdout_line(&rendered)
}

fn load_langspec_for_ncs(cmd: &InspectCmd) -> Option<nwscript::LangSpec> {
    if cmd.no_langspec {
        return None;
    }
    let candidate = cmd
        .langspec
        .clone()
        .or_else(|| cmd.path.parent().map(|parent| parent.join("nwscript.nss")))?;
    let bytes = fs::read(candidate).ok()?;
    nwscript::parse_langspec_bytes("nwscript.nss", &bytes).ok()
}

fn load_ndb_for_ncs(cmd: &InspectCmd) -> Result<Option<nwscript::Ndb>, String> {
    if cmd.no_ndb {
        return Ok(None);
    }
    let candidate = cmd.path.with_extension("ndb");
    let Some(bytes) = fs::read(&candidate).ok() else {
        if cmd.require_ndb {
            return Err(format!(
                "failed to locate required sibling {}",
                candidate.display()
            ));
        }
        return Ok(None);
    };
    let mut cursor = std::io::Cursor::new(bytes);
    nwscript::read_ndb(&mut cursor)
        .map(Some)
        .map_err(|error| format!("failed to parse {}: {error}", candidate.display()))
}

fn load_adjacent_source_files(path: &Path, ndb: &nwscript::Ndb) -> BTreeMap<String, Vec<String>> {
    let mut files = BTreeMap::new();
    let Some(parent) = path.parent() else {
        return files;
    };

    for file in &ndb.files {
        let candidate = parent.join(format!("{}.nss", file.name));
        let Ok(text) = fs::read_to_string(candidate) else {
            continue;
        };
        files.insert(
            file.name.clone(),
            text.lines().map(str::to_string).collect(),
        );
    }

    files
}

fn compiled_block_kinds(model: &mdl::BinaryModel) -> Vec<&'static str> {
    let mut kinds = std::collections::BTreeSet::new();
    for node in model.nodes.iter().chain(
        model
            .animations
            .iter()
            .flat_map(|animation| animation.nodes.iter()),
    ) {
        if node.content.has_header {
            kinds.insert("header");
        }
        if node.content.has_light {
            kinds.insert("light");
        }
        if node.content.has_emitter {
            kinds.insert("emitter");
        }
        if node.content.has_camera {
            kinds.insert("camera");
        }
        if node.content.has_reference {
            kinds.insert("reference");
        }
        if node.content.has_mesh {
            kinds.insert("mesh");
        }
        if node.content.has_skin {
            kinds.insert("skin");
        }
        if node.content.has_anim {
            kinds.insert("animmesh");
        }
        if node.content.has_dangly {
            kinds.insert("danglymesh");
        }
        if node.content.has_aabb {
            kinds.insert("aabb");
        }
    }
    kinds.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use std::{error::Error, path::Path};

    use nwnrs::prelude as nwn;
    use nwnrs_test_support::{
        materialize_bytes_to_temp_file, materialize_resource_to_temp_file, require_game_resource,
        skip_if_game_resources_unavailable,
    };

    use super::{compiled_block_kinds, inspect_model, inspect_ncs, run_inspect};
    use crate::args::InspectCmd;

    #[test]
    fn rejects_unsupported_extensions_before_reading() {
        let err = run_inspect(&InspectCmd {
            internal_names:    false,
            max_string_length: 15,
            require_ndb:       false,
            no_ndb:            false,
            no_source_weave:   false,
            no_local_offsets:  false,
            no_labels:         false,
            no_offsets:        false,
            no_langspec:       false,
            langspec:          None,
            path:              Path::new("unsupported.xyz").to_path_buf(),
        })
        .expect_err("inspect should fail");
        assert!(err.contains("unsupported file type"));
        assert!(err.contains("unsupported.xyz"));
    }

    #[test]
    fn compiled_model_summary_reports_encoding_and_counts() -> Result<(), Box<dyn Error>> {
        let fixture = match compiled_fixture() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let summary = inspect_model(&fixture).expect("compiled inspect should succeed");
        assert!(summary.contains("MDL encoding: compiled"));
        assert!(summary.contains("model: a_ba2"));
        assert!(summary.contains("node_count: 57"));
        assert!(summary.contains("animation_count: 20"));
        assert!(summary.contains("recognized_block_kinds:"));
        Ok(())
    }

    #[test]
    fn compiled_model_block_kinds_include_header_and_mesh() -> Result<(), Box<dyn Error>> {
        let fixture = match compiled_fixture() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let model =
            nwn::mdl::BinaryModel::from_file(&fixture).expect("compiled fixture should parse");
        let kinds = compiled_block_kinds(&model);
        assert!(kinds.contains(&"header"));
        assert!(kinds.contains(&"mesh"));
        Ok(())
    }

    #[test]
    fn inspect_ncs_renders_disassembly() -> Result<(), Box<dyn Error>> {
        let bytes = nwn::nwscript::encode_ncs_instructions(&[nwn::nwscript::NcsInstruction {
            opcode:  nwn::nwscript::NcsOpcode::Ret,
            auxcode: nwn::nwscript::NcsAuxCode::None,
            extra:   Vec::new(),
        }]);
        let path = materialize_bytes_to_temp_file(&bytes, "inspect_test.ncs")?;

        inspect_ncs(&InspectCmd {
            internal_names: false,
            max_string_length: 15,
            require_ndb: false,
            no_ndb: false,
            no_source_weave: false,
            no_local_offsets: false,
            no_labels: false,
            no_offsets: false,
            no_langspec: false,
            langspec: None,
            path,
        })
        .expect("ncs inspect should succeed");
        Ok(())
    }

    fn compiled_fixture() -> Result<std::path::PathBuf, Box<dyn Error>> {
        require_game_resource(materialize_resource_to_temp_file(
            "a_ba2",
            nwn::mdl::MODEL_RES_TYPE,
        ))
    }
}
