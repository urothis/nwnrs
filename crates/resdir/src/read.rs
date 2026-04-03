use crate::{ResDir, ResDirError, ResDirResult};
use indexmap::IndexMap;
use nwn_checksums::EMPTY_SECURE_HASH;
use nwn_exo::ExoResFileCompressionType;
use nwn_resman::{Res, new_res_origin};
use nwn_resref::{ResRef, ResolvedResRef};
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::SystemTime;
use tracing::{debug, instrument};

/// Reads a directory tree as a flat resource container.
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
        let io_size = file_metadata.len() as i64;
        let label_for_origin = path.display().to_string();
        let path_for_io = path.clone();
        let spawner = Arc::new(
            move || -> io::Result<Box<dyn nwn_resman::ReadSeek + Send>> {
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
    spawner: Arc<dyn Fn() -> io::Result<Box<dyn nwn_resman::ReadSeek + Send>> + Send + Sync>,
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
