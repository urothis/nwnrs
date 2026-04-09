use std::{io::Cursor, sync::Arc, time::SystemTime};

use nwnrs_checksums::prelude::*;
use nwnrs_exo::prelude::*;
use nwnrs_resman::prelude::*;
use nwnrs_resref::prelude::*;
use tracing::{debug, instrument};

use crate::{ResMemFile, ResMemFileResult};

/// Wraps owned bytes as a one-entry in-memory resource container.
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
            len as i64,
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
#[instrument(level = "debug", skip_all, err)]
pub fn read_resmemfile_arc(
    label: impl Into<String>,
    resref: ResRef,
    bytes: Arc<[u8]>,
) -> ResMemFileResult<ResMemFile> {
    read_resmemfile(label, resref, bytes.as_ref().to_vec())
}
