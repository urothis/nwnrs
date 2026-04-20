use std::{
    collections::{HashMap, hash_map::RandomState},
    fs,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use indexmap::IndexSet;
use nwnrs_checksums::prelude::*;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_resref::prelude::*;
use nwnrs_restype::prelude::*;
use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, instrument};

use crate::{
    DEFAULT_SHARD_ID, ManifestSha1, NWSYNC_COMPRESSED_BUF_MAGIC_STR, NWSync, NWSyncShard,
    ResNWSyncError, ResNWSyncManifest, ResNWSyncResult,
};

/// Returns the integer magic for `NSYC` compressed buffers.
///
/// # Errors
///
/// Returns [`ResNWSyncError`] if the magic string cannot be encoded.
#[instrument(level = "debug", skip_all, err)]
pub fn nwsync_compressed_buf_magic() -> ResNWSyncResult<u32> {
    make_magic(NWSYNC_COMPRESSED_BUF_MAGIC_STR)
        .map_err(|error| ResNWSyncError::msg(error.to_string()))
}

/// Opens an `NWSync` repository rooted at `path`.
///
/// # Errors
///
/// Returns [`ResNWSyncError`] if the meta database is missing, a shard database
/// is not found, or any SQL query fails.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn open_nwsync(path: impl AsRef<Path>) -> ResNWSyncResult<NWSync> {
    let root = path.as_ref().to_path_buf();
    let meta_path = root.join("nwsyncmeta.sqlite3");
    if !meta_path.is_file() {
        return Err(ResNWSyncError::msg(format!(
            "meta database not found: {}",
            meta_path.display()
        )));
    }

    let meta = Connection::open(&meta_path)?;
    let mut shards = HashMap::new();
    let mut shardmap = HashMap::new();

    let mut stmt = meta.prepare("select id, serial from shards")?;
    let shard_rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
    for row in shard_rows {
        let shard_id = row?;
        let shard_path = root.join(format!("nwsyncdata_{}.sqlite3", shard_id - 1));
        if !shard_path.is_file() {
            return Err(ResNWSyncError::msg(format!(
                "shard database not found: {}",
                shard_path.display()
            )));
        }

        let shard = NWSyncShard {
            id:   shard_id,
            path: shard_path.clone(),
        };
        let conn = Connection::open(&shard_path)?;
        let mut shard_stmt = conn.prepare("select sha1 from resrefs")?;
        let resref_rows = shard_stmt.query_map([], |row| row.get::<_, String>(0))?;
        for sha1 in resref_rows {
            let sha1 = parse_secure_hash(&sha1?)?;
            if shardmap.insert(sha1, shard_id).is_some() {
                return Err(ResNWSyncError::msg(format!(
                    "duplicate shard mapping for {sha1}"
                )));
            }
        }

        shards.insert(shard_id, shard);
    }

    let result = NWSync {
        root,
        meta_path,
        shards,
        shardmap,
    };
    debug!(
        shard_count = result.shards.len(),
        shardmap_entries = result.shardmap.len(),
        "opened nwsync repository"
    );
    Ok(result)
}

/// Opens an `NWSync` repository, creating the minimal `SQLite` layout when
/// needed.
///
/// # Errors
///
/// Returns [`ResNWSyncError`] if the directory cannot be created or the
/// database cannot be initialized.
#[instrument(level = "debug", skip_all, err, fields(path = %path.as_ref().display()))]
pub fn open_or_create_nwsync(path: impl AsRef<Path>) -> ResNWSyncResult<NWSync> {
    let root = path.as_ref();
    fs::create_dir_all(root)?;

    let meta_path = root.join("nwsyncmeta.sqlite3");
    let shard_path = root.join("nwsyncdata_0.sqlite3");

    let meta = Connection::open(&meta_path)?;
    initialize_meta_schema(&meta)?;
    meta.execute(
        "insert or ignore into shards (id, serial) values (?, ?)",
        params![DEFAULT_SHARD_ID, 0_i64],
    )?;

    let shard = Connection::open(&shard_path)?;
    initialize_shard_schema(&shard)?;

    open_nwsync(root)
}

