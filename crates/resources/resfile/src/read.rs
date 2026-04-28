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
///
/// # Errors
///
/// Returns [`ResFileError`] if the filename cannot be resolved or the file
/// cannot be read.
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
///
/// # Errors
///
/// Returns [`ResFileError`] if the path is not a regular file or metadata
/// cannot be read.
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
    let io_size = metadata.len().cast_signed();
    let path_for_io = path.to_path_buf();
    let origin_label = label.clone();
    let spawner = Arc::new(
        move || -> io::Result<Box<dyn nwnrs_resman::ReadSeek + Send>> {
            Ok(Box::new(File::open(&path_for_io)?))
        },
    );

    let result = ResFile {
        path:  path.to_path_buf(),
        label: label.clone(),
        entry: Res::new_with_spawner(
            new_res_origin(format!("ResFile:{label}"), origin_label),
            resref,
            mtime,
            spawner,
            io_size,
            0,
            ExoResFileCompressionType::None,
            None,
            usize::try_from(io_size.max(0)).unwrap_or(usize::MAX),
            EMPTY_SECURE_HASH,
        ),
    };
    debug!(io_size, "read resource file");
    Ok(result)
}

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_resman::{CachePolicy, ResContainer};
    use nwnrs_resref::{ResRef, ResolvedResRef};
    use nwnrs_restype::ResType;

    use crate::{read_resfile, read_resfile_as};

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-rf-{prefix}-{nanos}"))
    }

    #[test]
    fn reads_resref_from_filename() {
        let root = unique_test_dir("auto");
        if let Err(error) = fs::create_dir_all(&root) {
            panic!("create root: {error}");
        }
        let path = root.join("alpha.utc");
        if let Err(error) = fs::write(&path, b"payload") {
            panic!("write file: {error}");
        }

        let resfile = match read_resfile(&path) {
            Ok(value) => value,
            Err(error) => panic!("read resfile: {error}"),
        };
        let filename = match path.file_name().and_then(|value| value.to_str()) {
            Some(value) => value,
            None => panic!("filename should be valid utf-8"),
        };
        let rr = match ResolvedResRef::from_filename(filename) {
            Ok(value) => value,
            Err(error) => panic!("resolve rr: {error}"),
        };
        assert!(resfile.contains(rr.base()));
        let bytes = match resfile.res().read_all(CachePolicy::Bypass) {
            Ok(value) => value,
            Err(error) => panic!("read payload: {error}"),
        };
        assert_eq!(bytes, b"payload".to_vec());
    }

    #[test]
    fn supports_explicit_resref_override() {
        let root = unique_test_dir("override");
        if let Err(error) = fs::create_dir_all(&root) {
            panic!("create root: {error}");
        }
        let path = root.join("payload.bin");
        if let Err(error) = fs::write(&path, b"payload") {
            panic!("write file: {error}");
        }
        let rr = match ResRef::new("custom", ResType(2027)) {
            Ok(value) => value,
            Err(error) => panic!("custom rr: {error}"),
        };

        let resfile = match read_resfile_as(&path, rr.clone()) {
            Ok(value) => value,
            Err(error) => panic!("read resfile as: {error}"),
        };
        assert!(resfile.contains(&rr));
    }
}
