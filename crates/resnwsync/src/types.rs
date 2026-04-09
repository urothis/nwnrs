use std::{
    collections::hash_map::RandomState,
    collections::HashMap,
    fmt,
    io::{self, Cursor},
    path::{Path, PathBuf},
    time::SystemTime,
};

use indexmap::IndexSet;
use nwnrs_checksums::prelude::*;
use nwnrs_compressedbuf::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_nwsync::Manifest;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

/// The compressed-buffer magic used by NWSync shards.
pub const NWSYNC_COMPRESSED_BUF_MAGIC_STR: &str = "NSYC";

/// Errors returned while working with NWSync repositories.
#[derive(Debug)]
pub enum ResNWSyncError {
    /// Filesystem access failed.
    Io(io::Error),
    /// SQLite access failed.
    Sqlite(rusqlite::Error),
    /// SHA-1 parsing failed.
    ParseSecureHash(ParseSecureHashError),
    /// Resource reference parsing failed.
    ResRef(nwnrs_resref::ResRefError),
    /// Resource manager setup failed.
    ResMan(ResManError),
    /// Compressed-buffer decoding failed.
    Compression(CompressedBufError),
    /// The repository contents were invalid.
    Message(String),
}

impl ResNWSyncError {
    pub(crate) fn msg(message: impl Into<String>) -> Self {
        Self::Message(message.into())
    }
}

impl fmt::Display for ResNWSyncError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => error.fmt(f),
            Self::Sqlite(error) => error.fmt(f),
            Self::ParseSecureHash(error) => error.fmt(f),
            Self::ResRef(error) => error.fmt(f),
            Self::ResMan(error) => error.fmt(f),
            Self::Compression(error) => error.fmt(f),
            Self::Message(message) => f.write_str(message),
        }
    }
}

impl std::error::Error for ResNWSyncError {}

impl From<io::Error> for ResNWSyncError {
    fn from(value: io::Error) -> Self {
        Self::Io(value)
    }
}

impl From<rusqlite::Error> for ResNWSyncError {
    fn from(value: rusqlite::Error) -> Self {
        Self::Sqlite(value)
    }
}

impl From<ParseSecureHashError> for ResNWSyncError {
    fn from(value: ParseSecureHashError) -> Self {
        Self::ParseSecureHash(value)
    }
}

impl From<nwnrs_resref::ResRefError> for ResNWSyncError {
    fn from(value: nwnrs_resref::ResRefError) -> Self {
        Self::ResRef(value)
    }
}

impl From<ResManError> for ResNWSyncError {
    fn from(value: ResManError) -> Self {
        Self::ResMan(value)
    }
}

impl From<CompressedBufError> for ResNWSyncError {
    fn from(value: CompressedBufError) -> Self {
        Self::Compression(value)
    }
}

/// Result type for NWSync resource operations.
pub type ResNWSyncResult<T> = Result<T, ResNWSyncError>;

type ShardId = i64;
pub(crate) const DEFAULT_SHARD_ID: ShardId = 1;
/// SHA-1 identifier for a manifest row.
pub type ManifestSha1 = SecureHash;
/// SHA-1 identifier for a resource payload row.
pub type ResRefSha1 = SecureHash;

#[derive(Debug, Clone)]
pub(crate) struct NWSyncShard {
    pub(crate) id:   ShardId,
    pub(crate) path: PathBuf,
}

impl fmt::Display for NWSyncShard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ResNWSyncShard:({})", self.id)
    }
}

/// An opened NWSync repository.
#[derive(Debug, Clone)]
pub struct NWSync {
    pub(crate) root:      PathBuf,
    pub(crate) meta_path: PathBuf,
    pub(crate) shards:    HashMap<ShardId, NWSyncShard>,
    pub(crate) shardmap:  HashMap<ResRefSha1, ShardId>,
}

impl NWSync {
    /// Returns the repository root path.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Returns all manifest hashes stored in the repository.
    pub fn get_all_manifests(&self) -> ResNWSyncResult<Vec<ManifestSha1>> {
        let conn = self.meta_connection()?;
        let mut stmt = conn.prepare("select sha1 from manifests")?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut manifests = Vec::new();
        for row in rows {
            manifests.push(nwnrs_checksums::parse_secure_hash(&row?)?);
        }
        Ok(manifests)
    }

    /// Returns all resource payload hashes stored in the repository.
    pub fn get_all_resrefs(&self) -> Vec<ResRefSha1> {
        self.shardmap.keys().copied().collect()
    }

    /// Returns whether the repository already stores a payload for `sha1`.
    pub fn contains_resref_data(&self, sha1: ResRefSha1) -> bool {
        self.shardmap.contains_key(&sha1)
    }

