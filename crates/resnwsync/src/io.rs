use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, UNIX_EPOCH},
};

use indexmap::IndexSet;
use nwn_checksums::parse_secure_hash;
use nwn_compressedbuf::make_magic;
use nwn_resref::new_res_ref;
use nwn_restype::ResType;
use rusqlite::{Connection, OptionalExtension, params};
use tracing::{debug, instrument};

use crate::{
    ManifestSha1, NWSYNC_COMPRESSED_BUF_MAGIC_STR, NWSync, NWSyncShard, ResNWSyncError,
    ResNWSyncManifest, ResNWSyncResult,
};

/// Returns the integer magic for `NSYC` compressed buffers.
#[instrument(level = "debug", skip_all, err)]
pub fn nwsync_compressed_buf_magic() -> ResNWSyncResult<u32> {
    make_magic(NWSYNC_COMPRESSED_BUF_MAGIC_STR)
        .map_err(|error| ResNWSyncError::msg(error.to_string()))
}

/// Opens an NWSync repository rooted at `path`.
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

/// Exposes a single manifest row as a resource container.
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

    let mut contents = IndexSet::new();
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
        let rr = new_res_ref(resref_name, ResType(restype))?;
        contents.insert(rr.clone());
        sha1map.insert(rr, parse_secure_hash(&resref_sha1)?);
    }

    let result = ResNWSyncManifest {
        nwsync: nwsync.clone(),
        manifest_sha1,
        mtime: UNIX_EPOCH + Duration::from_secs(created_at.max(0) as u64),
        contents,
        sha1map,
    };
    debug!(
        entry_count = result.contents.len(),
        "built nwsync manifest container"
    );
    Ok(result)
}
