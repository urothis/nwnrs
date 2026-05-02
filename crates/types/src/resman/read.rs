use std::{
    collections::hash_map::RandomState,
    fs::{self, File},
    io::{self, Cursor},
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use indexmap::IndexMap;
use nwnrs_types::{checksums::prelude::*, exo::prelude::*};
use tracing::{debug, instrument};

use crate::resman::{
    Res, ResDir, ResDirError, ResDirResult, ResFile, ResFileError, ResFileResult, ResIoSpawner,
    ResManError, ResMemFile, ResMemFileResult, ResRef, ResolvedResRef, new_res_origin,
    shared_stream,
};

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
    let mut entries = IndexMap::with_hasher(RandomState::new());

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
            move || -> io::Result<Box<dyn crate::resman::ReadSeek + Send>> {
                Ok(Box::new(File::open(&path_for_io)?))
            },
        );

        entries.insert(
            resolved.base().clone(),
            new_spawner_res(
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
        move || -> io::Result<Box<dyn crate::resman::ReadSeek + Send>> {
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

/// Wraps owned bytes as a one-entry in-memory resource container.
///
/// # Errors
///
/// Returns [`crate::resman::ResMemFileError`] if the resource reference is
/// invalid.
#[instrument(level = "debug", skip_all, err)]
pub fn read_resmemfile(
    label: impl Into<String>,
    resref: ResRef,
    bytes: impl Into<Vec<u8>>,
) -> ResMemFileResult<ResMemFile> {
    let label = label.into();
    let bytes = bytes.into();
    let len = bytes.len();
    let stream = shared_stream(Cursor::new(bytes));

    let result = ResMemFile {
        label: label.clone(),
        len,
        entry: Res::new_with_stream(
            new_res_origin(format!("ResMemFile:{label}"), label.clone()),
            resref,
            SystemTime::UNIX_EPOCH,
            stream,
            i64::try_from(len).map_err(|e| {
                ResManError::Message(format!("resource size exceeds i64 range: {e}"))
            })?,
            0,
            ExoResFileCompressionType::None,
            None,
            len,
            EMPTY_SECURE_HASH,
        ),
    };
    debug!(len, "wrapped in-memory resource");
    Ok(result)
}

/// Wraps shared bytes as a one-entry in-memory resource container.
///
/// # Errors
///
/// Returns [`crate::resman::ResMemFileError`] if the resource reference is
/// invalid.
#[instrument(level = "debug", skip_all, err)]
pub fn read_resmemfile_arc(
    label: impl Into<String>,
    resref: ResRef,
    bytes: Arc<[u8]>,
) -> ResMemFileResult<ResMemFile> {
    read_resmemfile(label, resref, bytes.as_ref().to_vec())
}

fn new_spawner_res(
    container_name: &str,
    label_for_origin: String,
    resref: ResRef,
    mtime: SystemTime,
    io_spawner: ResIoSpawner,
    io_size: i64,
) -> Res {
    Res::new_with_spawner(
        new_res_origin(container_name.to_string(), label_for_origin),
        resref,
        mtime,
        io_spawner,
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

#[cfg(test)]
mod tests {
    use std::{
        fs,
        path::PathBuf,
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use crate::resman::{
        CachePolicy, ResContainer, ResRef, ResType, ResolvedResRef, read_resdir, read_resfile,
        read_resfile_as, read_resmemfile, read_resmemfile_arc,
    };

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-types-{prefix}-{nanos}"))
    }

    #[test]
    fn reads_directory_resources_and_skips_unknown_extensions() {
        let root = unique_test_dir("dir");
        if let Err(error) = fs::create_dir_all(&root) {
            panic!("create dir tree: {error}");
        }
        if let Err(error) = fs::write(root.join("alpha.utc"), b"one") {
            panic!("write alpha: {error}");
        }
        if let Err(error) = fs::write(root.join("beta.tga"), b"two") {
            panic!("write beta: {error}");
        }
        if let Err(error) = fs::write(root.join("notes.invalidext"), b"ignored") {
            panic!("write notes: {error}");
        }

        let dir = match read_resdir(&root) {
            Ok(value) => value,
            Err(error) => panic!("read resdir: {error}"),
        };

        assert_eq!(dir.count(), 2);
        let alpha = ResolvedResRef::from_filename("alpha.utc")
            .unwrap_or_else(|error| panic!("alpha resref: {error}"));
        let beta = ResolvedResRef::from_filename("beta.tga")
            .unwrap_or_else(|error| panic!("beta resref: {error}"));
        assert!(dir.contains(alpha.base()));
        assert!(dir.contains(beta.base()));
        let alpha_bytes = dir
            .demand(alpha.base())
            .and_then(|res| res.read_all(CachePolicy::Bypass))
            .unwrap_or_else(|error| panic!("read alpha payload: {error}"));
        assert_eq!(alpha_bytes, b"one".to_vec());

        if let Err(error) = fs::remove_dir_all(&root) {
            panic!("cleanup dir tree: {error}");
        }
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
            None => panic!("utf8 filename"),
        };
        let resolved = ResolvedResRef::from_filename(filename)
            .unwrap_or_else(|error| panic!("resolve filename: {error}"));

        assert_eq!(resfile.res().resref(), resolved.into());
        let bytes = match resfile.res().read_all(CachePolicy::Bypass) {
            Ok(value) => value,
            Err(error) => panic!("read payload: {error}"),
        };
        assert_eq!(bytes, b"payload".to_vec());

        if let Err(error) = fs::remove_dir_all(&root) {
            panic!("cleanup root: {error}");
        }
    }

    #[test]
    fn supports_explicit_resref_override() {
        let root = unique_test_dir("override");
        if let Err(error) = fs::create_dir_all(&root) {
            panic!("create root: {error}");
        }
        let path = root.join("source.bin");
        if let Err(error) = fs::write(&path, b"bytes") {
            panic!("write file: {error}");
        }

        let rr = ResRef::new("manual", ResType(2027)).unwrap_or_else(|error| {
            panic!("manual rr: {error}");
        });
        let resfile = match read_resfile_as(&path, rr.clone()) {
            Ok(value) => value,
            Err(error) => panic!("read resfile as: {error}"),
        };

        assert!(resfile.contains(&rr));
        let bytes = match resfile.res().read_all(CachePolicy::Bypass) {
            Ok(value) => value,
            Err(error) => panic!("read payload: {error}"),
        };
        assert_eq!(bytes, b"bytes".to_vec());

        if let Err(error) = fs::remove_dir_all(&root) {
            panic!("cleanup root: {error}");
        }
    }

    #[test]
    fn wraps_owned_bytes_as_resource_container() {
        let rr = match ResRef::new("alpha", ResType(2027)) {
            Ok(value) => value,
            Err(error) => panic!("alpha rr: {error}"),
        };
        let resmem = match read_resmemfile("mem", rr.clone(), b"payload".to_vec()) {
            Ok(value) => value,
            Err(error) => panic!("read resmemfile: {error}"),
        };
        assert_eq!(resmem.len(), 7);
        assert!(!resmem.is_empty());
        assert!(resmem.contains(&rr));
        let bytes = match resmem.res().read_all(CachePolicy::Bypass) {
            Ok(value) => value,
            Err(error) => panic!("read payload: {error}"),
        };
        assert_eq!(bytes, b"payload".to_vec());
    }

    #[test]
    fn wraps_shared_bytes_without_changing_contents() {
        let rr = match ResRef::new("beta", ResType(2027)) {
            Ok(value) => value,
            Err(error) => panic!("beta rr: {error}"),
        };
        let resmem = match read_resmemfile_arc("mem-arc", rr, Arc::from(&b"arc"[..])) {
            Ok(value) => value,
            Err(error) => panic!("read resmemfile arc: {error}"),
        };
        let bytes = match resmem.res().read_all(CachePolicy::Bypass) {
            Ok(value) => value,
            Err(error) => panic!("read shared payload: {error}"),
        };
        assert_eq!(bytes, b"arc".to_vec());
    }
}
