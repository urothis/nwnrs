use std::{fs::File, io::BufReader, path::Path};

use nwnrs::prelude::*;
use tracing::{debug, info, instrument};

use crate::util::{Kind, detect_kind, write_stdout_line};

#[instrument(level = "info", skip_all, err, fields(path = %path.display()))]
pub(crate) fn run_inspect(path: &Path) -> Result<(), String> {
    info!("inspecting file");
    match detect_kind(path) {
        Some(Kind::Erf) => {
            debug!("detected ERF-family input");
            let erf = erf::read_erf_from_file(path).map_err(|error| {
                format!("failed to parse {} as ERF/MOD: {error}", path.display())
            })?;
            write_stdout_line(&format!("{erf:#?}"))
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
            let model = model::read_model_from_file(path)
                .map_err(|error| format!("failed to parse {} as MDL: {error}", path.display()))?;
            write_stdout_line(&format!("{model:#?}"))
        }
        Some(Kind::Texture) => {
            debug!("detected texture input");
            inspect_texture(path)
        }
        Some(Kind::Tlk) => {
            debug!("detected TLK input");
            let tlk = tlk::SingleTlk::from_file(path, true)
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
            let texture = tga::read_tga_from_file(path)
                .map_err(|error| format!("failed to parse {} as TGA: {error}", path.display()))?;
            write_stdout_line(&format!("{texture:#?}"))
        }
        "dds" => {
            let texture = dds::read_dds_from_file(path)
                .map_err(|error| format!("failed to parse {} as DDS: {error}", path.display()))?;
            write_stdout_line(&format!("{texture:#?}"))
        }
        "plt" => {
            let texture = plt::read_plt_from_file(path)
                .map_err(|error| format!("failed to parse {} as PLT: {error}", path.display()))?;
            write_stdout_line(&format!("{texture:#?}"))
        }
        _ => Err(format!("unsupported texture format for {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::run_inspect;

    #[test]
    fn rejects_unsupported_extensions_before_reading() {
        let err = run_inspect(Path::new("unsupported.xyz")).expect_err("inspect should fail");
        assert!(err.contains("unsupported file type"));
        assert!(err.contains("unsupported.xyz"));
    }
}
