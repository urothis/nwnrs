use crate::util::{Kind, detect_kind, write_stdout_line};
use nwn_erf::prelude::*;
use nwn_gff::prelude::*;
use nwn_key::prelude::*;
use nwn_ssf::prelude::*;
use nwn_tlk::prelude::*;
use nwn_twoda::prelude::*;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use tracing::{debug, info, instrument};

#[instrument(level = "info", skip_all, err, fields(path = %path.display()))]
pub(crate) fn run_inspect(path: &Path) -> Result<(), String> {
    info!("inspecting file");
    match detect_kind(path) {
        Some(Kind::Erf) => {
            debug!("detected ERF-family input");
            let erf = read_erf_from_file(path).map_err(|error| {
                format!("failed to parse {} as ERF/MOD: {error}", path.display())
            })?;
            write_stdout_line(&format!("{erf:#?}"))
        }
        Some(Kind::Key) => {
            debug!("detected KEY input");
            let key = read_key_table_from_file(path)
                .map_err(|error| format!("failed to parse {} as KEY: {error}", path.display()))?;
            write_stdout_line(&format!("{key:#?}"))
        }
        Some(Kind::Ssf) => {
            debug!("detected SSF input");
            let file = File::open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut reader = BufReader::new(file);
            let ssf = read_ssf(&mut reader)
                .map_err(|error| format!("failed to parse {} as SSF: {error}", path.display()))?;
            write_stdout_line(&format!("{ssf:#?}"))
        }
        Some(Kind::Tlk) => {
            debug!("detected TLK input");
            let tlk = SingleTlk::from_file(path, true)
                .map_err(|error| format!("failed to parse {} as TLK: {error}", path.display()))?;
            write_stdout_line(&format!("{tlk:#?}"))
        }
        Some(Kind::TwoDa) => {
            debug!("detected 2DA input");
            let file = File::open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut reader = BufReader::new(file);
            let twoda = read_twoda(&mut reader)
                .map_err(|error| format!("failed to parse {} as 2DA: {error}", path.display()))?;
            write_stdout_line(&format!("{twoda:#?}"))
        }
        Some(Kind::Gff) => {
            debug!("detected GFF-family input");
            let file = File::open(path)
                .map_err(|error| format!("failed to open {}: {error}", path.display()))?;
            let mut reader = BufReader::new(file);
            let gff = read_gff_root(&mut reader)
                .map_err(|error| format!("failed to parse {} as GFF: {error}", path.display()))?;
            write_stdout_line(&format!("{gff:#?}"))
        }
        None => Err(format!("unsupported file type for {}", path.display())),
    }
}
