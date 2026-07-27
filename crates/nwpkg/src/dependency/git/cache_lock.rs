use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions, TryLockError},
    path::Path,
    thread,
    time::{Duration, Instant},
};

use nwnrs_nwscript::CancellationToken;

use super::check_cancelled;
use crate::{NwpkgError, NwpkgErrorKind};

const CACHE_LOCK_TIMEOUT: Duration = Duration::from_secs(300);

#[derive(Debug, Default)]
pub(super) struct CacheLeases {
    locks: BTreeMap<String, CacheLock>,
}

impl CacheLeases {
    pub(super) fn acquire(
        &mut self,
        cache_root: &Path,
        key: &str,
        cancellation: &CancellationToken,
    ) -> Result<(), NwpkgError> {
        if !self.locks.contains_key(key) {
            self.locks.insert(
                key.to_string(),
                CacheLock::acquire(cache_root, key, cancellation)?,
            );
        }
        Ok(())
    }

    pub(super) fn release(&mut self, key: &str) {
        self.locks.remove(key);
    }
}

#[derive(Debug)]
pub(super) struct CacheLock {
    _file: File,
}

impl CacheLock {
    pub(super) fn acquire(
        cache_root: &Path,
        key: &str,
        cancellation: &CancellationToken,
    ) -> Result<Self, NwpkgError> {
        let (path, file) = open_lock_file(cache_root, key)?;
        let started = Instant::now();
        loop {
            check_cancelled(cancellation)?;
            match file.try_lock() {
                Ok(()) => {
                    return Ok(Self {
                        _file: file
                    })
                }
                Err(TryLockError::WouldBlock) => {
                    if started.elapsed() >= CACHE_LOCK_TIMEOUT {
                        return Err(NwpkgError::at(
                            NwpkgErrorKind::Io,
                            &path,
                            "timed out waiting for another dependency cache writer",
                        ));
                    }
                    thread::sleep(Duration::from_millis(50));
                }
                Err(TryLockError::Error(error)) => {
                    return Err(NwpkgError::at(
                        NwpkgErrorKind::Io,
                        &path,
                        format!("failed to acquire dependency cache lock: {error}"),
                    ));
                }
            }
        }
    }

    pub(super) fn try_acquire(cache_root: &Path, key: &str) -> Result<Option<Self>, NwpkgError> {
        let (path, file) = open_lock_file(cache_root, key)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Self {
                _file: file
            })),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to acquire dependency cache lock: {error}"),
            )),
        }
    }
}

fn open_lock_file(cache_root: &Path, key: &str) -> Result<(std::path::PathBuf, File), NwpkgError> {
    let root = cache_root.join(".locks");
    fs::create_dir_all(&root).map_err(|error| {
        NwpkgError::at(
            NwpkgErrorKind::Io,
            &root,
            format!("failed to create dependency cache lock directory: {error}"),
        )
    })?;
    let path = root.join(format!("{key}.lock"));
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(&path)
        .map_err(|error| {
            NwpkgError::at(
                NwpkgErrorKind::Io,
                &path,
                format!("failed to open dependency cache lock: {error}"),
            )
        })?;
    Ok((path, file))
}
