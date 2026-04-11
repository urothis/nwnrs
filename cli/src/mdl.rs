use std::{fs::File, path::Path};

use nwnrs::prelude::*;
use tracing::{info, instrument};

use crate::{
    args::{MdlCmd, MdlCommand, MdlToAsciiCmd, MdlToCompiledCmd},
    util::ensure_output_file_ready,
};

#[instrument(level = "info", skip_all, err)]
pub(crate) fn run_mdl(cmd: MdlCmd) -> Result<(), String> {
    match cmd.command {
        MdlCommand::ToAscii(cmd) => run_mdl_to_ascii(cmd),
        MdlCommand::ToCompiled(cmd) => run_mdl_to_compiled(cmd),
    }
}

#[instrument(level = "info", skip_all, err, fields(path = %cmd.input.display()))]
fn run_mdl_to_ascii(cmd: MdlToAsciiCmd) -> Result<(), String> {
    info!("lowering mdl to canonical ascii");
    ensure_output_file_ready(&cmd.output, cmd.force)?;

    let ascii = canonical_ascii_model(&cmd.input)?;
    let mut output = create_output_file(&cmd.output)?;
    mdl::write_ascii_model(&mut output, &ascii)
        .map_err(|error| format!("failed to write {}: {error}", cmd.output.display()))
}

#[instrument(level = "info", skip_all, err, fields(path = %cmd.input.display()))]
fn run_mdl_to_compiled(cmd: MdlToCompiledCmd) -> Result<(), String> {
    info!("rebuilding compiled mdl from canonical ascii");
    ensure_output_file_ready(&cmd.output, cmd.force)?;

    let ascii = mdl::read_ascii_model_from_file(&cmd.input).map_err(|error| {
        format!(
            "failed to parse {} as ASCII MDL: {error}",
            cmd.input.display()
        )
    })?;
    let compiled = mdl::compile_ascii_model(&ascii).map_err(|error| {
        format!(
            "failed to rebuild compiled MDL from {}: {error}",
            cmd.input.display()
        )
    })?;
    let mut output = create_output_file(&cmd.output)?;
    mdl::write_model(&mut output, &compiled)
        .map_err(|error| format!("failed to write {}: {error}", cmd.output.display()))
}

fn canonical_ascii_model(path: &Path) -> Result<mdl::AsciiModel, String> {
    let parsed = mdl::read_parsed_model_from_file(path)
        .map_err(|error| format!("failed to parse {} as MDL: {error}", path.display()))?;
    match parsed {
        mdl::ParsedModel::Ascii(model) => Ok(model),
        mdl::ParsedModel::Compiled(model) => {
            mdl::lower_binary_model_to_ascii(&model).map_err(|error| {
                format!(
                    "failed to lower compiled MDL {} to ascii: {error}",
                    path.display()
                )
            })
        }
    }
}

fn create_output_file(path: &Path) -> Result<File, String> {
    File::create(path).map_err(|error| format!("failed to create {}: {error}", path.display()))
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{error::Error, fs};

    use nwnrs::prelude::mdl;
    use nwnrs_test_support::{
        demand_resource, materialize_bytes_to_temp_file, materialize_resource_to_temp_file,
        require_game_resource, skip_if_game_resources_unavailable,
    };

    use super::{run_mdl_to_ascii, run_mdl_to_compiled};
    use crate::args::{MdlToAsciiCmd, MdlToCompiledCmd};

    #[test]
    fn mdl_to_ascii_writes_canonical_text_for_compiled_input() -> Result<(), Box<dyn Error>> {
        let input = match compiled_fixture_path() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let output = temp_output_path("a_ba2-ascii.mdl");

        run_mdl_to_ascii(MdlToAsciiCmd {
            force: true,
            input,
            output: output.clone(),
        })
        .expect("lower compiled mdl to ascii");

        let ascii = mdl::read_ascii_model_from_file(&output).expect("read canonical ascii mdl");
        assert_eq!(ascii.geometry_name, "a_ba2");
        assert!(ascii.to_text().contains("# nwnrs-compiled-source begin"));
        Ok(())
    }

    #[test]
    fn mdl_to_compiled_roundtrips_generated_canonical_ascii() -> Result<(), Box<dyn Error>> {
        let input = match canonical_ascii_fixture_path() {
            Ok(path) => path,
            Err(error) => return skip_if_game_resources_unavailable(error),
        };
        let output = temp_output_path("a_ba2-compiled.mdl");

        run_mdl_to_compiled(MdlToCompiledCmd {
            force: true,
            input,
            output: output.clone(),
        })
        .expect("rebuild compiled mdl");

        let compiled = mdl::read_binary_model_from_file(&output).expect("read compiled mdl");
        assert_eq!(compiled.name, "a_ba2");
        assert_eq!(compiled.animations.len(), 20);
        Ok(())
    }

    fn compiled_fixture_path() -> Result<std::path::PathBuf, Box<dyn Error>> {
        require_game_resource(materialize_resource_to_temp_file(
            "a_ba2",
            mdl::MODEL_RES_TYPE,
        ))
    }

    fn canonical_ascii_fixture_path() -> Result<std::path::PathBuf, Box<dyn Error>> {
        let res = require_game_resource(demand_resource("a_ba2", mdl::MODEL_RES_TYPE))?;
        let binary = mdl::read_binary_model_from_res(&res, true)?;
        let ascii = mdl::lower_binary_model_to_ascii(&binary)?;
        Ok(materialize_bytes_to_temp_file(
            &ascii.to_text().into_bytes(),
            "a_ba2-ascii.mdl",
        )?)
    }

    fn temp_output_path(filename: &str) -> std::path::PathBuf {
        let path = std::env::temp_dir().join(format!("nwnrs-cli-{filename}"));
        let _ = fs::remove_file(&path);
        path
    }
}
