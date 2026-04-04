use std::{
    fs::{self, File},
    io,
    path::Path,
    sync::Arc,
    time::SystemTime,
};

use nwnrs_checksums::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use tracing::{debug, instrument};

use crate::{ResFile, ResFileError, ResFileResult};

/// Reads a resource file using its filename-derived resource reference.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_resfile(path: impl AsRef<Path>) -> ResFileResult<ResFile> {
    let path = path.as_ref();
    let file_name = path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| ResFileError::msg(format!("{} has no valid filename", path.display())))?;
    let resolved = ResolvedResRef::from_filename(file_name)?;
    read_resfile_as(path, resolved.base().clone())
}

/// Reads a resource file with an explicit resource reference override.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display(), resref = %resref))]
pub fn read_resfile_as(path: impl AsRef<Path>, resref: ResRef) -> ResFileResult<ResFile> {
    let path = path.as_ref();
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(ResFileError::msg(format!(
            "{} is not a regular file",
            path.display()
        )));
    }

    let label = path.display().to_string();
    let mtime = metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
    let io_size = metadata.len() as i64;
    let path_for_io = path.to_path_buf();
    let origin_label = label.clone();
    let spawner = Arc::new(
        move || -> io::Result<Box<dyn nwnrs_resman::ReadSeek + Send>> {
            Ok(Box::new(File::open(&path_for_io)?))
        },
    );

    let result = ResFile {
        path: path.to_path_buf(),
        label: label.clone(),
        entry: Res::new_with_spawner(
            new_res_origin(format!("ResFile:{label}"), origin_label),
            resref,
            mtime,
            spawner,
            io_size,
            0,
            ExoResFileCompressionType::None,
            usize::try_from(io_size.max(0)).unwrap_or(usize::MAX),
            EMPTY_SECURE_HASH,
        ),
    };
    debug!(io_size, "read resource file");
    Ok(result)
}
