use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use indexmap::IndexMap;
use nwnrs_checksums::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use tracing::{debug, instrument};

use crate::{ResDir, ResDirError, ResDirResult};

/// Reads a directory tree as a flat resource container.
///
/// # Errors
///
/// Returns [`ResDirError`] if the path is not a directory or any file metadata
/// cannot be read.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn read_resdir(path: impl AsRef<Path>) -> ResDirResult<ResDir> {
    let root = path.as_ref();
    let metadata = fs::metadata(root)?;
    if !metadata.is_dir() {
        return Err(ResDirError::msg(format!(
            "{} is not a directory",
            root.display()
        )));
    }

    let label = root.display().to_string();
    let container_name = format!("ResDir:{label}");
    let mut entries = IndexMap::new();

    let mut files = Vec::new();
    collect_files(root, root, &mut files)?;
    files.sort_by_key(|relative| relative.to_string_lossy().to_ascii_lowercase());

    for relative in files {
        let Some(resolved) = ResolvedResRef::try_from_filename(&relative.to_string_lossy()) else {
            continue;
        };

        let path = root.join(&relative);
        let file_metadata = fs::metadata(&path)?;
        let mtime = file_metadata.modified().unwrap_or(SystemTime::UNIX_EPOCH);
        let io_size = file_metadata.len().cast_signed();
        let label_for_origin = path.display().to_string();
        let path_for_io = path.clone();
        let spawner = Arc::new(
            move || -> io::Result<Box<dyn nwnrs_resman::ReadSeek + Send>> {
                Ok(Box::new(File::open(&path_for_io)?))
            },
        );

        entries.insert(
            resolved.base().clone(),
            new_res(
                &container_name,
                label_for_origin,
                resolved.base().clone(),
                mtime,
                spawner,
                io_size,
            ),
        );
    }

    let result = ResDir {
        root: root.to_path_buf(),
        label,
        entries,
    };
    debug!(
        entry_count = result.entries.len(),
        "read resource directory"
    );
    Ok(result)
}

fn new_res(
    container_name: &str,
    label_for_origin: String,
    resref: ResRef,
    mtime: SystemTime,
    spawner: Arc<dyn Fn() -> io::Result<Box<dyn nwnrs_resman::ReadSeek + Send>> + Send + Sync>,
    io_size: i64,
) -> Res {
    Res::new_with_spawner(
        new_res_origin(container_name.to_string(), label_for_origin),
        resref,
        mtime,
        spawner,
        io_size,
        0,
        ExoResFileCompressionType::None,
        None,
        usize::try_from(io_size.max(0)).unwrap_or(usize::MAX),
        EMPTY_SECURE_HASH,
    )
}

fn collect_files(root: &Path, directory: &Path, out: &mut Vec<PathBuf>) -> io::Result<()> {
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, io::Error>>()?;
    entries.sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());

    for entry in entries {
        let file_type = entry.file_type()?;
        let path = entry.path();
        if file_type.is_dir() {
            collect_files(root, &path, out)?;
        } else if file_type.is_file() {
            let relative = path
                .strip_prefix(root)
                .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?
                .to_path_buf();
            out.push(relative);
        }
    }
    Ok(())
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        time::{SystemTime, UNIX_EPOCH},
    };

    use nwnrs_resman::{CachePolicy, ResContainer};
    use nwnrs_resref::ResolvedResRef;

    use crate::read_resdir;

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-resdir-{prefix}-{nanos}"))
    }

    #[test]
    fn reads_valid_files_and_skips_unknown_extensions() {
        let root = unique_test_dir("scan");
        if let Err(error) = fs::create_dir_all(&root) {
            panic!("create root: {error}");
        }
        if let Err(error) = fs::write(root.join("alpha.utc"), b"alpha") {
            panic!("write alpha: {error}");
        }
        if let Err(error) = fs::write(root.join("notes.unknown"), b"ignored") {
            panic!("write ignored: {error}");
        }

        let dir = match read_resdir(&root) {
            Ok(value) => value,
            Err(error) => panic!("read resdir: {error}"),
        };
        assert_eq!(dir.count(), 1);

        let rr = match ResolvedResRef::from_filename("alpha.utc") {
            Ok(value) => value,
            Err(error) => panic!("resolve rr: {error}"),
        };
        let res = match dir.demand(rr.base()) {
            Ok(value) => value,
            Err(error) => panic!("demand alpha: {error}"),
        };
        let bytes = match res.read_all(CachePolicy::Bypass) {
            Ok(value) => value,
            Err(error) => panic!("read alpha: {error}"),
        };
        assert_eq!(bytes, b"alpha".to_vec());
    }
}