/// Exposes a single manifest row as a resource container.
///
/// # Errors
///
/// Returns [`ResNWSyncError`] if the manifest cannot be read from the database.
#[instrument(level = "debug", skip_all, err, fields(manifest_sha1 = %manifest_sha1))]
pub fn new_resnwsync_manifest(
    nwsync: &NWSync,
    manifest_sha1: ManifestSha1,
) -> ResNWSyncResult<ResNWSyncManifest> {
    let conn = nwsync.meta_connection()?;
    let created_at = conn
        .query_row(
            "select created_at from manifests where sha1 = ?",
            [manifest_sha1.to_string()],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .ok_or_else(|| ResNWSyncError::msg(format!("not found: {manifest_sha1}")))?;

    let mut contents = IndexSet::with_hasher(RandomState::new());
    let mut sha1map = HashMap::new();
    let mut stmt = conn.prepare(
        "select resref, restype, resref_sha1 from manifest_resrefs where manifest_sha1 = ?",
    )?;
    let rows = stmt.query_map(params![manifest_sha1.to_string()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
        ))
    })?;

    for row in rows {
        let (resref_name, restype_value, resref_sha1) = row?;
        let restype = u16::try_from(restype_value).map_err(|error| {
            ResNWSyncError::msg(format!("invalid restype {restype_value}: {error}"))
        })?;
        let rr = ResRef::new(resref_name, ResType(restype))?;
        contents.insert(rr.clone());
        sha1map.insert(rr, parse_secure_hash(&resref_sha1)?);
    }

    let result = ResNWSyncManifest {
        nwsync: nwsync.clone(),
        manifest_sha1,
        mtime: UNIX_EPOCH + Duration::from_secs(created_at.max(0).unsigned_abs()),
        contents,
        sha1map,
    };
    debug!(
        entry_count = result.contents.len(),
        "built nwsync manifest container"
    );
    Ok(result)
}

fn initialize_meta_schema(conn: &Connection) -> ResNWSyncResult<()> {
    conn.execute_batch(
        "create table if not exists shards (
            id integer primary key,
            serial integer not null
         );
         create table if not exists manifests (
            sha1 text primary key not null,
            created_at integer not null
         );
         create table if not exists manifest_resrefs (
            manifest_sha1 text not null,
            resref text not null,
            restype integer not null,
            resref_sha1 text not null
         );
         create index if not exists manifest_resrefs_manifest_sha1_idx
            on manifest_resrefs (manifest_sha1);",
    )?;
    Ok(())
}

fn initialize_shard_schema(conn: &Connection) -> ResNWSyncResult<()> {
    conn.execute_batch(
        "create table if not exists resrefs (
            sha1 text primary key not null,
            data blob not null
         );",
    )?;
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

    use nwnrs_checksums::secure_hash;
    use nwnrs_nwsync::{Manifest, ManifestEntry};
    use nwnrs_resref::ResRef;
    use nwnrs_restype::ResType;

    use super::open_or_create_nwsync;

    fn unique_test_dir(prefix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        std::env::temp_dir().join(format!("nwnrs-resnwsync-{prefix}-{nanos}"))
    }

    #[test]
    fn repository_write_and_prune_roundtrip() {
        let root = unique_test_dir("repo");
        if let Err(error) = fs::create_dir_all(&root) {
            panic!("create repo dir: {error}");
        }

        let mut repo = match open_or_create_nwsync(&root) {
            Ok(repo) => repo,
            Err(error) => panic!("open repo: {error}"),
        };

        let alpha = match ResRef::new("alpha", ResType(2017)) {
            Ok(value) => value,
            Err(error) => panic!("alpha resref: {error}"),
        };
        let beta = match ResRef::new("beta", ResType(2017)) {
            Ok(value) => value,
            Err(error) => panic!("beta resref: {error}"),
        };
        let alpha_sha1 = secure_hash(b"alpha");
        let beta_sha1 = secure_hash(b"beta");
        if let Err(error) = repo.put_resref_data(alpha_sha1, b"alpha") {
            panic!("insert alpha: {error}");
        }
        if let Err(error) = repo.put_resref_data(beta_sha1, b"beta") {
            panic!("insert beta: {error}");
        }

        let mut manifest = Manifest::default();
        manifest.add_entry(ManifestEntry::new(alpha_sha1, 5, alpha.clone()));
        let manifest_sha1 = secure_hash(b"manifest");
        if let Err(error) = repo.put_manifest(manifest_sha1, &manifest, 123) {
            panic!("insert manifest: {error}");
        }

        let reopened = match open_or_create_nwsync(&root) {
            Ok(repo) => repo,
            Err(error) => panic!("reopen repo: {error}"),
        };
        let manifests = match reopened.get_all_manifests() {
            Ok(values) => values,
            Err(error) => panic!("list manifests: {error}"),
        };
        assert_eq!(manifests, vec![manifest_sha1]);
        let container = match super::new_resnwsync_manifest(&reopened, manifest_sha1) {
            Ok(value) => value,
            Err(error) => panic!("manifest container: {error}"),
        };
        assert_eq!(container.sha1_for(&alpha), Some(alpha_sha1));
        assert_eq!(container.sha1_for(&beta), None);

        let mut mutable = match open_or_create_nwsync(&root) {
            Ok(repo) => repo,
            Err(error) => panic!("reopen mutable repo: {error}"),
        };
        let deleted = match mutable.delete_resref_data(&[beta_sha1]) {
            Ok(value) => value,
            Err(error) => panic!("delete beta: {error}"),
        };
        assert_eq!(deleted, 1);
        assert!(!mutable.contains_resref_data(beta_sha1));
        assert!(mutable.contains_resref_data(alpha_sha1));
    }
}
