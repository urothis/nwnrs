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
#[instrument(level = "debug", skip_all, err)]
pub fn read_resmemfile_arc(
    label: impl Into<String>,
    resref: ResRef,
    bytes: Arc<[u8]>,
) -> ResMemFileResult<ResMemFile> {
    read_resmemfile(label, resref, bytes.as_ref().to_vec())
}

#[allow(clippy::panic)]
#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nwnrs_resman::{CachePolicy, ResContainer};
    use nwnrs_resref::ResRef;
    use nwnrs_restype::ResType;

    use crate::{read_resmemfile, read_resmemfile_arc};

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