    /// Reads a resource payload by hash, decompressing `NSYC` buffers when
    /// needed.
    pub fn read_resref_data(&self, sha1: ResRefSha1) -> ResNWSyncResult<Vec<u8>> {
        let Some(shard_id) = self.shardmap.get(&sha1).copied() else {
            return Err(ResNWSyncError::msg(format!("not found: {sha1}")));
        };

        let conn = self.shard_connection(shard_id)?;
        let raw = conn
            .query_row(
                "select data from resrefs where sha1 = ?",
                [sha1.to_string()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()?
            .ok_or_else(|| ResNWSyncError::msg(format!("not found: {sha1}")))?;

        if raw.len() > 4
            && raw
                .get(..4)
                .is_some_and(|prefix| prefix == NWSYNC_COMPRESSED_BUF_MAGIC_STR.as_bytes())
        {
            nwnrs_compressedbuf::decompress_bytes(&raw, crate::nwsync_compressed_buf_magic()?)
                .map_err(Into::into)
        } else {
            Ok(raw)
        }
    }

    pub(crate) fn meta_connection(&self) -> ResNWSyncResult<Connection> {
        Ok(Connection::open(&self.meta_path)?)
    }

    pub(crate) fn shard_connection(&self, shard_id: ShardId) -> ResNWSyncResult<Connection> {
        let shard = self
            .shards
            .get(&shard_id)
            .ok_or_else(|| ResNWSyncError::msg(format!("unknown shard: {shard_id}")))?;
        Ok(Connection::open(&shard.path)?)
    }

    /// Writes a payload blob to the default shard when it is not already
    /// present.
    pub fn put_resref_data(&mut self, sha1: ResRefSha1, data: &[u8]) -> ResNWSyncResult<bool> {
        if self.shardmap.contains_key(&sha1) {
            return Ok(false);
        }

        let mut conn = self.shard_connection(DEFAULT_SHARD_ID)?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "insert or ignore into resrefs (sha1, data) values (?, ?)",
            params![sha1.to_string(), data],
        )?;
        tx.commit()?;

        self.shardmap.insert(sha1, DEFAULT_SHARD_ID);
        Ok(true)
    }

    /// Stores or replaces manifest metadata and resource mappings.
    pub fn put_manifest(
        &self,
        manifest_sha1: ManifestSha1,
        manifest: &Manifest,
        created_at: i64,
    ) -> ResNWSyncResult<()> {
        let mut conn = self.meta_connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute(
            "insert into manifests (sha1, created_at) values (?, ?)
             on conflict(sha1) do update set created_at = excluded.created_at",
            params![manifest_sha1.to_string(), created_at],
        )?;
        tx.execute(
            "delete from manifest_resrefs where manifest_sha1 = ?",
            params![manifest_sha1.to_string()],
        )?;
        {
            let mut stmt = tx.prepare(
                "insert into manifest_resrefs (manifest_sha1, resref, restype, resref_sha1)
                 values (?, ?, ?, ?)",
            )?;
            for entry in manifest.entries() {
                stmt.execute(params![
                    manifest_sha1.to_string(),
                    entry.resref.res_ref(),
                    i64::from(entry.resref.res_type().0),
                    entry.sha1.to_string(),
                ])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Deletes payload blobs by SHA-1 and returns the number of deleted rows.
    pub fn delete_resref_data(&mut self, sha1s: &[ResRefSha1]) -> ResNWSyncResult<usize> {
        let mut grouped = HashMap::<ShardId, Vec<ResRefSha1>>::new();
        for sha1 in sha1s {
            if let Some(shard_id) = self.shardmap.get(sha1).copied() {
                grouped.entry(shard_id).or_default().push(*sha1);
            }
        }

        let mut deleted = 0_usize;
        for (shard_id, hashes) in grouped {
            let mut conn = self.shard_connection(shard_id)?;
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            for sha1 in &hashes {
                deleted += tx.execute(
                    "delete from resrefs where sha1 = ?",
                    params![sha1.to_string()],
                )?;
            }
            tx.commit()?;
            for sha1 in hashes {
                self.shardmap.remove(&sha1);
            }
        }
        Ok(deleted)
    }
}

/// A single NWSync manifest exposed as a resource container.
#[derive(Debug, Clone)]
pub struct ResNWSyncManifest {
    pub(crate) nwsync:        NWSync,
    pub(crate) manifest_sha1: ManifestSha1,
    pub(crate) mtime:         SystemTime,
    pub(crate) contents:      IndexSet<ResRef, RandomState>,
    pub(crate) sha1map:       HashMap<ResRef, ResRefSha1>,
}

impl ResNWSyncManifest {
    /// Returns the manifest hash.
    pub fn manifest_sha1(&self) -> ManifestSha1 {
        self.manifest_sha1
    }

    /// Returns the manifest timestamp derived from the metadata database.
    pub fn mtime(&self) -> SystemTime {
        self.mtime
    }

    /// Returns the payload hash for a resource reference, if present.
    pub fn sha1_for(&self, rr: &ResRef) -> Option<ResRefSha1> {
        self.sha1map.get(rr).copied()
    }
}

impl fmt::Display for ResNWSyncManifest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "ResNWSyncManifest:({})",
            self.manifest_sha1.to_string().to_ascii_lowercase()
        )
    }
}

impl ResContainer for ResNWSyncManifest {
    fn contains(&self, rr: &ResRef) -> bool {
        self.contents.contains(rr)
    }

    fn demand(&self, rr: &ResRef) -> ResManResult<Res> {
        let sha1 = self
            .sha1map
            .get(rr)
            .copied()
            .ok_or_else(|| ResManError::Message(format!("not found: {rr}")))?;
        let shard_id = self
            .nwsync
            .shardmap
            .get(&sha1)
            .copied()
            .ok_or_else(|| ResManError::Message(format!("not found: {sha1}")))?;
        let conn = self
            .nwsync
            .shard_connection(shard_id)
            .map_err(|error| ResManError::Message(error.to_string()))?;
        let data = conn
            .query_row(
                "select data from resrefs where sha1 = ?",
                [sha1.to_string()],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .optional()
            .map_err(|error| ResManError::Message(error.to_string()))?
            .ok_or_else(|| ResManError::Message(format!("not found: {sha1}")))?;

        Ok(Res::new_with_stream(
            new_res_origin(self.to_string(), rr.to_string()),
            rr.clone(),
            self.mtime,
            shared_stream(Cursor::new(data.clone())),
            data.len() as i64,
            0,
            ExoResFileCompressionType::None,
            None,
            data.len(),
            sha1,
        ))
    }

    fn count(&self) -> usize {
        self.contents.len()
    }

    fn contents(&self) -> Vec<ResRef> {
        self.contents.iter().cloned().collect()
    }
}
